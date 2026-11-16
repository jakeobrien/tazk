# CLI Reference

Command reference for `tz`. For installation and an overview, see the
[README](../README.md). Related docs: [issue properties](issues.md), the
[terminal UI](tui.md) and the [git merge driver](merge-driver.md).

Run `tz quickstart` for a comprehensive guide.

## Core Commands

- `tz init [--tz-hash-ids[=true|false]] [--tz-issue-layout flat|sharded]` - Initialize a database
- `tz create TITLE -r human|agent [OPTIONS]` - Create new issue (status `open`). `-r/--role` is required for `task`, `bug`, `feature`, `chore` and `gate`, and rejected for `epic` and `spec`, which take no role.
  - Common options: `-p PRIORITY`, `-t TYPE`, `-d TEXT`, `-a ASSIGNEE`, `--effort 1-5`, `-l LABEL`
  - Body sections are checked against the issue's type (see [Body sections](issues.md#body-sections)); nothing is created when a check fails:
    - `--design` with text on an `epic`, `spec` or `gate` fails (`tz-4f1 is an epic; epics have no Design section...`).
    - `--acceptance` with text on a `spec` fails.
    - A `spec` or `gate` needs a non-empty description (`-d` / `--description-file`).
    - A `gate` needs non-empty `--acceptance`: how to check that its condition is true.
    - An `epic` created without `--acceptance`, or with a blank one, gets `- All child issues closed.` as its Acceptance Criteria. An explicit `--acceptance` replaces that default, so include the line yourself if you still want it.
