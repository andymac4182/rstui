//! A small deterministic character grid every non-flowchart Mermaid diagram
//! lays itself out on before a single centred, clipped blit to the [`Buffer`].
//!
//! The flowchart renderer carries its own `Canvas`/`CellRole` machinery (a
//! per-node skin cascade it needs and the other diagram types do not). Every
//! other diagram type shares this [`Surface`] instead: a flat `(char, Style)`
//! grid with integer-only box/line/text primitives, so each renderer is plain
//! arithmetic and the result is one snapshot-testable image. Out-of-bounds
//! writes are silently dropped, so a layout that overflows its computed size
//! degrades to a clip rather than a panic — the same leniency the parsers use.
//!
//! [`Buffer`]: rstui_core::Buffer

use rstui_core::{Buffer, Position, Rect, Style};

/// The corner/edge glyph family a [`Surface::rect`] is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoxStyle {
    /// Square corners — the default node/entity box.
    Square,
    /// Rounded corners — a softer container or a "round" node.
    Round,
    /// Doubled lines — emphasis (a start/stop state, a highlighted block).
    Double,
    /// Heavy lines — a strong border (a selected/critical element).
    Heavy,
}

impl BoxStyle {
    /// `(top_left, top_right, bottom_left, bottom_right, horizontal,
    /// vertical)` for this family.
    const fn glyphs(self) -> (char, char, char, char, char, char) {
        match self {
            Self::Square => ('┌', '┐', '└', '┘', '─', '│'),
            Self::Round => ('╭', '╮', '╰', '╯', '─', '│'),
            Self::Double => ('╔', '╗', '╚', '╝', '═', '║'),
            Self::Heavy => ('┏', '┓', '┗', '┛', '━', '┃'),
        }
    }
}

/// A `width`×`height` grid of `(glyph, style)` cells, all blank to start.
///
/// Coordinates are `i32` so a renderer can compute with negatives and rely on
/// the clip; nothing here ever panics on an out-of-range cell.
pub(crate) struct Surface {
    w: i32,
    h: i32,
    cells: Vec<(char, Style)>,
}

impl Surface {
    /// A blank `w`×`h` surface (both clamped to `>= 0`).
    pub(crate) fn new(w: i32, h: i32) -> Self {
        let w = w.max(0);
        let h = h.max(0);
        Self {
            w,
            h,
            cells: vec![(' ', Style::new()); (w * h).max(0) as usize],
        }
    }

    /// The grid width in cells.
    pub(crate) const fn width(&self) -> i32 {
        self.w
    }

    /// The grid height in cells.
    pub(crate) const fn height(&self) -> i32 {
        self.h
    }

