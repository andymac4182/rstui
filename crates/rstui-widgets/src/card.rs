//! [`Card`] — a titled container: a thin convenience composition over
//! [`Block`] with an optional header line, a body, and an optional footer
//! line. The basis for dashboard tiles, summary panels, and labelled boxes.
//!
//! # A thin `Block` composition, not a reinvented `Block`
//!
//! Every framed widget in rstui already composes with [`Block`] by rendering
//! the block then drawing into [`Block::inner`]. A "card" is just the
//! overwhelmingly common refinement of that pattern — a framed box with a
//! header strip, a body, and a footer strip — packaged so callers stop
//! hand-rolling the same `Layout::vertical([1, Fill(1), 1])` split on every
//! tile. `Card` therefore *delegates* its frame and its base geometry to a
//! real [`Block`] (it owns one, configurable via [`block`](Card::block) /
//! [`title`](Card::title)); it adds only the header/footer rows on top. It is
//! not a second border/title implementation.
//!
//! # A pure projection, like every container
//!
//! `Card` owns no application state and mutates nothing at render time. The
//! header and footer are caller-built [`Line`]s; the body is the caller's to
//! fill. [`inner`](Card::inner) is a pure function of the area and the
//! configuration — exactly the [`Block::inner`] contract, one row narrower
//! per present header/footer — so a card composes with whatever it frames the
//! same way `Block` does.
//!
//! # Deliberately deferred
//!
//! Multi-line headers/footers, independently styled action buttons in the
//! footer, and a body that itself scrolls, are additive follow-ups that
//! compose from a [`Paragraph`](crate::Paragraph)/[`List`](crate::List) drawn
//! into [`inner`](Card::inner) rather than changing this shape — so they are
//! not smuggled in here. Degenerate tiny areas clip to an empty body, never a
//! panic.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::Block;

/// A titled container framing a header, a body, and a footer.
///
/// `Card` wraps a [`Block`] (default [`Block::bordered`]) and, inside its
/// [`inner`](Block::inner), reserves the first row for an optional
/// [`header`](Self::header) [`Line`] and the last row for an optional
/// [`footer`](Self::footer) [`Line`]. The rows between are the body, returned
/// by [`Card::inner`] for the caller to render into — the same
/// render-then-fill-`inner` contract `Block` itself uses.
///
/// Styling cascades card → header/footer → line → span (the same
/// [`Style::patch`](rstui_core::Style) model the text model uses); the base
/// [`style`](Self::style) fills the content region beneath them.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Card;
///
/// let card = Card::new()
///     .title("Profile") // the framing Block's border title
///     .header("Ada Lovelace")
///     .footer("[esc] close");
///
/// // `inner` is the body: Block::inner minus the header and footer rows.
/// assert_eq!(card.inner(Rect::new(0, 0, 20, 7)), Rect::new(1, 2, 18, 3));
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 7));
/// card.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌'); // frame
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'P'); // title
/// assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'A'); // header
/// ```
#[derive(Debug, Clone)]
pub struct Card<'a> {
    block: Block<'a>,
    header: Option<Line<'a>>,
    footer: Option<Line<'a>>,
    style: Style,
    header_style: Style,
    footer_style: Style,
}

impl Default for Card<'_> {
    fn default() -> Self {
        Self {
            block: Block::bordered(),
            header: None,
            footer: None,
            style: Style::new(),
            header_style: Style::new(),
            footer_style: Style::new(),
        }
    }
}

impl<'a> Card<'a> {
    /// A bordered card with no header, footer, or title.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the framing [`Block`] (border, fill, padding, title).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = block;
        self
    }

    /// Sets the framing block's border title — a convenience for
    /// `card.block(card_block.title(..))`.
    #[must_use]
    pub fn title(mut self, title: impl Into<Line<'a>>) -> Self {
        self.block = self.block.title(title);
        self
    }

    /// Sets the header [`Line`] drawn on the first inner row.
    #[must_use]
    pub fn header(mut self, header: impl Into<Line<'a>>) -> Self {
        self.header = Some(header.into());
        self
    }

    /// Sets the footer [`Line`] drawn on the last inner row.
    #[must_use]
    pub fn footer(mut self, footer: impl Into<Line<'a>>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    /// Sets the base [`Style`] filling the content region (header, body,
    /// footer), beneath the header/footer → line → span cascade.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched over the header row.
    #[must_use]
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Sets the [`Style`] patched over the footer row.
    #[must_use]
    pub fn footer_style(mut self, style: Style) -> Self {
        self.footer_style = style;
        self
    }

    /// The body rect: [`Block::inner`] of `area`, minus the first row when a
    /// [`header`](Self::header) is set and the last row when a
    /// [`footer`](Self::footer) is set.
    ///
    /// A pure function of `area` and the configuration — render the caller's
    /// own body content here, exactly the [`Block::inner`] contract. A box too
    /// small to hold the header/footer collapses to an empty body rather than
    /// underflowing.
    #[must_use]
    pub fn inner(&self, area: Rect) -> Rect {
        let bi = self.block.inner(area);
        let top = u16::from(self.header.is_some());
        let bottom = u16::from(self.footer.is_some());
        Rect::new(
            bi.x,
            bi.y.saturating_add(top),
            bi.width,
            bi.height.saturating_sub(top.saturating_add(bottom)),
        )
    }
}

