# Source architecture

The binary composes safe-Rust modules around Markdown issue storage:

- `storage.rs`, `transaction.rs`, and `lock.rs` own checked local reads/writes,
  coarse locking, atomic replacement, and rollback.
- `storage/triage.rs` stores triage items: plain Markdown reports in
  `.tazk/triage/`, kept apart from issues, and promotes or attaches them to
  issues in a single transaction.
- `sync.rs` reconciles Markdown and JSONL using timestamps.
- `prose_merge.rs` implements bounded deterministic Markdown/prose merging. It
  does not use an LLM and never silently resolves typed conflicts.
- `format.rs` and `types.rs` define the Markdown representation and domain types.
  Serialization is deterministic (dependencies are sorted) so that merges and
  diffs see only real changes.
- `issue_merge.rs` is the whole-file three-way merge of issue Markdown (every
  field, label set, dependency map, timestamps, and all four prose sections via
  `prose_merge.rs`) and of comment JSON (three-way set keyed by comment ID).
  Conflicts are rendered as diff3 hunks covering only the conflicting lines;
  non-canonical input falls back to a textual merge instead of being rewritten.
- `diff3.rs` is the line-level three-way merge and conflict-hunk writer under it.
- `merge_driver.rs` implements `tz merge-driver run|install|show`: git's merge
  driver protocol, path classification, and `git config`/`.gitattributes` setup.
- `tui/` implements `tz tui`, a read-only ratatui/crossterm browser for issues and
  triage items, compiled only with the default-on `tui` cargo feature. `app.rs`
  holds state and key-event updates (no drawing), `model/` builds the pure
  grouping, filter, search and selection logic, `data.rs` loads snapshots (using the
  tolerant storage loader that skips bad files) and drives refresh, `keymap.rs`
  is the single key table behind both input dispatch and the help overlay,
  `ui/` renders, and `terminal.rs` owns raw mode and the `$EDITOR` hand-off.
  User docs are in `docs/tui.md`.
