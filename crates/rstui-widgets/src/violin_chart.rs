//! [`ViolinChart`] — a violin (density) plot over a shared value scale, the
//! statistics-dashboard primitive for "the *shape* of each group's
//! distribution side by side" (latency density per endpoint, score
//! distribution per cohort, an A/B arm's full metric density). The
//! density-curve sibling of [`BoxPlot`](crate::BoxPlot), which shows only the
//! five-number summary.
//!
//! # A pure projection, like every other widget
//!
//! `ViolinChart` owns no state. It is a list of caller-built [`Violin`]s (a
//! label plus a precomputed **density profile** and an optional median) and an
//! optional value window; the reducer decides what the densities are (it runs
//! the kernel-density / histogram estimate in `update` — a violin needs real
//! statistics, kept out of the dependency-free widget exactly as
//! [`BoxPlot`](crate::BoxPlot) keeps quartile computation out) and the widget
//! only projects them. That keeps it deterministically headless-testable and
//! composes with the Elm `view(&self)` model like [`List`](crate::List).
//!
//! # Sub-cell thickness, the [`Gauge`](crate::Gauge) ramp
//!
//! A violin's value marks are positions (the [`BoxPlot`](crate::BoxPlot)
//! [`Block`]-glyph discipline) but its *thickness* is a fraction that rarely
//! lands on a whole cell — so, exactly like [`BarChart`](crate::BarChart) and
//! [`Gauge`](crate::Gauge), the boundary cell of the symmetric body is drawn
//! with the eighth-block glyph nearest the true density (the vertical ramp
//! `▁…█` for a horizontal violin's vertical thickness, the horizontal ramp
//! `▏…█` for a vertical violin's). Each glyph is one Unicode scalar, mapping
//! 1:1 onto a [`Cell`](rstui_core::Buffer) with no grapheme machinery.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no violins, an empty or all-zero density, a single violin, a
//! zero-span window (`min == max`), and an area too small for the label gutter
//! or the body are all safe clips/no-ops — never a panic, no division by zero.
//! An optional framing [`Block`] follows the container-widget convention.
//!
//! ```text
//! cargo run -p rstui-widgets --example violin_chart_demo
//! ```

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The eight bottom-aligned block elements for a **horizontal** violin's
/// vertical thickness edge, `1/8` … `8/8`.
const VERTICAL_EIGHTHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The eight left-aligned block elements for a **vertical** violin's
/// horizontal thickness edge, `1/8` … `8/8` (the [`Gauge`](crate::Gauge) ramp).
const HORIZONTAL_EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// One distribution's density profile, plus an optional median and a label.
///
/// `density` is sampled left→right across the value window (`density[0]` at
/// the window minimum, `density[len - 1]` at the maximum); the caller computes
/// it (a KDE or a histogram) — the widget never derives it and never reorders
/// it. Build the label from anything a [`Line`] is built from; style it
/// through the [`Line`] it wraps.
#[derive(Debug, Default, Clone)]
pub struct Violin<'a> {
    label: Line<'a>,
    density: Vec<f64>,
    median: Option<f64>,
}

impl<'a> Violin<'a> {
    /// A violin labelled `label` with the caller-computed `density` profile
    /// (sampled across the value window) and no median marker.
    pub fn new(label: impl Into<Line<'a>>, density: Vec<f64>) -> Self {
        Self {
            label: label.into(),
            density,
            median: None,
        }
    }

    /// Sets the median value, drawn as a contrasting tick across the body.
    #[must_use]
    pub fn median(mut self, median: f64) -> Self {
        self.median = Some(median);
        self
    }
}

/// Which way a [`ViolinChart`]'s violins are laid out.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViolinOrientation {
    /// Violins are horizontal bands stacked top to bottom, value running
    /// left→right, the label in a reserved left column (the default).
    #[default]
    Horizontal,
    /// Violins stand vertically left to right, value running bottom→top, the
    /// label in a reserved bottom row.
    Vertical,
}

