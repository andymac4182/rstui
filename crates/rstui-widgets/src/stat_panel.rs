//! [`StatPanel`] — the Grafana "stat"/single-stat panel: one big headline
//! metric with a caption, a trend delta, and an optional inline sparkline
//! backdrop; the observability KPI tile (requests/s, error rate, p99, uptime).
//!
//! # A richer sibling of [`Card`](crate::Card)
//!
//! [`Card`](crate::Card) frames a header/body/footer; a `StatPanel` frames the
//! one layout every dashboard repeats for a single number — a small caption, a
//! big value, a `glyph delta` change row, and a faint trend backdrop. It is
//! the overwhelmingly common refinement of "a framed tile that shows one KPI",
//! packaged so callers stop hand-rolling the same split on every metric.
//!
//! # A pure projection, like every other widget
//!
//! `StatPanel` owns no state. The caption, value, and delta are caller-built
//! [`Line`]s and the sparkline series is a borrowed `&[u64]`; the reducer
//! decides *what* the numbers are (a ring buffer it pushes onto in `update`)
//! and the widget only projects "the KPI right now". That keeps it
//! deterministically headless-testable and composes with the Elm `view(&self)`
//! model exactly like [`Card`](crate::Card) and [`BarChart`](crate::BarChart).
//!
//! # No hardcoded semantics
//!
//! Whether a rise is good (throughput) or bad (error rate) is the caller's
//! call, so [`StatPanel`] never colours the delta itself: [`Trend`] only picks
//! the glyph (`▲`/`▼`/`▬`) and the caller supplies the
//! [`trend_style`](StatPanel::trend_style) (green/red/grey). The sparkline is
//! the same eighth-block trick [`Sparkline`](crate::Sparkline) uses, inlined
//! here as a faint backdrop rather than a dependency on that widget.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, a missing caption/delta, an empty or all-zero series, more samples
//! than columns (the tail is clipped), and an area too short to hold every row
//! (the lower rows are dropped) are all safe clips/no-ops — never a panic.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Position, Rect, Widget};
//! use rstui_widgets::{StatPanel, Trend};
//!
//! let series = [3u64, 5, 4, 8];
//! let mut buf = Buffer::empty(Rect::new(0, 0, 12, 4));
//! StatPanel::new("182 ms")
//!     .caption("p99 latency")
//!     .delta("+9 ms")
//!     .trend(Trend::Up)
//!     .sparkline(&series)
//!     .render(buf.area(), &mut buf);
//!
//! // Row 0 is the caption, row 1 the big value, row 2 the `glyph delta`.
//! assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'p');
//! assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '1');
//! assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, '▲');
//! assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, '+');
//! ```

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The eight bottom-aligned block elements, `1/8` … `8/8` tall, the same ramp
/// [`Sparkline`](crate::Sparkline) draws its trend with.
///
/// `BARS[n - 1]` is the glyph for `n` eighths; `BARS[7]` is the full block. A
/// zero sample draws no glyph (the cell stays the blank backdrop).
const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The direction a [`StatPanel`]'s metric moved, picking the delta glyph.
///
/// This is *only* a glyph selector — whether the move is good or bad is the
/// caller's [`trend_style`](StatPanel::trend_style), never hardcoded here.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Trend {
    /// The metric rose; the delta row is prefixed with `▲`.
    Up,
    /// The metric fell; the delta row is prefixed with `▼`.
    Down,
    /// The metric is unchanged (the default); the delta row is prefixed with
    /// `▬`.
    #[default]
    Flat,
}

impl Trend {
    /// The single-scalar glyph for this trend: `▲` up, `▼` down, `▬` flat.
    ///
    /// Each is one Unicode scalar, so it maps 1:1 onto a
    /// [`Cell`](rstui_core::Buffer) with no grapheme machinery — the same
    /// reasoning the [`Sparkline`](crate::Sparkline) ramp uses.
    #[must_use]
    pub fn glyph(self) -> char {
        match self {
            Trend::Up => '▲',
            Trend::Down => '▼',
            Trend::Flat => '▬',
        }
    }
}

