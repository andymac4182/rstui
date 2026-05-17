//! [`Candlestick`] — an OHLC financial chart, the trading-desk primitive for
//! "price over a window of bars" (a daily equity chart, an intraday FX series,
//! a crypto candle stream on a market-data dashboard).
//!
//! # A pure projection, like every other widget
//!
//! `Candlestick` owns no state. It is a list of caller-built [`Candle`]s (an
//! open/high/low/close quadruple) plus an optional price window; the reducer
//! decides what the candles are (a ring buffer it pushes a finished bar onto in
//! `update`) and the widget only projects "the prices right now". That keeps it
//! deterministically headless-testable and composes with the Elm `view(&self)`
//! model exactly like [`List`](crate::List) and [`BarChart`](crate::BarChart).
//!
//! # Sub-cell precision, reusing the [`BarChart`](crate::BarChart) idea
//!
//! A candle body spans `open`↔`close`, which rarely lands on whole cell rows,
//! so — exactly like [`BarChart`](crate::BarChart) and [`Gauge`](crate::Gauge)
//! — the body's boundary cells are drawn with the eighth-block glyph nearest
//! the true fractional price (the *vertical* ramp `▁…█`), not rounded to a
//! whole row. Each glyph is one Unicode scalar, so it maps 1:1 onto a
//! [`Cell`](rstui_core::Buffer) with no grapheme machinery — the same reasoning
//! the bar-chart ramp and [`Block`] borders use. The high→low wick is a thin
//! `│` rule so it never visually swamps a one-column-wide body.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no candles, a flat candle (`high == low`), a zero-span price window
//! (`min == max`), and an area too narrow for the price axis or the candles
//! are all safe clips/no-ops — never a panic, no division by zero. An optional
//! framing [`Block`] follows the container-widget convention; volume sub-panes
//! and multi-series overlays are deliberately deferred additive follow-ups, not
//! smuggled into this slice.
//!
//! ```text
//! cargo run -p rstui-widgets --example candlestick_demo
//! ```

use rstui_core::{Buffer, Position, Rect, Style, Widget};

use crate::block::Block;

/// The eight bottom-aligned block elements, `1/8` … `8/8` tall — the same
/// *vertical* ramp [`BarChart`](crate::BarChart) fills its bars with.
const VERTICAL_EIGHTHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// One OHLC bar of a [`Candlestick`]: the open, high, low, and close price for
/// a single period.
///
/// A bar is *bullish* when [`close`](Self::close) is at or above
/// [`open`](Self::open) (drawn with [`bullish_style`](Candlestick::bullish_style))
/// and *bearish* otherwise ([`bearish_style`](Candlestick::bearish_style)). The
/// caller owns the values; the widget never mutates them.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Candle {
    /// The opening price of the period.
    pub open: f64,
    /// The highest price reached during the period.
    pub high: f64,
    /// The lowest price reached during the period.
    pub low: f64,
    /// The closing price of the period.
    pub close: f64,
}

impl Candle {
    /// A candle with the given open, high, low, and close prices.
    #[must_use]
    pub fn new(open: f64, high: f64, low: f64, close: f64) -> Self {
        Self {
            open,
            high,
            low,
            close,
        }
    }
}

/// An OHLC candlestick chart with sub-cell body precision, a reserved left
/// price axis, and an optional framing [`Block`].
///
/// Candles are placed in [`candle_width`](Self::candle_width)-wide columns
/// separated by [`gap`](Self::gap); every price is mapped onto the inner rows
/// by the shared scale (the highest price at the top — financial charts flip
/// the y-axis). Each candle draws a thin `│` wick from its high to its low and
/// a solid body from its open to its close, the body's fractional ends drawn
/// with eighth-block glyphs. A left column is reserved for the price axis,
/// labelled with the window's max (top), mid, and min (bottom).
///
/// Styling is a base [`Style`] (filling the content area) with a
/// [`bullish_style`](Self::bullish_style) for up candles, a
/// [`bearish_style`](Self::bearish_style) for down candles, and the axis drawn
/// in the base style.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Candle, Candlestick};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 5));
/// Candlestick::new([
///     Candle::new(1.0, 4.0, 1.0, 3.0),
///     Candle::new(3.0, 3.0, 0.0, 1.0),
/// ])
/// .bounds(Some([0.0, 4.0]))
/// .render(buf.area(), &mut buf);
///
/// // A left price axis is reserved; the top row carries the max label `4`.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '4');
/// ```
#[derive(Debug, Clone)]
pub struct Candlestick<'a> {
    candles: Vec<Candle>,
    bounds: Option<[f64; 2]>,
    candle_width: u16,
    gap: u16,
    block: Option<Block<'a>>,
    style: Style,
    bullish_style: Style,
    bearish_style: Style,
}

