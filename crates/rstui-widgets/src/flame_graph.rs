//! [`FlameGraph`] — a flame/icicle graph of stack frames: the CPU-profile /
//! trace-profile observability view (where the wall time went, which call path
//! dominates a span).
//!
//! # A flattened projection, like [`Tree`](crate::Tree)
//!
//! A profile is a tree of calls, but rstui's `App::view` (in `rstui-runtime`)
//! takes `&self` — a view never walks or mutates a node graph at render time.
//! So, exactly like [`Tree`](crate::Tree), `FlameGraph` is a pure projection of
//! a **caller-owned flattened `&[FlameFrame]`**: each [`FlameFrame`] carries
//! only its [`depth`](FlameFrame::new) (stack level), its
//! [`start`](FlameFrame::new) and [`width`](FlameFrame::new) along a shared
//! `[0, total]` sample/duration axis, a [`Line`] label and a [`Style`]. Which
//! frames exist, which subtree is zoomed, and which frame is
//! [`selected`](FlameGraph::selected) is ordinary application state the reducer
//! owns and rebuilds in `update` (zooming is "re-flatten the chosen subtree
//! across the full axis"); the widget reads that slice and projects it — it
//! never writes it.
//!
//! That keeps the box geometry unambiguous (a frame is one rectangle: its
//! `start`/`width` map to an x-span, its `depth` to a row) and the whole widget
//! deterministically headless-testable, composing with the Elm `view(&self)`
//! model exactly like [`List`](crate::List) and [`BarChart`](crate::BarChart).
//!
//! # Flame vs. icicle
//!
//! [`inverted`](FlameGraph::inverted) is the only orientation knob: a *flame*
//! graph (the default) puts the root row at the **bottom** and stacks deeper
//! frames upward; an *icicle* graph puts the root at the **top** and grows
//! downward. Both are the same box geometry with the row axis flipped — not two
//! widgets.
//!
//! # Total, never a panic
//!
//! Per the [`BarChart`](crate::BarChart) rule a pure projection is *total*: an
//! empty area, no frames, a zero `total`, a zero-width frame, a frame whose
//! depth row falls outside the area, and a `start`/`width` past the axis are
//! all safe clips/no-ops — never a panic. An optional framing [`Block`]
//! follows the container-widget convention.
//!
//! # Example
//!
//! ```rust
//! use rstui_core::{Buffer, Position, Rect, Widget};
//! use rstui_widgets::{FlameFrame, FlameGraph};
//!
//! let frames = [
//!     FlameFrame::new(0, 0, 100, "main"),
//!     FlameFrame::new(1, 0, 60, "parse"),
//!     FlameFrame::new(1, 60, 40, "eval"),
//! ];
//! let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
//! FlameGraph::new(&frames).total(Some(100)).render(buf.area(), &mut buf);
//!
//! // Flame: the root `main` is the bottom row (the label sits one cell
//! // into its full-width box)…
//! assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'm');
//! // …and its first child `parse` stacks on the row above it.
//! assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'p');
//! ```

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// One frame of a [`FlameGraph`]: a box at stack level
/// [`depth`](FlameFrame::new) spanning [`start`](FlameFrame::new) …
/// `start + `[`width`](FlameFrame::new) along a shared `[0, total]` axis.
///
/// The caller (who owns the real profile tree) flattens the frames to draw
/// into a `Vec<FlameFrame>`; `start`/`width` are in sample or duration units
/// (the same unit [`total`](FlameGraph::total) is in), and `depth` is the
/// stack level with `0` the root row. Build the label from anything a
/// [`Line`] is built from (`&str`, `String`, [`Span`](rstui_core::Span),
/// [`Line`], `Vec<Span>`); style the box with [`style`](Self::style) (the
/// label keeps its own [`Line`]/[`Span`](rstui_core::Span) styles on top).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FlameFrame<'a> {
    /// The stack level; `0` is the root row.
    depth: u16,
    /// The frame's left edge along the shared `[0, total]` axis.
    start: u64,
    /// The frame's extent along the shared `[0, total]` axis.
    width: u64,
    /// The label drawn clipped inside the box.
    label: Line<'a>,
    /// The box fill [`Style`], beneath the label's own line/span styles.
    style: Style,
}

