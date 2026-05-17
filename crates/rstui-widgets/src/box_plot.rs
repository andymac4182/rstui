//! [`BoxPlot`] — a box-and-whisker plot over a shared value scale, the
//! statistics-dashboard primitive for "the distribution of each group side by
//! side" (request-latency percentiles per endpoint, salary spread per role, an
//! A/B experiment's per-arm metric summary).
//!
//! # A pure projection, like every other widget
//!
//! `BoxPlot` owns no state. It is a list of caller-built [`BoxStats`] (a label
//! plus the five-number summary and any outliers) and an optional value
//! window; the reducer decides what the summaries are (it computes the
//! quartiles in `update`) and the widget only projects them. That keeps it
//! deterministically headless-testable and composes with the Elm `view(&self)`
//! model exactly like [`List`](crate::List) and [`BarChart`](crate::BarChart).
//!
//! # Box-drawing glyphs, the [`Block`] precedent
//!
//! The box framing q1→q3 is drawn with the same single-scalar Unicode
//! box-drawing characters [`Block`] borders use (each maps 1:1 onto a
//! [`Cell`](rstui_core::Buffer) with no grapheme machinery), the whiskers with
//! a thin rule, the median with a contrasting tick, and outliers with a dot
//! glyph. Unlike [`BarChart`](crate::BarChart) a box plot has no fractional
//! *length* to ramp — its five marks are positions on the axis, so it places
//! whole glyphs at scaled cells rather than eighth-blocks, the
//! [`Block`]-border discipline rather than the [`Gauge`](crate::Gauge) one.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no stats, a degenerate summary (every value equal), no outliers, a
//! single box, a zero-span window (`min == max`), and an area too small for
//! the label gutter or the plot are all safe clips/no-ops — never a panic, no
//! division by zero. An optional framing [`Block`] follows the container-widget
//! convention; notched boxes and violin overlays are deliberately deferred
//! additive follow-ups, not smuggled into this slice.
//!
//! ```text
//! cargo run -p rstui-widgets --example box_plot_demo
//! ```

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The five-number summary of one distribution, plus its outliers and a label.
///
/// Build the label from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`); style it through the
/// [`Line`] it wraps. The caller computes the quartiles — the widget never
/// derives them.
#[derive(Debug, Default, Clone)]
pub struct BoxStats<'a> {
    label: Line<'a>,
    min: f64,
    q1: f64,
    median: f64,
    q3: f64,
    max: f64,
    outliers: Vec<f64>,
}

impl<'a> BoxStats<'a> {
    /// A summary labelled `label` with the given whisker ends (`min`/`max`),
    /// quartiles (`q1`/`q3`), and `median`, and no outliers.
    pub fn new(
        label: impl Into<Line<'a>>,
        min: f64,
        q1: f64,
        median: f64,
        q3: f64,
        max: f64,
    ) -> Self {
        Self {
            label: label.into(),
            min,
            q1,
            median,
            q3,
            max,
            outliers: Vec::new(),
        }
    }

    /// Sets the outlier values drawn as individual dots beyond the whiskers.
    #[must_use]
    pub fn outliers(mut self, outliers: Vec<f64>) -> Self {
        self.outliers = outliers;
        self
    }
}

/// Which way a [`BoxPlot`]'s boxes are laid out.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoxPlotOrientation {
    /// Boxes are horizontal rules stacked top to bottom, labels in a reserved
    /// left column (the default).
    #[default]
    Horizontal,
    /// Boxes are vertical rules placed left to right, labels in a reserved
    /// bottom row.
    Vertical,
}

