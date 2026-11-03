# tazk

A minimal, markdown-based issue tracker written in Rust.

## Overview

tazk (`tz`) is a dependency-aware issue tracker. Issues are stored as markdown files with YAML frontmatter, making them both human-readable and git-friendly. There is no database: just markdown files in `.tazk/issues/`.

### Key Features

- **Markdown-only storage**: Issues are plain `.md` files
- **Dependency tracking**: Issues can block each other, with automatic detection of ready work
- **Fast**: Rust implementation with coarse-grained file locking
- **Git-friendly**: Field-aware three-way merge driver for issue and comment files

## Installation

### Build from source

Use Rust 1.87 or newer (required by the build-time metadata helper).

```bash
# Build in debug mode (recommended for development)
make build

# Or build release version
make release

# Install to ~/.local/bin
make install
```

The binary is named `tz`.

## Quick Start

```bash
# Initialize a database in your project
tz init

# Create your first issue
tz create "Fix login bug" -p 1 -t bug

# List all issues
tz list

# Show issue details
tz show tz-1

# Update issue status
tz update tz-1 --status in_progress

# Add dependencies (tz-1 is blocked_by tz-2)
tz dep add tz-1 --target tz-2

# Find ready work (no blockers)
tz ready

# Get statistics
tz stats
```

Run `tz quickstart` for a comprehensive guide.

## Storage Format

tazk stores all data in `.tazk/issues/` as markdown files.

```
.tazk/
├── config.yaml           # Contains issue-prefix
├── .gitignore           # Auto-managed (tazk.lock, command_history.log)
├── issues/
│   ├── myproject-1.md   # Issue files with YAML frontmatter
│   └── myproject-2.md
└── comments/            # Optional per-issue comment JSON files
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

## Development

### Running Tests

```bash
# Run all tests (unit + e2e)
make test

# Run full validation (Rust tests + e2e + fmt + clippy)
make validate

# Run longer randomized/stress suites
make stress-test

# Format code
make fmt
```

### Project Structure

```
src/
├── main.rs      # CLI entry point and command handlers
├── storage.rs   # File-based storage operations
├── format.rs    # Markdown serialization/deserialization
├── types.rs     # Core data structures (Issue, Status, etc.)
├── lock.rs      # Coarse-grained file locking
└── merge_driver.rs, issue_merge.rs, diff3.rs  # Git merge driver (see src/README.md)

