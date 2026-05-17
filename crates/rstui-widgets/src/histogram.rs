//! [`Histogram`] — a bucketed value-distribution chart with percentile marker
//! overlays, the observability primitive for "how is this measurement
//! distributed" (request latency by bucket, response size by bucket, GC pause
//! by bucket) with p50/p95/p99 lines drawn over it.
//!
//! # A pure projection, like every other widget
//!
//! `Histogram` owns no state. It is a borrowed slice of caller-built
//! [`HistogramBucket`]s (a count plus a boundary-label [`Line`]) plus an
//! optional ceiling and an optional borrowed slice of [`Percentile`]s; the
//! reducer decides what the distribution is (the bucket counts it accumulates
//! in `update`) and the widget only projects "the distribution right now".
//! That keeps it deterministically headless-testable and composes with the Elm
//! `view(&self)` model exactly like [`List`](crate::List) and
//! [`Gauge`](crate::Gauge).
//!
//! # Distinct from [`BarChart`](crate::BarChart)
//!
//! [`BarChart`](crate::BarChart) is *categorical*: a handful of unordered
//! values (commits by author, disk by mount). A histogram's buckets are an
//! *ordered* distribution over a measured range, so it carries one thing a bar
//! chart does not — [`percentiles`](Histogram::percentiles): a vertical marker line
//! drawn at the bucket whose running cumulative count first crosses a fraction
//! of the total (the p50/p95/p99 an SLO is read off). That overlay is the
//! reason this is its own widget and not a [`BarChart`](crate::BarChart) mode.
//!
//! # Sub-cell precision, reusing the [`Gauge`](crate::Gauge) idea
//!
//! A bucket's bar end rarely lands on a whole cell, so — exactly like
//! [`Gauge`](crate::Gauge) and [`BarChart`](crate::BarChart) — the boundary
//! cell is drawn with the *vertical* eighth-block glyph nearest the true
//! fraction (`▁▂▃▄▅▆▇█`), not rounded to a whole cell. Each glyph is one
//! Unicode scalar, so it maps 1:1 onto a [`Cell`](rstui_core::Buffer) with no
//! grapheme machinery — the same reasoning the gauge ramp and
//! [`Block`] borders use.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no buckets, an all-zero distribution, a count above the ceiling
//! (clamped), a percentile fraction outside `0..=1` (clamped), a zero total (no
//! markers), and an area too narrow/short for the bars, labels, or marker
//! labels are all safe clips/no-ops — never a panic. An optional framing
//! [`Block`] follows the container-widget convention.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The eight bottom-aligned block elements, `1/8` … `8/8` tall (the same
/// vertical ramp [`BarChart`](crate::BarChart) fills its vertical bars with).
const VERTICAL_EIGHTHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The vertical-bar glyph a [`Percentile`] marker column is drawn with.
const MARKER_GLYPH: char = '│';

/// One bucket of a [`Histogram`]: how many samples fell in it and the boundary
/// it represents.
///
/// The `label` is the bucket *boundary* (e.g. `"≤25ms"`), not a category name;
/// build it from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`) and style it through the
/// [`Line`] it wraps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HistogramBucket<'a> {
    /// How many samples fell into this bucket.
    pub count: u64,
    /// The bucket boundary, drawn on the reserved bottom label row.
    pub label: Line<'a>,
}

impl<'a> HistogramBucket<'a> {
    /// A bucket holding `count` samples, labelled with the boundary `label`
    /// (anything convertible to a [`Line`]).
    pub fn new(count: u64, label: impl Into<Line<'a>>) -> Self {
        Self {
            count,
            label: label.into(),
        }
    }
}