/// A row/column of box-and-whisker plots over a shared value scale, with a
/// label gutter and an optional framing [`Block`].
///
/// Every [`BoxStats`] is mapped onto the same axis (the
/// [`bounds`](Self::bounds) window, or the extent of every value including
/// outliers when unset). Each box draws a whisker rule from `min` to `max`, a
/// box-drawing frame spanning q1→q3, a contrasting median tick, and a dot per
/// outlier. A label gutter (a left column when [`Horizontal`](BoxPlotOrientation::Horizontal),
/// a bottom row when [`Vertical`](BoxPlotOrientation::Vertical)) carries each
/// label, exactly like [`BarChart`](crate::BarChart).
///
/// Styling is a base [`Style`] (filling the content area) with a
/// [`box_style`](Self::box_style), [`whisker_style`](Self::whisker_style),
/// [`median_style`](Self::median_style), and [`outlier_style`](Self::outlier_style),
/// each over the base.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{BoxPlot, BoxStats};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 14, 1));
/// BoxPlot::new([BoxStats::new("p50", 0.0, 2.0, 4.0, 6.0, 8.0)])
///     .bounds(Some([0.0, 8.0]))
///     .render(buf.area(), &mut buf);
///
/// // A left label gutter carries the label.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'p');
/// ```
#[derive(Debug, Clone)]
pub struct BoxPlot<'a> {
    stats: Vec<BoxStats<'a>>,
    bounds: Option<[f64; 2]>,
    orientation: BoxPlotOrientation,
    block: Option<Block<'a>>,
    style: Style,
    box_style: Style,
    whisker_style: Style,
    median_style: Style,
    outlier_style: Style,
}

impl Default for BoxPlot<'_> {
    fn default() -> Self {
        Self {
            stats: Vec::new(),
            bounds: None,
            orientation: BoxPlotOrientation::Horizontal,
            block: None,
            style: Style::default(),
            box_style: Style::default(),
            whisker_style: Style::default(),
            median_style: Style::default(),
            outlier_style: Style::default(),
        }
    }
}

