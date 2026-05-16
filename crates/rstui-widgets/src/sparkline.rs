//! [`Sparkline`] — a compact, one-row trend of a numeric series drawn with the
//! eight vertical block glyphs, the inline "last N samples" strip a dashboard
//! pins next to a label (request rate, CPU, queue depth).
//!
//! # A pure projection, like every other widget
//!
//! `Sparkline` owns no state. It is a borrowed caller-owned `&[u64]` plus an
//! optional ceiling and a [`Style`]; the reducer decides *what* the series is
//! (a ring buffer it pushes a sample onto in `update`) and the widget only
//! projects "the numbers right now" onto glyphs. That keeps it
//! deterministically headless-testable and composes with the Elm `view(&self)`
//! model exactly like [`List`](crate::List) and [`Gauge`](crate::Gauge).
//!
//! # The sub-cell idea, one axis over from [`Gauge`](crate::Gauge)
//!
//! [`Gauge`](crate::Gauge) renders one horizontal bar at eighth-of-a-cell
//! precision. A sparkline is the same eighth-block trick applied per column on
//! the *vertical* axis: each sample becomes one of `▁▂▃▄▅▆▇█` (a single
//! Unicode scalar, so it maps 1:1 onto a [`Cell`](rstui_core::Buffer) with no
//! grapheme machinery — the same reasoning [`Block`](crate::Block) borders and
//! the gauge ramp use). A zero sample is the blank track, never a glyph.
//!
//! # A leaf adornment: one row, no `Block`
//!
//! Like [`StatusBar`](crate::StatusBar) and unlike the container widgets,
//! `Sparkline` has **no framing [`Block`](crate::Block)**: it draws the trend
//! on exactly the **top** row of its area (the base [`Style`] still fills the
//! whole area so a background reads as one strip), and the surrounding
//! [`Layout`](rstui_core::Layout) owns any frame. A *multi-row* column chart
//! with labels is deliberately a different widget ([`BarChart`](crate::BarChart)),
//! not a mode smuggled in here.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, an empty series, an all-zero series, a single sample, more samples
//! than columns (the tail is clipped), fewer samples than columns (the rest is
//! blank track), and a sample above the ceiling (clamped to a full block) are
//! all safe clips/no-ops — never a panic.

use rstui_core::{Buffer, Position, Rect, Style, Widget};

/// The eight bottom-aligned block elements, `1/8` … `8/8` tall.
///
/// `BARS[n - 1]` is the glyph for `n` eighths; `BARS[7]` is the full block. A
/// zero sample draws no glyph at all (the cell stays the blank track).
const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A one-row trend of a borrowed `&[u64]` series, scaled to an optional
/// ceiling and drawn with the eight vertical block glyphs.
///
/// Each sample is one column: its value, scaled against
/// [`max`](Self::max) (the largest sample when unset), picks the nearest of
/// `▁▂▃▄▅▆▇█`; a zero sample is the blank track. The newest convention is the
/// caller's — the widget renders `data[0]` in the leftmost column and clips the
/// tail at the right edge — so a reducer that `push`es and trims a ring buffer
/// composes without the widget ever reordering.
///
/// Styling is a single base [`Style`] (the series is one visual run, unlike the
/// text widgets' line/span cascade); it also fills the area so a background
/// covers the whole strip.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Sparkline;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
/// let series = [0u64, 2, 4, 8];
/// Sparkline::new(&series).max(Some(8)).render(buf.area(), &mut buf);
///
/// // 0/8 is the blank track; 8/8 is the full block; the rest scale between.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
/// assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, '█');
/// ```
#[derive(Debug, Default, Clone)]
pub struct Sparkline<'a> {
    data: &'a [u64],
    max: Option<u64>,
    style: Style,
}

impl<'a> Sparkline<'a> {
    /// A sparkline projecting `data` (sample `0` leftmost), auto-scaled to the
    /// largest sample, unstyled.
    #[must_use]
    pub fn new(data: &'a [u64]) -> Self {
        Self {
            data,
            max: None,
            style: Style::default(),
        }
    }

    /// Sets the value mapped to a full block, or `None` to auto-scale to the
    /// largest sample.
    ///
    /// A sample above the ceiling is clamped to a full block (never a panic —
    /// the [`Gauge`](crate::Gauge) totality rule); `Some(0)` and an all-zero
    /// auto-scaled series both render a blank track.
    #[must_use]
    pub fn max(mut self, max: Option<u64>) -> Self {
        self.max = max;
        self
    }

    /// Sets the base [`Style`] for the glyphs; it also fills the area so a
    /// background covers the whole strip.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl Widget for Sparkline<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Sparkline { data, max, style } = self;

