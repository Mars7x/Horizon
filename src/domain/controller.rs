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
}
impl Default for ControllerPreferences {
    fn default() -> Self {
        Self {
            swap_confirm: false,
            swap_hints: true,
            gba_bindings: GameplayBindings::default(),
            ds_bindings: DsBindings::default(),
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
}
