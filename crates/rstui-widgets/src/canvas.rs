//! [`Canvas`] — a free-form Cartesian plotting surface, the keystone every
//! "draw arbitrary points/lines in data space" chart is built on (the line
//! chart's series, the scatter plot's cloud, a sparkline's bigger sibling).
//!
//! # The sub-cell idea, taken all the way
//!
//! [`Gauge`](crate::Gauge) and [`Sparkline`](crate::Sparkline) buy *one* axis
//! of sub-cell precision from the eighth-block ramp. A plot needs *both* axes,
//! so `Canvas` resolves a [`Marker`] sub-grid inside every cell and collapses
//! it to a single Unicode scalar at stamp time:
//!
//! - [`Marker::Braille`] — `2×4` dots per cell via the Unicode Braille block
//!   (`U+2800`..=`U+28FF`); the finest, the default.
//! - [`Marker::HalfBlock`] — `1×2` via `▀`/`▄`/`█`, and the only marker that
//!   can show *two* colours in one cell (`▀` with a distinct `fg`/`bg`).
//! - [`Marker::Dot`] / [`Marker::Block`] — `1×1`, a `•` or a `█` per set cell.
//!
//! Each glyph is a single scalar, so it maps 1:1 onto a
//! [`Cell`](rstui_core::Buffer) with no grapheme machinery — the same reasoning
//! [`Block`] borders and the gauge ramp use.
//!
//! # A pure projection, like every other widget
//!
//! `Canvas` owns no state. The [`paint`](Canvas::paint) closure is handed a
//! [`Context`] and draws caller-owned data through it (`ctx.draw(&Points {
//! .. })`); the reducer owns *what* the data is and the closure only reads it,
//! exactly the [`Sparkline`](crate::Sparkline) `&[u64]` discipline one
//! dimension up. It is **not** a retained scene graph — the closure runs start
//! to finish on every `render`, draws into a pixel grid, and the grid is
//! stamped and dropped (immediate mode, ADR 0002/0004).
//!
//! # Layers and labels
//!
//! [`Context::layer`] flushes the painted pixels and starts a fresh grid so a
//! later shape's colour cannot blend into an earlier one;
//! [`Context::print`] places a text [`Line`] at data coordinates, stamped on
//! top of every layer (axis ticks, a point's value).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, a degenerate (zero-span) bound, a non-finite or out-of-bounds
//! coordinate, and a print past the edge are all safe clips/no-ops — never a
//! panic. An optional framing [`Block`] follows the container-widget
//! convention.

use rstui_core::{Buffer, Color, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The Braille dot bit for sub-cell column `0..2` × row `0..4`, added to
/// [`BRAILLE_BASE`] to form the glyph (the standard Unicode Braille layout).
const BRAILLE: [[u16; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

/// `U+2800 BRAILLE PATTERN BLANK` — the base every dot pattern offsets from.
const BRAILLE_BASE: u32 = 0x2800;

/// The sub-cell grid a [`Canvas`] resolves inside every terminal cell before
/// collapsing it to one Unicode scalar.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Marker {
    /// `2×4` dots per cell via the Unicode Braille block — the finest
    /// resolution and the default.
    #[default]
    Braille,
    /// `1×2` per cell via `▀`/`▄`/`█`; the only marker that can carry two
    /// colours in one cell (a `▀` whose `fg` and `bg` differ).
    HalfBlock,
    /// `1×1` — a `•` in any cell touched by a shape.
    Dot,
    /// `1×1` — a solid `█` in any cell touched by a shape.
    Block,
}

impl Marker {
    /// The `(per_cell_x, per_cell_y)` sub-cell resolution of this marker.
    const fn density(self) -> (u16, u16) {
        match self {
            Marker::Braille => (2, 4),
            Marker::HalfBlock => (1, 2),
            Marker::Dot | Marker::Block => (1, 1),
        }
    }
}

/// A shape a [`Context`] can [`draw`](Context::draw) — the extension point for
/// caller-defined geometry, with [`Points`], [`CanvasLine`] and
/// [`Rectangle`] provided.
pub trait Shape {
    /// Plot this shape by calling [`Painter::paint`] for each data-space point
    /// it covers (out-of-bounds points are dropped by the painter).
    fn draw(&self, painter: &mut Painter);
}

/// A scatter of data-space `(x, y)` coordinates in one [`Color`].
#[derive(Debug, Clone)]
pub struct Points<'a> {
    /// The coordinates to plot, in data space (the caller owns the slice).
    pub coords: &'a [(f64, f64)],
    /// The colour every point is painted with.
    pub color: Color,
}

impl Shape for Points<'_> {
    fn draw(&self, painter: &mut Painter) {
        for &(x, y) in self.coords {
            painter.paint(x, y, self.color);
        }
    }
}

