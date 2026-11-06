//! Every color and badge style lives here (spec §5.4, §12). No color literals
//! elsewhere. Truecolor / 256-color is assumed, so the palette is RGB, which also
//! lets [`dim`] scale colors down. Only one-cell-wide symbols are used.

use crate::types::{IssueType, Role, Status};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

// --- Palette -------------------------------------------------------------

pub const PURPLE: Color = Color::Rgb(176, 124, 232);
pub const TEAL: Color = Color::Rgb(64, 200, 190);
pub const GREEN: Color = Color::Rgb(96, 200, 104);
pub const RED: Color = Color::Rgb(236, 84, 84);
pub const BLUE: Color = Color::Rgb(94, 154, 240);
pub const YELLOW: Color = Color::Rgb(232, 204, 72);
pub const GRAY: Color = Color::Rgb(150, 150, 150);
pub const ORANGE: Color = Color::Rgb(255, 160, 40);
pub const CYAN: Color = Color::Rgb(80, 210, 230);
pub const MAGENTA: Color = Color::Rgb(220, 100, 200);
/// Low-contrast text: deferred / closed, unset values, P4.
pub const DIM: Color = Color::Rgb(110, 110, 110);
/// What "default foreground" is assumed to be when [`dim`] has to scale it.
const ASSUMED_DEFAULT_FG: (u8, u8, u8) = (220, 220, 220);
/// Brightness kept by [`dim`].
const DIM_FACTOR: f32 = 0.5;

/// Placeholder for an unset value.
pub const UNSET: &str = "—";
pub const BLOCKERS_SYMBOL: &str = "⊘";
pub const EFFORT_FILLED: char = '●';
pub const EFFORT_EMPTY: char = '○';
/// Number of effort dots (efforts run 1..=5).
pub const EFFORT_DOTS: usize = 5;

// --- Dimming ---------------------------------------------------------------

fn scale(c: Color) -> Color {
    let (r, g, b) = match c {
        Color::Rgb(r, g, b) => (r, g, b),
        // Reset / named / indexed colors can't be scaled exactly; treat them
        // as the default foreground.
        _ => ASSUMED_DEFAULT_FG,
    };
    let f = |v: u8| (f32::from(v) * DIM_FACTOR).round() as u8;
    Color::Rgb(f(r), f(g), f(b))
}

/// Reduce the brightness of every color in `style` (fg always, bg if set).
/// An unset foreground is treated as the default foreground, so the result
/// always has an explicit, reduced fg. Modifiers are kept.
pub fn dim(style: Style) -> Style {
    let mut out = style;
    out.fg = Some(scale(style.fg.unwrap_or(Color::Reset)));
    if let Some(bg) = style.bg {
        if bg != Color::Reset {
            out.bg = Some(scale(bg));
        }
    }
    out
}

/// [`dim`] applied to a span's style.
pub fn dim_span(span: Span<'static>) -> Span<'static> {
    let style = dim(span.style);
    span.style(style)
}

// --- Type ------------------------------------------------------------------

/// Border color for an issue type. Exhaustive: adding a type must add a color.
pub fn type_color(t: IssueType) -> Color {
    match t {
        IssueType::Epic => PURPLE,
        IssueType::Spec => TEAL,
        IssueType::Feature => GREEN,
        IssueType::Bug => RED,
        IssueType::Task => BLUE,
        IssueType::Gate => YELLOW,
        IssueType::Chore => GRAY,
    }
}

pub fn type_style(t: IssueType) -> Style {
    Style::new().fg(type_color(t))
}

// --- Badges ----------------------------------------------------------------

/// `agent` (cyan) / `human` (magenta) / dim `—`.
pub fn role_badge(role: Option<Role>) -> Span<'static> {
    match role {
        Some(Role::Agent) => Span::styled(Role::Agent.as_str(), Style::new().fg(CYAN)),
        Some(Role::Human) => Span::styled(Role::Human.as_str(), Style::new().fg(MAGENTA)),
        None => unset_badge(),
    }
}

/// Short status label as shown on cells (`in_progress` -> `in_prog`).
pub fn status_label(status: Status) -> &'static str {
    match status {
        Status::Open => "open",
        Status::InProgress => "in_prog",
        Status::Blocked => "blocked",
        Status::Deferred => "deferred",
        Status::Closed => "closed",
    }
}

/// open default, in_prog yellow, blocked red, deferred dim, closed dim + strikethrough.
pub fn status_badge(status: Status) -> Span<'static> {
    let style = match status {
        Status::Open => Style::new(),
        Status::InProgress => Style::new().fg(YELLOW),
        Status::Blocked => Style::new().fg(RED),
        Status::Deferred => Style::new().fg(DIM),
        Status::Closed => Style::new().fg(DIM).add_modifier(Modifier::CROSSED_OUT),
    };
    Span::styled(status_label(status), style)
}

