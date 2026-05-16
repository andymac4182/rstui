//! [`Alert`] — a persistent, level-accented inline banner: an icon, a title,
//! and an optional wrapped body framed in its area (a form's validation
//! summary, an empty-state hint, a "you are offline" strip).
//!
//! # Persistent, unlike [`Toast`](crate::Toast)
//!
//! [`Toast`](crate::Toast) is *transient* — it floats over content and the
//! reducer expires it. `Alert` is *persistent*: it occupies a real region in
//! the layout for as long as the app's model says the condition holds, the
//! same way a form keeps an error banner pinned above the fields. Both share
//! the per-level accent idea ([`AlertLevel`] mirrors
//! [`ToastLevel`](crate::ToastLevel)), so one palette themes both.
//!
//! # A pure projection; body wrap is *reused*
//!
//! `Alert` owns no state — a caller-built title [`Line`], an optional body
//! [`Text`], and an [`AlertLevel`], projected to glyphs. Its body is rendered
//! through a private [`Paragraph`] with soft
//! [`Wrap`], so wrapping and right-edge clipping are *inherited*,
//! never a second algorithm (the [`Toast`](crate::Toast)/[`DescriptionList`](crate::DescriptionList)
//! reuse). An optional framing [`Block`] follows the container-widget
//! convention.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no body, a one-row area (only the icon+title row, clipped), and a
//! title/body far wider than the area (clipped/wrapped) are all safe
//! clips/no-ops — never a panic.

use rstui_core::{Buffer, Line, Position, Rect, Style, Text, Widget};

use crate::block::Block;
use crate::paragraph::{Paragraph, Wrap};

/// The soft-wrap mode the body uses (matches [`Toast`](crate::Toast)'s).
const BODY_WRAP: Wrap = Wrap { trim: false };

/// The severity of an [`Alert`], selecting its default icon and which accent
/// [`Style`] the banner is drawn with (mirrors
/// [`ToastLevel`](crate::ToastLevel)).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AlertLevel {
    /// Neutral information (the default), icon `ℹ`.
    #[default]
    Info,
    /// A success / "done" state, icon `✓`.
    Success,
    /// A non-fatal caution, icon `⚠`.
    Warning,
    /// A failure the user should notice, icon `✗`.
    Error,
}

impl AlertLevel {
    /// The default single-`char` icon for this level (one Unicode scalar, so
    /// it maps 1:1 onto a [`Cell`](rstui_core::Buffer) — the
    /// [`Block`] border reasoning).
    #[must_use]
    pub const fn icon(self) -> char {
        match self {
            Self::Info => 'ℹ',
            Self::Success => '✓',
            Self::Warning => '⚠',
            Self::Error => '✗',
        }
    }
}

/// A persistent, level-accented banner: `icon title` on the first row and an
/// optional soft-wrapped body below it, over an accent fill.
///
/// The accent [`Style`] for the [`level`](Self::level) is patched over the base
/// [`style`](Self::style) and fills the whole banner; the title's and body's
/// own text styles cascade on top. An optional [`Block`] frames it.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Alert, AlertLevel};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
/// Alert::new(AlertLevel::Error, "Build failed").render(buf.area(), &mut buf);
///
/// // The level icon, a space, then the title.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '✗');
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'B');
/// ```
#[derive(Debug, Clone)]
pub struct Alert<'a> {
    level: AlertLevel,
    title: Line<'a>,
    body: Option<Text<'a>>,
    icon: Option<char>,
    block: Option<Block<'a>>,
    style: Style,
    info_style: Style,
    success_style: Style,
    warning_style: Style,
    error_style: Style,
}

impl<'a> Alert<'a> {
    /// An alert of `level` with `title` (anything convertible to a [`Line`]),
    /// no body, the level's default icon, and no frame.
    pub fn new(level: AlertLevel, title: impl Into<Line<'a>>) -> Self {
        Self {
            level,
            title: title.into(),
            body: None,
            icon: None,
            block: None,
            style: Style::default(),
            info_style: Style::default(),
            success_style: Style::default(),
            warning_style: Style::default(),
            error_style: Style::default(),
        }
    }

