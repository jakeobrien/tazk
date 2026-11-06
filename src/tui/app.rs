//! `App` state, the `Action` enum and `fn update(&mut App, Action)`. No rendering and no
//! terminal access, so key sequences are testable without drawing.

use super::data::Snapshot;
use super::keymap::Context;
use super::ui::layout::LayoutMode;
use super::{GroupByArg, ModeArg, TuiArgs};

/// Direction for selection / list movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Which filter a picker edits (spec §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Role,
    Status,
    Type,
    Priority,
    Effort,
}

/// Everything that can change the app state. The event loop only translates
/// terminal events (and the periodic timer) into these; key events are mapped
/// by `keymap::resolve` and nowhere else.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    // Events
    /// The terminal was resized to `(columns, rows)`.
    Resize(u16, u16),
    /// The terminal gained focus (reload trigger, spec §3.3).
    FocusGained,
    /// The 10-second timer fired (reload trigger, spec §3.3).
    Tick,
    /// A freshly loaded snapshot replaces the current one (spec §3.4).
    Reloaded(Box<Snapshot>),

    // Global
    /// Leave the TUI (`q`, `Ctrl-C`).
    Quit,
    OpenHelp,
    CloseHelp,
    /// Reload now (`R`).
    Reload,
    /// Cycle mode: Issues <-> Triage.
    CycleMode,
    /// Copy the selected / displayed ID or filename.
    CopyId,
    /// Open the selected / displayed file in `$EDITOR`.
    OpenEditor,

    // Grid
    Move(Dir),
    ColumnPageUp,
    ColumnPageDown,
    ColumnTop,
    ColumnBottom,
    OpenDetail,
    StartSearch,
    /// `Esc` in the grid: clear the active search.
    ClearSearchFromGrid,
    CycleGrouping,
    OpenPicker(FilterKind),
    ResetFilters,

    // Detail (also `Ctrl-U` / `Ctrl-D` from the grid in split view)
    DetailLineUp,
    DetailLineDown,
    DetailPageUp,
    DetailPageDown,
    DetailHalfPageUp,
    DetailHalfPageDown,
    DetailTop,
    DetailBottom,
    NextLink,
    PrevLink,
    FollowLink,
    /// Pop the link stack, then close / unfocus.
    DetailBack,

    // Search input
    SearchChar(char),
    SearchBackspace,
    SearchClear,
    SearchKeep,
    SearchCancel,

    // Picker popup
    PickerMove(Dir),
    PickerToggle,
    PickerAll,
    PickerNone,
    PickerApply,
    PickerCancel,
}

/// Which pane has input focus (spec §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Grid,
    Detail,
}

/// A modal overlay drawn above everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
    Picker(FilterKind),
}

/// One value per mode.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PerMode<T> {
    pub issues: T,
    pub triage: T,
}

impl<T> PerMode<T> {
    pub fn get(&self, mode: ModeArg) -> &T {
        match mode {
            ModeArg::Issues => &self.issues,
            ModeArg::Triage => &self.triage,
        }
    }

    pub fn get_mut(&mut self, mode: ModeArg) -> &mut T {
        match mode {
            ModeArg::Issues => &mut self.issues,
            ModeArg::Triage => &mut self.triage,
        }
    }
}

/// Application state.
///
/// The fields below are the single source of truth; `contexts` (what
/// `keymap::resolve` reads) is derived from them by [`App::sync_contexts`]
/// after every [`update`].
#[derive(Debug)]
pub struct App {
    pub snapshot: Snapshot,
    pub mode: ModeArg,
    pub grouping: GroupByArg,
    /// Search query per mode (spec §7: each mode keeps its own).
    pub search: PerMode<String>,
    /// True while typing in the search input.
    pub search_active: bool,
    pub focus: Focus,
    /// Detail is shown full-screen (narrow layout only).
    pub full_screen_detail: bool,
    pub overlay: Overlay,
    /// Transient message for the hint bar; `None` when there is nothing to show.
    pub status: Option<String>,
    /// Selected item per mode: issue ID / triage filename. Placeholder until
    /// the navigation issue gives it real behavior.
    pub selection: PerMode<Option<String>>,
    /// Input contexts, innermost last. Never empty (starts as `[Grid]`).
    /// Derived; see [`App::sync_contexts`].
    pub contexts: Vec<Context>,
    pub should_quit: bool,
    /// Last known terminal size `(columns, rows)`.
    pub size: (u16, u16),
    /// Set by `FocusGained` / `Tick` / `Reload`; the loader consumes it with [`App::take_reload`].
    pub reload_requested: bool,
}