/// A straight segment between two data-space points (Bresenham-rasterised in
/// sub-cell space). Named `CanvasLine` so it never collides with the text
/// [`Line`].
#[derive(Debug, Clone, Copy)]
pub struct CanvasLine {
    /// First endpoint `x`, in data space.
    pub x1: f64,
    /// First endpoint `y`, in data space.
    pub y1: f64,
    /// Second endpoint `x`, in data space.
    pub x2: f64,
    /// Second endpoint `y`, in data space.
    pub y2: f64,
    /// The colour the segment is painted with.
    pub color: Color,
}

impl Shape for CanvasLine {
    fn draw(&self, painter: &mut Painter) {
        // Rasterise in sub-cell pixel space so the line is gap-free at any
        // slope; out-of-bounds endpoints still anchor the visible part.
        let Some((x1, y1)) = painter.to_pixel(self.x1, self.y1) else {
            return;
        };
        let Some((x2, y2)) = painter.to_pixel(self.x2, self.y2) else {
            return;
        };
        let (mut x1, mut y1) = (x1 as i64, y1 as i64);
        let (x2, y2) = (x2 as i64, y2 as i64);
        let dx = (x2 - x1).abs();
        let dy = -(y2 - y1).abs();
        let sx = if x1 < x2 { 1 } else { -1 };
        let sy = if y1 < y2 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            painter.paint_pixel(x1 as usize, y1 as usize, self.color);
            if x1 == x2 && y1 == y2 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x1 += sx;
            }
            if e2 <= dx {
                err += dx;
                y1 += sy;
            }
        }
    }
}

/// An axis-aligned rectangle outline anchored at its lower-left data-space
/// corner.
#[derive(Debug, Clone, Copy)]
pub struct Rectangle {
    /// Lower-left corner `x`, in data space.
    pub x: f64,
    /// Lower-left corner `y`, in data space.
    pub y: f64,
    /// Width in data units.
    pub width: f64,
    /// Height in data units.
    pub height: f64,
    /// The colour the outline is painted with.
    pub color: Color,
}

impl Shape for Rectangle {
    fn draw(&self, painter: &mut Painter) {
        let (x, y, w, h) = (self.x, self.y, self.width, self.height);
        for seg in [
            CanvasLine {
                x1: x,
                y1: y,
                x2: x + w,
                y2: y,
                color: self.color,
            },
            CanvasLine {
                x1: x,
                y1: y + h,
                x2: x + w,
                y2: y + h,
                color: self.color,
            },
            CanvasLine {
                x1: x,
                y1: y,
                x2: x,
                y2: y + h,
                color: self.color,
            },
            CanvasLine {
                x1: x + w,
                y1: y,
                x2: x + w,
                y2: y + h,
                color: self.color,
            },
        ] {
            seg.draw(painter);
        }
    }
}

/// One painted sub-cell pixel: its colour, or `None` for the blank track.
type Pixel = Option<Color>;

/// The sub-cell pixel buffer of one [`Context`] layer. `width`/`height` are in
/// *pixels* (cells × [`Marker::density`]).
#[derive(Debug, Clone)]
struct Grid {
    width: usize,
    height: usize,
    pixels: Vec<Pixel>,
}

impl Grid {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![None; width.saturating_mul(height)],
        }
    }

    fn set(&mut self, px: usize, py: usize, color: Color) {
        if px < self.width && py < self.height {
            self.pixels[py * self.width + px] = Some(color);
        }
    }

    fn any_set(&self) -> bool {
        self.pixels.iter().any(Option::is_some)
    }
}

