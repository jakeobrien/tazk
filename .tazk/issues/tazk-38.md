---
title: 'TUI: manual acceptance pass and palette tuning in real terminals'
status: open
priority: 3
issue_type: task
role: human
effort: 2
labels:
- tui
dependencies:
  tazk-33: blocked_by
  tazk-34: blocked_by
  tazk-37: blocked_by
  tazk-6: child_of
created_at: 2026-11-06T03:28:37.383600920+00:00
updated_at: 2026-11-06T03:28:37.549226461+00:00
---

# Description

Human review of TUI v1 against docs/tui-spec.md in real terminals: a truecolor terminal (e.g. iTerm2/kitty/WezTerm/Ghostty), a 256-color terminal, inside tmux with `focus-events on`, and over SSH. Tune the §12 palette by eye (in `ui/theme.rs` only).

# Acceptance Criteria

- Checked: layout at ~120 and ~200 columns; too-small message; colors legible on dark and light backgrounds; dimmed headers distinguishable; `⊘` vs `blocked` badge distinct; focus-gain reload in tmux; `y` clipboard locally and over SSH; `o` editor round-trip; terminal restored after `q`, `Ctrl-C` and a forced panic.
- Palette adjustments (if any) committed and snapshot tests updated.
- Any defects filed as new issues linked `discovered_from` this one.
