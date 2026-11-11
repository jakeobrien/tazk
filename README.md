# tazk

A minimal, markdown-based issue tracker written in Rust.

## Overview

tazk (`tz`) is a dependency-aware issue tracker. Issues are stored as markdown files with YAML frontmatter, making them both human-readable and git-friendly. There is no database: just markdown files in `.tazk/issues/`.

### Key Features

- **Markdown-only storage**: Issues are plain `.md` files
- **Dependency tracking**: Issues can block each other, with automatic detection of ready work
- **Fast**: Rust implementation with coarse-grained file locking
- **Git-friendly**: Field-aware three-way merge driver for issue and comment files
- **Terminal UI**: `tz tui` is a read-only, full-screen browser for issues and triage items (see [USAGE.md](USAGE.md#terminal-ui); behind the default-on `tui` cargo feature)

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
tz create "Fix login bug" -p 1 -t bug -r agent

# Report something found during review/testing (a triage item, not an issue)
tz triage add "Login fails on Safari. Steps to reproduce..."

# Triager: turn triage items into an issue
tz triage list
tz triage promote 20251104T001800-k3f9 --title "Fix Safari login" -r agent -p 1 -t bug

# List all issues
tz list

# Show issue details
tz show tz-1

# Claim an issue (sets assignee and status to in_progress)
tz claim tz-1 --as alice

# Add dependencies (tz-1 is blocked_by tz-2)
tz dep add tz-1 --target tz-2

# Find ready work (no blockers)
tz ready

# Get statistics
tz stats
```

Run `tz quickstart` for a comprehensive guide.

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

## Design Philosophy

### Why Markdown?

- **Human-readable**: Issues are plain text files you can read, edit, and grep
- **Git-friendly**: Diffs, merges, and history work naturally
- **Simple**: No schema migrations, no database corruption, no SQL
- **Portable**: Copy `.tazk/` anywhere, it just works

### Locking Strategy

tazk uses coarse-grained locking with `.tazk/tazk.lock` containing the process PID. Operations use exponential backoff (up to 5 seconds) when lock is held. This is simple and sufficient for typical workflows.

## Usage

See [USAGE.md](USAGE.md) for the full command reference and a description of issue properties and their possible values.

## Environment Variables

- `TAZK_DIR` - Path to the `.tazk` directory

## License

MIT.
