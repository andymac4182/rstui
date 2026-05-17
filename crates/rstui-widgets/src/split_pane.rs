//! [`SplitPane`] — divides an area into two resizable panes with a divider
//! glyph between them; the basis for editor/preview, list/detail, and any
//! two-up layout.
//!
//! # A pure projection of a caller-owned split, on purpose
//!
//! A real split view *resizes*: dragging the divider, or a `Ctrl-+` keystroke,
//! grows one pane at the other's expense. That split position is ordinary
//! application state — exactly like [`List`](crate::List)'s `selected`/`offset`
//! — so `SplitPane` does not own it. The caller passes the first pane's size
//! as a [`Constraint`] (a `Ratio`, a `Percentage`, or a fixed `Length`); the
//! reducer changes that number in `update` when the user drags or keys a
//! resize, and `view` rebuilds the widget from it. The widget only ever
//! *reads* the split; it never writes one, so it fits `App::view(&self)` and
//! is deterministically headless-testable.
//!
//! Like [`Modal::inner`](crate::Modal::inner), it takes **no child widgets** —
//! it is pure layout. [`split`](SplitPane::split) hands back the two pane
//! [`Rect`]s and the caller renders its own content into them, so a split
//! composes with anything (including another `SplitPane`) without the widget
//! needing to know what lives inside.
//!
//! # Reuses the core divider, never reinvents one
//!
//! The two panes and the 1-cell gap between them are placed by
//! [`Layout`] with [`spacing(1)`](Layout::spacing) — the very same
//! deterministic divider top-level layout and [`Table`](crate::Table) columns
//! use — so a degenerate ratio, a constraint larger than the area, and an area
//! too small for a divider all resolve exactly the way every other layout in
//! rstui does (clamped, fully tiled, never a panic). The divider *glyph* is the
//! only thing this widget adds on top.
//!
//! # Deliberately deferred
//!
//! Drag-to-resize hit-testing is the reducer's job (the widget exposes
//! [`divider_rect`](SplitPane::divider_rect) so the app can map a click to a
//! resize, exactly as [`Modal::area`](crate::Modal::area) exposes its box for
//! click-to-focus); an N-way split, and per-pane minimum sizes, are additive
//! follow-ups that compose from this two-pane primitive rather than changing
//! its shape — so they are not smuggled in here.

use rstui_core::{Buffer, Constraint, Direction, Layout, Position, Rect, Style, Widget};

use crate::Block;

/// Splits an area into two panes by a caller-owned [`Constraint`], drawing a
/// 1-cell divider between them.
///
/// The [`constraint`](Self::constraint) sizes the **first** pane (left for
/// [`Direction::Horizontal`], top for [`Direction::Vertical`]); the second
/// pane takes the remainder. [`split`](Self::split) is a pure function of the
/// area and the configuration — the caller renders its own content into the
/// two returned rects, so a `SplitPane` takes no child widgets (it is pure
/// layout, like [`Modal::inner`](crate::Modal::inner)). An optional framing
/// [`Block`] composes exactly as it does for every other container widget.
///
/// ```
/// use rstui_core::{Buffer, Constraint, Position, Rect, Widget};
/// use rstui_widgets::SplitPane;
///
/// // The split position is *caller state* (a `Constraint`) — the reducer
/// // changes it on a drag/resize; the widget only reads it.
/// let sp = SplitPane::new(Constraint::Length(3));
/// let (left, right) = sp.split(Rect::new(0, 0, 10, 3));
/// assert_eq!(left, Rect::new(0, 0, 3, 3)); // first pane
/// assert_eq!(right, Rect::new(4, 0, 6, 3)); // second pane, past the divider
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
/// sp.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, '│'); // divider
/// ```
#[derive(Debug, Clone)]
pub struct SplitPane<'a> {
    direction: Direction,
    constraint: Constraint,
    divider: Option<char>,
    divider_style: Style,
    handle: Option<char>,
    style: Style,
    block: Option<Block<'a>>,
}