impl<'a> FlameFrame<'a> {
    /// A frame at stack level `depth` spanning `start` … `start + width` on the
    /// shared axis, labelled `label` (anything convertible to a [`Line`]),
    /// unstyled.
    pub fn new(depth: u16, start: u64, width: u64, label: impl Into<Line<'a>>) -> Self {
        Self {
            depth,
            start,
            width,
            label: label.into(),
            style: Style::default(),
        }
    }

    /// Sets the box fill [`Style`], beneath the label's own
    /// [`Line`]/[`Span`](rstui_core::Span) styles.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

/// A flame/icicle graph of a borrowed, caller-owned flattened
/// `&[FlameFrame]`, with an optional framing [`Block`].
///
/// Each [`FlameFrame`] is one rectangle: `start`/`width` map to a horizontal
/// span scaled against [`total`](Self::total) (the max of `start + width` when
/// unset), and `depth` maps to a row honouring [`row_height`](Self::row_height)
/// and [`inverted`](Self::inverted) (flame = root at the bottom; icicle = root
/// at the top). The box is filled with the frame's own [`Style`] over the
/// [`style`](Self::style) base, except the [`selected`](Self::selected) frame,
/// which uses [`selected_style`](Self::selected_style) — the zoomed/highlighted
/// frame the reducer tracks. The label is stamped clipped inside the box.
///
/// # Example
///
/// ```rust
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{FlameFrame, FlameGraph};
///
/// let frames = [
///     FlameFrame::new(0, 0, 8, "root"),
///     FlameFrame::new(1, 0, 4, "a"),
/// ];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 2));
/// FlameGraph::new(&frames)
///     .total(Some(8))
///     .inverted(true)
///     .render(buf.area(), &mut buf);
///
/// // Icicle: the root is the TOP row (its label sits one cell into the
/// // full-width box)…
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'r');
/// // …and its child grows downward onto the row below.
/// assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'a');
///
/// // Which subtree is flattened and which frame is selected is the
/// // reducer's job — it owns the `frames` slice and the `selected`
/// // index; the widget only projects them.
/// ```
#[derive(Debug, Clone)]
pub struct FlameGraph<'a> {
    frames: &'a [FlameFrame<'a>],
    total: Option<u64>,
    row_height: u16,
    inverted: bool,
    selected: Option<usize>,
    block: Option<Block<'a>>,
    style: Style,
    selected_style: Style,
}

impl<'a> FlameGraph<'a> {
    /// A flame graph projecting `frames`, auto-scaled so the widest
    /// `start + width` fills the area, root row at the bottom, nothing
    /// selected, no frame.
    #[must_use]
    pub fn new(frames: &'a [FlameFrame<'a>]) -> Self {
        Self {
            frames,
            total: None,
            // One cell per stack level: the sensible default that fits the
            // most levels into a pane (BarChart's reasoning).
            row_height: 1,
            inverted: false,
            selected: None,
            block: None,
            style: Style::default(),
            selected_style: Style::default(),
        }
    }

    /// Sets the full-width sample count (the axis denominator), or `None` to
    /// auto-scale to the widest `start + width`.
    ///
    /// A `start`/`width` past the axis simply clips at the right edge (never a
    /// panic — the [`BarChart`](crate::BarChart) totality rule); `Some(0)` (or
    /// an empty slice when unset) means there is nothing to plot.
    #[must_use]
    pub fn total(mut self, total: Option<u64>) -> Self {
        self.total = total;
        self
    }

    /// Sets the number of cells each stack level occupies (default `1`).
    /// Clamped to at least `1` at render time.
    #[must_use]
    pub fn row_height(mut self, row_height: u16) -> Self {
        self.row_height = row_height;
        self
    }

    /// Sets the orientation: `false` (default) is a *flame* graph (root row at
    /// the bottom, deeper frames stacking upward); `true` is an *icicle* graph
    /// (root at the top, deeper frames growing downward).
    #[must_use]
    pub fn inverted(mut self, inverted: bool) -> Self {
        self.inverted = inverted;
        self
    }

    /// Sets which frame index is drawn highlighted (the zoomed/selected
    /// frame), or `None` for none.
    ///
    /// An index outside the slice simply highlights nothing — the caller owns
    /// selection (see the [module docs](self)).
    #[must_use]
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the [`Style`] the [`selected`](Self::selected) frame's box is
    /// filled with instead of its own [`FlameFrame::style`].
    #[must_use]
    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a background
    /// covers the whole pane (including the gaps between boxes).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Frames the graph in `block`; boxes render into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }
}

