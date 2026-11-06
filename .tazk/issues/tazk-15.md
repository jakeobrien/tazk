---
title: 'TUI: open-blockers detection and cell sort order (pure)'
status: open
priority: 1
issue_type: task
role: agent
effort: 1
labels:
- tui
dependencies:
  tazk-2: child_of
  tazk-7: blocked_by
  tazk-8: blocked_by
created_at: 2026-11-06T03:28:37.353485128+00:00
updated_at: 2026-11-06T03:28:37.406580711+00:00
---

# Description

Spec docs/tui-spec.md §5.3 (cell order) and §5.4 (open-blockers marker).

- `has_open_blockers(issue, snapshot)` reusing the storage helper exposed in the tolerant-loader issue: at least one `blocked_by` target that is not closed; a **missing** target counts as unresolved. Independent of `status: blocked`.
- `cell_order(a, b)`: priority ascending (0 first), then `updated_at` newest first, then ID.

# Acceptance Criteria

- Unit tests: open target → true; closed target → false; missing target → true; `related_to` only → false; `status: blocked` with no deps → false.
- Sort test covering ties at each level (priority, updated_at, ID).
- Logic matches `tz ready` (shared helper, not a copy).
