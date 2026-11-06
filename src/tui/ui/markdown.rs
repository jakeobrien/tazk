//! Markdown rendering via `tui-markdown` (spec §9.3), shared by the issue body
//! and the triage item text.
//!
//! Performance: parsing, syntax highlighting and measuring a large body is
//! much more expensive than drawing a frame, and the UI redraws on every key
//! press. [`MarkdownCache`] therefore keeps the last rendering keyed by
//! `(source, width)`; redraws with an unchanged source and width reuse the
//! parsed `Text` and the wrapped line count, and only a resize, a different
//! item or an edited file re-renders. (Comparing the source is a single
//! `memcmp`.)

use crate::types::Issue;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use std::cell::RefCell;
use std::rc::Rc;
use tui_markdown::{DefaultStyleSheet, Options};

use super::theme;

/// Shown for a body with no text.
pub const EMPTY_BODY: &str = "(no description)";

/// The markdown source of an issue body: Description, Design, Acceptance
/// Criteria and Notes, in that order, each under its `#` heading; empty
/// sections are skipped. Empty if every section is blank.
pub fn issue_markdown(issue: &Issue) -> String {
    let sections = [
        ("Description", &issue.description),
        ("Design", &issue.design),
        ("Acceptance Criteria", &issue.acceptance_criteria),
        ("Notes", &issue.notes),
    ];
    let mut out = String::new();
    for (heading, text) in sections {
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("# ");
        out.push_str(heading);
        out.push_str("\n\n");
        out.push_str(text);
        out.push('\n');
    }
    out
}

/// A markdown source rendered for one pane width.
#[derive(Debug, Clone)]
pub struct Rendered {
    /// Unwrapped styled lines; draw with a wrapping [`Paragraph`] (see
    /// [`Rendered::paragraph`]) so the height equals [`Rendered::line_count`].
    pub text: Text<'static>,
    /// Lines the text occupies when wrapped to the width it was rendered for.
    pub line_count: usize,
}

impl Rendered {
    /// The widget for this text: wrapped without trimming, exactly as measured.
    pub fn paragraph(&self) -> Paragraph<'static> {
        wrapped(self.text.clone())
    }
}

fn wrapped(text: Text<'static>) -> Paragraph<'static> {
    Paragraph::new(text).wrap(Wrap { trim: false })
}

/// Number of lines `text` takes in a wrapping `Paragraph` of `width` columns.
pub fn wrapped_line_count(text: &Text<'static>, width: u16) -> usize {
    wrapped(text.clone()).line_count(width)
}

/// Render markdown to owned, styled text. Fenced code with a recognised
/// language is syntax-highlighted (tui-markdown's `highlight-code` feature,
/// on by default); tables wrap to `width`. Blank input gives the dim
/// [`EMPTY_BODY`] placeholder.
pub fn render_markdown(source: &str, width: u16) -> Text<'static> {
    if source.trim().is_empty() {
        return Text::from(Line::from(Span::styled(
            EMPTY_BODY,
            theme::dim(Style::default()),
        )));
    }
    let options = Options::new(DefaultStyleSheet).table_width(width);
    let text = tui_markdown::from_str_with_options(source, &options);
    owned(text)
}

/// Detach the text from the source string it borrows.
fn owned(text: Text<'_>) -> Text<'static> {
    Text {
        alignment: text.alignment,
        style: text.style,
        lines: text
            .lines
            .into_iter()
            .map(|line| Line {
                style: line.style,
                alignment: line.alignment,
                spans: line
                    .spans
                    .into_iter()
                    .map(|s| Span::styled(s.content.into_owned(), s.style))
                    .collect(),
            })
            .collect(),
    }
}

/// Render and measure `source` for `width` without caching.
pub fn render_at(source: &str, width: u16) -> Rendered {
    let text = render_markdown(source, width);
    let line_count = wrapped_line_count(&text, width);
    Rendered { text, line_count }
}

struct Entry {
    source: String,
    width: u16,
    rendered: Rc<Rendered>,
}

/// Remembers the last rendering so unchanged redraws are cheap. Interior
/// mutability because drawing only has `&App`.
#[derive(Default)]
pub struct MarkdownCache {
    entry: RefCell<Option<Entry>>,
}

impl MarkdownCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The rendering of `source` at `width`, from the cache when neither changed.
    pub fn get(&self, source: &str, width: u16) -> Rc<Rendered> {
        if let Some(e) = &*self.entry.borrow() {
            if e.width == width && e.source == source {
                return Rc::clone(&e.rendered);
            }
        }
        let rendered = Rc::new(render_at(source, width));
        *self.entry.borrow_mut() = Some(Entry {
            source: source.to_string(),
            width,
            rendered: Rc::clone(&rendered),
        });
        rendered
    }

    /// Wrapped line count of `source` at `width` (for clamping the scroll).
    pub fn line_count(&self, source: &str, width: u16) -> usize {
        self.get(source, width).line_count
    }
}

