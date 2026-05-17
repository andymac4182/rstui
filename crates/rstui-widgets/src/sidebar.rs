//! [`Sidebar`] — an application navigation rail: a vertical list of
//! icon+label items with optional collapsible group headers, in an expanded or
//! a narrow (icon-only) state.
//!
//! # A pure projection of caller-owned `selected` + `collapsed`
//!
//! Like every rstui widget `Sidebar` is a **pure projection**: it renders the
//! caller-owned `&[SidebarItem]` it is handed plus a caller-owned
//! [`selected`](Sidebar::selected) index, [`offset`](Sidebar::offset), and
//! [`collapsed`](Sidebar::collapsed) flag — ordinary application state the
//! reducer owns and moves in `update`. *Which* item is active, *whether* the
//! rail is collapsed, and *committing* a navigation are all the reducer's job;
//! the widget only ever reads these fields, exactly the read-only-state rule
//! [`List`]'s `selected`/`offset` and [`Menu`](crate::Menu)'s `highlight`
//! establish.
//!
//! # It **reuses [`List`]** for the column — and is *not* opaque
//!
//! A sidebar is an in-layout navigation pane, **not** a float over unrelated
//! content, so — unlike [`Modal`](crate::Modal)/[`Menu`](crate::Menu) — it does
//! **not** [`clear_region`](rstui_core::Buffer::clear_region); it draws into
//! the area it is laid out in like any container. Each item is projected to one
//! [`Line`] (the icon, a space, then the label when expanded; just the icon —
//! or the label's first character as a fallback — when collapsed) and handed to
//! an internal [`List`], so scrolling ([`offset`](Sidebar::offset)), the
//! full-row selection bar, the optional framing [`Block`], and totality are
//! **inherited**, never re-implemented — the same wholesale reuse
//! [`Menu`](crate::Menu) and [`Select`](crate::Select) make.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no items, an out-of-range [`selected`](Sidebar::selected) (no bar,
//! inherited from `List`), an [`offset`](Sidebar::offset) past the end, a
//! collapsed item with no icon (its label's first character is the rail
//! glyph), and a rail narrower than its labels (clipped by `List`) are all safe
//! clips/no-ops — never a panic.

use rstui_core::{Buffer, Line, Rect, Span, Style, Widget};

use crate::block::Block;
use crate::list::List;

/// The glyph a collapsed [`SidebarItem::group`] header is shown as in the
/// narrow rail (a label is meaningless one character wide, so a divider keeps
/// the visual grouping).
const COLLAPSED_GROUP_RULE: char = '─';

/// One row of a [`Sidebar`]: a navigation item (an optional icon plus a
/// [`Line`] label) or a non-selectable group header.
///
/// Build an item from anything a [`Line`] is built from (the
/// [`ListItem`](crate::ListItem) `From` family), add an icon with
/// [`icon`](SidebarItem::icon), or make a group header with
/// [`SidebarItem::group`]. Whether the keyboard skips a header is the reducer's
/// job (the widget still paints the selection bar wherever
/// [`Sidebar::selected`] points — the [`Menu`](crate::Menu) separator stance).
#[derive(Debug, Clone)]
pub struct SidebarItem<'a> {
    label: Line<'a>,
    icon: Option<char>,
    is_group: bool,
}

impl<'a> SidebarItem<'a> {
    /// A navigation item displaying `label` (anything convertible to a
    /// [`Line`]), with no icon.
    pub fn new(label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            is_group: false,
        }
    }

    /// A non-selectable group header labelling the items beneath it (drawn
    /// with [`Sidebar::group_style`]).
    #[must_use]
    pub fn group(label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            is_group: true,
        }
    }

    /// Sets the leading icon glyph (shown alone in the collapsed rail).
    #[must_use]
    pub fn icon(mut self, icon: char) -> Self {
        self.icon = Some(icon);
        self
    }
}

impl<'a> From<&'a str> for SidebarItem<'a> {
    fn from(s: &'a str) -> Self {
        Self::new(s)
    }
}

impl From<String> for SidebarItem<'_> {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl<'a> From<Line<'a>> for SidebarItem<'a> {
    fn from(line: Line<'a>) -> Self {
        Self::new(line)
    }
}

