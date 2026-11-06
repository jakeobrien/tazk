---
title: 'TUI: copy ID / triage filename to clipboard (`y`)'
status: closed
priority: 3
issue_type: task
role: agent
effort: 2
labels:
- tui
dependencies:
  tazk-11: blocked_by
  tazk-21: blocked_by
  tazk-25: blocked_by
  tazk-5: child_of
created_at: 2026-11-06T03:28:37.377305128+00:00
updated_at: 2026-11-06T19:28:44.483802385+00:00
closed_at: 2026-11-06T19:28:44.483802385+00:00
---

# Description

Spec docs/tui-spec.md §11 global `y`, §13.

- Copies the selected issue ID in the grid, or the displayed issue (top of link stack) when the detail has focus / is full-screen; in triage mode the filename.
- Implementation: an optional clipboard crate under the `tui` feature (e.g. `arboard`) with OSC 52 as a fallback for SSH/tmux is acceptable; document the choice.
- On success show a short confirmation, on failure a short error in the hint bar. Never panic.

# Acceptance Criteria

- Unit test of the 'what to copy' resolution for grid, split-detail-focused, full-screen with link stack, and triage.
- Clipboard backend behind a trait so tests use a fake; failure path shows a hint-bar message (test).
- `cargo build --no-default-features` does not pull the clipboard crate.
