//! [`Menu`] — a vertical, **opaque**, optionally-[`Block`]-framed list of
//! action items (right-aligned key hints, separators, disabled rows) that
//! floats as a context/command menu.
//!
//! # A pure projection of caller-owned `items` + `highlight`
//!
//! Like every rstui widget `Menu` is a **pure projection**: it renders the
//! caller-owned `&[MenuItem]` it is handed plus a caller-owned
//! [`highlight`](Menu::highlight) index, and reads nothing else. Which row the
//! keyboard is on is ordinary application state the reducer owns and moves in
//! `update`; **committing the highlighted item's action**, and *skipping
//! separators and disabled rows while navigating*, are likewise the reducer's
//! job — the widget only ever reads `highlight`, exactly the read-only-state
//! rule [`List`]'s `selected`/`offset` and
//! [`Select`](crate::Select)'s `highlight` establish.
//!
//! # Not a [`Select`](crate::Select) — it commits an *action*, not a value
//!
//! `Select` is a closed field with a *committed* `selected` value; `Menu` has
//! **no closed field and no committed value**. Activating a menu row runs a
//! command (the reducer's concern), so the widget models only the visible
//! list. It is the [`Select`](crate::Select)-not-[`Modal`](crate::Modal) precedent applied
//! once more: share a technique, not an ill-fitting type.
//!
//! # Opaque, and it **reuses [`List`]** for the column
//!
//! A context menu floats over whatever was already drawn, so — like
//! [`Modal`](crate::Modal) (see `modal.rs` *"Opaque on purpose"*, the
//! `clear_region` reasoning at `modal.rs:29-38`: a [`Style`] is a patch and
//! cannot reset a cell) — `Menu`
//! [`clear_region`](rstui_core::Buffer::clear_region)s its area before drawing,
//! taking exclusive ownership of those cells. The column itself *is* a
//! [`List`]: each item is projected to one [`Line`] (label, then a
//! computed pad, then the right-aligned key hint) and handed to a `List`, so
//! scrolling ([`offset`](Menu::offset)), the full-row highlight bar, the
//! optional framing [`Block`], and totality are **inherited**, never
//! re-implemented — the same wholesale reuse [`Select`](crate::Select) makes.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no items, an out-of-range [`highlight`](Menu::highlight) (no bar,
//! inherited from `List`), an [`offset`](Menu::offset) past the end, and a
//! width too narrow for the label+hint (the hint is clipped at the right edge)
//! are all safe clips/no-ops — never a panic. A nested submenu and type-ahead
//! are deliberately deferred additives, not smuggled into this slice.

use std::borrow::Cow;

use rstui_core::{Buffer, Line, Rect, Span, Style, Widget};

use crate::block::Block;
use crate::list::List;

/// The default glyph a [`MenuItem::separator`] rule is drawn with.
const SEPARATOR: char = '─';

/// One row of a [`Menu`]: an action item (a [`Line`] label with an optional
/// right-aligned key hint, possibly disabled) or a horizontal separator rule.
///
/// Build an action from anything a [`Line`] is built from (the
/// [`ListItem`](crate::ListItem) `From` family), add a hint with
/// [`key_hint`](MenuItem::key_hint), dim it with
/// [`disabled`](MenuItem::disabled), or make a rule with
/// [`MenuItem::separator`]. The hint/disabled builders are no-ops on a
/// separator.
#[derive(Debug, Clone)]
pub struct MenuItem<'a> {
    label: Line<'a>,
    key_hint: Option<Cow<'a, str>>,
    disabled: bool,
    is_separator: bool,
}

impl<'a> MenuItem<'a> {
    /// An action row displaying `label` (anything convertible to a [`Line`]),
    /// with no key hint and enabled.
    pub fn new(label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            key_hint: None,
            disabled: false,
            is_separator: false,
        }
    }

    /// A non-selectable horizontal rule between groups of actions.
    ///
    /// Whether the keyboard skips it is the reducer's job (the widget still
    /// paints the highlight bar wherever [`Menu::highlight`] points).
    #[must_use]
    pub fn separator() -> Self {
        Self {
            label: Line::default(),
            key_hint: None,
            disabled: false,
            is_separator: true,
        }
    }

    /// Sets the right-aligned key hint (e.g. `"Ctrl+S"`). No-op on a
    /// [`separator`](MenuItem::separator).
    #[must_use]
    pub fn key_hint(mut self, hint: impl Into<Cow<'a, str>>) -> Self {
        self.key_hint = Some(hint.into());
        self
    }

    /// Marks the row disabled, drawn with [`Menu::disabled_style`]. No-op on a
    /// [`separator`](MenuItem::separator).
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl<'a> From<&'a str> for MenuItem<'a> {
    fn from(s: &'a str) -> Self {
        Self::new(s)
    }
}

