# tazk TUI — v1 Spec

Status: agreed design, not yet implemented.

## 1. Summary

`tz tui` is a **read-only**, full-screen terminal UI for browsing tazk issues and
triage items. It is built with [`ratatui`](https://ratatui.rs) (crossterm
backend) and has two modes (Issues, Triage), each with a **grid** of item cells
and a **detail view**. On wide terminals the grid and the detail view are shown
side by side.

### Out of scope for v1

- Any write operation (status changes, edits, claims, triage promotion). The only
  "edit" path is handing the file to `$EDITOR` (§9.3); the TUI itself never
  writes.
- Comments (`.tazk/comments/`).
- Filtering by label or assignee.
- Sort controls (sort order is fixed, §5.3).
- Mouse input.
- 16-color terminals and `NO_COLOR` (truecolor/256-color assumed).
- Persisting view state between sessions.
- A "no grouping" (plain wrapped) layout for issues.
- Special rendering of nested epics (§5.2).

### Issue types

The TUI supports all seven issue types: `epic`, `spec`, `feature`, `bug`,
`task`, `gate`, `chore`. Wherever types are listed (type grouping, type filter,
picker, colors) all seven appear, in that order.

## 2. Launch and packaging

- Subcommand `tz tui` in the existing `tz` binary.
- All TUI code and dependencies sit behind a cargo feature `tui`, **enabled by
  default**. Building with `--no-default-features` yields a `tz` without the
  subcommand.
- New dependencies (all optional, enabled by `tui`):
  - `ratatui` (+ `crossterm`)
  - `tui-markdown` with its default `highlight-code` feature (syntax
    highlighting via `syntect`)
  - Versions must be chosen so `tui-markdown`'s `ratatui-core` matches
    `ratatui`'s.
- Respects the global `--tz-dir` option and `TAZK_DIR`, and the normal `.tazk`
  discovery, exactly like every other command.
- Code lives in its own module tree (e.g. `src/tui/`), not in `main.rs`.

### 2.1 Command-line flags

Flags set the **initial** view state only; nothing is persisted.

| Flag | Meaning | Default |
|------|---------|---------|
| `--mode issues\|triage` | Starting mode | `issues` |
| `--group-by epic\|status\|role\|type\|priority` | Starting grouping | `epic` |
| `-r/--role VALUES` | Initial role filter | all |
| `-s/--status VALUES` | Initial status filter | all except `closed` |
| `-t/--type VALUES` | Initial type filter | all |
| `-p/--priority VALUES` | Initial priority filter | all |
| `--effort VALUES` | Initial effort filter | all |
| `--search TEXT` | Initial search query (issues mode) | empty |

`VALUES` are repeatable and/or comma-separated (like `tz list -p 0,1`). The
literal `none` selects issues with the property unset (role, effort).

## 3. Data loading and refresh

### 3.1 Sources

- Issues: every issue file under `.tazk/issues/`, including closed ones (closed
  issues are needed for epic headers, blocker resolution and the status filter).
- Reverse dependencies (`Issue::dependents`) computed over the full set, as
  `storage.list_issues` already does.
- Triage items: `.tazk/triage/*.md` via the existing triage storage API.

### 3.2 Bad files

A file that fails to read or parse (invalid YAML, an unresolved merge-conflict
hunk, an invalid effort, etc.) is **skipped**; the rest load normally. The count
of skipped files is shown in the filter bar (`⚠ N skipped`). The first skipped
path and its error should be shown in the help overlay or a similar spot, so
the user can find it. (Exact placement is left to implementation.)

> Implementation note: the current issue loader
> (`list_all_issues_no_dependents` / `list_issues`) aborts on the first bad
> file. The TUI needs a tolerant variant that returns `(issues, skipped)`. Add
> it to `storage` rather than duplicating the parsing logic.

### 3.3 Refresh triggers

The full data set is reloaded:

1. When the terminal **gains focus** (crossterm `EnableFocusChange` /
   `Event::FocusGained`). Terminals that do not report focus simply never send
   the event; tmux needs `set -g focus-events on`.
2. Every **10 seconds** while running.
3. On **`R`** (manual).

Reloading must not block input noticeably. Reads take at most a short read lock
and never take the write lock.

### 3.4 State preserved across reload

- The selection stays on the same item (by issue ID / triage filename) if it is
  still visible. Otherwise it follows the "selection disappears" rule (§5.6).
