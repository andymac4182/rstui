//! [`MaskedInput`] — a single-line text-entry field that renders each
//! character as a **mask glyph** (a password field), with a caller-owned
//! reveal toggle.
//!
//! # The [`Input`](crate::Input) projection, masked
//!
//! [`Input`](crate::Input) is a pure projection of a caller-owned
//! [`TextEdit`] + `focused`. `MaskedInput` is the *same*
//! pure projection of the *same* borrowed [`TextEdit`] —
//! it only reads [`value`](rstui_core::TextEdit::value) /
//! [`cursor`](rstui_core::TextEdit::cursor), the reducer still owns the edit —
//! with one change: every value character is drawn as
//! [`mask`](MaskedInput::mask) (default `•`) instead of itself, unless the
//! caller-owned [`unmasked`](MaskedInput::unmasked) flag is set (the "show
//! password" eye). It deliberately does **not** modify or wrap
//! [`Input`](crate::Input): a password field's render rule differs at every
//! glyph, so it is its own focused projection (no shared mutable state, the
//! pure-`view` discipline).
//!
//! The [`placeholder`](MaskedInput::placeholder) hint shown while the value is
//! empty is **never** masked — a hint is not a secret — exactly as
//! [`Input`](crate::Input) renders it.
//!
//! # A leaf control with a rendered caret, total like [`Input`](crate::Input)
//!
//! Like [`Input`](crate::Input) it is one row with no [`Block`](crate::Block);
//! it draws its **own** reversed caret (a [`Widget`] is handed only a
//! [`Buffer`], never the [`Frame`](rstui_core::Frame)); and it derives a
//! stateless caret-following horizontal scroll as a pure function of
//! `cursor`/width. Per the [`Gauge`](crate::Gauge) totality rule an empty
//! area, a one-cell area, a value far wider than the field, a multi-byte
//! value, a multi-row area, and an empty value are all safe clips/no-ops —
//! never a panic.

use std::borrow::Cow;

use rstui_core::{Buffer, Modifier, Position, Rect, Style, TextEdit, Widget};

/// A single-line **masked** text-entry field, a pure projection of a
/// caller-owned [`TextEdit`] + `focused` + `unmasked`.
///
/// Layout matches [`Input`](crate::Input) exactly — the value on one row,
/// horizontally scrolled so the caret stays visible, the base
/// [`style`](Self::style) filling the row, [`focus_style`](Self::focus_style)
/// patched last when [`focused`](Self::focused), the caret cell additionally
/// taking [`cursor_style`](Self::cursor_style) — except each value glyph is the
/// [`mask`](Self::mask) unless [`unmasked`](Self::unmasked).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, TextEdit, Widget};
/// use rstui_widgets::MaskedInput;
///
/// // `edit` is plain caller-owned model state the widget only reads.
/// let edit = TextEdit::from_value("secret");
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
/// MaskedInput::new(&edit).render(buf.area(), &mut buf);
///
/// // The value renders as bullets, not the characters.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '•');
/// assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, '•');
///
/// // The reveal toggle (a "show password" eye the reducer owns) unmasks it.
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
/// MaskedInput::new(&edit).unmasked(true).render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 's');
/// ```
#[derive(Debug, Clone)]
pub struct MaskedInput<'a> {
    edit: &'a TextEdit,
    focused: bool,
    unmasked: bool,
    mask: char,
    placeholder: Cow<'a, str>,
    style: Style,
    focus_style: Style,
    cursor_style: Style,
    placeholder_style: Style,
}

impl<'a> MaskedInput<'a> {
    /// A masked input projecting `edit`: unfocused, masked with `•`, no
    /// placeholder, a default reversed-cell caret and otherwise unstyled.
    #[must_use]
    pub fn new(edit: &'a TextEdit) -> Self {
        Self {
            edit,
            focused: false,
            unmasked: false,
            mask: '•',
            placeholder: Cow::Borrowed(""),
            style: Style::new(),
            focus_style: Style::new(),
            // The caret is a text field's defining affordance (the one
            // justified exception to "styles default empty"), like `Input`.
            cursor_style: Style::new().add_modifier(Modifier::REVERSED),
            placeholder_style: Style::new(),
        }
    }

    /// Sets whether this field is focused — caller-owned state the widget only
    /// reads. When `true` the [`focus_style`](Self::focus_style) bar and the
    /// [`cursor_style`](Self::cursor_style) caret are drawn.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Sets whether the value is shown in the clear — caller-owned state the
    /// widget only reads (the "show password" toggle, moved in `update`).
    #[must_use]
    pub fn unmasked(mut self, unmasked: bool) -> Self {
        self.unmasked = unmasked;
        self
    }

    /// Sets the glyph each value character is masked with (default `•`).
    #[must_use]
    pub fn mask(mut self, mask: char) -> Self {
        self.mask = mask;
        self
    }

    /// Sets the hint shown when the value is empty (never masked — a hint is
    /// not a secret).
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

    /// Sets the [`Style`] applied when [`focused`](Self::focused), patched
    /// **last** across the full row so it reads as one bar.
    #[must_use]
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = style;
        self
    }

    /// Sets the [`Style`] of the caret cell when [`focused`](Self::focused)
    /// (default [`Modifier::REVERSED`](rstui_core::Modifier::REVERSED)).
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

impl Widget for MaskedInput<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let MaskedInput {
            edit,
            focused,
            unmasked,
            mask,
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

