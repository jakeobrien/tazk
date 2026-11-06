---
title: 'TUI: issues grid rendering (columns, title bars, headers, scrolling)'
status: closed
priority: 1
issue_type: task
role: agent
effort: 3
labels:
- tui
dependencies:
  tazk-18: blocked_by
  tazk-19: blocked_by
  tazk-2: child_of
created_at: 2026-11-06T03:28:37.361148211+00:00
updated_at: 2026-11-06T18:04:04.225308673+00:00
closed_at: 2026-11-06T18:04:04.225308673+00:00
---

# Description

Spec docs/tui-spec.md §4 main area, §5.1–§5.5.

- Lay out as many 32-col columns as fit, starting at the horizontal scroll offset.
- Column title bar: group value (status/role/type/priority label, `(none)`) or, for epic columns, the epic's ID and title (truncated), plus the cell count. "No epic" has a plain title and no header cell.
- Epic header cell at the top of epic columns (dimmed variant when applicable).
- Cells from the per-column vertical offset; no scrollbars.
- 'No matching issues' centered when nothing is visible.

# Acceptance Criteria

- Snapshot tests: grid in each of the five groupings with a shared fixture; epic grouping with a dimmed closed epic header and a nested epic; horizontally scrolled grid with selection in a far column; vertically scrolled column; 'No matching issues'.
- Selected cell is always fully on screen in snapshot tests at several sizes.
