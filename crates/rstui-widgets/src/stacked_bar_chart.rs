//! [`StackedBarChart`] — multi-series labelled bars, **stacked** or
//! **grouped**, horizontal or vertical, the composition-comparison primitive a
//! dashboard pins when one bar per category is not enough (revenue by region
//! split by product, latency by endpoint split by percentile bucket, error
//! budget burn by service).
//!
//! # A pure projection, like every other widget
//!
//! `StackedBarChart` owns no state. It is a list of caller-built
//! [`StackedBar`]s (a label [`Line`] plus a `Vec` of `(value, Color)`
//! segments) and an optional ceiling; the reducer decides what the categories
//! and series are and the widget only projects them. That keeps it
//! deterministically headless-testable and composes with the Elm
//! `view(&self)` model exactly like [`List`](crate::List) and
//! [`BarChart`](crate::BarChart) — it is the deliberately-deferred additive
//! [`BarChart`](crate::BarChart) called out in that widget's docs, shipped as
//! its own type rather than smuggled in as a mode.
//!
//! # Sub-cell precision, reusing the [`BarChart`](crate::BarChart) idea
//!
//! Each segment's end rarely lands on a whole cell, so — exactly like
//! [`BarChart`](crate::BarChart) and [`Gauge`](crate::Gauge) — the boundary
//! cell is drawn with the eighth-block glyph nearest the true fraction (the
//! *vertical* ramp `▁…█` for vertical bars, the *horizontal* ramp `▏…█` for
//! horizontal ones), not rounded to a whole cell. Each glyph is one Unicode
//! scalar, so it maps 1:1 onto a [`Cell`](rstui_core::Buffer) with no grapheme
//! machinery — the same reasoning the gauge ramp and [`Block`] borders use. In
//! [`StackMode::Stacked`] segments accumulate along the bar axis, each in its
//! own [`Color`]; in [`StackMode::Grouped`] the segments are adjacent thin
//! sub-bars sharing the category slot, the classic clustered chart.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no bars, a bar with no segments, an all-zero series (an empty chart),
//! a stack sum above the ceiling (clamped), and an area too narrow/short for
//! the bars or labels are all safe clips/no-ops — never a panic; the scale
//! ceiling never drops below `1` so there is no division by zero. An optional
//! framing [`Block`] follows the container-widget convention.

use rstui_core::{Buffer, Color, Line, Position, Rect, Style, Widget};

use crate::bar_chart::BarChartDirection;
use crate::block::Block;

/// The eight bottom-aligned block elements for **vertical** bars, `1/8` …
/// `8/8`.
const VERTICAL_EIGHTHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The eight left-aligned block elements for **horizontal** bars, `1/8` …
/// `8/8` (the same ramp [`Gauge`](crate::Gauge) fills its bar with).
const HORIZONTAL_EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// How a [`StackedBarChart`]'s segments share a category slot.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StackMode {
    /// Segments **accumulate** along the bar axis, each drawn in its own
    /// colour on top of the previous (the default — a composition bar).
    #[default]
    Stacked,
    /// Segments are **adjacent thin sub-bars** within the category slot (a
    /// clustered/grouped chart), each scaled independently.
    Grouped,
}

/// One category of a [`StackedBarChart`]: a label [`Line`] and its ordered
/// `(value, `[`Color`]`)` segments (bottom/left first).
///
/// Build the label from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`); style it through the
/// [`Line`] it wraps. Each segment carries its own [`Color`] so a stack reads
/// as distinct bands.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StackedBar<'a> {
    /// The category's label.
    label: Line<'a>,
    /// The ordered segments: `(value, colour)`, the first at the bar's
    /// baseline (bottom for vertical, left for horizontal).
    segments: Vec<(u64, Color)>,
}

impl<'a> StackedBar<'a> {
    /// A category labelled `label` with ordered `(value, colour)` `segments`
    /// (the first at the bar's baseline).
    pub fn new(label: impl Into<Line<'a>>, segments: Vec<(u64, Color)>) -> Self {
        Self {
            label: label.into(),
            segments,
        }
    }
}

