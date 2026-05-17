//! [`Flow`] — a wrapped horizontal run of variable-width items: the pill-row
//! / tag-cloud / chip-bar layout (opentui/opencode's `flexWrap:"wrap"` + `gap`)
//! expressed as an explicit, pure projection rather than a flexbox engine.
//!
//! # Why a widget, not an engine
//!
//! [ADR 0012](https://github.com/andymac4182/rstui/blob/main/docs/adr/0012-widget-composition-and-layout-model.md)
//! §2 records the load-bearing divergence from opentui/GPUI: rstui has **no
//! retained tree and no flexbox engine**, so `flexWrap:"wrap"` + `gap` — the
//! one layout shape plain [`Layout`](rstui_core::Layout) splits cannot express,
//! because the break points depend on the *content* widths — is discharged by
//! this bounded widget, not a solver. Everything else stays explicit `Layout`.
//!
//! # A pure projection with a `Rect` accessor
//!
//! Like every container in the model, `Flow` exposes a pure geometry accessor
//! ([`layout`](Flow::layout)) returning one [`Rect`] per item — the
//! [`Block::inner`](crate::Block::inner)/[`SplitPane::split`](crate::SplitPane::split)
//! discipline — so a caller can place its *own* widgets into the packed slots
//! instead of using the built-in [`Line`] rendering. It owns no state: the
//! items, the [`horizontal_gap`](Flow::horizontal_gap), and the
//! [`vertical_gap`](Flow::vertical_gap) are all caller-owned.
//!
//! # Total, never a panic
//!
//! Per the cross-widget rule a pure projection must be *total*: an empty item
//! list, a zero/tiny area, and an item **wider than the whole area** (clipped
//! to the row, taking its own line) are all safe no-ops/clips — never a panic.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

/// A wrapped horizontal run of [`Line`] items, packed left-to-right and
/// flowing onto new rows when the next item would overflow the area.
///
/// Each item is exactly one [`Line`] (the single-visual-row scoping
/// [`List`](crate::List)/[`Table`](crate::Table) use), so its packed width is
/// its [`Line::width`](rstui_core::Line::width). Items are separated by
/// [`horizontal_gap`](Self::horizontal_gap) cells within a row and rows by
/// [`vertical_gap`](Self::vertical_gap) blank rows; an item wider than the
/// area is clipped to the area width and takes a row of its own.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Flow;
///
/// // Three 3-wide pills with a 1-cell gap in a 9-wide area: the first two fit
/// // on row 0 (3 + 1 + 3 = 7 ≤ 9), the third wraps to row 1.
/// let mut buf = Buffer::empty(Rect::new(0, 0, 9, 2));
/// let flow = Flow::new(["aaa", "bbb", "ccc"]).horizontal_gap(1);
/// let slots = flow.layout(buf.area());
/// assert_eq!(slots[0], Rect::new(0, 0, 3, 1));
/// assert_eq!(slots[1], Rect::new(4, 0, 3, 1));
/// assert_eq!(slots[2], Rect::new(0, 1, 3, 1));
///
/// flow.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'a');
/// assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, 'b');
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'c');
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Flow<'a> {
    items: Vec<Line<'a>>,
    horizontal_gap: u16,
    vertical_gap: u16,
    style: Style,
}

