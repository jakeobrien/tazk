---
title: 'TUI: App state core, Action enum and update loop'
status: closed
priority: 1
issue_type: task
role: agent
effort: 3
labels:
- tui
dependencies:
  tazk-1: child_of
  tazk-10: blocked_by
  tazk-7: blocked_by
created_at: 2026-11-06T03:28:37.347693461+00:00
updated_at: 2026-11-06T15:59:18.961360360+00:00
closed_at: 2026-11-06T15:59:18.961360360+00:00
---

# Description

Central state machine (spec docs/tui-spec.md §14: state and update separate from rendering).

- `Snapshot` in `data.rs`: `issues: Vec<Issue>`, `by_id: HashMap<String, usize>`, `triage: Vec<TriageItem>`, `skipped: Vec<SkippedFile>`.
- `App`: snapshot, mode (Issues/Triage), grouping, per-mode search query, search-input active flag, focus (Grid/Detail), layout (Narrow/Split, derived from width ≥ 160), full-screen-detail flag, overlay (None/Help/Picker), transient status message, quit flag, per-mode selection placeholder.
- `Action` enum covering every binding in §11 plus `Resize`, `Tick`, `FocusGained`, `Reloaded(Snapshot)`.
- `update(&mut App, Action)` handling the generic parts: quit, mode cycle `m`, help open/close, search-input enter/exit (Enter keeps, Esc clears, Ctrl-K clears, Backspace/char edit), Esc in grid clears active search.
- Feature-specific transitions (navigation, picker, link stack, layout switching) are added by their own issues; leave clearly named extension points.

# Acceptance Criteria

- Unit tests drive `update` with key sequences (no terminal) for: `m` toggles mode; `/` + typing + `Enter` keeps query; `/` + typing + `Esc` clears; `Ctrl-K` clears; each mode keeps its own query; `?` then `Esc` closes help; `q` sets quit.
- `App` can be built from a `Snapshot` constructed in tests (fixture builder) without filesystem access.
- No ratatui types are referenced from `app.rs` except `Rect`/size if needed.
