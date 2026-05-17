//! [`Breadcrumb`] — a one-row path of segments joined by a separator glyph,
//! the last (or selected) segment emphasized, the middle elided when narrow.
//!
//! # A pure projection of caller-owned segments + optional `selected`
//!
//! Like every rstui widget `Breadcrumb` is a **pure projection**: it renders
//! the caller-owned `&[Line]` path it is handed plus an optional caller-owned
//! [`selected`](Breadcrumb::selected) index, and reads nothing else. The path
//! is ordinary application state the reducer owns (push a segment on navigate,
//! pop on "up"); *which crumb a click maps to, and navigating to it,* are the
//! reducer's job — the widget only ever reads, exactly the read-only-state
//! rule [`List`](crate::List)/[`StatusBar`](crate::StatusBar) establish.
//!
//! # A leaf strip, like [`StatusBar`](crate::StatusBar)
//!
//! Breadcrumb is one row and takes **no framing [`Block`](crate::Block)** —
//! the [`StatusBar`](crate::StatusBar)/[`Input`](crate::Input) leaf shape: the
//! base [`style`](Breadcrumb::style) fills the row and the surrounding
//! [`Layout`](rstui_core::Layout) owns the edge it pins to. Segments are
//! joined by ` {sep} ` (default `›`, space-padded), the
//! [`selected`](Breadcrumb::selected) crumb — or the **last** when none is
//! selected — patched with [`emphasis_style`](Breadcrumb::emphasis_style)
//! last so it wins over per-span colour, the highlight-wins-last idiom
//! [`List`](crate::List) uses.
//!
//! # Documented elision — total under any width
//!
//! When the full path is wider than the row and there are **three or more**
//! segments, the middle collapses to a single `…` crumb (`first › … › last`):
//! the two ends a user needs to orient are always kept, the way every file
//! manager elides a deep path. Fewer than three segments are never elided
//! (there is no middle to drop). If even the elided form overflows, it is
//! clipped at the right edge — the [`Gauge`](crate::Gauge) totality rule: an
//! empty row, no segments, a one-cell row, and an out-of-range
//! [`selected`](Breadcrumb::selected) (it falls back to emphasizing the last)
//! are all safe clips/no-ops, never a panic. A click-target accessor and
//! per-crumb icons are deliberately deferred additives, not smuggled in.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

/// The default glyph segments are joined by (space-padded as ` › `).
const SEPARATOR: char = '›';

/// The glyph a collapsed middle is shown as.
const ELLIPSIS: char = '…';

/// Stamps `chars` left-to-right from `*x` on `row`, clipped at `right`;
/// returns `false` once the right edge is reached so the caller stops. The
/// breadcrumb stamp sequence is `render`'s tail, so the old `break 'row` is
/// a `return`. No per-frame allocation (W1-02).
fn bc_chars(
    buf: &mut Buffer,
    x: &mut u16,
    row: u16,
    right: u16,
    chars: impl Iterator<Item = char>,
    style: Style,
) -> bool {
    for ch in chars {
        if *x >= right {
            return false;
        }
        buf.set_cell(Position::new(*x, row), ch, style);
        *x = x.saturating_add(1);
    }
    true
}

/// Stamps one path segment's `Line` with the breadcrumb cascade
/// (base → line → span, `emph` patched last when present), clipped at
/// `right`; same stop contract as [`bc_chars`].
fn bc_segment(
    buf: &mut Buffer,
    x: &mut u16,
    row: u16,
    right: u16,
    line: &Line<'_>,
    base: Style,
    emph: Option<Style>,
) -> bool {
    let line_base = base.patch(line.style);
    for span in &line.spans {
        let mut span_style = line_base.patch(span.style);
        if let Some(e) = emph {
            span_style = span_style.patch(e);
        }
        for ch in span.content.chars() {
            if *x >= right {
                return false;
            }
            buf.set_cell(Position::new(*x, row), ch, span_style);
            *x = x.saturating_add(1);
        }
    }
    true
}

/// A one-row path of segments joined by a separator glyph — a pure projection
/// of caller-owned segments + optional [`selected`](Self::selected).
///
/// The [`selected`](Self::selected) segment (or the last, when none is
/// selected) is emphasized; an over-long path elides its middle to `…`. A
/// leaf strip: no [`Block`](crate::Block), the base
/// [`style`](Self::style) fills the row.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Line, Position, Rect, Widget};
/// use rstui_widgets::Breadcrumb;
///
/// // The path is plain caller-owned model state the widget only reads —
/// // mapping a click to a crumb and navigating there is the reducer's job.
/// let path = [Line::raw("src"), Line::raw("widgets"), Line::raw("breadcrumb.rs")];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 30, 1));
/// Breadcrumb::new(&path).render(buf.area(), &mut buf);
///
/// // Segments joined by " › "; the last is the emphasized leaf.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 's');
/// assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, '›');
/// ```
#[derive(Debug, Clone)]
pub struct Breadcrumb<'a> {
    segments: &'a [Line<'a>],
    separator: char,
    selected: Option<usize>,
    style: Style,
    separator_style: Style,
    emphasis_style: Style,
}

