//! Curated preferences for the bundled core, independent of its C ABI.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePreferences {
    pub freeze_when_unfocused: bool,
    pub resume_last_state: bool,
    pub mgba: MgbaPreferences,
    pub ds: DsPreferences,
    pub three_ds: ThreeDsPreferences,
}

/// Azahar preferences: how Horizon arranges the two screens and the
/// resolution 3D is drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThreeDsPreferences {
    pub layout: DsLayout,
    pub resolution: ThreeDsResolution,
}
impl Default for ThreeDsPreferences {
    /// Stacked, as on the console (and as Azahar draws them).
    fn default() -> Self {
        Self {
            layout: DsLayout::Stacked,
            resolution: ThreeDsResolution::default(),
        }
    }
}

/// How many times the 3DS's own resolution (400×240 above, 320×240 below)
/// Azahar draws at, on the graphics card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct ThreeDsResolution(u8);
impl ThreeDsResolution {
    pub const MAX: u8 = 6;
    pub const fn scale(self) -> u8 {
        self.0
    }
    pub fn step(self, forward: bool) -> Self {
        Self(if forward {
            if self.0 >= Self::MAX { 1 } else { self.0 + 1 }
        } else if self.0 <= 1 {
            Self::MAX
        } else {
            self.0 - 1
        })
    }
    pub fn label(self) -> String {
        if self.0 == 1 {
            "Native".into()
        } else {
            format!("{}× ({}p)", self.0, u32::from(self.0) * 240)
        }
    }
}
impl Default for ThreeDsResolution {
    fn default() -> Self {
        Self(1)
    }
}
impl TryFrom<u8> for ThreeDsResolution {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if (1..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err("3DS resolution must be 1 to 6")
        }
    }
}
impl From<ThreeDsResolution> for u8 {
    fn from(value: ThreeDsResolution) -> Self {
        value.0
    }
}
impl Default for CorePreferences {
    fn default() -> Self {
        Self {
            freeze_when_unfocused: true,
            resume_last_state: true,
            mgba: MgbaPreferences::default(),
            ds: DsPreferences::default(),
            three_ds: ThreeDsPreferences::default(),
        }
    }
}

/// melonDS DS preferences: how Horizon arranges the two screens, the
/// filter each screen gets, and the resolution 3D graphics are drawn at.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DsPreferences {
    pub layout: DsLayout,
    pub video_filter: VideoFilter,
    pub resolution: DsResolution,
}

/// How many times the DS's own resolution its picture is drawn at. Native
/// uses melonDS's software renderer; higher ones its OpenGL renderer, drawn
/// on the graphics card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct DsResolution(u8);
impl DsResolution {
    pub const MAX: u8 = 8;
    pub const fn scale(self) -> u8 {
        self.0
    }
    pub fn step(self, forward: bool) -> Self {
        Self(if forward {
            if self.0 >= Self::MAX { 1 } else { self.0 + 1 }
        } else if self.0 <= 1 {
            Self::MAX
        } else {
            self.0 - 1
        })
    }
    pub fn label(self) -> String {
        if self.0 == 1 {
            "Native".into()
        } else {
            format!("{}× ({}p)", self.0, u32::from(self.0) * 192)
        }
    }
}
impl Default for DsResolution {
    fn default() -> Self {
        Self(1)
    }
}
impl TryFrom<u8> for DsResolution {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if (1..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err("DS resolution must be 1 to 8")
        }
    }
}
impl From<DsResolution> for u8 {
    fn from(value: DsResolution) -> Self {
        value.0
    }
}

/// How the DS's two 256×192 screens are arranged in the player.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DsLayout {
    /// Next to each other: fills a wide screen best.
    #[default]
    SideBySide,
    /// One above the other, as on the console.
    Stacked,
    /// One large screen with the other a third of its size beside it.
    LargeAndSmall,
}

