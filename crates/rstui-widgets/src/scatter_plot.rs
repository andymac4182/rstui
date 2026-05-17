//! [`ScatterPlot`] — an X/Y point cloud inside framed axes, the
//! correlation/regression dashboard panel ("latency vs. payload size", "cost
//! vs. usage", a model's residuals).
//!
//! # A pure projection, like every other widget
//!
//! `ScatterPlot` owns no state. Each [`Series`] borrows a caller-owned
//! `&[(f64, f64)]`; the reducer decides *what* the points are (a sliding
//! window it recomputes in `update`) and the widget only projects "the cloud
//! right now" onto a [`Canvas`]. That keeps it deterministically
//! headless-testable and composes with the Elm `view(&self)` model exactly like
//! [`List`](crate::List) and [`Gauge`](crate::Gauge).
//!
//! # Sub-cell precision, reusing the [`Canvas`]
//!
//! A point almost never lands on a whole cell, so — rather than re-implement
//! braille — `ScatterPlot` *composes* [`Canvas`]: it builds one
//! over the inner plot rectangle with the resolved `x_bounds`/`y_bounds` and
//! draws every series through [`Points`] at its
//! [`Marker`] resolution. Each plotted glyph is a single
//! Unicode scalar (the same reasoning [`Block`] borders and the gauge ramp
//! use), so the cloud maps 1:1 onto [`Cell`](rstui_core::Buffer)s with no
//! grapheme machinery. The axes themselves are single box-drawing scalars
//! (`│`, `─`, `└`).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no series, an empty point slice, a single point (a zero-span auto-fit
//! range), a non-finite coordinate, points outside explicit bounds (clipped by
//! the [`Canvas`]), and an area too narrow/short for the gutter
//! or tick rows are all safe clips/no-ops — never a panic. An optional framing
//! [`Block`] follows the container-widget convention; a legend and gridlines
//! are deliberately deferred additive follow-ups, not smuggled into this slice.

use rstui_core::{Buffer, Color, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::canvas::{Canvas, Marker, Points};

/// The width of the reserved left gutter (Y tick labels) and the bottom row
/// (X tick labels) — wide enough for a 2-significant-digit number plus its
/// sign and a decimal point.
const TICK_WIDTH: u16 = 6;

/// One plotted series of a [`ScatterPlot`]: a borrowed caller-owned slice of
/// data-space `(x, y)` points, a [`Color`], and the sub-cell
/// [`Marker`] the cloud is resolved at.
///
/// The slice stays caller-owned (the [`Sparkline`](crate::Sparkline) `&[u64]`
/// discipline one dimension up); the widget only ever reads it.
#[derive(Debug, Clone)]
pub struct Series<'a> {
    /// The data-space coordinates to plot (the caller owns the slice).
    pub points: &'a [(f64, f64)],
    /// The colour every point in this series is painted with.
    pub color: Color,
    /// The sub-cell [`Marker`] resolution the cloud is
    /// resolved at (Braille `2×4`, half-block, dot, or block).
    pub marker: Marker,
}

impl<'a> Series<'a> {
    /// A series plotting `points` in `color` at the default
    /// [`Marker::Braille`] resolution.
    #[must_use]
    pub fn new(points: &'a [(f64, f64)], color: Color) -> Self {
        Self {
            points,
            color,
            marker: Marker::Braille,
        }
    }

    /// Sets the sub-cell [`Marker`] the cloud is
    /// resolved at.
    #[must_use]
    pub fn marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }
}