impl App {
    pub fn new(args: &TuiArgs) -> Self {
        Self::with_snapshot(args, Snapshot::default())
    }

    pub fn with_snapshot(args: &TuiArgs, snapshot: Snapshot) -> Self {
        let mut app = Self::from_snapshot(args.mode, snapshot);
        app.grouping = args.group_by;
        if let Some(q) = &args.search {
            app.search.issues = q.clone();
        }
        app
    }

    pub fn with_mode(mode: ModeArg) -> Self {
        Self::from_snapshot(mode, Snapshot::default())
    }

    /// Build an app from a snapshot with default view state. No filesystem access.
    pub fn from_snapshot(mode: ModeArg, snapshot: Snapshot) -> Self {
        Self {
            snapshot,
            mode,
            grouping: GroupByArg::Epic,
            search: PerMode::default(),
            search_active: false,
            focus: Focus::Grid,
            full_screen_detail: false,
            overlay: Overlay::None,
            status: None,
            selection: PerMode::default(),
            contexts: vec![Context::Grid],
            should_quit: false,
            size: (0, 0),
            reload_requested: false,
        }
    }

    /// The active layout, derived from the terminal width (spec §8).
    pub fn layout(&self) -> LayoutMode {
        LayoutMode::for_width(self.size.0)
    }

    /// Whether the split (grid + detail pane) layout is active (spec §8).
    pub fn is_split(&self) -> bool {
        self.layout() == LayoutMode::Split
    }

    /// The innermost input context.
    pub fn context(&self) -> Context {
        self.contexts.last().copied().unwrap_or(Context::Grid)
    }

    /// The current mode's search query.
    pub fn query(&self) -> &str {
        self.search.get(self.mode)
    }

    fn query_mut(&mut self) -> &mut String {
        self.search.get_mut(self.mode)
    }

    /// Show a transient status message.
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
    }

    pub fn clear_status(&mut self) {
        self.status = None;
    }

    /// Returns and clears the pending reload request.
    pub fn take_reload(&mut self) -> bool {
        std::mem::take(&mut self.reload_requested)
    }

    /// Rebuild `contexts` from the state fields.
    fn sync_contexts(&mut self) {
        self.contexts.clear();
        self.contexts.push(Context::Grid);
        if self.focus == Focus::Detail || self.full_screen_detail {
            self.contexts.push(Context::Detail);
        }
        if self.search_active {
            self.contexts.push(Context::SearchInput);
        }
        match self.overlay {
            Overlay::None => {}
            Overlay::Help => self.contexts.push(Context::Help),
            Overlay::Picker(_) => self.contexts.push(Context::Picker),
        }
    }
}

