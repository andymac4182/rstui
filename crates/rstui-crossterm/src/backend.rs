//! The crossterm [`Backend`] implementation: rstui cells become queued ANSI.
//!
//! [`CrosstermBackend`] is the production side of the
//! [`Backend`] seam `rstui-core` defines. It is
//! generic over any [`std::io::Write`], which is the property that keeps the
//! one non-deterministic crate testable: every escape sequence it emits can be
//! asserted against an in-memory `Vec<u8>` with **no terminal involved** (ADR
//! 0001 testing layer L4b). Only [`size`](Backend::size) and
//! [`cursor_position`](Backend::cursor_position) genuinely query the terminal
//! device rather than the writer; they are the sole L4c (PTY) surface, and
//! every other method is exercised by the in-memory tests below.
//!
//! ## Everything queues; [`flush`](Backend::flush) flushes
//!
//! A deliberate, recorded divergence from ratatui's crossterm backend (which
//! uses crossterm's immediate `execute!` for one-shot operations like clear and
//! cursor moves): **every** method here only *queues* ANSI onto the writer, and
//! the bytes reach the device only when [`flush`](Backend::flush) is called.
//! This is correct precisely because rstui's `Terminal` owns the loop — one
//! `draw` pass queues the cell delta, the cursor state, then issues a single
//! `flush` — so batching every escape into one write per frame is fewer
//! syscalls, no mid-frame flushes, and no flicker. ratatui cannot assume a
//! trailing flush for its one-shot helpers, so it must `execute!`; rstui can,
//! so it does not. The uniform contract also makes the queue-vs-flush boundary
//! directly testable (see `queues_until_flushed`).
//!
//! ## Minimal output
//!
//! [`draw`](Backend::draw) consumes the [`Buffer`](rstui_core::Buffer) diff and
//! carries the proven running-state algorithm: a cursor `MoveTo` is emitted
//! only when the next cell is not the one immediately to the right of the last;
//! foreground/background colors and text attributes are re-emitted only when
//! they actually change from cell to cell; and a trailing reset is written only
//! when at least one cell was drawn, so an **empty diff emits zero bytes** —
//! the byte-level counterpart of rstui's "an idle frame sends nothing"
//! contract.
//!
//! Color and modifier mapping is private: applications depend on `rstui-core`'s
//! `Color`/`Modifier`, never on crossterm's, so the backend stays swappable
//! (stronger isolation than ratatui's public conversion traits, per ADR 0001).

use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::queue;
use crossterm::style::{
    Attribute as CtAttribute, Color as CtColor, Colors as CtColors, Print, SetAttribute,
    SetBackgroundColor, SetColors, SetForegroundColor,
};
use crossterm::terminal::{Clear, ClearType};
use rstui_core::backend::Backend;
use rstui_core::buffer::Cell;
use rstui_core::geometry::{Position, Size};
use rstui_core::style::{Color, ColorLevel, Modifier};

/// Begin Synchronized Update (DECSET ?2026h). A terminal that supports the
/// mode buffers everything until the matching end and presents it atomically,
/// eliminating mid-frame tearing/flicker; terminals that don't support it
/// ignore the unknown private mode harmlessly, so it is emitted
/// unconditionally (no capability round-trip). Adopted from opentui's
/// lazy-per-frame wrap and Textual's `?2026` usage.
const BEGIN_SYNC: &[u8] = b"\x1b[?2026h";
/// End Synchronized Update (DECSET ?2026l) — present the buffered frame.
const END_SYNC: &[u8] = b"\x1b[?2026l";