/// P0 red → P1 orange → P2 yellow → P3 default → P4 dim. Values below 0 use
/// the P0 color, above 4 the P4 color.
pub fn priority_style(priority: i32) -> Style {
    match priority.clamp(0, 4) {
        0 => Style::new().fg(RED),
        1 => Style::new().fg(ORANGE),
        2 => Style::new().fg(YELLOW),
        3 => Style::new(),
        _ => Style::new().fg(DIM),
    }
}

/// `P<n>` with the real value in the label.
pub fn priority_badge(priority: i32) -> Span<'static> {
    Span::styled(format!("P{priority}"), priority_style(priority))
}

/// Five dots filled to `effort` (`●●○○○` = 2), or dim `—` if unset.
pub fn effort_badge(effort: Option<u8>) -> Span<'static> {
    let Some(effort) = effort else {
        return unset_badge();
    };
    let filled = usize::from(effort).min(EFFORT_DOTS);
    let text: String = (0..EFFORT_DOTS)
        .map(|i| {
            if i < filled {
                EFFORT_FILLED
            } else {
                EFFORT_EMPTY
            }
        })
        .collect();
    Span::styled(text, Style::new().fg(BLUE))
}

/// Orange `⊘`: the issue has open blockers.
pub fn blockers_marker() -> Span<'static> {
    Span::styled(BLOCKERS_SYMBOL, Style::new().fg(ORANGE))
}

// --- Grid ------------------------------------------------------------------

/// Column title text: bold; the selected column's title is accented.
pub fn column_title_style(selected: bool) -> Style {
    let style = Style::new().add_modifier(Modifier::BOLD);
    if selected {
        style.fg(CYAN)
    } else {
        style
    }
}

/// The rule and count of a column title bar.
pub fn column_rule_style() -> Style {
    Style::new().fg(DIM)
}

/// The centered "No matching issues" message.
pub fn empty_message_style() -> Style {
    Style::new().fg(DIM)
}

// --- Panes ---------------------------------------------------------------------

/// Border of the detail pane: accented when it has focus (or is full-screen),
/// dim when the grid has focus (spec §8).
pub fn pane_border_style(focused: bool) -> Style {
    if focused {
        Style::new().fg(CYAN).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(DIM)
    }
}

// --- Detail view -------------------------------------------------------------

/// Marker in front of the highlighted relationship entry.
pub const HIGHLIGHT_MARKER: &str = "▶";
/// Background of the highlighted relationship entry.
pub const HIGHLIGHT_BG: Color = Color::Rgb(52, 58, 84);

/// Property labels (`Status`, `Created`, ...).
pub fn label_style() -> Style {
    Style::new().fg(GRAY)
}

/// The issue title and relationship section headings.
pub fn heading_style() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// Dim text: `(missing)`, unset values.
pub fn dim_style() -> Style {
    Style::new().fg(DIM)
}

/// Whole-line style of the highlighted relationship entry.
pub fn highlight_style() -> Style {
    Style::new().bg(HIGHLIGHT_BG).add_modifier(Modifier::BOLD)
}

