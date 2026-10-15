//! Curated preferences for the bundled core, independent of its C ABI.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SavedCorePreferences")]
pub struct CorePreferences {
    pub freeze_when_unfocused: bool,
    pub mgba: MgbaPreferences,
}
impl Default for CorePreferences {
    fn default() -> Self {
        Self {
            freeze_when_unfocused: true,
            mgba: MgbaPreferences::default(),
        }
    }
}

// Read the former core-specific setting without rewriting a user's file on
// load. A shared value, when present, takes precedence over the legacy field.
#[derive(Default, Deserialize)]
#[serde(default)]
struct SavedCorePreferences {
    freeze_when_unfocused: Option<bool>,
    mgba: SavedMgbaPreferences,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct SavedMgbaPreferences {
    #[serde(flatten)]
    values: MgbaPreferences,
    freeze_when_unfocused: Option<bool>,
}
impl From<SavedCorePreferences> for CorePreferences {
    fn from(saved: SavedCorePreferences) -> Self {
        Self {
            freeze_when_unfocused: saved
                .freeze_when_unfocused
                .or(saved.mgba.freeze_when_unfocused)
                .unwrap_or(true),
            mgba: saved.mgba.values,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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
    fn older_files_default_to_unfiltered_and_unknown_presets_are_rejected() {
        let values: CorePreferences =
            serde_json::from_str(r#"{"mgba":{"audio_smoothing":true,"sunlight":7}}"#).unwrap();
        assert_eq!(values.mgba.video_filter, VideoFilter::None);
        assert!(!values.mgba.color_correction);
        assert!(values.freeze_when_unfocused);
        assert_eq!(values.mgba.sunlight.level(), 7);
        assert!(
            serde_json::from_str::<CorePreferences>(r#"{"mgba":{"video_filter":"missing"}}"#)
                .is_err()
        );
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

#[cfg(test)]
mod compatibility_tests {
    use super::*;
    #[test]
    fn shared_freeze_preserves_legacy_false_and_wins_over_legacy_values() {
        for (json, expected) in [
            (
                r#"{"mgba":{"freeze_when_unfocused":false,"audio_smoothing":true}}"#,
                false,
            ),
            (
                r#"{"freeze_when_unfocused":true,"mgba":{"freeze_when_unfocused":false}}"#,
                true,
            ),
            (
                r#"{"freeze_when_unfocused":false,"mgba":{"freeze_when_unfocused":true}}"#,
                false,
            ),
            (r#"{}"#, true),
        ] {
            let values: CorePreferences = serde_json::from_str(json).unwrap();
            assert_eq!(values.freeze_when_unfocused, expected);
            let json = serde_json::to_value(values).unwrap();
            assert_eq!(json["freeze_when_unfocused"], expected);
            assert!(json["mgba"].get("freeze_when_unfocused").is_none());
            assert!(json["mgba"].get("audio_smoothing").is_none());
            assert_eq!(
                serde_json::from_value::<CorePreferences>(json).unwrap(),
                values
            );
        }
    }
}
