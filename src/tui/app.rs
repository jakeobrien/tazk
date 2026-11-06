//! `App` state, the `Action` enum and `fn update(&mut App, Action)`. No rendering and no
//! terminal access, so key sequences are testable without drawing.

use super::{ModeArg, TuiArgs};
use crossterm::event::KeyEvent;

/// Everything that can change the app state. The event loop only translates
/// terminal events (and the periodic timer) into these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Leave the TUI (`q`, `Ctrl-C`).
    Quit,
    /// The terminal was resized to `(columns, rows)`.
    Resize(u16, u16),
    /// The terminal gained focus (reload trigger, spec §3.3).
    FocusGained,
    /// The 10-second timer fired (reload trigger, spec §3.3).
    Tick,
    /// A key press not handled by the global keymap.
    Key(KeyEvent),
}

/// Application state.
#[derive(Debug)]
pub struct App {
    pub mode: ModeArg,
    pub should_quit: bool,
    /// Last known terminal size `(columns, rows)`.
    pub size: (u16, u16),
    /// Set by `FocusGained` / `Tick`; the loader consumes it with [`App::take_reload`].
    pub reload_requested: bool,
}

impl App {
    pub fn new(args: &TuiArgs) -> Self {
        Self {
            mode: args.mode,
            should_quit: false,
            size: (0, 0),
            reload_requested: false,
        }
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
        Action::Key(_) => {}
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
