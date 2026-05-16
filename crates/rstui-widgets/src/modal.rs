//! [`Modal`] — a centred, opaque, optionally-framed dialog drawn over an
//! overlay area, the visual half of the modal-focus model.
//!
//! # The visual companion to the `FocusRing` scope stack
//!
//! [ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
//! §6 splits a modal into two halves. The *state* half — trapping focus to the
//! modal's controls, capturing and validate-restoring the prior focus, and
//! gating background input — landed as the
//! [`FocusRing`](rstui_core::FocusRing) scope stack
//! ([`push_scope`](rstui_core::FocusRing::push_scope) /
//! [`pop_scope`](rstui_core::FocusRing::pop_scope) /
//! [`in_scope`](rstui_core::FocusRing::in_scope)). `Modal` is the *visual*
//! half: the centred, opaque, framed box that dialog content is drawn into.
//!
//! It is, like every rstui widget, a **pure projection**. It never reads
//! focus, never decides whether a modal is open, and never mutates anything at
//! render time. The app decides "is a modal open" in its own model (typically
//! `ring.in_scope()`); `view` renders `Modal` only when it is. The `modal_demo`
//! example wires the two halves together end to end under the headless
//! [`Harness`](https://docs.rs/rstui-runtime) — pushing a scope on open,
//! gating background keys on `in_scope()`, and popping on close.
//!
//! # Opaque on purpose — the defining modal affordance
//!
//! A modal *floats over unrelated content*: in rstui's immediate mode the
//! whole screen is redrawn each frame, so the background is drawn first and
//! the modal over it. If the modal box were merely *styled* over that
//! background, background glyphs would show through it, because a
//! [`Style`] is a *patch* — it can set a colour but cannot
//! reset one. So `Modal` **clears** its box (via
//! [`Buffer::clear_region`](rstui_core::Buffer::clear_region)) before drawing,
//! taking exclusive ownership of those cells. This is the modal's defining,
//! always-on affordance — the same "one justified exception to the
//! styles-default-empty rule" reasoning [`Input`](crate::Input)'s always-drawn
//! caret uses. The optional [`backdrop_style`](Modal::backdrop_style) scrim
//! over the rest of the area (to dim the background) is, by contrast, opt-in
//! and defaults empty like every other widget style.
//!
//! # Sizing reuses the `Constraint` vocabulary
//!
//! The dialog is sized within the overlay area by a
//! [`width`](Modal::width)/[`height`](Modal::height)
//! [`Constraint`] each (default `Percentage(60)` ×
//! `Percentage(40)`) and centred — reusing rstui's existing layout vocabulary
//! rather than inventing a popup-sizing type, exactly as
//! [`Table`](crate::Table) reuses `Constraint` for column widths. Odd leftover
//! space biases the box toward the start, matching
//! [`Alignment::Center`](rstui_core::Alignment).
//!
//! # Composes with `Block`, exactly like the container widgets
//!
//! An optional framing [`Block`] draws the border/title; the
//! caller renders the dialog's content into [`Modal::inner`] (the box minus
//! the frame), the same render-then-fill-`inner` pattern `Block` itself uses.
//! [`Modal::area`] exposes the centred box rect so an app can map a click
//! inside it to a [`FocusId`](rstui_core::FocusId) (ADR 0004 §4 click-to-focus
//! is app-owned) — both accessors are pure functions of the overlay area and
//! the configuration, like every other derived-geometry projection.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*):
//! an empty overlay, an overlay smaller than the frame, and constraints that
//! resolve the box to zero are all safe no-ops (a degenerate modal is just its
//! scrim) — never a panic.

use rstui_core::{Buffer, Constraint, Rect, Style, Widget};

use crate::Block;

