//! [`BarChart`] — labelled value bars, horizontal or vertical (the caller
//! picks), the dashboard primitive for "values across a handful of categories"
//! (latency by endpoint, commits by author, disk by mount).
//!
//! # A pure projection, like every other widget
//!
//! `BarChart` owns no state. It is a list of caller-built [`Bar`]s (a label
//! [`Line`] plus a `u64`) and an optional ceiling; the reducer decides what the
//! bars are and the widget only projects them. That keeps it deterministically
//! headless-testable and composes with the Elm `view(&self)` model exactly like
//! [`List`](crate::List) and [`Gauge`](crate::Gauge).
//!
//! # Sub-cell precision, reusing the [`Gauge`](crate::Gauge) idea
//!
//! A bar's end rarely lands on a whole cell, so — exactly like
//! [`Gauge`](crate::Gauge) — the boundary cell is drawn with the eighth-block
//! glyph nearest the true fraction (the *vertical* ramp `▁…█` for vertical
//! bars, the *horizontal* ramp `▏…█` for horizontal ones), not rounded to a
//! whole cell. Each glyph is one Unicode scalar, so it maps 1:1 onto a
//! [`Cell`](rstui_core::Buffer) with no grapheme machinery — the same reasoning
//! the gauge ramp and [`Block`] borders use.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no bars, an all-zero series, a value above the ceiling (clamped), and
//! an area too narrow/short for the bars or labels are all safe clips/no-ops —
//! never a panic. An optional framing [`Block`] follows the
//! container-widget convention; per-bar value labels and stacked/grouped series
//! are deliberately deferred additive follow-ups, not smuggled into this slice.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The eight bottom-aligned block elements for **vertical** bars, `1/8` … `8/8`.
const VERTICAL_EIGHTHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The eight left-aligned block elements for **horizontal** bars, `1/8` … `8/8`
/// (the same ramp [`Gauge`](crate::Gauge) fills its bar with).
const HORIZONTAL_EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// Which way a [`BarChart`]'s bars grow.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BarChartDirection {
    /// Bars grow **upward** from a baseline; categories are columns placed left
    /// to right with their labels on the bottom row (the default).
    #[default]
    Vertical,
    /// Bars grow **rightward**; categories are rows stacked top to bottom with
    /// their labels in a reserved left column.
    Horizontal,
}

/// One category of a [`BarChart`]: a label [`Line`] and its `u64` value.
///
/// Build the label from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`); style it through the
/// [`Line`] it wraps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Bar<'a> {
    label: Line<'a>,
    value: u64,
}

impl<'a> Bar<'a> {
    /// A bar of height `value` labelled `label` (anything convertible to a
    /// [`Line`]).
    pub fn new(value: u64, label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            value,
        }
    }
}

/// A row/column of labelled value bars with sub-cell precision and an optional
/// framing [`Block`].
///
/// Bars are placed in [`bar_width`](Self::bar_width)-wide groups separated by
/// [`bar_gap`](Self::bar_gap); each value is scaled against
/// [`max`](Self::max) (the largest value when unset) and drawn with full blocks
/// plus one fractional eighth-block boundary cell. Styling is a base
/// [`Style`] (filling the area) with a [`bar_style`](Self::bar_style) for the
/// glyphs and a [`label_style`](Self::label_style) beneath each label's own
/// [`Line`]/[`Span`](rstui_core::Span) styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Bar, BarChart};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 3, 3));
/// BarChart::new([Bar::new(8, "a"), Bar::new(4, "b")])
///     .max(Some(8))
///     .bar_gap(0)
///     .render(buf.area(), &mut buf);
///
/// // Two 1-wide vertical bars; the label row is the bottom inner row.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '█'); // a: 8/8
/// assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, 'a'); // label
/// assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, 'b');
/// ```
#[derive(Debug, Clone)]
pub struct BarChart<'a> {
    bars: Vec<Bar<'a>>,
    direction: BarChartDirection,
    max: Option<u64>,
    bar_width: u16,
    bar_gap: u16,
    block: Option<Block<'a>>,
    style: Style,
    bar_style: Style,
    label_style: Style,
}

