//! Filter picker popup (spec §6.1).
//! Rendering only.
//!
//! ```text
//! ┌ Status ──────────┐
//! │ [x] open         │
//! │ [ ] closed       │
//! └ space · a/n · ⏎ ─┘
//! ```

use super::theme;
use crate::tui::model::picker::PickerState;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Footer in the bottom border.
pub const FOOTER: &str = " space · a/n · ⏎ ";
/// Minimum inner width, so the popup matches the spec mock-up.
const MIN_INNER_WIDTH: u16 = 18;

/// The popup's rectangle: centered in `area`, as small as its content allows.
pub fn popup_area(area: Rect, picker: &PickerState) -> Rect {
    let widest = picker
        .options
        .iter()
        .map(|o| o.label.chars().count())
        .max()
        .unwrap_or(0) as u16;
    let title = picker.title().chars().count() as u16 + 2;
    let footer = FOOTER.chars().count() as u16 + 1;
    // " [x] " + label + " "
    let inner = (widest + 6).max(MIN_INNER_WIDTH).max(title).max(footer);
    let width = (inner + 2).min(area.width);
    let height = (picker.options.len() as u16 + 2).min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// Draw the popup centered over `area` (the grid).
pub fn draw_picker(frame: &mut Frame, area: Rect, picker: &PickerState) {
    let rect = popup_area(area, picker);
    if rect.width < 3 || rect.height < 3 {
        return;
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::pane_border_style(true))
        .title(format!(" {} ", picker.title()))
        .title_bottom(Line::from(FOOTER).alignment(Alignment::Left));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);

    let height = usize::from(inner.height);
    let offset = picker.scroll_offset(height);
    let lines: Vec<Line> = picker
        .options
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, o)| {
            let mark = if o.selected { "[x]" } else { "[ ]" };
            let line = Line::from(vec![Span::raw(format!(" {mark} {}", o.label))]);
            if i == picker.cursor {
                line.style(theme::highlight_style())
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::{update, Action, App, FilterKind};
    use crate::tui::testing::*;
    use crate::tui::ModeArg;

    fn app() -> App {
        let snap = snapshot_of([
            epic("e-1", "Auth rework").priority(1),
            issue("t-1", "Login page").priority(0).child_of("e-1"),
            issue("t-2", "Fix redirect").priority(1),
            issue("t-3", "Done thing").closed(),
        ]);
        let mut app = App::from_snapshot(ModeArg::Issues, snap);
        update(&mut app, Action::Resize(100, 30));
        app
    }

    #[test]
    fn status_picker_over_the_grid() {
        let mut app = app();
        update(&mut app, Action::OpenPicker(FilterKind::Status));
        insta::assert_snapshot!(render_app(&mut app, 100, 30));
    }

    #[test]
    fn status_picker_popup_matches_the_spec_layout() {
        let mut app = app();
        update(&mut app, Action::OpenPicker(FilterKind::Status));
        let text = render_app(&mut app, 100, 30);
        let popup: Vec<String> = text
            .lines()
            .filter_map(|l| {
                let chars: Vec<char> = l.chars().collect();
                // The popup is 20 columns wide, centered in the 100-column grid.
                let s: String = chars.get(40..60)?.iter().collect();
                s.starts_with(['┌', '│', '└']).then_some(s)
            })
            .collect();
        assert_eq!(
            popup,
            [
                "┌ Status ──────────┐",
                "│ [x] open         │",
                "│ [x] in_progress  │",
                "│ [x] blocked      │",
                "│ [x] deferred     │",
                "│ [ ] closed       │",
                "└ space · a/n · ⏎ ─┘",
            ]
        );
    }

    #[test]
    fn cursor_row_is_highlighted() {
        let mut app = app();
        update(&mut app, Action::OpenPicker(FilterKind::Status));
        update(&mut app, Action::PickerMove(crate::tui::app::Dir::Down));
        let out = render_app_styled(&mut app, 100, 30);
        assert!(
            out.lines()
                .any(|l| l.contains("mods=BOLD") && l.contains("[x] in_progress")),
            "{out}"
        );
    }

    #[test]
    fn many_rows_scroll_to_keep_the_cursor_visible() {
        let mut p = PickerState::open(
            FilterKind::Priority,
            &crate::tui::model::filter::Filters::default(),
            &snapshot_of((0..12).map(|i| issue(&format!("i{i}"), "x").priority(i))),
        );
        p.cursor = 11;
        let text = render(60, 8, |f| draw_picker(f, f.area(), &p));
        assert!(text.contains("P11"), "{text}");
        assert!(!text.contains("P0 "), "{text}");
    }
}