impl<'a> SplitPane<'a> {
    /// A side-by-side ([`Horizontal`](Direction::Horizontal)) split whose left
    /// pane is sized by `constraint` and right pane takes the rest.
    #[must_use]
    pub fn new(constraint: Constraint) -> Self {
        Self {
            direction: Direction::Horizontal,
            constraint,
            divider: None,
            divider_style: Style::new(),
            handle: None,
            style: Style::new(),
            block: None,
        }
    }

    /// A side-by-side split (left pane sized by `constraint`). Alias for
    /// [`new`](Self::new), mirroring [`Layout::horizontal`].
    #[must_use]
    pub fn horizontal(constraint: Constraint) -> Self {
        Self::new(constraint)
    }

    /// A stacked ([`Vertical`](Direction::Vertical)) split whose top pane is
    /// sized by `constraint` and bottom pane takes the rest.
    #[must_use]
    pub fn vertical(constraint: Constraint) -> Self {
        Self::new(constraint).direction(Direction::Vertical)
    }

    /// Sets the split axis.
    #[must_use]
    pub fn direction(mut self, direction: Direction) -> Self {
        self.direction = direction;
        self
    }

    /// Replaces the first pane's size [`Constraint`] (the caller-owned split
    /// position the reducer mutates on a resize).
    #[must_use]
    pub fn constraint(mut self, constraint: Constraint) -> Self {
        self.constraint = constraint;
        self
    }

    /// Sets the divider glyph. Defaults to the axis-appropriate box-drawing
    /// rule (`│` for a horizontal split, `─` for a vertical one).
    #[must_use]
    pub fn divider(mut self, divider: char) -> Self {
        self.divider = Some(divider);
        self
    }

    /// Sets the [`Style`] the divider (and handle) is drawn with.
    #[must_use]
    pub fn divider_style(mut self, style: Style) -> Self {
        self.divider_style = style;
        self
    }

    /// Sets a handle glyph stamped at the midpoint of the divider so it reads
    /// as draggable. Off by default; *what a drag does* is the reducer's job.
    #[must_use]
    pub fn handle(mut self, handle: char) -> Self {
        self.handle = Some(handle);
        self
    }

    /// Sets the base [`Style`], beneath the divider and the caller's panes. It
    /// fills the content area so a background covers the whole region.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Frames the split in `block`; the panes and divider are placed inside
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// The framed content rect: [`block.inner`](Block::inner) of `area`, or
    /// the whole `area` when there is no block. The panes are placed inside
    /// this, exactly as with [`Modal::inner`](crate::Modal::inner).
    #[must_use]
    pub fn inner(&self, area: Rect) -> Rect {
        match &self.block {
            Some(block) => block.inner(area),
            None => area,
        }
    }

    /// The two pane rects for `area`: `(first, second)` with the 1-cell
    /// divider gap between them.
    ///
    /// A pure function of `area` and the configuration — render the caller's
    /// own content into these. A degenerate ratio, an oversized constraint, or
    /// an area too small for a divider all clamp through [`Layout`] (the panes
    /// collapse rather than panicking).
    #[must_use]
    pub fn split(&self, area: Rect) -> (Rect, Rect) {
        let (first, _, second) = split_regions(self.direction, self.constraint, self.inner(area));
        (first, second)
    }

    /// The divider's rect for `area` — the 1-cell gap between the panes.
    ///
    /// Exposed so an app can map a click on the divider to a resize (the
    /// drag is the reducer's concern, the same way
    /// [`Modal::area`](crate::Modal::area) is exposed for click-to-focus).
    /// Degenerate to empty when there is no room for a divider.
    #[must_use]
    pub fn divider_rect(&self, area: Rect) -> Rect {
        split_regions(self.direction, self.constraint, self.inner(area)).1
    }