/// A row/column of violin (density) plots over a shared value scale, with a
/// label gutter and an optional framing [`Block`].
///
/// Every [`Violin`]'s density is resampled onto the shared value axis (the
/// [`bounds`](Self::bounds) window, or `0..=density.len()` extent when unset)
/// and drawn as a symmetric body whose half-thickness tracks the density,
/// normalised per violin so each uses the full band (the common "max width"
/// scale). A label gutter (a left column when
/// [`Horizontal`](ViolinOrientation::Horizontal), a bottom row when
/// [`Vertical`](ViolinOrientation::Vertical)) carries each label, exactly like
/// [`BarChart`](crate::BarChart).
///
/// Styling is a base [`Style`] (filling the content area) with a
/// [`violin_style`](Self::violin_style) for the body and a
/// [`median_style`](Self::median_style) for the median tick, each over the
/// base; the label takes [`label_style`](Self::label_style) beneath its own
/// [`Line`] spans.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Violin, ViolinChart};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 3));
/// ViolinChart::new([Violin::new("a", vec![0.0, 1.0, 0.0])])
///     .bounds(Some([0.0, 2.0]))
///     .render(buf.area(), &mut buf);
///
/// // A left label gutter carries the label.
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'a');
/// ```
#[derive(Debug, Clone)]
pub struct ViolinChart<'a> {
    violins: Vec<Violin<'a>>,
    bounds: Option<[f64; 2]>,
    orientation: ViolinOrientation,
    block: Option<Block<'a>>,
    style: Style,
    violin_style: Style,
    median_style: Style,
    label_style: Style,
}

impl Default for ViolinChart<'_> {
    fn default() -> Self {
        Self {
            violins: Vec::new(),
            bounds: None,
            orientation: ViolinOrientation::Horizontal,
            block: None,
            style: Style::default(),
            violin_style: Style::default(),
            median_style: Style::default(),
            label_style: Style::default(),
        }
    }
}

