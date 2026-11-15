---
title: 'TUI: refresh, global actions and help'
status: closed
priority: 2
issue_type: epic
role: agent
effort: 4
labels:
- tui
created_at: 2026-11-06T03:28:37.337710503+00:00
updated_at: 2026-11-15T16:33:18.950831+00:00
closed_at: 2026-11-15T16:33:18.950831+00:00
---

# Description

Background reload with state preservation, clipboard copy, `$EDITOR` hand-off and the help overlay (docs/tui-spec.md §3.3, §3.4, §11 global keys, §13).

# Acceptance Criteria

- All child issues closed.
- Reload (focus/10 s/`R`), `y`, `o` and `?` behave as in the spec and never crash the TUI.
