//! [`HelpOverlay`] — a centred, **opaque** keybinding cheat-sheet: a
//! caller-owned list of `(keys, description)` rows laid out in two aligned
//! columns, the `?`-summons overlay every editor/TUI floats over its content.
//!
//! # A pure projection that reuses [`Kbd`] and the [`Modal`](crate::Modal) idiom
//!
//! `HelpOverlay` owns no state — it is a caller-owned `&[HelpEntry]` projected
//! to glyphs, the headless-testable shape every widget here uses. The app
//! decides *whether* the cheat-sheet is shown (a `bool` in its model, toggled
//! on `?`/`Esc` in `update`); `view` renders the overlay only when it is — the
//! widget never reads that flag, exactly the pure-projection rule
//! [`Modal`](crate::Modal) follows.
//!
//! It is **not** a [`Modal`](crate::Modal) (it has no focus-scope trap and is
//! its own sized cheat-sheet type), but it borrows `Modal`'s two defining
//! techniques: it is centred and sized by a
//! [`width`](HelpOverlay::width)/[`height`](HelpOverlay::height)
//! [`Constraint`] each, and it is **opaque** —
//! [`clear_region`](rstui_core::Buffer::clear_region)d before drawing so the
//! content behind it cannot bleed through (a [`Style`] is a patch and cannot
//! reset a cell). Each row's key cluster is rendered by **reusing [`Kbd`]
//! wholesale** (its caps/clipping/totality inherited rather than
//! re-implemented), and the optional [`block`](HelpOverlay::block) frame by
//! [`Block`].
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*): an
//! empty overlay, no entries (just the cleared box), constraints that resolve
//! the box to zero, an inner too small for the rows (they clip), and an
//! out-of-range row count are all safe no-ops/clips — never a panic.

use std::borrow::Cow;

use rstui_core::{Buffer, Constraint, Line, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::kbd::Kbd;

/// One row of a [`HelpOverlay`]: a key cluster and a description.
///
/// The `keys` are projected through a [`Kbd`] (so `["Ctrl", "K"]` renders as
/// `[Ctrl] [K]`); the `description` is any value convertible to a [`Line`], so
/// it carries its own per-span styles in the cascade.
#[derive(Debug, Clone)]
pub struct HelpEntry<'a> {
    keys: Vec<Cow<'a, str>>,
    description: Line<'a>,
}

impl<'a> HelpEntry<'a> {
    /// An entry binding `keys` (each a [`Kbd`] cap) to `description`.
    pub fn new<I, T>(keys: I, description: impl Into<Line<'a>>) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Cow<'a, str>>,
    {
        Self {
            keys: keys.into_iter().map(Into::into).collect(),
            description: description.into(),
        }
    }
}

/// A centred, opaque, optionally-framed keybinding cheat-sheet.
///
/// Render it over the region it should cover (usually the whole
/// [`Frame::area`](rstui_core::Frame::area)); it paints an optional
/// [`backdrop_style`](Self::backdrop_style) scrim over that whole area, then
/// **clears** and fills a centred box sized by
/// [`width`](Self::width)/[`height`](Self::height), draws the optional framing
/// [`block`](Self::block), and lays the entries into two aligned columns (the
/// key clusters left, the descriptions right, the key column sized to the
/// widest cluster). [`area`](Self::area)/[`inner`](Self::inner) are pure
/// derived rects, exactly like [`Modal`](crate::Modal).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Constraint, Position, Rect, Widget};
/// use rstui_widgets::{HelpEntry, HelpOverlay};
///
/// // A background the cheat-sheet must not let bleed through.
/// let mut buf = Buffer::empty(Rect::new(0, 0, 24, 6));
/// for p in buf.area().positions() {
///     buf.set_cell(p, '.', Default::default());
/// }
///
/// let entries = [
///     HelpEntry::new(["Ctrl", "S"], "Save"),
///     HelpEntry::new(["Esc"], "Quit"),
/// ];
/// HelpOverlay::new(&entries)
///     .width(Constraint::Length(20))
///     .height(Constraint::Length(4))
///     .separator("+")
///     .render(buf.area(), &mut buf);
///
/// // The box is opaque: a cell inside it is no longer the '.' background.
/// assert_ne!(buf.get(Position::new(12, 3)).unwrap().symbol, '.');
/// // The first row's key cluster renders through the reused `Kbd`.
/// assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, '[');
/// ```
#[derive(Debug, Clone)]
pub struct HelpOverlay<'a> {
    entries: &'a [HelpEntry<'a>],
    block: Option<Block<'a>>,
    width: Constraint,
    height: Constraint,
    column_gap: u16,
    separator: Cow<'a, str>,
    style: Style,
    backdrop_style: Style,
    key_style: Style,
    description_style: Style,
}