/// A vertical application navigation rail — a pure projection of caller-owned
/// `items` + [`selected`](Self::selected) + [`collapsed`](Self::collapsed).
///
/// Each item is projected to one row (icon + label expanded; icon-only
/// collapsed) and rendered through an internal [`List`], so scrolling, the
/// selection bar, the optional [`block`](Self::block) frame, and totality are
/// inherited. Unlike a floating menu it is **not** opaque — it is an in-layout
/// pane.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Sidebar, SidebarItem};
///
/// // `selected`/`collapsed` are plain caller-owned model state the widget
/// // only reads — moving the selection and committing navigation are the
/// // reducer's job, never the widget's.
/// let items = [
///     SidebarItem::group("MAIN"),
///     SidebarItem::new("Files").icon('*'),
/// ];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
/// Sidebar::new(&items).selected(Some(1)).render(buf.area(), &mut buf);
///
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'M'); // group header
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '*'); // icon
/// assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, 'F'); // "Files"
/// ```
#[derive(Debug, Clone)]
pub struct Sidebar<'a> {
    items: &'a [SidebarItem<'a>],
    selected: Option<usize>,
    collapsed: bool,
    offset: usize,
    block: Option<Block<'a>>,
    style: Style,
    highlight_style: Style,
    group_style: Style,
}

