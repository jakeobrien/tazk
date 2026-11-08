//! Help overlay, generated from the keymap (spec §11).
//! Rendering only.
//!
//! The binding tables come from [`keymap::HELP_SECTIONS`], so there is no
//! hand-written key text to drift. When files were skipped (spec §3.2) the
//! overlay starts with the count, the first skipped path and its error.
//!
//! ```text
//! ┌ Help ─────────────────────────────┐
//! │ Global                            │
//! │   q, Ctrl-C  Quit                 │
//! │   ...                             │
//! └ ↑↓ scroll · ? / Esc close ────────┘
//! ```

use super::theme;
use crate::tui::app::App;
use crate::tui::keymap::{Binding, When, HELP_SECTIONS};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Footer in the bottom border.
pub const FOOTER: &str = " ↑↓ scroll · ? / Esc close ";
/// Preferred inner width of the popup.
const PREFERRED_WIDTH: u16 = 76;

/// Split `text` into lines of at most `width` characters, breaking at spaces
/// where possible and inside words that are too long.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut len = 0;
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        // Break words longer than a whole line.
        while word.len() > width {
            if len > 0 {
                lines.push(std::mem::take(&mut line));
                len = 0;
            }
            lines.push(word.drain(..width).collect());
        }
        let wlen = word.len();
        if wlen == 0 {
            continue;
        }
        if len > 0 && len + 1 + wlen > width {
            lines.push(std::mem::take(&mut line));
            len = 0;
        }
        if len > 0 {
            line.push(' ');
            len += 1;
        }
        line.extend(word);
        len += wlen;
    }
    if len > 0 {
        lines.push(line);
    }
    lines
}

/// Note appended to bindings that are not always active.
fn condition(when: When) -> &'static str {
    match when {
        When::Always => "",
        When::IssuesMode => " (issues mode)",
        When::SplitView => " (split view)",
    }
}

/// All help lines for a content width of `width` columns.
pub fn help_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let key_width = HELP_SECTIONS
        .iter()
        .flat_map(|(_, table)| table.iter())
        .map(|b| b.display.chars().count())
        .max()
        .unwrap_or(0);
    // "  " + keys + "  " + label
    let label_width = width.saturating_sub(key_width + 4).max(1);
    let mut lines: Vec<Line<'static>> = Vec::new();

    let skipped = &app.snapshot.skipped;
    if let Some(first) = skipped.first() {
        let count = skipped.len();
        let noun = if count == 1 { "file" } else { "files" };
        lines.push(Line::styled(
            format!("{} {count} {noun} skipped", super::bars::SKIPPED_SYMBOL),
            theme::heading_style(),
        ));
        lines.push(Line::styled("First:", theme::label_style()));
        for l in wrap(&first.path.display().to_string(), width.saturating_sub(2)) {
            lines.push(Line::raw(format!("  {l}")));
        }
        for l in wrap(&first.error, width.saturating_sub(2)) {
            lines.push(Line::styled(format!("  {l}"), theme::dim_style()));
        }
        lines.push(Line::raw(""));
    }

    for (i, (title, table)) in HELP_SECTIONS.iter().enumerate() {
        if i > 0 {
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(*title, theme::heading_style()));
        for b in table.iter() {
            lines.extend(binding_lines(b, key_width, label_width));
        }
    }
    lines
}

fn binding_lines(b: &Binding, key_width: usize, label_width: usize) -> Vec<Line<'static>> {
    let text = format!("{}{}", b.label, condition(b.when));
    wrap(&text, label_width)
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let keys = if i == 0 { b.display } else { "" };
            Line::from(vec![
                Span::styled(
                    format!("  {keys:<key_width$}  "),
                    Style::new().fg(theme::CYAN),
                ),
                Span::raw(l),
            ])
        })
        .collect()
}

