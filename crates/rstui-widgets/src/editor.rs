//! [`Editor`] — a multi-line text-entry panel, the multi-line sibling of
//! [`Input`](crate::Input).
//!
//! # A pure projection of a caller-owned [`TextArea`] + `focused`
//!
//! [`Input`](crate::Input) projects a caller-owned single-line
//! [`TextEdit`](rstui_core::TextEdit); `Editor` projects a caller-owned
//! **multi-line** [`TextArea`](rstui_core::TextArea) (the `rstui-core`
//! document model: a `Vec<String>` of logical lines plus a `(row, col)`
//! character-indexed cursor) plus a `focused: bool`. The widget borrows the
//! [`TextArea`] — [`Editor::new`] takes `&TextArea` — and only ever *reads*
//! [`lines`](rstui_core::TextArea::lines) and
//! [`cursor`](rstui_core::TextArea::cursor); the reducer owns the edit and
//! mutates it in `update` (insert on a `Char`, `insert_newline` on `Enter`,
//! `delete_backward` on `Backspace`, the `move_*` family on the arrows). The
//! widget never edits anything at render time, so it composes with the Elm
//! `view(&self)` model exactly like every other rstui widget.
//!
//! # Caller-owned 2D scroll — not derived
//!
//! [`Input`](crate::Input) derives its horizontal scroll as a pure function
//! of the cursor and width (no caller state). That keeps the caret on screen
//! with zero bookkeeping, but it cannot do the nicer "only scroll when the
//! caret leaves the view" UX, and it does not generalize cleanly to two axes.
//! `Editor` instead takes a caller-owned 2D [`scroll`](Editor::scroll)
//! `(row_offset, col_offset)` — the same caller-owned-offset model
//! [`List`](crate::List)/[`Table`](crate::Table) use, the option `Input`'s
//! docs name as the deferred-better one. This is the slice where it is
//! appropriate: a document needs scrolling on both axes, and the app/reducer
//! owns that state ([ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
//! §1 — scroll is plain model state the pure `view` reads, never widget- or
//! runtime-mutated). If the cursor is scrolled out of the visible window the
//! widget draws **no caret**; keeping it in view (a `scroll_into_view` seam on
//! [`TextArea`](rstui_core::TextArea)) is the caller's job and a deliberately
//! deferred additive — as are selection and undo. None are smuggled into this
//! slice (the same scoping discipline [`List`](crate::List) records).
//!
//! # The cursor is *rendered*, not the terminal's — on purpose
//!
//! A [`Widget`] is handed only a [`Buffer`] at render time, never the
//! [`Frame`](rstui_core::Frame), so it physically *cannot* call
//! `Frame::set_cursor_position`. `Editor` therefore draws its **own** caret,
//! generalizing [`Input`](crate::Input)'s to 2D: when
//! [`focused`](Editor::focused) the cell under the model cursor is stamped
//! with [`cursor_style`](Editor::cursor_style) (default
//! [`Modifier::REVERSED`](rstui_core::Modifier::REVERSED), so a focused
//! editor shows a visible block caret with zero configuration — the caret is
//! a text field's defining affordance, the one justified exception to the
//! "styles default empty" rule). This is the only TTY-free
//! snapshot-testable choice: the rendered caret shows in a
//! [`TestBackend`](rstui_core::TestBackend) frame, the terminal hardware
//! cursor does not.
//!
//! # A container: an optional framing [`Block`]
//!
//! Unlike the single-row leaf [`Input`](crate::Input), an `Editor` is a
//! panel: it takes an optional framing [`Block`](crate::Block) and renders
//! the document into the block's [`inner`](crate::Block::inner) area, exactly
//! like [`List`](crate::List)/[`Table`](crate::Table).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*):
//! an empty area, a one-cell inner, a document far larger than the panel, a
//! multi-byte line, a scroll past the end, and an empty document are all safe
//! clips/no-ops — never a panic. [`TextArea`](rstui_core::TextArea) already
//! guarantees a valid `(row, col)` cursor on a real char boundary, so the
//! cell math cannot escape the panel.

use std::borrow::Cow;

use crate::block::Block;
use rstui_core::{Buffer, Modifier, Position, Rect, Style, TextArea, Widget};