/// A vertical marker line for a [`Histogram`], drawn at the bucket whose
/// running cumulative count first reaches `fraction` of the total.
///
/// `fraction` is clamped to `0..=1` at render time, so `0.5`/`0.95`/`0.99` are
/// the p50/p95/p99 an SLO is read off. The `label` (e.g. `"p95"`) is drawn at
/// the top of the marker column when there is room; build it from anything a
/// [`Line`] is built from. The `style` is the marker column's [`Style`].
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Percentile<'a> {
    /// The cumulative fraction of the total this marker sits at, clamped to
    /// `0..=1` at render time.
    pub fraction: f64,
    /// The marker label (e.g. `"p95"`), drawn at the top of the column if it
    /// fits.
    pub label: Line<'a>,
    /// The [`Style`] the marker column and its label are drawn with.
    pub style: Style,
}

impl<'a> Percentile<'a> {
    /// A percentile marker at cumulative `fraction` (clamped to `0..=1` when
    /// rendered) labelled `label` (anything convertible to a [`Line`]),
    /// unstyled.
    pub fn new(fraction: f64, label: impl Into<Line<'a>>) -> Self {
        Self {
            fraction,
            label: label.into(),
            style: Style::default(),
        }
    }

    /// Sets the [`Style`] the marker column and its label are drawn with.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

/// A bucketed value-distribution chart with sub-cell precision, optional
/// percentile marker overlays, and an optional framing [`Block`].
///
/// Buckets are drawn in [`bar_width`](Self::bar_width)-wide columns separated
/// by [`bar_gap`](Self::bar_gap) (mirroring [`BarChart`](crate::BarChart)'s
/// layout); each count is scaled against [`max`](Self::max) (the largest count
/// when unset) and drawn with full blocks plus one fractional eighth-block
/// boundary cell. The bottom inner row is reserved for the bucket boundary
/// labels. [`percentiles`](Self::percentiles) overlay vertical marker columns
/// at the bucket whose running cumulative count first crosses each fraction.
/// Styling is a base [`Style`] (filling the area) with a
/// [`bar_style`](Self::bar_style) for the glyphs and a
/// [`label_style`](Self::label_style) beneath each label's own
/// [`Line`]/[`Span`](rstui_core::Span) styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Histogram, HistogramBucket, Percentile};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 3, 3));
/// let buckets = [
///     HistogramBucket::new(2, "a"),
///     HistogramBucket::new(8, "b"),
/// ];
/// let pcts = [Percentile::new(0.5, "")];
/// Histogram::new(&buckets)
///     .max(Some(8))
///     .bar_width(1)
///     .bar_gap(1)
///     .percentiles(&pcts)
///     .render(buf.area(), &mut buf);
///
/// // Two 1-wide bars; the label row is the bottom inner row. The cumulative
/// // count reaches half (5 of 10) inside bucket 1, so the p50 marker column
/// // sits over that bar's column (x = 2).
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, '│'); // p50 marker
/// assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, 'a'); // label
/// ```
#[derive(Debug, Clone)]
pub struct Histogram<'a> {
    buckets: &'a [HistogramBucket<'a>],
    percentiles: &'a [Percentile<'a>],
    max: Option<u64>,
    bar_width: u16,
    bar_gap: u16,
    block: Option<Block<'a>>,
    style: Style,
    bar_style: Style,
    label_style: Style,
}

impl Default for Histogram<'_> {
    fn default() -> Self {
        Self {
            buckets: &[],
            percentiles: &[],
            max: None,
            // Three-cell bars with a one-cell gap: wide enough for a boundary
            // label and never visually merging adjacent buckets
            // ([`BarChart`](crate::BarChart)'s default reasoning).
            bar_width: 3,
            bar_gap: 1,
            block: None,
            style: Style::default(),
            bar_style: Style::default(),
            label_style: Style::default(),
        }
    }
}

