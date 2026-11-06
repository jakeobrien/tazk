//! `App` state, the `Action` enum and `fn update(&mut App, Action)`. No rendering and no
//! terminal access, so key sequences are testable without drawing.

use super::data::Snapshot;
use super::keymap::Context;
use super::model::detail::DetailState;
use super::model::filter::{visible_issues, Filters};
use super::model::grid::{GridState, Viewport};
use super::model::relations::{relationships, Relationships};
use super::model::search::visible_triage;
use super::ui::layout::{grid_capacity, LayoutMode};
use super::{GroupByArg, ModeArg, TuiArgs};
use std::time::{Duration, Instant};

/// How long a transient status message stays in the hint bar if no key is
/// pressed (spec §13; cleared earlier by the next key press).
pub const STATUS_TTL: Duration = Duration::from_secs(4);

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
    /// Issues-mode filters (spec §6).
    pub filters: Filters,
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
    /// When `status` was set, for expiry ([`App::expire_status`]).
    pub status_since: Option<Instant>,
    /// Selected item per mode: issue ID / triage filename. In issues mode this
    /// mirrors `grid` (the source of truth), kept in sync by [`App::refresh_grid`].
    pub selection: PerMode<Option<String>>,
    /// Issues grid: columns, selection and scroll offsets (spec §5).
    pub grid: GridState,
    /// Detail view: scroll, relationship highlight and link stack (spec §9).
    pub detail: DetailState,
    /// Input contexts, innermost last. Never empty (starts as `[Grid]`).
    /// Derived; see [`App::sync_contexts`].
    pub contexts: Vec<Context>,
    pub should_quit: bool,
    /// Last known terminal size `(columns, rows)`.
    pub size: (u16, u16),
    /// Set by `FocusGained` / `Tick` / `Reload`; the loader consumes it with [`App::take_reload`].
    pub reload_requested: bool,
    /// Text `y` wants on the clipboard; the event loop performs the copy
    /// ([`App::take_copy`]) so `update` stays free of side effects.
    pub copy_request: Option<String>,
}

impl App {
    /// An app with the flags' initial state and an empty snapshot. Panics if a
    /// filter flag is invalid (tests only; [`crate::tui::run`] reports it).
    pub fn new(args: &TuiArgs) -> Self {
        let filters = args.initial_filters().expect("valid filter flags");
        Self::with_snapshot(args, filters, Snapshot::default())
    }

    /// Initial state from the flags (spec §2.1): mode, grouping, `filters`
    /// (from [`TuiArgs::initial_filters`]) and the issues-mode search.
    pub fn with_snapshot(args: &TuiArgs, filters: Filters, snapshot: Snapshot) -> Self {
        let mut app = Self::from_snapshot(args.mode, snapshot);
        app.grouping = args.group_by;
        app.filters = filters;
        if let Some(q) = &args.search {
            app.search.issues = q.clone();
        }
        app.refresh_grid();
        app
    }

    pub fn with_mode(mode: ModeArg) -> Self {
        Self::from_snapshot(mode, Snapshot::default())
    }

    /// Build an app from a snapshot with default view state. No filesystem access.
    pub fn from_snapshot(mode: ModeArg, snapshot: Snapshot) -> Self {
        let mut app = Self {
            snapshot,
            mode,
            grouping: GroupByArg::Epic,
            filters: Filters::default(),
            search: PerMode::default(),
            search_active: false,
            focus: Focus::Grid,
            full_screen_detail: false,
            overlay: Overlay::None,
            status: None,
            status_since: None,
            selection: PerMode::default(),
            grid: GridState::default(),
            detail: DetailState::default(),
            contexts: vec![Context::Grid],
            should_quit: false,
            size: (0, 0),
            reload_requested: false,
            copy_request: None,
        };
        app.refresh_grid();
        app
    }

    /// How much of the grid fits on screen, from the last known size.
    pub fn viewport(&self) -> Viewport {
        let (columns, rows) = grid_capacity(self.size);
        Viewport::new(columns, rows)
    }

    /// Recompute the grid columns from the snapshot, filters, issues-mode
    /// search and grouping, and reconcile the selection and scroll offsets
    /// (spec §5.6).
    pub fn refresh_grid(&mut self) {
        let viewport = self.viewport();
        self.grid.rebuild(
            &self.snapshot,
            &self.filters,
            &self.search.issues,
            self.grouping,
            viewport,
        );
        self.selection.issues = self.grid.selected_id().map(str::to_string);
    }

