//! Split-pane vs full-screen layout decisions.

use ratatui::layout::Rect;

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

/// Minimum terminal width for the split view (spec §8).
pub const SPLIT_MIN_WIDTH: u16 = 160;

/// True if a terminal `width` columns wide uses the split view.
pub fn is_split(width: u16) -> bool {
    width >= SPLIT_MIN_WIDTH
}
