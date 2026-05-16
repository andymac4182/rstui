//! [`Checkbox`] — a single-line labelled boolean control (`[x] Enable
//! logging`), the first of the interactive *form-control* family
//! (checkbox/radio/button/input) and the first widget to model a **focus**
//! visual.
//!
//! # Two pure projections: `checked` (data) and `focused` (focus)
//!
//! [`List`](crate::List) is a pure projection of a caller-owned *selection*,
//! [`Gauge`](crate::Gauge) of a caller-owned *scalar*,
//! [`Spinner`](crate::Spinner) of a caller-owned *tick*. `Checkbox` is a pure
//! projection of **two** caller-owned `bool`s:
//!
//! - [`checked`](Checkbox::checked) — the control's *data* (the analogue of
//!   [`List`](crate::List)'s `selected`): which marker the box shows.
//! - [`focused`](Checkbox::focused) — whether this is the control the keyboard
//!   is currently aimed at, drawn with a [`focus_style`](Checkbox::focus_style)
//!   emphasis.
//!
//! Both are ordinary application state the reducer owns and changes in
//! `update` (toggle `checked` on `Space`; move `focused` on `Tab`); the widget
//! only ever reads them, so it composes with the Elm `view(&self)` model
//! exactly like every other rstui widget. The widget never mutates anything at
//! render time.
//!
//! # Focus *visual* here, focus *routing* deliberately not
//!
//! This widget renders a focused control; it does **not** decide *which*
//! control is focused. A focus manager — a registry of focusable widgets, how
//! `Tab`/`Shift+Tab`/arrows traverse them, click-to-focus, how that composes
//! with the pure-projection rule — is a genuinely new architectural axis that
//! is expensive to reverse, so (exactly as [`List`](crate::List) did for the
//! stateful-widget question) it belongs in its own decision record and is
//! **not** smuggled into a widget slice. `Checkbox` stays architecturally
//! neutral: it reflects a plain caller-owned `focused: bool` and forecloses no
//! future routing design — wherever that bool comes from, the widget is
//! unchanged.
//!
//! # The marker is a multi-cell affordance, on purpose
//!
//! Borders/`Gauge`/`Spinner`/`Scrollbar` banked the single-[`char`]
//! [`Cell`](rstui_core::Buffer) dividend because each glyph is one decorative
//! scalar. A checkbox marker is different: the bracketed box `[x]`/`[ ]` is a
//! *semantic, multi-cell* affordance (the most portable, ASCII, terminal
//! convention), so the marker is a [`Cow<str>`](std::borrow::Cow) —
//! [`checked_symbol`](Checkbox::checked_symbol) /
//! [`unchecked_symbol`](Checkbox::unchecked_symbol), mirroring
//! [`List`](crate::List)'s string `highlight_symbol` gutter rather than the
//! single-`char` widgets. It also makes the obvious future reuse trivial: a
//! radio button is this widget with `( )`/`(•)` symbols.
//!
//! # A leaf control: one row, no `Block`
//!
//! Like [`Scrollbar`](crate::Scrollbar)/[`Spinner`](crate::Spinner) — and
//! unlike the container widgets — `Checkbox` has **no framing
//! [`Block`](crate::Block)**: it is a single-line *leaf* control, so it draws
//! on exactly the top row of the area it is given and the surrounding form /
//! [`Layout`](rstui_core::Layout) owns vertical placement, grouping, and any
//! pane frame.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*):
//! an empty area, an area too narrow for the marker or label, a multi-row
//! area, and an empty label are all safe clips/no-ops — never a panic.

use std::borrow::Cow;

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

/// A single-line labelled boolean control rendered as a pure projection of
/// caller-owned [`checked`](Self::checked) and [`focused`](Self::focused)
/// state.
///
/// Layout is `<marker><label>` on one row: the marker is
/// [`checked_symbol`](Self::checked_symbol) when `checked` else
/// [`unchecked_symbol`](Self::unchecked_symbol) (default `"[x] "` / `"[ ] "`,
/// the trailing space separating it from the label).
///
/// Styling cascades base → label-line → span (the same
/// [`Style::patch`] model the text model uses); the base
/// [`style`](Self::style) also fills the control's row so a background reads as
/// one bar. When [`focused`](Self::focused),
/// [`focus_style`](Self::focus_style) is patched **last** — over the fill, the
/// marker, and every label span — so the focus emphasis overrides per-span
/// colours and reads as one contiguous bar, exactly as
/// [`List`](crate::List)'s `highlight_style` does for the selected row.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Checkbox;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
/// // `checked` is plain caller-owned state the widget only reads.
/// Checkbox::new("Logging").checked(true).render(buf.area(), &mut buf);
///
/// // "[x] Logging"
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '[');
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'x');
/// assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, 'L');
///
/// // Unchecked only changes the marker glyph, never the column the label
/// // starts at (both symbols are the same width by default).
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
/// Checkbox::new("Logging").render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, ' ');
/// assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, 'L');
/// ```
#[derive(Debug, Clone)]
pub struct Checkbox<'a> {
    label: Line<'a>,
    checked: bool,
    focused: bool,
    style: Style,
    focus_style: Style,
    checked_symbol: Cow<'a, str>,
    unchecked_symbol: Cow<'a, str>,
}

