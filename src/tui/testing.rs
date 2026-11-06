//! Test support (spec §14): fixture builders and a snapshot-test harness.
//!
//! # Fixtures
//!
//! ```ignore
//! let snap = snapshot_of(vec![
//!     epic("tz-1", "Auth").closed(),                    // closed epic
//!     issue("tz-2", "Login").child_of("tz-1").child_of("tz-3"), // multi-parent
//!     issue("tz-4", "Orphan").child_of("tz-404"),       // missing target
//!     issue("tz-5", "Waits").blocked_by("tz-2"),
//! ]);
//! ```
//!
//! All timestamps default to [`base_time`], so output never depends on the
//! current time.
//!
//! # Rendering
//!
//! [`render`] / [`render_app`] draw into a `TestBackend` and return the buffer as
//! text (trailing spaces trimmed); [`render_styled`] also appends a dump of
//! every styled run of cells, for colour-sensitive assertions. Feed the result
//! to `insta::assert_snapshot!`. Review changes with `cargo insta review`
//! (or run with `INSTA_UPDATE=always` and inspect the diff).
//!
//! # Time zone
//!
//! Timestamps are rendered in local time, so every harness entry point calls
//! [`init`], which pins `TZ=UTC` for the process (once, before any rendering).

use super::app::App;
use super::data::Snapshot;
use crate::storage::triage::TriageItem;
use crate::types::{Dependency, DependencyType, Issue, IssueType, Role, Status};
use chrono::{DateTime, Duration, TimeZone, Utc};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::{Frame, Terminal};
use std::sync::Once;

/// Pin the process time zone to UTC. Idempotent; call before rendering.
pub fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::set_var("TZ", "UTC"));
}

/// The fixed "now" all fixture timestamps derive from: 2025-01-01T00:00:00Z.
pub fn base_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap()
}

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

/// A fluent builder for [`Issue`]. Convert with `.build()` or pass directly to
/// [`snapshot_of`] (anything `Into<Issue>`).
#[derive(Debug, Clone)]
pub struct IssueBuilder(Issue);

/// A task with priority 2, open, created and updated at [`base_time`].
pub fn issue(id: &str, title: &str) -> IssueBuilder {
    let mut i = Issue::new(id.to_string(), title.to_string(), 2, IssueType::Task);
    i.created_at = base_time();
    i.updated_at = base_time();
    IssueBuilder(i)
}

/// Same as [`issue`] but of type epic.
pub fn epic(id: &str, title: &str) -> IssueBuilder {
    issue(id, title).ty(IssueType::Epic)
}

impl IssueBuilder {
    pub fn build(self) -> Issue {
        self.0
    }
    pub fn ty(mut self, t: IssueType) -> Self {
        self.0.issue_type = t;
        self
    }
    pub fn status(mut self, s: Status) -> Self {
        self.0.status = s;
        self
    }
    /// Sets status closed and `closed_at`.
    pub fn closed(mut self) -> Self {
        self.0.status = Status::Closed;
        self.0.closed_at = Some(self.0.updated_at);
        self
    }
    pub fn priority(mut self, p: i32) -> Self {
        self.0.priority = p;
        self
    }
    pub fn role(mut self, r: Role) -> Self {
        self.0.role = Some(r);
        self
    }
    pub fn effort(mut self, e: u8) -> Self {
        self.0.effort = Some(e);
        self
    }
    pub fn assignee(mut self, a: &str) -> Self {
        self.0.assignee = a.to_string();
        self
    }
    pub fn labels(mut self, labels: &[&str]) -> Self {
        self.0.labels = labels.iter().map(|l| l.to_string()).collect();
        self
    }
    pub fn dep(mut self, target: &str, ty: DependencyType) -> Self {
        self.0.depends_on.insert(target.to_string(), ty);
        self
    }
    /// Add a `child_of` target; call repeatedly for several parents. The target
    /// need not exist in the snapshot (missing parent).
    pub fn child_of(self, target: &str) -> Self {
        self.dep(target, DependencyType::ChildOf)
    }
    pub fn blocked_by(self, target: &str) -> Self {
        self.dep(target, DependencyType::BlockedBy)
    }
    pub fn related_to(self, target: &str) -> Self {
        self.dep(target, DependencyType::RelatedTo)
    }
    pub fn description(mut self, s: &str) -> Self {
        self.0.description = s.to_string();
        self
    }
    pub fn design(mut self, s: &str) -> Self {
        self.0.design = s.to_string();
        self
    }
    pub fn notes(mut self, s: &str) -> Self {
        self.0.notes = s.to_string();
        self
    }
    pub fn acceptance(mut self, s: &str) -> Self {
        self.0.acceptance_criteria = s.to_string();
        self
    }
    pub fn created(mut self, t: DateTime<Utc>) -> Self {
        self.0.created_at = t;
        self
    }
    pub fn updated(mut self, t: DateTime<Utc>) -> Self {
        self.0.updated_at = t;
        self
    }
    /// `updated_at` = [`base_time`] + `minutes` (a larger number is newer).
    pub fn updated_after(self, minutes: i64) -> Self {
        self.updated(base_time() + Duration::minutes(minutes))
    }
}

