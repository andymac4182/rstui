//! [`Toast`] — a corner-anchored stack of transient, **opaque** notification
//! boxes floated over an overlay area, the editor/IDE "saved", "build failed",
//! "copied" strip.
//!
//! # A pure projection — the reducer owns timing and dismissal
//!
//! Like every rstui widget `Toast` is a **pure projection**: it renders
//! exactly the `&[ToastMessage]` slice it is handed and reads nothing else.
//! The notification list is ordinary caller-owned model state — the reducer
//! pushes a message in `update`, and *when a toast expires or is dismissed is
//! also the reducer's job* (a timer message trims the front/back of the Vec,
//! a keypress clears it). The widget has **no clock and no dismissal**: that
//! timing concern is deliberately deferred and kept out of the view, exactly
//! as [`Spinner`](crate::Spinner) leaves its animation `tick` caller-owned and
//! [`List`](crate::List) leaves scrolling to the reducer. The widget just
//! projects "what is in the list right now"; making it own expiry would smuggle
//! a wall clock into the pure `view`, the one thing the architecture forbids.
//!
//! ## Ordering convention
//!
//! `messages[0]` is the **newest**. It is anchored flush to the chosen
//! [`corner`](Toast::corner); older entries (higher indices) stack *away* from
//! that corner — downward for the `Top*` corners, upward for the `Bottom*`
//! ones — separated by [`gap`](Toast::gap) blank rows. A reducer therefore
//! `insert(0, …)`s a new toast and trims the tail; the widget never reorders.
//!
//! # Opaque on purpose, like [`Modal`](crate::Modal)
//!
//! A toast floats over unrelated content, so each box
//! [`clears`](rstui_core::Buffer::clear_region) its rectangle before drawing
//! (the same exclusive-ownership reasoning [`Modal`](crate::Modal) documents at
//! `modal.rs:29-38`: a [`Style`] is a patch and cannot reset a colour, so a
//! merely-styled box would let the background bleed through). An empty list
//! draws nothing at all — *no* `clear_region` calls — so it is a true no-op.
//!
//! # Sizing and wrapping reuse the existing vocabulary
//!
//! Box width is a [`Constraint`] resolved with
//! [`Constraint::apply`] (clamped to the overlay, exactly as
//! [`Modal`](crate::Modal) sizes its dialog). Each box's body is rendered
//! through a private [`Paragraph`](crate::Paragraph) with soft
//! [`Wrap`](crate::Wrap), and its height is
//! [`Paragraph::line_count`](crate::Paragraph::line_count) at the inner width —
//! so wrapping and right-edge clipping are *inherited*, never a second wrap
//! algorithm.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! overlay, an empty list, `max_visible == 0`, a width resolving to zero, an
//! overlay shorter than the stack (boxes whose start would fall off-screen are
//! simply not drawn), and a body far wider/taller than its box are all safe
//! clips/no-ops — never a panic.

use rstui_core::{Buffer, Constraint, Line, Rect, Style, Widget};

use crate::{Block, Paragraph, Wrap};

/// Soft-wrap configuration every toast body uses, so
/// [`Paragraph::line_count`] sizing and the rendered box agree exactly.
const TOAST_WRAP: Wrap = Wrap { trim: false };

/// The severity of a [`ToastMessage`], selecting which accent [`Style`] the
/// box is drawn with ([`info_style`](Toast::info_style) …
/// [`error_style`](Toast::error_style)).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    /// Neutral information (the default).
    #[default]
    Info,
    /// A successful, completed action.
    Success,
    /// A non-fatal caution.
    Warning,
    /// A failure the user should notice.
    Error,
}

/// One notification: a [`Line`] body plus its [`ToastLevel`].
///
/// Build one from anything a [`Line`] is built from (it defaults to
/// [`ToastLevel::Info`], mirroring [`ListItem`](crate::ListItem)'s `From`
/// family), or pick the level explicitly with [`ToastMessage::new`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ToastMessage<'a> {
    body: Line<'a>,
    level: ToastLevel,
}