/// An X/Y point cloud inside framed axes with min/mid/max tick labels and an
/// optional framing [`Block`].
///
/// Each [`Series`] is drawn through a composed [`Canvas`] over
/// the plot rectangle (a left gutter holds the Y tick labels and the Y axis
/// `│`; the bottom row holds the X tick labels under the X axis `─`, meeting at
/// an origin `└`). Bounds are the caller's via
/// [`x_bounds`](Self::x_bounds)/[`y_bounds`](Self::y_bounds), or `None` to
/// auto-fit the union of every series (a single point — or all-equal data —
/// degenerates to a zero-span range the [`Canvas`] pins without
/// a panic). Styling is a base [`Style`] (filling the content area); each
/// series carries its own [`Color`].
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Color, Position, Rect, Widget};
/// use rstui_widgets::canvas::Marker;
/// use rstui_widgets::scatter_plot::{ScatterPlot, Series};
///
/// let pts = [(0.0, 0.0), (1.0, 1.0)];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 10, 5));
/// ScatterPlot::new([Series::new(&pts, Color::Red).marker(Marker::Block)])
///     .render(buf.area(), &mut buf);
///
/// // A 6-wide Y-tick gutter; the Y axis is its last column (x = 5) and the
/// // origin corner `└` is where it meets the X axis row.
/// assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, '│');
/// assert_eq!(buf.get(Position::new(5, 3)).unwrap().symbol, '└');
/// ```
#[derive(Debug, Default, Clone)]
pub struct ScatterPlot<'a> {
    series: Vec<Series<'a>>,
    x_bounds: Option<[f64; 2]>,
    y_bounds: Option<[f64; 2]>,
    block: Option<Block<'a>>,
    style: Style,
}

