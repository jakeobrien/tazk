---
title: 'TUI: end-to-end key-sequence integration tests'
status: closed
priority: 2
issue_type: task
role: agent
effort: 2
labels:
- tui
dependencies:
  tazk-22: blocked_by
  tazk-23: blocked_by
  tazk-28: blocked_by
  tazk-31: blocked_by
  tazk-32: blocked_by
  tazk-35: blocked_by
  tazk-6: child_of
created_at: 2026-11-06T03:28:37.382331878+00:00
updated_at: 2026-11-08T19:11:11.621641901+00:00
closed_at: 2026-11-08T19:11:11.621641901+00:00
---

# Description

Spec docs/tui-spec.md §14. Build a fixture `.tazk` directory (temp dir) with epics, nested epic, multi-parent, closed epic, blockers (open/closed/missing), triage items and one bad file; drive the full `App` with key sequences and render to `TestBackend` at 120×40 and 200×50.

# Acceptance Criteria

- Scenarios covered: launch → navigate → open detail → Tab/Enter follow → Esc back; filter via picker then `c`; search in both modes; mode switch; regroup through all groupings; resize across 160 cols with detail open; reload after modifying a file on disk preserves selection.
- Snapshots committed; tests pass in CI and are skipped without the `tui` feature.

# Notes

Closed 2026-11-08T19:11:11Z: Implemented in src/tui/e2e_tests.rs
