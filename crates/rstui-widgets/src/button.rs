//! [`Button`] — a single-line, centred, focusable *action* label
//! (`   Save   `), the second of the interactive *form-control* family
//! (checkbox/radio/button/input) and the first widget that is a pure
//! projection of **only** a focus visual, with **no data state at all**.
//!
//! # The first control with no data — only `focused`
//!
//! [`Checkbox`](crate::Checkbox) is a pure projection of *two* caller-owned
//! `bool`s: `checked` (its *data*) and `focused` (its *focus* visual). A
//! button has **no data**: it does not persist anything, it *triggers* a
//! reducer message when activated. So `Button` is a pure projection of a
//! *single* caller-owned `bool` — [`focused`](Button::focused) — drawn with a
//! [`focus_style`](Button::focus_style) emphasis. What happens when the button
//! is pressed (`Enter`/`Space` while focused, or a click) is entirely the
//! reducer's concern in `update`: the widget renders the *affordance*, the app
//! decides the *action*. This keeps `Button` architecturally neutral exactly
//! like every other rstui widget — it never mutates anything at render time
//! and composes with the Elm `view(&self)` model unchanged.
//!
//! # Focus *visual* here, focus *routing* deliberately not
//!
//! As [`Checkbox`](crate::Checkbox) documents at length: this widget renders a
//! *focused* control but does **not** decide *which* control is focused. A
//! focus manager (a registry of focusable widgets, `Tab`/`Shift+Tab`/arrow
//! traversal, click-to-focus) is a genuinely new architectural axis that is
//! expensive to reverse, so it belongs in its own decision record and is
//! **not** smuggled into a widget slice. `Button` reflects a plain
//! caller-owned `focused: bool` and forecloses no future routing design.
//!
//! # Centred by default; the label's own alignment wins
//!
//! Unlike the left-aligned [`Checkbox`](crate::Checkbox) (whose label simply
//! follows the marker), a button conventionally **centres** its label, so
//! [`Button`]'s default [`alignment`](Button::alignment) is
//! [`Alignment::Center`]. If the label [`Line`] sets its *own* alignment it
//! wins over the button default — the exact line-wins-over-container rule a
//! [`Block`](crate::Block) title already uses — so a caller can left- or
//! right-justify a single button without disturbing the others. This needs no
//! new machinery: it reuses the [`Alignment`] placement primitive and the
//! existing [`Line`] alignment field.
//!
//! # A leaf control: one row, no `Block`
//!
//! Like [`Checkbox`](crate::Checkbox)/[`Scrollbar`](crate::Scrollbar)/[`Spinner`](crate::Spinner)
//! — and unlike the container widgets — `Button` has **no framing
//! [`Block`](crate::Block)**: it is a single-line *leaf* control drawn on
//! exactly the top row of the area it is given. The surrounding form /
//! [`Layout`](rstui_core::Layout) owns vertical placement and any pane frame;
//! a caller who wants a bordered button frames it with a [`Block`](crate::Block)
//! via [`Layout`](rstui_core::Layout).
//!
//! No mandatory decoration (no `[ ]`-style brackets) is *deliberate*: the
//! [`focus_style`](Button::focus_style) bar is itself the affordance — a
//! focused button reads as one contiguous highlighted bar, exactly as a
//! selected [`List`](crate::List) row does — and a caller who wants literal
//! brackets just includes them in the label (`"< OK >"`). Momentary
//! press feedback (a `pressed`/`press_style` pair) is a clean future
//! *additive* enhancement that does not change this shape.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*):
//! an empty area, an area too narrow for the label, a multi-row area, and an
//! empty label are all safe clips/no-ops — never a panic.

use rstui_core::{Alignment, Buffer, Line, Position, Rect, Style, Widget};

