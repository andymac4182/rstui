//! The boundary between the pure cell grid and a real terminal.
//!
//! [`Backend`] abstracts "make the screen look like these cells". The rest of
//! the framework never talks to a terminal directly: it draws into a
//! [`Buffer`], asks the buffer which cells changed
//! ([`Buffer::diff`](crate::Buffer::diff)), and hands that to a `Backend`.
//! That keeps every higher layer testable without a TTY — [`TestBackend`]
//! implements the same trait against an in-memory grid you can assert on.
//!
//! A real crossterm/termion backend belongs in its own crate (it needs a
//! terminal dependency); the trait and the in-memory test backend stay here so
//! `rstui-core` remains dependency-free and the runtime, components, and
//! plugin host can all be unit tested against [`TestBackend`].
//!
//! Methods that touch terminal state beyond drawing — line insertion, region
//! clears, pixel/window size, scroll regions — are deliberately deferred to a
//! later slice rather than stubbed speculatively.
//!
//! # Example
//!
//! ```
//! use rstui_core::backend::{Backend, TestBackend};
//! use rstui_core::{Buffer, Position, Rect, Style};
//!
//! let previous = Buffer::empty(Rect::new(0, 0, 5, 1));
//! let mut frame = previous.clone();
//! frame.set_str(Position::ORIGIN, "hi", Style::new());
//!
//! // Flush exactly the cells that changed, just like the runtime will.
//! let mut backend = TestBackend::new(5, 1);
//! backend.draw(frame.diff(&previous)).unwrap();
//!
//! assert_eq!(format!("{backend}"), "hi   \n");
//! ```

use std::convert::Infallible;
use std::fmt::{self, Write as _};

use crate::buffer::{Buffer, Cell};
use crate::geometry::{Position, Rect, Size};

/// An abstraction over a drawable terminal surface.
///
/// Implementors translate buffer cells into whatever the target understands:
/// ANSI escapes for a real terminal, an in-memory grid for tests. The unit of
/// work is the cell diff produced by [`Buffer::diff`](crate::Buffer::diff), so
/// [`draw`](Backend::draw) takes an iterator of `(Position, &Cell)` and plugs
/// straight into it.
///
/// The trait is intentionally **not** object-safe: [`draw`](Backend::draw) is
/// generic so backends accept any cell iterator without allocating. The
/// runtime is monomorphized over a concrete backend rather than boxing one.
pub trait Backend {
    /// How this backend reports failure.
    ///
    /// In-memory backends use [`Infallible`]; a real terminal backend would
    /// use [`std::io::Error`].
    type Error: std::error::Error;

    /// Writes `cells` to the surface at their absolute screen positions.
    ///
    /// Positions are absolute (origin top-left), matching
    /// [`Buffer::diff`](crate::Buffer::diff). Cells outside the surface are
    /// ignored rather than erroring, mirroring the bounds-safe buffer
    /// accessors.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if the surface could not be written.
    fn draw<'a, I>(&mut self, cells: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = (Position, &'a Cell)>;

    /// Hides the cursor.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if the request could not be issued.
    fn hide_cursor(&mut self) -> Result<(), Self::Error>;

    /// Shows the cursor.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if the request could not be issued.
    fn show_cursor(&mut self) -> Result<(), Self::Error>;

    /// Returns the current cursor position (origin top-left).
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if the position could not be read.
    fn cursor_position(&mut self) -> Result<Position, Self::Error>;

    /// Moves the cursor to `position` (origin top-left).
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if the cursor could not be moved.
    fn set_cursor_position(&mut self, position: Position) -> Result<(), Self::Error>;

    /// Clears the entire surface back to blank cells.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if the surface could not be cleared.
    fn clear(&mut self) -> Result<(), Self::Error>;

    /// Returns the surface size in cells.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if the size could not be queried.
    fn size(&self) -> Result<Size, Self::Error>;

    /// Flushes any buffered output so prior draws become visible.
    ///
    /// This is distinct from [`Buffer::diff`](crate::Buffer::diff): the diff
    /// decides *what* to send, `flush` makes sure it has actually been sent.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if buffered output could not be flushed.
    fn flush(&mut self) -> Result<(), Self::Error>;
}

/// An in-memory [`Backend`] for testing UIs without a terminal.
///
/// It keeps a [`Buffer`] anchored at the origin plus the cursor state a real
/// backend owns. Drive it exactly like a terminal backend, then assert on
/// [`buffer`](TestBackend::buffer) or the [`Display`](fmt::Display) snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestBackend {
    buffer: Buffer,
    cursor_visible: bool,
    cursor: Position,
}

