//! One shared rule for directional focus navigation across Horizon.
//!
//! A held/repeated input advances up to an edge, then stops. Only a fresh
//! press while ALREADY on that edge is allowed to wrap to the opposite end.

pub fn step_with_edge_wrap(current: i32, first: i32, last: i32, delta: i32, repeated: bool) -> i32 {
    if first > last || delta == 0 {
        return current;
    }
    let at = current.clamp(first, last);
    if !repeated && at == first && delta < 0 {
        last
    } else if !repeated && at == last && delta > 0 {
        first
    } else {
        at.saturating_add(delta).clamp(first, last)
    }
}

#[cfg(test)]
mod tests {
    use super::step_with_edge_wrap as step;

    #[test]
    fn held_repeat_stops_at_both_boundaries() {
        for item in 0..9 {
            assert_eq!(step(item, 0, 9, 1, true), item + 1);
        }
        assert_eq!(step(9, 0, 9, 1, true), 9);
        assert_eq!(step(0, 0, 9, -1, true), 0);
    }

    #[test]
    fn fresh_press_wraps_only_if_already_at_boundary() {
        assert_eq!(step(8, 0, 9, 1, false), 9);
        assert_eq!(step(9, 0, 9, 1, false), 0);
        assert_eq!(step(1, 0, 9, -1, false), 0);
        assert_eq!(step(0, 0, 9, -1, false), 9);
    }

    #[test]
    fn one_item_and_invalid_ranges_are_safe() {
        assert_eq!(step(0, 0, 0, 1, false), 0);
        assert_eq!(step(0, 0, 0, 1, true), 0);
        assert_eq!(step(4, 8, 7, 1, false), 4);
    }
}
