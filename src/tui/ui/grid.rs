//! Issues grid rendering (spec §4 main area, §5.1–§5.5).
//!
//! Columns are 32 wide and laid out left to right from the horizontal scroll
//! offset, as many as fit whole. Each has a one-row title bar (group value, or
//! the epic's ID and title, plus the cell count) and then whole 32×7 cells
//! from the column's vertical offset; the epic header is the first cell of an
//! epic column. Scroll offsets come from [`GridState`], which keeps the
//! selected cell fully visible; nothing here draws scrollbars.

use super::cell::{truncate_with_ellipsis, IssueCell, CELL_HEIGHT, CELL_WIDTH};
use super::layout::{COLUMN_TITLE_HEIGHT, MIN_COLUMN_WIDTH};
use super::theme;
use crate::tui::app::App;
use crate::tui::model::blockers::has_open_blockers;
use crate::tui::model::columns::Column;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Frame;

pub const NO_MATCHES_MESSAGE: &str = "No matching issues";

const RULE: &str = "─";

/// Draw the issues grid into `area`.
pub fn draw_grid(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let grid = &app.grid;
    if grid.columns().is_empty() {
        draw_empty(frame, area);
        return;
    }
    let max_columns = usize::from((area.width / MIN_COLUMN_WIDTH).max(1));
    let max_rows =
        usize::from((area.height.saturating_sub(COLUMN_TITLE_HEIGHT) / CELL_HEIGHT).max(1));
    let selected = grid.selected_column().zip(grid.selected_row());

    let first = grid.col_offset();
    for (slot, (ci, column)) in grid
        .columns()
        .iter()
        .enumerate()
        .skip(first)
        .take(max_columns)
        .enumerate()
    {
        let x = area.x + slot as u16 * CELL_WIDTH;
        let width = CELL_WIDTH.min(area.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        let selected_row = selected.and_then(|(c, r)| (c == ci).then_some(r));
        let buf = frame.buffer_mut();
        draw_title(
            buf,
            Rect::new(x, area.y, width, COLUMN_TITLE_HEIGHT),
            column,
            selected_row.is_some(),
        );

        let top = grid.row_offset(&column.key);
        for (n, row) in (top..column.row_count()).take(max_rows).enumerate() {
            let y = area.y + COLUMN_TITLE_HEIGHT + n as u16 * CELL_HEIGHT;
            if y + CELL_HEIGHT > area.bottom() {
                break;
            }
            let Some(issue) = column.id_at(row).and_then(|id| app.snapshot.issue(id)) else {
                continue;
            };
            let dimmed = row == 0 && column.header.as_ref().is_some_and(|h| h.dimmed);
            let cell = IssueCell::new(issue, has_open_blockers(issue, &app.snapshot))
                .selected(selected_row == Some(row))
                .dimmed(dimmed);
            cell.render(Rect::new(x, y, width, CELL_HEIGHT), frame.buffer_mut());
        }
    }
}

/// `━ title ──── count ` on one row: the title is cut with `…` to leave room
/// for the count.
fn draw_title(buf: &mut Buffer, area: Rect, column: &Column, selected: bool) {
    let width = usize::from(area.width);
    let count = format!(" {} ", column.count);
    let count_w = count.chars().count();
    let title_room = width.saturating_sub(count_w + 1 + 2);
    let title = format!(" {} ", truncate_with_ellipsis(&column.title, title_room));

    let rule = theme::column_rule_style();
    for x in 0..area.width {
        buf[(area.x + x, area.y)].set_symbol(RULE).set_style(rule);
    }
    buf.set_stringn(
        area.x,
        area.y,
        &title,
        width,
        theme::column_title_style(selected),
    );
    let count_x = area.x + area.width.saturating_sub(count_w as u16);
    buf.set_stringn(count_x, area.y, &count, count_w, rule);
}

fn draw_empty(frame: &mut Frame, area: Rect) {
    let [_, mid, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(NO_MATCHES_MESSAGE)
            .alignment(Alignment::Center)
            .style(theme::empty_message_style()),
        mid,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::{update, Action, Dir};
    use crate::tui::data::Snapshot;
    use crate::tui::testing::*;
    use crate::tui::ui::layout::{grid_capacity, MIN_HEIGHT};
    use crate::tui::{GroupByArg, ModeArg};
    use crate::types::{IssueType, Role, Status};

    /// Rows of the screen above the grid (mode and filter bars).
    const TOP_BARS: usize = 2;

    fn app_at(snap: Snapshot, g: GroupByArg, size: (u16, u16)) -> App {
        let mut app = App::from_snapshot(ModeArg::Issues, snap);
        app.grouping = g;
        update(&mut app, Action::Resize(size.0, size.1));
        app.refresh_grid();
        app
    }

    /// The grid region of the rendered screen (bars cut off).
    fn grid_text(app: &mut App, size: (u16, u16)) -> String {
        render_app(app, size.0, size.1)
            .lines()
            .skip(TOP_BARS)
            .take(usize::from(size.1) - 3)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn press(app: &mut App, action: Action) {
        update(app, action);
    }

    /// A little of everything: two epics (one closed, so its header is
    /// dimmed), a nested epic, an orphan, mixed roles/types/priorities.
    fn fixture() -> Snapshot {
        snapshot_of([
            epic("e-1", "Auth rework").priority(1),
            epic("e-2", "Old platform work").closed().priority(2),
            epic("e-3", "Nested sessions epic")
                .child_of("e-1")
                .priority(2),
            issue("t-1", "Login page")
                .ty(IssueType::Feature)
                .role(Role::Agent)
                .child_of("e-1")
                .priority(0)
                .effort(2),
            issue("t-2", "Fix redirect")
                .ty(IssueType::Bug)
                .role(Role::Human)
                .status(Status::InProgress)
                .child_of("e-2")
                .priority(1),
            issue("t-3", "Orphan chore")
                .ty(IssueType::Chore)
                .priority(3),
            issue("t-4", "Blocked task")
                .blocked_by("t-3")
                .status(Status::Blocked)
                .priority(2),
        ])
    }

    const WIDE: (u16, u16) = (128, 25);

    #[test]
    fn grid_by_epic_with_dimmed_header_and_nested_epic() {
        let mut app = app_at(fixture(), GroupByArg::Epic, WIDE);
        let cols: Vec<_> = app.grid.columns().iter().map(|c| &c.title).collect();
        assert_eq!(cols.len(), 4, "{cols:?}");
        insta::assert_snapshot!(grid_text(&mut app, WIDE));
    }

    #[test]
    fn dimmed_epic_header_is_dimmed_in_styles() {
        let mut app = app_at(fixture(), GroupByArg::Epic, WIDE);
        let out = render_app_styled(&mut app, WIDE.0, WIDE.1);
        // The closed e-2 (third column, x 64..96) is the only dimmed epic.
        let dimmed = theme::dim(theme::type_style(IssueType::Epic)).fg.unwrap();
        let needle = format!("fg={dimmed:?}");
        let starts: Vec<u16> = out
            .lines()
            .filter(|l| l.contains(&needle))
            .map(|l| {
                l.split(':')
                    .nth(1)
                    .unwrap()
                    .split("..")
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect();
        assert!(!starts.is_empty());
        assert!(starts.iter().all(|x| (64..96).contains(x)), "{starts:?}");
    }

    #[test]
    fn grid_by_status() {
        let mut app = app_at(fixture(), GroupByArg::Status, WIDE);
        insta::assert_snapshot!(grid_text(&mut app, WIDE));
    }

    #[test]
    fn grid_by_role() {
        let mut app = app_at(fixture(), GroupByArg::Role, WIDE);
        insta::assert_snapshot!(grid_text(&mut app, WIDE));
    }

    #[test]
    fn grid_by_type() {
        let mut app = app_at(fixture(), GroupByArg::Type, WIDE);
        insta::assert_snapshot!(grid_text(&mut app, WIDE));
    }

    #[test]
    fn grid_by_priority() {
        let mut app = app_at(fixture(), GroupByArg::Priority, WIDE);
        insta::assert_snapshot!(grid_text(&mut app, WIDE));
    }

    #[test]
    fn horizontally_scrolled_grid_with_selection_in_far_column() {
        let size = (70, 12);
        let mut app = app_at(fixture(), GroupByArg::Status, size);
        press(&mut app, Action::Move(Dir::Right));
        press(&mut app, Action::Move(Dir::Right));
        assert_eq!(app.grid.col_offset(), 1);
        insta::assert_snapshot!(grid_text(&mut app, size));
    }

    #[test]
    fn vertically_scrolled_column() {
        let size = (40, 3 + 1 + 14);
        let snap = snapshot_of(
            (0..5).map(|i| issue(&format!("v-{i}"), &format!("Item number {i}")).priority(i)),
        );
        let mut app = app_at(snap, GroupByArg::Status, size);
        for _ in 0..3 {
            press(&mut app, Action::Move(Dir::Down));
        }
        assert_eq!(app.grid.row_offset(&app.grid.columns()[0].key), 2);
        let text = grid_text(&mut app, size);
        assert!(!text.contains("v-0") && !text.contains("v-1"));
        assert!(text.contains("v-2") && text.contains("v-3"));
        insta::assert_snapshot!(text);
    }

    #[test]
    fn no_matching_issues_is_centered() {
        let size = (60, 12);
        let mut app = app_at(
            snapshot_of(Vec::<IssueBuilder>::new()),
            GroupByArg::Epic,
            size,
        );
        app.search.issues = "zzz".into();
        app.refresh_grid();
        let out = render_app(&mut app, size.0, size.1);
        let (y, line) = out
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains(NO_MATCHES_MESSAGE))
            .expect("message");
        assert_eq!(line.find('N'), Some((60 - NO_MATCHES_MESSAGE.len()) / 2));
        // Middle of the main area (rows 2..11).
        assert_eq!(y, 2 + (12 - 3) / 2);
        insta::assert_snapshot!(grid_text(&mut app, size));
    }

    #[test]
    fn long_epic_title_is_truncated_and_count_shown() {
        let snap = snapshot_of([
            epic("e-1", "An extremely long epic title that cannot fit"),
            issue("t-1", "a").child_of("e-1"),
            issue("t-2", "b").child_of("e-1"),
        ]);
        let size = (70, 12);
        let mut app = app_at(snap, GroupByArg::Epic, size);
        let text = grid_text(&mut app, size);
        // The epic's own column is the only one ("No epic" is empty, so hidden).
        let title: String = text.lines().next().unwrap().chars().take(32).collect();
        assert!(title.contains("e-1 An extremely"), "{title}");
        assert!(title.contains('…'));
        assert_eq!(title.chars().count(), 31, "{title}"); // 32 wide, trailing space trimmed
        assert!(title.trim_end().ends_with(" 2"), "{title}");
    }

    #[test]
    fn selected_cell_is_always_fully_on_screen() {
        let many = snapshot_of((0..30).map(|i| {
            issue(&format!("m-{i:02}"), "Many")
                .priority(i % 5)
                .status(if i % 3 == 0 {
                    Status::Open
                } else {
                    Status::Deferred
                })
                .role(if i % 2 == 0 { Role::Agent } else { Role::Human })
        }));
        for size in [(32, MIN_HEIGHT), (40, 12), (70, 20), (100, 26), (160, 30)] {
            for g in [GroupByArg::Status, GroupByArg::Role, GroupByArg::Priority] {
                let mut app = app_at(many.clone(), g, size);
                let moves = [
                    Action::Move(Dir::Right),
                    Action::Move(Dir::Down),
                    Action::ColumnPageDown,
                    Action::Move(Dir::Right),
                    Action::ColumnBottom,
                    Action::Move(Dir::Right),
                    Action::Move(Dir::Left),
                    Action::ColumnPageUp,
                    Action::ColumnTop,
                    Action::Move(Dir::Down),
                ];
                for action in moves {
                    press(&mut app, action.clone());
                    let id = app.grid.selected_id().unwrap().to_string();
                    let (_, rows) = grid_capacity(size);
                    let _ = rows;
                    let out = render_app(&mut app, size.0, size.1);
                    // The selected cell has a thick border and its whole box is drawn.
                    let tops: Vec<_> = out.lines().filter(|l| l.contains('┏')).collect();
                    let bottoms = out.lines().filter(|l| l.contains('┗')).count();
                    assert_eq!(tops.len(), 1, "{size:?} {g:?} {action:?}\n{out}");
                    assert_eq!(bottoms, 1, "{size:?} {g:?} {action:?}\n{out}");
                    assert!(
                        tops[0].contains(&id),
                        "{size:?} {g:?} {action:?} {id}\n{out}"
                    );
                }
            }
        }
    }

    #[test]
    fn rendering_stays_inside_the_main_area() {
        // Split view: the grid only gets the left half.
        let size = (160, 20);
        let mut app = app_at(fixture(), GroupByArg::Status, size);
        let out = render_app(&mut app, size.0, size.1);
        // The detail pane's border starts right after the left half, so
        // nothing of the grid reaches column 80.
        for (n, line) in out
            .lines()
            .skip(TOP_BARS)
            .take(usize::from(size.1) - 3)
            .enumerate()
        {
            let chars: Vec<char> = line.chars().collect();
            let pane_edge = if n == 0 { '┌' } else { '│' };
            let bottom = n == usize::from(size.1) - 4;
            let want = if bottom { '└' } else { pane_edge };
            assert_eq!(chars.get(80), Some(&want), "{line}");
        }
    }
}
