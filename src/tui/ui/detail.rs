//! Issue detail rendering (spec §9.1–§9.2).
//!
//! The whole detail is one scrollable pane. [`DetailView::build`] composes it
//! into a list of lines: a *head* (title, properties, relationships) where
//! every line is exactly one row (titles are wrapped and values truncated up
//! front), followed by the markdown *body*, which the renderer wraps. Because
//! head lines never wrap, the row index of every relationship entry is known
//! exactly, and the total height is `head rows + wrapped body rows`.
//!
//! The view is built for a width; [`DetailView::sync_state`] reports its
//! height to the [`DetailState`] and scrolls a newly highlighted entry into
//! view; [`DetailView::render`] draws the rows from the state's scroll offset.
//!
//! ```text
//! Fix Safari login                        title (bold, wrapped)
//!                                         
//! ID           tz-14                      properties
//! Status       open  ⊘ open blockers
//! ...
//!
//! Blocked by                              relationship sections
//! ▶ tz-3  open  Set up the session store  ▶ marks the highlighted entry
//!   tz-9  (missing)
//!
//! # Description                           body (markdown)
//! ```
//!
//! Rendering only.

use super::cell::{truncate_with_ellipsis, wrap_title};
use super::markdown::{issue_markdown, MarkdownCache, Rendered};
use super::theme;
use crate::tui::app::App;
use crate::tui::data::Snapshot;
use crate::tui::model::blockers::has_open_blockers;
use crate::tui::model::detail::DetailState;
use crate::tui::model::relations::{relationships, Entry, Relationships};
use crate::tui::ModeArg;
use crate::types::Issue;
use chrono::{DateTime, Local, TimeZone, Utc};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget};
use ratatui::Frame;
use std::rc::Rc;

/// Width of the property label column (`External ref` is the longest label).
const LABEL_WIDTH: usize = 12;
/// Columns in front of every relationship entry (marker + space).
const ENTRY_INDENT: usize = 2;
pub const MISSING_TEXT: &str = "(missing)";
pub const OPEN_BLOCKERS_TEXT: &str = "open blockers";
const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M";

/// `2025-11-04 14:32`: absolute, to the minute, in `dt`'s own time zone.
pub fn format_timestamp<Tz: TimeZone>(dt: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    dt.format(TIMESTAMP_FORMAT).to_string()
}

/// [`format_timestamp`] in the local time zone.
pub fn format_local(dt: DateTime<Utc>) -> String {
    format_timestamp(&dt.with_timezone(&Local))
}

/// A composed issue detail for one width.
#[derive(Debug, Clone)]
pub struct DetailView {
    /// Title, properties and relationships: one row per line.
    head: Vec<Line<'static>>,
    /// The rendered markdown body (wrapped when drawn).
    body: Rc<Rendered>,
    /// Row index (from the top) of each relationship entry, in display order.
    entry_rows: Vec<usize>,
}

impl DetailView {
    /// Compose the detail of `issue` for a pane `width` columns wide.
    /// `highlight` is the highlighted relationship entry (display order).
    pub fn build(
        issue: &Issue,
        snapshot: &Snapshot,
        width: u16,
        highlight: Option<usize>,
        markdown: &MarkdownCache,
    ) -> Self {
        let width = usize::from(width);
        let mut head: Vec<Line<'static>> = Vec::new();

        // Title.
        let title = wrap_title(&issue.title, width, usize::MAX);
        if title.is_empty() {
            head.push(Line::default());
        }
        for line in title {
            head.push(Line::styled(line, theme::heading_style()));
        }
        head.push(Line::default());

        // Properties.
        let blockers = has_open_blockers(issue, snapshot);
        head.extend(property_lines(issue, blockers, width));

        // Relationships.
        let rels = relationships(issue, snapshot);
        let entry_rows = relationship_lines(&rels, highlight, width, &mut head);

        // Body.
        head.push(Line::default());
        let body = markdown.get(&issue_markdown(issue), body_width(width));

        Self {
            head,
            body,
            entry_rows,
        }
    }

    /// Total rows of the whole composition.
    pub fn height(&self) -> usize {
        self.head.len() + self.body.line_count
    }

    /// Row index of the `index`th relationship entry (display order).
    pub fn entry_row(&self, index: usize) -> Option<usize> {
        self.entry_rows.get(index).copied()
    }

    /// Row index of every relationship entry, in display order.
    pub fn entry_rows(&self) -> &[usize] {
        &self.entry_rows
    }

