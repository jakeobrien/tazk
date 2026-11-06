---
title: 'TUI: snapshot-test harness and fixture builders'
status: closed
priority: 1
issue_type: task
role: agent
effort: 2
labels:
- tui
dependencies:
  tazk-1: child_of
  tazk-7: blocked_by
created_at: 2026-11-06T03:28:37.349342295+00:00
updated_at: 2026-11-06T16:07:34.549193054+00:00
closed_at: 2026-11-06T16:07:34.549193054+00:00
---

# Description

Infrastructure for spec docs/tui-spec.md §14 snapshot tests.

- Add `insta` as a dev-dependency (or an equivalent golden-file approach) and a helper that renders a widget/full app into `ratatui::backend::TestBackend` of a given size and returns the buffer as a string (optionally with a style dump for colour-sensitive assertions).
- Fixture builder for `Issue` (id, title, type, status, priority, role, effort, deps, timestamps, body sections) and `TriageItem`, plus `Snapshot` assembly with dependents computed.
- Fixed timezone for tests (timestamps are rendered in local time) — e.g. format via an injectable offset or set `TZ=UTC` in the test helper.

# Acceptance Criteria

- One example snapshot test (e.g. rendering an empty `App`) passes and is committed.
- Fixture builder can express multi-parent `child_of`, missing targets and closed epics in one or two lines each.
- Tests are deterministic regardless of the machine's timezone and current time.
- `cargo test` runs the snapshot tests only when the `tui` feature is enabled.