/// A centred, opaque, optionally-framed dialog rendered over an overlay area.
///
/// Render it over the region it should cover (usually the whole
/// [`Frame::area`](rstui_core::Frame::area)); it paints an optional
/// [`backdrop_style`](Self::backdrop_style) scrim over that whole area, then
/// **clears** and fills a centred box sized by
/// [`width`](Self::width)/[`height`](Self::height), then draws the optional
/// framing [`block`](Self::block). The caller draws the dialog's content into
/// [`inner`](Self::inner):
///
/// ```
/// use rstui_core::{Buffer, Constraint, Position, Rect, Widget};
/// use rstui_widgets::{Block, Modal};
///
/// // A background that the modal must not let bleed through.
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 7));
/// for p in buf.area().positions() {
///     buf.set_cell(p, '.', Default::default());
/// }
///
/// let modal = Modal::new()
///     .width(Constraint::Length(10))
///     .height(Constraint::Length(3))
///     .block(Block::bordered().title("Hi"));
///
/// // `area`/`inner` are pure functions of the overlay area + config.
/// let dialog = modal.area(buf.area()); // centred 10x3 box
/// let inner = modal.inner(buf.area()); // its content rect (inside the border)
/// assert_eq!(dialog, Rect::new(5, 2, 10, 3));
/// assert_eq!(inner, Rect::new(6, 3, 8, 1));
///
/// modal.render(buf.area(), &mut buf);
/// "ok".render(inner, &mut buf);
///
/// // Background outside the box is untouched; the box is opaque (cleared,
/// // so the '.' background does not show through), framed, and filled.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '.'); // background
/// assert_eq!(buf.get(Position::new(5, 2)).unwrap().symbol, '┌'); // border
/// assert_eq!(buf.get(Position::new(6, 3)).unwrap().symbol, 'o'); // content
/// assert_eq!(buf.get(Position::new(10, 3)).unwrap().symbol, ' '); // cleared
/// ```
#[derive(Debug, Clone)]
pub struct Modal<'a> {
    block: Option<Block<'a>>,
    width: Constraint,
    height: Constraint,
    style: Style,
    backdrop_style: Style,
}

impl<'a> Modal<'a> {
    /// A modal sized at 60% × 40% of the overlay, centred, with no frame, an
    /// unstyled (but opaque) box, and no backdrop scrim.
    #[must_use]
    pub fn new() -> Self {
        Self {
            block: None,
            width: Constraint::Percentage(60),
            height: Constraint::Percentage(40),
            style: Style::new(),
            backdrop_style: Style::new(),
        }
    }

    /// Sets the framing [`Block`] (border, title, padding). The dialog's
    /// content is rendered into [`inner`](Self::inner), which subtracts the
    /// block's frame — the same render-then-fill-`inner` pattern `Block` uses.
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the dialog's width within the overlay (default
    /// [`Percentage(60)`](Constraint::Percentage)). Resolved with
    /// [`Constraint::apply`], so it never exceeds the overlay width.
    #[must_use]
    pub fn width(mut self, width: Constraint) -> Self {
        self.width = width;
        self
    }

    /// Sets the dialog's height within the overlay (default
    /// [`Percentage(40)`](Constraint::Percentage)). Resolved with
    /// [`Constraint::apply`], so it never exceeds the overlay height.
    #[must_use]
    pub fn height(mut self, height: Constraint) -> Self {
        self.height = height;
        self
    }

    /// Sets the [`Style`] that fills the (already-cleared) dialog box, beneath
    /// the [`block`](Self::block) and content. The clear makes the box opaque
    /// regardless; this colours it.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the scrim [`Style`] patched over the *whole* overlay area (the
    /// dimming behind the dialog). Defaults empty — opt-in, like every other
    /// widget style; the dialog box itself is opaque whether or not a scrim is
    /// set. Unlike the box, the scrim only *patches* the background (so its
    /// glyphs stay, dimmed) rather than clearing it.
    #[must_use]
    pub fn backdrop_style(mut self, style: Style) -> Self {
        self.backdrop_style = style;
        self
    }