/// A multi-line text-entry panel rendered as a pure projection of a
/// caller-owned [`TextArea`](rstui_core::TextArea), a
/// [`focused`](Self::focused) `bool`, and a caller-owned 2D
/// [`scroll`](Self::scroll) offset.
///
/// The base [`style`](Self::style) fills the whole inner panel (so a
/// background reads as one block); when [`focused`](Self::focused),
/// [`focus_style`](Self::focus_style) is patched **last** over it — the same
/// highlight-wins-last fill [`List`](crate::List)/[`Input`](crate::Input) use
/// — and the cell under the cursor additionally gets
/// [`cursor_style`](Self::cursor_style). When the document is empty an
/// optional [`placeholder`](Self::placeholder) hint is shown on the first
/// row, styled with [`placeholder_style`](Self::placeholder_style).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, TextArea, Widget};
/// use rstui_widgets::Editor;
///
/// // `doc` is plain caller-owned model state the widget only reads.
/// let mut doc = TextArea::from_value("line one\nline two");
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 2));
/// Editor::new(&doc).focused(true).render(buf.area(), &mut buf);
///
/// // The document renders top-left, one logical line per row.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'l');
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'l');
///
/// // Editing happens in the reducer, on the model — never in the widget.
/// doc.move_doc_start();
/// doc.insert_char('!');
/// assert_eq!(doc.line(0), Some("!line one"));
/// ```
#[derive(Debug, Clone)]
pub struct Editor<'a> {
    model: &'a TextArea,
    focused: bool,
    scroll: (usize, usize),
    block: Option<Block<'a>>,
    style: Style,
    focus_style: Style,
    cursor_style: Style,
    placeholder: Cow<'a, str>,
    placeholder_style: Style,
}

impl<'a> Editor<'a> {
    /// An editor projecting `model`: unfocused, unscrolled, no block, no
    /// placeholder, a default reversed-cell caret and otherwise unstyled.
    #[must_use]
    pub fn new(model: &'a TextArea) -> Self {
        Self {
            model,
            focused: false,
            scroll: (0, 0),
            block: None,
            style: Style::new(),
            focus_style: Style::new(),
            // The caret is a text field's defining affordance: a focused
            // editor with an invisible cursor is broken, so unlike
            // `focus_style` this defaults to a visible reverse-video block.
            cursor_style: Style::new().add_modifier(Modifier::REVERSED),
            placeholder: Cow::Borrowed(""),
            placeholder_style: Style::new(),
        }
    }

    /// Sets whether this editor is focused — caller-owned state the widget
    /// only reads (move it in `update`, typically on `Tab`, e.g. via a
    /// `FocusRing`). When `true` the [`focus_style`](Self::focus_style) fill
    /// and the [`cursor_style`](Self::cursor_style) caret are drawn.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Sets the caller-owned 2D scroll offset `(row_offset, col_offset)`: the
    /// first document row and the first character column drawn at the inner
    /// top-left. Caller-owned state the reducer changes in `update`; the
    /// widget never derives or mutates it (see the [module docs](self)). A
    /// cursor scrolled outside the visible window draws no caret.
    #[must_use]
    pub fn scroll(mut self, scroll: (usize, usize)) -> Self {
        self.scroll = scroll;
        self
    }

    /// Frames the editor in `block`; the document renders into
    /// [`block.inner`](crate::Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]. It also fills the inner panel so a background
    /// covers it edge to edge.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] applied when [`focused`](Self::focused).
    ///
    /// Patched **last** across the inner panel, so the focus emphasis
    /// overrides the base and reads as one block — the same role
    /// [`List`](crate::List)'s `highlight_style` plays for selection.
    #[must_use]
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = style;
        self
    }

    /// Sets the [`Style`] of the caret cell when [`focused`](Self::focused)
    /// (default [`Modifier::REVERSED`](rstui_core::Modifier::REVERSED)).
    ///
    /// Patched over the base/focus fill at exactly the cursor cell.
    #[must_use]
    pub fn cursor_style(mut self, style: Style) -> Self {
        self.cursor_style = style;
        self
    }

    /// Sets the hint shown on the first row when the document is empty
    /// (default none).
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<Cow<'a, str>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets the [`Style`] of the [`placeholder`](Self::placeholder) hint,
    /// patched over the base (and the focus fill when focused).
    #[must_use]
    pub fn placeholder_style(mut self, style: Style) -> Self {
        self.placeholder_style = style;
        self
    }
}

