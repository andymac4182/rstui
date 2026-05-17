//! [`Treemap`] — area-proportional tiling: every category becomes a coloured
//! rectangle whose **screen area** is proportional to its `u64` weight, the
//! dashboard primitive for "what makes up this whole" (disk by directory,
//! spend by team, errors by service, bundle size by module).
//!
//! # A pure projection, like every other widget
//!
//! `Treemap` owns no state. It is a list of caller-built [`TreemapTile`]s (a
//! label [`Line`], a `u64` weight, a [`Color`]); the reducer decides *what* the
//! tiles are (`du -s` totals it recomputes in `update`) and the widget only
//! projects them. That keeps it deterministically headless-testable and
//! composes with the Elm `view(&self)` model exactly like [`List`](crate::List)
//! and [`BarChart`](crate::BarChart).
//!
//! # The layout: a deterministic squarified tiling on integer cell rects
//!
//! A pie wedge is hard to read and a single bar hides the small slices, so a
//! treemap spends *both* screen dimensions: each tile's [`Rect`] area is its
//! share of the content rect. The split is the classic **squarified** rule —
//! greedily grow a row of tiles along the shorter side while the worst tile
//! aspect ratio keeps improving, fix that row, then recurse into the rectangle
//! that is left — so tiles stay close to square and a glance compares areas,
//! not slivers. It runs entirely on integer cell [`Rect`]s (the
//! [`Grid`](crate::Grid) tiling discipline): the running fractional remainder
//! is carried so cell rounding never loses or double-spends a column, and a
//! sub-cell strip is clamped away rather than recursed into (no zero-size
//! recursion, no divide-by-zero). Tiles are laid largest-first, so the order is
//! a pure function of the weights and the rect — never the input order.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no tiles, an all-zero series (split evenly), a single tile, and far
//! more tiles than cells (the ones that round to nothing are simply dropped)
//! are all safe clips/no-ops — never a panic, never a divide-by-zero. An
//! optional framing [`Block`] follows the container-widget convention, and a
//! per-tile background plus a clipped label are stamped with the
//! [`BarChart`](crate::BarChart) clipped-`Line` pattern.

use rstui_core::{Buffer, Color, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// One category of a [`Treemap`]: a label [`Line`], its `u64` weight, and the
/// background [`Color`] its tile is filled with.
///
/// Build the label from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`); style its text through
/// the [`Line`] it wraps (the tile *background* is `color`, the
/// [`label_style`](Treemap::label_style) and per-span styles layer on top).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TreemapTile<'a> {
    /// The clipped caption stamped inside the tile.
    label: Line<'a>,
    /// The weight whose share of the content rect this tile's area is.
    value: u64,
    /// The tile's background fill colour.
    color: Color,
}

impl<'a> TreemapTile<'a> {
    /// A tile of weight `value`, background `color`, captioned `label`
    /// (anything convertible to a [`Line`]).
    pub fn new(value: u64, color: Color, label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            value,
            color,
        }
    }
}

/// An area-proportional tiling of caller-built [`TreemapTile`]s with an
/// optional framing [`Block`].
///
/// Each tile's [`Rect`] area is its share of the content rect, placed by a
/// deterministic squarified split (worst-aspect-ratio greedy rows, recursing
/// into the leftover rectangle, largest weight first). Every tile is filled
/// with its [`TreemapTile`] colour, an optional [`padding`](Self::padding) gap
/// is left between tiles, and the label is stamped clipped inside the tile with
/// a base [`Style`] (filling the content) cascading into
/// [`label_style`](Self::label_style) then the label's own
/// [`Line`]/[`Span`](rstui_core::Span) styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Color, Position, Rect, Widget};
/// use rstui_widgets::{Treemap, TreemapTile};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
/// Treemap::new([
///     TreemapTile::new(3, Color::Red, "a"),
///     TreemapTile::new(1, Color::Blue, "b"),
/// ])
/// .render(buf.area(), &mut buf);
///
/// // The 3:1 split fills the whole 4x2 rect with the two tile colours.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Red);
/// assert_eq!(buf.get(Position::new(3, 0)).unwrap().bg, Color::Blue);
/// ```
#[derive(Debug, Default, Clone)]
pub struct Treemap<'a> {
    tiles: Vec<TreemapTile<'a>>,
    padding: u16,
    block: Option<Block<'a>>,
    style: Style,
    label_style: Style,
}

