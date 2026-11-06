---
title: 'TUI: single keymap definition shared by input dispatch and help'
status: open
priority: 1
issue_type: task
role: agent
effort: 2
labels:
- tui
dependencies:
  tazk-1: child_of
  tazk-7: blocked_by
created_at: 2026-11-06T03:28:37.346194503+00:00
updated_at: 2026-11-06T03:28:37.387657586+00:00
---

# Description

Spec docs/tui-spec.md §11 (all tables) and the requirement that the help overlay is generated from the same keymap the input handler uses.

- Contexts: Global, Grid, Detail, SearchInput, Picker, Help.
- Each binding: key(s) (with modifiers), `Action`, short help label, and an applicability predicate (e.g. issues-mode only, split view only) used both to skip dispatch and to hide hints (§4 hint bar).
- `Ctrl-C` quits from every context; in SearchInput only search-input keys are active.
- Number keys are reserved: they must not be bound and are ignored.
- `fn resolve(context_stack, KeyEvent, &App) -> Option<Action>` and `fn bindings_for(context) -> &[Binding]` for help/hints.

# Acceptance Criteria

- Every key in spec §11 appears exactly once per context in the keymap table.
- Unit tests: `q` quits from grid but types `q` in SearchInput; `Ctrl-C` quits in all contexts; `g`/`r`/`s`/`t`/`p`/`e`/`c` resolve to nothing in triage mode; `Ctrl-U`/`Ctrl-D` in grid resolve only in split view; digits resolve to nothing.
- No key handling exists outside `keymap::resolve` (grep check in review).
