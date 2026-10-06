use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{Local, Timelike};
use slint::{ComponentHandle, Timer, TimerMode};
use tracing::{debug, warn};

use crate::{
    AppWindow,
    platform::clock::{ClockFormat, ClockFormatMonitor, ClockFormatMonitorError},
};

pub struct ClockController {
    format: Arc<Mutex<ClockFormat>>,
    _timer: Timer,
}

impl ClockController {
    pub fn new(ui: &AppWindow) -> Self {
        let format = Arc::new(Mutex::new(ClockFormat::default()));
        apply_clock(ui, ClockFormat::default());

        let timer = Timer::default();
        let timer_ui = ui.as_weak();
        let timer_format = Arc::clone(&format);
        timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let Some(ui) = timer_ui.upgrade() else {
                return;
            };
            apply_clock(&ui, *lock_format(&timer_format));
        });

        Self {
            format,
            _timer: timer,
        }
    }

    pub fn start_portal_monitor(
        &self,
        ui: &AppWindow,
    ) -> Result<ClockFormatMonitor, ClockFormatMonitorError> {
        let format = Arc::clone(&self.format);
        let ui_weak = ui.as_weak();

        ClockFormatMonitor::start(move |new_format| {
            *lock_format(&format) = new_format;

            let ui_weak = ui_weak.clone();
            if let Err(error) = ui_weak.upgrade_in_event_loop(move |ui| {
                apply_clock(&ui, new_format);
            }) {
                debug!(%error, "clock-format update ignored because UI event loop is unavailable");
            }
        })
    }
}

fn lock_format(format: &Arc<Mutex<ClockFormat>>) -> std::sync::MutexGuard<'_, ClockFormat> {
    match format.lock() {
        Ok(format) => format,
        Err(poisoned) => {
            warn!("clock-format mutex was poisoned; recovering latest state");
            poisoned.into_inner()
        }
    }
}

fn apply_clock(ui: &AppWindow, format: ClockFormat) {
    let now = Local::now();
    let minute = now.minute();

    let (time, suffix) = match format {
        ClockFormat::TwentyFourHour => (format!("{:02}:{minute:02}", now.hour()), String::new()),
        ClockFormat::TwelveHour => {
            let hour = match now.hour() % 12 {
                0 => 12,
                hour => hour,
            };
            let suffix = if now.hour() < 12 { "AM" } else { "PM" };
            (format!("{hour}:{minute:02}"), suffix.to_owned())
        }
    };

    ui.set_clock_time(time.into());
    ui.set_clock_suffix(suffix.into());
}
