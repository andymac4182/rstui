//! [`Checkpoint`] — an inline timeline marker: a bookmark glyph on a
//! horizontal separator rule with an optional label, the "rewind to here" /
//! "snapshot taken" beat the agent transcript drops between turns.
//!
//! # A pure, leaf projection
//!
//! The ai-elements `Checkpoint` is a labelled divider with a save/bookmark
//! affordance. There is no state to own — a checkpoint is a caller-built
//! label plus a glyph, drawn as one separator row. So, like
//! [`Divider`](rstui_widgets::Divider) (which this is modelled on) and
//! [`Badge`](rstui_widgets::Badge), `Checkpoint` owns nothing and is a leaf
//! adornment: it fills exactly its one row — a [`bookmark`](Checkpoint::bookmark)
//! glyph, the [`label`](Checkpoint::new) (if any), then a rule of
//! [`rule_char`](Checkpoint::rule_char) to the right edge.
//!
//! Selecting/activating a checkpoint is the host's job: it owns the list of
//! checkpoints and hit-tests a click against the row `Rect` it laid out (the
//! documented mouse seam) — the widget exposes no callback, exactly the
//! [`Divider`](rstui_widgets::Divider)/[`Badge`](rstui_widgets::Badge)
//! discipline.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule an empty area, an
//! empty label, and an area too narrow for the label all clip cleanly to the
//! row — never a panic.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

/// An inline timeline checkpoint marker drawn on one separator row.
///
/// The row is the [`bookmark`](Self::bookmark) glyph, a space, the
/// [`label`](Self::new) (a full [`Line`], so a styled label cascades over
/// [`label_style`](Self::label_style)), a space, then the
/// [`rule_char`](Self::rule_char) repeated to the right edge in
/// [`rule_style`](Self::rule_style). With no label it is the glyph then a
/// full-width rule. `Checkpoint` owns no state — see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::checkpoint::Checkpoint;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
/// Checkpoint::new("Restore").render(buf.area(), &mut buf);
///
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '⚑'); // bookmark
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'R'); // label
/// assert_eq!(buf.get(Position::new(11, 0)).unwrap().symbol, '─'); // rule
/// ```
#[derive(Debug, Clone)]
pub struct Checkpoint<'a> {
    label: Line<'a>,
    bookmark: char,
    rule_char: char,
    style: Style,
    label_style: Style,
    rule_style: Style,
}

impl<'a> Checkpoint<'a> {
    /// A checkpoint labelled `label` (any value convertible to a [`Line`]),
    /// with the default `⚑` bookmark and `─` rule.
    pub fn new(label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            bookmark: '⚑',
            rule_char: '─',
            style: Style::new(),
            label_style: Style::new(),
            rule_style: Style::new(),
        }
    }

    /// An unlabelled checkpoint — just the bookmark glyph then a full-width
    /// rule.
    #[must_use]
    pub fn unlabelled() -> Self {
        Self::new("")
    }

    /// Sets the bookmark glyph drawn at the row start (default `⚑`).
    #[must_use]
    pub fn bookmark(mut self, bookmark: char) -> Self {
        self.bookmark = bookmark;
        self
    }

    /// Sets the character the trailing rule is drawn with (default `─`).
    #[must_use]
    pub fn rule_char(mut self, rule_char: char) -> Self {
        self.rule_char = rule_char;
        self
    }

    /// Sets the base [`Style`], beneath the label and rule styles.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] the label is drawn with (over the base; the label
    /// line's own spans cascade on top).
    #[must_use]
    pub fn label_style(mut self, label_style: Style) -> Self {
        self.label_style = label_style;
        self
    }

    /// Sets the [`Style`] the trailing rule is drawn with (over the base).
    #[must_use]
    pub fn rule_style(mut self, rule_style: Style) -> Self {
        self.rule_style = rule_style;
        self
    }
}

impl Widget for Checkpoint<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let y = area.top();
        let right = area.right();
        let base = self.style;
        let mut x = area.left();

        // The bookmark glyph.
        buf.set_cell(Position::new(x, y), self.bookmark, base);
        x = x.saturating_add(1);

        // The label, padded one space each side, span styles cascading over
        // the label style over the base.
        let label_base = base.patch(self.label_style).patch(self.label.style);
        let has_label = self.label.width() > 0;
        if has_label && x < right {
            buf.set_cell(Position::new(x, y), ' ', base);
            x = x.saturating_add(1);
            'label: for span in &self.label.spans {
                let span_style = label_base.patch(span.style);
                for ch in span.content.chars() {
                    if x >= right {
                        break 'label;
                    }
                    buf.set_cell(Position::new(x, y), ch, span_style);
                    x = x.saturating_add(1);
                }
            }
            if x < right {
                buf.set_cell(Position::new(x, y), ' ', base);
                x = x.saturating_add(1);
            }
        }

        // The rule fills the rest of the row.
        let rule_style = base.patch(self.rule_style);
        while x < right {
            buf.set_cell(Position::new(x, y), self.rule_char, rule_style);
            x = x.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier, Span};

    fn row(widget: Checkpoint<'_>, width: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
        widget.render(buf.area(), &mut buf);
        (0..width)
            .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
            .collect()
    }

    #[test]
    fn a_labelled_checkpoint_is_glyph_label_then_rule() {
        assert_eq!(row(Checkpoint::new("Save"), 12), "⚑ Save ─────");
    }

    #[test]
    fn an_unlabelled_checkpoint_is_a_glyph_then_a_full_rule() {
        assert_eq!(row(Checkpoint::unlabelled(), 6), "⚑─────");
    }

    #[test]
    fn custom_glyph_and_rule_char() {
        assert_eq!(
            row(Checkpoint::new("x").bookmark('◆').rule_char('·'), 8),
            "◆ x ····"
        );
    }

    #[test]
    fn a_narrow_area_clips_the_label_and_drops_the_rule() {
        // width 4: glyph, space, "ov" — clipped, no rule room.
        assert_eq!(row(Checkpoint::new("overlong"), 4), "⚑ ov");
    }

    #[test]
    fn the_label_style_and_span_styles_cascade() {
        let widget = Checkpoint::new(Line::from(vec![Span::styled(
            "C",
            Style::new().fg(Color::Red),
        )]))
        .label_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        widget.render(buf.area(), &mut buf);
        let c = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(c.symbol, 'C');
        assert_eq!(c.fg, Color::Red);
        assert!(c.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Checkpoint::new("x").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