/// The Grafana single-stat tile: a caption, a big value, a `glyph delta` trend
/// row, and an optional faint sparkline backdrop, with an optional framing
/// [`Block`].
///
/// Inside the (optional [`Block`]) inner area the rows stack top to bottom:
/// the [`caption`](Self::caption) on row 0, the big [`value`](Self::new) on
/// the next row, the [`trend`](Self::trend) glyph followed by the
/// [`delta`](Self::delta) on the row after, and the
/// [`sparkline`](Self::sparkline) drawn across whatever rows remain at the
/// bottom (scaled to its own largest sample). Any row that does not fit is
/// dropped rather than overflowing.
///
/// Styling is a base [`Style`] (filling the content area) with a
/// [`value_style`](Self::value_style), [`caption_style`](Self::caption_style),
/// [`trend_style`](Self::trend_style), and [`spark_style`](Self::spark_style)
/// layered over it beneath each [`Line`]'s own
/// [`Span`](rstui_core::Span) styles — the same [`Style::patch`](rstui_core::Style)
/// cascade the text widgets use.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{StatPanel, Trend};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 14, 3));
/// StatPanel::new("12.4k")
///     .caption("Requests/s")
///     .delta("+3%")
///     .trend(Trend::Up)
///     .render(buf.area(), &mut buf);
///
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'R'); // caption
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '1'); // value
/// assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, '▲'); // trend
/// ```
#[derive(Debug, Clone)]
pub struct StatPanel<'a> {
    value: Line<'a>,
    caption: Option<Line<'a>>,
    delta: Option<Line<'a>>,
    trend: Trend,
    sparkline: &'a [u64],
    block: Option<Block<'a>>,
    style: Style,
    value_style: Style,
    caption_style: Style,
    trend_style: Style,
    spark_style: Style,
}

impl Default for StatPanel<'_> {
    fn default() -> Self {
        Self {
            value: Line::default(),
            caption: None,
            delta: None,
            trend: Trend::Flat,
            sparkline: &[],
            block: None,
            style: Style::default(),
            value_style: Style::default(),
            caption_style: Style::default(),
            trend_style: Style::default(),
            spark_style: Style::default(),
        }
    }
}

impl<'a> StatPanel<'a> {
    /// A panel whose big headline value is `value` (anything convertible to a
    /// [`Line`], e.g. `"182 ms"`), with no caption, delta, sparkline, or
    /// frame.
    #[must_use]
    pub fn new(value: impl Into<Line<'a>>) -> Self {
        Self {
            value: value.into(),
            ..Self::default()
        }
    }

