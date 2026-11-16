# Implementation

How tazk is laid out on disk and in the source tree. See also
[src/README.md](../src/README.md) for the source architecture.

## Storage Format

tazk stores issues in `.tazk/issues/` as markdown files, and triage items
(unplanned reports) in `.tazk/triage/` as plain markdown.

```
.tazk/
├── config.yaml           # tz-hash-ids, issue-storage-layout
├── .gitignore           # Auto-managed (tazk.lock, command_history.log)
├── issues/
│   ├── myproject-1.md   # Issue files with YAML frontmatter
│   └── myproject-2.md
├── comments/            # Optional per-issue comment JSON files
└── triage/              # Triage items: plain markdown, no frontmatter
    └── 20251104T001800-k3f9.md
```

### Issue Format

Each issue is a markdown file with YAML frontmatter:

```markdown
---
title: Fix authentication bug
status: in_progress
priority: 1
issue_type: bug
assignee: alice
dependencies:
  myproject-5: blocked_by
created_at: 2025-10-30T10:00:00Z
updated_at: 2025-10-30T11:00:00Z
---

# Description

User sessions expire too quickly. Need to extend timeout to 24 hours.

# Design

Update session middleware to use configurable timeout from environment variable.

# Acceptance Criteria

- [ ] Session timeout configurable via SESS_TIMEOUT env var
- [ ] Default remains 1 hour if not set
- [ ] Tests pass for various timeout values
```

## Project Structure

```
src/
├── main.rs      # CLI entry point and command handlers
├── storage.rs   # File-based storage operations
├── storage/triage.rs  # Triage items (.tazk/triage/)
├── format.rs    # Markdown serialization/deserialization
├── types.rs     # Core data structures (Issue, Status, etc.)
├── lock.rs      # Coarse-grained file locking
├── merge_driver.rs, issue_merge.rs, diff3.rs  # Git merge driver (see src/README.md)
└── tui/         # `tz tui` read-only terminal UI (feature `tui`, see src/README.md)

tests/
├── e2e_tests.rs           # Test harness
└── basic_operations.sh    # Shell-based e2e tests
```

## Locking Strategy

tazk uses coarse-grained locking with `.tazk/tazk.lock` containing the process PID. Operations use exponential backoff (up to 5 seconds) when lock is held. This is simple and sufficient for typical workflows.
