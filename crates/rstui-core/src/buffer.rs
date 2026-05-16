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

    /// Writes `text` starting at `position`, clipped to the buffer.
    ///
    /// Writing stops at the right edge of the buffer's area (no wrapping in
    /// this slice). Returns the position one cell past the last glyph written,
    /// which is convenient for laying out runs of text.
    pub fn set_str(&mut self, position: Position, text: &str, style: Style) -> Position {
        let mut x = position.x;
        let right = self.area.right();
        for ch in text.chars() {
            if x >= right {
                break;
            }
            if let Some(cell) = self.get_mut(Position::new(x, position.y)) {
                cell.symbol = ch;
                cell.apply_style(style);
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
        for position in area.intersection(self.area).positions() {
            if let Some(cell) = self.get_mut(position) {
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
        for position in area.intersection(self.area).positions() {
            if let Some(cell) = self.get_mut(position) {
                cell.reset();
            }
        }
    }

    /// Resets every cell to [`Cell::EMPTY`] without changing the area.
    pub fn reset(&mut self) {
        for cell in &mut self.cells {
            cell.reset();
        }
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
        for position in area.intersection(self.area).positions() {
            if let (Some(old), Some(new)) = (self.get(position), next.get_mut(position)) {
                *new = old.clone();
            }
        }
        *self = next;
    }

    /// The cells that differ from `previous`, as absolute positions.
    ///
    /// This is the unit of work a backend flushes each frame. When the two
    /// buffers cover different areas every cell is reported, since a resize
    /// invalidates the whole surface.
    #[must_use]
    pub fn diff<'a>(&'a self, previous: &Buffer) -> Vec<(Position, &'a Cell)> {
        if self.area != previous.area {
            return self
                .area
                .positions()
                .filter_map(|p| self.get(p).map(|c| (p, c)))
                .collect();
        }
        self.area
            .positions()
            .filter_map(|p| {
                let current = self.get(p)?;
                match previous.get(p) {
                    Some(prev) if prev == current => None,
                    _ => Some((p, current)),
                }
            })
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
}
