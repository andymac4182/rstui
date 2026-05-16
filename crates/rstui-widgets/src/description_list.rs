//! [`DescriptionList`] — an aligned two-column key→value list, the
//! inspector/detail pane every IDE and settings screen has (a property grid,
//! an "About" box, a request-header dump).
//!
//! # A pure projection, like every other widget
//!
//! `DescriptionList` owns no state. It is a list of caller-built
//! [`DescriptionRow`]s (a key [`Line`] and a value [`Text`]); the reducer
//! decides what the rows are and the widget only projects them, exactly the
//! deterministically headless-testable shape [`List`](crate::List) and
//! [`Table`](crate::Table) use.
//!
//! # Value wrapping is *reused*, never re-implemented
//!
//! A detail value is often a long sentence that must wrap inside its column.
//! Rather than grow a second wrap algorithm, the value column is rendered
//! through a private [`Paragraph`] with soft
//! [`Wrap`] and its row height is
//! [`Paragraph::line_count`](crate::Paragraph::line_count) at the value width —
//! so wrapping and right-edge clipping are *inherited*, the same reuse
//! [`Toast`](crate::Toast) makes. The key column width is a [`Constraint`]
//! (the layout vocabulary the whole framework already speaks — no bespoke size
//! type) or auto-fit to the longest key.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no rows, a key column wider than the area (the value column collapses
//! to zero and simply draws nothing), and rows taller than the remaining
//! height (clipped at the bottom) are all safe clips/no-ops — never a panic. An
//! optional framing [`Block`] follows the container-widget convention; a
//! per-row selection/highlight is a deliberately deferred additive (this is a
//! read-only inspector, not a menu — that is [`List`](crate::List)'s job).

use rstui_core::{Buffer, Constraint, Line, Rect, Style, Text, Widget};

use crate::block::Block;
use crate::paragraph::{Paragraph, Wrap};

/// The soft-wrap mode the value column uses, so
/// [`Paragraph::line_count`](crate::Paragraph::line_count) sizing and the
/// rendered value agree exactly.
const VALUE_WRAP: Wrap = Wrap { trim: false };

/// One key→value pair: a single-line [`Line`] key and a (wrappable) [`Text`]
/// value.
///
/// Build the key from anything a [`Line`] is built from and the value from
/// anything a [`Text`] is built from (`&str`, `String`, [`Line`],
/// `Vec<Line>`); style each through the text model it wraps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DescriptionRow<'a> {
    key: Line<'a>,
    value: Text<'a>,
}

impl<'a> DescriptionRow<'a> {
    /// A row pairing `key` with `value`.
    pub fn new(key: impl Into<Line<'a>>, value: impl Into<Text<'a>>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// An aligned two-column key→value list with an optional framing [`Block`].
///
/// The key column is [`key_width`](Self::key_width) wide (a [`Constraint`], or
/// auto-fit to the longest key when unset), then
/// [`column_spacing`](Self::column_spacing) blank columns, then the value
/// column, whose rows soft-wrap through a reused
/// [`Paragraph`]. Styling is a base [`Style`] (filling the
/// pane) with a [`key_style`](Self::key_style) and
/// [`value_style`](Self::value_style) cascaded beneath each row's own
/// [`Line`]/[`Text`] styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{DescriptionList, DescriptionRow};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 2));
/// DescriptionList::new([
///     DescriptionRow::new("Name", "Ada"),
///     DescriptionRow::new("Role", "Eng"),
/// ])
/// .column_spacing(1)
/// .render(buf.area(), &mut buf);
///
/// // Keys are left-aligned in the auto-fit key column…
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'N'); // "Name"
/// // …and values start one column past it (key 4 + spacing 1 → x = 5).
/// assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, 'A'); // "Ada"
/// ```
#[derive(Debug, Clone)]
pub struct DescriptionList<'a> {
    rows: Vec<DescriptionRow<'a>>,
    key_width: Option<Constraint>,
    column_spacing: u16,
    block: Option<Block<'a>>,
    style: Style,
    key_style: Style,
    value_style: Style,
}