impl<'a> Sidebar<'a> {
    /// A rail projecting `items`: nothing selected, expanded, scrolled to the
    /// top, unframed and unstyled.
    #[must_use]
    pub fn new(items: &'a [SidebarItem<'a>]) -> Self {
        Self {
            items,
            selected: None,
            collapsed: false,
            offset: 0,
            block: None,
            style: Style::new(),
            highlight_style: Style::new(),
            group_style: Style::new(),
        }
    }

    /// Sets which row the selection bar is on — caller-owned state the widget
    /// only reads. Out of range simply paints no bar (inherited from
    /// [`List`]).
    #[must_use]
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets whether the rail is collapsed to a narrow icon-only column —
    /// caller-owned state the widget only reads (toggle it in `update`).
    #[must_use]
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// Sets the index of the first visible row (the scroll offset), exactly
    /// [`List::offset`](crate::List::offset).
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Frames the rail in `block`; rows render into
    /// [`block.inner`](Block::inner), the same compose pattern [`List`] uses.
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`], beneath the row/label/span cascade; it also
    /// fills the content area so a background covers the whole rail.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched **last** over the selected row (one
    /// full-width bar), forwarded straight to the internal [`List`].
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Sets the [`Style`] of [`group`](SidebarItem::group) header rows, patched
    /// over the header label's own style.
    #[must_use]
    pub fn group_style(mut self, style: Style) -> Self {
        self.group_style = style;
        self
    }

    /// Projects one [`SidebarItem`] to its [`Line`] row: a group header is its
    /// label (a divider rule when collapsed); a normal item is `icon label`
    /// expanded, or just the icon (or the label's first character) collapsed.
    fn row(&self, item: &SidebarItem<'a>) -> Line<'a> {
        if item.is_group {
            if self.collapsed {
                return Line::raw(COLLAPSED_GROUP_RULE.to_string()).style(self.group_style);
            }
            return item
                .label
                .clone()
                .style(item.label.style.patch(self.group_style));
        }

        if self.collapsed {
            let glyph = item.icon.or_else(|| {
                item.label
                    .spans
                    .iter()
                    .flat_map(|s| s.content.chars())
                    .next()
            });
            return match glyph {
                Some(c) => Line::raw(c.to_string()).style(item.label.style),
                None => Line::default(),
            };
        }

        let mut spans = Vec::new();
        if let Some(icon) = item.icon {
            spans.push(Span::raw(format!("{icon} ")));
        }
        spans.extend(item.label.spans.iter().cloned());
        Line::from(spans).style(item.label.style)
    }
}

impl Widget for Sidebar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // Project every item to a row, then reuse `List` wholesale: scrolling,
        // the full-row selection bar, the frame, and totality are inherited,
        // never re-implemented (the Menu/Select precedent). Not opaque — a
        // sidebar is an in-layout pane, not a float.
        // SB-1: build only the window `List` will actually show — it
        // renders `items[offset, offset + inner.height)` from a pure
        // projection of `(items, selected, offset)` (no `len()`-derived
        // state), so the windowed slice + zero offset + rebased selection
        // is byte-identical (the offset/selection snapshot tests
        // gate-enforce it) while a long collapsed nav tree never builds
        // its off-screen rows. `inner` mirrors what `List::render` derives
        // from the block it is hereafter given.
        let h = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        }
        .height as usize;
        let start = self.offset.min(self.items.len());
        let end = self.offset.saturating_add(h).min(self.items.len());
        let rows: Vec<Line<'_>> = self.items[start..end]
            .iter()
            .map(|it| self.row(it))
            .collect();
        let mut list = List::new(rows)
            .selected(self.selected.and_then(|s| s.checked_sub(start)))
            .offset(0)
            .style(self.style)
            .highlight_style(self.highlight_style);
        if let Some(block) = self.block {
            list = list.block(block);
        }
        list.render(area, buf);
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
    fn expanded_shows_icon_space_label_per_row() {
        let items = [
            SidebarItem::new("Files").icon('*'),
            SidebarItem::new("Search"),
        ];
        // Icon + space + label; an icon-less item is just the label.
        assert_eq!(lines(Sidebar::new(&items), 8, 2), "* Files \nSearch  \n");
    }

    #[test]
    fn collapsed_shows_only_the_icon() {
        let items = [SidebarItem::new("Files").icon('*')];
        assert_eq!(lines(Sidebar::new(&items).collapsed(true), 4, 1), "*   \n");
    }

    #[test]
    fn collapsed_without_an_icon_falls_back_to_the_label_first_char() {
        let items = [SidebarItem::new("Search")];
        assert_eq!(lines(Sidebar::new(&items).collapsed(true), 3, 1), "S  \n");
    }

    #[test]
    fn a_group_header_takes_the_group_style() {
        let items = [SidebarItem::group("MAIN")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Sidebar::new(&items)
            .group_style(Style::new().fg(Color::DarkGray))
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'M');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::DarkGray);
    }

    #[test]
    fn a_collapsed_group_header_is_a_short_rule() {
        let items = [
            SidebarItem::group("MAIN"),
            SidebarItem::new("Files").icon('*'),
        ];
        assert_eq!(
            lines(Sidebar::new(&items).collapsed(true), 2, 2),
            "─ \n* \n"
        );
    }

    #[test]
    fn the_selected_row_gets_the_full_width_selection_bar() {
        let items = [SidebarItem::new("a"), SidebarItem::new("b")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Sidebar::new(&items)
            .selected(Some(1))
            .highlight_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for x in 0..4 {
            assert_eq!(buf.get(Position::new(x, 1)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn offset_scrolls_the_rail() {
        let items = [
            SidebarItem::new("i0"),
            SidebarItem::new("i1"),
            SidebarItem::new("i2"),
        ];
        assert_eq!(lines(Sidebar::new(&items).offset(1), 2, 2), "i1\ni2\n");
    }

    #[test]
    fn windowed_render_is_byte_identical_to_list_over_all_rows() {
        // SB-1 gate (PG-2/CM-3 exactness discipline): the caller-side
        // windowing must equal `List` — which Sidebar delegates to — over
        // the full row set at the same offset, including a selection on a
        // scrolled row and offsets past the end.
        let items: Vec<SidebarItem> = (0..8).map(|i| SidebarItem::new(format!("i{i}"))).collect();
        let hl = Style::new().bg(Color::Blue);
        for &(off, sel, h) in &[
            (0usize, None::<usize>, 3u16),
            (2, Some(3), 2),
            (1, Some(0), 2),
            (3, Some(9), 2),
            (5, Some(6), 4),
            (0, Some(7), 8),
            (9, None, 2),
        ] {
            let area = Rect::new(0, 0, 6, h);
            let sb = Sidebar::new(&items)
                .selected(sel)
                .offset(off)
                .highlight_style(hl);
            let full: Vec<Line<'_>> = items.iter().map(|it| sb.row(it)).collect();
            let mut want = Buffer::empty(area);
            List::new(full)
                .selected(sel)
                .offset(off)
                .highlight_style(hl)
                .render(area, &mut want);
            let mut got = Buffer::empty(area);
            Sidebar::new(&items)
                .selected(sel)
                .offset(off)
                .highlight_style(hl)
                .render(area, &mut got);
            assert_eq!(
                got.cells(),
                want.cells(),
                "windowed Sidebar diverged from List-over-all-rows: off={off} sel={sel:?} h={h}"
            );
        }
    }

    #[test]
    fn an_out_of_range_selection_paints_no_bar() {
        let items = [SidebarItem::new("a"), SidebarItem::new("b")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        Sidebar::new(&items)
            .selected(Some(9))
            .highlight_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Reset);
            }
        }
    }

    #[test]
    fn a_block_frames_the_rail() {
        let items = [SidebarItem::new("Hi")];
        assert_eq!(
            lines(Sidebar::new(&items).block(Block::bordered()), 4, 3),
            "┌──┐\n│Hi│\n└──┘\n"
        );
    }

    #[test]
    fn style_cascades_and_fills_the_whole_rail() {
        let items = [SidebarItem::new("x")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        Sidebar::new(&items)
            .style(Style::new().bg(Color::Red))
            .render(buf.area(), &mut buf);
        // The single item is row 0; the empty row 1 is still filled.
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
    }

    #[test]
    fn an_empty_sidebar_with_a_block_still_renders_the_frame() {
        let items: [SidebarItem<'_>; 0] = [];
        assert_eq!(
            lines(Sidebar::new(&items).block(Block::bordered()), 3, 3),
            "┌─┐\n│ │\n└─┘\n"
        );
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let items = [SidebarItem::new("a")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Sidebar::new(&items).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
