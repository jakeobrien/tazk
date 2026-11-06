//! Terminal setup/teardown, panic hook and the event loop.
//!
//! The loop is deliberately thin: it translates events into [`Action`]s,
//! calls [`app::update`] and redraws. All behavior lives in `app.rs`.

use super::app::{self, Action, App};
use super::{keymap, ui};
use anyhow::Result;
use crossterm::event::{self, DisableFocusChange, EnableFocusChange, Event};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Interval between automatic reloads (spec §3.3).
pub const TICK_INTERVAL: Duration = Duration::from_secs(10);

/// True while raw mode / the alternate screen are active. Makes [`restore`]
/// idempotent so the panic hook, `Drop` and normal exit can all call it.
static ACTIVE: AtomicBool = AtomicBool::new(false);

fn enter() -> Result<()> {
    enable_raw_mode()?;
    ACTIVE.store(true, Ordering::SeqCst);
    if let Err(e) = execute!(io::stdout(), EnterAlternateScreen, EnableFocusChange) {
        restore();
        return Err(e.into());
    }
    Ok(())
}

/// Undo everything [`enter`] did: disable focus reporting, leave the alternate
/// screen, show the cursor and leave raw mode. Safe to call repeatedly and
/// never fails (best effort), so it can run from a panic hook.
pub fn restore() {
    if ACTIVE.swap(false, Ordering::SeqCst) {
        let _ = execute!(
            io::stdout(),
            DisableFocusChange,
            LeaveAlternateScreen,
            cursor::Show
        );
        let _ = disable_raw_mode();
    }
}

/// Install a panic hook that restores the terminal **before** the previous
/// (default) hook prints the panic message, so the message stays visible.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}

/// Restores the terminal when dropped.
struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        restore();
    }
}

/// Temporarily leave the TUI so a child process (e.g. `$EDITOR`) can use the
/// terminal. Call [`resume`] afterwards. Leaves the alternate screen and raw
/// mode exactly as on exit.
pub fn suspend() {
    restore();
}

/// Re-enter raw mode and the alternate screen after [`suspend`] and force a
/// full redraw on the next frame.
pub fn resume(terminal: &mut Tui) -> Result<()> {
    enter()?;
    terminal.clear()?;
    Ok(())
}

/// Translate a terminal event into an [`Action`]. `None` for events we ignore.
pub fn translate(event: Event) -> Option<Action> {
    match event {
        Event::Key(key) => keymap::key_action(key),
        Event::Resize(w, h) => Some(Action::Resize(w, h)),
        Event::FocusGained => Some(Action::FocusGained),
        _ => None,
    }
}

/// Set up the terminal, run the event loop until quit, and restore the terminal.
pub fn run(mut app: App) -> Result<()> {
    install_panic_hook();
    enter()?;
    let _guard = Guard;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let size = terminal.size()?;
    app::update(&mut app, Action::Resize(size.width, size.height));
    event_loop(&mut terminal, &mut app)
}

fn event_loop(terminal: &mut Tui, app: &mut App) -> Result<()> {
    let mut next_tick = Instant::now() + TICK_INTERVAL;
    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, app))?;
        debug_panic_hook();

        let timeout = next_tick.saturating_duration_since(Instant::now());
        if event::poll(timeout)? {
            if let Some(action) = translate(event::read()?) {
                app::update(app, action);
            }
        }
        if Instant::now() >= next_tick {
            app::update(app, Action::Tick);
            next_tick = Instant::now() + TICK_INTERVAL;
        }
    }
    Ok(())
}

/// Debug builds only: `TAZK_TUI_PANIC=1` panics after the first draw, to
/// verify the terminal is restored and the message is printed.
fn debug_panic_hook() {
    #[cfg(debug_assertions)]
    if std::env::var_os("TAZK_TUI_PANIC").is_some() {
        panic!("deliberate TUI panic (TAZK_TUI_PANIC)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn events_translate_to_distinct_actions() {
        assert_eq!(translate(Event::FocusGained), Some(Action::FocusGained));
        assert_eq!(
            translate(Event::Resize(90, 30)),
            Some(Action::Resize(90, 30))
        );
        let q = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert_eq!(translate(q), Some(Action::Quit));
        assert_eq!(translate(Event::FocusLost), None);
        // Tick is produced by the loop timer, not an event; it is distinct.
        assert_ne!(Action::Tick, Action::FocusGained);
        assert_ne!(Action::Tick, Action::Resize(0, 0));
    }

    #[test]
    fn restore_is_idempotent_when_inactive() {
        restore();
        restore();
    }
}
