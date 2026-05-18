//! [`WhichKey`] — a transient **leader-hint** popup: when a prefix/leader
//! chord is armed, a small bottom-anchored panel of `key → action` rows so
//! the user can *see* what comes next instead of memorising it. The
//! discoverability affordance opencode, Helix and which-key.nvim are known
//! for, here as a pure projection.
//!
//! # A pure projection that reuses [`Kbd`], engine-agnostic
//!
//! `WhichKey` owns no state and depends on **no keymap engine** — it
//! renders a caller-owned `&[(key, label)]` slice (the app feeds it from
//! its keymap's "what can follow the armed leader" query each frame, only
//! while something is armed). So `rstui-widgets` keeps its
//! `rstui-core`-only boundary, exactly like
//! [`KeymapView`](crate::KeymapView) /
//! [`HelpOverlay`](crate::HelpOverlay), and the key caps are rendered by
//! **reusing [`Kbd`] wholesale**.
//!
//! # Bottom-anchored & content-sized, like [`Modal`](crate::Modal)
//!
//! It is not centred: a which-key panel sits at the **bottom** of the
//! area it is given (just above a footer), sized to its content and
//! clamped to the area — [`area`](WhichKey::area)/[`inner`](WhichKey::inner)
//! are pure derived rects. It is **opaque**
//! ([`clear_region`](rstui_core::Buffer::clear_region)d) so the content
//! behind cannot bleed through.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule: no rows (nothing is drawn — the
//! caller simply doesn't render it when nothing is armed), a zero area, an
//! area too small for the box (it clips), more rows than fit (clipped to
//! [`max_height`](WhichKey::max_height)) are all safe no-ops/clips.

use std::borrow::Cow;

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::kbd::Kbd;

/// A transient leader-hint popup: a title plus `(key, label)` rows.
///
/// The `key` is a single display token (`"P"`, `"⌃X"`) rendered as a
/// [`Kbd`] cap; the `label` is any [`Line`] (an action's help text), so it
/// carries its own per-span styles.
#[derive(Debug, Clone)]
pub struct WhichKey<'a> {
    rows: &'a [(Cow<'a, str>, Line<'a>)],
    block: Option<Block<'a>>,
    title: Line<'a>,
    column_gap: u16,
    max_height: u16,
    style: Style,
    backdrop_style: Style,
    key_style: Style,
    label_style: Style,
}

impl<'a> WhichKey<'a> {
    /// A popup of `rows`, bottom-anchored, framed by a rounded
    /// [`Block`] titled `⟨leader⟩`, opaque, no backdrop scrim.
    #[must_use]
    pub fn new(rows: &'a [(Cow<'a, str>, Line<'a>)]) -> Self {
        Self {
            rows,
            block: Some(Block::bordered()),
            title: Line::from(" ⟨leader⟩ "),
            column_gap: 2,
            max_height: u16::MAX,
            style: Style::new(),
            backdrop_style: Style::new(),
            key_style: Style::new(),
            label_style: Style::new(),
        }
    }

