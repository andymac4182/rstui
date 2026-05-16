//! The frame driver that turns "describe a frame" into "update the screen".
//!
//! [`Terminal`] owns a [`Backend`] and a pair of [`Buffer`]s and runs the
//! render loop every TUI needs: size the surface, let the caller draw a frame,
//! send only the cells that changed, place the cursor, then swap buffers so the
//! next frame is drawn from a clean slate. It is the seam a Bubble Tea–style
//! `Model`/`Msg`/`Cmd` runtime will sit on top of — the runtime decides *what*
//! to draw, [`Terminal`] decides *how little* to send.
//!
//! Double buffering is what makes redraws cheap and flicker-free: the caller
//! always draws into a freshly blank [`Frame`], and the other buffer remembers
//! what is on screen so [`Buffer::diff`](crate::Buffer::diff) can report just
//! the delta. Cells the new frame leaves blank are diffed away too, so content
//! that disappears is cleared without the caller tracking it.
//!
//! Like the rest of `rstui-core` this is dependency-free and TTY-free: drive a
//! [`Terminal`] over a [`TestBackend`](crate::TestBackend) and assert on the
//! resulting screen with no terminal involved.
//!
//! Only a fullscreen viewport is supported in this slice. Inline and
//! fixed-size viewports are a later surface and are deliberately not stubbed.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Position, Style, Terminal, TestBackend};
//!
//! let mut terminal = Terminal::new(TestBackend::new(12, 1)).unwrap();
//!
//! let completed = terminal
//!     .draw(|frame| {
//!         let area = frame.area();
//!         frame
//!             .buffer_mut()
//!             .set_str(area.position(), "hello rstui", Style::new());
//!         frame.set_cursor_position(Position::new(11, 0));
//!     })
//!     .unwrap();
//!
//! assert_eq!(completed.count, 0);
//! assert_eq!(format!("{}", terminal.backend()), "hello rstui \n");
//! assert!(terminal.backend().cursor_visible());
//! ```

use crate::backend::Backend;
use crate::buffer::Buffer;
use crate::geometry::{Position, Rect, Size};
use crate::widget::Widget;

/// A single in-progress frame handed to the render closure.
///
/// A `Frame` wraps the buffer the caller draws into plus the metadata a render
/// pass usually wants: the drawable [`area`](Frame::area), a monotonically
/// increasing [`count`](Frame::count) useful for animation phase, and an
/// optional cursor position the [`Terminal`] applies after flushing.
///
/// The buffer is always blank when the frame begins — [`Terminal`] resets it
/// during the previous swap — so a render pass never observes stale cells.
#[derive(Debug)]
pub struct Frame<'a> {
    cursor_position: Option<Position>,
    area: Rect,
    count: usize,
    buffer: &'a mut Buffer,
}

impl Frame<'_> {
    /// The region available to draw into (always anchored at the origin in the
    /// current fullscreen-only viewport).
    #[must_use]
    pub fn area(&self) -> Rect {
        self.area
    }

    /// The drawable area's size, a convenience over [`Frame::area`].
    #[must_use]
    pub fn size(&self) -> Size {
        self.area.size()
    }

    /// The number of frames drawn before this one (`0` for the first frame).
    ///
    /// Handy as a deterministic clock for animations and spinners without
    /// reaching for wall-clock time.
    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    /// The buffer this frame draws into.
    pub fn buffer_mut(&mut self) -> &mut Buffer {
        self.buffer
    }

    /// Draws `widget` into `area` of this frame.
    ///
    /// The ergonomic entry point a view reaches for: `frame.render_widget(
    /// Block::bordered().title("Logs"), area)`. It is exactly
    /// [`Widget::render`] against this frame's buffer — widgets compose by
    /// rendering into [`Block::inner`](crate::Block::inner) sub-areas carved
    /// out with [`Layout`](crate::Layout).
    pub fn render_widget<W: Widget>(&mut self, widget: W, area: Rect) {
        widget.render(area, self.buffer);
    }

    /// Requests that the cursor be shown at `position` after this frame is
    /// flushed.
    ///
    /// If a frame never calls this the cursor is hidden, which is what most
    /// full-screen TUIs want; an input widget calls it to park the caret.
    pub fn set_cursor_position(&mut self, position: Position) {
        self.cursor_position = Some(position);
    }
}

