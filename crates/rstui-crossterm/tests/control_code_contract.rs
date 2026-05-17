//! The control-code regression contract for [ADR
//! 0013](../../../docs/adr/0013-terminal-emulator-compatibility.md).
//!
//! ADR 0013 fixed three cross-emulator control-code bugs. Their unit tests
//! are scattered across `backend.rs` / `style.rs`, so a refactor could break
//! one while the others stay green and nobody would notice. **This file is
//! the single, discoverable, end-to-end lock**: every property is asserted
//! here as a byte-level contract, including the gaps the unit tests miss
//! (lazy-open ordering, `Indexed(>=16)` degrade through `draw`, and the
//! production `run` loop — not just direct `draw` calls). If a future change
//! regresses any ADR-0013 guarantee, `cargo xtask ci` (the `test` gate)
//! fails *here*, by name.
//!
//! No TTY: `CrosstermBackend` over an in-memory `Vec<u8>` (ADR 0001 L4b).

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

use rstui_core::style::ColorLevel;
use rstui_core::{Backend, Cell, Color, Event, Modifier, Position, Style, TestEventSource};
use rstui_crossterm::CrosstermBackend;
use rstui_runtime::{App, Cmd, Frame, run};

/// Begin/End Synchronized Update (DECSET ?2026) and Erase-in-Display 3
/// (purge scrollback) — the exact bytes ADR 0013 mandates.
const BSU: &[u8] = b"\x1b[?2026h";
const ESU: &[u8] = b"\x1b[?2026l";
const ED3: &[u8] = b"\x1b[3J";
/// The raw truecolor SGR payload for `Rgb(200, 30, 40)`. If this appears in
/// a downgraded stream, the degrade did not happen.
const TRUECOLOR_PAYLOAD: &[u8] = b"2;200;30;40";

