---
title: 'TUI: terminal lifecycle, panic-safe restore and event loop'
status: closed
priority: 1
issue_type: task
role: agent
effort: 3
labels:
- tui
dependencies:
  tz-1: child_of
  tz-7: blocked_by
created_at: 2026-11-06T03:28:37.344627003+00:00
updated_at: 2026-11-06T14:42:07.544377901+00:00
closed_at: 2026-11-06T14:42:07.544377901+00:00
---

# Description

Spec docs/tui-spec.md §3.3, §4 ('too small'), §11 (`Ctrl-C`), §13.

- Enter raw mode + alternate screen, enable `EnableFocusChange`; on exit disable all of these and show the cursor.
- Install a panic hook that restores the terminal **before** the default hook prints the panic.
- Event loop: multiplexes crossterm events (key, resize, `FocusGained`) and a 10-second tick into `Action`s, calls `app::update`, redraws.
- Provide `suspend()` / `resume()` helpers (leave/re-enter alt screen + raw mode) for the `$EDITOR` issue.
- 'Terminal too small': if the area can't fit one 32-col column plus the 3 bar rows (mode, filter, hint) and one 7-row cell, draw only a centered `Terminal too small` message.

# Design

Keep the loop thin: translate events → `Action`, everything else lives in `app.rs`. Use `crossterm::event::poll` with a timeout that wakes for the tick, or a channel fed by an input thread + timer; the reload issue will add a third channel source (loader results).

# Acceptance Criteria

- `tz tui` enters the alternate screen; `q` and `Ctrl-C` exit and the shell prompt/terminal modes are fully restored.
- A deliberate panic (test-only hook or debug env var) leaves the terminal usable and prints the panic message after restore.
- `FocusGained`, resize and a 10 s tick each produce a distinct `Action` (unit-tested via the event→action translation function).
- Snapshot test: a `TestBackend` smaller than 32+chrome columns or 3+7 rows renders the centered 'Terminal too small' message and nothing else.
- `suspend()`/`resume()` exist and are covered by a doc comment explaining their use.
