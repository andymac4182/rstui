//! [`Skeleton`] — a loading placeholder: shimmer blocks standing in for
//! content that has not arrived yet, the "list is fetching" / "panel is
//! booting" affordance.
//!
//! # A pure projection of a caller-owned animation tick — no wall clock
//!
//! [`Spinner`](crate::Spinner) extended the pure-projection model to *time*: a
//! caller-owned [`tick`](Skeleton::tick), advanced by the reducer (or
//! `frame.count()`), never the widget. `Skeleton` is the same contract for a
//! *region*: it fills its area with a placeholder glyph and sweeps a single
//! brighter "shimmer" column across it at `tick % width`. The widget never
//! advances anything at render time — making it own a clock would smuggle a
//! wall clock into the pure `view`, the one thing the architecture forbids
//! (the [`Spinner`](crate::Spinner) / [`Toast`](crate::Toast) caller-owned-tick
//! precedent).
//!
//! Like [`Spinner`](crate::Spinner)/[`Scrollbar`](crate::Scrollbar) every part
//! is a single [`char`], so `Skeleton` has **no lifetime**, **no
//! [`Block`](crate::Block)**, and **no label**: it is a placeholder
//! *adornment*, not a container — a real label is ordinary text the app
//! composes beside it with a [`Layout`](rstui_core::Layout) split.
//!
//! # Two shapes: a solid block, or text-like line bars
//!
//! A [`SkeletonShape::Block`] fills the whole area (a placeholder image/panel);
//! [`SkeletonShape::Lines`] draws *n* full-width bar rows with a blank row
//! between them (placeholder paragraphs/list rows). Both share the one shimmer
//! sweep.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: a zero
//! area renders nothing (and [`shimmer_column`](Skeleton::shimmer_column)
//! returns `None`) instead of a modulo-by-zero panic, any `tick` (however
//! large) wraps cleanly, and [`Lines(0)`](SkeletonShape::Lines) draws nothing.
//! A caller-owned counter can never abort the TUI.

use rstui_core::{Buffer, Position, Rect, Style, Widget};

/// The dim placeholder glyph filling a [`Skeleton`].
const PLACEHOLDER: char = '░';

/// The brighter glyph drawn on the swept shimmer column.
const SHIMMER: char = '▓';

/// What a [`Skeleton`] stands in for.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SkeletonShape {
    /// A solid placeholder filling the whole area (an image/panel) — the
    /// default.
    #[default]
    Block,
    /// `n` full-width bar rows with a blank row between them (placeholder text
    /// lines / list rows).
    Lines(u16),
}

/// A loading placeholder — a pure projection of a caller-owned animation
/// [`tick`](Self::tick).
///
/// Fills its area with a placeholder glyph in the [`shape`](Self::shape)
/// requested and sweeps one brighter shimmer column across it at
/// `tick % width`. There is no clock: advance [`tick`](Self::tick) in `update`
/// (or pass `frame.count()`) and the shimmer animates, exactly like
/// [`Spinner`](crate::Spinner).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Skeleton;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
/// Skeleton::new().tick(2).render(buf.area(), &mut buf);
///
/// // A solid placeholder row with the shimmer at column `tick % width`.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '░');
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, '▓');
///
/// // The tick wraps the width, so any caller counter is in range.
/// assert_eq!(Skeleton::new().tick(6).shimmer_column(4), Some(2));
/// ```
#[derive(Debug, Clone)]
pub struct Skeleton {
    shape: SkeletonShape,
    tick: usize,
    style: Style,
    shimmer_style: Style,
}

impl Default for Skeleton {
    fn default() -> Self {
        Self::new()
    }
}

impl Skeleton {
    /// A [`Block`](SkeletonShape::Block) placeholder at tick `0`, unstyled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            shape: SkeletonShape::Block,
            tick: 0,
            style: Style::new(),
            shimmer_style: Style::new(),
        }
    }

    /// Sets the placeholder [`SkeletonShape`] (default
    /// [`Block`](SkeletonShape::Block)).
    #[must_use]
    pub fn shape(mut self, shape: SkeletonShape) -> Self {
        self.shape = shape;
        self
    }

    /// Sets [`Lines(lines)`](SkeletonShape::Lines) — a convenience for the
    /// text-placeholder shape.
    #[must_use]
    pub fn lines(mut self, lines: u16) -> Self {
        self.shape = SkeletonShape::Lines(lines);
        self
    }

    /// Sets the animation index — the caller-owned counter (`frame.count()`,
    /// or a model field a `Cmd` advances). Taken modulo the area width, so any
    /// value, however large, is in range.
    #[must_use]
    pub fn tick(mut self, tick: usize) -> Self {
        self.tick = tick;
        self
    }

    /// Sets the base [`Style`] of the placeholder cells.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched **over the base** on the swept shimmer
    /// column (the moving highlight).
    #[must_use]
    pub fn shimmer_style(mut self, style: Style) -> Self {
        self.shimmer_style = style;
        self
    }

    /// Which column (offset from the area's left edge) carries the shimmer for
    /// `width` columns — `tick % width`, or `None` when `width` is `0`.
    ///
    /// This is exactly what [`render`](Widget::render) sweeps; it is public so
    /// callers/tests can assert the projection without a buffer (the
    /// [`Spinner::glyph`](crate::Spinner::glyph) precedent).
    #[must_use]
    pub fn shimmer_column(&self, width: u16) -> Option<u16> {
        if width == 0 {
            None
        } else {
            Some((self.tick % width as usize) as u16)
        }
    }
}

