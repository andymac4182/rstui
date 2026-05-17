//! [`Block`] — the foundational container nearly every other widget renders
//! inside.
//!
//! A `Block` is an optional border (any subset of the four sides, in one of
//! several line styles), a styled area fill, [`Padding`], and a clipped title
//! that is a full [`Line`] — so it carries per-span styles and its own
//! alignment, cascading over block-level `title_style`/`title_alignment` the
//! same text→line→span way [`Text`](rstui_core::Text) does. The all-important
//! [`Block::inner`] hands the area left after borders and padding back to the
//! content drawn inside it, which is how a block composes with what it frames.

use rstui_core::{Alignment, Buffer, Line, Position, Rect, Style, Widget};

/// Which sides of a [`Block`] draw a border, as a small bitset.
///
/// Hand-rolled rather than pulling in `bitflags`, matching
/// [`Modifier`](rstui_core::Modifier) — the core crate stays dependency-free.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Borders(u8);

impl Borders {
    /// No border on any side.
    pub const NONE: Self = Self(0);
    /// The top edge.
    pub const TOP: Self = Self(1 << 0);
    /// The right edge.
    pub const RIGHT: Self = Self(1 << 1);
    /// The bottom edge.
    pub const BOTTOM: Self = Self(1 << 2);
    /// The left edge.
    pub const LEFT: Self = Self(1 << 3);
    /// All four edges.
    pub const ALL: Self = Self(0b1111);

    /// Returns `true` if every side in `other` is also set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns `self` with the sides in `other` added.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns `true` if no side is set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for Borders {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for Borders {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

/// The six box-drawing glyphs that draw a border.
///
/// Every glyph is a single [`char`] (Unicode box-drawing is single-scalar), so
/// it maps 1:1 onto a [`Cell`](rstui_core::Cell). Construct one with
/// [`BorderType::set`] or build a custom set for bespoke frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BorderSet {
    /// Top-left corner.
    pub top_left: char,
    /// Top-right corner.
    pub top_right: char,
    /// Bottom-left corner.
    pub bottom_left: char,
    /// Bottom-right corner.
    pub bottom_right: char,
    /// Vertical edge (left and right sides).
    pub vertical: char,
    /// Horizontal edge (top and bottom sides).
    pub horizontal: char,
}

/// The visual style of a [`Block`]'s border lines.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BorderType {
    /// Single line with square corners (`┌─┐│└┘`).
    #[default]
    Plain,
    /// Single line with rounded corners (`╭─╮│╰╯`).
    Rounded,
    /// Double line (`╔═╗║╚╝`).
    Double,
    /// Heavy line (`┏━┓┃┗┛`).
    Thick,
}

impl BorderType {
    /// The [`BorderSet`] of glyphs this border type draws with.
    #[must_use]
    pub const fn set(self) -> BorderSet {
        match self {
            Self::Plain => BorderSet {
                top_left: '┌',
                top_right: '┐',
                bottom_left: '└',
                bottom_right: '┘',
                vertical: '│',
                horizontal: '─',
            },
            Self::Rounded => BorderSet {
                top_left: '╭',
                top_right: '╮',
                bottom_left: '╰',
                bottom_right: '╯',
                vertical: '│',
                horizontal: '─',
            },
            Self::Double => BorderSet {
                top_left: '╔',
                top_right: '╗',
                bottom_left: '╚',
                bottom_right: '╝',
                vertical: '║',
                horizontal: '═',
            },
            Self::Thick => BorderSet {
                top_left: '┏',
                top_right: '┓',
                bottom_left: '┗',
                bottom_right: '┛',
                vertical: '┃',
                horizontal: '━',
            },
        }
    }
}

/// Empty cells reserved inside a [`Block`], between its border and its content.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Padding {
    /// Columns reserved on the left.
    pub left: u16,
    /// Columns reserved on the right.
    pub right: u16,
    /// Rows reserved on the top.
    pub top: u16,
    /// Rows reserved on the bottom.
    pub bottom: u16,
}

impl Padding {
    /// No padding on any side.
    pub const ZERO: Self = Self {
        left: 0,
        right: 0,
        top: 0,
        bottom: 0,
    };