    /// Whether `pos` is on the divider for `area`, within a **1-cell grab
    /// tolerance** on the split axis.
    ///
    /// The forgiving hit-zone an app tests on mouse-down to begin a resize
    /// drag — a 1-cell divider is hard to click exactly, so a press one cell
    /// either side still counts. Always `false` when there is no room for a
    /// divider (the [`divider_rect`](Self::divider_rect) is empty). The drag
    /// itself is the reducer's concern; this is the *seam*, not the state
    /// (see the reusable pointer-gesture recipe in `docs/composition.md`).
    #[must_use]
    pub fn contains_divider(&self, area: Rect, pos: Position) -> bool {
        let d = self.divider_rect(area);
        if d.is_empty() {
            return false;
        }
        match self.direction {
            Direction::Horizontal => pos.y >= d.y && pos.y < d.bottom() && pos.x.abs_diff(d.x) <= 1,
            Direction::Vertical => pos.x >= d.x && pos.x < d.right() && pos.y.abs_diff(d.y) <= 1,
        }
    }

    /// The first-pane [`Constraint`] that puts the divider under `pos`.
    ///
    /// Feed this straight back as the new
    /// [`constraint`](Self::constraint) on every `MouseDrag` while a divider
    /// drag is in progress, so the split tracks the pointer — the ready-made
    /// generalisation of the bespoke pointer→ratio math an app would
    /// otherwise re-derive (and get wrong at the edges).
    ///
    /// Pure and **total**: the pointer is clamped into the framed inner area
    /// so neither pane can collapse below one cell, and an area with no room
    /// for two panes + a divider returns the current
    /// [`constraint`](Self::constraint) unchanged (a no-op). A
    /// [`Length`](Constraint::Length) is returned, not a ratio, so repeated
    /// drags are stable — a ratio re-derived from a clamped length and
    /// re-applied jitters by a cell.
    #[must_use]
    pub fn resize_to(&self, area: Rect, pos: Position) -> Constraint {
        let inner = self.inner(area);
        let first = match self.direction {
            Direction::Horizontal => {
                if inner.width < 3 {
                    return self.constraint;
                }
                pos.x.saturating_sub(inner.x).clamp(1, inner.width - 2)
            }
            Direction::Vertical => {
                if inner.height < 3 {
                    return self.constraint;
                }
                pos.y.saturating_sub(inner.y).clamp(1, inner.height - 2)
            }
        };
        Constraint::Length(first)
    }
}

impl Default for SplitPane<'_> {
    /// An even 50/50 horizontal split.
    fn default() -> Self {
        Self::new(Constraint::Ratio(1, 2))
    }
}

/// Resolves `(first, divider, second)` rects inside an already-framed `inner`
/// area, reusing the core [`Layout`] divider (`spacing(1)` is the gap). Shared
/// by every `&self` accessor and [`render`](Widget::render) so the geometry is
/// computed exactly one way.
fn split_regions(direction: Direction, constraint: Constraint, inner: Rect) -> (Rect, Rect, Rect) {
    if inner.is_empty() {
        return (Rect::ZERO, Rect::ZERO, Rect::ZERO);
    }
    let [first, second] = layout_panes(direction, constraint, inner);
    let divider = match direction {
        Direction::Horizontal => Rect::new(
            first.right(),
            inner.y,
            second.x.saturating_sub(first.right()),
            inner.height,
        ),
        Direction::Vertical => Rect::new(
            inner.x,
            first.bottom(),
            inner.width,
            second.y.saturating_sub(first.bottom()),
        ),
    };
    (first, divider, second)
}

/// The two pane rects from the core divider: `[constraint, Fill(1)]` with a
/// 1-cell gap reserved between them.
fn layout_panes(direction: Direction, constraint: Constraint, inner: Rect) -> [Rect; 2] {
    Layout::new(direction, [constraint, Constraint::Fill(1)])
        .spacing(1)
        .areas(inner)
}

impl Widget for SplitPane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let SplitPane {
            direction,
            constraint,
            divider,
            divider_style,
            handle,
            style,
            block,
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

        // Base fills the content area so a background covers the whole region;
        // the caller's panes and the divider layer on top.
        buf.set_style(inner, style);

        let (_, div, _) = split_regions(direction, constraint, inner);
        if div.is_empty() {
            return;
        }

