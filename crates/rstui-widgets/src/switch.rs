//! [`Switch`] — a single-line two-state toggle (`[● ] Off` / `[ ●] On`), the
//! *sliding-track* member of the interactive form-control family and a
//! deliberate visual contrast to [`Checkbox`](crate::Checkbox).
//!
//! # Two pure projections: `on` (data) and `focused` (focus)
//!
//! Structurally `Switch` is [`Checkbox`](crate::Checkbox) with a *sliding
//! track* marker and the on/off vocabulary: it is a pure projection of **two**
//! caller-owned `bool`s — [`on`](Switch::on) (the control's data: which side
//! the knob rests on) and [`focused`](Switch::focused) (whether the keyboard
//! is aimed here, drawn with a [`focus_style`](Switch::focus_style) emphasis).
//! Both are ordinary application state the reducer owns and changes in
//! `update` (flip `on` on `Space`, move `focused` on `Tab`); the widget only
//! ever reads them, so it composes with the Elm `view(&self)` model exactly
//! like every other rstui widget and never mutates anything at render time.
//!
//! # Why a switch *and* a checkbox
//!
//! A checkbox and a switch encode the same `bool` but read differently: a
//! checkbox is a *form field you submit*, a switch is a setting that *takes
//! effect immediately* (Wi-Fi, dark mode). The affordance difference is the
//! point of having both, so `Switch`'s marker is a **sliding track** —
//! `[● ]`/`[ ●]`, the knob moving side to side — not a tick in a box. The
//! marker is the same *semantic, multi-cell* affordance class
//! [`Checkbox`](crate::Checkbox)/[`Radio`](crate::Radio) use, so it follows
//! the recorded rule: a decorative scalar is a single [`char`], a semantic
//! multi-cell affordance is a [`Cow<str>`](std::borrow::Cow) symbol
//! ([`on_symbol`](Switch::on_symbol) / [`off_symbol`](Switch::off_symbol)).
//!
//! # Optional on/off state labels
//!
//! After the track an optional, state-dependent label is shown — the
//! [`on_label`](Switch::on_label) when on, the [`off_label`](Switch::off_label)
//! when off (both default empty, so a bare track is the zero-config look).
//! They are caller-built [`Line`]s; keep the two equal width to keep a column
//! of switches aligned, exactly the [`Checkbox`](crate::Checkbox) marker rule.
//!
//! # Focus *visual* here, focus *routing* deliberately not
//!
//! As [`Checkbox`](crate::Checkbox)/[`Radio`](crate::Radio) document at
//! length: this widget renders a *focused* control but does **not** decide
//! *which* control is focused (ADR 0004 — focus is caller-owned model state;
//! routing is the reducer's job, never the widget's or the runtime's).
//! `Switch` reflects a plain caller-owned `focused: bool` and forecloses no
//! routing design.
//!
//! # A leaf control: one row, no `Block`
//!
//! Like [`Checkbox`](crate::Checkbox)/[`Radio`](crate::Radio)/[`Input`](crate::Input)
//! — and unlike the container widgets — `Switch` has **no framing
//! [`Block`](crate::Block)**: it draws on exactly the top row of its area and
//! the surrounding [`Form`](crate::Form) / [`Layout`](rstui_core::Layout) owns
//! vertical placement and any pane frame.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*):
//! an empty area, an area too narrow for the marker or label, a multi-row
//! area, and an empty label are all safe clips/no-ops — never a panic.

use std::borrow::Cow;

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

/// A single-line two-state toggle rendered as a pure projection of caller-owned
/// [`on`](Self::on) and [`focused`](Self::focused) state.
///
/// Layout is `<track><label>` on one row: the track is
/// [`on_symbol`](Self::on_symbol) when `on` else
/// [`off_symbol`](Self::off_symbol) (default `"[ ●] "` / `"[● ] "`, the knob
/// sliding right when on and the trailing space separating it from the label),
/// and the label is the state-dependent [`on_label`](Self::on_label) /
/// [`off_label`](Self::off_label).
///
/// Styling cascades base → label-line → span (the same
/// [`Style::patch`](rstui_core::Style) model the text model uses); the base
/// [`style`](Self::style) also fills the control's row so a background reads as
/// one bar. When [`focused`](Self::focused),
/// [`focus_style`](Self::focus_style) is patched **last** — over the fill, the
/// track, and every label span — so the focus emphasis overrides per-span
/// colours and reads as one contiguous bar, exactly as
/// [`List`](crate::List)'s `highlight_style` does for the selected row.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Switch;
///
/// // `on` is plain caller-owned model state the widget only reads; the
/// // reducer flips it in `update` (e.g. on `Space`).
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
/// Switch::new().on(true).on_label("On").render(buf.area(), &mut buf);
///
/// // The knob slides right when on: "[ ●] On".
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, ' ');
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, '●');
/// assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, 'O');
/// ```
#[derive(Debug, Clone)]
pub struct Switch<'a> {
    on: bool,
    focused: bool,
    on_label: Line<'a>,
    off_label: Line<'a>,
    style: Style,
    focus_style: Style,
    on_symbol: Cow<'a, str>,
    off_symbol: Cow<'a, str>,
}

impl<'a> Switch<'a> {
    /// A switch that is off, unfocused, with no state labels and the default
    /// sliding-track symbols (`"[ ●] "` / `"[● ] "`).
    #[must_use]
    pub fn new() -> Self {
        Self {
            on: false,
            focused: false,
            on_label: Line::default(),
            off_label: Line::default(),
            style: Style::new(),
            focus_style: Style::new(),
            on_symbol: Cow::Borrowed("[ ●] "),
            off_symbol: Cow::Borrowed("[● ] "),
        }
    }

