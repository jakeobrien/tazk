//! Split-pane vs full-screen layout decisions.

use ratatui::layout::{Constraint, Layout, Rect};

/// Width of one grid column (spec §4).
pub const MIN_COLUMN_WIDTH: u16 = 32;
/// Mode bar, filter bar and hint bar.
pub const BAR_ROWS: u16 = 3;
/// Height of one issue cell.
pub const MIN_CELL_HEIGHT: u16 = 7;

pub const MIN_WIDTH: u16 = MIN_COLUMN_WIDTH;
pub const MIN_HEIGHT: u16 = BAR_ROWS + MIN_CELL_HEIGHT;

/// True if `area` can't fit one column plus the bars and one cell.
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
