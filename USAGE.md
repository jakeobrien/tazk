# tazk Usage

Command-line and issue-property reference for `tz`. For installation and an
overview, see the [README](README.md).

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

## Issue Properties

Every issue is a markdown file whose YAML frontmatter holds the structured
properties below; the free-text sections live in the markdown body (see
[Storage Format](README.md#storage-format)). With `--json`, the same properties
are emitted using the same names.

### Frontmatter properties

| Property | Required | Values | Notes |
|----------|----------|--------|-------|
| `id` | yes | `PREFIX-N`, or a hash/sharded form depending on how the database was initialised | Not stored in the frontmatter: it is the file name (`.tazk/issues/<id>.md`). Commands accept numeric shorthand (`14` expands to `PREFIX-14`). Set explicitly with `tz create --id`. |
| `title` | yes | Any text | Settable with `create` and `update --title`. |
| `status` | yes | See [Status](#status) | Default `open`. |
| `priority` | yes | Integer; `0`–`4` by convention, `0` is highest | Default `2`. Used for sorting in `tz ready`. |
| `issue_type` | yes | See [Issue type](#issue-type) | Default `task`. Set with `-t/--issue-type`. |
| `assignee` | no | Any text | Omitted when empty. Clear with `update --unassign`. |
| `external_ref` | no | Any text (e.g. a URL or ticket key in another tracker) | Omitted when unset. |
| `labels` | no | List of free-form strings | Stored sorted and de-duplicated; empty entries are dropped. Commas separate labels in `--labels`, `--set-labels`. |
| `dependencies` | no | Map of issue ID to [dependency type](#dependency-types) | Sorted by ID. |
| `created_at` | yes | RFC 3339 UTC timestamp | Set on creation. |
| `updated_at` | yes | RFC 3339 UTC timestamp | Refreshed on every change. |
| `closed_at` | no | RFC 3339 UTC timestamp | Set by `tz close`, cleared by `tz reopen`. Only present on closed issues. |
| `claimed_at` | no | RFC 3339 UTC timestamp | When the current claim was taken. Omitted if unclaimed. |
| `claimed_until` | no | RFC 3339 UTC timestamp | When the claim goes stale. Omitted if unclaimed. |

### Body sections

These are Markdown headings in the issue body. Each is free text and may be
empty.

| Section | Heading | `--field` name | Create/update flag |
|---------|---------|----------------|--------------------|
| Description | `# Description` | `description` (alias `desc`) | `-d/--description`, `--description-file` |
| Design | `# Design` | `design` | `--design` |
| Acceptance criteria | `# Acceptance Criteria` | `acceptance` (alias `acceptance_criteria`) | `--acceptance` |
| Notes | `# Notes` | `notes` | `--notes` (`update`) |

`--field` (used by `tz update --search/--replace` and `--append`) also accepts
`title`.

### Status

| Value | Meaning |
|-------|---------|
| `open` | Not started. Open issues with no unresolved blockers are reported by `tz ready`. |
| `in_progress` | Being worked on. Set by `tz update --claim` / `tz claim`. |
| `blocked` | Marked as blocked by hand. |
| `deferred` | Postponed. Not listed by `tz ready`. |
| `hooked` | Accepted for agent/workflow tooling; `tz` gives it no special behaviour. |
| `pinned` | Accepted for agent/workflow tooling; no special behaviour. |
| `staged_ready` | Accepted for staging workflows; no special behaviour. |
| `staged_warnings` | Accepted for staging workflows; no special behaviour. |
| `closed` | Complete. Sets `closed_at`. |

Status values are lowercase with underscores. Only `open` issues appear in `tz ready`. Note that `tz blocked` reports
issues that have unresolved `blocked_by` dependencies; it does not depend on the
`blocked` status value.

### Issue type

| Value | Typical use |
|-------|-------------|
| `bug` | Something is broken. |
| `feature` | New functionality. |
| `task` | General work item (default). |
| `epic` | Large body of work grouping other issues. |
| `chore` | Maintenance work. |

The following types are also accepted, mainly for agent and workflow
orchestration: `agent`, `role`, `rig`, `convoy`, `slot`, `queue`, `event`,
`message`, `molecule`, `gate`, `merge-request`, `workflow`, `patrol`.

### Dependency types

Dependencies are stored on the dependent issue: `A: {B: blocked_by}` means A
depends on B.

| Value | Meaning |
|-------|---------|
| `blocked_by` | Default. The issue is not ready until the target is closed. The only type that affects `tz ready` and `tz blocked`. |
| `related_to` | Informational link. |
| `child_of` | The issue is a child of the target (e.g. an epic). |
| `discovered-from` | The issue was discovered while working on the target. |
| `tracks` | The issue tracks the target. |

Note the spelling: `discovered-from` uses a hyphen; the others use underscores.

### Claims

`tz update --claim` (or `tz claim`) sets `assignee` to the claim identity
(hostname, optionally `host/team`; override with `--as`), sets `status` to
`in_progress`, and records `claimed_at` and `claimed_until`. `--for` sets the
duration using `90m`, `48h`, `2d`, or a bare number of hours (default `48h`). A
claim is active until `claimed_until` passes; after that it is stale and another
worker may reclaim the issue. An assignee with no `claimed_until` is treated as
held indefinitely.
