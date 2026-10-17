//! Curated preferences for the bundled core, independent of its C ABI.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePreferences {
    pub freeze_when_unfocused: bool,
    pub resume_last_state: bool,
    pub mgba: MgbaPreferences,
}
impl Default for CorePreferences {
    fn default() -> Self {
        Self {
            freeze_when_unfocused: true,
            resume_last_state: true,
            mgba: MgbaPreferences::default(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MgbaPreferences {
    pub sunlight: Sunlight,
    pub video_filter: VideoFilter,
    /// GBA and Game Boy Color screens; original Game Boy games keep their palette.
    pub color_correction: bool,
    pub game_boy_palette: GameBoyPalette,
    /// Off keeps every Game Boy picture 160×144.
    pub sgb_border: bool,
}

/// Colours for original Game Boy games; Super Game Boy and Game Boy Color
/// games keep their own. A curated few of mGBA's presets.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameBoyPalette {
    #[default]
    Grey,
    Green,
    Pocket,
    Light,
    /// The Game Boy Color's own colours for the games it knows.
    Color,
}
impl GameBoyPalette {
    const ALL: [Self; 5] = [
        Self::Grey,
        Self::Green,
        Self::Pocket,
        Self::Light,
        Self::Color,
    ];
    pub fn step(self, forward: bool) -> Self {
        let index = Self::ALL
            .iter()
            .position(|value| *value == self)
            .unwrap_or(0);
        Self::ALL[(index + if forward { 1 } else { Self::ALL.len() - 1 }) % Self::ALL.len()]
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Grey => "Grey",
            Self::Green => "Original green",
            Self::Pocket => "Pocket",
            Self::Light => "Light",
            Self::Color => "Game Boy Color",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Sunlight(u8);
impl Sunlight {
    pub fn level(self) -> u8 {
        self.0
    }
    pub fn step(self, delta: i32) -> Self {
        Self((i32::from(self.0) + delta).clamp(0, 10) as u8)
    }
}
impl TryFrom<u8> for Sunlight {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value <= 10 {
            Ok(Self(value))
        } else {
            Err("sunlight must be between 0 and 10")
        }
    }
}
impl From<Sunlight> for u8 {
    fn from(value: Sunlight) -> Self {
        value.0
    }
}

/// Horizon's curated output treatments; not arbitrary RetroArch shader presets.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoFilter {
    #[default]
    None,
    Lcd,
    Smooth,
    Xbrz,
}
impl VideoFilter {
    pub fn step(self, forward: bool) -> Self {
        let presets = [Self::None, Self::Lcd, Self::Smooth, Self::Xbrz];
        let index = presets.iter().position(|value| *value == self).unwrap_or(0);
        presets[(index + if forward { 1 } else { presets.len() - 1 }) % presets.len()]
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Lcd => "LCD",
            Self::Smooth => "Smooth",
            Self::Xbrz => "Smooth pixel art",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn core_values_default_to_unfiltered_and_unknown_presets_are_rejected() {
        let values: MgbaPreferences = serde_json::from_str(r#"{"sunlight":7}"#).unwrap();
        assert_eq!(values.video_filter, VideoFilter::None);
        assert!(!values.color_correction);
        assert_eq!(values.sunlight.level(), 7);
        assert!(serde_json::from_str::<MgbaPreferences>(r#"{"video_filter":"missing"}"#).is_err());
        // Files from before the Game Boy options keep working.
        assert_eq!(values.game_boy_palette, GameBoyPalette::Grey);
        assert!(!values.sgb_border);
        let mut palette = GameBoyPalette::Grey;
        for _ in 0..5 {
            assert_eq!(palette.step(true).step(false), palette);
            palette = palette.step(true);
        }
        assert_eq!(palette, GameBoyPalette::Grey);
        assert_eq!(GameBoyPalette::Grey.step(false), GameBoyPalette::Color);
        for filter in [
            VideoFilter::None,
            VideoFilter::Lcd,
            VideoFilter::Smooth,
            VideoFilter::Xbrz,
        ] {
            assert_eq!(filter.step(true).step(false), filter);
            assert_eq!(filter.step(true).step(true).step(true).step(true), filter);
        }
    }
}

/// Session-only playback speed; deliberately bounded to supported factors.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PlaybackSpeed {
    #[default]
    Normal = 1,
    Double = 2,
    Triple = 3,
    Quadruple = 4,
}
impl PlaybackSpeed {
    pub const ALL: [Self; 4] = [Self::Normal, Self::Double, Self::Triple, Self::Quadruple];
    pub fn factor(self) -> u8 {
        self as u8
    }
    pub fn step(self, forward: bool) -> Self {
        Self::ALL[(self.factor() as usize - 1)
            .saturating_add_signed(if forward { 1 } else { -1 })
            .min(3)]
    }
    pub fn cycle(self) -> Self {
        Self::ALL[self.factor() as usize % 4]
    }
}
impl TryFrom<u8> for PlaybackSpeed {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Normal),
            2 => Ok(Self::Double),
            3 => Ok(Self::Triple),
            4 => Ok(Self::Quadruple),
            _ => Err("Unsupported playback speed"),
        }
    }
}

#[cfg(test)]
mod speed_tests {
    use super::*;
    #[test]
    fn speed_boundaries_and_cycle() {
        assert_eq!(PlaybackSpeed::Normal.step(false), PlaybackSpeed::Normal);
        assert_eq!(
            PlaybackSpeed::Quadruple.step(true),
            PlaybackSpeed::Quadruple
        );
        let mut speed = PlaybackSpeed::Normal;
        for expected in [2, 3, 4, 1] {
            speed = speed.cycle();
            assert_eq!(speed.factor(), expected);
        }
        assert!(PlaybackSpeed::try_from(0).is_err());
        assert!(PlaybackSpeed::try_from(5).is_err());
    }
}