impl<'a> ToastMessage<'a> {
    /// A message of `level` displaying `body` (anything convertible to a
    /// [`Line`]).
    pub fn new(level: ToastLevel, body: impl Into<Line<'a>>) -> Self {
        Self {
            body: body.into(),
            level,
        }
    }

    /// Replaces the body's base [`Style`] (beneath each span's own style),
    /// patched under the box's level accent at render time.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.body = self.body.style(style);
        self
    }
}

impl<'a> From<&'a str> for ToastMessage<'a> {
    fn from(s: &'a str) -> Self {
        Self::new(ToastLevel::Info, s)
    }
}

impl From<String> for ToastMessage<'_> {
    fn from(s: String) -> Self {
        Self::new(ToastLevel::Info, s)
    }
}

impl<'a> From<Line<'a>> for ToastMessage<'a> {
    fn from(line: Line<'a>) -> Self {
        Self::new(ToastLevel::Info, line)
    }
}

/// Which overlay corner the newest toast is anchored flush to.
///
/// Older toasts stack *away* from the corner: downward for the `Top*`
/// variants, upward for the `Bottom*` ones.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ToastCorner {
    /// Top-right (the default), the conventional desktop notification corner.
    #[default]
    TopRight,
    /// Top-left.
    TopLeft,
    /// Bottom-right.
    BottomRight,
    /// Bottom-left.
    BottomLeft,
}

/// A corner-anchored, opaque stack of [`ToastMessage`]s over an overlay area.
///
/// A **pure projection** of a caller-owned `&[ToastMessage]` (see the
/// [module docs](self)): `messages[0]` is the newest and is drawn flush to
/// [`corner`](Self::corner); older entries stack away from it, separated by
/// [`gap`](Self::gap) blank rows, with only [`max_visible`](Self::max_visible)
/// boxes drawn (the reducer owns trimming and expiry). Each box is
/// [`clear`](rstui_core::Buffer::clear_region)ed opaque, then its body is
/// soft-wrapped through a [`Paragraph`] sized by
/// [`width`](Self::width)/[`Paragraph::line_count`] and tinted by the
/// per-[`ToastLevel`] accent style, with an optional framing
/// [`block`](Self::block) around every toast.
///
/// ```
/// use rstui_core::{Buffer, Constraint, Position, Rect, Widget};
/// use rstui_widgets::{Toast, ToastLevel, ToastMessage};
///
/// // The notification list is ordinary caller-owned model state; the reducer
/// // pushes onto it and (later) expires entries — the widget only projects
/// // what is in the list right now (dismissal/timing happens in the reducer).
/// let toasts = [
///     ToastMessage::new(ToastLevel::Error, "Disk full"),
///     ToastMessage::new(ToastLevel::Info, "Saved"),
/// ];
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 4));
/// Toast::new(&toasts)
///     .width(Constraint::Length(9))
///     .render(buf.area(), &mut buf);
///
/// // messages[0] — the newest — is flush to the default top-right corner;
/// // the older "Saved" stacks one gap row below it.
/// assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, 'D'); // "Disk full"
/// assert_eq!(buf.get(Position::new(3, 2)).unwrap().symbol, 'S'); // "Saved"
/// ```
#[derive(Debug, Clone)]
pub struct Toast<'a> {
    messages: &'a [ToastMessage<'a>],
    corner: ToastCorner,
    width: Constraint,
    max_visible: usize,
    gap: u16,
    style: Style,
    info_style: Style,
    success_style: Style,
    warning_style: Style,
    error_style: Style,
    block: Option<Block<'a>>,
}

impl Default for Toast<'_> {
    fn default() -> Self {
        Self {
            messages: &[],
            corner: ToastCorner::TopRight,
            width: Constraint::Length(36),
            max_visible: 5,
            gap: 1,
            style: Style::new(),
            info_style: Style::new(),
            success_style: Style::new(),
            warning_style: Style::new(),
            error_style: Style::new(),
            block: None,
        }
    }
}

