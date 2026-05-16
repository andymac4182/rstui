//! [`Badge`] — a tiny inline pill: a padded, level-accented label that sits
//! *within* a line of other content (a `[ERROR]` tag beside a log line, a
//! count chip on a tab, a `NEW` flag in a list).
//!
//! # A pure projection, and an inline one
//!
//! `Badge` owns no state — it is a caller-built label [`Line`] plus a
//! [`BadgeLevel`], projected to glyphs, the same headless-testable shape every
//! widget here uses. Its one *deliberate divergence* from the
//! fill-the-whole-area convention: a badge is an **inline pill**, not a bar, so
//! it paints **only its own pill cells** (the padded label) and leaves the rest
//! of the area untouched, so surrounding content shows up to either side.
//! (Filling the area is the right rule for a *region* widget like
//! [`List`](crate::List); a chip that clobbered its whole row would be
//! unusable mid-sentence — the same reason [`Toast`](crate::Toast) reasons
//! explicitly about what it does and does not paint.)
//!
//! # Level accent, like [`ToastLevel`](crate::ToastLevel)
//!
//! The [`BadgeLevel`] selects which accent [`Style`] the pill is drawn with
//! (defaulting to [`Neutral`](BadgeLevel::Neutral)), exactly the per-level
//! style-selection model [`Toast`](crate::Toast) uses, so an app themes badges
//! and toasts from one palette.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, an empty label, and an area narrower than the pill (the pill is
//! clipped at the right edge) are all safe clips/no-ops — never a panic. A
//! framing [`Block`](crate::Block) is intentionally absent: a pill is a leaf
//! adornment, like [`StatusBar`](crate::StatusBar), not a container.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

/// The accent level of a [`Badge`], selecting which accent [`Style`] the pill
/// is drawn with (mirrors [`ToastLevel`](crate::ToastLevel), with a
/// [`Neutral`](Self::Neutral) default for a plain tag).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BadgeLevel {
    /// A plain, un-accented tag (the default).
    #[default]
    Neutral,
    /// Neutral information.
    Info,
    /// A success / "done" state.
    Success,
    /// A non-fatal caution.
    Warning,
    /// An error the reader should notice.
    Error,
}

/// A one-row, padded, level-accented inline pill.
///
/// The pill is [`padding`](Self::padding) blank columns, the
/// label, then `padding` columns again, all in the level's
/// accent [`Style`] (patched over the base [`style`](Self::style)); the label's
/// own [`Line`]/[`Span`](rstui_core::Span) styles cascade on top. Only those
/// cells are painted — the badge is inline (see the [module docs](self)).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Badge;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
/// Badge::new("NEW").render(buf.area(), &mut buf);
///
/// // Default padding is one space each side: " NEW " is five columns.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'N');
/// assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, ' ');
/// // Past the pill the area is untouched (inline, not a bar).
/// assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, ' ');
/// ```
#[derive(Debug, Clone)]
pub struct Badge<'a> {
    label: Line<'a>,
    level: BadgeLevel,
    padding: u16,
    style: Style,
    neutral_style: Style,
    info_style: Style,
    success_style: Style,
    warning_style: Style,
    error_style: Style,
}

impl Default for Badge<'_> {
    fn default() -> Self {
        Self {
            label: Line::default(),
            level: BadgeLevel::Neutral,
            padding: 1,
            style: Style::default(),
            neutral_style: Style::default(),
            info_style: Style::default(),
            success_style: Style::default(),
            warning_style: Style::default(),
            error_style: Style::default(),
        }
    }
}

impl<'a> Badge<'a> {
    /// A [`Neutral`](BadgeLevel::Neutral) pill displaying `label` (anything
    /// convertible to a [`Line`]) with one space of padding each side.
    pub fn new(label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            ..Self::default()
        }
    }

    /// Sets the accent [`BadgeLevel`].
    #[must_use]
    pub fn level(mut self, level: BadgeLevel) -> Self {
        self.level = level;
        self
    }

    /// Sets the blank columns on each side of the label (default `1`).
    #[must_use]
    pub fn padding(mut self, padding: u16) -> Self {
        self.padding = padding;
        self
    }

    /// Sets the base [`Style`], beneath the level accent and the label's own
    /// styles.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the accent [`Style`] for [`BadgeLevel::Neutral`].
    #[must_use]
    pub fn neutral_style(mut self, style: Style) -> Self {
        self.neutral_style = style;
        self
    }

    /// Sets the accent [`Style`] for [`BadgeLevel::Info`].
    #[must_use]
    pub fn info_style(mut self, style: Style) -> Self {
        self.info_style = style;
        self
    }

    /// Sets the accent [`Style`] for [`BadgeLevel::Success`].
    #[must_use]
    pub fn success_style(mut self, style: Style) -> Self {
        self.success_style = style;
        self
    }

    /// Sets the accent [`Style`] for [`BadgeLevel::Warning`].
    #[must_use]
    pub fn warning_style(mut self, style: Style) -> Self {
        self.warning_style = style;
        self
    }

    /// Sets the accent [`Style`] for [`BadgeLevel::Error`].
    #[must_use]
    pub fn error_style(mut self, style: Style) -> Self {
        self.error_style = style;
        self
    }

    /// The accent [`Style`] for the current level, patched over the base.
    fn accent(&self) -> Style {
        let level = match self.level {
            BadgeLevel::Neutral => self.neutral_style,
            BadgeLevel::Info => self.info_style,
            BadgeLevel::Success => self.success_style,
            BadgeLevel::Warning => self.warning_style,
            BadgeLevel::Error => self.error_style,
        };
        self.style.patch(level)
    }
}

