//! Mode bar, filter bar (with the search input) and hint bar (spec §4, §7).
//!
//! Each bar is built as ratatui [`Line`]s by a pure function (`*_line`) and
//! then drawn; the pure parts are what the unit tests look at.

use super::theme;
use crate::tui::app::App;
use crate::tui::app::FilterKind;
use crate::tui::keymap;
use crate::tui::model::filter::Filters;
use crate::tui::{GroupByArg, ModeArg};
use clap::ValueEnum;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Drawn after the query while typing (one cell wide).
pub const CURSOR: &str = "█";
const SKIPPED_SYMBOL: &str = "⚠";

fn dim() -> Style {
    Style::new().fg(theme::DIM)
}

/// The grouping's name as accepted by `--group-by`.
pub fn grouping_name(g: GroupByArg) -> String {
    g.to_possible_value()
        .map(|v| v.get_name().to_string())
        .unwrap_or_default()
}

/// Draw `left` and `right` on one row: `right` flush to the edge, `left`
/// clipped so it never runs into it.
fn draw_split(frame: &mut Frame, area: Rect, left: Line<'static>, right: Line<'static>) {
    let right_w = u16::try_from(right.width())
        .unwrap_or(u16::MAX)
        .min(area.width);
    let [l, r] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(right_w)]).areas(area);
    // One space between them when there is room.
    let l = Rect {
        width: l.width.saturating_sub(1),
        ..l
    };
    frame.render_widget(Paragraph::new(left), l);
    frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), r);
}

// --- Mode bar ----------------------------------------------------------------

/// `Issues (N) │ Triage (M)`: N and M are the totals loaded; the current mode
/// is highlighted.
pub fn mode_line(app: &App) -> Line<'static> {
    let current = Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD);
    let tab = |label: &str, count: usize, active: bool| {
        let style = if active { current } else { dim() };
        Span::styled(format!(" {label} ({count}) "), style)
    };
    Line::from(vec![
        tab(
            "Issues",
            app.snapshot.issues.len(),
            app.mode == ModeArg::Issues,
        ),
        Span::styled("│", dim()),
        tab(
            "Triage",
            app.snapshot.triage.len(),
            app.mode == ModeArg::Triage,
        ),
    ])
}

/// `group: epic` (issues mode only).
pub fn grouping_line(app: &App) -> Line<'static> {
    if app.mode != ModeArg::Issues {
        return Line::default();
    }
    Line::from(vec![
        Span::styled("group: ", dim()),
        Span::raw(grouping_name(app.grouping)),
        Span::raw(" "),
    ])
}

pub fn draw_mode_bar(frame: &mut Frame, area: Rect, app: &App) {
    draw_split(frame, area, mode_line(app), grouping_line(app));
}

// --- Filter bar ----------------------------------------------------------------

/// The filter bar label and kind of each filter, in display order.
const FILTERS: [(&str, FilterKind); 5] = [
    ("role", FilterKind::Role),
    ("status", FilterKind::Status),
    ("type", FilterKind::Type),
    ("pri", FilterKind::Priority),
    ("effort", FilterKind::Effort),
];

/// `role:all  status:!closed  type:all  pri:all  effort:all`.
fn filter_spans(filters: &Filters, app: &App) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, (name, kind)) in FILTERS.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        let value = filters.summary(*kind, &app.snapshot);
        let style = if value == "all" {
            dim()
        } else {
            Style::new().add_modifier(Modifier::BOLD)
        };
        spans.push(Span::styled(format!("{name}:"), dim()));
        spans.push(Span::styled(value, style));
    }
    spans
}

/// The search part: `/query█` while typing, `/query` when a query is kept,
/// nothing otherwise.
fn search_spans(app: &App) -> Vec<Span<'static>> {
    let query = app.query();
    let accent = Style::new().fg(theme::CYAN);
    if app.search_active {
        vec![
            Span::styled(format!("/{query}"), accent),
            Span::styled(CURSOR, accent),
        ]
    } else if !query.is_empty() {
        vec![Span::styled(format!("/{query}"), accent)]
    } else {
        Vec::new()
    }
}

