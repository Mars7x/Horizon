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
    pub color_correction: bool,
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