/// A row/column of multi-series labelled bars — stacked or grouped — with
/// sub-cell precision and an optional framing [`Block`].
///
/// Bars are placed in [`bar_width`](Self::bar_width)-wide slots separated by
/// [`bar_gap`](Self::bar_gap). In [`StackMode::Stacked`] each bar's segments
/// accumulate and the scale is the per-bar segment **sum** against
/// [`max`](Self::max) (the largest sum when unset); in
/// [`StackMode::Grouped`] each segment is an independent thin sub-bar scaled
/// against the largest single segment when unset. Styling is a base
/// [`Style`] (filling the area) with each segment's own [`Color`] for its
/// band and a [`label_style`](Self::label_style) beneath each label's own
/// [`Line`]/[`Span`](rstui_core::Span) styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Color, Position, Rect, Widget};
/// use rstui_widgets::{StackedBar, StackedBarChart};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 1, 5));
/// StackedBarChart::new([StackedBar::new(
///     "a",
///     vec![(2, Color::Red), (2, Color::Blue)],
/// )])
/// .max(Some(4))
/// .bar_gap(0)
/// .render(buf.area(), &mut buf);
///
/// // 4 bar rows + 1 label row; the stack fills all four, the lower half red.
/// assert_eq!(buf.get(Position::new(0, 4)).unwrap().symbol, 'a'); // label
/// assert_eq!(buf.get(Position::new(0, 3)).unwrap().fg, Color::Red);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Blue);
/// ```
#[derive(Debug, Clone)]
pub struct StackedBarChart<'a> {
    bars: Vec<StackedBar<'a>>,
    mode: StackMode,
    direction: BarChartDirection,
    max: Option<u64>,
    bar_width: u16,
    bar_gap: u16,
    block: Option<Block<'a>>,
    style: Style,
    label_style: Style,
}

impl Default for StackedBarChart<'_> {
    fn default() -> Self {
        Self {
            bars: Vec::new(),
            mode: StackMode::Stacked,
            direction: BarChartDirection::Vertical,
            max: None,
            // One-cell bars with a one-cell gap: the sensible default that
            // never visually merges adjacent categories (BarChart's reasoning).
            bar_width: 1,
            bar_gap: 1,
            block: None,
            style: Style::default(),
            label_style: Style::default(),
        }
    }
}

