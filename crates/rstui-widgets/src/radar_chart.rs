//! [`RadarChart`] — a spider/radar plot: N axes radiating from a shared
//! centre, concentric ring gridlines, and one or more series polygons, the
//! dashboard primitive for "score this thing across a handful of dimensions"
//! (a service's SLOs, a candidate's skills, two products compared feature by
//! feature).
//!
//! # A pure projection, like every other widget
//!
//! `RadarChart` owns no state. It is a list of caller-built [`RadarAxis`]es (a
//! label [`Line`] and a per-axis `max`) and [`RadarSeries`] (a borrowed
//! `&[f64]` index-aligned to the axes, plus a [`Color`]); the reducer decides
//! what the axes and series are and the widget only projects them. That keeps
//! it deterministically headless-testable and composes with the Elm
//! `view(&self)` model exactly like [`List`](crate::List) and
//! [`BarChart`](crate::BarChart).
//!
//! # Composed on the [`Canvas`] keystone
//!
//! Spokes, rings, and series edges are all *straight segments*, so rather than
//! re-rasterise lines this widget **composes** [`Canvas`]: every spoke, every
//! ring chord, and every polygon edge is a [`CanvasLine`] drawn through a
//! [`Canvas`] over a centred, aspect-corrected square plot region (a terminal
//! cell is about twice as tall as it is wide, so the usable square is the
//! largest that fits *after* halving the height in data units). The canvas
//! resolves each segment at sub-cell [`Marker`] precision and collapses every
//! cell to one Unicode scalar — the same pure-projection, no-retained-scene
//! discipline [`Canvas`] itself uses.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, **fewer than three axes** (a polygon is undefined, so only the axis
//! labels are drawn), an empty series list, a zero (or non-finite) axis `max`
//! (that axis contributes the centre, no division by zero), a series shorter
//! or longer than the axis count (missing values sit at the centre, extra ones
//! are ignored), and a region too small for the plot are all safe clips/no-ops
//! — never a panic. An optional framing [`Block`] follows the container-widget
//! convention; a filled (translucent) polygon body is a deliberately deferred
//! additive follow-up, not smuggled into this slice.

use rstui_core::{Buffer, Color, Line, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::canvas::{Canvas, CanvasLine, Marker};

/// The terminal-cell aspect ratio: a cell is about twice as tall as it is
/// wide, so the plot's data-space height is halved to keep the rings round.
const CELL_ASPECT: f64 = 2.0;

/// One axis of a [`RadarChart`]: a rim label [`Line`] and the value mapped to
/// the outer ring (the axis full-scale).
///
/// A series value is plotted as its fraction of this `max` along the axis;
/// build the label from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`) and style it through the
/// [`Line`] it wraps.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RadarAxis<'a> {
    /// The label drawn at this axis's rim.
    label: Line<'a>,
    /// The value mapped to the outer ring; a value of this length reaches the
    /// rim. A zero or non-finite `max` pins this axis at the centre.
    max: f64,
}

impl<'a> RadarAxis<'a> {
    /// An axis whose outer ring is `max`, labelled `label` (anything
    /// convertible to a [`Line`]).
    pub fn new(max: f64, label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            max,
        }
    }
}

/// One polygon of a [`RadarChart`]: per-axis values (borrowed, index-aligned
/// to the axes) and the [`Color`] its outline is drawn with.
///
/// Value `i` is plotted on axis `i` as `values[i] / axes[i].max` of the way to
/// the rim. A series shorter than the axis count leaves the missing axes at
/// the centre; a longer one has its extra values ignored — both tolerated, not
/// a panic (the totality rule).
#[derive(Debug, Clone, PartialEq)]
pub struct RadarSeries<'a> {
    /// The per-axis values, index-aligned to the chart's axes.
    values: &'a [f64],
    /// The colour this series' polygon outline is drawn with.
    color: Color,
}

impl<'a> RadarSeries<'a> {
    /// A series plotting `values` (axis-aligned) with outline `color`.
    #[must_use]
    pub fn new(values: &'a [f64], color: Color) -> Self {
        Self { values, color }
    }
}

