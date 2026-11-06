//! The single keymap definition, used for both input dispatch and the help overlay.

use super::app::Action;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Map a key event to an [`Action`]. Releases are ignored (Windows reports them).
pub fn key_action(key: KeyEvent) -> Option<Action> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    Some(match key.code {
        KeyCode::Char('c') if ctrl => Action::Quit,
        KeyCode::Char('q') if !ctrl => Action::Quit,
        _ => Action::Key(key),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q_and_ctrl_c_quit() {
        let q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        let c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_action(q), Some(Action::Quit));
        assert_eq!(key_action(c), Some(Action::Quit));
    }

    #[test]
    fn other_keys_pass_through_and_releases_are_ignored() {
        let x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(key_action(x), Some(Action::Key(x)));
        let mut rel = x;
        rel.kind = KeyEventKind::Release;
        assert_eq!(key_action(rel), None);
    }
}
