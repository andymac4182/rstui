//! [`TraceWaterfall`] — a distributed-trace span waterfall (Jaeger/Tempo
//! style), the core observability "trace" view: one row per span, a left name
//! gutter, and a duration bar positioned on a single time axis.
//!
//! # A flattened projection, like every other widget
//!
//! A trace is a tree of spans, but rstui's `App::view` (in `rstui-runtime`)
//! takes `&self` — a view never walks or mutates a span graph at render time.
//! So, exactly like [`Tree`](crate::Tree), `TraceWaterfall` is a pure
//! projection of a **caller-owned flattened `&[TraceSpan]`** in display order:
//! each [`TraceSpan`] carries only its [`depth`](TraceSpan::new), `start`, and
//! `duration` on a single `[0, total]` axis. Which spans exist, which are
//! expanded, and which is [`selected`](TraceWaterfall::selected) is ordinary
//! application state the reducer owns and rebuilds in `update`; the widget
//! reads that slice and the `selected` index into it — it never writes them.
//! That keeps it deterministically headless-testable and composes with the Elm
//! `view(&self)` model exactly like [`List`](crate::List) and
//! [`Tree`](crate::Tree).
//!
//! # Sub-cell precision, reusing the [`BarChart`](crate::BarChart) idea
//!
//! A span's end rarely lands on a whole cell, so — exactly like
//! [`BarChart`](crate::BarChart) horizontal — the boundary cell is drawn with
//! the eighth-block glyph nearest the true fraction (the *horizontal* ramp
//! `▏…█`), not rounded to a whole cell. Each glyph is one Unicode scalar, so it
//! maps 1:1 onto a [`Cell`](rstui_core::Buffer) with no grapheme machinery —
//! the same reasoning the gauge ramp and [`Block`] borders use. A span whose
//! `duration` is zero is a single instant marker `▏`.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no spans, a zero `total` (names only), more spans than rows (the tail
//! is clipped), a name wider than the gutter (clipped), and a span starting or
//! ending past the axis (clamped) are all safe clips/no-ops — never a panic.
//! An optional framing [`Block`] follows the container-widget convention;
//! span-relative colouring and a collapse/expand affordance are deliberately
//! deferred additive follow-ups, not smuggled into this slice.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Position, Rect, Widget};
//! use rstui_widgets::{TraceSpan, TraceWaterfall};
//!
//! let spans = [
//!     TraceSpan::new(0, 0, 8, "GET /"),
//!     TraceSpan::new(1, 4, 4, "db.query"),
//! ];
//! let mut buf = Buffer::empty(Rect::new(0, 0, 16, 2));
//! TraceWaterfall::new(&spans)
//!     .total(Some(8))
//!     .name_width(8)
//!     .render(buf.area(), &mut buf);
//!
//! // The root name is flush in the gutter; the child is indented one column.
//! assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'G');
//! assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'd');
//! // The root bar fills the whole axis from the gutter edge.
//! assert_eq!(buf.get(Position::new(8, 0)).unwrap().symbol, '█');
//! ```

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The eight left-aligned block elements for the duration bar, `1/8` … `8/8`
/// (the same ramp [`BarChart`](crate::BarChart) fills its horizontal bars
/// with).
const HORIZONTAL_EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// One span of a [`TraceWaterfall`]: a name [`Line`] at an indentation
/// [`depth`](TraceSpan::new), plus a `start` and `duration` on the single
/// `[0, total]` time axis.
///
/// The caller (who owns the real trace) flattens the visible spans into a
/// `&[TraceSpan]` in display order. `start` and `duration` are opaque integer
/// time units — the widget never assumes a unit such as milliseconds. Build
/// the name from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`); style the whole row
/// with [`style`](Self::style).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TraceSpan<'a> {
    /// Indentation depth; column 0 is depth 0 (one gutter column per level).
    pub depth: u16,
    /// Offset of the span's start on the `[0, total]` time axis.
    pub start: u64,
    /// Length of the span in the same axis units (`0` is an instant marker).
    pub duration: u64,
    /// The span name drawn in the left gutter, indented by `depth`.
    pub name: Line<'a>,
    /// The base [`Style`] for this row (its `fg` overrides the bar glyph fg).
    pub style: Style,
}

