//! [`ScrollView`] — a clipping viewport over an oversized, caller-rendered
//! content [`Buffer`], paired with a [`Scrollbar`] on each overflowing axis;
//! the keystone primitive for chat transcripts, log panes, and any region
//! whose content is larger than the space it is given.
//!
//! # Why a borrowed content buffer (the immediate-mode contract)
//!
//! rstui is immediate-mode: there is no retained child tree a viewport could
//! re-clip each frame, and the [`Widget`] trait renders once into the screen
//! buffer with no signed coordinates, so a viewport cannot just *translate* a
//! child by a negative offset — content scrolled above/left of the window
//! would underflow `u16` and bleed into sibling regions.
//!
//! So the clip is done the only way that is total and immediate-mode-correct:
//! the caller renders its **full** content, once, into its **own** off-screen
//! [`Buffer`] sized to the content (origin `(0, 0)`,
//! `content_width × content_height`), then hands `ScrollView` a *borrowed*
//! reference. `ScrollView` copies only the visible window — the
//! offset-translated [`viewport`](ScrollView::viewport) sub-rect — into the
//! screen and draws the scrollbars over it. It is a **pure projection**: the
//! content buffer and the caller-owned `(col_offset, row_offset)` are read,
//! never mutated (the same borrowed-caller-state discipline
//! [`Input`](crate::Input) uses for [`TextEdit`](rstui_core::TextEdit) and
//! [`Editor`](crate::Editor) for [`TextArea`](rstui_core::TextArea)). The
//! offset is ordinary application state the reducer changes in `update`,
//! exactly the [`List`](crate::List)/[`Editor`](crate::Editor) offset model;
//! `ScrollView` owns no content — only the clip and the bars.
//!
//! ```text
//! caller's content Buffer (content_width × content_height)   on screen:
//! ┌───────────────────────────┐                              ┌────────┬┐
//! │ ........................   │   ScrollView clips the       │ window ║│  ← Scrollbar
//! │ ....┌────────┐.........    │   (col_offset,row_offset)    │ slice  ║│
//! │ ....│ window │.........    │ ───────────────────────────► │ of it  ║│
//! │ ....└────────┘.........    │   sub-rect into the screen   ├────────┼┤
//! │ ........................   │                              │═════════│  ← Scrollbar
//! └───────────────────────────┘                              └────────┴┘
//! ```
//!
//! # Deliberately deferred
//!
//! Scroll-the-cursor-into-view (an expensive-to-reverse stateful-widget seam,
//! the [`List`](crate::List) precedent forbids smuggling it in), scrollbar
//! drag hit-testing (the reducer's job, exposed via the pure geometry the way
//! [`SplitPane`](crate::SplitPane) exposes its divider rect), and a virtualized
//! "only render the visible rows" fast path are additive follow-ups that
//! compose from this shape rather than changing it. An over-scrolled offset is
//! clamped; content smaller than the window draws no bar.

use rstui_core::{Buffer, Position, Rect, ScrollState, Style, Widget};

use crate::{Block, Scrollbar, ScrollbarOrientation};

/// A clipping viewport over a borrowed content [`Buffer`], with an automatic
/// [`Scrollbar`] on each overflowing axis.
///
/// The caller renders its full content into its own [`Buffer`] (covering
/// `Rect::new(0, 0, content_width, content_height)`) and passes a reference;
/// `ScrollView` copies the [`viewport`](Self::viewport) window — the
/// `(col_offset, row_offset)`-translated slice, both offsets caller-owned and
/// **clamped** so an over-scroll parks at the end — onto the screen, then
/// draws a vertical/horizontal [`Scrollbar`] only on an axis whose content
/// overflows the window. An optional framing [`Block`] composes as it does for
/// every container widget; the base [`style`](Self::style) fills the content
/// region first.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::ScrollView;
///
/// // The caller renders its full 3×5 content into its own buffer, once.
/// let mut content = Buffer::empty(Rect::new(0, 0, 3, 5));
/// for y in 0..5 {
///     for x in 0..3 {
///         content.set_cell(Position::new(x, y), (b'a' + y as u8) as char, Default::default());
///     }
/// }
///
/// // A 4-wide × 2-tall screen: content is 5 rows but the window only 2, so
/// // the rightmost column is reserved for the vertical scrollbar and the
/// // 3-wide window shows rows 2 and 3 ('c','d') — the offset-translated slice.
/// let view = ScrollView::new(&content).offset(0, 2);
/// let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
/// view.render(buf.area(), &mut buf);
///
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'c');
/// assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, 'd');
/// assert_ne!(buf.get(Position::new(3, 0)).unwrap().symbol, ' '); // scrollbar
/// ```
#[derive(Debug, Clone)]
pub struct ScrollView<'a> {
    content: &'a Buffer,
    col_offset: u16,
    row_offset: u16,
    vertical_scrollbar: bool,
    horizontal_scrollbar: bool,
    style: Style,
    track_style: Style,
    thumb_style: Style,
    block: Option<Block<'a>>,
}

