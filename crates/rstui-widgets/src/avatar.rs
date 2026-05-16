//! [`Avatar`] — a small initials swatch: a caller-provided 1–3 character
//! monogram on an accent fill, the chat/comment/member-list identity chip.
//!
//! # A pure projection of caller-owned initials
//!
//! Like every rstui widget `Avatar` is a **pure projection**: it renders the
//! caller-owned initials it is handed (the first three [`char`]s — whatever the
//! app derived from a name; the widget does **not** compute or transform them,
//! exactly as [`Calendar`](crate::Calendar) does no date math) on an accent
//! [`style`](Avatar::style) fill, and reads nothing else.
//!
//! # A leaf swatch — no [`Block`](crate::Block), no label
//!
//! Like [`Badge`](crate::Badge)/[`Spinner`](crate::Spinner) it is a small
//! *adornment*, not a container: no optional frame and no label (a name beside
//! the chip is ordinary text the app composes with a
//! [`Layout`](rstui_core::Layout) split). The accent **fills the whole area**
//! (the base-fills convention) so the swatch reads as one solid block even when
//! the initials are empty, and the monogram is centred within it — biasing odd
//! leftover space toward the start, matching
//! [`Alignment::Center`](rstui_core::Alignment) exactly as
//! [`Modal`](crate::Modal) centres its dialog.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, empty initials (a blank accent swatch), and initials wider than the
//! area (clipped) are all safe no-ops/clips — never a panic.

use std::borrow::Cow;

use rstui_core::{Buffer, Position, Rect, Style, Widget};

/// The most initials an [`Avatar`] shows (a 1–3 character monogram).
const MAX_INITIALS: usize = 3;

/// A small initials swatch — a pure projection of caller-provided initials.
///
/// Up to three caller-provided characters, centred on an accent
/// [`style`](Self::style) fill. A leaf widget: no frame, no label.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Avatar;
///
/// // The initials are plain caller-owned state — whatever the app derived
/// // from the member's name; the widget only reads and centres them.
/// let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
/// Avatar::new("AM").render(buf.area(), &mut buf);
///
/// // "AM" centred in the 4-wide swatch: one pad column each side.
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'A');
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'M');
/// ```
#[derive(Debug, Clone)]
pub struct Avatar<'a> {
    initials: Cow<'a, str>,
    style: Style,
}

impl<'a> Avatar<'a> {
    /// An avatar showing `initials` (the first three [`char`]s are used),
    /// unstyled.
    pub fn new(initials: impl Into<Cow<'a, str>>) -> Self {
        Self {
            initials: initials.into(),
            style: Style::new(),
        }
    }

    /// Sets the accent [`Style`] — it fills the whole swatch and styles the
    /// initials' glyph cells.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl Default for Avatar<'_> {
    /// An empty-initials avatar — a blank accent swatch (total, see the
    /// [module docs](self)).
    fn default() -> Self {
        Self::new("")
    }
}

impl Widget for Avatar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // Accent fills the whole swatch (the base-fills convention) so an
        // empty / degenerate avatar still reads as one solid block.
        buf.set_style(area, self.style);

        // Up to three initials, clipped to the width, centred — odd leftover
        // biased toward the start (Alignment::Center), on the middle row.
        let glyphs: Vec<char> = self.initials.chars().take(MAX_INITIALS).collect();
        let take = (glyphs.len() as u16).min(area.width);
        if take == 0 {
            return;
        }
        let x0 = area.left() + (area.width - take) / 2;
        let y = area.top() + area.height / 2;
        for (i, ch) in glyphs.into_iter().take(take as usize).enumerate() {
            buf.set_cell(Position::new(x0 + i as u16, y), ch, self.style);
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
    fn initials_are_centred_horizontally_and_vertically() {
        // "AM" in a 4x3 swatch: centred column-wise (pad 1 each side) and on
        // the middle row.
        assert_eq!(lines(Avatar::new("AM"), 4, 3), "    \n AM \n    \n");
    }

    #[test]
    fn the_accent_style_fills_the_whole_swatch() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        Avatar::new("X")
            .style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Blue);
            }
        }
    }

    #[test]
    fn only_the_first_three_characters_are_used() {
        assert_eq!(lines(Avatar::new("ABCDE"), 3, 1), "ABC\n");
    }

    #[test]
    fn initials_wider_than_the_swatch_are_clipped() {
        // 3 initials but a 2-wide swatch: clipped to the first two, no panic.
        assert_eq!(lines(Avatar::new("ABC"), 2, 1), "AB\n");
    }

    #[test]
    fn a_single_initial_is_centred() {
        assert_eq!(lines(Avatar::new("Q"), 5, 1), "  Q  \n");
    }

    #[test]
    fn empty_initials_are_a_styled_blank_block() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        Avatar::new("")
            .style(Style::new().bg(Color::Green))
            .render(buf.area(), &mut buf);
        for p in buf.area().positions() {
            let c = buf.get(p).unwrap();
            assert_eq!(c.symbol, ' ');
            assert_eq!(c.bg, Color::Green);
        }
        // The Default avatar is exactly this blank swatch.
        let mut other = Buffer::empty(Rect::new(0, 0, 3, 2));
        Avatar::default()
            .style(Style::new().bg(Color::Green))
            .render(other.area(), &mut other);
        assert_eq!(buf.cells(), other.cells());
    }

    #[test]
    fn a_multi_row_block_places_initials_on_the_middle_row() {
        // 5 rows tall → middle row is index 2.
        assert_eq!(
            lines(Avatar::new("AB"), 4, 5),
            "    \n    \n AB \n    \n    \n"
        );
    }

    #[test]
    fn the_accent_styles_the_glyph_cells() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        Avatar::new("Z")
            .style(Style::new().fg(Color::Black).bg(Color::Yellow))
            .render(buf.area(), &mut buf);
        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.symbol, 'Z');
        assert_eq!(cell.fg, Color::Black);
        assert_eq!(cell.bg, Color::Yellow);
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
        Avatar::new("A").render(Rect::new(3, 2, 1, 1), &mut buf);
        assert_eq!(buf.get(Position::new(3, 2)).unwrap().symbol, 'A');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn a_one_cell_block_keeps_one_initial() {
        assert_eq!(lines(Avatar::new("AM"), 1, 1), "A\n");
    }

    #[test]
    fn odd_leftover_padding_biases_toward_the_start() {
        // Swatch 5 wide, "AB" 2 wide: 3 spare → 1 left, 2 right (start bias),
        // matching Alignment::Center's "odd remainder toward the start".
        assert_eq!(lines(Avatar::new("AB"), 5, 1), " AB  \n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        Avatar::new("AM")
            .style(Style::new().bg(Color::Red))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