impl<'a> HelpOverlay<'a> {
    /// A cheat-sheet of `entries`, sized 60% × 60% of the overlay, centred,
    /// unframed, opaque, with no backdrop scrim.
    #[must_use]
    pub fn new(entries: &'a [HelpEntry<'a>]) -> Self {
        Self {
            entries,
            block: None,
            width: Constraint::Percentage(60),
            height: Constraint::Percentage(60),
            column_gap: 2,
            separator: Cow::Borrowed(" "),
            style: Style::new(),
            backdrop_style: Style::new(),
            key_style: Style::new(),
            description_style: Style::new(),
        }
    }

    /// Sets the framing [`Block`]; the rows render into
    /// [`inner`](Self::inner), the render-then-fill-`inner` pattern `Block`
    /// uses.
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the box width within the overlay (default
    /// [`Percentage(60)`](Constraint::Percentage)).
    #[must_use]
    pub fn width(mut self, width: Constraint) -> Self {
        self.width = width;
        self
    }

    /// Sets the box height within the overlay (default
    /// [`Percentage(60)`](Constraint::Percentage)).
    #[must_use]
    pub fn height(mut self, height: Constraint) -> Self {
        self.height = height;
        self
    }

    /// Sets the blank columns between the key column and the descriptions
    /// (default `2`).
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

    /// Sets the [`Style`] filling the (already-cleared) box, beneath the frame
    /// and rows.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the scrim [`Style`] patched over the *whole* overlay (the dimming
    /// behind the cheat-sheet). Defaults empty — opt-in, like
    /// [`Modal::backdrop_style`](crate::Modal::backdrop_style); the box itself
    /// is opaque regardless.
    #[must_use]
    pub fn backdrop_style(mut self, style: Style) -> Self {
        self.backdrop_style = style;
        self
    }

    /// Sets the [`Style`] of each row's key caps (forwarded to the reused
    /// [`Kbd`]'s `key_style`), patched over the base.
    #[must_use]
    pub fn key_style(mut self, style: Style) -> Self {
        self.key_style = style;
        self
    }

    /// Sets the base [`Style`] of the description column, beneath each
    /// description [`Line`]'s own styles in the cascade.
    #[must_use]
    pub fn description_style(mut self, style: Style) -> Self {
        self.description_style = style;
        self
    }

    /// The centred box rect for a given `overlay` area — a pure function of the
    /// overlay and the size constraints, centred (odd leftover biased toward
    /// the start), exactly like [`Modal::area`](crate::Modal::area).
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

    /// The content rect inside the box: [`area`](Self::area) minus the framing
    /// [`block`](Self::block) (or the whole box when there is none), exactly
    /// like [`Modal::inner`](crate::Modal::inner).
    #[must_use]
    pub fn inner(&self, overlay: Rect) -> Rect {
        let dialog = self.area(overlay);
        match &self.block {
            Some(block) => block.inner(dialog),
            None => dialog,
        }
    }

    /// The reused [`Kbd`] for entry `i`, carrying this overlay's separator and
    /// key style.
    fn kbd(&self, i: usize) -> Kbd<'_> {
        Kbd::new(self.entries[i].keys.iter().cloned())
            .separator(self.separator.clone())
            .style(self.style)
            .key_style(self.key_style)
    }
}

