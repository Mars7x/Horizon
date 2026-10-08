use std::sync::{Arc, Mutex};

use slint::ComponentHandle;
use tracing::{debug, warn};

use crate::{
    AppWindow, Motion, Theme,
    appearance::{
        AccentPreference, AppearancePreferences, AppearanceState, Palette, Rgb, SystemAppearance, ThemePreference,
        store::{AppearanceStore, AppearanceStoreError},
        portal::{PortalMonitor, PortalMonitorError},
    },
};

#[derive(Clone)]
pub struct AppearanceController {
    state: Arc<Mutex<AppearanceState>>,
    store: Arc<AppearanceStore>,
}

impl AppearanceController {
    pub fn new(ui: &AppWindow, store: AppearanceStore) -> Result<Self, AppearanceStoreError> {
        let preferences = store.load()?;
        let controller = Self {
            state: Arc::new(Mutex::new(AppearanceState { preferences, ..AppearanceState::default() })),
            store: Arc::new(store),
        };
        controller.apply(ui);
        Ok(controller)
    }

    pub fn preferences(&self) -> AppearancePreferences { lock_state(&self.state).preferences }

    pub fn set_preferences(&self, ui: &AppWindow, preferences: AppearancePreferences)
        -> Result<(), AppearanceStoreError> {
        if self.preferences() == preferences { return Ok(()); }
        self.store.save(&preferences)?; // Never render a preference that failed to save.
        lock_state(&self.state).preferences = preferences;
        self.apply(ui);
        Ok(())
    }

    pub fn start_portal_monitor(
        &self,
        ui: &AppWindow,
    ) -> Result<PortalMonitor, PortalMonitorError> {
        let state = Arc::clone(&self.state);
        let ui_weak = ui.as_weak();

        PortalMonitor::start(move |system| {
            let resolved = {
                let mut state = lock_state(&state);
                state.system = system;
                state.resolve()
            };

            let ui_weak = ui_weak.clone();
            if let Err(error) = ui_weak.upgrade_in_event_loop(move |ui| {
                apply_resolved(&ui, resolved);
            }) {
                debug!(%error, "appearance update ignored because UI event loop is unavailable");
            }
        })
    }

    pub fn set_theme_preference(&self, ui: &AppWindow, preference: ThemePreference)
        -> Result<(), AppearanceStoreError> {
        let mut next = self.preferences();
        next.theme = preference;
        self.set_preferences(ui, next)
    }

    pub fn set_ui_sounds_enabled(&self, ui: &AppWindow, enabled: bool)
        -> Result<(), AppearanceStoreError> {
        let mut next = self.preferences();
        next.ui_sounds_enabled = enabled;
        self.set_preferences(ui, next)
    }

    pub fn use_system_accent(&self, ui: &AppWindow) -> Result<(), AppearanceStoreError> {
        let mut next = self.preferences();
        next.accent = AccentPreference::System;
        self.set_preferences(ui, next)
    }

    pub fn set_custom_accent(&self, ui: &AppWindow, color: Rgb) -> Result<(), AppearanceStoreError> {
        let mut next = self.preferences();
        next.accent = AccentPreference::Custom(color);
        self.set_preferences(ui, next)
    }

    pub fn update_system_appearance(&self, ui: &AppWindow, system: SystemAppearance) {
        {
            let mut state = lock_state(&self.state);
            state.system = system;
        }
        self.apply(ui);
    }

    fn apply(&self, ui: &AppWindow) {
        let resolved = lock_state(&self.state).resolve();
        apply_resolved(ui, resolved);
    }
}

fn lock_state(state: &Arc<Mutex<AppearanceState>>) -> std::sync::MutexGuard<'_, AppearanceState> {
    match state.lock() {
        Ok(state) => state,
        Err(poisoned) => {
            warn!("appearance state mutex was poisoned; recovering latest state");
            poisoned.into_inner()
        }
    }
}

fn apply_resolved(ui: &AppWindow, appearance: crate::appearance::ResolvedAppearance) {
    let palette = appearance.palette;
    let theme = ui.global::<Theme>();

    theme.set_is_dark(matches!(
        appearance.theme,
        crate::appearance::EffectiveTheme::Dark
    ));
    theme.set_high_contrast(appearance.high_contrast);
    theme.set_appearance_white_swatch(to_slint_color(
        Palette::preview_accent(appearance.theme, Rgb::WHITE, appearance.high_contrast),
    ));
    theme.set_background(to_slint_color(palette.background));
    theme.set_surface(to_slint_color(palette.surface));
    theme.set_surface_raised(to_slint_color(palette.surface_raised));
    theme.set_foreground(to_slint_color(palette.foreground));
    theme.set_secondary_foreground(to_slint_color(palette.secondary_foreground));
    theme.set_divider(to_slint_color(palette.divider));
    theme.set_accent(to_slint_color(palette.accent));
    theme.set_accent_hover(to_slint_color(palette.accent_hover));
    theme.set_accent_pressed(to_slint_color(palette.accent_pressed));
    theme.set_accent_subtle(to_slint_color(palette.accent_subtle));
    theme.set_accent_foreground(to_slint_color(palette.accent_foreground));
    let focus = to_slint_color(palette.focus);
    // Mix toward white instead of using Slint's generic `brighter()` so saturated
    // accents such as red still show the sweep. Keep the mix restrained: enough
    // luminance separation to remain visible while keeping the highlight subtle.
    let focus_highlight =
        to_slint_color(palette.focus.mix(crate::appearance::Rgb::WHITE, 0.34));
    theme.set_focus(focus);
    theme.set_focus_highlight(focus_highlight);

    ui.global::<Motion>()
        .set_reduced_motion(appearance.reduced_motion);
}

fn to_slint_color(color: Rgb) -> slint::Color {
    slint::Color::from_rgb_u8(color.red, color.green, color.blue)
}