/// The popup's rectangle for `content` lines: centered in `area`.
fn popup_area(area: Rect, content: usize) -> Rect {
    let width = (PREFERRED_WIDTH + 2).min(area.width);
    let height = (content.min(usize::from(u16::MAX)) as u16)
        .saturating_add(2)
        .min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// Draw the overlay centered over `area`. Records the page size and clamps
/// the scroll offset in `app`.
pub fn draw_help(frame: &mut Frame, area: Rect, app: &mut App) {
    let inner_width = usize::from((PREFERRED_WIDTH + 2).min(area.width).saturating_sub(4));
    let lines = help_lines(app, inner_width);
    let rect = popup_area(area, lines.len());
    if rect.width < 5 || rect.height < 3 {
        return;
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::pane_border_style(true))
        .title(" Help ")
        .title_bottom(Line::from(FOOTER).alignment(Alignment::Left));
    let inner = block.inner(rect);
    let view = usize::from(inner.height);
    app.help_view_height = view;
    let max_scroll = lines.len().saturating_sub(view);
    app.help_scroll = app.help_scroll.min(max_scroll);
    let scroll = app.help_scroll;

    frame.render_widget(Clear, rect);
    let more = max_scroll > 0;
    let block = if more {
        let position = format!(
            " {}-{} of {} ",
            scroll + 1,
            (scroll + view).min(lines.len()),
            lines.len()
        );
        block.title_bottom(Line::from(position).alignment(Alignment::Right))
    } else {
        block
    };
    frame.render_widget(block, rect);
    let shown: Vec<Line> = lines.into_iter().skip(scroll).take(view).collect();
    // One column of padding inside the border.
    let padded = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    frame.render_widget(Paragraph::new(shown), padded);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::SkippedFile;
    use crate::tui::app::{update, Action, Overlay};
    use crate::tui::data::Snapshot;
    use crate::tui::keymap;
    use crate::tui::testing::*;
    use crate::tui::ModeArg;

    fn app() -> App {
        let mut app = App::from_snapshot(
            ModeArg::Issues,
            snapshot_of([issue("t-1", "Login page").priority(1)]),
        );
        update(&mut app, Action::Resize(100, 50));
        update(&mut app, Action::OpenHelp);
        assert_eq!(app.overlay, Overlay::Help);
        app
    }

    fn with_skipped(mut app: App, n: usize) -> App {
        let skipped = (0..n)
            .map(|i| SkippedFile {
                path: format!("/work/.tazk/issues/bad{i}.md").into(),
                error: "invalid YAML frontmatter: mapping values are not allowed here".into(),
            })
            .collect();
        app.snapshot = Snapshot::new(app.snapshot.issues, app.snapshot.triage, skipped);
        app
    }

    #[test]
    fn help_overlay_without_skipped_files() {
        let mut app = app();
        insta::assert_snapshot!(render_app(&mut app, 100, 50));
    }

    #[test]
    fn help_overlay_with_skipped_files() {
        let mut app = with_skipped(app(), 3);
        insta::assert_snapshot!(render_app(&mut app, 100, 50));
    }

    #[test]
    fn no_skipped_section_without_skipped_files() {
        let text = render_app(&mut app(), 100, 50);
        assert!(!text.contains("skipped"));
    }

    #[test]
    fn skipped_section_shows_count_first_path_and_error() {
        let text = render_app(&mut with_skipped(app(), 3), 100, 50);
        assert!(text.contains("3 files skipped"), "{text}");
        assert!(text.contains("/work/.tazk/issues/bad0.md"));
        assert!(!text.contains("bad1.md"));
        assert!(text.contains("invalid YAML frontmatter"));
        let one = render_app(&mut with_skipped(app(), 1), 100, 50);
        assert!(one.contains("1 file skipped"));
    }

    /// Guards against drift: every binding's keys and label are listed.
    #[test]
    fn every_keymap_binding_appears_in_the_help() {
        let mut app = app();
        // Tall enough that nothing scrolls out of view.
        let text = render_app(&mut app, 100, 120);
        for (title, table) in HELP_SECTIONS {
            assert!(text.contains(title), "section {title}");
            for b in table.iter() {
                assert!(text.contains(b.display), "keys {:?}", b.display);
                assert!(text.contains(b.label), "label {:?}", b.label);
            }
        }
        // Every context's table is covered.
        for ctx in [
            keymap::Context::Grid,
            keymap::Context::Detail,
            keymap::Context::SearchInput,
            keymap::Context::Picker,
            keymap::Context::Help,
        ] {
            for b in keymap::bindings_for(ctx) {
                assert!(text.contains(b.label), "{ctx:?}: {:?}", b.label);
            }
        }
        for b in keymap::GLOBAL {
            assert!(text.contains(b.label));
        }
    }

    #[test]
    fn short_terminal_scrolls_and_clamps() {
        let mut app = app();
        update(&mut app, Action::Resize(100, 20));
        let top = render_app(&mut app, 100, 20);
        assert!(top.contains("Global"));
        assert!(!top.contains("Close help"));
        update(&mut app, Action::HelpBottom);
        let bottom = render_app(&mut app, 100, 20);
        assert!(bottom.contains("Close help"));
        assert!(!bottom.contains("Global"));
        let at_bottom = app.help_scroll;
        update(&mut app, Action::HelpLineDown);
        render_app(&mut app, 100, 20);
        assert_eq!(app.help_scroll, at_bottom, "clamped");
        update(&mut app, Action::HelpTop);
        assert_eq!(app.help_scroll, 0);
        update(&mut app, Action::HelpPageDown);
        render_app(&mut app, 100, 20);
        assert_eq!(app.help_scroll, app.help_view_height);
    }

    #[test]
    fn closing_and_reopening_resets_the_scroll() {
        let mut app = app();
        update(&mut app, Action::HelpBottom);
        render_app(&mut app, 100, 20);
        assert!(app.help_scroll > 0);
        update(&mut app, Action::CloseHelp);
        update(&mut app, Action::OpenHelp);
        assert_eq!(app.help_scroll, 0);
    }

    #[test]
    fn wrap_breaks_at_spaces_and_inside_long_words() {
        assert_eq!(wrap("aaa bbb ccc", 7), ["aaa bbb", "ccc"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("ab cdefghij", 4), ["ab", "cdef", "ghij"]);
        assert!(wrap("   ", 4).is_empty());
    }

    #[test]
    fn narrow_terminal_wraps_and_keeps_every_label_reachable() {
        let mut app = app();
        update(&mut app, Action::Resize(40, 40));
        let text = render_app(&mut app, 40, 100);
        assert!(text.contains("Close help"));
    }
}
