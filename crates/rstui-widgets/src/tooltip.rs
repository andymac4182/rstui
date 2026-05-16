//! [`Tooltip`] — a small, **opaque** popup anchored beside a caller-given
//! anchor [`Rect`], flipping side when it would overflow the buffer.
//!
//! # A pure projection of caller-owned text + anchor
//!
//! Like every rstui widget `Tooltip` is a **pure projection**: it renders the
//! caller-owned [`Text`] it is handed beside the caller-given
//! [`anchor`](Tooltip::new) rect, and reads nothing else. *Whether* a tooltip
//! is shown, and *which* anchor it hangs off (the hovered/focused control's
//! rect), are ordinary application state the reducer owns — the widget never
//! decides visibility, exactly as [`Modal`](crate::Modal) never decides a
//! dialog is open. The popup is **opaque** for the same reason
//! ([`clear_region`](rstui_core::Buffer::clear_region) — `modal.rs:29-38`: a
//! [`Style`] is a patch and cannot reset a cell), so it can float over
//! unrelated content.
//!
//! # Placement: the [`Select`](crate::Select) flip pattern, as a pure accessor
//!
//! [`placement`](Tooltip::placement) is a pure function of the anchor and the
//! buffer area: the popup is sized to its text (plus any [`Block`] frame),
//! anchored directly **below** the anchor, **flipped above** when the space
//! below is short, and clamped to the larger gap when it fits neither way —
//! the exact reasoning [`Select`](crate::Select)'s panel placement documents,
//! and the same "derived geometry is itself a projection" rule
//! [`Modal::area`](crate::Modal::area) follows. Horizontally it left-aligns
//! with the anchor, shifting left only as far as needed to stay on screen.
//! `render` calls the very same accessor, so the exposed rect and the drawn
//! one can never disagree.
//!
//! # Composition, not new glyph-stamping
//!
//! The body is drawn by **reusing [`Paragraph`]** (clipped,
//! no wrap) inside the optional framing [`Block`], so multi-line text,
//! right-edge clipping, and the frame are inherited rather than
//! re-implemented.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! buffer, an empty text, an anchor off-screen, and a popup that fits nowhere
//! (it is clamped to the larger gap and the body simply clips) are all safe
//! no-ops/clips — never a panic. A caller-chosen preferred side and an arrow
//! tail are deliberately deferred additives, not smuggled into this slice.

use rstui_core::{Buffer, Rect, Style, Text, Widget};

use crate::block::Block;
use crate::paragraph::Paragraph;

/// A small opaque popup anchored beside a caller-given [`Rect`] — a pure
/// projection of caller-owned text + anchor.
///
/// Sized to its [`Text`] (plus the optional
/// [`block`](Self::block) frame), [`clear`](rstui_core::Buffer::clear_region)ed
/// opaque, and placed by [`placement`](Self::placement) (below the anchor,
/// flipped above when short). The body is composed through
/// [`Paragraph`].
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Tooltip;
///
/// // The anchor is the hovered control's rect — plain caller-owned state; the
/// // reducer decides *whether* the tip is shown, never the widget.
/// let anchor = Rect::new(2, 0, 4, 1);
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 4));
/// Tooltip::new("hint").render_anchored(anchor, &mut buf);
///
/// // Sized to "hint" and dropped directly below the anchor.
/// assert_eq!(
///     Tooltip::new("hint").placement(anchor, buf.area()),
///     Rect::new(2, 1, 4, 1),
/// );
/// assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, 'h');
/// ```
#[derive(Debug, Clone)]
pub struct Tooltip<'a> {
    text: Text<'a>,
    block: Option<Block<'a>>,
    style: Style,
}

impl<'a> Tooltip<'a> {
    /// A tooltip displaying `text` (anything convertible to a
    /// [`Text`]), unframed and unstyled.
    pub fn new(text: impl Into<Text<'a>>) -> Self {
        Self {
            text: text.into(),
            block: None,
            style: Style::new(),
        }
    }

    /// Frames the popup in `block`; the body renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the popup so it reads as one
    /// solid box (the opaque clear makes it opaque regardless; this colours
    /// it).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// A [`Block`]'s constant frame overhead `(horizontal, vertical)` —
    /// border + padding columns/rows. [`Block::inner`] is pure arithmetic, so
    /// a max-sized probe measures the overhead exactly (the
    /// [`Select`](crate::Select) probe idiom).
    fn frame_overhead(&self) -> (u16, u16) {
        self.block.as_ref().map_or((0, 0), |b| {
            let probe = Rect::new(0, 0, u16::MAX, u16::MAX);
            let inner = b.inner(probe);
            (
                u16::MAX.saturating_sub(inner.width),
                u16::MAX.saturating_sub(inner.height),
            )
        })
    }

