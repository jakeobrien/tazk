//! Issue cell widget: a fixed 32×7 box (spec §5.4).
//!
//! ```text
//! ╭ tz-14 ───────────────────── ⊘ ╮   row 0: ID left, ⊘ right (open blockers)
//! │ Fix Safari login fails on     │   rows 1–3: title (bold, ≤ 3 lines, … if cut)
//! │ redirect after timeout when   │
//! │ the session cookie is…        │
//! │                               │   row 4: spacer
//! │ agent  in_prog  P1  ●●○○○     │   row 5: role / status / priority / effort
//! ╰────────────────────────── bug ╯   row 6: type name, in the type color
//! ```
//!
//! The border is rounded, or thick when selected; its color is the issue type's.
//! Assignee and labels are not shown. The widget never draws outside its
//! 32×7 box, whatever area it is given.

use super::theme;
use crate::types::Issue;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::Span;
use ratatui::widgets::Widget;

pub const CELL_WIDTH: u16 = 32;
pub const CELL_HEIGHT: u16 = 7;
/// Most title lines shown.
pub const TITLE_LINES: usize = 3;
/// Columns available to the title and badges (inside the border, 1 column of
/// padding each side).
pub const CONTENT_WIDTH: usize = CELL_WIDTH as usize - 4;

const ELLIPSIS: char = '…';

// --- Text helpers ------------------------------------------------------------

fn str_width(s: &str) -> usize {
    Span::raw(s).width()
}

fn char_width(c: char) -> usize {
    let mut buf = [0u8; 4];
    str_width(c.encode_utf8(&mut buf))
}

/// `s` shortened to at most `width` columns, ending in `…` only if it was cut.
pub fn truncate_with_ellipsis(s: &str, width: usize) -> String {
    if str_width(s) <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let budget = width - char_width(ELLIPSIS);
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = char_width(c);
        if used + w > budget {
            break;
        }
        out.push(c);
        used += w;
    }
    let mut out = out.trim_end().to_string();
    out.push(ELLIPSIS);
    out
}