    /// Paints `ch` at `(x, y)` with `style`; out-of-bounds is a no-op.
    pub(crate) fn set(&mut self, x: i32, y: i32, ch: char, style: Style) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        self.cells[(y * self.w + x) as usize] = (ch, style);
    }

    /// The glyph already at `(x, y)`, or a space if out of bounds — used by
    /// line joins and by snapshot tests.
    pub(crate) fn glyph(&self, x: i32, y: i32) -> char {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return ' ';
        }
        self.cells[(y * self.w + x) as usize].0
    }

    /// Paints `text` left-to-right from `(x, y)` with `style` (by `char`, so a
    /// multibyte glyph occupies one cell — terminal-cell, not byte, advance).
    pub(crate) fn text(&mut self, x: i32, y: i32, text: &str, style: Style) {
        for (i, ch) in text.chars().enumerate() {
            self.set(x + i as i32, y, ch, style);
        }
    }

    /// Paints at most `max` chars of `text` from `(x, y)`, appending `…` when
    /// it was truncated. `max <= 0` paints nothing.
    pub(crate) fn text_clipped(&mut self, x: i32, y: i32, text: &str, max: i32, style: Style) {
        if max <= 0 {
            return;
        }
        let n = text.chars().count() as i32;
        if n <= max {
            self.text(x, y, text, style);
        } else if max == 1 {
            self.set(x, y, '…', style);
        } else {
            let keep = (max - 1) as usize;
            let s: String = text.chars().take(keep).collect();
            self.text(x, y, &s, style);
            self.set(x + keep as i32, y, '…', style);
        }
    }

    /// Centres `text` within `[x, x + width)` on row `y`, clipping with `…`.
    pub(crate) fn text_centered(&mut self, x: i32, y: i32, width: i32, text: &str, style: Style) {
        let n = text.chars().count() as i32;
        if n >= width {
            self.text_clipped(x, y, text, width, style);
        } else {
            self.text(x + (width - n) / 2, y, text, style);
        }
    }

    /// A horizontal run of `ch`, `len` cells from `(x, y)`.
    pub(crate) fn hline(&mut self, x: i32, y: i32, len: i32, ch: char, style: Style) {
        for i in 0..len {
            self.set(x + i, y, ch, style);
        }
    }

    /// A vertical run of `ch`, `len` cells from `(x, y)`.
    pub(crate) fn vline(&mut self, x: i32, y: i32, len: i32, ch: char, style: Style) {
        for i in 0..len {
            self.set(x, y + i, ch, style);
        }
    }

    /// Fills the `w`×`h` rectangle at `(x, y)` with `ch`.
    pub(crate) fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, ch: char, style: Style) {
        for cy in y..y + h {
            for cx in x..x + w {
                self.set(cx, cy, ch, style);
            }
        }
    }

    /// Draws a `w`×`h` box outline at `(x, y)` in the chosen [`BoxStyle`].
    /// `w < 2 || h < 2` is a no-op (too small for a border).
    pub(crate) fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, kind: BoxStyle, style: Style) {
        if w < 2 || h < 2 {
            return;
        }
        let (tl, tr, bl, br, hz, vt) = kind.glyphs();
        let (x1, y1) = (x + w - 1, y + h - 1);
        for cx in x..=x1 {
            self.set(cx, y, hz, style);
            self.set(cx, y1, hz, style);
        }
        for cy in y..=y1 {
            self.set(x, cy, vt, style);
            self.set(x1, cy, vt, style);
        }
        self.set(x, y, tl, style);
        self.set(x1, y, tr, style);
        self.set(x, y1, bl, style);
        self.set(x1, y1, br, style);
    }

    /// Draws a box and centres a single-line `label` on its middle row,
    /// filling the interior so a `bg` style covers the whole box.
    ///
    /// The arguments are the irreducible geometry of a labelled box (rect,
    /// shape, text, two styles); a parameter struct would only move the same
    /// fields behind a name every one of the ~20 call sites must still
    /// populate, so the lint is allowed here rather than obscuring the
    /// primitive.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn labeled_box(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        kind: BoxStyle,
        label: &str,
        border: Style,
        text: Style,
    ) {
        self.fill(x + 1, y + 1, (w - 2).max(0), (h - 2).max(0), ' ', text);
        self.rect(x, y, w, h, kind, border);
        self.text_centered(x + 1, y + h / 2, (w - 2).max(0), label, text);
    }

    /// Blits the surface into `area`, centred when smaller and clipped when
    /// larger, skipping blank cells so `base` shows through. Each painted
    /// cell's style is layered over `base` exactly like the flowchart blit, so
    /// an unset colour falls through to the surrounding theme.
    pub(crate) fn blit(&self, area: Rect, buf: &mut Buffer, base: Style) {
        if self.w == 0 || self.h == 0 {
            return;
        }
        let off_x = ((area.width as i32 - self.w) / 2).max(0);
        let off_y = ((area.height as i32 - self.h) / 2).max(0);
        for cy in 0..self.h {
            for cx in 0..self.w {
                let (ch, style) = self.cells[(cy * self.w + cx) as usize];
                if ch == ' ' && style == Style::new() {
                    continue;
                }
                let px = area.x as i32 + off_x + cx;
                let py = area.y as i32 + off_y + cy;
                if px < area.x as i32
                    || py < area.y as i32
                    || px >= area.right() as i32
                    || py >= area.bottom() as i32
                {
                    continue;
                }
                buf.set_cell(Position::new(px as u16, py as u16), ch, base.patch(style));
            }
        }
    }
}

/// The surface glyphs as one newline-terminated string per row — the shared
/// snapshot helper this module's tests and every sibling diagram module's
/// tests assert against (reached as `super::draw::dump`). Defined ahead of
/// the test module so it is not an item after `mod tests`.
#[cfg(test)]
pub(crate) fn dump(s: &Surface) -> String {
    let mut out = String::new();
    for y in 0..s.h {
        for x in 0..s.w {
            out.push(s.glyph(x, y));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_draws_corners_and_centered_label() {
        let mut s = Surface::new(7, 3);
        s.labeled_box(
            0,
            0,
            7,
            3,
            BoxStyle::Square,
            "hi",
            Style::new(),
            Style::new(),
        );
        assert_eq!(dump(&s), "┌─────┐\n│ hi  │\n└─────┘\n");
    }

    #[test]
    fn text_clipped_appends_ellipsis() {
        let mut s = Surface::new(5, 1);
        s.text_clipped(0, 0, "abcdef", 5, Style::new());
        assert_eq!(dump(&s), "abcd…\n");
    }

    #[test]
    fn out_of_bounds_writes_are_dropped_not_panics() {
        let mut s = Surface::new(2, 2);
        s.set(-1, 0, 'x', Style::new());
        s.set(99, 99, 'x', Style::new());
        s.text(1, 1, "overflow", Style::new());
        assert_eq!(dump(&s), "  \n o\n");
    }

    #[test]
    fn blit_centers_and_skips_blanks() {
        let mut s = Surface::new(2, 1);
        s.text(0, 0, "AB", Style::new());
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        s.blit(buf.area(), &mut buf, Style::new());
        let row: String = (0..6)
            .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
            .collect();
        assert_eq!(row, "  AB  ");
    }
}