    /// Report the layout to `state`: total height and viewport height (which
    /// also re-clamps the scroll), then, if the highlight just moved, scroll
    /// its entry into view.
    pub fn sync_state(&self, state: &mut DetailState, viewport_height: u16) {
        state.set_metrics(self.height(), usize::from(viewport_height));
        if state.reveal_pending() {
            match state.highlight().and_then(|i| self.entry_row(i)) {
                Some(row) => state.reveal_lines(row, 1),
                None => state.reveal_lines(0, 1),
            }
        }
    }

    /// Draw the rows from `scroll` on into `area`.
    pub fn render(&self, area: Rect, buf: &mut Buffer, scroll: usize) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let head_rows = self.head.len();
        let visible_head = self.head.iter().skip(scroll).take(usize::from(area.height));
        let mut drawn: u16 = 0;
        for (row, line) in visible_head.enumerate() {
            let rect = Rect::new(area.x, area.y + row as u16, area.width, 1);
            line.render(rect, buf);
            drawn += 1;
        }
        if drawn < area.height {
            let rect = Rect::new(area.x, area.y + drawn, area.width, area.height - drawn);
            let offset = scroll.saturating_sub(head_rows).min(usize::from(u16::MAX));
            self.body
                .paragraph()
                .scroll((offset as u16, 0))
                .render(rect, buf);
        }
    }
}

/// Build, sync `state` and draw the detail of `issue` into `area`.
pub fn draw_issue_detail(
    frame: &mut Frame,
    area: Rect,
    issue: &Issue,
    snapshot: &Snapshot,
    state: &mut DetailState,
    markdown: &MarkdownCache,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let view = DetailView::build(issue, snapshot, area.width, state.highlight(), markdown);
    view.sync_state(state, area.height);
    view.render(area, frame.buffer_mut(), state.scroll());
}

/// Draw the detail pane (split view or full-screen) into `area`: a border
/// (accented when `focused`) around the displayed issue (spec §8, §9). The
/// displayed issue is the top of the link stack, else the grid selection.
/// Triage detail is drawn by its own issue.
pub fn draw_detail_pane(frame: &mut Frame, area: Rect, app: &mut App, focused: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let id = app.displayed_issue_id().map(str::to_string);
    let mut block = Block::bordered().border_style(theme::pane_border_style(focused));
    if let Some(id) = &id {
        block = block.title(format!(" {id} "));
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if app.mode != ModeArg::Issues || inner.width == 0 || inner.height == 0 {
        return;
    }
    match id.as_deref().and_then(|id| app.snapshot.issue(id)) {
        Some(issue) => draw_issue_detail(
            frame,
            inner,
            issue,
            &app.snapshot,
            &mut app.detail,
            &app.markdown,
        ),
        None => {
            let [_, mid, _] = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .areas(inner);
            frame.render_widget(
                Paragraph::new(NO_SELECTION_MESSAGE)
                    .alignment(Alignment::Center)
                    .style(theme::empty_message_style()),
                mid,
            );
        }
    }
}

pub const NO_SELECTION_MESSAGE: &str = "No issue selected";

fn body_width(width: usize) -> u16 {
    width.min(usize::from(u16::MAX)) as u16
}

// --- Properties ----------------------------------------------------------------

fn property_lines(issue: &Issue, blockers: bool, width: usize) -> Vec<Line<'static>> {
    let value_width = width.saturating_sub(LABEL_WIDTH + 1);
    let text = |s: &str| -> Span<'static> {
        if s.trim().is_empty() {
            Span::styled(theme::UNSET, theme::dim_style())
        } else {
            Span::raw(truncate_with_ellipsis(s, value_width))
        }
    };
    let opt_text = |s: Option<&str>| text(s.unwrap_or(""));

    let mut status = vec![theme::status_badge(issue.status)];
    if blockers {
        status.push(Span::raw("  "));
        status.push(theme::blockers_marker());
        status.push(Span::styled(
            format!(" {OPEN_BLOCKERS_TEXT}"),
            Style::new().fg(theme::ORANGE),
        ));
    }

    let mut rows: Vec<(&str, Vec<Span<'static>>)> = vec![
        ("ID", vec![Span::raw(issue.id.clone())]),
        (
            "Type",
            vec![Span::styled(
                issue.issue_type.as_str(),
                theme::type_style(issue.issue_type),
            )],
        ),
        ("Status", status),
        ("Priority", vec![theme::priority_badge(issue.priority)]),
        ("Role", vec![theme::role_badge(issue.role)]),
        ("Effort", vec![theme::effort_badge(issue.effort)]),
        ("Assignee", vec![text(&issue.assignee)]),
        ("Labels", vec![text(&issue.labels.join(", "))]),
        (
            "External ref",
            vec![opt_text(issue.external_ref.as_deref())],
        ),
        ("Created", vec![Span::raw(format_local(issue.created_at))]),
        ("Updated", vec![Span::raw(format_local(issue.updated_at))]),
    ];
    if let Some(closed) = issue.closed_at {
        rows.push(("Closed", vec![Span::raw(format_local(closed))]));
    }

    rows.into_iter()
        .map(|(label, value)| {
            let mut spans = vec![Span::styled(
                format!("{label:<LABEL_WIDTH$} "),
                theme::label_style(),
            )];
            spans.extend(value);
            Line::from(spans)
        })
        .collect()
}