impl<'a> ScrollView<'a> {
    /// A viewport over `content` (the caller's pre-rendered, full-size
    /// buffer), scrolled to the origin, with an automatic scrollbar on each
    /// overflowing axis.
    #[must_use]
    pub fn new(content: &'a Buffer) -> Self {
        Self {
            content,
            col_offset: 0,
            row_offset: 0,
            vertical_scrollbar: true,
            horizontal_scrollbar: true,
            style: Style::new(),
            track_style: Style::new(),
            thumb_style: Style::new(),
            block: None,
        }
    }

    /// Sets both scroll offsets at once: `col` columns from the left, `row`
    /// rows from the top. Caller-owned state the reducer mutates; values past
    /// the end are clamped (the view parks at the end), never a panic.
    #[must_use]
    pub fn offset(mut self, col: u16, row: u16) -> Self {
        self.col_offset = col;
        self.row_offset = row;
        self
    }

    /// Sets the horizontal scroll offset (columns from the left), clamped.
    #[must_use]
    pub fn col_offset(mut self, col: u16) -> Self {
        self.col_offset = col;
        self
    }

    /// Sets the vertical scroll offset (rows from the top), clamped.
    #[must_use]
    pub fn row_offset(mut self, row: u16) -> Self {
        self.row_offset = row;
        self
    }

    /// Drives the **vertical** offset from a caller-owned
    /// [`ScrollState`] — the reducer mutates the
    /// `ScrollState` (sticky-bottom while a transcript streams, scroll-into-
    /// view, `PageUp`/`End`) and `view` projects it here. Additive over the
    /// raw [`row_offset`](Self::row_offset): the same caller-owned-offset
    /// contract, just with the bookkeeping moved into the reusable primitive.
    /// `ScrollState`'s `usize` offset saturates into the viewport's `u16` and
    /// is then clamped against the content like any other offset (an
    /// over-scroll parks at the end). Pair with
    /// [`horizontal_scroll`](Self::horizontal_scroll) for a 2-D viewport —
    /// one `ScrollState` per axis, the documented compose-two model.
    #[must_use]
    pub fn vertical_scroll(mut self, scroll: &ScrollState) -> Self {
        self.row_offset = u16::try_from(scroll.offset()).unwrap_or(u16::MAX);
        self
    }

    /// Drives the **horizontal** offset from a caller-owned
    /// [`ScrollState`] (the columns-axis dual of
    /// [`vertical_scroll`](Self::vertical_scroll)). Additive over the raw
    /// [`col_offset`](Self::col_offset); the same saturate-then-clamp rule.
    #[must_use]
    pub fn horizontal_scroll(mut self, scroll: &ScrollState) -> Self {
        self.col_offset = u16::try_from(scroll.offset()).unwrap_or(u16::MAX);
        self
    }

    /// Sets whether a vertical [`Scrollbar`] is drawn when the content is
    /// taller than the window (default `true`). When off, the axis still
    /// scrolls — just without the indicator and without reserving its column.
    #[must_use]
    pub fn vertical_scrollbar(mut self, show: bool) -> Self {
        self.vertical_scrollbar = show;
        self
    }

    /// Sets whether a horizontal [`Scrollbar`] is drawn when the content is
    /// wider than the window (default `true`).
    #[must_use]
    pub fn horizontal_scrollbar(mut self, show: bool) -> Self {
        self.horizontal_scrollbar = show;
        self
    }

    /// Sets the base [`Style`] filling the content region, beneath the copied
    /// content and the scrollbars.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Scrollbar`] track [`Style`] (both axes).
    #[must_use]
    pub fn track_style(mut self, style: Style) -> Self {
        self.track_style = style;
        self
    }

