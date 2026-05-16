//! End-to-end runtime-boundary tests that drive real apps through
//! [`rstui_runtime::run`] over a [`TestBackend`] and a [`TestEventSource`],
//! with no TTY, threads, or clock.
//!
//! These exercise [`run`] as an *external* user would (this is a separate
//! integration-test crate, not an inline `#[cfg(test)]` module), and they
//! deliberately cover behaviour the inline unit tests in `run.rs` do **not**
//! prove end to end:
//!
//! - the live loop's repaint-after-every-event rule reflowing a frame after a
//!   scripted [`Event::Resize`] *and* after the backend surface itself grows
//!   (autoresize), asserted on the final frame the production loop presented;
//! - a real [`FocusRing`] routing keys to the focused pane, proving the
//!   `on_event` → `update` → `view` ordering through `run`;
//! - bracketed [`Event::Paste`] ingested and rendered;
//! - terminal-window [`Event::FocusGained`] / [`Event::FocusLost`] flipping an
//!   app flag and repainting (a *different* concept from widget focus);
//! - a clean end-of-input stop (the script drains with no [`Cmd::quit`]) that
//!   leaves the final app state and frame intact.
//!
//! The shared-handle [`RetainedBackend`] mirrors the `SharedTestBackend`
//! technique the inline `renders_each_frame_through_the_real_loop` test uses:
//! `run` consumes and drops the [`Terminal`](rstui_core) it owns, so a test can
//! only inspect the last presented frame through a surface it still holds a
//! handle to.

use std::cell::RefCell;
use std::convert::Infallible;
use std::rc::Rc;

use rstui_core::focus::{FocusId, FocusRing};
use rstui_core::{
    Backend, Cell, Event, KeyCode, KeyEvent, Position, Size, Style, TestBackend, TestEventSource,
};
use rstui_runtime::{App, Cmd, Frame, run};

/// A [`Backend`] sharing its in-memory surface through an `Rc<RefCell<_>>` so a
/// test can assert the final frame *after* [`run`] has consumed and dropped the
/// terminal that owned it.
///
/// This is the exact shared-handle technique the inline
/// `renders_each_frame_through_the_real_loop` / crossterm-guard tests use; it
/// lives here too because an integration test cannot reach `run`'s private
/// terminal any other way. Every method delegates to the inner [`TestBackend`].
#[derive(Clone)]
struct RetainedBackend(Rc<RefCell<TestBackend>>);

impl RetainedBackend {
    /// Wraps a fresh `width` × `height` surface, keeping a clone-able handle to
    /// it so the test can read the final frame and resize the surface from
    /// outside the loop.
    fn new(width: u16, height: u16) -> Self {
        Self(Rc::new(RefCell::new(TestBackend::new(width, height))))
    }

    /// A second handle to the same surface, for the test to inspect/resize.
    fn handle(&self) -> Rc<RefCell<TestBackend>> {
        Rc::clone(&self.0)
    }
}

impl Backend for RetainedBackend {
    type Error = Infallible;

    fn draw<'a, Iter>(&mut self, cells: Iter) -> Result<(), Self::Error>
    where
        Iter: IntoIterator<Item = (Position, &'a Cell)>,
    {
        self.0.borrow_mut().draw(cells)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().show_cursor()
    }

    fn cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.0.borrow_mut().cursor_position()
    }

    fn set_cursor_position(&mut self, position: Position) -> Result<(), Self::Error> {
        self.0.borrow_mut().set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().clear()
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.0.borrow().size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().flush()
    }
}

/// Builds a bare key-press event, the most common scripted input.
fn key(c: char) -> Event {
    Event::from(KeyEvent::char(c))
}

/// Builds a key-press event for a non-character code (Tab, BackTab, …).
fn code(code: KeyCode) -> Event {
    Event::from(KeyEvent::from_code(code))
}

// ---------------------------------------------------------------------------
// 1. Resize reflow through `run` (scripted event *and* surface autoresize).
// ---------------------------------------------------------------------------

/// An app whose view depends on the frame's *width*: it right-aligns a fixed
/// tag, so the rendered row only matches after the surface (and the
/// `Event::Resize` the loop also forwards) has actually reflowed it. This makes
/// "the live loop repaints after every event, including resize" observable on
/// the final frame, which the inline unit tests never assert end to end.
#[derive(Default)]
struct WidthAware {
    /// The width the app last learned from an `Event::Resize`.
    known_width: u16,
}

