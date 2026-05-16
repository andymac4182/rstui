//! [`Input`] — a single-line text-entry field, the fourth interactive
//! form control and the **first text-edit/cursor widget**.
//!
//! # A pure projection of a caller-owned [`TextEdit`] + `focused`
//!
//! [`Checkbox`](crate::Checkbox) projects a caller-owned `bool`,
//! [`Radio`](crate::Radio) a caller-owned selection. `Input` projects a
//! caller-owned **[`TextEdit`]** (the `rstui-core` single-line editing model:
//! a `String` plus a character-indexed cursor) plus a `focused: bool`. The
//! widget borrows the [`TextEdit`] — [`Input::new`] takes `&TextEdit` — and
//! only ever *reads* [`value`](TextEdit::value) and
//! [`cursor`](TextEdit::cursor); the reducer owns the edit and mutates it in
//! `update` (insert on a `Char`, `delete_backward` on `Backspace`, move on the
//! arrows). The widget never edits anything at render time, so it composes
//! with the Elm `view(&self)` model exactly like every other rstui widget.
//!
//! This is the first consumer of the [`focus`](rstui_core::focus) model: a
//! form holds one `FocusRing` of [`FocusId`](rstui_core::FocusId)s and projects
//! `focused(ring.is_focused(id))` into each `Input`. Which field is focused,
//! and routing a keystroke to *that* field's [`TextEdit`], is the reducer's
//! job (ADR 0004 §4) — never the runtime's or the widget's.
//!
//! # The cursor is *rendered*, not the terminal's — on purpose
//!
//! A [`Widget`] is handed only a
//! [`Buffer`] at render time, never the
//! [`Frame`](rstui_core::Frame), so it physically *cannot* call
//! `Frame::set_cursor_position`. `Input` therefore draws its **own** caret: when
//! [`focused`](Input::focused) the cell at the cursor column is stamped with
//! [`cursor_style`](Input::cursor_style) (default
//! [`Modifier::REVERSED`](rstui_core::Modifier::REVERSED), so a focused field
//! shows a visible block caret with zero configuration — the cursor *is* a text
//! field's defining affordance, the one justified exception to the
//! form-control "styles default empty" rule). This is the same approach
//! retained-mode OpenTUI takes (it draws its own cursor) and it is the only
//! choice that is TTY-free snapshot-testable: the rendered caret shows in a
//! [`TestBackend`](rstui_core::TestBackend) frame, the terminal hardware cursor
//! does not. An app that *also* wants the blinking hardware cursor places it
//! itself in `view` via the [`Frame`](rstui_core::Frame) — a clean future
//! additive, deliberately out of this slice.
//!
//! # Stateless right-anchored horizontal scroll
//!
//! When the value is longer than the field, the cursor must stay visible.
//! `Input` derives the scroll as a **pure function of `cursor` and width**
//! (`scroll = cursor − (width − 1)`, clamped at zero) so the caret is always
//! on screen with **no caller-owned scroll state** — the same "derived scroll
//! metric is a pure projection" reasoning [`Scrollbar`](crate::Scrollbar)
//! uses. A caller-owned scroll offset (the nicer "only scroll when the caret
//! leaves the view" UX, the [`List`](crate::List) `offset` shape) is a
//! deliberately deferred additive, not smuggled into this slice.
//!
//! # A leaf control: one row, no `Block`
//!
//! Like the other form controls
//! ([`Checkbox`](crate::Checkbox)/[`Radio`](crate::Radio)/[`Button`](crate::Button))
//! and unlike the container widgets, `Input` has **no framing
//! [`Block`](crate::Block)**: it draws on exactly the top row of its area, and
//! the surrounding form / [`Layout`](rstui_core::Layout) (or a `Block` whose
//! `inner` the field is rendered into, as the demo shows) owns any frame and
//! vertical placement.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*):
//! an empty area, a one-cell area, a value far wider than the field, a
//! multi-byte value, a multi-row area, and an empty value are all safe
//! clips/no-ops — never a panic. [`TextEdit`] already guarantees
//! `cursor ∈ 0..=len`, so the column math cannot escape the field.

use std::borrow::Cow;

