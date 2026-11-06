---
title: 'TUI: open selected/displayed file in `$EDITOR` (`o`)'
status: open
priority: 2
issue_type: task
role: agent
effort: 2
labels:
- tui
dependencies:
  tazk-21: blocked_by
  tazk-32: blocked_by
  tazk-5: child_of
  tazk-9: blocked_by
created_at: 2026-11-06T03:28:37.378493545+00:00
updated_at: 2026-11-06T03:28:37.509741545+00:00
---

# Description

Spec docs/tui-spec.md §11 global `o`, §13.

- Resolve the file: the selected or displayed issue's actual path (respect sharded layouts; expose a `pub` path lookup from storage if needed) or the triage item path.
- Suspend the TUI (terminal `suspend()`), run `$EDITOR` (fallback `vi`; support `EDITOR` values with arguments, e.g. `code -w`), wait, `resume()`, then trigger a reload.
- Launch failure → message in the hint bar; terminal always restored.
- The TUI itself never writes files.

# Acceptance Criteria

- Unit test for path resolution (flat and sharded layouts, triage).
- Test with `EDITOR=true` (or a script) shows the TUI resumes and a reload is requested; `EDITOR=/nonexistent` shows a hint-bar error and the UI keeps working.
- Unset `EDITOR` falls back to `vi` (unit test on command construction).
