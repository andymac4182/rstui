//! The cell grid that widgets draw into and renderers diff against.
//!
//! [`Buffer`] is an immediate-mode surface: a flat, row-major grid of
//! [`Cell`]s covering some [`Rect`]. Widgets mutate cells directly; a backend
//! later turns the buffer (or just its [`Buffer::diff`] against the previous
//! frame) into terminal escape sequences.
//!
//! For this first slice a cell holds a single [`char`]. Grapheme clusters and
//! double-width (CJK / emoji) handling are deliberately deferred — they change
//! how *many* cells a symbol occupies, which is a renderer concern layered on
//! top of this grid rather than a property of the grid itself.

use crate::geometry::{Position, Rect};
use crate::style::{Color, Modifier, Style};

/// A single addressable terminal cell: one symbol plus its styling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The glyph drawn in this cell.
    pub symbol: char,
    /// Foreground color.
    pub fg: Color,
    /// Background color.
    pub bg: Color,
    /// Active text attributes.
    pub modifier: Modifier,
}

impl Cell {
    /// A blank cell: a space with reset colors and no attributes.
    pub const EMPTY: Self = Self {
        symbol: ' ',
        fg: Color::Reset,
        bg: Color::Reset,
        modifier: Modifier::EMPTY,
    };

    /// Creates a cell displaying `symbol` with default styling.
    #[must_use]
    pub const fn new(symbol: char) -> Self {
        Self {
            symbol,
            ..Self::EMPTY
        }
    }

    /// Applies a [`Style`] patch to this cell.
    ///
    /// Set colors override; unset colors are left as-is. Modifier add/remove
    /// sets are applied so attributes can be both turned on and cleared.
    pub fn apply_style(&mut self, style: Style) -> &mut Self {
        if let Some(fg) = style.fg {
            self.fg = fg;
        }
        if let Some(bg) = style.bg {
            self.bg = bg;
        }
        self.modifier = self
            .modifier
            .difference(style.sub_modifier)
            .union(style.add_modifier);
        self
    }

    /// Returns this cell's styling as a fully-specified [`Style`].
    #[must_use]
    pub fn style(&self) -> Style {
        Style {
            fg: Some(self.fg),
            bg: Some(self.bg),
            add_modifier: self.modifier,
            sub_modifier: Modifier::EMPTY,
        }
    }

    /// Resets the cell back to [`Cell::EMPTY`].
    pub fn reset(&mut self) {
        *self = Self::EMPTY;
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::EMPTY
    }
}

/// A row-major grid of [`Cell`]s covering [`Buffer::area`].
///
/// Coordinates passed to the accessors are absolute screen coordinates, not
/// relative to the buffer's origin; anything outside the area is treated as
/// out of bounds rather than panicking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Buffer {
    area: Rect,
    cells: Vec<Cell>,
}

impl Buffer {
    /// Creates a buffer covering `area`, every cell blank.
    #[must_use]
    pub fn empty(area: Rect) -> Self {
        Self::filled(area, Cell::EMPTY)
    }

    /// Creates a buffer covering `area`, every cell a clone of `cell`.
    #[must_use]
    pub fn filled(area: Rect, cell: Cell) -> Self {
        let len = area.area() as usize;
        Self {
            area,
            cells: vec![cell; len],
        }
    }

    /// The region this buffer covers.
    #[must_use]
    pub fn area(&self) -> Rect {
        self.area
    }

    /// The backing cells in row-major order.
    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// The flat index of `position`, or `None` if it lies outside the area.
    #[must_use]
    pub fn index_of(&self, position: Position) -> Option<usize> {
        if !self.area.contains(position) {
            return None;
        }
        let col = (position.x - self.area.x) as usize;
        let row = (position.y - self.area.y) as usize;
        Some(row * self.area.width as usize + col)
    }

    /// A shared reference to the cell at `position`, if it is in bounds.
    #[must_use]
    pub fn get(&self, position: Position) -> Option<&Cell> {
        self.index_of(position).map(|i| &self.cells[i])
    }

    /// A mutable reference to the cell at `position`, if it is in bounds.
    #[must_use]
    pub fn get_mut(&mut self, position: Position) -> Option<&mut Cell> {
        self.index_of(position).map(|i| &mut self.cells[i])
    }