impl From<IssueBuilder> for Issue {
    fn from(b: IssueBuilder) -> Issue {
        b.0
    }
}

pub fn triage_item(name: &str, body: &str) -> TriageItem {
    TriageItem {
        name: name.to_string(),
        body: body.to_string(),
    }
}

/// Assemble a snapshot from issues: sorted by ID, `by_id` built, and
/// `dependents` computed from every issue's `depends_on` (targets that don't
/// exist are ignored). No triage items, nothing skipped.
pub fn snapshot_of<I: Into<Issue>>(issues: impl IntoIterator<Item = I>) -> Snapshot {
    snapshot_with(issues, Vec::new())
}

/// Like [`snapshot_of`], with triage items.
pub fn snapshot_with<I: Into<Issue>>(
    issues: impl IntoIterator<Item = I>,
    triage: Vec<TriageItem>,
) -> Snapshot {
    let mut issues: Vec<Issue> = issues.into_iter().map(Into::into).collect();
    issues.sort_by(|a, b| a.id.cmp(&b.id));
    let edges: Vec<(String, Dependency)> = issues
        .iter()
        .flat_map(|i| {
            i.depends_on.iter().map(move |(target, ty)| {
                (
                    target.clone(),
                    Dependency {
                        id: i.id.clone(),
                        dep_type: ty.to_string(),
                    },
                )
            })
        })
        .collect();
    for (target, dep) in edges {
        if let Some(t) = issues.iter_mut().find(|i| i.id == target) {
            t.dependents.push(dep);
        }
    }
    for i in &mut issues {
        i.dependents.sort_by(|a, b| a.id.cmp(&b.id));
    }
    Snapshot::new(issues, triage, Vec::new())
}

/// Two issues and one triage item.
pub fn snapshot() -> Snapshot {
    snapshot_with(
        [issue("tz-1", "First"), issue("tz-2", "Second")],
        vec![triage_item("20250101T000000-aaaa.md", "idea")],
    )
}

// ---------------------------------------------------------------------------
// Rendering harness
// ---------------------------------------------------------------------------

