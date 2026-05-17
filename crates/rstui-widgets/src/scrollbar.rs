//! [`Scrollbar`] — a thin track-and-thumb adornment that shows how far a
//! scrollable view is scrolled, the visible companion to [`List`](crate::List)'s
//! `offset` and [`Paragraph`](crate::Paragraph)'s scroll position.
//!
//! # A pure projection of caller-owned scroll metrics
//!
//! ratatui's scrollbar is a `StatefulWidget` whose `render` takes
//! `&mut ScrollbarState`. rstui's `App::view` (in `rstui-runtime`) takes
//! `&self` — a view never mutates state — so, exactly as
//! [`List`](crate::List)/[`Tabs`](crate::Tabs)/[`Gauge`](crate::Gauge)
//! established, `Scrollbar` is a *pure projection*: the
//! [`content_length`](Scrollbar::content_length),
//! [`position`](Scrollbar::position), and
//! [`viewport_length`](Scrollbar::viewport_length) are ordinary caller-owned
//! numbers the widget only reads. The same `position` field a `List` or a
//! `Paragraph` scrolls with feeds straight into the scrollbar; the reducer owns
//! it, the widget reflects it.
//!
//! # Clamp, don't panic
//!
//! Per the cross-widget rule [`Gauge`](crate::Gauge) recorded — a pure
//! projection must be *total* — an out-of-range `position` is **clamped** to
//! the last item rather than aborting the program, and a zero
//! `content_length` renders nothing (there is nothing to scroll). A scroll
//! offset that briefly runs past the end from caller arithmetic must never
//! take down a whole TUI.
//!
//! # No `Block`, no lifetime — two deliberate divergences
//!
//! Unlike every widget so far, `Scrollbar` has **no optional framing
//! [`Block`](crate::Block)**: a scrollbar is a one-cell-wide *adornment* drawn
//! into the edge of an area (typically the right border column a `Block`
//! already drew), not a content container that frames something. Giving the
//! widget the full content area makes it pick the correct edge strip itself
//! (see [`ScrollbarOrientation`]).
//!
//! And because every part of a scrollbar (`║`, `█`, `▲`, `▼`) is a single
//! Unicode scalar, the single-`char` [`Cell`](rstui_core::Buffer) model means
//! `Scrollbar` carries **no borrowed text and so no lifetime parameter** — the
//! first widget that is a plain `Scrollbar`, not `Widget<'a>`. That is the same
//! single-`char` dividend borders (`Block`) and the eighth-block ramp
//! (`Gauge`) banked, here paying out as a simpler type signature.

use rstui_core::{Buffer, Position, Rect, Style, Widget};

/// Where a [`Scrollbar`] sits around the area it is given, which also fixes
/// the axis it scrolls along.
///
/// ```text
///            HorizontalTop
///              ┌───────┐
///  VerticalLeft│       │VerticalRight
///              └───────┘
///           HorizontalBottom
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollbarOrientation {
    /// Vertical, on the right edge column — the common case, the default.
    #[default]
    VerticalRight,
    /// Vertical, on the left edge column.
    VerticalLeft,
    /// Horizontal, on the bottom edge row.
    HorizontalBottom,
    /// Horizontal, on the top edge row.
    HorizontalTop,
}

impl ScrollbarOrientation {
    /// `true` for the two vertical orientations.
    #[must_use]
    pub const fn is_vertical(self) -> bool {
        matches!(self, Self::VerticalRight | Self::VerticalLeft)
    }

    /// `true` for the two horizontal orientations.
    #[must_use]
    pub const fn is_horizontal(self) -> bool {
        !self.is_vertical()
    }
}

