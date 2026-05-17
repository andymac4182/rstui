//! [`KeymapView`] — an interactive keybinding **table**: a caller-owned list
//! of [`KeymapRow`]s (a label, a key cluster, an optional id, a per-row
//! [`RowState`]) laid out as aligned columns with a selection cursor and a
//! live *capture* affordance — the reusable "see and rebind your keys" panel
//! every app needs and no app should re-implement.
//!
//! # A pure projection that reuses [`Kbd`], and is engine-agnostic
//!
//! `KeymapView` owns no state. The app's reducer owns the selection, the
//! capture state machine, and the live keymap; the widget is a caller-owned
//! `&[KeymapRow]` projected to glyphs — the headless-testable shape every
//! widget here uses — and it reports the row under a click via
//! [`hit`](KeymapView::hit) so the reducer can move the cursor or arm a
//! rebind. It deliberately depends on **no keymap engine**: any
//! `(label, keys, state)` source drives it, so `rstui-widgets` stays
//! `rstui-core`-only (ADR 0002, the widget-crate boundary). An app adapts its
//! own keymap (e.g. `rstui-keymap`'s `keys_for`/`Action::help`) into rows in
//! `view`; the widget never learns what a keymap *is*.
//!
//! Each row's key cluster is rendered by **reusing [`Kbd`] wholesale** (its
//! caps / clipping / totality inherited rather than re-implemented), the
//! optional frame by [`Block`], exactly as [`HelpOverlay`](crate::HelpOverlay)
//! does — `KeymapView` is its interactive sibling (selectable rows, a
//! capture cursor) rather than a static cheat-sheet.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*):
//! no rows, a zero area, an inner too small for the columns (they clip), a
//! scroll offset past the end, a header/footer that does not fit, and an
//! out-of-range selection are all safe clips/no-ops — never a panic.

use std::borrow::Cow;

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::kbd::Kbd;

/// How one [`KeymapRow`] is drawn — the four states a rebindable key has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowState {
    /// An ordinary, bound row.
    #[default]
    Normal,
    /// The row the selection cursor is on (gets the cursor glyph + accent).
    Selected,
    /// This row is *armed for capture*: the next key the app reads rebinds
    /// it. Its key column shows the capture placeholder in the accent.
    Capturing,
    /// The action is unbound/disabled (`keys_for` → `—`); drawn dimmed.
    Disabled,
}

/// One row of a [`KeymapView`]: a description, the keys it is bound to (each
/// a [`Kbd`] cap), an optional stable id column, and a [`RowState`].
///
/// The `label` is any value convertible to a [`Line`] so it carries its own
/// per-span styles; the `keys` are projected through [`Kbd`] (so
/// `["Ctrl", "K"]` renders as `[Ctrl] [K]`, OS-aware caps the caller chooses).
#[derive(Debug, Clone)]
pub struct KeymapRow<'a> {
    label: Line<'a>,
    keys: Vec<Cow<'a, str>>,
    id: Option<Cow<'a, str>>,
    state: RowState,
}

impl<'a> KeymapRow<'a> {
    /// A row binding `label` to `keys` (each a [`Kbd`] cap), [`Normal`].
    ///
    /// [`Normal`]: RowState::Normal
    pub fn new<I, T>(label: impl Into<Line<'a>>, keys: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Cow<'a, str>>,
    {
        Self {
            label: label.into(),
            keys: keys.into_iter().map(Into::into).collect(),
            id: None,
            state: RowState::Normal,
        }
    }

    /// Sets the stable id shown in the (optional) middle column
    /// (`app.palette`) — what a config file keys an override by.
    #[must_use]
    pub fn id(mut self, id: impl Into<Cow<'a, str>>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the [`RowState`].
    #[must_use]
    pub fn state(mut self, state: RowState) -> Self {
        self.state = state;
        self
    }
}

/// An interactive, optionally-framed keybinding table with a selection
/// cursor and a capture affordance.
///
/// Render it into the area it should occupy (a panel, a
/// [`Drawer`](crate::Drawer) body, a [`Modal`](crate::Modal) — the caller
/// places it). It paints an optional [`backdrop_style`](Self::backdrop_style)
/// scrim, **clears** its box opaque (so it is usable floated, exactly like
/// [`HelpOverlay`](crate::HelpOverlay)), draws the optional framing
/// [`block`](Self::block), an optional [`header`](Self::header) line, the
/// rows in aligned columns (cursor · label · optional id · keys), and an
/// optional [`footer`](Self::footer) line (the capture prompt / legend).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{KeymapRow, KeymapView, RowState};
///
/// let rows = [
///     KeymapRow::new("Command palette", ["Ctrl", "K"])
///         .id("app.palette")
///         .state(RowState::Selected),
///     KeymapRow::new("Quit", ["q"]).id("app.quit"),
/// ];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 40, 6));
/// KeymapView::new(&rows)
///     .header("Vim · macOS")
///     .footer("↑↓ select · ⏎ rebind · x disable")
///     .render(buf.area(), &mut buf);
///
/// // The selected row carries the cursor glyph.
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '▶');
/// // A click on the second row maps back to row index 1.
/// let v = KeymapView::new(&rows).header("Vim · macOS");
/// assert_eq!(v.hit(Rect::new(0, 0, 40, 6), Position::new(5, 2)), Some(1));
/// ```
#[derive(Debug, Clone)]
pub struct KeymapView<'a> {
    rows: &'a [KeymapRow<'a>],
    block: Option<Block<'a>>,
    header: Option<Line<'a>>,
    footer: Option<Line<'a>>,
    scroll: usize,
    column_gap: u16,
    separator: Cow<'a, str>,
    cursor: Cow<'a, str>,
    capture_label: Cow<'a, str>,
    style: Style,
    backdrop_style: Style,
    label_style: Style,
    id_style: Style,
    key_style: Style,
    selected_style: Style,
    capturing_style: Style,
    disabled_style: Style,
}