- The detail pane's link stack is kept if every ID in it still exists.
  Otherwise it is cleared.
- Scroll positions are kept where possible.
- Filters, search and grouping are unchanged.

## 4. Screen layout

```
 Issues (42) │ Triage (3)                               group: epic
 role:all  status:!closed  type:all  pri:all  effort:all  /login   12 of 42   ⚠ 1 skipped
┌ No epic ─────────┐┌ tz-3 Auth rework ┐┌ tz-9 Sync v2 ────┐ │ detail pane (≥160 cols)
│ [cell]           ││ [epic header]    ││ [epic header]    │ │
│ [cell]           ││ [cell]           ││ [cell]           │ │
└──────────────────┘└──────────────────┘└──────────────────┘ │
 ? help  / search  … (key hints)
```

From top to bottom:

1. **Mode bar**: `Issues (N) │ Triage (M)`, with the current mode highlighted.
   N is the total number of issues loaded and M the total number of triage
   items. The current grouping is shown on the right (issues mode only).
2. **Filter bar** (always visible):
   - Issues mode: each filter's current value, the search query (if any),
     `X of Y` (visible / total), and `⚠ N skipped` if any.
   - Triage mode: search query, `X of Y`, skipped count.
   - A filter shows `all` when every value is selected, `!v` when exactly one
     value is excluded, and otherwise a comma list of the selected values.
3. **Main area**: the grid, or grid + detail pane (§8), or a full-screen detail
   view.
4. **Hint bar**: the most relevant keys for the current context. Keys that don't
   apply in the current context (e.g. filter keys in triage mode) are not shown.

**Terminal too small:** if the terminal can't fit at least one column (32 cols)
plus the bars, or one cell's height, show a centered "Terminal too small"
message instead of the UI.

## 5. Issues grid

### 5.1 Grouping

Issues are always arranged in **columns**. `g` cycles the grouping:
**epic → status → role → type → priority → epic**. Default: `epic`.

