use std::{sync::Arc, thread::JoinHandle};

use ashpd::desktop::settings::Settings;
use futures_util::StreamExt;
use thiserror::Error;
use tokio::sync::oneshot;
use tracing::{debug, warn};

const GNOME_INTERFACE_NAMESPACE: &str = "org.gnome.desktop.interface";
const CLOCK_FORMAT_KEY: &str = "clock-format";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClockFormat {
    #[default]
    TwelveHour,
    TwentyFourHour,
}

impl ClockFormat {
    fn from_portal_value(value: &str) -> Self {
        if value == "24h" {
            Self::TwentyFourHour
        } else {
            Self::TwelveHour
        }
    }
}

#[derive(Debug, Error)]
pub enum ClockFormatMonitorError {
    #[error("failed to spawn clock-format portal monitor: {0}")]
    ThreadSpawn(#[from] std::io::Error),
}

pub struct ClockFormatMonitor {
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl ClockFormatMonitor {
    pub fn start<F>(on_change: F) -> Result<Self, ClockFormatMonitorError>
    where
        F: Fn(ClockFormat) + Send + Sync + 'static,
    {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let on_change = Arc::new(on_change);

        let thread = std::thread::Builder::new()
            .name("horizon-clock-format".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        warn!(%error, "could not create clock-format portal runtime; using 12-hour fallback");
                        return;
                    }
                };

                runtime.block_on(run_clock_format_monitor(on_change, shutdown_rx));
            })?;

        Ok(Self {
            shutdown: Some(shutdown_tx),
            thread: Some(thread),
        })
    }
}

impl Drop for ClockFormatMonitor {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

async fn run_clock_format_monitor(
    on_change: Arc<dyn Fn(ClockFormat) + Send + Sync>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let settings = match Settings::new().await {
        Ok(settings) => settings,
        Err(error) => {
            warn!(%error, "XDG Settings portal unavailable; using 12-hour clock fallback");
            return;
        }
    };

    let initial = read_clock_format(&settings).await;
    on_change(initial);

    let mut changes = match settings
        .receive_setting_changed_with_args::<String>(
            GNOME_INTERFACE_NAMESPACE,
            CLOCK_FORMAT_KEY,
        )
        .await
    {
        Ok(changes) => changes,
        Err(error) => {
            warn!(%error, "could not subscribe to desktop clock-format changes");
            return;
        }
    };

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            change = changes.next() => {
                match change {
                    Some(Ok(value)) => on_change(ClockFormat::from_portal_value(&value)),
                    Some(Err(error)) => debug!(%error, "ignored invalid desktop clock-format update"),
                    None => {
                        debug!("desktop clock-format portal stream ended");
                        break;
                    }
                }
            }
        }
    }
}

async fn read_clock_format(settings: &Settings) -> ClockFormat {
    match settings
        .read::<String>(GNOME_INTERFACE_NAMESPACE, CLOCK_FORMAT_KEY)
        .await
    {
        Ok(value) => ClockFormat::from_portal_value(&value),
        Err(error) => {
            debug!(%error, "portal does not expose the desktop clock format; using 12-hour fallback");
            ClockFormat::TwelveHour
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ClockFormat;

    #[test]
    fn parses_24_hour_portal_value() {
        assert_eq!(
            ClockFormat::from_portal_value("24h"),
            ClockFormat::TwentyFourHour
        );
    }

    #[test]
    fn unknown_values_fall_back_to_12_hour() {
        assert_eq!(
            ClockFormat::from_portal_value("12h"),
            ClockFormat::TwelveHour
        );
        assert_eq!(
            ClockFormat::from_portal_value("unexpected"),
            ClockFormat::TwelveHour
        );
    }
}