/// A read-only view of the frame that was just rendered, returned by
/// [`Terminal::draw`].
///
/// It borrows the buffer that is now on screen so callers (and tests) can
/// inspect exactly what was presented, along with its area and frame number.
#[derive(Debug)]
pub struct CompletedFrame<'a> {
    /// The buffer that is now displayed.
    pub buffer: &'a Buffer,
    /// The area that buffer covered.
    pub area: Rect,
    /// The zero-based number of this frame.
    pub count: usize,
}

/// Drives the double-buffered render loop over a [`Backend`].
///
/// Construct one with [`Terminal::new`], then call [`Terminal::draw`] once per
/// frame. The terminal is generic over the backend and monomorphized over it
/// (the [`Backend`] trait is intentionally not object-safe), so there is no
/// dynamic dispatch on the hot path.
#[derive(Debug)]
pub struct Terminal<B: Backend> {
    backend: B,
    /// Front/back buffers. `current` is drawn into; the other tracks what is
    /// on screen so the diff is minimal.
    buffers: [Buffer; 2],
    current: usize,
    last_known_area: Rect,
    frame_count: usize,
}

impl<B: Backend> Terminal<B> {
    /// Creates a terminal sized to the backend's current surface.
    ///
    /// Both buffers start blank, so the first [`draw`](Terminal::draw) flushes
    /// every non-blank cell the frame produces.
    ///
    /// # Errors
    ///
    /// Returns [`B::Error`](Backend::Error) if the backend size cannot be
    /// queried.
    pub fn new(backend: B) -> Result<Self, B::Error> {
        let area = Rect::from_size(backend.size()?);
        Ok(Self {
            backend,
            buffers: [Buffer::empty(area), Buffer::empty(area)],
            current: 0,
            last_known_area: area,
            frame_count: 0,
        })
    }

    /// A shared reference to the underlying backend.
    #[must_use]
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// A mutable reference to the underlying backend.
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Consumes the terminal and returns the backend, e.g. to restore terminal
    /// state on shutdown.
    #[must_use]
    pub fn into_backend(self) -> B {
        self.backend
    }

    /// The area the terminal currently believes the screen covers.
    #[must_use]
    pub fn area(&self) -> Rect {
        self.last_known_area
    }