    /// The centred dialog box rect for a given `overlay` area.
    ///
    /// A pure function of `overlay` and the size constraints: the box is
    /// [`width`](Self::width)/[`height`](Self::height) resolved against the
    /// overlay (never larger than it) and centred, with any odd leftover
    /// biased toward the start (matching
    /// [`Alignment::Center`](rstui_core::Alignment)). Exposed so an app can
    /// map a click within the box to a [`FocusId`](rstui_core::FocusId)
    /// (ADR 0004 §4 — click-to-focus is app-owned).
    #[must_use]
    pub fn area(&self, overlay: Rect) -> Rect {
        let w = self.width.apply(overlay.width);
        let h = self.height.apply(overlay.height);
        let x = overlay
            .x
            .saturating_add(overlay.width.saturating_sub(w) / 2);
        let y = overlay
            .y
            .saturating_add(overlay.height.saturating_sub(h) / 2);
        Rect::new(x, y, w, h)
    }

    /// The content rect inside the dialog: [`area`](Self::area) minus the
    /// framing [`block`](Self::block)'s border/padding (or the whole box when
    /// there is no block). Render the dialog's content here, exactly as with
    /// [`Block::inner`].
    #[must_use]
    pub fn inner(&self, overlay: Rect) -> Rect {
        let dialog = self.area(overlay);
        match &self.block {
            Some(block) => block.inner(dialog),
            None => dialog,
        }
    }
}

