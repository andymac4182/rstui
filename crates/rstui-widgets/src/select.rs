//! [`Select`] — a single-line dropdown: a closed field that, when the
//! caller-owned `open` flag is set, drops an opaque option panel anchored to
//! the field, the seventh interactive control and the first *floating* one.
//!
//! # A pure projection of caller-owned `open` + `selected` + `highlight`
//!
//! [`Checkbox`](crate::Checkbox) projects a caller-owned `bool`,
//! [`Input`](crate::Input) a caller-owned [`TextEdit`](rstui_core::TextEdit),
//! [`List`] a caller-owned `selected`/`offset`. `Select` projects
//! **four** ordinary application-state fields the reducer owns and mutates in
//! `update`, never the widget at render time (the read-only-state rule
//! `List`'s `selected`/`offset` establish):
//!
//! - `open` — is the panel dropped (toggled in `update`, typically on
//!   `Enter`/`Esc`).
//! - `selected` — the *committed* choice shown in the closed field (the
//!   `List`-style "which one" datum).
//! - `highlight` — which option the keyboard is on *while the panel is open*
//!   (moved on the arrows); it is committed into `selected` by the reducer on
//!   `Enter`, never here.
//! - `offset` — the panel's scroll offset, exactly `List`'s caller-owned
//!   offset.
//!
//! Opening the panel and committing a highlighted row into `selected` are the
//! reducer's job; the widget only ever reads these fields, so it composes with
//! the Elm `view(&self)` model like every other rstui widget.
//!
//! # Opaque, but deliberately **not** a [`Modal`](crate::Modal)
//!
//! The open panel floats over whatever was already drawn, so — like
//! [`Modal`](crate::Modal) — it must be **opaque**: a [`Style`] is only a
//! patch and cannot reset a cell, so styling alone would let the background
//! bleed through. `Select` borrows `Modal`'s defining technique exactly (see
//! `modal.rs` *"Opaque on purpose"*): it
//! [`clear_region`](rstui_core::Buffer::clear_region)s the panel rect before
//! drawing into it, taking exclusive ownership of those cells.
//!
//! It does **not** reuse `Modal` itself, because a dropdown is not modal: it
//! has no focus-scope trap, it is **anchored to the field** (immediately below
//! it, flipping above only when the screen runs out), not centred in an
//! overlay, and it is sized to its options, not to a `Constraint`. Reusing
//! `Modal` would force all three modal behaviours the dropdown must not have.
//! Instead it **reuses [`List`] wholesale** for the option panel
//! (and [`Block`] for its optional frame): the panel *is* a
//! `List`, so its scrolling, highlight bar, and totality are inherited rather
//! than re-implemented.
//!
//! # The closed field is a leaf, like [`Input`](crate::Input)
//!
//! Closed, `Select` is one row with no `Block` (the `Input`/`Checkbox` leaf
//! shape): the base [`style`](Select::style) fills the row and, when
//! [`focused`](Select::focused), [`focus_style`](Select::focus_style) is
//! patched **last** so the focus emphasis reads as one bar — the same
//! highlight-wins-last idiom `List`/`Checkbox` use. A right-aligned
//! disclosure glyph (`▾`) marks the last column.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*):
//! an empty area is a no-op; a closed field with `selected = None` and no
//! placeholder is a blank row; an out-of-range `selected` falls back to the
//! placeholder/blank; an open panel with no options is a zero-row no-op; a
//! panel that fits neither below nor above is clamped to the larger gap (and
//! both-zero is an empty panel); out-of-range `highlight`/`offset` inherit
//! `List`'s totality; a one-cell field clips. [`panel`](Select::panel) is an
//! empty rect whenever the panel is not drawn. A `RadioGroup`-style owned
//! index, type-ahead, and a multi-select variant are deliberately deferred
//! additives, not smuggled into this slice.

use std::borrow::Cow;

use crate::block::Block;
use crate::list::List;
use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

/// The right-aligned glyph marking the closed field as a dropdown.
const DISCLOSURE: char = '▾';

