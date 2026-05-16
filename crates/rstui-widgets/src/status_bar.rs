//! [`StatusBar`] — a one-row bar with left-, centre-, and right-anchored
//! segments, the strip every editor/file-manager TUI keeps pinned to an edge
//! (mode + path on the left, a transient message in the middle, cursor
//! position on the right).
//!
//! # A pure projection, like every other widget
//!
//! `StatusBar` owns no state. It is three caller-built [`Line`]s plus a base
//! [`Style`]; the reducer decides *what* each segment says (mode, filename,
//! `12:also a Line`, `Ln 4, Col 9`) and the widget only places and clips them.
//! That keeps it deterministically headless-testable and composes with the
//! Elm `view(&self)` model exactly like [`List`](crate::List) and
//! [`Input`](crate::Input).
//!
//! # A leaf control: one row, no `Block`
//!
//! Like the form controls and unlike the container widgets, `StatusBar` has
//! **no framing [`Block`](crate::Block)**: it draws on exactly the top row of
//! its area, and the surrounding [`Layout`](rstui_core::Layout) owns the frame
//! and which edge the bar sits on (`Layout::vertical` with a
//! `Constraint::Length(1)` bottom row is the idiom — see the demo).
//!
//! # Placement and the precedence rule (total, documented, not ambiguous)
//!
//! The three segments are placed independently, then any contention is
//! resolved by one fixed rule so the output is always well-defined:
//!
//! - **right** is anchored to the right edge and drawn first; it is clipped
//!   only by the total width, and when it overflows its **tail** (the
//!   rightmost glyphs) is kept, not its head. The right segment is the
//!   cursor/position indicator users rely on, so it is the one kept intact
//!   when space is tight (helix/tmux behave the same way).
//! - **left** is drawn from the left edge but **clipped before it reaches the
//!   right segment** — a long path/title is truncated, never overlapped.
//! - **centre** is centred in the *full* width, then clipped to the gap
//!   strictly between where left ends and right begins; if there is no gap it
//!   simply does not draw. It never overwrites left or right.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*):
//! an empty area, a one-cell area, segments far wider than the bar, multi-byte
//! content, a multi-row area, and all-empty segments are safe clips/no-ops —
//! never a panic. Every column is computed with saturating arithmetic and
//! every segment is clipped to the area, so nothing can write out of bounds.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

/// A one-row status strip of three independently-anchored [`Line`] segments
/// (`left`, `center`, `right`) over a base [`Style`] fill.
///
/// Styling cascades bar → line → span (the same
/// [`Style::patch`](rstui_core::Style) model [`List`](crate::List) uses): the
/// base [`style`](Self::style) fills the whole row so a background reads as one
/// bar, then each segment's own [`Line`]/[`Span`](rstui_core::Span) styles
/// layer on top. Placement and the contention rule are described in the
/// [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::StatusBar;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
/// StatusBar::new()
///     .left(" NORMAL ")
///     .center("main.rs")
///     .right(" 12:4 ")
///     .render(buf.area(), &mut buf);
///
/// // Left segment starts at the left edge…
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'N');
/// // …and the right segment is anchored to the right edge.
/// assert_eq!(buf.get(Position::new(19, 0)).unwrap().symbol, ' ');
/// assert_eq!(buf.get(Position::new(18, 0)).unwrap().symbol, '4');
/// ```
#[derive(Debug, Default, Clone)]
pub struct StatusBar<'a> {
    left: Line<'a>,
    center: Line<'a>,
    right: Line<'a>,
    style: Style,
}

impl<'a> StatusBar<'a> {
    /// An empty status bar: no segments, unstyled. Add segments with
    /// [`left`](Self::left)/[`center`](Self::center)/[`right`](Self::right).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the left-anchored segment (drawn from the left edge, clipped before
    /// it reaches the right segment).
    #[must_use]
    pub fn left(mut self, content: impl Into<Line<'a>>) -> Self {
        self.left = content.into();
        self
    }

