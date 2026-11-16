# tazk

> **Warning:** tazk is work-in-progress software. Commands, the on-disk format and behavior may change without notice, and bugs are likely. Use it at your own risk.

A minimal, markdown-based issue tracker intended for use in managing agent workflows.

## Overview

tazk (`tz`) is designed around providing:
- a terminal-based workflow
- lightweight issue tracking
- dependency-aware issues (blocking issues, epics as parents)
- quick capture of triage items
- role-based routing (human or agent)
- human visibility (`tz tui` shows a browsable view of issues)
- portability

Issues are stored as markdown files with YAML frontmatter, making them both human-readable and git-friendly. There is no database: just markdown files in `.tazk/issues/`.

### Heritage/Acknowledgments

This project was developed from the MIT-licensed [rrnewton/minibeads](https://github.com/rrnewton/minibeads). That project's source was used as the starting point.

`minibeads` is itself a Rust port of [steveyegge/beads](https://github.com/steveyegge/beads).

Much of this project's approach comes from `beads`, but this project (unlike `minibeads`) is not a drop-in replacement for it.

## Installation

### Build from source

Use Rust 1.99 or newer (the `rust-version` in `Cargo.toml`).

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
tz show tz-4f1

# Claim an issue (sets assignee and status to in_progress)
tz claim tz-4f1 --as alice

# Add dependencies (tz-4f1 is blocked_by tz-b7k)
tz dep add tz-4f1 --target tz-b7k

# Find ready work (open, no blockers)
tz ready

# Get statistics
tz stats
```

Run `tz quickstart` for a comprehensive guide.

## Documentation

- [CLI reference](docs/cli.md): commands, options and environment variables
- [Issue properties](docs/issues.md): frontmatter, body sections, statuses, types, dependencies and claims
- [Terminal UI](docs/tui.md): `tz tui`
- [Git merge driver](docs/merge-driver.md): field-aware merging of issue and comment files
- [Implementation](docs/implementation.md): storage format, project structure and locking

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

## License

[MIT](LICENSE.md).
