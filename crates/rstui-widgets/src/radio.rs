//! [`Radio`] — a single-line labelled *exclusive-choice* control
//! (`(•) High`), the third of the interactive *form-control* family
//! (checkbox/radio/button/input) and the **exclusive-selection sibling** of
//! [`Checkbox`](crate::Checkbox).
//!
//! # Two pure projections: `selected` (data) and `focused` (focus)
//!
//! Structurally `Radio` is [`Checkbox`](crate::Checkbox) with a round marker
//! and the *selection* vocabulary: it is a pure projection of **two**
//! caller-owned `bool`s:
//!
//! - [`selected`](Radio::selected) — the control's *data*: whether this is the
//!   chosen option in its group. This is the [`List`](crate::List) *selection*
//!   concept (which one is chosen) decomposed across N single-line controls,
//!   so it uses `selected`, not `Checkbox`'s `checked` — a checkbox toggles an
//!   independent flag, a radio names the one choice in a set.
//! - [`focused`](Radio::focused) — whether this is the control the keyboard is
//!   currently aimed at, drawn with a [`focus_style`](Radio::focus_style)
//!   emphasis. This is the *distinct* focus sub-vocabulary every form control
//!   shares ([`Checkbox`](crate::Checkbox)/[`Button`](crate::Button)), not the
//!   selection concept.
//!
//! Both are ordinary application state the reducer owns and changes in
//! `update`; the widget only ever reads them, so it composes with the Elm
//! `view(&self)` model exactly like every other rstui widget and never mutates
//! anything at render time.
//!
//! # Exclusivity is the caller's invariant, not the widget's
//!
//! A `Radio` is **one option**. The defining radio-button rule — *exactly one
//! selected per group* — is not enforced by the widget; it is a plain
//! invariant of the caller's model (hold one `chosen: usize`, render each
//! option as `Radio::new(label).selected(i == chosen)`). The widget stays a
//! pure projection: the reducer owns the exclusivity, the widget reflects it.
//!
//! This is the upstream-validated shape, not an rstui shortcut.
//! gpui-component splits a single `Radio` from an optional `RadioGroup` and its
//! `Radio` doc states verbatim that the group *"is not included … you can
//! manage the group by yourself"*; its `RadioGroup` is itself only a
//! projection — it sets each child's checked from `selected_index == Some(ix)`,
//! exactly the per-option `bool` this widget already accepts. So the single
//! pure-projection `Radio` is the right primitive and a convenience
//! `RadioGroup` (owning one index + vertical/horizontal layout) is a clean,
//! separable, *additive* future widget — deliberately **not** smuggled into
//! this slice (it would also overlap [`List`](crate::List), which already
//! single-selects from a set).
//!
//! # Focus *visual* here, focus *routing* deliberately not
//!
//! As [`Checkbox`](crate::Checkbox)/[`Button`](crate::Button) document at
//! length: this widget renders a *focused* control but does **not** decide
//! *which* control is focused. A focus manager (a registry of focusable
//! widgets, `Tab`/`Shift+Tab`/arrow traversal, click-to-focus) is a genuinely
//! new architectural axis that is expensive to reverse, so it belongs in its
//! own decision record and is **not** smuggled into a widget slice. `Radio`
//! reflects a plain caller-owned `focused: bool` and forecloses no future
//! routing design.
//!
//! # The marker is a multi-cell affordance, on purpose
//!
//! Borders/[`Gauge`](crate::Gauge)/[`Spinner`](crate::Spinner)/[`Scrollbar`](crate::Scrollbar)
//! banked the single-[`char`] [`Cell`](rstui_core::Buffer) dividend because
//! each glyph is one decorative scalar. A radio marker is different: the
//! parenthesised bullet `(•)`/`( )` is a *semantic, multi-cell* affordance
//! (the most portable terminal convention, the round counterpart of
//! `Checkbox`'s square `[x]`/`[ ]`), so the marker is a
//! [`Cow<str>`](std::borrow::Cow) — [`selected_symbol`](Radio::selected_symbol)
//! / [`unselected_symbol`](Radio::unselected_symbol) — mirroring
//! [`Checkbox`](crate::Checkbox) rather than the single-`char` widgets. The
//! recordable rule: the single-`char` model is for decorative scalars;
//! semantic multi-cell affordances use the `Cow<str>` symbol model.
//!
//! # A leaf control: one row, no `Block`
//!
//! Like [`Checkbox`](crate::Checkbox)/[`Button`](crate::Button)/[`Scrollbar`](crate::Scrollbar)/[`Spinner`](crate::Spinner)
//! — and unlike the container widgets — `Radio` has **no framing
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