impl<'a> Breadcrumb<'a> {
    /// A breadcrumb projecting `segments`: nothing explicitly selected (the
    /// last is emphasized), the default `›` separator, unstyled.
    #[must_use]
    pub fn new(segments: &'a [Line<'a>]) -> Self {
        Self {
            segments,
            separator: SEPARATOR,
            selected: None,
            style: Style::new(),
            separator_style: Style::new(),
            emphasis_style: Style::new(),
        }
    }

    /// Sets which crumb is emphasized — caller-owned state the widget only
    /// reads. `None` (or an out-of-range index) emphasizes the **last**
    /// segment (the current location).
    #[must_use]
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the glyph segments are joined by (default `›`, drawn space-padded
    /// as ` {sep} `).
    #[must_use]
    pub fn separator(mut self, glyph: char) -> Self {
        self.separator = glyph;
        self
    }

    /// Sets the base [`Style`], beneath the segment/span cascade; it also
    /// fills the row so a background reads as one bar.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] of the joiner glyph and the elided `…` (patched over
    /// the base).
    #[must_use]
    pub fn separator_style(mut self, style: Style) -> Self {
        self.separator_style = style;
        self
    }

    /// Sets the [`Style`] patched **last** over the emphasized crumb, so it
    /// wins over per-span colour — the highlight-wins-last idiom
    /// [`List`](crate::List) uses.
    #[must_use]
    pub fn emphasis_style(mut self, style: Style) -> Self {
        self.emphasis_style = style;
        self
    }
}