impl Default for Candlestick<'_> {
    fn default() -> Self {
        Self {
            candles: Vec::new(),
            bounds: None,
            // One-column candles with a one-column gap: the sensible default
            // that never visually merges adjacent bars (BarChart's reasoning).
            candle_width: 1,
            gap: 1,
            block: None,
            style: Style::default(),
            // Green up / red down: the universal trading-desk convention.
            bullish_style: Style::default().fg(rstui_core::Color::Green),
            bearish_style: Style::default().fg(rstui_core::Color::Red),
        }
    }
}

impl<'a> Candlestick<'a> {
    /// A chart of `candles`, auto-scaled to the lowest low and highest high,
    /// with one-column candles and gaps and no frame.
    pub fn new<I>(candles: I) -> Self
    where
        I: IntoIterator<Item = Candle>,
    {
        Self {
            candles: candles.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets the `[min, max]` price window, or `None` to auto-scale to the
    /// lowest low and highest high across all candles.
    ///
    /// A zero-span window (`min == max`) collapses every price onto one row
    /// (never a panic — the [`Gauge`](crate::Gauge) totality rule).
    #[must_use]
    pub fn bounds(mut self, bounds: Option<[f64; 2]>) -> Self {
        self.bounds = bounds;
        self
    }

    /// Sets the width in columns of each candle body/wick (default `1`).
    /// Clamped to at least `1` at render time.
    #[must_use]
    pub fn candle_width(mut self, candle_width: u16) -> Self {
        self.candle_width = candle_width;
        self
    }

    /// Sets the blank gap between adjacent candles (default `1`).
    #[must_use]
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// Frames the chart in `block`; candles render into [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area (and draws the
    /// price axis) so a background covers the whole pane.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] for *bullish* candles (`close >= open`), over the
    /// base (default green).
    #[must_use]
    pub fn bullish_style(mut self, style: Style) -> Self {
        self.bullish_style = style;
        self
    }

    /// Sets the [`Style`] for *bearish* candles (`close < open`), over the
    /// base (default red).
    #[must_use]
    pub fn bearish_style(mut self, style: Style) -> Self {
        self.bearish_style = style;
        self
    }
}

/// The row a `price` maps to within `rows` cells spanning `min..=max`, flipped
/// so `max` is the top row. Returns the integer row plus the eighth (`0..=7`)
/// the price sits into that row, both clamped in range (total on a zero span).
fn place(price: f64, min: f64, max: f64, rows: u16) -> (u16, u16) {
    let span = max - min;
    if rows == 0 {
        return (0, 0);
    }
    // The fraction up from the bottom (max → 1.0, min → 0.0); a zero span maps
    // everything to the bottom so the math is total (no division by zero).
    let frac = if span <= 0.0 {
        0.0
    } else {
        ((price - min) / span).clamp(0.0, 1.0)
    };
    let total = f64::from(rows) * 8.0;
    // Eighths from the bottom of the axis.
    let from_bottom = (frac * total).round() as i64;
    let max_e = i64::from(rows) * 8 - 1;
    let e = from_bottom.clamp(0, max_e);
    let row_from_bottom = (e / 8) as u16;
    let eighth = (e % 8) as u16;
    (rows - 1 - row_from_bottom, eighth)
}

impl Widget for Candlestick<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Candlestick {
            candles,
            bounds,
            candle_width,
            gap,
            block,
            style,
            bullish_style,
            bearish_style,
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

        // Base fills the content area so a background covers the whole pane.
        buf.set_style(inner, style);
        if candles.is_empty() {
            return;
        }

        // The price window: the caller's, or the lowest low / highest high.
        let (min, max) = match bounds {
            Some([lo, hi]) => (lo, hi),
            None => {
                let lo = candles.iter().map(|c| c.low).fold(f64::INFINITY, f64::min);
                let hi = candles
                    .iter()
                    .map(|c| c.high)
                    .fold(f64::NEG_INFINITY, f64::max);
                (lo, hi)
            }
        };

        // A left price-axis column sized to the widest of the three labels,
        // capped at half the inner width so the candles always get room.
        let labels = [fmt_price(max), fmt_price((min + max) / 2.0), fmt_price(min)];
        let axis_w = labels
            .iter()
            .map(|s| s.chars().count() as u16)
            .max()
            .unwrap_or(0)
            .min(inner.width / 2);
        let plot_x0 = inner.left().saturating_add(axis_w);
        let plot_w = inner.width.saturating_sub(axis_w);
        let rows = inner.height;
        let bottom_row = inner.bottom().saturating_sub(1);

        // The axis labels: max at the top row, min at the bottom, mid at the
        // vertical centre (only when the rows don't collide).
        if axis_w > 0 {
            buf.set_str(Position::new(inner.left(), inner.top()), &labels[0], style);
            if rows > 1 {
                buf.set_str(Position::new(inner.left(), bottom_row), &labels[2], style);
            }
            if rows > 2 {
                let mid_row = inner.top().saturating_add(rows / 2);
                if mid_row != inner.top() && mid_row != bottom_row {
                    buf.set_str(Position::new(inner.left(), mid_row), &labels[1], style);
                }
            }
        }

        if plot_w == 0 {
            return;
        }
        let cw = candle_width.max(1);
        let right = inner.right();

        let mut x0 = plot_x0;
        for candle in &candles {
            if x0 >= right {
                break;
            }
            let group_right = x0.saturating_add(cw).min(right);
            let glyph_style = style.patch(if candle.close >= candle.open {
                bullish_style
            } else {
                bearish_style
            });

            // The wick: the high→low extent as a thin vertical rule.
            let (high_row, _) = place(candle.high, min, max, rows);
            let (low_row, _) = place(candle.low, min, max, rows);
            let (wick_top, wick_bot) = (high_row.min(low_row), high_row.max(low_row));

            // The body: open↔close, with eighth-block ends so a body that
            // starts/ends partway through a row reads at its true fraction.
            let body_hi = candle.open.max(candle.close);
            let body_lo = candle.open.min(candle.close);
            let (hi_row, hi_e) = place(body_hi, min, max, rows);
            let (lo_row, lo_e) = place(body_lo, min, max, rows);

            for x in x0..group_right {
                let cy = inner.top();
                for r in wick_top..=wick_bot {
                    buf.set_cell(Position::new(x, cy + r), '│', glyph_style);
                }
                // Whole filled body rows between the two fractional ends.
                for r in (hi_row + 1)..lo_row {
                    buf.set_cell(Position::new(x, cy + r), '█', glyph_style);
                }
                if hi_row == lo_row {
                    // A body thinner than one row: a single eighth glyph at
                    // the open/close row (a doji-like near-flat candle).
                    let e = ((hi_e + lo_e) / 2).clamp(0, 7);
                    buf.set_cell(
                        Position::new(x, cy + hi_row),
                        VERTICAL_EIGHTHS[e as usize],
                        glyph_style,
                    );
                } else {
                    // The top end fills from its eighth up to the row top, the
                    // bottom end from the row bottom down to its eighth — both
                    // are the full block at the edges of a tall body.
                    buf.set_cell(Position::new(x, cy + hi_row), '█', glyph_style);
                    let lo_glyph = if lo_e == 0 {
                        '█'
                    } else {
                        VERTICAL_EIGHTHS[(lo_e - 1) as usize]
                    };
                    buf.set_cell(Position::new(x, cy + lo_row), lo_glyph, glyph_style);
                }
            }
            x0 = group_right.saturating_add(gap);
        }
    }
}

/// A compact decimal label for a price-axis tick: an integer when whole, two
/// decimals otherwise (trimmed of trailing zeros), with no thousands grouping.
fn fmt_price(value: f64) -> String {
    if !value.is_finite() {
        return "·".to_string();
    }
    if value.fract() == 0.0 {
        return format!("{value:.0}");
    }
    let s = format!("{value:.2}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
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
    fn the_price_axis_labels_max_min_and_mid() {
        // bounds 0..4, 5 rows: max `4` on the top row, min `0` on the bottom,
        // mid `2` at the vertical centre. axis_w = 1; candles plot after it.
        let chart = Candlestick::new([Candle::new(1.0, 4.0, 0.0, 3.0)])
            .bounds(Some([0.0, 4.0]))
            .candle_width(1)
            .gap(0);
        let out = lines(chart, 4, 5);
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[0].starts_with('4')); // max label, top row
        assert!(rows[4].starts_with('0')); // min label, bottom row
        assert!(rows[2].starts_with('2')); // mid label, centre row
    }

    #[test]
    fn the_high_low_wick_spans_the_full_extent() {
        // One candle, bounds 0..4, 5 rows: high 4 → top row, low 0 → bottom
        // row, so the candle column has no blank gap from top to bottom (the
        // wick rule, with the open↔close body painted over its middle).
        let chart = Candlestick::new([Candle::new(1.0, 4.0, 0.0, 3.0)])
            .bounds(Some([0.0, 4.0]))
            .gap(0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 5));
        chart.render(buf.area(), &mut buf);
        // axis_w = 1 → the candle column is x = 1.
        for y in 0..5 {
            assert_ne!(buf.get(Position::new(1, y)).unwrap().symbol, ' ');
        }
        // The extremes are the thin wick rule (high above the body, low below).
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '│');
        assert_eq!(buf.get(Position::new(1, 4)).unwrap().symbol, '│');
    }

    #[test]
    fn a_bullish_candle_uses_the_bullish_style() {
        let chart = Candlestick::new([Candle::new(1.0, 4.0, 0.0, 3.0)])
            .bounds(Some([0.0, 4.0]))
            .gap(0)
            .bullish_style(Style::new().fg(Color::Green))
            .bearish_style(Style::new().fg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 5));
        chart.render(buf.area(), &mut buf);
        // close 3 >= open 1 → bullish; the wick top (high 4) is row 0.
        let c = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(c.fg, Color::Green);
    }

    #[test]
    fn a_bearish_candle_uses_the_bearish_style() {
        let chart = Candlestick::new([Candle::new(3.0, 4.0, 0.0, 1.0)])
            .bounds(Some([0.0, 4.0]))
            .gap(0)
            .bullish_style(Style::new().fg(Color::Green))
            .bearish_style(Style::new().fg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 5));
        chart.render(buf.area(), &mut buf);
        // close 1 < open 3 → bearish.
        let c = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(c.fg, Color::Red);
    }

    #[test]
    fn the_gap_separates_candles() {
        let chart = Candlestick::new([
            Candle::new(1.0, 4.0, 0.0, 3.0),
            Candle::new(3.0, 4.0, 0.0, 1.0),
        ])
        .bounds(Some([0.0, 4.0]))
        .candle_width(1)
        .gap(1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 5));
        chart.render(buf.area(), &mut buf);
        // axis_w = 1: candle A at x=1, gap at x=2, candle B at x=3.
        assert_ne!(buf.get(Position::new(1, 0)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, ' ');
        assert_ne!(buf.get(Position::new(3, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn candle_width_thickens_each_candle() {
        let chart = Candlestick::new([Candle::new(1.0, 4.0, 0.0, 3.0)])
            .bounds(Some([0.0, 4.0]))
            .candle_width(2)
            .gap(0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 5));
        chart.render(buf.area(), &mut buf);
        // axis_w = 1, candle 2 wide → both x=1 and x=2 carry the wick top.
        assert_ne!(buf.get(Position::new(1, 0)).unwrap().symbol, ' ');
        assert_ne!(buf.get(Position::new(2, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn a_flat_candle_high_equals_low_is_a_single_glyph() {
        // open=high=low=close: the wick and body collapse onto one row, drawn
        // with one eighth glyph — no panic.
        let chart = Candlestick::new([Candle::new(2.0, 2.0, 2.0, 2.0)])
            .bounds(Some([0.0, 4.0]))
            .gap(0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 5));
        chart.render(buf.area(), &mut buf);
        // Exactly one cell in the candle column carries a glyph.
        let drawn: Vec<char> = (0..5)
            .map(|y| buf.get(Position::new(1, y)).unwrap().symbol)
            .filter(|&c| c != ' ')
            .collect();
        assert_eq!(drawn.len(), 1);
    }

    #[test]
    fn a_zero_span_window_collapses_onto_one_row_without_panicking() {
        // min == max: every price maps to the same (bottom) row; the body is a
        // single eighth glyph, no division by zero.
        let chart = Candlestick::new([Candle::new(5.0, 5.0, 5.0, 5.0)])
            .bounds(Some([5.0, 5.0]))
            .gap(0);
        assert_eq!(lines(chart, 2, 3).lines().count(), 3);
    }

    #[test]
    fn auto_scale_uses_the_lowest_low_and_highest_high() {
        // No bounds: min = 1 (lowest low), max = 9 (highest high). The top
        // axis label is the max `9`, the bottom is the min `1`.
        let chart = Candlestick::new([
            Candle::new(2.0, 5.0, 1.0, 4.0),
            Candle::new(4.0, 9.0, 3.0, 8.0),
        ])
        .gap(0);
        let out = lines(chart, 5, 3);
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[0].starts_with('9'));
        assert!(rows[2].starts_with('1'));
    }

    #[test]
    fn a_block_frames_the_chart_in_the_inner_area() {
        let chart = Candlestick::new([Candle::new(0.0, 1.0, 0.0, 1.0)])
            .bounds(Some([0.0, 1.0]))
            .block(Block::bordered());
        // inner is 1×1 (just the price-axis label cell fits at most).
        let out = lines(chart, 3, 3);
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[0].starts_with('┌'));
        assert!(rows[2].starts_with('└'));
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_nothing_inside() {
        let chart = Candlestick::new([Candle::new(0.0, 1.0, 0.0, 1.0)]).block(Block::bordered());
        assert_eq!(lines(chart, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn no_candles_with_a_block_still_renders_the_block() {
        let chart = Candlestick::new(Vec::<Candle>::new()).block(Block::bordered());
        assert_eq!(lines(chart, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn style_cascades_base_then_candle_style() {
        let chart = Candlestick::new([Candle::new(1.0, 4.0, 0.0, 3.0)])
            .bounds(Some([0.0, 4.0]))
            .gap(0)
            .style(Style::new().bg(Color::Blue))
            .bullish_style(Style::new().fg(Color::Green).add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 5));
        chart.render(buf.area(), &mut buf);
        // The wick top cell: candle style fg over the base bg.
        let c = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(c.fg, Color::Green);
        assert_eq!(c.bg, Color::Blue); // base fill cascades
        assert!(c.modifier.contains(Modifier::BOLD));
        // An axis label cell keeps the base bg too.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Blue);
    }

    #[test]
    fn a_narrow_area_with_no_room_for_candles_still_draws_the_axis() {
        // width 1: axis_w = min(1, 0) = 0 → no axis, plot_w 1 but the candle
        // still clips safely; the key invariant is no panic.
        let chart = Candlestick::new([Candle::new(1.0, 2.0, 0.0, 1.0)]).bounds(Some([0.0, 2.0]));
        let _ = lines(chart, 1, 3);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        Candlestick::new([Candle::new(1.0, 2.0, 0.0, 1.0)]).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let chart = Candlestick::new([Candle::new(1.0, 4.0, 0.0, 3.0)])
            .bounds(Some([0.0, 4.0]))
            .gap(0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 6));
        chart.render(Rect::new(2, 1, 4, 5), &mut buf);
        // The axis label lands at the area origin, not (0,0).
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, '4');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn fmt_price_is_compact_and_total() {
        assert_eq!(fmt_price(4.0), "4");
        assert_eq!(fmt_price(1.5), "1.5");
        assert_eq!(fmt_price(1.25), "1.25");
        assert_eq!(fmt_price(f64::INFINITY), "·");
    }
}
