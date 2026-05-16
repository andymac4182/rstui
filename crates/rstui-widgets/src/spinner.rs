//! [`Spinner`] — a one-cell animated busy indicator, the visible "work is
//! happening" affordance for async tasks, loading states, and long commands.
//!
//! # A pure projection of a caller-owned animation index
//!
//! [`List`](crate::List)/[`Tabs`](crate::Tabs) are pure projections of a
//! caller-owned *selection*, [`Gauge`](crate::Gauge) of a caller-owned
//! *scalar*, [`Scrollbar`](crate::Scrollbar) of caller-owned *scroll metrics*.
//! `Spinner` extends the same model to *time*: it is a pure projection of a
//! caller-owned [`tick`](Spinner::tick) — the animation frame index — and
//! displays `frames[tick % frames.len()]`. The widget never advances anything
//! at render time; the reducer owns the tick and the widget reflects it.
//!
//! This is not just a convention here, it is *structurally enforced*.
//! [`Widget::render`] is handed a
//! [`Buffer`], **not** the
//! [`Frame`](rstui_core::Terminal) — so a widget physically cannot read the
//! frame counter and self-animate even if it wanted to. The animation clock
//! `Frame::count()` exposed since the `Terminal` driver landed has, until now,
//! had no consumer; a `Spinner` is its first one, and the seam works exactly
//! as designed: in `App::view(&self, frame)` the caller passes
//! `Spinner::new().tick(frame.count())` (a free monotonic clock) or a tick
//! field its own `Cmd` advances. Either way the widget only reads.
//!
//! # Single-`char` frames, no `Block`, no label
//!
//! Every built-in frame set is a slice of single Unicode scalars
//! (`⠋⠙⠹…`, `|/-\`), so — the same single-`char`
//! [`Cell`](rstui_core::Buffer) dividend borders/`Gauge`/`Scrollbar` banked —
//! frames are a `[char]`, never `&str`, with no grapheme machinery.
//!
//! Like [`Scrollbar`](crate::Scrollbar), `Spinner` has **no optional framing
//! [`Block`](crate::Block)** and **no label**: it is a single-cell *adornment*,
//! not a container, and a label ("Loading…") is ordinary text the app composes
//! beside it with a [`Layout`](rstui_core::Layout) split (Bubble Tea's spinner
//! is glyph-only for the same reason). Keeping the widget to exactly its one
//! responsibility keeps it total and trivially composable.
//!
//! # Clamp, don't panic
//!
//! Per the cross-widget rule [`Gauge`](crate::Gauge) recorded — a pure
//! projection must be *total* — an empty frame set renders nothing (and
//! [`glyph`](Spinner::glyph) returns `None`) instead of panicking on the
//! modulo, and any `tick`, however large, wraps cleanly. A caller-owned
//! counter can never abort the TUI.

use std::borrow::Cow;

use rstui_core::{Buffer, Position, Rect, Style, Widget};

/// A one-cell animated busy indicator.
///
/// `Spinner` draws a single glyph — `frames[tick % frames.len()]` — at the
/// top-left cell of the area it is given (size and place it, typically `1×1`,
/// with a [`Layout`](rstui_core::Layout) split). [`tick`](Self::tick) is an
/// ordinary caller-owned number the widget only reads, so it composes with the
/// Elm `view(&self)` model exactly like [`List`](crate::List) /
/// [`Gauge`](crate::Gauge): advance it in `update` (or pass `frame.count()`)
/// and the spinner animates.
///
/// The default frame set is [`BRAILLE`](Self::BRAILLE); [`LINE`](Self::LINE),
/// [`DOTS`](Self::DOTS), and [`ARC`](Self::ARC) are also provided, or supply
/// your own via [`frames`](Self::frames).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Spinner;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
/// // Frame 3 of the default braille set is its 4th glyph.
/// let spinner = Spinner::new().tick(3);
/// assert_eq!(spinner.glyph(), Some('⠸'));
/// spinner.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '⠸');
///
/// // The tick wraps the frame set, so any caller counter is in range.
/// assert_eq!(Spinner::new().tick(10).glyph(), Spinner::new().tick(0).glyph());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spinner<'a> {
    frames: Cow<'a, [char]>,
    tick: usize,
    style: Style,
}

impl Default for Spinner<'_> {
    /// The [`BRAILLE`](Self::BRAILLE) set at tick `0` with the default
    /// [`Style`] (a hand-written impl because the default frame set is a
    /// non-empty borrowed slice, which `#[derive(Default)]` cannot express —
    /// it would default `Cow<[char]>` to *empty*).
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Spinner<'a> {
    /// The ubiquitous ten-frame braille-dot spinner — the default.
    pub const BRAILLE: &'static [char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

    /// The four-frame ASCII spinner (`|`, `/`, `-`, `\`) — maximally portable
    /// for terminals or pipes without Unicode.
    pub const LINE: &'static [char] = &['|', '/', '-', '\\'];

    /// An eight-frame heavier braille-block spinner.
    pub const DOTS: &'static [char] = &['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];

    /// A six-frame smooth quarter-arc spinner.
    pub const ARC: &'static [char] = &['◜', '◠', '◝', '◞', '◡', '◟'];

    /// A spinner over [`BRAILLE`](Self::BRAILLE) at tick `0`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            frames: Cow::Borrowed(Self::BRAILLE),
            tick: 0,
            style: Style::new(),
        }
    }

    /// Replaces the animation frames.
    ///
    /// Accepts a borrowed slice (the built-in `&'static` sets are zero-alloc)
    /// or an owned `Vec<char>`. An empty set renders nothing rather than
    /// panicking (see the [module docs](self)).
    #[must_use]
    pub fn frames(mut self, frames: impl Into<Cow<'a, [char]>>) -> Self {
        self.frames = frames.into();
        self
    }

    /// Sets the animation index — the caller-owned counter (`frame.count()`,
    /// or a model field a `Cmd` advances). It is taken modulo the frame count,
    /// so any value, however large, is in range.
    #[must_use]
    pub const fn tick(mut self, tick: usize) -> Self {
        self.tick = tick;
        self
    }

    /// Sets the glyph [`Style`].
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The glyph this spinner currently shows — `frames[tick % frames.len()]`
    /// — or `None` if the frame set is empty.
    ///
    /// This is exactly what [`render`](Widget::render) stamps; it is public so
    /// callers can place the glyph themselves (e.g. inside a
    /// [`Line`](rstui_core::Line)) and so tests can assert the projection
    /// without a buffer.
    #[must_use]
    pub fn glyph(&self) -> Option<char> {
        if self.frames.is_empty() {
            None
        } else {
            Some(self.frames[self.tick % self.frames.len()])
        }
    }
}