impl Default for DescriptionList<'_> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            key_width: None,
            // Two blank columns read as a clear "key:  value" gutter without a
            // separator glyph (the conventional definition-list spacing).
            column_spacing: 2,
            block: None,
            style: Style::default(),
            key_style: Style::default(),
            value_style: Style::default(),
        }
    }
}

impl<'a> DescriptionList<'a> {
    /// A list of `rows` with an auto-fit key column and the default
    /// two-column spacing.
    pub fn new<I>(rows: I) -> Self
    where
        I: IntoIterator<Item = DescriptionRow<'a>>,
    {
        Self {
            rows: rows.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets the key column width [`Constraint`], or `None` to auto-fit it to
    /// the longest key (capped at half the content width so the value column
    /// always has room).
    #[must_use]
    pub fn key_width(mut self, key_width: Option<Constraint>) -> Self {
        self.key_width = key_width;
        self
    }

    /// Sets the blank columns between the key and value columns (default `2`).
    #[must_use]
    pub fn column_spacing(mut self, column_spacing: u16) -> Self {
        self.column_spacing = column_spacing;
        self
    }

    /// Frames the list in `block`; rows render into [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a background
    /// covers the whole pane.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the base [`Style`] for keys, beneath each key [`Line`]'s own style.
    #[must_use]
    pub fn key_style(mut self, style: Style) -> Self {
        self.key_style = style;
        self
    }

    /// Sets the base [`Style`] for values, beneath each value [`Text`]'s own
    /// style.
    #[must_use]
    pub fn value_style(mut self, style: Style) -> Self {
        self.value_style = style;
        self
    }
}

impl Widget for DescriptionList<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let DescriptionList {
            rows,
            key_width,
            column_spacing,
            block,
            style,
            key_style,
            value_style,
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

        // Base fills the content area so a background covers the whole pane
        // (the gutter column and rows past the last entry included).
        buf.set_style(inner, style);
        if rows.is_empty() {
            return;
        }

        // Key column width: the caller's Constraint, or auto-fit to the
        // longest key but never past half the width so a value always fits.
        let key_w = match key_width {
            Some(c) => c.apply(inner.width),
            None => {
                let longest = rows.iter().map(|r| r.key.width()).max().unwrap_or(0) as u16;
                longest.min(inner.width / 2)
            }
        };
        let value_x = inner
            .left()
            .saturating_add(key_w)
            .saturating_add(column_spacing);
        let value_w = inner
            .width
            .saturating_sub(key_w)
            .saturating_sub(column_spacing);

        let key_base = style.patch(key_style);
        let value_base = style.patch(value_style);

        let mut y = inner.top();
        let bottom = inner.bottom();
        for row in rows {
            if y >= bottom {
                break;
            }
            // Value height = its wrapped line count (reusing Paragraph's wrap,
            // never a second algorithm), at least one row for the key.
            let value_para = Paragraph::new(row.value).wrap(VALUE_WRAP).style(value_base);
            let want = u16::try_from(value_para.line_count(value_w)).unwrap_or(u16::MAX);
            let row_h = want.max(1).min(bottom - y);

            // Key: a single unwrapped line at the row's top, clipped to the
            // key column.
            if key_w > 0 {
                Paragraph::new(row.key)
                    .style(key_base)
                    .render(Rect::new(inner.left(), y, key_w, row_h), buf);
            }
            // Value: the wrapped Paragraph in the remaining column.
            if value_w > 0 {
                value_para.render(Rect::new(value_x, y, value_w, row_h), buf);
            }
            y = y.saturating_add(row_h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier, Position, Span};

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
    fn keys_and_values_align_in_two_columns() {
        let dl = DescriptionList::new([
            DescriptionRow::new("A", "x"),
            DescriptionRow::new("BB", "yy"),
        ])
        .column_spacing(1);
        // Auto key width = 2 ("BB"); spacing 1 → value column at x = 3.
        assert_eq!(lines(dl, 8, 2), "A  x    \nBB yy   \n");
    }

    #[test]
    fn an_explicit_key_width_constraint_sizes_the_key_column() {
        let dl = DescriptionList::new([DescriptionRow::new("k", "v")])
            .key_width(Some(Constraint::Length(3)))
            .column_spacing(1);
        // Key column is exactly 3 wide, then 1 gap, then the value at x = 4.
        assert_eq!(lines(dl, 6, 1), "k   v \n");
    }

    #[test]
    fn a_long_value_wraps_in_its_column_reusing_paragraph() {
        let dl = DescriptionList::new([DescriptionRow::new("k", "aa bb")])
            .key_width(Some(Constraint::Length(1)))
            .column_spacing(1);
        // value column width 2: "aa bb" soft-wraps to "aa" / "bb"; the key
        // stays on the row's top line.
        assert_eq!(lines(dl, 4, 2), "k aa\n  bb\n");
    }

    #[test]
    fn rows_stack_and_a_multi_row_value_pushes_the_next_row_down() {
        let dl = DescriptionList::new([
            DescriptionRow::new("a", "p q"),
            DescriptionRow::new("b", "r"),
        ])
        .key_width(Some(Constraint::Length(1)))
        .column_spacing(1);
        // Row 0's value wraps to two rows (cols 0,1); row 1 starts at y = 2.
        assert_eq!(lines(dl, 3, 3), "a p\n  q\nb r\n");
    }

    #[test]
    fn rows_past_the_bottom_are_clipped() {
        let dl = DescriptionList::new([
            DescriptionRow::new("a", "1"),
            DescriptionRow::new("b", "2"),
            DescriptionRow::new("c", "3"),
        ])
        .key_width(Some(Constraint::Length(1)))
        .column_spacing(1);
        // Only two rows of height fit; the third is clipped, no panic.
        assert_eq!(lines(dl, 3, 2), "a 1\nb 2\n");
    }

    #[test]
    fn no_rows_just_fills_the_area() {
        assert_eq!(
            lines(DescriptionList::new(Vec::<DescriptionRow>::new()), 3, 2),
            "   \n   \n"
        );
    }

    #[test]
    fn a_key_column_wider_than_the_area_collapses_the_value() {
        // Length(99) clamps to the width; the value column is then 0 and
        // simply draws nothing — no panic.
        let dl = DescriptionList::new([DescriptionRow::new("key", "value")])
            .key_width(Some(Constraint::Length(99)));
        assert_eq!(lines(dl, 3, 1), "key\n");
    }

    #[test]
    fn style_cascades_base_then_key_and_value_styles() {
        let dl = DescriptionList::new([DescriptionRow::new(
            Line::from(Span::styled("K", Style::new().fg(Color::Red))),
            "v",
        )])
        .key_width(Some(Constraint::Length(1)))
        .column_spacing(1)
        .style(Style::new().bg(Color::Blue))
        .key_style(Style::new().add_modifier(Modifier::BOLD))
        .value_style(Style::new().fg(Color::Green));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        dl.render(buf.area(), &mut buf);

        let k = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(k.symbol, 'K');
        assert_eq!(k.fg, Color::Red); // key span fg wins
        assert!(k.modifier.contains(Modifier::BOLD)); // key_style cascades
        assert_eq!(k.bg, Color::Blue); // base fill cascades

        let v = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(v.symbol, 'v');
        assert_eq!(v.fg, Color::Green); // value_style fg
        assert_eq!(v.bg, Color::Blue);
    }

    #[test]
    fn block_frames_the_list_in_the_inner_area() {
        let dl = DescriptionList::new([DescriptionRow::new("k", "v")])
            .key_width(Some(Constraint::Length(1)))
            .column_spacing(1)
            .block(Block::bordered());
        assert_eq!(lines(dl, 5, 3), "┌───┐\n│k v│\n└───┘\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_rows() {
        let dl = DescriptionList::new([DescriptionRow::new("k", "v")]).block(Block::bordered());
        assert_eq!(lines(dl, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn an_empty_list_with_a_block_still_renders_the_block() {
        let dl = DescriptionList::new(Vec::<DescriptionRow>::new()).block(Block::bordered());
        assert_eq!(lines(dl, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn the_base_style_fills_the_whole_content_area() {
        let dl = DescriptionList::new([DescriptionRow::new("k", "v")])
            .style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        dl.render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..4 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        DescriptionList::new([DescriptionRow::new("k", "v")])
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