    /// Sets the framing [`Block`] (default a bordered box). Pass a
    /// custom block to restyle the border / title bar.
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the title line shown on the frame (default `" ⟨leader⟩ "`).
    #[must_use]
    pub fn title(mut self, title: impl Into<Line<'a>>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the blank columns between the key column and the labels
    /// (default `2`).
    #[must_use]
    pub fn column_gap(mut self, gap: u16) -> Self {
        self.column_gap = gap;
        self
    }

    /// Caps the popup height (rows beyond it clip) — keep the hint small
    /// even when a prefix has many continuations.
    #[must_use]
    pub fn max_height(mut self, rows: u16) -> Self {
        self.max_height = rows;
        self
    }

    /// Sets the [`Style`] filling the (already-cleared) box.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the opt-in scrim [`Style`] patched over the whole area.
    #[must_use]
    pub fn backdrop_style(mut self, style: Style) -> Self {
        self.backdrop_style = style;
        self
    }

    /// Sets the [`Style`] of the key caps (forwarded to the reused
    /// [`Kbd`]).
    #[must_use]
    pub fn key_style(mut self, style: Style) -> Self {
        self.key_style = style;
        self
    }

    /// Sets the base [`Style`] of the label column.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// The natural key-column width (widest cap), used for alignment.
    fn key_col(&self) -> u16 {
        self.rows
            .iter()
            .map(|(k, _)| Kbd::new([k.clone()]).width())
            .max()
            .unwrap_or(0)
    }

    /// The bottom-anchored box rect within `outer` — a pure function of
    /// the rows and `outer`, sized to content, clamped to `outer`, with
    /// the (odd) leftover biased toward the bottom, exactly like
    /// [`Modal::area`](crate::Modal::area).
    #[must_use]
    pub fn area(&self, outer: Rect) -> Rect {
        if outer.is_empty() || self.rows.is_empty() {
            return Rect::new(outer.x, outer.y, 0, 0);
        }
        let frame = u16::from(self.block.is_some()) * 2;
        let widest_label = self
            .rows
            .iter()
            .map(|(_, l)| line_width(l))
            .max()
            .unwrap_or(0);
        let content_w = self
            .key_col()
            .saturating_add(self.column_gap)
            .saturating_add(widest_label);
        let w = content_w.saturating_add(frame).min(outer.width);
        let rows = u16::try_from(self.rows.len())
            .unwrap_or(u16::MAX)
            .min(self.max_height);
        let h = rows.saturating_add(frame).min(outer.height);
        let y = outer.y.saturating_add(outer.height.saturating_sub(h));
        Rect::new(outer.x, y, w, h)
    }

    /// The content rect inside the box: [`area`](Self::area) minus the
    /// framing [`block`](Self::block).
    #[must_use]
    pub fn inner(&self, outer: Rect) -> Rect {
        let b = self.area(outer);
        match &self.block {
            Some(block) => block.inner(b),
            None => b,
        }
    }
}

/// The display width of a [`Line`] in columns (one column per `char`).
fn line_width(line: &Line<'_>) -> u16 {
    let n: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
    u16::try_from(n).unwrap_or(u16::MAX)
}

impl Widget for WhichKey<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() || self.rows.is_empty() {
            return;
        }
        buf.set_style(area, self.backdrop_style);

        let dialog = self.area(area);
        if dialog.is_empty() {
            return;
        }
        buf.clear_region(dialog);
        buf.set_style(dialog, self.style);

        let inner = match &self.block {
            Some(b) => b.inner(dialog),
            None => dialog,
        };
        if let Some(b) = self.block.clone() {
            b.title(self.title.clone()).render(dialog, buf);
        }
        if inner.is_empty() {
            return;
        }

        let key_col = self.key_col().min(inner.width);
        let label_x = inner
            .left()
            .saturating_add(key_col)
            .saturating_add(self.column_gap);
        let right = inner.right();