/// Rounds `value / total * span` to the nearest whole cell in `u128` so a wide
/// axis or area never overflows (`total` is already `>= 1`).
fn scale(value: u64, total: u64, span: u16) -> u16 {
    let v = u128::from(value);
    let s = u128::from(span);
    let t = u128::from(total);
    ((v * s + t / 2) / t).min(u128::from(u16::MAX)) as u16
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

impl Widget for FlameGraph<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let FlameGraph {
            frames,
            total,
            row_height,
            inverted,
            selected,
            block,
            style,
            selected_style,
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

        // Base fills the content area so a background covers the whole pane;
        // boxes layer the frame → line → span cascade on top.
        buf.set_style(inner, style);
        if frames.is_empty() {
            return;
        }

        // The axis denominator: the caller's, or the widest `start + width`.
        // A zero total means there is nothing to plot (a no-op, never a
        // div-by-zero).
        let total = total.unwrap_or_else(|| {
            frames
                .iter()
                .map(|f| f.start.saturating_add(f.width))
                .max()
                .unwrap_or(0)
        });
        if total == 0 {
            return;
        }

        let row_h = row_height.max(1);
        let left = inner.left();
        let right = inner.right();
        let top = inner.top();
        let bottom = inner.bottom();
        let inner_w = inner.width;
        // How many whole rows of stack levels fit in the inner area.
        let rows = inner.height / row_h;

        for (idx, frame) in frames.iter().enumerate() {
            // A zero-width frame is nothing to draw.
            if frame.width == 0 {
                continue;
            }
            // A frame whose depth row falls outside the area is skipped (the
            // caller owns zoom/scroll — the widget never wraps a deep frame).
            if u32::from(frame.depth) >= u32::from(rows) {
                continue;
            }

            // Map the depth to a row band, flipping the axis for a flame
            // graph so the root sits at the bottom.
            let band = if inverted {
                frame.depth
            } else {
                rows - 1 - frame.depth
            };
            let y0 = top.saturating_add(band.saturating_mul(row_h));
            let y1 = y0.saturating_add(row_h).min(bottom);
            if y0 >= bottom {
                continue;
            }

            // Map start/width to an x-span, clipped to the inner right edge.
            // A non-zero width never rounds away to nothing — it shows at
            // least one column so a thin frame is still visible.
            let x0 = left
                .saturating_add(scale(frame.start, total, inner_w))
                .min(right);
            let w = scale(frame.width, total, inner_w).max(1);
            let x1 = x0.saturating_add(w).min(right);
            if x0 >= x1 {
                continue;
            }

            // The selected frame uses the highlight fill instead of its own;
            // every other frame uses its own over the base.
            let fill = if selected == Some(idx) {
                style.patch(selected_style)
            } else {
                style.patch(frame.style)
            };

            // Fill the box, then stamp the label clipped inside it: a one-cell
            // left pad when it fits, else flush so a truncated label still
            // reads, else (a one-cell box) just the fill.
            for y in y0..y1 {
                for x in x0..x1 {
                    buf.set_cell(Position::new(x, y), ' ', fill);
                }
            }
            let label_x = if x1 - x0 > 1 {
                x0.saturating_add(1)
            } else {
                x0
            };
            stamp_line(buf, &frame.label, fill, label_x, y0, x1);
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
    fn a_flame_graph_puts_the_root_on_the_bottom_row() {
        let frames = [
            FlameFrame::new(0, 0, 8, "root"),
            FlameFrame::new(1, 0, 4, "ab"),
        ];
        // Flame: depth 0 → bottom row (full width), depth 1 → row above it
        // over the left half; a 1-cell left pad fits "root" but not "ab".
        assert_eq!(
            lines(FlameGraph::new(&frames).total(Some(8)), 8, 2),
            " ab     \n root   \n"
        );
    }

    #[test]
    fn an_inverted_graph_is_an_icicle_with_the_root_on_top() {
        let frames = [
            FlameFrame::new(0, 0, 8, "root"),
            FlameFrame::new(1, 0, 4, "ab"),
        ];
        // Icicle: depth 0 → top row, depth 1 → row below it.
        assert_eq!(
            lines(FlameGraph::new(&frames).total(Some(8)).inverted(true), 8, 2),
            " root   \n ab     \n"
        );
    }

    #[test]
    fn start_and_width_map_to_a_scaled_x_span() {
        let frames = [FlameFrame::new(0, 5, 5, "X")];
        // total 10, width 10 cells → start 5 → column 5, width 5 → 5 cells;
        // the box is the right half, the label one cell into it.
        assert_eq!(
            lines(FlameGraph::new(&frames).total(Some(10)), 10, 1),
            "      X   \n"
        );
    }

    #[test]
    fn auto_total_uses_the_widest_start_plus_width() {
        let frames = [
            FlameFrame::new(0, 0, 4, "aa"),
            FlameFrame::new(1, 2, 2, "b"),
        ];
        // No total → max(0+4, 2+2) = 4; width-4 area, so 1 unit = 1 cell.
        // `b` starts at unit 2 (column 2), a 2-wide box → its label sits one
        // cell into it at column 3.
        assert_eq!(lines(FlameGraph::new(&frames), 4, 2), "   b\n aa \n");
    }

    #[test]
    fn the_selected_frame_uses_the_highlight_fill() {
        let frames = [
            FlameFrame::new(0, 0, 2, "r").style(Style::new().bg(Color::Green)),
            FlameFrame::new(1, 0, 2, "c").style(Style::new().bg(Color::Green)),
        ];
        let fg = FlameGraph::new(&frames)
            .total(Some(2))
            .selected(Some(1))
            .selected_style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        fg.render(buf.area(), &mut buf);
        // The selected child (row 0) is red; the unselected root keeps green.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Red);
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().bg, Color::Green);
    }

    #[test]
    fn a_selection_index_outside_the_slice_highlights_nothing() {
        let frames = [FlameFrame::new(0, 0, 2, "r").style(Style::new().bg(Color::Green))];
        let fg = FlameGraph::new(&frames)
            .total(Some(2))
            .selected(Some(9))
            .selected_style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        fg.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Green);
    }

    #[test]
    fn a_zero_width_frame_is_skipped() {
        let frames = [
            FlameFrame::new(0, 0, 4, "rr"),
            FlameFrame::new(1, 0, 0, "gone"),
        ];
        // The width-0 frame draws nothing; only the root box appears.
        assert_eq!(
            lines(FlameGraph::new(&frames).total(Some(4)), 4, 2),
            "    \n rr \n"
        );
    }

    #[test]
    fn a_zero_total_renders_nothing() {
        let frames = [FlameFrame::new(0, 0, 4, "x")];
        assert_eq!(
            lines(FlameGraph::new(&frames).total(Some(0)), 4, 1),
            "    \n"
        );
    }

    #[test]
    fn a_frame_deeper_than_the_rows_is_skipped() {
        let frames = [
            FlameFrame::new(0, 0, 4, "rr"),
            FlameFrame::new(5, 0, 4, "deep"),
        ];
        // A 2-row area holds depths 0 and 1; depth 5 falls outside and is
        // skipped without a panic.
        assert_eq!(
            lines(FlameGraph::new(&frames).total(Some(4)), 4, 2),
            "    \n rr \n"
        );
    }

    #[test]
    fn a_thin_frame_still_shows_at_least_one_column() {
        let frames = [FlameFrame::new(0, 0, 1, "x")];
        // 1 unit of a 1000 axis over 4 cells rounds to 0 cells, but a
        // non-zero width never vanishes — it floors at one column.
        assert_eq!(
            lines(FlameGraph::new(&frames).total(Some(1000)), 4, 1),
            "x   \n"
        );
    }

    #[test]
    fn row_height_thickens_each_stack_level() {
        let frames = [FlameFrame::new(0, 0, 2, "r"), FlameFrame::new(1, 0, 2, "c")];
        // row_height 2 → each level is 2 rows tall; 4-row area holds both.
        // The label sits one cell into each 2-wide box, on the box's first
        // row; the rest of the box is its (here blank) fill.
        assert_eq!(
            lines(FlameGraph::new(&frames).total(Some(2)).row_height(2), 2, 4),
            " c\n  \n r\n  \n"
        );
    }

    #[test]
    fn a_box_clips_at_the_inner_right_edge() {
        let frames = [FlameFrame::new(0, 0, 10, "abcdefgh")];
        // total 10 over a 3-wide area: the box is 3 cells, the label is
        // padded one cell in then truncated to fit.
        assert_eq!(
            lines(FlameGraph::new(&frames).total(Some(10)), 3, 1),
            " ab\n"
        );
    }

    #[test]
    fn a_block_frames_the_graph_in_the_inner_area() {
        let frames = [FlameFrame::new(0, 0, 1, "x")];
        // inner 1×1 → one full-width box with no room for a label pad.
        assert_eq!(
            lines(
                FlameGraph::new(&frames)
                    .total(Some(1))
                    .block(Block::bordered()),
                3,
                3
            ),
            "┌─┐\n│x│\n└─┘\n"
        );
    }

    #[test]
    fn no_frames_with_a_block_still_renders_the_block() {
        let frames: [FlameFrame; 0] = [];
        assert_eq!(
            lines(FlameGraph::new(&frames).block(Block::bordered()), 3, 3),
            "┌─┐\n│ │\n└─┘\n"
        );
    }

    #[test]
    fn style_cascades_base_then_frame_then_label_span() {
        let frame = FlameFrame::new(
            0,
            0,
            2,
            Line::from(Span::styled("L", Style::new().fg(Color::Red))),
        )
        .style(Style::new().add_modifier(Modifier::BOLD));
        let fg = FlameGraph::new(std::slice::from_ref(&frame))
            .total(Some(2))
            .style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        fg.render(buf.area(), &mut buf);

        // The box fill: base bg cascades, frame modifier patched over it.
        let pad = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(pad.symbol, ' ');
        assert_eq!(pad.bg, Color::Blue);
        assert!(pad.modifier.contains(Modifier::BOLD));

        // The label glyph: span fg wins, base bg and frame modifier cascade.
        let l = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(l.symbol, 'L');
        assert_eq!(l.fg, Color::Red);
        assert_eq!(l.bg, Color::Blue);
        assert!(l.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let frames = [FlameFrame::new(0, 0, 2, "ab")];
        let fg = FlameGraph::new(&frames).total(Some(2));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        fg.render(Rect::new(2, 3, 2, 1), &mut buf);
        // The box sits at the area origin, not (0, 0); a 2-wide box pads the
        // label one cell in.
        assert_eq!(buf.get(Position::new(3, 3)).unwrap().symbol, 'a');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let frames = [FlameFrame::new(0, 0, 4, "x")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        FlameGraph::new(&frames).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
