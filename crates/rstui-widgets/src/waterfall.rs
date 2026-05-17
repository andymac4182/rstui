//! [`Waterfall`] — a financial *bridge* (variance) chart: a running cumulative
//! where each step's bar **floats** from the previous running total to the new
//! one, the way a P&L walk goes "revenue → minus COGS → minus opex → operating
//! income" or a sales dashboard bridges "opening pipeline → +new → −slipped →
//! −lost → closing". Rises and falls are coloured differently and [`Total`]
//! steps are full absolute bars from the baseline, so the eye reads the
//! contribution of each line item *and* the subtotal it lands on at a glance.
//!
//! [`Total`]: WaterfallKind::Total
//!
//! # A pure projection, like every other widget
//!
//! `Waterfall` owns no state. It is a list of caller-built [`WaterfallStep`]s
//! (a label [`Line`], a signed `i64` delta, and whether the step is a delta or
//! a [`Total`](WaterfallKind::Total)) plus an optional ceiling; the reducer
//! decides what the steps are (it computes the variance bridge from the model)
//! and the widget only projects them. That keeps it deterministically
//! headless-testable and composes with the Elm `view(&self)` model exactly
//! like [`List`](crate::List) and [`BarChart`](crate::BarChart).
//!
//! # Sub-cell precision, reusing the [`Gauge`](crate::Gauge) idea
//!
//! A floating bar's leading end rarely lands on a whole cell, so — exactly
//! like [`BarChart`](crate::BarChart) and [`Gauge`](crate::Gauge) — that
//! boundary cell is drawn with the eighth-block glyph nearest the true
//! fraction (the *vertical* ramp `▁…█` for vertical bars, the *horizontal*
//! ramp `▏…█` for horizontal ones), not rounded to a whole cell. Each glyph is
//! one Unicode scalar, so it maps 1:1 onto a [`Cell`](rstui_core::Buffer) with
//! no grapheme machinery — the same reasoning the gauge ramp and [`Block`]
//! borders use. The bottom-aligned ramp can only fill a cell *from the axis
//! origin*, so a floating segment keeps eighth precision on its leading edge
//! (the salient end the eye lands on) and snaps its trailing edge to the
//! nearest whole cell; thin connector glyphs join consecutive steps at the
//! cumulative level so the "bridge" reads as one continuous walk.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no steps, an all-zero walk, a single step, a cumulative point above
//! the ceiling (clamped), and an area too narrow/short for the bars or labels
//! are all safe clips/no-ops — never a panic. An optional framing [`Block`]
//! follows the container-widget convention; per-step value annotations are a
//! deliberately deferred additive follow-up, not smuggled into this slice.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The eight bottom-aligned block elements for **vertical** bars, `1/8` … `8/8`.
const VERTICAL_EIGHTHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The eight left-aligned block elements for **horizontal** bars, `1/8` … `8/8`
/// (the same ramp [`Gauge`](crate::Gauge) fills its bar with).
const HORIZONTAL_EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// The thin connector glyph that joins one step's cumulative level to the next.
const CONNECTOR: char = '·';

/// Whether a [`WaterfallStep`] is an incremental delta or an absolute subtotal.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaterfallKind {
    /// An incremental change: the bar floats from the previous running
    /// cumulative to the new one and the running cumulative advances by the
    /// step's delta (the default — a P&L line item).
    #[default]
    Delta,
    /// An absolute subtotal: the bar is a full bar from the baseline up to the
    /// current running cumulative, and the running cumulative is **not**
    /// advanced (the step's delta is ignored — an "operating income"
    /// rule-off).
    Total,
}

/// One step of a [`Waterfall`]: a label [`Line`], a signed `i64` delta, and
/// whether it is an incremental [`Delta`](WaterfallKind::Delta) or an absolute
/// [`Total`](WaterfallKind::Total).
///
/// Build the label from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`); style it through the
/// [`Line`] it wraps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WaterfallStep<'a> {
    label: Line<'a>,
    delta: i64,
    kind: WaterfallKind,
}

