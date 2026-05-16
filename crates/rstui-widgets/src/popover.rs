//! [`Popover`] — the generic anchored, **opaque** floating panel that
//! [`Tooltip`](crate::Tooltip), [`Menu`](crate::Menu), and
//! [`Select`](crate::Select)'s dropdown are all specializations of.
//!
//! # The shared shape behind every floating widget
//!
//! [`Tooltip`](crate::Tooltip) is "a `Popover` whose content is a
//! [`Paragraph`](crate::Paragraph)"; [`Select`](crate::Select)'s open list is
//! "a `Popover` whose content is a [`List`](crate::List)";
//! [`Menu`](crate::Menu) is the same with action rows. Each of those slices
//! re-derived the *same* placement-and-opacity machinery. `Popover` factors
//! exactly that machinery out and leaves the **content to the caller**: it
//! anchors a caller-sized panel to a caller-given anchor [`Rect`], clears it
//! opaque, draws an optional frame, and hands back the content rect — the
//! render-then-fill-`inner` pattern [`Modal`](crate::Modal) uses, but anchored
//! instead of centred.
//!
//! # A pure projection of caller-owned anchor + size
//!
//! Like every rstui widget `Popover` is a **pure projection**: it reads only
//! the [`width`](Popover::width)/[`height`](Popover::height) and the anchor
//! [`Rect`] it is handed (the hovered/focused control's rect — ordinary
//! application state). *Whether* a popover is shown is the reducer's job, never
//! the widget's, exactly as [`Modal`](crate::Modal) never decides a dialog is
//! open.
//!
//! # Opaque on purpose, like [`Modal`](crate::Modal)
//!
//! A popover floats over unrelated content, so it is **opaque**: it
//! [`clear_region`](rstui_core::Buffer::clear_region)s its rect before drawing
//! (the exclusive-ownership reasoning [`Modal`](crate::Modal) documents at
//! `modal.rs:24-38`: a [`Style`] is a patch and cannot reset a cell, so a
//! merely-styled box would let the background bleed through).
//!
//! # Placement: the [`Tooltip`](crate::Tooltip) flip, generalised to four sides
//!
//! [`placement`](Popover::placement) is a pure function of the anchor and the
//! buffer: the panel opens on the preferred [`PopoverSide`], **flips to the
//! opposite side** when the preferred one has no room, and is finally **clamped
//! fully inside the buffer** when it fits on neither side or the anchor is
//! off-screen — the [`Tooltip`](crate::Tooltip) flip rule widened from
//! below/above to all four sides, and the same "derived geometry is itself a
//! projection" rule [`Modal::area`](crate::Modal::area) follows. `render` calls
//! the very same accessor, so the exposed rect and the drawn one can never
//! disagree.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! buffer, a zero [`width`](Popover::width)/[`height`](Popover::height), and an
//! anchor entirely off-screen are all safe no-ops/clamps — never a panic.

use rstui_core::{Buffer, Rect, Style, Widget};

use crate::block::Block;

/// Which side of the anchor a [`Popover`] prefers to open on.
///
/// The panel flips to the opposite side when the preferred one has no room
/// (the [`Tooltip`](crate::Tooltip) flip rule, widened to four sides).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PopoverSide {
    /// Below the anchor (the default) — the dropdown/tooltip direction.
    #[default]
    Bottom,
    /// Above the anchor.
    Top,
    /// To the right of the anchor.
    Right,
    /// To the left of the anchor.
    Left,
}