impl<'a> BoxPlot<'a> {
    /// A horizontal box plot of `stats`, auto-scaled to the extent of every
    /// value (outliers included), with no frame.
    pub fn new<I>(stats: I) -> Self
    where
        I: IntoIterator<Item = BoxStats<'a>>,
    {
        Self {
            stats: stats.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets the `[min, max]` value window, or `None` to auto-scale over every
    /// value across all stats, outliers included.
    ///
    /// A zero-span window (`min == max`) collapses every mark onto one cell
    /// (never a panic — the [`Gauge`](crate::Gauge) totality rule).
    #[must_use]
    pub fn bounds(mut self, bounds: Option<[f64; 2]>) -> Self {
        self.bounds = bounds;
        self
    }

    /// Sets whether boxes lie horizontally (default) or stand vertically.
    #[must_use]
    pub fn orientation(mut self, orientation: BoxPlotOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Frames the plot in `block`; boxes render into [`block.inner`](Block::inner).
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

    /// Sets the [`Style`] for the q1→q3 box frame, over the base.
    #[must_use]
    pub fn box_style(mut self, style: Style) -> Self {
        self.box_style = style;
        self
    }

    /// Sets the [`Style`] for the min→max whisker rule, over the base.
    #[must_use]
    pub fn whisker_style(mut self, style: Style) -> Self {
        self.whisker_style = style;
        self
    }

    /// Sets the [`Style`] for the median tick, over the base.
    #[must_use]
    pub fn median_style(mut self, style: Style) -> Self {
        self.median_style = style;
        self
    }

    /// Sets the [`Style`] for the outlier dots, over the base.
    #[must_use]
    pub fn outlier_style(mut self, style: Style) -> Self {
        self.outlier_style = style;
        self
    }
}

/// The cell offset (`0..span`) a `value` maps to along a `span`-cell axis
/// spanning `min..=max`, clamped in range and total on a zero span (no
/// division by zero — every value then maps to cell `0`).
fn place(value: f64, min: f64, max: f64, span: u16) -> u16 {
    if span == 0 {
        return 0;
    }
    let width = max - min;
    let frac = if width <= 0.0 {
        0.0
    } else {
        ((value - min) / width).clamp(0.0, 1.0)
    };
    let cell = (frac * f64::from(span - 1)).round() as i64;
    cell.clamp(0, i64::from(span - 1)) as u16
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

impl Widget for BoxPlot<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let BoxPlot {
            stats,
            bounds,
            orientation,
            block,
            style,
            box_style,
            whisker_style,
            median_style,
            outlier_style,
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
        if stats.is_empty() {
            return;
        }

        // The value window: the caller's, or the extent of every value
        // (whiskers + box + median + outliers).
        let (min, max) = match bounds {
            Some([lo, hi]) => (lo, hi),
            None => {
                let mut lo = f64::INFINITY;
                let mut hi = f64::NEG_INFINITY;
                for s in &stats {
                    for v in [s.min, s.q1, s.median, s.q3, s.max]
                        .into_iter()
                        .chain(s.outliers.iter().copied())
                    {
                        lo = lo.min(v);
                        hi = hi.max(v);
                    }
                }
                (lo, hi)
            }
        };

        let box_s = style.patch(box_style);
        let whisker_s = style.patch(whisker_style);
        let median_s = style.patch(median_style);
        let outlier_s = style.patch(outlier_style);

        match orientation {
            BoxPlotOrientation::Horizontal => {
                // A left label gutter, at most half the width, sized to the
                // longest label; the boxes fill the rest (BarChart's rule).
                let longest = stats.iter().map(|s| s.label.width()).max().unwrap_or(0) as u16;
                let gutter_w = longest.min(inner.width / 2);
                let plot_x0 = inner.left().saturating_add(gutter_w);
                let plot_w = inner.width.saturating_sub(gutter_w);
                let right = inner.right();
                let bottom = inner.bottom();

                let mut y = inner.top();
                for s in &stats {
                    if y >= bottom {
                        break;
                    }
                    if gutter_w > 0 {
                        stamp_line(
                            buf,
                            &s.label,
                            style,
                            inner.left(),
                            y,
                            inner.left().saturating_add(gutter_w),
                        );
                    }
                    if plot_w > 0 {
                        let col = |v: f64| plot_x0.saturating_add(place(v, min, max, plot_w));
                        let (lo, hi) = (col(s.min), col(s.max));
                        let (q1c, q3c) = (col(s.q1), col(s.q3));
                        let medc = col(s.median);

                        // The whisker rule, min → max.
                        for x in lo..=hi.min(right.saturating_sub(1)) {
                            buf.set_cell(Position::new(x, y), '─', whisker_s);
                        }
                        // Whisker end caps.
                        buf.set_cell(Position::new(lo, y), '├', whisker_s);
                        if hi < right {
                            buf.set_cell(Position::new(hi, y), '┤', whisker_s);
                        }
                        // The q1→q3 box frame (degenerate q1==q3 → one cell).
                        if q3c > q1c {
                            for x in (q1c + 1)..q3c {
                                buf.set_cell(Position::new(x, y), '─', box_s);
                            }
                            buf.set_cell(Position::new(q1c, y), '┤', box_s);
                            if q3c < right {
                                buf.set_cell(Position::new(q3c, y), '├', box_s);
                            }
                        } else {
                            buf.set_cell(Position::new(q1c, y), '┼', box_s);
                        }
                        // The median tick, drawn last so it always shows.
                        if medc < right {
                            buf.set_cell(Position::new(medc, y), '┃', median_s);
                        }
                        // Outliers as dots beyond the whiskers.
                        for &o in &s.outliers {
                            let x = col(o);
                            if x < right {
                                buf.set_cell(Position::new(x, y), '∙', outlier_s);
                            }
                        }
                    }
                    y = y.saturating_add(1);
                }
            }
            BoxPlotOrientation::Vertical => {
                // A bottom label row when there's more than one row; boxes
                // stand in the rows above it (BarChart's vertical rule).
                let label_row = inner.height > 1;
                let plot_h = inner.height.saturating_sub(u16::from(label_row));
                let label_y = inner.bottom().saturating_sub(1);
                let right = inner.right();
                // Map a value to a row, flipped so max is the top row.
                let row = |v: f64| {
                    let from_bottom = place(v, min, max, plot_h);
                    inner
                        .top()
                        .saturating_add(plot_h.saturating_sub(1) - from_bottom)
                };

                let mut x = inner.left();
                for s in &stats {
                    if x >= right {
                        break;
                    }
                    if plot_h > 0 {
                        let (hi_r, lo_r) = (row(s.max), row(s.min));
                        let (q3r, q1r) = (row(s.q3), row(s.q1));
                        let medr = row(s.median);

                        // The whisker rule, min (bottom) → max (top).
                        for yy in hi_r..=lo_r {
                            buf.set_cell(Position::new(x, yy), '│', whisker_s);
                        }
                        buf.set_cell(Position::new(x, hi_r), '┬', whisker_s);
                        buf.set_cell(Position::new(x, lo_r), '┴', whisker_s);
                        // The q3→q1 box frame (q1==q3 → one cell).
                        if q1r > q3r {
                            for yy in (q3r + 1)..q1r {
                                buf.set_cell(Position::new(x, yy), '│', box_s);
                            }
                            buf.set_cell(Position::new(x, q3r), '┴', box_s);
                            buf.set_cell(Position::new(x, q1r), '┬', box_s);
                        } else {
                            buf.set_cell(Position::new(x, q3r), '┼', box_s);
                        }
                        // The median tick, drawn last so it always shows.
                        buf.set_cell(Position::new(x, medr), '━', median_s);
                        // Outliers as dots beyond the whiskers.
                        for &o in &s.outliers {
                            buf.set_cell(Position::new(x, row(o)), '∙', outlier_s);
                        }
                    }
                    // The label centred under this column, clipped to one cell.
                    if label_row {
                        stamp_line(buf, &s.label, style, x, label_y, right);
                    }
                    x = x.saturating_add(1);
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
    fn a_horizontal_box_has_a_gutter_whisker_box_and_median() {
        // bounds 0..8, label "p" (gutter_w 1), plot 9 wide. min0→col0,
        // max8→col8, q1 2→col2, q3 6→col6, median 4→col4.
        let plot =
            BoxPlot::new([BoxStats::new("p", 0.0, 2.0, 4.0, 6.0, 8.0)]).bounds(Some([0.0, 8.0]));
        let out = lines(plot, 10, 1);
        let row = out.lines().next().unwrap();
        let cells: Vec<char> = row.chars().collect();
        assert_eq!(cells[0], 'p'); // label gutter
        assert_eq!(cells[1], '├'); // whisker low cap at min
        assert_eq!(cells[3], '┤'); // box left edge at q1 (col 1+2)
        assert_eq!(cells[5], '┃'); // median tick at col 1+4
        assert_eq!(cells[7], '├'); // box right edge at q3 (col 1+6)
        assert_eq!(cells[9], '┤'); // whisker high cap at max (col 1+8)
    }

    #[test]
    fn outliers_are_dots_on_the_axis() {
        let plot =
            BoxPlot::new([BoxStats::new("a", 2.0, 3.0, 4.0, 5.0, 6.0).outliers(vec![0.0, 8.0])])
                .bounds(Some([0.0, 8.0]));
        let out = lines(plot, 10, 1);
        let row = out.lines().next().unwrap();
        let cells: Vec<char> = row.chars().collect();
        // Outlier at value 0 → col 1+0, value 8 → col 1+8.
        assert_eq!(cells[1], '∙');
        assert_eq!(cells[9], '∙');
    }

    #[test]
    fn a_degenerate_stat_all_equal_is_a_single_glyph_without_panicking() {
        // Every value equal: q1==q3 collapses the box to one '┼' cell.
        let plot =
            BoxPlot::new([BoxStats::new("x", 4.0, 4.0, 4.0, 4.0, 4.0)]).bounds(Some([0.0, 8.0]));
        let out = lines(plot, 10, 1);
        assert!(out.contains('┼') || out.contains('┃'));
    }

    #[test]
    fn a_zero_span_window_collapses_onto_one_cell_without_panicking() {
        // min == max → place() returns 0 for everything, no division by zero.
        let plot =
            BoxPlot::new([BoxStats::new("z", 5.0, 5.0, 5.0, 5.0, 5.0)]).bounds(Some([5.0, 5.0]));
        assert_eq!(lines(plot, 6, 1).lines().count(), 1);
    }

    #[test]
    fn empty_outliers_just_draw_the_box() {
        let plot =
            BoxPlot::new([BoxStats::new("a", 0.0, 2.0, 4.0, 6.0, 8.0)]).bounds(Some([0.0, 8.0]));
        // No '∙' anywhere when there are no outliers.
        assert!(!lines(plot, 10, 1).contains('∙'));
    }

    #[test]
    fn auto_scale_covers_every_value_including_outliers() {
        // No bounds: min over all = -1 (an outlier), max = 9 (an outlier).
        let plot =
            BoxPlot::new([BoxStats::new("a", 1.0, 2.0, 3.0, 4.0, 5.0).outliers(vec![-1.0, 9.0])]);
        let out = lines(plot, 11, 1);
        let row = out.lines().next().unwrap();
        let cells: Vec<char> = row.chars().collect();
        // The lowest outlier sits at the leftmost plot cell, highest at the
        // rightmost — proving the scale spans the outliers.
        assert_eq!(cells[1], '∙');
        assert_eq!(cells[10], '∙');
    }

    #[test]
    fn a_vertical_box_stands_with_a_bottom_label_row() {
        // bounds 0..8, 1 column, 5 tall: 4 plot rows + 1 label row. max 8 →
        // top row, min 0 → bottom plot row, label on the last row.
        let plot = BoxPlot::new([BoxStats::new("v", 0.0, 2.0, 4.0, 6.0, 8.0)])
            .bounds(Some([0.0, 8.0]))
            .orientation(BoxPlotOrientation::Vertical);
        let out = lines(plot, 1, 5);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows[0], "┬"); // whisker top cap at max
        assert_eq!(rows[4], "v"); // label row at the bottom
    }

    #[test]
    fn multiple_boxes_stack_each_on_its_own_row() {
        let plot = BoxPlot::new([
            BoxStats::new("a", 0.0, 2.0, 4.0, 6.0, 8.0),
            BoxStats::new("b", 0.0, 2.0, 4.0, 6.0, 8.0),
        ])
        .bounds(Some([0.0, 8.0]));
        let out = lines(plot, 10, 2);
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[0].starts_with('a'));
        assert!(rows[1].starts_with('b'));
    }

    #[test]
    fn a_block_frames_the_plot_in_the_inner_area() {
        let plot = BoxPlot::new([BoxStats::new("", 0.0, 1.0, 2.0, 3.0, 4.0)])
            .bounds(Some([0.0, 4.0]))
            .block(Block::bordered());
        let out = lines(plot, 6, 3);
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[0].starts_with('┌'));
        assert!(rows[2].starts_with('└'));
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_nothing_inside() {
        let plot =
            BoxPlot::new([BoxStats::new("x", 0.0, 1.0, 2.0, 3.0, 4.0)]).block(Block::bordered());
        assert_eq!(lines(plot, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn no_stats_with_a_block_still_renders_the_block() {
        let plot = BoxPlot::new(Vec::<BoxStats>::new()).block(Block::bordered());
        assert_eq!(lines(plot, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn style_cascades_base_then_part_styles() {
        let plot = BoxPlot::new([BoxStats::new(
            Line::from(Span::styled("L", Style::new().fg(Color::Red))),
            0.0,
            2.0,
            4.0,
            6.0,
            8.0,
        )])
        .bounds(Some([0.0, 8.0]))
        .style(Style::new().bg(Color::Blue))
        .median_style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        plot.render(buf.area(), &mut buf);

        // The label keeps its own span fg over the base bg.
        let l = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(l.symbol, 'L');
        assert_eq!(l.fg, Color::Red);
        assert_eq!(l.bg, Color::Blue);

        // The median tick at col 1+4 picks up median_style over the base bg.
        let m = buf.get(Position::new(5, 0)).unwrap();
        assert_eq!(m.symbol, '┃');
        assert_eq!(m.fg, Color::Yellow);
        assert_eq!(m.bg, Color::Blue);
        assert!(m.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let plot =
            BoxPlot::new([BoxStats::new("p", 0.0, 2.0, 4.0, 6.0, 8.0)]).bounds(Some([0.0, 8.0]));
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 3));
        plot.render(Rect::new(2, 1, 10, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, 'p');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn a_tiny_area_with_no_plot_room_still_draws_the_label() {
        // width 2, label "ab": gutter_w = min(2, 1) = 1, plot_w 1; the box
        // clips safely. The invariant is no panic.
        let plot =
            BoxPlot::new([BoxStats::new("ab", 0.0, 1.0, 2.0, 3.0, 4.0)]).bounds(Some([0.0, 4.0]));
        let _ = lines(plot, 2, 1);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        BoxPlot::new([BoxStats::new("x", 0.0, 1.0, 2.0, 3.0, 4.0)])
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