- `tz list [FILTERS]` - List issues with optional filters (including `--assignee USER` and `-r/--role ROLE`, where ROLE is `human`, `agent` or `none` for issues with no role)
- `tz show ISSUE_ID` - Show detailed issue information
- `tz update ISSUE_ID [OPTIONS]` - Update issue fields (not status)
  - `-r/--role ROLE`, `--effort N` set the role or effort; `--clear-role` and
    `--clear-effort` remove them. `epic` and `spec` take no role: `-r` fails on
    them, and `--clear-role` is the only role change allowed (it succeeds only
    there; it fails on every other type).
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
  - Only the body sections a command changes are checked against the issue's
    type (see [Body sections](issues.md#body-sections)), on the value they will have
    afterwards, for `-d/--description`, `--description-file`, `--design`,
    `--acceptance`, `--notes`, `--search/--replace` and `--append` on any body
    `--field`. A change that would leave a forbidden section non-empty or a
    required one empty fails and the issue is left untouched; `--field title`
    and every other flag are never checked. For example, `--design ""` clears a
    Design and succeeds on any type, while `--design "x"` fails on an epic. When
    a rejected write is about a forbidden section that already has text, the
    error says that clearing it is allowed. With several IDs each issue is
    checked against its own type, and the first failure stops the command
    (issues before it stay updated). The epic default Acceptance Criteria is only
    set on `create`; `--acceptance ""` removes it.
- `tz claim ISSUE_ID... --as NAME [--team TEAM] [--release] [--force]` - Claim or release issues
- `tz block ISSUE_ID... [--release] [--reason TEXT]` - Mark issues `blocked`, or back to `open` with `--release`. A reason is appended to Notes as `Blocked <UTC timestamp>: <reason>` (`Unblocked ...` on release)
- `tz defer ISSUE_ID... [--release] [--reason TEXT]` - Same as `tz block`, for `deferred` (`Deferred ...` / `Undeferred ...` notes)
- `tz close ISSUE_ID... [--reason REASON]` - Close (complete) an issue. A reason is appended to Notes as `Closed <UTC timestamp>: <reason>`
- `tz reopen ISSUE_ID... [--role human|agent] [--reason REASON]` - Reopen closed issues. A reason is appended to Notes as `Reopened <UTC timestamp>: <reason>`. A work issue (not `epic` or `spec`) with no role is refused until you resubmit with `--role`; `epic` and `spec` need no role and reject `--role`
- `tz renumber ISSUE_ID [--to SUFFIX_OR_ID] [--dry-run] [--tz-patch-code]` - Give an issue a new ID, for fixing duplicate IDs (see [Duplicate IDs](merge-driver.md#duplicate-ids-after-a-merge)). The ID is always canonical and in the store's format:
  - Numeric store (`--tz-hash-ids=false`): without `--to`, the next free number (highest + 1); `--to` must be a free numeric ID (`--to 20` or `--to tz-20`).
  - Hash store (default, `tz-hash-ids: true`): without `--to`, a freshly generated hash; `--to` must be a free hash ID.
  - `--to` in the other format, or naming an existing issue, is an error.
  - Dependencies and parents in all issues, ID mentions in issue text, comments and triage items are rewritten in one transaction. `--dry-run` lists the changes without making them. `--tz-patch-code` also offers to patch references in code (interactive TTY only).
- `tz repair [--dry-run]` - Remove references (dependencies, parents) to issues that no longer exist
- `tz comments add ISSUE_ID --body TEXT --author NAME` - Add a local issue comment
- `tz comments list ISSUE_ID` - List local issue comments
- `tz comments delete ISSUE_ID COMMENT_ID...` - Delete local issue comment(s) by ID

## Triage

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
- `tz triage promote NAME... --title TITLE [-r human|agent] [OPTIONS]` - Create
  one issue from one or more items and delete the items. Accepts the same
  options as `tz create` (`-p`, `-t`, `-d` /
  `--description-file`, `--design`, `--acceptance`, `-a`, `--effort`,
  `-l`/`--labels`, `--external-ref`, `--deps`, `--parent`). The per-type
  body-section rules of `tz create` apply (nothing is promoted and no item is
  deleted when one fails): a `gate` still needs `--acceptance`, and an `epic`
  gets the default Acceptance Criteria. Without a
  description flag the description is the items' text, in the order given,
  separated by `---` lines, so a promoted `spec` or `gate` has a description
  unless `-d ""` is given. The issue's Notes record the item names
  (`Promoted from triage: ...`), so the originals can be found in git history.
- `tz triage attach NAME... --to ISSUE_ID --author NAME` - Add each item as a
  comment on an existing issue (one comment per item, in order) and delete the
  items. `--author` is required.

`promote`, `attach` and `rm` are transactions: the issue or comments are
written and the items deleted together, or nothing changes. Triage items are
not read by `list`, `ready`, `export` or `sync` (`stats` only counts them), and
the merge driver leaves them to git's ordinary line merge.

## Dependencies

- `tz dep add ISSUE_ID --target DEPENDENCY_ID [--type TYPE]` - Add dependency (ISSUE_ID has the dependency on DEPENDENCY_ID)
  - Types: `blocked_by` (default), `related_to`, `child_of`, `discovered_from`, `tracks`
- `tz dep remove ISSUE_ID --target DEPENDENCY_ID` - Remove dependency

## Queries

- `tz ready [--assignee USER] [-r/--role ROLE|none] [--priority N]` - Find ready work: open issues with no blockers
  - Printed as `id: title [priority: N, effort: N]` (effort omitted when unset).
  - Blockers inherited from `child_of` ancestors count.
  - `epic` and `spec` issues are hidden unless requested with `--type`.
- `tz blocked` - Show blocked issues and what blocks them
  - Blockers inherited from `child_of` ancestors are listed as
    `target (inherited from ANCESTOR)`, or `ANCESTOR (blocked status, inherited)`
    when the ancestor is in `blocked` status.
  - `--json` keeps `blocked_by` for the issue's own blockers and adds
    `inherited_blocked_by`: a list of `{ancestor, blocked_by, blocked_status}`.
    `blocked_by_count` covers both.
- `tz stats` - Show statistics (total, open, blocked, average lead time, and the
  number of triage items with the age of the oldest)
  - `--json` adds `triage_items` and `oldest_triage_item_age_hours` (`null`
    when there is no timestamped item).

## Options

- `--tz-dir PATH` - Path to .tazk directory
- `--json` - Output JSON format
- `--tz-no-cmd-logging` - Disable command history logging

## Environment Variables

- `TAZK_DIR` - Path to the `.tazk` directory