/// Left side of the filter bar: filter summaries (issues mode) and the search.
pub fn filter_line(app: &App) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    if app.mode == ModeArg::Issues {
        spans.extend(filter_spans(&app.filters, app));
    }
    let search = search_spans(app);
    if !search.is_empty() {
        if spans.len() > 1 {
            spans.push(Span::raw("  "));
        }
        spans.extend(search);
    }
    Line::from(spans)
}

/// Right side of the filter bar: `X of Y` and `⚠ N skipped` if any.
pub fn count_line(app: &App) -> Line<'static> {
    let (shown, total) = app.counts();
    let mut spans = vec![Span::raw(format!("{shown} of {total}"))];
    let skipped = app.snapshot.skipped.len();
    if skipped > 0 {
        spans.push(Span::styled(
            format!("  {SKIPPED_SYMBOL} {skipped} skipped"),
            Style::new().fg(theme::ORANGE),
        ));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

pub fn draw_filter_bar(frame: &mut Frame, area: Rect, app: &App) {
    draw_split(frame, area, filter_line(app), count_line(app));
}

// --- Hint bar -----------------------------------------------------------------

/// The hint bar: the transient status message if there is one, otherwise the
/// key hints for the current context (see [`keymap::hints`]). Hints that don't
/// fit in `width` are dropped from the end.
pub fn hint_line(app: &App, width: u16) -> Line<'static> {
    if let Some(message) = &app.status {
        return Line::from(Span::styled(
            format!(" {message}"),
            Style::new().fg(theme::ORANGE),
        ));
    }
    let mut spans = Vec::new();
    let mut used = 0usize;
    for hint in keymap::hints(app) {
        let text_w = 1 + hint.keys.chars().count() + 1 + hint.label.chars().count() + 1;
        if used + text_w > usize::from(width) {
            break;
        }
        used += text_w;
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            hint.keys,
            Style::new().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(format!(" {} ", hint.label), dim()));
    }
    Line::from(spans)
}

