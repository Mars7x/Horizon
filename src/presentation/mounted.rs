//! Long grids mount only the items near their camera (Album, the Shop). As
//! the camera moves, the mounted window scrolls in place: the items that
//! stay keep their Slint elements, so their focus and motion carry on.

use slint::{Model, VecModel};

/// The mounted window only scrolled: the items in both windows' overlap are
/// the same ones (by `key`), in the same places.
pub(crate) fn window_shift<K: PartialEq>(
    old: &[K],
    old_start: usize,
    new: &[K],
    new_start: usize,
) -> bool {
    let from = old_start.max(new_start);
    let to = (old_start + old.len()).min(new_start + new.len());
    from < to && (from..to).all(|at| old[at - old_start] == new[at - new_start])
}

/// Scroll the mounted rows in place: drop the rows that left, add the ones
/// that arrived, and update the rest where they changed.
pub(crate) fn shift_window<T: Clone + PartialEq + 'static>(
    model: &VecModel<T>,
    old_start: usize,
    new_start: usize,
    rows: Vec<T>,
) {
    let new_end = new_start + rows.len();
    // Leading rows that left, then trailing ones.
    for _ in old_start..new_start.max(old_start) {
        model.remove(0);
    }
    let kept_start = old_start.max(new_start);
    while kept_start + model.row_count() > new_end {
        model.remove(model.row_count() - 1);
    }
    // Leading rows that arrived (inserted last first), then trailing ones.
    for at in (new_start..kept_start).rev() {
        model.insert(0, rows[at - new_start].clone());
    }
    let kept = model.row_count();
    for row in rows.iter().skip(kept) {
        model.push(row.clone());
    }
    for (index, row) in rows.into_iter().enumerate() {
        if model.row_data(index).as_ref() != Some(&row) {
            model.set_row_data(index, row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scrolled_window_is_one_that_overlaps_in_place() {
        let keys = |range: std::ops::Range<usize>| -> Vec<String> {
            range.map(|at| format!("{at}.png")).collect()
        };
        assert!(window_shift(&keys(0..12), 0, &keys(4..16), 4));
        assert!(window_shift(&keys(4..16), 4, &keys(0..12), 0));
        // No overlap, or the items moved: rebuild.
        assert!(!window_shift(&keys(0..4), 0, &keys(8..12), 8));
        assert!(!window_shift(&keys(0..12), 0, &keys(1..13), 0));
    }

    #[test]
    fn scrolling_keeps_the_rows_that_stay() {
        for (old, new) in [(0..12, 4..16), (4..16, 0..12), (0..12, 0..8), (0..8, 0..12)] {
            let model = VecModel::from(old.clone().collect::<Vec<usize>>());
            shift_window(&model, old.start, new.start, new.clone().collect());
            assert_eq!(
                model.iter().collect::<Vec<_>>(),
                new.collect::<Vec<_>>(),
                "{old:?} to the new window"
            );
        }
    }
}