    /// The popup rect for `anchor` within the buffer `area`.
    ///
    /// A pure function: the popup is sized to its text plus the frame, clamped
    /// to `area`; placed flush below the anchor, flipped above when the space
    /// below is short, and clamped to the larger gap when it fits neither way
    /// (the [`Select`](crate::Select) flip pattern). Horizontally it
    /// left-aligns with the anchor, shifted left only enough to stay on
    /// screen. An empty `area` or a zero-sized popup yields
    /// [`Rect::ZERO`](rstui_core::Rect::ZERO).
    #[must_use]
    pub fn placement(&self, anchor: Rect, area: Rect) -> Rect {
        if area.is_empty() {
            return Rect::ZERO;
        }

        let (fw, fh) = self.frame_overhead();
        let want_w = (self.text.width() as u16)
            .saturating_add(fw)
            .min(area.width)
            .max(1);
        let want_h = (self.text.height() as u16)
            .saturating_add(fh)
            .min(area.height)
            .max(1);

        // Vertical: prefer flush below the anchor; flip flush above when the
        // space below is short; clamp to the larger gap otherwise.
        let gap_below = area.bottom().saturating_sub(anchor.bottom());
        let gap_above = anchor.top().saturating_sub(area.top());
        let (y, h) = if want_h <= gap_below {
            (anchor.bottom(), want_h)
        } else if want_h <= gap_above {
            (anchor.top().saturating_sub(want_h), want_h)
        } else if gap_below >= gap_above {
            (anchor.bottom(), gap_below)
        } else {
            (anchor.top().saturating_sub(gap_above), gap_above)
        };
        if h == 0 {
            return Rect::ZERO;
        }
        // Totality clamp: an off-screen anchor can drive the preferred side
        // off-buffer; pin the popup fully inside `area` regardless (the gap
        // branches are already in-bounds for an on-screen anchor, so this is
        // a no-op there).
        let y = y.clamp(area.top(), area.bottom().saturating_sub(h));

        // Horizontal: left-align with the anchor, then shift left just enough
        // to keep the whole popup on screen (clamped at the left edge).
        let max_x = area.right().saturating_sub(want_w);
        let x = anchor.left().min(max_x).max(area.left());

        Rect::new(x, y, want_w, h)
    }

    /// Renders the popup anchored beside `anchor` into `buf`.
    ///
    /// A thin convenience over [`Widget::render`] that names the anchor
    /// explicitly; [`render`](Widget::render) takes the whole buffer area as
    /// the anchor (so a tooltip rendered straight at `frame.area()` drops at
    /// the origin).
    pub fn render_anchored(self, anchor: Rect, buf: &mut Buffer) {
        let rect = self.placement(anchor, buf.area());
        if rect.is_empty() {
            return;
        }

        // Opaque: take exclusive ownership of the cells (the Modal
        // `clear_region` affordance), then compose the body through
        // `Paragraph` inside the optional frame.
        buf.clear_region(rect);
        let mut paragraph = Paragraph::new(self.text).style(self.style);
        if let Some(block) = self.block {
            paragraph = paragraph.block(block);
        }
        paragraph.render(rect, buf);
    }
}

