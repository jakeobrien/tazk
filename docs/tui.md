# Terminal UI

`tz tui` is a **read-only**, full-screen browser for issues and triage items. It
never writes to `.tazk/`; the only way to edit from it is to hand a file to
`$EDITOR` (`o`). It honours `--tz-dir` / `TAZK_DIR` like every other command and
exits with the usual error if no `.tazk` directory is found.

```bash
tz tui                                   # all issues except closed, grouped by epic
tz tui --mode triage                     # start in the triage inbox
tz tui --group-by role -r agent -s open  # open agent work, one column per role
tz tui -s open,in_progress --search login
```

Flags only set the **initial** view; nothing is persisted between sessions.
`VALUES` are repeatable and/or comma-separated (like `tz list -p 0,1`).

| Flag | Meaning | Default |
|------|---------|---------|
| `--mode issues\|triage` | Starting mode | `issues` |
| `--group-by epic\|status\|role\|type\|priority` | Starting grouping | `epic` |
| `-r, --role VALUES` | Role filter: `human`, `agent`, `none` (unset) | all |
| `-s, --status VALUES` | Status filter: `open`, `in_progress`, `blocked`, `deferred`, `closed` | all except `closed` |
| `-t, --type VALUES` | Type filter: `epic`, `spec`, `feature`, `bug`, `task`, `gate`, `chore` | all |
| `-p, --priority VALUES` | Priority filter, e.g. `0,1` | all |
| `--effort VALUES` | Effort filter: `1`-`5` or `none` (unset) | all |
| `--search TEXT` | Initial search query (issues mode) | empty |

**Layout.** A mode bar (`Issues (N) | Triage (M)`), a filter bar, the grid, and a
hint bar. Issues are shown as 32x7 cells arranged in columns by group; empty
columns are hidden. Each cell shows ID, title, role, status, priority, effort
and type; `⊘` in the top border marks an issue with open (or missing)
`blocked_by` targets. Triage items are a plain wrapped grid, oldest first.
On terminals at least 160 columns wide the detail pane sits beside the grid
and follows the selection; on narrower terminals `Enter` opens the detail
full-screen. The detail view shows properties, relationships (parent, blockers,
children, ...) and the markdown body; `Tab`/`Shift-Tab` highlight a
relationship and `Enter` follows it (`Esc` goes back).

**Keys.** Press `?` in the TUI for the full, always-current list. The main ones:

| Key | Action |
|-----|--------|
| arrows, `h` `j` `k` `l` | Move the selection |
| `Enter` / `Esc` | Open detail / go back (or clear an active search) |
| `/` | Search (live, case-insensitive; ID, title and body; `Enter` keeps, `Esc` clears, `Ctrl-K` empties) |
| `g` | Cycle grouping: epic, status, role, type, priority |
| `r` `s` `t` `p` `e` | Role / status / type / priority / effort filter picker (`Space` toggles, `a`/`n` all/none, `Enter` applies, `Esc` cancels) |
| `c` | Reset filters to the default |
| `m` | Switch between Issues and Triage |
| `y` | Copy the selected issue ID or triage filename to the clipboard |
| `o` | Open the selected file in `$EDITOR` (falls back to `vi`); the TUI resumes and reloads on exit |
| `R` | Reload now |
| `Ctrl-U` / `Ctrl-D` | Scroll the detail half a page (works from the grid in split view) |
| `?` | Help overlay |
| `q`, `Ctrl-C` | Quit |

**Refresh.** The data is reloaded when the terminal gains focus, every 10
seconds, and on `R`. The selection, filters, search and grouping are kept.
Reloading only takes a read lock, so the TUI can stay open while other `tz`
commands run. Focus reporting depends on your terminal; inside tmux add
`set -g focus-events on` to `~/.tmux.conf`, otherwise only the timer and `R`
refresh.

**Skipped files.** A file that cannot be read or parsed (invalid YAML, an
unresolved merge-conflict hunk, ...) is skipped and the rest load normally. The
filter bar shows `⚠ N skipped`; the help overlay (`?`) lists the first skipped
path and its error.

**Building without it.** The TUI sits behind the `tui` cargo feature, enabled
by default. `cargo build --no-default-features` produces a `tz` without the
`tui` subcommand.

## See also

- [CLI reference](cli.md) for the other `tz` commands.
