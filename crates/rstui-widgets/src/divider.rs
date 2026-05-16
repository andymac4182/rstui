//! [`Divider`] — a one-cell-thick horizontal or vertical rule, optionally
//! captioned, the separator that breaks a pane into sections (a settings
//! group heading, a toolbar split, a `── or ──` between form choices).
//!
//! # A pure projection, and a leaf
//!
//! `Divider` owns no state — an orientation, an optional caption [`Line`], and
//! a [`Style`], projected to glyphs, the same headless-testable shape every
//! widget here uses. Like [`StatusBar`](crate::StatusBar) and unlike the
//! container widgets it has **no framing [`Block`](crate::Block)**: it draws on
//! exactly one row (horizontal) or one column (vertical) of its area and the
//! surrounding [`Layout`](rstui_core::Layout) owns placement. The rule glyph
//! is taken from a [`BorderType`] so a divider matches the
//! [`Block`](crate::Block) frames around it — reusing that vocabulary rather
//! than inventing a parallel one.
//!
//! # The caption is horizontal-only, on purpose
//!
//! A caption is a run of glyphs; it only reads along a *horizontal* rule. A
//! vertical divider is therefore a plain rule and its caption (if any) is
//! ignored — a stacked/rotated vertical caption is a deliberately deferred
//! additive, not smuggled into this slice (the same "defer the rare mode, keep
//! the API honest" stance [`List`](crate::List) takes on multi-line rows).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, a width/height of `1`, and a caption wider than the rule (clipped at
//! the far edge) are all safe clips/no-ops — never a panic.

use rstui_core::{Alignment, Buffer, Line, Position, Rect, Style, Widget};

use crate::block::BorderType;

/// Which way a [`Divider`] runs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DividerOrientation {
    /// A horizontal rule on the top row of the area (the default); a caption
    /// reads along it.
    #[default]
    Horizontal,
    /// A vertical rule on the left column of the area; the caption is ignored
    /// (see the [module docs](self)).
    Vertical,
}

/// A captioned rule.
///
/// The rule glyph is [`BorderType::set`]'s horizontal/vertical edge for the
/// chosen [`border_type`](Self::border_type), drawn over the base
/// [`style`](Self::style) (which also fills the area). An optional
/// [`label`](Self::label) [`Line`] is inset into a horizontal rule with one
/// blank column of breathing room each side, positioned by
/// [`label_alignment`](Self::label_alignment); its own
/// [`Line`]/[`Span`](rstui_core::Span) styles cascade over
/// [`label_style`](Self::label_style).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Divider;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 9, 1));
/// Divider::new().label("Hi").render(buf.area(), &mut buf);
///
/// // A horizontal rule with the caption centred and a blank column each side.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '─');
/// assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, 'H');
/// assert_eq!(buf.get(Position::new(8, 0)).unwrap().symbol, '─');
/// ```
#[derive(Debug, Default, Clone)]
pub struct Divider<'a> {
    label: Option<Line<'a>>,
    orientation: DividerOrientation,
    label_alignment: Alignment,
    border_type: BorderType,
    style: Style,
    label_style: Style,
}

impl<'a> Divider<'a> {
    /// A plain horizontal, centred-caption rule with no caption and the
    /// [`Plain`](BorderType::Plain) glyph.
    #[must_use]
    pub fn new() -> Self {
        Self {
            label_alignment: Alignment::Center,
            ..Self::default()
        }
    }