impl Widget for Tooltip<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_anchored(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Position};

    /// Renders `widget` anchored at `anchor` into a fresh `width`×`height`
    /// buffer and returns the glyphs as one newline-terminated line per row.
    fn lines(widget: Tooltip<'_>, anchor: Rect, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render_anchored(anchor, &mut buf);
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    /// Fills `buf` with a styled `.` background so a clear is observable.
    fn background(buf: &mut Buffer) {
        let style = Style::new().fg(Color::Red).bg(Color::Blue);
        for p in buf.area().positions() {
            buf.set_cell(p, '.', style);
        }
    }

    #[test]
    fn drops_directly_below_the_anchor_sized_to_the_text() {
        let anchor = Rect::new(1, 1, 3, 1);
        assert_eq!(
            Tooltip::new("hi").placement(anchor, Rect::new(0, 0, 10, 5)),
            Rect::new(1, 2, 2, 1)
        );
        assert_eq!(
            lines(Tooltip::new("hi"), anchor, 6, 4),
            "      \n      \n hi   \n      \n"
        );
    }

    #[test]
    fn flips_above_when_there_is_no_room_below() {
        // Anchor on the last row: nothing fits below, so it flips above it.
        let anchor = Rect::new(0, 3, 2, 1);
        assert_eq!(
            Tooltip::new("ab").placement(anchor, Rect::new(0, 0, 4, 4)),
            Rect::new(0, 2, 2, 1)
        );
    }

    #[test]
    fn shifts_left_to_stay_on_screen() {
        // Anchor near the right edge; the 4-wide popup shifts left so it does
        // not overflow the 6-wide buffer.
        let anchor = Rect::new(5, 0, 1, 1);
        assert_eq!(
            Tooltip::new("wide").placement(anchor, Rect::new(0, 0, 6, 3)),
            Rect::new(2, 1, 4, 1)
        );
    }

    #[test]
    fn clamps_to_the_larger_gap_when_it_fits_neither_way() {
        // 3-line text, anchor mid-buffer: 1 row below, 2 above — neither
        // enough, so clamp to the larger (above) gap of 2 rows.
        let text = Text::from("a\nb\nc");
        let anchor = Rect::new(0, 2, 1, 1);
        assert_eq!(
            Tooltip::new(text).placement(anchor, Rect::new(0, 0, 3, 4)),
            Rect::new(0, 0, 1, 2)
        );
    }

    #[test]
    fn the_popup_is_opaque_so_the_background_does_not_bleed_through() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        background(&mut buf);
        Tooltip::new("x").render_anchored(Rect::new(0, 0, 1, 1), &mut buf);
        // Popup is a 1x1 box at (0,1): 'x', opaque (no '.' / Blue bg).
        let cell = buf.get(Position::new(0, 1)).unwrap();
        assert_eq!(cell.symbol, 'x');
        assert_eq!(cell.bg, Color::Reset);
        // Elsewhere the background survives.
        assert_eq!(buf.get(Position::new(5, 2)).unwrap().symbol, '.');
    }

    #[test]
    fn a_block_frames_the_popup_and_grows_it_to_fit() {
        let anchor = Rect::new(0, 0, 1, 1);
        assert_eq!(
            lines(Tooltip::new("ok").block(Block::bordered()), anchor, 5, 4),
            "     \n┌──┐ \n│ok│ \n└──┘ \n"
        );
    }

    #[test]
    fn style_fills_the_popup_box() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        Tooltip::new("z")
            .style(Style::new().bg(Color::Green))
            .render_anchored(Rect::new(0, 0, 1, 1), &mut buf);
        let cell = buf.get(Position::new(0, 1)).unwrap();
        assert_eq!(cell.symbol, 'z');
        assert_eq!(cell.bg, Color::Green);
    }

    #[test]
    fn a_multi_line_text_sizes_to_the_widest_line() {
        let anchor = Rect::new(0, 0, 1, 1);
        assert_eq!(
            lines(Tooltip::new("a\nbbb"), anchor, 5, 4),
            "     \na    \nbbb  \n     \n"
        );
    }

    #[test]
    fn a_text_wider_than_the_buffer_is_clamped_and_clipped() {
        let anchor = Rect::new(0, 0, 1, 1);
        // "abcdef" cannot fit a 3-wide buffer: clamped to width 3, clipped.
        assert_eq!(
            Tooltip::new("abcdef").placement(anchor, Rect::new(0, 0, 3, 3)),
            Rect::new(0, 1, 3, 1)
        );
        assert_eq!(lines(Tooltip::new("abcdef"), anchor, 3, 2), "   \nabc\n");
    }

    #[test]
    fn an_off_screen_anchor_below_clamps_into_the_buffer() {
        // Anchor entirely past the bottom: no gap below, so it flips/clamps
        // above without panicking.
        let anchor = Rect::new(0, 9, 2, 1);
        let rect = Tooltip::new("hi").placement(anchor, Rect::new(0, 0, 4, 4));
        assert!(rect.right() <= 4 && rect.bottom() <= 4);
    }

    #[test]
    fn render_uses_the_whole_area_as_the_anchor() {
        // The bare `Widget::render` anchors off the area it is given.
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
        Tooltip::new("hi").render(Rect::new(0, 0, 6, 1), &mut buf);
        // Area bottom is row 1, so the tip drops there.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'h');
    }

    #[test]
    fn zero_buffer_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 0, 0));
        Tooltip::new("hi").render_anchored(Rect::new(0, 0, 1, 1), &mut buf);
        assert_eq!(
            Tooltip::new("hi").placement(Rect::new(0, 0, 1, 1), Rect::ZERO),
            Rect::ZERO
        );
    }
}
