---
title: 'TUI: relationships model for the detail view (pure)'
status: closed
priority: 1
issue_type: task
role: agent
effort: 2
labels:
- tui
dependencies:
  tazk-3: child_of
  tazk-7: blocked_by
  tazk-8: blocked_by
created_at: 2026-11-06T03:28:37.366319336+00:00
updated_at: 2026-11-06T17:50:24.390130948+00:00
closed_at: 2026-11-06T17:50:24.390130948+00:00
---

# Description

Spec docs/tui-spec.md §9.2. From an issue + snapshot build ordered sections, each omitted when empty:

| Section | Source |
|---|---|
| Parent | outgoing `child_of` (all) |
| Blocked by | outgoing `blocked_by` |
| Children | reverse `child_of` |
| Blocks | reverse `blocked_by` |
| Related | outgoing + reverse `related_to` (deduplicated) |
| Discovered from | outgoing `discovered_from` |
| Discoveries | reverse `discovered_from` |
| Tracks | outgoing `tracks` |
| Tracked by | reverse `tracks` |

Entries sorted by ID; each carries target ID, status (if present) and title, or `missing`. Also a flat list of entries in display order for `Tab` navigation, with a `followable` flag (false for missing).

# Acceptance Criteria

- Unit tests: each section populated from a fixture; empty sections omitted; sort by ID; missing target marked and not followable; `related_to` in both directions appears once; the legacy `discovered-from` spelling (normalised by the parser) lands in the right section.
