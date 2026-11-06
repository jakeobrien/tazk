//! The single keymap definition, used for both input dispatch and the help overlay.
//!
//! This is the only place keys are interpreted: [`resolve`] turns a key event
//! into an [`Action`], and [`bindings_for`] exposes the same tables for the
//! help overlay and hint bar. Number keys are reserved: nothing binds them
//! (they are only accepted as text in the search input).

use super::app::{Action, App, Dir, FilterKind};
use super::ModeArg;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Where input is currently directed. The innermost context decides which
/// bindings are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    /// The grid (issues or triage). Also inherits [`GLOBAL`].
    Grid,
    /// Full-screen detail or the focused detail pane. Also inherits [`GLOBAL`].
    Detail,
    /// Typing in the search input: only search keys (and `Ctrl-C`).
    SearchInput,
    /// The multi-select picker popup (modal).
    Picker,
    /// The help overlay (modal).
    Help,
}

impl Context {
    /// Whether the [`GLOBAL`] table applies in this context (spec §11).
    fn inherits_global(self) -> bool {
        matches!(self, Context::Grid | Context::Detail)
    }
}

/// When a binding is active. Used both to skip dispatch and to hide hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Always,
    /// Issues mode only.
    IssuesMode,
    /// Split view only (terminal width >= 160).
    SplitView,
}

impl When {
    pub fn holds(self, app: &App) -> bool {
        match self {
            When::Always => true,
            When::IssuesMode => app.mode == ModeArg::Issues,
            When::SplitView => app.is_split(),
        }
    }
}

/// A key (with required modifiers) or "any plain character".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySpec {
    Key(KeyCode, KeyModifiers),
    /// Any printable character without Ctrl/Alt (search input text).
    AnyChar,
}

impl KeySpec {
    fn matches(self, key: KeyEvent) -> bool {
        match self {
            KeySpec::AnyChar => {
                matches!(key.code, KeyCode::Char(_))
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            }
            KeySpec::Key(code, mods) => {
                if key.code != code {
                    return false;
                }
                // Shift is already encoded in the character / BackTab.
                let mut got = key.modifiers;
                if matches!(code, KeyCode::Char(_) | KeyCode::BackTab) {
                    got.remove(KeyModifiers::SHIFT);
                }
                got == mods
            }
        }
    }
}

/// How a matched binding produces its [`Action`].
#[derive(Debug, Clone, Copy)]
pub enum Act {
    Fixed(Action),
    /// Built from the typed character ([`KeySpec::AnyChar`]).
    Char(fn(char) -> Action),
}

/// One row of the keymap.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub keys: &'static [KeySpec],
    pub act: Act,
    /// Key text for help / hints, e.g. `"q, Ctrl-C"`.
    pub display: &'static str,
    /// Short description for help / hints.
    pub label: &'static str,
    pub when: When,
}

impl Binding {
    fn matches(&self, key: KeyEvent) -> bool {
        self.keys.iter().any(|k| k.matches(key))
    }

    fn action(&self, key: KeyEvent) -> Action {
        match (self.act, key.code) {
            (Act::Fixed(a), _) => a,
            (Act::Char(f), KeyCode::Char(c)) => f(c),
            // AnyChar only matches Char keys.
            (Act::Char(_), _) => unreachable!("Act::Char bound to a non-character key"),
        }
    }
}

const NONE: KeyModifiers = KeyModifiers::NONE;
#[cfg(test)]
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;

const fn ch(c: char) -> KeySpec {
    KeySpec::Key(KeyCode::Char(c), NONE)
}
const fn ctrl(c: char) -> KeySpec {
    KeySpec::Key(KeyCode::Char(c), CTRL)
}
const fn key(code: KeyCode) -> KeySpec {
    KeySpec::Key(code, NONE)
}

