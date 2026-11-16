# Git Merge Driver

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

## Duplicate IDs after a merge

Two branches that each ran `tz create` can hand out the same numeric ID (say
both made `tz-39`). With no merge base the driver cannot combine the two files,
so give one of them a new ID before merging. On the incoming branch, before
merging:

```bash
git checkout feature
tz renumber 39 --to <n>   # <n> above the target branch's highest ID
git add -A && git commit -m "Renumber tz-39 to tz-<n>"
git checkout main && git merge feature
```

`renumber` rewrites that branch's own references to the issue (dependencies,
parents, text mentions, comments, triage items). Without `--to` it picks the
next number after the highest ID on the *current* branch, which may still
collide with the target branch, so choose `<n>` from the target branch's
highest ID. In a hash store collisions are very unlikely; if one happens,
`tz renumber <id>` generates a fresh hash.