    /// Padding with each side set explicitly.
    #[must_use]
    pub const fn new(left: u16, right: u16, top: u16, bottom: u16) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    /// The same padding on all four sides.
    #[must_use]
    pub const fn uniform(value: u16) -> Self {
        Self::new(value, value, value, value)
    }

    /// `horizontal` on the left/right and `vertical` on the top/bottom.
    #[must_use]
    pub const fn symmetric(horizontal: u16, vertical: u16) -> Self {
        Self::new(horizontal, horizontal, vertical, vertical)
    }
}

/// The foundational container widget: an optional border with a styled fill,
/// padding, and a clipped title.
///
/// A `Block` is the frame nearly every other widget renders inside. The usual
/// pattern is to render the block, then render content into the area it
/// reserves:
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_widgets::Block;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 5));
/// let block = Block::bordered().title("Logs");
/// let inner = block.inner(buf.area());
/// block.render(buf.area(), &mut buf);
/// "first log line".render(inner, &mut buf);
/// ```
#[derive(Debug, Default, Clone)]
pub struct Block<'a> {
    borders: Borders,
    border_type: BorderType,
    border_style: Style,
    style: Style,
    title: Option<Line<'a>>,
    title_alignment: Alignment,
    title_style: Style,
    bottom_title: Option<Line<'a>>,
    bottom_title_alignment: Alignment,
    padding: Padding,
}

impl<'a> Block<'a> {
    /// A block with no border, no title, and no fill — a transparent region.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A block bordered on all four sides ([`Borders::ALL`]).
    #[must_use]
    pub fn bordered() -> Self {
        Self::default().borders(Borders::ALL)
    }

    /// Sets which sides draw a border.
    #[must_use]
    pub fn borders(mut self, borders: Borders) -> Self {
        self.borders = borders;
        self
    }

    /// Sets the line style the border is drawn with.
    #[must_use]
    pub fn border_type(mut self, border_type: BorderType) -> Self {
        self.border_type = border_type;
        self
    }

    /// Sets the [`Style`] applied to the border glyphs.
    #[must_use]
    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    /// Sets the [`Style`] used to fill the whole block area (its background).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the title drawn on the top edge.
    ///
    /// The title is a full [`Line`], so `"plain"`, a styled [`Span`], or a
    /// `Vec<Span>` of differently-styled runs all work. A [`Line`] with its
    /// own [`alignment`](Line::alignment) overrides
    /// [`title_alignment`](Self::title_alignment); per-span styles cascade over
    /// [`title_style`](Self::title_style) — the same text→line→span model
    /// [`Text`](rstui_core::Text) uses.
    ///
    /// [`Span`]: rstui_core::Span
    #[must_use]
    pub fn title(mut self, title: impl Into<Line<'a>>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the default title alignment, used when the title [`Line`] does not
    /// set its own.
    #[must_use]
    pub fn title_alignment(mut self, alignment: Alignment) -> Self {
        self.title_alignment = alignment;
        self
    }

    /// Sets the base [`Style`] for the title, beneath each title span's own
    /// style in the cascade.
    #[must_use]
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// Sets a title drawn on the **bottom** edge row (the opencode
    /// compaction-divider / footer-label use).
    ///
    /// The mirror of [`title`](Self::title): a full [`Line`] clipped between
    /// the vertical borders, its own [`alignment`](Line::alignment) winning
    /// over [`bottom_title_alignment`](Self::bottom_title_alignment), per-span
    /// styles cascading over the shared [`title_style`](Self::title_style).
    #[must_use]
    pub fn bottom_title(mut self, title: impl Into<Line<'a>>) -> Self {
        self.bottom_title = Some(title.into());
        self
    }

    /// Sets the default bottom-title alignment, used when the bottom-title
    /// [`Line`] does not set its own.
    #[must_use]
    pub fn bottom_title_alignment(mut self, alignment: Alignment) -> Self {
        self.bottom_title_alignment = alignment;
        self
    }

    /// Sets the [`Padding`] reserved between the border and the content.
    #[must_use]
    pub fn padding(mut self, padding: Padding) -> Self {
        self.padding = padding;
        self
    }

    /// The area left for content after removing borders and padding.
    ///
    /// This is how a block composes with what it contains: render the block
    /// into `area`, then render content into `block.inner(area)`. The result
    /// is clamped (never larger than `area`, never negative) so a block in a
    /// tiny region degrades to an empty inner rect instead of panicking.
    #[must_use]
    pub fn inner(&self, area: Rect) -> Rect {
        let left = u16::from(self.borders.contains(Borders::LEFT)) + self.padding.left;
        let right = u16::from(self.borders.contains(Borders::RIGHT)) + self.padding.right;
        let top = u16::from(self.borders.contains(Borders::TOP)) + self.padding.top;
        let bottom = u16::from(self.borders.contains(Borders::BOTTOM)) + self.padding.bottom;
        Rect::new(
            area.x.saturating_add(left),
            area.y.saturating_add(top),
            area.width.saturating_sub(left.saturating_add(right)),
            area.height.saturating_sub(top.saturating_add(bottom)),
        )
    }
}

impl Widget for Block<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // Fill first so borders and the title layer on top of the background.
        buf.set_style(area, self.style);

