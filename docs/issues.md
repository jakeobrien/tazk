# Issue Properties

Every issue is a markdown file whose YAML frontmatter holds the structured
properties below; the free-text sections live in the markdown body (see
[Storage Format](implementation.md#storage-format)). With `--json`, the same properties
are emitted using the same names.

## Issue IDs

An ID is `tz-` plus either a number without leading zeros (`[1-9][0-9]*`) or
a lowercase base36 hash with at least one letter. IDs are always allocated by
`tz`; JSONL import and issue files accept only this canonical form. Output
always prints the full `tz-…` form.

**Shorthand:** every command argument that names an issue accepts the bare
suffix: `tz show 12`, `tz close a1b`, `tz create --parent 3`,
`--deps blocked_by:4,5`, `tz list --id 4,5`, `tz dep add 2 --target 1`. Input is
lowercased, so `TZ-12` and `A1B` work too. This covers `show`, `update`,
`claim`, `block`, `defer`, `close`, `reopen`, `children`, `renumber`,
`dep add/remove/list/tree`, `label add/remove/list`, `comments add/list/delete`,
`create` and `triage promote` (`--parent`, `--deps`), `list` and `ready`
(`--id`, `--parent`) and `triage attach --to`. Anything else is rejected with
`Invalid issue ID "<input>": expected tz-<n> or tz-<hash> (or just <n> / <hash>)`.
Triage item names are not IDs and are unaffected.

## Frontmatter properties

| Property | Required | Values | Notes |
|----------|----------|--------|-------|
| `id` | yes | `tz-<hash>`, or `tz-N` if the database was initialised with `--tz-hash-ids=false` | Not stored in the frontmatter: it is the file name (`.tazk/issues/<id>.md`). See [Issue IDs](#issue-ids) for the format and the shorthand commands accept. An issue file whose name does not match is an invalid file. |
| `title` | yes | Any text | Settable with `create`, `triage promote --title` and `update --title`. |
| `status` | yes | See [Status](#status) | Default `open`. |
| `priority` | yes | Integer; `0`–`4` by convention, `0` is highest | Default `2`. Used for sorting in `tz ready`. |
| `issue_type` | yes | See [Issue type](#issue-type) | Default `task`. Set with `-t/--issue-type`. |
| `assignee` | no | Any text | Omitted when empty. Clear with `update --unassign`. |
| `role` | no | `human`, `agent` | Who picks the issue up as work. Required on `task`, `bug`, `feature`, `chore` and `gate` by `tz create` and `tz triage promote` (`-r/--role`); forbidden on `epic` and `spec`. Omitted when unset. Change with `tz update -r/--role` (work types only); `update --clear-role` works only on `epic` and `spec`. An epic/spec that already has a role still loads and accepts other updates and `tz close`. Filter with `-r/--role` on `tz list` and `tz ready` (`none` matches issues with no role) and `tz export`. |
| `effort` | no | Integer `1`–`5`; `5` is the most effort | Omitted when unset. Set with `tz create/update --effort`; clear with `update --clear-effort`. Values outside 1–5 are rejected, including in hand-edited files. |
| `external_ref` | no | Any text (e.g. a URL or ticket key in another tracker) | Omitted when unset. |
| `labels` | no | List of free-form strings | Stored sorted and de-duplicated; empty entries are dropped. Commas separate labels in `--labels`, `--set-labels`. |
| `dependencies` | no | Map of issue ID to [dependency type](#dependency-types) | Sorted by ID. |
| `created_at` | yes | RFC 3339 UTC timestamp | Set on creation. |
| `updated_at` | yes | RFC 3339 UTC timestamp | Refreshed on every change. |
| `closed_at` | no | RFC 3339 UTC timestamp | Set by `tz close`, cleared by `tz reopen`. Only present on closed issues. |

## Body sections

These are Markdown headings in the issue body. Each is free text. Whether it
may be empty depends on the issue type (below).

| Section | Heading | `--field` name | Create/update flag |
|---------|---------|----------------|--------------------|
| Description | `# Description` | `description` (alias `desc`) | `-d/--description`, `--description-file` |
| Design | `# Design` | `design` | `--design` |
| Acceptance criteria | `# Acceptance Criteria` | `acceptance` (alias `acceptance_criteria`) | `--acceptance` |
| Notes | `# Notes` | `notes` | `--notes` (`update`) |

`--field` (used by `tz update --search/--replace` and `--append`) also accepts
`title`.

### Sections by issue type

| Type | Description | Design | Acceptance Criteria | Notes |
|------|-------------|--------|---------------------|-------|
| `task`, `bug`, `feature`, `chore` | optional | optional | optional | optional |
| `epic` | optional | forbidden | optional, defaults to `- All child issues closed.` on create | optional |
| `spec` | **required** | forbidden | forbidden | optional |
| `gate` | **required** | forbidden | **required** | optional |

- **Required:** the section must contain non-whitespace text.
- **Forbidden:** the section must be empty (whitespace only counts as empty).
- `tz create`, `tz triage promote` and `tz update` enforce the table as a hard
  error. `create` and `promote` check every section; `update` checks only the
  sections the command changes, after the change (see `tz update`).
- **Loading never fails on these rules.** Reading, listing, `tz show`, the TUI,
  `ready`, `close`, `reopen`, `claim`, labels, dependencies, comments, export
  and the merge driver ignore them. An issue that already breaks the table (an
  epic with Design, a spec with Acceptance Criteria, a gate with no Acceptance
  Criteria) keeps working: it accepts every update that doesn't touch the
  offending section. A forbidden section can be cleared (`--design ""`) but not
  set, appended to or search/replaced into a non-empty result; a required one
  fails only when a command touches it and leaves it empty.

## Status

| Value | Meaning |
|-------|---------|
| `open` | Not started. Open issues with no unresolved blockers are reported by `tz ready`. |
| `in_progress` | Being worked on. Set by `tz claim`. |
| `closed` | Complete. Sets `closed_at`. |
| `blocked` | Marked as blocked by hand (`tz block`). Not listed by `tz ready`. |
| `deferred` | Postponed (`tz defer`). Not listed by `tz ready`. |

Status values are lowercase with underscores. Only `open` issues appear in `tz ready`. Note that `tz blocked` reports
issues that have unresolved `blocked_by` dependencies, their own or inherited
from a `child_of` ancestor; the issue's own `blocked` status value does not
matter, though an ancestor's does.

## Issue type

| Value | Typical use |
|-------|-------------|
| `bug` | Something is broken. |
| `feature` | New functionality. |
| `task` | General work item (default). |
| `epic` | Large body of work grouping other issues. No Design; Acceptance Criteria defaults to `- All child issues closed.` |
| `gate` | Gate / checkpoint issue. Description is the condition, Acceptance Criteria how to check it; both required, no Design. |
| `chore` | Maintenance work. |
| `spec` | Specification or design document. The whole spec is the Description (required); no Design or Acceptance Criteria. |

Which [body sections](#sections-by-issue-type) each type may use is enforced
by `tz create`, `tz triage promote` and `tz update`.

## Dependency types

Dependencies are stored on the dependent issue: `A: {B: blocked_by}` means A
depends on B.

| Value | Meaning |
|-------|---------|
| `blocked_by` | Default. The issue is not ready until the target is closed. The only type that affects `tz ready` and `tz blocked`. Inherited: an issue is also not ready while any `child_of` ancestor has an open (or missing) `blocked_by` target or is in `blocked` status. A closed ancestor does not block. |
| `related_to` | Informational link. |
| `child_of` | The issue is a child of the target (e.g. an epic). An issue has at most one parent; adding a second is an error (remove the first with `tz dep remove`). |
| `discovered_from` | The issue was discovered while working on the target. |
| `tracks` | The issue tracks the target. |

The legacy spelling `discovered-from` is still accepted when reading, but files are written with `discovered_from`.

## Claims

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
