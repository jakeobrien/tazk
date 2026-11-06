//! Split-pane vs full-screen layout decisions.

use ratatui::layout::{Constraint, Layout, Rect};

/// Width of one grid column (spec §4).
pub const MIN_COLUMN_WIDTH: u16 = 32;
/// Mode bar, filter bar and hint bar.
pub const BAR_ROWS: u16 = 3;
/// Height of one issue cell.
pub const MIN_CELL_HEIGHT: u16 = 7;

pub const MIN_WIDTH: u16 = MIN_COLUMN_WIDTH;
pub const MIN_HEIGHT: u16 = BAR_ROWS + COLUMN_TITLE_HEIGHT + MIN_CELL_HEIGHT;

/// True if `area` can't fit one column plus the bars, a column title and one
/// cell.
pub fn too_small(area: Rect) -> bool {
    area.width < MIN_WIDTH || area.height < MIN_HEIGHT
}

/// The screen split into the three bars and the main area (spec §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Areas {
    pub mode_bar: Rect,
    pub filter_bar: Rect,
    /// Grid, grid + detail, or full-screen detail.
    pub main: Rect,
    pub hint_bar: Rect,
}

/// Split `area`: mode bar on top, then the filter bar, the main area, and the
/// hint bar at the bottom.
pub fn areas(area: Rect) -> Areas {
    let [mode_bar, filter_bar, main, hint_bar] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);
    Areas {
        mode_bar,
        filter_bar,
        main,
        hint_bar,
    }
}

/// Minimum terminal width for the split view (spec §8).
pub const SPLIT_MIN_WIDTH: u16 = 160;

/// True if a terminal `width` columns wide uses the split view.
pub fn is_split(width: u16) -> bool {
    width >= SPLIT_MIN_WIDTH
}

/// Which layout is active (derived from the terminal width, spec §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Grid only; detail opens full-screen.
    Narrow,
    /// Grid and detail pane side by side.
    Split,
}

impl LayoutMode {
    pub fn for_width(width: u16) -> Self {
        if is_split(width) {
            LayoutMode::Split
        } else {
            LayoutMode::Narrow
        }
    }
}

/// Split `main` into the grid (left) and detail pane (right) halves of the
/// split view (spec §8). The grid gets `width / 2` (what [`grid_capacity`]
/// counts columns in), the detail pane the rest.
pub fn split_main(main: Rect) -> (Rect, Rect) {
    let left = main.width / 2;
    (
        Rect::new(main.x, main.y, left, main.height),
        Rect::new(main.x + left, main.y, main.width - left, main.height),
    )
}

/// Height of a column's title bar.
pub const COLUMN_TITLE_HEIGHT: u16 = 1;

/// How many whole cells per row and rows of cells fit in the triage grid for a
/// terminal of `size`; at least 1 each. Unlike the issues grid it has no
/// column title bars.
pub fn triage_capacity(size: (u16, u16)) -> (usize, usize) {
    let (columns, _) = grid_capacity(size);
    let main = areas(Rect::new(0, 0, size.0, size.1)).main;
    (columns, (main.height / MIN_CELL_HEIGHT).max(1) as usize)
}

/// How many whole columns and cells fit in the grid for a terminal of
/// `size` `(columns, rows)`; at least 1 each. The grid gets the left half of
/// the main area in split view (spec §8).
pub fn grid_capacity(size: (u16, u16)) -> (usize, usize) {
    let main = areas(Rect::new(0, 0, size.0, size.1)).main;
    let width = if is_split(size.0) {
        main.width / 2
    } else {
        main.width
    };
    let columns = (width / MIN_COLUMN_WIDTH).max(1) as usize;
    let rows = (main.height.saturating_sub(COLUMN_TITLE_HEIGHT) / MIN_CELL_HEIGHT).max(1) as usize;
    (columns, rows)
}