        for (i, (key, label)) in self.rows.iter().enumerate().take(inner.height as usize) {
            let y = inner
                .top()
                .saturating_add(u16::try_from(i).unwrap_or(u16::MAX));

            // The key cap, via the reused `Kbd`, in its own column.
            Kbd::new([key.clone()])
                .style(self.style)
                .key_style(self.key_style)
                .render(Rect::new(inner.left(), y, key_col, 1), buf);

            // The label, base → label_style → line → span cascade.
            let base = self.style.patch(self.label_style).patch(label.style);
            let mut x = label_x;
            'lbl: for span in &label.spans {
                let st = base.patch(span.style);
                for ch in span.content.chars() {
                    if x >= right {
                        break 'lbl;
                    }
                    buf.set_cell(Position::new(x, y), ch, st);
                    x = x.saturating_add(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Color;

    fn rows() -> Vec<(Cow<'static, str>, Line<'static>)> {
        vec![
            (Cow::Borrowed("P"), Line::from("Command palette")),
            (Cow::Borrowed("Q"), Line::from("Quit")),
            (Cow::Borrowed("S"), Line::from("Settings")),
        ]
    }

    fn glyphs(buf: &Buffer, y: u16, w: u16) -> String {
        (0..w)
            .map(|x| buf.get(Position::new(x, y)).unwrap().symbol)
            .collect()
    }

    #[test]
    fn it_anchors_to_the_bottom_sized_to_content() {
        let r = rows();
        let outer = Rect::new(0, 0, 40, 20);
        let a = WhichKey::new(&r).area(outer);
        // 3 rows + bordered frame (2) = height 5, flush to the bottom.
        assert_eq!(a.height, 5);
        assert_eq!(a.y + a.height, 20, "flush against the bottom edge");
        assert_eq!(a.x, 0);
        assert!(a.width < 40, "content-sized, not full width");
    }

    #[test]
    fn it_renders_kbd_caps_and_labels_opaque() {
        let r = rows();
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 8));
        for p in buf.area().positions() {
            buf.set_cell(p, '.', Style::new());
        }
        WhichKey::new(&r).render(buf.area(), &mut buf);
        let all: String = (0..8).map(|y| glyphs(&buf, y, 30)).collect();
        assert!(all.contains('['), "key caps via reused Kbd");
        assert!(all.contains("Command palette"), "labels render");
        assert!(all.contains("⟨leader⟩"), "the title shows on the frame");
        // The box is opaque: its interior is not the '.' background.
        let a = WhichKey::new(&r).area(buf.area());
        assert_ne!(
            buf.get(Position::new(a.x + 1, a.y + 1)).unwrap().symbol,
            '.'
        );
    }

    #[test]
    fn max_height_clips_the_row_count() {
        let many: Vec<(Cow<str>, Line)> = (0..20)
            .map(|i| (Cow::Owned(i.to_string()), Line::from("x")))
            .collect();
        let a = WhichKey::new(&many)
            .max_height(4)
            .area(Rect::new(0, 0, 20, 30));
        assert_eq!(a.height, 4 + 2, "4 rows + frame, the rest clipped");
    }

    #[test]
    fn key_style_and_label_style_cascade() {
        let r = vec![(Cow::Borrowed("A"), Line::from("act"))];
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 4));
        WhichKey::new(&r)
            .block(Block::bordered())
            .key_style(Style::new().fg(Color::Cyan))
            .label_style(Style::new().fg(Color::Yellow))
            .render(buf.area(), &mut buf);
        let a = WhichKey::new(&r).area(buf.area());
        // Inside the frame: the key cap '[' is cyan…
        let cap = buf.get(Position::new(a.x + 1, a.y + 1)).unwrap();
        assert_eq!(cap.symbol, '[');
        assert_eq!(cap.fg, Color::Cyan);
    }

    #[test]
    fn no_rows_is_a_total_no_op() {
        let r: [(Cow<'_, str>, Line<'_>); 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        for p in buf.area().positions() {
            buf.set_cell(p, '.', Style::new());
        }
        WhichKey::new(&r).render(buf.area(), &mut buf);
        assert!(
            buf.cells().iter().all(|c| c.symbol == '.'),
            "nothing drawn when there is nothing to hint"
        );
        assert_eq!(WhichKey::new(&r).area(Rect::new(0, 0, 10, 4)).height, 0);
    }

    #[test]
    fn zero_and_tiny_areas_never_panic() {
        let r = rows();
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        WhichKey::new(&r).render(Rect::new(0, 0, 0, 0), &mut buf);
        WhichKey::new(&r).render(Rect::new(0, 0, 3, 2), &mut buf);
        // Clamped to the tiny area, still no panic.
        let a = WhichKey::new(&r).area(Rect::new(0, 0, 3, 2));
        assert!(a.width <= 3 && a.height <= 2);
    }

    #[test]
    fn inner_is_the_box_without_the_block() {
        let r = rows();
        let outer = Rect::new(0, 0, 40, 20);
        let wk = WhichKey::new(&r);
        let a = wk.area(outer);
        let inner = wk.inner(outer);
        assert_eq!(inner, Block::bordered().inner(a));
    }
}