/// A single-line dropdown rendered as a pure projection of caller-owned
/// `open`/`selected`/`highlight`/`offset` state.
///
/// Closed it is one row — the [`selected`](Self::selected) option's value (or
/// the [`placeholder`](Self::placeholder) when there is none) plus a
/// right-aligned disclosure marker. [`open`](Self::open) additionally drops an
/// **opaque** option panel anchored to the field (below it, or flipped above
/// when the screen runs out), which is an internal [`List`] of
/// the options framed by the optional [`block`](Self::block).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Select;
///
/// // `open`/`selected`/`highlight` are plain caller-owned model state the
/// // widget only reads — dropping the panel and committing a choice happen
/// // in the reducer's `update`, never here.
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
/// Select::new(["Red", "Green", "Blue"])
///     .selected(Some(1))
///     .render(buf.area(), &mut buf);
///
/// // Closed: the selected option's value on one row, with a right-aligned
/// // disclosure marker in the last column.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'G');
/// assert_eq!(buf.get(Position::new(7, 0)).unwrap().symbol, '▾');
/// ```
#[derive(Debug, Clone)]
pub struct Select<'a> {
    options: Vec<Line<'a>>,
    open: bool,
    selected: Option<usize>,
    highlight: usize,
    offset: usize,
    placeholder: Cow<'a, str>,
    block: Option<Block<'a>>,
    style: Style,
    focus_style: Style,
    focused: bool,
    highlight_style: Style,
    open_height: u16,
}

impl Default for Select<'_> {
    fn default() -> Self {
        Self {
            options: Vec::new(),
            open: false,
            selected: None,
            highlight: 0,
            offset: 0,
            placeholder: Cow::Borrowed(""),
            block: None,
            style: Style::default(),
            focus_style: Style::default(),
            focused: false,
            highlight_style: Style::default(),
            // A sensible default panel cap: tall enough for a real menu, short
            // enough to stay anchored to the field on a normal screen.
            open_height: 8,
        }
    }
}