impl Default for BarChart<'_> {
    fn default() -> Self {
        Self {
            bars: Vec::new(),
            direction: BarChartDirection::Vertical,
            max: None,
            // One-cell bars with a one-cell gap: the sensible default that
            // never visually merges adjacent categories (Table's reasoning).
            bar_width: 1,
            bar_gap: 1,
            block: None,
            style: Style::default(),
            bar_style: Style::default(),
            label_style: Style::default(),
        }
    }
}

impl<'a> BarChart<'a> {
    /// A vertical chart of `bars`, auto-scaled to the largest value, with
    /// one-cell bars and gaps and no frame.
    pub fn new<I>(bars: I) -> Self
    where
        I: IntoIterator<Item = Bar<'a>>,
    {
        Self {
            bars: bars.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets whether bars grow up (vertical) or right (horizontal).
    #[must_use]
    pub fn direction(mut self, direction: BarChartDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Sets the value mapped to a full-length bar, or `None` to auto-scale to
    /// the largest value.
    ///
    /// A value above the ceiling is clamped (never a panic — the
    /// [`Gauge`](crate::Gauge) totality rule).
    #[must_use]
    pub fn max(mut self, max: Option<u64>) -> Self {
        self.max = max;
        self
    }

    /// Sets the thickness of each bar (columns when vertical, rows when
    /// horizontal). Clamped to at least `1` at render time.
    #[must_use]
    pub fn bar_width(mut self, bar_width: u16) -> Self {
        self.bar_width = bar_width;
        self
    }

    /// Sets the blank gap between adjacent bars (default `1`).
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

impl Widget for BarChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let BarChart {
            bars,
            direction,
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
        if bars.is_empty() {
            return;
        }

        // The ceiling: the caller's, or the largest value, never below 1 so
        // the scale math is total (an all-zero series then renders empty).
        let ceiling = max
            .or_else(|| bars.iter().map(|b| b.value).max())
            .unwrap_or(0)
            .max(1);
        let bar_w = bar_width.max(1);
        let bar_glyph = style.patch(bar_style);

        match direction {
            BarChartDirection::Vertical => {
                // The bottom inner row is the label row (when there is more
                // than one row); bars rise in the rows above it.
                let label_row = inner.height > 1;
                let bar_rows = inner.height.saturating_sub(u16::from(label_row));
                let label_y = inner.bottom().saturating_sub(1);
                let right = inner.right();

                let mut x0 = inner.left();
                for bar in &bars {
                    if x0 >= right {
                        break;
                    }
                    let total_e = eighths(bar.value, ceiling, bar_rows);
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

                    // Label centred under the bar group, clipped to it.
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
                // A left label column, at most half the width, sized to the
                // longest label; bars fill the rest.
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
                    let total_e = eighths(bar.value, ceiling, bar_cols);
                    let full = (total_e / 8) as u16;
                    let rem = (total_e % 8) as u16;
                    let group_bottom = y0.saturating_add(bar_w).min(bottom);

                    for y in y0..group_bottom {
                        for c in 0..full {
                            let x = bar_x0.saturating_add(c);
                            if x >= bar_right {
                                break;
                            }
                            buf.set_cell(Position::new(x, y), '█', bar_glyph);
                        }
                        if rem > 0 && full < bar_cols {
                            let x = bar_x0.saturating_add(full);
                            if x < bar_right {
                                buf.set_cell(
                                    Position::new(x, y),
                                    HORIZONTAL_EIGHTHS[(rem - 1) as usize],
                                    bar_glyph,
                                );
                            }
                        }
                    }

                    // Label in the left column on the group's first row.
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
    fn vertical_bars_rise_from_a_baseline_with_a_label_row() {
        let chart = BarChart::new([Bar::new(8, "a"), Bar::new(4, "b")])
            .max(Some(8))
            .bar_gap(0);
        // 3 tall: 2 bar rows + 1 label row. a=8/8 → both rows full; b=4/8 →
        // 1.0 row → one full row at the baseline.
        assert_eq!(lines(chart, 2, 3), "█ \n██\nab\n");
    }

    #[test]
    fn a_fractional_vertical_bar_uses_a_sub_cell_glyph() {
        // value 1, ceiling 2, 1 bar row → 0.5 row → 4 eighths → ▄.
        let chart = BarChart::new([Bar::new(1, "x")]).max(Some(2));
        assert_eq!(lines(chart, 1, 2), "▄\nx\n");
    }

    #[test]
    fn the_bar_gap_separates_categories() {
        let chart = BarChart::new([Bar::new(8, "a"), Bar::new(8, "b")])
            .max(Some(8))
            .bar_gap(1);
        // 1-wide bars, a 1-wide gap between them, label row at the bottom.
        assert_eq!(lines(chart, 3, 2), "█ █\na b\n");
    }

    #[test]
    fn bar_width_thickens_each_bar() {
        let chart = BarChart::new([Bar::new(8, "ab")])
            .max(Some(8))
            .bar_width(2)
            .bar_gap(0);
        assert_eq!(lines(chart, 2, 2), "██\nab\n");
    }

    #[test]
    fn horizontal_bars_grow_rightward_with_a_left_label_column() {
        let chart = BarChart::new([Bar::new(8, "a"), Bar::new(4, "b")])
            .direction(BarChartDirection::Horizontal)
            .max(Some(8))
            .bar_gap(0);
        // label_w = min(1, 6/2)=1; bar area = 5 cols. a=8/8 → █████;
        // b=4/8 → 2.5 cols → ██▌.
        assert_eq!(lines(chart, 6, 2), "a█████\nb██▌  \n");
    }

    #[test]
    fn a_value_above_the_ceiling_clamps_to_a_full_bar() {
        let chart = BarChart::new([Bar::new(999, "x")]).max(Some(8)).bar_gap(0);
        assert_eq!(lines(chart, 1, 2), "█\nx\n");
    }

    #[test]
    fn an_all_zero_series_draws_no_bars() {
        let chart = BarChart::new([Bar::new(0, "a"), Bar::new(0, "b")]).bar_gap(0);
        // No ceiling, all zero → ceiling floors at 1, every bar empty; only
        // the labels show.
        assert_eq!(lines(chart, 2, 2), "  \nab\n");
    }

    #[test]
    fn auto_scale_maps_the_largest_value_to_a_full_bar() {
        let chart = BarChart::new([Bar::new(10, "a"), Bar::new(5, "b")]).bar_gap(0);
        // max = 10 → a full (2 rows), b half (1 row).
        assert_eq!(lines(chart, 2, 3), "█ \n██\nab\n");
    }

    #[test]
    fn a_block_frames_the_chart_in_the_inner_area() {
        let chart = BarChart::new([Bar::new(8, "x")])
            .max(Some(8))
            .block(Block::bordered());
        // inner 1×1 → only the bar row fits (no label row), one full block.
        assert_eq!(lines(chart, 3, 3), "┌─┐\n│█│\n└─┘\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_bars() {
        let chart = BarChart::new([Bar::new(8, "x")]).block(Block::bordered());
        assert_eq!(lines(chart, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn no_bars_with_a_block_still_renders_the_block() {
        let chart = BarChart::new(Vec::<Bar>::new()).block(Block::bordered());
        assert_eq!(lines(chart, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn style_cascades_base_then_bar_and_label_styles() {
        let bar = Bar::new(
            8,
            Line::from(Span::styled("L", Style::new().fg(Color::Red))),
        );
        let chart = BarChart::new([bar])
            .max(Some(8))
            .bar_gap(0)
            .style(Style::new().bg(Color::Blue))
            .bar_style(Style::new().fg(Color::Green))
            .label_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 2));
        chart.render(buf.area(), &mut buf);

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
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        BarChart::new([Bar::new(5, "x")]).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