impl TestBackend {
    /// Creates a `width` × `height` surface of blank cells, cursor hidden at
    /// the origin.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            buffer: Buffer::empty(Rect::new(0, 0, width, height)),
            cursor_visible: false,
            cursor: Position::ORIGIN,
        }
    }

    /// The current screen contents.
    #[must_use]
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Whether the cursor is currently visible.
    #[must_use]
    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    /// Resizes the surface, preserving cells that overlap the new area.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.buffer.resize(Rect::new(0, 0, width, height));
    }
}

impl fmt::Display for TestBackend {
    /// Renders the surface as one line of symbols per row, each newline
    /// terminated. Deterministic, so it doubles as a snapshot format.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let area = self.buffer.area();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let symbol = self
                    .buffer
                    .get(Position::new(x, y))
                    .map_or(' ', |c| c.symbol);
                f.write_char(symbol)?;
            }
            f.write_char('\n')?;
        }
        Ok(())
    }
}

impl Backend for TestBackend {
    type Error = Infallible;

    fn draw<'a, I>(&mut self, cells: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = (Position, &'a Cell)>,
    {
        for (position, cell) in cells {
            if let Some(slot) = self.buffer.get_mut(position) {
                *slot = cell.clone();
            }
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.cursor_visible = true;
        Ok(())
    }

    fn cursor_position(&mut self) -> Result<Position, Self::Error> {
        Ok(self.cursor)
    }

    fn set_cursor_position(&mut self, position: Position) -> Result<(), Self::Error> {
        self.cursor = position;
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.buffer.reset();
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(self.buffer.area().size())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Style;

    #[test]
    fn new_starts_blank_with_hidden_cursor() {
        let backend = TestBackend::new(4, 2);
        assert_eq!(backend.size().unwrap(), Size::new(4, 2));
        assert!(!backend.cursor_visible());
        assert!(backend.buffer().cells().iter().all(|c| *c == Cell::EMPTY));
    }

    #[test]
    fn draw_consumes_a_buffer_diff() {
        let previous = Buffer::empty(Rect::new(0, 0, 6, 1));
        let mut frame = previous.clone();
        frame.set_str(Position::new(1, 0), "hey", Style::new());

        let mut backend = TestBackend::new(6, 1);
        backend.draw(frame.diff(&previous)).unwrap();

        let buf = backend.buffer();
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'h');
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, 'y');
        // A cell the diff didn't touch stays blank.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn draw_ignores_out_of_bounds_cells() {
        let mut backend = TestBackend::new(2, 1);
        let cell = Cell::new('Z');
        backend
            .draw([(Position::new(9, 9), &cell), (Position::new(0, 0), &cell)])
            .unwrap();
        assert_eq!(
            backend.buffer().get(Position::new(0, 0)).unwrap().symbol,
            'Z'
        );
    }

    #[test]
    fn cursor_can_be_shown_hidden_and_moved() {
        let mut backend = TestBackend::new(8, 8);
        assert_eq!(backend.cursor_position().unwrap(), Position::ORIGIN);

        backend.set_cursor_position(Position::new(3, 5)).unwrap();
        backend.show_cursor().unwrap();
        assert_eq!(backend.cursor_position().unwrap(), Position::new(3, 5));
        assert!(backend.cursor_visible());

        backend.hide_cursor().unwrap();
        assert!(!backend.cursor_visible());
    }

    #[test]
    fn clear_blanks_every_cell() {
        let mut backend = TestBackend::new(3, 1);
        let cell = Cell::new('x');
        backend
            .draw((0..3).map(|x| (Position::new(x, 0), &cell)))
            .unwrap();
        backend.clear().unwrap();
        assert!(backend.buffer().cells().iter().all(|c| *c == Cell::EMPTY));
    }

    #[test]
    fn resize_preserves_overlapping_cells() {
        let mut backend = TestBackend::new(2, 2);
        let cell = Cell::new('Q');
        backend.draw([(Position::ORIGIN, &cell)]).unwrap();

        backend.resize(4, 4);
        assert_eq!(backend.size().unwrap(), Size::new(4, 4));
        assert_eq!(backend.buffer().get(Position::ORIGIN).unwrap().symbol, 'Q');
    }

    #[test]
    fn display_is_a_deterministic_snapshot() {
        let mut backend = TestBackend::new(3, 2);
        let cell = Cell::new('o');
        backend
            .draw([(Position::new(0, 0), &cell), (Position::new(2, 1), &cell)])
            .unwrap();
        assert_eq!(format!("{backend}"), "o  \n  o\n");
    }
}
