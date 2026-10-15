//! The on-screen keyboard's layout and focus policy.
//!
//! Drawn in Horizon's own window. Five rows, each 12 units wide, keep keys
//! aligned in columns so Up/Down lands where the eye expects. Two layers share the frame: letters (with
//! Shift for capitals) and symbols. Focus is a (row, key) position; Left and
//! Right follow the shared edge rule (a held press stops at the end, a fresh
//! one wraps), and Up and Down go to the key in the next row whose centre is
//! nearest, so a wide key (Space, Done) is reached from any key above it.

use std::time::Duration;

use super::step_with_edge_wrap;

/// Two Shift presses this close together lock capitals.
pub const DOUBLE_TAP: Duration = Duration::from_millis(400);

/// Shift (the key, L3 or R3): one tap capitalises the next letter; a quick
/// second tap locks capitals; a tap while locked turns them off. A slow
/// second tap just undoes the first.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    #[default]
    Off,
    /// The next letter only.
    Once,
    Locked,
}

impl Shift {
    /// The state after a tap, `since_last` after the previous one.
    pub fn tap(self, since_last: Option<Duration>) -> Self {
        match self {
            Self::Off => Self::Once,
            Self::Once if since_last.is_some_and(|gap| gap <= DOUBLE_TAP) => Self::Locked,
            Self::Once | Self::Locked => Self::Off,
        }
    }

    /// After a letter is typed: Once is used up, Locked stays.
    pub fn after_letter(self) -> Self {
        match self {
            Self::Once => Self::Off,
            other => other,
        }
    }
}

/// Every row's width in units.
pub const ROW_UNITS: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// Types this character (its capital with Shift, on the letters layer).
    Char(char),
    Backspace,
    Shift,
    /// Switch between letters and symbols.
    Layer,
    Space,
    /// Finish: the editor saves.
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub kind: KeyKind,
    /// Left edge and width, in units of `ROW_UNITS`.
    pub x: f32,
    pub width: f32,
}

impl Key {
    fn centre(self) -> f32 {
        self.x + self.width / 2.0
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    #[default]
    Letters,
    Symbols,
}

/// A row from its characters, then any wide keys after them.
fn row(chars: &str, before: &[(KeyKind, f32)], after: &[(KeyKind, f32)]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut x = 0.0;
    let mut push = |kind, width: f32| {
        keys.push(Key { kind, x, width });
        x += width;
    };
    for &(kind, width) in before {
        push(kind, width);
    }
    for c in chars.chars() {
        push(KeyKind::Char(c), 1.0);
    }
    for &(kind, width) in after {
        push(kind, width);
    }
    keys
}

/// The layer's rows, top to bottom.
pub fn rows(layer: Layer) -> Vec<Vec<Key>> {
    let bottom = row(
        "",
        &[(KeyKind::Layer, 2.0), (KeyKind::Space, 7.0)],
        &[(KeyKind::Done, 3.0)],
    );
    match layer {
        Layer::Letters => vec![
            row("1234567890", &[], &[(KeyKind::Backspace, 2.0)]),
            row("qwertyuiop-_", &[], &[]),
            row("asdfghjkl'.@", &[], &[]),
            row("zxcvbnm,?!", &[(KeyKind::Shift, 2.0)], &[]),
            bottom,
        ],
        Layer::Symbols => vec![
            row("1234567890", &[], &[(KeyKind::Backspace, 2.0)]),
            row("!@#$%^&*()[]", &[], &[]),
            row("~`+=|\\{}:;\"'", &[], &[]),
            row("<>/?,.-_€£¥§", &[], &[]),
            bottom,
        ],
    }
}

/// Where focus is: a row, and a key within it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KeyFocus {
    pub row: usize,
    pub key: usize,
}

impl KeyFocus {
    /// Its index among all keys, row by row (the order Slint draws them in).
    pub fn flat(self, rows: &[Vec<Key>]) -> usize {
        rows[..self.row].iter().map(Vec::len).sum::<usize>() + self.key
    }

    /// The focus at a flat index.
    pub fn from_flat(rows: &[Vec<Key>], mut index: usize) -> Option<Self> {
        for (row, keys) in rows.iter().enumerate() {
            if index < keys.len() {
                return Some(Self { row, key: index });
            }
            index -= keys.len();
        }
        None
    }