impl<'a> ScatterPlot<'a> {
    /// A scatter plot of `series`, auto-fitting both axes to the data, with no
    /// frame.
    #[must_use]
    pub fn new<I>(series: I) -> Self
    where
        I: IntoIterator<Item = Series<'a>>,
    {
        Self {
            series: series.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets the inclusive `[min, max]` data-space x-range, or `None` to
    /// auto-fit the union of every series (empty/single-point data is safe —
    /// the [`Canvas`] totality rule).
    #[must_use]
    pub fn x_bounds(mut self, bounds: Option<[f64; 2]>) -> Self {
        self.x_bounds = bounds;
        self
    }

    /// Sets the inclusive `[min, max]` data-space y-range, or `None` to
    /// auto-fit the union of every series.
    #[must_use]
    pub fn y_bounds(mut self, bounds: Option<[f64; 2]>) -> Self {
        self.y_bounds = bounds;
        self
    }

    /// Frames the plot in `block`; the axes render into [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a background
    /// covers the whole pane beneath the axes and cloud.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

/// The auto-fit `[min, max]` of `pick`ed coordinates across every finite point
/// of `series`, or `[0.0, 0.0]` when there is no finite data (a zero-span
/// range the [`Canvas`] pins without a panic).
fn auto_fit(series: &[Series], pick: impl Fn(f64, f64) -> f64) -> [f64; 2] {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for s in series {
        for &(x, y) in s.points {
            if x.is_finite() && y.is_finite() {
                let v = pick(x, y);
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
    }
    if lo.is_finite() && hi.is_finite() {
        [lo, hi]
    } else {
        [0.0, 0.0]
    }
}

/// Formats `value` to two significant digits for a tick label (`12`, `1.2`,
/// `0.012`, `-3.4`), trimming a trailing `.0`.
fn tick_label(value: f64) -> String {
    if !value.is_finite() {
        return "·".to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    let mag = value.abs().log10().floor();
    // Two significant digits → decimals = 1 - floor(log10(|v|)), clamped so a
    // large magnitude prints as a plain integer and a tiny one stays readable.
    let decimals = (1.0 - mag).clamp(0.0, 4.0) as usize;
    let s = format!("{value:.decimals$}");
    if s.contains('.') {
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    } else {
        s
    }
}

/// Stamps `text` left-to-right from `x0` on row `y`, clipped at `right`, in
/// `style`.
fn stamp(buf: &mut Buffer, text: &str, style: Style, x0: u16, y: u16, right: u16) {
    let mut x = x0;
    for ch in text.chars() {
        if x >= right {
            break;
        }
        buf.set_cell(Position::new(x, y), ch, style);
        x = x.saturating_add(1);
    }
}

impl Widget for ScatterPlot<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let ScatterPlot {
            series,
            x_bounds,
            y_bounds,
            block,
            style,
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

        // Resolve the bounds: the caller's, else auto-fit the data union. A
        // zero-span range is left as-is — the Canvas pins it without a panic.
        let xb = x_bounds.unwrap_or_else(|| auto_fit(&series, |x, _| x));
        let yb = y_bounds.unwrap_or_else(|| auto_fit(&series, |_, y| y));

        // Reserve a left gutter for Y tick labels + the Y axis column, and a
        // bottom row for the X axis + X tick labels. Each is dropped first if
        // the area is too small to hold it (totality).
        let gutter = TICK_WIDTH.min(inner.width);
        let has_gutter = inner.width > gutter;
        let gutter_w = if has_gutter { gutter } else { 0 };
        let has_x_row = inner.height > 1;
        let axis_x = inner.left().saturating_add(gutter_w);
        // The Y axis sits in the last gutter column; the X axis on the row just
        // above the X tick row; they meet at the origin corner.
        let axis_col = axis_x.saturating_sub(1);
        let axis_row = inner.bottom().saturating_sub(1 + u16::from(has_x_row));

        // The plot rectangle handed to the composed Canvas: everything right
        // of the gutter and above the axis/tick rows.
        let plot = Rect::new(
            axis_x,
            inner.top(),
            inner.right().saturating_sub(axis_x),
            axis_row.saturating_sub(inner.top()),
        );
        if !plot.is_empty() {
            // Clone the &[(f64,f64)] handles (cheap: a slice ref + Color +
            // Marker each) so the closure can plot them grouped by marker.
            let drawn = series.clone();
            Canvas::default()
                .x_bounds(xb)
                .y_bounds(yb)
                .marker(Marker::Braille)
                .background(style)
                .paint(|ctx| {
                    for s in &drawn {
                        // Each series is its own layer so colours never blend.
                        ctx.draw(&Points {
                            coords: s.points,
                            color: s.color,
                        });
                        ctx.layer();
                    }
                })
                .render(plot, buf);

            // The Canvas above always resolves at Braille; re-plot any
            // non-Braille series through a Canvas of its own marker so a
            // caller's Dot/Block/HalfBlock choice is honoured (later layers
            // overpaint, so the coarser marker wins its cells).
            for s in &series {
                if s.marker != Marker::Braille {
                    let pts = s.points;
                    let color = s.color;
                    Canvas::default()
                        .x_bounds(xb)
                        .y_bounds(yb)
                        .marker(s.marker)
                        .background(Style::default())
                        .paint(|ctx| {
                            ctx.draw(&Points { coords: pts, color });
                        })
                        .render(plot, buf);
                }
            }
        }

        // The Y axis: a column of `│` from the top down to the origin.
        if has_gutter {
            for y in inner.top()..=axis_row {
                buf.set_cell(Position::new(axis_col, y), '│', style);
            }
        }
        // The X axis: a `─` rule along axis_row, right of the Y axis.
        if has_x_row {
            for x in axis_x..inner.right() {
                buf.set_cell(Position::new(x, axis_row), '─', style);
            }
        }
        // The origin corner where the two axes meet.
        if has_gutter && has_x_row {
            buf.set_cell(Position::new(axis_col, axis_row), '└', style);
        }

        // Y tick labels (max at the top, mid, min just above the origin),
        // right-aligned in the gutter so they butt against the axis.
        if has_gutter && gutter_w > 1 {
            let label_right = axis_col;
            let plot_h = axis_row.saturating_sub(inner.top());
            let mut place_y = |frac_from_top: f64, value: f64| {
                let label = tick_label(value);
                let lw = (label.chars().count() as u16).min(label_right - inner.left());
                let x0 = label_right.saturating_sub(lw);
                let span = f64::from(plot_h.saturating_sub(1));
                let y = inner
                    .top()
                    .saturating_add((frac_from_top * span).round() as u16)
                    .min(axis_row.saturating_sub(1).max(inner.top()));
                stamp(buf, &label, style, x0, y, label_right);
            };
            let [y0, y1] = yb;
            let (lo_y, hi_y) = (y0.min(y1), y0.max(y1));
            if plot_h >= 1 {
                place_y(0.0, hi_y);
            }
            if plot_h >= 3 {
                place_y(0.5, (lo_y + hi_y) / 2.0);
                place_y(1.0, lo_y);
            }
        }

        // X tick labels on the bottom row: min at the left of the plot, max
        // right-aligned at the right edge, mid centred.
        if has_x_row {
            let tick_y = inner.bottom().saturating_sub(1);
            let [x0v, x1v] = xb;
            let (lo_x, hi_x) = (x0v.min(x1v), x0v.max(x1v));
            let plot_w = inner.right().saturating_sub(axis_x);

            let min_s = tick_label(lo_x);
            stamp(buf, &min_s, style, axis_x, tick_y, inner.right());

            if plot_w >= 6 {
                let max_s = tick_label(hi_x);
                let mw = (max_s.chars().count() as u16).min(plot_w);
                let mx = inner.right().saturating_sub(mw);
                stamp(buf, &max_s, style, mx, tick_y, inner.right());
            }
            if plot_w >= 12 {
                let mid_s = tick_label((lo_x + hi_x) / 2.0);
                let cw = mid_s.chars().count() as u16;
                let cx = axis_x.saturating_add(plot_w.saturating_sub(cw) / 2);
                stamp(buf, &mid_s, style, cx, tick_y, inner.right());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn axes_frame_a_block_marker_cloud_with_ticks() {
        // A 2-point cloud spanning the corners; Block marker so the glyphs are
        // plain `█`. Gutter is 6 wide, the bottom row holds X ticks.
        let pts = [(0.0, 0.0), (10.0, 10.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Red).marker(Marker::Block)]);
        // 12 wide, 5 tall. The Y axis is the column at x=5 (gutter_w=6 →
        // axis_col=5); the X axis row is y=3 (height 5 → axis_row = 5-1-1=3);
        // the origin `└` is (5,3); X ticks `0`/`10` on row 4.
        let out = lines(plot, 12, 5);
        let rows: Vec<&str> = out.lines().collect();
        // Top-right point in screen space, bottom-left at the origin region.
        assert_eq!(rows[0].chars().nth(11), Some('█'));
        // The Y axis column.
        assert_eq!(rows[0].chars().nth(5), Some('│'));
        assert_eq!(rows[2].chars().nth(5), Some('│'));
        // The origin corner and the X axis rule.
        assert_eq!(rows[3].chars().nth(5), Some('└'));
        assert_eq!(rows[3].chars().nth(6), Some('─'));
        // X tick labels on the bottom row: `0` at the plot left, `10` right.
        assert_eq!(rows[4].chars().nth(6), Some('0'));
        assert_eq!(&rows[4][..6], "      ");
        assert!(rows[4].ends_with("10"));
    }

    #[test]
    fn y_tick_labels_are_right_aligned_in_the_gutter() {
        let pts = [(0.0, 0.0), (1.0, 8.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Green).marker(Marker::Block)])
            .y_bounds(Some([0.0, 8.0]));
        // Tall enough (>=3 plot rows) for max/mid/min. max=8 at the top row,
        // min=0 just above the origin, both butting the axis column (x=5).
        let out = lines(plot, 12, 6);
        let rows: Vec<&str> = out.lines().collect();
        // `8` ends at x=4 (right-aligned against axis_col=5) on the top row.
        assert_eq!(rows[0].chars().nth(4), Some('8'));
        // mid label `4` somewhere in the gutter.
        assert!(rows.iter().any(|r| r[..5].contains('4')));
        // `0` near the origin row (axis_row = 6-1-1 = 4).
        assert!(rows.iter().take(4).any(|r| r[..5].contains('0')));
    }

    #[test]
    fn auto_fit_spans_the_data_union_without_explicit_bounds() {
        // No bounds set: the cloud should still place a point at each extreme
        // corner of the plot (auto-fit from the data).
        let pts = [(-5.0, -2.0), (5.0, 2.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Cyan).marker(Marker::Block)]);
        let out = lines(plot, 12, 5);
        let rows: Vec<&str> = out.lines().collect();
        // Bottom-left of the plot region and top-right corner are painted.
        assert_eq!(rows[0].chars().nth(11), Some('█'));
        assert_eq!(rows[2].chars().nth(6), Some('█'));
        // Auto-fitted X ticks show the data extremes.
        assert!(rows[4].contains("-5"));
        assert!(rows[4].ends_with('5'));
    }

    #[test]
    fn a_single_point_is_a_zero_span_pin_without_a_panic() {
        // One point → auto-fit gives a zero-span range on both axes; the
        // Canvas pins it to the bottom-left of the plot, no divide-by-zero.
        let pts = [(3.0, 3.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Red).marker(Marker::Block)]);
        let out = lines(plot, 12, 5);
        let rows: Vec<&str> = out.lines().collect();
        // Pinned to the plot's bottom-left cell (just right of the Y axis,
        // just above the X axis).
        assert_eq!(rows[2].chars().nth(6), Some('█'));
        // Both ticks read the single value.
        assert!(rows[4].contains('3'));
    }

    #[test]
    fn empty_series_just_draws_the_axes() {
        let plot = ScatterPlot::new(Vec::<Series>::new());
        let out = lines(plot, 12, 5);
        let rows: Vec<&str> = out.lines().collect();
        // The axes and origin still render; no cloud glyphs.
        assert_eq!(rows[3].chars().nth(5), Some('└'));
        assert_eq!(rows[0].chars().nth(5), Some('│'));
        // Zero-span auto-fit → `0` ticks, no panic.
        assert!(rows[4].contains('0'));
    }

    #[test]
    fn an_empty_point_slice_is_safe() {
        let pts: [(f64, f64); 0] = [];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Red)]);
        let out = lines(plot, 12, 5);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows[3].chars().nth(5), Some('└'));
    }