/// The data-space → sub-cell-pixel transform handed to each [`Shape::draw`].
///
/// A shape calls [`paint`](Self::paint) with data coordinates; the painter maps
/// them through the [`Canvas`] bounds and drops anything non-finite or
/// out-of-bounds (the totality rule), so a shape never has to clip itself.
#[derive(Debug)]
pub struct Painter<'a> {
    grid: &'a mut Grid,
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
}

impl Painter<'_> {
    /// Maps a data-space `(x, y)` to a sub-cell pixel, or `None` if it is
    /// non-finite or outside the [`Canvas`] bounds.
    pub fn to_pixel(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        if !x.is_finite() || !y.is_finite() || self.grid.width == 0 || self.grid.height == 0 {
            return None;
        }
        let [x0, x1] = self.x_bounds;
        let [y0, y1] = self.y_bounds;
        let (lo_x, hi_x) = (x0.min(x1), x0.max(x1));
        let (lo_y, hi_y) = (y0.min(y1), y0.max(y1));
        if x < lo_x || x > hi_x || y < lo_y || y > hi_y {
            return None;
        }
        let span_x = hi_x - lo_x;
        let span_y = hi_y - lo_y;
        let fx = if span_x == 0.0 {
            0.0
        } else {
            (x - lo_x) / span_x
        };
        let fy = if span_y == 0.0 {
            0.0
        } else {
            (y - lo_y) / span_y
        };
        let px = (fx * (self.grid.width as f64 - 1.0)).round();
        // Screen y grows downward; data y grows upward — flip it.
        let py = ((1.0 - fy) * (self.grid.height as f64 - 1.0)).round();
        Some((px as usize, py as usize))
    }

    /// Paints the data-space point `(x, y)` `color`; a no-op if it is
    /// non-finite or out of bounds.
    pub fn paint(&mut self, x: f64, y: f64, color: Color) {
        if let Some((px, py)) = self.to_pixel(x, y) {
            self.grid.set(px, py, color);
        }
    }

    /// Paints sub-cell pixel `(px, py)` directly (used by rasterising shapes
    /// that already work in pixel space); a no-op if out of range.
    pub fn paint_pixel(&mut self, px: usize, py: usize, color: Color) {
        self.grid.set(px, py, color);
    }
}

/// A text [`Line`] anchored at a data-space point, stamped above every layer.
#[derive(Debug, Clone)]
struct Label<'a> {
    x: f64,
    y: f64,
    line: Line<'a>,
}

/// The drawing surface handed to the [`Canvas::paint`] closure.
///
/// [`draw`](Self::draw) plots a [`Shape`]; [`layer`](Self::layer) commits the
/// current pixels and starts fresh so later colours never blend into earlier
/// ones; [`print`](Self::print) anchors a text [`Line`] at data coordinates.
#[derive(Debug)]
pub struct Context<'a> {
    grid: Grid,
    /// Flushed layers (oldest first); later layers overpaint earlier cells.
    layers: Vec<Grid>,
    labels: Vec<Label<'a>>,
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
}

impl<'a> Context<'a> {
    fn new(px_w: usize, px_h: usize, x_bounds: [f64; 2], y_bounds: [f64; 2]) -> Self {
        Self {
            grid: Grid::new(px_w, px_h),
            layers: Vec::new(),
            labels: Vec::new(),
            x_bounds,
            y_bounds,
        }
    }

    /// Plots `shape` into the current layer.
    pub fn draw<S: Shape>(&mut self, shape: &S) {
        let mut painter = Painter {
            grid: &mut self.grid,
            x_bounds: self.x_bounds,
            y_bounds: self.y_bounds,
        };
        shape.draw(&mut painter);
    }

    /// Commits the painted pixels and starts a fresh grid, so a shape drawn
    /// after this can use its own colour in a cell an earlier shape touched
    /// without the two blending.
    pub fn layer(&mut self) {
        let next = Grid::new(self.grid.width, self.grid.height);
        let done = std::mem::replace(&mut self.grid, next);
        self.layers.push(done);
    }