impl std::fmt::Debug for MarkdownCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MarkdownCache")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;

    fn text_of(t: &Text) -> String {
        t.lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn all_four_sections_in_order() {
        let i = issue("a", "A")
            .notes("n text")
            .acceptance("ac text")
            .design("d text")
            .description("desc text")
            .build();
        let md = issue_markdown(&i);
        assert_eq!(
            md,
            "# Description\n\ndesc text\n\n# Design\n\nd text\n\n# Acceptance Criteria\n\nac text\n\n# Notes\n\nn text\n"
        );
        insta::assert_snapshot!(draw(&render_at(&md, 40), 40, 16));
        let shown = text_of(&render_markdown(&md, 60));
        let pos = |s: &str| shown.find(s).unwrap_or_else(|| panic!("{s} in\n{shown}"));
        assert!(
            pos("Description") < pos("desc text")
                && pos("desc text") < pos("Design")
                && pos("Design") < pos("d text")
                && pos("d text") < pos("Acceptance Criteria")
                && pos("Acceptance Criteria") < pos("ac text")
                && pos("ac text") < pos("Notes")
                && pos("Notes") < pos("n text")
        );
    }

    /// The body as drawn into a `width` x `height` buffer.
    fn draw(r: &Rendered, width: u16, height: u16) -> String {
        render(width, height, |f| f.render_widget(r.paragraph(), f.area()))
    }

    #[test]
    fn only_notes_skips_the_other_sections() {
        let i = issue("a", "A").notes("just notes").build();
        let md = issue_markdown(&i);
        assert_eq!(md, "# Notes\n\njust notes\n");
        let shown = text_of(&render_markdown(&md, 60));
        assert!(shown.contains("Notes") && shown.contains("just notes"));
        insta::assert_snapshot!(draw(&render_at(&md, 40), 40, 4));
        for skipped in ["Description", "Design", "Acceptance"] {
            assert!(!shown.contains(skipped), "{shown}");
        }
    }

    #[test]
    fn whitespace_only_sections_count_as_empty() {
        let i = issue("a", "A").description("  \n\n").notes("x").build();
        assert_eq!(issue_markdown(&i), "# Notes\n\nx\n");
    }

    #[test]
    fn empty_body_shows_dim_placeholder() {
        let i = issue("a", "A").build();
        assert_eq!(issue_markdown(&i), "");
        let r = render_at("", 40);
        assert_eq!(text_of(&r.text), "(no description)");
        assert_eq!(r.line_count, 1);
        insta::assert_snapshot!(draw(&r, 40, 2));
        let style = r.text.lines[0].spans[0].style;
        assert_eq!(style, theme::dim(Style::default()));
    }

    #[test]
    fn rust_code_block_is_syntax_highlighted() {
        let md = "# Description\n\n```rust\nfn main() { let x = 1; }\n```\n";
        insta::assert_snapshot!(draw(&render_at(md, 60), 60, 8));
        let text = render_markdown(md, 60);
        let code: Vec<&Span> = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| {
                ["fn", "main", "let", "1"]
                    .iter()
                    .any(|w| s.content.trim() == *w)
            })
            .collect();
        assert!(!code.is_empty(), "{text:?}");
        // Highlighting produces more than one distinct foreground colour.
        let colors: std::collections::HashSet<_> = code.iter().map(|s| s.style.fg).collect();
        assert!(colors.len() > 1, "single style for code: {code:?}");
        assert!(colors.iter().all(|c| c.is_some()));
    }

    /// Lines actually drawn by a `Paragraph` into a tall buffer of `width`.
    fn drawn_lines(r: &Rendered, width: u16) -> usize {
        let area = Rect::new(0, 0, width, 500);
        let mut buf = Buffer::empty(area);
        r.paragraph().render(area, &mut buf);
        (0..area.height)
            .rev()
            .find(|&y| (0..width).any(|x| buf[(x, y)].symbol() != " "))
            .map_or(0, |y| y as usize + 1)
    }

    #[test]
    fn line_count_matches_paragraph_at_widths_40_and_80() {
        let long = "word ".repeat(60);
        let md = format!(
            "# Description\n\n{long}\n\n- item one {long}\n- item two\n\n```\n{}\n```\n\n# Notes\n\nshort\n",
            "x".repeat(130)
        );
        for width in [40u16, 80] {
            let r = render_at(&md, width);
            assert_eq!(r.line_count, drawn_lines(&r, width), "width {width}");
        }
        assert!(render_at(&md, 40).line_count > render_at(&md, 80).line_count);
    }

    #[test]
    fn large_body_is_cached_between_redraws() {
        let body: String = (0..2000)
            .map(|i| format!("line {i} with `code` and **bold** text\n\n"))
            .collect();
        let i = issue("a", "A").description(&body).build();
        let md = issue_markdown(&i);
        let cache = MarkdownCache::new();
        let first = cache.get(&md, 80);
        assert!(first.line_count >= 2000);
        assert!(Rc::ptr_eq(&first, &cache.get(&md, 80)));
        // A different width or source re-renders.
        assert!(!Rc::ptr_eq(&first, &cache.get(&md, 40)));
        let again = cache.get(&md, 40);
        assert!(Rc::ptr_eq(&again, &cache.get(&md, 40)));
        assert_eq!(cache.line_count("# Notes\n\nx\n", 40), 3);
        assert!(!Rc::ptr_eq(&again, &cache.get(&md, 40)));
    }
}