    /// A mutable slice over the in-bounds part of row `y` within `x_range`.
    ///
    /// The slice covers the cells whose absolute coordinates are `(x, y)`
    /// for `x` in `x_range`, clipped to this buffer's area; it is empty
    /// (rather than panicking) when the row is off-screen or the requested
    /// span lies wholly outside the area. The cells of one row are
    /// contiguous in the backing store, so this resolves a *single* flat
    /// index for the row and slices it — the bulk-write counterpart to
    /// [`get_mut`](Self::get_mut)'s per-cell `index_of`. [`Rect::rows`]
    /// pairs each row with the span to pass here.
    #[must_use]
    pub fn row_slice_mut(&mut self, y: u16, x_range: core::ops::Range<u16>) -> &mut [Cell] {
        // Clip the requested span to the row's columns, mirroring the
        // half-open bounds `index_of`/`Rect::contains` enforce per cell.
        if y < self.area.top() || y >= self.area.bottom() {
            return &mut [];
        }
        let lo = x_range.start.max(self.area.left());
        let hi = x_range.end.min(self.area.right());
        if lo >= hi {
            return &mut [];
        }
        let row = (y - self.area.y) as usize;
        let base = row * self.area.width as usize;
        let start = base + (lo - self.area.x) as usize;
        let end = base + (hi - self.area.x) as usize;
        &mut self.cells[start..end]
    }

    /// Writes `text` starting at `position`, clipped to the buffer.
    ///
    /// Writing stops at the right edge of the buffer's area (no wrapping in
    /// this slice). Returns the position one cell past the last glyph written,
    /// which is convenient for laying out runs of text.
    pub fn set_str(&mut self, position: Position, text: &str, style: Style) -> Position {
        let right = self.area.right();
        let left = self.area.left();
        // One flat index for the whole row instead of one `index_of` per
        // glyph. The slice is empty when the row is off-screen or the span
        // lies outside the area — in which case nothing is written, exactly
        // as the per-cell `get_mut` path skipped each cell. Its first cell
        // is column `max(position.x, left)`; a glyph whose column is left
        // of the area advances `x` without consuming a cell, so glyph N
        // still lands in column `position.x + N`. The returned `x` is
        // computed by the same saturating advance regardless of whether a
        // cell was written, preserving the out-of-bounds return value.
        let mut cells = self.row_slice_mut(position.y, position.x..right).iter_mut();
        let mut x = position.x;
        for ch in text.chars() {
            if x >= right {
                break;
            }
            if x >= left {
                if let Some(cell) = cells.next() {
                    cell.symbol = ch;
                    cell.apply_style(style);
                }
            }
            x = x.saturating_add(1);
        }
        Position::new(x, position.y)
    }

    /// Writes `symbol` at `position` and patches its [`Style`], skipping an
    /// out-of-bounds `position`.
    ///
    /// The single-cell sibling of [`set_str`](Self::set_str): the one
    /// bounds-safe path a [`Widget`](crate::Widget) stamps an individual glyph
    /// through. Writing past the edge is silently ignored, so a widget clips
    /// simply by drawing out of bounds. This is the public cell-stamping
    /// contract third-party widget crates build on, exactly as the first-party
    /// `rstui-widgets` crate does.
    pub fn set_cell(&mut self, position: Position, symbol: char, style: Style) {
        if let Some(cell) = self.get_mut(position) {
            cell.symbol = symbol;
            cell.apply_style(style);
        }
    }

    /// Applies `style` to every cell in `area` that overlaps this buffer.
    pub fn set_style(&mut self, area: Rect, style: Style) {
        // The overlap is already clipped to the buffer, so every row span is
        // fully in bounds: one flat index per row, then a slice walk —
        // instead of the per-cell `index_of` the `positions()` loop paid.
        for (y, xs) in area.intersection(self.area).rows() {
            for cell in self.row_slice_mut(y, xs) {
                cell.apply_style(style);
            }
        }
    }

    /// Resets every cell overlapping `area` back to [`Cell::EMPTY`].
    ///
    /// The region-scoped sibling of [`reset`](Self::reset) (which clears the
    /// whole buffer). This is the **opaque-overlay** primitive. A
    /// [`Style`] is a *patch*: [`set_style`](Self::set_style)
    /// can set a colour but cannot return one to the terminal default, so a
    /// style alone cannot make a floating region truly opaque over arbitrary
    /// background content. A widget that floats over unrelated content (a
    /// modal, popup, dropdown, autocomplete) calls this to take exclusive
    /// ownership of its area before drawing, so nothing underneath bleeds
    /// through the gaps. Cells outside the buffer are ignored, so it is total
    /// for any `area`.
    pub fn clear_region(&mut self, area: Rect) {
        // Same row-slice rewrite as `set_style`: the clipped overlap means
        // each row span is in bounds, so `fill` lowers to a contiguous
        // store loop rather than N bounds-checked `get_mut`s. A region
        // wholly outside the buffer yields no rows — still a total no-op.
        for (y, xs) in area.intersection(self.area).rows() {
            self.row_slice_mut(y, xs).fill(Cell::EMPTY);
        }
    }

