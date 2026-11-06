---
title: 'Storage: tolerant issue + triage loader returning (items, skipped)'
status: open
priority: 1
issue_type: task
role: agent
effort: 2
labels:
- tui
dependencies:
  tazk-1: child_of
created_at: 2026-11-06T03:28:37.342912003+00:00
updated_at: 2026-11-06T03:28:37.342912003+00:00
---

# Description

Spec docs/tui-spec.md §3.1–§3.2. The current `list_all_issues_no_dependents` / `list_issues` abort on the first bad file. Add tolerant variants to `storage` (do not duplicate parsing in the TUI):

- `list_all_issues_tolerant() -> Result<Loaded<Issue>>` — every issue under `.tazk/issues/` (including closed, any layout incl. sharded), with `dependents` populated over the full set, plus a list of skipped files.
- `list_triage_items_tolerant() -> Result<Loaded<TriageItem>>` — same idea for `.tazk/triage/*.md`.
- `pub struct Loaded<T> { items: Vec<T>, skipped: Vec<SkippedFile> }`, `pub struct SkippedFile { path: PathBuf, error: String }` (error = full anyhow chain, one line).
- Take only the **read** lock, held as briefly as possible; never the write lock (§3.3).
- Expose (pub) a helper for the 'open blockers' rule used by `tz ready` (`has_unresolved_dependencies`: a `blocked_by` target that is not closed, or missing, is unresolved) so the TUI reuses it instead of reimplementing.

# Acceptance Criteria

- Storage-level tests: a directory with one valid issue, one with invalid YAML, one with an unresolved merge-conflict hunk, and one with an invalid effort returns 1 issue and 3 `SkippedFile`s with useful error text and correct paths.
- Dependents are computed over all successfully parsed issues (test with reverse `child_of` and `blocked_by`).
- Closed issues are included.
- Triage variant skips an unreadable (e.g. non-UTF-8) file and returns the rest sorted by name.
- Only `lock_for_read` is used (verified by code review / a test that holds a read lock concurrently).
- Existing `list_issues` behaviour and all existing tests are unchanged.