impl<'a> Histogram<'a> {
    /// A histogram of `buckets`, auto-scaled to the largest count, with
    /// three-cell bars, one-cell gaps, no markers, and no frame.
    #[must_use]
    pub fn new(buckets: &'a [HistogramBucket<'a>]) -> Self {
        Self {
            buckets,
            ..Self::default()
        }
    }

    /// Sets the count mapped to a full-height bar, or `None` to auto-scale to
    /// the largest count.
    ///
    /// A count above the ceiling is clamped (never a panic — the
    /// [`Gauge`](crate::Gauge) totality rule).
    #[must_use]
    pub fn max(mut self, max: Option<u64>) -> Self {
        self.max = max;
        self
    }

    /// Sets the thickness in columns of each bucket bar (default `3`). Clamped
    /// to at least `1` at render time.
    #[must_use]
    pub fn bar_width(mut self, bar_width: u16) -> Self {
        self.bar_width = bar_width;
        self
    }

    /// Sets the blank gap between adjacent bucket bars (default `1`).
    #[must_use]
    pub fn bar_gap(mut self, bar_gap: u16) -> Self {
        self.bar_gap = bar_gap;
        self
    }

    /// Overlays vertical marker columns at the bucket whose running cumulative
    /// count first reaches each [`Percentile`]'s fraction.
    ///
    /// Each marker is drawn with the percentile's own [`Style`], with its label
    /// at the top of the column if it fits (a total no-op when there is no
    /// room). With a zero total no markers are drawn.
    #[must_use]
    pub fn percentiles(mut self, percentiles: &'a [Percentile<'a>]) -> Self {
        self.percentiles = percentiles;
        self
    }

    /// Frames the chart in `block`; bars render into [`block.inner`](Block::inner).
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

    /// Sets the [`Style`] the bar glyphs are drawn with, over the base.
    #[must_use]
    pub fn bar_style(mut self, style: Style) -> Self {
        self.bar_style = style;
        self
    }

    /// Sets the base [`Style`] for labels, beneath each label's own
    /// [`Line`]/[`Span`](rstui_core::Span) styles.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }
}