    /// The same place on another layer (rows can hold different counts).
    pub fn clamped(self, rows: &[Vec<Key>]) -> Self {
        let row = self.row.min(rows.len() - 1);
        Self {
            row,
            key: self.key.min(rows[row].len() - 1),
        }
    }

    /// Move by `delta` keys within the row (Left/Right).
    pub fn step_across(self, rows: &[Vec<Key>], delta: i32, repeated: bool) -> Self {
        let last = rows[self.row].len() as i32 - 1;
        Self {
            row: self.row,
            key: step_with_edge_wrap(self.key as i32, 0, last, delta, repeated) as usize,
        }
    }

    /// Move to the next row up or down, onto its key nearest below or above
    /// this one's centre. The top and bottom rows are ends, not wraps.
    pub fn step_rows(self, rows: &[Vec<Key>], delta: i32) -> Self {
        let target = self.row as i32 + delta;
        if target < 0 || target >= rows.len() as i32 {
            return self;
        }
        let target = target as usize;
        let centre = rows[self.row][self.key].centre();
        let key = rows[target]
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (a.centre() - centre)
                    .abs()
                    .total_cmp(&(b.centre() - centre).abs())
            })
            .map_or(0, |(index, _)| index);
        Self { row: target, key }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_fills_the_same_width() {
        for layer in [Layer::Letters, Layer::Symbols] {
            for keys in rows(layer) {
                let last = keys.last().unwrap();
                assert_eq!(last.x + last.width, ROW_UNITS, "{layer:?}");
            }
        }
    }

    #[test]
    fn up_and_down_land_on_the_nearest_key() {
        let rows = rows(Layer::Letters);
        // From "t" (row 1, column 4) down to "g", then "b", then Space.
        let mut focus = KeyFocus { row: 1, key: 4 };
        focus = focus.step_rows(&rows, 1);
        assert_eq!(rows[focus.row][focus.key].kind, KeyKind::Char('g'));
        focus = focus.step_rows(&rows, 1);
        // Row 3 starts with a 2-unit Shift: "c" (centre 4.5) is under "g".
        assert_eq!(rows[focus.row][focus.key].kind, KeyKind::Char('c'));
        focus = focus.step_rows(&rows, 1);
        assert_eq!(rows[focus.row][focus.key].kind, KeyKind::Space);
        // The bottom row is an end.
        assert_eq!(focus.step_rows(&rows, 1), focus);
        // Done, on the right, goes up to the right of the row above.
        let done = KeyFocus { row: 4, key: 2 };
        let up = done.step_rows(&rows, -1);
        assert_eq!(rows[up.row][up.key].kind, KeyKind::Char('?'));
    }

    #[test]
    fn shift_is_once_double_tap_locks_and_a_tap_unlocks() {
        let quick = Some(Duration::from_millis(200));
        let slow = Some(Duration::from_millis(900));
        assert_eq!(Shift::Off.tap(None), Shift::Once);
        assert_eq!(Shift::Once.after_letter(), Shift::Off);
        assert_eq!(Shift::Once.tap(quick), Shift::Locked);
        assert_eq!(Shift::Once.tap(slow), Shift::Off);
        assert_eq!(Shift::Locked.after_letter(), Shift::Locked);
        assert_eq!(Shift::Locked.tap(quick), Shift::Off);
        assert_eq!(Shift::Locked.tap(slow), Shift::Off);
    }

    #[test]
    fn left_and_right_follow_the_edge_rule() {
        let rows = rows(Layer::Letters);
        let first = KeyFocus { row: 0, key: 0 };
        // A fresh press wraps to Backspace; a held one stays.
        assert_eq!(first.step_across(&rows, -1, false).key, 10);
        assert_eq!(first.step_across(&rows, -1, true), first);
    }

    #[test]
    fn flat_indices_round_trip_and_layers_keep_a_valid_focus() {
        let letters = rows(Layer::Letters);
        let symbols = rows(Layer::Symbols);
        let focus = KeyFocus { row: 3, key: 9 };
        assert_eq!(
            KeyFocus::from_flat(&letters, focus.flat(&letters)),
            Some(focus)
        );
        assert_eq!(KeyFocus::from_flat(&letters, 10_000), None);
        // A layer with fewer keys in a row keeps focus on a real key.
        let moved = focus.clamped(&symbols);
        assert!(moved.key < symbols[moved.row].len());
    }
}