const UP: KeySpec = key(KeyCode::Up);
const DOWN: KeySpec = key(KeyCode::Down);
const LEFT: KeySpec = key(KeyCode::Left);
const RIGHT: KeySpec = key(KeyCode::Right);
const PGUP: KeySpec = key(KeyCode::PageUp);
const PGDN: KeySpec = key(KeyCode::PageDown);
const HOME: KeySpec = key(KeyCode::Home);
const END: KeySpec = key(KeyCode::End);
const ENTER: KeySpec = key(KeyCode::Enter);
const ESC: KeySpec = key(KeyCode::Esc);
const SPACE: KeySpec = ch(' ');

const fn fixed(
    keys: &'static [KeySpec],
    action: Action,
    display: &'static str,
    label: &'static str,
    when: When,
) -> Binding {
    Binding {
        keys,
        act: Act::Fixed(action),
        display,
        label,
        when,
    }
}

/// Spec §11 "Global" (Grid and Detail contexts). `Ctrl-C` is additionally
/// handled by [`resolve`] in every context.
#[rustfmt::skip]
pub const GLOBAL: &[Binding] = &[
    fixed(&[ch('q'), ctrl('c')], Action::Quit, "q, Ctrl-C", "Quit", When::Always),
    fixed(&[ch('?')], Action::OpenHelp, "?", "Help", When::Always),
    fixed(&[ch('R')], Action::Reload, "R", "Reload now", When::Always),
    fixed(&[ch('m')], Action::CycleMode, "m", "Cycle mode (Issues / Triage)", When::Always),
    fixed(&[ch('y')], Action::CopyId, "y", "Copy ID / filename", When::Always),
    fixed(&[ch('o')], Action::OpenEditor, "o", "Open in $EDITOR", When::Always),
];

#[rustfmt::skip]
pub const GRID: &[Binding] = &[
    fixed(&[LEFT, ch('h')], Action::Move(Dir::Left), "←, h", "Move left", When::Always),
    fixed(&[DOWN, ch('j')], Action::Move(Dir::Down), "↓, j", "Move down", When::Always),
    fixed(&[UP, ch('k')], Action::Move(Dir::Up), "↑, k", "Move up", When::Always),
    fixed(&[RIGHT, ch('l')], Action::Move(Dir::Right), "→, l", "Move right", When::Always),
    fixed(&[PGUP], Action::ColumnPageUp, "PgUp", "Page up in column", When::Always),
    fixed(&[PGDN], Action::ColumnPageDown, "PgDn", "Page down in column", When::Always),
    fixed(&[HOME], Action::ColumnTop, "Home", "Top of column", When::Always),
    fixed(&[END], Action::ColumnBottom, "End", "Bottom of column", When::Always),
    fixed(&[ENTER], Action::OpenDetail, "Enter", "Open detail", When::Always),
    fixed(&[ch('/')], Action::StartSearch, "/", "Search", When::Always),
    fixed(&[ESC], Action::ClearSearchFromGrid, "Esc", "Clear search", When::Always),
    fixed(&[ch('g')], Action::CycleGrouping, "g", "Cycle grouping", When::IssuesMode),
    fixed(&[ch('r')], Action::OpenPicker(FilterKind::Role), "r", "Role filter", When::IssuesMode),
    fixed(&[ch('s')], Action::OpenPicker(FilterKind::Status), "s", "Status filter", When::IssuesMode),
    fixed(&[ch('t')], Action::OpenPicker(FilterKind::Type), "t", "Type filter", When::IssuesMode),
    fixed(&[ch('p')], Action::OpenPicker(FilterKind::Priority), "p", "Priority filter", When::IssuesMode),
    fixed(&[ch('e')], Action::OpenPicker(FilterKind::Effort), "e", "Effort filter", When::IssuesMode),
    fixed(&[ch('c')], Action::ResetFilters, "c", "Reset filters", When::IssuesMode),
    fixed(&[ctrl('u')], Action::DetailHalfPageUp, "Ctrl-U", "Scroll detail half page up", When::SplitView),
    fixed(&[ctrl('d')], Action::DetailHalfPageDown, "Ctrl-D", "Scroll detail half page down", When::SplitView),
];