    /// Sets the optional wrapped body (anything convertible to a [`Text`]).
    #[must_use]
    pub fn body(mut self, body: impl Into<Text<'a>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Sets the accent [`AlertLevel`] (also changes the default icon).
    #[must_use]
    pub fn level(mut self, level: AlertLevel) -> Self {
        self.level = level;
        self
    }

    /// Overrides the level's default icon with `icon`.
    #[must_use]
    pub fn icon(mut self, icon: char) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Frames the alert in `block`; content renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`], beneath the level accent and the text cascade.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the accent [`Style`] for [`AlertLevel::Info`].
    #[must_use]
    pub fn info_style(mut self, style: Style) -> Self {
        self.info_style = style;
        self
    }

    /// Sets the accent [`Style`] for [`AlertLevel::Success`].
    #[must_use]
    pub fn success_style(mut self, style: Style) -> Self {
        self.success_style = style;
        self
    }

    /// Sets the accent [`Style`] for [`AlertLevel::Warning`].
    #[must_use]
    pub fn warning_style(mut self, style: Style) -> Self {
        self.warning_style = style;
        self
    }

    /// Sets the accent [`Style`] for [`AlertLevel::Error`].
    #[must_use]
    pub fn error_style(mut self, style: Style) -> Self {
        self.error_style = style;
        self
    }

    /// The accent [`Style`] for the current level, patched over the base.
    fn accent(&self) -> Style {
        let level = match self.level {
            AlertLevel::Info => self.info_style,
            AlertLevel::Success => self.success_style,
            AlertLevel::Warning => self.warning_style,
            AlertLevel::Error => self.error_style,
        };
        self.style.patch(level)
    }
}

impl Widget for Alert<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let accent = self.accent();
        let glyph = self.icon.unwrap_or_else(|| self.level.icon());
        let Alert {
            title, body, block, ..
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

        // The accent fills the whole banner so it reads as one coloured strip;
        // the icon, title, and body layer their own styles on top.
        buf.set_style(inner, accent);

        let right = inner.right();
        let y = inner.top();

        // Row 0: `icon`, a separator space, then the title (clipped).
        let mut x = inner.left();
        buf.set_cell(Position::new(x, y), glyph, accent);
        x = x.saturating_add(2); // icon + one separator space
        let title_base = accent.patch(title.style);
        'title: for span in &title.spans {
            let style = title_base.patch(span.style);
            for ch in span.content.chars() {
                if x >= right {
                    break 'title;
                }
                buf.set_cell(Position::new(x, y), ch, style);
                x = x.saturating_add(1);
            }
        }

        // Rows 1..: the optional body, soft-wrapped through a reused
        // Paragraph, in the accent (its own text styles cascade on top).
        if let Some(body) = body {
            if inner.height > 1 {
                let body_area = Rect::new(
                    inner.left(),
                    inner.top().saturating_add(1),
                    inner.width,
                    inner.height - 1,
                );
                Paragraph::new(body)
                    .wrap(BODY_WRAP)
                    .style(accent)
                    .render(body_area, buf);
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
    fn the_first_row_is_icon_space_then_title() {
        // Info icon ℹ, a separator space, then "Saved", clipped to width.
        assert_eq!(
            lines(Alert::new(AlertLevel::Info, "Saved"), 9, 1),
            "ℹ Saved  \n"
        );
    }

    #[test]
    fn each_level_has_its_own_default_icon() {
        for (level, icon) in [
            (AlertLevel::Info, 'ℹ'),
            (AlertLevel::Success, '✓'),
            (AlertLevel::Warning, '⚠'),
            (AlertLevel::Error, '✗'),
        ] {
            let out = lines(Alert::new(level, "x"), 3, 1);
            assert_eq!(out.chars().next().unwrap(), icon, "{level:?}");
        }
    }

    #[test]
    fn the_icon_can_be_overridden() {
        let out = lines(Alert::new(AlertLevel::Error, "x").icon('!'), 3, 1);
        assert_eq!(out, "! x\n");
    }

    #[test]
    fn a_body_wraps_below_the_title_reusing_paragraph() {
        // Body "aa bb" soft-wraps to width 5 across the two rows below the
        // title (Paragraph's wrap, reused — not a second algorithm).
        let alert = Alert::new(AlertLevel::Info, "T").body("aa bb");
        assert_eq!(lines(alert, 5, 3), "ℹ T  \naa bb\n     \n");
    }

    #[test]
    fn no_body_is_just_the_title_row() {
        assert_eq!(
            lines(Alert::new(AlertLevel::Info, "Hi"), 5, 2),
            "ℹ Hi \n     \n"
        );
    }

    #[test]
    fn a_one_row_area_drops_the_body_without_panicking() {
        let alert = Alert::new(AlertLevel::Warning, "W").body("hidden");
        assert_eq!(lines(alert, 4, 1), "⚠ W \n");
    }

    #[test]
    fn the_title_is_clipped_at_the_right_edge() {
        assert_eq!(
            lines(Alert::new(AlertLevel::Info, "overlong"), 5, 1),
            "ℹ ove\n"
        );
    }

    #[test]
    fn the_accent_fills_the_whole_banner() {
        let alert = Alert::new(AlertLevel::Error, "E")
            .body("b")
            .error_style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        alert.render(buf.area(), &mut buf);
        // Every cell of the banner — title row and body row — is the accent.
        for y in 0..2 {
            for x in 0..4 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
    }

    #[test]
    fn the_title_style_cascades_over_the_accent() {
        let alert = Alert::new(
            AlertLevel::Info,
            Line::from(Span::styled("X", Style::new().fg(Color::Red))),
        )
        .style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        alert.render(buf.area(), &mut buf);
        // Title starts at x = 2 (icon + separator space).
        let x = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(x.symbol, 'X');
        assert_eq!(x.fg, Color::Red); // span fg wins
        assert!(x.modifier.contains(Modifier::BOLD)); // base style cascades
    }

    #[test]
    fn a_block_frames_the_alert_in_the_inner_area() {
        let alert = Alert::new(AlertLevel::Info, "Hi").block(Block::bordered());
        assert_eq!(lines(alert, 6, 3), "┌────┐\n│ℹ Hi│\n└────┘\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_content() {
        let alert = Alert::new(AlertLevel::Info, "Z").block(Block::bordered());
        assert_eq!(lines(alert, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Alert::new(AlertLevel::Info, "hi").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