impl Widget for Badge<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let accent = self.accent();
        let Badge { label, padding, .. } = self;

        let y = area.top();
        let right = area.right();
        // The pill is padding + label + padding, clipped to the area width;
        // only those cells are painted (inline, not a bar).
        let label_w = label.width() as u16;
        let pill_w = label_w
            .saturating_add(padding.saturating_mul(2))
            .min(area.width);

        // Paint the pill background (padding + label cells) in the accent.
        buf.set_style(Rect::new(area.left(), y, pill_w, 1), accent);

        // Then the label glyphs after the left padding, label→span cascade
        // over the accent, clipped at the pill's (and the area's) right edge.
        let line_base = accent.patch(label.style);
        let mut x = area.left().saturating_add(padding);
        'pill: for span in &label.spans {
            let style = line_base.patch(span.style);
            for ch in span.content.chars() {
                if x >= right {
                    break 'pill;
                }
                buf.set_cell(Position::new(x, y), ch, style);
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
    fn a_badge_is_a_padded_pill() {
        // Default padding 1: " OK " (4-wide pill) then untouched cells.
        assert_eq!(lines(Badge::new("OK"), 8, 1), " OK     \n");
    }

    #[test]
    fn padding_widens_the_pill() {
        assert_eq!(lines(Badge::new("x").padding(2), 7, 1), "  x    \n");
    }

    #[test]
    fn zero_padding_is_a_bare_label() {
        assert_eq!(lines(Badge::new("hi").padding(0), 4, 1), "hi  \n");
    }

    #[test]
    fn the_pill_clips_at_the_right_edge() {
        // " WARN " is 6 wide but the area is 4: clipped after " WAR".
        assert_eq!(lines(Badge::new("WARN"), 4, 1), " WAR\n");
    }

    #[test]
    fn only_the_pill_cells_are_painted_not_the_whole_row() {
        // Pre-fill the row; the badge must leave the cells past its pill.
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        for x in 0..8 {
            buf.set_cell(Position::new(x, 0), '.', Style::new());
        }
        Badge::new("A").padding(0).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'A');
        // Everything past the 1-cell pill is the untouched '.' fill.
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '.');
        assert_eq!(buf.get(Position::new(7, 0)).unwrap().symbol, '.');
    }

    #[test]
    fn the_level_selects_the_accent_style() {
        let levels = [
            (BadgeLevel::Neutral, Color::Gray),
            (BadgeLevel::Info, Color::Blue),
            (BadgeLevel::Success, Color::Green),
            (BadgeLevel::Warning, Color::Yellow),
            (BadgeLevel::Error, Color::Red),
        ];
        for (level, color) in levels {
            let badge = Badge::new("x")
                .padding(0)
                .level(level)
                .neutral_style(Style::new().bg(Color::Gray))
                .info_style(Style::new().bg(Color::Blue))
                .success_style(Style::new().bg(Color::Green))
                .warning_style(Style::new().bg(Color::Yellow))
                .error_style(Style::new().bg(Color::Red));
            let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
            badge.render(buf.area(), &mut buf);
            assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, color, "{level:?}");
        }
    }

    #[test]
    fn the_accent_fills_the_padding_as_well_as_the_label() {
        let badge = Badge::new("x")
            .padding(1)
            .level(BadgeLevel::Error)
            .error_style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        badge.render(buf.area(), &mut buf);
        // The padding cells share the accent so the pill reads as one chip.
        for x in 0..3 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Red);
        }
        // Past the pill: untouched.
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn the_label_style_cascades_over_the_accent() {
        let badge = Badge::new(Line::from(vec![
            Span::styled("E", Style::new().fg(Color::Red)),
            Span::raw("r"),
        ]))
        .padding(0)
        .style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        badge.render(buf.area(), &mut buf);
        let e = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(e.symbol, 'E');
        assert_eq!(e.fg, Color::Red); // span fg wins
        assert!(e.modifier.contains(Modifier::BOLD)); // base style cascades
        let r = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(r.symbol, 'r');
        assert!(r.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn an_empty_label_is_just_padding() {
        // No label, padding 2 → a 4-wide blank accented pill, no panic.
        let badge = Badge::new("")
            .padding(2)
            .level(BadgeLevel::Info)
            .info_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        badge.render(buf.area(), &mut buf);
        for x in 0..4 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        Badge::new("Z")
            .padding(0)
            .render(Rect::new(3, 1, 4, 1), &mut buf);
        assert_eq!(buf.get(Position::new(3, 1)).unwrap().symbol, 'Z');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Badge::new("hi").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
