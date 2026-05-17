//! [`Sankey`] — a left→right flow diagram: nodes are vertical bars whose height
//! is proportional to their throughput and links are proportional connector
//! bands between them, the dashboard primitive for "where does the volume go"
//! (a request funnel, an energy/spend breakdown, an ETL pipeline, a conversion
//! path).
//!
//! # A pure projection, like every other widget
//!
//! `Sankey` owns no state. It is a slice of caller-built [`SankeyNode`]s (a
//! label [`Line`] and a column) and a slice of [`SankeyLink`]s (`from`/`to`
//! node indices and a `u64` value); the reducer decides *what* the graph is and
//! the widget only projects it. That keeps it deterministically
//! headless-testable and composes with the Elm `view(&self)` model exactly like
//! [`BarChart`](crate::BarChart) and [`List`](crate::List).
//!
//! # The layout, then a reused plotting surface for the links
//!
//! Nodes are grouped into columns; a column's bars are stacked top to bottom
//! with even gaps, each bar's height the node's **throughput** (the larger of
//! its inbound and outbound flow) scaled against the busiest column so the
//! whole diagram fits. Columns are placed at evenly spaced x positions across
//! the content width — pure integer geometry, the [`Grid`](crate::Grid) tiling
//! discipline. The link bands are *not* a second renderer: a band is a fan of
//! straight segments, so the widget **composes [`Canvas`] +
//! [`CanvasLine`]** over the inner area (Bresenham,
//! gap-free at any slope, already total over non-finite/out-of-bounds), exactly
//! the reuse [`DescriptionList`](crate::DescriptionList) makes of
//! [`Paragraph`](crate::Paragraph).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! node/link list, a link whose `from`/`to` is out of range (skipped), a link
//! that does not flow strictly left→right (a same-column or backward link —
//! its band is skipped, the bars stay), a single column, an all-zero series (no
//! divide-by-zero — every bar is the floor height), and an area too small for
//! the columns are all safe clips/no-ops — never a panic. An optional framing
//! [`Block`] follows the container-widget convention; deterministic geometry is
//! chosen over visual perfection (curved ribbons, crossing minimisation are
//! deliberately deferred additives).

use rstui_core::{Buffer, Color, Line, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::canvas::{Canvas, CanvasLine, Context};

/// One node of a [`Sankey`]: a label [`Line`] and the 0-based column it sits
/// in.
///
/// Build the label from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`); style it through the
/// [`Line`] it wraps. The node's *height* is derived from the links, not stored
/// here — the reducer owns the flow, the widget only projects it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SankeyNode<'a> {
    /// The clipped caption stamped beside the node bar.
    label: Line<'a>,
    /// The 0-based column this node is stacked in (left to right).
    column: u16,
}

impl<'a> SankeyNode<'a> {
    /// A node in `column` (0-based, left to right) captioned `label` (anything
    /// convertible to a [`Line`]).
    pub fn new(column: u16, label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            column,
        }
    }
}

/// One directed flow of a [`Sankey`]: a `value` flowing from node
/// [`from`](Self::from) to node [`to`](Self::to) (both indices into the
/// [`Sankey::new`] node slice).
///
/// An index outside the node slice makes the link a safe no-op (the totality
/// rule) — the reducer can rebuild the node list freely without first pruning
/// every link.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SankeyLink {
    /// Source node index (into the [`Sankey::new`] node slice).
    pub from: usize,
    /// Target node index (into the [`Sankey::new`] node slice).
    pub to: usize,
    /// The flow magnitude, scaled into a proportional connector band.
    pub value: u64,
}

impl SankeyLink {
    /// A link carrying `value` from node `from` to node `to`.
    #[must_use]
    pub fn new(from: usize, to: usize, value: u64) -> Self {
        Self { from, to, value }
    }
}

