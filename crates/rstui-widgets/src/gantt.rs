//! [`Gantt`] — a project-timeline chart: one labelled bar per task on a shared
//! integer time axis, the planning surface a PM TUI pins in a pane (sprint
//! plans, release roadmaps, rollout schedules, CI stage timelines).
//!
//! # A pure projection, like every other widget
//!
//! `Gantt` owns no state. It is a list of caller-built [`GanttTask`]s (a label
//! [`Line`] plus a `start`/`end` on a caller-chosen integer axis and a
//! `progress` percent) and an optional explicit range and `today` column; the
//! reducer decides what the tasks and the axis units are (days since the epoch,
//! sprint indices, hour offsets — the widget never interprets them) and the
//! widget only projects them. There is **no date math at all** — exactly the
//! [`Calendar`](crate::Calendar) discipline — so it adds no dependency and
//! stays deterministically headless-testable, composing with the Elm
//! `view(&self)` model exactly like [`List`](crate::List) and
//! [`BarChart`](crate::BarChart).
//!
//! # Sub-cell precision, reusing the [`BarChart`](crate::BarChart) idea
//!
//! A task's `[start, end)` span rarely lands on whole cells, so — exactly like
//! [`BarChart`](crate::BarChart) and [`Gauge`](crate::Gauge) — each bar end is
//! drawn with the horizontal eighth-block glyph (`▏…█`) nearest the true
//! fractional position on the time axis, not rounded to a whole cell. Each
//! glyph is one Unicode scalar, so it maps 1:1 onto a
//! [`Cell`](rstui_core::Buffer) with no grapheme machinery — the same reasoning
//! the gauge ramp and [`Block`] borders use. The geometry maps the axis range
//! `[lo, hi]` linearly across the bar columns; an empty range collapses to a
//! single column and every task draws at the origin (no division by zero).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no tasks, an `end` before its `start` (clamped to a zero-length bar),
//! a zero-span range, a `today` outside the range (clipped off-pane), a
//! `progress` above `100` (clamped), and an area too narrow/short for the bars
//! or labels are all safe clips/no-ops — never a panic. An optional framing
//! [`Block`] follows the container-widget convention.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The eight left-aligned block elements for the bar ends, `1/8` … `8/8` (the
/// same horizontal ramp [`BarChart`](crate::BarChart) and [`Gauge`](crate::Gauge)
/// fill with).
const HORIZONTAL_EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// One task row of a [`Gantt`] chart: a label [`Line`] and its `[start, end)`
/// span on the caller's shared integer time axis, plus a completed percentage.
///
/// Build the label from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`); style it through the
/// [`Line`] it wraps. The axis units are entirely the caller's — the widget
/// does **no date math** (see the [module docs](self)).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GanttTask<'a> {
    /// The task's row label.
    label: Line<'a>,
    /// The inclusive start position on the shared integer time axis.
    start: u64,
    /// The exclusive end position on the shared integer time axis. An `end`
    /// before `start` renders a zero-length bar (the totality rule).
    end: u64,
    /// The completed fraction as a percent, clamped to `0..=100` at render.
    progress: u16,
}

impl<'a> GanttTask<'a> {
    /// A task labelled `label` spanning `[start, end)` on the caller's shared
    /// integer time axis, with no progress fill.
    ///
    /// An `end` at or before `start` is a zero-length bar (never a panic — the
    /// [`Gauge`](crate::Gauge) totality rule).
    pub fn new(start: u64, end: u64, label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            start,
            end,
            progress: 0,
        }
    }

    /// Sets the completed fraction as a percent; clamped to `0..=100` at render
    /// time (never a panic).
    #[must_use]
    pub fn progress(mut self, progress: u16) -> Self {
        self.progress = progress;
        self
    }
}

/// A project-timeline chart: one labelled bar per task on a shared integer time
/// axis, with sub-cell-precise bar ends, an optional progress fill, an optional
/// `today` marker column, and an optional framing [`Block`].
///
/// Each row is a left label gutter (at most half the width, sized to the
/// longest label exactly like a horizontal [`BarChart`](crate::BarChart)) plus
/// a bar spanning `[start, end)` mapped linearly across the remaining columns.
/// The axis range is the caller's [`range`](Self::range) or, when unset, the
/// minimum `start` to the maximum `end` of the tasks. Styling is a base
/// [`Style`] (filling the area) with a [`bar_style`](Self::bar_style) for the
/// bar body, a [`progress_style`](Self::progress_style) for the completed
/// fraction, a [`today_style`](Self::today_style) for the marker column, and a
/// [`label_style`](Self::label_style) beneath each label's own
/// [`Line`]/[`Span`](rstui_core::Span) styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Gantt, GanttTask};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 7, 2));
/// Gantt::new([
///     GanttTask::new(0, 4, "a"),
///     GanttTask::new(2, 4, "b"),
/// ])
/// .range(Some((0, 4)))
/// .render(buf.area(), &mut buf);
///
/// // label_w = 1; the 6-col bar area maps [0,4]. Task "a" spans the lot.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'a'); // label
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '█'); // bar
/// ```
#[derive(Debug, Default, Clone)]
pub struct Gantt<'a> {
    tasks: Vec<GanttTask<'a>>,
    range: Option<(u64, u64)>,
    today: Option<u64>,
    block: Option<Block<'a>>,
    style: Style,
    bar_style: Style,
    progress_style: Style,
    today_style: Style,
    label_style: Style,
}