impl<'a> StackedBarChart<'a> {
    /// A vertical, stacked chart of `bars`, auto-scaled, with one-cell bars and
    /// gaps and no frame.
    pub fn new<I>(bars: I) -> Self
    where
        I: IntoIterator<Item = StackedBar<'a>>,
    {
        Self {
            bars: bars.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets whether segments stack or are grouped into adjacent sub-bars.
    #[must_use]
    pub fn mode(mut self, mode: StackMode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets whether bars grow up (vertical) or right (horizontal).
    #[must_use]
    pub fn direction(mut self, direction: BarChartDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Sets the value mapped to a full-length bar, or `None` to auto-scale.
    ///
    /// Stacked auto-scales to the largest per-bar segment **sum**; grouped to
    /// the largest single segment. A value above the ceiling is clamped (never
    /// a panic — the [`Gauge`](crate::Gauge) totality rule).
    #[must_use]
    pub fn max(mut self, max: Option<u64>) -> Self {
        self.max = max;
        self
    }

    /// Sets the thickness of each category slot (columns when vertical, rows
    /// when horizontal). Clamped to at least `1` at render time.
    #[must_use]
    pub fn bar_width(mut self, bar_width: u16) -> Self {
        self.bar_width = bar_width;
        self
    }

    /// Sets the blank gap between adjacent category slots (default `1`).
    #[must_use]
    pub fn bar_gap(mut self, bar_gap: u16) -> Self {
        self.bar_gap = bar_gap;
        self
    }

    /// Frames the chart in `block`; bars render into
    /// [`block.inner`](Block::inner).
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

/// The sum of a bar's segment values, saturating (never overflows the scale).
fn segment_sum(bar: &StackedBar) -> u64 {
    bar.segments
        .iter()
        .fold(0u64, |acc, &(v, _)| acc.saturating_add(v))
}

impl Widget for StackedBarChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let StackedBarChart {
            bars,
            mode,
            direction,
            max,
            bar_width,
            bar_gap,
            block,
            style,
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
        if bars.is_empty() {
            return;
        }

        // The ceiling: the caller's, else by mode — the largest stack sum
        // (stacked) or the largest single segment (grouped). Never below 1 so
        // the scale math is total (an all-zero series then renders empty).
        let ceiling = max
            .or_else(|| match mode {
                StackMode::Stacked => bars.iter().map(segment_sum).max(),
                StackMode::Grouped => bars
                    .iter()
                    .flat_map(|b| b.segments.iter().map(|&(v, _)| v))
                    .max(),
            })
            .unwrap_or(0)
            .max(1);
        let bar_w = bar_width.max(1);

        match direction {
            BarChartDirection::Vertical => {
                let label_row = inner.height > 1;
                let bar_rows = inner.height.saturating_sub(u16::from(label_row));
                let label_y = inner.bottom().saturating_sub(1);
                let right = inner.right();

                let mut x0 = inner.left();
                for bar in &bars {
                    if x0 >= right {
                        break;
                    }
                    let group_right = x0.saturating_add(bar_w).min(right);

                    match mode {
                        StackMode::Stacked => {
                            // Segments accumulate up the bar; each band runs
                            // from the previous cumulative eighth to the new
                            // one, its boundary cell a fractional glyph.
                            let mut acc = 0u64;
                            for &(value, color) in &bar.segments {
                                let lo_e = eighths(acc.min(ceiling), ceiling, bar_rows);
                                acc = acc.saturating_add(value);
                                let hi_e = eighths(acc.min(ceiling), ceiling, bar_rows);
                                if hi_e <= lo_e {
                                    continue;
                                }
                                let seg_style = style.fg(color);
                                let full = (hi_e / 8) as u16;
                                let rem = (hi_e % 8) as u16;
                                let lo_full = (lo_e / 8) as u16;
                                for x in x0..group_right {
                                    for r in lo_full..full {
                                        let y = inner.top().saturating_add(bar_rows - 1 - r);
                                        buf.set_cell(Position::new(x, y), '█', seg_style);
                                    }
                                    if rem > 0 && full < bar_rows {
                                        let y = inner.top().saturating_add(bar_rows - 1 - full);
                                        buf.set_cell(
                                            Position::new(x, y),
                                            VERTICAL_EIGHTHS[(rem - 1) as usize],
                                            seg_style,
                                        );
                                    }
                                }
                            }
                        }
                        StackMode::Grouped => {
                            // Segments are adjacent thin sub-bars sharing the
                            // slot, each scaled independently against ceiling.
                            let n = bar.segments.len().max(1) as u16;
                            let sub_w = (bar_w / n).max(1);
                            let mut sx = x0;
                            for &(value, color) in &bar.segments {
                                if sx >= group_right {
                                    break;
                                }
                                let sub_right = sx.saturating_add(sub_w).min(group_right);
                                let total_e = eighths(value, ceiling, bar_rows);
                                let full = (total_e / 8) as u16;
                                let rem = (total_e % 8) as u16;
                                let seg_style = style.fg(color);
                                for x in sx..sub_right {
                                    for r in 0..full {
                                        let y = inner.top().saturating_add(bar_rows - 1 - r);
                                        buf.set_cell(Position::new(x, y), '█', seg_style);
                                    }
                                    if rem > 0 && full < bar_rows {
                                        let y = inner.top().saturating_add(bar_rows - 1 - full);
                                        buf.set_cell(
                                            Position::new(x, y),
                                            VERTICAL_EIGHTHS[(rem - 1) as usize],
                                            seg_style,
                                        );
                                    }
                                }
                                sx = sub_right;
                            }
                        }
                    }

                    if label_row {
                        let lw = (bar.label.width() as u16).min(bar_w);
                        let lx = x0.saturating_add((bar_w - lw) / 2);
                        stamp_line(
                            buf,
                            &bar.label,
                            style.patch(label_style),
                            lx,
                            label_y,
                            group_right,
                        );
                    }
                    x0 = group_right.saturating_add(bar_gap);
                }
            }
            BarChartDirection::Horizontal => {
                let longest = bars.iter().map(|b| b.label.width()).max().unwrap_or(0) as u16;
                let label_w = longest.min(inner.width / 2);
                let bar_x0 = inner.left().saturating_add(label_w);
                let bar_cols = inner.width.saturating_sub(label_w);
                let bottom = inner.bottom();
                let bar_right = inner.right();

                let mut y0 = inner.top();
                for bar in &bars {
                    if y0 >= bottom {
                        break;
                    }
                    let group_bottom = y0.saturating_add(bar_w).min(bottom);

                    match mode {
                        StackMode::Stacked => {
                            let mut acc = 0u64;
                            for &(value, color) in &bar.segments {
                                let lo_e = eighths(acc.min(ceiling), ceiling, bar_cols);
                                acc = acc.saturating_add(value);
                                let hi_e = eighths(acc.min(ceiling), ceiling, bar_cols);
                                if hi_e <= lo_e {
                                    continue;
                                }
                                let seg_style = style.fg(color);
                                let full = (hi_e / 8) as u16;
                                let rem = (hi_e % 8) as u16;
                                let lo_full = (lo_e / 8) as u16;
                                for y in y0..group_bottom {
                                    for c in lo_full..full {
                                        let x = bar_x0.saturating_add(c);
                                        if x >= bar_right {
                                            break;
                                        }
                                        buf.set_cell(Position::new(x, y), '█', seg_style);
                                    }
                                    if rem > 0 && full < bar_cols {
                                        let x = bar_x0.saturating_add(full);
                                        if x < bar_right {
                                            buf.set_cell(
                                                Position::new(x, y),
                                                HORIZONTAL_EIGHTHS[(rem - 1) as usize],
                                                seg_style,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        StackMode::Grouped => {
                            let n = bar.segments.len().max(1) as u16;
                            let sub_h = (bar_w / n).max(1);
                            let mut sy = y0;
                            for &(value, color) in &bar.segments {
                                if sy >= group_bottom {
                                    break;
                                }
                                let sub_bottom = sy.saturating_add(sub_h).min(group_bottom);
                                let total_e = eighths(value, ceiling, bar_cols);
                                let full = (total_e / 8) as u16;
                                let rem = (total_e % 8) as u16;
                                let seg_style = style.fg(color);
                                for y in sy..sub_bottom {
                                    for c in 0..full {
                                        let x = bar_x0.saturating_add(c);
                                        if x >= bar_right {
                                            break;
                                        }
                                        buf.set_cell(Position::new(x, y), '█', seg_style);
                                    }
                                    if rem > 0 && full < bar_cols {
                                        let x = bar_x0.saturating_add(full);
                                        if x < bar_right {
                                            buf.set_cell(
                                                Position::new(x, y),
                                                HORIZONTAL_EIGHTHS[(rem - 1) as usize],
                                                seg_style,
                                            );
                                        }
                                    }
                                }
                                sy = sub_bottom;
                            }
                        }
                    }

                    if label_w > 0 {
                        stamp_line(
                            buf,
                            &bar.label,
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

    #[test]
    fn stacked_vertical_segments_accumulate_up_the_bar() {
        let chart = StackedBarChart::new([StackedBar::new(
            "a",
            vec![(2, Color::Red), (2, Color::Blue)],
        )])
        .max(Some(4))
        .bar_gap(0);
        // 4 bar rows + label row. Sum 4 → full bar; lower 2 red, upper 2 blue.
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 5));
        chart.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 4)).unwrap().symbol, 'a');
        for y in 0..2 {
            assert_eq!(buf.get(Position::new(0, y)).unwrap().fg, Color::Blue);
        }
        for y in 2..4 {
            assert_eq!(buf.get(Position::new(0, y)).unwrap().fg, Color::Red);
        }
        assert_eq!(buf.get(Position::new(0, 3)).unwrap().symbol, '█');
    }

    #[test]
    fn stacked_auto_scales_to_the_largest_segment_sum() {
        let chart = StackedBarChart::new([
            StackedBar::new("a", vec![(1, Color::Red), (1, Color::Blue)]),
            StackedBar::new("b", vec![(2, Color::Red), (2, Color::Blue)]),
        ])
        .bar_gap(0);
        // Largest sum = 4 (b). b full (4 rows), a half (2 rows).
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 5));
        chart.render(buf.area(), &mut buf);
        // b column full to the top.
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '█');
        // a column only the bottom half is filled.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(0, 3)).unwrap().symbol, '█');
        assert_eq!(buf.get(Position::new(0, 4)).unwrap().symbol, 'a');
    }

    #[test]
    fn a_fractional_segment_uses_a_sub_cell_glyph() {
        // One segment value 1, ceiling 2, 1 bar row → 0.5 row → ▄.
        let chart =
            StackedBarChart::new([StackedBar::new("x", vec![(1, Color::Red)])]).max(Some(2));
        assert_eq!(lines(chart, 1, 2), "▄\nx\n");
    }

    #[test]
    fn grouped_segments_are_adjacent_sub_bars() {
        let chart = StackedBarChart::new([StackedBar::new(
            "ab",
            vec![(4, Color::Red), (2, Color::Blue)],
        )])
        .mode(StackMode::Grouped)
        .max(Some(4))
        .bar_width(2)
        .bar_gap(0);
        // 4 bar rows + label. Slot width 2 → two 1-wide sub-bars: red 4/4
        // (full), blue 2/4 (half).
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 5));
        chart.render(buf.area(), &mut buf);
        // Sub-bar 0 (red) full height.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Red);
        assert_eq!(buf.get(Position::new(0, 3)).unwrap().symbol, '█');
        // Sub-bar 1 (blue) half height: top empty, bottom filled.
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(1, 3)).unwrap().fg, Color::Blue);
    }

    #[test]
    fn grouped_auto_scales_to_the_largest_single_segment() {
        let chart = StackedBarChart::new([StackedBar::new(
            "ab",
            vec![(8, Color::Red), (4, Color::Blue)],
        )])
        .mode(StackMode::Grouped)
        .bar_width(2)
        .bar_gap(0);
        // Largest single = 8. Two bar rows: red full, blue half.
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 3));
        chart.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '█');
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, '█');
    }

