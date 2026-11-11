//! End-to-end key-sequence tests (spec §14).
//!
//! A fixture `.tazk` directory is written to a temp dir, loaded with the real
//! tolerant loader, and the full [`App`] is driven with key events through the
//! same path the event loop uses ([`terminal::handle_event`]). Screens are
//! rendered to a `TestBackend` at 120x40 (narrow layout) and 200x50 (split
//! view) and compared with committed snapshots.
//!
//! The fixture (all timestamps fixed, so output is deterministic):
//!
//! ```text
//! tz-1  epic   Platform                      (top-level epic)
//!  ├ tz-2 epic Auth rework                   (nested epic)
//!  │  ├ tz-3 Login form       blocked by tz-5 (open blocker)
//!  │  └ tz-4 Session cookie   also child of tz-6 (closed epic), blocked by tz-7 (closed)
//!  └ tz-5 Session store       in progress
//! tz-6  epic   Legacy cleanup                (closed epic)
//! tz-7  task   Old migration                 (closed)
//! tz-8  task   Standalone chore              blocked by tz-404 (missing)
//! tz-9  feature Docs                         (human)
//! tz-10 task   Orphan                        child of tz-404 (missing parent)
//! + 3 triage items and one unparsable issue file (skipped).
//! ```

use super::app::{App, Focus};
use super::data::{Reloader, Snapshot};
use super::terminal::{handle_event, pump_reloads};
use super::testing::{self as fixtures, render_app};
use super::{ModeArg, TuiArgs};
use crate::format::issue_to_markdown;
use crate::storage::{IssueStorageLayout, Storage};
use crate::types::{Role, Status};
use clap::Parser;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

const NARROW: (u16, u16) = (120, 40);
const WIDE: (u16, u16) = (200, 50);

#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    args: TuiArgs,
}

fn fixture_issues() -> Vec<fixtures::IssueBuilder> {
    vec![
        fixtures::epic("tz-1", "Platform")
            .priority(0)
            .description("Everything that makes up the platform."),
        fixtures::epic("tz-2", "Auth rework")
            .priority(1)
            .child_of("tz-1"),
        fixtures::issue("tz-3", "Login form redirects loop")
            .priority(1)
            .role(Role::Agent)
            .effort(3)
            .child_of("tz-2")
            .blocked_by("tz-5")
            .description("The **login** form loops after a timeout.\n\nSee also tz-4.")
            .acceptance("- works in Safari\n- no loop"),
        fixtures::issue("tz-4", "Session cookie dropped")
            .ty(crate::types::IssueType::Bug)
            .priority(0)
            .child_of("tz-2")
            .child_of("tz-6")
            .blocked_by("tz-7")
            .related_to("tz-3"),
        fixtures::issue("tz-5", "Session store")
            .priority(2)
            .status(Status::InProgress)
            .child_of("tz-1"),
        fixtures::epic("tz-6", "Legacy cleanup")
            .priority(3)
            .closed(),
        fixtures::issue("tz-7", "Old migration")
            .priority(3)
            .closed(),
        fixtures::issue("tz-8", "Standalone chore")
            .priority(3)
            .blocked_by("tz-404"),
        fixtures::issue("tz-9", "Write the docs")
            .ty(crate::types::IssueType::Feature)
            .priority(2)
            .role(Role::Human)
            .effort(2),
        fixtures::issue("tz-10", "Orphan task")
            .priority(4)
            .child_of("tz-404"),
    ]
}

const TRIAGE: [(&str, &str); 3] = [
    (
        "20250101T000000-aaaa.md",
        "Login page is slow on mobile\n\nTakes about 5 seconds.\n",
    ),
    ("20250102T000000-bbbb.md", "Add dark mode\n"),
    ("by-hand.md", "A note written by hand.\n"),
];

struct Fixture {
    _tmp: tempfile::TempDir,
    storage: Arc<Storage>,
}

impl Fixture {
    fn new() -> Self {
        fixtures::init();
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        let storage = Storage::init(tazk_dir.clone(), false, IssueStorageLayout::Flat).unwrap();
        for issue in fixture_issues() {
            write_issue(&tazk_dir, issue);
        }
        fs::write(tazk_dir.join("issues/tz-bad.md"), "not a valid issue\n").unwrap();
        let triage = tazk_dir.join("triage");
        fs::create_dir_all(&triage).unwrap();
        for (name, body) in TRIAGE {
            fs::write(triage.join(name), body).unwrap();
        }
        Self {
            _tmp: tmp,
            storage: Arc::new(storage),
        }
    }

    fn dir(&self) -> std::path::PathBuf {
        self.storage.get_tazk_dir()
    }

