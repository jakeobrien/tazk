//! Triage grid rendering (spec §10). Triage detail is drawn by its own issue.
//!
//! ```text
//! ╭ 2025-11-04 00:18 ───────────╮   top: creation time (or the file name)
//! │ Login page crashes when the │   up to two lines of raw text
//! │ session cookie is…          │
//! │                             │
//! │                             │
//! │                             │
//! ╰──── 20251104T001800-k3f9.md ╯   bottom: file name, dim
//! ```

use super::cell::{
    blit, draw_border, truncate_with_ellipsis, CELL_HEIGHT, CELL_WIDTH, CONTENT_WIDTH,
};
use super::theme;
use crate::storage::triage::TriageItem;
use crate::tui::app::App;
use crate::tui::model::triage::{cell_preview, created_label};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Frame;

pub const NO_MATCHES_MESSAGE: &str = "No matching triage items";

/// One triage item as a 32×7 cell.
#[derive(Debug, Clone)]
pub struct TriageCell<'a> {
    item: &'a TriageItem,
    selected: bool,
}

impl<'a> TriageCell<'a> {
    pub fn new(item: &'a TriageItem) -> Self {
        Self {
            item,
            selected: false,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn draw(&self, buf: &mut Buffer) {
        let edge = theme::triage_edge_style();
        let right = CELL_WIDTH - 1;
        let bottom = CELL_HEIGHT - 1;
        draw_border(buf, self.selected, edge);

        // Top border: the readable creation time, or the file name.
        let name = &self.item.name;
        let label = created_label(name).unwrap_or_else(|| name.clone());
        let label = truncate_with_ellipsis(&label, 27);
        let label_style = if self.selected {
            Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::new()
        };
        buf.set_stringn(
            1,
            0,
            format!(" {label} "),
            usize::from(right) - 1,
            label_style,
        );

        // Bottom border: " name " (dim), ending two columns before the corner.
        let name = format!(" {} ", truncate_with_ellipsis(name, 27));
        let name_w = name.chars().count() as u16;
        buf.set_string(
            right - 2 - name_w.min(right - 3),
            bottom,
            &name,
            theme::dim_style(),
        );

        // Body.
        for (i, line) in cell_preview(&self.item.body).iter().enumerate() {
            buf.set_stringn(2, 1 + i as u16, line, CONTENT_WIDTH, Style::new());
        }
    }
}

impl Widget for TriageCell<'_> {
    /// Draws at the top-left of `area`, clipped, never larger than 32×7.
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut cell = Buffer::empty(Rect::new(0, 0, CELL_WIDTH, CELL_HEIGHT));
        self.draw(&mut cell);
        blit(&cell, area, buf);
    }
}

/// Draw the triage grid into `area`: whole cells, `per_row` to a row, from the
/// scroll offset.
pub fn draw_triage_grid(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let grid = &app.triage_grid;
    if grid.names().is_empty() {
        draw_empty(frame, area);
        return;
    }
    let per_row = usize::from((area.width / CELL_WIDTH).max(1));
    let rows = usize::from((area.height / CELL_HEIGHT).max(1));
    let first = grid.row_offset() * per_row;
    let selected = grid.selected_index();
    for (slot, (i, name)) in grid
        .names()
        .iter()
        .enumerate()
        .skip(first)
        .take(per_row * rows)
        .enumerate()
    {
        let x = area.x + (slot % per_row) as u16 * CELL_WIDTH;
        let y = area.y + (slot / per_row) as u16 * CELL_HEIGHT;
        let width = CELL_WIDTH.min(area.right().saturating_sub(x));
        let height = CELL_HEIGHT.min(area.bottom().saturating_sub(y));
        if width == 0 || height < CELL_HEIGHT {
            break;
        }
        let Some(item) = app.snapshot.triage.iter().find(|t| &t.name == name) else {
            continue;
        };
        TriageCell::new(item)
            .selected(selected == Some(i))
            .render(Rect::new(x, y, width, height), frame.buffer_mut());
    }
}

fn draw_empty(frame: &mut Frame, area: Rect) {
    let [_, mid, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(NO_MATCHES_MESSAGE)
            .alignment(Alignment::Center)
            .style(theme::empty_message_style()),
        mid,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::{update, Action, Dir};
    use crate::tui::testing::*;
    use crate::tui::ModeArg;

    fn items() -> Vec<TriageItem> {
        vec![
            triage_item(
                "20251104T001800-k3f9.md",
                "Login page crashes when the session cookie is dropped on redirect after a timeout",
            ),
            triage_item("20251104T093000-a1b2.md", "Typo in\n\n  the\tREADME"),
            triage_item("notes.md", "# Hand dropped\n\nSome notes"),
            triage_item("20251105T120000-zz99.md", "Third standard item"),
        ]
    }

    fn app(width: u16, height: u16) -> App {
        let mut app = App::from_snapshot(
            ModeArg::Triage,
            snapshot_with([issue("tz-1", "x")], items()),
        );
        update(&mut app, Action::Resize(width, height));
        app
    }

    #[test]
    fn cell_with_a_standard_name() {
        let item = &items()[0];
        let out = render_styled(32, 7, |f| {
            f.render_widget(TriageCell::new(item), f.area());
        });
        insta::assert_snapshot!(out);
    }

    #[test]
    fn selected_cell_with_a_non_standard_name() {
        let item = &items()[2];
        let out = render_styled(32, 7, |f| {
            f.render_widget(TriageCell::new(item).selected(true), f.area());
        });
        insta::assert_snapshot!(out);
    }

    #[test]
    fn grid_wraps_left_to_right_then_top_to_bottom() {
        let mut app = app(100, 24); // 3 cells per row, 2 rows
        update(&mut app, Action::Move(Dir::Right));
        insta::assert_snapshot!(render_app(&mut app, 100, 24));
    }

    #[test]
    fn grid_scrolls_to_a_selection_below() {
        // 2 cells per row, 1 row of cells: moving down scrolls by a whole row.
        let mut app = app(70, 11);
        let before = render_app(&mut app, 70, 11);
        assert!(before.contains("2025-11-04 00:18"), "{before}");
        update(&mut app, Action::Move(Dir::Down));
        assert_eq!(
            app.selection.triage.as_deref(),
            Some("20251105T120000-zz99.md")
        );
        let text = render_app(&mut app, 70, 11);
        assert!(text.contains("2025-11-05 12:00"), "{text}");
        assert!(!text.contains("2025-11-04 00:18"), "{text}");
        insta::assert_snapshot!(text);
    }

    #[test]
    fn no_matching_items() {
        let mut app = app(100, 24);
        app.search.triage = "zzz".into();
        app.refresh_grid();
        let text = render_app(&mut app, 100, 24);
        assert!(text.contains(NO_MATCHES_MESSAGE), "{text}");
        insta::assert_snapshot!(text);
    }

    #[test]
    fn search_narrows_the_grid() {
        let mut app = app(100, 24);
        app.search.triage = "typo".into();
        app.refresh_grid();
        let text = render_app(&mut app, 100, 24);
        assert!(text.contains("2025-11-04 09:30"));
        assert!(!text.contains("notes.md"));
    }
}
