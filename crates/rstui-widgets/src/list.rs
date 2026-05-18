//! [`List`] — a vertical, scrollable, single-select column of rows, the
//! basis for menus, file pickers, command-palette results, and log panes.
//!
//! # A pure projection, on purpose
//!
//! ratatui's list is a `StatefulWidget`: rendering takes `&mut ListState` and
//! *mutates* the scroll offset to keep the selected row on screen. rstui's
//! `App::view` (in `rstui-runtime`) takes `&self` — a view never mutates
//! state — so that pattern does not fit. `List` is therefore a pure
//! projection of `(items, selected, offset)` onto cells: the selection index
//! and the scroll [`offset`](List::offset) are ordinary application state the
//! reducer owns and changes in `update`, exactly like every other field. The
//! widget reads them; it never writes them.
//!
//! That keeps the one-row-per-item index math unambiguous and the whole widget
//! deterministically headless-testable. Multi-line items, per-row alignment,
//! and an ergonomic "scroll the selection into view" seam are deliberately out
//! of scope for this slice (the last is an expensive-to-reverse core-trait
//! question — whether rstui grows a stateful-widget seam at all — and belongs
//! in its own decision record, not smuggled in here).

use std::borrow::Cow;

use crate::block::Block;
use rstui_core::{Buffer, Line, Position, Rect, Span, Style, Widget};

/// One row of a [`List`]: a single [`Line`] of styled text.
///
/// A list row is exactly one visual row in this slice — the overwhelmingly
/// common case (menus, file lists, palette results). Build one from anything a
/// [`Line`] is built from (`&str`, `String`, [`Span`], [`Line`], a
/// `Vec<Span>`); style it through the [`Line`] it wraps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ListItem<'a> {
    line: Line<'a>,
}

impl<'a> ListItem<'a> {
    /// A row displaying `content` (any value convertible to a [`Line`]).
    pub fn new(content: impl Into<Line<'a>>) -> Self {
        Self {
            line: content.into(),
        }
    }

    /// Replaces this row's base [`Style`] (beneath each span's own style).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.line = self.line.style(style);
        self
    }
}

impl<'a> From<&'a str> for ListItem<'a> {
    fn from(s: &'a str) -> Self {
        Self::new(Line::from(s))
    }
}

impl From<String> for ListItem<'_> {
    fn from(s: String) -> Self {
        Self::new(Line::from(s))
    }
}

impl<'a> From<Cow<'a, str>> for ListItem<'a> {
    fn from(s: Cow<'a, str>) -> Self {
        Self::new(Line::from(s))
    }
}

impl<'a> From<Span<'a>> for ListItem<'a> {
    fn from(span: Span<'a>) -> Self {
        Self::new(Line::from(span))
    }
}

impl<'a> From<Line<'a>> for ListItem<'a> {
    fn from(line: Line<'a>) -> Self {
        Self::new(line)
    }
}

impl<'a> From<Vec<Span<'a>>> for ListItem<'a> {
    fn from(spans: Vec<Span<'a>>) -> Self {
        Self::new(Line::from(spans))
    }
}

/// A vertical column of selectable rows with an optional framing [`Block`].
///
/// `List` shows the window of items `[offset, offset + height)` — one row per
/// item — applying a full-width [`highlight_style`](Self::highlight_style) bar
/// and an optional gutter [`highlight_symbol`](Self::highlight_symbol) to the
/// [`selected`](Self::selected) row. Both the selection index and the scroll
/// [`offset`](Self::offset) are caller-supplied state, never mutated here (see
/// the [module docs](self) for why).
///
/// Styling cascades list → item-line → span (the same
/// [`Style::patch`](rstui_core::Style) model the text model uses); the list
/// base style also fills the content area so a background covers the whole
/// pane. On the selected row [`highlight_style`](Self::highlight_style) is
/// patched **last**, so it overrides per-item styling and reads as one
/// contiguous bar across the gutter, the text, and the trailing padding.
///
/// The gutter is reserved (blank) on every row whenever a
/// [`highlight_symbol`](Self::highlight_symbol) is set, so row text never
/// shifts column as the selection moves.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::List;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
/// List::new(["one", "two"])
///     .highlight_symbol("> ")
///     .selected(Some(1))
///     .render(buf.area(), &mut buf);
///
/// // The gutter is reserved on every row, so "one" and "two" share a column…
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'o');
/// assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, 't');
/// // …and only the selected row paints the symbol into that gutter.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '>');
/// ```
#[derive(Debug, Default, Clone)]
pub struct List<'a> {
    items: Cow<'a, [ListItem<'a>]>,
    block: Option<Block<'a>>,
    style: Style,
    highlight_style: Style,
    highlight_symbol: Option<Cow<'a, str>>,
    selected: Option<usize>,
    offset: usize,
}