impl<'a> Checkbox<'a> {
    /// A checkbox showing `label`: unchecked, unfocused, default symbols
    /// (`"[x] "` / `"[ ] "`) and styles.
    pub fn new(label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            checked: false,
            focused: false,
            style: Style::new(),
            focus_style: Style::new(),
            checked_symbol: Cow::Borrowed("[x] "),
            unchecked_symbol: Cow::Borrowed("[ ] "),
        }
    }

    /// Sets whether the box is checked — caller-owned state the widget only
    /// reads (toggle it in `update`, typically on `Space`).
    #[must_use]
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
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

    /// Sets the marker drawn when [`checked`](Self::checked) (default
    /// `"[x] "`). Keep both symbols the same width so the label column does
    /// not shift as the box toggles.
    #[must_use]
    pub fn checked_symbol(mut self, symbol: impl Into<Cow<'a, str>>) -> Self {
        self.checked_symbol = symbol.into();
        self
    }

    /// Sets the marker drawn when not checked (default `"[ ] "`). Keep both
    /// symbols the same width so the label column does not shift.
    #[must_use]
    pub fn unchecked_symbol(mut self, symbol: impl Into<Cow<'a, str>>) -> Self {
        self.unchecked_symbol = symbol.into();
        self
    }
}

impl Widget for Checkbox<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Checkbox {
            label,
            checked,
            focused,
            style,
            focus_style,
            checked_symbol,
            unchecked_symbol,
        } = self;

        let y = area.top();
        let left = area.left();
        let right = area.right();

        // The base, with the focus emphasis patched in when focused. Filling
        // the whole row makes a focused control read as one contiguous bar —
        // List's selection-bar idiom, here keyed by `focused` instead of
        // `selected`.
        let base = if focused {
            style.patch(focus_style)
        } else {
            style
        };
        buf.set_style(Rect::new(left, y, area.width, 1), base);

        // The marker is control chrome: styled by base/focus only, never the
        // label's span styles. Stamp it left to right, clipping at the edge.
        let marker = if checked {
            checked_symbol.as_ref()
        } else {
            unchecked_symbol.as_ref()
        };
        let mut x = left;
        for ch in marker.chars() {
            if x >= right {
                break;
            }
            buf.set_cell(Position::new(x, y), ch, base);
            x = x.saturating_add(1);
        }

        // The label cascades base → line → span; `focus_style` is patched
        // LAST per glyph when focused so the focus emphasis wins over per-span
        // colours, exactly as List patches `highlight_style` last.
        let line_base = style.patch(label.style);
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
    fn unchecked_and_checked_differ_only_by_the_marker_glyph() {
        // Default symbols are equal width, so the label column is stable.
        assert_eq!(lines(Checkbox::new("Log"), 8, 1), "[ ] Log \n");
        assert_eq!(
            lines(Checkbox::new("Log").checked(true), 8, 1),
            "[x] Log \n"
        );
    }

    #[test]
    fn custom_symbols_replace_the_marker() {
        let cb = Checkbox::new("On")
            .checked_symbol("☑ ")
            .unchecked_symbol("☐ ");
        assert_eq!(lines(cb.clone(), 5, 1), "☐ On \n");
        assert_eq!(lines(cb.checked(true), 5, 1), "☑ On \n");
    }

    #[test]
    fn the_label_is_clipped_at_the_right_edge() {
        assert_eq!(lines(Checkbox::new("Logging"), 6, 1), "[ ] Lo\n");
    }

    #[test]
    fn an_area_too_narrow_for_the_marker_clips_without_panicking() {
        // Two cells: only "[ " fits, the label never starts; no panic.
        assert_eq!(lines(Checkbox::new("X"), 2, 1), "[ \n");
    }

    #[test]
    fn an_empty_label_renders_just_the_marker() {
        assert_eq!(lines(Checkbox::new("").checked(true), 5, 1), "[x]  \n");
    }

    #[test]
    fn focus_style_is_a_full_width_bar_over_marker_label_and_padding() {
        let cb = Checkbox::new("Hi")
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        cb.render(buf.area(), &mut buf);
        // Marker, label, and the empty cells after it all share the focus
        // background — one contiguous bar.
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '[');
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, 'H');
        assert_eq!(buf.get(Position::new(7, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn unfocused_paints_no_focus_style() {
        let cb = Checkbox::new("Hi").focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        cb.render(buf.area(), &mut buf);
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Reset);
        }
    }

    #[test]
    fn base_style_fills_the_whole_row() {
        let cb = Checkbox::new("x").style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        cb.render(buf.area(), &mut buf);
        // Including the cells past the end of the short label.
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Red);
        }
    }

    #[test]
    fn style_cascades_base_label_span_and_focus_wins_last() {
        // Label line is BOLD; one span is red. Base is green. Focused, so the
        // focus bg is patched last over everything.
        let label = Line::from(vec![
            Span::styled("X", Style::new().fg(Color::Red)),
            Span::raw("y"),
        ])
        .style(Style::new().add_modifier(Modifier::BOLD));
        let cb = Checkbox::new(label)
            .unchecked_symbol("")
            .style(Style::new().fg(Color::Green))
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        cb.render(buf.area(), &mut buf);

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
        Checkbox::new("Z")
            .checked(true)
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '[');
        for y in 1..3 {
            for x in 0..5 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().symbol, ' ');
            }
        }
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        // A Layout-placed checkbox draws where it was placed.
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        Checkbox::new("A").render(Rect::new(2, 3, 6, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 3)).unwrap().symbol, '[');
        assert_eq!(buf.get(Position::new(6, 3)).unwrap().symbol, 'A');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Checkbox::new("hello")
            .checked(true)
            .focused(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