impl From<String> for MenuItem<'_> {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl<'a> From<Line<'a>> for MenuItem<'a> {
    fn from(line: Line<'a>) -> Self {
        Self::new(line)
    }
}

/// A vertical, opaque, optionally-framed list of action rows that floats as a
/// context/command menu — a pure projection of caller-owned
/// `items` + [`highlight`](Self::highlight).
///
/// Each item is projected to one row (label, a computed pad, then the
/// right-aligned key hint) and rendered through an internal
/// [`List`], so scrolling, the highlight bar, the optional
/// [`block`](Self::block) frame, and totality are inherited. The area is
/// [`clear`](rstui_core::Buffer::clear_region)ed opaque first so the menu can
/// float over unrelated content.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Menu, MenuItem};
///
/// // `highlight` is plain caller-owned model state the widget only reads —
/// // moving it (skipping separators/disabled) and running the highlighted
/// // action are the reducer's job, never the widget's.
/// let items = [
///     MenuItem::new("Save").key_hint("Ctrl+S"),
///     MenuItem::separator(),
///     MenuItem::new("Quit").key_hint("Ctrl+Q"),
/// ];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 3));
/// Menu::new(&items).highlight(0).render(buf.area(), &mut buf);
///
/// // Label is left-aligned; the key hint is flush to the right edge.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'S');
/// assert_eq!(buf.get(Position::new(11, 0)).unwrap().symbol, 'S'); // "Ctrl+S"
/// ```
#[derive(Debug, Clone)]
pub struct Menu<'a> {
    items: &'a [MenuItem<'a>],
    highlight: usize,
    offset: usize,
    block: Option<Block<'a>>,
    style: Style,
    highlight_style: Style,
    disabled_style: Style,
    separator: char,
}

