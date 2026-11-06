---
title: 'TUI: filter and search model (pure)'
status: open
priority: 1
issue_type: task
role: agent
effort: 3
labels:
- tui
dependencies:
  tazk-2: child_of
  tazk-7: blocked_by
created_at: 2026-11-06T03:28:37.352120378+00:00
updated_at: 2026-11-06T06:09:23.317955588+00:00
---

# Description

Spec docs/tui-spec.md §6 and §7 (matching only; UI is elsewhere).

- `FilterSet` per property (role, status, type, priority, effort): set of allowed values. Role and effort include a `None` value. Priority values = P0–P4 plus any other values present in the data.
- Default: everything allowed except status `closed`. `reset()` restores the default (`c`).
- `Filters::matches(&Issue)` — AND across properties.
- Summary string for the filter bar: `all` when every value selected, `!v` when exactly one excluded, otherwise comma list of selected values (empty set → empty list representation, e.g. `none selected`; pick and document).
- Search: plain, case-insensitive substring. Issues: ID, title, and all four body sections. Triage: full text. Combined with filters via AND.
- `visible_issues(snapshot, filters, query)` helper.

# Design

Type values are all seven issue types, in spec order (docs/tui-spec.md §1 "Issue types"): epic, spec, feature, bug, task, gate, chore. Derive the list from an exhaustive `match` on `IssueType` (no wildcard arm) so adding a type later is a compile error here.

# Acceptance Criteria

- Unit tests: default filter hides closed only; `none` role/effort matching; priority value list includes an out-of-range value (e.g. 7) present in data; summary strings for all/one-excluded/list cases (e.g. default status shows `!closed`).
- Search tests: case-insensitive match in title, ID, each of Description/Design/Acceptance Criteria/Notes; non-match; triage full-text match.
- Filter + search combine with AND (test).

- Unit test: the type filter value list is exactly `epic, spec, feature, bug, task, gate, chore`, in that order, and filtering on `gate` / `spec` works like any other type.