    /// Sets the [`Scrollbar`] thumb [`Style`] (both axes), patched over the
    /// track style on the thumb cells.
    #[must_use]
    pub fn thumb_style(mut self, style: Style) -> Self {
        self.thumb_style = style;
        self
    }

    /// Frames the viewport in `block`; the window and scrollbars are placed
    /// inside [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// The framed content rect: [`block.inner`](Block::inner) of `area`, or
    /// the whole `area` when there is no block.
    #[must_use]
    pub fn inner(&self, area: Rect) -> Rect {
        match &self.block {
            Some(block) => block.inner(area),
            None => area,
        }
    }

    /// `(inner, window, show_v, show_h)`: the framed area, the visible window
    /// rect (inner minus any reserved scrollbar strips), and which bars are
    /// drawn. Computed exactly one way so [`viewport`](Self::viewport) and
    /// [`render`](Widget::render) never disagree.
    fn geometry(&self, area: Rect) -> (Rect, Rect, bool, bool) {
        let inner = self.inner(area);
        if inner.is_empty() {
            return (inner, Rect::new(inner.x, inner.y, 0, 0), false, false);
        }
        let content = self.content.area();
        // First pass against the full inner; then account for the fact that
        // reserving one axis's strip shrinks the other into overflow.
        let mut show_v = self.vertical_scrollbar && content.height > inner.height;
        let mut show_h = self.horizontal_scrollbar && content.width > inner.width;
        if show_v && self.horizontal_scrollbar && content.width > inner.width.saturating_sub(1) {
            show_h = true;
        }
        if show_h && self.vertical_scrollbar && content.height > inner.height.saturating_sub(1) {
            show_v = true;
        }
        let win_w = inner.width.saturating_sub(u16::from(show_v));
        let win_h = inner.height.saturating_sub(u16::from(show_h));
        (
            inner,
            Rect::new(inner.x, inner.y, win_w, win_h),
            show_v,
            show_h,
        )
    }

    /// The offset-translated slice of the content buffer that is currently
    /// visible, in the content buffer's own coordinate space.
    ///
    /// A pure function of `area`, the content size, and the caller-owned
    /// offsets: the window dimensions positioned at the **clamped** offset (an
    /// over-scroll parks at the last full window), clipped to the content. Use
    /// it to map a click in the window back to a content position, exactly as
    /// [`SplitPane::divider_rect`](crate::SplitPane::divider_rect) is exposed
    /// for hit-testing.
    #[must_use]
    pub fn viewport(&self, area: Rect) -> Rect {
        let (_, window, _, _) = self.geometry(area);
        let content = self.content.area();
        let col = self
            .col_offset
            .min(content.width.saturating_sub(window.width));
        let row = self
            .row_offset
            .min(content.height.saturating_sub(window.height));
        Rect::new(
            content.x.saturating_add(col),
            content.y.saturating_add(row),
            window.width.min(content.width.saturating_sub(col)),
            window.height.min(content.height.saturating_sub(row)),
        )
    }

    /// The rect the **vertical** scrollbar occupies for `area`, or `None`
    /// when it is not shown (content fits, or it is disabled).
    ///
    /// The pure mouse seam, exactly the strip [`render`](Widget::render)
    /// draws into: on a press inside it, build a
    /// [`Scrollbar`] with the content/viewport lengths the
    /// app owns (the content buffer's height and
    /// [`viewport`](Self::viewport)`(area).height`) and call
    /// [`Scrollbar::position_at`] to get the
    /// new row offset — the same composition the kitchen-sink uses.
    #[must_use]
    pub fn vertical_scrollbar_rect(&self, area: Rect) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        let (inner, window, show_v, _) = self.geometry(area);
        if !show_v || inner.is_empty() {
            return None;
        }
        Some(Rect::new(
            inner.right().saturating_sub(1),
            inner.y,
            1,
            window.height,
        ))
    }

    /// The rect the **horizontal** scrollbar occupies for `area`, or `None`
    /// when it is not shown. The pure mouse seam (see
    /// [`vertical_scrollbar_rect`](Self::vertical_scrollbar_rect)).
    #[must_use]
    pub fn horizontal_scrollbar_rect(&self, area: Rect) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        let (inner, window, _, show_h) = self.geometry(area);
        if !show_h || inner.is_empty() {
            return None;
        }
        Some(Rect::new(
            inner.x,
            inner.bottom().saturating_sub(1),
            window.width,
            1,
        ))
    }
}