impl<'a> ViolinChart<'a> {
    /// A horizontal violin chart of `violins`, auto-scaled to each density's
    /// sample extent, with no frame.
    pub fn new<I>(violins: I) -> Self
    where
        I: IntoIterator<Item = Violin<'a>>,
    {
        Self {
            violins: violins.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets the `[min, max]` value window the densities are sampled across, or
    /// `None` to span `0..=len` of the longest density.
    ///
    /// A zero-span window (`min == max`) collapses the axis onto one cell
    /// (never a panic — the [`Gauge`](crate::Gauge) totality rule).
    #[must_use]
    pub fn bounds(mut self, bounds: Option<[f64; 2]>) -> Self {
        self.bounds = bounds;
        self
    }

    /// Sets whether violins lie horizontally (default) or stand vertically.
    #[must_use]
    pub fn orientation(mut self, orientation: ViolinOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Frames the chart in `block`; violins render into [`block.inner`](Block::inner).
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

    /// Sets the [`Style`] for the violin body, over the base.
    #[must_use]
    pub fn violin_style(mut self, style: Style) -> Self {
        self.violin_style = style;
        self
    }

    /// Sets the [`Style`] for the median tick, over the base.
    #[must_use]
    pub fn median_style(mut self, style: Style) -> Self {
        self.median_style = style;
        self
    }

    /// Sets the base [`Style`] for labels, beneath each label's own spans.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
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

/// The density at axis fraction `t` (`0..=1`) by nearest-sample lookup;
/// `0.0` for an empty profile (no division by zero).
fn sample(density: &[f64], t: f64) -> f64 {
    if density.is_empty() {
        return 0.0;
    }
    let n = density.len();
    let idx = (t.clamp(0.0, 1.0) * (n as f64 - 1.0)).round() as usize;
    density[idx.min(n - 1)].max(0.0)
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

impl Widget for ViolinChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let ViolinChart {
            violins,
            bounds,
            orientation,
            block,
            style,
            violin_style,
            median_style,
            label_style,
        } = self;

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

        buf.set_style(inner, style);
        if violins.is_empty() {
            return;
        }

        // The value window: the caller's, or `0..=len` of the longest density.
        let (min, max) = match bounds {
            Some([lo, hi]) => (lo, hi),
            None => {
                let longest = violins.iter().map(|v| v.density.len()).max().unwrap_or(0);
                (0.0, longest.saturating_sub(1).max(1) as f64)
            }
        };

        let body_s = style.patch(violin_style);
        let median_s = style.patch(median_style);
        let label_s = style.patch(label_style);
        let n = violins.len() as u16;

        match orientation {
            ViolinOrientation::Horizontal => {
                let longest = violins.iter().map(|v| v.label.width()).max().unwrap_or(0) as u16;
                let gutter_w = longest.min(inner.width / 2);
                let plot_x0 = inner.left().saturating_add(gutter_w);
                let plot_w = inner.width.saturating_sub(gutter_w);
                let right = inner.right();
                let band_h = (inner.height / n).max(1);

                for (i, v) in violins.iter().enumerate() {
                    let band_top = inner.top().saturating_add(i as u16 * band_h);
                    if band_top >= inner.bottom() {
                        break;
                    }
                    let band_bot = band_top.saturating_add(band_h).min(inner.bottom());
                    let center = band_top + (band_bot - band_top) / 2;
                    // Half-thickness budget, in eighths of a cell.
                    let half_cells = (band_h / 2).max(1);
                    let max_e = u32::from(half_cells) * 8;
                    let dmax = v
                        .density
                        .iter()
                        .copied()
                        .fold(0.0_f64, |m, d| m.max(d.max(0.0)));

                    if gutter_w > 0 {
                        stamp_line(
                            buf,
                            &v.label,
                            label_s,
                            inner.left(),
                            center,
                            inner.left().saturating_add(gutter_w),
                        );
                    }
                    if plot_w == 0 || dmax <= 0.0 {
                        continue;
                    }
                    for cx in 0..plot_w {
                        let x = plot_x0.saturating_add(cx);
                        if x >= right {
                            break;
                        }
                        let t = if plot_w <= 1 {
                            0.0
                        } else {
                            f64::from(cx) / f64::from(plot_w - 1)
                        };
                        let frac = (sample(&v.density, t.clamp(0.0, 1.0)).min(dmax) / dmax)
                            .clamp(0.0, 1.0);
                        let e = (frac * f64::from(max_e)).round() as u32;
                        if e == 0 {
                            continue;
                        }
                        let full = (e / 8) as u16;
                        let rem = (e % 8) as u16;
                        // The spine plus full cells either side of centre.
                        buf.set_cell(Position::new(x, center), '█', body_s);
                        for k in 1..=full {
                            if center >= band_top + k {
                                buf.set_cell(Position::new(x, center - k), '█', body_s);
                            }
                            if center + k < band_bot {
                                buf.set_cell(Position::new(x, center + k), '█', body_s);
                            }
                        }
                        if rem > 0 {
                            let g = VERTICAL_EIGHTHS[(rem - 1) as usize];
                            if center > band_top + full {
                                buf.set_cell(Position::new(x, center - full - 1), g, body_s);
                            }
                            if center + full + 1 < band_bot {
                                buf.set_cell(Position::new(x, center + full + 1), g, body_s);
                            }
                        }
                    }
                    // The median tick across the band, drawn last.
                    if let Some(m) = v.median {
                        let x = plot_x0.saturating_add(place(m, min, max, plot_w));
                        if x < right {
                            for y in band_top..band_bot {
                                buf.set_cell(Position::new(x, y), '┃', median_s);
                            }
                        }
                    }
                }
            }
            ViolinOrientation::Vertical => {
                let label_row = inner.height > 1;
                let plot_h = inner.height.saturating_sub(u16::from(label_row));
                let label_y = inner.bottom().saturating_sub(1);
                let right = inner.right();
                let band_w = (inner.width / n).max(1);

                for (i, v) in violins.iter().enumerate() {
                    let band_left = inner.left().saturating_add(i as u16 * band_w);
                    if band_left >= right {
                        break;
                    }
                    let band_right = band_left.saturating_add(band_w).min(right);
                    let center = band_left + (band_right - band_left) / 2;
                    let half_cells = (band_w / 2).max(1);
                    let max_e = u32::from(half_cells) * 8;
                    let dmax = v
                        .density
                        .iter()
                        .copied()
                        .fold(0.0_f64, |m, d| m.max(d.max(0.0)));

                    if plot_h > 0 && dmax > 0.0 {
                        for cy in 0..plot_h {
                            // Bottom row is the window min, top row the max.
                            let y = inner.top() + (plot_h - 1 - cy);
                            let t = if plot_h <= 1 {
                                0.0
                            } else {
                                f64::from(cy) / f64::from(plot_h - 1)
                            };
                            let frac = (sample(&v.density, t).min(dmax) / dmax).clamp(0.0, 1.0);
                            let e = (frac * f64::from(max_e)).round() as u32;
                            if e == 0 {
                                continue;
                            }
                            let full = (e / 8) as u16;
                            let rem = (e % 8) as u16;
                            buf.set_cell(Position::new(center, y), '█', body_s);
                            for k in 1..=full {
                                if center >= band_left + k {
                                    buf.set_cell(Position::new(center - k, y), '█', body_s);
                                }
                                if center + k < band_right {
                                    buf.set_cell(Position::new(center + k, y), '█', body_s);
                                }
                            }
                            if rem > 0 {
                                let g = HORIZONTAL_EIGHTHS[(rem - 1) as usize];
                                if center > band_left + full {
                                    buf.set_cell(Position::new(center - full - 1, y), g, body_s);
                                }
                                if center + full + 1 < band_right {
                                    buf.set_cell(Position::new(center + full + 1, y), g, body_s);
                                }
                            }
                        }
                        if let Some(m) = v.median {
                            let from_bottom = place(m, min, max, plot_h);
                            let y = inner.top() + plot_h.saturating_sub(1) - from_bottom;
                            for x in band_left..band_right {
                                buf.set_cell(Position::new(x, y), '━', median_s);
                            }
                        }
                    }
                    if label_row {
                        stamp_line(buf, &v.label, label_s, band_left, label_y, right);
                    }
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
    fn a_horizontal_violin_is_symmetric_about_its_band_centre() {
        // 1 violin, band 3 tall, a single full-density column: the centre row
        // and one row each side fill — a symmetric 3-cell body.
        let v = ViolinChart::new([Violin::new("a", vec![1.0])]).bounds(Some([0.0, 1.0]));
        let out = lines(v, 2, 3);
        let rows: Vec<&str> = out.lines().collect();
        // gutter col 0 carries 'a' on the centre row; col 1 is the body.
        assert_eq!(rows[1].chars().next().unwrap(), 'a');
        assert_eq!(rows[0].chars().nth(1).unwrap(), '█');
        assert_eq!(rows[1].chars().nth(1).unwrap(), '█');
        assert_eq!(rows[2].chars().nth(1).unwrap(), '█');
    }

    #[test]
    fn an_all_zero_density_draws_only_the_label() {
        let v = ViolinChart::new([Violin::new("z", vec![0.0, 0.0, 0.0])]).bounds(Some([0.0, 2.0]));
        let out = lines(v, 6, 3);
        assert!(out.contains('z'));
        assert!(!out.contains('█'));
    }

    #[test]
    fn an_empty_density_is_total() {
        let v = ViolinChart::new([Violin::new("e", Vec::new())]);
        // No panic, no body glyphs, label still shows.
        let out = lines(v, 6, 3);
        assert!(out.contains('e'));
        assert!(!out.contains('█'));
    }

    #[test]
    fn the_median_tick_spans_the_band() {
        let v = ViolinChart::new([Violin::new("m", vec![1.0, 1.0, 1.0]).median(1.0)])
            .bounds(Some([0.0, 2.0]));
        let out = lines(v, 8, 3);
        assert!(out.contains('┃'), "median tick must render:\n{out}");
    }

    #[test]
    fn a_zero_span_window_collapses_without_panicking() {
        let v = ViolinChart::new([Violin::new("s", vec![1.0, 2.0, 1.0])]).bounds(Some([5.0, 5.0]));
        assert_eq!(lines(v, 6, 3).lines().count(), 3);
    }

    #[test]
    fn multiple_violins_stack_each_in_its_own_band() {
        let v = ViolinChart::new([Violin::new("a", vec![1.0]), Violin::new("b", vec![1.0])])
            .bounds(Some([0.0, 1.0]));
        let out = lines(v, 3, 6);
        // Each label appears once, in a different band.
        assert_eq!(out.matches('a').count(), 1);
        assert_eq!(out.matches('b').count(), 1);
    }

    #[test]
    fn a_vertical_violin_stands_with_a_bottom_label_row() {
        let v = ViolinChart::new([Violin::new("v", vec![0.0, 1.0, 0.0])])
            .bounds(Some([0.0, 2.0]))
            .orientation(ViolinOrientation::Vertical);
        let out = lines(v, 3, 5);
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[4].contains('v')); // label on the bottom row
        assert!(out.contains('█')); // a body somewhere above
    }

    #[test]
    fn a_block_frames_the_chart_in_the_inner_area() {
        let v = ViolinChart::new([Violin::new("", vec![1.0])])
            .bounds(Some([0.0, 1.0]))
            .block(Block::bordered());
        let out = lines(v, 5, 3);
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[0].starts_with('┌'));
        assert!(rows[2].starts_with('└'));
    }

    #[test]
    fn no_violins_with_a_block_still_renders_the_block() {
        let v = ViolinChart::new(Vec::<Violin>::new()).block(Block::bordered());
        assert_eq!(lines(v, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn the_density_shapes_the_body_widest_where_densest() {
        // Triangular density peaking in the middle column: the centre column
        // is taller (more filled rows) than the edges.
        let v = ViolinChart::new([Violin::new("p", vec![0.0, 1.0, 0.0])]).bounds(Some([0.0, 2.0]));
        let out = lines(v, 4, 5); // gutter 1, plot 3 wide, band 5 tall
        let rows: Vec<&str> = out.lines().collect();
        let col_fill = |c: usize| {
            rows.iter()
                .filter(|r| r.chars().nth(c) == Some('█'))
                .count()
        };
        // The middle plot column (index 2) is the densest.
        assert!(col_fill(2) >= col_fill(1));
        assert!(col_fill(2) >= col_fill(3));
    }

    #[test]
    fn style_cascades_base_then_part_styles() {
        let v = ViolinChart::new([Violin::new(
            Line::from(Span::styled("L", Style::new().fg(Color::Red))),
            vec![1.0],
        )
        .median(0.5)])
        .bounds(Some([0.0, 1.0]))
        .style(Style::new().bg(Color::Blue))
        .violin_style(Style::new().fg(Color::Green))
        .median_style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        v.render(buf.area(), &mut buf);
        // Label keeps its own span fg over the base bg.
        let l = buf.get(Position::new(0, 1)).unwrap();
        assert_eq!(l.symbol, 'L');
        assert_eq!(l.fg, Color::Red);
        assert_eq!(l.bg, Color::Blue);
        // A body cell takes violin_style fg over the base bg.
        let mut found = false;
        for x in 0..4 {
            for y in 0..3 {
                let c = buf.get(Position::new(x, y)).unwrap();
                if c.symbol == '█' {
                    assert_eq!(c.fg, Color::Green);
                    assert_eq!(c.bg, Color::Blue);
                    found = true;
                }
            }
        }
        assert!(found, "a body cell must render");
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let v = ViolinChart::new([Violin::new("p", vec![1.0])]).bounds(Some([0.0, 1.0]));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 5));
        v.render(Rect::new(2, 1, 4, 3), &mut buf);
        assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, 'p');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn a_tiny_area_with_no_body_room_still_draws_the_label() {
        let v = ViolinChart::new([Violin::new("ab", vec![1.0, 2.0])]).bounds(Some([0.0, 1.0]));
        let _ = lines(v, 2, 1); // invariant: no panic
    }

    #[test]
    fn auto_scale_spans_the_density_length() {
        // No bounds: the window is 0..=len-1; a peak at the last sample lands
        // at the rightmost plot column.
        let v = ViolinChart::new([Violin::new("a", vec![0.0, 0.0, 1.0])]);
        let out = lines(v, 4, 3);
        let rows: Vec<&str> = out.lines().collect();
        // Rightmost plot column (index 3) carries body; the left edge does not.
        assert_eq!(rows[1].chars().nth(3).unwrap(), '█');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        ViolinChart::new([Violin::new("x", vec![1.0])]).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