/// A spider/radar plot: evenly-spaced axes, concentric ring gridlines, and one
/// or more series polygons, with an optional framing [`Block`].
///
/// Axes radiate from a shared centre, the first pointing straight up and the
/// rest spaced evenly clockwise. [`rings`](Self::rings) concentric gridlines
/// (drawn as the polygon joining the axis points at each fraction) give the
/// scale; each [`RadarSeries`] is the polygon joining its per-axis values.
/// Styling is a base [`Style`] (filling the area so a background covers the
/// whole pane) with a [`grid_style`](Self::grid_style) for the spokes/rings
/// under each series' own [`Color`] and each axis label's own
/// [`Line`]/[`Span`](rstui_core::Span) styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Color, Rect, Widget};
/// use rstui_widgets::{RadarAxis, RadarChart, RadarSeries};
///
/// let axes = [
///     RadarAxis::new(10.0, "spd"),
///     RadarAxis::new(10.0, "pow"),
///     RadarAxis::new(10.0, "def"),
/// ];
/// let vals = [8.0, 5.0, 9.0];
/// let series = [RadarSeries::new(&vals, Color::Cyan)];
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 24, 12));
/// RadarChart::new(&axes, &series).render(buf.area(), &mut buf);
/// ```
#[derive(Debug, Clone)]
pub struct RadarChart<'a> {
    axes: &'a [RadarAxis<'a>],
    series: &'a [RadarSeries<'a>],
    rings: u16,
    block: Option<Block<'a>>,
    style: Style,
    grid_style: Style,
}

impl<'a> RadarChart<'a> {
    /// A radar chart over `axes` with `series` polygons, four rings, no frame.
    #[must_use]
    pub fn new(axes: &'a [RadarAxis<'a>], series: &'a [RadarSeries<'a>]) -> Self {
        Self {
            axes,
            series,
            rings: 4,
            block: None,
            style: Style::default(),
            grid_style: Style::default(),
        }
    }

    /// Sets the number of concentric ring gridlines (default `4`; `0` draws
    /// only the spokes).
    #[must_use]
    pub fn rings(mut self, rings: u16) -> Self {
        self.rings = rings;
        self
    }

    /// Frames the chart in `block`; the plot renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a background
    /// covers the whole pane beneath the plot.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] (really its [`Color`]) the spokes and rings are
    /// drawn with, over the base.
    #[must_use]
    pub fn grid_style(mut self, style: Style) -> Self {
        self.grid_style = style;
        self
    }
}

/// The colour a [`Style`] paints with on the [`Canvas`], or a mid grey when it
/// sets none (so the grid is always visible without forcing the caller to
/// style it).
fn line_color(style: Style, fallback: Color) -> Color {
    match style.fg {
        None | Some(Color::Reset) => fallback,
        Some(c) => c,
    }
}