    fn app(&self, flags: &[&str]) -> App {
        let args = Cli::parse_from(std::iter::once("tz").chain(flags.iter().copied())).args;
        let filters = args.initial_filters().unwrap();
        let snapshot = Snapshot::load(&self.storage).unwrap();
        let mut app = App::with_snapshot(&args, filters, snapshot);
        resize(&mut app, NARROW);
        app
    }

    fn reloader(&self) -> Reloader {
        Reloader::for_shared(Arc::clone(&self.storage))
    }
}

fn write_issue(tazk_dir: &Path, issue: fixtures::IssueBuilder) {
    let issue = issue.build();
    let path = tazk_dir.join("issues").join(format!("{}.md", issue.id));
    fs::write(path, issue_to_markdown(&issue).unwrap()).unwrap();
}

// ---- Driving the app

fn resize(app: &mut App, (w, h): (u16, u16)) {
    handle_event(app, Event::Resize(w, h));
}

fn press(app: &mut App, code: KeyCode) {
    press_mod(app, code, KeyModifiers::NONE);
}

fn press_mod(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    handle_event(app, Event::Key(KeyEvent::new(code, mods)));
}

/// Press each character as a key.
fn keys(app: &mut App, chars: &str) {
    for c in chars.chars() {
        press(app, KeyCode::Char(c));
    }
}

fn selected(app: &App) -> Option<&str> {
    app.selection.get(app.mode).as_deref()
}

/// Render at `size`, with the app's recorded size set to match.
fn screen(app: &mut App, size: (u16, u16)) -> String {
    resize(app, size);
    render_app(app, size.0, size.1)
}

/// Request a reload with `R` and wait for the background load to be applied.
fn reload(app: &mut App, reloader: &mut Reloader) {
    keys(app, "R");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        pump_reloads(app, reloader);
        if !reloader.in_flight() {
            break;
        }
        assert!(Instant::now() < deadline, "reload did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Move to the Auth rework column and select tz-3 (the cell with an open
/// blocker): No epic -> Platform -> Auth rework, then down past the header
/// and tz-4.
fn select_login_form(app: &mut App) {
    keys(app, "ll");
    keys(app, "jj");
    assert_eq!(selected(app), Some("tz-3"));
}

fn displayed(app: &App) -> Option<&str> {
    app.displayed_issue_id()
}

macro_rules! snap {
    ($name:expr, $text:expr) => {
        insta::assert_snapshot!($name, $text)
    };
}

#[test]
fn launch_shows_the_grid_at_both_sizes() {
    let fx = Fixture::new();
    let mut app = fx.app(&[]);
    assert_eq!(app.mode, ModeArg::Issues);
    // Closed issues are hidden by default; the bad file is counted.
    assert_eq!(app.counts(), (8, 10));
    assert_eq!(app.snapshot.skipped.len(), 1);
    assert_eq!(selected(&app), Some("tz-1"));
    let narrow = screen(&mut app, NARROW);
    assert!(narrow.contains("⚠ 1 skipped"));
    snap!("launch_120x40", narrow);
    snap!("launch_200x50", screen(&mut app, WIDE));
}

#[test]
fn navigate_open_detail_follow_a_link_and_go_back() {
    let fx = Fixture::new();
    let mut app = fx.app(&[]);
    select_login_form(&mut app);
    snap!("nav_01_selected", screen(&mut app, NARROW));

    press(&mut app, KeyCode::Enter);
    assert!(app.full_screen_detail);
    assert_eq!(displayed(&app), Some("tz-3"));
    snap!("nav_02_detail", screen(&mut app, NARROW));

    // Tab highlights the first relationship; Enter follows it.
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Enter);
    let first = displayed(&app).unwrap().to_string();
    assert_ne!(first, "tz-3", "followed a link");
    snap!("nav_03_followed", screen(&mut app, NARROW));

    // Follow on to another relationship, then unwind one step at a time.
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Enter);
    let second = displayed(&app).unwrap().to_string();
    press(&mut app, KeyCode::Esc);
    assert_eq!(displayed(&app), Some(first.as_str()), "back from {second}");
    press(&mut app, KeyCode::Esc);
    assert_eq!(displayed(&app), Some("tz-3"));
    assert!(app.full_screen_detail);
    press(&mut app, KeyCode::Esc);
    assert!(!app.full_screen_detail);
    assert_eq!(
        selected(&app),
        Some("tz-3"),
        "the grid selection never moved"
    );
    snap!("nav_04_back_in_grid", screen(&mut app, NARROW));
}