    #[test]
    fn horizontal_stacked_grows_rightward_with_a_left_label_column() {
        let chart = StackedBarChart::new([StackedBar::new(
            "a",
            vec![(2, Color::Red), (2, Color::Blue)],
        )])
        .direction(BarChartDirection::Horizontal)
        .max(Some(4))
        .bar_gap(0);
        // label_w = min(1, 6/2) = 1; 5-col bar. Sum 4 over ceiling 4 → 5 cols
        // full; first ~2.5 red, rest blue.
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        chart.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'a');
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().fg, Color::Red);
        assert_eq!(buf.get(Position::new(5, 0)).unwrap().fg, Color::Blue);
    }

    #[test]
    fn a_stack_sum_above_the_ceiling_clamps_to_a_full_bar() {
        let chart = StackedBarChart::new([StackedBar::new(
            "x",
            vec![(999, Color::Red), (999, Color::Blue)],
        )])
        .max(Some(8))
        .bar_gap(0);
        // First segment alone clamps the bar to full; nothing panics.
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 2));
        chart.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '█');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Red);
    }

    #[test]
    fn an_all_zero_series_draws_no_bars() {
        let chart = StackedBarChart::new([
            StackedBar::new("a", vec![(0, Color::Red)]),
            StackedBar::new("b", vec![(0, Color::Blue)]),
        ])
        .bar_gap(0);
        assert_eq!(lines(chart, 2, 2), "  \nab\n");
    }

    #[test]
    fn a_bar_with_no_segments_is_just_its_label() {
        let chart = StackedBarChart::new([StackedBar::new("a", Vec::new())]).bar_gap(0);
        assert_eq!(lines(chart, 1, 2), " \na\n");
    }

    #[test]
    fn the_bar_gap_separates_categories() {
        let chart = StackedBarChart::new([
            StackedBar::new("a", vec![(4, Color::Red)]),
            StackedBar::new("b", vec![(4, Color::Red)]),
        ])
        .max(Some(4))
        .bar_gap(1);
        assert_eq!(lines(chart, 3, 2), "█ █\na b\n");
    }

    #[test]
    fn a_block_frames_the_chart_in_the_inner_area() {
        let chart = StackedBarChart::new([StackedBar::new("x", vec![(4, Color::Red)])])
            .max(Some(4))
            .block(Block::bordered());
        // inner 1×1 → only the bar row fits (no label row), one full block.
        assert_eq!(lines(chart, 3, 3), "┌─┐\n│█│\n└─┘\n");
    }

    #[test]
    fn no_bars_with_a_block_still_renders_the_block() {
        let chart = StackedBarChart::new(Vec::<StackedBar>::new()).block(Block::bordered());
        assert_eq!(lines(chart, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn style_cascades_base_then_label_style_and_segment_colour() {
        let bar = StackedBar::new(
            Line::from(Span::styled("L", Style::new().fg(Color::Yellow))),
            vec![(4, Color::Green)],
        );
        let chart = StackedBarChart::new([bar])
            .max(Some(4))
            .bar_gap(0)
            .style(Style::new().bg(Color::Blue))
            .label_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 2));
        chart.render(buf.area(), &mut buf);

        let g = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(g.symbol, '█');
        assert_eq!(g.fg, Color::Green); // the segment's own colour
        assert_eq!(g.bg, Color::Blue); // base fill cascades

        let l = buf.get(Position::new(0, 1)).unwrap();
        assert_eq!(l.symbol, 'L');
        assert_eq!(l.fg, Color::Yellow); // span fg wins
        assert!(l.modifier.contains(Modifier::BOLD)); // label_style cascades
        assert_eq!(l.bg, Color::Blue);
    }

    #[test]
    fn a_narrow_area_clips_the_bars() {
        let chart = StackedBarChart::new([
            StackedBar::new("a", vec![(4, Color::Red)]),
            StackedBar::new("b", vec![(4, Color::Red)]),
            StackedBar::new("c", vec![(4, Color::Red)]),
        ])
        .max(Some(4))
        .bar_gap(0);
        // Width 2 fits only a, b; c is clipped, no panic.
        assert_eq!(lines(chart, 2, 2), "██\nab\n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        StackedBarChart::new([StackedBar::new("x", vec![(5, Color::Red)])])
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