    /// Apply a navigation action to the issues grid (issues mode only).
    fn navigate(&mut self, action: &Action) {
        if self.mode != ModeArg::Issues {
            return;
        }
        let vp = self.viewport();
        match action {
            Action::Move(dir) => self.grid.move_dir(*dir, vp),
            Action::ColumnPageUp => self.grid.page_up(vp),
            Action::ColumnPageDown => self.grid.page_down(vp),
            Action::ColumnTop => self.grid.top(vp),
            Action::ColumnBottom => self.grid.bottom(vp),
            _ => {}
        }
        self.selection.issues = self.grid.selected_id().map(str::to_string);
    }

    /// The issue the detail view shows (issues mode): the top of the link stack,
    /// else the selected grid item (spec §8, §9.4). `None` in triage mode.
    pub fn displayed_issue_id(&self) -> Option<&str> {
        if self.mode != ModeArg::Issues {
            return None;
        }
        self.detail.top().or(self.selection.issues.as_deref())
    }

    /// Relationships of the displayed issue; empty if there is none (or in
    /// triage mode).
    pub fn displayed_relationships(&self) -> Relationships {
        self.displayed_issue_id()
            .and_then(|id| self.snapshot.issue(id))
            .map(|issue| relationships(issue, &self.snapshot))
            .unwrap_or_default()
    }

    /// `Esc` in the detail: pop the link stack, else close the full-screen
    /// detail / return focus to the grid.
    fn detail_back(&mut self) {
        if !self.detail.pop() {
            self.full_screen_detail = false;
            self.focus = Focus::Grid;
        }
    }

