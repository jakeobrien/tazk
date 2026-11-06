//! Issues grid state: columns, selection, navigation and scroll-follow
//! (spec §5.5, §5.6). Pure: no rendering, no terminal access.
//!
//! Rows are numbered per column: for an epic column the header is row 0 and
//! the cells follow; otherwise the first cell is row 0. The selection is kept
//! by issue ID (the header selects its epic), plus the column key and row as a
//! fallback for when the item disappears.

use super::columns::{build_columns, Column, ColumnKey};
use super::filter::{visible_issues, Filters};
use crate::tui::app::Dir;
use crate::tui::data::Snapshot;
use crate::tui::GroupByArg;
use std::collections::HashMap;

/// How much of the grid fits on screen, in whole columns and whole cells per
/// column. Both are at least 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub columns: usize,
    pub rows: usize,
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new(1, 1)
    }
}

impl Viewport {
    pub fn new(columns: usize, rows: usize) -> Self {
        Self {
            columns: columns.max(1),
            rows: rows.max(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Selected {
    id: String,
    column: ColumnKey,
    row: usize,
}

#[derive(Debug, Default)]
pub struct GridState {
    columns: Vec<Column>,
    selected: Option<Selected>,
    /// Index of the first visible column.
    col_offset: usize,
    /// First visible row, per column.
    row_offsets: HashMap<ColumnKey, usize>,
}

impl GridState {
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The selected issue ID (the epic's ID for a header).
    pub fn selected_id(&self) -> Option<&str> {
        self.selected.as_ref().map(|s| s.id.as_str())
    }

    /// Index of the selected column in [`columns`](Self::columns).
    pub fn selected_column(&self) -> Option<usize> {
        let key = &self.selected.as_ref()?.column;
        self.columns.iter().position(|c| &c.key == key)
    }

    /// Selected row within its column (header = row 0 in epic columns).
    pub fn selected_row(&self) -> Option<usize> {
        self.selected.as_ref().map(|s| s.row)
    }

    /// Index of the first visible column.
    pub fn col_offset(&self) -> usize {
        self.col_offset
    }

    /// First visible row of the column with this key.
    pub fn row_offset(&self, key: &ColumnKey) -> usize {
        self.row_offsets.get(key).copied().unwrap_or(0)
    }

    /// Recompute the columns from the inputs, then reconcile the selection
    /// (spec §5.6) and scroll offsets.
    pub fn rebuild(
        &mut self,
        snapshot: &Snapshot,
        filters: &Filters,
        query: &str,
        grouping: GroupByArg,
        viewport: Viewport,
    ) {
        let visible = visible_issues(snapshot, filters, query);
        self.columns = build_columns(snapshot, &visible, grouping);
        self.selected = self.reconcile();
        self.follow(viewport);
    }

    /// The selection after the columns changed.
    fn reconcile(&self) -> Option<Selected> {
        let first = || self.select_at(0, 0);
        let Some(old) = &self.selected else {
            return first();
        };
        // Still visible: stay on the item, preferring the same column.
        let same_column = self.columns.iter().find(|c| c.key == old.column);
        if let Some((col, row)) = same_column
            .and_then(|c| c.row_of(&old.id).map(|r| (c, r)))
            .or_else(|| {
                self.columns
                    .iter()
                    .find_map(|c| c.row_of(&old.id).map(|r| (c, r)))
            })
        {
            return Some(Selected {
                id: old.id.clone(),
                column: col.key.clone(),
                row,
            });
        }
        // Gone: same column at the same row (or its last), else the first cell.
        match self.columns.iter().position(|c| c.key == old.column) {
            Some(ci) => {
                let last = self.columns[ci].row_count() - 1;
                self.select_at(ci, old.row.min(last))
            }
            None => first(),
        }
    }

    fn select_at(&self, col: usize, row: usize) -> Option<Selected> {
        let column = self.columns.get(col)?;
        Some(Selected {
            id: column.id_at(row)?.to_string(),
            column: column.key.clone(),
            row,
        })
    }

    /// Move the selection and keep it visible. `↑/↓` stay in the column,
    /// `←/→` change column keeping the row as close as possible. Movement
    /// stops at the edges.
    pub fn move_dir(&mut self, dir: Dir, viewport: Viewport) {
        let (Some(ci), Some(row)) = (self.selected_column(), self.selected_row()) else {
            return;
        };
        let (ci, row) = match dir {
            Dir::Up => (ci, row.saturating_sub(1)),
            Dir::Down => (ci, row + 1),
            Dir::Left => match ci.checked_sub(1) {
                Some(c) => (c, row),
                None => (ci, row),
            },
            Dir::Right => (ci + 1, row),
        };
        self.select_clamped(ci, row, viewport);
    }

    /// Move one screen of cells up (`PgUp`).
    pub fn page_up(&mut self, viewport: Viewport) {
        if let (Some(ci), Some(row)) = (self.selected_column(), self.selected_row()) {
            self.select_clamped(ci, row.saturating_sub(viewport.rows), viewport);
        }
    }

    /// Move one screen of cells down (`PgDn`).
    pub fn page_down(&mut self, viewport: Viewport) {
        if let (Some(ci), Some(row)) = (self.selected_column(), self.selected_row()) {
            self.select_clamped(ci, row + viewport.rows, viewport);
        }
    }

    /// Top of the column (`Home`).
    pub fn top(&mut self, viewport: Viewport) {
        if let Some(ci) = self.selected_column() {
            self.select_clamped(ci, 0, viewport);
        }
    }

    /// Bottom of the column (`End`).
    pub fn bottom(&mut self, viewport: Viewport) {
        if let Some(ci) = self.selected_column() {
            self.select_clamped(ci, usize::MAX, viewport);
        }
    }

    /// Select `(col, row)` with both clamped into range.
    fn select_clamped(&mut self, col: usize, row: usize, viewport: Viewport) {
        if self.columns.is_empty() {
            return;
        }
        let col = col.min(self.columns.len() - 1);
        let row = row.min(self.columns[col].row_count() - 1);
        if let Some(sel) = self.select_at(col, row) {
            self.selected = Some(sel);
        }
        self.follow(viewport);
    }

    /// Clamp the scroll offsets and move them just enough that the selected
    /// cell is fully visible.
    pub fn follow(&mut self, viewport: Viewport) {
        let max_col = self.columns.len().saturating_sub(viewport.columns);
        self.col_offset = self.col_offset.min(max_col);
        let sel_col = self.selected_column();
        if let Some(ci) = sel_col {
            if ci < self.col_offset {
                self.col_offset = ci;
            } else if ci >= self.col_offset + viewport.columns {
                self.col_offset = ci + 1 - viewport.columns;
            }
        }

        let mut offsets = HashMap::new();
        for (i, column) in self.columns.iter().enumerate() {
            let mut off = self
                .row_offset(&column.key)
                .min(column.row_count().saturating_sub(viewport.rows));
            if sel_col == Some(i) {
                let row = self.selected_row().unwrap_or(0);
                if row < off {
                    off = row;
                } else if row >= off + viewport.rows {
                    off = row + 1 - viewport.rows;
                }
            }
            if off > 0 {
                offsets.insert(column.key.clone(), off);
            }
        }
        self.row_offsets = offsets;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::*;

    const VP: Viewport = Viewport {
        columns: 2,
        rows: 2,
    };

    fn state(snap: &Snapshot, g: GroupByArg, vp: Viewport) -> GridState {
        let mut s = GridState::default();
        s.rebuild(snap, &Filters::default(), "", g, vp);
        s
    }

    fn at(s: &GridState) -> (usize, usize) {
        (s.selected_column().unwrap(), s.selected_row().unwrap())
    }

    /// Epic grouping: "No epic" with n0 cells, then epics e1 (header + n1
    /// children) and e2 (header + n2 children). Priority orders children.
    fn sizes(n0: usize, n1: usize, n2: usize) -> Snapshot {
        let mut v = vec![epic("e1", "One").priority(1), epic("e2", "Two").priority(2)];
        for (n, parent) in [(n0, None), (n1, Some("e1")), (n2, Some("e2"))] {
            for i in 0..n {
                let id = format!("{}-{i}", parent.unwrap_or("n"));
                let mut b = issue(&id, "x").priority(i as i32);
                if let Some(p) = parent {
                    b = b.child_of(p);
                }
                v.push(b);
            }
        }
        snapshot_of(v)
    }

    /// Open issues only: one column of `n` cells `o-0..`, in priority order.
    fn flat(n: usize) -> Snapshot {
        statuses(&[n])
    }

    /// Status columns open, in_progress, blocked, deferred with the given
    /// numbers of cells (`o-i`, `i-i`, `b-i`, `d-i`); a 0 hides the column.
    fn statuses(counts: &[usize]) -> Snapshot {
        use crate::types::Status::*;
        let mut v = Vec::new();
        for (n, (status, tag)) in counts.iter().zip([
            (Open, "o"),
            (InProgress, "i"),
            (Blocked, "b"),
            (Deferred, "d"),
        ]) {
            for i in 0..*n {
                v.push(
                    issue(&format!("{tag}-{i}"), "x")
                        .status(status)
                        .priority(i as i32),
                );
            }
        }
        snapshot_of(v)
    }

    fn open_key() -> ColumnKey {
        ColumnKey::Status(crate::types::Status::Open)
    }

    #[test]
    fn starts_on_first_cell_of_first_column() {
        let s = state(&sizes(2, 1, 1), GroupByArg::Epic, VP);
        assert_eq!(s.selected_id(), Some("n-0"));
        assert_eq!(at(&s), (0, 0));
    }

    #[test]
    fn empty_grid_has_no_selection_and_ignores_movement() {
        let mut s = state(
            &snapshot_of(Vec::<IssueBuilder>::new()),
            GroupByArg::Epic,
            VP,
        );
        assert_eq!(s.selected_id(), None);
        s.move_dir(Dir::Down, VP);
        s.page_down(VP);
        s.bottom(VP);
        assert_eq!(s.selected_id(), None);
    }

    #[test]
    fn up_down_stop_at_the_edges() {
        let mut s = state(&flat(3), GroupByArg::Status, VP);
        s.move_dir(Dir::Up, VP);
        assert_eq!(at(&s), (0, 0));
        for _ in 0..5 {
            s.move_dir(Dir::Down, VP);
        }
        assert_eq!(at(&s), (0, 2));
        assert_eq!(s.selected_id(), Some("o-2"));
    }

    #[test]
    fn left_right_clamp_row_to_shorter_column() {
        // open: 5 cells (rows 0..=4); in_progress: 2 (rows 0..=1);
        // blocked: 1 (row 0).
        let vp = Viewport::new(3, 3);
        let mut s = state(&statuses(&[5, 2, 1]), GroupByArg::Status, vp);
        s.bottom(vp);
        assert_eq!(at(&s), (0, 4));
        s.move_dir(Dir::Right, vp);
        assert_eq!(at(&s), (1, 1));
        assert_eq!(s.selected_id(), Some("i-1"));
        s.move_dir(Dir::Right, vp);
        assert_eq!(at(&s), (2, 0));
        assert_eq!(s.selected_id(), Some("b-0"));
        // Right at the last column stays put.
        s.move_dir(Dir::Right, vp);
        assert_eq!(at(&s), (2, 0));
        // Going back to a taller column does not restore the old row.
        s.move_dir(Dir::Left, vp);
        s.move_dir(Dir::Left, vp);
        assert_eq!(at(&s), (0, 0));
        s.move_dir(Dir::Left, vp);
        assert_eq!(at(&s), (0, 0));
    }

    #[test]
    fn left_right_keep_row_when_the_target_is_long_enough() {
        let vp = Viewport::new(3, 3);
        let mut s = state(&statuses(&[4, 4]), GroupByArg::Status, vp);
        s.move_dir(Dir::Down, vp);
        s.move_dir(Dir::Down, vp);
        s.move_dir(Dir::Right, vp);
        assert_eq!(at(&s), (1, 2));
    }

    #[test]
    fn epic_header_is_row_zero_and_selects_the_epic() {
        let snap = snapshot_of([
            epic("e", "E"),
            issue("t1", "x").child_of("e").priority(1),
            issue("t2", "x").child_of("e").priority(2),
        ]);
        // Columns: No epic [e]; e [header, t1, t2].
        let mut s = state(&snap, GroupByArg::Epic, VP);
        s.move_dir(Dir::Right, VP);
        assert_eq!(at(&s), (1, 0));
        assert_eq!(s.selected_id(), Some("e"));
        s.move_dir(Dir::Down, VP);
        assert_eq!(s.selected_id(), Some("t1"));
        s.move_dir(Dir::Up, VP);
        assert_eq!(s.selected_id(), Some("e"));
        s.bottom(VP);
        assert_eq!(s.selected_id(), Some("t2"));
        assert_eq!(at(&s), (1, 2));
    }

    #[test]
    fn page_down_up_home_end_within_column() {
        let mut s = state(&flat(10), GroupByArg::Status, VP);
        s.page_down(VP);
        assert_eq!(at(&s), (0, 2));
        s.page_down(VP);
        assert_eq!(at(&s), (0, 4));
        s.bottom(VP);
        assert_eq!(at(&s), (0, 9));
        s.page_down(VP);
        assert_eq!(at(&s), (0, 9));
        s.page_up(VP);
        assert_eq!(at(&s), (0, 7));
        s.top(VP);
        assert_eq!(at(&s), (0, 0));
        s.page_up(VP);
        assert_eq!(at(&s), (0, 0));
    }

    #[test]
    fn page_size_comes_from_the_viewport() {
        let vp = Viewport::new(1, 4);
        let mut s = state(&flat(10), GroupByArg::Status, vp);
        s.page_down(vp);
        assert_eq!(at(&s), (0, 4));
    }

    #[test]
    fn vertical_scroll_keeps_selected_cell_visible() {
        let mut s = state(&flat(10), GroupByArg::Status, VP);
        let key = open_key();
        assert_eq!(s.row_offset(&key), 0);
        s.move_dir(Dir::Down, VP);
        assert_eq!(s.row_offset(&key), 0);
        s.move_dir(Dir::Down, VP); // row 2: rows 1..=2 visible
        assert_eq!(s.row_offset(&key), 1);
        s.bottom(VP);
        assert_eq!(s.row_offset(&key), 8);
        s.move_dir(Dir::Up, VP); // row 8 still visible
        assert_eq!(s.row_offset(&key), 8);
        s.move_dir(Dir::Up, VP); // row 7 above the window
        assert_eq!(s.row_offset(&key), 7);
        s.top(VP);
        assert_eq!(s.row_offset(&key), 0);
    }

    #[test]
    fn every_move_leaves_the_selection_fully_visible() {
        let vp = Viewport::new(2, 3);
        let mut s = state(&sizes(7, 9, 5), GroupByArg::Epic, vp);
        let dirs = [
            Dir::Down,
            Dir::Right,
            Dir::Down,
            Dir::Right,
            Dir::Left,
            Dir::Up,
        ];
        for step in 0..60 {
            match step % 7 {
                0 => s.page_down(vp),
                1 => s.page_up(vp),
                2 => s.bottom(vp),
                3 => s.top(vp),
                n => s.move_dir(dirs[n % dirs.len()], vp),
            }
            let (ci, row) = at(&s);
            assert!(
                (s.col_offset()..s.col_offset() + vp.columns).contains(&ci),
                "step {step}: column {ci} offset {}",
                s.col_offset()
            );
            let off = s.row_offset(&s.columns()[ci].key);
            assert!((off..off + vp.rows).contains(&row), "step {step}");
        }
    }

    #[test]
    fn horizontal_scroll_follows_selection() {
        let vp = Viewport::new(2, 3);
        let mut s = state(&sizes(1, 1, 1), GroupByArg::Epic, vp); // 3 columns
        assert_eq!(s.col_offset(), 0);
        s.move_dir(Dir::Right, vp);
        assert_eq!(s.col_offset(), 0);
        s.move_dir(Dir::Right, vp);
        assert_eq!(s.col_offset(), 1);
        s.move_dir(Dir::Left, vp);
        assert_eq!(s.col_offset(), 1);
        s.move_dir(Dir::Left, vp);
        assert_eq!(s.col_offset(), 0);
    }

    #[test]
    fn offsets_are_clamped_when_the_viewport_grows() {
        let vp = Viewport::new(1, 1);
        let mut s = state(&sizes(1, 1, 1), GroupByArg::Epic, vp);
        s.move_dir(Dir::Right, vp);
        s.move_dir(Dir::Right, vp);
        assert_eq!(s.col_offset(), 2);
        let big = Viewport::new(5, 5);
        s.follow(big);
        assert_eq!(s.col_offset(), 0);
    }

    #[test]
    fn scroll_position_of_other_columns_is_kept() {
        let vp = Viewport::new(2, 2);
        let mut s = state(&statuses(&[10, 5]), GroupByArg::Status, vp);
        s.bottom(vp);
        assert_eq!(s.row_offset(&open_key()), 8);
        s.move_dir(Dir::Right, vp);
        assert_eq!(s.row_offset(&open_key()), 8);
    }

    // ---- selection after the columns change (spec §5.6) ----

    fn rebuild(s: &mut GridState, snap: &Snapshot, f: &Filters, q: &str, g: GroupByArg) {
        s.rebuild(snap, f, q, g, VP);
    }

    fn select(s: &mut GridState, col: usize, row: usize) {
        s.select_clamped(col, row, VP);
    }

    #[test]
    fn selected_item_stays_selected_when_still_visible() {
        let snap = flat(3);
        let mut s = state(&snap, GroupByArg::Status, VP);
        select(&mut s, 0, 2);
        // A new snapshot with an extra, higher-priority issue pushes it down.
        let mut issues = snap.issues.clone();
        let mut extra = issue("z", "x").priority(-1).build();
        extra.updated_at = base_time();
        issues.push(extra);
        let snap2 = snapshot_of(issues);
        rebuild(&mut s, &snap2, &Filters::default(), "", GroupByArg::Status);
        assert_eq!(s.selected_id(), Some("o-2"));
        assert_eq!(at(&s), (0, 3));
    }

    #[test]
    fn vanished_item_selects_same_row_in_same_column() {
        let snap = snapshot_of([
            issue("a", "a").priority(0),
            issue("b", "b").priority(1),
            issue("c", "c").priority(2),
            issue("d", "d").priority(3),
        ]);
        let mut s = state(&snap, GroupByArg::Status, VP);
        select(&mut s, 0, 1);
        assert_eq!(s.selected_id(), Some("b"));
        // Searching removes "b" but keeps "a" and "c".
        let f = Filters::default();
        rebuild(&mut s, &snap, &f, "", GroupByArg::Status); // unchanged
        assert_eq!(s.selected_id(), Some("b"));
        let snap2 = snapshot_of([
            issue("a", "a").priority(0),
            issue("c", "c").priority(2),
            issue("d", "d").priority(3),
        ]);
        rebuild(&mut s, &snap2, &f, "", GroupByArg::Status);
        assert_eq!(s.selected_id(), Some("c"));
        assert_eq!(at(&s), (0, 1));
    }

    #[test]
    fn vanished_item_in_a_shorter_column_selects_its_last_cell() {
        let snap = snapshot_of([
            issue("a", "a").priority(0),
            issue("b", "b").priority(1),
            issue("c", "c").priority(2),
        ]);
        let mut s = state(&snap, GroupByArg::Status, VP);
        select(&mut s, 0, 2);
        assert_eq!(s.selected_id(), Some("c"));
        let snap2 = snapshot_of([issue("a", "a").priority(0)]);
        rebuild(&mut s, &snap2, &Filters::default(), "", GroupByArg::Status);
        assert_eq!(s.selected_id(), Some("a"));
        assert_eq!(at(&s), (0, 0));
    }

    #[test]
    fn column_gone_selects_first_cell_of_first_column() {
        let snap = snapshot_of([
            issue("a", "a"),
            issue("b", "b").status(crate::types::Status::Blocked),
            issue("c", "c").status(crate::types::Status::Deferred),
        ]);
        let mut s = state(&snap, GroupByArg::Status, VP);
        select(&mut s, 2, 0); // the deferred column
        assert_eq!(s.selected_id(), Some("c"));
        let snap2 = snapshot_of([
            issue("a", "a"),
            issue("b", "b").status(crate::types::Status::Blocked),
        ]);
        rebuild(&mut s, &snap2, &Filters::default(), "", GroupByArg::Status);
        assert_eq!(s.selected_id(), Some("a"));
        assert_eq!(at(&s), (0, 0));
    }

    #[test]
    fn regrouping_follows_the_item_when_it_stays_visible() {
        let snap = snapshot_of([
            issue("a", "a").role(crate::types::Role::Agent),
            issue("b", "b").role(crate::types::Role::Human),
        ]);
        let mut s = state(&snap, GroupByArg::Status, VP);
        rebuild(&mut s, &snap, &Filters::default(), "", GroupByArg::Role);
        select(&mut s, 1, 0);
        assert_eq!(s.selected_id(), Some("b"));
        rebuild(&mut s, &snap, &Filters::default(), "", GroupByArg::Status);
        assert_eq!(s.selected_id(), Some("b"));
        assert_eq!(
            s.columns()[s.selected_column().unwrap()].key,
            ColumnKey::Status(crate::types::Status::Open)
        );
    }

    #[test]
    fn regrouping_away_from_a_dimmed_header_selects_the_first_cell() {
        let snap = snapshot_of([
            issue("a", "a"),
            epic("e", "Done").closed(),
            issue("t", "t").child_of("e"),
        ]);
        let mut s = state(&snap, GroupByArg::Epic, VP);
        // Columns: No epic [a], e (dimmed header) [t].
        select(&mut s, 1, 0);
        assert_eq!(s.selected_id(), Some("e"));
        rebuild(&mut s, &snap, &Filters::default(), "", GroupByArg::Status);
        assert_eq!(s.selected_id(), Some("a"));
        assert_eq!(at(&s), (0, 0));
    }

    #[test]
    fn selecting_a_dimmed_epic_header_is_possible() {
        let snap = snapshot_of([epic("e", "Done").closed(), issue("t", "t").child_of("e")]);
        let s = state(&snap, GroupByArg::Epic, VP);
        assert_eq!(s.selected_id(), Some("e"));
        assert!(s.columns()[0].header.as_ref().unwrap().dimmed);
    }

    #[test]
    fn narrowing_to_zero_then_widening_selects_first_cell() {
        let snap = snapshot_of([
            issue("a", "alpha").priority(0),
            issue("b", "beta").priority(1),
        ]);
        let mut s = state(&snap, GroupByArg::Status, VP);
        select(&mut s, 0, 1);
        let f = Filters::default();
        rebuild(&mut s, &snap, &f, "zzz", GroupByArg::Status);
        assert_eq!(s.selected_id(), None);
        assert!(s.columns().is_empty());
        // Movement is a no-op with nothing visible.
        s.move_dir(Dir::Down, VP);
        assert_eq!(s.selected_id(), None);
        rebuild(&mut s, &snap, &f, "", GroupByArg::Status);
        assert_eq!(s.selected_id(), Some("a"));
        assert_eq!(at(&s), (0, 0));
    }

    #[test]
    fn narrowing_keeps_the_item_when_it_still_matches() {
        let snap = snapshot_of([
            issue("a", "alpha").priority(0),
            issue("b", "beta").priority(1),
        ]);
        let mut s = state(&snap, GroupByArg::Status, VP);
        select(&mut s, 0, 1);
        rebuild(
            &mut s,
            &snap,
            &Filters::default(),
            "beta",
            GroupByArg::Status,
        );
        assert_eq!(s.selected_id(), Some("b"));
        assert_eq!(at(&s), (0, 0));
    }

    #[test]
    fn nested_epic_selection_prefers_the_same_column() {
        let snap = snapshot_of([epic("e-1", "Outer"), epic("e-2", "Inner").child_of("e-1")]);
        // Columns: No epic [e-1], e-1 [header e-1, e-2], e-2 [header e-2].
        let mut s = state(&snap, GroupByArg::Epic, VP);
        select(&mut s, 2, 0);
        assert_eq!(s.selected_id(), Some("e-2"));
        rebuild(&mut s, &snap, &Filters::default(), "", GroupByArg::Epic);
        assert_eq!(at(&s), (2, 0));
        select(&mut s, 1, 1);
        assert_eq!(s.selected_id(), Some("e-2"));
        rebuild(&mut s, &snap, &Filters::default(), "", GroupByArg::Epic);
        assert_eq!(at(&s), (1, 1));
    }

    #[test]
    fn scroll_offsets_are_reclamped_after_the_grid_shrinks() {
        let vp = Viewport::new(1, 2);
        let mut s = state(&flat(10), GroupByArg::Status, vp);
        s.bottom(vp);
        assert_eq!(s.row_offset(&open_key()), 8);
        s.rebuild(&flat(3), &Filters::default(), "", GroupByArg::Status, vp);
        assert_eq!(s.selected_id(), Some("o-2"));
        assert_eq!(s.row_offset(&open_key()), 1);
    }
}