fn draw_buffer(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> Buffer {
    init();
    let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
    term.draw(draw).unwrap();
    term.backend().buffer().clone()
}

/// The buffer's text, one line per row, trailing spaces trimmed.
pub fn buffer_text(buf: &Buffer) -> String {
    let area = buf.area;
    (area.top()..area.bottom())
        .map(|y| {
            let line: String = (area.left()..area.right())
                .map(|x| buf[(x, y)].symbol())
                .collect();
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One line per run of adjacent cells with the same non-default style:
/// `y:x..x_end fg=… bg=… mods=… "text"`. Default-styled cells are omitted.
pub fn style_dump(buf: &Buffer) -> String {
    let area = buf.area;
    let mut out = Vec::new();
    for y in area.top()..area.bottom() {
        let mut x = area.left();
        while x < area.right() {
            let cell = &buf[(x, y)];
            let key = (cell.fg, cell.bg, cell.modifier);
            if key == (Color::Reset, Color::Reset, Modifier::empty()) {
                x += 1;
                continue;
            }
            let start = x;
            let mut text = String::new();
            while x < area.right() && {
                let c = &buf[(x, y)];
                (c.fg, c.bg, c.modifier) == key
            } {
                text.push_str(buf[(x, y)].symbol());
                x += 1;
            }
            out.push(format!(
                "{y}:{start}..{x} fg={:?} bg={:?} mods={:?} {:?}",
                key.0, key.1, key.2, text
            ));
        }
    }
    out.join("\n")
}

/// Draw with `draw` into a `width` x `height` test terminal; return the text.
pub fn render(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> String {
    buffer_text(&draw_buffer(width, height, draw))
}

/// Like [`render`], followed by a `--- styles ---` section from [`style_dump`].
pub fn render_styled(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> String {
    let buf = draw_buffer(width, height, draw);
    format!(
        "{}\n--- styles ---\n{}",
        buffer_text(&buf),
        style_dump(&buf)
    )
}

/// Render the whole app. The app's recorded size is *not* changed; set it with
/// `Action::Resize` first if layout depends on it.
pub fn render_app(app: &App, width: u16, height: u16) -> String {
    render(width, height, |f| super::ui::draw(f, app))
}

/// [`render_app`] with a style dump.
pub fn render_app_styled(app: &App, width: u16, height: u16) -> String {
    render_styled(width, height, |f| super::ui::draw(f, app))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::{update, Action};
    use crate::tui::ModeArg;
    use ratatui::style::Stylize;
    use ratatui::widgets::Paragraph;

    #[test]
    fn empty_app_snapshot() {
        let mut app = App::from_snapshot(ModeArg::Issues, Snapshot::default());
        update(&mut app, Action::Resize(40, 11));
        insta::assert_snapshot!(render_app(&app, 40, 11));
    }

    #[test]
    fn styled_dump_lists_styled_runs() {
        let out = render_styled(12, 2, |f| {
            f.render_widget(
                Paragraph::new("plain red").style(ratatui::style::Style::new()),
                f.area(),
            );
            let mut rect = f.area();
            rect.y = 1;
            f.render_widget(Paragraph::new("hi".red().bold()), rect);
        });
        insta::assert_snapshot!(out);
    }

    #[test]
    fn fixtures_express_hierarchies_in_one_line_each() {
        let s = snapshot_of([
            epic("tz-1", "Closed epic").closed(),
            issue("tz-2", "Two parents")
                .child_of("tz-1")
                .child_of("tz-3"),
            issue("tz-4", "Missing parent").child_of("tz-404"),
            issue("tz-5", "Blocked").blocked_by("tz-2"),
        ]);
        assert_eq!(s.issue("tz-1").unwrap().status, Status::Closed);
        let deps: Vec<_> = s
            .issue("tz-1")
            .unwrap()
            .dependents
            .iter()
            .map(|d| &d.id)
            .collect();
        assert_eq!(deps, ["tz-2"]);
        assert_eq!(s.issue("tz-2").unwrap().depends_on.len(), 2);
        assert_eq!(s.issue("tz-2").unwrap().dependents[0].id, "tz-5");
        assert!(!s.by_id.contains_key("tz-404"));
        assert!(s.issue("tz-4").unwrap().depends_on.contains_key("tz-404"));
    }

    #[test]
    fn timestamps_are_fixed_and_timezone_independent() {
        init();
        let i = issue("tz-1", "x").build();
        assert_eq!(i.created_at, base_time());
        assert_eq!(i.updated_at, base_time());
        let local = i.created_at.with_timezone(&chrono::Local);
        assert_eq!(
            local.format("%Y-%m-%d %H:%M").to_string(),
            "2025-01-01 00:00"
        );
    }
}