/// A single-line focusable *action* label rendered as a pure projection of
/// caller-owned [`focused`](Self::focused) state.
///
/// The label is placed on one row at the [`alignment`](Self::alignment)
/// (default [`Alignment::Center`]); the label [`Line`]'s own alignment, if
/// set, wins. Styling cascades base → label-line → span (the same
/// [`Style::patch`] model the text model uses); the base [`style`](Self::style)
/// also fills the control's row so a background reads as one bar. When
/// [`focused`](Self::focused), [`focus_style`](Self::focus_style) is patched
/// **last** — over the fill and every label span — so the focus emphasis
/// overrides per-span colours and reads as one contiguous bar, exactly as
/// [`List`](crate::List)'s `highlight_style` does for the selected row.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Button;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
/// // `focused` is plain caller-owned state the widget only reads.
/// Button::new("Save").render(buf.area(), &mut buf);
///
/// // Centred by default in the 8-wide area: "  Save  "
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'S');
/// assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, 'e');
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
/// ```
#[derive(Debug, Clone)]
pub struct Button<'a> {
    label: Line<'a>,
    focused: bool,
    style: Style,
    focus_style: Style,
    alignment: Alignment,
}

impl<'a> Button<'a> {
    /// A button showing `label`: unfocused, centred, default styles.
    pub fn new(label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            focused: false,
            style: Style::new(),
            focus_style: Style::new(),
            alignment: Alignment::Center,
        }
    }

    /// Sets whether this control is focused — caller-owned state the widget
    /// only reads (move it in `update`, typically on `Tab`). When `true` the
    /// [`focus_style`](Self::focus_style) emphasis is applied.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Sets the base [`Style`], beneath the base → label → span cascade. It
    /// also fills the control's row so a background covers it edge to edge.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] applied when [`focused`](Self::focused).
    ///
    /// Patched **last** in the cascade and across the full row, so the focus
    /// emphasis overrides per-span colours and reads as one bar — the same
    /// role [`List`](crate::List)'s `highlight_style` plays for selection.
    #[must_use]
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = style;
        self
    }

    /// Sets the label [`Alignment`] within the button (default
    /// [`Alignment::Center`]).
    ///
    /// This is the button-level default; if the label [`Line`] sets its *own*
    /// alignment, the line wins (the same line-wins-over-container rule a
    /// [`Block`](crate::Block) title uses).
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }
}