use rstui_core::{Buffer, Modifier, Position, Rect, Style, TextEdit, Widget};

/// A single-line text-entry field rendered as a pure projection of a
/// caller-owned [`TextEdit`] and a [`focused`](Self::focused) `bool`.
///
/// Layout is the value on one row, horizontally scrolled so the caret is
/// always visible. The base [`style`](Self::style) fills the whole row (so a
/// background reads as one bar); when [`focused`](Self::focused),
/// [`focus_style`](Self::focus_style) is patched **last** over the row — the
/// same highlight-wins-last bar [`List`](crate::List)/[`Checkbox`](crate::Checkbox)
/// use — and the caret cell additionally gets
/// [`cursor_style`](Self::cursor_style). When the value is empty an optional
/// [`placeholder`](Self::placeholder) hint is shown instead, styled with
/// [`placeholder_style`](Self::placeholder_style).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, TextEdit, Widget};
/// use rstui_widgets::Input;
///
/// // `edit` is plain caller-owned model state the widget only reads.
/// let mut edit = TextEdit::from_value("Ada");
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
/// Input::new(&edit).focused(true).render(buf.area(), &mut buf);
///
/// // The value renders left-aligned: "Ada".
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'A');
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'a');
/// // The cursor is at the end (col 3 — the blank just past the text),
/// // drawn reversed because the field is focused.
/// assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, ' ');
///
/// // Editing happens in the reducer, on the model — never in the widget.
/// edit.move_home();
/// edit.insert_char('!');
/// assert_eq!(edit.value(), "!Ada");
/// ```
#[derive(Debug, Clone)]
pub struct Input<'a> {
    edit: &'a TextEdit,
    focused: bool,
    placeholder: Cow<'a, str>,
    style: Style,
    focus_style: Style,
    cursor_style: Style,
    placeholder_style: Style,
}

impl<'a> Input<'a> {
    /// An input projecting `edit`: unfocused, no placeholder, a default
    /// reversed-cell caret and otherwise unstyled.
    #[must_use]
    pub fn new(edit: &'a TextEdit) -> Self {
        Self {
            edit,
            focused: false,
            placeholder: Cow::Borrowed(""),
            style: Style::new(),
            focus_style: Style::new(),
            // The caret is a text field's defining affordance: a focused field
            // with an invisible cursor is broken, so unlike `focus_style` this
            // defaults to a visible reverse-video block rather than empty.
            cursor_style: Style::new().add_modifier(Modifier::REVERSED),
            placeholder_style: Style::new(),
        }
    }

    /// Sets whether this field is focused — caller-owned state the widget only
    /// reads (move it in `update`, typically on `Tab`, e.g. via a
    /// `FocusRing`). When `true` the [`focus_style`](Self::focus_style) bar and
    /// the [`cursor_style`](Self::cursor_style) caret are drawn.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Sets the hint shown when the value is empty (default none).
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<Cow<'a, str>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets the base [`Style`]. It also fills the field's row so a background
    /// covers it edge to edge.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] applied when [`focused`](Self::focused).
    ///
    /// Patched **last** across the full row, so the focus emphasis overrides
    /// the base and reads as one bar — the same role
    /// [`List`](crate::List)'s `highlight_style` plays for selection.
    #[must_use]
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = style;
        self
    }

    /// Sets the [`Style`] of the caret cell when [`focused`](Self::focused)
    /// (default [`Modifier::REVERSED`](rstui_core::Modifier::REVERSED)).
    ///
    /// Patched over the base/focus row style at exactly the cursor column.
    #[must_use]
    pub fn cursor_style(mut self, style: Style) -> Self {
        self.cursor_style = style;
        self
    }

    /// Sets the [`Style`] of the [`placeholder`](Self::placeholder) hint,
    /// patched over the base (and the focus bar when focused).
    #[must_use]
    pub fn placeholder_style(mut self, style: Style) -> Self {
        self.placeholder_style = style;
        self
    }
}