        // The base, with the focus emphasis patched in when focused, filling
        // the whole row so a focused field reads as one bar (Input's idiom).
        let base = if focused {
            style.patch(focus_style)
        } else {
            style
        };
        buf.set_style(Rect::new(left, y, area.width, 1), base);

        let value = edit.value();

        // Empty value: the placeholder hint (never scrolled, never masked).
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
        // caller-owned cursor and the field width (Input's derived scroll).
        let cursor = edit.cursor();
        let scroll = cursor.saturating_sub(width.saturating_sub(1));

        // Stamp the visible window, each glyph the mask unless unmasked, and
        // remember the caret column (the glyph at `cursor`, or the blank just
        // past the end when appending).
        let mut x = left;
        let mut caret_x = None;
        for (i, ch) in value.chars().enumerate().skip(scroll) {
            if x >= right {
                break;
            }
            if i == cursor {
                caret_x = Some(x);
            }
            let glyph = if unmasked { ch } else { mask };
            buf.set_cell(Position::new(x, y), glyph, base);
            x = x.saturating_add(1);
        }
        if caret_x.is_none() && x < right {
            caret_x = Some(x);
        }

        if focused {
            if let Some(cx) = caret_x {
                let under = value.chars().nth(cursor);
                let glyph = match under {
                    Some(ch) if unmasked => ch,
                    Some(_) => mask,
                    None => ' ',
                };
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
    fn each_value_char_is_masked_with_a_bullet() {
        let edit = TextEdit::from_value("hunter2");
        assert_eq!(lines(MaskedInput::new(&edit), 9, 1), "•••••••  \n");
    }

    #[test]
    fn unmasked_shows_the_real_characters() {
        let edit = TextEdit::from_value("abc");
        assert_eq!(
            lines(MaskedInput::new(&edit).unmasked(true), 6, 1),
            "abc   \n"
        );
    }

    #[test]
    fn a_custom_mask_glyph_is_used() {
        let edit = TextEdit::from_value("pw");
        assert_eq!(lines(MaskedInput::new(&edit).mask('*'), 4, 1), "**  \n");
    }

    #[test]
    fn the_placeholder_is_shown_unmasked_while_the_value_is_empty() {
        let empty = TextEdit::new();
        assert_eq!(
            lines(MaskedInput::new(&empty).placeholder("password"), 10, 1),
            "password  \n"
        );
    }

    #[test]
    fn focused_draws_a_reversed_caret_over_the_masked_glyph() {
        let mut edit = TextEdit::from_value("abc");
        edit.set_cursor(1); // over 'b' → masked '•'
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        MaskedInput::new(&edit)
            .focused(true)
            .render(buf.area(), &mut buf);
        let caret = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(caret.symbol, '•');
        assert!(caret.modifier.contains(Modifier::REVERSED));
        assert!(
            !buf.get(Position::new(0, 0))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn an_unmasked_focused_caret_shows_the_real_glyph() {
        let mut edit = TextEdit::from_value("abc");
        edit.set_cursor(1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        MaskedInput::new(&edit)
            .focused(true)
            .unmasked(true)
            .render(buf.area(), &mut buf);
        let caret = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(caret.symbol, 'b');
        assert!(caret.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn an_unfocused_field_draws_no_caret() {
        let edit = TextEdit::from_value("ab");
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        MaskedInput::new(&edit).render(buf.area(), &mut buf);
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
    fn a_long_value_scrolls_to_keep_the_end_caret_visible() {
        // 10 chars, width 5, cursor at end (10): scroll = 6, window 4 masked
        // glyphs + the caret blank.
        let edit = TextEdit::from_value("abcdefghij");
        assert_eq!(lines(MaskedInput::new(&edit), 5, 1), "•••• \n");
    }

    #[test]
    fn focus_style_is_a_full_width_bar() {
        let edit = TextEdit::from_value("hi");
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        MaskedInput::new(&edit)
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
    }

    #[test]
    fn base_style_fills_the_whole_row() {
        let edit = TextEdit::from_value("x");
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        MaskedInput::new(&edit)
            .style(Style::new().bg(Color::Red))
            .render(buf.area(), &mut buf);
        for x in 0..6 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Red);
        }
    }

    #[test]
    fn the_placeholder_style_is_applied() {
        let empty = TextEdit::new();
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        MaskedInput::new(&empty)
            .placeholder("pw")
            .placeholder_style(Style::new().fg(Color::DarkGray))
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::DarkGray);
    }

    #[test]
    fn a_multibyte_value_maps_each_char_to_one_masked_column() {
        let edit = TextEdit::from_value("é日x");
        assert_eq!(lines(MaskedInput::new(&edit), 5, 1), "•••  \n");
    }

    #[test]
    fn a_one_cell_area_is_total() {
        let mut edit = TextEdit::from_value("abc");
        edit.set_cursor(1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        MaskedInput::new(&edit)
            .focused(true)
            .render(buf.area(), &mut buf);
        let only = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(only.symbol, '•');
        assert!(only.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn render_uses_the_area_origin_and_only_the_top_row() {
        let edit = TextEdit::from_value("Hi");
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        MaskedInput::new(&edit).render(Rect::new(2, 3, 6, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 3)).unwrap().symbol, '•');
        assert_eq!(buf.get(Position::new(3, 3)).unwrap().symbol, '•');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let edit = TextEdit::from_value("hello");
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        MaskedInput::new(&edit)
            .focused(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