        // Base fills the whole area so a background reads as one strip; glyphs
        // layer on the top row only (this is a one-row adornment).
        buf.set_style(area, style);

        // The ceiling: the caller's, or the largest sample. A zero ceiling
        // means every column is the blank track.
        let ceiling = max.unwrap_or_else(|| data.iter().copied().max().unwrap_or(0));
        if ceiling == 0 {
            return;
        }

        let y = area.top();
        let right = area.right();
        let mut x = area.left();
        for &value in data {
            if x >= right {
                break;
            }
            // Clamp to the ceiling, then round to the nearest eighth. A
            // non-zero sample never rounds away to nothing — it shows at least
            // one eighth so a small blip is still visible.
            let clamped = value.min(ceiling);
            if clamped > 0 {
                let eighths = ((clamped * 8) + ceiling / 2) / ceiling;
                let level = eighths.clamp(1, 8) as usize;
                buf.set_cell(Position::new(x, y), BARS[level - 1], style);
            }
            x = x.saturating_add(1);
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
    fn auto_scale_maps_the_largest_sample_to_a_full_block() {
        // max is the largest sample (8); 0→blank, 4→half, 8→full.
        let data = [0u64, 4, 8];
        assert_eq!(lines(Sparkline::new(&data), 3, 1), " ▄█\n");
    }

    #[test]
    fn an_explicit_ceiling_scales_against_it() {
        let data = [2u64, 4, 8];
        // Ceiling 8: 2/8≈▂, 4/8=▄, 8/8=█.
        assert_eq!(lines(Sparkline::new(&data).max(Some(8)), 3, 1), "▂▄█\n");
    }

    #[test]
    fn the_eighth_ramp_maps_each_fraction_to_its_block() {
        let data = [1u64, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(
            lines(Sparkline::new(&data).max(Some(8)), 8, 1),
            "▁▂▃▄▅▆▇█\n"
        );
    }

    #[test]
    fn a_value_above_the_ceiling_clamps_to_a_full_block() {
        let data = [99u64];
        assert_eq!(lines(Sparkline::new(&data).max(Some(8)), 1, 1), "█\n");
    }

    #[test]
    fn a_tiny_non_zero_sample_still_shows_one_eighth() {
        // 1 against a ceiling of 1000 rounds to 0 eighths, but a non-zero
        // sample is never invisible — it floors at ▁.
        let data = [0u64, 1];
        assert_eq!(lines(Sparkline::new(&data).max(Some(1000)), 2, 1), " ▁\n");
    }

    #[test]
    fn an_all_zero_series_is_a_blank_track() {
        let data = [0u64, 0, 0];
        assert_eq!(lines(Sparkline::new(&data), 3, 1), "   \n");
    }

    #[test]
    fn an_explicit_zero_ceiling_renders_nothing() {
        let data = [3u64, 7];
        assert_eq!(lines(Sparkline::new(&data).max(Some(0)), 2, 1), "  \n");
    }

    #[test]
    fn a_single_sample_is_one_column() {
        let data = [5u64];
        assert_eq!(lines(Sparkline::new(&data).max(Some(5)), 3, 1), "█  \n");
    }

    #[test]
    fn more_samples_than_columns_clip_at_the_right_edge() {
        let data = [8u64, 8, 8, 8, 8];
        assert_eq!(lines(Sparkline::new(&data).max(Some(8)), 3, 1), "███\n");
    }

    #[test]
    fn an_empty_series_just_fills_the_area() {
        let data: [u64; 0] = [];
        assert_eq!(lines(Sparkline::new(&data), 3, 1), "   \n");
    }

    #[test]
    fn only_the_top_row_of_a_taller_area_is_touched() {
        let data = [8u64];
        let spark = Sparkline::new(&data).max(Some(8));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 3));
        spark.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '█');
        for y in 1..3 {
            for x in 0..2 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().symbol, ' ');
            }
        }
    }

    #[test]
    fn the_base_style_fills_the_whole_area() {
        let data = [8u64];
        let spark = Sparkline::new(&data)
            .max(Some(8))
            .style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        spark.render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
        // The glyph also carries the style fg.
        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.symbol, '█');
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let data = [8u64, 8];
        let spark = Sparkline::new(&data).max(Some(8));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        spark.render(Rect::new(2, 3, 2, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 3)).unwrap().symbol, '█');
        assert_eq!(buf.get(Position::new(3, 3)).unwrap().symbol, '█');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let data = [1u64, 2, 3];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Sparkline::new(&data).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
