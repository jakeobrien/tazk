//! Detail view state (spec §9.4, §9.6): scroll offset, relationship highlight
//! and the link stack. Pure; the renderer reports the content and viewport
//! heights from its last layout with [`DetailState::set_metrics`].

/// Scroll, highlight and link-stack state of the detail view.
///
/// Conventions:
/// - `highlight` indexes the relationship entries in display order
///   ([`super::relations::Relationships::entry`]); missing targets can be
///   highlighted too (they are just not followable).
/// - `Tab` / `Shift-Tab` **wrap** around the ends. With no highlight yet,
///   `Tab` goes to the first entry and `Shift-Tab` to the last.
/// - A page is the viewport height, a half page half of it (both at least 1).
/// - Pushing a link resets scroll and highlight; popping it resets them too
///   (positions are not remembered per stack level).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetailState {
    stack: Vec<String>,
    scroll: usize,
    highlight: Option<usize>,
    /// The highlight changed and the renderer should scroll it into view.
    reveal: bool,
    content_height: usize,
    viewport_height: usize,
}

impl DetailState {
    /// Issue IDs followed from the selection, oldest first.
    pub fn stack(&self) -> &[String] {
        &self.stack
    }

    /// The most recently followed issue; `None` when following the selection.
    pub fn top(&self) -> Option<&str> {
        self.stack.last().map(String::as_str)
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn highlight(&self) -> Option<usize> {
        self.highlight
    }

    /// Record the renderer's last layout (total lines, visible lines) and
    /// clamp the scroll offset to it.
    pub fn set_metrics(&mut self, content_height: usize, viewport_height: usize) {
        self.content_height = content_height;
        self.viewport_height = viewport_height;
        self.clamp();
    }

    /// The largest valid scroll offset.
    pub fn max_scroll(&self) -> usize {
        self.content_height.saturating_sub(self.viewport_height)
    }

    fn clamp(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }

    fn page(&self) -> usize {
        self.viewport_height.max(1)
    }

    fn half_page(&self) -> usize {
        (self.viewport_height / 2).max(1)
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll = self.scroll.saturating_sub(lines);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll = self.scroll.saturating_add(lines).min(self.max_scroll());
    }

    pub fn line_up(&mut self) {
        self.scroll_up(1);
    }

    pub fn line_down(&mut self) {
        self.scroll_down(1);
    }

    pub fn page_up(&mut self) {
        self.scroll_up(self.page());
    }

    pub fn page_down(&mut self) {
        self.scroll_down(self.page());
    }

    pub fn half_page_up(&mut self) {
        self.scroll_up(self.half_page());
    }

    pub fn half_page_down(&mut self) {
        self.scroll_down(self.half_page());
    }

    pub fn top_of_content(&mut self) {
        self.scroll = 0;
    }

    pub fn bottom_of_content(&mut self) {
        self.scroll = self.max_scroll();
    }

    /// Move the highlight to the next of `len` entries (wrapping).
    pub fn next_link(&mut self, len: usize) {
        self.highlight = match (len, self.highlight) {
            (0, _) => None,
            (_, None) => Some(0),
            (n, Some(i)) => Some((i + 1) % n),
        };
        self.reveal = self.highlight.is_some();
    }

    /// Move the highlight to the previous of `len` entries (wrapping).
    pub fn prev_link(&mut self, len: usize) {
        self.highlight = match (len, self.highlight) {
            (0, _) => None,
            (n, None) => Some(n - 1),
            (n, Some(i)) => Some((i + n - 1) % n),
        };
        self.reveal = self.highlight.is_some();
    }

    /// Whether the renderer still has to scroll the highlight into view.
    pub fn reveal_pending(&self) -> bool {
        self.reveal
    }

    /// Scroll so lines `top..top + height` are visible (as far as fits) and
    /// clear the pending request. Called by the renderer, which knows where
    /// the highlighted entry is.
    pub fn reveal_lines(&mut self, top: usize, height: usize) {
        self.reveal = false;
        let height = height.max(1);
        if top < self.scroll {
            self.scroll = top;
        } else if top + height > self.scroll + self.viewport_height {
            self.scroll = (top + height).saturating_sub(self.viewport_height);
        }
        self.clamp();
    }

    /// Follow a link: show `id` on top of the stack from the top, no highlight.
    pub fn push(&mut self, id: String) {
        self.stack.push(id);
        self.reset_view();
    }

    /// Go back one issue. False (and no change) if the stack is empty.
    pub fn pop(&mut self) -> bool {
        let popped = self.stack.pop().is_some();
        if popped {
            self.reset_view();
        }
        popped
    }

    /// Back to following the grid selection, scrolled to the top.
    pub fn reset(&mut self) {
        self.stack.clear();
        self.reset_view();
    }

    fn reset_view(&mut self) {
        self.scroll = 0;
        self.highlight = None;
        self.reveal = false;
    }

    /// Keep the highlight valid after the entry count changed.
    pub fn clamp_highlight(&mut self, len: usize) {
        if self.highlight.is_some_and(|i| i >= len) {
            self.highlight = len.checked_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(content: usize, viewport: usize) -> DetailState {
        let mut s = DetailState::default();
        s.set_metrics(content, viewport);
        s
    }

    #[test]
    fn scroll_clamps_at_both_ends() {
        let mut s = state(30, 10);
        s.line_up();
        assert_eq!(s.scroll(), 0);
        s.page_down();
        s.page_down();
        s.page_down();
        assert_eq!(s.scroll(), 20);
        s.line_down();
        assert_eq!(s.scroll(), 20);
        s.top_of_content();
        assert_eq!(s.scroll(), 0);
        s.bottom_of_content();
        assert_eq!(s.scroll(), 20);
    }

    #[test]
    fn half_pages_and_short_content() {
        let mut s = state(100, 10);
        s.half_page_down();
        assert_eq!(s.scroll(), 5);
        s.half_page_up();
        s.half_page_up();
        assert_eq!(s.scroll(), 0);
        let mut s = state(5, 10);
        s.page_down();
        s.bottom_of_content();
        assert_eq!(s.scroll(), 0);
    }

    #[test]
    fn shrinking_content_reclamps_scroll() {
        let mut s = state(100, 10);
        s.bottom_of_content();
        s.set_metrics(40, 10);
        assert_eq!(s.scroll(), 30);
    }

    #[test]
    fn links_wrap() {
        let mut s = DetailState::default();
        s.next_link(0);
        assert_eq!(s.highlight(), None);
        s.next_link(3);
        assert_eq!(s.highlight(), Some(0));
        s.prev_link(3);
        assert_eq!(s.highlight(), Some(2));
        s.next_link(3);
        assert_eq!(s.highlight(), Some(0));
        assert!(s.reveal_pending());
        let mut s = DetailState::default();
        s.prev_link(3);
        assert_eq!(s.highlight(), Some(2));
    }

    #[test]
    fn reveal_scrolls_minimally() {
        let mut s = state(50, 10);
        s.next_link(1);
        s.reveal_lines(25, 1);
        assert_eq!(s.scroll(), 16);
        assert!(!s.reveal_pending());
        s.reveal_lines(12, 1);
        assert_eq!(s.scroll(), 12);
        s.reveal_lines(14, 1);
        assert_eq!(s.scroll(), 12);
    }

    #[test]
    fn push_and_pop_reset_the_view() {
        let mut s = state(50, 10);
        s.next_link(2);
        s.page_down();
        s.push("a".into());
        assert_eq!((s.scroll(), s.highlight(), s.top()), (0, None, Some("a")));
        s.push("b".into());
        s.page_down();
        assert!(s.pop());
        assert_eq!((s.top(), s.scroll()), (Some("a"), 0));
        assert!(s.pop());
        assert!(!s.pop());
        assert_eq!(s.top(), None);
    }
}
