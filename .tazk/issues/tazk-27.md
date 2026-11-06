---
title: 'TUI: issue detail rendering (title, properties, relationships, body)'
status: open
priority: 1
issue_type: task
role: agent
effort: 4
labels:
- tui
dependencies:
  tazk-17: blocked_by
  tazk-24: blocked_by
  tazk-25: blocked_by
  tazk-26: blocked_by
  tazk-3: child_of
created_at: 2026-11-06T03:28:37.370007003+00:00
updated_at: 2026-11-06T03:28:37.465763628+00:00
---

# Description

Spec docs/tui-spec.md §9.1–§9.2 rendering.

1. Title, bold, wrapped.
2. Properties as label/value pairs: ID, Type, Status, Priority, Role, Effort, Assignee, Labels, External ref, Created, Updated, Closed. Colors/badges match the cell; unset → dim `—`; Closed only when set; timestamps local time, absolute, to the minute (`2025-11-04 14:32`); open-blockers marker repeated if it applies.
3. Relationships: section headings, each entry = ID, status badge, title truncated to one line; missing → ID + `(missing)`; the highlighted entry is visibly marked.
4. Body from the markdown renderer.

One scrollable pane: the whole composition is a single scrolled `Text`/line list honoring the detail scroll offset; report total height and the line index of each relationship entry (for scroll-into-view).

# Acceptance Criteria

- Snapshot tests: full issue with every property and every relationship section; minimal issue (unset values dim `—`, no Closed row, `(no description)`); issue with open blockers shows marker; highlighted relationship entry; missing target entry; scrolled-down view.
- Timestamp formatting unit test with a fixed offset.
- Tab to an entry below the fold scrolls it into view (state + render test).
