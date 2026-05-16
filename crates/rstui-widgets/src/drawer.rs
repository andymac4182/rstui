//! [`Drawer`] — a panel slid in from a screen edge over an overlay, the
//! mobile/IDE "side sheet" (navigation drawer, properties sheet, command
//! output pane).
//!
//! # A pure projection of a caller-owned `open` flag
//!
//! Like every rstui widget `Drawer` is a **pure projection**: it reads only
//! the caller-owned [`open`](Drawer::open) flag (ordinary application state the
//! reducer toggles in `update`, typically on a keypress) and its
//! configuration. The widget never decides whether the drawer is open, exactly
//! as [`Modal`](crate::Modal) never decides a dialog is open and
//! [`Select`](crate::Select) never decides its panel is dropped.
//!
//! # Edge-anchored, not centred — a [`Modal`](crate::Modal) on an edge
//!
//! A `Drawer` is the [`Modal`](crate::Modal) overlay model with one change:
//! the panel is flush to a [`DrawerSide`] edge (full height on
//! [`Left`](DrawerSide::Left)/[`Right`](DrawerSide::Right), full width on
//! [`Top`](DrawerSide::Top)/[`Bottom`](DrawerSide::Bottom)) instead of centred.
//! It borrows `Modal`'s two defining affordances verbatim:
//!
//! - **Opaque.** The panel
//!   [`clear_region`](rstui_core::Buffer::clear_region)s its rect before
//!   drawing (the exclusive-ownership reasoning `modal.rs:24-38` records: a
//!   [`Style`] is a patch and cannot reset a cell, so a merely-styled box would
//!   let the background bleed through).
//! - **An opt-in dim backdrop.** [`backdrop_style`](Drawer::backdrop_style)
//!   patches the *whole* overlay (keeping its glyphs, dimmed), defaulting empty
//!   like every other widget style — `Modal`'s scrim, edge-anchored.
//!
//! The panel size reuses the [`Constraint`] vocabulary
//! ([`Constraint::apply`], clamped to the overlay) exactly as
//! [`Modal`](crate::Modal) sizes its dialog, rather than inventing a
//! drawer-sizing type. An optional framing [`Block`] composes the same
//! render-then-fill-[`inner`](Drawer::inner) way.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: a
//! **closed** drawer is a true no-op (no scrim, no clear — the background is
//! left exactly as it was, even with a backdrop set), an empty overlay draws
//! nothing, and a size resolving past the overlay is clamped — never a panic.

use rstui_core::{Buffer, Constraint, Rect, Style, Widget};

use crate::block::Block;

/// Which screen edge a [`Drawer`] slides in from.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DrawerSide {
    /// The left edge (the default) — full height, [`size`](Drawer::size) wide.
    #[default]
    Left,
    /// The right edge — full height, [`size`](Drawer::size) wide.
    Right,
    /// The top edge — full width, [`size`](Drawer::size) tall.
    Top,
    /// The bottom edge — full width, [`size`](Drawer::size) tall.
    Bottom,
}

/// A panel slid in from a screen edge over an overlay — a pure projection of a
/// caller-owned [`open`](Self::open) flag.
///
/// When [`open`](Self::open), the panel is anchored flush to the
/// [`side`](Self::side) edge, sized by the [`size`](Self::size)
/// [`Constraint`], [`clear`](rstui_core::Buffer::clear_region)ed opaque (with
/// an optional dim [`backdrop_style`](Self::backdrop_style) over the rest), and
/// optionally framed by a [`block`](Self::block); the caller draws content into
/// [`inner`](Self::inner). Closed, it is a true no-op.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Constraint, Position, Rect, Widget};
/// use rstui_widgets::{Block, Drawer, DrawerSide};
///
/// // `open` is plain caller-owned model state the widget only reads — the
/// // reducer toggles it in `update`, never the widget at render time.
/// let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
/// let drawer = Drawer::new()
///     .open(true)
///     .side(DrawerSide::Left)
///     .size(Constraint::Length(4))
///     .block(Block::bordered());
///
/// // `panel`/`inner` are pure functions of the overlay + config.
/// assert_eq!(drawer.panel(buf.area()), Rect::new(0, 0, 4, 4));
/// let inner = drawer.inner(buf.area());
///
/// drawer.render(buf.area(), &mut buf);
/// "x".render(inner, &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌'); // frame
/// assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'x'); // content
/// ```
#[derive(Debug, Clone)]
pub struct Drawer<'a> {
    open: bool,
    side: DrawerSide,
    size: Constraint,
    block: Option<Block<'a>>,
    style: Style,
    backdrop_style: Style,
}

