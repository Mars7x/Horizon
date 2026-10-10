//! Background decoding driven by what is on screen.
//!
//! The UI says what it wants *now* (`want`), replacing whatever was still
//! queued: scrolling past a hundred captures never leaves a hundred decodes
//! behind it. Display images (the viewer) always go before thumbnails.
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, PoisonError},
    thread,
};

use image::RgbaImage;

use super::media::{Thumbnail, ThumbnailCache, VideoBackend, decode_photo};
use crate::domain::album::Capture;

pub enum MediaResult {
    Thumbnail {
        path: PathBuf,
        result: Result<Thumbnail, String>,
    },
    /// A photo decoded to fit `max` for the viewer.
    Display {
        path: PathBuf,
        max: (u32, u32),
        result: Result<RgbaImage, String>,
    },
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum JobKey {
    Thumbnail(PathBuf),
    Display(PathBuf, (u32, u32)),
}

enum Job {
    Thumbnail(Capture),
    Display(Capture, (u32, u32)),
}

impl Job {
    fn key(&self) -> JobKey {
        match self {
            Self::Thumbnail(capture) => JobKey::Thumbnail(capture.path().to_owned()),
            Self::Display(capture, max) => JobKey::Display(capture.path().to_owned(), *max),
        }
    }
}

#[derive(Default)]
struct Queue {
    display: VecDeque<Job>,
    thumbnails: VecDeque<Job>,
    running: HashSet<JobKey>,
    shutdown: bool,
}

impl Queue {
    fn next(&mut self) -> Option<Job> {
        loop {
            let job = self
                .display
                .pop_front()
                .or_else(|| self.thumbnails.pop_front())?;
            // Already being decoded by another worker; its result will come.
            if self.running.insert(job.key()) {
                return Some(job);
            }
        }
    }
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

pub struct MediaLoader {
    shared: Shared,
}

impl MediaLoader {
    /// `deliver` runs on a worker thread for every finished job; it should
    /// only hand the result to the UI thread.
    pub fn start(
        workers: usize,
        cache: ThumbnailCache,
        video: Arc<dyn VideoBackend>,
        deliver: Arc<dyn Fn(MediaResult) + Send + Sync>,
    ) -> Self {
        let shared: Shared = Arc::default();
        for index in 0..workers.max(1) {
            let shared = Arc::clone(&shared);
            let cache = cache.clone();
            let video = Arc::clone(&video);
            let deliver = Arc::clone(&deliver);
            let spawned = thread::Builder::new()
                .name(format!("album-media-{index}"))
                .spawn(move || work(&shared, &cache, video.as_ref(), deliver.as_ref()));
            if let Err(error) = spawned {
                tracing::warn!(%error, "Album media worker could not start");
            }
        }
        Self { shared }
    }

    /// Replace all queued work with these, in priority order.
    pub fn want(&self, display: Vec<(Capture, (u32, u32))>, thumbnails: Vec<Capture>) {
        let (lock, wake) = &*self.shared;
        let mut queue = lock.lock().unwrap_or_else(PoisonError::into_inner);
        queue.display = display
            .into_iter()
            .map(|(capture, max)| Job::Display(capture, max))
            .collect();
        queue.thumbnails = thumbnails.into_iter().map(Job::Thumbnail).collect();
        wake.notify_all();
    }
}

impl Drop for MediaLoader {
    fn drop(&mut self) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap_or_else(PoisonError::into_inner).shutdown = true;
        wake.notify_all();
    }
}

fn work(
    shared: &Shared,
    cache: &ThumbnailCache,
    video: &dyn VideoBackend,
    deliver: &(dyn Fn(MediaResult) + Send + Sync),
) {
    let (lock, wake) = &**shared;
    loop {
        let job = {
            let mut queue = lock.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if queue.shutdown {
                    return;
                }
                if let Some(job) = queue.next() {
                    break job;
                }
                queue = wake.wait(queue).unwrap_or_else(PoisonError::into_inner);
            }
        };
        let key = job.key();
        let result = match job {
            Job::Thumbnail(capture) => MediaResult::Thumbnail {
                path: capture.path().to_owned(),
                result: cache
                    .thumbnail(&capture, video)
                    .map_err(|error| error.to_string()),
            },
            Job::Display(capture, max) => MediaResult::Display {
                path: capture.path().to_owned(),
                max,
                result: decode_photo(capture.path(), max).map_err(|error| error.to_string()),
            },
        };
        lock.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .running
            .remove(&key);
        deliver(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::album::{CaptureOrigin, MediaKind},
        services::album::media::NoVideo,
    };
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn display_images_are_decoded_before_thumbnails() {
        let dir = std::env::temp_dir().join(format!("horizon-loader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut captures = Vec::new();
        for name in ["a.png", "b.png", "c.png"] {
            let path = dir.join(name);
            RgbaImage::new(64, 36).save(&path).unwrap();
            captures.push(Capture::new(
                path,
                MediaKind::Photo,
                CaptureOrigin::Horizon,
                None,
                0,
            ));
        }
        let (sender, receiver) = mpsc::channel();
        let sender = Mutex::new(sender);
        // Queue before any worker exists, so the order is fully decided by
        // the queue rather than by timing.
        let loader = MediaLoader {
            shared: Arc::default(),
        };
        loader.want(
            vec![(captures[2].clone(), (32, 32))],
            vec![captures[0].clone(), captures[1].clone()],
        );
        let shared = Arc::clone(&loader.shared);
        let handle = thread::spawn(move || {
            let deliver = move |result: MediaResult| {
                let label = match result {
                    MediaResult::Display { path, result, .. } => {
                        assert_eq!(result.unwrap().dimensions(), (32, 18));
                        format!("display {}", path.file_name().unwrap().to_string_lossy())
                    }
                    MediaResult::Thumbnail { path, result } => {
                        assert!(result.is_ok());
                        format!("thumb {}", path.file_name().unwrap().to_string_lossy())
                    }
                };
                let _ = sender.lock().unwrap().send(label);
            };
            work(
                &shared,
                &ThumbnailCache::new(None),
                &NoVideo("none".into()),
                &deliver,
            );
        });
        let order: Vec<String> = (0..3)
            .map(|_| receiver.recv_timeout(Duration::from_secs(10)).unwrap())
            .collect();
        assert_eq!(order, ["display c.png", "thumb a.png", "thumb b.png"]);
        drop(loader);
        handle.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn wanting_new_work_drops_what_was_still_queued() {
        let mut queue = Queue::default();
        let capture = |name: &str| {
            Capture::new(
                PathBuf::from(name),
                MediaKind::Photo,
                CaptureOrigin::Horizon,
                None,
                0,
            )
        };
        queue.thumbnails = [capture("old")].into_iter().map(Job::Thumbnail).collect();
        queue.thumbnails = [capture("new")].into_iter().map(Job::Thumbnail).collect();
        let next = queue.next().expect("job");
        assert!(next.key() == JobKey::Thumbnail(PathBuf::from("new")));
        // A job a worker is already running is not started twice.
        queue.thumbnails = [capture("new")].into_iter().map(Job::Thumbnail).collect();
        assert!(queue.next().is_none());
    }
}
