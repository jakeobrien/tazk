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

use super::app::App;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub const TOO_SMALL_MESSAGE: &str = "Terminal too small";

/// Draw the whole UI.
pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if layout::too_small(area) {
        draw_too_small(frame, area);
        return;
    }
    let areas = layout::areas(area);
    bars::draw_mode_bar(frame, areas.mode_bar, app);
    bars::draw_filter_bar(frame, areas.filter_bar, app);
    bars::draw_hint_bar(frame, areas.hint_bar, app);
    // The main area is drawn by the later grid/detail issues.
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
        let app = App::new(&Cli::parse_from(["tz"]).args);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, &app)).unwrap();
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
        let lines = render(80, 9);
        let text: Vec<_> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(text.len(), 1);
        assert_eq!(text[0].trim(), TOO_SMALL_MESSAGE);
        assert_eq!(lines[4].trim(), TOO_SMALL_MESSAGE);
    }

    #[test]
    fn minimum_size_is_not_too_small() {
        let lines = render(32, 10);
        assert!(lines.iter().all(|l| !l.contains(TOO_SMALL_MESSAGE)));
    }
}
