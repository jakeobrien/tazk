---
title: 'TUI: add `tui` cargo feature, dependencies and `tz tui` subcommand stub'
status: closed
priority: 1
issue_type: task
role: agent
effort: 2
labels:
- tui
dependencies:
  tz-1: child_of
created_at: 2026-11-06T03:28:37.341074211+00:00
updated_at: 2026-11-06T14:29:55.473223+00:00
closed_at: 2026-11-06T14:29:55.473223+00:00
---

# Description

Packaging per docs/tui-spec.md §2.

- Add cargo feature `tui`, **enabled by default** (`default = ["tui"]`).
- Optional deps enabled by `tui`: `ratatui` (crossterm backend), `crossterm`, `tui-markdown` with its default `highlight-code` feature.
- Pick versions so `tui-markdown`'s `ratatui-core` is the same version `ratatui` uses (check `cargo tree -d`, no duplicate `ratatui-core`).
- Add `Tui(TuiArgs)` subcommand, `#[cfg(feature = "tui")]`, dispatching to `tui::run`. Clap only here; flag semantics are handled in the CLI-flags issue (declare the flags now so `tz tui --help` is complete: `--mode`, `--group-by`, `-r/--role`, `-s/--status`, `-t/--type`, `-p/--priority`, `--effort`, `--search`).
- Respect `--tz-dir`, `TAZK_DIR` and normal `.tazk` discovery exactly like other commands; a missing `.tazk` exits with the usual error **before** touching the terminal (§13).
- Create the module tree skeleton (empty modules with doc comments).

# Design

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

- `cargo build` (default features) produces `tz` with a `tui` subcommand listed in `tz --help`.
- `cargo build --no-default-features` succeeds and `tz tui` is an unknown subcommand.
- `cargo tree -e normal -i ratatui-core` shows a single version.
- `tz tui` outside a tazk project prints the same 'not found' error as `tz list` and exits non-zero without altering terminal state.
- `src/tui/` module skeleton exists per the layout; nothing TUI-related is added to `main.rs` beyond the subcommand variant and dispatch.
- Existing tests and `cargo clippy --all-targets` pass in both feature configurations.