// --- Relationships ---------------------------------------------------------------

/// Append the relationship sections to `head`; returns the row of each entry.
fn relationship_lines(
    rels: &Relationships,
    highlight: Option<usize>,
    width: usize,
    head: &mut Vec<Line<'static>>,
) -> Vec<usize> {
    let mut rows = Vec::with_capacity(rels.len());
    let mut index = 0;
    for section in &rels.sections {
        head.push(Line::default());
        head.push(Line::styled(section.kind.title(), theme::heading_style()));
        for entry in &section.entries {
            rows.push(head.len());
            head.push(entry_line(entry, highlight == Some(index), width));
            index += 1;
        }
    }
    rows
}

/// `▶ tz-3 open Title` (title truncated to the rest of the row); a missing
/// target is `tz-9 (missing)`. The highlighted entry has the marker and a
/// background across the whole row.
fn entry_line(entry: &Entry, highlighted: bool, width: usize) -> Line<'static> {
    let marker = if highlighted {
        theme::HIGHLIGHT_MARKER
    } else {
        " "
    };
    let mut spans = vec![Span::raw(format!("{marker} ")), Span::raw(entry.id.clone())];
    match &entry.target {
        None => {
            spans.push(Span::styled(format!(" {MISSING_TEXT}"), theme::dim_style()));
        }
        Some(target) => {
            spans.push(Span::raw(" "));
            spans.push(theme::status_badge(target.status));
            spans.push(Span::raw(" "));
            let used: usize = spans.iter().map(Span::width).sum();
            let room = width.saturating_sub(used.max(ENTRY_INDENT));
            // One-line title: newlines can't appear in a title, but be safe.
            let title = target
                .title
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            spans.push(Span::raw(truncate_with_ellipsis(&title, room)));
        }
    }
    let line = Line::from(spans);
    if highlighted {
        line.style(theme::highlight_style())
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::*;
    use crate::types::{IssueType, Role, Status};
    use chrono::{FixedOffset, TimeZone};

    fn view(snap: &Snapshot, id: &str, width: u16, hl: Option<usize>) -> DetailView {
        DetailView::build(
            snap.issue(id).unwrap(),
            snap,
            width,
            hl,
            &MarkdownCache::new(),
        )
    }

    fn draw(view: &DetailView, w: u16, h: u16, scroll: usize) -> String {
        render(w, h, |f| view.render(f.area(), f.buffer_mut(), scroll))
    }

    fn draw_styled(view: &DetailView, w: u16, h: u16, scroll: usize) -> String {
        render_styled(w, h, |f| view.render(f.area(), f.buffer_mut(), scroll))
    }

    fn full_snapshot() -> Snapshot {
        let at = |h| base_time() + chrono::Duration::hours(h);
        snapshot_of([
            issue("tz-1", "Fix Safari login fails on redirect after timeout")
                .ty(IssueType::Bug)
                .status(Status::InProgress)
                .priority(1)
                .role(Role::Agent)
                .effort(3)
                .assignee("jake")
                .labels(&["web", "auth"])
                .created(at(10))
                .updated(at(20))
                .child_of("tz-2")
                .child_of("tz-404")
                .blocked_by("tz-3")
                .blocked_by("tz-4")
                .related_to("tz-5")
                .dep("tz-6", crate::types::DependencyType::DiscoveredFrom)
                .dep("tz-7", crate::types::DependencyType::Tracks)
                .description("The session cookie is dropped.")
                .design("Keep it in `localStorage`.")
                .acceptance("- works in Safari")
                .notes("See the ticket."),
            epic("tz-2", "Auth epic"),
            issue("tz-3", "Set up the session store").status(Status::Blocked),
            issue("tz-4", "Closed work").closed(),
            issue("tz-5", "Related thing").related_to("tz-1"),
            issue("tz-6", "Origin"),
            issue("tz-7", "Tracked thing"),
            issue("tz-8", "Child one").child_of("tz-1"),
            issue("tz-9", "Waits on it").blocked_by("tz-1"),
            issue("tz-10", "Found while fixing")
                .dep("tz-1", crate::types::DependencyType::DiscoveredFrom),
            issue("tz-11", "Tracker").dep("tz-1", crate::types::DependencyType::Tracks),
        ])
    }

    #[test]
    fn timestamp_uses_the_given_offset_to_the_minute() {
        let utc = Utc.with_ymd_and_hms(2025, 11, 4, 22, 32, 59).unwrap();
        assert_eq!(format_timestamp(&utc), "2025-11-04 22:32");
        let east = FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
        assert_eq!(
            format_timestamp(&utc.with_timezone(&east)),
            "2025-11-05 04:02"
        );
        let west = FixedOffset::west_opt(8 * 3600).unwrap();
        assert_eq!(
            format_timestamp(&utc.with_timezone(&west)),
            "2025-11-04 14:32"
        );
    }

    #[test]
    fn local_timestamp_is_pinned_to_utc_in_tests() {
        init();
        assert_eq!(format_local(base_time()), "2025-01-01 00:00");
    }

    #[test]
    fn full_issue_with_every_property_and_section() {
        let snap = full_snapshot();
        // Add a closed_at for the Closed row.
        let mut snap = snap;
        let i = snap.by_id["tz-1"];
        snap.issues[i].closed_at = Some(base_time() + chrono::Duration::hours(30));
        snap.issues[i].external_ref = Some("gh-123".into());
        let v = view(&snap, "tz-1", 60, None);
        insta::assert_snapshot!(draw(&v, 60, v.height() as u16, 0));
    }

    #[test]
    fn full_issue_styles() {
        let snap = full_snapshot();
        let v = view(&snap, "tz-1", 60, None);
        insta::assert_snapshot!(draw_styled(&v, 60, 28, 0));
    }

    #[test]
    fn minimal_issue_shows_dim_dashes_and_no_closed_row() {
        let snap = snapshot_of([issue("tz-1", "Bare")]);
        let v = view(&snap, "tz-1", 50, None);
        let out = draw_styled(&v, 50, 16, 0);
        assert!(!out.contains("Closed"));
        assert!(out.contains("(no description)"));
        insta::assert_snapshot!(out);
    }

    #[test]
    fn open_blockers_marker_is_repeated_in_properties() {
        let snap = snapshot_of([
            issue("tz-1", "Waits").blocked_by("tz-2"),
            issue("tz-2", "Other"),
        ]);
        let v = view(&snap, "tz-1", 50, None);
        let out = draw_styled(&v, 50, 20, 0);
        assert!(out.contains("⊘ open blockers"));
        insta::assert_snapshot!(out);
        let snap = snapshot_of([issue("tz-1", "Fine")]);
        let out = draw(&view(&snap, "tz-1", 50, None), 50, 16, 0);
        assert!(!out.contains('⊘'));
    }

    #[test]
    fn highlighted_entry_is_marked_and_styled() {
        let snap = full_snapshot();
        let v = view(&snap, "tz-1", 60, Some(2));
        let out = draw_styled(&v, 60, 28, 0);
        // Index 2 in display order: Parent tz-2, tz-404, then Blocked by tz-3.
        let row = v.entry_row(2).unwrap();
        let text = out.lines().nth(row).unwrap();
        assert!(text.starts_with("▶ tz-3"), "{text}");
        assert_eq!(
            out.split("--- styles ---")
                .next()
                .unwrap()
                .matches('▶')
                .count(),
            1
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn missing_target_entry_shows_id_and_missing() {
        let snap = snapshot_of([issue("tz-1", "Orphan").child_of("tz-404")]);
        let v = view(&snap, "tz-1", 50, Some(0));
        let out = draw_styled(&v, 50, 20, 0);
        assert!(out.contains("▶ tz-404 (missing)"));
        insta::assert_snapshot!(out);
    }

    #[test]
    fn long_entry_titles_truncate_to_one_row() {
        let snap = snapshot_of([
            issue("tz-1", "Parent").child_of("tz-2"),
            issue(
                "tz-2",
                "A very long parent title that cannot possibly fit in a narrow pane",
            ),
        ]);
        let v = view(&snap, "tz-1", 30, None);
        let row = v.entry_row(0).unwrap();
        let out = draw(&v, 30, 20, 0);
        let line = out.lines().nth(row).unwrap();
        assert!(line.starts_with("  tz-2 open A very long"), "{line}");
        assert!(line.ends_with('…'), "{line}");
        assert!(Span::raw(line).width() <= 30);
    }

    #[test]
    fn long_title_wraps_and_shifts_entry_rows() {
        let snap = snapshot_of([
            issue("tz-1", "one two three four five six seven eight nine ten").child_of("tz-2"),
            issue("tz-2", "P"),
        ]);
        let narrow = view(&snap, "tz-1", 20, None);
        let wide = view(&snap, "tz-1", 80, None);
        assert!(narrow.entry_row(0).unwrap() > wide.entry_row(0).unwrap());
        let out = draw(&narrow, 20, 30, 0);
        let row = narrow.entry_row(0).unwrap();
        assert!(out.lines().nth(row).unwrap().contains("tz-2"));
    }

    #[test]
    fn reports_height_and_entry_rows() {
        let snap = full_snapshot();
        let v = view(&snap, "tz-1", 60, None);
        // Parent: tz-2, tz-404; Blocked by: tz-3, tz-4; Children: tz-8; ...
        assert_eq!(v.entry_rows().len(), 11);
        assert!(v.entry_rows().windows(2).all(|w| w[0] < w[1]));
        assert!(v.height() > *v.entry_rows().last().unwrap());
        // Height is the number of rows drawn: nothing below it.
        let out = draw(&v, 60, v.height() as u16 + 3, 0);
        let lines: Vec<_> = out.lines().collect();
        assert!(!lines[v.height() - 1].is_empty());
        assert!(lines[v.height()..].iter().all(|l| l.is_empty()));
    }

    #[test]
    fn scrolled_down_view() {
        let snap = full_snapshot();
        let v = view(&snap, "tz-1", 60, None);
        // Scroll past the whole head into the markdown body.
        let out = draw(&v, 60, 10, v.height() - 12);
        insta::assert_snapshot!(out);
        // Scrolling by one drops the first row.
        let a = draw(&v, 60, 6, 0);
        let b = draw(&v, 60, 6, 1);
        assert_eq!(a.lines().nth(1), b.lines().next());
    }

    #[test]
    fn tab_to_entry_below_the_fold_scrolls_it_into_view() {
        let mut issues = vec![issue("tz-1", "Many children")];
        for n in 2..32 {
            issues.push(issue(&format!("tz-{n}"), &format!("Child {n}")).child_of("tz-1"));
        }
        let snap = snapshot_of(issues);
        let (w, h) = (50u16, 12u16);
        let mut state = DetailState::default();
        let md = MarkdownCache::new();
        let build = |state: &DetailState| {
            DetailView::build(
                snap.issue("tz-1").unwrap(),
                &snap,
                w,
                state.highlight(),
                &md,
            )
        };

        // First layout: nothing highlighted, top of the pane.
        let v = build(&state);
        v.sync_state(&mut state, h);
        assert_eq!(state.scroll(), 0);
        assert!(v.entry_row(20).unwrap() >= usize::from(h));

        // Entries sort by ID as text: tz-10, tz-11, ..., tz-19, tz-2, tz-20, ...
        // Shift-Tab wraps to the last entry (tz-9), far below the fold.
        state.prev_link(v.entry_rows().len());
        let v = build(&state);
        v.sync_state(&mut state, h);
        let row = v.entry_row(29).unwrap();
        assert_eq!(row, *v.entry_rows().last().unwrap());
        assert!(state.scroll() > 0);
        assert!(row >= state.scroll() && row < state.scroll() + usize::from(h));
        let out = draw(&v, w, h, state.scroll());
        assert!(out.contains("▶ tz-9 open Child 9"), "{out}");

        // Tab wraps back to the first entry and scrolls up to it.
        state.next_link(v.entry_rows().len());
        let v = build(&state);
        v.sync_state(&mut state, h);
        let row = v.entry_row(0).unwrap();
        assert!(row >= state.scroll() && row < state.scroll() + usize::from(h));
        let out = draw(&v, w, h, state.scroll());
        assert!(out.contains("▶ tz-10 open Child 10"), "{out}");
        assert!(!state.reveal_pending());
    }

    #[test]
    fn draw_issue_detail_syncs_state_and_draws() {
        let snap = full_snapshot();
        let mut state = DetailState::default();
        state.next_link(11);
        let md = MarkdownCache::new();
        let out = render(60, 8, |f| {
            draw_issue_detail(
                f,
                f.area(),
                snap.issue("tz-1").unwrap(),
                &snap,
                &mut state,
                &md,
            )
        });
        assert!(state.max_scroll() > 0);
        assert!(!state.reveal_pending());
        // The highlighted entry (the first one) was revealed.
        assert!(out.contains("▶ tz-2 "), "{out}");
    }
}
