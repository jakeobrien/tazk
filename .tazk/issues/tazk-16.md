---
title: 'TUI: column building for all groupings (pure)'
status: open
priority: 1
issue_type: task
role: agent
effort: 4
labels:
- tui
dependencies:
  tazk-14: blocked_by
  tazk-15: blocked_by
  tazk-2: child_of
created_at: 2026-11-06T03:28:37.354754628+00:00
updated_at: 2026-11-06T06:09:23.330691338+00:00
---

# Description

Spec docs/tui-spec.md §5.1–§5.3. Produce `Vec<Column>` from the snapshot + visible set + grouping.

`Column { key, title, count, header: Option<Header { epic_id, dimmed }>, cells: Vec<IssueId> }`.

**Epic grouping** (§5.2):
1. Parent = `child_of` target; with several, the lowest target ID in stored (sorted) order.
2. Parent exists and is type `epic` → that epic's column; otherwise (no parent / missing / non-epic) → "No epic".
3. Every visible epic gets its own column even with no visible children.
4. Header looked up from the **full** set; dimmed when the epic fails filters/search (e.g. closed). Columns with visible children always show their header.
5. A visible epic that is a child of another epic appears as a cell in the parent column **and** as its own column header. No nesting.
6. Non-epic parents get no column.
7. Order: "No epic" first, then epics by priority asc, `updated_at` newest, ID.

**Other groupings** (§5.3): status (open, in_progress, blocked, deferred, closed), role (agent, human, (none)), type (epic, spec, feature, bug, task, gate, chore), priority (P0..P4 then other values numerically).

All groupings: empty columns hidden; cells sorted with `cell_order`; `count` = number of cells (header excluded). `g` cycles epic→status→role→type→priority→epic.

# Acceptance Criteria

- Unit tests for epic grouping: multi-parent picks lowest ID; missing parent → No epic; non-epic parent → No epic; closed epic with open children → dimmed header; visible epic with no visible children → header-only column; nested epic appears both as a cell and a column header; column order by priority/updated/ID; "No epic" omitted when empty.
- Unit tests for status/role/type/priority groupings: value order, `(none)` last, out-of-range priorities after P4 in numeric order, empty columns hidden (closed column appears only when status filter includes closed).
- Grouping cycle order test.

- Unit test for type grouping with an issue of each of the seven types: columns appear in the order epic, spec, feature, bug, task, gate, chore.
