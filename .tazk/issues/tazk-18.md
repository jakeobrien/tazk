---
title: 'TUI: issue cell widget (32×7)'
status: open
priority: 1
issue_type: task
role: agent
effort: 3
labels:
- tui
dependencies:
  tazk-12: blocked_by
  tazk-15: blocked_by
  tazk-17: blocked_by
  tazk-2: child_of
created_at: 2026-11-06T03:28:37.357610253+00:00
updated_at: 2026-11-06T06:09:23.347872921+00:00
---

# Description

Spec docs/tui-spec.md §5.4.

- Fixed 32 cols × 7 rows including border; border color = type color.
- Top border: ID on the left; `⊘` (orange) on the right when the issue has open blockers.
- Bottom border: type name on the right in the type color.
- Title: bold, word-wrapped to ≤3 lines, ending in `…` if cut (handle single words longer than the width).
- Badges row: role, status, priority, effort (from the theme).
- Selected: thick or double border (still type-colored) + highlighted ID.
- Dimmed variant for epic headers that fail filters: all colors dimmed, status badge visible.
- Assignee and labels are not shown.

# Acceptance Criteria

- Snapshot tests (TestBackend): normal, selected, dimmed, with-blockers, long title truncated with `…`, very long single word, unset role/effort.
- Wrap/truncate helper has unit tests (≤3 lines, ellipsis only when cut, exact-fit title has no ellipsis).
- Rendered width never exceeds 32 columns, including with wide IDs (e.g. sharded/hash IDs) — ID is truncated if necessary.

- Snapshot tests include a `spec` cell and a `gate` cell (type name and color in the bottom border).
