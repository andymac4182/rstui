//! [`LineChart`] — multi-series XY lines inside framed axes, the core
//! observability "metric over time" panel (request rate, p99 latency, CPU),
//! the continuous-curve sibling of the categorical `BarChart`.
//!
//! # A pure projection, like every other widget
//!
//! `LineChart` owns no state. It is a borrowed slice of caller-built
//! [`Series`] (a name [`Line`], a `&[(f64, f64)]` of points, a [`Style`], a
//! marker glyph) plus optional axis bounds; the reducer decides what the
//! series are (a ring buffer it pushes samples onto in `update`) and the
//! widget only projects "the curves right now". That keeps it
//! deterministically headless-testable and composes with the Elm
//! `view(&self)` model exactly like `List` and `Gauge`.
//!
//! # One marker per column, not braille
//!
//! Unlike a braille canvas this stays a *cell* projection: a left Y gutter
//! and a bottom X axis row are reserved, and for every remaining plot column
//! each series places exactly one `marker` glyph at the row its
//! linearly-interpolated value falls on. Each marker is one Unicode scalar,
//! so it maps 1:1 onto a [`Cell`](rstui_core::Buffer) with no grapheme
//! machinery — the same reasoning `Block` borders and the `Gauge` ramp use.
//! The result is a readable poly-line per series without sub-cell
//! line-drawing complexity.
//!
//! # A framed container, with axes and an optional legend
//!
//! Like `BarChart` and the other container widgets — and unlike the leaf
//! `Sparkline` — `LineChart` takes an optional framing `Block` and draws its
//! own decoration: a `│` Y axis, a `─` X axis, an `└` origin corner, the
//! y-max/y-min and x-min/x-max labels, and (by default) a `marker name`
//! legend on the top-right rows.
//!
//! # Total, never a panic
//!
//! Per the `Gauge` rule a pure projection is *total*: an empty area, no
//! series, a series with zero or one points, `NaN`/infinite points
//! (skipped), degenerate bounds (`min == max`, padded so the scale never
//! divides by zero), a value outside the bounds (clamped into the plot) and
//! an area too small for the gutter/axis are all safe clips/no-ops — never a
//! panic.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Position, Rect, Widget};
//! use rstui_widgets::{LineChart, Series};
//!
//! let points = [(0.0, 0.0), (3.0, 3.0)];
//! let series = [Series::new("s", &points).marker('*')];
//! let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
//! LineChart::new(&series).legend(false).render(buf.area(), &mut buf);
//!
//! // 1-wide Y gutter, bottom row is the X axis; the origin corner is `└`
//! // and the rising line plots `*` markers in the plot body.
//! assert_eq!(buf.get(Position::new(1, 3)).unwrap().symbol, '└');
//! assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, '*');
//! ```

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// An inclusive numeric range `[min, max]` for one axis of a [`LineChart`].
///
/// Used for both the x and y axis through the `x_bounds`/`y_bounds` builders;
/// when unset the chart auto-derives the range from the union of every
/// [`Series`]' points. A degenerate range (`min == max`) is padded by `1.0`
/// at render time so the scale never divides by zero.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct AxisBounds {
    /// The low end of the range, mapped to the axis origin.
    pub min: f64,
    /// The high end of the range, mapped to the far edge of the axis.
    pub max: f64,
}

impl AxisBounds {
    /// Bounds spanning `[min, max]` (the caller need not pre-order them — the
    /// chart normalises and pads a degenerate range when scaling).
    #[must_use]
    pub fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }
}

/// One line of a [`LineChart`]: a name [`Line`], a borrowed slice of
/// `(x, y)` points, a [`Style`], and a marker glyph.
///
/// Build the name from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`); the points are plain
/// caller-owned state (a ring buffer the reducer pushes onto) and assumed
/// sorted by `x` — an unsorted slice still renders totally (the column lookup
/// just falls back to the nearest point). `NaN`/infinite points are skipped.
#[derive(Debug, Clone)]
pub struct Series<'a> {
    /// The series' name, drawn in the legend after its marker.
    pub name: Line<'a>,
    /// The `(x, y)` samples, assumed sorted by `x`.
    pub points: &'a [(f64, f64)],
    /// The [`Style`] the markers (and legend entry) are drawn with.
    pub style: Style,
    /// The single glyph stamped at each plotted column.
    pub marker: char,
}