impl Widget for ScrollView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let (inner, window, show_v, show_h) = self.geometry(area);
        let ScrollView {
            content,
            col_offset,
            row_offset,
            style,
            track_style,
            thumb_style,
            block,
            ..
        } = self;

        // The block (if any) frames the content and reserves the inner area.
        if let Some(b) = block {
            b.render(area, buf);
        }
        if inner.is_empty() {
            return;
        }

        // Base fills the content region so a background covers the whole pane
        // (including any cells past the end of the content); the copied
        // content and the bars layer on top.
        buf.set_style(inner, style);
        if window.is_empty() {
            return;
        }

        // Clamp the caller-owned offsets so an over-scroll parks at the end.
        let area_box = content.area();
        let col = col_offset.min(area_box.width.saturating_sub(window.width));
        let row = row_offset.min(area_box.height.saturating_sub(window.height));

        // Copy only the visible window out of the caller's content buffer —
        // the clip. Out-of-content cells stay as the base fill.
        for p in Rect::new(0, 0, window.width, window.height).positions() {
            let src = Position::new(
                area_box.x.saturating_add(col).saturating_add(p.x),
                area_box.y.saturating_add(row).saturating_add(p.y),
            );
            if let Some(cell) = content.get(src) {
                let cell = cell.clone();
                let dst = Position::new(window.x.saturating_add(p.x), window.y.saturating_add(p.y));
                if let Some(slot) = buf.get_mut(dst) {
                    *slot = cell;
                }
            }
        }

        // A bar per overflowing axis, over the strip reserved in `geometry`.
        if show_v {
            let strip = Rect::new(inner.right().saturating_sub(1), inner.y, 1, window.height);
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .content_length(area_box.height as usize)
                .viewport_length(window.height as usize)
                .position(row as usize)
                .begin_symbol(None)
                .end_symbol(None)
                .style(track_style)
                .thumb_style(thumb_style)
                .render(strip, buf);
        }
        if show_h {
            let strip = Rect::new(inner.x, inner.bottom().saturating_sub(1), window.width, 1);
            Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
                .content_length(area_box.width as usize)
                .viewport_length(window.width as usize)
                .position(col as usize)
                .begin_symbol(None)
                .end_symbol(None)
                .style(track_style)
                .thumb_style(thumb_style)
                .render(strip, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Position};

    /// A `width`×`height` content buffer where row `y` is the letter
    /// `'a' + y` repeated, so a vertical scroll is visible at a glance.
    fn content(width: u16, height: u16) -> Buffer {
        let mut b = Buffer::empty(Rect::new(0, 0, width, height));
        for y in 0..height {
            for x in 0..width {
                b.set_cell(
                    Position::new(x, y),
                    (b'a' + (y as u8 % 26)) as char,
                    Style::new(),
                );
            }
        }
        b
    }

    /// The rendered screen as one newline-terminated line per row.
    fn screen(view: ScrollView, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        view.render(buf.area(), &mut buf);
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
    fn content_smaller_than_the_window_draws_it_all_with_no_bars() {
        let c = content(3, 2);
        // Window 5×4 ⊇ 3×2 content: no overflow, no scrollbar reserved.
        assert_eq!(
            screen(ScrollView::new(&c), 5, 4),
            "aaa  \nbbb  \n     \n     \n"
        );
        let view = ScrollView::new(&c);
        assert_eq!(view.viewport(Rect::new(0, 0, 5, 4)), Rect::new(0, 0, 3, 2));
    }

    #[test]
    fn a_vertical_overflow_reserves_a_column_and_draws_the_right_bar() {
        let c = content(3, 10);
        // 4 wide: col 3 is the vertical scrollbar; rows 0..4 of content show.
        let out = screen(ScrollView::new(&c), 4, 4);
        for (y, line) in out.lines().enumerate() {
            let want = ((b'a' + y as u8) as char).to_string().repeat(3);
            assert_eq!(line.chars().take(3).collect::<String>(), want);
            assert_ne!(line.chars().nth(3).unwrap(), ' '); // scrollbar column
        }
    }

    #[test]
    fn the_offset_translates_which_content_slice_is_shown() {
        let c = content(3, 10);
        let view = ScrollView::new(&c).offset(0, 3);
        let out = screen(view, 4, 2);
        // Rows 3 and 4 ('d','e') now occupy the window.
        assert_eq!(
            out.lines()
                .next()
                .unwrap()
                .chars()
                .take(3)
                .collect::<String>(),
            "ddd"
        );
        assert_eq!(
            out.lines()
                .nth(1)
                .unwrap()
                .chars()
                .take(3)
                .collect::<String>(),
            "eee"
        );
    }

    #[test]
    fn viewport_is_the_offset_translated_slice_in_content_space() {
        let c = content(8, 8);
        let view = ScrollView::new(&c)
            .offset(2, 3)
            .vertical_scrollbar(false)
            .horizontal_scrollbar(false);
        // Both bars disabled → window == inner == 5×4, offset by (2,3).
        assert_eq!(view.viewport(Rect::new(0, 0, 5, 4)), Rect::new(2, 3, 5, 4));
    }

    #[test]
    fn an_over_scrolled_offset_is_clamped_to_the_end_not_a_panic() {
        let c = content(3, 6);
        let view = ScrollView::new(&c)
            .offset(0, 9999)
            .vertical_scrollbar(false);
        // 6 rows, 2-row window: max row offset is 4, so rows 4 and 5 ('e','f').
        let out = screen(view, 3, 2);
        assert_eq!(out, "eee\nfff\n");
        assert_eq!(
            ScrollView::new(&c)
                .offset(0, 9999)
                .vertical_scrollbar(false)
                .viewport(Rect::new(0, 0, 3, 2)),
            Rect::new(0, 4, 3, 2)
        );
    }

    #[test]
    fn a_horizontal_overflow_reserves_a_row_and_draws_the_bottom_bar() {
        let c = content(20, 2);
        let view = ScrollView::new(&c);
        let out = screen(view, 6, 3);
        // 3 tall: bottom row is the horizontal bar; top two rows are content.
        let bottom = out.lines().nth(2).unwrap();
        assert!(
            bottom.chars().any(|ch| ch != ' '),
            "expected a bottom bar: {bottom:?}"
        );
        assert_eq!(
            out.lines()
                .next()
                .unwrap()
                .chars()
                .take(6)
                .collect::<String>(),
            "aaaaaa"
        );
    }

    #[test]
    fn both_axes_overflowing_reserve_both_strips() {
        let c = content(20, 20);
        let view = ScrollView::new(&c);
        let (_, window, sv, sh) = view.geometry(Rect::new(0, 0, 8, 6));
        assert!(sv && sh);
        assert_eq!(window, Rect::new(0, 0, 7, 5)); // one column + one row reserved
    }

    #[test]
    fn scrollbars_can_be_disabled_and_the_axis_still_scrolls() {
        let c = content(3, 10);
        let view = ScrollView::new(&c).vertical_scrollbar(false).offset(0, 1);
        // No reserved column: the full 3-wide window shows rows 1 and 2.
        assert_eq!(screen(view, 3, 2), "bbb\nccc\n");
    }

    #[test]
    fn a_block_frames_the_viewport_in_the_inner_area() {
        let c = content(2, 2);
        let view = ScrollView::new(&c).block(Block::bordered());
        assert_eq!(view.inner(Rect::new(0, 0, 6, 4)), Rect::new(1, 1, 4, 2));
        assert_eq!(screen(view, 6, 4), "┌────┐\n│aa  │\n│bb  │\n└────┘\n");
    }

    #[test]
    fn base_style_fills_the_region_under_and_around_the_content() {
        // The 1×1 content covers only (0,0); the base fill must show on every
        // cell the content does not (the copied content is authoritative
        // within its own bounds, the base fill everywhere else).
        let c = content(1, 1);
        let view = ScrollView::new(&c).style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        view.render(buf.area(), &mut buf);
        for p in buf.area().positions() {
            let cell = buf.get(p).unwrap();
            if p == Position::new(0, 0) {
                assert_eq!(cell.symbol, 'a'); // the copied content cell
            } else {
                assert_eq!(cell.bg, Color::Red, "base fill at {p:?}");
            }
        }
    }

    #[test]
    fn the_thumb_style_paints_the_scrollbar_thumb() {
        let c = content(3, 20);
        let view = ScrollView::new(&c)
            .thumb_style(Style::new().fg(Color::Green))
            .offset(0, 0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 6));
        view.render(buf.area(), &mut buf);
        // Position 0 ⇒ the thumb sits at the top of the reserved column 3.
        let thumb = buf.get(Position::new(3, 0)).unwrap();
        assert_eq!(thumb.fg, Color::Green);
    }

    #[test]
    fn an_empty_content_buffer_is_a_safe_no_op_over_the_base_fill() {
        let c = Buffer::empty(Rect::new(0, 0, 0, 0));
        let view = ScrollView::new(&c).style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        view.render(buf.area(), &mut buf);
        // No content, no overflow, no bars — just the base fill, no panic.
        for p in buf.area().positions() {
            let cell = buf.get(p).unwrap();
            assert_eq!(cell.symbol, ' ');
            assert_eq!(cell.bg, Color::Blue);
        }
    }

    #[test]
    fn a_scroll_state_drives_the_axes_like_the_raw_offsets() {
        use rstui_core::ScrollState;

        let c = content(3, 10);
        // A streaming transcript pinned to the tail: on_content_change snaps
        // the offset to the end, and ScrollView shows the last window.
        let mut v = ScrollState::new();
        v.on_content_change(10, 2); // 10 rows, 2-row window -> offset 8
        let view = ScrollView::new(&c)
            .vertical_scroll(&v)
            .vertical_scrollbar(false);
        // Window == inner (3×2); offset 8 shows the last two rows.
        assert_eq!(view.viewport(Rect::new(0, 0, 3, 2)), Rect::new(0, 8, 3, 2));
        // Equivalent to driving the raw row_offset with the same value.
        assert_eq!(
            view.viewport(Rect::new(0, 0, 3, 2)),
            ScrollView::new(&c)
                .row_offset(8)
                .vertical_scrollbar(false)
                .viewport(Rect::new(0, 0, 3, 2)),
        );

        // Equivalent to the raw row_offset, and the horizontal dual matches
        // col_offset (compose two states for 2-D).
        let wide = content(20, 4);
        let mut h = ScrollState::default();
        h.set_offset(5);
        assert_eq!(
            ScrollView::new(&wide)
                .horizontal_scroll(&h)
                .vertical_scrollbar(false)
                .horizontal_scrollbar(false)
                .viewport(Rect::new(0, 0, 6, 4)),
            ScrollView::new(&wide)
                .col_offset(5)
                .vertical_scrollbar(false)
                .horizontal_scrollbar(false)
                .viewport(Rect::new(0, 0, 6, 4)),
        );
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let c = content(4, 4);
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        ScrollView::new(&c)
            .style(Style::new().bg(Color::Red))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(
            buf.cells()
                .iter()
                .all(|cell| cell.symbol == ' ' && cell.bg == Color::Reset)
        );
    }

    #[test]
    fn scrollbar_rects_expose_the_drawn_bars_for_mouse_hit_testing() {
        let area = Rect::new(0, 0, 12, 10);

        // Tall content ⇒ a vertical bar in the rightmost column.
        let tall = Buffer::empty(Rect::new(0, 0, 8, 60));
        let sv = ScrollView::new(&tall);
        let v = sv
            .vertical_scrollbar_rect(area)
            .expect("overflowing content shows a vertical bar");
        assert_eq!(v.x, area.right() - 1);
        assert_eq!(v.width, 1);
        assert!(v.height >= 1 && v.y == 0);
        // And it is exactly where `render` paints it (drift guard).
        let mut buf = Buffer::empty(area);
        ScrollView::new(&tall).render(area, &mut buf);
        assert_ne!(
            buf.get(Position::new(v.x, v.y)).unwrap().symbol,
            ' ',
            "the reported vertical-bar column is actually painted"
        );

        // Content that fits ⇒ no bars, both accessors `None`.
        let small = Buffer::empty(Rect::new(0, 0, 4, 3));
        let fits = ScrollView::new(&small);
        assert!(fits.vertical_scrollbar_rect(area).is_none());
        assert!(fits.horizontal_scrollbar_rect(area).is_none());
    }
}