impl<'a> List<'a> {
    /// A list of `items`, nothing selected, scrolled to the top.
    pub fn new<I, T>(items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<ListItem<'a>>,
    {
        Self {
            items: Cow::Owned(items.into_iter().map(Into::into).collect()),
            ..Self::default()
        }
    }

    /// A list over caller-owned `items` the widget **borrows** instead of
    /// collecting a fresh `Vec` each frame — the allocation-free path for a
    /// reducer that already holds `&[ListItem]` in its model (the
    /// pure-projection seam, the same one `Menu`/`Sidebar` window through).
    /// Identical projection to [`new`](Self::new); the owned-iterator
    /// constructor is unchanged (it wraps its collected `Vec` in
    /// `Cow::Owned`), so this is purely additive.
    #[must_use]
    pub fn from_slice(items: &'a [ListItem<'a>]) -> Self {
        Self {
            items: Cow::Borrowed(items),
            ..Self::default()
        }
    }

    /// Frames the list in `block`; rows render into [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`], beneath the list → item → span cascade. It
    /// also fills the content area so a background covers the whole pane.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched over the selected row.
    ///
    /// Patched **last** in the cascade, so it overrides per-item styling, and
    /// applied across the full row width so the selection reads as one bar.
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Sets the gutter string drawn before the selected row.
    ///
    /// The gutter is reserved (blank) on unselected rows too, so row text
    /// keeps its column as the selection moves.
    #[must_use]
    pub fn highlight_symbol(mut self, symbol: impl Into<Cow<'a, str>>) -> Self {
        self.highlight_symbol = Some(symbol.into());
        self
    }

    /// Sets which item index is highlighted, or `None` for no selection.
    ///
    /// An index outside the visible window simply paints no bar — the caller
    /// owns scrolling (see the [module docs](self)).
    #[must_use]
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the index of the first visible item (the scroll offset).
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// The item index at cell `pos` for `area`, if a row is there.
    ///
    /// The pure inverse of the render layout — clicking what you see picks
    /// that item. It accounts for the framing [`block`](Self::block) and the
    /// caller-owned scroll [`offset`](Self::offset) once, here, instead of
    /// every app re-deriving it (and getting the border/offset wrong).
    /// `None` outside the populated rows; the gutter and the trailing pad
    /// share a row's index, so a click anywhere on a row hits it. Hit-test
    /// on a click to select, or on press+drag to reorder.
    #[must_use]
    pub fn row_at(&self, area: Rect, pos: Position) -> Option<usize> {
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if inner.is_empty() || !inner.contains(pos) {
            return None;
        }
        let idx = self.offset + usize::from(pos.y - inner.top());
        (idx < self.items.len()).then_some(idx)
    }
}