/// A [`Backend`] that renders rstui cells to a terminal via crossterm.
///
/// Wraps any [`Write`]: a real terminal handle in production
/// (`CrosstermBackend::new(std::io::stdout())`), or an in-memory buffer in
/// tests (`CrosstermBackend::new(Vec::new())`) so the emitted ANSI can be
/// asserted without a TTY. Construct one, hand it to
/// [`Terminal`](rstui_core::Terminal), and the frame driver runs the loop.
///
/// The terminal lifecycle (raw mode, alternate screen, mouse/paste/focus
/// capture, and panic-safe restore) is intentionally **not** owned here yet —
/// that is the next slice's RAII guard. This type is purely the drawing seam.
#[derive(Debug)]
pub struct CrosstermBackend<W: Write> {
    writer: W,
    /// The colour fidelity to render at. Every cell colour is
    /// [`Color::degrade`]d to this before mapping, so a truecolor theme never
    /// emits `38;2` escapes a 256/16-colour terminal cannot parse.
    color: ColorLevel,
}

impl<W: Write> CrosstermBackend<W> {
    /// Wraps `writer` as a backend. Nothing is written until a draw is
    /// flushed. Defaults to [`ColorLevel::TrueColor`] (no downgrade); call
    /// [`with_color_level`](Self::with_color_level) — or, for a real terminal,
    /// [`detect_color_level`](Self::detect_color_level) — to degrade.
    #[must_use]
    pub const fn new(writer: W) -> Self {
        Self {
            writer,
            color: ColorLevel::TrueColor,
        }
    }

    /// Render at `level`, degrading every colour to fit it.
    #[must_use]
    pub fn with_color_level(mut self, level: ColorLevel) -> Self {
        self.color = level;
        self
    }

    /// The colour level this backend renders at.
    #[must_use]
    pub fn color_level(&self) -> ColorLevel {
        self.color
    }

    /// Detect the terminal's colour fidelity from the environment — the
    /// conservative, no-round-trip ladder distilled from the Rich / opentui
    /// deep dive: `FORCE_COLOR` (explicit override) → `NO_COLOR` →
    /// not-a-terminal → `COLORTERM` (`truecolor`/`24bit`) → `TERM`
    /// (`*-256color` ⇒ 256, `dumb` ⇒ none, otherwise the 16-colour floor) →
    /// 256-colour default. `run_app` applies this automatically.
    #[must_use]
    pub fn detect_color_level() -> ColorLevel {
        color_level_from(
            |k| std::env::var(k).ok(),
            std::io::IsTerminal::is_terminal(&std::io::stdout()),
        )
    }

    /// The wrapped writer, e.g. to assert on emitted bytes in tests.
    #[must_use]
    pub fn writer(&self) -> &W {
        &self.writer
    }

    /// The wrapped writer, mutably.
    ///
    /// The seam the future terminal-lifecycle guard writes its enter/leave
    /// escape sequences through, since the [`Backend`] trait deliberately has
    /// no "write raw bytes" method.
    pub fn writer_mut(&mut self) -> &mut W {
        &mut self.writer
    }
}