/// A left→right flow diagram of caller-built [`SankeyNode`]s and
/// [`SankeyLink`]s with an optional framing [`Block`].
///
/// Nodes are stacked per column with heights proportional to their throughput;
/// links are drawn as straight proportional connector bands (a reused
/// [`Canvas`] + [`CanvasLine`]). Styling is a
/// base [`Style`] (filling the content) with a [`node_style`](Self::node_style)
/// for the bars, a [`link_style`](Self::link_style) for the connectors, and a
/// [`label_style`](Self::label_style) beneath each label's own
/// [`Line`]/[`Span`](rstui_core::Span) styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_widgets::{Sankey, SankeyLink, SankeyNode};
///
/// let nodes = [SankeyNode::new(0, "in"), SankeyNode::new(1, "out")];
/// let links = [SankeyLink::new(0, 1, 10)];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 4));
/// Sankey::new(&nodes, &links).render(buf.area(), &mut buf);
/// // Two columns, a connector band between them — no panic, deterministic.
/// ```
#[derive(Debug, Clone)]
pub struct Sankey<'a> {
    nodes: &'a [SankeyNode<'a>],
    links: &'a [SankeyLink],
    node_width: u16,
    block: Option<Block<'a>>,
    style: Style,
    node_style: Style,
    link_style: Style,
    label_style: Style,
}