    /// Anchors `line` at data-space `(x, y)`; it is stamped on top of every
    /// layer, clipped at the content edge (out-of-bounds anchors are dropped).
    pub fn print(&mut self, x: f64, y: f64, line: impl Into<Line<'a>>) {
        self.labels.push(Label {
            x,
            y,
            line: line.into(),
        });
    }

    fn finish(&mut self) {
        let next = Grid::new(self.grid.width, self.grid.height);
        let done = std::mem::replace(&mut self.grid, next);
        self.layers.push(done);
    }
}

/// A Cartesian plotting surface: a [`paint`](Self::paint) closure draws
/// caller-owned data through a [`Context`] in data space, resolved at
/// [`marker`](Self::marker) sub-cell precision and stamped into an optional
/// framing [`Block`].
///
/// `Canvas` owns no state — the closure reads caller-owned data, the same pure
/// projection [`Sparkline`](crate::Sparkline) uses one dimension up.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Color, Position, Rect, Widget};
/// use rstui_widgets::canvas::{Canvas, Marker, Points};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 5, 3));
/// Canvas::default()
///     .x_bounds([0.0, 1.0])
///     .y_bounds([0.0, 1.0])
///     .marker(Marker::Block)
///     .paint(|ctx| {
///         ctx.draw(&Points { coords: &[(0.0, 0.0)], color: Color::Red });
///     })
///     .render(buf.area(), &mut buf);
///
/// // (0,0) in data space is the bottom-left cell (screen y is flipped).
/// assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, '█');
/// ```
pub struct Canvas<'a, F> {
    block: Option<Block<'a>>,
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
    marker: Marker,
    background: Style,
    painter: Option<F>,
}

impl<F> std::fmt::Debug for Canvas<'_, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Canvas")
            .field("block", &self.block)
            .field("x_bounds", &self.x_bounds)
            .field("y_bounds", &self.y_bounds)
            .field("marker", &self.marker)
            .field("background", &self.background)
            .field("painter", &self.painter.as_ref().map(|_| "<closure>"))
            .finish()
    }
}

impl<F> Default for Canvas<'_, F> {
    fn default() -> Self {
        Self {
            block: None,
            x_bounds: [0.0, 0.0],
            y_bounds: [0.0, 0.0],
            marker: Marker::Braille,
            background: Style::default(),
            painter: None,
        }
    }
}

impl<'a, F> Canvas<'a, F>
where
    F: FnOnce(&mut Context),
{
    /// Frames the canvas in `block`; the surface fills [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the inclusive `[min, max]` data-space x-range mapped across the
    /// content width (a zero-span range pins everything to the left edge).
    #[must_use]
    pub fn x_bounds(mut self, bounds: [f64; 2]) -> Self {
        self.x_bounds = bounds;
        self
    }

    /// Sets the inclusive `[min, max]` data-space y-range mapped across the
    /// content height (a zero-span range pins everything to the bottom edge).
    #[must_use]
    pub fn y_bounds(mut self, bounds: [f64; 2]) -> Self {
        self.y_bounds = bounds;
        self
    }

    /// Sets the sub-cell [`Marker`] resolution (default [`Marker::Braille`]).
    #[must_use]
    pub fn marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }

    /// Sets the base [`Style`]; it fills the content area so a background
    /// covers the whole pane beneath the plotted glyphs.
    #[must_use]
    pub fn background(mut self, style: Style) -> Self {
        self.background = style;
        self
    }

    /// Sets the closure that draws caller-owned data through the [`Context`].
    /// It runs once per `render` (immediate mode — no retained scene).
    #[must_use]
    pub fn paint(mut self, painter: F) -> Self {
        self.painter = Some(painter);
        self
    }
}

