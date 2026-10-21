//! Volatile rewind history. No save files, UI objects or core handles live here.
use std::{collections::VecDeque, time::Duration};

pub const WINDOW: Duration = Duration::from_secs(30);
pub const INTERVAL: Duration = Duration::from_millis(500);
pub const MEMORY_LIMIT: usize = 128 * 1024 * 1024;

pub struct Point<P> {
    pub at: Duration,
    pub state: Vec<u8>,
    pub preview: P,
    cost: usize,
}

pub struct History<P> {
    points: VecDeque<Point<P>>,
    used: usize,
    budget: usize,
}

impl<P> Default for History<P> {
    fn default() -> Self {
        Self::new(MEMORY_LIMIT)
    }
}

impl<P> History<P> {
    pub fn new(budget: usize) -> Self {
        Self {
            points: VecDeque::new(),
            used: 0,
            budget,
        }
    }
    pub fn len(&self) -> usize {
        self.points.len()
    }
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
    pub fn get(&self, index: usize) -> Option<&Point<P>> {
        self.points.get(index)
    }
    pub fn iter(&self) -> impl Iterator<Item = &Point<P>> {
        self.points.iter()
    }
    pub fn clear(&mut self) {
        self.points.clear();
        self.used = 0;
    }

    /// Equal times replace a periodic snapshot with the exact paused moment.
    /// An oversized point leaves existing history untouched.
    pub fn push(&mut self, at: Duration, state: Vec<u8>, preview: P, preview_bytes: usize) -> bool {
        let Some(cost) = state.len().checked_add(preview_bytes) else {
            return false;
        };
        if cost > self.budget || state.is_empty() {
            return false;
        }
        while self.points.back().is_some_and(|point| point.at >= at) {
            if let Some(point) = self.points.pop_back() {
                self.used -= point.cost;
            }
        }
        while self.used > self.budget - cost
            || self
                .points
                .front()
                .is_some_and(|point| at.saturating_sub(point.at) > WINDOW)
        {
            if let Some(point) = self.points.pop_front() {
                self.used -= point.cost;
            }
        }
        self.used += cost;
        self.points.push_back(Point {
            at,
            state,
            preview,
            cost,
        });
        true
    }

    /// Discard a future branch only after the core accepted the selected state.
    pub fn truncate_after(&mut self, index: usize) {
        if index >= self.len() {
            return;
        }
        while self.len() > index + 1 {
            if let Some(point) = self.points.pop_back() {
                self.used -= point.cost;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_bounds_memory_and_time_and_keeps_matching_previews() {
        let mut history = History::new(12);
        for i in 0..8 {
            assert!(history.push(Duration::from_secs(i), vec![i as u8; 3], i, 1));
        }
        assert_eq!(history.used, 12);
        assert_eq!(
            history.iter().map(|p| p.preview).collect::<Vec<_>>(),
            [5, 6, 7]
        );
        assert!(!history.push(Duration::from_secs(8), vec![0; 13], 8, 0));
        assert_eq!(history.len(), 3);
        assert!(history.push(Duration::from_secs(40), vec![40; 3], 40, 1));
        assert_eq!(history.len(), 1);
        assert_eq!(history.get(0).unwrap().state, [40; 3]);
    }
    #[test]
    fn half_second_points_cover_at_most_thirty_seconds() {
        let mut history = History::new(1024);
        for i in 0..100 {
            history.push(INTERVAL * i, vec![1], (), 0);
        }
        assert_eq!(history.len(), 61);
        assert_eq!(
            history.get(60).unwrap().at - history.get(0).unwrap().at,
            WINDOW
        );
    }
    #[test]
    fn cancelled_browsing_keeps_future_and_committing_removes_it() {
        let mut history = History::new(1024);
        for i in 0..6 {
            history.push(Duration::from_secs(i), vec![i as u8], i, 4);
        }
        assert_eq!(history.get(2).unwrap().preview, 2);
        assert_eq!(history.len(), 6); // Looking never restores or truncates.
        history.truncate_after(2);
        assert_eq!(history.len(), 3);
        assert_eq!(history.used, 15);
        history.push(Duration::from_secs(3), vec![90], 90, 4);
        assert_eq!(history.get(3).unwrap().preview, 90);
        history.push(Duration::from_secs(3), vec![91], 91, 4);
        assert_eq!(history.len(), 4);
        assert_eq!(history.used, 20);
        history.clear();
        assert!(history.is_empty());
        assert_eq!(history.used, 0);
    }
}