impl Widget for HelpOverlay<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // 1. The scrim over the whole overlay (a patch, default-empty no-op).
        buf.set_style(area, self.backdrop_style);

        // 2. The centred box; a zero-sized box leaves just the scrim — total.
        let dialog = self.area(area);
        if dialog.is_empty() {
            return;
        }

        // 3. Clear it opaque so background content cannot bleed through, then
        //    colour it (the `Modal` opacity affordance — see the module docs).
        buf.clear_region(dialog);
        buf.set_style(dialog, self.style);

        // 4. The optional frame; rows go into `inner`.
        let inner = match &self.block {
            Some(b) => b.inner(dialog),
            None => dialog,
        };
        if let Some(b) = &self.block {
            b.render_ref(dialog, buf);
        }
        if inner.is_empty() {
            return;
        }

        // 5. Two aligned columns: the key clusters left (column sized to the
        //    widest, clamped to the inner width), the descriptions right.
        let key_col = (0..self.entries.len())
            .map(|i| self.kbd(i).width())
            .max()
            .unwrap_or(0)
            .min(inner.width);
        let desc_x = inner
            .left()
            .saturating_add(key_col)
            .saturating_add(self.column_gap);
        let right = inner.right();

        for (i, entry) in self.entries.iter().enumerate().take(inner.height as usize) {
            let y = inner.top().saturating_add(i as u16);

            // The key cluster, via the reused `Kbd`, in its own column.
            self.kbd(i)
                .render(Rect::new(inner.left(), y, key_col, 1), buf);

            // The description, base → description_style → line → span.
            let line = &entry.description;
            let line_base = self.style.patch(self.description_style).patch(line.style);
            let mut x = desc_x;
            'desc: for span in &line.spans {
                let span_style = line_base.patch(span.style);
                for ch in span.content.chars() {
                    if x >= right {
                        break 'desc;
                    }
                    buf.set_cell(Position::new(x, y), ch, span_style);
                    x = x.saturating_add(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Span};

    /// Fills `buf` with a styled `.` background so a clear is observable.
    fn background(buf: &mut Buffer) {
        let style = Style::new().fg(Color::Red).bg(Color::Blue);
        for p in buf.area().positions() {
            buf.set_cell(p, '.', style);
        }
    }

    fn sample() -> Vec<HelpEntry<'static>> {
        vec![
            HelpEntry::new(["Ctrl", "S"], "Save"),
            HelpEntry::new(["Esc"], "Quit"),
        ]
    }

    #[test]
    fn the_box_is_centred_within_the_overlay() {
        let entries = sample();
        let help = HelpOverlay::new(&entries)
            .width(Constraint::Length(10))
            .height(Constraint::Length(4));
        // 20x10, 10x4 box centred at ((20-10)/2, (10-4)/2).
        assert_eq!(help.area(Rect::new(0, 0, 20, 10)), Rect::new(5, 3, 10, 4));
    }

    #[test]
    fn the_box_is_cleared_opaque_so_the_background_cannot_bleed_through() {
        let entries = sample();
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 6));
        background(&mut buf);
        HelpOverlay::new(&entries)
            .width(Constraint::Length(8))
            .height(Constraint::Length(2))
            .render(buf.area(), &mut buf);
        // Box centred at (4,2) 8x2: cleared, so no '.' / red-blue bleeds in.
        let cell = buf.get(Position::new(7, 2)).unwrap();
        assert_eq!(cell.bg, Color::Reset);
        assert_ne!(cell.symbol, '.');
        // Outside the box the background survives.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '.');
    }

    #[test]
    fn entries_are_one_row_each_with_a_kbd_cluster_and_a_description() {
        let entries = sample();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        HelpOverlay::new(&entries)
            .width(Constraint::Length(20))
            .height(Constraint::Length(2))
            .render(buf.area(), &mut buf);
        // Row 0 (box top): "[Ctrl] [S]" then a 2-col gap then "Save".
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '[');
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'C');
        // Row 1: "[Esc]" then "Quit", aligned in the same description column.
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, '[');
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, 'E');
    }

    #[test]
    fn the_description_column_is_aligned_to_the_widest_key_cluster() {
        let entries = sample();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        HelpOverlay::new(&entries)
            .width(Constraint::Length(20))
            .height(Constraint::Length(2))
            .column_gap(1)
            .render(buf.area(), &mut buf);
        // Widest cluster is "[Ctrl] [S]" (10 wide); +1 gap → descriptions at
        // col 11 on BOTH rows, so they line up under each other.
        assert_eq!(buf.get(Position::new(11, 1)).unwrap().symbol, 'S'); // "Save"
        assert_eq!(buf.get(Position::new(11, 2)).unwrap().symbol, 'Q'); // "Quit"
    }

    #[test]
    fn the_separator_is_forwarded_to_the_reused_kbd() {
        let entries = vec![HelpEntry::new(["Ctrl", "K"], "x")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 3));
        HelpOverlay::new(&entries)
            .width(Constraint::Length(16))
            .height(Constraint::Length(1))
            .separator("+")
            .render(buf.area(), &mut buf);
        // "[Ctrl]+[K]" — the '+' separator reached the embedded Kbd.
        assert_eq!(buf.get(Position::new(6, 1)).unwrap().symbol, '+');
    }

    #[test]
    fn the_block_frames_the_box_and_rows_render_inside_it() {
        let entries = vec![HelpEntry::new(["A"], "act")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 5));
        HelpOverlay::new(&entries)
            .width(Constraint::Length(12))
            .height(Constraint::Length(3))
            .block(Block::bordered())
            .render(buf.area(), &mut buf);
        // Box at (0,1) 12x3, bordered: corner on its top row, row inside it.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, '[');
    }

    #[test]
    fn key_style_and_description_style_cascade() {
        let entries = vec![HelpEntry::new(["A"], "d")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        HelpOverlay::new(&entries)
            .width(Constraint::Length(8))
            .height(Constraint::Length(1))
            .column_gap(1)
            .key_style(Style::new().fg(Color::Cyan))
            .description_style(Style::new().fg(Color::Yellow))
            .render(buf.area(), &mut buf);
        // "[A]" key cap in cyan; "d" description (col 4) in yellow.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().fg, Color::Cyan);
        assert_eq!(buf.get(Position::new(4, 1)).unwrap().symbol, 'd');
        assert_eq!(buf.get(Position::new(4, 1)).unwrap().fg, Color::Yellow);
    }

    #[test]
    fn a_description_span_keeps_its_own_style_over_the_base() {
        let desc = Line::from(Span::styled("R", Style::new().fg(Color::Red)));
        let entries = vec![HelpEntry::new(["A"], desc)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        HelpOverlay::new(&entries)
            .width(Constraint::Length(8))
            .height(Constraint::Length(1))
            .column_gap(1)
            .description_style(Style::new().fg(Color::Yellow))
            .render(buf.area(), &mut buf);
        let cell = buf.get(Position::new(4, 1)).unwrap();
        assert_eq!(cell.symbol, 'R');
        assert_eq!(cell.fg, Color::Red); // span fg wins over description_style
    }

    #[test]
    fn the_backdrop_scrim_patches_the_whole_overlay_but_keeps_glyphs() {
        let entries = sample();
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        background(&mut buf);
        HelpOverlay::new(&entries)
            .width(Constraint::Length(4))
            .height(Constraint::Length(1))
            .backdrop_style(Style::new().bg(Color::Black))
            .render(buf.area(), &mut buf);
        // Outside the box: glyph kept, backdrop bg applied.
        let scrim = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(scrim.symbol, '.');
        assert_eq!(scrim.bg, Color::Black);
    }

    #[test]
    fn no_entries_is_just_the_cleared_box() {
        let entries: [HelpEntry<'_>; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        background(&mut buf);
        HelpOverlay::new(&entries)
            .width(Constraint::Length(6))
            .height(Constraint::Length(2))
            .render(buf.area(), &mut buf);
        // Box at (2,1) 6x2 cleared; no rows; no panic.
        assert_eq!(buf.get(Position::new(4, 1)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '.');
    }

    #[test]
    fn inner_is_the_box_without_a_block_and_the_frame_with_one() {
        let entries = sample();
        let overlay = Rect::new(0, 0, 20, 10);
        let help = HelpOverlay::new(&entries)
            .width(Constraint::Length(10))
            .height(Constraint::Length(4));
        assert_eq!(help.inner(overlay), help.area(overlay));
        let framed = help.clone().block(Block::bordered());
        assert_eq!(framed.inner(overlay), Rect::new(6, 4, 8, 2));
    }

    #[test]
    fn rows_clip_when_the_inner_is_too_short() {
        let entries = sample(); // 2 entries
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 3));
        HelpOverlay::new(&entries)
            .width(Constraint::Length(16))
            .height(Constraint::Length(1)) // room for one row only
            .render(buf.area(), &mut buf);
        // Only the first entry's cluster is drawn (box is one row tall).
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '[');
        // The second row would be outside the one-row box: untouched.
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_overlay_area_is_a_total_no_op() {
        let entries = sample();
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        background(&mut buf);
        HelpOverlay::new(&entries).render(Rect::new(0, 0, 0, 0), &mut buf);
        let c = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(c.symbol, '.');
        assert_eq!(c.bg, Color::Blue);
    }
}