    /// Resets every cell to [`Cell::EMPTY`] without changing the area.
    ///
    /// A single contiguous `slice::fill` rather than a per-cell method call:
    /// the runtime blanks the back buffer once per frame in
    /// `Terminal::swap_buffers`, so this is on the idle render path and the
    /// fill lowers to a tight, vectorizable store loop instead of N method
    /// calls.
    pub fn reset(&mut self) {
        self.cells.fill(Cell::EMPTY);
    }

    /// Resizes the buffer to cover `area`, preserving overlapping cells.
    ///
    /// Cells that fall outside the new area are dropped; newly exposed cells
    /// are blank. This keeps a resize from flashing unrelated content.
    pub fn resize(&mut self, area: Rect) {
        if area == self.area {
            return;
        }
        let mut next = Self::empty(area);
        // Copy the preserved overlap a row at a time: one flat index per row
        // in each buffer + a `clone_from_slice`, instead of the per-cell
        // `index_of`-twice (once per buffer) the `positions()` loop paid.
        let overlap = area.intersection(self.area);
        for (y, xs) in overlap.rows() {
            let src = self.row_slice_mut(y, xs.clone());
            let dst = next.row_slice_mut(y, xs);
            // Both spans are the clipped overlap on row `y`, so they are the
            // same length; `clone_from_slice` keeps `Cell`'s value clone.
            dst.clone_from_slice(src);
        }
        *self = next;
    }