impl<W: Write> Backend for CrosstermBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, cells: I) -> io::Result<()>
    where
        I: IntoIterator<Item = (Position, &'a Cell)>,
    {
        // The running terminal state. A fresh frame starts from the terminal
        // defaults; we only emit an escape when a cell departs from it.
        let mut fg = Color::Reset;
        let mut bg = Color::Reset;
        let mut modifier = Modifier::EMPTY;
        let mut last_pos: Option<Position> = None;
        // Copied out so the per-cell `degrade` does not borrow `self` while
        // `self.writer` is borrowed mutably below.
        let level = self.color;

        for (pos, cell) in cells {
            // Lazily open a synchronized update on the first changed cell, so
            // the whole repaint is presented atomically. Doing it here (not in
            // `flush`) keeps the invariant below: an empty diff never enters
            // this loop, so it still produces *zero* bytes.
            if last_pos.is_none() {
                self.writer.write_all(BEGIN_SYNC)?;
            }
            // `Print` advances the cursor one column, so a `MoveTo` is only
            // needed when this cell is not immediately right of the last one.
            // `checked_add` keeps the adjacency test overflow-safe at u16::MAX.
            let adjacent =
                matches!(last_pos, Some(p) if p.y == pos.y && p.x.checked_add(1) == Some(pos.x));
            if !adjacent {
                queue!(self.writer, MoveTo(pos.x, pos.y))?;
            }
            last_pos = Some(pos);

            if cell.modifier != modifier {
                write_modifier_diff(modifier, cell.modifier, &mut self.writer)?;
                modifier = cell.modifier;
            }
            if cell.fg != fg || cell.bg != bg {
                // Degrade to the terminal's real fidelity before mapping, so
                // a truecolor theme on a 256/16-colour terminal emits an
                // index it can render instead of an ignored `38;2`.
                let colors = CtColors::new(
                    to_crossterm_color(cell.fg.degrade(level)),
                    to_crossterm_color(cell.bg.degrade(level)),
                );
                queue!(self.writer, SetColors(colors))?;
                fg = cell.fg;
                bg = cell.bg;
            }
            queue!(self.writer, Print(cell.symbol))?;
        }

        // Leave the terminal in a clean state for whatever is drawn next — but
        // only if anything was drawn. An empty diff (an idle frame in rstui's
        // per-frame loop) must produce zero bytes.
        if last_pos.is_some() {
            queue!(
                self.writer,
                SetForegroundColor(CtColor::Reset),
                SetBackgroundColor(CtColor::Reset),
                SetAttribute(CtAttribute::Reset),
            )?;
            // Close the synchronized update opened on the first cell.
            self.writer.write_all(END_SYNC)?;
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        queue!(self.writer, Hide)
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        queue!(self.writer, Show)
    }

    fn cursor_position(&mut self) -> io::Result<Position> {
        // Queries the terminal device, not the writer: the one read path that
        // needs a real TTY (ADR 0001 testing layer L4c), so it is not covered
        // by the in-memory tests below.
        let (x, y) = crossterm::cursor::position()?;
        Ok(Position::new(x, y))
    }

    fn set_cursor_position(&mut self, position: Position) -> io::Result<()> {
        queue!(self.writer, MoveTo(position.x, position.y))
    }

    fn clear(&mut self) -> io::Result<()> {
        queue!(self.writer, Clear(ClearType::All))
    }

    fn size(&self) -> io::Result<Size> {
        // Like `cursor_position`, this asks the terminal device, not the
        // writer — the other L4c (PTY) surface.
        let (width, height) = crossterm::terminal::size()?;
        Ok(Size::new(width, height))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

/// Maps an rstui [`Color`] to its crossterm equivalent.
///
/// The named colors follow the standard ANSI palette, where the "normal"
/// indices 0–7 are crossterm's `Dark*` variants and the "bright" indices 8–15
/// are crossterm's plain variants (e.g. rstui `Red` is ANSI index 1 ==
/// crossterm `DarkRed`; rstui `LightRed` is index 9 == crossterm `Red`). This
/// is the proven ratatui correspondence, matching crossterm 0.29's own color
/// table.
fn to_crossterm_color(color: Color) -> CtColor {
    match color {
        Color::Reset => CtColor::Reset,
        Color::Black => CtColor::Black,
        Color::Red => CtColor::DarkRed,
        Color::Green => CtColor::DarkGreen,
        Color::Yellow => CtColor::DarkYellow,
        Color::Blue => CtColor::DarkBlue,
        Color::Magenta => CtColor::DarkMagenta,
        Color::Cyan => CtColor::DarkCyan,
        Color::Gray => CtColor::Grey,
        Color::DarkGray => CtColor::DarkGrey,
        Color::LightRed => CtColor::Red,
        Color::LightGreen => CtColor::Green,
        Color::LightYellow => CtColor::Yellow,
        Color::LightBlue => CtColor::Blue,
        Color::LightMagenta => CtColor::Magenta,
        Color::LightCyan => CtColor::Cyan,
        Color::White => CtColor::White,
        Color::Indexed(i) => CtColor::AnsiValue(i),
        Color::Rgb(r, g, b) => CtColor::Rgb { r, g, b },
    }
}

/// Pure colour-capability detection: the conservative environment ladder,
/// factored out of [`CrosstermBackend::detect_color_level`] so it is unit
/// testable with no process-wide env mutation. `get` resolves an env var;
/// `is_tty` is whether stdout is a real terminal.
///
/// Order (highest precedence first): `FORCE_COLOR` explicit override →
/// `NO_COLOR` (any value disables, per no-color.org) → not-a-terminal →
/// `COLORTERM` ∈ {`truecolor`,`24bit`} → `TERM` ladder. The point is to
/// **never** assume truecolor: a terminal must positively advertise it.
fn color_level_from(get: impl Fn(&str) -> Option<String>, is_tty: bool) -> ColorLevel {
    if let Some(f) = get("FORCE_COLOR") {
        return match f.trim() {
            "0" | "false" => ColorLevel::NoColor,
            "1" | "true" | "" => ColorLevel::Ansi16,
            "2" => ColorLevel::Ansi256,
            _ => ColorLevel::TrueColor,
        };
    }
    if get("NO_COLOR").is_some() {
        return ColorLevel::NoColor;
    }
    if !is_tty {
        return ColorLevel::NoColor;
    }
    if let Some(ct) = get("COLORTERM") {
        let ct = ct.trim().to_ascii_lowercase();
        if ct == "truecolor" || ct == "24bit" {
            return ColorLevel::TrueColor;
        }
    }
    match get("TERM") {
        // Unset but a real terminal: a safe modern floor, not truecolor.
        None => ColorLevel::Ansi256,
        Some(t) => {
            let t = t.trim().to_ascii_lowercase();
            if t.is_empty() || t == "dumb" {
                ColorLevel::NoColor
            } else if t.contains("256color") || t.contains("-256") {
                ColorLevel::Ansi256
            } else {
                // A known terminal that did not advertise more: 16-colour.
                ColorLevel::Ansi16
            }
        }
    }
}

/// Queues the minimal `SetAttribute` sequence to move the terminal from the
/// `from` attribute set to `to`.
///
/// Mirrors ratatui's proven ordering, including the subtlety that crossterm's
/// intensity reset (`NormalIntensity`) clears bold *and* dim together, so any
/// attribute that survives the reset must be re-applied afterwards. Getting
/// this wrong is a real terminal-correctness bug, so the proven sequence is
/// reproduced rather than reinvented. rstui's [`Modifier`] has no `Sub`
/// operator; [`Modifier::difference`] is the identical set subtraction.
fn write_modifier_diff(from: Modifier, to: Modifier, w: &mut impl Write) -> io::Result<()> {
    let removed = from.difference(to);
    if removed.contains(Modifier::REVERSED) {
        queue!(w, SetAttribute(CtAttribute::NoReverse))?;
    }

    // Bold and Dim are both cleared only by resetting intensity; any of the
    // two that should remain on must then be re-applied.
    let reset_intensity = removed.contains(Modifier::BOLD) || removed.contains(Modifier::DIM);
    if reset_intensity {
        queue!(w, SetAttribute(CtAttribute::NormalIntensity))?;
        if to.contains(Modifier::DIM) {
            queue!(w, SetAttribute(CtAttribute::Dim))?;
        }
        if to.contains(Modifier::BOLD) {
            queue!(w, SetAttribute(CtAttribute::Bold))?;
        }
    }

    if removed.contains(Modifier::ITALIC) {
        queue!(w, SetAttribute(CtAttribute::NoItalic))?;
    }
    if removed.contains(Modifier::UNDERLINED) {
        queue!(w, SetAttribute(CtAttribute::NoUnderline))?;
    }
    if removed.contains(Modifier::CROSSED_OUT) {
        queue!(w, SetAttribute(CtAttribute::NotCrossedOut))?;
    }
    if removed.contains(Modifier::HIDDEN) {
        queue!(w, SetAttribute(CtAttribute::NoHidden))?;
    }
    if removed.contains(Modifier::SLOW_BLINK) || removed.contains(Modifier::RAPID_BLINK) {
        queue!(w, SetAttribute(CtAttribute::NoBlink))?;
    }

    let added = to.difference(from);
    if added.contains(Modifier::REVERSED) {
        queue!(w, SetAttribute(CtAttribute::Reverse))?;
    }
    if added.contains(Modifier::BOLD) && !reset_intensity {
        queue!(w, SetAttribute(CtAttribute::Bold))?;
    }
    if added.contains(Modifier::ITALIC) {
        queue!(w, SetAttribute(CtAttribute::Italic))?;
    }
    if added.contains(Modifier::UNDERLINED) {
        queue!(w, SetAttribute(CtAttribute::Underlined))?;
    }
    if added.contains(Modifier::DIM) && !reset_intensity {
        queue!(w, SetAttribute(CtAttribute::Dim))?;
    }
    if added.contains(Modifier::CROSSED_OUT) {
        queue!(w, SetAttribute(CtAttribute::CrossedOut))?;
    }
    if added.contains(Modifier::HIDDEN) {
        queue!(w, SetAttribute(CtAttribute::Hidden))?;
    }
    if added.contains(Modifier::SLOW_BLINK) {
        queue!(w, SetAttribute(CtAttribute::SlowBlink))?;
    }
    if added.contains(Modifier::RAPID_BLINK) {
        queue!(w, SetAttribute(CtAttribute::RapidBlink))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes a crossterm command sequence into a byte buffer so a test can
    /// state its expectation in crossterm terms (robust to the exact escape
    /// encoding) rather than hand-writing escape strings. The same technique
    /// ratatui uses to assert its backend output.
    fn encoded(build: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> Vec<u8> {
        let mut out = Vec::new();
        build(&mut out).expect("in-memory writes never fail");
        out
    }

    fn cell(symbol: char, fg: Color, bg: Color, modifier: Modifier) -> Cell {
        Cell {
            symbol,
            fg,
            bg,
            modifier,
        }
    }

    /// Wrap an expected per-frame body in the synchronized-output markers the
    /// backend now emits around every non-empty diff (DECSET ?2026).
    fn synced(body: Vec<u8>) -> Vec<u8> {
        let mut v = BEGIN_SYNC.to_vec();
        v.extend(body);
        v.extend_from_slice(END_SYNC);
        v
    }

    #[test]
    fn empty_diff_emits_zero_bytes() {
        let mut backend = CrosstermBackend::new(Vec::new());
        let empty: Vec<(Position, &Cell)> = Vec::new();
        backend.draw(empty).unwrap();
        assert!(
            backend.writer().is_empty(),
            "an idle frame must produce no output, got {:?}",
            backend.writer()
        );
    }

    #[test]
    fn single_cell_emits_move_attrs_colors_print_then_reset() {
        let c = cell('x', Color::Red, Color::Blue, Modifier::BOLD);
        let mut backend = CrosstermBackend::new(Vec::new());
        backend.draw([(Position::new(2, 3), &c)]).unwrap();

        let expected = encoded(|w| {
            queue!(
                w,
                MoveTo(2, 3),
                SetAttribute(CtAttribute::Bold),
                SetColors(CtColors::new(CtColor::DarkRed, CtColor::DarkBlue)),
                Print('x'),
                SetForegroundColor(CtColor::Reset),
                SetBackgroundColor(CtColor::Reset),
                SetAttribute(CtAttribute::Reset),
            )
        });
        assert_eq!(backend.writer(), &synced(expected));
    }

    #[test]
    fn contiguous_run_moves_once_and_a_gap_forces_a_new_move() {
        let a = cell('a', Color::Reset, Color::Reset, Modifier::EMPTY);
        let b = cell('b', Color::Reset, Color::Reset, Modifier::EMPTY);
        let d = cell('d', Color::Reset, Color::Reset, Modifier::EMPTY);

        let mut backend = CrosstermBackend::new(Vec::new());
        backend
            .draw([
                (Position::new(0, 0), &a),
                (Position::new(1, 0), &b),
                // gap at x = 2: not adjacent, so a fresh MoveTo is required.
                (Position::new(3, 0), &d),
            ])
            .unwrap();

        let expected = encoded(|w| {
            queue!(
                w,
                MoveTo(0, 0),
                Print('a'),
                Print('b'),
                MoveTo(3, 0),
                Print('d'),
                SetForegroundColor(CtColor::Reset),
                SetBackgroundColor(CtColor::Reset),
                SetAttribute(CtAttribute::Reset),
            )
        });
        assert_eq!(backend.writer(), &synced(expected));
    }

    #[test]
    fn running_color_and_modifier_state_is_not_re_emitted() {
        // Two adjacent cells sharing style: the color/attribute escapes must
        // appear exactly once, not per cell.
        let a = cell('a', Color::Green, Color::Reset, Modifier::ITALIC);
        let b = cell('b', Color::Green, Color::Reset, Modifier::ITALIC);

        let mut backend = CrosstermBackend::new(Vec::new());
        backend
            .draw([(Position::new(0, 0), &a), (Position::new(1, 0), &b)])
            .unwrap();

        let expected = encoded(|w| {
            queue!(
                w,
                MoveTo(0, 0),
                SetAttribute(CtAttribute::Italic),
                SetColors(CtColors::new(CtColor::DarkGreen, CtColor::Reset)),
                Print('a'),
                Print('b'),
                SetForegroundColor(CtColor::Reset),
                SetBackgroundColor(CtColor::Reset),
                SetAttribute(CtAttribute::Reset),
            )
        });
        assert_eq!(backend.writer(), &synced(expected));
    }

    #[test]
    fn non_empty_frame_is_wrapped_in_synchronized_output() {
        // A non-empty diff must open with BSU (?2026h) and close with ESU
        // (?2026l) so the terminal presents the repaint atomically (no
        // mid-frame tearing). The markers bracket the *entire* frame,
        // including the trailing SGR reset.
        let c = cell('z', Color::Reset, Color::Reset, Modifier::EMPTY);
        let mut backend = CrosstermBackend::new(Vec::new());
        backend.draw([(Position::new(0, 0), &c)]).unwrap();

        let out = backend.writer();
        assert!(
            out.starts_with(BEGIN_SYNC),
            "frame must open with \\x1b[?2026h, got {out:?}"
        );
        assert!(
            out.ends_with(END_SYNC),
            "frame must close with \\x1b[?2026l, got {out:?}"
        );
        // Exactly one wrap (markers are not re-emitted per cell).
        assert_eq!(
            out.windows(BEGIN_SYNC.len())
                .filter(|w| *w == BEGIN_SYNC)
                .count(),
            1
        );
    }

    #[test]
    fn empty_frame_is_not_wrapped_in_synchronized_output() {
        // The "idle frame = zero bytes" invariant must survive the wrap: an
        // empty diff opens no synchronized update at all.
        let mut backend = CrosstermBackend::new(Vec::new());
        let empty: Vec<(Position, &Cell)> = Vec::new();
        backend.draw(empty).unwrap();
        assert!(backend.writer().is_empty());
    }

    /// A writer that distinguishes bytes written from explicit flushes, so the
    /// "queue, do not execute" contract is directly assertable.
    #[derive(Default)]
    struct CountingWriter {
        buf: Vec<u8>,
        flushes: usize,
    }

    impl Write for CountingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.buf.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    #[test]
    fn queues_until_flushed() {
        let mut backend = CrosstermBackend::new(CountingWriter::default());

        backend.hide_cursor().unwrap();
        backend.set_cursor_position(Position::new(4, 2)).unwrap();
        backend.clear().unwrap();

        // Bytes are buffered, but nothing has been flushed to the device: the
        // deliberate divergence from ratatui's `execute!` one-shots.
        assert!(!backend.writer().buf.is_empty());
        assert_eq!(backend.writer().flushes, 0);

        backend.flush().unwrap();
        assert_eq!(backend.writer().flushes, 1);
    }

    #[test]
    fn cursor_and_clear_queue_the_expected_sequences() {
        let mut backend = CrosstermBackend::new(Vec::new());
        backend.hide_cursor().unwrap();
        backend.show_cursor().unwrap();
        backend.set_cursor_position(Position::new(7, 9)).unwrap();
        backend.clear().unwrap();

        let expected = encoded(|w| queue!(w, Hide, Show, MoveTo(7, 9), Clear(ClearType::All)));
        assert_eq!(backend.writer(), &expected);
    }

    #[test]
    fn to_crossterm_color_maps_every_variant() {
        let cases = [
            (Color::Reset, CtColor::Reset),
            (Color::Black, CtColor::Black),
            (Color::Red, CtColor::DarkRed),
            (Color::Green, CtColor::DarkGreen),
            (Color::Yellow, CtColor::DarkYellow),
            (Color::Blue, CtColor::DarkBlue),
            (Color::Magenta, CtColor::DarkMagenta),
            (Color::Cyan, CtColor::DarkCyan),
            (Color::Gray, CtColor::Grey),
            (Color::DarkGray, CtColor::DarkGrey),
            (Color::LightRed, CtColor::Red),
            (Color::LightGreen, CtColor::Green),
            (Color::LightYellow, CtColor::Yellow),
            (Color::LightBlue, CtColor::Blue),
            (Color::LightMagenta, CtColor::Magenta),
            (Color::LightCyan, CtColor::Cyan),
            (Color::White, CtColor::White),
            (Color::Indexed(200), CtColor::AnsiValue(200)),
            (
                Color::Rgb(10, 20, 30),
                CtColor::Rgb {
                    r: 10,
                    g: 20,
                    b: 30,
                },
            ),
        ];
        for (rstui, expected) in cases {
            assert_eq!(
                to_crossterm_color(rstui),
                expected,
                "rstui {rstui:?} should map to crossterm {expected:?}",
            );
        }
    }

    #[test]
    fn modifier_diff_adds_removes_and_reapplies_intensity() {
        let attrs = |a: &[CtAttribute]| {
            encoded(|w| {
                for attr in a {
                    queue!(w, SetAttribute(*attr))?;
                }
                Ok(())
            })
        };

        // Add a single attribute.
        assert_eq!(
            encoded(|w| write_modifier_diff(Modifier::EMPTY, Modifier::BOLD, w)),
            attrs(&[CtAttribute::Bold]),
        );
        // Remove bold: only `NormalIntensity`, nothing re-applied.
        assert_eq!(
            encoded(|w| write_modifier_diff(Modifier::BOLD, Modifier::EMPTY, w)),
            attrs(&[CtAttribute::NormalIntensity]),
        );
        // Bold -> Dim: the intensity reset clears bold, then Dim is applied.
        assert_eq!(
            encoded(|w| write_modifier_diff(Modifier::BOLD, Modifier::DIM, w)),
            attrs(&[CtAttribute::NormalIntensity, CtAttribute::Dim]),
        );
        // Bold|Dim -> Dim: bold is removed, so intensity is reset; Dim
        // survives the reset and must be re-applied afterwards.
        assert_eq!(
            encoded(|w| write_modifier_diff(Modifier::BOLD | Modifier::DIM, Modifier::DIM, w,)),
            attrs(&[CtAttribute::NormalIntensity, CtAttribute::Dim]),
        );
        // Dim -> Bold|Dim: nothing intensity-related is *removed*, so there is
        // no reset churn — only the newly added Bold is emitted.
        assert_eq!(
            encoded(|w| write_modifier_diff(Modifier::DIM, Modifier::BOLD | Modifier::DIM, w,)),
            attrs(&[CtAttribute::Bold]),
        );
        // Reversed toggles on and off independently of intensity.
        assert_eq!(
            encoded(|w| write_modifier_diff(Modifier::EMPTY, Modifier::REVERSED, w)),
            attrs(&[CtAttribute::Reverse]),
        );
        assert_eq!(
            encoded(|w| write_modifier_diff(Modifier::REVERSED, Modifier::EMPTY, w)),
            attrs(&[CtAttribute::NoReverse]),
        );
        // No change emits nothing.
        assert!(encoded(|w| write_modifier_diff(Modifier::BOLD, Modifier::BOLD, w)).is_empty());
    }

    #[test]
    fn color_level_ladder_resolves_each_signal() {
        // A getter built from a fixed table; absent keys return None.
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| (*v).to_owned())
            }
        };

        // FORCE_COLOR is the explicit override and beats everything (incl. a
        // non-tty and NO_COLOR).
        assert_eq!(
            color_level_from(env(&[("FORCE_COLOR", "0"), ("NO_COLOR", "1")]), true),
            ColorLevel::NoColor
        );
        assert_eq!(
            color_level_from(env(&[("FORCE_COLOR", "1")]), false),
            ColorLevel::Ansi16
        );
        assert_eq!(
            color_level_from(env(&[("FORCE_COLOR", "2")]), false),
            ColorLevel::Ansi256
        );
        assert_eq!(
            color_level_from(env(&[("FORCE_COLOR", "3")]), false),
            ColorLevel::TrueColor
        );
        // NO_COLOR (any value) disables colour.
        assert_eq!(
            color_level_from(env(&[("NO_COLOR", ""), ("COLORTERM", "truecolor")]), true),
            ColorLevel::NoColor
        );
        // Not a terminal (piped) ⇒ no colour.
        assert_eq!(
            color_level_from(env(&[("COLORTERM", "truecolor")]), false),
            ColorLevel::NoColor
        );
        // COLORTERM must positively advertise 24-bit.
        assert_eq!(
            color_level_from(env(&[("COLORTERM", "truecolor")]), true),
            ColorLevel::TrueColor
        );
        assert_eq!(
            color_level_from(env(&[("COLORTERM", "24bit")]), true),
            ColorLevel::TrueColor
        );
        // TERM ladder.
        assert_eq!(
            color_level_from(env(&[("TERM", "xterm-256color")]), true),
            ColorLevel::Ansi256
        );
        assert_eq!(
            color_level_from(env(&[("TERM", "dumb")]), true),
            ColorLevel::NoColor
        );
        assert_eq!(
            color_level_from(env(&[("TERM", "xterm")]), true),
            ColorLevel::Ansi16
        );
        // A real terminal with no signals at all: the 256-colour floor, NOT
        // truecolor (the whole point — never assume 24-bit).
        assert_eq!(color_level_from(env(&[]), true), ColorLevel::Ansi256);
    }

    #[test]
    fn draw_degrades_truecolor_to_the_detected_level() {
        let draw_one = |level: ColorLevel, color: Color| -> Vec<u8> {
            let c = cell('x', color, Color::Reset, Modifier::EMPTY);
            let mut b = CrosstermBackend::new(Vec::new()).with_color_level(level);
            b.draw([(Position::new(0, 0), &c)]).unwrap();
            b.writer().clone()
        };
        let red = Color::Rgb(255, 0, 0);

        // TrueColor passes 24-bit through unchanged.
        let truecolor = draw_one(ColorLevel::TrueColor, red);
        // Ansi256 must emit the *degraded index*, identical to drawing the
        // already-degraded `Indexed` at full fidelity — and different from
        // the truecolor bytes.
        let as256 = draw_one(ColorLevel::Ansi256, red);
        assert_eq!(
            as256,
            draw_one(ColorLevel::TrueColor, Color::Indexed(red.to_indexed()))
        );
        assert_ne!(as256, truecolor, "256 must not emit the 38;2 truecolor SGR");
        // Ansi16 collapses to the nearest 0–15 index.
        let as16 = draw_one(ColorLevel::Ansi16, red);
        assert_eq!(
            as16,
            draw_one(ColorLevel::TrueColor, Color::Indexed(red.to_ansi16()))
        );
        // NoColor strips the colour entirely (distinct from every above).
        let none = draw_one(ColorLevel::NoColor, red);
        assert_ne!(none, truecolor);
        assert_ne!(none, as256);
    }
}
