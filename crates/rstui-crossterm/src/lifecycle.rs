//! The panic-safe terminal-lifecycle RAII guard.
//!
//! A full-screen TUI must put the terminal into a non-default state — raw
//! mode, the alternate screen, mouse/paste/focus reporting — and, far more
//! importantly, *reliably restore it*. If an application returns early or
//! **panics** without restoring, the user is left with a broken shell (no
//! echo, no line editing, a frozen alternate screen). [`TerminalGuard`] makes
//! that impossible to get wrong: it enables the requested modes on
//! construction and restores exactly those on [`Drop`] — including while the
//! stack is unwinding from a panic, because Rust runs destructors during
//! unwinding.
//!
//! ## A deliberate divergence from ratatui
//!
//! ratatui ships free `init()`/`restore()` functions plus a manually wired
//! panic hook, leaving lifecycle ownership to the application. rstui owns the
//! loop, so it can own the lifecycle as an RAII value instead: bind the guard
//! and the terminal is restored automatically at end of scope or on panic,
//! with no teardown call to forget. This is the ergonomic win ADR 0001 records
//! for choosing crossterm-behind-a-guard.
//!
//! ## Ordering is borrowed from ratatui's proven teardown
//!
//! Enable order is raw mode, then the escape-sequence modes (alternate screen,
//! mouse, paste, focus). Restore reverses that **except that raw mode is
//! disabled first** — it has the most side effects (input canonicalization and
//! signal generation), so getting the terminal out of raw mode is the
//! highest-priority step on the way down. ratatui's own `try_restore`
//! documents this exact reasoning ("disabling raw mode first is important as it
//! has more side effects than leaving the alternate screen buffer"); rstui
//! reproduces the proven sequence rather than reinventing it. The alternate
//! screen is left *last* so the user's original screen and scrollback are what
//! remain visible.
//!
//! ## Testability and the one PTY-only seam
//!
//! Every escape sequence the guard emits goes through the wrapped writer, so
//! the full enter/leave choreography — and that it still runs while unwinding
//! from a panic — is asserted in memory with **no terminal** (ADR 0001 testing
//! layer L4b; see the tests below). The single exception is
//! [`enable_raw_mode`]/[`disable_raw_mode`], which toggle the real terminal
//! device; [`LifecycleOptions::raw_mode`] gates them, so tests construct a
//! guard with `raw_mode: false` and the raw-mode calls are the only genuine
//! L4c (PTY) surface, exactly as the ADR anticipated.
//!
//! ## Scope: the guard restores the terminal; making a panic *message visible*
//! is the next slice
//!
//! Because [`Drop`] runs during unwinding, a panicking app's terminal **is**
//! restored by this guard — the ADR's "restore on drop including on panic"
//! guarantee. It does not yet make the panic *message* visible: the default
//! panic hook prints before unwinding begins, i.e. while still on the
//! alternate screen, so that text is discarded when the guard later leaves it.
//! Fixing that needs a process-global panic hook installed *before* the
//! default one — a separate concern (global mutable state, ordering against
//! user hooks, a restore path that cannot borrow the guard) that ratatui also
//! implements separately from teardown. It is the natural next slice and
//! belongs with the `rstui-runtime` driver wiring, which owns panic policy.

use std::io::{self, Write};

use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture,
};
use crossterm::queue;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use rstui_core::backend::Backend;
use rstui_core::buffer::Cell;
use rstui_core::geometry::{Position, Size};

use crate::backend::CrosstermBackend;