    /// The cells that differ from `previous`, as absolute positions.
    ///
    /// This is the unit of work a backend flushes each frame, and the single
    /// hottest per-frame operation (a full-frame diff scans every cell — see
    /// `docs/benchmarking.md`). Both buffers store their cells in the same
    /// row-major order over [`Buffer::area`], so the two flat `cells` slices
    /// are walked in lockstep and the changed cell's [`Position`] is recovered
    /// from its linear index. This avoids the per-cell `Rect::contains` bounds
    /// re-check + multiply that `Position`-at-a-time iteration would pay twice
    /// per cell (once for each buffer) on a scan whose indices are in bounds by
    /// construction. When the two buffers cover different areas every cell is
    /// reported, since a resize invalidates the whole surface.
    #[must_use]
    pub fn diff<'a>(&'a self, previous: &Buffer) -> Vec<(Position, &'a Cell)> {
        // `width == 0` implies `area() == 0`, so `cells` is empty and the
        // iterators below yield nothing before any `% w` / `/ w` runs.
        let w = self.area.width as usize;
        let (ox, oy) = (self.area.x, self.area.y);
        let pos_of = move |i: usize| Position::new(ox + (i % w) as u16, oy + (i / w) as u16);
        if self.area != previous.area {
            return self
                .cells
                .iter()
                .enumerate()
                .map(|(i, c)| (pos_of(i), c))
                .collect();
        }
        self.cells
            .iter()
            .zip(previous.cells.iter())
            .enumerate()
            .filter(|(_, (current, prev))| current != prev)
            .map(|(i, (current, _))| (pos_of(i), current))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer_has_blank_cells() {
        let buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        assert_eq!(buf.cells().len(), 6);
        assert!(buf.cells().iter().all(|c| *c == Cell::EMPTY));
    }

    #[test]
    fn index_of_respects_buffer_origin_and_bounds() {
        let buf = Buffer::empty(Rect::new(10, 5, 4, 3));
        assert_eq!(buf.index_of(Position::new(10, 5)), Some(0));
        assert_eq!(buf.index_of(Position::new(13, 7)), Some(11));
        assert_eq!(buf.index_of(Position::new(9, 5)), None);
        assert_eq!(buf.index_of(Position::new(14, 5)), None);
    }

    #[test]
    fn set_str_clips_at_right_edge() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        let end = buf.set_str(Position::new(2, 0), "hello", Style::new());
        assert_eq!(end, Position::new(4, 0));
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'h');
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, 'e');
        // 'l', 'l', 'o' fell off the edge.
        assert_eq!(buf.get(Position::new(4, 0)), None);
    }

    #[test]
    fn set_str_applies_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        let style = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
        buf.set_str(Position::new(0, 0), "hi", style);
        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.fg, Color::Red);
        assert!(cell.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn set_cell_writes_one_glyph_and_skips_out_of_bounds() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        let style = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
        buf.set_cell(Position::new(1, 0), 'Z', style);
        let cell = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(cell.symbol, 'Z');
        assert_eq!(cell.fg, Color::Red);
        assert!(cell.modifier.contains(Modifier::BOLD));

        // Out of bounds: silently ignored, the buffer is untouched.
        buf.set_cell(Position::new(9, 9), 'X', Style::new());
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn set_style_only_touches_the_overlap() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 4));
        buf.set_style(Rect::new(1, 1, 100, 100), Style::new().bg(Color::Blue));
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Reset);
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().bg, Color::Blue);
        assert_eq!(buf.get(Position::new(3, 3)).unwrap().bg, Color::Blue);
    }

    #[test]
    fn clear_region_resets_only_the_overlap_back_to_empty() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 4));
        let styled = Style::new().fg(Color::Red).bg(Color::Blue);
        // Paint the whole buffer with content and colour.
        for p in buf.area().positions() {
            buf.set_cell(p, 'x', styled);
        }

        // Clear an inner 2x2 box (also given an out-of-bounds extent to prove
        // it is total — only the overlap is touched).
        buf.clear_region(Rect::new(1, 1, 100, 100));

        // The box is back to EMPTY: blank glyph, reset colours.
        for y in 1..4 {
            for x in 1..4 {
                assert_eq!(*buf.get(Position::new(x, y)).unwrap(), Cell::EMPTY);
            }
        }
        // Cells outside the cleared box are untouched.
        let kept = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(kept.symbol, 'x');
        assert_eq!(kept.fg, Color::Red);
        assert_eq!(kept.bg, Color::Blue);

        // A region entirely outside the buffer is a total no-op.
        buf.clear_region(Rect::new(50, 50, 4, 4));
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'x');
    }

    #[test]
    fn resize_preserves_overlapping_cells() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 3));
        buf.set_str(Position::new(0, 0), "X", Style::new());
        buf.resize(Rect::new(0, 0, 5, 5));
        assert_eq!(buf.area(), Rect::new(0, 0, 5, 5));
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'X');
        assert_eq!(buf.get(Position::new(4, 4)).unwrap().symbol, ' ');
    }

    #[test]
    fn diff_reports_only_changed_cells() {
        let previous = Buffer::empty(Rect::new(0, 0, 3, 1));
        let mut current = previous.clone();
        current.set_str(Position::new(1, 0), "A", Style::new());

        let changes = current.diff(&previous);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, Position::new(1, 0));
        assert_eq!(changes[0].1.symbol, 'A');
    }

    #[test]
    fn diff_redraws_everything_after_a_resize() {
        let previous = Buffer::empty(Rect::new(0, 0, 2, 2));
        let current = Buffer::empty(Rect::new(0, 0, 3, 3));
        assert_eq!(current.diff(&previous).len(), 9);
    }

    // --- Row-slice primitive + the bulk paths it backs (CR-08/03/06/04).
    // These pin the exact totality/clipping semantics the rewrite must keep
    // identical to the old per-cell `index_of` loops.

    #[test]
    fn row_slice_mut_clips_to_the_row_and_is_total_off_screen() {
        let mut buf = Buffer::empty(Rect::new(10, 5, 4, 3)); // cols 10..14
        // A span wider than the row clips to the row's columns.
        assert_eq!(buf.row_slice_mut(5, 0..100).len(), 4);
        // Partial overlap clips on both sides.
        assert_eq!(buf.row_slice_mut(6, 12..50).len(), 2);
        assert_eq!(buf.row_slice_mut(6, 0..12).len(), 2);
        // Off-screen row / disjoint span / empty span: empty, no panic.
        assert!(buf.row_slice_mut(4, 10..14).is_empty());
        assert!(buf.row_slice_mut(8, 10..14).is_empty());
        assert!(buf.row_slice_mut(5, 0..10).is_empty());
        assert!(buf.row_slice_mut(5, 14..20).is_empty());
        assert!(buf.row_slice_mut(5, 11..11).is_empty());
        // The slice aliases exactly the cells `get_mut` would reach.
        buf.row_slice_mut(7, 11..13)
            .iter_mut()
            .for_each(|c| c.symbol = 'q');
        assert_eq!(buf.get(Position::new(10, 7)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(11, 7)).unwrap().symbol, 'q');
        assert_eq!(buf.get(Position::new(12, 7)).unwrap().symbol, 'q');
        assert_eq!(buf.get(Position::new(13, 7)).unwrap().symbol, ' ');
    }

    #[test]
    fn set_str_off_screen_row_writes_nothing_but_still_advances_x() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 2));
        // Row 9 is off-screen: no cell is touched, but the returned x is the
        // same saturating advance (capped at right) as the in-bounds path.
        let end = buf.set_str(Position::new(2, 9), "hello", Style::new());
        assert_eq!(end, Position::new(7, 9));
        assert!(buf.cells().iter().all(|c| *c == Cell::EMPTY));
        // Start column already past the right edge: x unchanged, no writes.
        let end = buf.set_str(Position::new(8, 0), "hi", Style::new());
        assert_eq!(end, Position::new(8, 0));
        assert!(buf.cells().iter().all(|c| *c == Cell::EMPTY));
    }

    #[test]
    fn set_str_starting_left_of_a_nonzero_origin_buffer() {
        // Buffer covers columns 5..10 on row 3. Writing from x=3 must put
        // glyph N at column 3+N: the first two glyphs fall off the left and
        // are dropped, the rest land starting at column 5.
        let mut buf = Buffer::empty(Rect::new(5, 3, 5, 1));
        let end = buf.set_str(Position::new(3, 3), "ABCDEFGHIJ", Style::new());
        // x advances one per glyph until it hits right (10), then stops.
        assert_eq!(end, Position::new(10, 3));
        assert_eq!(buf.get(Position::new(5, 3)).unwrap().symbol, 'C');
        assert_eq!(buf.get(Position::new(6, 3)).unwrap().symbol, 'D');
        assert_eq!(buf.get(Position::new(9, 3)).unwrap().symbol, 'G');
    }

    #[test]
    fn set_str_handles_multibyte_glyphs_per_char_not_per_byte() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        let end = buf.set_str(Position::new(0, 0), "áé—x", Style::new());
        assert_eq!(end, Position::new(4, 0));
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'á');
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'é');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, '—');
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, 'x');
    }

    #[test]
    fn set_style_and_clear_region_respect_a_nonzero_origin_overlap() {
        let mut buf = Buffer::empty(Rect::new(4, 2, 6, 4)); // cols 4..10, rows 2..6
        // A region overhanging every edge only touches the in-bounds part.
        buf.set_style(Rect::new(0, 0, 100, 100), Style::new().bg(Color::Blue));
        assert_eq!(buf.get(Position::new(4, 2)).unwrap().bg, Color::Blue);
        assert_eq!(buf.get(Position::new(9, 5)).unwrap().bg, Color::Blue);

        let styled = Style::new().fg(Color::Red).bg(Color::Green);
        for p in buf.area().positions() {
            buf.set_cell(p, 'x', styled);
        }
        buf.clear_region(Rect::new(6, 3, 100, 100));
        for y in 3..6 {
            for x in 6..10 {
                assert_eq!(*buf.get(Position::new(x, y)).unwrap(), Cell::EMPTY);
            }
        }
        // Outside the cleared box but inside the buffer: untouched.
        let kept = buf.get(Position::new(4, 2)).unwrap();
        assert_eq!(
            (kept.symbol, kept.fg, kept.bg),
            ('x', Color::Red, Color::Green)
        );
        // A region wholly outside the buffer is a total no-op.
        buf.clear_region(Rect::new(50, 50, 4, 4));
        assert_eq!(buf.get(Position::new(4, 2)).unwrap().symbol, 'x');
    }

    #[test]
    fn resize_shrink_keeps_only_the_overlap_at_a_nonzero_origin() {
        let mut buf = Buffer::empty(Rect::new(2, 2, 4, 4));
        buf.set_str(Position::new(2, 2), "TL", Style::new());
        buf.set_str(Position::new(4, 5), "BR", Style::new());
        // Shrink to a sub-rectangle that drops the bottom row entirely.
        buf.resize(Rect::new(2, 2, 3, 3));
        assert_eq!(buf.area(), Rect::new(2, 2, 3, 3));
        assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, 'T');
        assert_eq!(buf.get(Position::new(3, 2)).unwrap().symbol, 'L');
        // Row 5 / column 5 are gone; the surviving newly-uncovered-free
        // cells are blank and the dropped content is unreachable.
        assert_eq!(buf.get(Position::new(4, 4)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(4, 5)), None);
    }
}
