//! Appearance domain and host integration.
//!
//! This module owns theme preference resolution and the XDG Settings portal
//! adapter. Slint only receives the resolved, semantic appearance state.

mod color;
mod model;
pub mod portal;

pub use color::Rgb;
pub use model::{
    AccentPreference, AppearancePreferences, AppearanceState, EffectiveTheme, Palette,
    ResolvedAppearance, SystemAppearance, ThemePreference,
};