impl Widget for Input<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Input {
            edit,
            focused,
            placeholder,
            style,
            focus_style,
            cursor_style,
            placeholder_style,
        } = self;

        let y = area.top();
        let left = area.left();
        let right = area.right();
        let width = area.width as usize;

        // The base, with the focus emphasis patched in when focused. Filling
        // the whole row makes a focused field read as one contiguous bar —
        // List's selection-bar idiom, here keyed by `focused`.
        let base = if focused {
            style.patch(focus_style)
        } else {
            style
        };
        buf.set_style(Rect::new(left, y, area.width, 1), base);

        let value = edit.value();

        // Empty value: show the placeholder hint (never scrolled). When
        // focused, the caret sits at column 0 over the placeholder's first
        // glyph (a reversed blank if there is no placeholder) — the same
        // "caret reverses the glyph under it" rule the value path uses.
        if value.is_empty() {
            let placeholder = placeholder.as_ref();
            let ph_style = base.patch(placeholder_style);
            let mut x = left;
            for ch in placeholder.chars() {
                if x >= right {
                    break;
                }
                buf.set_cell(Position::new(x, y), ch, ph_style);
                x = x.saturating_add(1);
            }
            if focused {
                let glyph = placeholder.chars().next().unwrap_or(' ');
                buf.set_cell(Position::new(left, y), glyph, base.patch(cursor_style));
            }
            return;
        }

        // Right-anchored stateless horizontal scroll: a pure function of the
        // caller-owned cursor and the field width, so the caret is always in
        // view with no caller-owned scroll state (see the module docs).
        let cursor = edit.cursor();
        let scroll = cursor.saturating_sub(width.saturating_sub(1));

        // Stamp the visible window [scroll, scroll+width) and remember where
        // the caret column lands (the char at `cursor`, or the blank just past
        // the end when the cursor is appending).
        let mut x = left;
        let mut caret_x = None;
        for (i, ch) in value.chars().enumerate().skip(scroll) {
            if x >= right {
                break;
            }
            if i == cursor {
                caret_x = Some(x);
            }
            buf.set_cell(Position::new(x, y), ch, base);
            x = x.saturating_add(1);
        }
        // Cursor at end-of-text: it is the next free cell, if it still fits.
        if caret_x.is_none() && x < right {
            caret_x = Some(x);
        }

        if focused {
            if let Some(cx) = caret_x {
                let glyph = value.chars().nth(cursor).unwrap_or(' ');
                buf.set_cell(Position::new(cx, y), glyph, base.patch(cursor_style));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Color;

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

    #[test]
    fn renders_the_value_left_aligned_and_pads_the_row() {
        let edit = TextEdit::from_value("hi");
        assert_eq!(lines(Input::new(&edit), 6, 1), "hi    \n");
    }

    #[test]
    fn a_value_that_fits_is_not_scrolled() {
        // Cursor at end (5) but width 8 fits it, so scroll stays 0.
        let edit = TextEdit::from_value("abcde");
        assert_eq!(lines(Input::new(&edit), 8, 1), "abcde   \n");
    }

    #[test]
    fn a_long_value_scrolls_right_to_keep_the_end_cursor_visible() {
        // 10 chars, width 5, cursor at end (10): scroll = 10 - 4 = 6, so the
        // window is chars[6..] = "ghij" and the caret blank is the 5th cell.
        let edit = TextEdit::from_value("abcdefghij");
        assert_eq!(lines(Input::new(&edit), 5, 1), "ghij \n");
    }

    #[test]
    fn the_window_follows_the_cursor_when_it_is_not_at_the_end() {
        // Cursor moved home: scroll = 0 - 4 -> 0, window is the first 5 chars.
        let mut edit = TextEdit::from_value("abcdefghij");
        edit.move_home();
        assert_eq!(lines(Input::new(&edit), 5, 1), "abcde\n");

        // Cursor at index 6: scroll = 6 - 4 = 2, window is chars[2..7].
        edit.set_cursor(6);
        assert_eq!(lines(Input::new(&edit), 5, 1), "cdefg\n");
    }

    #[test]
    fn focused_draws_a_reversed_caret_at_the_cursor_column() {
        let mut edit = TextEdit::from_value("abc");
        edit.set_cursor(1); // over 'b'
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Input::new(&edit).focused(true).render(buf.area(), &mut buf);

        let caret = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(caret.symbol, 'b');
        assert!(caret.modifier.contains(Modifier::REVERSED));
        // Neighbours are not reversed.
        assert!(
            !buf.get(Position::new(0, 0))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            !buf.get(Position::new(2, 0))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn the_caret_at_end_of_text_is_the_blank_after_the_last_char() {
        let edit = TextEdit::from_value("ab"); // cursor at 2 (end)
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Input::new(&edit).focused(true).render(buf.area(), &mut buf);
        let caret = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(caret.symbol, ' ');
        assert!(caret.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn an_unfocused_input_draws_no_caret() {
        let edit = TextEdit::from_value("ab");
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Input::new(&edit).render(buf.area(), &mut buf);
        for x in 0..6 {
            assert!(
                !buf.get(Position::new(x, 0))
                    .unwrap()
                    .modifier
                    .contains(Modifier::REVERSED)
            );
        }
    }

    #[test]
    fn the_placeholder_shows_only_while_the_value_is_empty() {
        let empty = TextEdit::new();
        assert_eq!(
            lines(Input::new(&empty).placeholder("name"), 8, 1),
            "name    \n"
        );

        let typed = TextEdit::from_value("Ann");
        assert_eq!(
            lines(Input::new(&typed).placeholder("name"), 8, 1),
            "Ann     \n"
        );
    }

    #[test]
    fn a_focused_empty_input_shows_the_caret_at_column_zero() {
        let empty = TextEdit::new();
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        Input::new(&empty)
            .placeholder("hint")
            .focused(true)
            .render(buf.area(), &mut buf);
        // Placeholder is drawn, with the caret reversed over its first cell.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'h');
        assert!(
            buf.get(Position::new(0, 0))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            !buf.get(Position::new(1, 0))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn focus_style_is_a_full_width_bar() {
        let edit = TextEdit::from_value("hi");
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        Input::new(&edit)
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
    }

    #[test]
    fn unfocused_paints_no_focus_style() {
        let edit = TextEdit::from_value("hi");
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        Input::new(&edit)
            .focus_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Reset);
        }
    }

    #[test]
    fn base_style_fills_the_whole_row_including_past_the_value() {
        let edit = TextEdit::from_value("x");
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Input::new(&edit)
            .style(Style::new().bg(Color::Red))
            .render(buf.area(), &mut buf);
        for x in 0..6 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Red);
        }
    }

    #[test]
    fn the_value_is_clipped_at_the_right_edge_when_the_cursor_is_home() {
        let mut edit = TextEdit::from_value("abcdef");
        edit.move_home(); // scroll 0, so the tail is simply clipped
        assert_eq!(lines(Input::new(&edit), 4, 1), "abcd\n");
    }

    #[test]
    fn a_multibyte_value_maps_each_char_index_to_one_column() {
        // "é" and "日" are multi-byte; the cursor is a char index so it maps
        // straight to a column with no byte math leaking through.
        let mut edit = TextEdit::from_value("é日x");
        edit.set_cursor(1); // over "日"
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        Input::new(&edit).focused(true).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'é');
        let caret = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(caret.symbol, '日');
        assert!(caret.modifier.contains(Modifier::REVERSED));
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'x');
    }

    #[test]
    fn only_the_top_row_of_a_taller_area_is_touched() {
        let edit = TextEdit::from_value("Z");
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 3));
        Input::new(&edit).focused(true).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'Z');
        for y in 1..3 {
            for x in 0..5 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().symbol, ' ');
            }
        }
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let edit = TextEdit::from_value("Hi");
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        Input::new(&edit).render(Rect::new(2, 3, 6, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 3)).unwrap().symbol, 'H');
        assert_eq!(buf.get(Position::new(3, 3)).unwrap().symbol, 'i');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn a_one_cell_area_is_total() {
        // Width 1: scroll = cursor, the window is the single char at the
        // cursor (or the blank past the end); no panic.
        let mut edit = TextEdit::from_value("abc");
        edit.set_cursor(1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        Input::new(&edit).focused(true).render(buf.area(), &mut buf);
        let only = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(only.symbol, 'b');
        assert!(only.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let edit = TextEdit::from_value("hello");
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Input::new(&edit)
            .focused(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