impl<'a> Drawer<'a> {
    /// A closed drawer on the [`Left`](DrawerSide::Left) edge, sized at
    /// [`Percentage(30)`](Constraint::Percentage), unframed, with no backdrop.
    #[must_use]
    pub fn new() -> Self {
        Self {
            open: false,
            side: DrawerSide::Left,
            size: Constraint::Percentage(30),
            block: None,
            style: Style::new(),
            backdrop_style: Style::new(),
        }
    }

    /// Sets whether the drawer is open — caller-owned state the widget only
    /// reads (toggle it in `update`). Closed is a true no-op.
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the edge the drawer slides in from (default
    /// [`Left`](DrawerSide::Left)).
    #[must_use]
    pub fn side(mut self, side: DrawerSide) -> Self {
        self.side = side;
        self
    }

    /// Sets the panel's cross-edge size (width for
    /// [`Left`](DrawerSide::Left)/[`Right`](DrawerSide::Right), height for
    /// [`Top`](DrawerSide::Top)/[`Bottom`](DrawerSide::Bottom)). Resolved with
    /// [`Constraint::apply`], so it never exceeds the overlay.
    #[must_use]
    pub fn size(mut self, size: Constraint) -> Self {
        self.size = size;
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
    /// reads as one solid box.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the scrim [`Style`] patched over the *whole* overlay (the dimming
    /// behind the panel). Defaults empty — opt-in, like every other widget
    /// style; the panel itself is opaque whether or not a scrim is set. Unlike
    /// the panel, the scrim only *patches* the background (so its glyphs stay,
    /// dimmed) rather than clearing it.
    #[must_use]
    pub fn backdrop_style(mut self, style: Style) -> Self {
        self.backdrop_style = style;
        self
    }

    /// The panel rect for a given `overlay`, or
    /// [`Rect::ZERO`](rstui_core::Rect::ZERO) when the drawer is closed (or the
    /// overlay is empty).
    ///
    /// A pure function of `overlay` and the configuration: flush to the
    /// [`side`](Self::side) edge, [`size`](Self::size) resolved against the
    /// overlay (never larger than it) on the cross edge, full extent on the
    /// other. Exposed so an app can map a click in the panel to a
    /// [`FocusId`](rstui_core::FocusId) (the
    /// [`Modal::area`](crate::Modal::area) precedent — click-to-focus is
    /// app-owned).
    #[must_use]
    pub fn panel(&self, overlay: Rect) -> Rect {
        if !self.open || overlay.is_empty() {
            return Rect::ZERO;
        }
        match self.side {
            DrawerSide::Left => {
                let w = self.size.apply(overlay.width);
                Rect::new(overlay.x, overlay.y, w, overlay.height)
            }
            DrawerSide::Right => {
                let w = self.size.apply(overlay.width);
                Rect::new(
                    overlay.right().saturating_sub(w),
                    overlay.y,
                    w,
                    overlay.height,
                )
            }
            DrawerSide::Top => {
                let h = self.size.apply(overlay.height);
                Rect::new(overlay.x, overlay.y, overlay.width, h)
            }
            DrawerSide::Bottom => {
                let h = self.size.apply(overlay.height);
                Rect::new(
                    overlay.x,
                    overlay.bottom().saturating_sub(h),
                    overlay.width,
                    h,
                )
            }
        }
    }

    /// The content rect inside the panel: [`panel`](Self::panel) minus the
    /// framing [`block`](Self::block) (or the whole panel when there is no
    /// block). Render the drawer's content here, exactly as with
    /// [`Block::inner`].
    #[must_use]
    pub fn inner(&self, overlay: Rect) -> Rect {
        let panel = self.panel(overlay);
        match &self.block {
            Some(b) => b.inner(panel),
            None => panel,
        }
    }
}

impl Default for Drawer<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Drawer<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Closed (or no overlay) is a TOTAL no-op: no scrim, no clear — the
        // background is left exactly as it was (even with a backdrop set).
        if !self.open || area.is_empty() {
            return;
        }

        // The optional dim backdrop over the whole overlay. `set_style` only
        // patches, so the background glyphs stay (dimmed); default-empty is a
        // no-op (Modal's scrim, edge-anchored).
        buf.set_style(area, self.backdrop_style);

        let panel = self.panel(area);
        if panel.is_empty() {
            return;
        }

        // Opaque: take exclusive ownership of the cells (the Modal
        // `clear_region` affordance — modal.rs:24-38), colour the box, then
        // draw the optional frame; the caller draws content into `inner`.
        buf.clear_region(panel);
        buf.set_style(panel, self.style);
        if let Some(block) = self.block {
            block.render(panel, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Position};

    /// Fills `buf` with a styled `.` background so a clear is observable.
    fn background(buf: &mut Buffer) {
        let style = Style::new().fg(Color::Red).bg(Color::Blue);
        for p in buf.area().positions() {
            buf.set_cell(p, '.', style);
        }
    }

    #[test]
    fn closed_is_a_total_no_op_even_with_a_backdrop() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        background(&mut buf);
        Drawer::new()
            .style(Style::new().bg(Color::Green))
            .backdrop_style(Style::new().bg(Color::Black))
            .render(buf.area(), &mut buf);
        // Nothing touched: the '.'/Blue background is fully intact.
        let c = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(c.symbol, '.');
        assert_eq!(c.bg, Color::Blue);
        assert!(Drawer::new().panel(buf.area()).is_empty());
    }

    #[test]
    fn each_edge_anchors_the_panel_flush_to_that_edge() {
        let overlay = Rect::new(0, 0, 10, 8);
        let p = |side| {
            Drawer::new()
                .open(true)
                .side(side)
                .size(Constraint::Length(3))
                .panel(overlay)
        };
        // Left/Right are full height, 3 wide; Top/Bottom full width, 3 tall.
        assert_eq!(p(DrawerSide::Left), Rect::new(0, 0, 3, 8));
        assert_eq!(p(DrawerSide::Right), Rect::new(7, 0, 3, 8));
        assert_eq!(p(DrawerSide::Top), Rect::new(0, 0, 10, 3));
        assert_eq!(p(DrawerSide::Bottom), Rect::new(0, 5, 10, 3));
    }

    #[test]
    fn the_size_constraint_resolves_and_clamps_when_oversize() {
        let overlay = Rect::new(0, 0, 10, 6);
        // Percentage of the cross edge.
        assert_eq!(
            Drawer::new()
                .open(true)
                .size(Constraint::Percentage(50))
                .panel(overlay),
            Rect::new(0, 0, 5, 6)
        );
        // A request past the overlay is clamped, never overflows.
        assert_eq!(
            Drawer::new()
                .open(true)
                .size(Constraint::Length(999))
                .panel(overlay),
            Rect::new(0, 0, 10, 6)
        );
    }

    #[test]
    fn the_panel_is_opaque_so_the_background_does_not_bleed_through() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        background(&mut buf);
        Drawer::new()
            .open(true)
            .size(Constraint::Length(3))
            .render(buf.area(), &mut buf);
        // The panel (cols 0..3) is cleared opaque; outside it survives.
        let inside = buf.get(Position::new(1, 1)).unwrap();
        assert_eq!(inside.symbol, ' ');
        assert_eq!(inside.bg, Color::Reset);
        assert_eq!(buf.get(Position::new(5, 2)).unwrap().symbol, '.');
    }