/// Stamps one [`Line`] left-to-right from `(x0, y)`, clipped at `right`,
/// resolving each glyph through `base` → line → span.
fn paint_line(buf: &mut Buffer, line: &Line, x0: u16, y: u16, right: u16, base: Style) {
    let line_base = base.patch(line.style);
    let mut x = x0;
    for span in &line.spans {
        let span_style = line_base.patch(span.style);
        for ch in span.content.chars() {
            if x >= right {
                return;
            }
            buf.set_cell(Position::new(x, y), ch, span_style);
            x = x.saturating_add(1);
        }
    }
}

impl Widget for Card<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Card {
            block,
            header,
            footer,
            style,
            header_style,
            footer_style,
        } = self;

        // The frame is a real Block: it fills its area and draws the
        // border/title. The content region is its inner rect.
        let bi = block.inner(area);
        block.render(area, buf);
        if bi.is_empty() {
            return;
        }

        // Base fills the content region so a background covers header, body,
        // and footer; the lines layer the cascade on top.
        buf.set_style(bi, style);

        let header_rows = u16::from(header.is_some());
        if let Some(header) = header {
            paint_line(
                buf,
                &header,
                bi.left(),
                bi.top(),
                bi.right(),
                style.patch(header_style),
            );
        }
        // The footer takes the last row only when it is distinct from the
        // header row (a one-row box gives the header priority).
        if let Some(footer) = footer {
            if bi.height > header_rows {
                paint_line(
                    buf,
                    &footer,
                    bi.left(),
                    bi.bottom().saturating_sub(1),
                    bi.right(),
                    style.patch(footer_style),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier, Span};

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
    fn a_bare_card_is_just_a_bordered_block() {
        assert_eq!(lines(Card::new(), 4, 3), "┌──┐\n│  │\n└──┘\n");
        // No header/footer: the body is the whole Block inner.
        assert_eq!(
            Card::new().inner(Rect::new(0, 0, 4, 3)),
            Rect::new(1, 1, 2, 1)
        );
    }

    #[test]
    fn a_header_and_footer_each_reserve_one_inner_row() {
        let card = Card::new().header("H").footer("F");
        assert_eq!(card.inner(Rect::new(0, 0, 5, 5)), Rect::new(1, 2, 3, 1));
        assert_eq!(lines(card, 5, 5), "┌───┐\n│H  │\n│   │\n│F  │\n└───┘\n");
    }

    #[test]
    fn only_a_header_reserves_only_the_top_row() {
        let card = Card::new().header("Hi");
        assert_eq!(card.inner(Rect::new(0, 0, 6, 5)), Rect::new(1, 2, 4, 2));
    }

    #[test]
    fn only_a_footer_reserves_only_the_bottom_row() {
        let card = Card::new().footer("Bye");
        assert_eq!(card.inner(Rect::new(0, 0, 6, 5)), Rect::new(1, 1, 4, 2));
    }

    #[test]
    fn title_sets_the_framing_blocks_border_title() {
        assert_eq!(
            lines(Card::new().title("Hi"), 6, 3),
            "┌Hi──┐\n│    │\n└────┘\n"
        );
    }

    #[test]
    fn a_borderless_block_makes_the_card_a_plain_header_body_footer_split() {
        let card = Card::new().block(Block::new()).header("h").footer("f");
        assert_eq!(card.inner(Rect::new(0, 0, 4, 3)), Rect::new(0, 1, 4, 1));
        assert_eq!(lines(card, 4, 3), "h   \n    \nf   \n");
    }

    #[test]
    fn header_and_footer_lines_clip_at_the_inner_right_edge() {
        let card = Card::new()
            .block(Block::new())
            .header("abcdef")
            .footer("ghijkl");
        assert_eq!(lines(card, 3, 3), "abc\n   \nghi\n");
    }

    #[test]
    fn a_one_row_box_gives_the_header_priority_over_the_footer() {
        // 3x3 bordered → 1x1 inner: only the header fits, no body, no footer.
        let card = Card::new().header("H").footer("F");
        assert_eq!(card.inner(Rect::new(0, 0, 3, 3)), Rect::new(1, 2, 1, 0));
        assert_eq!(lines(card, 3, 3), "┌─┐\n│H│\n└─┘\n");
    }

    #[test]
    fn the_body_is_empty_when_the_box_cannot_hold_header_and_footer() {
        // 4x4 bordered → 2x2 inner; header+footer consume both rows.
        let card = Card::new().header("H").footer("F");
        assert!(card.inner(Rect::new(0, 0, 4, 4)).is_empty());
    }

    #[test]
    fn style_cascades_card_header_line_span_and_fills_the_region() {
        let header = Line::from(vec![Span::styled("T", Style::new().fg(Color::Red))])
            .style(Style::new().add_modifier(Modifier::BOLD));
        let card = Card::new()
            .block(Block::new())
            .header(header)
            .style(Style::new().bg(Color::Blue))
            .header_style(Style::new().fg(Color::Green));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        card.render(buf.area(), &mut buf);

        // Span fg wins; line BOLD, card header fg, and base bg cascade.
        let t = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(t.symbol, 'T');
        assert_eq!(t.fg, Color::Red);
        assert_eq!(t.bg, Color::Blue);
        assert!(t.modifier.contains(Modifier::BOLD));
        // The cell past the header still takes the base fill.
        let pad = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(pad.bg, Color::Blue);
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_only_the_frame() {
        assert_eq!(lines(Card::new().header("H").footer("F"), 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Card::new()
            .header("H")
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