/// Carries the new width from a forwarded `Event::Resize` into the reducer.
enum WidthMsg {
    Resized(u16),
}

impl App for WidthAware {
    type Message = WidthMsg;

    fn on_event(&self, event: Event) -> Option<WidthMsg> {
        match event {
            Event::Resize(size) => Some(WidthMsg::Resized(size.width)),
            _ => None,
        }
    }

    fn update(&mut self, message: WidthMsg) -> Cmd<WidthMsg> {
        match message {
            WidthMsg::Resized(width) => {
                self.known_width = width;
                Cmd::none()
            }
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        // Right-align "END" against the *frame's* current width, so the glyph
        // column is wrong until both the surface and the reducer reflowed.
        let area = frame.area();
        let tag = "END";
        let start_x = area.right().saturating_sub(tag.len() as u16);
        frame
            .buffer_mut()
            .set_str(Position::new(start_x, 0), tag, Style::new());
        // Echo the width the reducer learned, so both the autoresized surface
        // and the forwarded resize event are visible in one snapshot.
        frame.buffer_mut().set_str(
            Position::new(0, 0),
            &format!("w{}", self.known_width),
            Style::new(),
        );
    }
}

#[test]
fn resize_event_and_surface_autoresize_reflow_the_final_frame() {
    // Start narrow. The test grows the *surface* (so `Terminal::draw`'s
    // autoresize widens the buffer) and also scripts the matching
    // `Event::Resize` the real backend would emit, so both halves of a real
    // resize are exercised through `run`.
    let backend = RetainedBackend::new(6, 1);
    let surface = backend.handle();

    // Grow the shared surface to 12×1 *before* the loop polls the resize event:
    // the first post-init poll triggers autoresize on the next `draw`.
    surface.borrow_mut().resize(12, 1);

    let mut input = TestEventSource::with_events([Event::Resize(Size::new(12, 1))]);
    let app = run(WidthAware::default(), backend, &mut input).unwrap();

    // The reducer saw the forwarded resize…
    assert_eq!(app.known_width, 12);
    // …and the final frame the production loop presented reflowed to 12 cols:
    // "w12" at the left, "END" right-aligned against the *new* width.
    assert_eq!(format!("{}", surface.borrow()), "w12      END\n");
}

// ---------------------------------------------------------------------------
// 2. Focus / input routing through a real `FocusRing`.
// ---------------------------------------------------------------------------

/// A two-pane app whose keyboard is aimed by a [`FocusRing`] held in the
/// model: a typed character is appended to *whichever* pane is focused, and
/// `Tab` / `BackTab` move focus. This proves the full `on_event` (decide
/// intent) → `update` (the only mutation, including the ring) → `view` (reads
/// `is_focused`) pipeline runs in that order through the live [`run`] loop —
/// the headless `run.rs` cases never drive focus routing.
struct TwoPanes {
    ring: FocusRing,
    left: String,
    right: String,
}

/// The id of the left pane in [`TwoPanes::ring`].
const LEFT: FocusId = FocusId::new(0);
/// The id of the right pane in [`TwoPanes::ring`].
const RIGHT: FocusId = FocusId::new(1);

impl Default for TwoPanes {
    fn default() -> Self {
        Self {
            ring: FocusRing::with_ids([LEFT, RIGHT]),
            left: String::new(),
            right: String::new(),
        }
    }
}

/// Intents the panes app maps input to: move focus, or type into the focused
/// pane.
enum PaneMsg {
    FocusNext,
    FocusPrev,
    Type(char),
}

impl App for TwoPanes {
    type Message = PaneMsg;

    fn on_event(&self, event: Event) -> Option<PaneMsg> {
        let key = event.as_key_press()?;
        match key.code {
            KeyCode::Tab => Some(PaneMsg::FocusNext),
            KeyCode::BackTab => Some(PaneMsg::FocusPrev),
            KeyCode::Char(c) => Some(PaneMsg::Type(c)),
            _ => None,
        }
    }