Each column has a title bar showing its group value (or, for epic columns, the
epic's ID and title) and the count of cells in it.

**Empty columns are hidden** in every grouping.

### 5.2 Grouping by epic

Rule:

1. Take the **visible** issues (those passing filters and search, §6–7).
2. Each visible issue's **parent** is its `child_of` target. If it has more than
   one `child_of` dependency, use the **lowest target ID** in the order the file
   stores them (sorted by ID).
3. Group visible issues by parent. A group becomes an epic column if its parent
   exists **and** is of type `epic`. Issues with no parent, a missing parent, or
   a non-epic parent go in the **"No epic"** column.
   Epics that are not children of another epic are **excluded** from "No epic":
   each already appears as the header of its own column (rule 4).
4. Every **visible epic** also gets its own column, even if it has no visible
   children (the column then holds only its header).
5. The column's **header** is the epic's cell, looked up from the **full** issue
   set. If the epic itself does not pass the current filters (e.g. it is closed,
   or filtered out by role), the header is still shown but **dimmed**, with its
   status badge visible.
6. A visible epic that is itself a child of another epic appears **both** as a
   regular cell in its parent's column **and** as the header of its own column.
   There is no nested rendering.
7. Non-epic issues with children get no column. Their children are listed only in
   the detail view's relationships (§9.2).

The epic header is the top cell of its column. It is selectable and opens the
epic's detail like any other cell. The "No epic" column has a plain title bar
and no header cell.

**Column order:** "No epic" first, then epic columns sorted by the epic's
priority (ascending, 0 first), then `updated_at` (newest first), then ID.

### 5.3 Grouping by status, role, type or priority

One column per value, in value order. A `(none)` column comes last where the
property can be unset.

| Grouping | Column order |
|----------|--------------|
| status | open, in_progress, blocked, deferred, closed |
| role | agent, human, (none) |
| type | epic, spec, feature, bug, task, gate, chore |
| priority | P0, P1, P2, P3, P4, then any other values in numeric order |

Because empty columns are hidden, the `closed` column only appears when the
status filter includes `closed`.

**Cell order within every column:** priority ascending (0 first), then
`updated_at` newest first, then ID. (For epic columns, this applies below the
header.)

### 5.4 Issue cell

Fixed size: **32 columns × 7 rows** including the border. The border is rounded
or plain, and its color shows the issue **type**.

```
┌ tz-14 ──────────────────── ⊘ ┐   ID in the top border; ⊘ = has open blockers
│ Fix Safari login fails on    │   title: bold, up to 3 lines, ends in … if cut
│ redirect after timeout when  │
│ the session cookie is…       │
│ agent  in_prog  P1  ●●○○○    │   badges: role / status / priority / effort
└─────────────────────── bug ──┘   type name in the bottom border, in the type color
```

- **Title**: bold, word-wrapped to at most 3 lines, ending in `…` if cut.
- **Badges row**, in order, each color-coded:
  - **role**: `agent` / `human`, each in its own color; dim `—` if unset.
  - **status**: `open`, `in_prog`, `blocked` (red), `deferred`, `closed`.
  - **priority**: `P0`…`P4`, colored on a scale from red (P0) to dim (P4).
    Values below 0 use the P0 color, values above 4 use the P4 color, and the
    label shows the real value.
  - **effort**: 5 dots, filled to the effort value (`●●○○○` = 2); dim `—` if
    unset.
- **Open-blockers marker** `⊘` (orange) in the top-right border. It means the
  same as in `tz ready`: the issue has at least one `blocked_by` target that is
  not `closed`, and a **missing target counts as unresolved**. This is a
  separate signal from `status: blocked`, and the two are styled differently
  (status = red badge, open blockers = orange border marker).
- **Selected cell**: thick or double border (still in the type color), plus a
  highlighted ID.
- **Dimmed** (epic header that fails the filters, §5.2): all colors dimmed;
  still selectable.

Assignee and labels are not shown on cells.

### 5.5 Navigation and scrolling

- `←/→` (`h`/`l`) move between columns, keeping the row as close as possible.
  `↑/↓` (`j`/`k`) move within a column.
- `PgUp`/`PgDn` move by one screen of cells within the column. `Home`/`End` go to
  the column's top and bottom.
- Columns scroll horizontally, and cells vertically, **following the selection**.
  The selected cell is always fully visible. There are no scrollbars.

### 5.6 When the selection disappears

If the selected item is no longer visible (after a filter, search, regrouping or
reload change), select the nearest cell in the same column (same row index, or
the last cell if the column is now shorter). If the column itself is gone,
select the first cell of the first column. If nothing is visible, show a
centered "No matching issues" message.

## 6. Filters (issues mode only)

Filterable properties: **role, status, type, priority, effort**.

- Each filter is a **set of allowed values**. An issue is visible if every
  property's value is in that property's set.
- Role and effort include a `(none)` value for "unset".
- Priority lists P0–P4 plus any other values present in the data.
- **Default**: every value allowed, except status, which allows everything but
  `closed`. Seeing closed issues is just a status-filter change; there is no
  separate mode.
- `c` resets all filters to the default.
- Epic headers in epic grouping follow the dimming rule in §5.2. Filters never
  hide a column header whose column has visible children.

### 6.1 Picker popup

Each filter key (`r` `s` `t` `p` `e`) opens one shared multi-select popup,
centered over the grid:

```
┌ Status ──────────┐
│ [x] open         │
│ [x] in_progress  │
│ [x] blocked      │
│ [x] deferred     │
│ [ ] closed       │
└ space · a/n · ⏎ ─┘
```

- `↑/↓` (`j`/`k`) move; `Space` toggles; `a` selects all; `n` selects none.
- `Enter` applies, `Esc` cancels (discards the changes).
- Applying with nothing selected is allowed and simply shows no issues.

## 7. Search

- `/` opens a search input in the filter bar. Results filter **live** as you
  type.
- Plain, case-insensitive substring match.
- **Issues**: matches against ID, title, and the full body (all four sections).
  Search combines with the filters using AND.
- **Triage**: matches against the item's full text.
- Each mode keeps its **own** query.
- Input keys: typing and `Backspace` edit; `Ctrl-K` clears the whole query;
  `Enter` keeps the query and returns to the grid; `Esc` clears the query and
  returns to the grid.
- In the grid, `Esc` clears an active search (when the detail pane isn't
  focused).

## 8. Split pane vs full-screen detail

- **Terminal width ≥ 160 columns**: split view. Grid on the left, detail pane on
  the right, **50/50**.
  - The detail pane **always shows the selected grid item** (live follow), except
    while it is showing followed links (§9.4).
  - Focus starts on the grid. `Enter` moves focus to the detail pane. `Esc`
    from the detail pane (with an empty link stack) returns focus to the grid.
  - `Ctrl-U` / `Ctrl-D` in the grid scroll the detail pane by half a page
    without moving focus.
  - The focused pane is marked (e.g. a highlighted border).
- **Width < 160 columns**: grid only. `Enter` opens the detail view
  **full-screen**. `Esc` (with an empty link stack) returns to the grid.
- Crossing the threshold while running (resize) switches layout, keeping the
  selection. If a full-screen detail was open and the window becomes wide, show
  the split with focus on the detail pane. Going from split to narrow with the
  detail pane focused opens the full-screen detail.

## 9. Detail view

### 9.1 Issue detail layout (one scrollable pane)

1. **Title** (bold, wrapped).
2. **Properties block**, as label/value pairs:
   ID, Type, Status, Priority, Role, Effort, Assignee, Labels, External ref,
   Created, Updated, Closed.
   - Colors and badges match the cell.
   - Unset values show as dim `—`. Closed is only shown when set.
   - Timestamps in **local time, absolute, to the minute**
     (e.g. `2025-11-04 14:32`).
   - The open-blockers marker is repeated here if it applies.
3. **Relationships** (§9.2).
4. **Body**, rendered as markdown (§9.3).

### 9.2 Relationships

Sections, each omitted when empty. Each entry shows the target's ID, a status
badge and its title (truncated to one line). A missing target shows its ID and
`(missing)`.

| Section | Source |
|---------|--------|
| Parent | outgoing `child_of` (all of them, if there are several) |
| Blocked by | outgoing `blocked_by` (each with its status, so you can see which are open) |
| Children | reverse `child_of` |
| Blocks | reverse `blocked_by` |
| Related | outgoing and reverse `related_to` |
| Discovered from | outgoing `discovered_from` |
| Discoveries | reverse `discovered_from` |
| Tracks | outgoing `tracks` |
| Tracked by | reverse `tracks` |

Entries in each section are sorted by ID.

### 9.3 Body rendering

- The body is the four sections: Description, Design, Acceptance Criteria,
  Notes. They are rendered in that order, each under its `#` heading, and
  **empty sections are skipped**. (The issue format rejects any other `#`
  heading, so this is the whole body.)
- Rendered with `tui-markdown`, with syntax highlighting enabled for fenced code
  blocks. Wrapped to the pane width.
- An issue with an entirely empty body shows a dim "(no description)".

### 9.4 Following links

- `Tab` / `Shift-Tab` move a highlight through the relationship entries (all
  sections, in display order), scrolling the highlighted entry into view.
- `Enter` on a highlighted entry pushes the target onto the detail view's **link
  stack** and shows the target. A missing target is not followable.
- `Esc` pops the stack (going back one issue). With an empty stack, `Esc` closes
  the full-screen detail or returns focus to the grid (split view).
- Following links **does not move the grid selection** (the target may not even
  be visible in the grid). **Moving the grid selection clears the stack**, and
  the pane goes back to following the selection.

### 9.5 Triage detail

- Header: the filename, plus the creation time parsed from it
  (local time, to the minute) when the name follows the standard
  `<timestamp>-<rand>.md` form.
- The item's full text, rendered as markdown (same renderer). No properties, no
  relationships, so `Tab`/`Enter` do nothing.

### 9.6 Scrolling

`↑/↓` (`j`/`k`) one line; `PgUp`/`PgDn`/`Space` one page; `Ctrl-U`/`Ctrl-D` half
a page; `Home`/`End` top and bottom.

## 10. Triage grid

- A plain wrapped grid: as many 32-column cells per row as fit, filled left to
  right then top to bottom. No grouping, no filters.
- Sorted by filename ascending, which means oldest first.
- Cell (same 32 × 7 size, neutral border color):
  - Top border: readable creation time (`2025-11-04 00:18`) if the name follows
    the standard form, otherwise the filename.
  - Bottom border: the filename, dim.
  - Body: up to **two lines** of the item's text, raw (markdown not rendered),
    whitespace collapsed, ending in `…` if cut.
- Arrow/`hjkl` navigation moves through the wrapped grid. Vertical scrolling
  follows the selection.
- Search (§7) applies.

## 11. Key bindings

`Ctrl-C` quits from anywhere. While typing in the search input, only the
search-input keys below are active.

### Global (all contexts except search input)

| Key | Action |
|-----|--------|
| `q`, `Ctrl-C` | Quit |
| `?` | Help overlay (`?` or `Esc` closes) |
| `R` | Reload now |
| `m` | Cycle mode: Issues ↔ Triage |
| `y` | Copy the selected / displayed issue ID or triage filename to the clipboard |
| `o` | Open the selected / displayed file in `$EDITOR` (suspend the TUI, resume and reload on exit) |

Number keys are reserved for future use.

### Grid

| Key | Action |
|-----|--------|
| arrows, `h` `j` `k` `l` | Move the selection |
| `PgUp` / `PgDn` | Page within the column |
| `Home` / `End` | Top / bottom of the column |
| `Enter` | Open detail (full-screen if narrow; focus the detail pane if split) |
| `/` | Search |
| `Esc` | Clear the active search |
| `g` | Cycle grouping (issues mode) |
| `r` `s` `t` `p` `e` | Role / status / type / priority / effort picker (issues mode) |
| `c` | Reset filters to the default (issues mode) |
| `Ctrl-U` / `Ctrl-D` | Scroll the detail pane half a page up / down (split view only) |

### Detail (full-screen, or the focused pane)

| Key | Action |
|-----|--------|
| `↑` `↓`, `j` `k` | Scroll one line |
| `PgUp` / `PgDn`, `Space` | Scroll one page |
| `Ctrl-U` / `Ctrl-D` | Scroll half a page |
| `Home` / `End` | Top / bottom |
| `Tab` / `Shift-Tab` | Highlight the next / previous relationship |
| `Enter` | Follow the highlighted relationship |
| `Esc` | Back (pop the link stack), then close / unfocus |

### Search input

| Key | Action |
|-----|--------|
| characters, `Backspace` | Edit the query (live filtering) |
| `Ctrl-K` | Clear the query |
| `Enter` | Keep the query, return to the grid |
| `Esc` | Clear the query, return to the grid |

### Picker popup

| Key | Action |
|-----|--------|
| `↑` `↓`, `j` `k` | Move |
| `Space` | Toggle |
| `a` / `n` | Select all / none |
| `Enter` / `Esc` | Apply / cancel |

The help overlay lists these tables, generated from the same keymap definition
the input handler uses, so the help can't drift from the real bindings.

## 12. Colors

Truecolor or 256-color is assumed. Suggested starting palette (to tune by eye):

| Element | Values |
|---------|--------|
| Type (border) | epic = purple, spec = teal, feature = green, bug = red, task = blue, gate = yellow, chore = gray |
| Role | agent = cyan, human = magenta |
| Status | open = default/white, in_prog = yellow, blocked = red, deferred = dim, closed = dim + strikethrough |
| Priority | P0 red → P1 orange → P2 yellow → P3 default → P4 dim |
| Open blockers `⊘` | orange |
| Dimmed epic header | all colors at reduced brightness |

Use only symbols that are one cell wide (`⊘`, `●`, `○`, `—`, `…`). No emoji.

## 13. Error handling

- No `.tazk` directory found: exit before entering the TUI, with the same error
  as other commands.
- Bad files: skipped and counted (§3.2).
- Clipboard unavailable (`y`): show a short message in the hint bar; never crash.
- `$EDITOR` unset (`o`): fall back to `vi`. If launching fails, show a message
  in the hint bar.
- The terminal is always restored (raw mode off, alternate screen left), even on
  panic: install a panic hook that restores it first.

## 14. Testing

- **Unit tests** for the pure parts, with no terminal involved: grouping and
  column building (including the multi-parent, missing-parent, non-epic-parent,
  dimmed-header and nested-epic cases), sorting, filter and search matching,
  open-blockers detection, selection-disappears logic, and link-stack behavior.
- **Snapshot tests** using ratatui's `TestBackend`: the grid in each grouping,
  cells (normal / selected / dimmed / with blockers), split vs full-screen
  detail, the picker popup, triage grid and detail, "too small" and "no
  matching" states.
- Keep the app state and update logic separate from rendering, so key-event
  sequences can be tested against state without drawing.
- The new tolerant loader (§3.2) gets storage-level tests.

## 15. Future (not v1)

- Number-key bindings (reserved).
- Comments in the detail view.
- Label/assignee filters, sort controls, persisted view state.
- Write actions (claim, close, triage promote, …).