/// Which terminal modes a [`TerminalGuard`] manages.
///
/// [`Default`] is the full-screen-application preset (every mode on);
/// [`NONE`](LifecycleOptions::NONE) is the empty preset. Build any subset with
/// Rust's struct-update syntax, e.g. a full-screen app that does not want the
/// mouse:
///
/// ```
/// use rstui_crossterm::LifecycleOptions;
///
/// let opts = LifecycleOptions {
///     mouse_capture: false,
///     ..LifecycleOptions::default()
/// };
/// assert!(opts.alternate_screen && !opts.mouse_capture);
/// ```
///
/// `raw_mode` doubles as the test seam: with it `false` the guard touches no
/// real terminal device, so its full behaviour is unit-testable without a TTY.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifecycleOptions {
    /// Put the terminal into raw mode (no line buffering, echo, or signal
    /// generation). This is the only field that touches the real terminal
    /// device rather than the writer.
    pub raw_mode: bool,
    /// Switch to the alternate screen so the app does not scroll the user's
    /// shell history; the original screen is restored on teardown.
    pub alternate_screen: bool,
    /// Report mouse press/drag/scroll as input events.
    pub mouse_capture: bool,
    /// Deliver pasted text as one bracketed-paste event instead of as
    /// synthetic keystrokes.
    pub bracketed_paste: bool,
    /// Report terminal focus gained/lost as input events.
    pub focus_change: bool,
}

impl LifecycleOptions {
    /// No modes enabled — the explicit starting point for a custom subset and
    /// the preset tests use to assert delegation without enter/leave noise.
    pub const NONE: Self = Self {
        raw_mode: false,
        alternate_screen: false,
        mouse_capture: false,
        bracketed_paste: false,
        focus_change: false,
    };
}

impl Default for LifecycleOptions {
    /// The full-screen-application preset: every mode enabled.
    fn default() -> Self {
        Self {
            raw_mode: true,
            alternate_screen: true,
            mouse_capture: true,
            bracketed_paste: true,
            focus_change: true,
        }
    }
}

/// An RAII guard that enables terminal modes and restores them on drop.
///
/// Wraps a [`CrosstermBackend`] and *is itself* a
/// [`Backend`] (it delegates every method to the
/// inner backend), so it drops straight into
/// [`Terminal::new`](rstui_core::Terminal::new) and gives a single panic-safe
/// ownership chain `Terminal -> TerminalGuard -> CrosstermBackend -> W`. When
/// the terminal value is dropped — at end of scope or while unwinding from a
/// panic — the guard restores exactly the modes it enabled.
///
/// ```
/// use rstui_core::backend::Backend;
/// use rstui_crossterm::{CrosstermBackend, LifecycleOptions, TerminalGuard};
///
/// // An in-memory writer with raw mode off needs no terminal.
/// let backend = CrosstermBackend::new(Vec::new());
/// let opts = LifecycleOptions {
///     raw_mode: false,
///     ..LifecycleOptions::default()
/// };
/// let mut guard = TerminalGuard::with_options(backend, opts).unwrap();
///
/// // The enter sequence has been written; the guard is a `Backend`, so
/// // `Terminal::new(guard)` would take it from here.
/// assert!(!guard.backend().writer().is_empty());
///
/// // Dropping it (here explicit; in a real app at end of scope or on a
/// // panic unwind) writes the matching disable sequence.
/// drop(guard);
/// ```
#[must_use = "the guard restores the terminal when dropped; binding it too \
              briefly restores immediately"]
#[derive(Debug)]
pub struct TerminalGuard<W: Write> {
    backend: CrosstermBackend<W>,
    /// The modes to restore on drop. Set to the requested options up front so
    /// that if an enable step fails mid-construction, drop still attempts to
    /// undo the whole set — over-restoring (e.g. a redundant disable escape)
    /// is harmless, whereas under-restoring leaves the terminal broken.
    active: LifecycleOptions,
}

impl<W: Write> TerminalGuard<W> {
    /// Wraps `backend` and enables the full-screen preset
    /// ([`LifecycleOptions::default`]).
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] from the first failing enable step. The
    /// partially constructed guard is dropped on the way out, so any mode that
    /// did get enabled is restored.
    pub fn new(backend: CrosstermBackend<W>) -> io::Result<Self> {
        Self::with_options(backend, LifecycleOptions::default())
    }

    /// Wraps `backend` and enables exactly the modes in `options`.
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] from the first failing enable step; the
    /// partially constructed guard is dropped, restoring anything enabled so
    /// far.
    pub fn with_options(
        backend: CrosstermBackend<W>,
        options: LifecycleOptions,
    ) -> io::Result<Self> {
        let mut guard = Self {
            backend,
            active: options,
        };
        guard.enter()?;
        Ok(guard)
    }

    /// The wrapped backend, e.g. to assert on emitted bytes in tests.
    #[must_use]
    pub fn backend(&self) -> &CrosstermBackend<W> {
        &self.backend
    }

    /// The wrapped backend, mutably.
    pub fn backend_mut(&mut self) -> &mut CrosstermBackend<W> {
        &mut self.backend
    }

    /// Enables the requested modes in the proven acquisition order: raw mode
    /// (the device), then the escape-sequence modes, flushed once.
    fn enter(&mut self) -> io::Result<()> {
        let opts = self.active;
        if opts.raw_mode {
            enable_raw_mode()?;
        }
        let w = self.backend.writer_mut();
        if opts.alternate_screen {
            queue!(w, EnterAlternateScreen)?;
        }
        if opts.mouse_capture {
            queue!(w, EnableMouseCapture)?;
        }
        if opts.bracketed_paste {
            queue!(w, EnableBracketedPaste)?;
        }
        if opts.focus_change {
            queue!(w, EnableFocusChange)?;
        }
        w.flush()
    }
}

