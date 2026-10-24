//! Filters game frames on their own thread, one at a time, when the screen
//! has drawn the last one. The core never waits for a filter, so fast-forward
//! keeps its speed, and the picture keeps up with the display's refresh rate
//! as far as the filter allows.
use super::gba_core::{DisplaySize, Frame, LatestFrame, unpack_ds_view};
use super::gba_video::{Scratch, present, present_ds};
use crate::domain::core_settings::MgbaPreferences;
use std::sync::{Condvar, Mutex, PoisonError, atomic::Ordering};

#[derive(Default)]
struct Pending {
    /// The newest raw frame not yet filtered; newer frames replace it.
    raw: Option<Frame>,
    /// Already sampled on the GPU; preserve each screen’s exact viewport size.
    screens: Option<[(u32, u32); 2]>,
    /// The screen can take another frame.
    wanted: bool,
    stop: bool,
}

#[derive(Default)]
pub struct FrameStage {
    pending: Mutex<Pending>,
    wake: Condvar,
}

impl FrameStage {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(Pending {
                wanted: true,
                ..Pending::default()
            }),
            wake: Condvar::new(),
        }
    }

    /// The core's newest frame. Cheap: one native-size copy into a reused buffer.
    pub fn submit(&self, frame: &Frame) {
        self.submit_frame(frame, None);
    }

    pub fn submit_viewport(&self, frame: &Frame, screens: [(u32, u32); 2]) {
        self.submit_frame(frame, Some(screens));
    }

    fn submit_frame(&self, frame: &Frame, screens: Option<[(u32, u32); 2]>) {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.screens = screens;
        match pending.raw.as_mut() {
            Some(raw) => {
                raw.width = frame.width;
                raw.height = frame.height;
                raw.rgba.clone_from(&frame.rgba);
            }
            None => pending.raw = Some(frame.clone()),
        }
        self.wake.notify_one();
    }

    /// The filter can take a frame now (a newer one would replace it unseen
    /// otherwise): costly frames are produced only then.
    pub fn wanted(&self) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .wanted
    }

    /// The screen took the last frame: prepare the next.
    pub fn want(&self) {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .wanted = true;
        self.wake.notify_one();
    }

    pub fn stop(&self) {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop = true;
        self.wake.notify_one();
    }

    /// The filter thread's loop: until `stop`, filter the newest frame each
    /// time one is wanted, publish it to `output`, then call `ready`.
    pub fn run(
        &self,
        preferences: MgbaPreferences,
        ds_view: Option<&std::sync::atomic::AtomicU8>,
        display: &DisplaySize,
        output: &LatestFrame,
        ready: &dyn Fn(),
    ) {
        let mut scratch = Scratch::default();
        loop {
            let (raw, screens) = {
                let mut pending = self
                    .wake
                    .wait_while(
                        self.pending.lock().unwrap_or_else(PoisonError::into_inner),
                        |pending| !pending.stop && !(pending.wanted && pending.raw.is_some()),
                    )
                    .unwrap_or_else(PoisonError::into_inner);
                if pending.stop {
                    return;
                }
                pending.wanted = false;
                (pending.raw.take(), pending.screens.take())
            };
            let Some(raw) = raw else {
                continue;
            };
            let size = display.load(Ordering::Acquire);
            let window = ((size >> 32) as u32, size as u32);
            let presented = if let Some(screens) = screens {
                super::gba_core::Presented {
                    pixels: slint::SharedPixelBuffer::clone_from_slice(
                        &raw.rgba, raw.width, raw.height,
                    ),
                    smooth: false,
                    exact: None,
                    screens: Some(screens),
                }
            } else if let Some(three_ds) = ds_view
                .is_some()
                .then(|| super::gba_video::present_three_ds(&raw))
                .flatten()
            {
                // Azahar draws no filter; Horizon only places its screens.
                three_ds
            } else if let Some(view) = ds_view {
                // Each screen is filtered at the size the layout shows it.
                let (layout, swapped) = unpack_ds_view(view.load(Ordering::Acquire));
                let rects = layout.screens(swapped, (f64::from(window.0), f64::from(window.1)));
                let targets =
                    rects.map(|rect| (rect.width.round() as u32, rect.height.round() as u32));
                present_ds(&raw, preferences.video_filter, targets)
            } else {
                present(
                    &raw,
                    preferences.color_correction,
                    preferences.video_filter,
                    window,
                    &mut scratch,
                )
            };
            *output.lock().unwrap_or_else(PoisonError::into_inner) = Some(presented);
            ready();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize},
    };

    fn frame(value: u8) -> Frame {
        Frame {
            width: 2,
            height: 1,
            rgba: vec![value; 8],
            smooth: false,
        }
    }

    #[test]
    fn filters_only_the_newest_frame_and_only_when_wanted() {
        let stage = Arc::new(FrameStage::new());
        let output: LatestFrame = Arc::default();
        let display = DisplaySize::new(AtomicU64::new(0));
        let shown = Arc::new(AtomicUsize::new(0));
        let worker = {
            let (stage, output, display, shown) = (
                Arc::clone(&stage),
                Arc::clone(&output),
                Arc::clone(&display),
                Arc::clone(&shown),
            );
            std::thread::spawn(move || {
                stage.run(MgbaPreferences::default(), None, &display, &output, &|| {
                    shown.fetch_add(1, Ordering::SeqCst);
                })
            })
        };
        let wait = |count| {
            for _ in 0..200 {
                if shown.load(Ordering::SeqCst) >= count {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("no frame shown");
        };
        stage.submit(&frame(1));
        wait(1);
        // Not wanted yet: newer frames replace each other, unfiltered.
        stage.submit(&frame(2));
        stage.submit(&frame(3));
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(shown.load(Ordering::SeqCst), 1);
        stage.want();
        wait(2);
        let latest = output.lock().unwrap().take().unwrap();
        assert_eq!(latest.pixels.as_bytes()[0], 3);
        let viewport = Frame {
            width: 3,
            height: 3,
            rgba: vec![42; 36],
            smooth: false,
        };
        let screens = [(3, 2), (1, 1)];
        stage.submit_viewport(&viewport, screens);
        stage.want();
        wait(3);
        let latest = output.lock().unwrap().take().unwrap();
        assert_eq!(latest.screens, Some(screens));
        assert_eq!(latest.pixels.as_bytes(), viewport.rgba);
        stage.submit(&frame(4));
        stage.want();
        wait(4);
        let latest = output.lock().unwrap().take().unwrap();
        assert_eq!(
            latest.screens, None,
            "full-source fallback must discard viewport metadata"
        );
        assert_eq!(latest.pixels.as_bytes()[0], 4);
        stage.stop();
        worker.join().unwrap();
        assert_eq!(shown.load(Ordering::SeqCst), 4);
    }
}
