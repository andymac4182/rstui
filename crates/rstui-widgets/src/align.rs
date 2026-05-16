//! [`Align`] — places a smaller child [`Rect`] within a larger area on both
//! axes; the [`Modal`](crate::Modal)-centring math generalized into a reusable
//! primitive (a centred toast, a bottom-right hint, a top-centred banner).
//!
//! # Pure layout, owns no state — the centring math, not a Modal
//!
//! [`Modal`](crate::Modal) centres an opaque dialog and clears it; that
//! opacity is its defining affordance. The *placement* underneath it — "size a
//! box by a [`Constraint`] per axis and position it within an area" — is a
//! widely reused calculation (every popup, hint, watermark, and centred
//! splash needs it), so this slice lifts exactly that math out into a pure
//! geometry primitive. `Align` is **not** a `Modal`: it does not clear, is not
//! inherently opaque, and takes **no child widgets** —
//! [`rect`](Align::rect) is a pure function of the area and the configuration
//! and the caller renders its own content into the returned [`Rect`], exactly
//! like [`SplitPane::split`](crate::SplitPane::split) and
//! [`Grid::split`](crate::Grid::split). It mutates nothing at render time, so
//! it fits `App::view(&self)` and is deterministically headless-testable.
//!
//! Horizontal placement reuses the core [`Alignment`] vocabulary (the same
//! `Left`/`Center`/`Right` a [`Block`] title or a [`Line`](rstui_core::Line)
//! uses); the vertical axis has no core type, so this module adds the minimal
//! [`VerticalAlignment`] (`Top`/`Center`/`Bottom`) rather than overloading
//! `Alignment`. The child size is a [`Constraint`] per axis — reusing rstui's
//! layout vocabulary rather than inventing a sizing type, exactly as
//! [`Modal`](crate::Modal) does. Odd leftover space biases the child toward
//! the start, matching [`Alignment::Center`].
//!
//! # Deliberately deferred
//!
//! A per-side margin/inset and aligning *multiple* stacked children are
//! additive follow-ups that compose from this one-child shape rather than
//! changing it — so they are not smuggled in here. A child constraint that
//! resolves larger than the area is clamped to the area (totality), never an
//! overflow.

use rstui_core::{Alignment, Buffer, Constraint, Rect, Style, Widget};

use crate::Block;

/// Vertical placement of a child within an available span — the vertical-axis
/// companion to the core horizontal [`Alignment`].
///
/// `Alignment` (in `rstui-core`) only models the horizontal axis (a title, a
/// text line). Two-axis placement needs a vertical counterpart; rather than
/// overload `Alignment` with axis-ambiguous variants, [`Align`] pairs it with
/// this small, unambiguous enum. `Center` biases an odd remainder toward the
/// top, matching [`Alignment::Center`]'s start-bias.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerticalAlignment {
    /// Flush with the top edge.
    #[default]
    Top,
    /// Centered, with any odd remainder biased toward the top.
    Center,
    /// Flush with the bottom edge.
    Bottom,
}

/// Positions a [`width`](Self::width)×[`height`](Self::height)-sized child rect
/// within an area, aligned on both axes.
///
/// [`rect`](Self::rect) resolves each [`Constraint`] against the matching area
/// dimension (clamped so the child never exceeds the area), then offsets it by
/// the [`horizontal`](Self::horizontal) [`Alignment`] and the
/// [`vertical`](Self::vertical) [`VerticalAlignment`]. The caller renders its
/// own content into the returned rect; `Align` takes no child widget. An
/// optional framing [`Block`] composes as it does for every container widget,
/// and the base [`style`](Self::style) fills the child rect.
///
/// # Example
///
/// ```
/// use rstui_core::{Constraint, Rect};
/// use rstui_widgets::{Align, VerticalAlignment};
///
/// // A fixed 6×2 child, centred in a 20×8 area.
/// let centred = Align::new()
///     .width(Constraint::Length(6))
///     .height(Constraint::Length(2));
/// assert_eq!(centred.rect(Rect::new(0, 0, 20, 8)), Rect::new(7, 3, 6, 2));
///
/// // Pin the same child to the bottom-right corner instead.
/// let corner = centred
///     .horizontal(rstui_core::Alignment::Right)
///     .vertical(VerticalAlignment::Bottom);
/// assert_eq!(corner.rect(Rect::new(0, 0, 20, 8)), Rect::new(14, 6, 6, 2));
/// ```
#[derive(Debug, Clone)]
pub struct Align<'a> {
    horizontal: Alignment,
    vertical: VerticalAlignment,
    width: Constraint,
    height: Constraint,
    style: Style,
    block: Option<Block<'a>>,
}