impl<W: Write> Drop for TerminalGuard<W> {
    fn drop(&mut self) {
        // `active` is `Copy`; take it before borrowing the backend mutably.
        let active = self.active;

        // Raw mode off first: it has the most side effects, so this is the
        // highest-priority step on the way down (ratatui's documented rule).
        if active.raw_mode {
            let _ = disable_raw_mode();
        }

        // Then the escape-sequence modes in reverse acquisition order, leaving
        // the alternate screen last so the user's original screen and
        // scrollback are what remain visible. Best-effort: a teardown failure
        // has nowhere to go, and an over-broad disable (if construction half
        // failed) is harmless.
        let w = self.backend.writer_mut();
        if active.focus_change {
            let _ = queue!(w, DisableFocusChange);
        }
        if active.bracketed_paste {
            let _ = queue!(w, DisableBracketedPaste);
        }
        if active.mouse_capture {
            let _ = queue!(w, DisableMouseCapture);
        }
        if active.alternate_screen {
            let _ = queue!(w, LeaveAlternateScreen);
        }
        let _ = w.flush();
    }
}

/// `TerminalGuard` is transparently a [`Backend`]: every method delegates to
/// the wrapped [`CrosstermBackend`], so it composes with
/// [`Terminal`](rstui_core::Terminal) unchanged while owning the lifecycle.
impl<W: Write> Backend for TerminalGuard<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, cells: I) -> io::Result<()>
    where
        I: IntoIterator<Item = (Position, &'a Cell)>,
    {
        self.backend.draw(cells)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.backend.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.backend.show_cursor()
    }

    fn cursor_position(&mut self) -> io::Result<Position> {
        self.backend.cursor_position()
    }

    fn set_cursor_position(&mut self, position: Position) -> io::Result<()> {
        self.backend.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.backend.clear()
    }

    fn size(&self) -> io::Result<Size> {
        self.backend.size()
    }

    fn flush(&mut self) -> io::Result<()> {
        self.backend.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    use rstui_core::style::Color;

    use super::*;

    /// A writer whose bytes survive its owner being dropped, so a test can
    /// inspect what the guard emitted *after* it has torn down (including
    /// after a panic unwind). Clones share one buffer.
    #[derive(Clone, Default)]
    struct SharedWriter(Rc<RefCell<Vec<u8>>>);

    impl SharedWriter {
        fn bytes(&self) -> Vec<u8> {
            self.0.borrow().clone()
        }
    }

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Encodes a crossterm command sequence so expectations are stated in
    /// crossterm terms, robust to the exact escape encoding (the technique the
    /// backend tests already use).
    fn encoded(build: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> Vec<u8> {
        let mut out = Vec::new();
        build(&mut out).expect("in-memory writes never fail");
        out
    }

    /// Every escape-sequence mode, raw mode off so no terminal is touched.
    fn escapes_only() -> LifecycleOptions {
        LifecycleOptions {
            raw_mode: false,
            ..LifecycleOptions::default()
        }
    }

    #[test]
    fn default_is_the_full_screen_preset_and_none_is_empty() {
        let full = LifecycleOptions::default();
        assert!(
            full.raw_mode
                && full.alternate_screen
                && full.mouse_capture
                && full.bracketed_paste
                && full.focus_change
        );
        let none = LifecycleOptions::NONE;
        assert!(
            !none.raw_mode
                && !none.alternate_screen
                && !none.mouse_capture
                && !none.bracketed_paste
                && !none.focus_change
        );
    }

    #[test]
    fn enter_writes_the_modes_in_acquisition_order() {
        let backend = CrosstermBackend::new(Vec::new());
        let guard = TerminalGuard::with_options(backend, escapes_only()).unwrap();

        let expected = encoded(|w| {
            queue!(
                w,
                EnterAlternateScreen,
                EnableMouseCapture,
                EnableBracketedPaste,
                EnableFocusChange,
            )
        });
        assert_eq!(guard.backend().writer(), &expected);
    }

    #[test]
    fn drop_writes_the_reverse_disable_sequence() {
        let shared = SharedWriter::default();
        {
            let backend = CrosstermBackend::new(shared.clone());
            let _guard = TerminalGuard::with_options(backend, escapes_only()).unwrap();
            // Guard drops at end of this scope.
        }

        // The full stream is exactly the enter sequence followed by the
        // reverse disable sequence (alternate screen left last). Raw mode is
        // off, so no device call appears.
        let expected = encoded(|w| {
            queue!(
                w,
                EnterAlternateScreen,
                EnableMouseCapture,
                EnableBracketedPaste,
                EnableFocusChange,
                DisableFocusChange,
                DisableBracketedPaste,
                DisableMouseCapture,
                LeaveAlternateScreen,
            )
        });
        assert_eq!(shared.bytes(), expected);
    }

    #[test]
    fn drop_restores_even_while_unwinding_from_a_panic() {
        let shared = SharedWriter::default();
        let writer = shared.clone();

        let result = catch_unwind(AssertUnwindSafe(|| {
            let backend = CrosstermBackend::new(writer);
            let _guard = TerminalGuard::with_options(backend, escapes_only()).unwrap();
            panic!("app blew up mid-frame");
        }));

        assert!(result.is_err(), "the panic must propagate");
        // The destructor still ran during unwinding: the disable sequence is
        // present. This is the deterministic, TTY-free proof of ADR 0001's
        // "restore on drop including on panic" guarantee.
        let expected = encoded(|w| {
            queue!(
                w,
                EnterAlternateScreen,
                EnableMouseCapture,
                EnableBracketedPaste,
                EnableFocusChange,
                DisableFocusChange,
                DisableBracketedPaste,
                DisableMouseCapture,
                LeaveAlternateScreen,
            )
        });
        assert_eq!(shared.bytes(), expected);
    }

    #[test]
    fn only_the_selected_modes_are_entered_and_left() {
        let shared = SharedWriter::default();
        {
            let opts = LifecycleOptions {
                alternate_screen: true,
                ..LifecycleOptions::NONE
            };
            let backend = CrosstermBackend::new(shared.clone());
            let _guard = TerminalGuard::with_options(backend, opts).unwrap();
        }

        // Exactly one mode in, exactly its inverse out — nothing else.
        let expected = encoded(|w| queue!(w, EnterAlternateScreen, LeaveAlternateScreen));
        assert_eq!(shared.bytes(), expected);
    }

    #[test]
    fn delegates_backend_methods_transparently() {
        // No modes, so the byte stream is only the delegated draw output and
        // the guard's drop adds nothing.
        let shared = SharedWriter::default();
        {
            let backend = CrosstermBackend::new(shared.clone());
            let mut guard = TerminalGuard::with_options(backend, LifecycleOptions::NONE).unwrap();

            let mut cell = Cell::new('Z');
            cell.fg = Color::Green;
            guard.draw([(Position::new(0, 0), &cell)]).unwrap();
            guard.flush().unwrap();
        }

        // Identical to driving the inner CrosstermBackend directly: the guard
        // is a transparent Backend.
        let mut direct = CrosstermBackend::new(Vec::new());
        let mut cell = Cell::new('Z');
        cell.fg = Color::Green;
        direct.draw([(Position::new(0, 0), &cell)]).unwrap();
        direct.flush().unwrap();

        assert_eq!(shared.bytes(), *direct.writer());
    }
}