impl<'a> TraceSpan<'a> {
    /// A span at `depth` named `name` (any value convertible to a [`Line`]),
    /// starting at `start` and lasting `duration` on the `[0, total]` axis.
    pub fn new(depth: u16, start: u64, duration: u64, name: impl Into<Line<'a>>) -> Self {
        Self {
            depth,
            start,
            duration,
            name: name.into(),
            style: Style::default(),
        }
    }

    /// Sets the row's base [`Style`]; its `fg` overrides the bar glyph colour
    /// for this span, beneath the name's own [`Line`]/[`Span`](rstui_core::Span)
    /// styles.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

/// A distributed-trace span waterfall: one row per [`TraceSpan`], a left name
/// gutter, and a duration bar on a single time axis, with an optional framing
/// [`Block`].
///
/// Rows render top→down (row `i` is span `i`); more spans than rows clip the
/// tail. Each row is a [`name_width`](Self::name_width)-wide gutter (the name,
/// indented one column per `depth`, clipped) then a bar at
/// `x0 = name_width + round(start / total * bar_w)` of length
/// `round(duration / total * bar_w)` drawn with full blocks plus one
/// fractional eighth-block boundary cell. With
/// [`duration_labels`](Self::duration_labels) a right-aligned ` {duration}`
/// is drawn after the bar when it fits (the integer is formatted verbatim — no
/// unit is assumed).
///
/// Styling is a base [`Style`] (filling the content area) with a
/// [`bar_style`](Self::bar_style) for the glyphs (a span's own
/// [`style`](TraceSpan::style) `fg` overrides it) and a
/// [`name_style`](Self::name_style) beneath each name's own
/// [`Line`]/[`Span`](rstui_core::Span) styles. The
/// [`selected`](Self::selected) row is patched with
/// [`selected_style`](Self::selected_style) **last**, across the full inner
/// width, so the gutter, bar, and trailing padding read as one bar (the
/// [`Tree`](crate::Tree) highlight model).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{TraceSpan, TraceWaterfall};
///
/// let spans = [TraceSpan::new(0, 0, 4, "root")];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
/// TraceWaterfall::new(&spans)
///     .total(Some(8))
///     .name_width(6)
///     .duration_labels(false)
///     .render(buf.area(), &mut buf);
///
/// // 4/8 of a 6-cell bar area = 3 full cells from the gutter edge.
/// assert_eq!(buf.get(Position::new(6, 0)).unwrap().symbol, '█');
/// assert_eq!(buf.get(Position::new(8, 0)).unwrap().symbol, '█');
/// assert_eq!(buf.get(Position::new(9, 0)).unwrap().symbol, ' ');
/// ```
#[derive(Debug, Clone)]
pub struct TraceWaterfall<'a> {
    spans: &'a [TraceSpan<'a>],
    total: Option<u64>,
    name_width: u16,
    selected: Option<usize>,
    duration_labels: bool,
    selected_style: Style,
    bar_style: Style,
    name_style: Style,
    block: Option<Block<'a>>,
    style: Style,
}

impl<'a> TraceWaterfall<'a> {
    /// A waterfall projecting `spans` (span `0` topmost), the axis auto-scaled
    /// to the latest span end, a 24-column gutter, duration labels on, nothing
    /// selected, no frame.
    #[must_use]
    pub fn new(spans: &'a [TraceSpan<'a>]) -> Self {
        Self {
            spans,
            total: None,
            name_width: 24,
            selected: None,
            duration_labels: true,
            selected_style: Style::default(),
            bar_style: Style::default(),
            name_style: Style::default(),
            block: None,
            style: Style::default(),
        }
    }

    /// Sets the axis denominator, or `None` to auto-scale to the latest
    /// `start + duration`.
    ///
    /// A `Some(0)` total (or an auto-scaled trace with no extent) draws the
    /// names only — no bars (never a panic, the [`Gauge`](crate::Gauge)
    /// totality rule).
    #[must_use]
    pub fn total(mut self, total: Option<u64>) -> Self {
        self.total = total;
        self
    }

    /// Sets the left gutter width in columns (default `24`), clipped to the
    /// content area; the bar gets the rest.
    #[must_use]
    pub fn name_width(mut self, name_width: u16) -> Self {
        self.name_width = name_width;
        self
    }

    /// Sets which span index is highlighted, or `None` for none.
    ///
    /// An index outside the visible rows simply paints no bar — the caller
    /// owns the slice (see the [module docs](self)).
    #[must_use]
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets whether a right-aligned ` {duration}` label is drawn after each
    /// bar when it fits (default `true`).
    ///
    /// The duration is an opaque integer formatted verbatim; no time unit is
    /// assumed.
    #[must_use]
    pub fn duration_labels(mut self, duration_labels: bool) -> Self {
        self.duration_labels = duration_labels;
        self
    }

    /// Sets the [`Style`] patched over the selected row.
    ///
    /// Patched **last** in the cascade, so it overrides per-span styling, and
    /// applied across the full inner width so the selection reads as one bar.
    #[must_use]
    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Sets the [`Style`] the bar glyphs are drawn with, over the base; a
    /// span's own [`style`](TraceSpan::style) `fg` overrides it per row.
    #[must_use]
    pub fn bar_style(mut self, style: Style) -> Self {
        self.bar_style = style;
        self
    }

    /// Sets the base [`Style`] for names, beneath each name's own
    /// [`Line`]/[`Span`](rstui_core::Span) styles.
    #[must_use]
    pub fn name_style(mut self, style: Style) -> Self {
        self.name_style = style;
        self
    }

    /// Frames the waterfall in `block`; rows render into
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
}

