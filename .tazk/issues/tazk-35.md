---
title: 'TUI: help overlay generated from the keymap, with skipped-file details'
status: open
priority: 2
issue_type: task
role: agent
effort: 2
labels:
- tui
dependencies:
  tazk-10: blocked_by
  tazk-11: blocked_by
  tazk-12: blocked_by
  tazk-5: child_of
  tazk-8: blocked_by
created_at: 2026-11-06T03:28:37.379781253+00:00
updated_at: 2026-11-06T03:28:37.516884920+00:00
---

# Description

Spec docs/tui-spec.md §3.2 and §11.

- `?` opens a centered overlay listing the binding tables (Global, Grid, Detail, Search input, Picker) generated from the keymap — no hand-written key text.
- When files were skipped, show the count plus the first skipped path and its error.
- `?` or `Esc` closes; scrollable if it doesn't fit.

# Acceptance Criteria

- Snapshot test of the help overlay with and without skipped files.
- A test asserts every keymap binding label appears in the rendered help (guards against drift).
- `?`/`Esc` close it; other keys are swallowed while open (except `Ctrl-C`).