impl Widget for Skeleton {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Nowhere to draw: a total no-op (never a modulo-by-zero panic) — the
        // Gauge "a pure projection must be total" rule.
        if area.is_empty() {
            return;
        }
        let shimmer_x = area.left() + (self.tick % area.width as usize) as u16;
        let base = self.style;
        let shimmer = self.style.patch(self.shimmer_style);

        for (row, y) in (area.top()..area.bottom()).enumerate() {
            let is_bar = match self.shape {
                SkeletonShape::Block => true,
                // Bars on even rows (0, 2, 4, …), at most `n` of them, with a
                // blank row between — clipped by the area height.
                SkeletonShape::Lines(n) => row % 2 == 0 && (row as u16 / 2) < n,
            };
            if !is_bar {
                continue;
            }
            for x in area.left()..area.right() {
                let (glyph, style) = if x == shimmer_x {
                    (SHIMMER, shimmer)
                } else {
                    (PLACEHOLDER, base)
                };
                buf.set_cell(Position::new(x, y), glyph, style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Color;

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
    fn block_mode_fills_the_area_with_the_placeholder_glyph() {
        // tick 0 → shimmer in column 0; every other cell is the placeholder.
        assert_eq!(lines(Skeleton::new(), 3, 2), "▓░░\n▓░░\n");
    }

    #[test]
    fn the_shimmer_sits_at_tick_modulo_width() {
        assert_eq!(lines(Skeleton::new().tick(2), 4, 1), "░░▓░\n");
        assert_eq!(Skeleton::new().tick(2).shimmer_column(4), Some(2));
    }

    #[test]
    fn the_tick_wraps_the_width_so_any_counter_is_in_range() {
        // One full cycle later is the same column; a huge tick never panics.
        assert_eq!(lines(Skeleton::new().tick(4), 4, 1), "▓░░░\n");
        assert_eq!(
            Skeleton::new().tick(usize::MAX).shimmer_column(4),
            Some((usize::MAX % 4) as u16)
        );
    }

    #[test]
    fn lines_mode_draws_bar_rows_separated_by_blank_rows() {
        // 2 bars: rows 0 and 2 are full bars, row 1 between them is blank.
        assert_eq!(
            lines(Skeleton::new().lines(2).tick(0), 3, 3),
            "▓░░\n   \n▓░░\n"
        );
    }

    #[test]
    fn lines_mode_clips_to_the_available_height() {
        // 5 bars requested but only rows 0 and 2 fit in a 3-tall area.
        assert_eq!(
            lines(Skeleton::new().lines(5).tick(1), 2, 3),
            "░▓\n  \n░▓\n"
        );
    }

    #[test]
    fn lines_zero_draws_nothing() {
        assert_eq!(lines(Skeleton::new().lines(0), 3, 2), "   \n   \n");
    }

    #[test]
    fn style_paints_the_placeholder_cells() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        Skeleton::new()
            .tick(1)
            .style(Style::new().fg(Color::DarkGray))
            .render(buf.area(), &mut buf);
        // Column 0 is the plain placeholder, styled by the base.
        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.symbol, '░');
        assert_eq!(cell.fg, Color::DarkGray);
    }

    #[test]
    fn shimmer_style_is_patched_over_the_base_on_the_shimmer_column() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        Skeleton::new()
            .tick(0)
            .style(Style::new().fg(Color::DarkGray))
            .shimmer_style(Style::new().bg(Color::White))
            .render(buf.area(), &mut buf);
        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.symbol, '▓');
        assert_eq!(cell.fg, Color::DarkGray); // base cascades
        assert_eq!(cell.bg, Color::White); // shimmer patched over it
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 5));
        Skeleton::new()
            .tick(0)
            .render(Rect::new(3, 4, 2, 1), &mut buf);
        assert_eq!(buf.get(Position::new(3, 4)).unwrap().symbol, '▓');
        assert_eq!(buf.get(Position::new(4, 4)).unwrap().symbol, '░');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn shimmer_column_is_none_for_zero_width() {
        assert_eq!(Skeleton::new().tick(7).shimmer_column(0), None);
    }

    #[test]
    fn a_one_wide_area_keeps_the_shimmer_in_column_zero() {
        assert_eq!(lines(Skeleton::new().tick(99), 1, 1), "▓\n");
        assert_eq!(Skeleton::new().tick(99).shimmer_column(1), Some(0));
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        Skeleton::new().render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