/// A track-and-thumb scrollbar drawn along one edge of an area.
///
/// The widget reflects three caller-owned numbers it never mutates:
///
/// - [`content_length`](Self::content_length): total scrollable length (rows
///   in a list, lines in a paragraph). Zero renders nothing.
/// - [`position`](Self::position): the current scroll offset, **clamped** to
///   the last index — an over-scrolled value parks the thumb at the end, it
///   never panics.
/// - [`viewport_length`](Self::viewport_length): how much is visible at once;
///   the thumb's length is its fraction of the content. Left `0`, it defaults
///   to the strip's axis length (the right choice for one-row-per-item lists
///   where the scrollbar overlays the content area).
///
/// The bar is laid out along the axis the [`ScrollbarOrientation`] picks as:
/// an optional [`begin_symbol`](Self::begin_symbol) arrow, the track with the
/// thumb over its scrolled span, then an optional [`end_symbol`](Self::end_symbol)
/// arrow. The track and arrows take the base [`style`](Self::style); the thumb
/// takes [`thumb_style`](Self::thumb_style) patched over it. The thumb length
/// and start are computed with the ratatui-proven round-to-nearest integer
/// division, so the result is deterministic and float-free.
///
/// Given the whole content area it draws on the correct edge strip itself, so
/// it composes directly over a [`Block`](crate::Block)'s border column without
/// any margin arithmetic at the call site.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Scrollbar;
///
/// // The canonical list scrollbar: 10 items, scrolled to the top, drawn down
/// // a 1-wide × 10-tall strip with the end arrows removed. The viewport
/// // defaults to the strip length (10), so the thumb is 10/(9+10) of it ≈ the
/// // top 5 cells; the rest is the `║` track.
/// let mut buf = Buffer::empty(Rect::new(0, 0, 1, 10));
/// Scrollbar::default()
///     .content_length(10)
///     .position(0)
///     .begin_symbol(None)
///     .end_symbol(None)
///     .render(buf.area(), &mut buf);
///
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '█'); // thumb top
/// assert_eq!(buf.get(Position::new(0, 4)).unwrap().symbol, '█'); // thumb end
/// assert_eq!(buf.get(Position::new(0, 5)).unwrap().symbol, '║'); // track
/// assert_eq!(buf.get(Position::new(0, 9)).unwrap().symbol, '║'); // track end
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scrollbar {
    orientation: ScrollbarOrientation,
    content_length: usize,
    position: usize,
    viewport_length: usize,
    track_symbol: char,
    thumb_symbol: char,
    begin_symbol: Option<char>,
    end_symbol: Option<char>,
    style: Style,
    thumb_style: Style,
}

impl Default for Scrollbar {
    /// A [`ScrollbarOrientation::VerticalRight`] scrollbar with the default
    /// symbol set (a hand-written impl because the symbols depend on the
    /// orientation, so `#[derive(Default)]` cannot express it).
    fn default() -> Self {
        Self::new(ScrollbarOrientation::VerticalRight)
    }
}

impl Scrollbar {
    /// A scrollbar in `orientation`, with that orientation's default symbols
    /// (a doubled-line track `║`/`═`, a full-block `█` thumb, and `▲▼`/`◄►`
    /// arrows).
    ///
    /// Orientation is fixed at construction on purpose: it selects both the
    /// geometry *and* the matching symbol set, so there is no mid-chain
    /// "orientation changed but the vertical track glyph stayed" footgun
    /// ratatui has to paper over by resetting symbols. Override the glyphs
    /// afterwards with [`symbols`](Self::symbols) /
    /// [`begin_symbol`](Self::begin_symbol) / [`end_symbol`](Self::end_symbol)
    /// if you want a different look.
    #[must_use]
    pub const fn new(orientation: ScrollbarOrientation) -> Self {
        let (track_symbol, begin_symbol, end_symbol) = if orientation.is_vertical() {
            ('║', '▲', '▼')
        } else {
            ('═', '◄', '►')
        };
        Self {
            orientation,
            content_length: 0,
            position: 0,
            viewport_length: 0,
            track_symbol,
            thumb_symbol: '█',
            begin_symbol: Some(begin_symbol),
            end_symbol: Some(end_symbol),
            style: Style::new(),
            thumb_style: Style::new(),
        }
    }

    /// Sets the total scrollable length (e.g. a list's item count). Zero (the
    /// default) renders nothing — there is nothing to scroll.
    #[must_use]
    pub const fn content_length(mut self, content_length: usize) -> Self {
        self.content_length = content_length;
        self
    }

    /// Sets the current scroll offset. Values past the last index are
    /// **clamped** (the thumb parks at the end); this never panics.
    #[must_use]
    pub const fn position(mut self, position: usize) -> Self {
        self.position = position;
        self
    }

    /// Sets how much content is visible at once, which fixes the thumb's
    /// length as its fraction of [`content_length`](Self::content_length).
    ///
    /// Left `0` (the default) it falls back to the strip's length along its
    /// axis — the right behaviour for one-visual-row-per-item lists where the
    /// scrollbar overlays the content area, so the visible item count *is*
    /// that length.
    #[must_use]
    pub const fn viewport_length(mut self, viewport_length: usize) -> Self {
        self.viewport_length = viewport_length;
        self
    }

