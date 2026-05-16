//! [`Kbd`] — an inline keycap cluster: caller-provided key labels rendered as
//! bracketed caps (`[Ctrl]+[K]`, `[⌃][⇧][P]`), the `<kbd>` glyph a help line,
//! a menu row, or a hint strip pins beside an action.
//!
//! # A pure projection, and an inline one
//!
//! `Kbd` owns no state — it is a caller-owned list of key labels projected to
//! glyphs, the headless-testable shape every widget here uses. Like
//! [`Badge`](crate::Badge) it is an **inline** adornment, not a bar: it paints
//! **only its own cap/separator cells** and leaves the rest of the area
//! untouched, so it sits *within* a line of other content (a help row, a menu
//! item). Filling the whole row is the right rule for a *region* widget like
//! [`List`](crate::List); a keycap that clobbered its row would be unusable
//! mid-sentence — the same reasoning [`Badge`](crate::Badge) records.
//!
//! # A leaf control: one row, no `Block`
//!
//! Like the other leaf adornments ([`Badge`](crate::Badge) /
//! [`Breadcrumb`](crate::Breadcrumb)) and unlike the container widgets, `Kbd`
//! has **no framing [`Block`](crate::Block)**: it draws on exactly the top row
//! of its area, and the surrounding [`Layout`](rstui_core::Layout) owns the
//! frame and placement.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*): an
//! empty area, no keys, an empty key label, a multi-byte label, a cluster
//! wider than the area (clipped at the right edge), and a multi-row area
//! (only the top row is touched) are all safe clips/no-ops — never a panic.

use std::borrow::Cow;

use rstui_core::{Buffer, Position, Rect, Style, Widget};

/// An inline cluster of key labels, each wrapped in a cap delimiter and joined
/// by a separator.
///
/// Each key is drawn wrapped in the [`delimiters`](Self::delimiters) (default
/// `[`…`]`), the caps joined by the [`separator`](Self::separator) (default a
/// single space). Only those cells are painted — `Kbd` is inline (see the
/// [module docs](self)). The base [`style`](Self::style) sits beneath the
/// cap/label glyphs ([`key_style`](Self::key_style)) and the separator
/// ([`separator_style`](Self::separator_style)) in the cascade.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Kbd;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
/// Kbd::new(["Ctrl", "K"]).separator("+").render(buf.area(), &mut buf);
///
/// // "[Ctrl]+[K]" — each label is a bracketed cap, joined by '+'.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '[');
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'C');
/// assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, ']');
/// assert_eq!(buf.get(Position::new(6, 0)).unwrap().symbol, '+');
/// assert_eq!(buf.get(Position::new(7, 0)).unwrap().symbol, '[');
/// ```
#[derive(Debug, Clone)]
pub struct Kbd<'a> {
    keys: Vec<Cow<'a, str>>,
    open: Cow<'a, str>,
    close: Cow<'a, str>,
    separator: Cow<'a, str>,
    style: Style,
    key_style: Style,
    separator_style: Style,
}

impl Default for Kbd<'_> {
    fn default() -> Self {
        Self {
            keys: Vec::new(),
            open: Cow::Borrowed("["),
            close: Cow::Borrowed("]"),
            separator: Cow::Borrowed(" "),
            style: Style::new(),
            key_style: Style::new(),
            separator_style: Style::new(),
        }
    }
}