    /// Sets whether the switch is on — caller-owned state the widget only
    /// reads (flip it in `update`, typically on `Space`).
    #[must_use]
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
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

    /// Sets the label shown after the track when [`on`](Self::on) (default
    /// empty). Keep it the same width as [`off_label`](Self::off_label) so a
    /// column of switches stays aligned as they toggle.
    #[must_use]
    pub fn on_label(mut self, label: impl Into<Line<'a>>) -> Self {
        self.on_label = label.into();
        self
    }

    /// Sets the label shown after the track when off (default empty). Keep it
    /// the same width as [`on_label`](Self::on_label) so a column of switches
    /// stays aligned.
    #[must_use]
    pub fn off_label(mut self, label: impl Into<Line<'a>>) -> Self {
        self.off_label = label.into();
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

    /// Sets the track drawn when [`on`](Self::on) (default `"[ ●] "`, knob
    /// right). Keep both symbols the same width so the label column does not
    /// shift as the switch toggles.
    #[must_use]
    pub fn on_symbol(mut self, symbol: impl Into<Cow<'a, str>>) -> Self {
        self.on_symbol = symbol.into();
        self
    }

    /// Sets the track drawn when off (default `"[● ] "`, knob left). Keep both
    /// symbols the same width so the label column does not shift.
    #[must_use]
    pub fn off_symbol(mut self, symbol: impl Into<Cow<'a, str>>) -> Self {
        self.off_symbol = symbol.into();
        self
    }
}

impl Default for Switch<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Switch<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Switch {
            on,
            focused,
            on_label,
            off_label,
            style,
            focus_style,
            on_symbol,
            off_symbol,
        } = self;

        let y = area.top();
        let left = area.left();
        let right = area.right();

        // The base, with the focus emphasis patched in when focused. Filling
        // the whole row makes a focused control read as one contiguous bar —
        // List's selection-bar idiom, here keyed by `focused`.
        let base = if focused {
            style.patch(focus_style)
        } else {
            style
        };
        buf.set_style(Rect::new(left, y, area.width, 1), base);

        // The track is control chrome: styled by base/focus only, never the
        // label's span styles. Stamp it left to right, clipping at the edge.
        let track = if on {
            on_symbol.as_ref()
        } else {
            off_symbol.as_ref()
        };
        let mut x = left;
        for ch in track.chars() {
            if x >= right {
                break;
            }
            buf.set_cell(Position::new(x, y), ch, base);
            x = x.saturating_add(1);
        }

        // The state-dependent label cascades base → line → span; `focus_style`
        // is patched LAST per glyph when focused so the focus emphasis wins
        // over per-span colours, exactly as List patches `highlight_style`.
        let label = if on { on_label } else { off_label };
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
    fn off_and_on_slide_the_knob_across_an_equal_width_track() {
        // Default symbols are equal width, so the label column is stable.
        assert_eq!(lines(Switch::new(), 6, 1), "[● ]  \n");
        assert_eq!(lines(Switch::new().on(true), 6, 1), "[ ●]  \n");
    }

    #[test]
    fn the_state_label_follows_the_toggle() {
        let sw = Switch::new().on_label("On").off_label("Off");
        assert_eq!(lines(sw.clone(), 8, 1), "[● ] Off\n");
        assert_eq!(lines(sw.on(true), 8, 1), "[ ●] On \n");
    }

    #[test]
    fn custom_symbols_replace_the_track() {
        let sw = Switch::new().on_symbol("<=>").off_symbol("<->");
        assert_eq!(lines(sw.clone(), 3, 1), "<->\n");
        assert_eq!(lines(sw.on(true), 3, 1), "<=>\n");
    }

    #[test]
    fn the_label_is_clipped_at_the_right_edge() {
        let sw = Switch::new().on(true).on_label("Enabled");
        assert_eq!(lines(sw, 8, 1), "[ ●] Ena\n");
    }

    #[test]
    fn an_area_too_narrow_for_the_track_clips_without_panicking() {
        // Two cells: only "[●" fits, the label never starts; no panic.
        assert_eq!(lines(Switch::new().off_label("X"), 2, 1), "[●\n");
    }

    #[test]
    fn an_empty_label_renders_just_the_track() {
        assert_eq!(lines(Switch::new().on(true), 5, 1), "[ ●] \n");
    }

    #[test]
    fn focus_style_is_a_full_width_bar_over_track_label_and_padding() {
        let sw = Switch::new()
            .on(true)
            .on_label("Hi")
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 9, 1));
        sw.render(buf.area(), &mut buf);
        for x in 0..9 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '[');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, '●');
        assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, 'H');
    }

    #[test]
    fn unfocused_paints_no_focus_style() {
        let sw = Switch::new().focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        sw.render(buf.area(), &mut buf);
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Reset);
        }
    }

    #[test]
    fn base_style_fills_the_whole_row() {
        let sw = Switch::new().style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        sw.render(buf.area(), &mut buf);
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
        let sw = Switch::new()
            .on(true)
            .on_label(label)
            .on_symbol("")
            .style(Style::new().fg(Color::Green))
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        sw.render(buf.area(), &mut buf);

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
        Switch::new().on(true).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '[');
        for cell_y in 1..3 {
            for x in 0..5 {
                assert_eq!(buf.get(Position::new(x, cell_y)).unwrap().symbol, ' ');
            }
        }
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        Switch::new().render(Rect::new(2, 3, 6, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 3)).unwrap().symbol, '[');
        assert_eq!(buf.get(Position::new(3, 3)).unwrap().symbol, '●');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Switch::new()
            .on(true)
            .focused(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