    /// Replaces the track and thumb glyphs.
    #[must_use]
    pub const fn symbols(mut self, track: char, thumb: char) -> Self {
        self.track_symbol = track;
        self.thumb_symbol = thumb;
        self
    }

    /// Sets (or with `None` removes) the arrow drawn before the track. A
    /// removed arrow gives that cell back to the track.
    #[must_use]
    pub const fn begin_symbol(mut self, symbol: Option<char>) -> Self {
        self.begin_symbol = symbol;
        self
    }

    /// Sets (or with `None` removes) the arrow drawn after the track. A
    /// removed arrow gives that cell back to the track.
    #[must_use]
    pub const fn end_symbol(mut self, symbol: Option<char>) -> Self {
        self.end_symbol = symbol;
        self
    }

    /// Toggles **both** end-cap arrows at once (a convenience over
    /// [`begin_symbol`](Self::begin_symbol) / [`end_symbol`](Self::end_symbol)).
    ///
    /// `true` restores this orientation's default caps (`▲`/`▼` vertical,
    /// `◄`/`►` horizontal); `false` removes both, handing those two cells
    /// back to the track. This never changes the constructor default (arrows
    /// **on**) — not calling it leaves existing behaviour unchanged; calling
    /// it after a per-end override replaces both ends.
    #[must_use]
    pub const fn arrows(mut self, arrows: bool) -> Self {
        if arrows {
            let (begin, end) = if self.orientation.is_vertical() {
                ('▲', '▼')
            } else {
                ('◄', '►')
            };
            self.begin_symbol = Some(begin);
            self.end_symbol = Some(end);
        } else {
            self.begin_symbol = None;
            self.end_symbol = None;
        }
        self
    }

    /// Sets the base [`Style`] for the track and the arrows.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the thumb [`Style`], patched over the base [`style`](Self::style)
    /// on the thumb cells only.
    #[must_use]
    pub const fn thumb_style(mut self, style: Style) -> Self {
        self.thumb_style = style;
        self
    }

    /// The one-cell-wide (or one-cell-tall) edge strip of `area` this
    /// orientation occupies. `area` is non-empty here, so the edge always
    /// exists.
    fn strip(&self, area: Rect) -> Rect {
        match self.orientation {
            ScrollbarOrientation::VerticalRight => {
                Rect::new(area.right() - 1, area.y, 1, area.height)
            }
            ScrollbarOrientation::VerticalLeft => Rect::new(area.x, area.y, 1, area.height),
            ScrollbarOrientation::HorizontalBottom => {
                Rect::new(area.x, area.bottom() - 1, area.width, 1)
            }
            ScrollbarOrientation::HorizontalTop => Rect::new(area.x, area.y, area.width, 1),
        }
    }

    /// The thumb + track geometry for `area`, or `None` when there is
    /// nothing to scroll/draw. The single source [`thumb_rect`](Self::thumb_rect)
    /// and [`position_at`](Self::position_at) share — the very metric
    /// [`render`](Widget::render) stamps (a consistency test pins them equal).
    fn thumb_geom(&self, area: Rect) -> Option<ThumbGeom> {
        if area.is_empty() || self.content_length == 0 {
            return None;
        }
        let strip = self.strip(area);
        let vertical = self.orientation.is_vertical();
        let axis_len = if vertical { strip.height } else { strip.width } as usize;
        let begin = usize::from(self.begin_symbol.is_some());
        let end = usize::from(self.end_symbol.is_some());
        let track_length = axis_len.saturating_sub(begin + end);
        if track_length == 0 {
            return None;
        }
        let viewport = if self.viewport_length == 0 {
            axis_len
        } else {
            self.viewport_length
        };
        let max_position = self.content_length - 1;
        let start_position = self.position.min(max_position);
        let max_viewport_position = max_position.saturating_add(viewport);
        let (thumb_start, thumb_length) = if max_viewport_position == 0 {
            (0, track_length)
        } else {
            let len = rounding_divide(viewport * track_length, max_viewport_position)
                .clamp(1, track_length);
            let start = rounding_divide(start_position * track_length, max_viewport_position)
                .clamp(0, track_length - 1);
            (start, len)
        };
        let off = (begin + thumb_start) as u16;
        let len = thumb_length as u16;
        let thumb = if vertical {
            Rect::new(strip.x, strip.y.saturating_add(off), 1, len)
        } else {
            Rect::new(strip.x.saturating_add(off), strip.y, len, 1)
        };
        Some(ThumbGeom {
            thumb,
            strip,
            vertical,
            begin,
            track_length,
            max_viewport_position,
        })
    }