impl<'a> Align<'a> {
    /// A child centred on both axes, sized to the full area
    /// ([`Percentage(100)`](Constraint::Percentage) each) until you set a
    /// smaller [`width`](Self::width)/[`height`](Self::height).
    #[must_use]
    pub fn new() -> Self {
        Self {
            horizontal: Alignment::Center,
            vertical: VerticalAlignment::Center,
            width: Constraint::Percentage(100),
            height: Constraint::Percentage(100),
            style: Style::new(),
            block: None,
        }
    }

    /// Sets the horizontal placement (default [`Alignment::Center`]).
    #[must_use]
    pub fn horizontal(mut self, horizontal: Alignment) -> Self {
        self.horizontal = horizontal;
        self
    }

    /// Sets the vertical placement (default [`VerticalAlignment::Center`]).
    #[must_use]
    pub fn vertical(mut self, vertical: VerticalAlignment) -> Self {
        self.vertical = vertical;
        self
    }

    /// Sets the child's width within the area (default
    /// [`Percentage(100)`](Constraint::Percentage)). Resolved against the area
    /// width and clamped to it, so the child never overflows.
    #[must_use]
    pub fn width(mut self, width: Constraint) -> Self {
        self.width = width;
        self
    }

    /// Sets the child's height within the area (default
    /// [`Percentage(100)`](Constraint::Percentage)). Resolved against the area
    /// height and clamped to it, so the child never overflows.
    #[must_use]
    pub fn height(mut self, height: Constraint) -> Self {
        self.height = height;
        self
    }

    /// Sets the base [`Style`] that fills the child rect, beneath the
    /// [`block`](Self::block) and the caller's content.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Frames the child in `block`; content is rendered into
    /// [`inner`](Self::inner) (the child minus the frame).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// The aligned child rect within `area`.
    ///
    /// A pure function of `area` and the configuration: each
    /// [`Constraint`] is resolved against the matching area dimension and
    /// clamped to it (so an oversized child fills, never overflows), then
    /// offset by the alignment with any odd leftover biased toward the
    /// start/top — matching [`Alignment::Center`]. Render the caller's own
    /// content here, exactly as with
    /// [`Modal::area`](crate::Modal::area).
    #[must_use]
    pub fn rect(&self, area: Rect) -> Rect {
        let w = self.width.apply(area.width).min(area.width);
        let h = self.height.apply(area.height).min(area.height);
        let free_x = area.width.saturating_sub(w);
        let free_y = area.height.saturating_sub(h);
        let x = area.x.saturating_add(match self.horizontal {
            Alignment::Left => 0,
            Alignment::Center => free_x / 2,
            Alignment::Right => free_x,
        });
        let y = area.y.saturating_add(match self.vertical {
            VerticalAlignment::Top => 0,
            VerticalAlignment::Center => free_y / 2,
            VerticalAlignment::Bottom => free_y,
        });
        Rect::new(x, y, w, h)
    }

    /// The content rect inside the child: [`rect`](Self::rect) minus the
    /// framing [`block`](Self::block) (or the whole child when there is no
    /// block). Render content here, exactly as with [`Block::inner`].
    #[must_use]
    pub fn inner(&self, area: Rect) -> Rect {
        let child = self.rect(area);
        match &self.block {
            Some(block) => block.inner(child),
            None => child,
        }
    }
}