impl Widget for Button<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Button {
            label,
            focused,
            style,
            focus_style,
            alignment,
        } = self;

        let y = area.top();
        let left = area.left();
        let right = area.right();

        // The base, with the focus emphasis patched in when focused. Filling
        // the whole row makes a focused button read as one contiguous bar —
        // List's selection-bar idiom, here keyed by `focused`.
        let base = if focused {
            style.patch(focus_style)
        } else {
            style
        };
        buf.set_style(Rect::new(left, y, area.width, 1), base);

        // The label's own alignment wins over the button default (the same
        // line-wins-over-container rule a Block title uses); place the clipped
        // label width at the resolved start column. `min` is done in `usize`
        // space then narrowed, so the cast is lossless (result <= width).
        let align = label.alignment.unwrap_or(alignment);
        let avail = area.width as usize;
        let drawn = label.width().min(avail) as u16;
        let start = match align {
            Alignment::Left => left,
            Alignment::Right => right.saturating_sub(drawn),
            Alignment::Center => left.saturating_add(area.width.saturating_sub(drawn) / 2),
        };

        // The label cascades base → line → span; `focus_style` is patched
        // LAST per glyph when focused so the focus emphasis wins over per-span
        // colours, exactly as List patches `highlight_style` last.
        let line_base = style.patch(label.style);
        let mut x = start;
        'label: for span in label.spans {
            let mut span_style = line_base.patch(span.style);
            if focused {
                span_style = span_style.patch(focus_style);
            }
            for ch in span.content.chars() {
                if x >= right {
                    break 'label;
                }
                buf.set_cell(Position::new(x, y), ch, span_style);
                x = x.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier, Span};

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
    fn the_label_is_centred_by_default() {
        // "Save" is 4 wide in an 8-wide button: 2 cells of pad each side.
        assert_eq!(lines(Button::new("Save"), 8, 1), "  Save  \n");
    }

    #[test]
    fn an_odd_remainder_biases_toward_the_start() {
        // 7 wide, "Hi" is 2: 5 spare → 2 left, 3 right (matches Line/title).
        assert_eq!(lines(Button::new("Hi"), 7, 1), "  Hi   \n");
    }

    #[test]
    fn left_alignment_pins_the_label_to_the_left_edge() {
        assert_eq!(
            lines(Button::new("Go").alignment(Alignment::Left), 6, 1),
            "Go    \n"
        );
    }

    #[test]
    fn right_alignment_pins_the_label_to_the_right_edge() {
        assert_eq!(
            lines(Button::new("Go").alignment(Alignment::Right), 6, 1),
            "    Go\n"
        );
    }

    #[test]
    fn the_label_lines_own_alignment_wins_over_the_button_default() {
        // Button default is Center, but the Line explicitly asks for Left.
        let label = Line::from("Go").alignment(Alignment::Left);
        assert_eq!(lines(Button::new(label), 6, 1), "Go    \n");
    }

    #[test]
    fn a_label_wider_than_the_area_is_clipped_from_the_left() {
        // drawn == avail, so every alignment collapses to the left edge.
        assert_eq!(lines(Button::new("Submit"), 4, 1), "Subm\n");
    }

    #[test]
    fn an_empty_label_renders_just_the_styled_bar() {
        assert_eq!(lines(Button::new(""), 5, 1), "     \n");
    }

    #[test]
    fn focus_style_is_a_full_width_bar_over_label_and_padding() {
        let btn = Button::new("Hi")
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        btn.render(buf.area(), &mut buf);
        // The label and the centring padding all share the focus background —
        // one contiguous bar.
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
        // Centred: "Hi" at columns 3..5 in the 8-wide bar.
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, 'H');
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, 'i');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn unfocused_paints_no_focus_style() {
        let btn = Button::new("Hi").focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        btn.render(buf.area(), &mut buf);
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Reset);
        }
    }

    #[test]
    fn base_style_fills_the_whole_row() {
        let btn = Button::new("x").style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        btn.render(buf.area(), &mut buf);
        // Including the centring padding around the short label.
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Red);
        }
    }

    #[test]
    fn style_cascades_base_label_span_and_focus_wins_last() {
        // Label line is BOLD; one span is red. Base is green. Focused, so the
        // focus bg is patched last over everything. Left-aligned and width 2
        // so the two glyphs land at columns 0 and 1.
        let label = Line::from(vec![
            Span::styled("X", Style::new().fg(Color::Red)),
            Span::raw("y"),
        ])
        .style(Style::new().add_modifier(Modifier::BOLD))
        .alignment(Alignment::Left);
        let btn = Button::new(label)
            .style(Style::new().fg(Color::Green))
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        btn.render(buf.area(), &mut buf);

        let x = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(x.symbol, 'X');
        assert_eq!(x.fg, Color::Red); // span fg survives
        assert_eq!(x.bg, Color::Blue); // focus patched last
        assert!(x.modifier.contains(Modifier::BOLD)); // line modifier cascades

        let y = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(y.symbol, 'y');
        assert_eq!(y.fg, Color::Green); // inherits base (no span fg)
        assert_eq!(y.bg, Color::Blue);
        assert!(y.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn only_the_top_row_of_a_taller_area_is_touched() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 3));
        Button::new("Z").render(buf.area(), &mut buf);
        // Centred on the top row only.
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'Z');
        for y in 1..3 {
            for x in 0..5 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().symbol, ' ');
            }
        }
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        // A Layout-placed button draws where it was placed, centred there.
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        Button::new("Go").render(Rect::new(2, 3, 6, 1), &mut buf);
        // 6-wide region at x=2: "Go" centred → spare 4, 2 left → columns 4,5.
        assert_eq!(buf.get(Position::new(4, 3)).unwrap().symbol, 'G');
        assert_eq!(buf.get(Position::new(5, 3)).unwrap().symbol, 'o');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Button::new("hello")
            .focused(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