fn unset_badge() -> Span<'static> {
    Span::styled(UNSET, Style::new().fg(DIM))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(c: Color) -> (u8, u8, u8) {
        match c {
            Color::Rgb(r, g, b) => (r, g, b),
            other => panic!("expected rgb, got {other:?}"),
        }
    }

    fn all_badges() -> Vec<Span<'static>> {
        let mut v = vec![
            role_badge(Some(Role::Agent)),
            role_badge(Some(Role::Human)),
            role_badge(None),
            priority_badge(-1),
            priority_badge(0),
            priority_badge(3),
            priority_badge(7),
            effort_badge(Some(2)),
            effort_badge(None),
            blockers_marker(),
        ];
        for s in [
            Status::Open,
            Status::InProgress,
            Status::Blocked,
            Status::Deferred,
            Status::Closed,
        ] {
            v.push(status_badge(s));
        }
        v
    }

    #[test]
    fn priority_out_of_range_uses_edge_colors_but_real_label() {
        let low = priority_badge(-1);
        assert_eq!(low.content, "P-1");
        assert_eq!(low.style, priority_style(0));
        let high = priority_badge(7);
        assert_eq!(high.content, "P7");
        assert_eq!(high.style, priority_style(4));
        assert_ne!(priority_style(0), priority_style(4));
    }

    #[test]
    fn priority_scale() {
        assert_eq!(priority_style(0).fg, Some(RED));
        assert_eq!(priority_style(1).fg, Some(ORANGE));
        assert_eq!(priority_style(2).fg, Some(YELLOW));
        assert_eq!(priority_style(3).fg, None);
        assert_eq!(priority_style(4).fg, Some(DIM));
    }

    #[test]
    fn effort_dots() {
        assert_eq!(effort_badge(Some(2)).content, "●●○○○");
        assert_eq!(effort_badge(Some(0)).content, "○○○○○");
        assert_eq!(effort_badge(Some(5)).content, "●●●●●");
        assert_eq!(effort_badge(Some(9)).content, "●●●●●");
    }

    #[test]
    fn unset_role_and_effort_are_dim_dash() {
        for b in [role_badge(None), effort_badge(None)] {
            assert_eq!(b.content, "—");
            assert_eq!(b.style.fg, Some(DIM));
        }
    }

    #[test]
    fn role_colors() {
        assert_eq!(role_badge(Some(Role::Agent)).content, "agent");
        assert_eq!(role_badge(Some(Role::Agent)).style.fg, Some(CYAN));
        assert_eq!(role_badge(Some(Role::Human)).content, "human");
        assert_eq!(role_badge(Some(Role::Human)).style.fg, Some(MAGENTA));
    }

    #[test]
    fn status_labels_and_styles() {
        let labels: Vec<_> = [
            Status::Open,
            Status::InProgress,
            Status::Blocked,
            Status::Deferred,
            Status::Closed,
        ]
        .map(|s| status_badge(s).content.to_string())
        .into();
        assert_eq!(labels, ["open", "in_prog", "blocked", "deferred", "closed"]);
        assert_eq!(status_badge(Status::Open).style, Style::new());
        assert_eq!(status_badge(Status::InProgress).style.fg, Some(YELLOW));
        assert_eq!(status_badge(Status::Blocked).style.fg, Some(RED));
        assert_eq!(status_badge(Status::Deferred).style.fg, Some(DIM));
        let closed = status_badge(Status::Closed).style;
        assert_eq!(closed.fg, Some(DIM));
        assert!(closed.add_modifier.contains(Modifier::CROSSED_OUT));
        assert!(!status_badge(Status::Deferred)
            .style
            .add_modifier
            .contains(Modifier::CROSSED_OUT));
    }

    #[test]
    fn every_type_has_its_own_color() {
        let all = [
            (IssueType::Epic, PURPLE),
            (IssueType::Spec, TEAL),
            (IssueType::Feature, GREEN),
            (IssueType::Bug, RED),
            (IssueType::Task, BLUE),
            (IssueType::Gate, YELLOW),
            (IssueType::Chore, GRAY),
        ];
        for (t, c) in all {
            assert_eq!(type_color(t), c, "{t}");
            assert_eq!(type_style(t).fg, Some(c));
        }
        for (i, (_, a)) in all.iter().enumerate() {
            for (_, b) in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn blockers_marker_is_orange() {
        let m = blockers_marker();
        assert_eq!(m.content, "⊘");
        assert_eq!(m.style.fg, Some(ORANGE));
    }

    #[test]
    fn dim_reduces_every_badge() {
        for badge in all_badges() {
            let before = badge.style;
            let after = dim(before).fg.expect("dimmed fg is explicit");
            let (r, g, b) = rgb(after);
            let (r0, g0, b0) = match before.fg {
                Some(c) => rgb(c),
                None => ASSUMED_DEFAULT_FG,
            };
            assert!(
                r < r0 && g < g0 && b < b0,
                "{:?}: {after:?} not dimmer than {:?}",
                badge.content,
                before.fg
            );
            // Modifiers survive.
            assert_eq!(dim(before).add_modifier, before.add_modifier);
            assert_eq!(dim_span(badge.clone()).content, badge.content);
        }
    }

    #[test]
    fn dim_scales_background_too() {
        let s = dim(Style::new()
            .fg(Color::Rgb(200, 100, 50))
            .bg(Color::Rgb(100, 100, 100)));
        assert_eq!(s.fg, Some(Color::Rgb(100, 50, 25)));
        assert_eq!(s.bg, Some(Color::Rgb(50, 50, 50)));
    }

    #[test]
    fn symbols_are_one_cell_wide() {
        for sym in [UNSET, BLOCKERS_SYMBOL, "●", "○", "…", HIGHLIGHT_MARKER] {
            assert_eq!(Span::raw(sym).width(), 1, "{sym}");
        }
    }
}
