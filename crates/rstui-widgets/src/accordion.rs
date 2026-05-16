//! [`Accordion`] — a vertical stack of titled, collapsible sections; the
//! basis for settings panes, inspector sidebars, and grouped option lists.
//!
//! # A pure projection of caller-owned expansion, on purpose
//!
//! Which sections are open is ordinary application state — exactly like
//! [`List`](crate::List)'s `selected` or [`Tree`](crate::Tree)'s flattened
//! visible rows. `Accordion` therefore owns none of it: each
//! [`AccordionSection`] carries a caller-owned [`expanded`](AccordionSection::expanded)
//! `bool` and a caller-owned [`body_height`](AccordionSection::body_height),
//! and the reducer flips `expanded` in `update` when the user toggles a
//! header. The widget only ever *reads* that state, so it fits
//! `App::view(&self)` and is deterministically headless-testable; it never
//! mutates anything at render time.
//!
//! Like [`SplitPane`](crate::SplitPane) and
//! [`Modal::inner`](crate::Modal::inner), it takes **no child widgets** — it
//! is pure layout. [`layout`](Accordion::layout) hands back one
//! `Option<Rect>` per section (`Some` is an expanded section's body rect,
//! `None` is collapsed or off-screen) and the caller renders its own content
//! into the open ones. The widget itself draws only the headers (a ▾/▸ marker
//! plus the title); a single mutually-exclusive "only one open at a time"
//! accordion is just the caller keeping one `expanded` true — that invariant
//! is the reducer's, never the widget's.
//!
//! # Deliberately deferred
//!
//! Animated open/close transitions (a wall clock smuggled into a pure `view`,
//! the [`Spinner`](crate::Spinner) caller-owned-tick precedent forbids that),
//! per-section framing borders, and a scrolling overflow when the sections are
//! taller than the area, are additive follow-ups that compose from this shape
//! rather than changing it — so they are not smuggled in here. An over-tall
//! body simply clips; sections past the area get `None`.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::Block;

/// One section of an [`Accordion`]: a title, a caller-owned
/// [`expanded`](Self::expanded) flag, and a caller-owned
/// [`body_height`](Self::body_height) for its content.
///
/// The title is a full [`Line`], so a plain `&str`, a styled
/// [`Span`](rstui_core::Span), or a `Vec<Span>` all work and cascade over the
/// accordion's header style the same text→line→span way the rest of the
/// codebase does. `expanded` and `body_height` are *application state* the
/// reducer owns — the section only records them for the widget to read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AccordionSection<'a> {
    title: Line<'a>,
    expanded: bool,
    body_height: u16,
}

impl<'a> AccordionSection<'a> {
    /// A collapsed section titled `title`, with a zero-height body.
    pub fn new(title: impl Into<Line<'a>>) -> Self {
        Self {
            title: title.into(),
            expanded: false,
            body_height: 0,
        }
    }

    /// Sets whether the section is open (caller-owned state the reducer
    /// flips; the widget only reads it).
    #[must_use]
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// Sets the body's height in rows, reserved below the header *when*
    /// [`expanded`](Self::expanded). Clamped to the space left, so an
    /// over-tall body clips rather than overflowing.
    #[must_use]
    pub fn body_height(mut self, body_height: u16) -> Self {
        self.body_height = body_height;
        self
    }
}

/// A vertical stack of titled, collapsible sections.
///
/// Each section is a 1-row header (a ▾/▸ marker plus its title) followed,
/// *only when [`expanded`](AccordionSection::expanded)*, by a reserved body of
/// [`body_height`](AccordionSection::body_height) rows the caller draws into.
/// `Accordion` owns no expansion state — see the [module docs](self) — and
/// renders only the headers; [`layout`](Self::layout) returns the body rects.
///
/// Styling cascades accordion → header → title-line → span (the same
/// [`Style::patch`](rstui_core::Style) model the text model uses); the base
/// style fills the content area and [`header_style`](Self::header_style) is a
/// full-width bar over each header row.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_widgets::{Accordion, AccordionSection};
///
/// // `expanded` is caller state — the reducer flips it on a header toggle;
/// // the widget only reads it to decide whether to reserve a body rect.
/// let acc = Accordion::new([
///     AccordionSection::new("General").expanded(true).body_height(2),
///     AccordionSection::new("Advanced"),
/// ]);
///
/// let bodies = acc.layout(Rect::new(0, 0, 12, 6));
/// assert_eq!(bodies[0], Some(Rect::new(0, 1, 12, 2))); // open: body reserved
/// assert_eq!(bodies[1], None); // collapsed: no body
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 6));
/// acc.render(buf.area(), &mut buf);
/// // The headers carry the open/closed marker; bodies are the caller's.
/// assert_eq!(buf.get(rstui_core::Position::new(0, 0)).unwrap().symbol, '▾');
/// assert_eq!(buf.get(rstui_core::Position::new(0, 3)).unwrap().symbol, '▸');
/// ```
#[derive(Debug, Clone)]
pub struct Accordion<'a> {
    sections: Vec<AccordionSection<'a>>,
    block: Option<Block<'a>>,
    style: Style,
    header_style: Style,
    expanded_marker: char,
    collapsed_marker: char,
}