    /// The rect the draggable **thumb** occupies in `area` (empty when there
    /// is nothing to scroll).
    ///
    /// Hit-test this on mouse-down to begin a thumb drag; it is exactly the
    /// cells [`render`](Widget::render) paints the thumb glyph into, so what
    /// the user grabs is what they see.
    #[must_use]
    pub fn thumb_rect(&self, area: Rect) -> Rect {
        self.thumb_geom(area).map_or(Rect::ZERO, |g| g.thumb)
    }

    /// The scroll [`position`](Self::position) that places the thumb under
    /// `pos` — feed this back as the new position while dragging the thumb,
    /// or on a click anywhere along the track to page there.
    ///
    /// Pure and **total**: the pointer is clamped onto the track and the
    /// result into `0..content_length`; a degenerate scrollbar returns the
    /// current [`position`](Self::position) unchanged.
    #[must_use]
    pub fn position_at(&self, area: Rect, pos: Position) -> usize {
        let Some(g) = self.thumb_geom(area) else {
            return self.position.min(self.content_length.saturating_sub(1));
        };
        let axis = usize::from(if g.vertical {
            pos.y.saturating_sub(g.strip.y)
        } else {
            pos.x.saturating_sub(g.strip.x)
        });
        let i = axis
            .saturating_sub(g.begin)
            .min(g.track_length.saturating_sub(1));
        let max_position = self.content_length - 1;
        if g.max_viewport_position == 0 {
            0
        } else {
            rounding_divide(i * g.max_viewport_position, g.track_length).min(max_position)
        }
    }
}

/// Shared thumb/track geometry — see [`Scrollbar::thumb_geom`].
struct ThumbGeom {
    thumb: Rect,
    strip: Rect,
    vertical: bool,
    begin: usize,
    track_length: usize,
    max_viewport_position: usize,
}

/// Round-to-nearest integer division (ties round up), the ratatui-proven thumb
/// metric. `denominator` is always non-zero at every call site.
fn rounding_divide(numerator: usize, denominator: usize) -> usize {
    (numerator + denominator / 2) / denominator
}

