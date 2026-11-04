# tazk Usage

Command-line and issue-property reference for `tz`. For installation and an
overview, see the [README](README.md).

## Command Reference

### Core Commands

- `tz init [--prefix PREFIX]` - Initialize a database
- `tz create TITLE -r human|agent [OPTIONS]` - Create new issue (status `open`). `-r/--role` is required.
  - Common options: `-p PRIORITY`, `-t TYPE`, `-d TEXT`, `-a ASSIGNEE`, `--effort 1-5`, `-l LABEL`
- `tz list [FILTERS]` - List issues with optional filters (including `--assignee USER` and `-r/--role ROLE`)
- `tz show ISSUE_ID` - Show detailed issue information
- `tz update ISSUE_ID [OPTIONS]` - Update issue fields (not status)
  - `-r/--role ROLE`, `--effort N` set the role or effort; `--clear-role` and
    `--clear-effort` remove them.
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
- `tz claim ISSUE_ID... --as NAME [--team TEAM] [--release] [--force]` - Claim or release issues
- `tz block ISSUE_ID... [--release] [--reason TEXT]` - Mark issues `blocked`, or back to `open` with `--release`. A reason is appended to Notes as `Blocked <UTC timestamp>: <reason>` (`Unblocked ...` on release)
- `tz defer ISSUE_ID... [--release] [--reason TEXT]` - Same as `tz block`, for `deferred` (`Deferred ...` / `Undeferred ...` notes)
- `tz close ISSUE_ID [--reason REASON]` - Close (complete) an issue. A reason is appended to Notes as `Closed <UTC timestamp>: <reason>`
- `tz reopen ISSUE_ID... [--role human|agent] [--reason REASON]` - Reopen closed issues. A reason is appended to Notes as `Reopened <UTC timestamp>: <reason>`. An issue with no role is refused until you resubmit with `--role`
- `tz comments add ISSUE_ID --body TEXT --author NAME` - Add a local issue comment
- `tz comments list ISSUE_ID` - List local issue comments
- `tz comments delete ISSUE_ID COMMENT_ID...` - Delete local issue comment(s) by ID

### Triage

Triage items are an inbox for things found during code review, validation,
testing or ad hoc feedback. They are not issues. Each one is a plain Markdown
file in `.tazk/triage/` holding only text: no frontmatter, ID, title, status,
priority, role, assignee, labels, dependencies or comments. Being in that
folder is what makes it a triage item, so the reporter doesn't have to plan
anything. A triager later sweeps the folder and promotes, attaches or removes
each item.

Items are named `<UTC timestamp>-<4 random characters>.md`, e.g.
`20251104T001800-k3f9.md`. Names sort by creation time (to the second) and are
unique without coordination, so items reported on different branches never
collide. Any other `*.md` file dropped into the folder by hand is also an item.
Commands that take an item accept the name with or without `.md`, or a path to
the file in `.tazk/triage/`.

- `tz triage add [TEXT]` - Create an item. Its text is `TEXT` followed, after
  a blank line, by anything piped on stdin. At least one must be non-empty.
  Pipe external references, logs or review output:
  `gh pr view 12 --json url -q .url | tz triage add "Flaky login test"`.
  Piping also keeps the shell from expanding backticks and `$(...)` in prose.
  stdin is read only when it is not a terminal; if it is a pipe that never
  closes, the command waits, so redirect stdin from `/dev/null` in such
  environments.
- `tz triage list` - Print item file names, one per line, sorted by name.
  With `--json`, an array of `{name, body}`.
- `tz triage show NAME` - Print an item's text.
- `tz triage update NAME (--search TEXT --replace TEXT [--replace-all] | --append TEXT)` -
  Edit an item's text with the same rules as `tz update --search/--replace`
  and `--append` (there is no `--field`). An edit that would leave the item
  empty is refused; use `tz triage rm`.
- `tz triage rm NAME...` - Delete items. All must exist, otherwise none is
  deleted.
- `tz triage promote NAME... --title TITLE -r human|agent [OPTIONS]` - Create
  one issue from one or more items and delete the items. Accepts the same
  options as `tz create` except `--id` (`-p`, `-t`, `-d` /
  `--description-file`, `--design`, `--acceptance`, `-a`, `--effort`,
  `-l`/`--labels`, `--external-ref`, `--deps`, `--parent`). Without a
  description flag the description is the items' text, in the order given,
  separated by `---` lines. The issue's Notes record the item names
  (`Promoted from triage: ...`), so the originals can be found in git history.