/// Stamps `line` left-to-right from `x0` on row `y`, clipped to `[left, right)`,
/// with `base` beneath the line→span cascade (anchored so it stays on-buffer).
fn stamp_label(buf: &mut Buffer, line: &Line, base: Style, cx: i32, y: u16, left: u16, right: u16) {
    let w = line.width() as i32;
    // Centre the label on `cx`, then clamp it fully inside the content band.
    let mut x = cx - w / 2;
    if x < i32::from(left) {
        x = i32::from(left);
    }
    if x + w > i32::from(right) {
        x = i32::from(right) - w;
    }
    if x < i32::from(left) {
        x = i32::from(left);
    }
    let mut x = x.max(0) as u16;
    let line_base = base.patch(line.style);
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

impl Widget for RadarChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let RadarChart {
            axes,
            series,
            rings,
            block,
            style,
            grid_style,
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
        let n = axes.len();
        if n == 0 {
            return;
        }

        // The unit angle of axis `i`, clockwise from straight up. Returned as a
        // `(dx, dy)` direction in data space where +y is up (canvas convention).
        let tau = std::f64::consts::TAU;
        let dir = |i: usize| -> (f64, f64) {
            let a = (i as f64) * tau / (n as f64);
            (a.sin(), a.cos())
        };

        // The plot works in a centred unit square `[-1, 1]²`; the canvas
        // y-bounds are pre-divided by CELL_ASPECT so the rings read round on a
        // 2:1 cell grid. A point at fraction `f` on axis `i` is `f * dir(i)`.
        let lim = 1.05_f64;
        let y_lim = lim / CELL_ASPECT;

        // Fewer than three axes cannot bound a polygon — render the labels
        // only (the totality rule), no spokes or series.
        let polygonal = n >= 3;

        // The fraction of full-scale a series reaches on one axis, clamped to
        // `0..=1`; a zero / non-finite axis max pins it at the centre.
        let frac = |value: f64, max: f64| -> f64 {
            if !value.is_finite() || !max.is_finite() || max <= 0.0 {
                0.0
            } else {
                (value / max).clamp(0.0, 1.0)
            }
        };

        let grid_c = line_color(grid_style, Color::Indexed(244));

        // Draw the whole plot through one composed Canvas over the inner area.
        Canvas::default()
            .x_bounds([-lim, lim])
            .y_bounds([-y_lim, y_lim])
            .marker(Marker::Braille)
            .paint(|ctx| {
                if !polygonal {
                    return;
                }
                // Spokes: centre out to each axis at the rim.
                for i in 0..n {
                    let (dx, dy) = dir(i);
                    ctx.draw(&CanvasLine {
                        x1: 0.0,
                        y1: 0.0,
                        x2: dx,
                        y2: dy / CELL_ASPECT,
                        color: grid_c,
                    });
                }
                // Rings: the closed polygon joining every axis at fraction
                // `k / rings`, for each ring `k`.
                for k in 1..=rings {
                    let rf = f64::from(k) / f64::from(rings.max(1));
                    for i in 0..n {
                        let (ax, ay) = dir(i);
                        let (bx, by) = dir((i + 1) % n);
                        ctx.draw(&CanvasLine {
                            x1: ax * rf,
                            y1: ay * rf / CELL_ASPECT,
                            x2: bx * rf,
                            y2: by * rf / CELL_ASPECT,
                            color: grid_c,
                        });
                    }
                }
                // Each series: its own layer so colours never blend, the
                // closed polygon joining its per-axis fractions.
                for s in series {
                    ctx.layer();
                    for i in 0..n {
                        let fa = s.values.get(i).map_or(0.0, |&v| frac(v, axes[i].max));
                        let fb = s
                            .values
                            .get((i + 1) % n)
                            .map_or(0.0, |&v| frac(v, axes[(i + 1) % n].max));
                        let (ax, ay) = dir(i);
                        let (bx, by) = dir((i + 1) % n);
                        ctx.draw(&CanvasLine {
                            x1: ax * fa,
                            y1: ay * fa / CELL_ASPECT,
                            x2: bx * fb,
                            y2: by * fb / CELL_ASPECT,
                            color: s.color,
                        });
                    }
                }
            })
            .render(inner, buf);

        // Axis labels at the rim, stamped on top of the plot. Each label is
        // anchored at the axis tip in the *same* data space the spokes use,
        // mapped to a cell with the identical transform [`Canvas`] applies (so
        // a label always sits on its spoke). The up-axis (i = 0) maps to the
        // top row because screen y grows downward while data +y is up.
        let map_cell = |x: f64, y: f64| -> (f64, f64) {
            // Fraction across each bound, exactly Canvas's to_pixel math.
            let fx = (x + lim) / (2.0 * lim);
            let fy = (y + y_lim) / (2.0 * y_lim);
            let col = f64::from(inner.left()) + fx * f64::from(inner.width.saturating_sub(1));
            // Screen y is flipped relative to data y.
            let row =
                f64::from(inner.top()) + (1.0 - fy) * f64::from(inner.height.saturating_sub(1));
            (col, row)
        };
        for (i, axis) in axes.iter().enumerate() {
            let (dx, dy) = dir(i);
            let (col, row) = map_cell(dx, dy / CELL_ASPECT);
            let row = row.round();
            if row < f64::from(inner.top()) || row >= f64::from(inner.bottom()) {
                continue;
            }
            stamp_label(
                buf,
                &axis.label,
                style,
                col.round() as i32,
                row as u16,
                inner.left(),
                inner.right(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Modifier, Span};

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

    /// `true` if any cell carries a Braille glyph (the canvas plotted there).
    fn any_braille(buf: &Buffer, w: u16, h: u16) -> bool {
        for y in 0..h {
            for x in 0..w {
                let c = buf.get(Position::new(x, y)).unwrap().symbol as u32;
                if (0x2800..=0x28FF).contains(&c) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn three_axes_draw_spokes_and_a_series_polygon() {
        let axes = [
            RadarAxis::new(10.0, "a"),
            RadarAxis::new(10.0, "b"),
            RadarAxis::new(10.0, "c"),
        ];
        let vals = [10.0, 6.0, 8.0];
        let series = [RadarSeries::new(&vals, Color::Cyan)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 16));
        RadarChart::new(&axes, &series).render(buf.area(), &mut buf);
        // The grid + polygon are Braille runs from the composed Canvas.
        assert!(any_braille(&buf, 30, 16));
        // A series cell carries the series colour somewhere on the plot.
        let mut found = false;
        for y in 0..16 {
            for x in 0..30 {
                if buf.get(Position::new(x, y)).unwrap().fg == Color::Cyan {
                    found = true;
                }
            }
        }
        assert!(found, "the series polygon should paint at least one cell");
    }

    #[test]
    fn axis_labels_are_stamped_at_the_rim() {
        let axes = [
            RadarAxis::new(1.0, "TOP"),
            RadarAxis::new(1.0, "BR"),
            RadarAxis::new(1.0, "BL"),
        ];
        let series: [RadarSeries; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 11));
        RadarChart::new(&axes, &series).render(buf.area(), &mut buf);
        // The first axis points straight up → its label is on the top row.
        let top: String = (0..20)
            .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
            .collect();
        assert!(top.contains("TOP"), "top row = {top:?}");
    }

    #[test]
    fn fewer_than_three_axes_render_labels_only_without_a_panic() {
        let axes = [RadarAxis::new(1.0, "solo"), RadarAxis::new(1.0, "duo")];
        let vals = [1.0, 1.0];
        let series = [RadarSeries::new(&vals, Color::Red)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 9));
        RadarChart::new(&axes, &series).render(buf.area(), &mut buf);
        // Degenerate: no polygon, so no Braille — only the labels.
        assert!(!any_braille(&buf, 16, 9));
        let joined: String = (0..9)
            .flat_map(|y| (0..16).map(move |x| (x, y)))
            .map(|(x, y)| buf.get(Position::new(x, y)).unwrap().symbol)
            .collect();
        assert!(joined.contains("solo"));
        assert!(joined.contains("duo"));
    }

    #[test]
    fn an_empty_series_list_still_draws_the_grid() {
        let axes = [
            RadarAxis::new(1.0, "a"),
            RadarAxis::new(1.0, "b"),
            RadarAxis::new(1.0, "c"),
        ];
        let series: [RadarSeries; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 13));
        RadarChart::new(&axes, &series).render(buf.area(), &mut buf);
        // No series, but the spokes + rings are still a Braille grid.
        assert!(any_braille(&buf, 24, 13));
    }

    #[test]
    fn a_zero_axis_max_pins_that_axis_at_the_centre_without_dividing_by_zero() {
        let axes = [
            RadarAxis::new(0.0, "a"),
            RadarAxis::new(10.0, "b"),
            RadarAxis::new(10.0, "c"),
        ];
        let vals = [5.0, 5.0, 5.0];
        let series = [RadarSeries::new(&vals, Color::Green)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 13));
        // No panic; the zero-max axis just contributes the centre point.
        RadarChart::new(&axes, &series).render(buf.area(), &mut buf);
        assert!(any_braille(&buf, 24, 13));
    }

    #[test]
    fn a_short_or_long_series_is_tolerated() {
        let axes = [
            RadarAxis::new(10.0, "a"),
            RadarAxis::new(10.0, "b"),
            RadarAxis::new(10.0, "c"),
        ];
        // Two values for three axes (third sits at the centre) and one extra.
        let short = [8.0, 4.0];
        let long = [1.0, 2.0, 3.0, 9.0, 9.0];
        let series = [
            RadarSeries::new(&short, Color::Red),
            RadarSeries::new(&long, Color::Blue),
        ];
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 13));
        // Neither over- nor under-length values panic.
        RadarChart::new(&axes, &series).render(buf.area(), &mut buf);
        assert!(any_braille(&buf, 24, 13));
    }

    #[test]
    fn rings_zero_draws_spokes_but_no_ring_chords() {
        let axes = [
            RadarAxis::new(1.0, "a"),
            RadarAxis::new(1.0, "b"),
            RadarAxis::new(1.0, "c"),
        ];
        let series: [RadarSeries; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 13));
        RadarChart::new(&axes, &series)
            .rings(0)
            .render(buf.area(), &mut buf);
        // Spokes alone are still a Braille grid (no panic on rings = 0).
        assert!(any_braille(&buf, 24, 13));
    }

    #[test]
    fn a_block_frames_the_chart_in_the_inner_area() {
        let axes = [
            RadarAxis::new(1.0, "a"),
            RadarAxis::new(1.0, "b"),
            RadarAxis::new(1.0, "c"),
        ];
        let series: [RadarSeries; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 11));
        RadarChart::new(&axes, &series)
            .block(Block::bordered())
            .render(buf.area(), &mut buf);
        // The border is intact (corners + edges) and the plot is confined to
        // the inner area, so the frame is never overpainted by a spoke.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(19, 0)).unwrap().symbol, '┐');
        assert_eq!(buf.get(Position::new(0, 10)).unwrap().symbol, '└');
        assert_eq!(buf.get(Position::new(19, 10)).unwrap().symbol, '┘');
        for x in 1..19 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().symbol, '─');
            assert_eq!(buf.get(Position::new(x, 10)).unwrap().symbol, '─');
        }
        for y in 1..10 {
            assert_eq!(buf.get(Position::new(0, y)).unwrap().symbol, '│');
            assert_eq!(buf.get(Position::new(19, y)).unwrap().symbol, '│');
        }
    }

    #[test]
    fn a_tiny_area_clips_without_a_panic() {
        let axes = [
            RadarAxis::new(1.0, "a"),
            RadarAxis::new(1.0, "b"),
            RadarAxis::new(1.0, "c"),
        ];
        let vals = [1.0, 1.0, 1.0];
        let series = [RadarSeries::new(&vals, Color::Red)];
        // 2×1 is far too small for any meaningful plot — must not panic.
        let _ = lines(RadarChart::new(&axes, &series), 2, 1);
    }

    #[test]
    fn the_grid_style_colours_the_spokes_and_rings() {
        let axes = [
            RadarAxis::new(1.0, "a"),
            RadarAxis::new(1.0, "b"),
            RadarAxis::new(1.0, "c"),
        ];
        let series: [RadarSeries; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 13));
        RadarChart::new(&axes, &series)
            .grid_style(Style::new().fg(Color::Magenta))
            .render(buf.area(), &mut buf);
        let mut found = false;
        for y in 0..13 {
            for x in 0..24 {
                if buf.get(Position::new(x, y)).unwrap().fg == Color::Magenta {
                    found = true;
                }
            }
        }
        assert!(found, "grid_style should colour the spokes/rings");
    }

    #[test]
    fn style_cascades_the_base_under_the_axis_labels() {
        let axes = [
            RadarAxis::new(
                1.0,
                Line::from(Span::styled("X", Style::new().add_modifier(Modifier::BOLD))),
            ),
            RadarAxis::new(1.0, "b"),
            RadarAxis::new(1.0, "c"),
        ];
        let series: [RadarSeries; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 11));
        RadarChart::new(&axes, &series)
            .style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        // The first axis label is on the top row; its span modifier wins over
        // the cascaded base background.
        let mut cell = None;
        for x in 0..20 {
            let c = buf.get(Position::new(x, 0)).unwrap();
            if c.symbol == 'X' {
                cell = Some(c);
            }
        }
        let cell = cell.expect("the up-axis 'X' label should be on the top row");
        assert!(cell.modifier.contains(Modifier::BOLD));
        assert_eq!(cell.bg, Color::Blue);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let axes = [
            RadarAxis::new(1.0, "a"),
            RadarAxis::new(1.0, "b"),
            RadarAxis::new(1.0, "c"),
        ];
        let series: [RadarSeries; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        RadarChart::new(&axes, &series).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