impl Widget for Scrollbar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Nothing to scroll, or nowhere to draw: a total no-op.
        if area.is_empty() || self.content_length == 0 {
            return;
        }
        let strip = self.strip(area);
        let vertical = self.orientation.is_vertical();
        let axis_len = if vertical { strip.height } else { strip.width } as usize;

        // The arrows bracket the track; each present one costs it a cell.
        let begin = usize::from(self.begin_symbol.is_some());
        let end = usize::from(self.end_symbol.is_some());
        let track_length = axis_len.saturating_sub(begin + end);
        if track_length == 0 {
            return;
        }

        // Default the viewport to the strip's full axis length — what the
        // visible row count is when the scrollbar overlays the content area
        // (it is an edge-column adornment, not a shortener). This is exactly
        // ratatui's documented fallback, so the thumb metric below matches its
        // proven vectors.
        let viewport = if self.viewport_length == 0 {
            axis_len
        } else {
            self.viewport_length
        };

        // The ratatui-proven integer thumb metric, clamped (never panicking)
        // per the Gauge cross-widget rule.
        let max_position = self.content_length - 1; // content_length > 0 here
        let start_position = self.position.min(max_position);
        let max_viewport_position = max_position.saturating_add(viewport);
        let (thumb_start, thumb_length) = if max_viewport_position == 0 {
            (0, track_length)
        } else {
            let len = rounding_divide(viewport * track_length, max_viewport_position)
                .clamp(1, track_length);
            let start = rounding_divide(start_position * track_length, max_viewport_position)
                .clamp(0, track_length - 1);
            (start, len)
        };
        let thumb_end = thumb_start + thumb_length;

        // Stamp the strip: begin arrow, then `track_length` track cells (the
        // thumb glyph over [thumb_start, thumb_end)), then end arrow. Indexing
        // each track cell directly (rather than ratatui's symbol-stream zip)
        // is inherently clip-safe at the saturated edge and simpler here.
        let thumb_style = self.style.patch(self.thumb_style);
        let cell = |buf: &mut Buffer, axis: u16, symbol: char, style: Style| {
            let pos = if vertical {
                Position::new(strip.x, strip.top().saturating_add(axis))
            } else {
                Position::new(strip.left().saturating_add(axis), strip.y)
            };
            buf.set_cell(pos, symbol, style);
        };

        if let Some(symbol) = self.begin_symbol {
            cell(buf, 0, symbol, self.style);
        }
        for i in 0..track_length {
            let axis = (begin + i) as u16;
            if i >= thumb_start && i < thumb_end {
                cell(buf, axis, self.thumb_symbol, thumb_style);
            } else {
                cell(buf, axis, self.track_symbol, self.style);
            }
        }
        if let Some(symbol) = self.end_symbol {
            cell(buf, (begin + track_length) as u16, symbol, self.style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier};

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row (the gauge-test helper).
    fn lines(widget: Scrollbar, width: u16, height: u16) -> String {
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

    /// The single rendered column (vertical) as a string.
    fn column(widget: Scrollbar, width: u16, height: u16, x: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        (0..height)
            .map(|y| buf.get(Position::new(x, y)).unwrap().symbol)
            .collect()
    }

    #[test]
    fn zero_content_length_renders_nothing() {
        // The default content_length is 0 ⇒ a total no-op even given an area.
        assert_eq!(lines(Scrollbar::default(), 1, 4), " \n \n \n \n");
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 4));
        Scrollbar::default().render(buf.area(), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 3));
        Scrollbar::default()
            .content_length(10)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn default_is_vertical_on_the_right_edge_column() {
        // A 3-wide area: the bar must be column 2, the other columns blank.
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 6));
        Scrollbar::default()
            .content_length(20)
            .position(0)
            .render(buf.area(), &mut buf);
        for y in 0..6 {
            assert_eq!(buf.get(Position::new(0, y)).unwrap().symbol, ' ');
            assert_eq!(buf.get(Position::new(1, y)).unwrap().symbol, ' ');
            assert_ne!(buf.get(Position::new(2, y)).unwrap().symbol, ' ');
        }
    }

    #[test]
    fn arrows_bracket_the_track_and_cost_it_a_cell_each() {
        // Height 6, both arrows ⇒ track is the middle 4 rows.
        let bar = column(
            Scrollbar::default().content_length(100).position(0),
            1,
            6,
            0,
        );
        let mut it = bar.chars();
        assert_eq!(it.next(), Some('▲')); // begin arrow
        assert_eq!(bar.chars().next_back(), Some('▼')); // end arrow
        // The 4 track cells contain the thumb (█) and track (║) glyphs only.
        for ch in bar.chars().skip(1).take(4) {
            assert!(ch == '█' || ch == '║', "unexpected track glyph {ch:?}");
        }
    }

    #[test]
    fn no_arrows_uses_the_full_axis_as_track() {
        // Removing both arrows hands their two cells back to the track, so a
        // height-4 bar is 4 track/thumb cells with no ▲/▼.
        let bar = column(
            Scrollbar::default()
                .content_length(100)
                .position(0)
                .begin_symbol(None)
                .end_symbol(None),
            1,
            4,
            0,
        );
        assert_eq!(bar.chars().count(), 4);
        assert!(!bar.contains('▲') && !bar.contains('▼'));
        assert!(bar.starts_with('█')); // position 0 ⇒ thumb at the top
    }

    #[test]
    fn a_single_item_is_a_full_length_thumb() {
        // content_length 1 ⇒ nothing to scroll ⇒ the thumb is the whole
        // track (the ratatui-proven `fullbar` case). Note this is *one item*,
        // not "content happens to fit": the proven metric uses
        // (content_length - 1) + viewport, so content == viewport is not a
        // full bar — only a single item is.
        let bar = column(
            Scrollbar::default()
                .content_length(1)
                .begin_symbol(None)
                .end_symbol(None),
            1,
            4,
            0,
        );
        assert_eq!(bar, "████");
    }

    #[test]
    fn the_thumb_sits_at_the_top_for_position_zero_and_the_end_for_the_last() {
        let make = |position| {
            Scrollbar::default()
                .content_length(20)
                .viewport_length(4)
                .position(position)
                .begin_symbol(None)
                .end_symbol(None)
        };
        // 20 items, 4 visible, 10-row track: thumb ≈ 2 cells.
        let top = column(make(0), 1, 10, 0);
        assert!(top.starts_with('█'));
        assert!(top.ends_with('║'));

        let bottom = column(make(19), 1, 10, 0);
        assert!(bottom.starts_with('║'));
        assert!(bottom.ends_with('█'));
    }

    #[test]
    fn a_mid_position_puts_the_thumb_in_the_middle() {
        // Halfway down a long list ⇒ thumb neither touches the top nor bottom.
        let bar = column(
            Scrollbar::default()
                .content_length(100)
                .viewport_length(10)
                .position(45)
                .begin_symbol(None)
                .end_symbol(None),
            1,
            10,
            0,
        );
        assert_eq!(bar.chars().next(), Some('║'));
        assert_eq!(bar.chars().next_back(), Some('║'));
        assert!(bar.contains('█'));
    }

    #[test]
    fn an_over_scrolled_position_is_clamped_not_panicking() {
        // position far past the end must park the thumb at the bottom, never
        // panic — the Gauge "a pure projection must be total" rule.
        let bar = column(
            Scrollbar::default()
                .content_length(20)
                .viewport_length(4)
                .position(9999)
                .begin_symbol(None)
                .end_symbol(None),
            1,
            8,
            0,
        );
        assert!(bar.ends_with('█'));
        assert!(!bar.starts_with('█'));
    }

    #[test]
    fn a_one_cell_axis_with_both_arrows_has_no_track_so_nothing_renders() {
        // Height 1, two arrows ⇒ track_length saturates to 0 ⇒ no-op (not a
        // panic, not a stray arrow on top of itself).
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        Scrollbar::default()
            .content_length(10)
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn vertical_left_draws_on_the_left_column() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 5));
        Scrollbar::new(ScrollbarOrientation::VerticalLeft)
            .content_length(20)
            .render(buf.area(), &mut buf);
        for y in 0..5 {
            assert_ne!(buf.get(Position::new(0, y)).unwrap().symbol, ' ');
            assert_eq!(buf.get(Position::new(2, y)).unwrap().symbol, ' ');
        }
    }

    #[test]
    fn horizontal_bottom_draws_along_the_bottom_row_with_horizontal_glyphs() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
            .content_length(40)
            .position(0)
            .render(buf.area(), &mut buf);
        // Top two rows untouched; the bar is the bottom row.
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().symbol, ' ');
            assert_eq!(buf.get(Position::new(x, 1)).unwrap().symbol, ' ');
        }
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, '◄');
        assert_eq!(buf.get(Position::new(7, 2)).unwrap().symbol, '►');
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, '█'); // thumb
    }

    #[test]
    fn horizontal_top_draws_along_the_top_row() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        Scrollbar::new(ScrollbarOrientation::HorizontalTop)
            .content_length(40)
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '◄');
        for x in 0..6 {
            assert_eq!(buf.get(Position::new(x, 2)).unwrap().symbol, ' ');
        }
    }

    #[test]
    fn the_base_style_paints_the_track_and_arrows_the_thumb_style_the_thumb() {
        let bar = Scrollbar::default()
            .content_length(20)
            .viewport_length(4)
            .position(0)
            .style(Style::new().fg(Color::Blue))
            .thumb_style(Style::new().fg(Color::Green).add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 8));
        bar.render(buf.area(), &mut buf);

        // Cell 0 is the begin arrow: base style.
        let arrow = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(arrow.symbol, '▲');
        assert_eq!(arrow.fg, Color::Blue);

        // The first track cell (row 1) holds the thumb at position 0: the
        // thumb_style is patched over the base, so fg flips to Green + BOLD.
        let thumb = buf.get(Position::new(0, 1)).unwrap();
        assert_eq!(thumb.symbol, '█');
        assert_eq!(thumb.fg, Color::Green);
        assert!(thumb.modifier.contains(Modifier::BOLD));

        // A track cell below the thumb keeps the base style.
        let track = buf.get(Position::new(0, 6)).unwrap();
        assert_eq!(track.symbol, '║');
        assert_eq!(track.fg, Color::Blue);
    }

    #[test]
    fn custom_symbols_replace_the_defaults() {
        let bar = column(
            Scrollbar::default()
                .content_length(4)
                .symbols('.', '#')
                .begin_symbol(Some('^'))
                .end_symbol(Some('v')),
            1,
            6,
            0,
        );
        assert_eq!(bar.chars().next(), Some('^'));
        assert_eq!(bar.chars().next_back(), Some('v'));
        assert!(bar.contains('#')); // custom thumb
    }

    #[test]
    fn arrows_false_removes_both_end_caps() {
        // .arrows(false) == begin_symbol(None).end_symbol(None): a 4-tall bar
        // is 4 track/thumb cells with no ▲/▼.
        let bar = column(
            Scrollbar::default()
                .content_length(100)
                .position(0)
                .arrows(false),
            1,
            4,
            0,
        );
        assert_eq!(bar.chars().count(), 4);
        assert!(!bar.contains('▲') && !bar.contains('▼'));
        assert!(bar.starts_with('█'));
    }

    #[test]
    fn arrows_true_restores_the_orientation_default_caps() {
        // Re-enabling after a removal brings back this orientation's arrows.
        let v = column(
            Scrollbar::default()
                .content_length(100)
                .begin_symbol(None)
                .end_symbol(None)
                .arrows(true),
            1,
            6,
            0,
        );
        assert_eq!(v.chars().next(), Some('▲'));
        assert_eq!(v.chars().next_back(), Some('▼'));

        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Scrollbar::new(ScrollbarOrientation::HorizontalTop)
            .content_length(40)
            .arrows(true)
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '◄');
        assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, '►');
    }

    #[test]
    fn horizontal_new_uses_horizontal_default_glyphs() {
        // new(Horizontal*) must pick ═/◄/► not the vertical ║/▲/▼ — the
        // constructor-fixed-symbols contract (no orientation-reset footgun).
        let s = Scrollbar::new(ScrollbarOrientation::HorizontalTop);
        assert_eq!(s.track_symbol, '═');
        assert_eq!(s.begin_symbol, Some('◄'));
        assert_eq!(s.end_symbol, Some('►'));
        assert!(s.orientation.is_horizontal());
        assert!(!s.orientation.is_vertical());
    }

    #[test]
    fn thumb_rect_is_exactly_the_rendered_thumb() {
        let area = Rect::new(0, 0, 1, 12);
        let sb = Scrollbar::default()
            .content_length(40)
            .viewport_length(8)
            .position(12);
        // Where the seam says the thumb is…
        let claimed = sb.thumb_rect(area);
        // …must be exactly where `render` paints the thumb glyph.
        let mut buf = Buffer::empty(area);
        sb.clone().render(area, &mut buf);
        let mut cells: Vec<Position> = Vec::new();
        for y in 0..area.height {
            for x in 0..area.width {
                if buf.get(Position::new(x, y)).unwrap().symbol == '█' {
                    cells.push(Position::new(x, y));
                }
            }
        }
        assert!(!cells.is_empty(), "the thumb is drawn");
        let min_y = cells.iter().map(|p| p.y).min().unwrap();
        let max_y = cells.iter().map(|p| p.y).max().unwrap();
        let drawn = Rect::new(0, min_y, 1, max_y - min_y + 1);
        assert_eq!(claimed, drawn, "thumb_rect must match the painted thumb");
    }

    #[test]
    fn position_at_inverts_the_track_and_is_total() {
        let area = Rect::new(0, 0, 1, 12);
        let sb = Scrollbar::default().content_length(40).viewport_length(8);
        // Top of the track ⇒ scrolled to the very start.
        assert_eq!(sb.position_at(area, Position::new(0, 0)), 0);
        // Bottom (and far past it) ⇒ clamped to the last index, no panic.
        assert_eq!(sb.position_at(area, Position::new(0, 11)), 39);
        assert_eq!(sb.position_at(area, Position::new(0, 9999)), 39);
        // Monotonic: a lower cell never scrolls less than a higher one.
        let mid = sb.position_at(area, Position::new(0, 6));
        assert!((0..=39).contains(&mid));
        assert!(sb.position_at(area, Position::new(0, 5)) <= mid);
        // Degenerate scrollbar ⇒ the current position, never a panic.
        assert_eq!(
            Scrollbar::default().position_at(area, Position::new(0, 4)),
            0
        );
    }
}