/// The number of eighths a `value` spans of a `cells`-cell axis against
/// `total` (rounded to the nearest eighth; `total` is already `>= 1`).
fn eighths(value: u64, total: u64, cells: u16) -> u64 {
    let clamped = u128::from(value.min(total));
    let span = u128::from(cells) * 8;
    ((clamped * span + u128::from(total) / 2) / u128::from(total)) as u64
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

impl Widget for TraceWaterfall<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let TraceWaterfall {
            spans,
            total,
            name_width,
            selected,
            duration_labels,
            selected_style,
            bar_style,
            name_style,
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

        // Base fills the content area so a background covers the whole pane
        // (including the gutter and rows past the last span); glyphs layer the
        // base → bar/name → span cascade on top.
        buf.set_style(inner, style);
        if spans.is_empty() {
            return;
        }

        // The axis denominator: the caller's, or the latest span end. Zero
        // means no extent — only the names are drawn.
        let axis = total.unwrap_or_else(|| {
            spans
                .iter()
                .map(|s| s.start.saturating_add(s.duration))
                .max()
                .unwrap_or(0)
        });

        let left = inner.left();
        let top = inner.top();

        // The gutter is clipped to the content area; the bar gets the rest.
        let gutter_w = name_width.min(inner.width);
        let bar_x0 = left.saturating_add(gutter_w);
        let bar_w = inner.width.saturating_sub(gutter_w);

        for (row, span) in spans.iter().take(inner.height as usize).enumerate() {
            let y = top.saturating_add(row as u16);
            let is_selected = selected == Some(row);

            if is_selected {
                // The selection bar: highlight patched over the base fill
                // across the full inner width, so the gutter, bar, and
                // trailing padding read as one contiguous block.
                buf.set_style(Rect::new(left, y, inner.width, 1), selected_style);
            }

            // The name, indented one column per depth, clipped to the gutter;
            // the highlight is patched last on the selected row.
            let mut name_base = style.patch(name_style);
            if is_selected {
                name_base = name_base.patch(selected_style);
            }
            let indent = span.depth.min(gutter_w);
            let name_x0 = left.saturating_add(indent);
            let gutter_right = left.saturating_add(gutter_w);
            stamp_line(buf, &span.name, name_base, name_x0, y, gutter_right);

            // No bar room or no axis → names only for this row.
            if bar_w == 0 || axis == 0 {
                continue;
            }

            // The bar glyph cascade: base → bar_style, then the span's own
            // fg (a per-span colour), then the highlight last when selected.
            let mut bar_glyph = style.patch(bar_style);
            if span.style != Style::default() {
                bar_glyph = bar_glyph.patch(span.style);
            }
            if is_selected {
                bar_glyph = bar_glyph.patch(selected_style);
            }

            // Offset is rounded to the nearest cell; the span is clamped to
            // the axis so a start past the end still lands on a cell.
            let start = span.start.min(axis);
            let off_e = eighths(start, axis, bar_w);
            let off_cells = (off_e / 8) as u16;
            let span_x0 = bar_x0.saturating_add(off_cells.min(bar_w));
            let span_right = bar_x0.saturating_add(bar_w);

            if span.duration == 0 {
                // An instant: a single sub-cell marker, never invisible.
                if span_x0 < span_right {
                    buf.set_cell(Position::new(span_x0, y), HORIZONTAL_EIGHTHS[0], bar_glyph);
                }
            } else {
                // Full blocks plus one fractional boundary cell, exactly like
                // a `BarChart` horizontal bar. The length is `duration` scaled
                // over the *whole* axis (`bar_w`), not the post-offset
                // remainder — `start` only shifts the bar, it never compresses
                // it (the module-doc contract); the draw loop clips the tail
                // at `span_right`. A non-zero duration shows at least one
                // eighth so a tiny span is still visible.
                let dur_e = eighths(span.duration, axis, bar_w).max(1);
                let full = (dur_e / 8) as u16;
                let rem = (dur_e % 8) as u16;
                for c in 0..full {
                    let x = span_x0.saturating_add(c);
                    if x >= span_right {
                        break;
                    }
                    buf.set_cell(Position::new(x, y), '█', bar_glyph);
                }
                if rem > 0 {
                    let x = span_x0.saturating_add(full);
                    if x < span_right {
                        buf.set_cell(
                            Position::new(x, y),
                            HORIZONTAL_EIGHTHS[(rem - 1) as usize],
                            bar_glyph,
                        );
                    }
                }
            }

            // A right-aligned ` {duration}` after the bar, only if it fits
            // entirely inside the bar area (the integer is opaque — no unit).
            if duration_labels {
                let text = format!(" {duration}", duration = span.duration);
                let label_w = text.chars().count() as u16;
                if label_w <= bar_w {
                    let label_x0 = span_right.saturating_sub(label_w);
                    let mut label_style = style.patch(name_style);
                    if is_selected {
                        label_style = label_style.patch(selected_style);
                    }
                    stamp_line(buf, &Line::raw(text), label_style, label_x0, y, span_right);
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
    fn one_row_per_span_with_a_gutter_then_a_bar() {
        let spans = [
            TraceSpan::new(0, 0, 8, "root"),
            TraceSpan::new(1, 0, 4, "sub"),
        ];
        // gutter 5, axis 8, bar area 5. root=8/8 → █████; sub=4/8 → 2.5 →
        // ██▌. The child name is indented one column.
        let wf = TraceWaterfall::new(&spans)
            .total(Some(8))
            .name_width(5)
            .duration_labels(false);
        assert_eq!(lines(wf, 10, 2), "root █████\n sub ██▌  \n");
    }

    #[test]
    fn a_start_offset_shifts_the_bar_right_on_the_axis() {
        let spans = [TraceSpan::new(0, 4, 4, "x")];
        // axis 8, bar area 8. start 4/8 → offset 4 cells; duration 4 over the
        // remaining 4 cells → 4 full blocks.
        let wf = TraceWaterfall::new(&spans)
            .total(Some(8))
            .name_width(0)
            .duration_labels(false);
        assert_eq!(lines(wf, 8, 1), "    ████\n");
    }

    #[test]
    fn a_fractional_bar_uses_a_sub_cell_glyph() {
        let spans = [TraceSpan::new(0, 0, 1, "x")];
        // axis 4, bar area 1 → 1/4 of a cell → 2 eighths → ▎.
        let wf = TraceWaterfall::new(&spans)
            .total(Some(4))
            .name_width(0)
            .duration_labels(false);
        assert_eq!(lines(wf, 1, 1), "▎\n");
    }

    #[test]
    fn a_zero_duration_span_is_a_single_instant_marker() {
        let spans = [TraceSpan::new(0, 4, 0, "x")];
        // duration 0 → one ▏ marker at the start offset (4/8 of 8 cells).
        let wf = TraceWaterfall::new(&spans)
            .total(Some(8))
            .name_width(0)
            .duration_labels(false);
        assert_eq!(lines(wf, 8, 1), "    ▏   \n");
    }

    #[test]
    fn a_tiny_non_zero_duration_still_shows_one_eighth() {
        let spans = [TraceSpan::new(0, 0, 1, "x")];
        // 1 against an axis of 1000 over 1 cell rounds to 0 eighths, but a
        // non-zero duration is never invisible — it floors at ▏.
        let wf = TraceWaterfall::new(&spans)
            .total(Some(1000))
            .name_width(0)
            .duration_labels(false);
        assert_eq!(lines(wf, 1, 1), "▏\n");
    }

    #[test]
    fn auto_scale_maps_the_latest_span_end_to_the_full_axis() {
        let spans = [TraceSpan::new(0, 0, 10, "a"), TraceSpan::new(1, 5, 5, "b")];
        // No total → axis = max(start+duration) = 10. a=10/10 over 4 cells →
        // ████; b starts 5/10 → offset 2, duration 5 over 2 → ██.
        let wf = TraceWaterfall::new(&spans)
            .name_width(0)
            .duration_labels(false);
        assert_eq!(lines(wf, 4, 2), "████\n  ██\n");
    }

    #[test]
    fn a_zero_total_draws_names_only() {
        let spans = [TraceSpan::new(0, 0, 5, "name")];
        let wf = TraceWaterfall::new(&spans)
            .total(Some(0))
            .name_width(4)
            .duration_labels(false);
        // Axis 0 → no bar, just the clipped name in the gutter.
        assert_eq!(lines(wf, 8, 1), "name    \n");
    }

    #[test]
    fn the_name_is_indented_one_column_per_depth_and_clipped() {
        let spans = [TraceSpan::new(0, 0, 0, "a"), TraceSpan::new(2, 0, 0, "bbb")];
        // Depth 0 flush; depth 2 indented two columns. Gutter width 4 clips
        // "bbb" to "bb" (only cols 2..4 fit). No labels, instant markers.
        let wf = TraceWaterfall::new(&spans)
            .total(Some(8))
            .name_width(4)
            .duration_labels(false);
        // Row 1: two indent blanks, "bb" (the gutter clips the third char),
        // then the instant marker at offset 0 in the bar area.
        assert_eq!(lines(wf, 5, 2), "a   ▏\n  bb▏\n");
    }

    #[test]
    fn more_spans_than_rows_clip_the_tail() {
        let spans = [
            TraceSpan::new(0, 0, 0, "a"),
            TraceSpan::new(0, 0, 0, "b"),
            TraceSpan::new(0, 0, 0, "c"),
        ];
        let wf = TraceWaterfall::new(&spans)
            .total(Some(8))
            .name_width(2)
            .duration_labels(false);
        // Only the first two rows fit a height-2 area.
        assert_eq!(lines(wf, 3, 2), "a ▏\nb ▏\n");
    }

    #[test]
    fn a_duration_label_is_drawn_right_aligned_after_the_bar() {
        let spans = [TraceSpan::new(0, 0, 4, "x")];
        // axis 8, bar area 8. bar = ████; label " 4" right-aligned in the
        // 8-cell bar area (overwrites the tail of the blank track).
        let wf = TraceWaterfall::new(&spans).total(Some(8)).name_width(0);
        assert_eq!(lines(wf, 8, 1), "████   4\n");
    }

    #[test]
    fn a_duration_label_too_wide_for_the_bar_area_is_dropped() {
        // " 100000" is 7 wide but the bar area is only 2 cells → no label;
        // just the (rounded) bar.
        let spans = [TraceSpan::new(0, 0, 100_000, "x")];
        let wf = TraceWaterfall::new(&spans)
            .total(Some(100_000))
            .name_width(0);
        // 100000/100000 over 2 cells = full bar; label 7 wide > 2 → dropped.
        assert_eq!(lines(wf, 2, 1), "██\n");
    }

    #[test]
    fn the_selected_row_is_a_full_width_bar_over_gutter_and_track() {
        let spans = [TraceSpan::new(0, 0, 2, "x")];
        let wf = TraceWaterfall::new(&spans)
            .total(Some(8))
            .name_width(2)
            .duration_labels(false)
            .selected(Some(0))
            .selected_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        wf.render(buf.area(), &mut buf);
        // Gutter, bar, and trailing track all share the highlight background.
        for x in 0..6 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'x');
    }

    #[test]
    fn a_selection_outside_the_visible_rows_paints_no_bar() {
        let spans = [TraceSpan::new(0, 0, 1, "a"), TraceSpan::new(0, 0, 1, "b")];
        let wf = TraceWaterfall::new(&spans)
            .total(Some(8))
            .name_width(1)
            .selected(Some(5))
            .selected_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        wf.render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..4 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Reset);
            }
        }
    }

    #[test]
    fn style_cascades_base_then_bar_name_and_per_span_fg() {
        let span = TraceSpan::new(
            0,
            0,
            8,
            Line::from(Span::styled("N", Style::new().fg(Color::Red))),
        )
        .style(Style::new().fg(Color::Green));
        let wf = TraceWaterfall::new(std::slice::from_ref(&span))
            .total(Some(8))
            .name_width(1)
            .duration_labels(false)
            .style(Style::new().bg(Color::Blue))
            .bar_style(Style::new().fg(Color::Magenta));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        wf.render(buf.area(), &mut buf);

        // The name: span fg (Red) wins, base bg (Blue) cascades.
        let n = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(n.symbol, 'N');
        assert_eq!(n.fg, Color::Red);
        assert_eq!(n.bg, Color::Blue);

        // The bar glyph: the span's own fg (Green) overrides bar_style
        // (Magenta); the base bg (Blue) still cascades.
        let g = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(g.symbol, '█');
        assert_eq!(g.fg, Color::Green);
        assert_eq!(g.bg, Color::Blue);
    }

    #[test]
    fn the_selected_style_is_patched_last_over_the_bar() {
        let spans = [TraceSpan::new(0, 0, 8, "x").style(Style::new().fg(Color::Green))];
        let wf = TraceWaterfall::new(&spans)
            .total(Some(8))
            .name_width(0)
            .duration_labels(false)
            .selected(Some(0))
            .selected_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        wf.render(buf.area(), &mut buf);
        let g = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(g.symbol, '█');
        assert_eq!(g.fg, Color::Green); // span fg survives
        assert!(g.modifier.contains(Modifier::BOLD)); // highlight patched last
    }

    #[test]
    fn a_block_frames_the_waterfall_in_the_inner_area() {
        let spans = [TraceSpan::new(0, 0, 1, "x")];
        let wf = TraceWaterfall::new(&spans)
            .total(Some(8))
            .name_width(1)
            .duration_labels(false)
            .block(Block::bordered());
        // inner 1×1: just the indented name, no bar room.
        assert_eq!(lines(wf, 3, 3), "┌─┐\n│x│\n└─┘\n");
    }

    #[test]
    fn no_spans_with_a_block_still_renders_the_block() {
        let spans: [TraceSpan; 0] = [];
        let wf = TraceWaterfall::new(&spans).block(Block::bordered());
        assert_eq!(lines(wf, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn an_empty_span_slice_just_fills_the_area() {
        let spans: [TraceSpan; 0] = [];
        let wf = TraceWaterfall::new(&spans).total(Some(8));
        assert_eq!(lines(wf, 4, 2), "    \n    \n");
    }

    #[test]
    fn a_name_width_wider_than_the_area_leaves_no_bar_room() {
        let spans = [TraceSpan::new(0, 0, 8, "name")];
        // name_width 99 clips to the area width; no columns left for a bar.
        let wf = TraceWaterfall::new(&spans)
            .total(Some(8))
            .name_width(99)
            .duration_labels(false);
        assert_eq!(lines(wf, 4, 1), "name\n");
    }

    #[test]
    fn a_start_past_the_axis_clamps_without_panic() {
        let spans = [TraceSpan::new(0, 999, 999, "x")];
        // start and duration far past the axis: clamped, the bar lands at the
        // far edge and nothing panics.
        let wf = TraceWaterfall::new(&spans)
            .total(Some(8))
            .name_width(0)
            .duration_labels(false);
        // start clamps to 8/8 → offset 4 cells; the remaining run is the tail.
        let out = lines(wf, 4, 1);
        assert_eq!(out.chars().count(), 5); // 4 cells + newline, no panic
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let spans = [TraceSpan::new(0, 0, 5, "x")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        TraceWaterfall::new(&spans)
            .selected(Some(0))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let spans = [TraceSpan::new(0, 0, 8, "x")];
        let wf = TraceWaterfall::new(&spans)
            .total(Some(8))
            .name_width(0)
            .duration_labels(false);
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        wf.render(Rect::new(2, 3, 2, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 3)).unwrap().symbol, '█');
        assert_eq!(buf.get(Position::new(3, 3)).unwrap().symbol, '█');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }
}