#[test]
fn picker_filter_then_reset_with_c() {
    let fx = Fixture::new();
    let mut app = fx.app(&[]);
    assert_eq!(app.counts(), (8, 10));

    // Status picker: open, in_progress, blocked, deferred, closed.
    keys(&mut app, "s");
    snap!("filter_01_status_picker", screen(&mut app, NARROW));
    keys(&mut app, "jjjj ");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.counts(), (10, 10), "closed issues are now shown");
    snap!("filter_02_with_closed", screen(&mut app, NARROW));

    // Narrow by priority too, then `c` restores every default at once.
    keys(&mut app, "pn");
    keys(&mut app, " ");
    press(&mut app, KeyCode::Enter);
    assert!(app.counts().0 < 10);
    keys(&mut app, "c");
    assert_eq!(app.counts(), (8, 10));
    assert_eq!(screen(&mut app, NARROW), {
        let fresh = fx.app(&[]);
        let mut fresh = fresh;
        screen(&mut fresh, NARROW)
    });
}

#[test]
fn picker_cancel_changes_nothing() {
    let fx = Fixture::new();
    let mut app = fx.app(&[]);
    keys(&mut app, "t");
    keys(&mut app, "n");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.counts(), (8, 10));
}

#[test]
fn search_in_both_modes_with_separate_queries() {
    let fx = Fixture::new();
    let mut app = fx.app(&[]);

    keys(&mut app, "/login");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.query(), "login");
    assert_eq!(app.counts().0, 1);
    // The match is shown under its (dimmed) epic header, which is selected first.
    assert_eq!(selected(&app), Some("tz-2"));
    snap!("search_01_issues", screen(&mut app, NARROW));

    // Triage mode has its own query.
    keys(&mut app, "m");
    assert_eq!(app.mode, ModeArg::Triage);
    assert_eq!(app.query(), "");
    assert_eq!(app.counts(), (3, 3));
    keys(&mut app, "/dark");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.counts(), (1, 3));
    assert_eq!(selected(&app), Some("20250102T000000-bbbb.md"));
    snap!("search_02_triage", screen(&mut app, NARROW));

    // Back in issues mode the first query is still active; Esc clears it.
    keys(&mut app, "m");
    assert_eq!(app.query(), "login");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.query(), "");
    assert_eq!(app.counts(), (8, 10));
    keys(&mut app, "m");
    assert_eq!(app.query(), "dark", "the other mode is untouched");

    // Escape while typing discards the query.
    keys(&mut app, "/zzz");
    assert_eq!(app.counts().0, 0);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.query(), "");
    assert_eq!(app.counts(), (3, 3));
}

#[test]
fn mode_switch_keeps_each_selection_and_targets_the_right_file() {
    let fx = Fixture::new();
    let mut app = fx.app(&[]);
    select_login_form(&mut app);

    keys(&mut app, "y");
    assert_eq!(app.take_copy().as_deref(), Some("tz-3"));
    keys(&mut app, "o");
    assert_eq!(
        app.take_editor_request(),
        Some(super::app::EditorTarget::Issue("tz-3".into()))
    );

    keys(&mut app, "m");
    assert_eq!(selected(&app), Some("20250101T000000-aaaa.md"));
    keys(&mut app, "l");
    assert_eq!(selected(&app), Some("20250102T000000-bbbb.md"));
    snap!("mode_triage_200x50", screen(&mut app, WIDE));
    keys(&mut app, "y");
    assert_eq!(app.take_copy().as_deref(), Some("20250102T000000-bbbb.md"));
    keys(&mut app, "o");
    assert_eq!(
        app.take_editor_request(),
        Some(super::app::EditorTarget::Triage(
            "20250102T000000-bbbb.md".into()
        ))
    );

    // Filter keys do nothing in triage mode.
    keys(&mut app, "sg");
    assert_eq!(app.overlay, super::app::Overlay::None);

    keys(&mut app, "m");
    assert_eq!(app.mode, ModeArg::Issues);
    assert_eq!(selected(&app), Some("tz-3"));
}

#[test]
fn cycling_through_every_grouping_keeps_the_selection() {
    let fx = Fixture::new();
    let mut app = fx.app(&[]);
    select_login_form(&mut app);
    for name in ["status", "role", "type", "priority", "epic"] {
        keys(&mut app, "g");
        assert_eq!(selected(&app), Some("tz-3"), "after grouping by {name}");
        snap!(format!("group_{name}"), screen(&mut app, NARROW));
    }
    assert_eq!(app.grouping, super::GroupByArg::Epic);
}