- `tz triage attach NAME... --to ISSUE_ID --author NAME` - Add each item as a
  comment on an existing issue (one comment per item, in order) and delete the
  items. `--author` is required.

`promote`, `attach` and `rm` are transactions: the issue or comments are
written and the items deleted together, or nothing changes. Triage items are
not read by `list`, `ready`, `stats`, `export` or `sync`, and the merge driver
leaves them to git's ordinary line merge.

### Dependencies

- `tz dep add ISSUE_ID --target DEPENDENCY_ID [--type TYPE]` - Add dependency (ISSUE_ID has the dependency on DEPENDENCY_ID)
  - Types: `blocked_by` (default), `related_to`, `child_of`, `discovered_from`, `tracks`
- `tz dep remove ISSUE_ID --target DEPENDENCY_ID` - Remove dependency

### Queries

- `tz ready [--assignee USER] [-r/--role ROLE] [--priority N]` - Find ready work (no blockers)
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

- **Scalar fields** (title, status, priority, type, assignee, role, effort, external ref)
  take the side that changed; two different changes conflict.
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
| `title` | yes | Any text | Settable with `create`, `triage promote --title` and `update --title`. |
| `status` | yes | See [Status](#status) | Default `open`. |
| `priority` | yes | Integer; `0`–`4` by convention, `0` is highest | Default `2`. Used for sorting in `tz ready`. |
| `issue_type` | yes | See [Issue type](#issue-type) | Default `task`. Set with `-t/--issue-type`. |
| `assignee` | no | Any text | Omitted when empty. Clear with `update --unassign`. |
| `role` | no | `human`, `agent` | Who the issue is for. Omitted when unset. Required by `tz create` and `tz triage promote` (`-r/--role`); change with `tz update -r/--role`; clear with `update --clear-role`. Filter with `-r/--role` on `tz list`, `tz ready` and `tz export`. |
| `effort` | no | Integer `1`–`5`; `5` is the most effort | Omitted when unset. Set with `tz create/update --effort`; clear with `update --clear-effort`. Values outside 1–5 are rejected, including in hand-edited files. |
| `external_ref` | no | Any text (e.g. a URL or ticket key in another tracker) | Omitted when unset. |
| `labels` | no | List of free-form strings | Stored sorted and de-duplicated; empty entries are dropped. Commas separate labels in `--labels`, `--set-labels`. |
| `dependencies` | no | Map of issue ID to [dependency type](#dependency-types) | Sorted by ID. |
| `created_at` | yes | RFC 3339 UTC timestamp | Set on creation. |
| `updated_at` | yes | RFC 3339 UTC timestamp | Refreshed on every change. |
| `closed_at` | no | RFC 3339 UTC timestamp | Set by `tz close`, cleared by `tz reopen`. Only present on closed issues. |

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
| `in_progress` | Being worked on. Set by `tz claim`. |
| `closed` | Complete. Sets `closed_at`. |
| `blocked` | Marked as blocked by hand (`tz block`). Not listed by `tz ready`. |
| `deferred` | Postponed (`tz defer`). Not listed by `tz ready`. |

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
| `discovered_from` | The issue was discovered while working on the target. |
| `tracks` | The issue tracks the target. |

The legacy spelling `discovered-from` is still accepted when reading, but files are written with `discovered_from`.

### Claims

`tz claim ISSUE_ID... --as NAME` sets `assignee` to `NAME` (or `NAME/TEAM` with
`--team`), sets `status` to `in_progress`, and bumps `updated_at`. `--as` is
required. There is no expiry: a claim lasts until it is released.

The lock is the status. Claiming an issue that is already `in_progress` fails,
even for the same `--as` identity; `--force` overrides the lock and reassigns
the issue. Claiming a `closed` issue always fails, even with `--force`.

`tz claim ISSUE_ID... --as NAME --release` clears `assignee` and moves an
`in_progress` issue back to `open`. Releasing a claim held by a different
identity requires `--force`.

Status cannot be set with `tz update`; it changes only through dedicated
commands (`claim`, `close`, `reopen`).