impl<'a> Series<'a> {
    /// A series named `name` (anything convertible to a [`Line`]) over
    /// `points`, with the default `•` marker and an unset [`Style`].
    #[must_use]
    pub fn new(name: impl Into<Line<'a>>, points: &'a [(f64, f64)]) -> Self {
        Self {
            name: name.into(),
            points,
            style: Style::default(),
            marker: '•',
        }
    }

    /// Sets the [`Style`] the markers and legend entry are drawn with, over
    /// the chart's base and axis styles.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the single glyph stamped at each plotted column (default `•`).
    #[must_use]
    pub fn marker(mut self, marker: char) -> Self {
        self.marker = marker;
        self
    }
}

/// A multi-series XY line chart with framed axes, an optional legend and an
/// optional framing `Block`.
///
/// A left Y gutter (as wide as the formatted y-min/y-max, at least `1`) and a
/// bottom X axis row are reserved; the remaining body is the plot. For every
/// plot column each [`Series`] is sampled by linear interpolation between its
/// two bracketing points and one `marker` is stamped at the scaled row.
/// Bounds come from the `x_bounds`/`y_bounds` builders, or are auto-derived
/// from the union of all series when unset. Styling is a base [`Style`]
/// (filling the area) with an `axis_style` for the axes/labels and each
/// series' own [`Style`] for its markers.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{AxisBounds, LineChart, Series};
///
/// let points = [(0.0, 0.0), (1.0, 1.0)];
/// let series = [Series::new("s", &points)];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 5));
/// LineChart::new(&series)
///     .x_bounds(AxisBounds::new(0.0, 1.0))
///     .y_bounds(AxisBounds::new(0.0, 1.0))
///     .legend(false)
///     .render(buf.area(), &mut buf);
///
/// // The origin corner sits at the gutter/axis crossing.
/// assert_eq!(buf.get(Position::new(1, 4)).unwrap().symbol, '└');
/// ```
#[derive(Debug, Clone)]
pub struct LineChart<'a> {
    series: &'a [Series<'a>],
    x_bounds: Option<AxisBounds>,
    y_bounds: Option<AxisBounds>,
    block: Option<Block<'a>>,
    style: Style,
    axis_style: Style,
    legend: bool,
}

impl Default for LineChart<'_> {
    fn default() -> Self {
        Self {
            series: &[],
            x_bounds: None,
            y_bounds: None,
            block: None,
            style: Style::default(),
            axis_style: Style::default(),
            // A legend on by default: a multi-series chart is unreadable
            // without one and a single series still benefits (BarChart's
            // "sensible default" reasoning).
            legend: true,
        }
    }
}

impl<'a> LineChart<'a> {
    /// A chart of `series`, axes auto-scaled to the union of all points, with
    /// a legend and no frame.
    #[must_use]
    pub fn new(series: &'a [Series<'a>]) -> Self {
        Self {
            series,
            ..Self::default()
        }
    }

    /// Sets the x-axis range, or leaves it auto-derived from the data union
    /// when unset. A point outside the range is clamped into the plot.
    #[must_use]
    pub fn x_bounds(mut self, bounds: AxisBounds) -> Self {
        self.x_bounds = Some(bounds);
        self
    }

    /// Sets the y-axis range, or leaves it auto-derived from the data union
    /// when unset. A point outside the range is clamped into the plot.
    #[must_use]
    pub fn y_bounds(mut self, bounds: AxisBounds) -> Self {
        self.y_bounds = Some(bounds);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a
    /// background covers the whole pane.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] the axis lines, origin corner and bound labels are
    /// drawn with, over the base.
    #[must_use]
    pub fn axis_style(mut self, style: Style) -> Self {
        self.axis_style = style;
        self
    }

    /// Frames the chart in `block`; the axes and plot render into the
    /// block's inner area.
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets whether the per-series `marker name` legend is drawn on the
    /// top-right rows (default `true`).
    #[must_use]
    pub fn legend(mut self, legend: bool) -> Self {
        self.legend = legend;
        self
    }
}

/// `true` when `v` is a real, finite number safe to scale.
fn is_finite(v: f64) -> bool {
    v.is_finite()
}