/// A rectangle in the units of the area it was laid out in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl DsLayout {
    const ALL: [Self; 3] = [Self::SideBySide, Self::Stacked, Self::LargeAndSmall];
    pub fn step(self, forward: bool) -> Self {
        let index = Self::ALL
            .iter()
            .position(|value| *value == self)
            .unwrap_or(0);
        Self::ALL[(index + if forward { 1 } else { Self::ALL.len() - 1 }) % Self::ALL.len()]
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::SideBySide => "Side by side",
            Self::Stacked => "Stacked",
            Self::LargeAndSmall => "Large and small",
        }
    }
    pub fn index(self) -> u8 {
        self as u8
    }
    pub fn from_index(index: u8) -> Self {
        Self::ALL
            .get(usize::from(index))
            .copied()
            .unwrap_or_default()
    }
    /// The DS's two 256×192 screens.
    pub const DS_SCREENS: [(f64, f64); 2] = [(256.0, 192.0), (256.0, 192.0)];
    /// The 3DS's 400×240 top screen and 320×240 bottom one.
    pub const THREE_DS_SCREENS: [(f64, f64); 2] = [(400.0, 240.0), (320.0, 240.0)];

    /// Where the DS's top and bottom screens go in an `area` (any units).
    pub fn screens(self, swapped: bool, area: (f64, f64)) -> [ScreenRect; 2] {
        self.place(swapped, area, Self::DS_SCREENS)
    }

    /// Where a top and bottom screen of `sizes` go in an `area` (any
    /// units), fitted and centred, keeping their shapes. `swapped` puts the
    /// bottom screen first (in Large and small, it becomes the large one).
    pub fn place(self, swapped: bool, area: (f64, f64), sizes: [(f64, f64); 2]) -> [ScreenRect; 2] {
        let [a, b] = if swapped { [sizes[1], sizes[0]] } else { sizes };
        // The group's size and each screen's (x, y, scale) within it, in
        // screen pixels, first screen first.
        let (group, first, second) = match self {
            Self::SideBySide => {
                let height = a.1.max(b.1);
                (
                    (a.0 + b.0, height),
                    (0.0, (height - a.1) / 2.0, 1.0),
                    (a.0, (height - b.1) / 2.0, 1.0),
                )
            }
            Self::Stacked => {
                let width = a.0.max(b.0);
                (
                    (width, a.1 + b.1),
                    ((width - a.0) / 2.0, 0.0, 1.0),
                    ((width - b.0) / 2.0, a.1, 1.0),
                )
            }
            // The small screen, a third the size, sits beside the large one,
            // level with its bottom.
            Self::LargeAndSmall => {
                let height = a.1.max(b.1 / 3.0);
                (
                    (a.0 + b.0 / 3.0, height),
                    (0.0, height - a.1, 1.0),
                    (a.0, height - b.1 / 3.0, 1.0 / 3.0),
                )
            }
        };
        let scale = (area.0 / group.0).min(area.1 / group.1).max(0.0);
        let (left, top) = (
            (area.0 - group.0 * scale) / 2.0,
            (area.1 - group.1 * scale) / 2.0,
        );
        let place = |(x, y, size): (f64, f64, f64), (width, height): (f64, f64)| ScreenRect {
            x: left + x * scale,
            y: top + y * scale,
            width: width * size * scale,
            height: height * size * scale,
        };
        let (first, second) = (place(first, a), place(second, b));
        if swapped {
            [second, first]
        } else {
            [first, second]
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
}
impl PlaybackSpeed {
    pub const ALL: [Self; 2] = [Self::Normal, Self::Double];
    pub fn factor(self) -> u8 {
        self as u8
    }
    pub fn step(self, forward: bool) -> Self {
        Self::ALL[(self.factor() as usize - 1)
            .saturating_add_signed(if forward { 1 } else { -1 })
            .min(Self::ALL.len() - 1)]
    }
    pub fn cycle(self) -> Self {
        Self::ALL[self.factor() as usize % Self::ALL.len()]
    }
}
impl TryFrom<u8> for PlaybackSpeed {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Normal),
            2 => Ok(Self::Double),
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
        assert_eq!(PlaybackSpeed::Double.step(true), PlaybackSpeed::Double);
        let mut speed = PlaybackSpeed::Normal;
        for expected in [2, 1, 2, 1] {
            speed = speed.cycle();
            assert_eq!(speed.factor(), expected);
        }
        assert!(PlaybackSpeed::try_from(0).is_err());
        assert!(PlaybackSpeed::try_from(3).is_err());
        assert!(PlaybackSpeed::try_from(4).is_err());
        assert!(PlaybackSpeed::try_from(5).is_err());
    }
}

#[cfg(test)]
mod ds_layout_tests {
    use super::*;