#[rustfmt::skip]
pub const DETAIL: &[Binding] = &[
    fixed(&[UP, ch('k')], Action::DetailLineUp, "↑, k", "Scroll up one line", When::Always),
    fixed(&[DOWN, ch('j')], Action::DetailLineDown, "↓, j", "Scroll down one line", When::Always),
    fixed(&[PGUP], Action::DetailPageUp, "PgUp", "Scroll up one page", When::Always),
    fixed(&[PGDN, SPACE], Action::DetailPageDown, "PgDn, Space", "Scroll down one page", When::Always),
    fixed(&[ctrl('u')], Action::DetailHalfPageUp, "Ctrl-U", "Scroll up half a page", When::Always),
    fixed(&[ctrl('d')], Action::DetailHalfPageDown, "Ctrl-D", "Scroll down half a page", When::Always),
    fixed(&[HOME], Action::DetailTop, "Home", "Top", When::Always),
    fixed(&[END], Action::DetailBottom, "End", "Bottom", When::Always),
    // Triage items have no relationships (spec §9.5).
    fixed(&[key(KeyCode::Tab)], Action::NextLink, "Tab", "Next relationship", When::IssuesMode),
    fixed(&[key(KeyCode::BackTab)], Action::PrevLink, "Shift-Tab", "Previous relationship", When::IssuesMode),
    fixed(&[ENTER], Action::FollowLink, "Enter", "Follow relationship", When::IssuesMode),
    fixed(&[ESC], Action::DetailBack, "Esc", "Back / close", When::Always),
];

#[rustfmt::skip]
pub const SEARCH_INPUT: &[Binding] = &[
    fixed(&[ctrl('k')], Action::SearchClear, "Ctrl-K", "Clear query", When::Always),
    fixed(&[ENTER], Action::SearchKeep, "Enter", "Keep query, back to grid", When::Always),
    fixed(&[ESC], Action::SearchCancel, "Esc", "Clear query, back to grid", When::Always),
    fixed(&[key(KeyCode::Backspace)], Action::SearchBackspace, "Backspace", "Delete character", When::Always),
    Binding {
        keys: &[KeySpec::AnyChar],
        act: Act::Char(Action::SearchChar),
        display: "characters",
        label: "Edit query",
        when: When::Always,
    },
];

#[rustfmt::skip]
pub const PICKER: &[Binding] = &[
    fixed(&[UP, ch('k')], Action::PickerMove(Dir::Up), "↑, k", "Move up", When::Always),
    fixed(&[DOWN, ch('j')], Action::PickerMove(Dir::Down), "↓, j", "Move down", When::Always),
    fixed(&[SPACE], Action::PickerToggle, "Space", "Toggle", When::Always),
    fixed(&[ch('a')], Action::PickerAll, "a", "Select all", When::Always),
    fixed(&[ch('n')], Action::PickerNone, "n", "Select none", When::Always),
    fixed(&[ENTER], Action::PickerApply, "Enter", "Apply", When::Always),
    fixed(&[ESC], Action::PickerCancel, "Esc", "Cancel", When::Always),
];

/// The help overlay swallows everything except these (and `Ctrl-C`).
pub const HELP: &[Binding] = &[fixed(
    &[ch('?'), ESC],
    Action::CloseHelp,
    "?, Esc",
    "Close help",
    When::Always,
)];

/// The tables the help overlay lists, in order, with their titles (spec §11).
pub const HELP_SECTIONS: &[(&str, &[Binding])] = &[
    ("Global", GLOBAL),
    ("Grid", GRID),
    ("Detail", DETAIL),
    ("Search input", SEARCH_INPUT),
    ("Picker", PICKER),
];

/// The context-specific bindings of `context` (not including [`GLOBAL`]),
/// for the help overlay and hint bar. Filter with [`Binding::when`] and
/// [`When::holds`] to hide inapplicable keys.
pub fn bindings_for(context: Context) -> &'static [Binding] {
    match context {
        Context::Grid => GRID,
        Context::Detail => DETAIL,
        Context::SearchInput => SEARCH_INPUT,
        Context::Picker => PICKER,
        Context::Help => HELP,
    }
}