tests/
├── e2e_tests.rs           # Test harness
└── basic_operations.sh    # Shell-based e2e tests
```

## Design Philosophy

### Why Markdown?

- **Human-readable**: Issues are plain text files you can read, edit, and grep
- **Git-friendly**: Diffs, merges, and history work naturally
- **Simple**: No schema migrations, no database corruption, no SQL
- **Portable**: Copy `.tazk/` anywhere, it just works

### Why Rust?

- **Performance**: Fast file operations
- **Safety**: No null pointers, no data races
- **Zero-copy**: Minimize allocations
- **Small binary**: Single ~23MB binary with no runtime dependencies

### Locking Strategy

tazk uses coarse-grained locking with `.tazk/tazk.lock` containing the process PID. Operations use exponential backoff (up to 5 seconds) when lock is held. This is simple and sufficient for typical workflows.

## Command Reference

### Core Commands

- `tz init [--prefix PREFIX]` - Initialize a database
- `tz create TITLE [OPTIONS]` - Create new issue
- `tz list [FILTERS]` - List issues with optional filters
- `tz show ISSUE_ID` - Show detailed issue information
- `tz update ISSUE_ID [OPTIONS]` - Update issue fields
  - `--search TEXT --replace TEXT [--field FIELD] [--replace-all]` - targeted,
    aider-style edit of a text field (default `description`) instead of
    overwriting it wholesale. By default the search text must match exactly once;
    a missing or ambiguous match is an error and the issue is left untouched.
    This is the recommended way for agents to revise a long description — far
    safer than rewriting the whole field.
  - `--append TEXT [--field FIELD]` - append `TEXT` to the end of a text field
    (default `description`), inserting a blank line before it when the field is
    non-empty so it becomes its own paragraph. Simpler than a search/replace when
    you only want to add to the end.
- `tz close ISSUE_ID [--reason REASON]` - Close (complete) an issue
- `tz reopen ISSUE_ID...` - Reopen closed issues
- `tz comments add ISSUE_ID --body TEXT` - Add a local issue comment
- `tz comments list ISSUE_ID` - List local issue comments
- `tz comments delete ISSUE_ID COMMENT_ID...` - Delete local issue comment(s) by ID

### Dependencies

- `tz dep add ISSUE_ID --target DEPENDENCY_ID [--type TYPE]` - Add dependency (ISSUE_ID has the dependency on DEPENDENCY_ID)
  - Types: `blocked_by` (default), `related_to`, `child_of`, `discovered-from`, `tracks`
- `tz dep remove ISSUE_ID --target DEPENDENCY_ID` - Remove dependency

### Queries

- `tz ready [--assignee USER] [--priority N]` - Find ready work (no blockers)
- `tz blocked` - Show blocked issues and what blocks them
- `tz stats` - Show statistics (total, open, blocked, average lead time)

### Git Merge Driver

Issue files and comment files are ordinary files under version control, so two
branches that touch the same issue meet in `git merge`. Git's line merge handles
them poorly: both sides always change `updated_at:`, two appended comments
collide at the end of the JSON array, and two edits to one long prose line
conflict. `tz merge-driver` replaces it with a field-aware three-way merge that
uses the merge base git already computed.

Install it once per clone (or with `--global` once per machine):

```bash
tz merge-driver install            # git config merge.tz.* + .gitattributes lines
tz merge-driver install --command /path/to/tz   # pin an executable instead of PATH lookup
tz merge-driver show               # print the config and attributes without changing anything
```

`install` writes `merge.tz.name` and `merge.tz.driver` into the local git config
and appends any missing routing lines to the repository's `.gitattributes`
(commit that file so every clone routes the same paths; each clone still needs
the `git config` part, because git never runs drivers named by the repository
alone). The equivalent manual setup is:

```bash
git config merge.tz.name "tazk three-way issue and comment merge"
git config merge.tz.driver "tz merge-driver run %O %A %B --marker-size %L --path %P"
cat >> .gitattributes <<'ATTRS'
**/.tazk/issues/**/*.md merge=tz
**/.tazk/comments/*.json merge=tz
ATTRS
```

Merge rules:

- **Scalar fields** (title, status, priority, type, assignee, external ref,
  claim window) take the side that changed; two different changes conflict.
- **Labels** merge as a set (additions and removals from both sides).
  **Dependencies** merge per target issue; changing a dependency's type on one
  side while removing it on the other conflicts.
- **Timestamps** never conflict: `created_at` keeps the earliest, `updated_at`
  the latest, and `closed_at` follows the merged status.
- **Prose sections** (Description, Design, Acceptance Criteria, Notes) use the
  same bounded paragraph/word merger. Edits to different
  paragraphs merge; competing edits conflict. A side that already contains the
  other side's whole change is taken as it stands. The merger is
  conservative: when repeated text makes the alignment of two edits ambiguous
  it reports a conflict rather than guess.
- **Comments** merge as a three-way set keyed by comment ID: comments added on
  either side are all kept, exactly once, in `created_at` order. A comment
  deleted on one side is deleted, as git would delete its lines, unless the
  other side edited it: then the edited comment is kept, with a `NOTICE` on
  stderr. A comment edited on both sides merges its body as prose or
  conflicts.
- Files with no merge base (both branches created the same ID) merge only when
  identical; otherwise the whole file is one conflict.
- Input that is not exactly what `tz` writes (hand-edited YAML, unknown keys)
  is never re-serialized: if only one side changed it is taken byte for byte,
  otherwise the file falls back to a plain line merge.

Conflicts are written as standard diff3 hunks with fixed labels, confined to
the lines of the conflicting field, paragraph, or comment:

```
<<<<<<< ours
title: Main title
||||||| base
title: Original title
=======
title: Feature title
>>>>>>> theirs
```

Everything outside the hunks is already merged, so a tool can resolve a file
by keeping one section of each hunk: keeping the same side everywhere
reproduces exactly that side's value of every conflicting field. Markers are
made longer than any content line that looks like one (such as a setext
`=======` underline), so they are never ambiguous. Git's own tools assume
markers of exactly `conflict-marker-size` (default 7), so `git diff --check`
and rerere do not recognize a grown hunk; resolve such a file by hand or with
a resolver that reads the marker length from the `<<<<<<<` line. The driver
also prints one `tz merge-driver: CONFLICT <path>: <field or comment>:
<reason>` line per conflict. `tz` refuses to read an issue file that still
contains a hunk outside a code fence, so a hunk accidentally committed is
reported instead of silently parsed as prose. (Text that quotes a conflict outside fenced code is
stored with its `<<<<<<<` line escaped as `\<<<<<<<` and reads back
unchanged; fenced code is stored as it is.)

`tz merge-driver run BASE OURS THEIRS --path REPO/PATH --stdout` previews a
merge without writing any file.

### Options

- `--tz-dir PATH` - Path to .tazk directory
- `--json` - Output JSON format
- `--tz-validation MODE` - Validation mode: silent, warn, error (default)
- `--tz-no-cmd-logging` - Disable command history logging

## Environment Variables

- `TAZK_DIR` - Path to the `.tazk` directory

## License

MIT.