    /// Sets the centre segment (centred in the full width, then clipped to the
    /// gap between left and right — never overwrites either).
    #[must_use]
    pub fn center(mut self, content: impl Into<Line<'a>>) -> Self {
        self.center = content.into();
        self
    }

    /// Sets the right-anchored segment (drawn at the right edge first; the one
    /// kept intact when space is tight).
    #[must_use]
    pub fn right(mut self, content: impl Into<Line<'a>>) -> Self {
        self.right = content.into();
        self
    }

    /// Sets the base [`Style`], beneath the bar → line → span cascade. It also
    /// fills the bar's row so a background covers it edge to edge.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

/// Flattens `line` into per-column `(char, Style)` cells, resolving each glyph
/// through the base → line → span cascade (identical to `List`'s rule).
fn cells<'a>(line: &Line<'a>, base: Style) -> Vec<(char, Style)> {
    let line_base = base.patch(line.style);
    let mut out = Vec::new();
    for span in &line.spans {
        let span_style = line_base.patch(span.style);
        for ch in span.content.chars() {
            out.push((ch, span_style));
        }
    }
    out
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let StatusBar {
            left,
            center,
            right,
            style,
        } = self;

        let y = area.top();
        let x0 = area.left();
        let width = area.width as usize;

        // Base fills the whole row so a background reads as one contiguous bar
        // (List's selection-bar idiom); segment glyphs layer on top.
        buf.set_style(Rect::new(x0, y, area.width, 1), style);

        let left = cells(&left, style);
        let center = cells(&center, style);
        let right = cells(&right, style);

        // Right is anchored to the right edge and drawn first. When it is too
        // wide for the bar its **tail** is kept (the rightmost glyphs — the
        // `Col 12` of `Ln 128, Col 12`, which is what the user reads), not its
        // head: skip the overflow prefix so the right edge stays intact.
        let right_len = right.len().min(width);
        let right_start = width - right_len; // column index within the area
        let right_skip = right.len().saturating_sub(right_len);
        for (i, (ch, st)) in right.iter().skip(right_skip).take(right_len).enumerate() {
            let col = x0.saturating_add((right_start + i) as u16);
            buf.set_cell(Position::new(col, y), *ch, *st);
        }

        // Left is clipped *before* the right segment so it never overlaps it.
        let left_limit = right_start; // exclusive column (within the area)
        let left_len = left.len().min(left_limit);
        for (i, (ch, st)) in left.iter().take(left_len).enumerate() {
            let col = x0.saturating_add(i as u16);
            buf.set_cell(Position::new(col, y), *ch, *st);
        }

        // Centre is centred in the *full* width, then clamped into the gap
        // strictly between left's end and right's start; no gap → no draw, so
        // it can never overwrite the anchored segments.
        let gap_start = left_len;
        let gap_end = right_start;
        if gap_end > gap_start && !center.is_empty() {
            let gap = gap_end - gap_start;
            let c_len = center.len().min(gap);
            // Ideal centred start in the full width, then clamped to the gap.
            let ideal = width.saturating_sub(center.len()) / 2;
            let start = ideal.clamp(gap_start, gap_end - c_len);
            for (i, (ch, st)) in center.iter().take(c_len).enumerate() {
                let col = x0.saturating_add((start + i) as u16);
                buf.set_cell(Position::new(col, y), *ch, *st);
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
    fn the_three_segments_anchor_left_centre_and_right() {
        let bar = StatusBar::new().left("L").center("C").right("R");
        // width 9: L at col 0, R at col 8, C centred at (9-1)/2 = 4.
        assert_eq!(lines(bar, 9, 1), "L   C   R\n");
    }

    #[test]
    fn right_is_anchored_to_the_right_edge() {
        let bar = StatusBar::new().right("end");
        assert_eq!(lines(bar, 7, 1), "    end\n");
    }

    #[test]
    fn left_is_clipped_before_it_reaches_the_right_segment() {
        // Left wants 10 cols but right occupies the last 3, so left is hard
        // clipped at col 4 — never overlapping the anchored right segment.
        let bar = StatusBar::new().left("0123456789").right("XYZ");
        assert_eq!(lines(bar, 7, 1), "0123XYZ\n");
    }

    #[test]
    fn centre_only_draws_in_the_gap_between_left_and_right() {
        // Left takes 3, right takes 3, width 8 → gap is exactly col 3..5.
        // "MID" (3 chars) cannot fit a 2-wide gap, so it is clipped to 2.
        let bar = StatusBar::new().left("aaa").center("MID").right("bbb");
        assert_eq!(lines(bar, 8, 1), "aaaMIbbb\n");
    }

    #[test]
    fn centre_does_not_draw_when_there_is_no_gap() {
        // Left (4) and right (4) meet exactly at width 8: no gap, centre is
        // silently dropped rather than overwriting an anchored segment.
        let bar = StatusBar::new().left("LLLL").center("xxxx").right("RRRR");
        assert_eq!(lines(bar, 8, 1), "LLLLRRRR\n");
    }

    #[test]
    fn right_survives_a_bar_too_narrow_for_everything() {
        // Width 3 with a 5-char right segment: right is clipped to the width
        // and still wins; left/centre get no room.
        let bar = StatusBar::new().left("left").center("c").right("12345");
        assert_eq!(lines(bar, 3, 1), "345\n");
    }

    #[test]
    fn an_all_empty_bar_just_fills_the_row() {
        assert_eq!(lines(StatusBar::new(), 4, 1), "    \n");
    }

    #[test]
    fn base_style_fills_the_whole_row_including_gaps() {
        let bar = StatusBar::new()
            .left("a")
            .right("b")
            .style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        bar.render(buf.area(), &mut buf);
        for x in 0..6 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
    }

    #[test]
    fn style_cascades_bar_line_then_span() {
        // Bar base is green; the left line is BOLD; one span in it is red.
        let left = Line::from(vec![
            Span::styled("X", Style::new().fg(Color::Red)),
            Span::raw("y"),
        ])
        .style(Style::new().add_modifier(Modifier::BOLD));
        let bar = StatusBar::new()
            .left(left)
            .style(Style::new().fg(Color::Green));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        bar.render(buf.area(), &mut buf);

        let x = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(x.symbol, 'X');
        assert_eq!(x.fg, Color::Red); // span fg wins
        assert!(x.modifier.contains(Modifier::BOLD)); // line modifier cascades

        let y = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(y.symbol, 'y');
        assert_eq!(y.fg, Color::Green); // inherits the bar base (no span fg)
        assert!(y.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn only_the_top_row_of_a_taller_area_is_touched() {
        let bar = StatusBar::new().left("Z");
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        bar.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'Z');
        for y in 1..3 {
            for x in 0..4 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().symbol, ' ');
            }
        }
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let bar = StatusBar::new().left("Hi").right("z");
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        bar.render(Rect::new(2, 3, 6, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 3)).unwrap().symbol, 'H');
        assert_eq!(buf.get(Position::new(3, 3)).unwrap().symbol, 'i');
        assert_eq!(buf.get(Position::new(7, 3)).unwrap().symbol, 'z');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn a_multibyte_segment_maps_each_char_to_one_column() {
        let bar = StatusBar::new().left("é日x");
        assert_eq!(lines(bar, 5, 1), "é日x  \n");
    }

    #[test]
    fn a_one_cell_area_is_total() {
        // Width 1: right wins the only cell; no panic.
        let bar = StatusBar::new().left("L").center("C").right("R");
        assert_eq!(lines(bar, 1, 1), "R\n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        StatusBar::new()
            .left("hello")
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