    /// Apply a detail-view action (spec §9.4, §9.6).
    fn detail_action(&mut self, action: &Action) {
        match action {
            Action::DetailLineUp => self.detail.line_up(),
            Action::DetailLineDown => self.detail.line_down(),
            Action::DetailPageUp => self.detail.page_up(),
            Action::DetailPageDown => self.detail.page_down(),
            Action::DetailHalfPageUp => self.detail.half_page_up(),
            Action::DetailHalfPageDown => self.detail.half_page_down(),
            Action::DetailTop => self.detail.top_of_content(),
            Action::DetailBottom => self.detail.bottom_of_content(),
            // Triage items have no relationships: Tab / Enter do nothing.
            Action::NextLink if self.mode == ModeArg::Issues => {
                let len = self.displayed_relationships().len();
                self.detail.next_link(len);
            }
            Action::PrevLink if self.mode == ModeArg::Issues => {
                let len = self.displayed_relationships().len();
                self.detail.prev_link(len);
            }
            Action::FollowLink if self.mode == ModeArg::Issues => {
                let target = self
                    .detail
                    .highlight()
                    .and_then(|i| self.displayed_relationships().entry(i).cloned())
                    .filter(|e| e.followable());
                if let Some(entry) = target {
                    self.detail.push(entry.id);
                }
            }
            Action::DetailBack => self.detail_back(),
            _ => {}
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
        self.status_since = Some(Instant::now());
    }

    pub fn clear_status(&mut self) {
        self.status = None;
        self.status_since = None;
    }

    /// Clear the status message once it is older than [`STATUS_TTL`] at `now`.
    pub fn expire_status(&mut self, now: Instant) {
        if let Some(since) = self.status_since {
            if now.saturating_duration_since(since) >= STATUS_TTL {
                self.clear_status();
            }
        }
    }

    /// When the current status message expires (for the event loop's poll
    /// timeout); `None` if there is none.
    pub fn status_deadline(&self) -> Option<Instant> {
        self.status.as_ref()?;
        self.status_since.map(|t| t + STATUS_TTL)
    }

    /// `(shown, total)` for the filter bar's `X of Y` (spec §4).
    ///
    /// - Issues: `shown` is the number of issues passing the filters and the
    ///   search; `total` is every loaded issue, closed ones included. Each
    ///   issue counts once, so an epic that passes is counted once even though
    ///   epic grouping draws it both as a header and (if nested) a cell. An
    ///   epic that only appears as a dimmed header (it fails the filters or
    ///   search) is not counted, since it is not a matching issue.
    /// - Triage: items matching the search vs all items.
    pub fn counts(&self) -> (usize, usize) {
        match self.mode {
            ModeArg::Issues => (
                visible_issues(&self.snapshot, &self.filters, self.query()).len(),
                self.snapshot.issues.len(),
            ),
            ModeArg::Triage => (
                visible_triage(&self.snapshot, self.query()).len(),
                self.snapshot.triage.len(),
            ),
        }
    }

    /// Returns and clears the pending reload request.
    pub fn take_reload(&mut self) -> bool {
        std::mem::take(&mut self.reload_requested)
    }

    /// What `y` copies (spec §11): in triage mode the selected filename; in
    /// issues mode the selected issue while the grid has focus, or the
    /// displayed issue (top of the link stack) when the detail has focus or
    /// is full-screen. `None` if nothing is selected.
    pub fn copy_text(&self) -> Option<String> {
        match self.mode {
            ModeArg::Triage => self.selection.triage.clone(),
            ModeArg::Issues => {
                let detail = self.focus == Focus::Detail || self.full_screen_detail;
                let id = if detail {
                    self.displayed_issue_id()
                } else {
                    self.selection.issues.as_deref()
                };
                id.map(str::to_string)
            }
        }
    }

    /// Returns and clears the pending clipboard request.
    pub fn take_copy(&mut self) -> Option<String> {
        self.copy_request.take()
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
    // Moving the selection (by key, or because search / filters / grouping
    // changed it) clears the link stack and resets the detail scroll
    // (spec §9.4). A reload keeps both: it has its own rule (spec §3.4).
    let before = (app.mode, app.selection.get(app.mode).clone());
    let reloaded = matches!(action, Action::Reloaded(_));
    apply(app, action);
    if !reloaded && before != (app.mode, app.selection.get(app.mode).clone()) {
        app.detail.reset();
        app.sync_contexts();
    }
}

fn apply(app: &mut App, action: Action) {
    // Pure navigation moves the selection only; everything else may change
    // what the grid shows (filters, search, grouping, snapshot, size).
    let navigation = matches!(
        action,
        Action::Move(_)
            | Action::ColumnPageUp
            | Action::ColumnPageDown
            | Action::ColumnTop
            | Action::ColumnBottom
    );
    if navigation {
        app.navigate(&action);
        app.sync_contexts();
        return;
    }
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
            // The grid keeps the selection by ID (refresh_grid, below).
            // EXTENSION POINT (reload issue): keep or clear the link stack (spec §3.4).
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
            app.detail.reset();
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
        Action::CycleGrouping => app.grouping = app.grouping.next(),

        Action::DetailLineUp
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
        | Action::DetailBack => app.detail_action(&action),

        // EXTENSION POINTS, each handled by its own issue:
        // - navigation: OpenDetail, layout switching, OpenEditor
        //   (grid movement: Move, Column* are handled by `App::navigate`)
        // - filters / grouping: CycleGrouping, OpenPicker, ResetFilters, Picker*
        Action::CopyId => match app.copy_text() {
            Some(text) => app.copy_request = Some(text),
            None => app.set_status("Nothing to copy"),
        },
        Action::OpenEditor
        | Action::Move(_)
        | Action::ColumnPageUp
        | Action::ColumnPageDown
        | Action::ColumnTop
        | Action::ColumnBottom
        | Action::OpenDetail
        | Action::OpenPicker(_)
        | Action::ResetFilters
        | Action::PickerMove(_)
        | Action::PickerToggle
        | Action::PickerAll
        | Action::PickerNone
        | Action::PickerApply
        | Action::PickerCancel => {}
    }
    app.refresh_grid();
    app.sync_contexts();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::keymap;
    use crate::tui::model::columns::ColumnKey;
    use crate::tui::testing as fixtures;
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
        let snap = Snapshot::new(vec![fixtures::issue("tz-9", "New").build()], vec![], vec![]);
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
        let a = App::with_snapshot(&args, args.initial_filters().unwrap(), fixtures::snapshot());
        assert_eq!(a.mode, ModeArg::Triage);
        assert_eq!(a.grouping, GroupByArg::Role);
        assert_eq!(a.search.issues, "x");
        assert_eq!(a.search.triage, "");
    }