/// Map a key event to an [`Action`] for the innermost context in `stack`.
///
/// - Key releases are ignored (Windows reports them).
/// - `Ctrl-C` quits from every context.
/// - Context bindings take precedence; Grid and Detail then fall back to
///   [`GLOBAL`]. Bindings whose [`When`] doesn't hold are skipped.
pub fn resolve(stack: &[Context], key: KeyEvent, app: &App) -> Option<Action> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.code == KeyCode::Char('c') && key.modifiers == CTRL {
        return Some(Action::Quit);
    }
    let top = stack.last().copied().unwrap_or(Context::Grid);
    let tables: &[&[Binding]] = if top.inherits_global() {
        &[bindings_for(top), GLOBAL]
    } else {
        &[bindings_for(top)]
    };
    tables
        .iter()
        .flat_map(|t| t.iter())
        .find(|b| b.when.holds(app) && b.matches(key))
        .map(|b| b.action(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Context; 5] = [
        Context::Grid,
        Context::Detail,
        Context::SearchInput,
        Context::Picker,
        Context::Help,
    ];

    fn press(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }
    fn c(chr: char) -> KeyEvent {
        press(KeyCode::Char(chr), NONE)
    }
    fn app(mode: ModeArg, width: u16) -> App {
        let mut a = App::with_mode(mode);
        a.size = (width, 40);
        a
    }
    fn res(ctx: Context, k: KeyEvent, a: &App) -> Option<Action> {
        resolve(&[Context::Grid, ctx], k, a)
    }

    #[test]
    fn q_quits_in_grid_but_types_in_search() {
        let a = app(ModeArg::Issues, 100);
        assert_eq!(res(Context::Grid, c('q'), &a), Some(Action::Quit));
        assert_eq!(
            res(Context::SearchInput, c('q'), &a),
            Some(Action::SearchChar('q'))
        );
    }

    #[test]
    fn ctrl_c_quits_in_all_contexts() {
        let a = app(ModeArg::Issues, 100);
        let cc = press(KeyCode::Char('c'), CTRL);
        for ctx in ALL {
            assert_eq!(res(ctx, cc, &a), Some(Action::Quit), "{ctx:?}");
        }
    }

    #[test]
    fn issues_only_keys_resolve_to_nothing_in_triage_mode() {
        let t = app(ModeArg::Triage, 100);
        let i = app(ModeArg::Issues, 100);
        for k in ['g', 'r', 's', 't', 'p', 'e', 'c'] {
            assert_eq!(res(Context::Grid, c(k), &t), None, "{k}");
            assert!(res(Context::Grid, c(k), &i).is_some(), "{k}");
        }
        // Triage detail has no relationships.
        for k in [
            press(KeyCode::Tab, NONE),
            press(KeyCode::BackTab, SHIFT),
            press(KeyCode::Enter, NONE),
        ] {
            assert_eq!(res(Context::Detail, k, &t), None);
            assert!(res(Context::Detail, k, &i).is_some());
        }
    }

    #[test]
    fn ctrl_u_d_in_grid_only_in_split_view() {
        let narrow = app(ModeArg::Issues, 159);
        let wide = app(ModeArg::Issues, 160);
        let u = press(KeyCode::Char('u'), CTRL);
        let d = press(KeyCode::Char('d'), CTRL);
        assert_eq!(res(Context::Grid, u, &narrow), None);
        assert_eq!(res(Context::Grid, d, &narrow), None);
        assert_eq!(res(Context::Grid, u, &wide), Some(Action::DetailHalfPageUp));
        assert_eq!(
            res(Context::Grid, d, &wide),
            Some(Action::DetailHalfPageDown)
        );
        // Always available in detail.
        assert_eq!(
            res(Context::Detail, u, &narrow),
            Some(Action::DetailHalfPageUp)
        );
    }

    #[test]
    fn digits_resolve_to_nothing_outside_search_input() {
        let a = app(ModeArg::Issues, 200);
        for ctx in [
            Context::Grid,
            Context::Detail,
            Context::Picker,
            Context::Help,
        ] {
            for d in '0'..='9' {
                assert_eq!(res(ctx, c(d), &a), None, "{ctx:?} {d}");
            }
        }
    }

    #[test]
    fn global_keys_apply_in_grid_and_detail_only() {
        let a = app(ModeArg::Issues, 100);
        for ctx in [Context::Grid, Context::Detail] {
            assert_eq!(res(ctx, c('?'), &a), Some(Action::OpenHelp));
            assert_eq!(res(ctx, c('R'), &a), Some(Action::Reload));
            assert_eq!(
                res(ctx, press(KeyCode::Char('R'), SHIFT), &a),
                Some(Action::Reload)
            );
            assert_eq!(res(ctx, c('m'), &a), Some(Action::CycleMode));
        }
        for ctx in [Context::SearchInput, Context::Picker, Context::Help] {
            assert_ne!(res(ctx, c('m'), &a), Some(Action::CycleMode), "{ctx:?}");
        }
    }

    #[test]
    fn help_swallows_everything_but_close_and_ctrl_c() {
        let a = app(ModeArg::Issues, 100);
        assert_eq!(res(Context::Help, c('?'), &a), Some(Action::CloseHelp));
        assert_eq!(
            res(Context::Help, press(KeyCode::Esc, NONE), &a),
            Some(Action::CloseHelp)
        );
        assert_eq!(res(Context::Help, c('j'), &a), None);
        assert_eq!(res(Context::Help, c('q'), &a), None);
    }

    #[test]
    fn search_input_keys() {
        let a = app(ModeArg::Issues, 100);
        let r = |k| res(Context::SearchInput, k, &a);
        assert_eq!(
            r(press(KeyCode::Char('k'), CTRL)),
            Some(Action::SearchClear)
        );
        assert_eq!(r(press(KeyCode::Enter, NONE)), Some(Action::SearchKeep));
        assert_eq!(r(press(KeyCode::Esc, NONE)), Some(Action::SearchCancel));
        assert_eq!(
            r(press(KeyCode::Backspace, NONE)),
            Some(Action::SearchBackspace)
        );
        assert_eq!(
            r(press(KeyCode::Char('Q'), SHIFT)),
            Some(Action::SearchChar('Q'))
        );
        assert_eq!(r(c('7')), Some(Action::SearchChar('7')));
        assert_eq!(r(press(KeyCode::Char('x'), CTRL)), None);
        assert_eq!(r(press(KeyCode::Up, NONE)), None);
    }

    #[test]
    fn key_releases_are_ignored() {
        let a = app(ModeArg::Issues, 100);
        let mut k = c('q');
        k.kind = KeyEventKind::Release;
        assert_eq!(res(Context::Grid, k, &a), None);
    }

    #[test]
    fn no_key_is_bound_twice_in_a_context() {
        for ctx in ALL {
            let mut seen: Vec<KeySpec> = Vec::new();
            let tables: Vec<&[Binding]> = if ctx.inherits_global() {
                vec![bindings_for(ctx), GLOBAL]
            } else {
                vec![bindings_for(ctx)]
            };
            for b in tables.into_iter().flatten() {
                for k in b.keys {
                    assert!(!seen.contains(k), "{ctx:?}: {k:?} bound twice");
                    seen.push(*k);
                }
            }
        }
    }

    #[test]
    fn no_digit_is_bound() {
        for ctx in ALL {
            for b in bindings_for(ctx).iter().chain(GLOBAL) {
                for k in b.keys {
                    if let KeySpec::Key(KeyCode::Char(chr), _) = k {
                        assert!(!chr.is_ascii_digit(), "{ctx:?}: digit bound");
                    }
                }
            }
        }
    }

    #[test]
    fn every_binding_has_help_text() {
        for ctx in ALL {
            for b in bindings_for(ctx).iter().chain(GLOBAL) {
                assert!(!b.display.is_empty() && !b.label.is_empty());
            }
        }
    }
}
