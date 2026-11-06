---
title: 'TUI: mode bar, filter bar (incl. search input) and hint bar'
status: closed
priority: 1
issue_type: task
role: agent
effort: 2
labels:
- tui
dependencies:
  tazk-10: blocked_by
  tazk-11: blocked_by
  tazk-12: blocked_by
  tazk-14: blocked_by
  tazk-2: child_of
created_at: 2026-11-06T03:28:37.362487753+00:00
updated_at: 2026-11-06T17:04:58.691400669+00:00
closed_at: 2026-11-06T17:04:58.691400669+00:00
---

# Description

Spec docs/tui-spec.md §4 items 1, 2, 4 and §7 input display.

- Mode bar: `Issues (N) │ Triage (M)` with current mode highlighted; current grouping on the right in issues mode.
- Filter bar, always visible: issues mode shows each filter summary (`role:all status:!closed …`), the search query if any, `X of Y`, and `⚠ N skipped`; triage mode shows search, `X of Y`, skipped count. While typing, the search input with cursor lives here.
- Hint bar: most relevant keys for the current context, generated from the keymap with inapplicable keys hidden (e.g. filter keys in triage mode).
- Transient status message slot in the hint bar (used by clipboard/editor errors); cleared on next key or after a few seconds.

# Acceptance Criteria

- Snapshot tests: issues mode default bars; with search + skipped files; triage mode bars (no filter keys in hints); search input active; transient message shown.
- `X of Y` counts are correct for issues (visible cells excluding dimmed headers vs total loaded) and triage (matching vs total) — unit test; document the epic-header counting choice.
- Hints change with context (grid vs detail vs picker vs search input).
