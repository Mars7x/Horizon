use super::Rgb;

pub const DEFAULT_ACCENT: Rgb = Rgb::new(53, 132, 228);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccentPreference {
    #[default]
    System,
    Custom(Rgb),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveTheme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppearancePreferences {
    pub theme: ThemePreference,
    pub accent: AccentPreference,
}

impl Default for AppearancePreferences {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            accent: AccentPreference::System,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemAppearance {
    pub preferred_theme: Option<EffectiveTheme>,
    pub accent: Option<Rgb>,
    pub high_contrast: bool,
    pub reduced_motion: bool,
}

impl Default for SystemAppearance {
    fn default() -> Self {
        Self {
            preferred_theme: None,
            accent: None,
            high_contrast: false,
            reduced_motion: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub background: Rgb,
    pub surface: Rgb,
    pub surface_raised: Rgb,
    pub foreground: Rgb,
    pub secondary_foreground: Rgb,
    pub divider: Rgb,
    pub accent: Rgb,
    pub accent_hover: Rgb,
    pub accent_pressed: Rgb,
    pub accent_subtle: Rgb,
    pub accent_foreground: Rgb,
    pub focus: Rgb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedAppearance {
    pub theme: EffectiveTheme,
    pub high_contrast: bool,
    pub reduced_motion: bool,
    pub palette: Palette,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppearanceState {
    pub preferences: AppearancePreferences,
    pub system: SystemAppearance,
}

impl AppearanceState {
    pub fn resolve(self) -> ResolvedAppearance {
        let theme = match self.preferences.theme {
            ThemePreference::System => self.system.preferred_theme.unwrap_or(EffectiveTheme::Light),
            ThemePreference::Light => EffectiveTheme::Light,
            ThemePreference::Dark => EffectiveTheme::Dark,
        };

        let accent = match self.preferences.accent {
            AccentPreference::System => self.system.accent.unwrap_or(DEFAULT_ACCENT),
            AccentPreference::Custom(color) => color,
        };

        ResolvedAppearance {
            theme,
            high_contrast: self.system.high_contrast,
            reduced_motion: self.system.reduced_motion,
            palette: Palette::new(theme, accent, self.system.high_contrast),
        }
    }
}

impl Palette {
    fn new(theme: EffectiveTheme, accent: Rgb, high_contrast: bool) -> Self {
        let (
            background,
            surface,
            surface_raised,
            foreground,
            secondary_foreground,
            divider,
        ) = match (theme, high_contrast) {
            (EffectiveTheme::Light, false) => (
                Rgb::new(255, 255, 255),
                Rgb::new(245, 245, 247),
                Rgb::new(255, 255, 255),
                Rgb::new(24, 24, 27),
                Rgb::new(99, 99, 107),
                Rgb::new(218, 218, 224),
            ),
            (EffectiveTheme::Dark, false) => (
                Rgb::new(17, 17, 19),
                Rgb::new(27, 27, 31),
                Rgb::new(35, 35, 40),
                Rgb::new(246, 246, 247),
                Rgb::new(165, 165, 173),
                Rgb::new(56, 56, 63),
            ),
            (EffectiveTheme::Light, true) => (
                Rgb::WHITE,
                Rgb::WHITE,
                Rgb::WHITE,
                Rgb::BLACK,
                Rgb::new(45, 45, 45),
                Rgb::BLACK,
            ),
            (EffectiveTheme::Dark, true) => (
                Rgb::BLACK,
                Rgb::BLACK,
                Rgb::new(16, 16, 16),
                Rgb::WHITE,
                Rgb::new(220, 220, 220),
                Rgb::WHITE,
            ),
        };

        let (accent_hover, accent_pressed, accent_subtle) = match theme {
            EffectiveTheme::Light => (
                accent.mix(Rgb::BLACK, 0.08),
                accent.mix(Rgb::BLACK, 0.16),
                background.mix(accent, if high_contrast { 0.22 } else { 0.13 }),
            ),
            EffectiveTheme::Dark => (
                accent.mix(Rgb::WHITE, 0.12),
                accent.mix(Rgb::WHITE, 0.20),
                background.mix(accent, if high_contrast { 0.28 } else { 0.18 }),
            ),
        };

        Self {
            background,
            surface,
            surface_raised,
            foreground,
            secondary_foreground,
            divider,
            accent,
            accent_hover,
            accent_pressed,
            accent_subtle,
            accent_foreground: accent.best_foreground(),
            focus: accent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AccentPreference, AppearanceState, EffectiveTheme, Rgb, SystemAppearance, ThemePreference,
        DEFAULT_ACCENT,
    };

    #[test]
    fn system_defaults_are_deterministic_without_a_portal_preference() {
        let resolved = AppearanceState::default().resolve();
        assert_eq!(resolved.theme, EffectiveTheme::Light);
        assert_eq!(resolved.palette.accent, DEFAULT_ACCENT);
    }

    #[test]
    fn system_dark_mode_is_respected() {
        let state = AppearanceState {
            system: SystemAppearance {
                preferred_theme: Some(EffectiveTheme::Dark),
                ..SystemAppearance::default()
            },
            ..AppearanceState::default()
        };

        assert_eq!(state.resolve().theme, EffectiveTheme::Dark);
    }

    #[test]
    fn explicit_theme_override_wins_over_system() {
        let mut state = AppearanceState {
            system: SystemAppearance {
                preferred_theme: Some(EffectiveTheme::Dark),
                ..SystemAppearance::default()
            },
            ..AppearanceState::default()
        };
        state.preferences.theme = ThemePreference::Light;

        assert_eq!(state.resolve().theme, EffectiveTheme::Light);
    }

    #[test]
    fn explicit_accent_override_wins_over_system() {
        let custom = Rgb::new(200, 70, 210);
        let mut state = AppearanceState {
            system: SystemAppearance {
                accent: Some(Rgb::new(10, 20, 30)),
                ..SystemAppearance::default()
            },
            ..AppearanceState::default()
        };
        state.preferences.accent = AccentPreference::Custom(custom);

        assert_eq!(state.resolve().palette.accent, custom);
    }
}
