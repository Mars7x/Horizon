//! Clipboard access through Horizon's existing focused Wayland display.
//!
//! Unlike arboard's Wayland data-control backend, smithay-clipboard uses the
//! ordinary wl_data_device protocol available to regular GNOME applications.
//! No new display connection, X11 fallback, or privileged portal is used.

use std::sync::Mutex;

use slint::winit_030::{WinitWindowAccessor, winit};
use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};

pub struct WaylandClipboard {
    clipboard: Mutex<smithay_clipboard::Clipboard>,
}

impl WaylandClipboard {
    pub fn from_window(window: &slint::Window) -> Result<Self, &'static str> {
        let display = window
            .with_winit_window(|window| {
                let handle = window.display_handle().ok()?;
                match handle.as_raw() {
                    RawDisplayHandle::Wayland(wayland) => Some(wayland.display.as_ptr()),
                    _ => None,
                }
            })
            .flatten()
            .ok_or("The active Wayland window is unavailable.")?;

        // SAFETY: this is Winit's live wl_display pointer. The owner stores
        // this clipboard for no longer than the application's window lifetime;
        // its own Wayland event queue stays on the connected display.
        let clipboard = unsafe { smithay_clipboard::Clipboard::new(display) };
        Ok(Self {
            clipboard: Mutex::new(clipboard),
        })
    }

    /// Called from a worker, never Slint's event loop. Clipboard content is
    /// not written to logs or exposed beyond the Settings edit session.
    pub fn read_key(&self) -> Option<String> {
        let clipboard = self.clipboard.lock().ok()?;
        let text = match clipboard.load() {
            Ok(text) => text,
            Err(_) => {
                // wl_data_device starts observing seats asynchronously. A
                // newly initialized device may not have its first offer yet.
                // Retry once on the worker, never on Slint's UI event loop.
                std::thread::sleep(std::time::Duration::from_millis(70));
                clipboard.load().ok()?
            }
        };
        validate_token(&text)
    }
}

fn validate_token(text: &str) -> Option<String> {
    let key = text.trim();
    if key.is_empty()
        || key.len() > 512
        || !key.is_ascii()
        || key
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return None;
    }
    Some(key.to_owned())
}

#[cfg(test)]
mod tests {
    use super::validate_token;

    #[test]
    fn clipboard_accepts_one_token_and_trims_external_newlines() {
        assert_eq!(
            validate_token("  ab-CD_123\n").as_deref(),
            Some("ab-CD_123")
        );
        assert!(validate_token(" \t\n").is_none());
        assert!(validate_token("ab CD").is_none());
        assert!(validate_token("ab\nCD").is_none());
        assert!(validate_token("不可").is_none());
        assert!(validate_token(&"a".repeat(513)).is_none());
        assert!(validate_token(&"a".repeat(512)).is_some());
    }
}
