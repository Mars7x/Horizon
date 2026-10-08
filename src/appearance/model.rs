use super::Rgb;

pub const DEFAULT_ACCENT: Rgb = Rgb::new(53, 132, 228);

/// UI order follows the colour wheel from warm to cool; pink and white end it.
/// Values are visual swatches, not necessarily the contrast-adjusted focus token.
pub const ACCENT_PRESETS: [(&str, Rgb); 9] = [
    ("Red", Rgb::new(229, 72, 77)),
    ("Orange", Rgb::new(242, 140, 40)),
    ("Yellow", Rgb::new(242, 201, 76)),
    ("Green", Rgb::new(60, 185, 120)),
    ("Teal", Rgb::new(36, 182, 168)),
    ("Blue", Rgb::new(53, 132, 228)),
    ("Purple", Rgb::new(139, 92, 246)),
    ("Pink", Rgb::new(230, 93, 167)),
    ("White", Rgb::WHITE),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AppearancePreferences {
    pub theme: ThemePreference,
    pub accent: AccentPreference,
    // Default-on, including configs written before sound settings existed.
    pub ui_sounds_enabled: bool,
}

impl AppearancePreferences {
    pub fn theme_index(self) -> i32 {
        match self.theme { ThemePreference::System => 0, ThemePreference::Light => 1, ThemePreference::Dark => 2 }
    }

    pub fn accent_index(self) -> i32 {
        match self.accent {
            AccentPreference::System => 0,
            AccentPreference::Custom(color) => ACCENT_PRESETS.iter()
                .position(|(_, preset)| *preset == color)
                .map_or(0, |index| index as i32 + 1),
        }
    }
}

impl Default for AppearancePreferences {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            accent: AccentPreference::System,
            ui_sounds_enabled: true,
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
    /// Swatch preview uses precisely the same contrast adjustment as focus and
    /// controls, even when the user has selected another accent colour.
    pub fn preview_accent(theme: EffectiveTheme, raw: Rgb, high_contrast: bool) -> Rgb {
        Self::new(theme, raw, high_contrast).accent
    }

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

        // Accents are used for text and focus strokes across differently toned
        // surfaces. A white or yellow swatch on light mode cannot also be its
        // readable focus token; shade it while preserving the chosen hue.
        // 4.5:1 also exceeds the 3:1 non-text focus contrast requirement.
        let threshold = if high_contrast { 7.0 } else { 4.5 };
        let target = match theme { EffectiveTheme::Light => Rgb::BLACK, EffectiveTheme::Dark => Rgb::WHITE };
        let accent = (0..=100).map(|step| accent.mix(target, step as f32 / 100.0))
            .find(|candidate| [background, surface, surface_raised].iter()
                .all(|surface| candidate.contrast_against(*surface) >= threshold))
            .unwrap_or(target);

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
        DEFAULT_ACCENT, Palette,
    };

    #[test]
    fn all_preset_accents_remain_readable_on_all_theme_surfaces() {
        for theme in [EffectiveTheme::Light, EffectiveTheme::Dark] {
            for high_contrast in [false, true] {
                for (_, swatch) in super::ACCENT_PRESETS {
                    let palette = super::Palette::new(theme, swatch, high_contrast);
                    let min_ratio = if high_contrast { 7.0 } else { 4.5 };
                    for background in [palette.background, palette.surface, palette.surface_raised] {
                        assert!(palette.accent.contrast_against(background) >= min_ratio,
                            "accent {:?} on {:?}: insufficient contrast", swatch, background);
                    }
                    assert!(palette.accent_foreground.contrast_against(palette.accent) >= 4.5);
                }
            }
        }
    }

    #[test]
    fn white_preset_preview_is_the_effective_accent_in_each_theme() {
        for high_contrast in [false, true] {
            let light = Palette::preview_accent(EffectiveTheme::Light, Rgb::WHITE, high_contrast);
            let dark = Palette::preview_accent(EffectiveTheme::Dark, Rgb::WHITE, high_contrast);
            assert_eq!(dark, Rgb::WHITE);
            assert!(light.red < 255);
            assert_eq!(light.red, light.green);
            assert_eq!(light.green, light.blue);
            let surface = Palette::new(EffectiveTheme::Light, Rgb::WHITE, high_contrast).surface;
            assert!(light.contrast_against(surface) >= if high_contrast { 7.0 } else { 4.5 });
        }
    }

    #[test]
    fn system_defaults_are_deterministic_without_a_portal_preference() {
        let resolved = AppearanceState::default().resolve();
        assert_eq!(resolved.theme, EffectiveTheme::Light);
        assert_eq!(resolved.palette.accent, Palette::new(EffectiveTheme::Light, DEFAULT_ACCENT, false).accent);
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

        assert_eq!(state.preferences.accent, AccentPreference::Custom(custom));
        assert!(state.resolve().palette.accent.contrast_against(state.resolve().palette.background) >= 4.5);
    }
}