impl Default for Align<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Align<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let child = self.rect(area);
        if child.is_empty() {
            return;
        }

        // Base fills the child so a background covers it; the optional frame
        // and the caller's content (into `inner`) layer on top.
        buf.set_style(child, self.style);
        if let Some(block) = self.block {
            block.render(child, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Position};

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row (the list.rs helper).
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
    fn default_centres_a_full_size_child_which_is_the_whole_area() {
        // Percentage(100) each → child == area, centred (no offset).
        let a = Align::new();
        assert_eq!(a.rect(Rect::new(0, 0, 10, 4)), Rect::new(0, 0, 10, 4));
    }

    #[test]
    fn a_fixed_child_is_centred_on_both_axes() {
        let a = Align::new()
            .width(Constraint::Length(4))
            .height(Constraint::Length(2));
        // 10-4=6 → 3 left; 6-2=4 → 2 top.
        assert_eq!(a.rect(Rect::new(0, 0, 10, 6)), Rect::new(3, 2, 4, 2));
    }

    #[test]
    fn horizontal_and_vertical_alignment_pin_each_axis_independently() {
        let a = Align::new()
            .width(Constraint::Length(2))
            .height(Constraint::Length(1))
            .horizontal(Alignment::Left)
            .vertical(VerticalAlignment::Bottom);
        assert_eq!(a.rect(Rect::new(0, 0, 6, 4)), Rect::new(0, 3, 2, 1));

        let b = a
            .clone()
            .horizontal(Alignment::Right)
            .vertical(VerticalAlignment::Top);
        assert_eq!(b.rect(Rect::new(0, 0, 6, 4)), Rect::new(4, 0, 2, 1));
    }

    #[test]
    fn the_area_origin_is_honoured_not_just_the_size() {
        let a = Align::new()
            .width(Constraint::Length(2))
            .height(Constraint::Length(2));
        assert_eq!(a.rect(Rect::new(5, 3, 6, 6)), Rect::new(7, 5, 2, 2));
    }

    #[test]
    fn odd_leftover_space_biases_the_child_toward_the_start_and_top() {
        // 5-2=3 spare → 1 left, 2 right (start bias), matching Alignment::Center.
        let a = Align::new()
            .width(Constraint::Length(2))
            .height(Constraint::Length(2));
        assert_eq!(a.rect(Rect::new(0, 0, 5, 5)), Rect::new(1, 1, 2, 2));
    }

    #[test]
    fn a_percentage_child_is_resolved_against_the_area() {
        let a = Align::new()
            .width(Constraint::Percentage(50))
            .height(Constraint::Percentage(50));
        assert_eq!(a.rect(Rect::new(0, 0, 20, 8)), Rect::new(5, 2, 10, 4));
    }

    #[test]
    fn a_child_larger_than_the_area_is_clamped_to_it_not_overflowing() {
        let a = Align::new()
            .width(Constraint::Length(999))
            .height(Constraint::Min(50));
        let r = a.rect(Rect::new(0, 0, 8, 4));
        assert_eq!(r, Rect::new(0, 0, 8, 4)); // clamped, centred at origin
        assert!(r.right() <= 8 && r.bottom() <= 4);
    }

    #[test]
    fn inner_is_the_child_without_a_block_and_the_frame_with_one() {
        let area = Rect::new(0, 0, 12, 6);
        let a = Align::new()
            .width(Constraint::Length(6))
            .height(Constraint::Length(4));
        assert_eq!(a.inner(area), a.rect(area));

        let framed = a.clone().block(Block::bordered());
        assert_eq!(framed.inner(area), Rect::new(4, 2, 4, 2));
    }

    #[test]
    fn render_fills_the_child_and_draws_the_frame_only_there() {
        let a = Align::new()
            .width(Constraint::Length(2))
            .height(Constraint::Length(2))
            .block(Block::bordered());
        // 4×4 area, 2×2 child centred at (1,1): just the border box there.
        assert_eq!(lines(a, 4, 4), "    \n ┌┐ \n └┘ \n    \n");
    }

    #[test]
    fn base_style_fills_only_the_child_rect() {
        let a = Align::new()
            .width(Constraint::Length(2))
            .height(Constraint::Length(2))
            .style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 4));
        a.render(buf.area(), &mut buf);
        // Child is centred at (1,1) 2×2.
        for p in buf.area().positions() {
            let inside = (1..3).contains(&p.x) && (1..3).contains(&p.y);
            let want = if inside { Color::Red } else { Color::Reset };
            assert_eq!(buf.get(p).unwrap().bg, want, "at {p:?}");
        }
    }

    #[test]
    fn a_zero_constraint_child_is_empty_and_render_is_a_no_op() {
        let a = Align::new()
            .width(Constraint::Length(0))
            .style(Style::new().bg(Color::Red));
        assert!(a.rect(Rect::new(0, 0, 8, 4)).is_empty());
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        a.render(buf.area(), &mut buf);
        assert!(buf.cells().iter().all(|c| c.bg == Color::Reset));
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Align::new()
            .style(Style::new().bg(Color::Red))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(
            buf.cells()
                .iter()
                .all(|c| c.symbol == ' ' && c.bg == Color::Reset)
        );
    }
}