impl<'a> WaterfallStep<'a> {
    /// An incremental step that changes the running cumulative by `value`
    /// (negative for a fall), labelled `label`.
    pub fn delta(value: i64, label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            delta: value,
            kind: WaterfallKind::Delta,
        }
    }

    /// An absolute subtotal bar from the baseline to the running cumulative so
    /// far, labelled `label`. The running cumulative is not advanced by a
    /// total (it carries no delta of its own).
    pub fn total(label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            delta: 0,
            kind: WaterfallKind::Total,
        }
    }
}

/// Which way a [`Waterfall`]'s bars grow.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaterfallDirection {
    /// Bars are columns placed left to right with their labels on the bottom
    /// row; the cumulative axis is vertical (the default).
    #[default]
    Vertical,
    /// Bars are rows stacked top to bottom with their labels in a reserved
    /// left column; the cumulative axis is horizontal.
    Horizontal,
}

/// A financial bridge/variance waterfall with sub-cell precision and an
/// optional framing [`Block`].
///
/// Steps are placed in equal-width groups along the category axis. The running
/// cumulative starts at `0`; each [`Delta`](WaterfallKind::Delta) step's bar
/// floats from the previous cumulative to the new one (rises and falls styled
/// differently) and advances the cumulative, while a
/// [`Total`](WaterfallKind::Total) step draws a full bar from the baseline to
/// the current cumulative. The value axis spans the minimum and maximum of the
/// cumulative path together with `0`; [`max`](Self::max) raises the visible
/// ceiling (a point above it is clamped, never a panic). The leading bar end
/// uses one fractional eighth-block boundary cell and thin connector glyphs
/// join consecutive steps. Styling is a base [`Style`] (filling the area) with
/// rise/fall/total bar styles, a connector style, and a
/// [`label_style`](Self::label_style) beneath each label's own
/// [`Line`]/[`Span`](rstui_core::Span) styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Waterfall, WaterfallStep};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 3, 5));
/// Waterfall::new([
///     WaterfallStep::delta(4, "a"),
///     WaterfallStep::delta(-2, "b"),
///     WaterfallStep::total("t"),
/// ])
/// .bar_gap(0)
/// .render(buf.area(), &mut buf);
///
/// // Three 1-wide columns; the bottom inner row is the label row.
/// assert_eq!(buf.get(Position::new(0, 4)).unwrap().symbol, 'a');
/// assert_eq!(buf.get(Position::new(1, 4)).unwrap().symbol, 'b');
/// assert_eq!(buf.get(Position::new(2, 4)).unwrap().symbol, 't');
/// ```
#[derive(Debug, Clone)]
pub struct Waterfall<'a> {
    steps: Vec<WaterfallStep<'a>>,
    direction: WaterfallDirection,
    max: Option<u64>,
    bar_gap: u16,
    block: Option<Block<'a>>,
    style: Style,
    rise_style: Style,
    fall_style: Style,
    total_style: Style,
    connector_style: Style,
    label_style: Style,
}

impl Default for Waterfall<'_> {
    fn default() -> Self {
        Self {
            steps: Vec::new(),
            direction: WaterfallDirection::Vertical,
            max: None,
            // A one-cell gap never visually merges adjacent steps (the
            // BarChart reasoning); bars otherwise fill their group.
            bar_gap: 1,
            block: None,
            style: Style::default(),
            rise_style: Style::default(),
            fall_style: Style::default(),
            total_style: Style::default(),
            connector_style: Style::default(),
            label_style: Style::default(),
        }
    }
}