    #[test]
    fn non_finite_coordinates_are_dropped() {
        let pts = [(f64::NAN, 0.0), (0.0, f64::INFINITY), (5.0, 5.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Red).marker(Marker::Block)])
            .x_bounds(Some([0.0, 10.0]))
            .y_bounds(Some([0.0, 10.0]));
        // Only the finite (5,5) survives — no panic from NaN/inf.
        let out = lines(plot, 12, 5);
        assert!(out.contains('█'));
    }

    #[test]
    fn explicit_bounds_clip_out_of_range_points() {
        // (100,100) is outside the bounds and dropped by the Canvas; only
        // (1,1) plots.
        let pts = [(1.0, 1.0), (100.0, 100.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Red).marker(Marker::Block)])
            .x_bounds(Some([0.0, 2.0]))
            .y_bounds(Some([0.0, 2.0]));
        let out = lines(plot, 12, 5);
        // Exactly one cloud cell (the in-range point); the OOB one is clipped.
        assert_eq!(out.matches('█').count(), 1);
    }

    #[test]
    fn two_series_keep_their_own_colours() {
        let a = [(0.0, 0.0)];
        let b = [(10.0, 10.0)];
        let plot = ScatterPlot::new([
            Series::new(&a, Color::Red).marker(Marker::Block),
            Series::new(&b, Color::Blue).marker(Marker::Block),
        ])
        .x_bounds(Some([0.0, 10.0]))
        .y_bounds(Some([0.0, 10.0]));
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 5));
        plot.render(buf.area(), &mut buf);
        // Series A bottom-left of the plot (red), series B top-right (blue).
        let red = buf.get(Position::new(6, 2)).unwrap();
        assert_eq!(red.symbol, '█');
        assert_eq!(red.fg, Color::Red);
        let blue = buf.get(Position::new(11, 0)).unwrap();
        assert_eq!(blue.symbol, '█');
        assert_eq!(blue.fg, Color::Blue);
    }

    #[test]
    fn a_block_frames_the_plot_in_the_inner_area() {
        let pts = [(0.0, 0.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Red).marker(Marker::Block)])
            .block(Block::bordered());
        // 12×6 → inner is 10×4: the border frames it and the axes render only
        // inside (the origin `└` sits at inner col 5, inner row 2).
        let out = lines(plot, 12, 6);
        let rows: Vec<&str> = out.lines().collect();
        // The outer border is intact on every edge.
        assert_eq!(rows[0], "┌──────────┐");
        assert_eq!(rows[5], "└──────────┘");
        assert!(
            rows.iter()
                .all(|r| r.starts_with('│') || r.starts_with('┌') || r.starts_with('└'))
        );
        // The plot's own Y axis is *inside* the frame (col 6 overall) and the
        // origin corner is at inner row 2 (overall row 3).
        assert_eq!(rows[1].chars().nth(6), Some('│'));
        assert_eq!(rows[3].chars().nth(6), Some('└'));
    }

    #[test]
    fn a_tiny_inner_area_drops_the_gutter_but_never_panics() {
        let pts = [(0.0, 0.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Red).marker(Marker::Block)])
            .block(Block::bordered());
        // 4×4 → inner 2×2: too small for the 6-wide gutter (dropped), but the
        // X axis/tick row still fit — a safe clip, never a panic.
        assert_eq!(lines(plot, 4, 4), "┌──┐\n│──│\n│0 │\n└──┘\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_nothing_inside() {
        let pts = [(0.0, 0.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Red)]).block(Block::bordered());
        assert_eq!(lines(plot, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn the_base_style_fills_the_whole_content_area() {
        let pts = [(0.0, 0.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Red).marker(Marker::Block)])
            .style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        plot.render(buf.area(), &mut buf);
        for y in 0..4 {
            for x in 0..8 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Blue);
            }
        }
    }

    #[test]
    fn the_axes_carry_the_base_style() {
        let pts = [(0.0, 0.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Red).marker(Marker::Block)])
            .style(Style::new().fg(Color::Yellow));
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 5));
        plot.render(buf.area(), &mut buf);
        let axis = buf.get(Position::new(5, 0)).unwrap();
        assert_eq!(axis.symbol, '│');
        assert_eq!(axis.fg, Color::Yellow);
    }

    #[test]
    fn a_narrow_area_drops_the_gutter_but_still_plots() {
        // Width 5 <= TICK_WIDTH(6) so no gutter; the cloud + X axis still draw.
        let pts = [(0.0, 0.0), (1.0, 1.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Red).marker(Marker::Block)])
            .x_bounds(Some([0.0, 1.0]))
            .y_bounds(Some([0.0, 1.0]));
        let out = lines(plot, 5, 4);
        let rows: Vec<&str> = out.lines().collect();
        // No `│` gutter column; the X axis rule is still on axis_row (y=2).
        assert!(!out.contains('│'));
        assert!(rows[2].contains('─'));
    }

    #[test]
    fn a_one_row_area_drops_the_x_tick_row() {
        let pts = [(0.0, 0.0)];
        let plot = ScatterPlot::new([Series::new(&pts, Color::Red).marker(Marker::Block)]);
        // Height 1 → no X tick row, no X axis; just the gutter column + cloud.
        let out = lines(plot, 12, 1);
        assert!(!out.contains('─'));
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let pts = [(1.0, 2.0)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        ScatterPlot::new([Series::new(&pts, Color::Red)]).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn tick_label_formats_to_two_significant_digits() {
        assert_eq!(tick_label(0.0), "0");
        assert_eq!(tick_label(12.0), "12");
        assert_eq!(tick_label(1234.0), "1234");
        assert_eq!(tick_label(1.2), "1.2");
        assert_eq!(tick_label(-3.4), "-3.4");
        assert_eq!(tick_label(0.012), "0.012");
        assert_eq!(tick_label(f64::NAN), "·");
    }
}