impl<'a> Kbd<'a> {
    /// A cluster of `keys`, each a bracketed cap, joined by a single space.
    pub fn new<I, T>(keys: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Cow<'a, str>>,
    {
        Self {
            keys: keys.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// Sets the cap delimiters wrapping each key label (default `[` and `]`).
    #[must_use]
    pub fn delimiters(
        mut self,
        open: impl Into<Cow<'a, str>>,
        close: impl Into<Cow<'a, str>>,
    ) -> Self {
        self.open = open.into();
        self.close = close.into();
        self
    }

    /// Sets the string drawn between adjacent caps (default a single space).
    #[must_use]
    pub fn separator(mut self, separator: impl Into<Cow<'a, str>>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Sets the base [`Style`], beneath the cap and separator styles.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] of the cap cells (the delimiters and the label),
    /// patched over the base.
    #[must_use]
    pub fn key_style(mut self, style: Style) -> Self {
        self.key_style = style;
        self
    }

    /// Sets the [`Style`] of the [`separator`](Self::separator) cells, patched
    /// over the base.
    #[must_use]
    pub fn separator_style(mut self, style: Style) -> Self {
        self.separator_style = style;
        self
    }

    /// The cluster's display width in columns (characters), the natural width
    /// the cluster occupies when not clipped.
    ///
    /// A pure function of the keys and delimiters, exposed so a composing
    /// widget (e.g. [`HelpOverlay`](crate::HelpOverlay)) can align a key
    /// column to the widest cluster — the same derived-geometry-is-a-projection
    /// reasoning [`Modal::area`](crate::Modal::area) uses.
    #[must_use]
    pub fn width(&self) -> u16 {
        if self.keys.is_empty() {
            return 0;
        }
        let cap = self.open.chars().count() + self.close.chars().count();
        let keys: usize = self.keys.iter().map(|k| k.chars().count() + cap).sum();
        let seps = self.separator.chars().count() * self.keys.len().saturating_sub(1);
        u16::try_from(keys + seps).unwrap_or(u16::MAX)
    }
}

impl Widget for Kbd<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() || self.keys.is_empty() {
            return;
        }
        let y = area.top();
        let right = area.right();
        let key_style = self.style.patch(self.key_style);
        let sep_style = self.style.patch(self.separator_style);

        let mut x = area.left();
        // Stamp each cap (`open` + label + `close`) in the key style, joined by
        // the separator in the separator style. Only these cells are painted —
        // the widget is inline, like `Badge`.
        'cluster: for (i, key) in self.keys.iter().enumerate() {
            if i > 0 {
                for ch in self.separator.chars() {
                    if x >= right {
                        break 'cluster;
                    }
                    buf.set_cell(Position::new(x, y), ch, sep_style);
                    x = x.saturating_add(1);
                }
            }
            for ch in self
                .open
                .chars()
                .chain(key.chars())
                .chain(self.close.chars())
            {
                if x >= right {
                    break 'cluster;
                }
                buf.set_cell(Position::new(x, y), ch, key_style);
                x = x.saturating_add(1);
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
    fn each_key_is_a_bracketed_cap_joined_by_a_space() {
        assert_eq!(lines(Kbd::new(["Ctrl", "K"]), 12, 1), "[Ctrl] [K]  \n");
    }

    #[test]
    fn a_custom_separator_joins_the_caps() {
        assert_eq!(
            lines(Kbd::new(["Ctrl", "K"]).separator("+"), 12, 1),
            "[Ctrl]+[K]  \n"
        );
    }

    #[test]
    fn custom_delimiters_wrap_each_label() {
        assert_eq!(lines(Kbd::new(["A"]).delimiters("⟨", "⟩"), 5, 1), "⟨A⟩  \n");
    }

    #[test]
    fn a_single_key_has_no_separator() {
        assert_eq!(lines(Kbd::new(["Esc"]), 8, 1), "[Esc]   \n");
    }

    #[test]
    fn an_empty_label_is_just_the_delimiters() {
        assert_eq!(lines(Kbd::new([""]), 4, 1), "[]  \n");
    }

    #[test]
    fn no_keys_is_a_total_no_op() {
        // No keys: nothing is painted (inline, not a bar), no panic.
        assert_eq!(lines(Kbd::new(Vec::<&str>::new()), 4, 1), "    \n");
    }

    #[test]
    fn the_cluster_clips_at_the_right_edge() {
        // "[Ctrl] [K]" is 10 wide; area 6 clips after "[Ctrl]".
        assert_eq!(lines(Kbd::new(["Ctrl", "K"]), 6, 1), "[Ctrl]\n");
    }

    #[test]
    fn only_the_cluster_cells_are_painted_not_the_whole_row() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        for x in 0..8 {
            buf.set_cell(Position::new(x, 0), '.', Style::new());
        }
        Kbd::new(["X"]).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '[');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, ']');
        // Past the 3-cell cap the '.' fill is untouched (inline).
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, '.');
        assert_eq!(buf.get(Position::new(7, 0)).unwrap().symbol, '.');
    }

    #[test]
    fn key_style_and_separator_style_cascade_over_the_base() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 7, 1));
        Kbd::new(["A", "B"])
            .style(Style::new().bg(Color::Black))
            .key_style(Style::new().fg(Color::Cyan))
            .separator_style(Style::new().fg(Color::Red))
            .render(buf.area(), &mut buf);
        // "[A] [B]": cap cells get the base bg + key fg…
        let cap = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cap.symbol, '[');
        assert_eq!(cap.bg, Color::Black);
        assert_eq!(cap.fg, Color::Cyan);
        // …and the separator (col 3) gets the base bg + separator fg.
        let sep = buf.get(Position::new(3, 0)).unwrap();
        assert_eq!(sep.symbol, ' ');
        assert_eq!(sep.bg, Color::Black);
        assert_eq!(sep.fg, Color::Red);
    }

    #[test]
    fn width_is_the_natural_cluster_width() {
        // "[Ctrl]+[K]" = 6 + 1 + 3 = 10.
        assert_eq!(Kbd::new(["Ctrl", "K"]).separator("+").width(), 10);
        // No keys → zero width.
        assert_eq!(Kbd::new(Vec::<&str>::new()).width(), 0);
    }

    #[test]
    fn a_multibyte_label_maps_each_char_to_one_column() {
        assert_eq!(lines(Kbd::new(["⌘"]), 4, 1), "[⌘] \n");
    }

    #[test]
    fn render_uses_the_area_origin_and_only_the_top_row() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        Kbd::new(["Z"]).render(Rect::new(2, 1, 5, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, '[');
        assert_eq!(buf.get(Position::new(3, 1)).unwrap().symbol, 'Z');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Kbd::new(["A"]).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
