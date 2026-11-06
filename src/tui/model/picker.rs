//! State of the shared multi-select filter picker popup (spec §6.1).
//! Pure logic (no rendering, no terminal access).
//!
//! The picker edits a *draft*: [`PickerState::toggle`] and friends never touch
//! the real filters. Applying hands the draft's checkboxes to
//! [`Filters::apply_picker`](super::filter::Filters::apply_picker); cancelling
//! just drops the state.

use super::filter::{Filters, PickerOption};
use crate::tui::app::{Dir, FilterKind};
use crate::tui::data::Snapshot;

/// An open picker: which filter, its rows (with draft checkboxes) and the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerState {
    pub kind: FilterKind,
    pub options: Vec<PickerOption>,
    pub cursor: usize,
}

impl PickerState {
    /// Open the picker for `kind`, seeded from the current filters.
    pub fn open(kind: FilterKind, filters: &Filters, snapshot: &Snapshot) -> Self {
        Self {
            kind,
            options: filters.picker_options(kind, snapshot),
            cursor: 0,
        }
    }

    /// The popup title: the property name.
    pub fn title(&self) -> &'static str {
        match self.kind {
            FilterKind::Role => "Role",
            FilterKind::Status => "Status",
            FilterKind::Type => "Type",
            FilterKind::Priority => "Priority",
            FilterKind::Effort => "Effort",
        }
    }

    /// Move the cursor one row (no wrap). Left/right do nothing.
    pub fn move_cursor(&mut self, dir: Dir) {
        match dir {
            Dir::Up => self.cursor = self.cursor.saturating_sub(1),
            Dir::Down => {
                if self.cursor + 1 < self.options.len() {
                    self.cursor += 1;
                }
            }
            Dir::Left | Dir::Right => {}
        }
    }

    pub fn toggle(&mut self) {
        if let Some(o) = self.options.get_mut(self.cursor) {
            o.selected = !o.selected;
        }
    }

    pub fn select_all(&mut self) {
        self.options.iter_mut().for_each(|o| o.selected = true);
    }

    pub fn select_none(&mut self) {
        self.options.iter_mut().for_each(|o| o.selected = false);
    }

    /// The draft checkboxes, in row order.
    pub fn selected(&self) -> Vec<bool> {
        self.options.iter().map(|o| o.selected).collect()
    }

    /// Whether the rows still match what the filters would list now (the
    /// priority list can change when the data reloads).
    pub fn is_stale(&self, filters: &Filters, snapshot: &Snapshot) -> bool {
        let now = filters.picker_options(self.kind, snapshot);
        now.len() != self.options.len()
            || now
                .iter()
                .zip(&self.options)
                .any(|(a, b)| a.label != b.label)
    }

    /// First visible row when `height` rows fit, keeping the cursor in view.
    pub fn scroll_offset(&self, height: usize) -> usize {
        if height == 0 {
            return 0;
        }
        let max = self.options.len().saturating_sub(height);
        (self.cursor + 1).saturating_sub(height).min(max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::*;

    fn picker(kind: FilterKind) -> PickerState {
        PickerState::open(kind, &Filters::default(), &snapshot())
    }

    #[test]
    fn opens_seeded_from_filters() {
        let p = picker(FilterKind::Status);
        assert_eq!(p.title(), "Status");
        assert_eq!(p.selected(), [true, true, true, true, false]);
        assert_eq!(p.cursor, 0);
    }

    #[test]
    fn cursor_clamps_and_toggle_flips_only_the_draft() {
        let mut p = picker(FilterKind::Status);
        p.move_cursor(Dir::Up);
        assert_eq!(p.cursor, 0);
        for _ in 0..10 {
            p.move_cursor(Dir::Down);
        }
        assert_eq!(p.cursor, 4);
        p.toggle();
        assert!(p.selected()[4]);
        p.toggle();
        assert!(!p.selected()[4]);
    }

    #[test]
    fn all_and_none() {
        let mut p = picker(FilterKind::Role);
        p.select_none();
        assert!(p.selected().iter().all(|s| !s));
        p.select_all();
        assert!(p.selected().iter().all(|s| *s));
    }

    #[test]
    fn scroll_offset_keeps_cursor_visible() {
        let mut p = picker(FilterKind::Priority); // 5 rows
        assert_eq!(p.scroll_offset(3), 0);
        p.cursor = 2;
        assert_eq!(p.scroll_offset(3), 0);
        p.cursor = 4;
        assert_eq!(p.scroll_offset(3), 2);
        assert_eq!(p.scroll_offset(10), 0);
    }

    #[test]
    fn stale_when_priorities_change() {
        let p = picker(FilterKind::Priority);
        let snap = snapshot_of([issue("a", "x").priority(9)]);
        assert!(p.is_stale(&Filters::default(), &snap));
        assert!(!p.is_stale(&Filters::default(), &snapshot()));
    }
}