impl<'a> Select<'a> {
    /// A closed dropdown over `options`, nothing selected, no placeholder.
    pub fn new<I, T>(options: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Line<'a>>,
    {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// Sets whether the option panel is dropped — caller-owned state the
    /// widget only reads (toggle it in `update`, typically on `Enter`/`Esc`).
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the *committed* choice shown in the closed field, or `None` for
    /// the placeholder. An out-of-range index falls back to the placeholder.
    #[must_use]
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets which option the open panel highlights (the keyboard row while
    /// open). Committed into [`selected`](Self::selected) by the reducer on
    /// `Enter` — never here. Out of range simply paints no bar (inherited
    /// from [`List`]).
    #[must_use]
    pub fn highlight(mut self, highlight: usize) -> Self {
        self.highlight = highlight;
        self
    }

    /// Sets the open panel's scroll offset (the index of its first visible
    /// row), exactly [`List::offset`](crate::List::offset).
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Sets the hint shown in the closed field when nothing is selected.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<Cow<'a, str>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Frames the open option panel in `block`; the options render into
    /// [`block.inner`](Block::inner), the same compose pattern
    /// [`List`] uses. Does not frame the closed field (a leaf).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the closed field's row so a
    /// background reads as one bar.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets whether the closed field is focused — caller-owned state the
    /// widget only reads (move it in `update`, typically on `Tab`).
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Sets the [`Style`] applied when [`focused`](Self::focused).
    ///
    /// Patched **last** across the full field row, so the focus emphasis
    /// overrides per-span colours and reads as one bar — the same role
    /// [`List`]'s `highlight_style` plays for selection.
    #[must_use]
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = style;
        self
    }

    /// Sets the [`Style`] patched over the highlighted row of the open panel,
    /// forwarded straight to the internal [`List`].
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Sets the maximum number of option rows the open panel shows before it
    /// relies on [`offset`](Self::offset) scrolling (default `8`).
    #[must_use]
    pub fn open_height(mut self, open_height: u16) -> Self {
        self.open_height = open_height;
        self
    }

    /// The rect the open option panel occupies for a closed-field `area`, or
    /// an empty rect when the panel is not drawn (closed, or no visible rows).
    ///
    /// A pure function of `area` and the configuration giving the panel's
    /// natural placement: anchored directly **below** the field, `area.width`
    /// wide, and tall enough for `min(options, open_height)` rows plus any
    /// [`block`](Self::block) frame. [`render`](Widget::render) mirrors this
    /// but, knowing the live buffer, flips the panel **above** the field (or
    /// clamps its height) when the space below is short — the same
    /// derived-geometry-is-a-projection reasoning
    /// [`Input`](crate::Input)'s scroll and [`Modal::area`](crate::Modal::area)
    /// use. Exposed so an app can map a click in the panel back to an option
    /// index.
    #[must_use]
    pub fn panel(&self, area: Rect) -> Rect {
        if !self.open || area.is_empty() {
            return Rect::ZERO;
        }
        let visible = self.visible_rows();
        if visible == 0 {
            return Rect::ZERO;
        }
        let height = visible.saturating_add(Self::block_vertical_frame(self.block.as_ref()));
        Rect::new(area.x, area.bottom(), area.width, height)
    }

    /// How many option rows the panel shows: the options, capped at
    /// [`open_height`](Self::open_height).
    fn visible_rows(&self) -> u16 {
        self.options.len().min(self.open_height as usize) as u16
    }

    /// A [`Block`]'s constant vertical frame overhead (borders + padding).
    ///
    /// [`Block::inner`] is pure arithmetic, so a tall probe measures the
    /// overhead exactly, letting the panel be sized so the inner `List` shows
    /// `visible_rows` content rows.
    fn block_vertical_frame(block: Option<&Block<'_>>) -> u16 {
        block.map_or(0, |b| {
            let probe = Rect::new(0, 0, 1, u16::MAX);
            probe.height.saturating_sub(b.inner(probe).height)
        })
    }
}

impl Widget for Select<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Select {
            options,
            open,
            selected,
            highlight,
            offset,
            placeholder,
            block,
            style,
            focus_style,
            focused,
            highlight_style,
            open_height,
        } = self;

        let y = area.top();
        let left = area.left();
        let right = area.right();

        // --- The closed field row (always drawn, even when open) ---

        // The base, with the focus emphasis patched in when focused. Filling
        // the whole row makes a focused field read as one contiguous bar —
        // Input/Checkbox's focus-bar idiom, here for the dropdown field.
        let base = if focused {
            style.patch(focus_style)
        } else {
            style
        };
        buf.set_style(Rect::new(left, y, area.width, 1), base);

        // The disclosure marker owns the last column; the value/placeholder is
        // clipped before it so they never collide. A zero-width area already
        // returned, so `right >= 1` here; a one-cell field is just the marker.
        let text_right = right.saturating_sub(1);
        buf.set_cell(Position::new(text_right, y), DISCLOSURE, base);

        let mut x = left;
        if let Some(line) = selected.and_then(|i| options.get(i)) {
            // Cascade base → line → span; `focus_style` is patched LAST per
            // glyph when focused so focus wins over per-span colours, exactly
            // as List patches `highlight_style` last.
            let line_base = style.patch(line.style);
            'value: for span in &line.spans {
                let mut span_style = line_base.patch(span.style);
                if focused {
                    span_style = span_style.patch(focus_style);
                }
                for ch in span.content.chars() {
                    if x >= text_right {
                        break 'value;
                    }
                    buf.set_cell(Position::new(x, y), ch, span_style);
                    x = x.saturating_add(1);
                }
            }
        } else {
            // No (in-range) selection: the placeholder hint, styled by the
            // base/focus only (an empty placeholder leaves a blank row).
            for ch in placeholder.chars() {
                if x >= text_right {
                    break;
                }
                buf.set_cell(Position::new(x, y), ch, base);
                x = x.saturating_add(1);
            }
        }

        // --- The open option panel ---

        if !open {
            return;
        }
        let visible = options.len().min(open_height as usize) as u16;
        if visible == 0 {
            // An open dropdown with no rows is a no-op panel — total.
            return;
        }
        let frame_v = Self::block_vertical_frame(block.as_ref());
        let desired = visible.saturating_add(frame_v);

