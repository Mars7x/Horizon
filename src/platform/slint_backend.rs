use slint::winit_030::{SlintEvent, winit};
use tracing::debug;

use crate::domain::SessionMode;

/// Select Horizon's Slint platform explicitly before any Slint API that needs
/// access to the windowing system is called.
///
/// Horizon intentionally builds Slint without its `backend-default` feature so
/// the application does not accidentally pick up Qt or another backend. The
/// Winit backend and Skia renderer are compiled in explicitly in
/// `Cargo.toml`, and this function selects that exact pair at runtime.
///
/// On a desktop, winit chooses the display (Wayland). The gamescope session
/// is X11 only: gamescope shows only X11 windows tagged with an app ID
/// (ADR 0189), so the session never tries Wayland.
pub fn initialize(mode: SessionMode) -> Result<(), slint::PlatformError> {
    let mut selector = slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("skia".into());
    let display_server = match mode {
        SessionMode::Desktop => "auto",
        SessionMode::Gamescope => {
            use winit::platform::x11::EventLoopBuilderExtX11;
            let mut builder = winit::event_loop::EventLoop::<SlintEvent>::with_user_event();
            builder.with_x11();
            selector = selector.with_winit_event_loop_builder(builder);
            "x11"
        }
    };
    selector.select()?;

    debug!(
        backend = "winit",
        renderer = "skia",
        display_server,
        "Slint platform ready"
    );
    Ok(())
}
