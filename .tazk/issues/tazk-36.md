---
title: 'TUI: document `tz tui` in USAGE.md and README'
status: open
priority: 3
issue_type: task
role: agent
effort: 1
labels:
- tui
dependencies:
  tazk-13: blocked_by
  tazk-22: blocked_by
  tazk-23: blocked_by
  tazk-28: blocked_by
  tazk-31: blocked_by
  tazk-32: blocked_by
  tazk-33: blocked_by
  tazk-34: blocked_by
  tazk-35: blocked_by
  tazk-6: child_of
created_at: 2026-11-06T03:28:37.381097003+00:00
updated_at: 2026-11-06T03:28:37.533132795+00:00
---

# Description

Add a `tz tui` section to USAGE.md: purpose (read-only browser), flags (§2.1), key bindings (or point to `?`), refresh behaviour incl. the tmux `set -g focus-events on` note, `$EDITOR` hand-off, skipped-files indicator, and the `tui` cargo feature / `--no-default-features`. Mention it in README's feature list and `src/README.md` module overview.

# Acceptance Criteria

- USAGE.md has a `tz tui` section covering every flag and the items above; README and `src/README.md` reference it.
- Documented flags/keys match `tz tui --help` and the keymap (spot-check).
