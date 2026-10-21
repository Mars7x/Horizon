//! Gameplay bindings use normalized controller buttons, independent of SDL/UI.
use serde::{Deserialize, Serialize};
pub const ACTIONS: [u8; 6] = [8, 0, 10, 11, 3, 2];
pub const ACTION_NAMES: [&str; 6] = ["A", "B", "L", "R", "Start", "Select"];
pub const SOURCES: [u8; 10] = [8, 0, 9, 1, 10, 11, 12, 13, 3, 2];
/// Nintendo DS has four face buttons, so its mapping has X and Y too, and a
/// Microphone control: held, the core hears a blow (its mic button, L3).
pub const DS_ACTIONS: [u8; 9] = [8, 0, 9, 1, 10, 11, 3, 2, 14];
pub const DS_ACTION_NAMES: [&str; 9] = [
    "A",
    "B",
    "X",
    "Y",
    "L",
    "R",
    "Start",
    "Select",
    "Microphone",
];
/// The DS defaults: the same buttons, and the left trigger blows.
const DS_DEFAULTS: [u8; 9] = [8, 0, 9, 1, 10, 11, 3, 2, 12];

/// One system's mapping: for each of its actions (in `$actions` order), the
/// controller button that presses it. A and B lead, so the global swap can
/// exchange them.
macro_rules! bindings {
    ($(#[$serde:meta])* $name:ident, $count:literal, $actions:ident, $defaults:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        $(#[$serde])*
        pub struct $name([u8; $count]);
        impl Default for $name {
            fn default() -> Self {
                Self($defaults)
            }
        }
        impl TryFrom<[u8; $count]> for $name {
            type Error = &'static str;
            fn try_from(values: [u8; $count]) -> Result<Self, Self::Error> {
                if values.iter().any(|v| !SOURCES.contains(v))
                    || values
                        .iter()
                        .enumerate()
                        .any(|(i, v)| values[..i].contains(v))
                {
                    return Err("Invalid or duplicate gameplay binding");
                }
                Ok(Self(values))
            }
        }
        impl From<$name> for [u8; $count] {
            fn from(v: $name) -> Self {
                v.0
            }
        }
        impl $name {
            pub fn source(self, row: usize, swapped: bool) -> Option<u8> {
                self.0
                    .get(if swapped && row < 2 { 1 - row } else { row })
                    .copied()
            }
            pub fn assign(mut self, row: usize, source: u8, swapped: bool) -> Self {
                if row >= $actions.len() || !SOURCES.contains(&source) {
                    return self;
                }
                let row = if swapped && row < 2 { 1 - row } else { row };
                if let Some(other) = self.0.iter().position(|v| *v == source) {
                    self.0.swap(row, other);
                } else {
                    self.0[row] = source;
                }
                self
            }
            pub fn apply(self, buttons: u16) -> u16 {
                let mut mapped = buttons & 0xF0; // D-pad/stick directions remain available.
                for (target, source) in $actions.into_iter().zip(self.0) {
                    if buttons & (1 << source) != 0 {
                        mapped |= 1 << target;
                    }
                }
                mapped
            }
        }
    };
}
bindings!(
    #[serde(try_from = "[u8; 6]", into = "[u8; 6]")]
    GameplayBindings,
    6,
    ACTIONS,
    ACTIONS
);
bindings!(
    #[serde(try_from = "Vec<u8>", into = "Vec<u8>")]
    DsBindings,
    9,
    DS_ACTIONS,
    DS_DEFAULTS
);
/// Saved DS mappings from before the Microphone row have eight entries: they
/// keep them and gain its default (or the first free button).
impl TryFrom<Vec<u8>> for DsBindings {
    type Error = &'static str;
    fn try_from(mut values: Vec<u8>) -> Result<Self, Self::Error> {
        if values.len() == 8 {
            let free = std::iter::once(DS_DEFAULTS[8])
                .chain(SOURCES)
                .find(|source| !values.contains(source))
                .ok_or("No free button for the DS microphone")?;
            values.push(free);
        }
        let values: [u8; 9] = values
            .try_into()
            .map_err(|_| "A DS mapping has nine controls")?;
        Self::try_from(values)
    }
}
impl From<DsBindings> for Vec<u8> {
    fn from(value: DsBindings) -> Self {
        <[u8; 9]>::from(value).to_vec()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControllerPreferences {
    pub swap_confirm: bool,
    pub swap_hints: bool,
    pub gba_bindings: GameplayBindings,
    pub ds_bindings: DsBindings,
    pub shortcuts: Shortcuts,
}

pub const SHORTCUT_NAMES: [&str; 4] = ["Fast-forward", "Quick save", "Quick load", "Screenshot"];

/// In-game shortcuts, shared by every system: one or two controller buttons
/// (`SOURCES`) for each, or none. A one-button shortcut's button is held back
/// from games; a pair's buttons still reach the game alone and are held back
/// only while the whole pair is down. No button serves two shortcuts.
/// Fast-forward works while held; the others act once per press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SavedShortcuts", into = "Vec<Vec<u8>>")]
pub struct Shortcuts([u16; 4]);

/// Keep the original single-button/null settings readable when adding pairs.
/// Writes always use the current list-of-buttons format.
#[derive(Deserialize)]
#[serde(untagged)]
enum SavedShortcuts {
    Buttons(Vec<Vec<u8>>),
    SingleButtons(Vec<Option<u8>>),
}

impl TryFrom<SavedShortcuts> for Shortcuts {
    type Error = &'static str;

    fn try_from(saved: SavedShortcuts) -> Result<Self, Self::Error> {
        let rows = match saved {
            SavedShortcuts::Buttons(rows) => rows,
            SavedShortcuts::SingleButtons(rows) => rows
                .into_iter()
                .map(|button| button.into_iter().collect())
                .collect(),
        };
        Self::try_from(rows)
    }
}

impl Default for Shortcuts {
    /// Both triggers fast-forward: a pair, so either trigger alone still
    /// reaches the game (the DS microphone is the left trigger).
    fn default() -> Self {
        Self([1 << 12 | 1 << 13, 0, 0, 0])
    }
}
impl TryFrom<Vec<Vec<u8>>> for Shortcuts {
    type Error = &'static str;
    fn try_from(rows: Vec<Vec<u8>>) -> Result<Self, Self::Error> {
        let mut masks = [0u16; 4];
        if rows.len() != masks.len() {
            return Err("Shortcuts have four rows");
        }
        for (mask, row) in masks.iter_mut().zip(&rows) {
            if row.len() > 2 || row.iter().any(|v| !SOURCES.contains(v)) {
                return Err("Invalid shortcut button");
            }
            *mask = row.iter().fold(0, |mask, bit| mask | 1 << bit);
            if mask.count_ones() as usize != row.len() {
                return Err("Duplicate shortcut button");
            }
        }
        let all = masks.iter().fold(0u16, |all, mask| all | mask);
        if all.count_ones() != masks.iter().map(|m| m.count_ones()).sum::<u32>() {
            return Err("A button serves two shortcuts");
        }
        Ok(Self(masks))
    }
}
impl From<Shortcuts> for Vec<Vec<u8>> {
    fn from(value: Shortcuts) -> Self {
        value
            .0
            .iter()
            .map(|mask| {
                SOURCES
                    .into_iter()
                    .filter(|bit| mask & (1 << bit) != 0)
                    .collect()
            })
            .collect()
    }
}
impl Shortcuts {
    pub const FAST_FORWARD: usize = 0;
    pub const QUICK_SAVE: usize = 1;
    pub const QUICK_LOAD: usize = 2;
    pub const SCREENSHOT: usize = 3;
    /// `row`'s buttons, in `SOURCES` order (none, one or two).
    pub fn buttons(self, row: usize) -> Vec<u8> {
        let mask = self.0.get(row).copied().unwrap_or(0);
        SOURCES
            .into_iter()
            .filter(|bit| mask & (1 << bit) != 0)
            .collect()
    }
    /// Give `row` one or two buttons (a mask of `SOURCES` bits); shortcuts
    /// that used any of them lose them.
    pub fn assign(mut self, row: usize, buttons: u16) -> Self {
        let valid = SOURCES.iter().fold(0u16, |all, bit| all | 1 << bit);
        if row >= self.0.len() || buttons == 0 || buttons & !valid != 0 || buttons.count_ones() > 2
        {
            return self;
        }
        for (index, mask) in self.0.iter_mut().enumerate() {
            if index != row && *mask & buttons != 0 {
                *mask = 0;
            }
        }
        self.0[row] = buttons;
        self
    }
    pub fn clear(mut self, row: usize) -> Self {
        if let Some(value) = self.0.get_mut(row) {
            *value = 0;
        }
        self
    }
    pub fn is_set(self, row: usize) -> bool {
        self.0.get(row).is_some_and(|mask| *mask != 0)
    }
    /// Every button some shortcut uses.
    pub fn mask(self) -> u16 {
        self.0.iter().fold(0, |all, mask| all | mask)
    }
    /// Whether all of `row`'s buttons are down in `buttons`.
    pub fn held(self, row: usize, buttons: u16) -> bool {
        self.0
            .get(row)
            .is_some_and(|&mask| mask != 0 && buttons & mask == mask)
    }
    /// The buttons to hold back from the game given what's held: single
    /// shortcut buttons always, a pair's only while the whole pair is down.
    pub fn withheld(self, buttons: u16) -> u16 {
        self.0.iter().fold(0, |out, &mask| {
            if mask.count_ones() == 1 || (mask != 0 && buttons & mask == mask) {
                out | mask
            } else {
                out
            }
        })
    }
}
impl Default for ControllerPreferences {
    fn default() -> Self {
        Self {
            swap_confirm: false,
            swap_hints: true,
            gba_bindings: GameplayBindings::default(),
            ds_bindings: DsBindings::default(),
            shortcuts: Shortcuts::default(),
        }
    }
}
impl ControllerPreferences {
    pub const fn hints_swapped(self) -> bool {
        self.swap_confirm && self.swap_hints
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remapping_swaps_conflicts_and_respects_global_swap() {
        let defaults = GameplayBindings::default();
        let map = defaults.assign(0, 0, false);
        assert_eq!(map.source(0, false), Some(0));
        assert_eq!(map.source(1, false), Some(8));
        assert_eq!(map.apply(1), 256);
        assert_eq!(map.apply(256), 1);
        let map = defaults.assign(0, 9, true);
        assert_eq!(map.source(0, true), Some(9));
        assert_eq!(map.apply(1 << 9), 1);
        assert_eq!(map.apply(0xF0), 0xF0);
        assert!(GameplayBindings::try_from([8, 8, 10, 11, 3, 2]).is_err());
        assert!(GameplayBindings::try_from([8, 0, 10, 15, 3, 2]).is_err());
    }
    #[test]
    fn ds_mapping_covers_x_and_y_and_keeps_old_files() {
        let defaults = DsBindings::default();
        assert_eq!(defaults.apply(1 << 9), 1 << 9);
        assert_eq!(defaults.apply(1 << 1), 1 << 1);
        let map = defaults.assign(2, 1, false);
        assert_eq!(map.source(2, false), Some(1));
        assert_eq!(map.source(3, false), Some(9));
        assert_eq!(map.apply(1 << 1), 1 << 9);
        assert!(DsBindings::try_from([8, 0, 9, 1, 10, 11, 3, 3, 12]).is_err());
        // The left trigger blows by default; eight-control files gain it.
        assert_eq!(defaults.apply(1 << 12), 1 << 14);
        let eight: DsBindings = serde_json::from_str("[8,0,9,1,10,11,3,2]").unwrap();
        assert_eq!(eight, DsBindings::default());
        let taken: DsBindings = serde_json::from_str("[8,0,9,1,10,12,3,2]").unwrap();
        assert_eq!(taken.source(8, false), Some(11));
        let old: ControllerPreferences =
            serde_json::from_str(r#"{"swap_confirm":false,"gba_bindings":[8,0,10,11,3,2]}"#)
                .unwrap();
        assert_eq!(old.ds_bindings, DsBindings::default());
    }
    #[test]
    fn legacy_shortcuts_preserve_bindings_and_write_the_current_format() {
        let old: ControllerPreferences = serde_json::from_str(
            r#"{"swap_confirm":true,"swap_hints":false,"shortcuts":[13,null,9,0]}"#,
        )
        .unwrap();
        assert!(old.swap_confirm);
        assert!(!old.swap_hints);
        assert_eq!(old.shortcuts.buttons(Shortcuts::FAST_FORWARD), vec![13]);
        assert!(!old.shortcuts.is_set(Shortcuts::QUICK_SAVE));
        assert_eq!(old.shortcuts.buttons(Shortcuts::QUICK_LOAD), vec![9]);
        assert_eq!(old.shortcuts.buttons(Shortcuts::SCREENSHOT), vec![0]);
        let saved = serde_json::to_string(&old).unwrap();
        assert!(saved.contains(r#""shortcuts":[[13],[],[9],[0]]"#));
        assert_eq!(
            serde_json::from_str::<ControllerPreferences>(&saved).unwrap(),
            old
        );
        // The reported configuration keeps its right-trigger binding.
        let reported: Shortcuts = serde_json::from_str("[13,null,null,null]").unwrap();
        assert_eq!(reported.mask(), 1 << 13);
        // Legacy data still goes through the same validation as button pairs.
        for invalid in ["[13,null,null]", "[13,13,null,null]", "[4,null,null,null]"] {
            assert!(serde_json::from_str::<Shortcuts>(invalid).is_err());
        }
        let pairs: Shortcuts = serde_json::from_str("[[12,13],[],[9],[0]]").unwrap();
        assert_eq!(pairs.buttons(Shortcuts::FAST_FORWARD), vec![12, 13]);
    }

    #[test]
    fn shortcuts_take_pairs_and_hold_back_only_what_they_use() {
        let defaults = Shortcuts::default();
        assert_eq!(defaults.buttons(Shortcuts::FAST_FORWARD), vec![12, 13]);
        // A pair: either trigger alone still reaches the game.
        assert_eq!(defaults.withheld(1 << 12), 0);
        assert_eq!(defaults.withheld(1 << 12 | 1 << 13), 1 << 12 | 1 << 13);
        assert!(defaults.held(Shortcuts::FAST_FORWARD, 1 << 12 | 1 << 13 | 1));
        assert!(!defaults.held(Shortcuts::FAST_FORWARD, 1 << 13));
        // One button: always held back. Taking a pair's button clears it.
        let map = defaults.assign(Shortcuts::QUICK_SAVE, 1 << 13);
        assert!(!map.is_set(Shortcuts::FAST_FORWARD));
        assert_eq!(map.withheld(1 << 13), 1 << 13);
        assert_eq!(
            map.assign(Shortcuts::QUICK_LOAD, 1 << 9 | 1 << 1 | 1 << 8),
            map
        );
        assert_eq!(map.clear(Shortcuts::QUICK_SAVE).mask(), 0);
        let round: Vec<Vec<u8>> = defaults.into();
        assert_eq!(Shortcuts::try_from(round).unwrap(), defaults);
        assert!(Shortcuts::try_from(vec![vec![12, 13], vec![13], vec![], vec![]]).is_err());
        let old: ControllerPreferences = serde_json::from_str(r#"{"swap_confirm":false}"#).unwrap();
        assert_eq!(old.shortcuts, Shortcuts::default());
    }
}
