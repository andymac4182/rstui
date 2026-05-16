//! [`Tabs`] — a single horizontal row of titles with one highlighted, the
//! basis for top-level app sections, editor tab strips, and settings panes.
//!
//! # The same pure projection as [`List`](crate::List), one axis over
//!
//! [`List`](crate::List) made the case (see its module docs) that a selectable
//! widget in rstui is a *pure projection* of caller-owned state, never a
//! render-time mutator like ratatui's `StatefulWidget`: `App::view` takes
//! `&self`, so the selected index is ordinary application state the reducer
//! owns and changes in `update`. `Tabs` is the same pattern with the rows laid
//! out left-to-right instead of top-to-bottom — the
//! [`selected`](Tabs::selected) tab is read here, never written — which is the
//! concrete evidence that the projection model is axis-independent, not a
//! one-off that happened to fit a vertical list.
//!
//! The method and field are named [`selected`](Tabs::selected), identical to
//! [`List`](crate::List), on purpose: a developer composing both in one app
//! moves the same "which index is highlighted" idiom between them unchanged.
//!
//! Unlike `List`'s full-width selection *bar*, `Tabs` highlights only the
//! selected title's glyphs — the divider and the inter-title padding keep the
//! base style. That is the ratatui-proven tab idiom (the selected label reads
//! as emphasised text, e.g. bold/reversed, not a block fill) and the right
//! divergence: a tab strip and a list row mean different things by "selected".
//! Scrolling a long strip so the selected tab stays visible is the same
//! deliberately-deferred stateful-widget question `List` flagged, and is not
//! smuggled in here.

use crate::block::Block;
use rstui_core::{Buffer, Line, Position, Rect, Span, Style, Widget};

/// A one-row strip of titles with at most one [`selected`](Self::selected).
///
/// Each title is a [`Line`] (built from anything a `Line` is: `&str`,
/// `String`, [`Span`], `Vec<Span>`, …). Titles render left to right as
/// `␣title␣` cells joined by a [`divider`](Self::divider) (default `│`), all
/// on the first row of the area (or of [`block.inner`](Block::inner) when a
/// framing [`block`](Self::block) is set), clipped at the right edge.
///
/// Styling cascades tabs → title-line → span (the same
/// [`Style::patch`](rstui_core::Style) model the text model uses); the base
/// [`style`](Self::style) also fills the strip row so a background spans it.
/// On the [`selected`](Self::selected) tab
/// [`highlight_style`](Self::highlight_style) is patched **last** over the
/// title glyphs only — the padding and dividers keep the base style, so the
/// selected label reads as emphasised text rather than a block bar (the
/// deliberate divergence from [`List`](crate::List); see the [module
/// docs](self)). A `selected` index outside the title range simply highlights
/// nothing.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Tabs;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 13, 1));
/// Tabs::new(["One", "Two"]).selected(Some(1)).render(buf.area(), &mut buf);
///
/// // Rendered as ` One │ Two   `: a leading pad, a divider between tabs.
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'O');
/// assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, '│');
/// assert_eq!(buf.get(Position::new(7, 0)).unwrap().symbol, 'T');
/// ```
#[derive(Debug, Clone)]
pub struct Tabs<'a> {
    titles: Vec<Line<'a>>,
    block: Option<Block<'a>>,
    style: Style,
    highlight_style: Style,
    divider: Span<'a>,
    selected: Option<usize>,
}

impl Default for Tabs<'_> {
    fn default() -> Self {
        Self {
            titles: Vec::new(),
            block: None,
            style: Style::new(),
            highlight_style: Style::new(),
            divider: Span::raw("│"),
            selected: None,
        }
    }
}