impl<'a> Toast<'a> {
    /// A stack projecting `messages` (newest first) with the documented
    /// defaults: top-right corner, [`Length(36)`](Constraint::Length) wide, at
    /// most 5 visible, a 1-row gap, unstyled, no frame.
    #[must_use]
    pub fn new(messages: &'a [ToastMessage<'a>]) -> Self {
        Self {
            messages,
            ..Self::default()
        }
    }

    /// Sets which overlay corner the newest toast anchors flush to.
    #[must_use]
    pub fn corner(mut self, corner: ToastCorner) -> Self {
        self.corner = corner;
        self
    }

    /// Sets the box width within the overlay (default
    /// [`Length(36)`](Constraint::Length)). Resolved with
    /// [`Constraint::apply`], so it never exceeds the overlay width.
    #[must_use]
    pub fn width(mut self, width: Constraint) -> Self {
        self.width = width;
        self
    }

    /// Sets how many toasts are drawn at most; the surplus tail is silently
    /// not rendered (the reducer owns trimming/expiry, exactly as
    /// [`List`](crate::List) clips beyond its height).
    #[must_use]
    pub fn max_visible(mut self, max_visible: usize) -> Self {
        self.max_visible = max_visible;
        self
    }

    /// Sets the number of blank rows between stacked toasts (default `1`).
    #[must_use]
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// Sets the base [`Style`], beneath the per-level accent and the
    /// body's own style cascade.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the accent [`Style`] patched over the base for
    /// [`ToastLevel::Info`] boxes.
    #[must_use]
    pub fn info_style(mut self, style: Style) -> Self {
        self.info_style = style;
        self
    }

    /// Sets the accent [`Style`] patched over the base for
    /// [`ToastLevel::Success`] boxes.
    #[must_use]
    pub fn success_style(mut self, style: Style) -> Self {
        self.success_style = style;
        self
    }

    /// Sets the accent [`Style`] patched over the base for
    /// [`ToastLevel::Warning`] boxes.
    #[must_use]
    pub fn warning_style(mut self, style: Style) -> Self {
        self.warning_style = style;
        self
    }

    /// Sets the accent [`Style`] patched over the base for
    /// [`ToastLevel::Error`] boxes.
    #[must_use]
    pub fn error_style(mut self, style: Style) -> Self {
        self.error_style = style;
        self
    }

    /// Frames every toast in `block`; the body renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// The accent [`Style`] for `level`, patched over the base at render time.
    fn accent(&self, level: ToastLevel) -> Style {
        match level {
            ToastLevel::Info => self.info_style,
            ToastLevel::Success => self.success_style,
            ToastLevel::Warning => self.warning_style,
            ToastLevel::Error => self.error_style,
        }
    }
}