        // The axis-appropriate rule unless the caller overrode the glyph.
        let glyph = divider.unwrap_or(match direction {
            Direction::Horizontal => '│',
            Direction::Vertical => '─',
        });
        for p in div.positions() {
            buf.set_cell(p, glyph, divider_style);
        }

        // The optional handle reads as the draggable grip, stamped over the
        // divider's midpoint cell.
        if let Some(h) = handle {
            let mid = Position::new(
                div.x.saturating_add(div.width / 2),
                div.y.saturating_add(div.height / 2),
            );
            buf.set_cell(mid, h, divider_style);
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
    fn horizontal_split_places_two_panes_around_a_vertical_divider() {
        let sp = SplitPane::new(Constraint::Length(3));
        let (l, r) = sp.split(Rect::new(0, 0, 10, 2));
        assert_eq!(l, Rect::new(0, 0, 3, 2));
        assert_eq!(r, Rect::new(4, 0, 6, 2));
        assert_eq!(
            sp.divider_rect(Rect::new(0, 0, 10, 2)),
            Rect::new(3, 0, 1, 2)
        );
    }

    #[test]
    fn vertical_split_stacks_two_panes_around_a_horizontal_divider() {
        let sp = SplitPane::vertical(Constraint::Length(1));
        let (t, b) = sp.split(Rect::new(0, 0, 4, 5));
        assert_eq!(t, Rect::new(0, 0, 4, 1));
        assert_eq!(b, Rect::new(0, 2, 4, 3));
        assert_eq!(
            lines(SplitPane::vertical(Constraint::Length(1)), 4, 5),
            "    \n────\n    \n    \n    \n"
        );
    }

    #[test]
    fn the_divider_glyph_follows_the_axis_by_default() {
        assert_eq!(
            lines(SplitPane::new(Constraint::Length(1)), 5, 2),
            " │   \n │   \n"
        );
        assert_eq!(
            lines(SplitPane::vertical(Constraint::Length(1)), 3, 3),
            "   \n───\n   \n"
        );
    }

    #[test]
    fn a_custom_divider_glyph_overrides_the_default() {
        assert_eq!(
            lines(SplitPane::new(Constraint::Length(1)).divider('┃'), 4, 1),
            " ┃  \n"
        );
    }

    #[test]
    fn the_handle_is_stamped_at_the_divider_midpoint() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 5));
        SplitPane::new(Constraint::Length(1))
            .handle('◆')
            .render(buf.area(), &mut buf);
        // Divider column is x=1; midpoint of a height-5 divider is y=2.
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, '◆');
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '│');
    }

    #[test]
    fn default_is_an_even_ratio_split() {
        // Ratio(1,2) of (10 - 1 gap) = 5 / 4, then Fill takes the slack.
        let (l, r) = SplitPane::default().split(Rect::new(0, 0, 10, 1));
        assert_eq!(l.width + r.width + 1, 10);
        assert_eq!(l, Rect::new(0, 0, 5, 1));
        assert_eq!(r, Rect::new(6, 0, 4, 1));
    }

    #[test]
    fn a_zero_ratio_collapses_the_first_pane_without_panicking() {
        let (l, r) = SplitPane::new(Constraint::Ratio(0, 1)).split(Rect::new(0, 0, 6, 1));
        assert_eq!(l.width, 0);
        // Everything past the divider goes to the second pane.
        assert_eq!(r, Rect::new(1, 0, 5, 1));
    }

    #[test]
    fn a_full_ratio_collapses_the_second_pane_without_panicking() {
        let (l, r) = SplitPane::new(Constraint::Ratio(1, 1)).split(Rect::new(0, 0, 6, 1));
        assert_eq!(r.width, 0);
        assert_eq!(l, Rect::new(0, 0, 5, 1));
    }

    #[test]
    fn an_oversized_constraint_is_clamped_not_a_panic() {
        let (l, r) = SplitPane::new(Constraint::Length(999)).split(Rect::new(0, 0, 5, 1));
        // Clamped to fit: the oversized first pane is scaled into the area and
        // the panes plus the 1-cell divider never exceed it (no panic).
        assert_eq!(l, Rect::new(0, 0, 4, 1));
        assert_eq!(r, Rect::new(5, 0, 0, 1));
        assert!(l.right() <= 5 && r.right() <= 5);
    }

    #[test]
    fn an_area_too_small_for_a_divider_clips_safely() {
        // 1 column: the divider sliver is all there is; both panes collapse.
        let (l, r) = SplitPane::new(Constraint::Length(1)).split(Rect::new(0, 0, 1, 1));
        assert!(l.is_empty() && r.is_empty());
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        SplitPane::new(Constraint::Length(1)).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '│');
    }

    #[test]
    fn a_block_frames_the_split_in_the_inner_area() {
        let sp = SplitPane::new(Constraint::Length(1)).block(Block::bordered());
        assert_eq!(sp.inner(Rect::new(0, 0, 6, 3)), Rect::new(1, 1, 4, 1));
        assert_eq!(lines(sp, 6, 3), "┌────┐\n│ │  │\n└────┘\n");
    }

    #[test]
    fn base_style_fills_the_whole_content_area() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        SplitPane::new(Constraint::Length(1))
            .style(Style::new().bg(Color::Red))
            .render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        SplitPane::new(Constraint::Length(2)).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn contains_divider_has_a_one_cell_grab_tolerance() {
        let sp = SplitPane::new(Constraint::Length(3));
        let area = Rect::new(0, 0, 10, 2);
        assert_eq!(sp.divider_rect(area), Rect::new(3, 0, 1, 2));
        // On the divider and one cell either side, anywhere down its height.
        for x in [2, 3, 4] {
            assert!(sp.contains_divider(area, Position::new(x, 0)));
            assert!(sp.contains_divider(area, Position::new(x, 1)));
        }
        // Two cells away, or off the divider's span, misses.
        assert!(!sp.contains_divider(area, Position::new(1, 0)));
        assert!(!sp.contains_divider(area, Position::new(6, 0)));
        assert!(!sp.contains_divider(area, Position::new(3, 5)));
        // A wide area but a press well clear of the divider still misses.
        let wide = Rect::new(0, 0, 40, 4);
        assert!(!sp.contains_divider(wide, Position::new(30, 1)));
    }

    #[test]
    fn resize_to_tracks_the_pointer_and_clamps_both_panes_open() {
        let sp = SplitPane::new(Constraint::Length(5));
        let area = Rect::new(0, 0, 20, 3); // no block ⇒ inner == area
        // The divider follows the pointer's column.
        assert!(matches!(
            sp.resize_to(area, Position::new(8, 1)),
            Constraint::Length(8)
        ));
        // Clamped so neither pane collapses: far left ⇒ 1, far right ⇒ w-2.
        assert!(matches!(
            sp.resize_to(area, Position::new(0, 1)),
            Constraint::Length(1)
        ));
        assert!(matches!(
            sp.resize_to(area, Position::new(99, 1)),
            Constraint::Length(18)
        ));
        // Too small for two panes + a divider ⇒ unchanged (a no-op).
        assert!(matches!(
            sp.resize_to(Rect::new(0, 0, 2, 1), Position::new(1, 0)),
            Constraint::Length(5)
        ));
    }

    #[test]
    fn resize_to_works_on_the_vertical_axis_too() {
        let sp = SplitPane::vertical(Constraint::Length(2));
        let area = Rect::new(0, 0, 4, 10);
        assert!(matches!(
            sp.resize_to(area, Position::new(1, 6)),
            Constraint::Length(6)
        ));
        assert!(matches!(
            sp.resize_to(area, Position::new(1, 99)),
            Constraint::Length(8)
        ));
        // The grab tolerance works on the horizontal divider line too.
        assert_eq!(sp.divider_rect(area), Rect::new(0, 2, 4, 1));
        assert!(sp.contains_divider(area, Position::new(2, 3)));
        assert!(!sp.contains_divider(area, Position::new(2, 5)));
    }
}