    fn update(&mut self, message: PaneMsg) -> Cmd<PaneMsg> {
        match message {
            PaneMsg::FocusNext => {
                self.ring.focus_next();
            }
            PaneMsg::FocusPrev => {
                self.ring.focus_prev();
            }
            PaneMsg::Type(c) => {
                // The single mutation site routes the key by focus.
                if self.ring.is_focused(LEFT) {
                    self.left.push(c);
                } else if self.ring.is_focused(RIGHT) {
                    self.right.push(c);
                }
            }
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        // `view` only reads the ring (the pure-projection rule): an arrow marks
        // the focused pane so focus is observable in the snapshot itself.
        let left_mark = if self.ring.is_focused(LEFT) { '>' } else { ' ' };
        let right_mark = if self.ring.is_focused(RIGHT) {
            '>'
        } else {
            ' '
        };
        let buffer = frame.buffer_mut();
        buffer.set_str(
            Position::new(0, 0),
            &format!("{left_mark}L:{}", self.left),
            Style::new(),
        );
        buffer.set_str(
            Position::new(0, 1),
            &format!("{right_mark}R:{}", self.right),
            Style::new(),
        );
    }
}

#[test]
fn focus_ring_routes_typed_keys_to_the_focused_pane_through_run() {
    // 'a' lands left (LEFT is focused first), Tab moves to right, 'b' lands
    // right, BackTab returns to left, 'c' lands left.
    let backend = RetainedBackend::new(8, 2);
    let surface = backend.handle();
    let mut input = TestEventSource::with_events([
        key('a'),
        code(KeyCode::Tab),
        key('b'),
        code(KeyCode::BackTab),
        key('c'),
    ]);

    let app = run(TwoPanes::default(), backend, &mut input).unwrap();

    assert_eq!(app.left, "ac");
    assert_eq!(app.right, "b");
    // Focus ended back on the left pane.
    assert!(app.ring.is_focused(LEFT));
    // The final frame proves view read the ring end to end: the focus arrow is
    // on the left row, both panes show their routed text.
    assert_eq!(format!("{}", surface.borrow()), ">L:ac   \n R:b    \n");
}

// ---------------------------------------------------------------------------
// 3. Bracketed paste handled and rendered end to end.
// ---------------------------------------------------------------------------

/// An app that ingests bracketed-paste text. `Event::Paste` carries a
/// `String`, so it is *not* covered by the key-only fixtures the inline tests
/// use; this drives it through `on_event` → `update` → `view` via `run`.
#[derive(Default)]
struct PasteSink {
    /// Everything pasted so far, concatenated in arrival order.
    pasted: String,
}

/// The paste-sink's only intent: absorb one pasted chunk.
struct Pasted(String);

impl App for PasteSink {
    type Message = Pasted;

    fn on_event(&self, event: Event) -> Option<Pasted> {
        match event {
            Event::Paste(text) => Some(Pasted(text)),
            _ => None,
        }
    }

    fn update(&mut self, message: Pasted) -> Cmd<Pasted> {
        self.pasted.push_str(&message.0);
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let pos = frame.area().position();
        frame.buffer_mut().set_str(pos, &self.pasted, Style::new());
    }
}

#[test]
fn pasted_text_is_ingested_and_rendered_through_run() {
    let backend = RetainedBackend::new(11, 1);
    let surface = backend.handle();
    // Two paste chunks plus an ignored key prove concatenation and that a
    // non-paste event leaves the buffer untouched.
    let mut input = TestEventSource::with_events([
        Event::Paste("hello ".to_string()),
        key('x'),
        Event::Paste("rstui".to_string()),
    ]);

    let app = run(PasteSink::default(), backend, &mut input).unwrap();

    assert_eq!(app.pasted, "hello rstui");
    assert_eq!(format!("{}", surface.borrow()), "hello rstui\n");
}

// ---------------------------------------------------------------------------
// 4. Terminal-window focus events flip an app flag and repaint.
// ---------------------------------------------------------------------------

/// An app reacting to *terminal-window* focus (`Event::FocusGained` /
/// `Event::FocusLost`) — explicitly a different concept from widget
/// [`FocusRing`] focus. It dims when the window is unfocused, which a real TUI
/// does to de-emphasise an inactive pane; the inline tests never script these
/// events.
struct WindowDimming {
    /// Whether the terminal window currently has focus (starts focused).
    window_focused: bool,
}

impl Default for WindowDimming {
    fn default() -> Self {
        Self {
            window_focused: true,
        }
    }
}

/// The window-dimming app's intents: the OS told us the window gained or lost
/// focus.
enum WindowMsg {
    Gained,
    Lost,
}

impl App for WindowDimming {
    type Message = WindowMsg;

