//! Triage grid state: a plain wrapped grid of items with selection, navigation
//! and vertical scroll-follow (spec §10, §5.6). Pure: no rendering, no
//! terminal access.
//!
//! Items are sorted by file name (oldest first) and laid out left to right,
//! then top to bottom, `per_row` cells to a row. The selection is kept by file
//! name, plus its index as a fallback for when the item disappears.

use super::grid::Viewport;
use super::search::visible_triage;
use crate::tui::app::Dir;
use crate::tui::data::Snapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Selected {
    name: String,
    index: usize,
}

#[derive(Debug, Default)]
pub struct TriageGridState {
    /// Visible item names, sorted ascending.
    names: Vec<String>,
    selected: Option<Selected>,
    /// First visible grid row.
    row_offset: usize,
    /// Cells per row (>= 1), from the last viewport.
    per_row: usize,
}

impl TriageGridState {
    /// Visible item names in grid order.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn selected_name(&self) -> Option<&str> {
        self.selected.as_ref().map(|s| s.name.as_str())
    }

    /// Index of the selected item in [`names`](Self::names).
    pub fn selected_index(&self) -> Option<usize> {
        self.selected.as_ref().map(|s| s.index)
    }

    /// Cells per row.
    pub fn per_row(&self) -> usize {
        self.per_row.max(1)
    }

    /// Index of the first visible grid row.
    pub fn row_offset(&self) -> usize {
        self.row_offset
    }

    /// Number of grid rows.
    pub fn row_count(&self) -> usize {
        self.names.len().div_ceil(self.per_row())
    }

    /// Recompute the visible items from the snapshot and the triage search
    /// `query`, and reconcile the selection (spec §5.6, adapted): the same
    /// item if still visible, else the item now at the same index (or the last
    /// one), else nothing. A first selection is the first item.
    pub fn rebuild(&mut self, snapshot: &Snapshot, query: &str, viewport: Viewport) {
        let mut names: Vec<String> = visible_triage(snapshot, query)
            .into_iter()
            .map(|t| t.name.clone())
            .collect();
        names.sort();
        self.names = names;
        self.per_row = viewport.columns;
        self.selected = match self.selected.take() {
            _ if self.names.is_empty() => None,
            None => self.select_at(0),
            Some(old) => match self.names.iter().position(|n| *n == old.name) {
                Some(i) => self.select_at(i),
                None => self.select_at(old.index.min(self.names.len() - 1)),
            },
        };
        self.follow(viewport);
    }

    fn select_at(&self, index: usize) -> Option<Selected> {
        Some(Selected {
            name: self.names.get(index)?.clone(),
            index,
        })
    }

    /// Move the selection and keep it visible. `←/→` step one item (wrapping
    /// onto the neighbouring row), `↑/↓` one row. Movement stops at the edges;
    /// `↓` from a row above a short last row lands on the last item.
    pub fn move_dir(&mut self, dir: Dir, viewport: Viewport) {
        let Some(i) = self.selected_index() else {
            return;
        };
        let per_row = viewport.columns;
        let target = match dir {
            Dir::Left => i.saturating_sub(1),
            Dir::Right => i + 1,
            Dir::Up => i.saturating_sub(per_row),
            Dir::Down => self.down_from(i, per_row, 1),
        };
        self.select_clamped(target, viewport);
    }

    /// `i` moved down `rows` grid rows, stopping on the last item if the
    /// target row is short; unchanged when already on the last row.
    fn down_from(&self, i: usize, per_row: usize, rows: usize) -> usize {
        let last_row = (self.names.len().saturating_sub(1)) / per_row;
        if i / per_row >= last_row {
            return i;
        }
        i + per_row * rows
    }

    /// One screen of rows up (`PgUp`), keeping the column.
    pub fn page_up(&mut self, viewport: Viewport) {
        if let Some(i) = self.selected_index() {
            self.select_clamped(i.saturating_sub(viewport.columns * viewport.rows), viewport);
        }
    }

    /// One screen of rows down (`PgDn`), keeping the column; stops on the
    /// last item.
    pub fn page_down(&mut self, viewport: Viewport) {
        if let Some(i) = self.selected_index() {
            let target = self.down_from(i, viewport.columns, viewport.rows);
            self.select_clamped(target, viewport);
        }
    }

    /// First item (`Home`).
    pub fn top(&mut self, viewport: Viewport) {
        self.select_clamped(0, viewport);
    }

    /// Last item (`End`).
    pub fn bottom(&mut self, viewport: Viewport) {
        self.select_clamped(usize::MAX, viewport);
    }

    fn select_clamped(&mut self, index: usize, viewport: Viewport) {
        if self.names.is_empty() {
            return;
        }
        self.selected = self.select_at(index.min(self.names.len() - 1));
        self.follow(viewport);
    }

    /// Clamp the scroll offset and move it just enough that the selected
    /// row is fully visible.
    pub fn follow(&mut self, viewport: Viewport) {
        self.per_row = viewport.columns;
        let max = self.row_count().saturating_sub(viewport.rows);
        let mut off = self.row_offset.min(max);
        if let Some(i) = self.selected_index() {
            let row = i / self.per_row();
            if row < off {
                off = row;
            } else if row >= off + viewport.rows {
                off = row + 1 - viewport.rows;
            }
        }
        self.row_offset = off;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::*;

    fn snap(n: usize) -> Snapshot {
        // Deliberately unsorted: the grid sorts by name.
        let mut items: Vec<_> = (0..n)
            .map(|i| triage_item(&format!("n{i:02}.md"), &format!("item {i}")))
            .collect();
        items.reverse();
        snapshot_with(Vec::<IssueBuilder>::new(), items)
    }

    fn vp(columns: usize, rows: usize) -> Viewport {
        Viewport::new(columns, rows)
    }

    fn state(snap: &Snapshot, vp: Viewport) -> TriageGridState {
        let mut s = TriageGridState::default();
        s.rebuild(snap, "", vp);
        s
    }

    fn sel(s: &TriageGridState) -> usize {
        s.selected_index().unwrap()
    }

    #[test]
    fn sorted_by_name_and_first_item_selected() {
        let s = state(&snap(4), vp(2, 2));
        assert_eq!(s.names(), ["n00.md", "n01.md", "n02.md", "n03.md"]);
        assert_eq!(s.selected_name(), Some("n00.md"));
    }

    #[test]
    fn one_cell_per_row_navigates_like_a_column() {
        let mut s = state(&snap(4), vp(1, 2));
        s.move_dir(Dir::Down, vp(1, 2));
        assert_eq!(sel(&s), 1);
        // Left/right walk the same sequence.
        s.move_dir(Dir::Right, vp(1, 2));
        assert_eq!(sel(&s), 2);
        s.move_dir(Dir::Left, vp(1, 2));
        assert_eq!(sel(&s), 1);
        s.move_dir(Dir::Up, vp(1, 2));
        s.move_dir(Dir::Up, vp(1, 2));
        assert_eq!(sel(&s), 0);
        for _ in 0..9 {
            s.move_dir(Dir::Down, vp(1, 2));
        }
        assert_eq!(sel(&s), 3);
    }

    #[test]
    fn three_per_row_wraps_left_right_and_moves_rows_up_down() {
        let v = vp(3, 2);
        let mut s = state(&snap(8), v); // rows: 0 1 2 / 3 4 5 / 6 7
        s.move_dir(Dir::Left, v);
        assert_eq!(sel(&s), 0, "stops at the start");
        for _ in 0..3 {
            s.move_dir(Dir::Right, v);
        }
        assert_eq!(sel(&s), 3, "→ wraps onto the next row");
        s.move_dir(Dir::Left, v);
        assert_eq!(sel(&s), 2, "← wraps back");
        s.move_dir(Dir::Down, v);
        assert_eq!(sel(&s), 5);
        s.move_dir(Dir::Down, v);
        assert_eq!(sel(&s), 7, "short last row: clamp to the last item");
        s.move_dir(Dir::Down, v);
        assert_eq!(sel(&s), 7, "already on the last row");
        s.move_dir(Dir::Right, v);
        assert_eq!(sel(&s), 7);
        s.move_dir(Dir::Up, v);
        assert_eq!(sel(&s), 4);
        s.move_dir(Dir::Up, v);
        assert_eq!(sel(&s), 1);
        s.move_dir(Dir::Up, v);
        assert_eq!(sel(&s), 0, "stays in range at the top row");
    }

    #[test]
    fn five_per_row() {
        let v = vp(5, 2);
        let mut s = state(&snap(12), v); // 0-4 / 5-9 / 10 11
        s.move_dir(Dir::Down, v);
        s.move_dir(Dir::Right, v);
        assert_eq!(sel(&s), 6);
        s.move_dir(Dir::Down, v);
        assert_eq!(sel(&s), 11);
        s.move_dir(Dir::Up, v);
        assert_eq!(sel(&s), 6);
        s.move_dir(Dir::Up, v);
        assert_eq!(sel(&s), 1);
        s.move_dir(Dir::Up, v);
        assert_eq!(sel(&s), 0);
    }

    #[test]
    fn home_end_and_paging() {
        let v = vp(3, 2);
        let mut s = state(&snap(20), v);
        s.move_dir(Dir::Right, v);
        s.page_down(v);
        assert_eq!(sel(&s), 7, "two rows down, same column");
        s.page_down(v);
        s.page_down(v);
        s.page_down(v);
        assert_eq!(sel(&s), 19, "stops on the last item");
        s.page_up(v);
        assert_eq!(sel(&s), 13);
        s.top(v);
        assert_eq!(sel(&s), 0);
        s.page_up(v);
        assert_eq!(sel(&s), 0);
        s.bottom(v);
        assert_eq!(sel(&s), 19);
    }

    #[test]
    fn vertical_scroll_follows_the_selection() {
        let v = vp(3, 2);
        let mut s = state(&snap(20), v); // 7 rows
        assert_eq!(s.row_count(), 7);
        assert_eq!(s.row_offset(), 0);
        let visible = |s: &TriageGridState| {
            let row = sel(s) / 3;
            (s.row_offset()..s.row_offset() + 2).contains(&row)
        };
        for _ in 0..6 {
            s.move_dir(Dir::Down, v);
            assert!(visible(&s));
        }
        assert_eq!(s.row_offset(), 5);
        s.bottom(v);
        assert_eq!(s.row_offset(), 5);
        for _ in 0..6 {
            s.move_dir(Dir::Up, v);
            assert!(visible(&s));
        }
        assert_eq!(s.row_offset(), 0);
        s.move_dir(Dir::Down, v);
        s.move_dir(Dir::Down, v);
        assert_eq!(s.row_offset(), 1, "scrolls by the minimum");
    }

    #[test]
    fn resize_keeps_the_selection_visible() {
        let mut s = state(&snap(20), vp(3, 3));
        s.bottom(vp(3, 3));
        s.follow(vp(5, 1));
        assert_eq!(s.per_row(), 5);
        assert_eq!(s.row_offset(), 3);
        s.follow(vp(1, 2));
        assert_eq!(s.row_offset(), 18);
    }

    #[test]
    fn selection_stays_on_the_same_item_when_the_list_changes() {
        let v = vp(2, 2);
        let mut s = state(&snap(4), v);
        s.move_dir(Dir::Right, v);
        assert_eq!(s.selected_name(), Some("n01.md"));
        let mut items: Vec<_> = (0..4)
            .map(|i| triage_item(&format!("n{i:02}.md"), "x"))
            .collect();
        items.insert(0, triage_item("n--.md", "new"));
        s.rebuild(&snapshot_with(Vec::<IssueBuilder>::new(), items), "", v);
        assert_eq!(s.selected_name(), Some("n01.md"));
        assert_eq!(sel(&s), 2);
    }

    #[test]
    fn search_narrows_and_selection_falls_back_to_the_same_index_or_last() {
        let v = vp(2, 2);
        let snapshot = snapshot_with(
            Vec::<IssueBuilder>::new(),
            vec![
                triage_item("a.md", "apple"),
                triage_item("b.md", "banana"),
                triage_item("c.md", "cherry pie"),
                triage_item("d.md", "date pie"),
                triage_item("e.md", "elder"),
            ],
        );
        let mut s = state(&snapshot, v);
        s.move_dir(Dir::Right, v);
        s.move_dir(Dir::Right, v);
        assert_eq!(s.selected_name(), Some("c.md"));
        // Still matches: stays on c.
        s.rebuild(&snapshot, "pie", v);
        assert_eq!(s.names(), ["c.md", "d.md"]);
        assert_eq!(s.selected_name(), Some("c.md"));
        // b was selected at index 1; it vanishes, index 1 is now "d".
        let mut s = state(&snapshot, v);
        s.move_dir(Dir::Right, v);
        s.rebuild(&snapshot, "pie", v);
        assert_eq!(s.selected_name(), Some("d.md"));
        // e (index 4) vanishes: the last item is selected.
        let mut s = state(&snapshot, v);
        s.bottom(v);
        s.rebuild(&snapshot, "pie", v);
        assert_eq!(s.selected_name(), Some("d.md"));
        // Nothing matches: no selection; widening reselects the first item.
        s.rebuild(&snapshot, "zzz", v);
        assert!(s.names().is_empty());
        assert_eq!(s.selected_name(), None);
        s.move_dir(Dir::Down, v);
        s.rebuild(&snapshot, "", v);
        assert_eq!(s.selected_name(), Some("a.md"));
    }

    #[test]
    fn search_is_case_insensitive_over_the_full_text() {
        let snapshot = snapshot_with(
            Vec::<IssueBuilder>::new(),
            vec![
                triage_item("a.md", "Login BUG"),
                triage_item("b.md", "other"),
            ],
        );
        let mut s = TriageGridState::default();
        s.rebuild(&snapshot, "bug", vp(2, 2));
        assert_eq!(s.names(), ["a.md"]);
    }

    #[test]
    fn empty_snapshot_has_nothing_selected() {
        let mut s = state(&snapshot_of(Vec::<IssueBuilder>::new()), vp(2, 2));
        assert_eq!(s.selected_name(), None);
        s.move_dir(Dir::Right, vp(2, 2));
        s.page_down(vp(2, 2));
        s.bottom(vp(2, 2));
        assert_eq!(s.selected_name(), None);
        assert_eq!(s.row_count(), 0);
    }
}