impl Widget for Breadcrumb<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        let row = area.top();
        let left = area.left();
        let right = area.right();

        // Base fills the whole row (the leaf-strip bar idiom).
        buf.set_style(Rect::new(left, row, area.width, 1), self.style);

        let n = self.segments.len();
        if n == 0 {
            return;
        }

        // `None`/out-of-range emphasizes the last crumb (the current node).
        let emphasized = self.selected.filter(|&i| i < n).unwrap_or(n - 1);

        // The joiner is the glyph space-padded: ` › ` — three columns.
        let joiner: [char; 3] = [' ', self.separator, ' '];
        let joiner_w = joiner.len();

        let segments_w: usize = self.segments.iter().map(Line::width).sum();
        let full_w = segments_w + joiner_w * n.saturating_sub(1);

        // Lay out the row. The middle elides to a single `…` only when the
        // full path overflows *and* there is a middle to drop (≥ 3 segments);
        // anything still too wide is clipped at the right edge below.
        // Stamp the crumbs directly — no per-frame `Vec<Crumb>` scratch
        // (W1-02). Two shapes: the full path, or the elided
        // `seg0 › … › segN-1`. The stamp helpers return `false` (caller
        // returns) on a right-edge clip, exactly the old `break 'row`.
        let sep_style = self.style.patch(self.separator_style);
        let mut x = left;
        if full_w <= area.width as usize || n < 3 {
            for i in 0..n {
                if i > 0 && !bc_chars(buf, &mut x, row, right, joiner.into_iter(), sep_style) {
                    return;
                }
                let emph = if i == emphasized {
                    Some(self.emphasis_style)
                } else {
                    None
                };
                if !bc_segment(buf, &mut x, row, right, &self.segments[i], self.style, emph) {
                    return;
                }
            }
        } else {
            let emph0 = if emphasized == 0 {
                Some(self.emphasis_style)
            } else {
                None
            };
            if !bc_segment(
                buf,
                &mut x,
                row,
                right,
                &self.segments[0],
                self.style,
                emph0,
            ) || !bc_chars(buf, &mut x, row, right, joiner.into_iter(), sep_style)
                || !bc_chars(
                    buf,
                    &mut x,
                    row,
                    right,
                    std::iter::once(ELLIPSIS),
                    sep_style,
                )
                || !bc_chars(buf, &mut x, row, right, joiner.into_iter(), sep_style)
            {
                return;
            }
            let emph_last = if emphasized == n - 1 {
                Some(self.emphasis_style)
            } else {
                None
            };
            let _ = bc_segment(
                buf,
                &mut x,
                row,
                right,
                &self.segments[n - 1],
                self.style,
                emph_last,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Span};

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
    fn segments_are_joined_by_the_separator() {
        let path = [Line::raw("a"), Line::raw("bb"), Line::raw("c")];
        assert_eq!(lines(Breadcrumb::new(&path), 12, 1), "a › bb › c  \n");
    }

    #[test]
    fn a_custom_separator_glyph_is_used() {
        let path = [Line::raw("a"), Line::raw("b")];
        assert_eq!(
            lines(Breadcrumb::new(&path).separator('/'), 7, 1),
            "a / b  \n"
        );
    }

    #[test]
    fn a_single_segment_has_no_separator() {
        let path = [Line::raw("only")];
        assert_eq!(lines(Breadcrumb::new(&path), 6, 1), "only  \n");
    }

    #[test]
    fn the_last_segment_is_emphasized_by_default() {
        let path = [Line::raw("a"), Line::raw("b")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        Breadcrumb::new(&path)
            .emphasis_style(Style::new().fg(Color::Cyan))
            .render(buf.area(), &mut buf);
        // "a › b": only the last crumb "b" (col 4) takes the emphasis.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Reset);
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().fg, Color::Cyan);
    }

    #[test]
    fn selected_emphasizes_a_chosen_crumb() {
        let path = [Line::raw("a"), Line::raw("b"), Line::raw("c")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 9, 1));
        Breadcrumb::new(&path)
            .selected(Some(0))
            .emphasis_style(Style::new().fg(Color::Cyan))
            .render(buf.area(), &mut buf);
        // "a › b › c": crumb 0 ("a", col 0) is emphasized, not the last.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Cyan);
        assert_eq!(buf.get(Position::new(8, 0)).unwrap().fg, Color::Reset);
    }

    #[test]
    fn an_out_of_range_selected_falls_back_to_the_last() {
        let path = [Line::raw("a"), Line::raw("b")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        Breadcrumb::new(&path)
            .selected(Some(9))
            .emphasis_style(Style::new().fg(Color::Cyan))
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().fg, Color::Cyan);
    }

    #[test]
    fn a_narrow_row_elides_the_middle_to_an_ellipsis() {
        let path = [Line::raw("root"), Line::raw("middle"), Line::raw("here")];
        // Full "root › middle › here" is 20 wide; in 14 it collapses to
        // "root › … › here".
        assert_eq!(lines(Breadcrumb::new(&path), 16, 1), "root › … › here \n");
    }

    #[test]
    fn two_segments_are_never_elided_only_clipped() {
        let path = [Line::raw("alpha"), Line::raw("omega")];
        // No middle to drop: the full path is simply clipped at the edge.
        assert_eq!(lines(Breadcrumb::new(&path), 8, 1), "alpha › \n");
    }

    #[test]
    fn an_elided_path_still_too_narrow_is_clipped() {
        let path = [Line::raw("alpha"), Line::raw("beta"), Line::raw("gamma")];
        // Even "alpha › … › gamma" overflows width 7: clipped, no panic.
        assert_eq!(lines(Breadcrumb::new(&path), 7, 1), "alpha ›\n");
    }

    #[test]
    fn the_separator_takes_the_separator_style() {
        let path = [Line::raw("a"), Line::raw("b")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        Breadcrumb::new(&path)
            .separator_style(Style::new().fg(Color::DarkGray))
            .render(buf.area(), &mut buf);
        // "a › b": the '›' (col 2) is styled; the segments are not.
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().fg, Color::DarkGray);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Reset);
    }

    #[test]
    fn base_style_fills_the_whole_row() {
        let path = [Line::raw("x")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        Breadcrumb::new(&path)
            .style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for x in 0..5 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
    }

    #[test]
    fn style_cascades_segment_span_and_emphasis_wins_last() {
        let seg = Line::from(Span::styled("Z", Style::new().fg(Color::Red)));
        let path = [seg];
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        Breadcrumb::new(&path)
            .selected(Some(0))
            .emphasis_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.symbol, 'Z');
        assert_eq!(cell.fg, Color::Red); // span fg survives
        assert_eq!(cell.bg, Color::Blue); // emphasis patched last
    }

    #[test]
    fn no_segments_only_fills_the_base() {
        let path: [Line<'_>; 0] = [];
        assert_eq!(lines(Breadcrumb::new(&path), 4, 1), "    \n");
    }

    #[test]
    fn only_the_top_row_is_touched() {
        let path = [Line::raw("a"), Line::raw("b")];
        assert_eq!(lines(Breadcrumb::new(&path), 5, 2), "a › b\n     \n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let path = [Line::raw("a")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Breadcrumb::new(&path).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