    #[test]
    fn backdrop_style_patches_the_whole_overlay_but_keeps_its_glyphs() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        background(&mut buf);
        Drawer::new()
            .open(true)
            .size(Constraint::Length(2))
            .backdrop_style(Style::new().bg(Color::Black))
            .render(buf.area(), &mut buf);
        // Outside the panel: glyph kept, backdrop bg applied.
        let scrim = buf.get(Position::new(5, 0)).unwrap();
        assert_eq!(scrim.symbol, '.');
        assert_eq!(scrim.bg, Color::Black);
        // The panel was cleared after the scrim, so it is not black.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn style_fills_the_panel() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        Drawer::new()
            .open(true)
            .size(Constraint::Length(2))
            .style(Style::new().bg(Color::Green))
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Green);
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn a_block_frames_the_panel_and_inner_subtracts_the_frame() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        let drawer = Drawer::new()
            .open(true)
            .size(Constraint::Length(4))
            .block(Block::bordered());
        assert_eq!(drawer.inner(buf.area()), Rect::new(1, 1, 2, 1));
        drawer.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, '┐');
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, '└');
    }

    #[test]
    fn inner_is_the_panel_when_there_is_no_block() {
        let drawer = Drawer::new().open(true).size(Constraint::Length(3));
        let overlay = Rect::new(0, 0, 10, 4);
        assert_eq!(drawer.inner(overlay), drawer.panel(overlay));
    }

    #[test]
    fn panel_is_empty_when_closed_and_the_edge_rect_when_open() {
        let overlay = Rect::new(2, 1, 8, 6);
        assert!(
            Drawer::new()
                .size(Constraint::Length(3))
                .panel(overlay)
                .is_empty()
        );
        assert_eq!(
            Drawer::new()
                .open(true)
                .side(DrawerSide::Right)
                .size(Constraint::Length(3))
                .panel(overlay),
            // Honours the overlay origin, not the buffer origin.
            Rect::new(7, 1, 3, 6)
        );
    }

    #[test]
    fn zero_overlay_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        Drawer::new()
            .open(true)
            .style(Style::new().bg(Color::Red))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        assert!(Drawer::new().open(true).panel(Rect::ZERO).is_empty());
    }
}