impl Default for Modal<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Modal<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // 1. The scrim over the whole overlay. `set_style` only patches, so
        //    the background glyphs stay (dimmed); default-empty is a no-op.
        buf.set_style(area, self.backdrop_style);

        // 2. The centred dialog box. A degenerate (zero-sized) box leaves just
        //    the scrim — total, no panic.
        let dialog = self.area(area);
        if dialog.is_empty() {
            return;
        }

        // 3. Clear the box opaque so background content cannot bleed through
        //    the gaps, then colour it. The clear is the defining modal
        //    affordance (see the module docs); the fill is optional polish.
        buf.clear_region(dialog);
        buf.set_style(dialog, self.style);

        // 4. The optional frame. Content goes into `inner`, drawn by the
        //    caller (the render-then-fill-`inner` pattern `Block` uses).
        if let Some(block) = self.block {
            block.render(dialog, buf);
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
    fn default_box_is_centred_within_the_overlay() {
        // 20x10, default 60% x 40% -> 12 x 4, centred at ((20-12)/2,(10-4)/2).
        let modal = Modal::new();
        assert_eq!(modal.area(Rect::new(0, 0, 20, 10)), Rect::new(4, 3, 12, 4));
    }

    #[test]
    fn explicit_constraints_size_and_centre_the_box() {
        let modal = Modal::new()
            .width(Constraint::Length(10))
            .height(Constraint::Length(4));
        assert_eq!(modal.area(Rect::new(0, 0, 20, 10)), Rect::new(5, 3, 10, 4));

        // Honours the overlay origin, not the buffer origin.
        assert_eq!(modal.area(Rect::new(2, 1, 20, 10)), Rect::new(7, 4, 10, 4));
    }

    #[test]
    fn odd_leftover_space_biases_the_box_toward_the_start() {
        // Overlay 5 wide, box 2 wide: 3 spare -> 1 left, 2 right (start bias),
        // matching Alignment::Center's "odd remainder toward the start".
        let modal = Modal::new()
            .width(Constraint::Length(2))
            .height(Constraint::Length(1));
        assert_eq!(modal.area(Rect::new(0, 0, 5, 3)), Rect::new(1, 1, 2, 1));
    }

    #[test]
    fn a_box_request_larger_than_the_overlay_is_clamped() {
        let modal = Modal::new()
            .width(Constraint::Length(999))
            .height(Constraint::Percentage(100));
        // Clamped to the overlay, so it is centred at the origin, full size.
        assert_eq!(modal.area(Rect::new(0, 0, 8, 4)), Rect::new(0, 0, 8, 4));
    }

    #[test]
    fn inner_is_the_box_when_there_is_no_block_and_the_frame_otherwise() {
        let overlay = Rect::new(0, 0, 20, 10);
        let modal = Modal::new()
            .width(Constraint::Length(10))
            .height(Constraint::Length(4));
        // No block: inner == the box.
        assert_eq!(modal.inner(overlay), modal.area(overlay));

        // With a bordered block: inner is the box minus the 1-cell border.
        let framed = modal.clone().block(Block::bordered());
        assert_eq!(framed.inner(overlay), Rect::new(6, 4, 8, 2));
    }

    #[test]
    fn the_box_is_cleared_opaque_so_the_background_cannot_bleed_through() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 6));
        background(&mut buf);
        Modal::new()
            .width(Constraint::Length(6))
            .height(Constraint::Length(2))
            .render(buf.area(), &mut buf);

        // The box (centred at (3,2) 6x2) is back to EMPTY — no '.' bleeds in,
        // and the red/blue background colour is gone (a style patch could not
        // have reset it; clear_region did).
        for y in 2..4 {
            for x in 3..9 {
                assert_eq!(
                    *buf.get(Position::new(x, y)).unwrap(),
                    rstui_core::Cell::EMPTY
                );
            }
        }
        // Outside the box the background survives untouched.
        let kept = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(kept.symbol, '.');
        assert_eq!(kept.bg, Color::Blue);
    }

    #[test]
    fn style_colours_the_cleared_box() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        background(&mut buf);
        Modal::new()
            .width(Constraint::Length(4))
            .height(Constraint::Length(2))
            .style(Style::new().bg(Color::Green))
            .render(buf.area(), &mut buf);

        // Box centred at (3,1) 4x2: cleared then filled green, blank glyph.
        let cell = buf.get(Position::new(3, 1)).unwrap();
        assert_eq!(cell.symbol, ' ');
        assert_eq!(cell.bg, Color::Green);
        assert_eq!(cell.fg, Color::Reset); // the clear reset fg; style set bg only
    }

    #[test]
    fn backdrop_style_patches_the_whole_overlay_but_keeps_its_glyphs() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        background(&mut buf);
        Modal::new()
            .width(Constraint::Length(4))
            .height(Constraint::Length(2))
            .backdrop_style(Style::new().bg(Color::Black))
            .render(buf.area(), &mut buf);

        // A cell outside the box keeps its '.' glyph (scrim only patches
        // style) but takes the backdrop background.
        let scrim = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(scrim.symbol, '.');
        assert_eq!(scrim.bg, Color::Black);
        // The box itself was cleared after the scrim, so it is not black.
        assert_eq!(buf.get(Position::new(3, 1)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn a_framing_block_draws_over_the_cleared_box() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 6));
        background(&mut buf);
        Modal::new()
            .width(Constraint::Length(6))
            .height(Constraint::Length(4))
            .block(Block::bordered().title("Hi"))
            .render(buf.area(), &mut buf);

        // Box centred at (3,1) 6x4: border corner + title on its top row.
        assert_eq!(buf.get(Position::new(3, 1)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(4, 1)).unwrap().symbol, 'H');
        assert_eq!(buf.get(Position::new(8, 1)).unwrap().symbol, '┐');
        // The interior is the cleared box, not the '.' background.
        assert_eq!(buf.get(Position::new(5, 2)).unwrap().symbol, ' ');
    }

    #[test]
    fn a_zero_constraint_box_leaves_only_the_scrim() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        background(&mut buf);
        Modal::new()
            .width(Constraint::Length(0))
            .backdrop_style(Style::new().bg(Color::Black))
            .render(buf.area(), &mut buf);

        // No box drawn (width 0), but the scrim still applied everywhere.
        for p in buf.area().positions() {
            let c = buf.get(p).unwrap();
            assert_eq!(c.symbol, '.');
            assert_eq!(c.bg, Color::Black);
        }
    }

    #[test]
    fn zero_overlay_area_is_a_total_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        background(&mut buf);
        Modal::new()
            .style(Style::new().bg(Color::Green))
            .backdrop_style(Style::new().bg(Color::Black))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        // Nothing was touched.
        let c = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(c.symbol, '.');
        assert_eq!(c.bg, Color::Blue);
    }
}