impl<'a> Treemap<'a> {
    /// A treemap of `tiles`, gapless, with no frame.
    pub fn new<I>(tiles: I) -> Self
    where
        I: IntoIterator<Item = TreemapTile<'a>>,
    {
        Self {
            tiles: tiles.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets the blank gap left on each inner side between adjacent tiles
    /// (default `0`). A tile shrunk to nothing by the gap is dropped, never a
    /// panic.
    #[must_use]
    pub fn padding(mut self, padding: u16) -> Self {
        self.padding = padding;
        self
    }

    /// Frames the map in `block`; tiles render into [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a background
    /// covers any cell no tile reaches.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the base [`Style`] for labels, beneath each label's own
    /// [`Line`]/[`Span`](rstui_core::Span) styles and over the tile colour.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }
}

/// A tile resolved to an integer cell [`Rect`]; the layout's output unit.
#[derive(Debug, Clone, Copy)]
struct Placed {
    rect: Rect,
    idx: usize,
}

/// The weight share of one input tile, kept paired with its original index so
/// the colour/label survive the largest-first sort.
#[derive(Debug, Clone, Copy)]
struct Weighted {
    value: f64,
    idx: usize,
}

/// The worst (largest) aspect ratio in a row of `areas` laid along a strip of
/// thickness `side`, given the row's running area `sum` (Bruls et al.). Never
/// divides by zero — an empty row or a zero side is "infinitely bad".
fn worst_ratio(areas: &[f64], sum: f64, side: f64) -> f64 {
    if areas.is_empty() || side <= 0.0 || sum <= 0.0 {
        return f64::INFINITY;
    }
    let mut lo = f64::INFINITY;
    let mut hi: f64 = 0.0;
    for &a in areas {
        lo = lo.min(a);
        hi = hi.max(a);
    }
    let s2 = sum * sum;
    let side2 = side * side;
    (side2 * hi / s2).max(s2 / (side2 * lo))
}

/// Squarified layout: places `weights` (already largest-first, all `> 0`) into
/// `area`, appending one [`Placed`] per tile that rounds to a non-empty rect.
///
/// Iterative worklist over (sub-rect, weight-range, remaining-area) so a deep
/// recursion can never blow the stack; integer cell rects with a carried
/// fractional remainder so rounding neither loses nor double-spends a cell.
fn squarify(weights: &[Weighted], area: Rect, out: &mut Vec<Placed>) {
    // (start, end) into `weights`, the rect that range tiles, and the data-area
    // sum still to place inside it.
    let mut stack: Vec<(usize, usize, Rect, f64)> = Vec::new();
    let total: f64 = weights.iter().map(|w| w.value).sum();
    if total <= 0.0 || area.width == 0 || area.height == 0 {
        return;
    }
    stack.push((0, weights.len(), area, total));

    while let Some((start, end, rect, remaining)) = stack.pop() {
        if start >= end || rect.width == 0 || rect.height == 0 || remaining <= 0.0 {
            continue;
        }
        // Lay the next row along the shorter side so tiles trend square.
        let horizontal = rect.width <= rect.height;
        let side = if horizontal {
            f64::from(rect.width)
        } else {
            f64::from(rect.height)
        };
        if side <= 0.0 {
            continue;
        }

        // Grow the row while the worst aspect ratio keeps improving.
        let mut row_end = start + 1;
        let mut row_sum = weights[start].value;
        let mut areas = vec![weights[start].value];
        while row_end < end {
            let next = weights[row_end].value;
            let cur = worst_ratio(&areas, row_sum, side);
            areas.push(next);
            let with = worst_ratio(&areas, row_sum + next, side);
            if with > cur {
                areas.pop();
                break;
            }
            row_sum += next;
            row_end += 1;
        }

        // The fraction of the still-remaining data area this row consumes maps
        // to its thickness across the long axis (integer cells).
        let frac = (row_sum / remaining).clamp(0.0, 1.0);
        let (placed_rect, rest_rect) = if horizontal {
            let band_h = (frac * f64::from(rect.height)).round() as u16;
            let band_h = band_h.min(rect.height).max(1);
            (
                Rect::new(rect.x, rect.y, rect.width, band_h),
                Rect::new(
                    rect.x,
                    rect.y.saturating_add(band_h),
                    rect.width,
                    rect.height.saturating_sub(band_h),
                ),
            )
        } else {
            let band_w = (frac * f64::from(rect.width)).round() as u16;
            let band_w = band_w.min(rect.width).max(1);
            (
                Rect::new(rect.x, rect.y, band_w, rect.height),
                Rect::new(
                    rect.x.saturating_add(band_w),
                    rect.y,
                    rect.width.saturating_sub(band_w),
                    rect.height,
                ),
            )
        };

        // Slice the row band proportionally along its long axis, carrying the
        // fractional remainder so the cells exactly fill the band.
        if horizontal {
            let mut x = placed_rect.x;
            let mut acc = 0.0_f64;
            let mut used = 0u16;
            for (k, w) in weights[start..row_end].iter().enumerate() {
                let last = k + 1 == row_end - start;
                acc += w.value / row_sum * f64::from(placed_rect.width);
                let w_cells = if last {
                    placed_rect.width.saturating_sub(used)
                } else {
                    (acc.round() as u16).saturating_sub(used)
                };
                if w_cells > 0 {
                    out.push(Placed {
                        rect: Rect::new(x, placed_rect.y, w_cells, placed_rect.height),
                        idx: w.idx,
                    });
                    x = x.saturating_add(w_cells);
                    used = used.saturating_add(w_cells);
                }
            }
        } else {
            let mut y = placed_rect.y;
            let mut acc = 0.0_f64;
            let mut used = 0u16;
            for (k, w) in weights[start..row_end].iter().enumerate() {
                let last = k + 1 == row_end - start;
                acc += w.value / row_sum * f64::from(placed_rect.height);
                let h_cells = if last {
                    placed_rect.height.saturating_sub(used)
                } else {
                    (acc.round() as u16).saturating_sub(used)
                };
                if h_cells > 0 {
                    out.push(Placed {
                        rect: Rect::new(placed_rect.x, y, placed_rect.width, h_cells),
                        idx: w.idx,
                    });
                    y = y.saturating_add(h_cells);
                    used = used.saturating_add(h_cells);
                }
            }
        }

        // Recurse into what is left with the rest of the weights.
        if row_end < end {
            stack.push((row_end, end, rest_rect, remaining - row_sum));
        }
    }
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

impl Widget for Treemap<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Treemap {
            tiles,
            padding,
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

        // Base fills the content area so a background covers any gap a tile
        // never reaches (padding gutters, sub-cell rounding loss).
        buf.set_style(inner, style);
        if tiles.is_empty() {
            return;
        }

        // All-zero weights tile evenly (equal share) so the map is never blank
        // for a degenerate series; otherwise the share is the weight. Largest
        // first makes the layout a pure function of the weights, not the order.
        let all_zero = tiles.iter().all(|t| t.value == 0);
        let mut weights: Vec<Weighted> = tiles
            .iter()
            .enumerate()
            .map(|(idx, t)| Weighted {
                value: if all_zero { 1.0 } else { t.value as f64 },
                idx,
            })
            .collect();
        weights.sort_by(|a, b| {
            b.value
                .partial_cmp(&a.value)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.idx.cmp(&b.idx))
        });

        let mut placed = Vec::with_capacity(weights.len());
        squarify(&weights, inner, &mut placed);

        let label_base = style.patch(label_style);
        for p in &placed {
            let tile = &tiles[p.idx];
            // Apply the optional inter-tile gap; a tile the gap shrinks to
            // nothing is simply not drawn (totality, no underflow).
            let pad = padding;
            let r = p.rect;
            if r.width <= pad.saturating_mul(2) || r.height <= pad.saturating_mul(2) {
                // Too small to inset both sides — fill it solid (no room for a
                // gap) so a sliver still reads as its colour.
                buf.set_style(r, style.patch(Style::new().bg(tile.color)));
                continue;
            }
            let body = Rect::new(
                r.x.saturating_add(pad),
                r.y.saturating_add(pad),
                r.width.saturating_sub(pad.saturating_mul(2)),
                r.height.saturating_sub(pad.saturating_mul(2)),
            );
            if body.is_empty() {
                continue;
            }
            // The tile background is the base patched with this tile's colour.
            buf.set_style(body, style.patch(Style::new().bg(tile.color)));

            // The caption, clipped to the tile body, on its first row.
            let tile_base = label_base.patch(Style::new().bg(tile.color));
            stamp_line(
                buf,
                &tile.label,
                tile_base,
                body.left(),
                body.top(),
                body.right(),
            );
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

    /// The background colour at every cell, one newline-terminated row of
    /// single-letter colour codes, for area-proportion snapshots.
    fn bg_grid<W: Widget>(widget: W, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push(match buf.get(Position::new(x, y)).unwrap().bg {
                    Color::Red => 'R',
                    Color::Blue => 'B',
                    Color::Green => 'G',
                    Color::Yellow => 'Y',
                    Color::Reset => '.',
                    _ => '?',
                });
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn two_tiles_split_proportional_to_their_weights() {
        // 3:1 over a 4x2 (8 cells) → Red ~6 cells, Blue ~2.
        let map = Treemap::new([
            TreemapTile::new(3, Color::Red, ""),
            TreemapTile::new(1, Color::Blue, ""),
        ]);
        assert_eq!(bg_grid(map, 4, 2), "RRRB\nRRRB\n");
    }

    #[test]
    fn a_single_tile_fills_the_whole_area() {
        let map = Treemap::new([TreemapTile::new(1, Color::Green, "x")]);
        assert_eq!(bg_grid(map, 3, 2), "GGG\nGGG\n");
        // The label is stamped at the tile's top-left.
        assert_eq!(
            lines(Treemap::new([TreemapTile::new(1, Color::Green, "x")]), 3, 2),
            "x  \n   \n"
        );
    }

    #[test]
    fn the_label_is_clipped_to_its_tile() {
        // "longlabel" cannot fit a 2-wide tile; it is clipped, never overflows.
        let map = Treemap::new([
            TreemapTile::new(1, Color::Red, "longlabel"),
            TreemapTile::new(1, Color::Blue, "y"),
        ]);
        // 1:1 over 4x1 → two 2-wide tiles; "lo" then "y".
        assert_eq!(lines(map, 4, 1), "loy \n");
    }

    #[test]
    fn an_all_zero_series_tiles_evenly_not_blank() {
        // Every weight 0 → equal share, the map is never blank.
        let map = Treemap::new([
            TreemapTile::new(0, Color::Red, ""),
            TreemapTile::new(0, Color::Blue, ""),
        ]);
        assert_eq!(bg_grid(map, 4, 1), "RRBB\n");
    }

    #[test]
    fn far_more_tiles_than_cells_drop_the_ones_that_round_away() {
        // 6 equal tiles into 2 cells: only the cells that exist are filled,
        // every cell is some tile's colour, nothing panics.
        let map = Treemap::new([
            TreemapTile::new(1, Color::Red, ""),
            TreemapTile::new(1, Color::Blue, ""),
            TreemapTile::new(1, Color::Green, ""),
            TreemapTile::new(1, Color::Yellow, ""),
            TreemapTile::new(1, Color::Red, ""),
            TreemapTile::new(1, Color::Blue, ""),
        ]);
        let g = bg_grid(map, 2, 1);
        assert_eq!(g.len(), 3); // "??\n"
        assert!(!g.contains('.'), "every cell is covered by some tile");
    }

    #[test]
    fn padding_insets_each_tile_leaving_the_base_between() {
        // One tile, padding 1, over 4x3 → a 2x1 body inset by one cell.
        let map = Treemap::new([TreemapTile::new(1, Color::Red, "")]).padding(1);
        assert_eq!(bg_grid(map, 4, 3), "....\n.RR.\n....\n");
    }

    #[test]
    fn a_block_frames_the_map_in_the_inner_area() {
        let map = Treemap::new([TreemapTile::new(1, Color::Red, "")]).block(Block::bordered());
        assert_eq!(lines(map, 3, 3), "┌─┐\n│ │\n└─┘\n");
        assert_eq!(
            bg_grid(
                Treemap::new([TreemapTile::new(1, Color::Red, "")]).block(Block::bordered()),
                3,
                3
            ),
            "...\n.R.\n...\n"
        );
    }

    #[test]
    fn no_tiles_with_a_block_still_renders_the_block() {
        let map = Treemap::new(Vec::<TreemapTile>::new()).block(Block::bordered());
        assert_eq!(lines(map, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn style_cascades_base_then_label_style_then_span() {
        let tile = TreemapTile::new(
            1,
            Color::Blue,
            Line::from(Span::styled("L", Style::new().fg(Color::Red))),
        );
        let map = Treemap::new([tile])
            .style(Style::new().bg(Color::Green))
            .label_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        map.render(buf.area(), &mut buf);

        let l = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(l.symbol, 'L');
        assert_eq!(l.fg, Color::Red); // span fg wins
        assert!(l.modifier.contains(Modifier::BOLD)); // label_style cascades
        assert_eq!(l.bg, Color::Blue); // tile colour over the base fill
    }

    #[test]
    fn a_tiny_area_clips_without_a_panic() {
        let map = Treemap::new([
            TreemapTile::new(5, Color::Red, "a"),
            TreemapTile::new(2, Color::Blue, "b"),
        ]);
        // 1x1: a single cell, the largest tile wins it, no divide-by-zero.
        assert_eq!(bg_grid(map, 1, 1), "R\n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Treemap::new([TreemapTile::new(5, Color::Red, "x")])
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(
            buf.cells()
                .iter()
                .all(|c| c.symbol == ' ' && c.bg == Color::Reset)
        );
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_tiles() {
        let map = Treemap::new([TreemapTile::new(1, Color::Red, "x")]).block(Block::bordered());
        assert_eq!(lines(map, 2, 2), "┌┐\n└┘\n");
    }
}
