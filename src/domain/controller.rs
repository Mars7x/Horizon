//! Gameplay bindings use normalized controller buttons, independent of SDL/UI.
use serde::{Deserialize, Serialize};
pub const ACTIONS: [u8; 6] = [8, 0, 10, 11, 3, 2];
pub const ACTION_NAMES: [&str; 6] = ["A", "B", "L", "R", "Start", "Select"];
pub const SOURCES: [u8; 10] = [8, 0, 9, 1, 10, 11, 12, 13, 3, 2];
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "[u8; 6]", into = "[u8; 6]")]
pub struct GameplayBindings([u8; 6]);
impl Default for GameplayBindings {
    fn default() -> Self {
        Self(ACTIONS)
    }
}
impl TryFrom<[u8; 6]> for GameplayBindings {
    type Error = &'static str;
    fn try_from(values: [u8; 6]) -> Result<Self, Self::Error> {
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
impl From<GameplayBindings> for [u8; 6] {
    fn from(v: GameplayBindings) -> Self {
        v.0
    }
}
impl GameplayBindings {
    pub fn source(self, row: usize, swapped: bool) -> Option<u8> {
        self.0
            .get(if swapped && row < 2 { 1 - row } else { row })
            .copied()
    }
    pub fn assign(mut self, row: usize, source: u8, swapped: bool) -> Self {
        if row >= ACTIONS.len() || !SOURCES.contains(&source) {
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
        for (target, source) in ACTIONS.into_iter().zip(self.0) {
            if buttons & (1 << source) != 0 {
                mapped |= 1 << target;
            }
        }
        mapped
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControllerPreferences {
    pub swap_confirm: bool,
    pub swap_hints: bool,
    pub gba_bindings: GameplayBindings,
}
impl Default for ControllerPreferences {
    fn default() -> Self {
        Self {
            swap_confirm: false,
            swap_hints: true,
            gba_bindings: GameplayBindings::default(),
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
}