/// Word-wrap `title` into at most `max_lines` lines of at most `width` columns.
///
/// Whitespace runs collapse to one space. A word wider than `width` is broken
/// across lines. If text remains after `max_lines`, the last line ends in `…`;
/// a title that fits exactly gets no ellipsis.
pub fn wrap_title(title: &str, width: usize, max_lines: usize) -> Vec<String> {
    if width == 0 || max_lines == 0 {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0;

    for word in title.split_whitespace() {
        let word_w = str_width(word);
        if word_w <= width {
            if cur.is_empty() {
                cur.push_str(word);
                cur_w = word_w;
            } else if cur_w + 1 + word_w <= width {
                cur.push(' ');
                cur.push_str(word);
                cur_w += 1 + word_w;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur.push_str(word);
                cur_w = word_w;
            }
            continue;
        }
        // Too long for any line: start on a fresh line and break by character.
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        cur_w = 0;
        for c in word.chars() {
            let w = char_width(c);
            if cur_w + w > width && !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            cur.push(c);
            cur_w += w;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }

    if lines.len() > max_lines {
        lines.truncate(max_lines);
        // Always mark the cut, even if the last line has room to spare.
        let last = lines.pop().unwrap_or_default();
        lines.push(truncate_with_ellipsis(&format!("{last}{ELLIPSIS}"), width));
    }
    lines
}

/// The 32×7 border: rounded, or thick when `selected`, all in `edge`.
pub(super) fn draw_border(buf: &mut Buffer, selected: bool, edge: Style) {
    let set = if selected {
        border::THICK
    } else {
        border::ROUNDED
    };
    let right = CELL_WIDTH - 1;
    let bottom = CELL_HEIGHT - 1;
    for x in 1..right {
        buf[(x, 0)].set_symbol(set.horizontal_top).set_style(edge);
        buf[(x, bottom)]
            .set_symbol(set.horizontal_bottom)
            .set_style(edge);
    }
    for y in 1..bottom {
        buf[(0, y)].set_symbol(set.vertical_left).set_style(edge);
        buf[(right, y)]
            .set_symbol(set.vertical_right)
            .set_style(edge);
    }
    buf[(0, 0)].set_symbol(set.top_left).set_style(edge);
    buf[(right, 0)].set_symbol(set.top_right).set_style(edge);
    buf[(0, bottom)].set_symbol(set.bottom_left).set_style(edge);
    buf[(right, bottom)]
        .set_symbol(set.bottom_right)
        .set_style(edge);
}

/// Copy a finished cell buffer to `area` of `buf`, clipped to `area` and the
/// target buffer.
pub(super) fn blit(cell: &Buffer, area: Rect, buf: &mut Buffer) {
    let w = area.width.min(CELL_WIDTH);
    let h = area.height.min(CELL_HEIGHT);
    let target = buf.area;
    for y in 0..h {
        for x in 0..w {
            let (tx, ty) = (area.x + x, area.y + y);
            if tx < target.right() && ty < target.bottom() {
                buf[(tx, ty)] = cell[(x, y)].clone();
            }
        }
    }
}

// --- Widget ------------------------------------------------------------------

/// One issue as a 32×7 cell. Build with [`IssueCell::new`].
#[derive(Debug, Clone)]
pub struct IssueCell<'a> {
    issue: &'a Issue,
    has_blockers: bool,
    selected: bool,
    dimmed: bool,
}

impl<'a> IssueCell<'a> {
    /// `has_blockers`: see `model::blockers::has_open_blockers`.
    pub fn new(issue: &'a Issue, has_blockers: bool) -> Self {
        Self {
            issue,
            has_blockers,
            selected: false,
            dimmed: false,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Epic header that fails the filters (spec §5.2): every color dimmed.
    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    fn style(&self, style: Style) -> Style {
        if self.dimmed {
            theme::dim(style)
        } else {
            style
        }
    }

    fn span(&self, span: Span<'static>) -> Span<'static> {
        if self.dimmed {
            theme::dim_span(span)
        } else {
            span
        }
    }

    /// Draw into a buffer that is exactly the cell's size.
    fn draw(&self, buf: &mut Buffer) {
        let issue = self.issue;
        let edge = self.style(theme::type_style(issue.issue_type));
        let right = CELL_WIDTH - 1;
        let bottom = CELL_HEIGHT - 1;
        draw_border(buf, self.selected, edge);

        // Top border: " id " on the left, " ⊘ " on the right. The ID is cut so
        // that at least one rule character stays between the two.
        let max_id = if self.has_blockers { 24 } else { 27 };
        let id = truncate_with_ellipsis(&issue.id, max_id);
        let id_style = if self.selected {
            self.style(Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD))
        } else {
            self.style(Style::new())
        };
        buf.set_stringn(1, 0, format!(" {id} "), usize::from(right) - 1, id_style);
        if self.has_blockers {
            let marker = self.span(theme::blockers_marker());
            buf.set_string(right - 3, 0, " ", edge);
            buf.set_string(right - 2, 0, marker.content.as_ref(), marker.style);
            buf.set_string(right - 1, 0, " ", edge);
        }

        // Bottom border: " type " ending two columns before the corner.
        let ty = format!(" {} ", issue.issue_type.as_str());
        let ty_w = u16::try_from(str_width(&ty)).unwrap_or(0);
        buf.set_string(right - 2 - ty_w, bottom, &ty, edge);

        // Title.
        let title_style = self.style(Style::new().add_modifier(Modifier::BOLD));
        for (i, line) in wrap_title(&issue.title, CONTENT_WIDTH, TITLE_LINES)
            .iter()
            .enumerate()
        {
            buf.set_stringn(2, 1 + i as u16, line, CONTENT_WIDTH, title_style);
        }

        // Badges: role, status, priority, effort.
        let badges = [
            theme::role_badge(issue.role),
            theme::status_badge(issue.status),
            theme::priority_badge(issue.priority),
            theme::effort_badge(issue.effort),
        ];
        let mut x = 2u16;
        let end = 2 + CONTENT_WIDTH as u16;
        for badge in badges {
            let badge = self.span(badge);
            if x >= end {
                break;
            }
            let (nx, _) = buf.set_stringn(
                x,
                bottom - 1,
                badge.content.as_ref(),
                usize::from(end - x),
                badge.style,
            );
            x = nx + 2;
        }
    }
}

impl Widget for IssueCell<'_> {
    /// Draws at the top-left of `area`, clipped to `area` if it is smaller
    /// than 32×7 and never larger.
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut cell = Buffer::empty(Rect::new(0, 0, CELL_WIDTH, CELL_HEIGHT));
        self.draw(&mut cell);
        blit(&cell, area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::*;
    use crate::types::{IssueType, Role, Status};

    fn draw_cell(cell: IssueCell, w: u16) -> String {
        render(w, CELL_HEIGHT, |f| f.render_widget(cell, f.area()))
    }

    fn draw_cell_styled(cell: IssueCell) -> String {
        render_styled(CELL_WIDTH, CELL_HEIGHT, |f| f.render_widget(cell, f.area()))
    }

    fn sample() -> Issue {
        issue(
            "tz-14",
            "Fix Safari login fails on redirect after timeout when the session",
        )
        .ty(IssueType::Bug)
        .role(Role::Agent)
        .status(Status::InProgress)
        .priority(1)
        .effort(2)
        .build()
    }

    // ---- wrap / truncate ----

    fn wrap(s: &str) -> Vec<String> {
        wrap_title(s, 10, 3)
    }

    #[test]
    fn short_title_is_one_line_without_ellipsis() {
        assert_eq!(wrap("Fix bug"), ["Fix bug"]);
        assert!(wrap("").is_empty());
        assert!(wrap("   ").is_empty());
    }

    #[test]
    fn wraps_on_words() {
        assert_eq!(wrap("aaaa bbbb cccc dddd"), ["aaaa bbbb", "cccc dddd"]);
        assert_eq!(wrap("a   b\tc"), ["a b c"]);
    }

    #[test]
    fn exact_fit_has_no_ellipsis() {
        assert_eq!(wrap("0123456789"), ["0123456789"]);
        assert_eq!(
            wrap("0123456789 0123456789 0123456789"),
            ["0123456789", "0123456789", "0123456789"]
        );
        assert_eq!(wrap_title("abc def", 7, 1), ["abc def"]);
    }

    #[test]
    fn never_more_than_three_lines_and_ellipsis_only_when_cut() {
        let lines = wrap("one two three four five six seven eight nine ten eleven");
        assert_eq!(lines.len(), 3);
        assert!(lines[2].ends_with('…'));
        assert!(!lines[0].contains('…') && !lines[1].contains('…'));
        assert!(lines.iter().all(|l| str_width(l) <= 10));
        // One word too many after three full lines.
        let lines = wrap("0123456789 0123456789 0123456789 x");
        assert_eq!(lines[2], "012345678…");
    }

    #[test]
    fn ellipsis_added_even_when_last_line_has_room() {
        assert_eq!(wrap_title("aa bb cc dd ee", 5, 2), ["aa bb", "cc d…"]);
        assert_eq!(wrap_title("aa bb cc", 2, 2), ["aa", "b…"]);
        assert_eq!(wrap_title("a b c", 3, 1), ["a…"]);
    }

    #[test]
    fn long_single_word_is_broken_then_cut() {
        let lines = wrap(&"w".repeat(25));
        assert_eq!(lines, ["wwwwwwwwww", "wwwwwwwwww", "wwwww"]);
        let lines = wrap(&"w".repeat(40));
        assert_eq!(lines, ["wwwwwwwwww", "wwwwwwwwww", "wwwwwwwww…"]);
        let lines = wrap(&format!("hi {}", "w".repeat(12)));
        assert_eq!(lines, ["hi", "wwwwwwwwww", "ww"]);
    }

    #[test]
    fn wide_characters_count_by_columns() {
        let lines = wrap_title("日本語日本語日本語", 6, 3);
        assert_eq!(lines, ["日本語", "日本語", "日本語"]);
        let lines = wrap_title("日本語日本語日本語日", 6, 3);
        assert_eq!(lines[2], "日本…");
        assert!(lines.iter().all(|l| str_width(l) <= 6));
    }

    #[test]
    fn truncate_helper() {
        assert_eq!(truncate_with_ellipsis("abc", 3), "abc");
        assert_eq!(truncate_with_ellipsis("abcd", 3), "ab…");
        assert_eq!(truncate_with_ellipsis("abcd", 1), "…");
        assert_eq!(truncate_with_ellipsis("abcd", 0), "");
        assert_eq!(truncate_with_ellipsis("", 0), "");
    }

    // ---- rendering ----

    #[test]
    fn normal_cell() {
        insta::assert_snapshot!(draw_cell_styled(IssueCell::new(&sample(), false)));
    }

    #[test]
    fn selected_cell() {
        insta::assert_snapshot!(draw_cell_styled(
            IssueCell::new(&sample(), false).selected(true)
        ));
    }

    #[test]
    fn dimmed_epic_header_keeps_status_badge() {
        let epic = epic("tz-3", "Auth overhaul")
            .closed()
            .role(Role::Human)
            .build();
        let out = draw_cell_styled(IssueCell::new(&epic, false).dimmed(true));
        assert!(out.contains("closed"));
        insta::assert_snapshot!(out);
    }

    #[test]
    fn cell_with_blockers() {
        insta::assert_snapshot!(draw_cell_styled(IssueCell::new(&sample(), true)));
    }

    #[test]
    fn long_title_is_truncated_with_ellipsis() {
        let i = issue(
            "tz-15",
            "A really quite long title that goes on and on and on and on well past three lines of text",
        )
        .build();
        insta::assert_snapshot!(draw_cell(IssueCell::new(&i, false), CELL_WIDTH));
    }

    #[test]
    fn very_long_single_word_title() {
        let i = issue("tz-16", &"supercalifragilistic".repeat(6)).build();
        insta::assert_snapshot!(draw_cell(IssueCell::new(&i, false), CELL_WIDTH));
    }

    #[test]
    fn unset_role_and_effort() {
        let i = issue("tz-17", "Nothing set").build();
        insta::assert_snapshot!(draw_cell_styled(IssueCell::new(&i, false)));
    }

    #[test]
    fn spec_cell() {
        let i = issue("tz-18", "Plan the importer")
            .ty(IssueType::Spec)
            .build();
        insta::assert_snapshot!(draw_cell_styled(IssueCell::new(&i, false)));
    }

    #[test]
    fn gate_cell() {
        let i = issue("tz-19", "Release sign-off")
            .ty(IssueType::Gate)
            .role(Role::Human)
            .status(Status::Blocked)
            .priority(0)
            .build();
        insta::assert_snapshot!(draw_cell_styled(IssueCell::new(&i, true)));
    }

    #[test]
    fn border_color_is_type_color_and_selected_border_is_thick() {
        let i = sample();
        for selected in [false, true] {
            let out = draw_cell_styled(IssueCell::new(&i, false).selected(selected));
            let edge = format!("fg={:?}", theme::type_color(IssueType::Bug));
            assert!(out.contains(&edge));
            assert_eq!(out.contains('┏'), selected);
            assert_eq!(out.contains('╭'), !selected);
        }
    }

    #[test]
    fn selected_id_is_highlighted_and_dimmed_colors_are_reduced() {
        let i = sample();
        let plain = draw_cell_styled(IssueCell::new(&i, false));
        let sel = draw_cell_styled(IssueCell::new(&i, false).selected(true));
        assert!(!plain.contains("REVERSED"));
        assert!(sel
            .lines()
            .any(|l| l.contains("REVERSED") && l.contains("tz-14")));
        let dim = draw_cell_styled(IssueCell::new(&i, false).dimmed(true));
        assert!(!dim.contains(&format!("{:?}", theme::BLUE)));
        assert!(dim.contains("Rgb(118, 42, 42)"), "{dim}");
    }

    #[test]
    fn assignee_and_labels_are_not_shown() {
        let i = issue("tz-1", "T")
            .assignee("zed-the-assignee")
            .labels(&["lbl-x"])
            .build();
        let out = draw_cell(IssueCell::new(&i, false), CELL_WIDTH);
        assert!(!out.contains("zed") && !out.contains("lbl"));
    }

    #[test]
    fn rendered_width_never_exceeds_32_even_with_wide_ids() {
        let ids = [
            "tazk-h/a/b/abcdefghijklmnopqrstuvwxyz0123456789",
            "日本語日本語日本語日本語日本語日本語日本語日本語",
            "x",
        ];
        for id in ids {
            for blockers in [false, true] {
                let i = issue(id, "Title").build();
                let mut buf = Buffer::empty(Rect::new(0, 0, 40, CELL_HEIGHT));
                IssueCell::new(&i, blockers)
                    .selected(true)
                    .render(buf.area, &mut buf);
                for y in 0..CELL_HEIGHT {
                    for x in CELL_WIDTH..40 {
                        assert_eq!(buf[(x, y)].symbol(), " ", "({x},{y}) id {id}");
                    }
                }
                assert_eq!(buf[(0, 0)].symbol(), "┏");
                assert_eq!(buf[(CELL_WIDTH - 1, 0)].symbol(), "┓");
                let top: String = (0..CELL_WIDTH).map(|x| buf[(x, 0)].symbol()).collect();
                if id.len() > 30 {
                    assert!(top.contains('…'), "{top:?}");
                }
                if blockers {
                    assert!(top.ends_with(" ⊘ ┓"), "{top:?}");
                }
            }
        }
    }

    #[test]
    fn clipped_to_a_smaller_area_without_panicking() {
        let i = sample();
        let out = render(10, 3, |f| {
            f.render_widget(IssueCell::new(&i, true), f.area())
        });
        assert_eq!(out.lines().count(), 3);
        // Offset area partly outside the buffer.
        render(20, 5, |f| {
            f.render_widget(IssueCell::new(&i, true), Rect::new(10, 2, 32, 7))
        });
    }
}