impl<'a> Gantt<'a> {
    /// A timeline of `tasks`, auto-ranged from the minimum `start` to the
    /// maximum `end`, with no `today` marker and no frame.
    pub fn new<I>(tasks: I) -> Self
    where
        I: IntoIterator<Item = GanttTask<'a>>,
    {
        Self {
            tasks: tasks.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets the explicit axis range `[lo, hi]`, or `None` to auto-range to the
    /// minimum `start` … maximum `end` of the tasks.
    ///
    /// A zero-span range (`lo >= hi`) collapses to a single column and every
    /// bar draws at the origin (never a panic — the [`Gauge`](crate::Gauge)
    /// totality rule).
    #[must_use]
    pub fn range(mut self, range: Option<(u64, u64)>) -> Self {
        self.range = range;
        self
    }

    /// Sets a `today` marker column on the axis (a vertical rule), or `None`.
    /// A position outside the range is clipped off-pane.
    #[must_use]
    pub fn today(mut self, today: Option<u64>) -> Self {
        self.today = today;
        self
    }

    /// Frames the chart in `block`; rows render into
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

    /// Sets the [`Style`] the bar body is drawn with, over the base.
    #[must_use]
    pub fn bar_style(mut self, style: Style) -> Self {
        self.bar_style = style;
        self
    }

    /// Sets the [`Style`] the completed fraction of each bar is drawn with,
    /// over the base (patched after [`bar_style`](Self::bar_style)).
    #[must_use]
    pub fn progress_style(mut self, style: Style) -> Self {
        self.progress_style = style;
        self
    }

    /// Sets the [`Style`] the `today` marker column is drawn with, over the
    /// base.
    #[must_use]
    pub fn today_style(mut self, style: Style) -> Self {
        self.today_style = style;
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

/// The eighth-column position of `value` along an axis `[lo, hi]` mapped across
/// `span` cells (rounded to the nearest eighth). `hi` is already `> lo`, so
/// there is no division by zero.
fn axis_eighths(value: u64, lo: u64, hi: u64, span: u16) -> u64 {
    let clamped = value.clamp(lo, hi);
    let offset = u128::from(clamped - lo);
    let range = u128::from(hi - lo);
    let total = u128::from(span) * 8;
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

impl Widget for Gantt<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Gantt {
            tasks,
            range,
            today,
            block,
            style,
            bar_style,
            progress_style,
            today_style,
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
        if tasks.is_empty() {
            return;
        }

        // A left label column, at most half the width, sized to the longest
        // label — exactly the horizontal `BarChart` rule.
        let longest = tasks.iter().map(|t| t.label.width()).max().unwrap_or(0) as u16;
        let label_w = longest.min(inner.width / 2);
        let bar_x0 = inner.left().saturating_add(label_w);
        let bar_cols = inner.width.saturating_sub(label_w);
        let right = inner.right();
        let bottom = inner.bottom();
        if bar_cols == 0 {
            // Only the label column fits; still draw the labels, no bars.
            let mut y = inner.top();
            for task in &tasks {
                if y >= bottom {
                    break;
                }
                stamp_line(
                    buf,
                    &task.label,
                    style.patch(label_style),
                    inner.left(),
                    y,
                    right,
                );
                y = y.saturating_add(1);
            }
            return;
        }

        // The axis range: the caller's, or the min start … max end of the
        // tasks. A zero-span range (`lo >= hi`) has no time extent, so every
        // bar collapses to nothing (only the labels show) — total, never a
        // division by zero.
        let (lo, hi) = match range {
            Some((lo, hi)) => (lo, hi),
            None => {
                let lo = tasks.iter().map(|t| t.start).min().unwrap_or(0);
                let hi = tasks.iter().map(|t| t.end.max(t.start)).max().unwrap_or(0);
                (lo, hi)
            }
        };
        let span_zero = lo >= hi;

        let bar_glyph = style.patch(bar_style);
        let prog_glyph = bar_glyph.patch(progress_style);

        let mut y = inner.top();
        for task in &tasks {
            if y >= bottom {
                break;
            }

            // The label in the left column on this row.
            if label_w > 0 {
                stamp_line(
                    buf,
                    &task.label,
                    style.patch(label_style),
                    inner.left(),
                    y,
                    bar_x0,
                );
            }

            if span_zero {
                // No axis extent: the bar collapses, only the label shows.
                y = y.saturating_add(1);
                continue;
            }

            // The bar covers the eighth interval `[start_e, end_e)`; an `end`
            // before `start` is a zero-length bar (empty interval).
            let task_end = task.end.max(task.start);
            let start_e = axis_eighths(task.start, lo, hi, bar_cols);
            let end_e = axis_eighths(task_end, lo, hi, bar_cols);
            // The completed split point inside the interval, by progress
            // percent: the boundary eighth up to which the fill is "done".
            let span_e = end_e.saturating_sub(start_e);
            let pct = u128::from(task.progress.min(100));
            let done_e = start_e + ((u128::from(span_e) * pct + 50) / 100) as u64;

            let first_col = (start_e / 8) as u16;
            let last_col = (end_e.saturating_sub(1) / 8) as u16;
            for col in first_col..=last_col {
                let x = bar_x0.saturating_add(col);
                if x >= right {
                    break;
                }
                // Overlap of this cell's eighth interval with the bar's.
                let cell_lo = u64::from(col) * 8;
                let cell_hi = cell_lo + 8;
                let ov_lo = start_e.max(cell_lo);
                let ov_hi = end_e.min(cell_hi);
                if ov_hi <= ov_lo {
                    continue;
                }
                let covered = (ov_hi - ov_lo) as usize;
                // A whole-cell run is a full block; a partial overlap is the
                // nearest left-anchored eighth wedge (the horizontal ramp).
                let glyph = if covered >= 8 {
                    '█'
                } else {
                    HORIZONTAL_EIGHTHS[covered.clamp(1, 8) - 1]
                };
                // The cell is the progress style while its whole overlap is
                // within the completed eighths, else the bar style.
                let glyph_style = if done_e >= ov_hi {
                    prog_glyph
                } else {
                    bar_glyph
                };
                buf.set_cell(Position::new(x, y), glyph, glyph_style);
            }

            y = y.saturating_add(1);
        }

        // The `today` marker column, drawn last over the whole bar height so
        // it reads as one vertical rule. Clipped off-pane when out of range or
        // when the axis has no extent.
        if let Some(t) = today {
            if !span_zero && t >= lo && t <= hi {
                let col_e = axis_eighths(t, lo, hi, bar_cols);
                let col = (col_e / 8).min(u64::from(bar_cols.saturating_sub(1))) as u16;
                let x = bar_x0.saturating_add(col);
                if x < right {
                    let marker = style.patch(today_style);
                    let rows = (tasks.len() as u16).min(bottom.saturating_sub(inner.top()));
                    for r in 0..rows {
                        let my = inner.top().saturating_add(r);
                        buf.set_cell(Position::new(x, my), '│', marker);
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
    fn each_task_is_a_labelled_bar_on_a_shared_axis() {
        let g =
            Gantt::new([GanttTask::new(0, 4, "a"), GanttTask::new(2, 4, "b")]).range(Some((0, 4)));
        // label_w = 1; 6-col bar area maps [0,4]. a: [0,4] → full 6 cols;
        // b: [2,4] → starts at col 3 (2/4 of 6).
        assert_eq!(lines(g, 7, 2), "a██████\nb   ███\n");
    }

    #[test]
    fn a_fractional_bar_end_uses_a_sub_cell_glyph() {
        // Range [0,4] over 4 cols: 1 col = 1 unit. Task [0,1) ends after 1
        // col; task [0,3) ends after 3 cols. A non-whole end lands a wedge.
        let g = Gantt::new([GanttTask::new(0, 1, "x")]).range(Some((0, 8)));
        // 4 cols, range span 8 → 0.5 cell per unit; end at 1 → 0.5 cell → ▌.
        assert_eq!(lines(g, 5, 1), "x▌   \n");
    }

    #[test]
    fn auto_range_spans_min_start_to_max_end() {
        let g = Gantt::new([GanttTask::new(2, 4, "a"), GanttTask::new(0, 8, "b")]);
        // lo = 0, hi = 8. 8-col bar area. a: [2,4] → cols 2..4; b: [0,8] full.
        assert_eq!(lines(g, 9, 2), "a  ██    \nb████████\n");
    }

    #[test]
    fn progress_fills_the_completed_fraction_with_its_own_style() {
        let g = Gantt::new([GanttTask::new(0, 4, "t").progress(50)])
            .range(Some((0, 4)))
            .bar_style(Style::new().fg(Color::Blue))
            .progress_style(Style::new().fg(Color::Green));
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        g.render(buf.area(), &mut buf);
        // 4-col bar [0,4], 50% → first 2 cols progress (green), last 2 bar.
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().fg, Color::Green);
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().fg, Color::Green);
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().fg, Color::Blue);
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().fg, Color::Blue);
    }

    #[test]
    fn progress_above_one_hundred_clamps_to_a_full_fill() {
        let g = Gantt::new([GanttTask::new(0, 4, "t").progress(999)])
            .range(Some((0, 4)))
            .bar_style(Style::new().fg(Color::Blue))
            .progress_style(Style::new().fg(Color::Green));
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        g.render(buf.area(), &mut buf);
        for x in 1..5 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().fg, Color::Green);
        }
    }

    #[test]
    fn an_end_before_start_is_a_zero_length_bar() {
        let g = Gantt::new([GanttTask::new(4, 1, "x")]).range(Some((0, 8)));
        // end < start → clamped to a zero-length bar; only the label shows.
        assert_eq!(lines(g, 9, 1), "x        \n");
    }

    #[test]
    fn a_today_marker_is_a_vertical_rule_over_the_rows() {
        let g = Gantt::new([GanttTask::new(0, 8, "a"), GanttTask::new(0, 8, "b")])
            .range(Some((0, 8)))
            .today(Some(4));
        // 8-col bar, today at 4 → col 4; a vertical bar over both rows.
        let out = lines(g, 9, 2);
        assert_eq!(out, "a████│███\nb████│███\n");
    }

    #[test]
    fn a_today_marker_outside_the_range_is_clipped() {
        let g = Gantt::new([GanttTask::new(0, 4, "a")])
            .range(Some((0, 4)))
            .today(Some(99));
        // today past hi → no rule drawn anywhere.
        assert_eq!(lines(g, 5, 1), "a████\n");
    }

    #[test]
    fn a_zero_span_range_collapses_bars_to_the_origin() {
        let g = Gantt::new([GanttTask::new(5, 9, "x")]).range(Some((7, 7)));
        // lo == hi → hi floors to lo+1; the bar collapses, only the label.
        assert_eq!(lines(g, 6, 1), "x     \n");
    }

    #[test]
    fn no_tasks_renders_nothing_but_a_clear_area() {
        let g = Gantt::new(Vec::<GanttTask>::new());
        assert_eq!(lines(g, 5, 2), "     \n     \n");
    }

    #[test]
    fn a_block_frames_the_chart_in_the_inner_area() {
        let g = Gantt::new([GanttTask::new(0, 4, "x")])
            .range(Some((0, 4)))
            .block(Block::bordered());
        // 7×3 bordered → inner Rect(1,1,5,1): label_w = 1, 4-col bar full.
        assert_eq!(lines(g, 7, 3), "┌─────┐\n│x████│\n└─────┘\n");
    }

    #[test]
    fn no_tasks_with_a_block_still_renders_the_block() {
        let g = Gantt::new(Vec::<GanttTask>::new()).block(Block::bordered());
        assert_eq!(lines(g, 4, 3), "┌──┐\n│  │\n└──┘\n");
    }

    #[test]
    fn a_narrow_area_clips_the_bars() {
        let g = Gantt::new([GanttTask::new(0, 8, "task")]).range(Some((0, 8)));
        // Width 4: label_w = min(4, 4/2) = 2 ("ta"); 2-col bar clipped.
        assert_eq!(lines(g, 4, 1), "ta██\n");
    }

    #[test]
    fn style_cascades_base_then_bar_and_label_styles() {
        let task = GanttTask::new(
            0,
            4,
            Line::from(Span::styled("L", Style::new().fg(Color::Red))),
        );
        let g = Gantt::new([task])
            .range(Some((0, 4)))
            .style(Style::new().bg(Color::Blue))
            .bar_style(Style::new().fg(Color::Green))
            .label_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        g.render(buf.area(), &mut buf);

        let l = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(l.symbol, 'L');
        assert_eq!(l.fg, Color::Red); // span fg wins
        assert!(l.modifier.contains(Modifier::BOLD)); // label_style cascades
        assert_eq!(l.bg, Color::Blue); // base fill cascades

        let b = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(b.symbol, '█');
        assert_eq!(b.fg, Color::Green); // bar_style fg
        assert_eq!(b.bg, Color::Blue); // base fill cascades
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        Gantt::new([GanttTask::new(0, 4, "x")]).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