impl<'a> KeymapView<'a> {
    /// A table of `rows`: unframed, opaque, no header/footer, no scroll, a
    /// `▶ ` cursor, the id column shown when any row carries one.
    #[must_use]
    pub fn new(rows: &'a [KeymapRow<'a>]) -> Self {
        Self {
            rows,
            block: None,
            header: None,
            footer: None,
            scroll: 0,
            column_gap: 2,
            separator: Cow::Borrowed(" "),
            cursor: Cow::Borrowed("▶ "),
            capture_label: Cow::Borrowed("…"),
            style: Style::new(),
            backdrop_style: Style::new(),
            label_style: Style::new(),
            id_style: Style::new(),
            key_style: Style::new(),
            selected_style: Style::new(),
            capturing_style: Style::new(),
            disabled_style: Style::new(),
        }
    }

    /// Sets the framing [`Block`]; the table renders into its
    /// [`inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the one-line summary drawn above the rows (e.g.
    /// `"Vim · macOS · leader ⌃X"`).
    #[must_use]
    pub fn header(mut self, header: impl Into<Line<'a>>) -> Self {
        self.header = Some(header.into());
        self
    }

    /// Sets the one-line hint/legend drawn below the rows (e.g. the capture
    /// prompt `"● press a key — Esc cancels"`).
    #[must_use]
    pub fn footer(mut self, footer: impl Into<Line<'a>>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    /// Sets the first visible row — caller-owned windowing (the
    /// pure-projection answer to scrolling: only `[scroll, scroll+rows)` is
    /// drawn, the reducer owns the offset).
    #[must_use]
    pub fn scroll(mut self, first: usize) -> Self {
        self.scroll = first;
        self
    }

    /// Sets the blank columns between adjacent columns (default `2`).
    #[must_use]
    pub fn column_gap(mut self, gap: u16) -> Self {
        self.column_gap = gap;
        self
    }

    /// Sets the string each row's [`Kbd`] joins its caps with (default a
    /// single space; `"+"` for the `[Ctrl]+[K]` look).
    #[must_use]
    pub fn separator(mut self, separator: impl Into<Cow<'a, str>>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Sets the cursor glyph drawn before a [`Selected`]/[`Capturing`] row
    /// (default `"▶ "`); its width reserves the gutter on every row so the
    /// labels stay aligned.
    ///
    /// [`Selected`]: RowState::Selected
    /// [`Capturing`]: RowState::Capturing
    #[must_use]
    pub fn cursor(mut self, cursor: impl Into<Cow<'a, str>>) -> Self {
        self.cursor = cursor.into();
        self
    }

    /// Sets the placeholder shown in a [`Capturing`](RowState::Capturing)
    /// row's key column (default `"…"`).
    #[must_use]
    pub fn capture_label(mut self, label: impl Into<Cow<'a, str>>) -> Self {
        self.capture_label = label.into();
        self
    }

    /// Sets the [`Style`] filling the (already-cleared) box, beneath the
    /// frame and rows.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the scrim [`Style`] patched over the whole area (opt-in dimming,
    /// like [`HelpOverlay::backdrop_style`](crate::HelpOverlay::backdrop_style)).
    #[must_use]
    pub fn backdrop_style(mut self, style: Style) -> Self {
        self.backdrop_style = style;
        self
    }

    /// Sets the base [`Style`] of the label column.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Sets the [`Style`] of the id column.
    #[must_use]
    pub fn id_style(mut self, style: Style) -> Self {
        self.id_style = style;
        self
    }

    /// Sets the [`Style`] of each row's key caps (forwarded to the reused
    /// [`Kbd`]).
    #[must_use]
    pub fn key_style(mut self, style: Style) -> Self {
        self.key_style = style;
        self
    }

    /// Sets the [`Style`] patched over a [`Selected`](RowState::Selected)
    /// row.
    #[must_use]
    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Sets the [`Style`] patched over a [`Capturing`](RowState::Capturing)
    /// row.
    #[must_use]
    pub fn capturing_style(mut self, style: Style) -> Self {
        self.capturing_style = style;
        self
    }

    /// Sets the [`Style`] patched over a [`Disabled`](RowState::Disabled)
    /// row.
    #[must_use]
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// The content rect inside the optional [`block`](Self::block) (or the
    /// whole `area` when there is none) — a pure derived rect.
    #[must_use]
    pub fn inner(&self, area: Rect) -> Rect {
        match &self.block {
            Some(b) => b.inner(area),
            None => area,
        }
    }

    /// The y of the first table row within `area` (below the frame and the
    /// optional header).
    fn table_top(&self, area: Rect) -> u16 {
        let inner = self.inner(area);
        inner.top().saturating_add(u16::from(self.header.is_some()))
    }

    /// How many rows are visible in `area` (inner height minus the header
    /// and footer lines).
    fn table_height(&self, area: Rect) -> u16 {
        let inner = self.inner(area);
        inner
            .height
            .saturating_sub(u16::from(self.header.is_some()))
            .saturating_sub(u16::from(self.footer.is_some()))
    }

    /// The source row index under `pos`, or `None` for the frame, the
    /// header/footer, an empty gap below the last row, or outside the table.
    /// The inverse of the scroll-windowed layout, for click-to-select /
    /// click-to-rebind.
    #[must_use]
    pub fn hit(&self, area: Rect, pos: Position) -> Option<usize> {
        let inner = self.inner(area);
        if !inner.contains(pos) {
            return None;
        }
        let top = self.table_top(area);
        let h = self.table_height(area);
        if pos.y < top || pos.y >= top.saturating_add(h) {
            return None;
        }
        let row = self.scroll.checked_add(usize::from(pos.y - top))?;
        (row < self.rows.len()).then_some(row)
    }

    /// The reused [`Kbd`] for a row's keys, carrying this view's separator
    /// and (state-patched) key style.
    fn kbd<I, T>(&self, keys: I, base: Style) -> Kbd<'_>
    where
        I: IntoIterator<Item = T>,
        T: Into<Cow<'a, str>>,
    {
        Kbd::new(keys.into_iter().map(Into::into).collect::<Vec<_>>())
            .separator(self.separator.clone())
            .style(base)
            .key_style(self.key_style)
    }
}

/// The display width of a [`Line`] in columns (one column per `char`).
fn line_width(line: &Line<'_>) -> u16 {
    let n: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
    u16::try_from(n).unwrap_or(u16::MAX)
}

/// Stamps a [`Line`] at `(x, y)`, clipped at `right`, cascading
/// `base → line → span`. Returns the x after the last glyph.
fn put_line(buf: &mut Buffer, mut x: u16, y: u16, right: u16, base: Style, line: &Line<'_>) -> u16 {
    let line_base = base.patch(line.style);
    for span in &line.spans {
        let st = line_base.patch(span.style);
        for ch in span.content.chars() {
            if x >= right {
                return x;
            }
            buf.set_cell(Position::new(x, y), ch, st);
            x = x.saturating_add(1);
        }
    }
    x
}

/// Stamps `s` at `(x, y)`, clipped at `right`; returns the x after it.
fn put_str(buf: &mut Buffer, mut x: u16, y: u16, right: u16, st: Style, s: &str) -> u16 {
    for ch in s.chars() {
        if x >= right {
            break;
        }
        buf.set_cell(Position::new(x, y), ch, st);
        x = x.saturating_add(1);
    }
    x
}

impl Widget for KeymapView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        // 1. Scrim, then an opaque, styled box (the `HelpOverlay` idiom, so
        //    the panel is safe floated over content too).
        buf.set_style(area, self.backdrop_style);
        buf.clear_region(area);
        buf.set_style(area, self.style);

        // 2. Optional frame; everything else goes into `inner`.
        let inner = self.inner(area);
        if let Some(b) = self.block.clone() {
            b.render(area, buf);
        }
        if inner.is_empty() {
            return;
        }
        let right = inner.right();

        // 3. Header / footer lines (clipped, total).
        if let Some(h) = &self.header {
            put_line(buf, inner.left(), inner.top(), right, self.style, h);
        }
        if let Some(f) = &self.footer {
            let fy = inner.bottom().saturating_sub(1);
            put_line(buf, inner.left(), fy, right, self.style, f);
        }

        // 4. Column geometry: cursor gutter, label column sized to the
        //    widest label, optional id column sized to the widest id.
        let cursor_w = u16::try_from(self.cursor.chars().count()).unwrap_or(0);
        let label_col = self
            .rows
            .iter()
            .map(|r| line_width(&r.label))
            .max()
            .unwrap_or(0);
        let show_id = self.rows.iter().any(|r| r.id.is_some());
        let id_col = if show_id {
            self.rows
                .iter()
                .filter_map(|r| r.id.as_deref())
                .map(|s| u16::try_from(s.chars().count()).unwrap_or(0))
                .max()
                .unwrap_or(0)
        } else {
            0
        };

        let label_x = inner.left().saturating_add(cursor_w);
        let id_x = label_x
            .saturating_add(label_col)
            .saturating_add(self.column_gap);
        let keys_x = if show_id {
            id_x.saturating_add(id_col).saturating_add(self.column_gap)
        } else {
            id_x
        };

        // 5. The visible window of rows.
        let top = self.table_top(area);
        let h = self.table_height(area);
        for (vis, row) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(h as usize)
        {
            let i = vis - self.scroll;
            let y = top.saturating_add(u16::try_from(i).unwrap_or(u16::MAX));
            if y >= top.saturating_add(h) {
                break;
            }
            let state_style = match row.state {
                RowState::Normal => Style::new(),
                RowState::Selected => self.selected_style,
                RowState::Capturing => self.capturing_style,
                RowState::Disabled => self.disabled_style,
            };
            let base = self.style.patch(state_style);

            // Fill the whole row with the state base so selection reads as a
            // bar (a region row, unlike inline `Kbd`).
            buf.set_style(Rect::new(inner.left(), y, inner.width, 1), base);

            // Cursor gutter (only on selected/capturing; blanks keep align).
            if matches!(row.state, RowState::Selected | RowState::Capturing) {
                put_str(buf, inner.left(), y, right, base, &self.cursor);
            }

            // Label, then optional id (dim), then the keys via `Kbd`.
            put_line(
                buf,
                label_x,
                y,
                right,
                base.patch(self.label_style),
                &row.label,
            );
            if show_id {
                if let Some(id) = &row.id {
                    put_str(buf, id_x, y, right, base.patch(self.id_style), id);
                }
            }
            if keys_x < right {
                let kw = right - keys_x;
                if row.state == RowState::Capturing {
                    self.kbd(
                        [self.capture_label.clone()],
                        base.patch(self.capturing_style),
                    )
                    .render(Rect::new(keys_x, y, kw, 1), buf);
                } else {
                    self.kbd(row.keys.iter().cloned(), base)
                        .render(Rect::new(keys_x, y, kw, 1), buf);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Color;

    fn rows() -> Vec<KeymapRow<'static>> {
        vec![
            KeymapRow::new("Command palette", ["Ctrl", "K"])
                .id("app.palette")
                .state(RowState::Selected),
            KeymapRow::new("Quit", ["q"]).id("app.quit"),
            KeymapRow::new("Help", ["?"])
                .id("app.help")
                .state(RowState::Disabled),
        ]
    }

    fn glyphs(buf: &Buffer, y: u16, w: u16) -> String {
        (0..w)
            .map(|x| buf.get(Position::new(x, y)).unwrap().symbol)
            .collect()
    }

    #[test]
    fn rows_render_aligned_with_label_id_and_kbd_columns() {
        let r = rows();
        let mut buf = Buffer::empty(Rect::new(0, 0, 44, 4));
        KeymapView::new(&r).render(buf.area(), &mut buf);
        // Row 0 is Selected → cursor glyph in the gutter, then the label.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '▶');
        assert!(glyphs(&buf, 0, 44).contains("Command palette"));
        // The id column is present (some row carries one) and the keys
        // render through the reused Kbd (a bracketed cap).
        assert!(glyphs(&buf, 0, 44).contains("app.palette"));
        assert!(glyphs(&buf, 0, 44).contains("[Ctrl] [K]"));
        // Row 1 is Normal → no cursor glyph in its gutter.
        assert_ne!(buf.get(Position::new(0, 1)).unwrap().symbol, '▶');
    }

    #[test]
    fn header_and_footer_take_a_line_each_and_offset_the_table() {
        let r = rows();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 6));
        KeymapView::new(&r)
            .header("Vim · macOS")
            .footer("press a key")
            .render(buf.area(), &mut buf);
        assert!(glyphs(&buf, 0, 40).contains("Vim · macOS"));
        // The table now starts on row 1 (header consumed row 0).
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '▶');
        assert!(glyphs(&buf, 5, 40).contains("press a key"));
    }

    #[test]
    fn hit_maps_a_click_back_to_the_source_row_through_scroll() {
        let r = rows();
        let v = KeymapView::new(&r).header("h").scroll(1);
        let area = Rect::new(0, 0, 40, 6);
        // Header is row 0; table starts row 1 showing source row 1 (scroll).
        assert_eq!(v.hit(area, Position::new(3, 1)), Some(1));
        assert_eq!(v.hit(area, Position::new(3, 2)), Some(2));
        // The header line is not a row.
        assert_eq!(v.hit(area, Position::new(3, 0)), None);
        // Past the last row → None (no phantom selection).
        assert_eq!(v.hit(area, Position::new(3, 3)), None);
        // Outside the box → None.
        assert_eq!(v.hit(area, Position::new(80, 1)), None);
    }

    #[test]
    fn a_capturing_row_shows_the_placeholder_not_its_keys() {
        let r = vec![KeymapRow::new("Help", ["?"]).state(RowState::Capturing)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 1));
        KeymapView::new(&r)
            .capture_label("press…")
            .render(buf.area(), &mut buf);
        let line = glyphs(&buf, 0, 30);
        assert!(line.contains("press…"), "got {line:?}");
        assert!(!line.contains('?'));
    }

    #[test]
    fn the_block_frames_the_box_and_rows_render_inside_it() {
        let r = rows();
        let mut buf = Buffer::empty(Rect::new(0, 0, 44, 6));
        KeymapView::new(&r)
            .block(Block::bordered())
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        // The first row is inside the frame (col 1, row 1).
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, '▶');
    }

    #[test]
    fn the_box_is_cleared_opaque_so_a_background_cannot_bleed_through() {
        let r = rows();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
        for p in buf.area().positions() {
            buf.set_cell(p, '.', Style::new().fg(Color::Red));
        }
        KeymapView::new(&r).render(buf.area(), &mut buf);
        // Every cell in the box is cleared first, so the '.' background can
        // never bleed through (a cell is either blank or widget content).
        for p in buf.area().positions() {
            assert_ne!(buf.get(p).unwrap().symbol, '.');
        }
    }

    #[test]
    fn styles_cascade_state_over_base_over_column() {
        let r = vec![KeymapRow::new("X", ["a"]).state(RowState::Selected)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
        KeymapView::new(&r)
            .label_style(Style::new().fg(Color::Cyan))
            .selected_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        // Cursor gutter carries the selected bg; the label its fg + that bg.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Blue);
        let lbl = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(lbl.symbol, 'X');
        assert_eq!(lbl.fg, Color::Cyan);
        assert_eq!(lbl.bg, Color::Blue);
    }

    #[test]
    fn no_rows_is_just_the_cleared_box() {
        let r: [KeymapRow<'_>; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        KeymapView::new(&r).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_and_scroll_past_end_are_total_no_ops() {
        let r = rows();
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        KeymapView::new(&r).render(Rect::new(0, 0, 0, 0), &mut buf);
        // Scroll past the last row: nothing to draw, no panic.
        let mut buf2 = Buffer::empty(Rect::new(0, 0, 20, 3));
        KeymapView::new(&r)
            .scroll(99)
            .render(buf2.area(), &mut buf2);
        assert!(
            KeymapView::new(&r)
                .scroll(99)
                .hit(Rect::new(0, 0, 20, 3), Position::new(1, 0))
                .is_none()
        );
    }

    #[test]
    fn inner_is_the_area_without_a_block_and_the_frame_with_one() {
        let r = rows();
        let area = Rect::new(0, 0, 20, 8);
        assert_eq!(KeymapView::new(&r).inner(area), area);
        assert_eq!(
            KeymapView::new(&r).block(Block::bordered()).inner(area),
            Rect::new(1, 1, 18, 6)
        );
    }
}