    #[test]
    fn ds_resolution_supports_native_through_eight_and_retains_saved_values() {
        for scale in 1..=8 {
            let value = DsResolution::try_from(scale).unwrap();
            assert_eq!(value.scale(), scale);
            assert_eq!(value.step(true).step(false), value);
            assert_eq!(
                serde_json::from_str::<DsResolution>(&scale.to_string()).unwrap(),
                value
            );
        }
        assert_eq!(DsResolution::try_from(8).unwrap().label(), "8× (1536p)");
        assert_eq!(DsResolution::default().step(false).scale(), 8);
        assert!(DsResolution::try_from(0).is_err());
        assert!(DsResolution::try_from(9).is_err());
    }

    fn rounded(rect: ScreenRect) -> (i64, i64, i64, i64) {
        (
            rect.x.round() as i64,
            rect.y.round() as i64,
            rect.width.round() as i64,
            rect.height.round() as i64,
        )
    }

    #[test]
    fn three_ds_layouts_keep_each_screens_shape() {
        let screens = DsLayout::THREE_DS_SCREENS;
        let area = (1920.0, 1080.0);
        // Stacked: the narrower bottom screen is centred under the top one.
        let [top, bottom] = DsLayout::Stacked.place(false, area, screens);
        assert_eq!(rounded(top), (510, 0, 900, 540));
        assert_eq!(rounded(bottom), (600, 540, 720, 540));
        let [top, bottom] = DsLayout::Stacked.place(true, area, screens);
        assert_eq!(rounded(bottom), (600, 0, 720, 540));
        assert_eq!(rounded(top), (510, 540, 900, 540));
        // Side by side: 720 wide in screen pixels, 1920 / 720 = 2.67×.
        let [top, bottom] = DsLayout::SideBySide.place(false, area, screens);
        assert_eq!(rounded(top), (0, 220, 1067, 640));
        assert_eq!(rounded(bottom), (1067, 220, 853, 640));
        // Large and small, swapped: the touch screen is the large one.
        let [top, bottom] = DsLayout::LargeAndSmall.place(true, area, screens);
        // 320 + 400 / 3 screen pixels across: the width is what limits it.
        assert!((bottom.width + top.width - 1920.0).abs() < 1e-9);
        assert!((bottom.width / bottom.height - 4.0 / 3.0).abs() < 1e-9);
        assert!((top.width / top.height - 5.0 / 3.0).abs() < 1e-9);
        assert!((top.height * 3.0 - bottom.height).abs() < 1e-9);
        assert!((top.y + top.height - (bottom.y + bottom.height)).abs() < 1e-9);
        assert_eq!(ThreeDsPreferences::default().layout, DsLayout::Stacked);
    }

    #[test]
    fn layouts_fit_centre_and_swap() {
        let wide = (3440.0, 1440.0);
        // Side by side on an ultrawide: each screen 1720×1290, centred.
        let [top, bottom] = DsLayout::SideBySide.screens(false, wide);
        assert_eq!(rounded(top), (0, 75, 1720, 1290));
        assert_eq!(rounded(bottom), (1720, 75, 1720, 1290));
        let [top, bottom] = DsLayout::SideBySide.screens(true, wide);
        assert_eq!((rounded(top).0, rounded(bottom).0), (1720, 0));
        // Stacked on 16:9: 720×540 each, one above the other.
        let [top, bottom] = DsLayout::Stacked.screens(false, (1920.0, 1080.0));
        assert_eq!(rounded(top), (600, 0, 720, 540));
        assert_eq!(rounded(bottom), (600, 540, 720, 540));
        // Large and small fills 16:9 exactly: 1440×1080 beside 480×360.
        let [top, bottom] = DsLayout::LargeAndSmall.screens(false, (1920.0, 1080.0));
        assert_eq!(rounded(top), (0, 0, 1440, 1080));
        assert_eq!(rounded(bottom), (1440, 720, 480, 360));
        // Swapped, the touchscreen is the large one.
        let [top, bottom] = DsLayout::LargeAndSmall.screens(true, (1920.0, 1080.0));
        assert_eq!(rounded(bottom), (0, 0, 1440, 1080));
        assert_eq!(rounded(top), (1440, 720, 480, 360));
        for layout in DsLayout::ALL {
            assert_eq!(layout.step(true).step(false), layout);
            assert_eq!(DsLayout::from_index(layout.index()), layout);
        }
    }
}