impl Widget for Editor<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Editor {
            model,
            focused,
            scroll,
            block,
            style,
            focus_style,
            cursor_style,
            placeholder,
            placeholder_style,
        } = self;

        // The block (if any) frames the content and reserves the inner area.
        let inner = match &block {
            Some(b) => b.inner(area),
            None => area,
        };
        if let Some(b) = block {
            b.render(area, buf);
        }
        if inner.is_empty() {
            return;
        }

        // The base, with the focus emphasis patched in when focused. Filling
        // the whole inner panel makes a focused editor read as one block —
        // List's selection-fill idiom, here keyed by `focused`.
        let base = if focused {
            style.patch(focus_style)
        } else {
            style
        };
        buf.set_style(inner, base);

        let left = inner.left();
        let right = inner.right();
        let top = inner.top();
        let bottom = inner.bottom();
        let (row_off, col_off) = scroll;

        // Empty document: show the placeholder on the first inner row (never
        // scrolled — there is nothing to scroll). When focused, the caret
        // sits at the inner origin over the placeholder's first glyph (a
        // reversed blank if there is no placeholder), the same "caret
        // reverses the glyph under it" rule the document path uses.
        if model.is_empty() {
            let placeholder = placeholder.as_ref();
            let ph_style = base.patch(placeholder_style);
            let mut x = left;
            for ch in placeholder.chars() {
                if x >= right {
                    break;
                }
                buf.set_cell(Position::new(x, top), ch, ph_style);
                x = x.saturating_add(1);
            }
            if focused {
                let glyph = placeholder.chars().next().unwrap_or(' ');
                buf.set_cell(Position::new(left, top), glyph, base.patch(cursor_style));
            }
            return;
        }

        // Stamp the visible window: rows [row_off, row_off + height), each
        // clipped to columns [col_off, col_off + width). A row or column
        // past the document is simply blank (the base fill), never a panic.
        let lines = model.lines();
        for screen_row in 0..inner.height {
            let doc_row = row_off + screen_row as usize;
            let Some(line) = lines.get(doc_row) else {
                break;
            };
            let y = top.saturating_add(screen_row);
            let mut x = left;
            for ch in line.chars().skip(col_off) {
                if x >= right {
                    break;
                }
                buf.set_cell(Position::new(x, y), ch, base);
                x = x.saturating_add(1);
            }
        }

        // The caret: translate the model cursor to a screen cell. If it is
        // scrolled out of the visible window draw nothing — keeping it in
        // view is the caller's scroll_into_view job (see the module docs).
        if focused {
            let (cur_row, cur_col) = model.cursor();
            if cur_row >= row_off && cur_col >= col_off {
                let sx = left as usize + (cur_col - col_off);
                let sy = top as usize + (cur_row - row_off);
                if sx < right as usize && sy < bottom as usize {
                    let glyph = lines
                        .get(cur_row)
                        .and_then(|l| l.chars().nth(cur_col))
                        .unwrap_or(' ');
                    buf.set_cell(
                        Position::new(sx as u16, sy as u16),
                        glyph,
                        base.patch(cursor_style),
                    );
                }
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
    fn renders_visible_lines_and_fills_the_inner_panel() {
        let model = TextArea::from_value("abc\nde\nf");
        assert_eq!(lines(Editor::new(&model), 4, 4), "abc \nde  \nf   \n    \n");
    }

    #[test]
    fn row_and_col_offset_scroll_both_axes() {
        let model = TextArea::from_value("row0\nrow1\nrow2\nrow3");
        // Skip the first row and the first two columns.
        let scrolled = Editor::new(&model).scroll((1, 2));
        assert_eq!(lines(scrolled, 3, 3), "w1 \nw2 \nw3 \n");
    }

    #[test]
    fn focused_draws_a_reversed_caret_at_the_2d_cursor_cell() {
        let mut model = TextArea::from_value("abc\ndef");
        model.set_cursor(1, 1); // over 'e' on row 1
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        Editor::new(&model)
            .focused(true)
            .render(buf.area(), &mut buf);

        let caret = buf.get(Position::new(1, 1)).unwrap();
        assert_eq!(caret.symbol, 'e');
        assert!(caret.modifier.contains(Modifier::REVERSED));
        // A neighbouring cell is not reversed.
        assert!(
            !buf.get(Position::new(0, 0))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn caret_scrolled_out_of_view_draws_nothing() {
        let mut model = TextArea::from_value("abc\ndef\nghi");
        model.set_cursor(0, 0); // top-left of the document…
        // …but the view is scrolled down two rows, so it is off-screen.
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Editor::new(&model)
            .focused(true)
            .scroll((2, 0))
            .render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..4 {
                assert!(
                    !buf.get(Position::new(x, y))
                        .unwrap()
                        .modifier
                        .contains(Modifier::REVERSED)
                );
            }
        }
    }

    #[test]
    fn caret_sits_on_the_blank_past_the_end_of_a_line() {
        let model = TextArea::from_value("ab\ncd"); // cursor at (1, 2)
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        Editor::new(&model)
            .focused(true)
            .render(buf.area(), &mut buf);
        let caret = buf.get(Position::new(2, 1)).unwrap();
        assert_eq!(caret.symbol, ' ');
        assert!(caret.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn an_unfocused_editor_draws_no_caret() {
        let model = TextArea::from_value("ab\ncd");
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        Editor::new(&model).render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..5 {
                assert!(
                    !buf.get(Position::new(x, y))
                        .unwrap()
                        .modifier
                        .contains(Modifier::REVERSED)
                );
            }
        }
    }

    #[test]
    fn focus_style_fills_the_panel() {
        let model = TextArea::from_value("hi");
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Editor::new(&model)
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..4 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Blue);
            }
        }
    }

    #[test]
    fn placeholder_shows_only_when_the_model_is_empty() {
        let empty = TextArea::new();
        assert_eq!(
            lines(Editor::new(&empty).placeholder("type…"), 6, 2),
            "type… \n      \n"
        );

        let typed = TextArea::from_value("hi");
        assert_eq!(
            lines(Editor::new(&typed).placeholder("type…"), 6, 2),
            "hi    \n      \n"
        );
    }

    #[test]
    fn a_focused_empty_editor_shows_the_caret_at_the_inner_origin() {
        let empty = TextArea::new();
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        Editor::new(&empty)
            .placeholder("hint")
            .focused(true)
            .render(buf.area(), &mut buf);
        let caret = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(caret.symbol, 'h');
        assert!(caret.modifier.contains(Modifier::REVERSED));
        assert!(
            !buf.get(Position::new(1, 0))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn block_frames_the_editor_in_the_inner_area() {
        let model = TextArea::from_value("hi");
        assert_eq!(
            lines(Editor::new(&model).block(Block::bordered()), 4, 3),
            "┌──┐\n│hi│\n└──┘\n"
        );
    }

    #[test]
    fn a_one_cell_inner_is_total() {
        let mut model = TextArea::from_value("abc\ndef");
        model.set_cursor(0, 0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        Editor::new(&model)
            .focused(true)
            .render(buf.area(), &mut buf);
        let only = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(only.symbol, 'a');
        assert!(only.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn scroll_past_the_document_is_blank_not_a_panic() {
        let model = TextArea::from_value("a\nb");
        // Both offsets far past the end: every cell is the blank base fill.
        assert_eq!(
            lines(Editor::new(&model).scroll((99, 99)), 3, 2),
            "   \n   \n"
        );
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let model = TextArea::from_value("hello\nworld");
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        Editor::new(&model)
            .focused(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let model = TextArea::from_value("Hi\nYo");
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 5));
        Editor::new(&model).render(Rect::new(2, 1, 4, 2), &mut buf);
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, 'H');
        assert_eq!(buf.get(Position::new(3, 1)).unwrap().symbol, 'i');
        assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, 'Y');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn a_multibyte_line_maps_each_char_to_one_column() {
        // "é" and "日" are multi-byte; the cursor is a char index so it maps
        // straight to a column with no byte math leaking through.
        let mut model = TextArea::from_value("é日x\nynext");
        model.set_cursor(0, 1); // over "日"
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        Editor::new(&model)
            .focused(true)
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'é');
        let caret = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(caret.symbol, '日');
        assert!(caret.modifier.contains(Modifier::REVERSED));
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'x');
    }
}
