---
title: 'TUI: foundation (packaging, loader, terminal, app core)'
status: closed
priority: 1
issue_type: epic
effort: 5
labels:
- tui
created_at: 2026-11-06T03:28:37.329201003+00:00
updated_at: 2026-11-15T16:45:20.107975762+00:00
closed_at: 2026-11-15T16:33:02.019903+00:00
---

# Description

Groundwork for `tz tui` (docs/tui-spec.md §2, §3.2, §11, §13, §14): cargo feature + deps, tolerant storage loader, terminal lifecycle/event loop, keymap, app state core, snapshot-test harness and CLI flags.

Shared module layout (all under `src/tui/`, behind the `tui` feature):

- `mod.rs` – `pub fn run(storage, TuiArgs) -> Result<()>` entry point
- `terminal.rs` – terminal setup/teardown, panic hook, event loop
- `keymap.rs` – the single keymap definition (input dispatch + help overlay)
- `app.rs` – `App` state, `Action` enum, `fn update(&mut App, Action)`; no rendering
- `data.rs` – `Snapshot` (issues incl. closed + dependents, `by_id` index, triage items, skipped files) and loading
- `model/` – pure logic: `filter.rs`, `search.rs`, `sort.rs`, `blockers.rs`, `columns.rs`, `relations.rs`, `triage.rs`
- `ui/` – rendering only: `theme.rs`, `cell.rs`, `grid.rs`, `bars.rs`, `picker.rs`, `detail.rs`, `markdown.rs`, `triage.rs`, `help.rs`, `layout.rs`
- `actions/` – side effects: `clipboard.rs`, `editor.rs`

Rule: state + update logic never touch the terminal, so key sequences are testable without drawing (spec §14).

# Acceptance Criteria

- All child issues closed.
- `cargo build`, `cargo build --no-default-features`, `cargo test` and `cargo clippy --all-targets` pass.
- `tz tui` launches into an (initially empty) full-screen UI and quits cleanly with `q`/`Ctrl-C`.