    /// Builds the [`Frame`] for the next render pass.
    ///
    /// Most callers use [`Terminal::draw`]; this is exposed for runtimes that
    /// want to own the loop and call [`Terminal::flush`] themselves.
    pub fn get_frame(&mut self) -> Frame<'_> {
        let count = self.frame_count;
        let area = self.last_known_area;
        Frame {
            cursor_position: None,
            area,
            count,
            buffer: &mut self.buffers[self.current],
        }
    }

    /// Renders one frame and presents it.
    ///
    /// The flow is: reconcile the surface size, hand `render` a blank
    /// [`Frame`], diff the result against what is on screen and send only the
    /// changed cells, apply the requested cursor state, flush the backend, and
    /// swap buffers so the next frame starts clean.
    ///
    /// Returns a [`CompletedFrame`] borrowing the buffer that is now displayed.
    ///
    /// # Errors
    ///
    /// Returns [`B::Error`](Backend::Error) if any backend operation fails.
    pub fn draw<F>(&mut self, render: F) -> Result<CompletedFrame<'_>, B::Error>
    where
        F: FnOnce(&mut Frame),
    {
        self.autoresize()?;

        let count = self.frame_count;
        let mut frame = self.get_frame();
        render(&mut frame);
        let cursor_position = frame.cursor_position;

        self.flush()?;

        match cursor_position {
            None => self.backend.hide_cursor()?,
            Some(position) => {
                self.backend.show_cursor()?;
                self.backend.set_cursor_position(position)?;
            }
        }

        self.backend.flush()?;
        self.swap_buffers();
        self.frame_count = self.frame_count.wrapping_add(1);

        let area = self.last_known_area;
        Ok(CompletedFrame {
            buffer: &self.buffers[1 - self.current],
            area,
            count,
        })
    }

    /// Sends the cells that changed since the last presented frame to the
    /// backend.
    ///
    /// Exposed so a runtime owning the loop can drive `get_frame` → `flush`
    /// itself; [`Terminal::draw`] calls this for you.
    ///
    /// # Errors
    ///
    /// Returns [`B::Error`](Backend::Error) if the backend draw fails.
    pub fn flush(&mut self) -> Result<(), B::Error> {
        // Borrow the two buffer slots and the backend as disjoint fields so
        // the diff (which borrows `buffers`) and the draw (which borrows
        // `backend`) can coexist without an intermediate allocation.
        let previous = &self.buffers[1 - self.current];
        let current = &self.buffers[self.current];
        let updates = current.diff(previous);
        self.backend.draw(updates)
    }

    /// Clears the screen and forces the next frame to redraw in full.
    ///
    /// The on-screen-tracking buffer is reset to blank so the following diff
    /// reports every non-blank cell, matching the now-empty terminal.
    ///
    /// # Errors
    ///
    /// Returns [`B::Error`](Backend::Error) if the backend clear fails.
    pub fn clear(&mut self) -> Result<(), B::Error> {
        self.backend.clear()?;
        self.buffers[1 - self.current].reset();
        Ok(())
    }

    /// Resizes both buffers to `area` and forces a full redraw.
    ///
    /// A terminal resize reflows whatever was on screen, so the old contents
    /// can no longer be trusted; this clears the surface and invalidates the
    /// tracking buffer so the next frame is sent in full.
    ///
    /// # Errors
    ///
    /// Returns [`B::Error`](Backend::Error) if clearing the resized surface
    /// fails.
    pub fn resize(&mut self, area: Rect) -> Result<(), B::Error> {
        for buffer in &mut self.buffers {
            buffer.resize(area);
        }
        self.last_known_area = area;
        self.clear()
    }

    /// Resizes to match the backend if the surface size changed.
    fn autoresize(&mut self) -> Result<(), B::Error> {
        let area = Rect::from_size(self.backend.size()?);
        if area != self.last_known_area {
            self.resize(area)?;
        }
        Ok(())
    }

    /// Blanks the buffer that is about to become current and swaps.
    ///
    /// After this the just-presented frame is the tracking buffer and the
    /// caller's next frame is drawn into a clean buffer.
    fn swap_buffers(&mut self) {
        self.buffers[1 - self.current].reset();
        self.current = 1 - self.current;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use crate::buffer::Cell;
    use crate::style::Style;
    use crate::{Color, TestBackend};
    use std::convert::Infallible;

    #[test]
    fn new_sizes_buffers_to_the_backend() {
        let mut terminal = Terminal::new(TestBackend::new(10, 4)).unwrap();
        assert_eq!(terminal.area(), Rect::new(0, 0, 10, 4));
        // The frame the render closure receives covers the same area.
        assert_eq!(terminal.get_frame().area(), Rect::new(0, 0, 10, 4));
    }

    #[test]
    fn draw_presents_the_rendered_frame() {
        let mut terminal = Terminal::new(TestBackend::new(6, 1)).unwrap();
        let completed = terminal
            .draw(|frame| {
                let pos = frame.area().position();
                frame.buffer_mut().set_str(pos, "hi", Style::new());
            })
            .unwrap();

        assert_eq!(completed.count, 0);
        assert_eq!(completed.area, Rect::new(0, 0, 6, 1));
        assert_eq!(completed.buffer.get(Position::ORIGIN).unwrap().symbol, 'h');
        assert_eq!(format!("{}", terminal.backend()), "hi    \n");
    }

    #[test]
    fn vacated_cells_are_cleared_between_frames() {
        let mut terminal = Terminal::new(TestBackend::new(6, 1)).unwrap();
        terminal
            .draw(|f| {
                f.buffer_mut()
                    .set_str(Position::ORIGIN, "hello", Style::new());
            })
            .unwrap();
        assert_eq!(format!("{}", terminal.backend()), "hello \n");

        // The shorter second frame draws into a blank buffer, so "llo" must be
        // diffed away without the caller tracking it.
        terminal
            .draw(|f| {
                f.buffer_mut().set_str(Position::ORIGIN, "hi", Style::new());
            })
            .unwrap();
        assert_eq!(format!("{}", terminal.backend()), "hi    \n");
    }

    #[test]
    fn frame_count_increments_and_is_visible_during_render() {
        let mut terminal = Terminal::new(TestBackend::new(3, 1)).unwrap();
        let seen = std::cell::Cell::new(usize::MAX);
        let first = terminal.draw(|f| seen.set(f.count())).unwrap();
        assert_eq!(seen.get(), 0);
        assert_eq!(first.count, 0);

        let second = terminal.draw(|f| seen.set(f.count())).unwrap();
        assert_eq!(seen.get(), 1);
        assert_eq!(second.count, 1);
    }

    #[test]
    fn cursor_is_hidden_unless_a_frame_requests_it() {
        let mut terminal = Terminal::new(TestBackend::new(4, 1)).unwrap();

        terminal
            .draw(|f| f.set_cursor_position(Position::new(2, 0)))
            .unwrap();
        assert!(terminal.backend().cursor_visible());
        assert_eq!(
            terminal.backend_mut().cursor_position().unwrap(),
            Position::new(2, 0)
        );

        terminal.draw(|_| {}).unwrap();
        assert!(!terminal.backend().cursor_visible());
    }

    #[test]
    fn render_widget_draws_into_the_frame_buffer() {
        use crate::widget::Block;

        let mut terminal = Terminal::new(TestBackend::new(4, 3)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                frame.render_widget(Block::bordered(), area);
            })
            .unwrap();

        assert_eq!(format!("{}", terminal.backend()), "┌──┐\n│  │\n└──┘\n");
    }

    #[test]
    fn autoresize_redraws_the_full_screen_after_a_resize() {
        let mut terminal = Terminal::new(TestBackend::new(3, 1)).unwrap();
        terminal
            .draw(|f| {
                f.buffer_mut().set_str(Position::ORIGIN, "ab", Style::new());
            })
            .unwrap();

        terminal.backend_mut().resize(5, 2);
        let completed = terminal
            .draw(|f| {
                f.buffer_mut()
                    .set_str(Position::ORIGIN, "wxyz", Style::new());
            })
            .unwrap();

        assert_eq!(completed.area, Rect::new(0, 0, 5, 2));
        assert_eq!(terminal.area(), Rect::new(0, 0, 5, 2));
        assert_eq!(format!("{}", terminal.backend()), "wxyz \n     \n");
    }

    /// A backend that records how many cells each `draw` call received, so the
    /// diff-minimal contract can be asserted directly.
    #[derive(Debug)]
    struct RecordingBackend {
        inner: TestBackend,
        last_draw_len: usize,
    }

    impl RecordingBackend {
        fn new(width: u16, height: u16) -> Self {
            Self {
                inner: TestBackend::new(width, height),
                last_draw_len: 0,
            }
        }
    }

    impl Backend for RecordingBackend {
        type Error = Infallible;

        fn draw<'a, I>(&mut self, cells: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = (Position, &'a Cell)>,
        {
            let cells: Vec<_> = cells.into_iter().collect();
            self.last_draw_len = cells.len();
            self.inner.draw(cells)
        }

        fn hide_cursor(&mut self) -> Result<(), Self::Error> {
            self.inner.hide_cursor()
        }

        fn show_cursor(&mut self) -> Result<(), Self::Error> {
            self.inner.show_cursor()
        }

        fn cursor_position(&mut self) -> Result<Position, Self::Error> {
            self.inner.cursor_position()
        }

        fn set_cursor_position(&mut self, position: Position) -> Result<(), Self::Error> {
            self.inner.set_cursor_position(position)
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            self.inner.clear()
        }

        fn size(&self) -> Result<Size, Self::Error> {
            self.inner.size()
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            self.inner.flush()
        }
    }

    #[test]
    fn redrawing_an_identical_frame_sends_zero_cells() {
        let mut terminal = Terminal::new(RecordingBackend::new(4, 1)).unwrap();
        let paint = |f: &mut Frame| {
            f.buffer_mut()
                .set_str(Position::ORIGIN, "ok", Style::new().fg(Color::Green));
        };

        terminal.draw(paint).unwrap();
        assert_eq!(terminal.backend().last_draw_len, 2);

        // Same content, drawn into a clean buffer: the diff is empty, so the
        // backend must receive nothing.
        terminal.draw(paint).unwrap();
        assert_eq!(terminal.backend().last_draw_len, 0);
    }
}