    fn on_event(&self, event: Event) -> Option<WindowMsg> {
        match event {
            Event::FocusGained => Some(WindowMsg::Gained),
            Event::FocusLost => Some(WindowMsg::Lost),
            _ => None,
        }
    }

    fn update(&mut self, message: WindowMsg) -> Cmd<WindowMsg> {
        match message {
            WindowMsg::Gained => self.window_focused = true,
            WindowMsg::Lost => self.window_focused = false,
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        // "ON " when the window is focused, "dim" when not — so each focus
        // transition is visible in the repainted frame.
        let label = if self.window_focused { "ON " } else { "dim" };
        let pos = frame.area().position();
        frame.buffer_mut().set_str(pos, label, Style::new());
    }
}

#[test]
fn terminal_focus_lost_then_gained_repaints_through_run() {
    let backend = RetainedBackend::new(3, 1);
    let surface = backend.handle();
    // Lose focus, then regain it: the final frame must reflect the *last*
    // transition (focused again), proving every event repaints.
    let mut input =
        TestEventSource::with_events([Event::FocusLost, Event::FocusGained, Event::FocusLost]);

    let app = run(WindowDimming::default(), backend, &mut input).unwrap();

    // Last scripted event was FocusLost, so the app ends unfocused…
    assert!(!app.window_focused);
    // …and the final presented frame shows the dimmed label.
    assert_eq!(format!("{}", surface.borrow()), "dim\n");
}

#[test]
fn terminal_focus_gained_is_the_last_repaint_when_it_ends_focused() {
    // Mirror of the above with the opposite final transition, so the assertion
    // is on the "ON " branch rather than only ever the dim branch.
    let backend = RetainedBackend::new(3, 1);
    let surface = backend.handle();
    let mut input = TestEventSource::with_events([Event::FocusLost, Event::FocusGained]);

    let app = run(WindowDimming::default(), backend, &mut input).unwrap();

    assert!(app.window_focused);
    assert_eq!(format!("{}", surface.borrow()), "ON \n");
}

// ---------------------------------------------------------------------------
// 5. Clean end-of-input stop with the final state and frame intact.
// ---------------------------------------------------------------------------

/// A minimal accumulator that never returns [`Cmd::quit`]: the loop can only
/// end when the scripted source drains and `poll_event(None)` yields
/// `Ok(None)`. This isolates the end-of-input path with a *snapshot* assertion
/// (the inline `exits_cleanly_when_input_is_exhausted` case asserts only state,
/// not the final rendered frame).
#[derive(Default)]
struct Accumulator {
    /// The running sum of every digit key seen.
    total: u32,
}

/// The accumulator's only intent: fold one digit into the sum.
struct AddDigit(u32);

impl App for Accumulator {
    type Message = AddDigit;

    fn on_event(&self, event: Event) -> Option<AddDigit> {
        let key = event.as_key_press()?;
        match key.code {
            KeyCode::Char(c) => c.to_digit(10).map(AddDigit),
            _ => None,
        }
    }

    fn update(&mut self, message: AddDigit) -> Cmd<AddDigit> {
        self.total += message.0;
        // Deliberately never quits: only end-of-input can stop this loop.
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let pos = frame.area().position();
        frame
            .buffer_mut()
            .set_str(pos, &format!("sum={}", self.total), Style::new());
    }
}

#[test]
fn draining_the_source_without_quit_ends_with_state_and_frame_intact() {
    let backend = RetainedBackend::new(6, 1);
    let surface = backend.handle();
    // No quit anywhere; a non-digit key is ignored. The loop ends purely
    // because the source drains (poll_event(None) -> Ok(None)).
    let mut input = TestEventSource::with_events([key('2'), key('z'), key('3'), key('4')]);

    let app = run(Accumulator::default(), backend, &mut input).unwrap();

    // Every digit folded in (2 + 3 + 4); the unmapped 'z' changed nothing.
    assert_eq!(app.total, 9);
    // The source is fully drained — end-of-input stopped the loop, not a quit.
    assert!(input.is_empty());
    // The final frame the loop presented before stopping is intact.
    assert_eq!(format!("{}", surface.borrow()), "sum=9 \n");
}