impl<'a> Sankey<'a> {
    /// A diagram of borrowed `nodes` and `links`, with 1-wide node bars and no
    /// frame.
    #[must_use]
    pub fn new(nodes: &'a [SankeyNode<'a>], links: &'a [SankeyLink]) -> Self {
        Self {
            nodes,
            links,
            node_width: 1,
            block: None,
            style: Style::default(),
            node_style: Style::default(),
            link_style: Style::default(),
            label_style: Style::default(),
        }
    }

    /// Sets the width of each node bar in cells (default `1`). Clamped to at
    /// least `1` at render time.
    #[must_use]
    pub fn node_width(mut self, node_width: u16) -> Self {
        self.node_width = node_width;
        self
    }

    /// Frames the diagram in `block`; it renders into [`block.inner`](Block::inner).
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

    /// Sets the [`Style`] the node bars are drawn with, over the base.
    #[must_use]
    pub fn node_style(mut self, style: Style) -> Self {
        self.node_style = style;
        self
    }

    /// Sets the [`Style`] the connector bands are drawn with, over the base.
    #[must_use]
    pub fn link_style(mut self, style: Style) -> Self {
        self.link_style = style;
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

/// A node resolved to a vertical bar: its cell column span and row span.
#[derive(Debug, Clone, Copy)]
struct NodeBox {
    x: u16,
    /// The bar's top row.
    top: u16,
    /// The bar's height in rows (always `>= 1` once placed).
    height: u16,
}

/// Stamps `line` left-to-right from `x0` on row `y`, clipped at `right`, with
/// `base` beneath the line→span cascade (the [`BarChart`](crate::BarChart)
/// clipped pattern).
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

impl Widget for Sankey<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Sankey {
            nodes,
            links,
            node_width,
            block,
            style,
            node_style,
            link_style,
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
        if nodes.is_empty() {
            return;
        }

        // Each node's throughput is max(inbound, outbound); an out-of-range
        // link index is skipped (the totality rule), never indexed.
        let mut in_flow = vec![0u64; nodes.len()];
        let mut out_flow = vec![0u64; nodes.len()];
        for link in links {
            if link.from < nodes.len() {
                out_flow[link.from] = out_flow[link.from].saturating_add(link.value);
            }
            if link.to < nodes.len() {
                in_flow[link.to] = in_flow[link.to].saturating_add(link.value);
            }
        }
        let throughput: Vec<u64> = (0..nodes.len())
            .map(|i| in_flow[i].max(out_flow[i]))
            .collect();

        // Columns: the index range is [0, max_column]; an empty column simply
        // contributes nothing. Width is divided evenly across the columns.
        let max_col = nodes.iter().map(|n| n.column).max().unwrap_or(0);
        let col_count = u32::from(max_col) + 1;
        let node_w = node_width.max(1).min(inner.width.max(1));

        // The busiest column's total flow scales every bar so the tallest
        // column fills the height (minus inter-node gaps). Never zero.
        let mut col_total = vec![0u64; col_count as usize];
        let mut col_nodes = vec![0u32; col_count as usize];
        for (i, n) in nodes.iter().enumerate() {
            let c = usize::from(n.column);
            col_total[c] = col_total[c].saturating_add(throughput[i]);
            col_nodes[c] += 1;
        }
        let busiest = col_total.iter().copied().max().unwrap_or(0).max(1);
        let max_nodes_in_col = col_nodes.iter().copied().max().unwrap_or(0);

        // One blank row between stacked bars; never let gaps exceed the height.
        let gap: u16 = 1;
        let total_gap = gap.saturating_mul(max_nodes_in_col.saturating_sub(1) as u16);
        let bar_span = inner.height.saturating_sub(total_gap).max(1);

        // Place every node as a vertical bar. X is the column's left edge;
        // columns are spread so the first hugs the left and the last the right.
        let span_x = inner.width.saturating_sub(node_w);
        let mut boxes: Vec<Option<NodeBox>> = vec![None; nodes.len()];
        for col in 0..col_count {
            let cx = if col_count <= 1 {
                inner.left()
            } else {
                let frac = f64::from(col) / f64::from(col_count - 1);
                inner
                    .left()
                    .saturating_add((frac * f64::from(span_x)).round() as u16)
            };

            // Stack this column's nodes (in input order) top to bottom.
            let mut y = inner.top();
            for (i, n) in nodes.iter().enumerate() {
                if u32::from(n.column) != col {
                    continue;
                }
                let h = ((u128::from(throughput[i]) * u128::from(bar_span)) / u128::from(busiest))
                    as u16;
                let h = h.max(1).min(inner.height);
                if y >= inner.bottom() {
                    break;
                }
                let h = h.min(inner.bottom().saturating_sub(y));
                if h == 0 {
                    break;
                }
                boxes[i] = Some(NodeBox {
                    x: cx,
                    top: y,
                    height: h,
                });
                y = y.saturating_add(h).saturating_add(gap);
            }
        }

        // The connector bands: a reused Canvas + CanvasLine over the inner
        // area (immediate mode, gap-free Bresenham, already total). Each link
        // fans `value`-many evenly spaced segments from the source's right
        // edge to the target's left edge so a band reads as a proportional
        // ribbon without a second renderer.
        let band_color = link_style.fg.unwrap_or(Color::DarkGray);
        // Track how much of each node's edge is already consumed by earlier
        // links so stacked links do not all overlap on the same row.
        let mut src_used = vec![0u16; nodes.len()];
        let mut dst_used = vec![0u16; nodes.len()];
        let w = inner.width;
        let h = inner.height;
        let boxes_ref = &boxes;
        let canvas = Canvas::default()
            .x_bounds([0.0, f64::from(w.max(1))])
            .y_bounds([0.0, f64::from(h.max(1))])
            .marker(crate::canvas::Marker::HalfBlock)
            .paint(move |ctx: &mut Context| {
                for link in links {
                    if link.from >= boxes_ref.len() || link.to >= boxes_ref.len() {
                        continue;
                    }
                    let (Some(s), Some(d)) = (boxes_ref[link.from], boxes_ref[link.to]) else {
                        continue;
                    };
                    if link.value == 0 {
                        continue;
                    }
                    // A band only reads as a flow when the source sits left of
                    // the target; a same-column or backward link has no
                    // sensible left→right ribbon, so it is skipped (still
                    // total — the bars and labels are unaffected).
                    if s.x.saturating_add(node_w) > d.x {
                        continue;
                    }
                    // The band thickness is the link's share of the source's
                    // throughput, mapped to the source bar height.
                    let src_tp = throughput[link.from].max(1);
                    let band_s = ((u128::from(link.value) * u128::from(s.height))
                        / u128::from(src_tp)) as u16;
                    let band_s = band_s.max(1).min(s.height);
                    let dst_tp = throughput[link.to].max(1);
                    let band_d = ((u128::from(link.value) * u128::from(d.height))
                        / u128::from(dst_tp)) as u16;
                    let band_d = band_d.max(1).min(d.height);

                    let s_off = src_used[link.from].min(s.height.saturating_sub(1));
                    let d_off = dst_used[link.to].min(d.height.saturating_sub(1));
                    src_used[link.from] = src_used[link.from].saturating_add(band_s);
                    dst_used[link.to] = dst_used[link.to].saturating_add(band_d);

                    // Screen rows → canvas y (it grows upward, screen down).
                    let to_cy =
                        |row: u16| f64::from(h.saturating_sub(row.saturating_sub(inner.top())));
                    let sx = f64::from(s.x.saturating_add(node_w).saturating_sub(inner.left()));
                    let dx = f64::from(d.x.saturating_sub(inner.left()));
                    let band = band_s.max(band_d);
                    for k in 0..band {
                        let sy = to_cy(
                            s.top
                                .saturating_add(s_off)
                                .saturating_add(k.min(band_s.saturating_sub(1))),
                        );
                        let dy = to_cy(
                            d.top
                                .saturating_add(d_off)
                                .saturating_add(k.min(band_d.saturating_sub(1))),
                        );
                        ctx.draw(&CanvasLine {
                            x1: sx,
                            y1: sy,
                            x2: dx,
                            y2: dy,
                            color: band_color,
                        });
                    }
                }
            });
        canvas.render(inner, buf);

        // Node bars on top of the links so a node always reads as solid.
        let bar_glyph = style.patch(node_style);
        for b in boxes.iter().flatten() {
            for dx in 0..node_w {
                let x = b.x.saturating_add(dx);
                if x >= inner.right() {
                    break;
                }
                for dy in 0..b.height {
                    let y = b.top.saturating_add(dy);
                    if y >= inner.bottom() {
                        break;
                    }
                    buf.set_cell(Position::new(x, y), '█', bar_glyph);
                }
            }
        }

        // Labels last, beside each bar, clipped at the content edge.
        let label_base = style.patch(label_style);
        for (i, n) in nodes.iter().enumerate() {
            let Some(b) = boxes[i] else {
                continue;
            };
            // Place the caption just right of the bar, or left of it when the
            // bar is hard against the right edge.
            let lx = b.x.saturating_add(node_w);
            if lx < inner.right() {
                stamp_line(buf, &n.label, label_base, lx, b.top, inner.right());
            } else {
                let want = n.label.width() as u16;
                let lx = b.x.saturating_sub(want).max(inner.left());
                stamp_line(buf, &n.label, label_base, lx, b.top, b.x);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Modifier, Span};

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row (the bar_chart helper).
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
    fn two_nodes_one_link_place_two_columns_with_a_connector() {
        let nodes = [SankeyNode::new(0, "a"), SankeyNode::new(1, "b")];
        let links = [SankeyLink::new(0, 1, 10)];
        let out = lines(Sankey::new(&nodes, &links), 12, 5);
        // The left bar is at x=0, the right bar at the right edge; both rise
        // the full height (single node per column), with a connector between.
        assert_eq!(buf_char(&out, 0, 0), '█'); // left bar top
        assert_eq!(buf_char(&out, 0, 4), '█'); // left bar bottom (full height)
        assert_eq!(buf_char(&out, 11, 0), '█'); // right bar at the right edge
        // A label sits beside each bar.
        assert_eq!(buf_char(&out, 1, 0), 'a');
    }

    /// The glyph at `(x, y)` of a `lines()` snapshot.
    fn buf_char(s: &str, x: usize, y: usize) -> char {
        s.lines().nth(y).unwrap().chars().nth(x).unwrap()
    }

    #[test]
    fn a_link_with_an_out_of_range_index_is_skipped_not_a_panic() {
        let nodes = [SankeyNode::new(0, "a")];
        // `to = 9` is past the single node — the link is a no-op.
        let links = [SankeyLink::new(0, 9, 5), SankeyLink::new(7, 0, 3)];
        let out = lines(Sankey::new(&nodes, &links), 8, 3);
        // The one valid node still renders as a full-height bar; no panic.
        assert_eq!(buf_char(&out, 0, 0), '█');
        assert_eq!(buf_char(&out, 0, 2), '█');
    }

    #[test]
    fn a_single_column_stacks_its_nodes_and_skips_the_same_column_band() {
        let nodes = [SankeyNode::new(0, "x"), SankeyNode::new(0, "y")];
        // A same-column link has no left→right ribbon, so its band is skipped
        // (the bars/labels stay) — the gap row reads as blank.
        let links = [SankeyLink::new(0, 1, 4)];
        // 5 tall, 2 nodes, equal throughput (4 each), 1-row gap → 2 rows each.
        let out = lines(Sankey::new(&nodes, &links), 6, 5);
        assert_eq!(buf_char(&out, 0, 0), '█'); // node x row 0
        assert_eq!(buf_char(&out, 0, 1), '█'); // node x row 1
        assert_eq!(buf_char(&out, 0, 2), ' '); // the gap row, no band drawn
        assert_eq!(buf_char(&out, 0, 3), '█'); // node y
        assert_eq!(buf_char(&out, 0, 4), '█'); // node y row 1
    }

    #[test]
    fn an_all_zero_series_floors_every_bar_without_a_divide_by_zero() {
        let nodes = [SankeyNode::new(0, "a"), SankeyNode::new(1, "b")];
        let links = [SankeyLink::new(0, 1, 0)];
        // Zero flow everywhere: busiest floors at 1, every bar is the 1-row
        // floor, the zero-value link is skipped — no panic, no div0.
        let out = lines(Sankey::new(&nodes, &links), 10, 3);
        assert_eq!(buf_char(&out, 0, 0), '█');
        assert_eq!(buf_char(&out, 9, 0), '█');
    }

    #[test]
    fn empty_nodes_just_fill_the_area() {
        let nodes: [SankeyNode; 0] = [];
        let links: [SankeyLink; 0] = [];
        assert_eq!(lines(Sankey::new(&nodes, &links), 4, 2), "    \n    \n");
    }

    #[test]
    fn empty_nodes_with_a_block_still_render_the_block() {
        let nodes: [SankeyNode; 0] = [];
        let links: [SankeyLink; 0] = [];
        let s = Sankey::new(&nodes, &links).block(Block::bordered());
        assert_eq!(lines(s, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn a_block_frames_the_diagram_in_the_inner_area() {
        let nodes = [SankeyNode::new(0, "")];
        let links: [SankeyLink; 0] = [];
        let s = Sankey::new(&nodes, &links).block(Block::bordered());
        // inner is 1x1 → one full-block node bar in the centre.
        assert_eq!(lines(s, 3, 3), "┌─┐\n│█│\n└─┘\n");
    }

    #[test]
    fn node_width_thickens_each_bar() {
        let nodes = [SankeyNode::new(0, "")];
        let links: [SankeyLink; 0] = [];
        let s = Sankey::new(&nodes, &links).node_width(2);
        assert_eq!(lines(s, 2, 1), "██\n");
    }

    #[test]
    fn style_cascades_base_then_node_and_label_styles() {
        let nodes = [SankeyNode::new(
            0,
            Line::from(Span::styled("L", Style::new().fg(Color::Red))),
        )];
        let links: [SankeyLink; 0] = [];
        let s = Sankey::new(&nodes, &links)
            .style(Style::new().bg(Color::Blue))
            .node_style(Style::new().fg(Color::Green))
            .label_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        s.render(buf.area(), &mut buf);

        let bar = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(bar.symbol, '█');
        assert_eq!(bar.fg, Color::Green); // node_style fg
        assert_eq!(bar.bg, Color::Blue); // base fill cascades

        let lab = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(lab.symbol, 'L');
        assert_eq!(lab.fg, Color::Red); // span fg wins
        assert!(lab.modifier.contains(Modifier::BOLD)); // label_style cascades
        assert_eq!(lab.bg, Color::Blue);
    }

    #[test]
    fn a_tiny_area_clips_without_a_panic() {
        let nodes = [SankeyNode::new(0, "a"), SankeyNode::new(1, "b")];
        let links = [SankeyLink::new(0, 1, 7)];
        // 1x1: a single cell, the first column's bar wins it, no panic.
        let out = lines(Sankey::new(&nodes, &links), 1, 1);
        assert_eq!(out, "█\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_nodes() {
        let nodes = [SankeyNode::new(0, "x")];
        let links: [SankeyLink; 0] = [];
        let s = Sankey::new(&nodes, &links).block(Block::bordered());
        assert_eq!(lines(s, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let nodes = [SankeyNode::new(0, "x")];
        let links: [SankeyLink; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Sankey::new(&nodes, &links).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