        // Anchor to the field: prefer directly below; flip above when the
        // space below is short; clamp to the larger gap when it fits neither.
        // Both gaps zero ⇒ an empty panel (no-op). `buf.area()` is the screen.
        let screen = buf.area();
        let gap_below = screen.bottom().saturating_sub(area.bottom());
        let gap_above = area.top().saturating_sub(screen.top());
        let panel = if desired <= gap_below {
            Rect::new(area.x, area.bottom(), area.width, desired)
        } else if desired <= gap_above {
            Rect::new(
                area.x,
                area.top().saturating_sub(desired),
                area.width,
                desired,
            )
        } else if gap_below >= gap_above {
            Rect::new(area.x, area.bottom(), area.width, gap_below)
        } else {
            Rect::new(
                area.x,
                area.top().saturating_sub(gap_above),
                area.width,
                gap_above,
            )
        };
        if panel.is_empty() {
            return;
        }

        // Opaque: take exclusive ownership of the panel cells so background
        // content cannot bleed through (the Modal opacity technique — see the
        // module docs for why this is NOT a Modal), then reuse `List`
        // wholesale so its scrolling, highlight bar, and totality are
        // inherited rather than re-implemented.
        buf.clear_region(panel);
        let mut list = List::new(options)
            .selected(Some(highlight))
            .offset(offset)
            .highlight_style(highlight_style);
        if let Some(block) = block {
            list = list.block(block);
        }
        list.render(panel, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Span};

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row.
    fn lines<W: Widget>(widget: W, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    /// Fills `buf` with a styled `.` background so a clear is observable.
    fn background(buf: &mut Buffer) {
        let style = Style::new().fg(Color::Red).bg(Color::Blue);
        for p in buf.area().positions() {
            buf.set_cell(p, '.', style);
        }
    }

    #[test]
    fn closed_shows_the_selected_option_value() {
        let select = Select::new(["Red", "Green", "Blue"]).selected(Some(1));
        assert_eq!(lines(select, 8, 1), "Green  ▾\n");
    }

    #[test]
    fn closed_with_no_selection_shows_the_placeholder() {
        let select = Select::new(["Red"]).placeholder("Pick");
        assert_eq!(lines(select, 8, 1), "Pick   ▾\n");
    }

    #[test]
    fn closed_draws_a_right_aligned_disclosure_marker() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        Select::new(["A"])
            .selected(Some(0))
            .render(buf.area(), &mut buf);
        // The marker is right-anchored: it is always the last column.
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, '▾');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'A');
    }

    #[test]
    fn focused_closed_field_is_a_full_width_focus_bar() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Select::new(["A"])
            .selected(Some(0))
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        // The whole row — value, padding, and the marker — is one focus bar.
        for x in 0..6 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
    }

    #[test]
    fn open_panel_renders_below_the_field() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
        Select::new(["A", "B", "C"])
            .open(true)
            .render(Rect::new(0, 0, 6, 1), &mut buf);
        // Field on row 0; the option panel directly below it.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'A');
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, 'B');
        assert_eq!(buf.get(Position::new(0, 3)).unwrap().symbol, 'C');
    }

    #[test]
    fn open_panel_flips_above_when_it_would_overflow_the_bottom() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 5));
        // Field on the last row: no room below, so the panel flips above it.
        Select::new(["A", "B", "C"])
            .open(true)
            .render(Rect::new(0, 4, 6, 1), &mut buf);
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'A');
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, 'B');
        assert_eq!(buf.get(Position::new(0, 3)).unwrap().symbol, 'C');
        // The field row itself still carries its disclosure marker.
        assert_eq!(buf.get(Position::new(5, 4)).unwrap().symbol, '▾');
    }

    #[test]
    fn open_panel_clamps_height_when_it_fits_neither_way() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 5));
        // Field mid-screen with 8 options: 2 rows below, 2 above, neither
        // enough — clamp to the larger (tied ⇒ below) gap of 2 rows.
        Select::new(["A", "B", "C", "D", "E", "F", "G", "H"])
            .open(true)
            .render(Rect::new(0, 2, 6, 1), &mut buf);
        assert_eq!(buf.get(Position::new(0, 3)).unwrap().symbol, 'A');
        assert_eq!(buf.get(Position::new(0, 4)).unwrap().symbol, 'B');
        // Clamped to 2 rows: nothing flipped above the field.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, ' ');
    }

    #[test]
    fn open_panel_is_opaque_background_does_not_bleed_through() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
        background(&mut buf);
        Select::new(["A"])
            .open(true)
            .render(Rect::new(0, 0, 6, 1), &mut buf);
        // The 1-row panel is cleared opaque: a blank panel cell is EMPTY,
        // not the '.' background, and the red/blue style is gone.
        let cell = buf.get(Position::new(3, 1)).unwrap();
        assert_eq!(cell.symbol, ' ');
        assert_eq!(cell.bg, Color::Reset);
        // Below the (1-row) panel the background is untouched.
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, '.');
    }

    #[test]
    fn highlight_row_gets_the_highlight_bar_in_the_open_panel() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
        Select::new(["A", "B", "C"])
            .open(true)
            .highlight(1)
            .highlight_style(Style::new().bg(Color::Blue))
            .render(Rect::new(0, 0, 6, 1), &mut buf);
        // Option 1 ("B") is on panel row 2 and gets the full-width bar.
        for x in 0..6 {
            assert_eq!(buf.get(Position::new(x, 2)).unwrap().bg, Color::Blue);
        }
        // Its neighbour does not.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn offset_scrolls_the_open_panel() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
        Select::new(["A", "B", "C", "D"])
            .open(true)
            .offset(1)
            .render(Rect::new(0, 0, 6, 1), &mut buf);
        // Offset 1: the panel starts at the second option.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'B');
    }

    #[test]
    fn block_frames_the_open_panel() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 5));
        Select::new(["A"])
            .open(true)
            .block(Block::bordered())
            .render(Rect::new(0, 0, 6, 1), &mut buf);
        // The panel is sized for the frame: border on row 1, option inside.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, 'A');
        assert_eq!(buf.get(Position::new(0, 3)).unwrap().symbol, '└');
    }

    #[test]
    fn open_with_no_options_is_a_total_no_op_panel() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
        background(&mut buf);
        Select::new(Vec::<&str>::new())
            .open(true)
            .render(Rect::new(0, 0, 6, 1), &mut buf);
        // No options ⇒ no panel: the background below the field is intact.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '.');
        assert_eq!(buf.get(Position::new(3, 2)).unwrap().symbol, '.');
        assert!(
            Select::new(Vec::<&str>::new())
                .open(true)
                .panel(Rect::new(0, 0, 6, 1))
                .is_empty()
        );
    }

    #[test]
    fn panel_is_empty_when_closed() {
        let closed = Select::new(["A", "B"]);
        assert!(closed.panel(Rect::new(0, 0, 6, 1)).is_empty());

        // Open: the panel is the natural below-the-field rect.
        let opened = Select::new(["A", "B"]).open(true);
        assert_eq!(opened.panel(Rect::new(0, 0, 6, 1)), Rect::new(0, 1, 6, 2));
    }

    #[test]
    fn style_cascades_base_line_span_and_focus_wins_last() {
        // The selected line is a red span; the field base is green; focused so
        // focus bg is patched LAST (over everything), like List's highlight.
        let option = Line::from(Span::styled("X", Style::new().fg(Color::Red)));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        Select::new([option])
            .selected(Some(0))
            .style(Style::new().fg(Color::Green))
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);

        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.symbol, 'X');
        assert_eq!(cell.fg, Color::Red); // the span fg survives
        assert_eq!(cell.bg, Color::Blue); // focus patched last
    }

    #[test]
    fn an_out_of_range_selection_falls_back_to_the_placeholder() {
        let select = Select::new(["A"]).selected(Some(9)).placeholder("hint");
        assert_eq!(lines(select, 8, 1), "hint   ▾\n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Select::new(["A", "B"])
            .open(true)
            .selected(Some(0))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