        let borders = self.borders;
        let set = self.border_type.set();
        let bs = self.border_style;
        let (left, right) = (area.left(), area.right());
        let (top, bottom) = (area.top(), area.bottom());

        // Draw full-length edges, then stamp corners only where two edges
        // meet — a single present edge stays a straight rule with no corner.
        if borders.contains(Borders::TOP) {
            for x in left..right {
                buf.set_cell(Position::new(x, top), set.horizontal, bs);
            }
        }
        if borders.contains(Borders::BOTTOM) {
            for x in left..right {
                buf.set_cell(Position::new(x, bottom - 1), set.horizontal, bs);
            }
        }
        if borders.contains(Borders::LEFT) {
            for y in top..bottom {
                buf.set_cell(Position::new(left, y), set.vertical, bs);
            }
        }
        if borders.contains(Borders::RIGHT) {
            for y in top..bottom {
                buf.set_cell(Position::new(right - 1, y), set.vertical, bs);
            }
        }
        if borders.contains(Borders::TOP) && borders.contains(Borders::LEFT) {
            buf.set_cell(Position::new(left, top), set.top_left, bs);
        }
        if borders.contains(Borders::TOP) && borders.contains(Borders::RIGHT) {
            buf.set_cell(Position::new(right - 1, top), set.top_right, bs);
        }
        if borders.contains(Borders::BOTTOM) && borders.contains(Borders::LEFT) {
            buf.set_cell(Position::new(left, bottom - 1), set.bottom_left, bs);
        }
        if borders.contains(Borders::BOTTOM) && borders.contains(Borders::RIGHT) {
            buf.set_cell(Position::new(right - 1, bottom - 1), set.bottom_right, bs);
        }