#[test]
fn resizing_across_160_columns_with_the_detail_open() {
    let fx = Fixture::new();
    let mut app = fx.app(&[]);
    select_login_form(&mut app);
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Enter);
    let followed = displayed(&app).unwrap().to_string();
    assert!(app.full_screen_detail);

    // Widening turns the full-screen detail into the focused split pane.
    let wide = screen(&mut app, WIDE);
    assert!(app.is_split());
    assert!(!app.full_screen_detail);
    assert_eq!(app.focus, Focus::Detail);
    assert_eq!(displayed(&app), Some(followed.as_str()), "link stack kept");
    snap!("resize_01_split_200x50", wide);

    // Narrowing again opens it full-screen with the same item.
    let narrow = screen(&mut app, NARROW);
    assert!(app.full_screen_detail);
    assert_eq!(displayed(&app), Some(followed.as_str()));
    snap!("resize_02_full_screen_120x40", narrow);

    // 159 is still narrow, 160 is split.
    resize(&mut app, (159, 40));
    assert!(!app.is_split());
    resize(&mut app, (160, 40));
    assert!(app.is_split());

    // Esc unwinds the stack and the detail; the grid selection is unchanged.
    press(&mut app, KeyCode::Esc);
    assert_eq!(displayed(&app), Some("tz-3"));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.focus, Focus::Grid);
    assert_eq!(selected(&app), Some("tz-3"));
}

#[test]
fn reload_after_editing_files_on_disk_preserves_the_selection() {
    let fx = Fixture::new();
    let mut reloader = fx.reloader();
    let mut app = fx.app(&[]);
    select_login_form(&mut app);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.snapshot.skipped.len(), 1);

    // Retitle the selected issue, close another, add one, delete one, and
    // repair the bad file.
    let dir = fx.dir();
    write_issue(
        &dir,
        fixtures::issue("tz-3", "Login form: fixed title")
            .priority(1)
            .role(Role::Agent)
            .effort(3)
            .child_of("tz-2")
            .blocked_by("tz-5"),
    );
    write_issue(&dir, fixtures::issue("tz-9", "Write the docs").closed());
    write_issue(
        &dir,
        fixtures::issue("tz-11", "Brand new issue").priority(0),
    );
    fs::remove_file(dir.join("issues/tz-10.md")).unwrap();
    write_issue(&dir, fixtures::issue("tz-bad", "Repaired file"));
    fs::write(dir.join("triage/20250103T000000-cccc.md"), "Fresh report\n").unwrap();

    reload(&mut app, &mut reloader);
    assert_eq!(app.snapshot.skipped.len(), 0, "the repaired file loads");
    assert_eq!(selected(&app), Some("tz-3"), "selection kept by ID");
    assert_eq!(displayed(&app), Some("tz-3"));
    assert!(app.full_screen_detail, "the open detail stays open");
    let detail = screen(&mut app, NARROW);
    assert!(detail.contains("Login form: fixed title"));
    assert!(!detail.contains("skipped"));
    snap!("reload_01_detail_shows_new_title", detail);

    press(&mut app, KeyCode::Esc);
    let grid = screen(&mut app, NARROW);
    assert!(grid.contains("Brand new issue"));
    assert!(!grid.contains("Orphan task"));
    assert!(!grid.contains("Write the docs"), "tz-9 is closed now");
    snap!("reload_02_grid", grid);

    // Triage items are reloaded too.
    keys(&mut app, "m");
    assert_eq!(app.counts(), (4, 4));

    // Deleting the selected issue moves the selection somewhere valid.
    keys(&mut app, "m");
    fs::remove_file(dir.join("issues/tz-3.md")).unwrap();
    reload(&mut app, &mut reloader);
    let now = selected(&app).map(str::to_string);
    assert!(now.is_some() && now.as_deref() != Some("tz-3"), "{now:?}");
}

#[test]
fn help_overlay_lists_the_skipped_file_and_swallows_keys() {
    let fx = Fixture::new();
    let mut app = fx.app(&[]);
    keys(&mut app, "?");
    let help = screen(&mut app, NARROW);
    assert!(help.contains("1 file skipped"), "{help}");
    assert!(help.contains("tz-bad.md"));
    // Swallowed while open.
    keys(&mut app, "mgs");
    assert_eq!(app.mode, ModeArg::Issues);
    assert_eq!(app.overlay, super::app::Overlay::Help);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.overlay, super::app::Overlay::None);
    // Ctrl-C quits from anywhere, even with the help open.
    keys(&mut app, "?");
    press_mod(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(app.should_quit);
}

#[test]
fn flags_set_the_initial_view() {
    let fx = Fixture::new();
    // `--search` is an issues-mode query; triage starts unfiltered.
    let mut app = fx.app(&["--mode", "triage", "--search", "login"]);
    assert_eq!(app.mode, ModeArg::Triage);
    assert_eq!(app.counts(), (3, 3));
    keys(&mut app, "m");
    assert_eq!(app.query(), "login");
    assert_eq!(app.counts().0, 1);
    snap!("flags_search_in_issues", screen(&mut app, NARROW));

    let mut app = fx.app(&["--group-by", "status", "-s", "closed"]);
    assert_eq!(app.counts(), (2, 10));
    snap!("flags_group_by_status_closed", screen(&mut app, NARROW));
}