    /// Sets the small caption [`Line`] drawn on the first row, above the value
    /// (e.g. `"p99 latency"`).
    #[must_use]
    pub fn caption(mut self, caption: impl Into<Line<'a>>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// Sets the change-text [`Line`] drawn after the [`trend`](Self::trend)
    /// glyph on the delta row (e.g. `"+9 ms"`).
    #[must_use]
    pub fn delta(mut self, delta: impl Into<Line<'a>>) -> Self {
        self.delta = Some(delta.into());
        self
    }

    /// Sets which glyph (`▲`/`▼`/`▬`) prefixes the delta row; default
    /// [`Trend::Flat`]. This picks the glyph only — the colour is
    /// [`trend_style`](Self::trend_style).
    #[must_use]
    pub fn trend(mut self, trend: Trend) -> Self {
        self.trend = trend;
        self
    }

    /// Sets the borrowed trend series drawn as a faint backdrop along the
    /// bottom rows, auto-scaled to its largest sample (`sample[0]` leftmost,
    /// the tail clipped at the right edge; empty draws nothing).
    #[must_use]
    pub fn sparkline(mut self, sparkline: &'a [u64]) -> Self {
        self.sparkline = sparkline;
        self
    }

    /// Frames the panel in `block`; everything renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a background
    /// covers the whole tile.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] for the big value row, beneath the value
    /// [`Line`]'s own [`Span`](rstui_core::Span) styles (the caller adds
    /// [`BOLD`](rstui_core::Modifier) here for the headline weight).
    #[must_use]
    pub fn value_style(mut self, style: Style) -> Self {
        self.value_style = style;
        self
    }

    /// Sets the [`Style`] for the caption row, beneath the caption
    /// [`Line`]'s own [`Span`](rstui_core::Span) styles.
    #[must_use]
    pub fn caption_style(mut self, style: Style) -> Self {
        self.caption_style = style;
        self
    }

    /// Sets the [`Style`] for the `glyph delta` row. The caller decides the
    /// semantics (green for a good move, red for a bad one) — this widget
    /// never picks a colour from the [`Trend`].
    #[must_use]
    pub fn trend_style(mut self, style: Style) -> Self {
        self.trend_style = style;
        self
    }

    /// Sets the [`Style`] for the sparkline backdrop glyphs (typically a
    /// faint/[`DIM`](rstui_core::Modifier) colour so it reads as a backdrop).
    #[must_use]
    pub fn spark_style(mut self, style: Style) -> Self {
        self.spark_style = style;
        self
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

impl Widget for StatPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let StatPanel {
            value,
            caption,
            delta,
            trend,
            sparkline,
            block,
            style,
            value_style,
            caption_style,
            trend_style,
            spark_style,
        } = self;

        // The block (if any) frames the content and reserves the inner area —
        // the BarChart render-then-fill-`inner` contract.
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

        // Base fills the content area so a background covers the whole tile;
        // the rows layer the cascade on top.
        buf.set_style(inner, style);

        let left = inner.left();
        let right = inner.right();
        let bottom = inner.bottom();

        // The caption is row 0 (when present); the value is the next row; the
        // `glyph delta` is the row after. Each row is skipped when there is no
        // vertical room for it — a short tile keeps the topmost rows.
        let mut y = inner.top();

        if let Some(caption) = caption {
            if y < bottom {
                paint_line(buf, &caption, left, y, right, style.patch(caption_style));
                y = y.saturating_add(1);
            }
        }

        if y < bottom {
            paint_line(buf, &value, left, y, right, style.patch(value_style));
            y = y.saturating_add(1);
        }

        // The trend row: the glyph, a space, then the delta text, all in
        // trend_style (the caller's colour, not one derived from Trend).
        if y < bottom {
            let row_style = style.patch(trend_style);
            let mut x = left;
            buf.set_cell(Position::new(x, y), trend.glyph(), row_style);
            x = x.saturating_add(1);
            if x < right {
                buf.set_cell(Position::new(x, y), ' ', row_style);
                x = x.saturating_add(1);
            }
            if let Some(delta) = delta {
                paint_line(buf, &delta, x, y, right, row_style);
            }
            y = y.saturating_add(1);
        }

        // The sparkline backdrop fills whatever rows remain at the bottom,
        // drawn on the last of them so a taller gap reads as a baseline strip.
        // It is the inlined Sparkline ramp, never a dependency on that widget.
        if y < bottom && !sparkline.is_empty() {
            let ceiling = sparkline.iter().copied().max().unwrap_or(0);
            if ceiling == 0 {
                return;
            }
            let spark_y = bottom.saturating_sub(1);
            let glyph_style = style.patch(spark_style);
            let mut x = left;
            for &sample in sparkline {
                if x >= right {
                    break;
                }
                // Clamp to the ceiling, round to the nearest eighth; a
                // non-zero sample never rounds away to nothing.
                let clamped = sample.min(ceiling);
                if clamped > 0 {
                    let eighths = ((clamped * 8) + ceiling / 2) / ceiling;
                    let level = eighths.clamp(1, 8) as usize;
                    buf.set_cell(Position::new(x, spark_y), BARS[level - 1], glyph_style);
                }
                x = x.saturating_add(1);
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
    fn the_trend_glyph_is_an_arrow_per_direction() {
        assert_eq!(Trend::Up.glyph(), '▲');
        assert_eq!(Trend::Down.glyph(), '▼');
        assert_eq!(Trend::Flat.glyph(), '▬');
        assert_eq!(Trend::default(), Trend::Flat);
    }

    #[test]
    fn rows_stack_caption_then_value_then_glyph_delta() {
        let panel = StatPanel::new("12.4k")
            .caption("Req/s")
            .delta("+3%")
            .trend(Trend::Up);
        // Row 0 caption, row 1 value, row 2 "▲ +3%".
        assert_eq!(lines(panel, 6, 3), "Req/s \n12.4k \n▲ +3% \n");
    }

    #[test]
    fn the_delta_row_uses_the_chosen_trend_glyph() {
        let panel = StatPanel::new("0.42%").delta("-0.1%").trend(Trend::Down);
        // No caption: value on row 0, "▼ -0.1%" on row 1.
        assert_eq!(lines(panel, 7, 2), "0.42%  \n▼ -0.1%\n");
    }

    #[test]
    fn a_flat_trend_is_the_default_glyph() {
        let panel = StatPanel::new("99.98%").delta("0");
        assert_eq!(lines(panel, 4, 2), "99.9\n▬ 0 \n");
    }

    #[test]
    fn an_absent_caption_promotes_the_value_to_the_top_row() {
        let panel = StatPanel::new("V").delta("d").trend(Trend::Up);
        assert_eq!(lines(panel, 3, 3), "V  \n▲ d\n   \n");
    }

    #[test]
    fn an_absent_delta_still_draws_the_trend_glyph() {
        let panel = StatPanel::new("V").caption("C").trend(Trend::Flat);
        assert_eq!(lines(panel, 3, 3), "C  \nV  \n▬  \n");
    }

    #[test]
    fn a_sparkline_is_a_backdrop_on_the_bottom_rows() {
        let series = [0u64, 4, 8];
        let panel = StatPanel::new("V")
            .caption("C")
            .delta("d")
            .trend(Trend::Up)
            .sparkline(&series);
        // Rows 0..3 are caption/value/trend; the spark draws on the last row.
        assert_eq!(lines(panel, 3, 5), "C  \nV  \n▲ d\n   \n ▄█\n");
    }

    #[test]
    fn an_empty_sparkline_series_draws_no_backdrop() {
        let series: [u64; 0] = [];
        let panel = StatPanel::new("V").sparkline(&series);
        assert_eq!(lines(panel, 3, 3), "V  \n▬  \n   \n");
    }

    #[test]
    fn an_all_zero_sparkline_series_is_a_blank_backdrop() {
        let series = [0u64, 0, 0];
        let panel = StatPanel::new("V").sparkline(&series);
        assert_eq!(lines(panel, 3, 3), "V  \n▬  \n   \n");
    }

    #[test]
    fn a_sparkline_auto_scales_to_its_largest_sample() {
        let series = [1u64, 2, 4];
        let panel = StatPanel::new("V").sparkline(&series);
        // ceiling 4: 1/4≈▂, 2/4=▄, 4/4=█ on the bottom row.
        assert_eq!(lines(panel, 3, 3), "V  \n▬  \n▂▄█\n");
    }

    #[test]
    fn more_sparkline_samples_than_columns_clip_at_the_right_edge() {
        let series = [4u64, 4, 4, 4, 4];
        let panel = StatPanel::new("V").sparkline(&series);
        // Value, trend, then the sparkline backdrop on the bottom row: five
        // full-block samples clipped to the three columns available.
        assert_eq!(lines(panel, 3, 3), "V  \n▬  \n███\n");
    }

    #[test]
    fn a_block_frames_the_panel_in_the_inner_area() {
        let panel = StatPanel::new("V")
            .caption("C")
            .delta("d")
            .trend(Trend::Up)
            .block(Block::bordered());
        // 5x5 bordered → 3x3 inner: caption/value/trend rows.
        assert_eq!(lines(panel, 5, 5), "┌───┐\n│C  │\n│V  │\n│▲ d│\n└───┘\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_only_the_frame() {
        let panel = StatPanel::new("V").caption("C").block(Block::bordered());
        assert_eq!(lines(panel, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn rows_clip_at_the_inner_right_edge() {
        let panel = StatPanel::new("123456").caption("abcdef").delta("ghijkl");
        assert_eq!(lines(panel, 3, 3), "abc\n123\n▬ g\n");
    }

    #[test]
    fn a_one_row_tile_keeps_the_caption_only() {
        let panel = StatPanel::new("V").caption("C").delta("d");
        assert_eq!(lines(panel, 3, 1), "C  \n");
    }

    #[test]
    fn style_cascades_base_then_row_styles_and_fills_the_area() {
        let value = Line::from(vec![Span::styled("V", Style::new().fg(Color::Red))]);
        let panel = StatPanel::new(value)
            .caption("C")
            .delta("d")
            .trend(Trend::Up)
            .style(Style::new().bg(Color::Blue))
            .value_style(Style::new().add_modifier(Modifier::BOLD))
            .caption_style(Style::new().fg(Color::Green))
            .trend_style(Style::new().fg(Color::Yellow));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 3));
        panel.render(buf.area(), &mut buf);

        // Caption: caption_style fg over the base fill.
        let c = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(c.symbol, 'C');
        assert_eq!(c.fg, Color::Green);
        assert_eq!(c.bg, Color::Blue);

        // Value: the span fg wins; value_style BOLD and base bg cascade.
        let v = buf.get(Position::new(0, 1)).unwrap();
        assert_eq!(v.symbol, 'V');
        assert_eq!(v.fg, Color::Red);
        assert!(v.modifier.contains(Modifier::BOLD));
        assert_eq!(v.bg, Color::Blue);

        // Trend glyph: trend_style fg over the base fill.
        let t = buf.get(Position::new(0, 2)).unwrap();
        assert_eq!(t.symbol, '▲');
        assert_eq!(t.fg, Color::Yellow);
        assert_eq!(t.bg, Color::Blue);
    }

    #[test]
    fn the_spark_style_applies_to_the_backdrop_glyphs() {
        let series = [8u64];
        let panel = StatPanel::new("V")
            .sparkline(&series)
            .spark_style(Style::new().fg(Color::Magenta));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 3));
        panel.render(buf.area(), &mut buf);
        let g = buf.get(Position::new(0, 2)).unwrap();
        assert_eq!(g.symbol, '█');
        assert_eq!(g.fg, Color::Magenta);
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let panel = StatPanel::new("V").caption("C");
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 6));
        panel.render(Rect::new(2, 1, 3, 2), &mut buf);
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, 'C');
        assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, 'V');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        StatPanel::new("V")
            .caption("C")
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