fn cell(symbol: char, fg: Color) -> Cell {
    Cell {
        symbol,
        fg,
        bg: Color::Reset,
        modifier: Modifier::EMPTY,
    }
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

/// Render `cells` through a fresh backend at `level` and return the bytes.
fn render(level: ColorLevel, cells: &[(Position, Cell)]) -> Vec<u8> {
    let mut backend = CrosstermBackend::new(Vec::new()).with_color_level(level);
    backend
        .draw(cells.iter().map(|(p, c)| (*p, c)))
        .expect("in-memory draw never fails");
    backend.writer().clone()
}

// --- Contract 1: synchronized output envelope ----------------------------

#[test]
fn sync_output_brackets_every_non_empty_frame_exactly_once() {
    // One cell.
    let out = render(
        ColorLevel::TrueColor,
        &[(Position::new(0, 0), cell('x', Color::Reset))],
    );
    assert!(out.starts_with(BSU), "frame must open with ?2026h: {out:?}");
    assert!(out.ends_with(ESU), "frame must close with ?2026l: {out:?}");

    // Fifty contiguous cells: still exactly ONE wrap (not per-cell).
    let many: Vec<_> = (0..50)
        .map(|x| (Position::new(x, 0), cell('a', Color::Reset)))
        .collect();
    let out = render(ColorLevel::TrueColor, &many);
    assert_eq!(count(&out, BSU), 1, "exactly one BSU for the whole frame");
    assert_eq!(count(&out, ESU), 1, "exactly one ESU for the whole frame");
    assert!(out.starts_with(BSU) && out.ends_with(ESU));
}

#[test]
fn empty_frame_is_never_wrapped() {
    // The "idle frame = zero bytes" invariant must survive the wrap.
    let out = render(ColorLevel::TrueColor, &[]);
    assert!(out.is_empty(), "empty diff must emit nothing, got {out:?}");
}

#[test]
fn sync_output_opens_before_any_cursor_or_style_byte() {
    // Closes the unit-test gap: BSU must be the *very first* bytes, emitted
    // lazily before the first cell's MoveTo — not after a cursor move. A
    // first cell at (5, 5) forces a MoveTo; BSU must still precede it.
    let out = render(
        ColorLevel::TrueColor,
        &[(Position::new(5, 5), cell('z', Color::Reset))],
    );
    assert!(
        out.starts_with(BSU),
        "BSU must precede the MoveTo for an offset first cell: {out:?}"
    );
    // Nothing resembling a CSI appears before the BSU prefix.
    assert_eq!(&out[..BSU.len()], BSU);
}

// --- Contract 2: colour degraded through `draw` at every level -----------

#[test]
fn colour_is_degraded_through_draw_for_every_level_and_kind() {
    // The contract: `draw` renders `color` at `level` byte-identically to
    // rendering the *already-degraded* colour at full fidelity — for EVERY
    // colour kind, including `Indexed(>=16)` at Ansi16 (the unit-test gap)
    // and `Reset`. If `draw` ever stops calling `degrade`, these diverge.
    let colours = [
        Color::Rgb(200, 30, 40),
        Color::Indexed(200), // >= 16: must collapse to a 0-15 index at Ansi16
        Color::Indexed(7),
        Color::Red,
        Color::Reset,
    ];
    let levels = [
        ColorLevel::TrueColor,
        ColorLevel::Ansi256,
        ColorLevel::Ansi16,
        ColorLevel::NoColor,
    ];
    for c in colours {
        for lvl in levels {
            let actual = render(lvl, &[(Position::new(0, 0), cell('q', c))]);
            let expected = render(
                ColorLevel::TrueColor,
                &[(Position::new(0, 0), cell('q', c.degrade(lvl)))],
            );
            assert_eq!(
                actual, expected,
                "draw must apply degrade for {c:?} @ {lvl:?}"
            );
        }
    }

    // And concretely: an Rgb cell at Ansi256 must NOT carry the 24-bit SGR.
    let degraded = render(
        ColorLevel::Ansi256,
        &[(Position::new(0, 0), cell('r', Color::Rgb(200, 30, 40)))],
    );
    assert_eq!(
        count(&degraded, TRUECOLOR_PAYLOAD),
        0,
        "Ansi256 must not emit the 38;2 truecolor SGR: {degraded:?}"
    );

    // At NoColor, an Rgb cell emits NO colour-set escape at all (not even a
    // redundant `SetColors(Reset, Reset)`) — the running-state minimisation
    // tracks the degraded colour, so a colour that degrades to default is
    // never written. (Regression lock for the backend fix this contract
    // surfaced.)
    let mono = render(
        ColorLevel::NoColor,
        &[(Position::new(0, 0), cell('m', Color::Rgb(200, 30, 40)))],
    );
    assert_eq!(
        count(&mono, b"38;"),
        0,
        "NoColor: no fg-set escape: {mono:?}"
    );
    assert_eq!(
        count(&mono, b"48;"),
        0,
        "NoColor: no bg-set escape: {mono:?}"
    );
    assert_eq!(
        count(&mono, b"\x1b[39;49m"),
        0,
        "NoColor: no redundant SetColors(Reset,Reset): {mono:?}"
    );
}

// --- Contract 3: scrollback purge on clear -------------------------------

#[test]
fn clear_purges_scrollback() {
    let mut backend = CrosstermBackend::new(Vec::new());
    backend.clear().unwrap();
    let out = backend.writer();
    assert_eq!(count(out, ED3), 1, "clear() must emit exactly one ESC[3J");
    assert!(
        out.ends_with(ED3),
        "ESC[3J must come last in clear() (after erase + home): {out:?}"
    );
}

// --- Contract 4: end-to-end through the production `run` loop ------------

/// A shared in-memory writer so the test can read the bytes after `run`
/// consumes and drops the terminal that owned the backend.
#[derive(Clone, Default)]
struct Shared(Rc<RefCell<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Renders one truecolor cell, then ends (empty event source, no tick) so
/// `run` presents exactly one frame and stops on end-of-input.
struct TruecolorApp;
enum Never {}

impl App for TruecolorApp {
    type Message = Never;
    fn update(&mut self, _: Never) -> Cmd<Never> {
        Cmd::none()
    }
    fn view(&self, frame: &mut Frame<'_>) {
        frame.buffer_mut().set_str(
            Position::new(0, 0),
            "RGB",
            Style::new().fg(Color::Rgb(200, 30, 40)),
        );
    }
}

#[test]
fn production_run_loop_emits_a_synchronized_degraded_stream() {
    // The missing end-to-end lock: not a direct `draw` call but the *real*
    // production loop, over a real `CrosstermBackend`, at a degraded level.
    // The presented bytes must be synchronized-wrapped AND carry no 24-bit
    // SGR — proving both ADR-0013 fixes survive the whole pipeline.
    let sink = Shared::default();
    let backend = CrosstermBackend::new(sink.clone()).with_color_level(ColorLevel::Ansi256);
    let mut events: TestEventSource = TestEventSource::with_events(Vec::<Event>::new());

    run(TruecolorApp, backend, &mut events).expect("run completes on end-of-input");

    let out = sink.0.borrow();
    assert!(
        count(&out, BSU) >= 1 && count(&out, ESU) >= 1,
        "the production loop must synchronized-wrap the frame: {out:?}"
    );
    assert_eq!(
        count(&out, TRUECOLOR_PAYLOAD),
        0,
        "the production loop must degrade Rgb at Ansi256 (no 38;2): {out:?}"
    );
}
