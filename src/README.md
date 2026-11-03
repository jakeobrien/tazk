# Source architecture

The binary composes safe-Rust modules around Markdown issue storage:

- `storage.rs`, `transaction.rs`, and `lock.rs` own checked local reads/writes,
  coarse locking, atomic replacement, and rollback.
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
