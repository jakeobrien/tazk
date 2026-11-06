---
title: 'TUI: issues grid selection, navigation and scroll-follow (state)'
status: closed
priority: 1
issue_type: task
role: agent
effort: 3
labels:
- tui
dependencies:
  tazk-11: blocked_by
  tazk-16: blocked_by
  tazk-2: child_of
created_at: 2026-11-06T03:28:37.359141920+00:00
updated_at: 2026-11-06T17:45:58.631354238+00:00
closed_at: 2026-11-06T17:45:58.631354238+00:00
---

# Description

Spec docs/tui-spec.md §5.5 and §5.6, as pure state logic on `App` driven by the column model.

- Selection stored by issue ID (plus column key/row index for fallback).
- `←/→`/`h`/`l`: move between columns keeping the row as close as possible; `↑/↓`/`j`/`k` within a column (header cell is row 0 for epic columns).
- `PgUp/PgDn`: one screen of cells within the column (cells-per-screen comes from the last layout size). `Home/End`: column top/bottom.
- Scroll offsets: horizontal (first visible column) and per-column vertical, updated so the selected cell is always fully visible.
- Selection-disappears rule after filter/search/regroup/reload: same column → same row index or last cell; column gone → first cell of first column; nothing visible → `None` (renders 'No matching issues').
- Recompute columns whenever filters, search, grouping or snapshot change.

# Acceptance Criteria

- Unit tests with fixed viewport sizes: left/right across columns of different lengths clamps the row; PgDn/PgUp/Home/End; scroll offset keeps the selected cell fully visible horizontally and vertically.
- Unit tests for each branch of the selection-disappears rule, including regrouping and search narrowing to zero results then widening again.
- Selecting an epic header is possible and counts as selecting that epic.