/// The `[min, max]` span of every finite point coordinate selected by `pick`
/// across `series`, or `None` when there is no finite point.
fn data_span(series: &[Series], pick: impl Fn(&(f64, f64)) -> f64) -> Option<(f64, f64)> {
    let mut span: Option<(f64, f64)> = None;
    for s in series {
        for p in s.points {
            let v = pick(p);
            if !is_finite(v) {
                continue;
            }
            span = Some(match span {
                Some((lo, hi)) => (lo.min(v), hi.max(v)),
                None => (v, v),
            });
        }
    }
    span
}

/// Normalises `bounds` into an ordered, non-degenerate `(min, max)`: an
/// out-of-order pair is swapped and a zero-width range is padded by `1.0` so
/// the scale never divides by zero.
fn resolve_bounds(bounds: Option<AxisBounds>, span: Option<(f64, f64)>) -> (f64, f64) {
    let (mut lo, mut hi) = match bounds {
        Some(b) => (b.min, b.max),
        None => span.unwrap_or((0.0, 1.0)),
    };
    if !is_finite(lo) || !is_finite(hi) {
        lo = 0.0;
        hi = 1.0;
    }
    if lo > hi {
        core::mem::swap(&mut lo, &mut hi);
    }
    if (hi - lo).abs() < f64::EPSILON {
        lo -= 1.0;
        hi += 1.0;
    }
    (lo, hi)
}