    /// Sets the caption [`Line`] (anything convertible to one). Ignored for a
    /// [`Vertical`](DividerOrientation::Vertical) divider.
    #[must_use]
    pub fn label(mut self, label: impl Into<Line<'a>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the orientation.
    #[must_use]
    pub fn orientation(mut self, orientation: DividerOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Sets where the caption sits along a horizontal rule (default
    /// [`Center`](Alignment::Center)).
    #[must_use]
    pub fn label_alignment(mut self, alignment: Alignment) -> Self {
        self.label_alignment = alignment;
        self
    }

    /// Sets the [`BorderType`] whose edge glyph the rule is drawn with, so a
    /// divider matches the [`Block`](crate::Block)s around it.
    #[must_use]
    pub fn border_type(mut self, border_type: BorderType) -> Self {
        self.border_type = border_type;
        self
    }

    /// Sets the base [`Style`] for the rule; it also fills the area so a
    /// background covers the whole strip.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the base [`Style`] for the caption, beneath its own
    /// [`Line`]/[`Span`](rstui_core::Span) styles.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }
}

impl Widget for Divider<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Divider {
            label,
            orientation,
            label_alignment,
            border_type,
            style,
            label_style,
        } = self;

        // Base fills the area so a background reads as one strip.
        buf.set_style(area, style);
        let set = border_type.set();

        match orientation {
            DividerOrientation::Vertical => {
                // A plain rule down the left column; captions don't apply.
                let x = area.left();
                for y in area.top()..area.bottom() {
                    buf.set_cell(Position::new(x, y), set.vertical, style);
                }
            }
            DividerOrientation::Horizontal => {
                let y = area.top();
                let left = area.left();
                let right = area.right();
                for x in left..right {
                    buf.set_cell(Position::new(x, y), set.horizontal, style);
                }

                // Inset the caption (if any) with one blank column each side.
                let Some(label) = label else { return };
                let label_w = label.width() as u16;
                if label_w == 0 {
                    return;
                }
                // If the caption plus its breathing space cannot fit the
                // rule, drop it for a clean full rule rather than paint a
                // ragged stub (still total — just the cleaner projection).
                let total = label_w.saturating_add(2);
                if total > area.width {
                    return;
                }
                let start = match label_alignment {
                    Alignment::Left => left.saturating_add(1),
                    Alignment::Right => right.saturating_sub(total),
                    Alignment::Center => left.saturating_add(area.width.saturating_sub(total) / 2),
                };
                let line_base = style.patch(label_style).patch(label.style);

                // Leading breathing space, the caption spans, trailing space —
                // all over the rule, clipped at the right edge.
                let mut x = start;
                let mut put = |x: &mut u16, ch: char, st: Style| {
                    if *x < right {
                        buf.set_cell(Position::new(*x, y), ch, st);
                        *x = x.saturating_add(1);
                    }
                };
                put(&mut x, ' ', line_base);
                'cap: for span in &label.spans {
                    let st = line_base.patch(span.style);
                    for ch in span.content.chars() {
                        if x >= right {
                            break 'cap;
                        }
                        put(&mut x, ch, st);
                    }
                }
                put(&mut x, ' ', line_base);
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
    fn a_bare_horizontal_divider_is_a_full_rule() {
        assert_eq!(lines(Divider::new(), 5, 1), "─────\n");
    }

    #[test]
    fn the_border_type_selects_the_rule_glyph() {
        assert_eq!(
            lines(Divider::new().border_type(BorderType::Double), 4, 1),
            "════\n"
        );
        assert_eq!(
            lines(Divider::new().border_type(BorderType::Thick), 4, 1),
            "━━━━\n"
        );
    }

    #[test]
    fn a_centred_caption_sits_in_the_rule_with_breathing_space() {
        // width 9, "Hi" → total 4, centred at (9-4)/2 = 2.
        assert_eq!(lines(Divider::new().label("Hi"), 9, 1), "── Hi ───\n");
    }

    #[test]
    fn a_left_aligned_caption_starts_after_one_rule_cell() {
        assert_eq!(
            lines(
                Divider::new().label("Hi").label_alignment(Alignment::Left),
                9,
                1
            ),
            "─ Hi ────\n"
        );
    }

    #[test]
    fn a_right_aligned_caption_ends_one_cell_before_the_edge() {
        assert_eq!(
            lines(
                Divider::new().label("Hi").label_alignment(Alignment::Right),
                9,
                1
            ),
            "───── Hi \n"
        );
    }

    #[test]
    fn a_vertical_divider_is_a_left_column_rule() {
        assert_eq!(
            lines(
                Divider::new().orientation(DividerOrientation::Vertical),
                3,
                2
            ),
            "│  \n│  \n"
        );
    }

    #[test]
    fn a_vertical_divider_ignores_the_caption() {
        let d = Divider::new()
            .orientation(DividerOrientation::Vertical)
            .label("ignored");
        assert_eq!(lines(d, 2, 2), "│ \n│ \n");
    }

    #[test]
    fn an_empty_caption_is_just_the_rule() {
        assert_eq!(lines(Divider::new().label(""), 4, 1), "────\n");
    }

    #[test]
    fn a_caption_that_cannot_fit_is_dropped_for_a_clean_rule() {
        // "Hello" + 2 padding = 7 > width 3: the caption is dropped and the
        // full rule remains (total, no panic, no ragged stub).
        assert_eq!(lines(Divider::new().label("Hello"), 3, 1), "───\n");
    }

    #[test]
    fn a_width_one_horizontal_divider_is_one_rule_cell() {
        // The caption cannot fit, so it is dropped: just the rule cell.
        assert_eq!(lines(Divider::new().label("x"), 1, 1), "─\n");
    }

    #[test]
    fn the_caption_style_cascades_over_the_base() {
        let d = Divider::new()
            .label(Line::from(Span::styled("X", Style::new().fg(Color::Red))))
            .style(Style::new().bg(Color::Blue))
            .label_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        d.render(buf.area(), &mut buf);
        // width 5, "X" total 3, centred at (5-3)/2 = 1 → space@1, X@2.
        let x = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(x.symbol, 'X');
        assert_eq!(x.fg, Color::Red); // span fg wins
        assert!(x.modifier.contains(Modifier::BOLD)); // label_style cascades
        assert_eq!(x.bg, Color::Blue); // base fill cascades
        // A rule cell keeps the base style only.
        let rule = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(rule.symbol, '─');
        assert_eq!(rule.bg, Color::Blue);
    }

    #[test]
    fn the_base_style_fills_the_whole_area() {
        let d = Divider::new().style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        d.render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Divider::new()
            .label("x")
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
