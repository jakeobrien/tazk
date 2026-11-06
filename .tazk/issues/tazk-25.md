---
title: 'TUI: detail state — scrolling, relationship highlight and link stack'
status: closed
priority: 1
issue_type: task
role: agent
effort: 3
labels:
- tui
dependencies:
  tazk-11: blocked_by
  tazk-19: blocked_by
  tazk-24: blocked_by
  tazk-3: child_of
created_at: 2026-11-06T03:28:37.367619753+00:00
updated_at: 2026-11-06T19:06:29.777098857+00:00
closed_at: 2026-11-06T19:06:29.777098857+00:00
---

# Description

Spec docs/tui-spec.md §9.4, §9.6 and the live-follow rule in §8.

- Detail shows the selected grid item unless the link stack is non-empty (then the top of the stack).
- Scroll: `↑/↓`/`j`/`k` line; `PgUp`/`PgDn`/`Space` page; `Ctrl-U`/`Ctrl-D` half page; `Home`/`End`. Clamp to content height (supplied by the renderer from the last layout).
- `Tab`/`Shift-Tab` move a highlight through relationship entries in display order (wrapping or clamping — pick and document); highlight change requests scroll-into-view.
- `Enter` on a followable entry pushes its ID and resets scroll/highlight; missing targets do nothing.
- `Esc`: pop the stack; with empty stack, close full-screen detail or return focus to the grid.
- Moving the grid selection clears the stack and resets detail scroll.
- In triage mode `Tab`/`Enter` are no-ops.

# Acceptance Criteria

- Key-sequence tests: Tab/Shift-Tab order across sections; Enter pushes, Esc pops back one at a time, final Esc closes/unfocuses; Enter on missing does nothing; moving grid selection clears stack; scroll clamps at top/bottom; Ctrl-U/D move half a page.