impl Widget for Toast<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Empty overlay / empty list / nothing visible: a true no-op — no
        // box is cleared, so a background underneath stays intact.
        if area.is_empty() || self.messages.is_empty() || self.max_visible == 0 {
            return;
        }

        // Box width, clamped to the overlay (Modal's Constraint idiom). A
        // width resolving to zero leaves nothing to draw.
        let box_w = self.width.apply(area.width);
        if box_w == 0 {
            return;
        }

        // Horizontal anchor: flush to the corner's side.
        let x = match self.corner {
            ToastCorner::TopLeft | ToastCorner::BottomLeft => area.left(),
            ToastCorner::TopRight | ToastCorner::BottomRight => area.right().saturating_sub(box_w),
        };
        let top_anchored = matches!(self.corner, ToastCorner::TopLeft | ToastCorner::TopRight);

        // The optional frame contributes a fixed inner width and vertical
        // frame-row count, derived (like Block::inner everywhere) by probing.
        let (inner_w, frame_v) = match &self.block {
            Some(b) => {
                let probe = b.inner(Rect::new(area.left(), area.top(), box_w, area.height));
                (probe.width, area.height.saturating_sub(probe.height))
            }
            None => (box_w, 0),
        };

        // `cursor` is the next free edge: the top row for Top* corners, the
        // exclusive bottom row for Bottom*. The newest box sits flush to the
        // corner; each subsequent (older) box steps `gap` rows away.
        let mut cursor = if top_anchored {
            area.top()
        } else {
            area.bottom()
        };

        for message in self.messages.iter().take(self.max_visible) {
            // Height = wrapped line count at the inner width (+ frame rows),
            // clamped into the overlay — reusing Paragraph's wrap, never a
            // second algorithm.
            let wrapped = Paragraph::new(message.body.clone())
                .wrap(TOAST_WRAP)
                .line_count(inner_w);
            let lines = u16::try_from(wrapped).unwrap_or(u16::MAX);
            let box_h = lines.saturating_add(frame_v).min(area.height).max(1);

            // A box whose start would fall outside the overlay is simply not
            // drawn (and neither is anything older behind it).
            let start_y = if top_anchored {
                if cursor >= area.bottom() {
                    break;
                }
                cursor
            } else {
                match cursor.checked_sub(box_h) {
                    Some(s) if s >= area.top() => s,
                    _ => break,
                }
            };

            let box_rect = Rect::new(x, start_y, box_w, box_h).intersection(area);
            if box_rect.is_empty() {
                break;
            }

            // Opaque box (Modal's clear_region affordance), then the
            // accent-tinted, soft-wrapped body through Paragraph.
            buf.clear_region(box_rect);
            let mut paragraph = Paragraph::new(message.body.clone())
                .wrap(TOAST_WRAP)
                .style(self.style.patch(self.accent(message.level)));
            if let Some(b) = &self.block {
                paragraph = paragraph.block(b.clone());
            }
            paragraph.render(box_rect, buf);

            // Step away from the corner, leaving `gap` blank rows.
            if top_anchored {
                cursor = start_y.saturating_add(box_h).saturating_add(self.gap);
            } else {
                cursor = start_y.saturating_sub(self.gap);
                if cursor <= area.top() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Position};

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
    fn newest_message_is_flush_to_the_top_right_corner_by_default() {
        // Default corner is TopRight: messages[0] ("ab") is the newest and
        // hugs the right edge at the top row; the older "cd" stacks one
        // default gap row below it.
        let toasts = [ToastMessage::from("ab"), ToastMessage::from("cd")];
        assert_eq!(
            lines(Toast::new(&toasts).width(Constraint::Length(3)), 6, 4),
            "   ab \n      \n   cd \n      \n"
        );
    }

    #[test]
    fn each_corner_anchors_and_stacks_in_the_correct_direction() {
        let toasts = [ToastMessage::from("a"), ToastMessage::from("b")];
        let at = |corner| {
            lines(
                Toast::new(&toasts)
                    .width(Constraint::Length(2))
                    .gap(0)
                    .corner(corner),
                4,
                4,
            )
        };
        // Newest flush to the corner; older steps away (down for Top*, up
        // for Bottom*).
        assert_eq!(at(ToastCorner::TopLeft), "a   \nb   \n    \n    \n");
        assert_eq!(at(ToastCorner::TopRight), "  a \n  b \n    \n    \n");
        assert_eq!(at(ToastCorner::BottomLeft), "    \n    \nb   \na   \n");
        assert_eq!(at(ToastCorner::BottomRight), "    \n    \n  b \n  a \n");
    }

    #[test]
    fn level_selects_the_accent_style() {
        let toasts = [
            ToastMessage::new(ToastLevel::Info, "i"),
            ToastMessage::new(ToastLevel::Success, "s"),
            ToastMessage::new(ToastLevel::Warning, "w"),
            ToastMessage::new(ToastLevel::Error, "e"),
        ];
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 4));
        Toast::new(&toasts)
            .corner(ToastCorner::TopLeft)
            .width(Constraint::Length(1))
            .gap(0)
            .info_style(Style::new().bg(Color::Blue))
            .success_style(Style::new().bg(Color::Green))
            .warning_style(Style::new().bg(Color::Yellow))
            .error_style(Style::new().bg(Color::Red))
            .render(buf.area(), &mut buf);

        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Blue);
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().bg, Color::Green);
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().bg, Color::Yellow);
        assert_eq!(buf.get(Position::new(0, 3)).unwrap().bg, Color::Red);
    }

    #[test]
    fn gap_inserts_blank_rows_between_stacked_toasts() {
        // gap(2): two blank rows separate the newest from the next.
        let toasts = [ToastMessage::from("a"), ToastMessage::from("b")];
        assert_eq!(
            lines(
                Toast::new(&toasts)
                    .corner(ToastCorner::TopLeft)
                    .width(Constraint::Length(1))
                    .gap(2),
                1,
                5,
            ),
            "a\n \n \nb\n \n"
        );
    }

    #[test]
    fn max_visible_caps_the_rows_and_the_reducer_owns_the_rest() {
        let toasts = [
            ToastMessage::from("a"),
            ToastMessage::from("b"),
            ToastMessage::from("c"),
            ToastMessage::from("d"),
        ];
        // Only the first two are drawn; trimming the rest is the reducer's
        // job, not the widget's.
        assert_eq!(
            lines(
                Toast::new(&toasts)
                    .corner(ToastCorner::TopLeft)
                    .width(Constraint::Length(1))
                    .gap(0)
                    .max_visible(2),
                1,
                6,
            ),
            "a\nb\n \n \n \n \n"
        );
        // max_visible(0) draws nothing at all.
        assert_eq!(
            lines(
                Toast::new(&toasts)
                    .corner(ToastCorner::TopLeft)
                    .width(Constraint::Length(1))
                    .max_visible(0),
                1,
                4,
            ),
            " \n \n \n \n"
        );
    }

    #[test]
    fn a_long_body_wraps_within_the_box_width() {
        // "the quick" soft-wraps to width 4; the box height is the wrapped
        // line count (Paragraph's wrap, reused — not a second algorithm).
        let toasts = [ToastMessage::from("the quick")];
        assert_eq!(
            lines(Toast::new(&toasts).width(Constraint::Length(4)), 4, 3),
            "the \nquic\nk   \n"
        );
    }

    #[test]
    fn each_toast_box_is_opaque_background_does_not_bleed_through() {
        // A '.' background the toast must not let through.
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        let bg = Style::new().fg(Color::Red).bg(Color::Blue);
        for p in buf.area().positions() {
            buf.set_cell(p, '.', bg);
        }

        let toasts = [ToastMessage::from("x")];
        Toast::new(&toasts)
            .corner(ToastCorner::TopLeft)
            .width(Constraint::Length(3))
            .render(buf.area(), &mut buf);

        // The box (cols 0..3, row 0) is cleared opaque: 'x', then blanks with
        // no '.' and no leftover Blue background bleeding through.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'x');
        let blank = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(blank.symbol, ' ');
        assert_eq!(blank.bg, Color::Reset);
        // Outside the box the background is untouched.
        let kept = buf.get(Position::new(5, 2)).unwrap();
        assert_eq!(kept.symbol, '.');
        assert_eq!(kept.bg, Color::Blue);
    }

    #[test]
    fn an_optional_block_frames_each_toast() {
        // Bordered: inner width 2 fits "hi"; box height = 1 wrapped row + 2
        // frame rows.
        let toasts = [ToastMessage::from("hi")];
        assert_eq!(
            lines(
                Toast::new(&toasts)
                    .corner(ToastCorner::TopLeft)
                    .width(Constraint::Length(4))
                    .block(Block::bordered()),
                4,
                3,
            ),
            "┌──┐\n│hi│\n└──┘\n"
        );
    }

    #[test]
    fn an_empty_message_list_is_a_total_no_op() {
        // No messages: not even a clear_region — the background is intact.
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 3));
        for p in buf.area().positions() {
            buf.set_cell(p, '.', Style::new());
        }
        Toast::new(&[])
            .style(Style::new().bg(Color::Red))
            .render(buf.area(), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == '.'));
    }

    #[test]
    fn a_box_wider_than_the_overlay_is_clamped() {
        // Length(999) clamps to the 5-wide overlay; no panic, no overflow.
        let toasts = [ToastMessage::from("abc")];
        assert_eq!(
            lines(Toast::new(&toasts).width(Constraint::Length(999)), 5, 2),
            "abc  \n     \n"
        );
    }

    #[test]
    fn zero_overlay_area_is_a_total_no_op() {
        let toasts = [ToastMessage::from("hello")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        Toast::new(&toasts)
            .style(Style::new().bg(Color::Red))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