impl<'a> Flow<'a> {
    /// A flow of `items` (each convertible to a [`Line`]), no gaps, no base
    /// style.
    pub fn new<I, T>(items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Line<'a>>,
    {
        Self {
            items: items.into_iter().map(Into::into).collect(),
            horizontal_gap: 0,
            vertical_gap: 0,
            style: Style::new(),
        }
    }

    /// Replaces the items.
    #[must_use]
    pub fn items<I, T>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Line<'a>>,
    {
        self.items = items.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the blank cells reserved between adjacent items on a row.
    #[must_use]
    pub fn horizontal_gap(mut self, gap: u16) -> Self {
        self.horizontal_gap = gap;
        self
    }

    /// Sets the blank rows reserved between wrapped rows.
    #[must_use]
    pub fn vertical_gap(mut self, gap: u16) -> Self {
        self.vertical_gap = gap;
        self
    }

    /// Sets both gaps at once (`horizontal`, then `vertical`).
    #[must_use]
    pub fn gap(mut self, horizontal: u16, vertical: u16) -> Self {
        self.horizontal_gap = horizontal;
        self.vertical_gap = vertical;
        self
    }

    /// Sets the base [`Style`], beneath the base → item-line → span cascade.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The packed slot rectangle for every item, in item order — the pure
    /// geometry accessor (the [`Block::inner`](crate::Block::inner) pattern),
    /// so a caller can render its own widgets into the slots.
    ///
    /// Items pack left-to-right with [`horizontal_gap`](Self::horizontal_gap)
    /// between them; when the next item would pass the right edge it wraps to
    /// a new row [`vertical_gap`](Self::vertical_gap) rows down. Every slot is
    /// **clipped to `area`**: an item wider than the area is one row tall and
    /// at most `area.width` wide, and an item flowed past the bottom collapses
    /// to a zero-area rect (it simply does not render) — never a panic.
    #[must_use]
    pub fn layout(&self, area: Rect) -> Vec<Rect> {
        let mut rects = Vec::with_capacity(self.items.len());
        if area.is_empty() {
            // One zero-area slot per item: total, and still positional.
            rects.resize(self.items.len(), Rect::new(area.x, area.y, 0, 0));
            return rects;
        }

        let mut x = area.left();
        let mut y = area.top();
        let mut row_start = true;
        for line in &self.items {
            // A Line is one visual row, so the packed width is its display
            // width, clamped to u16 then to the row (a wider item is clipped
            // and takes a row of its own).
            let natural = u16::try_from(line.width()).unwrap_or(u16::MAX);
            let item_w = natural.min(area.width);

            if !row_start {
                if x.saturating_add(self.horizontal_gap).saturating_add(item_w) > area.right() {
                    // The next item would overflow: flow onto a new row. No
                    // leading gap — the wrapped item starts the new row.
                    y = y.saturating_add(1).saturating_add(self.vertical_gap);
                    x = area.left();
                } else {
                    x = x.saturating_add(self.horizontal_gap);
                }
            }

            let slot = Rect::new(x, y, item_w, 1).intersection(area);
            rects.push(slot);
            x = x.saturating_add(item_w);
            row_start = false;
        }
        rects
    }
}

impl Widget for Flow<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let slots = self.layout(area);
        for (line, slot) in self.items.iter().zip(&slots) {
            if slot.is_empty() {
                continue;
            }
            // Cascade base → line → span, exactly the text→line→span model the
            // text widgets use; only the item's own glyph cells are stamped so
            // the gaps stay transparent (the pill-row look).
            let line_base = self.style.patch(line.style);
            let right = slot.right();
            let mut x = slot.x;
            'item: for span in &line.spans {
                let span_style = line_base.patch(span.style);
                for ch in span.content.chars() {
                    if x >= right {
                        break 'item;
                    }
                    buf.set_cell(Position::new(x, slot.y), ch, span_style);
                    x = x.saturating_add(1);
                }
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
    fn grid<W: Widget>(widget: W, width: u16, height: u16) -> String {
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
    fn an_empty_flow_lays_out_and_renders_nothing() {
        let flow = Flow::new(Vec::<&str>::new());
        assert!(flow.layout(Rect::new(0, 0, 10, 3)).is_empty());
        assert_eq!(grid(Flow::new(Vec::<&str>::new()), 4, 2), "    \n    \n");
    }

    #[test]
    fn a_single_item_that_fits_sits_at_the_origin() {
        let flow = Flow::new(["hi"]);
        assert_eq!(
            flow.layout(Rect::new(0, 0, 6, 1)),
            vec![Rect::new(0, 0, 2, 1)]
        );
        assert_eq!(grid(Flow::new(["hi"]), 6, 1), "hi    \n");
    }

    #[test]
    fn items_pack_left_to_right_then_wrap_to_the_next_row() {
        // Three 3-wide pills, gap 1, width 9: 3+1+3 = 7 fits, +1+3 = 11 > 9.
        let flow = Flow::new(["aaa", "bbb", "ccc"]).horizontal_gap(1);
        let slots = flow.layout(Rect::new(0, 0, 9, 2));
        assert_eq!(
            slots,
            vec![
                Rect::new(0, 0, 3, 1),
                Rect::new(4, 0, 3, 1),
                Rect::new(0, 1, 3, 1),
            ]
        );
        assert_eq!(grid(flow, 9, 2), "aaa bbb  \nccc      \n");
    }

    #[test]
    fn the_horizontal_gap_is_reserved_between_items_on_a_row() {
        let flow = Flow::new(["a", "b"]).horizontal_gap(2);
        assert_eq!(
            flow.layout(Rect::new(0, 0, 10, 1)),
            vec![Rect::new(0, 0, 1, 1), Rect::new(3, 0, 1, 1)]
        );
    }

    #[test]
    fn the_vertical_gap_is_reserved_between_wrapped_rows() {
        // Two 4-wide items in a 4-wide area: the second wraps; vgap 1 ⇒ it
        // lands on row 2, not row 1.
        let flow = Flow::new(["xxxx", "yyyy"]).vertical_gap(1);
        let slots = flow.layout(Rect::new(0, 0, 4, 3));
        assert_eq!(slots[0], Rect::new(0, 0, 4, 1));
        assert_eq!(slots[1], Rect::new(0, 2, 4, 1));
    }

    #[test]
    fn an_item_wider_than_the_area_is_clipped_to_its_own_row() {
        // "overlong" is 8 wide in a 5-wide area: clipped to width 5, alone on
        // its row; the next item wraps below it.
        let flow = Flow::new(["overlong", "z"]).horizontal_gap(1);
        let slots = flow.layout(Rect::new(0, 0, 5, 2));
        assert_eq!(slots[0], Rect::new(0, 0, 5, 1));
        assert_eq!(slots[1], Rect::new(0, 1, 1, 1));
        assert_eq!(grid(flow, 5, 2), "overl\nz    \n");
    }

    #[test]
    fn an_exact_fit_keeps_items_on_the_same_row() {
        // 4 + gap 1 + 4 = 9 ≤ 9: both stay on row 0, nothing wraps.
        let flow = Flow::new(["aaaa", "bbbb"]).horizontal_gap(1);
        let slots = flow.layout(Rect::new(0, 0, 9, 2));
        assert_eq!(slots, vec![Rect::new(0, 0, 4, 1), Rect::new(5, 0, 4, 1)]);
    }

    #[test]
    fn a_zero_area_is_a_total_no_op() {
        let flow = Flow::new(["a", "b"]).horizontal_gap(1);
        // One collapsed slot per item — total and still positional.
        assert_eq!(
            flow.layout(Rect::new(3, 4, 0, 0)),
            vec![Rect::new(3, 4, 0, 0), Rect::new(3, 4, 0, 0)]
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Flow::new(["a"]).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn items_flowed_past_the_bottom_collapse_and_do_not_render() {
        // Three rows' worth of wrapped items into a height-1 area: only the
        // first row's slot survives the clip; the rest are zero-area.
        let flow = Flow::new(["aaaa", "bbbb", "cccc"]);
        let slots = flow.layout(Rect::new(0, 0, 4, 1));
        assert_eq!(slots[0], Rect::new(0, 0, 4, 1));
        assert!(slots[1].is_empty());
        assert!(slots[2].is_empty());
        assert_eq!(grid(flow, 4, 1), "aaaa\n");
    }

    #[test]
    fn the_style_cascades_base_then_line_then_span() {
        let item = Line::from(vec![Span::styled("X", Style::new().fg(Color::Red))])
            .style(Style::new().add_modifier(Modifier::BOLD));
        let flow = Flow::new([item]).style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        flow.render(buf.area(), &mut buf);
        let c = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(c.symbol, 'X');
        assert_eq!(c.fg, Color::Red); // span fg wins
        assert_eq!(c.bg, Color::Blue); // base bg shows through
        assert!(c.modifier.contains(Modifier::BOLD)); // line modifier cascades
    }

    #[test]
    fn the_layout_accessor_and_render_agree_with_an_origin_offset() {
        // Slots are buffer-absolute and render stamps exactly there.
        let flow = Flow::new(["ab", "cd"]).horizontal_gap(1);
        let area = Rect::new(2, 1, 20, 1);
        let slots = flow.layout(area);
        assert_eq!(slots, vec![Rect::new(2, 1, 2, 1), Rect::new(5, 1, 2, 1)]);
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        flow.render(area, &mut buf);
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, 'a');
        assert_eq!(buf.get(Position::new(5, 1)).unwrap().symbol, 'c');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }
}