        if let Some(title) = self.title {
            // The title lives on the top row, inside whatever vertical
            // borders are present so it never overwrites a corner.
            let start = left + u16::from(borders.contains(Borders::LEFT));
            let end = right - u16::from(borders.contains(Borders::RIGHT));
            if end > start {
                let avail = end - start;
                let len = (title.width() as u16).min(avail);
                // A Line's own alignment wins; otherwise the block default.
                let alignment = title.alignment.unwrap_or(self.title_alignment);
                let x0 = match alignment {
                    Alignment::Left => start,
                    Alignment::Right => end - len,
                    Alignment::Center => start + (avail - len) / 2,
                };
                // Cascade: block title style → line style → span style, the
                // same text→line→span model `Text` uses. Only the title's own
                // glyph cells are stamped, so the border still shows through
                // around a short title.
                let base = self.title_style.patch(title.style);
                let mut x = x0;
                'title: for span in title.spans {
                    let style = base.patch(span.style);
                    for ch in span.content.chars() {
                        if x >= end {
                            break 'title;
                        }
                        buf.set_cell(Position::new(x, top), ch, style);
                        x = x.saturating_add(1);
                    }
                }
            }
        }

        if let Some(title) = self.bottom_title {
            // The mirror of the top title, on the bottom edge row, inside
            // whatever vertical borders are present so it never clobbers a
            // corner — the opencode compaction-divider / footer-label use.
            let start = left + u16::from(borders.contains(Borders::LEFT));
            let end = right - u16::from(borders.contains(Borders::RIGHT));
            if end > start {
                let avail = end - start;
                let len = (title.width() as u16).min(avail);
                let alignment = title.alignment.unwrap_or(self.bottom_title_alignment);
                let x0 = match alignment {
                    Alignment::Left => start,
                    Alignment::Right => end - len,
                    Alignment::Center => start + (avail - len) / 2,
                };
                // Same block title style → line → span cascade as the top
                // title; only the title's own cells are stamped so a short
                // bottom title leaves the border showing around it.
                let base = self.title_style.patch(title.style);
                let mut x = x0;
                'bottom_title: for span in title.spans {
                    let style = base.patch(span.style);
                    for ch in span.content.chars() {
                        if x >= end {
                            break 'bottom_title;
                        }
                        buf.set_cell(Position::new(x, bottom - 1), ch, style);
                        x = x.saturating_add(1);
                    }
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
    /// glyphs as one newline-terminated line per row — legible for asserting
    /// border art directly.
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
    fn bordered_block_draws_a_box() {
        assert_eq!(
            lines(Block::bordered(), 4, 3),
            "┌──┐\n\
             │  │\n\
             └──┘\n"
        );
    }

    #[test]
    fn border_type_selects_the_glyph_set() {
        assert_eq!(
            lines(Block::bordered().border_type(BorderType::Rounded), 3, 2),
            "╭─╮\n╰─╯\n"
        );
        assert_eq!(
            lines(Block::bordered().border_type(BorderType::Double), 3, 2),
            "╔═╗\n╚═╝\n"
        );
        assert_eq!(
            lines(Block::bordered().border_type(BorderType::Thick), 3, 2),
            "┏━┓\n┗━┛\n"
        );
    }

    #[test]
    fn a_single_edge_is_a_rule_with_no_corners() {
        assert_eq!(
            lines(Block::new().borders(Borders::TOP), 4, 2),
            "────\n    \n"
        );
        // Two adjacent edges meet at exactly one corner.
        assert_eq!(
            lines(Block::new().borders(Borders::TOP | Borders::LEFT), 3, 2),
            "┌──\n│  \n"
        );
    }

    #[test]
    fn title_is_aligned_and_clipped_inside_the_borders() {
        assert_eq!(
            lines(Block::bordered().title("Hi"), 6, 2),
            "┌Hi──┐\n└────┘\n"
        );
        assert_eq!(
            lines(
                Block::bordered()
                    .title("Hi")
                    .title_alignment(Alignment::Right),
                6,
                2
            ),
            "┌──Hi┐\n└────┘\n"
        );
        assert_eq!(
            lines(
                Block::bordered()
                    .title("Hi")
                    .title_alignment(Alignment::Center),
                7,
                2
            ),
            "┌─Hi──┐\n└─────┘\n"
        );
        // Overlong titles are truncated to the span between the borders.
        assert_eq!(
            lines(Block::bordered().title("overlong"), 5, 2),
            "┌ove┐\n└───┘\n"
        );
    }

    #[test]
    fn title_spans_keep_their_own_style_over_the_block_title_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 7, 2));
        Block::bordered()
            .title(Line::from(vec![
                Span::styled("E", Style::new().fg(Color::Red)),
                Span::raw("rr"),
            ]))
            .title_style(Style::new().add_modifier(Modifier::BOLD))
            .render(buf.area(), &mut buf);

        // ALL borders → the title starts just inside the left edge (x = 1).
        let e = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(e.symbol, 'E');
        assert_eq!(e.fg, Color::Red); // the span's own fg wins
        assert!(e.modifier.contains(Modifier::BOLD)); // block title_style base cascades

        let r = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(r.symbol, 'r');
        assert_eq!(r.fg, Color::Reset); // raw span sets no fg → not red
        assert!(r.modifier.contains(Modifier::BOLD)); // still inherits the base
    }

    #[test]
    fn title_line_alignment_overrides_the_block_default() {
        // The block defaults titles to the left, but this Line asks for the
        // right — the Line wins, mirroring the Text→Line alignment cascade.
        assert_eq!(
            lines(
                Block::bordered()
                    .title(Line::raw("Hi").right_aligned())
                    .title_alignment(Alignment::Left),
                6,
                2
            ),
            "┌──Hi┐\n└────┘\n"
        );
    }

    #[test]
    fn border_around_a_short_title_keeps_the_border_style_not_the_title_style() {
        // Regression: only the title's own cells are stamped. The horizontal
        // border cells beside a short title must keep the border style, not
        // pick up the title style (i.e. no whole-row fill).
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 2));
        Block::bordered()
            .border_style(Style::new().fg(Color::Cyan))
            .title(Span::styled("Hi", Style::new().fg(Color::Red)))
            .render(buf.area(), &mut buf);

        // x=1,2 are the title; x=3..6 are the top border between title and
        // the top-right corner.
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().fg, Color::Red);
        let border = buf.get(Position::new(4, 0)).unwrap();
        assert_eq!(border.symbol, '─');
        assert_eq!(border.fg, Color::Cyan); // border style untouched by the title
    }

    #[test]
    fn inner_subtracts_borders_and_padding_and_saturates() {
        let block = Block::bordered().padding(Padding::uniform(1));
        assert_eq!(block.inner(Rect::new(0, 0, 10, 6)), Rect::new(2, 2, 6, 2));

        // Asymmetric: only left+top borders, extra right padding.
        let block = Block::new()
            .borders(Borders::LEFT | Borders::TOP)
            .padding(Padding::new(0, 3, 0, 0));
        assert_eq!(block.inner(Rect::new(4, 5, 20, 10)), Rect::new(5, 6, 16, 9));

        // Too small to fit the frame: inner collapses instead of underflowing.
        assert!(Block::bordered().inner(Rect::new(0, 0, 1, 1)).is_empty());
    }

    #[test]
    fn style_fills_the_area_and_border_style_overrides_the_edges() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 3));
        Block::bordered()
            .style(Style::new().bg(Color::Blue))
            .border_style(Style::new().fg(Color::Red).add_modifier(Modifier::BOLD))
            .render(buf.area(), &mut buf);

        // The interior cell keeps the fill background.
        let inner = buf.get(Position::new(1, 1)).unwrap();
        assert_eq!(inner.bg, Color::Blue);
        assert_eq!(inner.symbol, ' ');

        // A border cell keeps the fill background but takes the border fg.
        let corner = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(corner.symbol, '┌');
        assert_eq!(corner.bg, Color::Blue);
        assert_eq!(corner.fg, Color::Red);
        assert!(corner.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn zero_sized_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Block::bordered()
            .title("x")
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    // ---- ADR 0012 §P2 additive: bottom title ----

    #[test]
    fn bottom_title_is_aligned_and_clipped_on_the_bottom_border() {
        // Mirrors the top-title alignment cases, on the bottom edge row.
        assert_eq!(
            lines(Block::bordered().bottom_title("Hi"), 6, 2),
            "┌────┐\n└Hi──┘\n"
        );
        assert_eq!(
            lines(
                Block::bordered()
                    .bottom_title("Hi")
                    .bottom_title_alignment(Alignment::Right),
                6,
                2
            ),
            "┌────┐\n└──Hi┘\n"
        );
        assert_eq!(
            lines(
                Block::bordered()
                    .bottom_title("Hi")
                    .bottom_title_alignment(Alignment::Center),
                7,
                2
            ),
            "┌─────┐\n└─Hi──┘\n"
        );
        // Overlong bottom titles truncate between the borders.
        assert_eq!(
            lines(Block::bordered().bottom_title("overlong"), 5, 2),
            "┌───┐\n└ove┘\n"
        );
    }

    #[test]
    fn top_and_bottom_titles_coexist_and_line_alignment_wins() {
        // Both edges carry a title; the bottom Line's own right-alignment
        // overrides the block default (the Text→Line cascade, like the top).
        assert_eq!(
            lines(
                Block::bordered()
                    .title("Top")
                    .bottom_title(Line::raw("End").right_aligned())
                    .bottom_title_alignment(Alignment::Left),
                7,
                3
            ),
            "┌Top──┐\n│     │\n└──End┘\n"
        );
    }

    #[test]
    fn bottom_title_span_styles_cascade_over_the_shared_title_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 7, 2));
        Block::bordered()
            .title_style(Style::new().add_modifier(Modifier::BOLD))
            .bottom_title(Span::styled("Lo", Style::new().fg(Color::Red)))
            .render(buf.area(), &mut buf);
        // x=1,2 on the bottom row hold the title; it cascades the shared
        // title_style base under its own span fg.
        let l = buf.get(Position::new(1, 1)).unwrap();
        assert_eq!(l.symbol, 'L');
        assert_eq!(l.fg, Color::Red);
        assert!(l.modifier.contains(Modifier::BOLD));
    }
}