/// Collapses a `dx×dy` sub-cell block of `grid` to its glyph + colours, or
/// `None` if no pixel in the block is set.
fn collapse(grid: &Grid, cx: usize, cy: usize, marker: Marker) -> Option<(char, Color, Color)> {
    let (dx, dy) = marker.density();
    let (dx, dy) = (dx as usize, dy as usize);
    let px0 = cx * dx;
    let py0 = cy * dy;
    let at = |sx: usize, sy: usize| -> Pixel {
        let (x, y) = (px0 + sx, py0 + sy);
        if x < grid.width && y < grid.height {
            grid.pixels[y * grid.width + x]
        } else {
            None
        }
    };
    match marker {
        Marker::Braille => {
            let mut pattern = 0u16;
            let mut color = None;
            for (sy, row) in BRAILLE.iter().enumerate() {
                for (sx, bit) in row.iter().enumerate() {
                    if let Some(c) = at(sx, sy) {
                        pattern |= *bit;
                        color.get_or_insert(c);
                    }
                }
            }
            color.map(|c| {
                let ch = char::from_u32(BRAILLE_BASE + u32::from(pattern)).unwrap_or(' ');
                (ch, c, Color::Reset)
            })
        }
        Marker::HalfBlock => {
            let top = at(0, 0);
            let bottom = at(0, 1);
            match (top, bottom) {
                (None, None) => None,
                (Some(t), None) => Some(('▀', t, Color::Reset)),
                (None, Some(b)) => Some(('▄', b, Color::Reset)),
                (Some(t), Some(b)) if t == b => Some(('█', t, Color::Reset)),
                // Two colours in one cell — the half-block's unique trick.
                (Some(t), Some(b)) => Some(('▀', t, b)),
            }
        }
        Marker::Dot => at(0, 0).map(|c| ('•', c, Color::Reset)),
        Marker::Block => at(0, 0).map(|c| ('█', c, Color::Reset)),
    }
}