/// Apply an action to the state.
pub fn update(app: &mut App, action: Action) {
    match action {
        // Events
        Action::Resize(w, h) => {
            app.size = (w, h);
            // EXTENSION POINT (layout switching issue): reconcile `focus` /
            // `full_screen_detail` when the width crosses the split threshold.
        }
        Action::FocusGained | Action::Tick | Action::Reload => app.reload_requested = true,
        Action::Reloaded(snapshot) => {
            app.snapshot = *snapshot;
            // EXTENSION POINT (navigation issue): keep the selection by ID,
            // keep or clear the link stack (spec §3.4).
        }

        // Global
        Action::Quit => app.should_quit = true,
        Action::OpenHelp => {
            if app.overlay == Overlay::None {
                app.overlay = Overlay::Help;
            }
        }
        Action::CloseHelp => {
            if app.overlay == Overlay::Help {
                app.overlay = Overlay::None;
            }
        }
        Action::CycleMode => {
            app.mode = match app.mode {
                ModeArg::Issues => ModeArg::Triage,
                ModeArg::Triage => ModeArg::Issues,
            };
            // Each mode has its own selection; start from the grid.
            app.focus = Focus::Grid;
            app.full_screen_detail = false;
        }

        // Search input
        Action::StartSearch => app.search_active = true,
        Action::SearchChar(c) => app.query_mut().push(c),
        Action::SearchBackspace => {
            app.query_mut().pop();
        }
        Action::SearchClear => app.query_mut().clear(),
        Action::SearchKeep => app.search_active = false,
        Action::SearchCancel => {
            app.query_mut().clear();
            app.search_active = false;
        }
        Action::ClearSearchFromGrid => app.query_mut().clear(),

        // EXTENSION POINTS, each handled by its own issue:
        // - navigation: Move, Column*, OpenDetail, Detail*, NextLink, PrevLink,
        //   FollowLink, DetailBack, layout switching, CopyId, OpenEditor
        // - filters / grouping: CycleGrouping, OpenPicker, ResetFilters, Picker*
        Action::CopyId
        | Action::OpenEditor
        | Action::Move(_)
        | Action::ColumnPageUp
        | Action::ColumnPageDown
        | Action::ColumnTop
        | Action::ColumnBottom
        | Action::OpenDetail
        | Action::CycleGrouping
        | Action::OpenPicker(_)
        | Action::ResetFilters
        | Action::DetailLineUp
        | Action::DetailLineDown
        | Action::DetailPageUp
        | Action::DetailPageDown
        | Action::DetailHalfPageUp
        | Action::DetailHalfPageDown
        | Action::DetailTop
        | Action::DetailBottom
        | Action::NextLink
        | Action::PrevLink
        | Action::FollowLink
        | Action::DetailBack
        | Action::PickerMove(_)
        | Action::PickerToggle
        | Action::PickerAll
        | Action::PickerNone
        | Action::PickerApply
        | Action::PickerCancel => {}
    }
    app.sync_contexts();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::data::fixtures;
    use crate::tui::keymap;
    use clap::Parser;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: TuiArgs,
    }

    fn app() -> App {
        App::new(&Cli::parse_from(["tz"]).args)
    }

    /// An app built from a fixture snapshot, wide enough for any layout.
    fn fixture_app() -> App {
        let mut a = App::from_snapshot(ModeArg::Issues, fixtures::snapshot());
        update(&mut a, Action::Resize(100, 40));
        a
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// Feed a key through the real keymap into `update`.
    fn send(a: &mut App, k: KeyEvent) {
        if let Some(action) = keymap::resolve(&a.contexts, k, a) {
            update(a, action);
        }
    }

    fn type_str(a: &mut App, s: &str) {
        for c in s.chars() {
            send(a, key(KeyCode::Char(c)));
        }
    }

    #[test]
    fn quit_sets_flag() {
        let mut a = app();
        update(&mut a, Action::Quit);
        assert!(a.should_quit);
    }

    #[test]
    fn resize_records_size() {
        let mut a = app();
        update(&mut a, Action::Resize(80, 24));
        assert_eq!(a.size, (80, 24));
    }

    #[test]
    fn focus_and_tick_request_reload() {
        for action in [Action::FocusGained, Action::Tick] {
            let mut a = app();
            update(&mut a, action);
            assert!(a.take_reload());
            assert!(!a.take_reload());
        }
    }

    #[test]
    fn built_from_fixture_snapshot() {
        let a = fixture_app();
        assert_eq!(a.snapshot.issues.len(), 2);
        assert_eq!(a.snapshot.triage.len(), 1);
        assert_eq!(a.context(), Context::Grid);
        assert_eq!(a.focus, Focus::Grid);
        assert_eq!(a.overlay, Overlay::None);
    }

    #[test]
    fn q_sets_quit() {
        let mut a = fixture_app();
        send(&mut a, key(KeyCode::Char('q')));
        assert!(a.should_quit);
    }

    #[test]
    fn m_toggles_mode() {
        let mut a = fixture_app();
        send(&mut a, key(KeyCode::Char('m')));
        assert_eq!(a.mode, ModeArg::Triage);
        send(&mut a, key(KeyCode::Char('m')));
        assert_eq!(a.mode, ModeArg::Issues);
    }

    #[test]
    fn search_enter_keeps_query() {
        let mut a = fixture_app();
        send(&mut a, key(KeyCode::Char('/')));
        assert!(a.search_active);
        assert_eq!(a.context(), Context::SearchInput);
        type_str(&mut a, "logq");
        send(&mut a, key(KeyCode::Backspace));
        assert_eq!(a.query(), "log");
        send(&mut a, key(KeyCode::Enter));
        assert!(!a.search_active);
        assert_eq!(a.context(), Context::Grid);
        assert_eq!(a.query(), "log");
    }

    #[test]
    fn search_esc_clears_query() {
        let mut a = fixture_app();
        send(&mut a, key(KeyCode::Char('/')));
        type_str(&mut a, "abc");
        send(&mut a, key(KeyCode::Esc));
        assert!(!a.search_active);
        assert_eq!(a.query(), "");
    }

    #[test]
    fn ctrl_k_clears_query_and_stays_in_input() {
        let mut a = fixture_app();
        send(&mut a, key(KeyCode::Char('/')));
        type_str(&mut a, "abc");
        send(
            &mut a,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
        );
        assert_eq!(a.query(), "");
        assert!(a.search_active);
    }

    #[test]
    fn esc_in_grid_clears_active_search() {
        let mut a = fixture_app();
        send(&mut a, key(KeyCode::Char('/')));
        type_str(&mut a, "abc");
        send(&mut a, key(KeyCode::Enter));
        send(&mut a, key(KeyCode::Esc));
        assert_eq!(a.query(), "");
    }

    #[test]
    fn each_mode_keeps_its_own_query() {
        let mut a = fixture_app();
        send(&mut a, key(KeyCode::Char('/')));
        type_str(&mut a, "issue");
        send(&mut a, key(KeyCode::Enter));

        send(&mut a, key(KeyCode::Char('m')));
        assert_eq!(a.query(), "");
        send(&mut a, key(KeyCode::Char('/')));
        type_str(&mut a, "triage");
        send(&mut a, key(KeyCode::Enter));
        assert_eq!(a.query(), "triage");

        send(&mut a, key(KeyCode::Char('m')));
        assert_eq!(a.query(), "issue");
        assert_eq!(a.search.triage, "triage");
    }

    #[test]
    fn help_opens_and_esc_closes() {
        let mut a = fixture_app();
        send(&mut a, key(KeyCode::Char('?')));
        assert_eq!(a.overlay, Overlay::Help);
        assert_eq!(a.context(), Context::Help);
        // Help swallows other keys.
        send(&mut a, key(KeyCode::Char('q')));
        assert!(!a.should_quit);
        send(&mut a, key(KeyCode::Esc));
        assert_eq!(a.overlay, Overlay::None);
        assert_eq!(a.context(), Context::Grid);
    }

    #[test]
    fn question_mark_also_closes_help() {
        let mut a = fixture_app();
        send(&mut a, key(KeyCode::Char('?')));
        send(&mut a, key(KeyCode::Char('?')));
        assert_eq!(a.overlay, Overlay::None);
    }

    #[test]
    fn reloaded_replaces_snapshot() {
        let mut a = fixture_app();
        let snap = Snapshot::new(vec![fixtures::issue("tz-9", "New")], vec![], vec![]);
        update(&mut a, Action::Reloaded(Box::new(snap)));
        assert_eq!(a.snapshot.issues.len(), 1);
        assert!(a.snapshot.by_id.contains_key("tz-9"));
    }

    #[test]
    fn initial_state_comes_from_args() {
        let args = Cli::parse_from([
            "tz",
            "--mode",
            "triage",
            "--group-by",
            "role",
            "--search",
            "x",
        ])
        .args;
        let a = App::with_snapshot(&args, fixtures::snapshot());
        assert_eq!(a.mode, ModeArg::Triage);
        assert_eq!(a.grouping, GroupByArg::Role);
        assert_eq!(a.search.issues, "x");
        assert_eq!(a.search.triage, "");
    }

    #[test]
    fn layout_is_derived_from_width() {
        let mut a = app();
        update(&mut a, Action::Resize(159, 40));
        assert_eq!(a.layout(), LayoutMode::Narrow);
        update(&mut a, Action::Resize(160, 40));
        assert_eq!(a.layout(), LayoutMode::Split);
    }
}
