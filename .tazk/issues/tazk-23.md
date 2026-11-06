---
title: 'TUI: wire grouping cycle, filter reset and live search in issues mode'
status: closed
priority: 2
issue_type: task
role: agent
effort: 2
labels:
- tui
dependencies:
  tazk-19: blocked_by
  tazk-2: child_of
  tazk-21: blocked_by
created_at: 2026-11-06T03:28:37.364953670+00:00
updated_at: 2026-11-06T20:27:24.627174+00:00
closed_at: 2026-11-06T20:27:24.627174+00:00
---

# Description

Spec docs/tui-spec.md §5.1 (`g`), §6 (`c`), §7 (live search).

- `g` cycles grouping and re-applies the selection rule (selection stays on the same issue if still visible).
- `c` resets all filters to default.
- Search input filters live on every keystroke; `Esc` in the grid (detail not focused) clears an active search.

# Acceptance Criteria

- Key-sequence tests: `g` ×5 returns to epic grouping and keeps the selected issue selected throughout when visible; `c` after narrowing filters restores default; typing in search narrows the grid after each character; `Esc` in grid clears search but `Esc` with detail pane focused does not.