impl<'a> Waterfall<'a> {
    /// A vertical waterfall of `steps`, auto-scaled to the cumulative path
    /// (and `0`), with one-cell gaps and no frame.
    pub fn new<I>(steps: I) -> Self
    where
        I: IntoIterator<Item = WaterfallStep<'a>>,
    {
        Self {
            steps: steps.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets the value mapped to the far end of the axis, or `None` to
    /// auto-scale to the cumulative path.
    ///
    /// A cumulative point above the ceiling is clamped (never a panic — the
    /// [`Gauge`](crate::Gauge) totality rule).
    #[must_use]
    pub fn max(mut self, max: Option<u64>) -> Self {
        self.max = max;
        self
    }

    /// Sets whether bars are columns (vertical axis) or rows (horizontal
    /// axis).
    #[must_use]
    pub fn direction(mut self, direction: WaterfallDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Sets the blank gap between adjacent steps (default `1`).
    #[must_use]
    pub fn bar_gap(mut self, bar_gap: u16) -> Self {
        self.bar_gap = bar_gap;
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

    /// Sets the [`Style`] for a step whose delta is a **rise** (a positive
    /// change), over the base.
    #[must_use]
    pub fn rise_style(mut self, style: Style) -> Self {
        self.rise_style = style;
        self
    }

    /// Sets the [`Style`] for a step whose delta is a **fall** (a negative
    /// change), over the base.
    #[must_use]
    pub fn fall_style(mut self, style: Style) -> Self {
        self.fall_style = style;
        self
    }

    /// Sets the [`Style`] for a [`Total`](WaterfallKind::Total) step's
    /// absolute bar, over the base.
    #[must_use]
    pub fn total_style(mut self, style: Style) -> Self {
        self.total_style = style;
        self
    }

    /// Sets the [`Style`] the thin connector glyphs between steps are drawn
    /// with, over the base.
    #[must_use]
    pub fn connector_style(mut self, style: Style) -> Self {
        self.connector_style = style;
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

/// The cumulative span of one step: the value its bar starts at and the value
/// it ends at (in cumulative units, before scaling), plus its
/// [`WaterfallKind`].
struct StepSpan {
    start: i64,
    end: i64,
    kind: WaterfallKind,
}

/// Walks `steps` into per-step start/end cumulative spans (a total spans
/// `0..cumulative`), returned with the overall `(lo, hi)` cumulative range
/// including the `0` baseline.
fn walk(steps: &[WaterfallStep]) -> (Vec<StepSpan>, i64, i64) {
    let mut cum: i64 = 0;
    let mut lo: i64 = 0;
    let mut hi: i64 = 0;
    let mut spans = Vec::with_capacity(steps.len());
    for step in steps {
        let span = match step.kind {
            WaterfallKind::Delta => {
                let start = cum;
                let end = cum.saturating_add(step.delta);
                cum = end;
                StepSpan {
                    start,
                    end,
                    kind: WaterfallKind::Delta,
                }
            }
            WaterfallKind::Total => StepSpan {
                start: 0,
                end: cum,
                kind: WaterfallKind::Total,
            },
        };
        lo = lo.min(span.start).min(span.end);
        hi = hi.max(span.start).max(span.end);
        spans.push(span);
    }
    (spans, lo, hi)
}

/// The position, in eighths from the axis origin, of cumulative `value` on a
/// `cells`-cell axis covering `lo..=hi` (already non-degenerate, `hi > lo`).
/// `value` is clamped into the range so the result is always in `0..=cells*8`.
fn eighths(value: i64, lo: i64, hi: i64, cells: u16) -> u64 {
    let clamped = value.clamp(lo, hi);
    let offset = i128::from(clamped) - i128::from(lo);
    let range = i128::from(hi) - i128::from(lo);
    let total = i128::from(cells) * 8;
    ((offset * total + range / 2) / range) as u64
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

/// The per-step group size along the category axis: the axis split evenly
/// across `count` steps after removing the inter-step `gap`s, floored at `1`.
fn group_width(axis: u16, count: usize, gap: u16) -> u16 {
    let count = count.max(1) as u16;
    let gaps = gap.saturating_mul(count.saturating_sub(1));
    (axis.saturating_sub(gaps) / count).max(1)
}

/// Stamps a step's label centred under its `group_w`-wide column, clipped to
/// the group.
fn stamp_label(buf: &mut Buffer, step: &WaterfallStep, base: Style, x0: u16, y: u16, group_w: u16) {
    let lw = (step.label.width() as u16).min(group_w);
    let lx = x0.saturating_add((group_w - lw) / 2);
    stamp_line(buf, &step.label, base, lx, y, x0.saturating_add(group_w));
}

/// The bar style for a span: total/rise/fall over the base.
fn bar_style_for(span: &StepSpan, base: Style, rise: Style, fall: Style, total: Style) -> Style {
    base.patch(match span.kind {
        WaterfallKind::Total => total,
        WaterfallKind::Delta if span.end >= span.start => rise,
        WaterfallKind::Delta => fall,
    })
}

/// The `(trailing_cell, leading_full, leading_rem)` decomposition of a
/// floating segment that spans eighths `from_e..=to_e` (already `from_e <=
/// to_e`) on a `cells`-cell axis: the trailing edge snaps to the nearest whole
/// cell, the leading edge keeps eighth precision (one boundary glyph). When
/// `sliver` is set a zero-height segment still yields a one-eighth sliver so a
/// break-even step is never invisible; on a flat walk (no real range) it does
/// not, so the bars stay empty like the [`BarChart`](crate::BarChart)
/// all-zero-series rule.
fn segment(from_e: u64, to_e: u64, cells: u16, sliver: bool) -> (u16, u16, u16) {
    if cells == 0 {
        return (0, 0, 0);
    }
    let total = u64::from(cells) * 8;
    if from_e == to_e {
        // A zero-height (break-even) segment: nothing unless `sliver`, in
        // which case exactly a one-eighth glyph in the cell at that level
        // (clamped one cell down when the level sits on the very top edge so
        // the sliver still has a cell to live in).
        if !sliver {
            return (0, 0, 0);
        }
        let cell = (to_e / 8).min(u64::from(cells - 1)) as u16;
        return (cell, cell, 1);
    }
    // Snap the trailing edge to its nearest whole cell; keep eighth precision
    // on the leading edge, nudged up one eighth if rounding collapsed the
    // segment so a thin float is never invisible.
    let trailing_cell = ((from_e + 4) / 8).min(u64::from(cells)) as u16;
    let lead = to_e.max(u64::from(trailing_cell) * 8 + 1).min(total);
    let leading_full = (lead / 8) as u16;
    let leading_rem = (lead % 8) as u16;
    (trailing_cell, leading_full, leading_rem)
}

impl Widget for Waterfall<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Waterfall {
            steps,
            direction,
            max,
            bar_gap,
            block,
            style,
            rise_style,
            fall_style,
            total_style,
            connector_style,
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
        if steps.is_empty() {
            return;
        }

        let (spans, lo, raw_hi) = walk(&steps);
        // The caller's ceiling raises the visible top (clamping points above
        // it); the range is floored non-degenerate so the scale math is total.
        let hi = match max {
            Some(m) => i64::try_from(m).unwrap_or(i64::MAX).max(lo),
            None => raw_hi,
        }
        .max(lo.saturating_add(1));
        // A flat walk with no explicit ceiling has no real cumulative range:
        // every bar is empty (only the labels show), exactly the BarChart
        // all-zero-series rule — no sliver floor, no connectors.
        let flat = max.is_none() && raw_hi == lo;
        let connector = style.patch(connector_style);

        match direction {
            WaterfallDirection::Vertical => {
                let label_row = inner.height > 1;
                let bar_rows = inner.height.saturating_sub(u16::from(label_row));
                let label_y = inner.bottom().saturating_sub(1);
                let right = inner.right();
                let top = inner.top();
                let group_w = group_width(inner.width, steps.len(), bar_gap);

                let mut x0 = inner.left();
                let mut prev_end_cell: Option<u16> = None;
                for (i, step) in steps.iter().enumerate() {
                    if x0 >= right {
                        break;
                    }
                    let span = &spans[i];
                    let group_right = x0.saturating_add(group_w).min(right);
                    let glyph = bar_style_for(span, style, rise_style, fall_style, total_style);

                    if bar_rows > 0 && !flat {
                        let a = eighths(span.start, lo, hi, bar_rows);
                        let b = eighths(span.end, lo, hi, bar_rows);
                        let (trailing, lead_full, lead_rem) =
                            segment(a.min(b), a.max(b), bar_rows, true);

                        // The connector joins the previous step's cumulative
                        // *end* level to this step, in the gap column just
                        // left of the group so it never overpaints a bar
                        // (flush bars — no gap — already read continuous).
                        if let Some(pe) = prev_end_cell {
                            if bar_gap > 0 && x0 > inner.left() {
                                let r = pe.min(bar_rows - 1);
                                let cy = top + bar_rows - 1 - r;
                                buf.set_cell(Position::new(x0 - 1, cy), CONNECTOR, connector);
                            }
                        }

                        for x in x0..group_right {
                            // Full cells from the trailing edge up to (but not
                            // including) the leading boundary cell.
                            for r in trailing..lead_full.min(bar_rows) {
                                let y = top + bar_rows - 1 - r;
                                buf.set_cell(Position::new(x, y), '█', glyph);
                            }
                            // The single fractional leading boundary cell.
                            if lead_rem > 0 && lead_full < bar_rows {
                                let y = top + bar_rows - 1 - lead_full;
                                buf.set_cell(
                                    Position::new(x, y),
                                    VERTICAL_EIGHTHS[(lead_rem - 1) as usize],
                                    glyph,
                                );
                            }
                        }
                        // Where the walk continues from: this step's end
                        // cumulative level snapped to its whole cell.
                        let end_e = eighths(span.end, lo, hi, bar_rows);
                        prev_end_cell = Some(((end_e + 4) / 8).min(u64::from(bar_rows)) as u16);
                    }

                    if label_row {
                        stamp_label(buf, step, style.patch(label_style), x0, label_y, group_w);
                    }
                    x0 = group_right.saturating_add(bar_gap);
                }
            }
            WaterfallDirection::Horizontal => {
                let longest = steps.iter().map(|s| s.label.width()).max().unwrap_or(0) as u16;
                let label_w = longest.min(inner.width / 2);
                let bar_x0 = inner.left().saturating_add(label_w);
                let bar_cols = inner.width.saturating_sub(label_w);
                let bottom = inner.bottom();
                let bar_right = inner.right();
                let group_h = group_width(inner.height, steps.len(), bar_gap);

                let mut y0 = inner.top();
                let mut prev_end_cell: Option<u16> = None;
                for (i, step) in steps.iter().enumerate() {
                    if y0 >= bottom {
                        break;
                    }
                    let span = &spans[i];
                    let group_bottom = y0.saturating_add(group_h).min(bottom);
                    let glyph = bar_style_for(span, style, rise_style, fall_style, total_style);

                    if bar_cols > 0 && !flat {
                        let a = eighths(span.start, lo, hi, bar_cols);
                        let b = eighths(span.end, lo, hi, bar_cols);
                        let (trailing, lead_full, lead_rem) =
                            segment(a.min(b), a.max(b), bar_cols, true);

                        // The connector joins the previous step's cumulative
                        // *end* level to this step, on the gap row just above
                        // the group so it never overpaints a bar (flush bars
                        // — no gap — already read continuous).
                        if let Some(pe) = prev_end_cell {
                            if bar_gap > 0 && y0 > inner.top() {
                                let c = pe.min(bar_cols - 1);
                                let cx = bar_x0.saturating_add(c);
                                if cx < bar_right {
                                    buf.set_cell(Position::new(cx, y0 - 1), CONNECTOR, connector);
                                }
                            }
                        }

                        for y in y0..group_bottom {
                            for c in trailing..lead_full.min(bar_cols) {
                                let x = bar_x0.saturating_add(c);
                                if x >= bar_right {
                                    break;
                                }
                                buf.set_cell(Position::new(x, y), '█', glyph);
                            }
                            if lead_rem > 0 && lead_full < bar_cols {
                                let x = bar_x0.saturating_add(lead_full);
                                if x < bar_right {
                                    buf.set_cell(
                                        Position::new(x, y),
                                        HORIZONTAL_EIGHTHS[(lead_rem - 1) as usize],
                                        glyph,
                                    );
                                }
                            }
                        }
                        let end_e = eighths(span.end, lo, hi, bar_cols);
                        prev_end_cell = Some(((end_e + 4) / 8).min(u64::from(bar_cols)) as u16);
                    }

                    if label_w > 0 {
                        stamp_line(
                            buf,
                            &step.label,
                            style.patch(label_style),
                            inner.left(),
                            y0,
                            inner.left().saturating_add(label_w),
                        );
                    }
                    y0 = group_bottom.saturating_add(bar_gap);
                }
            }
        }
    }
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
    fn a_rise_then_a_total_floats_then_rules_off_from_the_baseline() {
        // Walk: +4 (0→4), total (0→4). Range lo=0, hi=4, 4 bar rows + label
        // row. Step 1 floats 0→4 (full column). Total is an absolute bar 0→4
        // (full column). 1-wide groups, gap 1.
        let w = Waterfall::new([WaterfallStep::delta(4, "a"), WaterfallStep::total("t")]);
        // The connector dot sits in the gap column at step 1's cumulative
        // level (the axis top here, since +4 fills the 0..4 range).
        assert_eq!(lines(w, 3, 5), "█·█\n█ █\n█ █\n█ █\na t\n");
    }

    #[test]
    fn a_fall_floats_down_from_the_previous_cumulative() {
        // Walk: +4 (0→4), -2 (4→2). lo=0, hi=4, 4 rows. Step 1: 0→4 full.
        // Step 2: floats 2→4 (the upper half: the top two rows).
        let w = Waterfall::new([WaterfallStep::delta(4, "a"), WaterfallStep::delta(-2, "b")]);
        // col0 = full bar; the connector marks step 1's cumulative end (level
        // 4 = the top row); col2 = the float segment in the top two rows.
        assert_eq!(lines(w, 3, 5), "█·█\n█ █\n█  \n█  \na b\n");
    }

    #[test]
    fn a_break_even_step_still_shows_a_one_eighth_sliver() {
        // +8 then 0 (break-even). lo=0 hi=8, 8 rows. Step 2 floats 8→8 → a
        // one-eighth sliver at the cumulative level (the top bar row).
        let w =
            Waterfall::new([WaterfallStep::delta(8, "a"), WaterfallStep::delta(0, "z")]).bar_gap(0);
        let out = lines(w, 2, 9);
        // col0 is the full +8 bar; col1's break-even step is a one-eighth
        // sliver (`▁`) in the top row at the cumulative level.
        assert_eq!(out.lines().next().unwrap(), "█▁");
    }

    #[test]
    fn a_value_above_the_ceiling_clamps_to_the_axis_top() {
        // +999 with an explicit ceiling of 8 → clamps to a full column.
        let w = Waterfall::new([WaterfallStep::delta(999, "x")])
            .max(Some(8))
            .bar_gap(0);
        assert_eq!(lines(w, 1, 3), "█\n█\nx\n");
    }

    #[test]
    fn an_all_zero_walk_draws_no_bars_only_labels() {
        // Every delta 0 → lo=hi=0, range floors to 1, every bar empty.
        let w =
            Waterfall::new([WaterfallStep::delta(0, "a"), WaterfallStep::delta(0, "b")]).bar_gap(0);
        assert_eq!(lines(w, 2, 2), "  \nab\n");
    }

    #[test]
    fn a_single_step_floats_from_the_baseline() {
        let w = Waterfall::new([WaterfallStep::delta(5, "s")]).max(Some(5));
        assert_eq!(lines(w, 1, 6), "█\n█\n█\n█\n█\ns\n");
    }

    #[test]
    fn the_bar_gap_separates_steps() {
        let w = Waterfall::new([WaterfallStep::delta(4, "a"), WaterfallStep::delta(4, "b")])
            .max(Some(8))
            .bar_gap(1);
        // a: 0→4 (bottom half), b: 4→8 (top half), 1-wide cols, 1 gap; the
        // connector marks step 1's end (level 4 → row 1) in the gap column.
        assert_eq!(lines(w, 3, 5), "  █\n ·█\n█  \n█  \na b\n");
    }

    #[test]
    fn horizontal_bars_grow_rightward_with_a_left_label_column() {
        // +8 then total, ceiling 8. label_w = min(1, 8/2)=1, 7 bar cols.
        let w = Waterfall::new([WaterfallStep::delta(8, "a"), WaterfallStep::total("t")])
            .direction(WaterfallDirection::Horizontal)
            .max(Some(8))
            .bar_gap(0);
        assert_eq!(lines(w, 8, 2), "a███████\nt███████\n");
    }

    #[test]
    fn a_horizontal_fall_floats_from_the_running_cumulative() {
        // +8 (0→8), -4 (8→4), ceiling 8. label_w=1, 7 cols. Row 0 full;
        // row 1 floats 4→8 → the right half of the 7-col axis.
        let w = Waterfall::new([WaterfallStep::delta(8, "a"), WaterfallStep::delta(-4, "b")])
            .direction(WaterfallDirection::Horizontal)
            .max(Some(8))
            .bar_gap(0);
        let out = lines(w, 8, 2);
        let row1 = out.lines().nth(1).unwrap();
        assert_eq!(&row1[0..1], "b");
        // The bar does not start at the label column edge (it floats).
        assert_eq!(&row1[1..2], " ");
    }

    #[test]
    fn a_block_frames_the_chart_in_the_inner_area() {
        let w = Waterfall::new([WaterfallStep::delta(8, "x")])
            .max(Some(8))
            .block(Block::bordered());
        // inner 1×1 → only the bar row fits (no label row), one full block.
        assert_eq!(lines(w, 3, 3), "┌─┐\n│█│\n└─┘\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_bars() {
        let w = Waterfall::new([WaterfallStep::delta(8, "x")]).block(Block::bordered());
        assert_eq!(lines(w, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn no_steps_with_a_block_still_renders_the_block() {
        let w = Waterfall::new(Vec::<WaterfallStep>::new()).block(Block::bordered());
        assert_eq!(lines(w, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn the_connector_joins_consecutive_steps() {
        // +4 then +4, ceiling 8, 1-wide cols with a 1 gap. The first step
        // ends at level 4; a connector sits in the gap column at that level.
        let w = Waterfall::new([WaterfallStep::delta(4, "a"), WaterfallStep::delta(4, "b")])
            .max(Some(8))
            .bar_gap(1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 5));
        w.render(buf.area(), &mut buf);
        let mut found = false;
        for y in 0..4 {
            if buf.get(Position::new(1, y)).unwrap().symbol == CONNECTOR {
                found = true;
            }
        }
        assert!(found);
    }

    #[test]
    fn style_cascades_base_then_rise_fall_total_and_label_styles() {
        let w = Waterfall::new([
            WaterfallStep::delta(4, "r"),
            WaterfallStep::delta(-2, "f"),
            WaterfallStep::total("t"),
        ])
        .max(Some(4))
        .bar_gap(0)
        .style(Style::new().bg(Color::Blue))
        .rise_style(Style::new().fg(Color::Green))
        .fall_style(Style::new().fg(Color::Red))
        .total_style(Style::new().fg(Color::Yellow))
        .label_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 5));
        w.render(buf.area(), &mut buf);

        // Rise column bottom cell: green over the blue base fill.
        let rise = buf.get(Position::new(0, 3)).unwrap();
        assert_eq!(rise.symbol, '█');
        assert_eq!(rise.fg, Color::Green);
        assert_eq!(rise.bg, Color::Blue);

        // Fall column (x=1): the segment floats 2→4 (top rows); a fall cell
        // is red over the base.
        let fall = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(fall.fg, Color::Red);
        assert_eq!(fall.bg, Color::Blue);

        // Total column (x=2): a full absolute bar in yellow.
        let total = buf.get(Position::new(2, 3)).unwrap();
        assert_eq!(total.symbol, '█');
        assert_eq!(total.fg, Color::Yellow);

        // Label row cascades the base + label_style.
        let l = buf.get(Position::new(0, 4)).unwrap();
        assert_eq!(l.symbol, 'r');
        assert!(l.modifier.contains(Modifier::BOLD));
        assert_eq!(l.bg, Color::Blue);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Waterfall::new([WaterfallStep::delta(5, "x")]).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn a_tiny_area_clips_without_panicking() {
        // One row only → the label row, no bar rows; must not panic.
        let w = Waterfall::new([WaterfallStep::delta(4, "a"), WaterfallStep::delta(-9, "b")]);
        let _ = lines(w, 1, 1);
        // Zero-width inner via a bordered block in a 1×1 area.
        let w = Waterfall::new([WaterfallStep::delta(4, "a")]).block(Block::bordered());
        let _ = lines(w, 1, 1);
    }
}