impl<'a> Tabs<'a> {
    /// A strip of `titles`, nothing selected, default `│` divider.
    pub fn new<I, T>(titles: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Line<'a>>,
    {
        Self {
            titles: titles.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// Frames the strip in `block`; titles render into [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`], beneath the tabs → title → span cascade. It
    /// also fills the strip row so a background spans the whole strip.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched over the selected title's glyphs.
    ///
    /// Patched **last** in the cascade, so it overrides per-span styling, but
    /// applied to the title text only — the padding and dividers keep the
    /// base style (see the [module docs](self) for why).
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Sets the [`Span`] drawn between adjacent titles (never before the
    /// first or after the last). Defaults to `│`; carries its own style,
    /// patched over the base.
    #[must_use]
    pub fn divider(mut self, divider: impl Into<Span<'a>>) -> Self {
        self.divider = divider.into();
        self
    }

    /// Sets which title index is highlighted, or `None` for no selection.
    ///
    /// An index outside the title range simply highlights nothing — the
    /// caller owns the index (see the [module docs](self)).
    #[must_use]
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }
}

impl Widget for Tabs<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Tabs {
            titles,
            block,
            style,
            highlight_style,
            divider,
            selected,
        } = self;

        // The block (if any) frames the strip and reserves the inner area.
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

        // The strip is one row; its base style fills that row so a background
        // spans the whole strip (padding and dividers included), with glyphs
        // layering the tabs → title → span cascade on top.
        let y = inner.top();
        buf.set_style(Rect::new(inner.left(), y, inner.width, 1), style);

        let right = inner.right();
        let divider_style = style.patch(divider.style);
        let mut x = inner.left();

        // Stamps `text`'s chars at the cursor in `cell_style`, stopping at the
        // right edge; returns `false` once clipped so the caller bails out.
        let stamp = |buf: &mut Buffer, x: &mut u16, text: &str, cell_style: Style| {
            for ch in text.chars() {
                if *x >= right {
                    return false;
                }
                buf.set_cell(Position::new(*x, y), ch, cell_style);
                *x = x.saturating_add(1);
            }
            true
        };

        'strip: for (i, title) in titles.into_iter().enumerate() {
            // A divider sits between tabs only — not before the first.
            if i > 0 && !stamp(buf, &mut x, &divider.content, divider_style) {
                break 'strip;
            }
            // A space pads each side of every title (the readable default).
            if !stamp(buf, &mut x, " ", style) {
                break 'strip;
            }

            let is_selected = selected == Some(i);
            let line_base = style.patch(title.style);
            for span in title.spans {
                let mut span_style = line_base.patch(span.style);
                if is_selected {
                    // Highlight wins last, over the selected title glyphs only.
                    span_style = span_style.patch(highlight_style);
                }
                if !stamp(buf, &mut x, &span.content, span_style) {
                    break 'strip;
                }
            }

            if !stamp(buf, &mut x, " ", style) {
                break 'strip;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier};

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
    fn titles_are_space_padded_and_divider_joined() {
        assert_eq!(lines(Tabs::new(["A", "B", "C"]), 13, 1), " A │ B │ C   \n");
    }

    #[test]
    fn no_divider_before_the_first_or_after_the_last_tab() {
        // One tab: just ` X `, no divider at all.
        assert_eq!(lines(Tabs::new(["X"]), 5, 1), " X   \n");
    }

    #[test]
    fn selected_highlights_only_the_title_glyphs() {
        let tabs = Tabs::new(["A", "B"])
            .selected(Some(1))
            .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        tabs.render(buf.area(), &mut buf);
        // ` A │ B  ` — only the 'B' glyph at x=5 carries the highlight; the
        // divider, padding, and the unselected 'A' keep the base style.
        assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, 'B');
        assert_eq!(buf.get(Position::new(5, 0)).unwrap().bg, Color::Blue);
        for x in [1, 3, 4, 6] {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Reset);
        }
    }

    #[test]
    fn no_selection_highlights_nothing() {
        let tabs = Tabs::new(["A", "B"]).highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        tabs.render(buf.area(), &mut buf);
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Reset);
        }
    }

    #[test]
    fn a_selection_out_of_range_highlights_nothing() {
        let tabs = Tabs::new(["A", "B"])
            .selected(Some(7))
            .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        tabs.render(buf.area(), &mut buf);
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Reset);
        }
    }

    #[test]
    fn a_custom_divider_replaces_the_default() {
        assert_eq!(
            lines(Tabs::new(["A", "B"]).divider("--"), 9, 1),
            " A -- B  \n"
        );
    }

    #[test]
    fn the_strip_clips_at_the_right_edge_without_panicking() {
        // Width 6 cuts through the second tab: ` A │ B  ` clipped to ` A │ B`.
        assert_eq!(lines(Tabs::new(["A", "B", "C"]), 6, 1), " A │ B\n");
    }

    #[test]
    fn block_frames_the_strip_in_the_inner_area() {
        assert_eq!(
            lines(Tabs::new(["A", "B"]).block(Block::bordered()), 9, 3),
            "┌───────┐\n│ A │ B │\n└───────┘\n"
        );
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_titles() {
        assert_eq!(
            lines(Tabs::new(["A"]).block(Block::bordered()), 2, 2),
            "┌┐\n└┘\n"
        );
    }

    #[test]
    fn base_style_fills_the_whole_strip_row() {
        let tabs = Tabs::new(["A", "B"]).style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        tabs.render(buf.area(), &mut buf);
        // Padding, dividers, and the trailing slack all share the base bg.
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Red);
        }
    }

    #[test]
    fn style_cascades_tabs_title_span_and_highlight_wins_last() {
        // Title line is BOLD; one span is red. The tabs base is green. On the
        // selected tab the highlight bg is patched last over the glyphs.
        let title = Line::from(vec![
            Span::styled("X", Style::new().fg(Color::Red)),
            Span::raw("y"),
        ])
        .style(Style::new().add_modifier(Modifier::BOLD));
        let tabs = Tabs::new([title])
            .style(Style::new().fg(Color::Green))
            .selected(Some(0))
            .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        tabs.render(buf.area(), &mut buf);

        // x=0 is the leading pad (base only); the title is at x=1,2.
        let cx = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(cx.symbol, 'X');
        assert_eq!(cx.fg, Color::Red); // span fg survives
        assert_eq!(cx.bg, Color::Blue); // highlight patched last
        assert!(cx.modifier.contains(Modifier::BOLD)); // line modifier cascades

        let cy = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(cy.symbol, 'y');
        assert_eq!(cy.fg, Color::Green); // inherits tabs base (no span fg)
        assert_eq!(cy.bg, Color::Blue);
        assert!(cy.modifier.contains(Modifier::BOLD));

        // The leading pad is base style only — no highlight bleed.
        let pad = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(pad.fg, Color::Green);
        assert_eq!(pad.bg, Color::Reset);
    }

    #[test]
    fn an_empty_strip_with_a_block_still_renders_the_block() {
        assert_eq!(
            lines(Tabs::new(Vec::<&str>::new()).block(Block::bordered()), 3, 3),
            "┌─┐\n│ │\n└─┘\n"
        );
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Tabs::new(["hello"])
            .selected(Some(0))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