/// A single-line labelled exclusive-choice control rendered as a pure
/// projection of caller-owned [`selected`](Self::selected) and
/// [`focused`](Self::focused) state.
///
/// Layout is `<marker><label>` on one row: the marker is
/// [`selected_symbol`](Self::selected_symbol) when `selected` else
/// [`unselected_symbol`](Self::unselected_symbol) (default `"(•) "` / `"( ) "`,
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
/// Exactly-one-per-group is the caller's invariant: hold one chosen index and
/// pass `selected(i == chosen)` per option. The widget only reads the `bool`.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Radio;
///
/// // A group of three; the caller owns the single chosen index.
/// let chosen = 1usize;
/// for (i, label) in ["Low", "Medium", "High"].iter().enumerate() {
///     let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
///     // `selected` is plain caller-owned state the widget only reads.
///     Radio::new(*label).selected(i == chosen).render(buf.area(), &mut buf);
///     let marker = buf.get(Position::new(1, 0)).unwrap().symbol;
///     // Only option 1 ("Medium") shows the filled bullet.
///     assert_eq!(marker, if i == chosen { '•' } else { ' ' });
/// }
///
/// // The label always starts at the same column (both markers are equal
/// // width), so a column of radios stays aligned as the choice moves.
/// let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
/// Radio::new("Medium").selected(true).render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, 'M');
/// ```
#[derive(Debug, Clone)]
pub struct Radio<'a> {
    label: Line<'a>,
    selected: bool,
    focused: bool,
    style: Style,
    focus_style: Style,
    selected_symbol: Cow<'a, str>,
    unselected_symbol: Cow<'a, str>,
}

impl<'a> Radio<'a> {
    /// A radio showing `label`: unselected, unfocused, default symbols
    /// (`"(•) "` / `"( ) "`) and styles.
    pub fn new(label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            selected: false,
            focused: false,
            style: Style::new(),
            focus_style: Style::new(),
            selected_symbol: Cow::Borrowed("(•) "),
            unselected_symbol: Cow::Borrowed("( ) "),
        }
    }

    /// Sets whether this option is the chosen one in its group — caller-owned
    /// state the widget only reads. Exactly-one-per-group is the caller's
    /// invariant (set it from `i == chosen` in `update`); the widget never
    /// enforces exclusivity.
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
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

    /// Sets the marker drawn when [`selected`](Self::selected) (default
    /// `"(•) "`). Keep both symbols the same width so the label column does
    /// not shift as the choice moves.
    #[must_use]
    pub fn selected_symbol(mut self, symbol: impl Into<Cow<'a, str>>) -> Self {
        self.selected_symbol = symbol.into();
        self
    }

    /// Sets the marker drawn when not selected (default `"( ) "`). Keep both
    /// symbols the same width so the label column does not shift.
    #[must_use]
    pub fn unselected_symbol(mut self, symbol: impl Into<Cow<'a, str>>) -> Self {
        self.unselected_symbol = symbol.into();
        self
    }
}