/// The number of eighths a `value` fills of a `span`-cell axis against
/// `ceiling` (rounded to the nearest eighth; `ceiling` is already `>= 1`).
fn eighths(value: u64, ceiling: u64, span: u16) -> u64 {
    let clamped = u128::from(value.min(ceiling));
    let total = u128::from(span) * 8;
    ((clamped * total + u128::from(ceiling) / 2) / u128::from(ceiling)) as u64
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

/// The index of the bucket whose running cumulative count first reaches
/// `fraction` of `total` (`total` is already `>= 1`; `fraction` clamped to
/// `0..=1`).
fn percentile_bucket(buckets: &[HistogramBucket], fraction: f64, total: u64) -> usize {
    let fraction = fraction.clamp(0.0, 1.0);
    // The integer threshold the cumulative count must reach: ceil(f * total),
    // computed in u128 so the multiply never overflows and floats never decide
    // the index.
    let scaled = (fraction * total as f64).ceil();
    let threshold = if scaled <= 0.0 {
        0
    } else if scaled >= total as f64 {
        total
    } else {
        scaled as u64
    };
    let mut cumulative: u64 = 0;
    for (i, bucket) in buckets.iter().enumerate() {
        cumulative = cumulative.saturating_add(bucket.count);
        if cumulative >= threshold {
            return i;
        }
    }
    buckets.len().saturating_sub(1)
}

impl Widget for Histogram<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Histogram {
            buckets,
            percentiles,
            max,
            bar_width,
            bar_gap,
            block,
            style,
            bar_style,
            label_style,
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
        if buckets.is_empty() {
            return;
        }

        // The ceiling: the caller's, or the largest count, never below 1 so
        // the scale math is total (an all-zero distribution renders empty).
        let ceiling = max
            .or_else(|| buckets.iter().map(|b| b.count).max())
            .unwrap_or(0)
            .max(1);
        let bar_w = bar_width.max(1);
        let bar_glyph = style.patch(bar_style);

        // The bottom inner row is the label row (when there is more than one
        // row); bars rise in the rows above it.
        let label_row = inner.height > 1;
        let bar_rows = inner.height.saturating_sub(u16::from(label_row));
        let label_y = inner.bottom().saturating_sub(1);
        let right = inner.right();

        // Each bucket's column origin, recorded so a percentile marker can land
        // on the same column the bar occupies.
        let mut bucket_x = vec![0u16; buckets.len()];
        let mut bucket_drawn = vec![false; buckets.len()];

        let mut x0 = inner.left();
        for (i, bucket) in buckets.iter().enumerate() {
            if x0 >= right {
                break;
            }
            bucket_x[i] = x0;
            bucket_drawn[i] = true;
            let total_e = eighths(bucket.count, ceiling, bar_rows);
            let full = (total_e / 8) as u16;
            let rem = (total_e % 8) as u16;
            let group_right = x0.saturating_add(bar_w).min(right);

            for x in x0..group_right {
                // Full blocks from the baseline up.
                for r in 0..full {
                    let y = inner.top().saturating_add(bar_rows - 1 - r);
                    buf.set_cell(Position::new(x, y), '█', bar_glyph);
                }
                // One fractional boundary cell above the full run.
                if rem > 0 && full < bar_rows {
                    let y = inner.top().saturating_add(bar_rows - 1 - full);
                    buf.set_cell(
                        Position::new(x, y),
                        VERTICAL_EIGHTHS[(rem - 1) as usize],
                        bar_glyph,
                    );
                }
            }

            // Boundary label centred under the bar group, clipped to it.
            if label_row {
                let lw = (bucket.label.width() as u16).min(bar_w);
                let lx = x0.saturating_add((bar_w - lw) / 2);
                stamp_line(
                    buf,
                    &bucket.label,
                    style.patch(label_style),
                    lx,
                    label_y,
                    group_right,
                );
            }
            x0 = group_right.saturating_add(bar_gap);
        }

        // Percentile marker overlay: a vertical column at the bucket whose
        // running cumulative count first crosses the fraction. A zero total
        // means no markers (the distribution is empty).
        let total: u64 = buckets
            .iter()
            .fold(0u64, |acc, b| acc.saturating_add(b.count));
        if total == 0 || percentiles.is_empty() {
            return;
        }
        for percentile in percentiles {
            let idx = percentile_bucket(buckets, percentile.fraction, total);
            // Only buckets that actually fit on screen were drawn; skip a
            // marker whose bucket was clipped past the right edge.
            if !bucket_drawn[idx] {
                continue;
            }
            let marker_style = style.patch(percentile.style);
            let mx = bucket_x[idx];
            if mx >= right {
                continue;
            }
            // The marker spans the bar rows only; the label row stays the
            // boundary labels'.
            for r in 0..bar_rows {
                let y = inner.top().saturating_add(r);
                buf.set_cell(Position::new(mx, y), MARKER_GLYPH, marker_style);
            }
            // The label sits on the top row when it fits within the bar group.
            if percentile.label.width() > 0 && bar_rows > 0 {
                let group_right = mx.saturating_add(bar_w).min(right);
                if percentile.label.width() as u16 <= group_right.saturating_sub(mx) {
                    stamp_line(
                        buf,
                        &percentile.label,
                        marker_style,
                        mx,
                        inner.top(),
                        group_right,
                    );
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
    fn buckets_rise_from_a_baseline_with_a_label_row() {
        let buckets = [HistogramBucket::new(8, "a"), HistogramBucket::new(4, "b")];
        let hist = Histogram::new(&buckets)
            .max(Some(8))
            .bar_gap(0)
            .bar_width(1);
        // 3 tall: 2 bar rows + 1 label row. a=8/8 → both rows full; b=4/8 →
        // 1.0 row → one full row at the baseline.
        assert_eq!(lines(hist, 2, 3), "█ \n██\nab\n");
    }

    #[test]
    fn a_fractional_bucket_uses_a_sub_cell_glyph() {
        // count 1, ceiling 2, 1 bar row → 0.5 row → 4 eighths → ▄.
        let buckets = [HistogramBucket::new(1, "x")];
        let hist = Histogram::new(&buckets).max(Some(2)).bar_width(1);
        assert_eq!(lines(hist, 1, 2), "▄\nx\n");
    }

    #[test]
    fn the_bar_gap_separates_buckets() {
        let buckets = [HistogramBucket::new(8, "a"), HistogramBucket::new(8, "b")];
        let hist = Histogram::new(&buckets)
            .max(Some(8))
            .bar_gap(1)
            .bar_width(1);
        // 1-wide bars, a 1-wide gap between them, label row at the bottom.
        assert_eq!(lines(hist, 3, 2), "█ █\na b\n");
    }

    #[test]
    fn bar_width_thickens_each_bucket() {
        let buckets = [HistogramBucket::new(8, "ab")];
        let hist = Histogram::new(&buckets)
            .max(Some(8))
            .bar_width(2)
            .bar_gap(0);
        assert_eq!(lines(hist, 2, 2), "██\nab\n");
    }

    #[test]
    fn a_count_above_the_ceiling_clamps_to_a_full_bar() {
        let buckets = [HistogramBucket::new(999, "x")];
        let hist = Histogram::new(&buckets)
            .max(Some(8))
            .bar_gap(0)
            .bar_width(1);
        assert_eq!(lines(hist, 1, 2), "█\nx\n");
    }

    #[test]
    fn an_all_zero_distribution_draws_no_bars() {
        let buckets = [HistogramBucket::new(0, "a"), HistogramBucket::new(0, "b")];
        let hist = Histogram::new(&buckets).bar_gap(0).bar_width(1);
        // No ceiling, all zero → ceiling floors at 1, every bar empty; only
        // the labels show.
        assert_eq!(lines(hist, 2, 2), "  \nab\n");
    }

    #[test]
    fn auto_scale_maps_the_largest_count_to_a_full_bar() {
        let buckets = [HistogramBucket::new(10, "a"), HistogramBucket::new(5, "b")];
        let hist = Histogram::new(&buckets).bar_gap(0).bar_width(1);
        // max = 10 → a full (2 rows), b half (1 row).
        assert_eq!(lines(hist, 2, 3), "█ \n██\nab\n");
    }

    #[test]
    fn a_percentile_marker_lands_in_its_cumulative_bucket() {
        // Totals 10; cumulative reaches 5 (p50 threshold) inside bucket 1
        // (count 2 then 10), so the marker sits in bucket 1's column. Bucket 0
        // (2/8 → ▄) keeps its own column; the marker overwrites bucket 1's bar.
        let buckets = [HistogramBucket::new(2, "a"), HistogramBucket::new(8, "b")];
        let pcts = [Percentile::new(0.5, "")];
        let hist = Histogram::new(&buckets)
            .max(Some(8))
            .bar_gap(0)
            .bar_width(1)
            .percentiles(&pcts);
        assert_eq!(lines(hist, 2, 3), " │\n▄│\nab\n");
    }

    #[test]
    fn a_percentile_label_sits_at_the_top_when_it_fits() {
        let buckets = [HistogramBucket::new(1, "a"), HistogramBucket::new(9, "b")];
        let pcts = [Percentile::new(0.99, "p")];
        let hist = Histogram::new(&buckets)
            .max(Some(9))
            .bar_gap(0)
            .bar_width(1)
            .percentiles(&pcts);
        // p99 falls in bucket 1 (cumulative 1 then 10, threshold 10); the
        // 1-wide label "p" fits and overwrites the top marker cell of that
        // column. Bucket 0 (1/9 → ▂) keeps its own column.
        assert_eq!(lines(hist, 2, 3), " p\n▂│\nab\n");
    }

    #[test]
    fn a_zero_total_draws_no_markers() {
        let buckets = [HistogramBucket::new(0, "a"), HistogramBucket::new(0, "b")];
        let pcts = [Percentile::new(0.5, "p")];
        let hist = Histogram::new(&buckets)
            .bar_gap(0)
            .bar_width(1)
            .percentiles(&pcts);
        // Empty distribution → no bars and no markers, only labels.
        assert_eq!(lines(hist, 2, 2), "  \nab\n");
    }

    #[test]
    fn a_fraction_outside_zero_to_one_is_clamped() {
        let buckets = [HistogramBucket::new(5, "a"), HistogramBucket::new(5, "b")];
        // fraction 2.0 clamps to 1.0 → the last bucket; -1.0 clamps to 0.0 →
        // the first bucket.
        let high = [Percentile::new(2.0, "")];
        let low = [Percentile::new(-1.0, "")];
        let h1 = Histogram::new(&buckets)
            .max(Some(5))
            .bar_gap(0)
            .bar_width(1)
            .percentiles(&high);
        assert_eq!(lines(h1, 2, 3), "█│\n█│\nab\n");
        let h2 = Histogram::new(&buckets)
            .max(Some(5))
            .bar_gap(0)
            .bar_width(1)
            .percentiles(&low);
        assert_eq!(lines(h2, 2, 3), "│█\n│█\nab\n");
    }

    #[test]
    fn a_block_frames_the_chart_in_the_inner_area() {
        let buckets = [HistogramBucket::new(8, "x")];
        let hist = Histogram::new(&buckets)
            .max(Some(8))
            .bar_width(1)
            .block(Block::bordered());
        // inner 1×1 → only the bar row fits (no label row), one full block.
        assert_eq!(lines(hist, 3, 3), "┌─┐\n│█│\n└─┘\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_bars() {
        let buckets = [HistogramBucket::new(8, "x")];
        let hist = Histogram::new(&buckets).block(Block::bordered());
        assert_eq!(lines(hist, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn no_buckets_with_a_block_still_renders_the_block() {
        let buckets: [HistogramBucket; 0] = [];
        let hist = Histogram::new(&buckets).block(Block::bordered());
        assert_eq!(lines(hist, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn style_cascades_base_then_bar_and_label_styles() {
        let buckets = [HistogramBucket::new(
            8,
            Line::from(Span::styled("L", Style::new().fg(Color::Red))),
        )];
        let hist = Histogram::new(&buckets)
            .max(Some(8))
            .bar_gap(0)
            .bar_width(1)
            .style(Style::new().bg(Color::Blue))
            .bar_style(Style::new().fg(Color::Green))
            .label_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 2));
        hist.render(buf.area(), &mut buf);

        let g = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(g.symbol, '█');
        assert_eq!(g.fg, Color::Green); // bar_style fg
        assert_eq!(g.bg, Color::Blue); // base fill cascades

        let l = buf.get(Position::new(0, 1)).unwrap();
        assert_eq!(l.symbol, 'L');
        assert_eq!(l.fg, Color::Red); // span fg wins
        assert!(l.modifier.contains(Modifier::BOLD)); // label_style cascades
        assert_eq!(l.bg, Color::Blue);
    }

    #[test]
    fn a_marker_carries_the_percentile_style() {
        let buckets = [HistogramBucket::new(5, "a"), HistogramBucket::new(5, "b")];
        let pcts = [Percentile::new(0.5, "").style(Style::new().fg(Color::Yellow))];
        let hist = Histogram::new(&buckets)
            .max(Some(5))
            .bar_gap(0)
            .bar_width(1)
            .percentiles(&pcts);
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 3));
        hist.render(buf.area(), &mut buf);
        let m = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(m.symbol, '│');
        assert_eq!(m.fg, Color::Yellow);
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let buckets = [HistogramBucket::new(8, "a")];
        let hist = Histogram::new(&buckets)
            .max(Some(8))
            .bar_width(1)
            .bar_gap(0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        hist.render(Rect::new(2, 1, 1, 2), &mut buf);
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, '█');
        assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, 'a');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let buckets = [HistogramBucket::new(5, "x")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Histogram::new(&buckets).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