    fn app_from_flags(flags: &[&str], snapshot: Snapshot) -> App {
        let args = Cli::parse_from(std::iter::once("tz").chain(flags.iter().copied())).args;
        App::with_snapshot(&args, args.initial_filters().unwrap(), snapshot)
    }

    #[test]
    fn mode_triage_flag_starts_in_triage_mode() {
        let a = app_from_flags(&["--mode", "triage"], fixtures::snapshot());
        assert_eq!(a.mode, ModeArg::Triage);
        assert_eq!(a.filters, Filters::default());
    }

    #[test]
    fn group_by_status_with_closed_status_shows_only_closed_issues() {
        let snap = fixtures::snapshot_of([
            fixtures::issue("a", "Open one"),
            fixtures::issue("b", "Closed one").closed(),
            fixtures::issue("c", "Closed two").closed(),
        ]);
        let a = app_from_flags(&["--group-by", "status", "-s", "closed"], snap);
        assert_eq!(a.grouping, GroupByArg::Status);
        let cols = a.grid.columns();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].key, ColumnKey::Status(crate::types::Status::Closed));
        assert_eq!(cols[0].cells, ["b", "c"]);
        assert_eq!(a.counts(), (2, 3));
        assert_eq!(a.selection.issues.as_deref(), Some("b"));
    }

    #[test]
    fn filter_and_search_flags_narrow_the_initial_grid() {
        use crate::types::Role;
        let snap = fixtures::snapshot_of([
            fixtures::issue("a", "Login").role(Role::Agent).priority(0),
            fixtures::issue("b", "Login too").priority(1),
            fixtures::issue("c", "Logout").role(Role::Agent).priority(1),
        ]);
        let a = app_from_flags(
            &[
                "--group-by",
                "priority",
                "-r",
                "agent",
                "-p",
                "0,1",
                "--search",
                "login",
            ],
            snap,
        );
        let ids: Vec<_> = a
            .grid
            .columns()
            .iter()
            .flat_map(|c| c.cells.clone())
            .collect();
        assert_eq!(ids, ["a"]);
    }

    #[test]
    fn invalid_flag_values_are_errors_naming_the_value() {
        let err = Cli::try_parse_from(["tz", "--group-by", "bar"])
            .err()
            .unwrap();
        assert!(err.to_string().contains("'bar'"), "{err}");
        let args = Cli::parse_from(["tz", "-s", "foo"]).args;
        let err = format!("{:#}", args.initial_filters().unwrap_err());
        assert!(err.contains("--status") && err.contains("'foo'"), "{err}");
    }

    #[test]
    fn help_documents_none_and_the_comma_repeat_syntax() {
        use clap::CommandFactory;
        let help = Cli::command().render_long_help().to_string();
        for needle in [
            "none",
            "comma-separated",
            "Repeatable",
            "--effort",
            "--search",
        ] {
            assert!(help.contains(needle), "missing {needle:?} in\n{help}");
        }
    }

    fn selected(a: &App) -> Option<&str> {
        a.selection.issues.as_deref()
    }

    fn grid_app(issues: Vec<fixtures::IssueBuilder>) -> App {
        let mut a = App::from_snapshot(ModeArg::Issues, fixtures::snapshot_of(issues));
        update(&mut a, Action::Resize(100, 40));
        a
    }

    #[test]
    fn keys_move_the_selection_through_the_grid() {
        use crate::types::Role::{Agent, Human};
        let mut a = grid_app(vec![
            fixtures::issue("a", "A").priority(0).role(Agent),
            fixtures::issue("b", "B").priority(1).role(Agent),
            fixtures::issue("c", "C").priority(2).role(Human),
        ]);
        send(&mut a, key(KeyCode::Char('g'))); // status
        send(&mut a, key(KeyCode::Char('g'))); // role: agent [a, b], human [c]
        assert_eq!(selected(&a), Some("a"));
        send(&mut a, key(KeyCode::Right));
        assert_eq!(selected(&a), Some("c"));
        send(&mut a, key(KeyCode::Char('h')));
        assert_eq!(selected(&a), Some("a"));
        send(&mut a, key(KeyCode::Char('j')));
        assert_eq!(selected(&a), Some("b"));
        send(&mut a, key(KeyCode::Char('l')));
        assert_eq!(selected(&a), Some("c"));
        send(&mut a, key(KeyCode::Char('h')));
        send(&mut a, key(KeyCode::Home));
        assert_eq!(selected(&a), Some("a"));
        send(&mut a, key(KeyCode::End));
        assert_eq!(selected(&a), Some("b"));
        send(&mut a, key(KeyCode::PageUp));
        assert_eq!(selected(&a), Some("a"));
        send(&mut a, key(KeyCode::PageDown));
        assert_eq!(selected(&a), Some("b"));
        send(&mut a, key(KeyCode::Char('k')));
        assert_eq!(selected(&a), Some("a"));
    }

    #[test]
    fn search_narrowing_to_zero_then_widening_reselects_first_cell() {
        let mut a = grid_app(vec![
            fixtures::issue("a", "alpha").priority(0),
            fixtures::issue("b", "beta").priority(1),
        ]);
        send(&mut a, key(KeyCode::Down));
        assert_eq!(selected(&a), Some("b"));
        send(&mut a, key(KeyCode::Char('/')));
        type_str(&mut a, "zzz");
        assert_eq!(selected(&a), None);
        assert!(a.grid.columns().is_empty());
        send(&mut a, key(KeyCode::Backspace));
        send(&mut a, key(KeyCode::Backspace));
        send(&mut a, key(KeyCode::Backspace));
        assert_eq!(selected(&a), Some("a"));
    }

    #[test]
    fn typing_in_search_keeps_selection_while_it_matches() {
        let mut a = grid_app(vec![
            fixtures::issue("a", "alpha").priority(0),
            fixtures::issue("b", "beta").priority(1),
        ]);
        send(&mut a, key(KeyCode::Down));
        send(&mut a, key(KeyCode::Char('/')));
        type_str(&mut a, "beta");
        assert_eq!(selected(&a), Some("b"));
    }

    #[test]
    fn cycling_grouping_recomputes_columns_and_keeps_the_item() {
        let mut a = grid_app(vec![
            fixtures::issue("a", "A").role(crate::types::Role::Agent),
            fixtures::issue("b", "B").role(crate::types::Role::Human),
        ]);
        assert_eq!(a.grid.columns().len(), 1);
        send(&mut a, key(KeyCode::Char('j')));
        assert_eq!(selected(&a), Some("b"));
        send(&mut a, key(KeyCode::Char('g'))); // status
        send(&mut a, key(KeyCode::Char('g'))); // role
        assert_eq!(a.grid.columns().len(), 2);
        assert_eq!(selected(&a), Some("b"));
        assert_eq!(a.grid.selected_column(), Some(1));
    }

    #[test]
    fn reload_keeps_selection_by_id_and_handles_removal() {
        let mut a = grid_app(vec![
            fixtures::issue("a", "A").priority(0),
            fixtures::issue("b", "B").priority(1),
            fixtures::issue("c", "C").priority(2),
        ]);
        send(&mut a, key(KeyCode::End));
        assert_eq!(selected(&a), Some("c"));
        let snap = fixtures::snapshot_of(vec![
            fixtures::issue("n", "New").priority(-1),
            fixtures::issue("a", "A").priority(0),
            fixtures::issue("b", "B").priority(1),
            fixtures::issue("c", "C").priority(2),
        ]);
        update(&mut a, Action::Reloaded(Box::new(snap)));
        assert_eq!(selected(&a), Some("c"));
        let snap = fixtures::snapshot_of(vec![
            fixtures::issue("n", "New").priority(-1),
            fixtures::issue("a", "A").priority(0),
        ]);
        update(&mut a, Action::Reloaded(Box::new(snap)));
        assert_eq!(selected(&a), Some("a"));
    }

    #[test]
    fn page_size_follows_the_terminal_size() {
        let issues: Vec<_> = (0..20)
            .map(|i| fixtures::issue(&format!("i{i:02}"), "x").priority(i))
            .collect();
        let mut a = grid_app(issues);
        // 40 rows: 37 main - 1 title = 36 / 7 = 5 cells per screen.
        send(&mut a, key(KeyCode::PageDown));
        assert_eq!(selected(&a), Some("i05"));
        update(&mut a, Action::Resize(100, 24)); // (21 - 1) / 7 = 2
        send(&mut a, key(KeyCode::PageDown));
        assert_eq!(selected(&a), Some("i07"));
        // The selection is still visible after the shrink.
        let off = a
            .grid
            .row_offset(&crate::tui::model::columns::ColumnKey::NoEpic);
        assert!((off..off + 2).contains(&7));
    }

    #[test]
    fn grid_keys_do_nothing_in_triage_mode() {
        let mut a = grid_app(vec![
            fixtures::issue("a", "A").priority(0),
            fixtures::issue("b", "B").priority(1),
        ]);
        send(&mut a, key(KeyCode::Char('m')));
        send(&mut a, key(KeyCode::Down));
        assert_eq!(selected(&a), Some("a"));
    }

    // ---- Detail: scrolling, link highlight and link stack (spec §9.4, §9.6)

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    /// `me` (selected, first) has parent `p`, blocker `b`, child `k` and a
    /// missing blocker `ghost`. Display order: Parent p; Blocked by b, ghost;
    /// Children k. Priorities keep `me` first in the grid.
    fn detail_app() -> App {
        let mut a = grid_app(vec![
            fixtures::issue("me", "Me")
                .priority(0)
                .child_of("p")
                .blocked_by("b")
                .blocked_by("ghost"),
            fixtures::issue("p", "Parent").priority(1),
            fixtures::issue("b", "Blocker").priority(2),
            fixtures::issue("k", "Kid").priority(3).child_of("me"),
        ]);
        assert_eq!(selected(&a), Some("me"));
        a.focus = Focus::Detail;
        a.sync_contexts();
        assert_eq!(a.context(), Context::Detail);
        a
    }

    fn displayed(a: &App) -> Option<&str> {
        a.displayed_issue_id()
    }

    fn tab(a: &mut App) {
        send(a, key(KeyCode::Tab));
    }

    #[test]
    fn tab_and_shift_tab_walk_entries_across_sections() {
        let mut a = detail_app();
        let back = key(KeyCode::BackTab);
        assert_eq!(a.detail.highlight(), None);
        for want in [0, 1, 2, 3, 0] {
            tab(&mut a);
            assert_eq!(a.detail.highlight(), Some(want));
        }
        let rels = a.displayed_relationships();
        assert_eq!(rels.entry(3).unwrap().id, "k");
        send(&mut a, back);
        assert_eq!(a.detail.highlight(), Some(3));
        send(&mut a, back);
        assert_eq!(a.detail.highlight(), Some(2));
        assert!(a.detail.reveal_pending());
    }

    #[test]
    fn enter_pushes_esc_pops_one_at_a_time_and_last_esc_unfocuses() {
        let mut a = detail_app();
        tab(&mut a); // p
        send(&mut a, key(KeyCode::Enter));
        assert_eq!(displayed(&a), Some("p"));
        assert_eq!(a.detail.stack(), ["p"]);
        assert_eq!(a.detail.highlight(), None);
        // `p` has the child `me`: follow it back.
        tab(&mut a);
        send(&mut a, key(KeyCode::Enter));
        assert_eq!(a.detail.stack(), ["p", "me"]);
        // Following never moves the grid selection.
        assert_eq!(selected(&a), Some("me"));
        send(&mut a, key(KeyCode::Esc));
        assert_eq!(displayed(&a), Some("p"));
        assert_eq!(a.focus, Focus::Detail);
        send(&mut a, key(KeyCode::Esc));
        assert_eq!(displayed(&a), Some("me"));
        assert_eq!(a.focus, Focus::Detail);
        send(&mut a, key(KeyCode::Esc));
        assert_eq!(a.focus, Focus::Grid);
        assert_eq!(a.context(), Context::Grid);
    }

    #[test]
    fn final_esc_closes_full_screen_detail() {
        let mut a = detail_app();
        a.focus = Focus::Grid;
        a.full_screen_detail = true;
        a.sync_contexts();
        tab(&mut a);
        send(&mut a, key(KeyCode::Enter));
        send(&mut a, key(KeyCode::Esc));
        assert!(a.full_screen_detail);
        send(&mut a, key(KeyCode::Esc));
        assert!(!a.full_screen_detail);
        assert_eq!(a.context(), Context::Grid);
    }

    #[test]
    fn enter_on_a_missing_target_or_no_highlight_does_nothing() {
        let mut a = detail_app();
        send(&mut a, key(KeyCode::Enter));
        assert!(a.detail.stack().is_empty());
        tab(&mut a);
        tab(&mut a);
        tab(&mut a); // ghost
        assert!(a.displayed_relationships().entry(2).unwrap().is_missing());
        send(&mut a, key(KeyCode::Enter));
        assert!(a.detail.stack().is_empty());
        assert_eq!(a.detail.highlight(), Some(2));
    }

    #[test]
    fn moving_the_grid_selection_clears_the_stack_and_scroll() {
        let mut a = detail_app();
        a.detail.set_metrics(100, 10);
        tab(&mut a);
        send(&mut a, key(KeyCode::Enter));
        send(&mut a, key(KeyCode::PageDown));
        assert!(a.detail.scroll() > 0);
        a.focus = Focus::Grid;
        a.sync_contexts();
        send(&mut a, key(KeyCode::Down));
        assert_eq!(selected(&a), Some("p"));
        assert!(a.detail.stack().is_empty());
        assert_eq!(a.detail.scroll(), 0);
        assert_eq!(displayed(&a), Some("p"));
    }

    #[test]
    fn a_reload_keeps_the_stack() {
        let mut a = detail_app();
        tab(&mut a);
        send(&mut a, key(KeyCode::Enter));
        let snap = a.snapshot.clone();
        update(&mut a, Action::Reloaded(Box::new(snap)));
        assert_eq!(a.detail.stack(), ["p"]);
    }

    #[test]
    fn scroll_keys_clamp_and_ctrl_u_d_move_half_a_page() {
        let mut a = detail_app();
        a.detail.set_metrics(40, 10);
        send(&mut a, key(KeyCode::Up));
        assert_eq!(a.detail.scroll(), 0);
        send(&mut a, key(KeyCode::Char('j')));
        send(&mut a, key(KeyCode::Down));
        assert_eq!(a.detail.scroll(), 2);
        send(&mut a, key(KeyCode::Char('k')));
        assert_eq!(a.detail.scroll(), 1);
        send(&mut a, ctrl('d'));
        assert_eq!(a.detail.scroll(), 6);
        send(&mut a, ctrl('u'));
        assert_eq!(a.detail.scroll(), 1);
        send(&mut a, key(KeyCode::PageDown));
        assert_eq!(a.detail.scroll(), 11);
        send(&mut a, key(KeyCode::Char(' ')));
        assert_eq!(a.detail.scroll(), 21);
        send(&mut a, key(KeyCode::PageDown));
        send(&mut a, key(KeyCode::PageDown));
        assert_eq!(a.detail.scroll(), 30);
        send(&mut a, key(KeyCode::Down));
        assert_eq!(a.detail.scroll(), 30);
        send(&mut a, key(KeyCode::Home));
        assert_eq!(a.detail.scroll(), 0);
        send(&mut a, key(KeyCode::End));
        assert_eq!(a.detail.scroll(), 30);
        send(&mut a, key(KeyCode::PageUp));
        assert_eq!(a.detail.scroll(), 20);
    }

    #[test]
    fn ctrl_d_in_the_split_grid_scrolls_the_detail_without_moving_focus() {
        let mut a = detail_app();
        update(&mut a, Action::Resize(160, 40));
        a.focus = Focus::Grid;
        a.sync_contexts();
        a.detail.set_metrics(40, 10);
        send(&mut a, ctrl('d'));
        assert_eq!(a.detail.scroll(), 5);
        assert_eq!(a.focus, Focus::Grid);
        assert_eq!(selected(&a), Some("me"));
    }

    #[test]
    fn tab_and_enter_are_no_ops_in_triage_mode() {
        let mut a = fixture_app();
        send(&mut a, key(KeyCode::Char('m')));
        a.focus = Focus::Detail;
        a.sync_contexts();
        tab(&mut a);
        send(&mut a, key(KeyCode::Enter));
        assert_eq!(a.detail.highlight(), None);
        assert!(a.detail.stack().is_empty());
        // Direct actions are guarded too.
        update(&mut a, Action::NextLink);
        update(&mut a, Action::FollowLink);
        assert_eq!(a.detail.highlight(), None);
        assert_eq!(a.displayed_issue_id(), None);
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