/// Maps `value` from `[lo, hi]` onto `0..cells`, clamped to the last cell.
fn cell_of(value: f64, lo: f64, hi: f64, cells: u16) -> u16 {
    if cells == 0 {
        return 0;
    }
    let t = ((value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let max_idx = u32::from(cells - 1);
    let idx = (t * f64::from(max_idx)).round() as i64;
    idx.clamp(0, i64::from(max_idx)) as u16
}

/// The y for `x` on `points` (assumed sorted by `x`): linear interpolation
/// between the two bracketing finite points, or the nearest finite point's
/// value when `x` is outside the data or the slice is unsorted. `None` when
/// no point is finite.
fn sample_at(points: &[(f64, f64)], x: f64) -> Option<f64> {
    let mut prev: Option<(f64, f64)> = None;
    let mut nearest: Option<(f64, f64)> = None;
    for &(px, py) in points {
        if !is_finite(px) || !is_finite(py) {
            continue;
        }
        nearest = Some(match nearest {
            Some((nx, ny)) if (nx - x).abs() <= (px - x).abs() => (nx, ny),
            _ => (px, py),
        });
        if let Some((qx, qy)) = prev {
            if qx <= x && x <= px {
                let span = px - qx;
                if span.abs() < f64::EPSILON {
                    return Some(py);
                }
                let t = (x - qx) / span;
                return Some(qy + t * (py - qy));
            }
        }
        prev = Some((px, py));
    }
    nearest.map(|(_, ny)| ny)
}

/// Formats an axis bound compactly: no decimals for whole numbers, otherwise
/// one decimal place.
fn fmt_bound(v: f64) -> String {
    if !is_finite(v) {
        return "0".to_string();
    }
    let rounded = v.round();
    if (v - rounded).abs() < f64::EPSILON {
        format!("{rounded:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// Stamps `line` left-to-right from `x0` on row `y`, clipped at `right`, with
/// `base` beneath the line→span cascade.
fn stamp_line(buf: &mut Buffer, line: &Line, base: Style, x0: u16, y: u16, right: u16) {
    let line_base = base.patch(line.style);
    let mut x = x0;
    'line: for span in &line.spans {
        let style = line_base.patch(span.style);
        for ch in span.content.chars() {
            if x >= right {
                break 'line;
            }
            buf.set_cell(Position::new(x, y), ch, style);
            x = x.saturating_add(1);
        }
    }
}

impl Widget for LineChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let LineChart {
            series,
            x_bounds,
            y_bounds,
            block,
            style,
            axis_style,
            legend,
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

        // Resolved, ordered, non-degenerate axis ranges.
        let (x_lo, x_hi) = resolve_bounds(x_bounds, data_span(series, |p| p.0));
        let (y_lo, y_hi) = resolve_bounds(y_bounds, data_span(series, |p| p.1));

        // The gutter is the widest bound label (at least 1) plus one column
        // for the Y axis line itself, never wider than the area; the bottom
        // row is the X axis.
        let label_w = fmt_bound(y_hi)
            .chars()
            .count()
            .max(fmt_bound(y_lo).chars().count())
            .max(1) as u16;
        let gutter = label_w.saturating_add(1).min(inner.width);
        let axis_glyph = style.patch(axis_style);

        // Plot body: right of the gutter, above the X axis row.
        let plot_x0 = inner.left().saturating_add(gutter);
        let plot_w = inner.width.saturating_sub(gutter);
        let axis_y = inner.bottom().saturating_sub(1);
        let plot_h = inner.height.saturating_sub(1);

        // Y axis column (the gutter's last cell) and the X axis row.
        if gutter > 0 {
            let axis_x = plot_x0.saturating_sub(1);
            for y in inner.top()..axis_y {
                buf.set_cell(Position::new(axis_x, y), '│', axis_glyph);
            }
            buf.set_cell(Position::new(axis_x, axis_y), '└', axis_glyph);

            // y-max at the gutter top, y-min just above the X axis, both in
            // the label columns left of the axis line.
            stamp_line(
                buf,
                &Line::raw(fmt_bound(y_hi)),
                axis_glyph,
                inner.left(),
                inner.top(),
                axis_x,
            );
            if plot_h > 1 {
                stamp_line(
                    buf,
                    &Line::raw(fmt_bound(y_lo)),
                    axis_glyph,
                    inner.left(),
                    axis_y.saturating_sub(1),
                    axis_x,
                );
            }
        }

        // X axis row right of the origin corner, with x-min/x-max labels.
        for x in plot_x0..inner.right() {
            buf.set_cell(Position::new(x, axis_y), '─', axis_glyph);
        }
        if plot_w > 0 {
            stamp_line(
                buf,
                &Line::raw(fmt_bound(x_lo)),
                axis_glyph,
                plot_x0,
                axis_y,
                inner.right(),
            );
            let hi = fmt_bound(x_hi);
            let hw = (hi.chars().count() as u16).min(plot_w);
            let hx = inner.right().saturating_sub(hw);
            stamp_line(
                buf,
                &Line::raw(hi),
                axis_glyph,
                hx.max(plot_x0),
                axis_y,
                inner.right(),
            );
        }

        if plot_w == 0 || plot_h == 0 {
            return;
        }

        // One marker per plot column per series, by linear interpolation.
        for s in series {
            let glyph_style = style.patch(s.style);
            match s.points.len() {
                0 => {}
                1 => {
                    let (px, py) = s.points[0];
                    if is_finite(px) && is_finite(py) {
                        let cx = plot_x0.saturating_add(cell_of(px, x_lo, x_hi, plot_w));
                        let row = plot_h - 1 - cell_of(py, y_lo, y_hi, plot_h);
                        let cy = inner.top().saturating_add(row);
                        buf.set_cell(Position::new(cx, cy), s.marker, glyph_style);
                    }
                }
                _ => {
                    for col in 0..plot_w {
                        // The data x at this column's centre.
                        let t = if plot_w == 1 {
                            0.0
                        } else {
                            f64::from(col) / f64::from(plot_w - 1)
                        };
                        let x = x_lo + t * (x_hi - x_lo);
                        if let Some(y) = sample_at(s.points, x) {
                            if !is_finite(y) {
                                continue;
                            }
                            let cx = plot_x0.saturating_add(col);
                            let row = plot_h - 1 - cell_of(y, y_lo, y_hi, plot_h);
                            let cy = inner.top().saturating_add(row);
                            buf.set_cell(Position::new(cx, cy), s.marker, glyph_style);
                        }
                    }
                }
            }
        }

        // Legend: `marker name` per series on the top-right rows, never over
        // the gutter or the axis row.
        if legend && !series.is_empty() {
            let legend_w = series.iter().map(|s| s.name.width() + 2).max().unwrap_or(0) as u16;
            let legend_w = legend_w.min(plot_w);
            if legend_w > 0 {
                let lx = inner.right().saturating_sub(legend_w);
                let mut ly = inner.top();
                for s in series {
                    if ly >= axis_y {
                        break;
                    }
                    let glyph_style = style.patch(s.style);
                    buf.set_cell(Position::new(lx, ly), s.marker, glyph_style);
                    stamp_line(
                        buf,
                        &s.name,
                        axis_glyph,
                        lx.saturating_add(2),
                        ly,
                        inner.right(),
                    );
                    ly = ly.saturating_add(1);
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
    fn the_gutter_axis_and_origin_corner_frame_the_plot() {
        let points = [(0.0, 0.0), (1.0, 1.0)];
        let series = [Series::new("s", &points)];
        let chart = LineChart::new(&series)
            .x_bounds(AxisBounds::new(0.0, 1.0))
            .y_bounds(AxisBounds::new(0.0, 1.0))
            .legend(false);
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
        chart.render(buf.area(), &mut buf);
        // Gutter = 1 label column + 1 axis column. y-max "1" tops the label
        // column, y-min "0" sits one row above the X axis. The Y axis `│`
        // runs down column 1, the origin `└` is the gutter/axis crossing,
        // and the X axis `─` runs right of it.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '1');
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, '0');
        assert_eq!(buf.get(Position::new(1, 3)).unwrap().symbol, '└');
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, '│');
        assert_eq!(buf.get(Position::new(3, 3)).unwrap().symbol, '─');
    }

    #[test]
    fn a_rising_line_climbs_left_to_right() {
        let points = [(0.0, 0.0), (3.0, 3.0)];
        let series = [Series::new("s", &points).marker('*')];
        let chart = LineChart::new(&series)
            .x_bounds(AxisBounds::new(0.0, 3.0))
            .y_bounds(AxisBounds::new(0.0, 3.0))
            .legend(false);
        // Gutter is 2 wide so the plot is 4 wide × 3 tall; y rises with x so
        // the marker row climbs from the baseline to the top.
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
        chart.render(buf.area(), &mut buf);
        // First plot column (x≈0,y≈0) sits on the bottom plot row.
        assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, '*');
        // The rightmost plot column (x≈3,y≈3) sits on the top plot row.
        assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, '*');
    }

    #[test]
    fn the_default_marker_is_a_bullet() {
        let points = [(0.0, 0.0), (1.0, 0.0)];
        let series = [Series::new("s", &points)];
        let out = lines(
            LineChart::new(&series)
                .x_bounds(AxisBounds::new(0.0, 1.0))
                .y_bounds(AxisBounds::new(0.0, 1.0))
                .legend(false),
            6,
            4,
        );
        assert!(out.contains('•'));
    }

    #[test]
    fn linear_interpolation_fills_columns_between_points() {
        // Two points only; every plot column still gets a marker via
        // interpolation, so the line is continuous (not just two dots).
        let points = [(0.0, 0.0), (4.0, 4.0)];
        let series = [Series::new("s", &points).marker('#')];
        let mut buf = Buffer::empty(Rect::new(0, 0, 7, 5));
        LineChart::new(&series)
            .x_bounds(AxisBounds::new(0.0, 4.0))
            .y_bounds(AxisBounds::new(0.0, 4.0))
            .legend(false)
            .render(buf.area(), &mut buf);
        // Plot is x:2..7 (5 cols). Every column has exactly one '#'.
        for x in 2..7 {
            let col_has = (0..4).any(|y| buf.get(Position::new(x, y)).unwrap().symbol == '#');
            assert!(col_has, "column {x} has no marker");
        }
    }

    #[test]
    fn auto_bounds_span_the_union_of_all_series() {
        let a = [(0.0, 0.0), (10.0, 0.0)];
        let b = [(0.0, 5.0), (10.0, 5.0)];
        let series = [
            Series::new("a", &a).marker('a'),
            Series::new("b", &b).marker('b'),
        ];
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 6));
        LineChart::new(&series)
            .legend(false)
            .render(buf.area(), &mut buf);
        // y auto-range is [0,5]: 'a' (y=0) on the bottom plot row, 'b' (y=5)
        // on the top plot row.
        let out = lines(LineChart::new(&series).legend(false), 10, 6);
        assert!(out.contains('a') && out.contains('b'));
        // 'b' (the max) appears above 'a' (the min) somewhere.
        let row_of = |m: char| {
            (0..6)
                .find(|&y| (0..10).any(|x| buf.get(Position::new(x, y)).unwrap().symbol == m))
                .unwrap()
        };
        assert!(row_of('b') < row_of('a'));
    }

    #[test]
    fn a_single_point_series_plots_just_that_point() {
        let points = [(2.0, 2.0)];
        let series = [Series::new("s", &points).marker('@')];
        let out = lines(
            LineChart::new(&series)
                .x_bounds(AxisBounds::new(0.0, 4.0))
                .y_bounds(AxisBounds::new(0.0, 4.0))
                .legend(false),
            8,
            6,
        );
        assert_eq!(out.matches('@').count(), 1);
    }

    #[test]
    fn an_empty_series_list_just_draws_the_axes() {
        let series: [Series; 0] = [];
        let out = lines(LineChart::new(&series), 6, 4);
        // Axes still render; no marker glyphs.
        assert!(out.contains('└') && out.contains('│') && out.contains('─'));
    }

    #[test]
    fn a_series_with_no_points_draws_no_markers() {
        let points: [(f64, f64); 0] = [];
        let series = [Series::new("s", &points).marker('x')];
        let out = lines(LineChart::new(&series).legend(false), 6, 4);
        assert!(!out.contains('x'));
    }

    #[test]
    fn non_finite_points_are_skipped() {
        let points = [
            (0.0, 0.0),
            (1.0, f64::NAN),
            (2.0, f64::INFINITY),
            (3.0, 3.0),
        ];
        let series = [Series::new("s", &points).marker('o')];
        // Must not panic and still plots the finite endpoints.
        let out = lines(
            LineChart::new(&series)
                .x_bounds(AxisBounds::new(0.0, 3.0))
                .y_bounds(AxisBounds::new(0.0, 3.0))
                .legend(false),
            8,
            6,
        );
        assert!(out.contains('o'));
    }

    #[test]
    fn degenerate_bounds_are_padded_and_do_not_divide_by_zero() {
        // All points share one x and one y → auto-bounds are degenerate.
        let points = [(5.0, 5.0), (5.0, 5.0)];
        let series = [Series::new("s", &points).marker('=')];
        // Must not panic; the padded range keeps the scale total.
        let out = lines(LineChart::new(&series).legend(false), 8, 6);
        assert!(out.contains('='));
    }

    #[test]
    fn a_value_outside_the_bounds_is_clamped_into_the_plot() {
        let points = [(0.0, -100.0), (1.0, 100.0)];
        let series = [Series::new("s", &points).marker('c')];
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 6));
        LineChart::new(&series)
            .x_bounds(AxisBounds::new(0.0, 1.0))
            .y_bounds(AxisBounds::new(0.0, 1.0))
            .legend(false)
            .render(buf.area(), &mut buf);
        // Every 'c' stays within the plot body rows (above the X axis).
        for y in 0..6 {
            for x in 0..8 {
                if buf.get(Position::new(x, y)).unwrap().symbol == 'c' {
                    assert!(y < 5, "marker leaked onto/under the axis row");
                }
            }
        }
    }

    #[test]
    fn the_legend_lists_each_series_after_its_marker() {
        let p = [(0.0, 0.0), (1.0, 1.0)];
        let series = [
            Series::new("p50", &p).marker('A'),
            Series::new("p99", &p).marker('B'),
        ];
        let out = lines(LineChart::new(&series), 20, 6);
        assert!(out.contains("p50") && out.contains("p99"));
        assert!(out.contains('A') && out.contains('B'));
    }

    #[test]
    fn the_legend_is_suppressed_when_disabled() {
        let p = [(0.0, 0.0), (1.0, 1.0)];
        let series = [Series::new("zzz", &p)];
        let out = lines(LineChart::new(&series).legend(false), 20, 6);
        assert!(!out.contains("zzz"));
    }

    #[test]
    fn a_block_frames_the_chart_in_the_inner_area() {
        let p = [(0.0, 0.0), (1.0, 1.0)];
        let series = [Series::new("s", &p)];
        let out = lines(
            LineChart::new(&series)
                .legend(false)
                .block(Block::bordered()),
            8,
            6,
        );
        assert!(out.starts_with("┌──────┐\n"));
        assert!(out.contains('│'));
        assert!(out.contains('└'));
    }

    #[test]
    fn no_series_with_a_block_still_renders_the_block_and_its_axes() {
        let series: [Series; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 5));
        LineChart::new(&series)
            .block(Block::bordered())
            .render(buf.area(), &mut buf);
        // The block frame is intact (corners + edges)…
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, '┐');
        assert_eq!(buf.get(Position::new(0, 4)).unwrap().symbol, '└');
        assert_eq!(buf.get(Position::new(4, 4)).unwrap().symbol, '┘');
        // …and the empty chart still frames itself with axes inside it (the
        // observability panel is useful while data is still loading).
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, '│');
        assert_eq!(buf.get(Position::new(2, 3)).unwrap().symbol, '└');
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_plot() {
        let p = [(0.0, 0.0), (1.0, 1.0)];
        let series = [Series::new("s", &p)];
        let out = lines(LineChart::new(&series).block(Block::bordered()), 2, 2);
        assert_eq!(out, "┌┐\n└┘\n");
    }

    #[test]
    fn style_cascades_base_then_axis_and_series_styles() {
        let p = [(0.0, 0.0), (1.0, 1.0)];
        let series = [Series::new("s", &p)
            .marker('M')
            .style(Style::new().fg(Color::Green))];
        let chart = LineChart::new(&series)
            .legend(false)
            .x_bounds(AxisBounds::new(0.0, 1.0))
            .y_bounds(AxisBounds::new(0.0, 1.0))
            .style(Style::new().bg(Color::Blue))
            .axis_style(Style::new().fg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
        chart.render(buf.area(), &mut buf);

        // The origin corner carries the axis fg over the base bg.
        let corner = buf.get(Position::new(1, 3)).unwrap();
        assert_eq!(corner.symbol, '└');
        assert_eq!(corner.fg, Color::Red);
        assert_eq!(corner.bg, Color::Blue);

        // A marker carries the series fg over the base bg.
        let mut marker = None;
        for y in 0..3 {
            for x in 2..6 {
                let c = buf.get(Position::new(x, y)).unwrap();
                if c.symbol == 'M' {
                    marker = Some(c.clone());
                }
            }
        }
        let m = marker.expect("a marker was plotted");
        assert_eq!(m.fg, Color::Green);
        assert_eq!(m.bg, Color::Blue);
    }

    #[test]
    fn a_styled_legend_name_keeps_its_own_span_style() {
        let p = [(0.0, 0.0), (1.0, 1.0)];
        let name = Line::from(Span::styled(
            "hot",
            Style::new().add_modifier(Modifier::BOLD),
        ));
        let series = [Series {
            name,
            points: &p,
            style: Style::default(),
            marker: 'h',
        }];
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 6));
        LineChart::new(&series).render(buf.area(), &mut buf);
        let mut bold = false;
        for y in 0..6 {
            for x in 0..14 {
                let c = buf.get(Position::new(x, y)).unwrap();
                if c.symbol == 'h' && c.modifier.contains(Modifier::BOLD) {
                    bold = true;
                }
            }
        }
        assert!(bold, "the legend name kept its bold span style");
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let p = [(0.0, 0.0), (1.0, 1.0)];
        let series = [Series::new("s", &p)];
        let chart = LineChart::new(&series)
            .legend(false)
            .x_bounds(AxisBounds::new(0.0, 1.0))
            .y_bounds(AxisBounds::new(0.0, 1.0));
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 8));
        chart.render(Rect::new(4, 3, 6, 4), &mut buf);
        // The origin corner is relative to the render area, not the buffer.
        assert_eq!(buf.get(Position::new(5, 6)).unwrap().symbol, '└');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn a_tiny_area_clips_without_panicking() {
        let p = [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)];
        let series = [Series::new("s", &p)];
        // 1×1, 2×1, 1×2 must all be safe no-ops/clips.
        for (w, h) in [(1, 1), (2, 1), (1, 2), (3, 2)] {
            let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
            LineChart::new(&series).render(buf.area(), &mut buf);
        }
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let p = [(0.0, 0.0), (1.0, 1.0)];
        let series = [Series::new("s", &p)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        LineChart::new(&series).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
