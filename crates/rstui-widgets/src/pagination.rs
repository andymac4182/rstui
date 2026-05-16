//! [`Pagination`] — a one-row pager (`‹ 1 … 4 [5] 6 … 20 ›`), the windowed
//! page strip a table footer, a search-results pane, or a gallery pins to an
//! edge.
//!
//! # A pure projection of caller-owned `page` + `page_count`
//!
//! Like every rstui widget `Pagination` is a **pure projection**: it renders
//! the caller-owned current [`page`](Pagination::new) (a zero-based index) and
//! [`page_count`](Pagination::new) it is handed, and reads nothing else. Both
//! are ordinary application state the reducer owns and moves in `update`
//! (`PageUp`/click a number); *which page a click maps to, and loading it,*
//! are the reducer's job — the widget only ever reads, exactly the
//! read-only-state rule [`List`](crate::List) establishes. It needs no
//! lifetime — every part is a glyph or a number — like
//! [`Scrollbar`](crate::Scrollbar).
//!
//! # A leaf strip, like [`Breadcrumb`](crate::Breadcrumb)
//!
//! `Pagination` is one row and takes **no framing [`Block`](crate::Block)** —
//! the [`Breadcrumb`](crate::Breadcrumb)/[`StatusBar`](crate::StatusBar) leaf
//! shape: the base [`style`](Pagination::style) fills the row and the
//! surrounding [`Layout`](rstui_core::Layout) owns the edge it pins to.
//!
//! # Documented windowing — total under any input
//!
//! The strip always shows the **first** and **last** page plus a window of
//! [`siblings`](Pagination::siblings) pages either side of the current one;
//! every skipped run collapses to a single `…`, and prev/next chevrons
//! (`‹`/`›`) bracket the strip. Per the [`Gauge`](crate::Gauge) totality rule:
//! a `page_count` of `0` is a blank row, a `page_count` of `1` is `‹ [1] ›`, a
//! `page` past the end clamps into range, an empty/one-cell/multi-row area and
//! a strip wider than the row (clipped at the right edge) are all safe
//! clips/no-ops — never a panic.

use rstui_core::{Buffer, Position, Rect, Style, Widget};

/// The left (previous) chevron.
const PREV: char = '‹';

/// The right (next) chevron.
const NEXT: char = '›';

/// The glyph a collapsed run of skipped pages is shown as.
const ELLIPSIS: char = '…';

/// A one-row windowed pager — a pure projection of a caller-owned `page` /
/// `page_count`.
///
/// The current page is bracketed (`[5]`) and patched with
/// [`current_style`](Self::current_style); the chevrons and `…` gap markers
/// take [`control_style`](Self::control_style); every other page and the base
/// fill take [`style`](Self::style).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Pagination;
///
/// // `page`/`page_count` are plain caller-owned model state the widget only
/// // reads — mapping a click to a page and loading it is the reducer's job.
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
/// Pagination::new(4, 20).render(buf.area(), &mut buf);
///
/// // "‹ 1 … 4 [5] 6 … 20 ›" — page index 4 (1-based "5") is the bracketed
/// // current page, with the first/last always shown and gaps elided.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '‹');
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, '1');
/// assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, '…');
/// assert_eq!(buf.get(Position::new(8, 0)).unwrap().symbol, '[');
/// assert_eq!(buf.get(Position::new(9, 0)).unwrap().symbol, '5');
/// ```
#[derive(Debug, Clone)]
pub struct Pagination {
    page: usize,
    page_count: usize,
    siblings: usize,
    style: Style,
    current_style: Style,
    control_style: Style,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            page: 0,
            page_count: 0,
            // One page either side of the current is the sensible default
            // window (the shape every web pager ships with).
            siblings: 1,
            style: Style::new(),
            current_style: Style::new(),
            control_style: Style::new(),
        }
    }
}

impl Pagination {
    /// A pager showing the zero-based `page` of `page_count` total pages.
    #[must_use]
    pub fn new(page: usize, page_count: usize) -> Self {
        Self {
            page,
            page_count,
            ..Self::default()
        }
    }

    /// Sets how many pages are shown either side of the current one before the
    /// run collapses to `…` (default `1`).
    #[must_use]
    pub fn siblings(mut self, siblings: usize) -> Self {
        self.siblings = siblings;
        self
    }

    /// Sets the base [`Style`]; it also fills the row so a background reads as
    /// one bar.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched over the bracketed current page.
    #[must_use]
    pub fn current_style(mut self, style: Style) -> Self {
        self.current_style = style;
        self
    }

    /// Sets the [`Style`] patched over the chevrons and the `…` gap markers.
    #[must_use]
    pub fn control_style(mut self, style: Style) -> Self {
        self.control_style = style;
        self
    }