pub fn draw_hint_bar(frame: &mut Frame, area: Rect, app: &App) {
    frame.render_widget(Paragraph::new(hint_line(app, area.width)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::SkippedFile;
    use crate::tui::app::{update, Action, Overlay};
    use crate::tui::data::Snapshot;
    use crate::tui::keymap::Context;
    use crate::tui::model::filter::FilterSet;
    use crate::tui::testing::*;
    use crate::types::{IssueType, Status};

    /// 4 issues (one closed, one epic) + 3 triage items.
    fn fixture(mode: ModeArg) -> App {
        let snap = snapshot_with(
            [
                issue("tz-1", "Login page").description("uses oauth"),
                issue("tz-2", "Logout"),
                epic("tz-3", "Auth rework"),
                issue("tz-4", "Old thing").closed(),
            ],
            vec![
                triage_item("a.md", "apples"),
                triage_item("b.md", "bananas"),
                triage_item("c.md", "Apple pie"),
            ],
        );
        let mut app = App::from_snapshot(mode, snap);
        update(&mut app, Action::Resize(100, 10));
        app
    }

    fn with_skipped(mut app: App, n: usize) -> App {
        let skipped = (0..n)
            .map(|i| SkippedFile {
                path: format!("bad{i}.md").into(),
                error: "oops".into(),
            })
            .collect();
        app.snapshot = Snapshot::new(app.snapshot.issues, app.snapshot.triage, skipped);
        app
    }

    /// The three bar rows only (the main area is empty in these tests).
    fn bars(app: &App, width: u16) -> String {
        let text = render_app(app, width, 10);
        let lines: Vec<&str> = text.lines().collect();
        format!("{}\n{}\n{}", lines[0], lines[1], lines[9])
    }

    fn send_key(app: &mut App, code: crossterm::event::KeyCode) {
        let k = crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
        crate::tui::terminal::handle_event(app, crossterm::event::Event::Key(k));
    }

    fn type_str(app: &mut App, s: &str) {
        for c in s.chars() {
            send_key(app, crossterm::event::KeyCode::Char(c));
        }
    }

    #[test]
    fn issues_mode_default_bars() {
        let app = fixture(ModeArg::Issues);
        insta::assert_snapshot!(bars(&app, 100));
    }

    #[test]
    fn issues_mode_with_search_and_skipped() {
        let mut app = with_skipped(fixture(ModeArg::Issues), 2);
        app.search.issues = "log".into();
        app.filters.role = FilterSet::only([None]);
        insta::assert_snapshot!(bars(&app, 100));
    }

    #[test]
    fn triage_mode_bars_have_no_filter_keys() {
        let mut app = with_skipped(fixture(ModeArg::Triage), 1);
        app.search.triage = "apple".into();
        insta::assert_snapshot!(bars(&app, 100));

        let hints = keymap::hints(&app);
        let keys: Vec<&str> = hints.iter().map(|h| h.keys.as_str()).collect();
        for k in ["g", "r/s/t/p/e", "c"] {
            assert!(!keys.contains(&k), "{k} hinted in triage mode");
        }
        assert!(!hints
            .iter()
            .any(|h| h.label == "filter" || h.label == "group"));
    }

    #[test]
    fn search_input_active() {
        let mut app = fixture(ModeArg::Issues);
        type_str(&mut app, "/log");
        assert!(app.search_active);
        insta::assert_snapshot!(bars(&app, 100));
    }

    #[test]
    fn transient_message_replaces_hints() {
        let mut app = fixture(ModeArg::Issues);
        app.set_status("clipboard unavailable");
        insta::assert_snapshot!(bars(&app, 100));
        // The next key press clears it.
        send_key(&mut app, crossterm::event::KeyCode::Char('~'));
        assert!(bars(&app, 100).contains("help"));
    }

    #[test]
    fn status_expires_after_ttl() {
        let mut app = fixture(ModeArg::Issues);
        app.set_status("x");
        let t0 = app.status_since.unwrap();
        assert_eq!(
            app.status_deadline(),
            Some(t0 + crate::tui::app::STATUS_TTL)
        );
        app.expire_status(t0 + crate::tui::app::STATUS_TTL / 2);
        assert!(app.status.is_some());
        app.expire_status(t0 + crate::tui::app::STATUS_TTL);
        assert_eq!(app.status, None);
        assert_eq!(app.status_deadline(), None);
    }

    #[test]
    fn mode_bar_highlights_current_mode_and_shows_grouping() {
        let app = fixture(ModeArg::Issues);
        let out = render_app_styled(&app, 100, 10);
        let styles: Vec<&str> = out
            .lines()
            .skip_while(|l| *l != "--- styles ---")
            .filter(|l| l.starts_with("0:"))
            .collect();
        // Issues tab is reversed + bold; Triage tab is dim.
        assert!(styles[0].contains("REVERSED") && styles[0].contains("Issues (4)"));
        assert!(!styles[1].contains("REVERSED"));
        assert!(out.lines().next().unwrap().ends_with("group: epic"));

        let mut triage = fixture(ModeArg::Triage);
        triage.grouping = GroupByArg::Status;
        let top = render_app(&triage, 100, 10)
            .lines()
            .next()
            .unwrap()
            .to_string();
        assert!(top.contains("Triage (3)") && !top.contains("group"));
        let mut issues = fixture(ModeArg::Issues);
        issues.grouping = GroupByArg::Priority;
        assert!(render_app(&issues, 100, 10)
            .lines()
            .next()
            .unwrap()
            .ends_with("group: priority"));
    }

    #[test]
    fn counts_issues() {
        let mut app = fixture(ModeArg::Issues);
        // Closed hidden by default: 3 of 4.
        assert_eq!(app.counts(), (3, 4));
        app.filters.status = FilterSet::all();
        assert_eq!(app.counts(), (4, 4));
        app.search.issues = "log".into();
        assert_eq!(app.counts(), (2, 4));
        app.search.issues = "oauth".into();
        assert_eq!(app.counts(), (1, 4));
        app.search.issues = "zzz".into();
        assert_eq!(app.counts(), (0, 4));
    }

    #[test]
    fn counts_epic_headers() {
        // The epic matches the filters: counted once (header, no extra cells).
        // A closed epic with one visible child appears only as a dimmed
        // header: not counted.
        let snap = snapshot_of([
            epic("tz-1", "Open epic"),
            epic("tz-2", "Closed epic").closed(),
            issue("tz-3", "child a").child_of("tz-1"),
            issue("tz-4", "child b").child_of("tz-2"),
        ]);
        let app = App::from_snapshot(ModeArg::Issues, snap);
        assert_eq!(app.counts(), (3, 4));
        let mut app = app;
        app.filters.issue_type = FilterSet::all_except([IssueType::Epic]);
        assert_eq!(app.counts(), (2, 4));
        app.filters.status = FilterSet::only([Status::Closed]);
        assert_eq!(app.counts(), (0, 4));
    }

    #[test]
    fn counts_triage() {
        let mut app = fixture(ModeArg::Triage);
        assert_eq!(app.counts(), (3, 3));
        app.search.triage = "apple".into();
        assert_eq!(app.counts(), (2, 3));
        // Filters and the issues query do not apply in triage mode.
        app.search.issues = "zzz".into();
        app.filters.status = FilterSet::only([]);
        assert_eq!(app.counts(), (2, 3));
    }

    #[test]
    fn no_skipped_marker_without_skipped_files() {
        let app = fixture(ModeArg::Issues);
        assert!(!bars(&app, 100).contains("skipped"));
    }

    #[test]
    fn narrow_terminal_keeps_counts_visible() {
        let mut app = with_skipped(fixture(ModeArg::Issues), 1);
        update(&mut app, Action::Resize(40, 10));
        let text = bars(&app, 40);
        let filter_row = text.lines().nth(1).unwrap();
        assert!(filter_row.ends_with("3 of 4  ⚠ 1 skipped"), "{filter_row}");
    }

    fn labels(app: &App) -> Vec<&'static str> {
        keymap::hints(app).into_iter().map(|h| h.label).collect()
    }

    #[test]
    fn hints_change_with_context() {
        let mut app = fixture(ModeArg::Issues);
        assert_eq!(app.context(), Context::Grid);
        let grid = labels(&app);
        assert!(grid.contains(&"search") && grid.contains(&"filter") && grid.contains(&"quit"));
        assert!(!grid.contains(&"clear search"));
        app.search.issues = "x".into();
        assert!(labels(&app).contains(&"clear search"));
        app.search.issues.clear();

        // Detail
        app.focus = crate::tui::app::Focus::Detail;
        update(&mut app, Action::Resize(100, 10));
        assert_eq!(app.context(), Context::Detail);
        let detail = labels(&app);
        assert!(
            detail.contains(&"scroll") && detail.contains(&"links") && detail.contains(&"back")
        );
        assert!(!detail.contains(&"filter"));
        app.focus = crate::tui::app::Focus::Grid;

        // Search input
        app.search_active = true;
        update(&mut app, Action::Resize(100, 10));
        assert_eq!(labels(&app), ["keep", "cancel", "clear"]);
        app.search_active = false;

        // Picker
        app.overlay = Overlay::Picker(FilterKind::Status);
        update(&mut app, Action::Resize(100, 10));
        assert_eq!(app.context(), Context::Picker);
        assert_eq!(
            labels(&app),
            ["move", "toggle", "all/none", "apply", "cancel"]
        );

        // Help
        app.overlay = Overlay::Help;
        update(&mut app, Action::Resize(100, 10));
        assert_eq!(labels(&app), ["close"]);
    }

    #[test]
    fn hint_keys_come_from_the_keymap() {
        let app = fixture(ModeArg::Issues);
        let hints = keymap::hints(&app);
        let find = |l: &str| hints.iter().find(|h| h.label == l).unwrap().keys.clone();
        assert_eq!(find("help"), "?");
        assert_eq!(find("search"), "/");
        assert_eq!(find("filter"), "r/s/t/p/e");
        assert_eq!(find("group"), "g");
        assert_eq!(find("open"), "Enter");
    }

    #[test]
    fn triage_detail_hides_follow_link() {
        let mut app = fixture(ModeArg::Triage);
        app.focus = crate::tui::app::Focus::Detail;
        update(&mut app, Action::Resize(100, 10));
        let l = labels(&app);
        assert!(!l.contains(&"follow") && !l.contains(&"links"));
        assert!(l.contains(&"scroll"));
    }

    #[test]
    fn hints_that_do_not_fit_are_dropped_from_the_end() {
        let app = fixture(ModeArg::Issues);
        let full = hint_line(&app, 200).width();
        let short = hint_line(&app, 30);
        assert!(short.width() <= 30);
        assert!(short.width() < full);
        assert!(short.to_string().contains("help"));
        assert!(!short.to_string().contains("quit"));
    }
}
