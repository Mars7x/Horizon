//! Focus and camera rules shared by Horizon's grids (Library, Album).
//! Selection is an index in reading order; the camera is the first whole row
//! on screen.

pub fn total_rows(count: usize, columns: usize) -> usize {
    count.div_ceil(columns.max(1))
}

// Camera-only pointer scrolling. Focus identity and metadata are deliberately
// independent from this viewport movement.
pub fn wheel_scroll_top(
    top: usize,
    count: usize,
    columns: usize,
    visible: usize,
    delta: i32,
) -> usize {
    let max_top = total_rows(count, columns).saturating_sub(visible);
    (top as i64 + i64::from(delta)).clamp(0, max_top as i64) as usize
}

pub fn scroll_to_selection(
    selection: usize,
    columns: usize,
    visible: usize,
    previous_top: usize,
    count: usize,
) -> usize {
    let row = selection / columns.max(1);
    let max_top = total_rows(count, columns).saturating_sub(visible);
    let next = if row < previous_top {
        row
    } else if row >= previous_top + visible {
        row + 1 - visible
    } else {
        previous_top
    };
    next.min(max_top)
}

/// Horizontal navigation follows reading order through row boundaries on
/// both fresh presses and held repeat. Neither end wraps to the opposite end.
pub fn horizontal_step(selected: usize, count: usize, delta: i32) -> usize {
    if count == 0 {
        return 0;
    }
    if delta > 0 {
        (selected + 1).min(count - 1)
    } else if delta < 0 {
        selected.saturating_sub(1)
    } else {
        selected
    }
}

/// Vertical navigation keeps the same column where possible and clamps at
/// collection boundaries; there is no surprising jump to the opposite end.
pub fn vertical_step(selected: usize, count: usize, columns: usize, down: bool) -> usize {
    if count == 0 {
        return 0;
    }
    if down {
        if selected + columns < count {
            selected + columns
        } else if selected < (total_rows(count, columns) - 1) * columns {
            count - 1
        } else {
            selected
        }
    } else if selected >= columns {
        selected - columns
    } else {
        selected
    }
}