    /// The sorted set of zero-based page indices the strip shows: the first,
    /// the last, and `siblings` either side of the (clamped) current page.
    fn shown(&self, page: usize) -> Vec<usize> {
        let last = self.page_count - 1;
        let lo = page.saturating_sub(self.siblings);
        let hi = (page + self.siblings).min(last);
        let mut idx: Vec<usize> = (lo..=hi).collect();
        idx.push(0);
        idx.push(last);
        idx.sort_unstable();
        idx.dedup();
        idx
    }
}

impl Widget for Pagination {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let y = area.top();
        let left = area.left();
        let right = area.right();

        // Base fills the whole row (the leaf-strip bar idiom).
        buf.set_style(Rect::new(left, y, area.width, 1), self.style);

        if self.page_count == 0 {
            // No pages: a blank base-filled row — total, no panic.
            return;
        }

        let page = self.page.min(self.page_count - 1);
        let ctrl = self.style.patch(self.control_style);
        let current = self.style.patch(self.current_style);

        // Build the token stream: `‹`, the windowed page numbers with `…`
        // standing in for each skipped run, then `›`.
        let mut tokens: Vec<(String, Style)> = vec![(PREV.to_string(), ctrl)];
        let mut prev: Option<usize> = None;
        for idx in self.shown(page) {
            if let Some(p) = prev {
                if idx > p + 1 {
                    tokens.push((ELLIPSIS.to_string(), ctrl));
                }
            }
            let label = (idx + 1).to_string();
            if idx == page {
                tokens.push((format!("[{label}]"), current));
            } else {
                tokens.push((label, self.style));
            }
            prev = Some(idx);
        }
        tokens.push((NEXT.to_string(), ctrl));

        // Stamp the tokens left to right, one base-filled blank between each,
        // clipped hard at the right edge.
        let mut x = left;
        'render: for (i, (text, style)) in tokens.iter().enumerate() {
            if i > 0 {
                if x >= right {
                    break 'render;
                }
                x = x.saturating_add(1);
            }
            for ch in text.chars() {
                if x >= right {
                    break 'render;
                }
                buf.set_cell(Position::new(x, y), ch, *style);
                x = x.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Color;

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row.
    fn lines<W: Widget>(widget: W, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn a_long_range_is_windowed_with_ellipses() {
        assert_eq!(
            lines(Pagination::new(4, 20), 20, 1),
            "‹ 1 … 4 [5] 6 … 20 ›\n"
        );
    }

    #[test]
    fn a_short_range_shows_every_page_with_no_ellipsis() {
        // 5 pages, current index 2: every page is within first/last/siblings.
        assert_eq!(lines(Pagination::new(2, 5), 15, 1), "‹ 1 2 [3] 4 5 ›\n");
    }

    #[test]
    fn page_count_zero_is_a_blank_row() {
        assert_eq!(lines(Pagination::new(0, 0), 6, 1), "      \n");
    }

    #[test]
    fn page_count_one_is_just_the_single_page() {
        assert_eq!(lines(Pagination::new(0, 1), 7, 1), "‹ [1] ›\n");
    }

    #[test]
    fn a_page_past_the_end_clamps_into_range() {
        // page 99 of 3 clamps to the last (index 2 → "[3]").
        assert_eq!(lines(Pagination::new(99, 3), 11, 1), "‹ 1 2 [3] ›\n");
    }

    #[test]
    fn siblings_widens_the_window() {
        assert_eq!(
            lines(Pagination::new(4, 20).siblings(2), 24, 1),
            "‹ 1 … 3 4 [5] 6 7 … 20 ›\n"
        );
    }

    #[test]
    fn the_current_page_is_bracketed_and_takes_the_current_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 15, 1));
        Pagination::new(2, 5)
            .current_style(Style::new().bg(Color::Cyan))
            .render(buf.area(), &mut buf);
        // "‹ 1 2 [3] 4 5 ›": "[3]" is cols 6,7,8.
        for x in 6..9 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Cyan);
        }
        // A plain page ("1", col 2) is not styled current.
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn the_chevrons_and_ellipses_take_the_control_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
        Pagination::new(4, 20)
            .control_style(Style::new().fg(Color::DarkGray))
            .render(buf.area(), &mut buf);
        // "‹ 1 … 4 [5] 6 … 20 ›": '‹'@0, '…'@4 and @14, '›'@19.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::DarkGray);
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().fg, Color::DarkGray);
        assert_eq!(buf.get(Position::new(19, 0)).unwrap().fg, Color::DarkGray);
        // A page number keeps the base style.
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().fg, Color::Reset);
    }

    #[test]
    fn the_base_style_fills_the_whole_row() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        Pagination::new(0, 3)
            .style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
    }

    #[test]
    fn the_strip_clips_hard_at_the_right_edge() {
        // Full "‹ 1 … 4 [5] 6 … 20 ›" is 20 wide; width 8 clips after "4 ".
        assert_eq!(lines(Pagination::new(4, 20), 8, 1), "‹ 1 … 4 \n");
    }

    #[test]
    fn render_uses_the_area_origin_and_only_the_top_row() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        Pagination::new(0, 1).render(Rect::new(2, 1, 7, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, '‹');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Pagination::new(2, 9).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