impl Widget for Radio<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Radio {
            label,
            selected,
            focused,
            style,
            focus_style,
            selected_symbol,
            unselected_symbol,
        } = self;

        let y = area.top();
        let left = area.left();
        let right = area.right();

        // The base, with the focus emphasis patched in when focused. Filling
        // the whole row makes a focused control read as one contiguous bar —
        // List's selection-bar idiom, here keyed by `focused` (the focus
        // visual, distinct from the `selected` data marker).
        let base = if focused {
            style.patch(focus_style)
        } else {
            style
        };
        buf.set_style(Rect::new(left, y, area.width, 1), base);

        // The marker is control chrome: styled by base/focus only, never the
        // label's span styles. Stamp it left to right, clipping at the edge.
        let marker = if selected {
            selected_symbol.as_ref()
        } else {
            unselected_symbol.as_ref()
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
    fn unselected_and_selected_differ_only_by_the_marker_glyph() {
        // Default symbols are equal width, so the label column is stable.
        assert_eq!(lines(Radio::new("Hi"), 8, 1), "( ) Hi  \n");
        assert_eq!(lines(Radio::new("Hi").selected(true), 8, 1), "(•) Hi  \n");
    }

    #[test]
    fn custom_symbols_replace_the_marker() {
        let r = Radio::new("On")
            .selected_symbol("(*) ")
            .unselected_symbol("( ) ");
        assert_eq!(lines(r.clone(), 6, 1), "( ) On\n");
        assert_eq!(lines(r.selected(true), 6, 1), "(*) On\n");
    }

    #[test]
    fn the_label_is_clipped_at_the_right_edge() {
        assert_eq!(lines(Radio::new("Medium"), 6, 1), "( ) Me\n");
    }

    #[test]
    fn an_area_too_narrow_for_the_marker_clips_without_panicking() {
        // Two cells: only "( " fits, the label never starts; no panic.
        assert_eq!(lines(Radio::new("X"), 2, 1), "( \n");
    }

    #[test]
    fn an_empty_label_renders_just_the_marker() {
        assert_eq!(lines(Radio::new("").selected(true), 5, 1), "(•)  \n");
    }

    #[test]
    fn exactly_one_of_a_caller_owned_group_shows_the_filled_marker() {
        // The exclusive-selection invariant lives in the caller: one chosen
        // index, each option projected to `selected(i == chosen)`. The widget
        // enforces nothing — it just reflects the bool.
        let labels = ["Low", "Med", "High"];
        let chosen = 2usize;
        let markers: Vec<char> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
                Radio::new(*l)
                    .selected(i == chosen)
                    .render(buf.area(), &mut buf);
                buf.get(Position::new(1, 0)).unwrap().symbol
            })
            .collect();
        // Only the chosen option's marker is the filled bullet.
        assert_eq!(markers, vec![' ', ' ', '•']);
    }

    #[test]
    fn focus_style_is_a_full_width_bar_over_marker_label_and_padding() {
        let r = Radio::new("Hi")
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        r.render(buf.area(), &mut buf);
        // Marker, label, and the empty cells after it all share the focus
        // background — one contiguous bar.
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '(');
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, 'H');
        assert_eq!(buf.get(Position::new(7, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn unfocused_paints_no_focus_style() {
        let r = Radio::new("Hi").focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        r.render(buf.area(), &mut buf);
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Reset);
        }
    }

    #[test]
    fn base_style_fills_the_whole_row() {
        let r = Radio::new("x").style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        r.render(buf.area(), &mut buf);
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
        let r = Radio::new(label)
            .unselected_symbol("")
            .style(Style::new().fg(Color::Green))
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        r.render(buf.area(), &mut buf);

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
        Radio::new("Z").selected(true).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '(');
        for y in 1..3 {
            for x in 0..5 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().symbol, ' ');
            }
        }
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        // A Layout-placed radio draws where it was placed.
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        Radio::new("A").render(Rect::new(2, 3, 6, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 3)).unwrap().symbol, '(');
        assert_eq!(buf.get(Position::new(6, 3)).unwrap().symbol, 'A');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Radio::new("hello")
            .selected(true)
            .focused(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
