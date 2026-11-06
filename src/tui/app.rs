//! `App` state, the `Action` enum and `fn update(&mut App, Action)`. No rendering and no
//! terminal access, so key sequences are testable without drawing.

use super::keymap::Context;
use super::ui::layout;
use super::{ModeArg, TuiArgs};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    // Events
    /// The terminal was resized to `(columns, rows)`.
    Resize(u16, u16),
    /// The terminal gained focus (reload trigger, spec §3.3).
    FocusGained,
    /// The 10-second timer fired (reload trigger, spec §3.3).
    Tick,

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

/// Application state.
#[derive(Debug)]
pub struct App {
    pub mode: ModeArg,
    /// Input contexts, innermost last. Never empty (starts as `[Grid]`).
    pub contexts: Vec<Context>,
    pub should_quit: bool,
    /// Last known terminal size `(columns, rows)`.
    pub size: (u16, u16),
    /// Set by `FocusGained` / `Tick`; the loader consumes it with [`App::take_reload`].
    pub reload_requested: bool,
}

impl App {
    pub fn new(args: &TuiArgs) -> Self {
        Self::with_mode(args.mode)
    }

    pub fn with_mode(mode: ModeArg) -> Self {
        Self {
            mode,
            contexts: vec![Context::Grid],
            should_quit: false,
            size: (0, 0),
            reload_requested: false,
        }
    }

    /// Whether the split (grid + detail pane) layout is active (spec §8).
    pub fn is_split(&self) -> bool {
        layout::is_split(self.size.0)
    }

    /// The innermost input context.
    pub fn context(&self) -> Context {
        self.contexts.last().copied().unwrap_or(Context::Grid)
    }

    /// Returns and clears the pending reload request.
    pub fn take_reload(&mut self) -> bool {
        std::mem::take(&mut self.reload_requested)
    }
}

/// Apply an action to the state.
pub fn update(app: &mut App, action: Action) {
    match action {
        Action::Quit => app.should_quit = true,
        Action::Resize(w, h) => app.size = (w, h),
        Action::FocusGained | Action::Tick => app.reload_requested = true,
        // Handled by later issues.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: TuiArgs,
    }

    fn app() -> App {
        App::new(&Cli::parse_from(["tz"]).args)
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
}
