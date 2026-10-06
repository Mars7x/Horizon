use std::{sync::Arc, thread::JoinHandle};

use ashpd::desktop::{
    Color,
    settings::{
        APPEARANCE_NAMESPACE, ColorScheme, Contrast, ReducedMotion, Settings,
    },
};
use futures_util::StreamExt;
use thiserror::Error;
use tokio::sync::oneshot;
use tracing::{debug, warn};

use super::{EffectiveTheme, Rgb, SystemAppearance};

#[derive(Debug, Error)]
pub enum PortalMonitorError {
    #[error("failed to spawn appearance portal monitor: {0}")]
    ThreadSpawn(#[from] std::io::Error),
}

pub struct PortalMonitor {
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl PortalMonitor {
    pub fn start<F>(on_change: F) -> Result<Self, PortalMonitorError>
    where
        F: Fn(SystemAppearance) + Send + Sync + 'static,
    {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let on_change = Arc::new(on_change);

        let thread = std::thread::Builder::new()
            .name("horizon-appearance".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        warn!(%error, "could not create portal runtime; using fallback appearance");
                        return;
                    }
                };

                runtime.block_on(run_portal_monitor(on_change, shutdown_rx));
            })?;

        Ok(Self {
            shutdown: Some(shutdown_tx),
            thread: Some(thread),
        })
    }
}

impl Drop for PortalMonitor {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

async fn run_portal_monitor(
    on_change: Arc<dyn Fn(SystemAppearance) + Send + Sync>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let settings = match Settings::new().await {
        Ok(settings) => settings,
        Err(error) => {
            warn!(%error, "XDG Settings portal unavailable; using fallback appearance");
            return;
        }
    };

    on_change(read_system_appearance(&settings).await);

    let mut changes = match settings.receive_setting_changed().await {
        Ok(changes) => changes,
        Err(error) => {
            warn!(%error, "could not subscribe to XDG appearance changes");
            return;
        }
    };

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            change = changes.next() => {
                let Some(change) = change else {
                    debug!("XDG Settings portal change stream ended");
                    break;
                };

                if change.namespace() == APPEARANCE_NAMESPACE {
                    on_change(read_system_appearance(&settings).await);
                }
            }
        }
    }
}

async fn read_system_appearance(settings: &Settings) -> SystemAppearance {
    let preferred_theme = match settings.color_scheme().await {
        Ok(ColorScheme::PreferDark) => Some(EffectiveTheme::Dark),
        Ok(ColorScheme::PreferLight) => Some(EffectiveTheme::Light),
        Ok(ColorScheme::NoPreference) => None,
        Err(error) => {
            debug!(%error, "portal does not expose a color-scheme preference");
            None
        }
    };

    let accent = match settings.accent_color().await {
        Ok(color) => portal_color(color),
        Err(error) => {
            debug!(%error, "portal does not expose an accent-color preference");
            None
        }
    };

    let high_contrast = match settings.contrast().await {
        Ok(Contrast::High) => true,
        Ok(Contrast::NoPreference) => false,
        Err(error) => {
            debug!(%error, "portal does not expose a contrast preference");
            false
        }
    };

    let reduced_motion = match settings.reduced_motion().await {
        Ok(ReducedMotion::ReducedMotion) => true,
        Ok(ReducedMotion::NoPreference) => false,
        Err(error) => {
            debug!(%error, "portal does not expose a reduced-motion preference");
            false
        }
    };

    SystemAppearance {
        preferred_theme,
        accent,
        high_contrast,
        reduced_motion,
    }
}

fn portal_color(color: Color) -> Option<Rgb> {
    let channels = [color.red(), color.green(), color.blue()];
    if channels
        .iter()
        .any(|channel| !channel.is_finite() || !(0.0..=1.0).contains(channel))
    {
        return None;
    }

    let to_byte = |channel: f64| (channel * 255.0).round() as u8;
    Some(Rgb::new(
        to_byte(channels[0]),
        to_byte(channels[1]),
        to_byte(channels[2]),
    ))
}