impl Default for Accordion<'_> {
    fn default() -> Self {
        Self {
            sections: Vec::new(),
            block: None,
            style: Style::new(),
            header_style: Style::new(),
            expanded_marker: '▾',
            collapsed_marker: '▸',
        }
    }
}

impl<'a> Accordion<'a> {
    /// An accordion of `sections`, unframed, with the default ▾/▸ markers.
    pub fn new<I>(sections: I) -> Self
    where
        I: IntoIterator<Item = AccordionSection<'a>>,
    {
        Self {
            sections: sections.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Frames the accordion in `block`; sections are placed inside
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`], beneath the header → title → span cascade. It
    /// fills the content area so a background covers the whole region.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] applied across each full header row as a bar,
    /// patched over the base.
    #[must_use]
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Sets the marker drawn before an [`expanded`](AccordionSection::expanded)
    /// section's title (default `▾`).
    #[must_use]
    pub fn expanded_marker(mut self, marker: char) -> Self {
        self.expanded_marker = marker;
        self
    }

    /// Sets the marker drawn before a collapsed section's title (default `▸`).
    #[must_use]
    pub fn collapsed_marker(mut self, marker: char) -> Self {
        self.collapsed_marker = marker;
        self
    }

    /// The body rect of every section, in order: `Some(rect)` for an open
    /// section whose body fits (clipped if it only partly fits), `None` for a
    /// collapsed section, a zero-height body, or a section pushed off the
    /// bottom.
    ///
    /// A pure function of `area` and the caller-owned section state — render
    /// the caller's own content into the `Some` rects, exactly as with
    /// [`SplitPane::split`](crate::SplitPane::split).
    #[must_use]
    pub fn layout(&self, area: Rect) -> Vec<Option<Rect>> {
        self.place(area).into_iter().map(|p| p.body).collect()
    }

    /// Per-section header row + body rect, computed exactly one way so
    /// [`layout`](Self::layout) and [`render`](Widget::render) never disagree.
    fn place(&self, area: Rect) -> Vec<Placement> {
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        let x = inner.left();
        let width = inner.width;
        let bottom = inner.bottom();
        let mut y = inner.top();

        let mut out = Vec::with_capacity(self.sections.len());
        for section in &self.sections {
            if inner.is_empty() || y >= bottom {
                out.push(Placement::HIDDEN);
                continue;
            }
            let header_y = y;
            y = y.saturating_add(1);

            let body = if section.expanded {
                let avail = bottom.saturating_sub(y);
                let h = section.body_height.min(avail);
                if h > 0 {
                    let rect = Rect::new(x, y, width, h);
                    y = y.saturating_add(h);
                    Some(rect)
                } else {
                    None
                }
            } else {
                None
            };
            out.push(Placement {
                header: Some(header_y),
                body,
            });
        }
        out
    }
}

/// Where one section landed: its header row (if it fit) and its body rect (if
/// open and non-empty).
#[derive(Clone, Copy)]
struct Placement {
    header: Option<u16>,
    body: Option<Rect>,
}

impl Placement {
    /// A section pushed off the bottom (or inside an empty area).
    const HIDDEN: Self = Self {
        header: None,
        body: None,
    };
}

impl Widget for Accordion<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let placements = self.place(area);

        let Accordion {
            sections,
            block,
            style,
            header_style,
            expanded_marker,
            collapsed_marker,
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
        // headers and the caller's bodies layer the cascade on top.
        buf.set_style(inner, style);

        let left = inner.left();
        let right = inner.right();
        for (section, placement) in sections.iter().zip(&placements) {
            let Some(hy) = placement.header else {
                continue;
            };

            // The header reads as a full-width bar; the marker and title are
            // stamped over it through accordion → header → line → span.
            buf.set_style(Rect::new(left, hy, inner.width, 1), header_style);
            let header_base = style.patch(header_style);

            let marker = if section.expanded {
                expanded_marker
            } else {
                collapsed_marker
            };
            buf.set_cell(Position::new(left, hy), marker, header_base);

            // The title starts past the marker and a one-cell gap, clipped at
            // the right edge.
            let title_base = header_base.patch(section.title.style);
            let mut tx = left.saturating_add(2);
            'title: for span in &section.title.spans {
                let span_style = title_base.patch(span.style);
                for ch in span.content.chars() {
                    if tx >= right {
                        break 'title;
                    }
                    buf.set_cell(Position::new(tx, hy), ch, span_style);
                    tx = tx.saturating_add(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier, Span};

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

    fn sample() -> Accordion<'static> {
        Accordion::new([
            AccordionSection::new("One").expanded(true).body_height(2),
            AccordionSection::new("Two"),
        ])
    }

    #[test]
    fn headers_carry_a_marker_and_open_sections_reserve_a_body() {
        // Row 0 header "One" (open ▾); rows 1-2 body (caller's, blank here);
        // row 3 header "Two" (closed ▸); row 4 unused.
        assert_eq!(
            lines(sample(), 6, 5),
            "▾ One \n      \n      \n▸ Two \n      \n"
        );
    }

    #[test]
    fn layout_returns_one_body_rect_per_section() {
        let bodies = sample().layout(Rect::new(0, 0, 6, 5));
        assert_eq!(bodies, vec![Some(Rect::new(0, 1, 6, 2)), None]);
    }

    #[test]
    fn a_collapsed_section_reserves_no_body() {
        let acc = Accordion::new([AccordionSection::new("X").body_height(3)]);
        assert_eq!(acc.layout(Rect::new(0, 0, 5, 5)), vec![None]);
        assert_eq!(lines(acc, 5, 2), "▸ X  \n     \n");
    }

    #[test]
    fn an_expanded_section_with_zero_body_height_is_none() {
        let acc = Accordion::new([AccordionSection::new("Z").expanded(true)]);
        assert_eq!(acc.layout(Rect::new(0, 0, 5, 5)), vec![None]);
    }

    #[test]
    fn an_over_tall_body_is_clipped_to_the_remaining_space() {
        // body_height 9 but only 2 rows remain after the header → clipped to 2.
        let acc = Accordion::new([AccordionSection::new("Big").expanded(true).body_height(9)]);
        assert_eq!(
            acc.layout(Rect::new(0, 0, 4, 3)),
            vec![Some(Rect::new(0, 1, 4, 2))]
        );
    }

    #[test]
    fn a_section_pushed_off_the_bottom_is_hidden() {
        // Two headers need 2 rows; with height 1 only the first header fits.
        let acc = Accordion::new([AccordionSection::new("A"), AccordionSection::new("B")]);
        assert_eq!(acc.layout(Rect::new(0, 0, 4, 1)), vec![None, None]);
        assert_eq!(lines(acc, 4, 1), "▸ A \n");
    }

    #[test]
    fn an_expanded_section_with_no_room_for_a_body_still_draws_its_header() {
        // Header on the only row; the body has zero rows left → None.
        let acc = Accordion::new([AccordionSection::new("H").expanded(true).body_height(3)]);
        assert_eq!(acc.layout(Rect::new(0, 0, 4, 1)), vec![None]);
        assert_eq!(lines(acc, 4, 1), "▾ H \n");
    }

    #[test]
    fn a_narrow_header_clips_the_title() {
        let acc = Accordion::new([AccordionSection::new("Overlong")]);
        // x0 marker, x1 gap, x2.. title clipped at width 5.
        assert_eq!(lines(acc, 5, 1), "▸ Ove\n");
    }

    #[test]
    fn custom_markers_replace_the_defaults() {
        let acc = Accordion::new([
            AccordionSection::new("o").expanded(true),
            AccordionSection::new("c"),
        ])
        .expanded_marker('-')
        .collapsed_marker('+');
        assert_eq!(lines(acc, 3, 2), "- o\n+ c\n");
    }

    #[test]
    fn a_block_frames_the_sections_in_the_inner_area() {
        let acc = Accordion::new([AccordionSection::new("Hi")]).block(Block::bordered());
        assert_eq!(lines(acc, 6, 3), "┌────┐\n│▸ Hi│\n└────┘\n");
    }

    #[test]
    fn style_cascades_accordion_header_line_span_and_fills_the_area() {
        let title = Line::from(vec![Span::styled("T", Style::new().fg(Color::Red))])
            .style(Style::new().add_modifier(Modifier::BOLD));
        let acc = Accordion::new([AccordionSection::new(title)])
            .style(Style::new().bg(Color::Blue))
            .header_style(Style::new().fg(Color::Green));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        acc.render(buf.area(), &mut buf);

        // Marker inherits header fg over the base bg.
        let m = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(m.symbol, '▸');
        assert_eq!(m.fg, Color::Green);
        assert_eq!(m.bg, Color::Blue);
        // Title span fg wins; the line BOLD and base bg cascade through.
        let t = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(t.symbol, 'T');
        assert_eq!(t.fg, Color::Red);
        assert_eq!(t.bg, Color::Blue);
        assert!(t.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn an_empty_accordion_is_safe() {
        assert!(
            Accordion::new(Vec::<AccordionSection>::new())
                .layout(Rect::new(0, 0, 5, 5))
                .is_empty()
        );
        assert_eq!(
            lines(Accordion::new(Vec::<AccordionSection>::new()), 3, 2),
            "   \n   \n"
        );
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_headers() {
        let acc = Accordion::new([AccordionSection::new("Z")]).block(Block::bordered());
        assert_eq!(acc.layout(Rect::new(0, 0, 2, 2)), vec![None]);
        assert_eq!(lines(acc, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        sample().render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
