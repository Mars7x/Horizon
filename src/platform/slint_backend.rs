use tracing::info;

/// Select Horizon's Slint platform explicitly before any Slint API that needs
/// access to the windowing system is called.
///
/// Horizon intentionally builds Slint without its `backend-default` feature so
/// the application does not accidentally pick up Qt or another backend. The
/// Winit backend and Skia renderer are compiled in explicitly in
/// `Cargo.toml`, and this function selects that exact pair at runtime.
pub fn initialize() -> Result<(), slint::PlatformError> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("skia".into())
        .select()?;

    info!(backend = "winit", renderer = "skia", "Slint platform initialized");
    Ok(())
}
