---
title: 'TUI: triage helpers — filename timestamp and cell preview text (pure)'
status: open
priority: 2
issue_type: task
role: agent
effort: 1
labels:
- tui
dependencies:
  tazk-4: child_of
  tazk-7: blocked_by
created_at: 2026-11-06T03:28:37.372286170+00:00
updated_at: 2026-11-06T03:28:37.472690586+00:00
---

# Description

Spec docs/tui-spec.md §9.5, §10.

- Parse the standard name `<YYYYMMDDTHHMMSS>-<4 chars>.md` (UTC) → local time `YYYY-MM-DD HH:MM`; `None` for other names (reuse any existing parser in `storage/triage.rs` if present).
- Cell preview: raw text (markdown not rendered), whitespace collapsed to single spaces, wrapped to ≤2 lines of cell width, ending in `…` if cut.

# Acceptance Criteria

- Unit tests: standard name parses (with a fixed offset); hand-dropped `notes.md` and malformed timestamps return `None`; preview collapses newlines/tabs/multiple spaces, truncates with `…` only when cut.