impl<'a> Menu<'a> {
    /// A menu projecting `items`: the first row highlighted, scrolled to the
    /// top, unframed and unstyled, with the default `─` separator rule.
    #[must_use]
    pub fn new(items: &'a [MenuItem<'a>]) -> Self {
        Self {
            items,
            highlight: 0,
            offset: 0,
            block: None,
            style: Style::new(),
            highlight_style: Style::new(),
            disabled_style: Style::new(),
            separator: SEPARATOR,
        }
    }

    /// Sets which row the highlight bar is on — caller-owned state the widget
    /// only reads. Out of range simply paints no bar (inherited from
    /// [`List`]).
    #[must_use]
    pub fn highlight(mut self, highlight: usize) -> Self {
        self.highlight = highlight;
        self
    }

    /// Sets the index of the first visible row (the scroll offset), exactly
    /// [`List::offset`](crate::List::offset).
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Frames the menu in `block`; rows render into
    /// [`block.inner`](Block::inner), the same compose pattern
    /// [`List`] uses.
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`], beneath the row/label/span cascade; it also
    /// fills the content area so the floating menu reads as one solid panel.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched **last** over the highlighted row (one
    /// full-width bar), forwarded straight to the internal
    /// [`List`].
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Sets the [`Style`] of [`disabled`](MenuItem::disabled) rows (label and
    /// hint), patched over the base beneath the highlight bar.
    #[must_use]
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// Sets the glyph a [`MenuItem::separator`] rule repeats (default `─`).
    #[must_use]
    pub fn separator(mut self, glyph: char) -> Self {
        self.separator = glyph;
        self
    }

    /// Projects one [`MenuItem`] to its [`Line`] row at content width
    /// `width`: a separator is a full-width rule; an action is its label, a
    /// pad, then the right-aligned key hint, dimmed when disabled.
    fn row(&self, item: &MenuItem<'a>, width: usize) -> Line<'a> {
        if item.is_separator {
            return Line::raw(self.separator.to_string().repeat(width));
        }

        let mut spans = item.label.spans.clone();
        if let Some(hint) = &item.key_hint {
            let label_w = item.label.width();
            let hint_w = hint.chars().count();
            // Pad pushes the hint flush right; when label+hint overflow the
            // row there is no pad and the hint is clipped at the right edge.
            let pad = width.saturating_sub(label_w.saturating_add(hint_w));
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            spans.push(Span::raw(hint.clone()));
        }

        let base = if item.disabled {
            item.label.style.patch(self.disabled_style)
        } else {
            item.label.style
        };
        Line::from(spans).style(base)
    }
}

impl Widget for Menu<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // Opaque float: take exclusive ownership of the cells so background
        // content cannot bleed through (the Modal `clear_region` affordance —
        // see the module docs; this is the Select-not-Modal precedent).
        buf.clear_region(area);

        // The content width drives the right-aligned hint / separator rule;
        // it is the inner of the optional frame, exactly `List::block`'s.
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        let width = inner.width as usize;

        let rows: Vec<Line<'_>> = self.items.iter().map(|it| self.row(it, width)).collect();

        // Reuse `List` wholesale: scrolling, the full-row highlight bar, the
        // frame, and totality are inherited, never re-implemented.
        let mut list = List::new(rows)
            .selected(Some(self.highlight))
            .offset(self.offset)
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

    /// Fills `buf` with a styled `.` background so a clear is observable.
    fn background(buf: &mut Buffer) {
        let style = Style::new().fg(Color::Red).bg(Color::Blue);
        for p in buf.area().positions() {
            buf.set_cell(p, '.', style);
        }
    }

    #[test]
    fn each_item_is_one_left_aligned_clipped_row() {
        let items = [MenuItem::new("Open"), MenuItem::new("Close")];
        assert_eq!(lines(Menu::new(&items), 5, 3), "Open \nClose\n     \n");
    }

    #[test]
    fn a_key_hint_is_flush_to_the_right_edge() {
        let items = [MenuItem::new("Save").key_hint("^S")];
        // "Save" left, "^S" right, the gap padded.
        assert_eq!(lines(Menu::new(&items), 8, 1), "Save  ^S\n");
    }

    #[test]
    fn a_separator_is_a_full_width_rule() {
        let items = [
            MenuItem::new("a"),
            MenuItem::separator(),
            MenuItem::new("b"),
        ];
        assert_eq!(lines(Menu::new(&items), 4, 3), "a   \n────\nb   \n");
    }

    #[test]
    fn a_custom_separator_glyph_is_used() {
        let items = [MenuItem::separator()];
        assert_eq!(lines(Menu::new(&items).separator('='), 3, 1), "===\n");
    }

    #[test]
    fn the_highlight_is_a_full_width_bar() {
        let items = [MenuItem::new("a"), MenuItem::new("b")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Menu::new(&items)
            .highlight(1)
            .highlight_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for x in 0..4 {
            assert_eq!(buf.get(Position::new(x, 1)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn an_out_of_range_highlight_paints_no_bar() {
        let items = [MenuItem::new("a"), MenuItem::new("b")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        Menu::new(&items)
            .highlight(9)
            .highlight_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Reset);
            }
        }
    }

    #[test]
    fn offset_scrolls_the_rows() {
        let items = [
            MenuItem::new("i0"),
            MenuItem::new("i1"),
            MenuItem::new("i2"),
        ];
        assert_eq!(lines(Menu::new(&items).offset(1), 2, 2), "i1\ni2\n");
    }

    #[test]
    fn a_disabled_row_takes_the_disabled_style() {
        let items = [MenuItem::new("x").disabled(true)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        Menu::new(&items)
            .disabled_style(Style::new().fg(Color::DarkGray))
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::DarkGray);
    }

    #[test]
    fn the_menu_is_opaque_so_the_background_does_not_bleed_through() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        background(&mut buf);
        let items = [MenuItem::new("ok")];
        Menu::new(&items).render(buf.area(), &mut buf);
        // The cleared row past the single item is EMPTY, not the '.' bg.
        let blank = buf.get(Position::new(0, 1)).unwrap();
        assert_eq!(blank.symbol, ' ');
        assert_eq!(blank.bg, Color::Reset);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'o');
    }

    #[test]
    fn a_block_frames_the_menu_and_hints_align_to_the_inner_width() {
        let items = [MenuItem::new("Go").key_hint("g")];
        assert_eq!(
            lines(Menu::new(&items).block(Block::bordered()), 8, 3),
            "┌──────┐\n│Go   g│\n└──────┘\n"
        );
    }

    #[test]
    fn an_empty_menu_with_a_block_still_renders_the_frame() {
        let items: [MenuItem<'_>; 0] = [];
        assert_eq!(
            lines(Menu::new(&items).block(Block::bordered()), 3, 3),
            "┌─┐\n│ │\n└─┘\n"
        );
    }

    #[test]
    fn a_hint_wider_than_the_row_is_clipped_at_the_right_edge() {
        let items = [MenuItem::new("label").key_hint("verylong")];
        // No room for a pad: label then hint, clipped to the width — no panic.
        assert_eq!(lines(Menu::new(&items), 6, 1), "labelv\n");
    }

    #[test]
    fn style_cascades_and_the_highlight_wins_last() {
        let items = [MenuItem::new("A")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        Menu::new(&items)
            .style(Style::new().fg(Color::Green))
            .highlight(0)
            .highlight_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.symbol, 'A');
        assert_eq!(cell.fg, Color::Green); // base cascades
        assert_eq!(cell.bg, Color::Blue); // highlight patched last
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let items = [MenuItem::new("a")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Menu::new(&items).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
