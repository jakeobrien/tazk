//! Rendering only.

pub mod bars;
pub mod cell;
pub mod detail;
pub mod grid;
pub mod help;
pub mod layout;
pub mod markdown;
pub mod picker;
pub mod theme;
pub mod triage;

use super::app::{App, Focus};
use super::ModeArg;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub const TOO_SMALL_MESSAGE: &str = "Terminal too small";

/// Draw the whole UI.
///
/// Takes `&mut App` because drawing the detail reports its layout (content
/// and viewport height) back to the detail state, which clamps the scroll.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if layout::too_small(area) {
        draw_too_small(frame, area);
        return;
    }
    let areas = layout::areas(area);
    bars::draw_mode_bar(frame, areas.mode_bar, app);
    bars::draw_filter_bar(frame, areas.filter_bar, app);
    bars::draw_hint_bar(frame, areas.hint_bar, app);
    // Where the issues grid is drawn, if at all (the picker is centered over it).
    let mut grid_area = None;
    if app.full_screen_detail {
        detail::draw_detail_pane(frame, areas.main, app, true);
    } else if app.is_split() {
        // Grid on the left half, detail pane on the right (spec §8).
        let (left, detail_area) = layout::split_main(areas.main);
        if app.mode == ModeArg::Issues {
            grid::draw_grid(frame, left, app);
            grid_area = Some(left);
        }
        let focused = app.focus == Focus::Detail;
        detail::draw_detail_pane(frame, detail_area, app, focused);
    } else if app.mode == ModeArg::Issues {
        grid::draw_grid(frame, areas.main, app);
        grid_area = Some(areas.main);
    }
    if let (Some(area), Some(picker)) = (grid_area, &app.picker) {
        picker::draw_picker(frame, area, picker);
    }
    // The triage grid is drawn by a later issue.
}

/// Only a centered message; nothing else is drawn.
fn draw_too_small(frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let [_, mid, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(TOO_SMALL_MESSAGE).alignment(Alignment::Center),
        mid,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::TuiArgs;
    use clap::Parser;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: TuiArgs,
    }

    fn render(w: u16, h: u16) -> Vec<String> {
        let mut app = App::new(&Cli::parse_from(["tz"]).args);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect()
    }

    #[test]
    fn too_small_by_width_shows_only_message() {
        let lines = render(31, 20);
        let non_blank: Vec<_> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty())
            .collect();
        assert_eq!(non_blank.len(), 1);
        let (y, line) = non_blank[0];
        assert_eq!(y, 10);
        // 31 cols: message (18) centered -> starts at col 6.
        assert_eq!(line.trim(), TOO_SMALL_MESSAGE);
        assert_eq!(line.find('T'), Some((31 - TOO_SMALL_MESSAGE.len()) / 2));
    }

    #[test]
    fn too_small_by_height_shows_only_message() {
        let lines = render(80, 10);
        let text: Vec<_> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(text.len(), 1);
        assert_eq!(text[0].trim(), TOO_SMALL_MESSAGE);
        assert_eq!(lines[5].trim(), TOO_SMALL_MESSAGE);
    }

    #[test]
    fn minimum_size_is_not_too_small() {
        let lines = render(32, 11);
        assert!(lines.iter().all(|l| !l.contains(TOO_SMALL_MESSAGE)));
    }

    // ---- Split vs full-screen detail snapshots (spec §8)

    use crate::tui::app::{update, Action, Focus};
    use crate::tui::testing as fixtures;
    use crate::tui::testing::{render_app, render_app_styled};

    fn layout_app(size: (u16, u16)) -> App {
        let snap = fixtures::snapshot_of([
            fixtures::epic("tz-1", "Auth rework").priority(0),
            fixtures::issue("tz-2", "Fix Safari login fails on redirect after timeout")
                .priority(1)
                .child_of("tz-1")
                .blocked_by("tz-3")
                .role(crate::types::Role::Agent)
                .effort(3)
                .description("The session cookie is dropped on redirect.")
                .acceptance("- works in Safari"),
            fixtures::issue("tz-3", "Set up the session store")
                .priority(2)
                .child_of("tz-1"),
            fixtures::issue("tz-4", "Unrelated chore").priority(3),
        ]);
        let mut app = App::from_snapshot(ModeArg::Issues, snap);
        update(&mut app, Action::Resize(size.0, size.1));
        // Select tz-2: the epic column is right of "No epic"; its header is
        // the first cell.
        update(&mut app, Action::Move(crate::tui::app::Dir::Right));
        update(&mut app, Action::Move(crate::tui::app::Dir::Down));
        assert_eq!(app.selection.issues.as_deref(), Some("tz-2"));
        app
    }

    #[test]
    fn split_view_200x50_grid_focused() {
        let mut app = layout_app((200, 50));
        assert_eq!(app.focus, Focus::Grid);
        insta::assert_snapshot!(render_app_styled(&mut app, 200, 50));
    }

    #[test]
    fn split_view_200x50_detail_focused() {
        let mut app = layout_app((200, 50));
        update(&mut app, Action::OpenDetail);
        update(&mut app, Action::NextLink);
        assert_eq!(app.focus, Focus::Detail);
        insta::assert_snapshot!(render_app_styled(&mut app, 200, 50));
    }

    #[test]
    fn full_screen_detail_120x40() {
        let mut app = layout_app((120, 40));
        update(&mut app, Action::OpenDetail);
        assert!(app.full_screen_detail);
        insta::assert_snapshot!(render_app(&mut app, 120, 40));
    }

    #[test]
    fn grid_only_120x40() {
        let mut app = layout_app((120, 40));
        insta::assert_snapshot!(render_app(&mut app, 120, 40));
    }

    #[test]
    fn detail_pane_border_is_accented_only_when_focused() {
        let mut app = layout_app((200, 50));
        // The pane's top-left corner is at column 100, below the two top bars.
        let corner = |app: &mut App| {
            let buf = fixtures::render_buffer(200, 50, |f| draw(f, &mut *app));
            buf[(100, 2)].clone()
        };
        let c = corner(&mut app);
        assert_eq!(c.symbol(), "┌");
        assert_eq!(c.fg, theme::pane_border_style(false).fg.unwrap());
        update(&mut app, Action::OpenDetail);
        let c = corner(&mut app);
        assert_eq!(c.fg, theme::pane_border_style(true).fg.unwrap());
    }
}