impl Widget for Spinner<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Nowhere to draw, or no frame to draw: a total no-op (never a
        // modulo-by-zero panic) — the Gauge "a pure projection must be
        // total" rule.
        if area.is_empty() {
            return;
        }
        if let Some(glyph) = self.glyph() {
            buf.set_cell(Position::new(area.x, area.y), glyph, self.style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier};

    /// Renders `spinner` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row.
    fn lines(spinner: Spinner, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        spinner.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn default_is_braille_frame_zero() {
        assert_eq!(Spinner::default().glyph(), Some('⠋'));
        assert_eq!(Spinner::new(), Spinner::default());
        assert_eq!(lines(Spinner::new(), 1, 1), "⠋\n");
    }

    #[test]
    fn the_tick_indexes_the_frame_set() {
        for (tick, glyph) in Spinner::BRAILLE.iter().enumerate() {
            assert_eq!(Spinner::new().tick(tick).glyph(), Some(*glyph));
        }
    }

    #[test]
    fn the_tick_wraps_the_frame_set_so_any_counter_is_in_range() {
        let len = Spinner::BRAILLE.len();
        // One full cycle later is the same glyph; a huge tick never panics.
        assert_eq!(Spinner::new().tick(len).glyph(), Spinner::new().glyph());
        assert_eq!(
            Spinner::new().tick(len * 7 + 3).glyph(),
            Spinner::new().tick(3).glyph()
        );
        assert_eq!(
            Spinner::new().tick(usize::MAX).glyph(),
            Some(Spinner::BRAILLE[usize::MAX % len])
        );
    }

    #[test]
    fn render_stamps_one_glyph_at_the_area_top_left() {
        // A larger area: only its top-left cell is touched, the rest blank.
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        Spinner::new().tick(1).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '⠙');
        for y in 0..3 {
            for x in 0..4 {
                if (x, y) != (0, 0) {
                    assert_eq!(buf.get(Position::new(x, y)).unwrap().symbol, ' ');
                }
            }
        }
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        // Given a sub-area the glyph lands at that area's top-left, so a
        // Layout-placed spinner draws where it was placed.
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 5));
        Spinner::new()
            .tick(2)
            .render(Rect::new(3, 4, 1, 1), &mut buf);
        assert_eq!(buf.get(Position::new(3, 4)).unwrap().symbol, '⠹');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn the_style_paints_the_glyph_cell() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        Spinner::new()
            .tick(0)
            .style(Style::new().fg(Color::Green).add_modifier(Modifier::BOLD))
            .render(buf.area(), &mut buf);
        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.symbol, '⠋');
        assert_eq!(cell.fg, Color::Green);
        assert!(cell.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_borrowed_built_in_set_can_be_swapped_in() {
        // LINE is a &'static slice → Cow::Borrowed, zero-alloc.
        let s = Spinner::new().frames(Spinner::LINE);
        assert_eq!(s.clone().tick(0).glyph(), Some('|'));
        assert_eq!(s.clone().tick(1).glyph(), Some('/'));
        assert_eq!(s.clone().tick(2).glyph(), Some('-'));
        assert_eq!(s.clone().tick(3).glyph(), Some('\\'));
        assert_eq!(s.tick(4).glyph(), Some('|')); // wraps at 4
    }

    #[test]
    fn an_owned_vec_of_frames_works_via_cow_owned() {
        let s = Spinner::new().frames(vec!['a', 'b', 'c']);
        assert_eq!(s.clone().tick(0).glyph(), Some('a'));
        assert_eq!(s.tick(7).glyph(), Some('b')); // 7 % 3 == 1
    }

    #[test]
    fn the_built_in_sets_are_the_documented_lengths() {
        assert_eq!(Spinner::BRAILLE.len(), 10);
        assert_eq!(Spinner::LINE, &['|', '/', '-', '\\']);
        assert_eq!(Spinner::DOTS.len(), 8);
        assert_eq!(Spinner::ARC.len(), 6);
    }

    #[test]
    fn an_empty_frame_set_is_a_total_no_op_not_a_panic() {
        // The modulo would divide by zero; instead glyph() is None and
        // render draws nothing (the "a pure projection must be total" rule).
        let s = Spinner::new().frames(Vec::new()).tick(99);
        assert_eq!(s.glyph(), None);
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        s.render(buf.area(), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        Spinner::new().render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