impl Widget for List<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let List {
            items,
            block,
            style,
            highlight_style,
            highlight_symbol,
            selected,
            offset,
        } = self;

        // The block (if any) frames the content and reserves the inner area.
        let inner = match &block {
            Some(b) => b.inner(area),
            None => area,
        };
        if let Some(b) = block {
            b.render(area, buf);
        }
        if inner.is_empty() {
            return;
        }

        // List base fills the content area so a background covers the whole
        // pane (including rows past the last item); glyphs layer the
        // list → item → span cascade on top.
        buf.set_style(inner, style);

        let gutter = highlight_symbol.as_deref().unwrap_or("");
        let gutter_width = gutter.chars().count() as u16;
        let bar_style = style.patch(highlight_style);

        let left = inner.left();
        let right = inner.right();
        let top = inner.top();
        let content_x0 = left.saturating_add(gutter_width);

        for (row, (idx, item)) in items
            .iter()
            .enumerate()
            .skip(offset)
            .take(inner.height as usize)
            .enumerate()
        {
            let y = top.saturating_add(row as u16);
            let is_selected = selected == Some(idx);

            if is_selected {
                // The selection bar: highlight patched over the base fill
                // across the full row, so the gutter and the trailing padding
                // read as one contiguous block, not just the glyph cells.
                buf.set_style(Rect::new(left, y, inner.width, 1), highlight_style);

                // The gutter symbol only paints on the selected row; every
                // other row leaves it blank so columns stay put.
                let mut x = left;
                for ch in gutter.chars() {
                    if x >= content_x0 || x >= right {
                        break;
                    }
                    buf.set_cell(Position::new(x, y), ch, bar_style);
                    x = x.saturating_add(1);
                }
            }

            // Resolve each glyph through list → item-line → span, then patch
            // the highlight last on the selected row so it wins over per-item
            // styling exactly as the full-width bar does.
            let line = &item.line;
            let line_base = style.patch(line.style);
            let mut x = content_x0;
            'row: for span in &line.spans {
                let mut span_style = line_base.patch(span.style);
                if is_selected {
                    span_style = span_style.patch(highlight_style);
                }
                for ch in span.content.chars() {
                    if x >= right {
                        break 'row;
                    }
                    buf.set_cell(Position::new(x, y), ch, span_style);
                    x = x.saturating_add(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier};

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
    fn each_item_is_one_left_aligned_clipped_row() {
        assert_eq!(
            lines(List::new(["abcdef", "XY"]), 4, 3),
            "abcd\nXY  \n    \n"
        );
    }

    #[test]
    fn offset_skips_leading_items_and_height_clips_trailing() {
        let list = List::new(["i0", "i1", "i2", "i3"]).offset(1);
        assert_eq!(lines(list, 2, 2), "i1\ni2\n");
    }

    #[test]
    fn highlight_symbol_gutter_is_reserved_on_every_row() {
        // "> " is two columns: only the selected row paints it, but text on
        // every row starts past it so columns never shift.
        let list = List::new(["one", "two"])
            .highlight_symbol("> ")
            .selected(Some(1));
        assert_eq!(lines(list, 5, 2), "  one\n> two\n");
    }

    #[test]
    fn no_selection_paints_no_symbol_anywhere() {
        let list = List::new(["one", "two"]).highlight_symbol("> ");
        assert_eq!(lines(list, 5, 2), "  one\n  two\n");
    }

    #[test]
    fn a_selection_outside_the_visible_window_paints_no_bar() {
        // Item 3 is selected but the offset/height window only shows 0..2;
        // nothing is highlighted and rendering does not panic.
        let list = List::new(["a", "b", "c", "d"])
            .selected(Some(3))
            .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        list.render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Reset);
            }
        }
    }

    #[test]
    fn highlight_style_is_a_full_width_bar_over_gutter_text_and_padding() {
        let list = List::new(["hi"])
            .highlight_symbol("> ")
            .selected(Some(0))
            .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        list.render(buf.area(), &mut buf);
        // Gutter symbol, text, and the empty cells after the text all share
        // the highlight background — one contiguous bar.
        for x in 0..6 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '>');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'h');
        assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn base_style_fills_the_whole_content_area() {
        let list = List::new(["x"]).style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        list.render(buf.area(), &mut buf);
        // The single item is row 0; the empty row 1 is still filled.
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
    }

    #[test]
    fn block_frames_rows_in_the_inner_area() {
        assert_eq!(
            lines(List::new(["hi"]).block(Block::bordered()), 4, 3),
            "┌──┐\n│hi│\n└──┘\n"
        );
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_rows() {
        assert_eq!(
            lines(List::new(["Z"]).block(Block::bordered()), 2, 2),
            "┌┐\n└┘\n"
        );
    }

    #[test]
    fn style_cascades_list_item_span_and_highlight_wins_last() {
        // Item line is BOLD; one span is red. The list base is green. On the
        // selected row the highlight bg is patched last (over everything).
        let item = ListItem::new(
            Line::from(vec![
                Span::styled("X", Style::new().fg(Color::Red)),
                Span::raw("y"),
            ])
            .style(Style::new().add_modifier(Modifier::BOLD)),
        );
        let list = List::new([item])
            .style(Style::new().fg(Color::Green))
            .selected(Some(0))
            .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        list.render(buf.area(), &mut buf);

        let x = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(x.symbol, 'X');
        assert_eq!(x.fg, Color::Red); // span fg survives
        assert_eq!(x.bg, Color::Blue); // highlight patched last
        assert!(x.modifier.contains(Modifier::BOLD)); // line modifier cascades

        let y = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(y.symbol, 'y');
        assert_eq!(y.fg, Color::Green); // inherits list base (no span fg)
        assert_eq!(y.bg, Color::Blue);
        assert!(y.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn an_empty_list_with_a_block_still_renders_the_block() {
        assert_eq!(
            lines(List::new(Vec::<&str>::new()).block(Block::bordered()), 3, 3),
            "┌─┐\n│ │\n└─┘\n"
        );
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        List::new(["hello"])
            .selected(Some(0))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn from_slice_renders_identically_to_the_owned_constructor() {
        // `Cow::Borrowed` vs `Cow::Owned` is invisible to render; pinned
        // with a selection + offset so the bar/scroll paths are covered.
        let items = [
            ListItem::new("alpha"),
            ListItem::new("beta"),
            ListItem::new("gamma"),
            ListItem::new("delta"),
        ];
        let area = Rect::new(0, 0, 12, 3);
        let mut owned = Buffer::empty(area);
        List::new(items.iter().cloned())
            .selected(Some(2))
            .offset(1)
            .render(area, &mut owned);
        let mut borrowed = Buffer::empty(area);
        List::from_slice(&items)
            .selected(Some(2))
            .offset(1)
            .render(area, &mut borrowed);
        assert_eq!(owned.cells(), borrowed.cells());
    }

    #[test]
    fn row_at_inverts_the_layout_with_offset_and_block() {
        let l = List::new(["a", "b", "c", "d", "e"]);
        let area = Rect::new(0, 0, 10, 5);
        assert_eq!(l.row_at(area, Position::new(3, 0)), Some(0));
        assert_eq!(l.row_at(area, Position::new(0, 2)), Some(2));
        assert_eq!(l.row_at(area, Position::new(9, 4)), Some(4));
        assert_eq!(l.row_at(area, Position::new(0, 9)), None); // off-area
        // The scroll offset shifts the mapping; past the last item ⇒ None.
        let s = List::new(["a", "b", "c", "d", "e"]).offset(2);
        assert_eq!(s.row_at(area, Position::new(0, 0)), Some(2));
        assert_eq!(s.row_at(area, Position::new(0, 2)), Some(4));
        assert_eq!(s.row_at(area, Position::new(0, 3)), None); // idx 5 ≥ len
        // A framing block insets the rows by its border.
        let b = List::new(["a", "b", "c"]).block(crate::Block::bordered());
        let ba = Rect::new(0, 0, 10, 7);
        assert_eq!(b.row_at(ba, Position::new(0, 0)), None); // on the border
        assert_eq!(b.row_at(ba, Position::new(2, 1)), Some(0)); // first inner row
        assert_eq!(b.row_at(ba, Position::new(2, 3)), Some(2));
    }
}