/// A generic anchored, opaque floating panel — a pure projection of a
/// caller-owned anchor [`Rect`] plus a caller-chosen size.
///
/// Sized by [`width`](Self::width)/[`height`](Self::height),
/// [`clear`](rstui_core::Buffer::clear_region)ed opaque, placed by
/// [`placement`](Self::placement) (on the preferred [`PopoverSide`], flipped
/// and clamped to stay on the buffer). The caller draws the panel's content
/// into [`inner`](Self::inner), the same render-then-fill-`inner` pattern
/// [`Modal`](crate::Modal) and [`Block`] use.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Block, Popover};
///
/// // The anchor is the hovered control's rect — plain caller-owned state; the
/// // reducer decides *whether* the popover is shown, never the widget.
/// let anchor = Rect::new(2, 1, 4, 1);
/// let mut buf = Buffer::empty(Rect::new(0, 0, 14, 8));
/// let popover = Popover::new().width(6).height(3).block(Block::bordered());
///
/// // `placement`/`inner` are pure functions of the anchor + buffer.
/// assert_eq!(popover.placement(anchor, buf.area()), Rect::new(2, 2, 6, 3));
/// let inner = popover.inner(anchor, buf.area()); // content rect (inside the frame)
///
/// popover.render_anchored(anchor, &mut buf);
/// "hi".render(inner, &mut buf);
/// assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, '┌'); // frame
/// assert_eq!(buf.get(Position::new(3, 3)).unwrap().symbol, 'h'); // content
/// ```
#[derive(Debug, Clone)]
pub struct Popover<'a> {
    width: u16,
    height: u16,
    side: PopoverSide,
    block: Option<Block<'a>>,
    style: Style,
}

impl<'a> Popover<'a> {
    /// A popover with no size yet (a degenerate no-op until
    /// [`width`](Self::width)/[`height`](Self::height) are set), opening
    /// [`Bottom`](PopoverSide::Bottom), unframed and unstyled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            side: PopoverSide::Bottom,
            block: None,
            style: Style::new(),
        }
    }

    /// Sets the panel width (including any [`block`](Self::block) frame). A
    /// width wider than the buffer is clamped; `0` makes the popover a no-op.
    #[must_use]
    pub fn width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    /// Sets the panel height (including any [`block`](Self::block) frame). A
    /// height taller than the buffer is clamped; `0` makes the popover a no-op.
    #[must_use]
    pub fn height(mut self, height: u16) -> Self {
        self.height = height;
        self
    }

    /// Sets the preferred [`PopoverSide`] (default
    /// [`Bottom`](PopoverSide::Bottom)).
    #[must_use]
    pub fn side(mut self, side: PopoverSide) -> Self {
        self.side = side;
        self
    }

    /// Frames the panel in `block`; content renders into
    /// [`inner`](Self::inner), the same compose pattern [`Block`] uses.
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the (already-cleared) panel so it
    /// reads as one solid box (the opaque clear makes it opaque regardless;
    /// this colours it).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The panel rect for `anchor` within the buffer `area`.
    ///
    /// A pure function: the panel is sized to
    /// [`width`](Self::width)/[`height`](Self::height) clamped to `area`,
    /// placed on the preferred [`PopoverSide`], flipped to the opposite side
    /// when the preferred one has no room, then clamped fully inside `area` so
    /// an off-screen anchor (or a panel that fits neither way) never goes
    /// off-buffer. A zero size or empty `area` yields
    /// [`Rect::ZERO`](rstui_core::Rect::ZERO).
    #[must_use]
    pub fn placement(&self, anchor: Rect, area: Rect) -> Rect {
        if area.is_empty() || self.width == 0 || self.height == 0 {
            return Rect::ZERO;
        }

        // Oversize shrinks rather than overflowing (the totality clamp).
        let w = self.width.min(area.width);
        let h = self.height.min(area.height);

        let (x, y) = match self.side {
            PopoverSide::Bottom | PopoverSide::Top => {
                let below = anchor.bottom();
                let above = anchor.top().saturating_sub(h);
                let fits_below = h <= area.bottom().saturating_sub(anchor.bottom());
                let fits_above = h <= anchor.top().saturating_sub(area.top());
                let y = if matches!(self.side, PopoverSide::Bottom) {
                    if fits_below || !fits_above {
                        below
                    } else {
                        above
                    }
                } else if fits_above || !fits_below {
                    above
                } else {
                    below
                };
                // Cross axis: left-align with the anchor.
                (anchor.left(), y)
            }
            PopoverSide::Right | PopoverSide::Left => {
                let right = anchor.right();
                let left = anchor.left().saturating_sub(w);
                let fits_right = w <= area.right().saturating_sub(anchor.right());
                let fits_left = w <= anchor.left().saturating_sub(area.left());
                let x = if matches!(self.side, PopoverSide::Right) {
                    if fits_right || !fits_left {
                        right
                    } else {
                        left
                    }
                } else if fits_left || !fits_right {
                    left
                } else {
                    right
                };
                // Cross axis: top-align with the anchor.
                (x, anchor.top())
            }
        };

        // Totality clamp: pin the panel fully inside `area` regardless of an
        // off-buffer anchor or a side that fits nowhere. `w <= area.width` and
        // `h <= area.height`, so the upper bound is never below the lower one.
        let x = x.min(area.right().saturating_sub(w)).max(area.left());
        let y = y.min(area.bottom().saturating_sub(h)).max(area.top());
        Rect::new(x, y, w, h)
    }

    /// The content rect inside the panel: [`placement`](Self::placement) minus
    /// the framing [`block`](Self::block) (or the whole panel when there is no
    /// block). Render the popover's content here, exactly as with
    /// [`Block::inner`].
    #[must_use]
    pub fn inner(&self, anchor: Rect, area: Rect) -> Rect {
        let panel = self.placement(anchor, area);
        match &self.block {
            Some(b) => b.inner(panel),
            None => panel,
        }
    }

    /// Renders the panel anchored beside `anchor` into `buf`.
    ///
    /// A thin convenience over [`Widget::render`] that names the anchor
    /// explicitly; [`render`](Widget::render) takes the whole buffer area as
    /// the anchor (so a popover rendered straight at `frame.area()` anchors off
    /// the origin).
    pub fn render_anchored(self, anchor: Rect, buf: &mut Buffer) {
        let rect = self.placement(anchor, buf.area());
        if rect.is_empty() {
            return;
        }
        // Opaque: take exclusive ownership of the cells (the Modal
        // `clear_region` affordance), colour the box, then draw the frame; the
        // caller draws content into `inner`.
        buf.clear_region(rect);
        buf.set_style(rect, self.style);
        if let Some(block) = self.block {
            block.render(rect, buf);
        }
    }
}