impl<F> Widget for Canvas<'_, F>
where
    F: FnOnce(&mut Context),
{
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Canvas {
            block,
            x_bounds,
            y_bounds,
            marker,
            background,
            painter,
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
        buf.set_style(inner, background);

        let Some(painter) = painter else {
            return;
        };
        let (dx, dy) = marker.density();
        let px_w = inner.width as usize * dx as usize;
        let px_h = inner.height as usize * dy as usize;

        let mut ctx = Context::new(px_w, px_h, x_bounds, y_bounds);
        painter(&mut ctx);
        ctx.finish();

        // Stamp layers oldest-first so a later layer overpaints an earlier
        // cell (Context::layer's whole purpose).
        let glyph_base = background;
        for layer in &ctx.layers {
            if !layer.any_set() {
                continue;
            }
            for cy in 0..inner.height as usize {
                for cx in 0..inner.width as usize {
                    if let Some((ch, fg, bg)) = collapse(layer, cx, cy, marker) {
                        let mut style = glyph_base.fg(fg);
                        if bg != Color::Reset {
                            style = style.bg(bg);
                        }
                        buf.set_cell(
                            Position::new(inner.left() + cx as u16, inner.top() + cy as u16),
                            ch,
                            style,
                        );
                    }
                }
            }
        }

        // Labels last, on top of every layer, clipped to the content edge.
        for label in &ctx.labels {
            let painter = Painter {
                grid: &mut Grid::new(px_w, px_h),
                x_bounds,
                y_bounds,
            };
            let Some((lpx, lpy)) = painter.to_pixel(label.x, label.y) else {
                continue;
            };
            let cx = inner.left() + (lpx / dx as usize) as u16;
            let cy = inner.top() + (lpy / dy as usize) as u16;
            if cy >= inner.bottom() {
                continue;
            }
            let mut x = cx;
            let base = glyph_base.patch(label.line.style);
            'spans: for span in &label.line.spans {
                let st = base.patch(span.style);
                for ch in span.content.chars() {
                    if x >= inner.right() {
                        break 'spans;
                    }
                    buf.set_cell(Position::new(x, cy), ch, st);
                    x += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines<F: FnOnce(&mut Context)>(c: Canvas<'_, F>, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        c.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn block_marker_plots_a_point_in_flipped_screen_space() {
        // Data (0,0) is bottom-left; (1,1) is top-right.
        let c = Canvas::default()
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .marker(Marker::Block)
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &[(0.0, 0.0), (1.0, 1.0)],
                    color: Color::Red,
                });
            });
        assert_eq!(lines(c, 3, 3), "  █\n   \n█  \n");
    }

    #[test]
    fn a_braille_line_is_gap_free() {
        let c = Canvas::default()
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .marker(Marker::Braille)
            .paint(|ctx| {
                ctx.draw(&CanvasLine {
                    x1: 0.0,
                    y1: 0.0,
                    x2: 1.0,
                    y2: 1.0,
                    color: Color::Green,
                });
            });
        // Every cell on the diagonal carries a Braille glyph (>= U+2800).
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 4));
        c.render(buf.area(), &mut buf);
        let diag = (0..4).all(|i| {
            let g = buf.get(Position::new(i, 3 - i)).unwrap().symbol;
            (g as u32) >= BRAILLE_BASE && (g as u32) <= BRAILLE_BASE + 0xFF
        });
        assert!(diag, "the diagonal should be an unbroken Braille run");
    }

    #[test]
    fn half_block_carries_two_colours_in_one_cell() {
        let c = Canvas::default()
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .marker(Marker::HalfBlock)
            .paint(|ctx| {
                // Top and bottom sub-pixel of the same cell, different colours.
                ctx.draw(&Points {
                    coords: &[(0.0, 1.0)],
                    color: Color::Red,
                });
                ctx.draw(&Points {
                    coords: &[(0.0, 0.0)],
                    color: Color::Blue,
                });
            });
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        c.render(buf.area(), &mut buf);
        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.symbol, '▀');
        assert_eq!(cell.fg, Color::Red); // top
        assert_eq!(cell.bg, Color::Blue); // bottom
    }

    #[test]
    fn a_block_frames_the_surface_in_the_inner_area() {
        let c = Canvas::default()
            .block(Block::bordered())
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .marker(Marker::Block)
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &[(0.0, 0.0)],
                    color: Color::Red,
                });
            });
        assert_eq!(lines(c, 3, 3), "┌─┐\n│█│\n└─┘\n");
    }

    #[test]
    fn a_later_layer_overpaints_an_earlier_cell() {
        let c = Canvas::default()
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .marker(Marker::Block)
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &[(0.0, 0.0)],
                    color: Color::Red,
                });
                ctx.layer();
                ctx.draw(&Points {
                    coords: &[(0.0, 0.0)],
                    color: Color::Blue,
                });
            });
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        c.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Blue);
    }

    #[test]
    fn print_anchors_a_label_at_data_coordinates() {
        let c = Canvas::default()
            .x_bounds([0.0, 10.0])
            .y_bounds([0.0, 10.0])
            .marker(Marker::Block)
            .paint(|ctx| {
                ctx.print(0.0, 10.0, "hi");
            });
        // Top-left in screen space (data y=10 is the top).
        assert_eq!(lines(c, 4, 2), "hi  \n    \n");
    }

    #[test]
    fn out_of_bounds_and_non_finite_points_are_dropped() {
        let c = Canvas::default()
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .marker(Marker::Block)
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &[(2.0, 2.0), (f64::NAN, 0.0), (-1.0, 0.5)],
                    color: Color::Red,
                });
            });
        assert_eq!(lines(c, 2, 2), "  \n  \n");
    }

    #[test]
    fn a_zero_span_bound_pins_without_a_panic() {
        let c = Canvas::default()
            .x_bounds([5.0, 5.0])
            .y_bounds([5.0, 5.0])
            .marker(Marker::Block)
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &[(5.0, 5.0)],
                    color: Color::Red,
                });
            });
        // Pinned to the bottom-left cell, no divide-by-zero panic.
        assert_eq!(lines(c, 2, 2), "  \n█ \n");
    }

    #[test]
    fn no_painter_just_fills_the_background() {
        let c: Canvas<'_, fn(&mut Context)> = Canvas::default()
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .background(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        c.render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Blue);
            }
        }
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Canvas::default()
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &[(0.5, 0.5)],
                    color: Color::Red,
                })
            })
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
