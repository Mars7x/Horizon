//! Bounded, cancellable animated JPEG XL playback on a media worker.
use crate::services::image::{self as decoding, JxlAnimation};
use image::{DynamicImage, RgbaImage};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub struct AnimationPlayback {
    stop: Arc<AtomicBool>,
    thread: std::thread::Thread,
    animated: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
}
impl AnimationPlayback {
    pub fn set_playing(&self, playing: bool) {
        self.paused.store(!playing, Ordering::SeqCst);
        self.thread.unpark();
    }
    pub fn is_animated(&self) -> bool {
        self.animated.load(Ordering::SeqCst)
    }
}
impl Drop for AnimationPlayback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.unpark();
    }
}

pub fn play_animation(
    path: &Path,
    max: (u32, u32),
    frame: Box<dyn Fn(Result<RgbaImage, String>) + Send>,
) -> std::io::Result<AnimationPlayback> {
    let stop = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::clone(&stop);
    let animated = Arc::new(AtomicBool::new(false));
    let moving = Arc::clone(&animated);
    let paused = Arc::new(AtomicBool::new(false));
    let inactive = Arc::clone(&paused);
    let path = path.to_owned();
    let thread = std::thread::Builder::new()
        .name("album-jxl".into())
        .spawn(move || {
            let result = (|| -> Result<(), String> {
                let bytes = decoding::read_bytes(&path).map_err(|e| e.to_string())?;
                let Some(mut animation) =
                    JxlAnimation::from_bytes(&bytes).map_err(|e| e.to_string())?
                else {
                    return Ok(());
                };
                moving.store(animation.frame_count() > 1, Ordering::SeqCst);
                let mut first = true;
                let mut passes = 0;
                let mut due = Instant::now();
                loop {
                    animation.rewind();
                    while !cancelled.load(Ordering::SeqCst) {
                        while inactive.load(Ordering::SeqCst) && !cancelled.load(Ordering::SeqCst) {
                            std::thread::park_timeout(Duration::from_millis(250));
                            due = Instant::now();
                        }
                        if cancelled.load(Ordering::SeqCst) {
                            return Ok(());
                        }
                        let Some((image, delay)) =
                            animation.next_frame().map_err(|e| e.to_string())?
                        else {
                            break;
                        };
                        if Instant::now() > due + Duration::from_secs(1) {
                            due = Instant::now();
                        }
                        while !cancelled.load(Ordering::SeqCst) && Instant::now() < due {
                            std::thread::park_timeout(
                                due.saturating_duration_since(Instant::now()),
                            );
                        }
                        if cancelled.load(Ordering::SeqCst) {
                            return Ok(());
                        }
                        // The first frame already comes from the photo loader.
                        if !first && !inactive.load(Ordering::SeqCst) {
                            frame(Ok(decoding::fit(DynamicImage::ImageRgba8(image), max)));
                        }
                        first = false;
                        due += delay;
                    }
                    passes += 1;
                    if cancelled.load(Ordering::SeqCst)
                        || (animation.loops != 0 && passes >= animation.loops)
                    {
                        return Ok(());
                    }
                }
            })();
            if let Err(error) = result
                && !cancelled.load(Ordering::SeqCst)
            {
                frame(Err(error));
            }
        })?;
    Ok(AnimationPlayback {
        stop,
        thread: thread.thread().clone(),
        animated,
        paused,
    })
}