impl Default for Popover<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Popover<'_> {
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
    fn lines(widget: Popover<'_>, anchor: Rect, width: u16, height: u16) -> String {
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
    fn defaults_drop_below_the_anchor_sized_to_width_and_height() {
        let anchor = Rect::new(1, 1, 3, 1);
        assert_eq!(
            Popover::new()
                .width(4)
                .height(2)
                .placement(anchor, Rect::new(0, 0, 10, 6)),
            Rect::new(1, 2, 4, 2)
        );
    }

    #[test]
    fn each_side_anchors_on_the_requested_side() {
        // Anchor mid-buffer with room on every side.
        let anchor = Rect::new(5, 5, 2, 2);
        let area = Rect::new(0, 0, 14, 14);
        let p = |side| {
            Popover::new()
                .width(3)
                .height(3)
                .side(side)
                .placement(anchor, area)
        };
        assert_eq!(p(PopoverSide::Bottom), Rect::new(5, 7, 3, 3));
        assert_eq!(p(PopoverSide::Top), Rect::new(5, 2, 3, 3));
        assert_eq!(p(PopoverSide::Right), Rect::new(7, 5, 3, 3));
        assert_eq!(p(PopoverSide::Left), Rect::new(2, 5, 3, 3));
    }

    #[test]
    fn flips_to_the_opposite_side_when_the_preferred_one_has_no_room() {
        // Anchor on the last row: nothing fits below, so Bottom flips above.
        let anchor = Rect::new(0, 3, 2, 1);
        assert_eq!(
            Popover::new()
                .width(2)
                .height(2)
                .placement(anchor, Rect::new(0, 0, 4, 4)),
            Rect::new(0, 1, 2, 2)
        );
        // Anchor flush to the right edge: Right flips to the left of it.
        let anchor = Rect::new(6, 0, 2, 1);
        assert_eq!(
            Popover::new()
                .width(3)
                .height(1)
                .side(PopoverSide::Right)
                .placement(anchor, Rect::new(0, 0, 8, 4)),
            Rect::new(3, 0, 3, 1)
        );
    }

    #[test]
    fn clamps_fully_into_the_buffer_when_it_fits_neither_way() {
        // 4-tall panel, anchor mid-buffer: 1 row below, 1 above — neither
        // enough; the panel is clamped fully inside the 4-tall buffer.
        let anchor = Rect::new(0, 2, 1, 1);
        let rect = Popover::new()
            .width(1)
            .height(4)
            .placement(anchor, Rect::new(0, 0, 3, 4));
        assert_eq!(rect, Rect::new(0, 0, 1, 4));
    }

    #[test]
    fn an_off_buffer_anchor_is_clamped_inside_without_panicking() {
        let anchor = Rect::new(40, 40, 2, 1);
        let rect = Popover::new()
            .width(3)
            .height(2)
            .placement(anchor, Rect::new(0, 0, 6, 5));
        assert!(rect.right() <= 6 && rect.bottom() <= 5);
        assert_eq!(rect, Rect::new(3, 3, 3, 2));
    }

    #[test]
    fn the_panel_is_opaque_so_the_background_does_not_bleed_through() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        background(&mut buf);
        Popover::new()
            .width(2)
            .height(1)
            .render_anchored(Rect::new(0, 0, 1, 1), &mut buf);
        // Panel is a 2x1 box at (0,1): cleared (EMPTY), no '.' / Blue bleed.
        let cell = buf.get(Position::new(0, 1)).unwrap();
        assert_eq!(cell.symbol, ' ');
        assert_eq!(cell.bg, Color::Reset);
        // Elsewhere the background survives.
        assert_eq!(buf.get(Position::new(5, 2)).unwrap().symbol, '.');
    }

    #[test]
    fn style_fills_the_panel_box() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        Popover::new()
            .width(2)
            .height(1)
            .style(Style::new().bg(Color::Green))
            .render_anchored(Rect::new(0, 0, 1, 1), &mut buf);
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().bg, Color::Green);
    }

    #[test]
    fn a_block_frames_the_panel_and_inner_subtracts_the_frame() {
        let anchor = Rect::new(0, 0, 1, 1);
        assert_eq!(
            lines(
                Popover::new().width(4).height(3).block(Block::bordered()),
                anchor,
                5,
                4
            ),
            "     \n┌──┐ \n│  │ \n└──┘ \n"
        );
        let popover = Popover::new().width(4).height(3).block(Block::bordered());
        let area = Rect::new(0, 0, 5, 4);
        assert_eq!(popover.inner(anchor, area), Rect::new(1, 2, 2, 1));
    }

    #[test]
    fn inner_is_the_panel_when_there_is_no_block() {
        let anchor = Rect::new(2, 2, 1, 1);
        let popover = Popover::new().width(3).height(2);
        let area = Rect::new(0, 0, 10, 10);
        assert_eq!(popover.inner(anchor, area), popover.placement(anchor, area));
    }

    #[test]
    fn a_zero_size_popover_is_a_total_no_op() {
        let anchor = Rect::new(0, 0, 1, 1);
        let area = Rect::new(0, 0, 8, 4);
        assert_eq!(Popover::new().placement(anchor, area), Rect::ZERO);
        assert_eq!(Popover::new().width(4).placement(anchor, area), Rect::ZERO);
        let mut buf = Buffer::empty(area);
        background(&mut buf);
        Popover::new().height(2).render_anchored(anchor, &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '.');
    }

    #[test]
    fn zero_buffer_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 0, 0));
        Popover::new()
            .width(2)
            .height(2)
            .render_anchored(Rect::new(0, 0, 1, 1), &mut buf);
        assert_eq!(
            Popover::new()
                .width(2)
                .height(2)
                .placement(Rect::new(0, 0, 1, 1), Rect::ZERO),
            Rect::ZERO
        );
    }

    #[test]
    fn render_uses_the_whole_area_as_the_anchor() {
        // The bare `Widget::render` anchors off the area it is handed.
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
        Popover::new()
            .width(3)
            .height(1)
            .render(Rect::new(0, 0, 6, 1), &mut buf);
        // Area bottom is row 1, so the panel drops there.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }
}
