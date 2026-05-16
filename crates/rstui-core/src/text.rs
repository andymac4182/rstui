//! Styled text primitives: [`Span`], [`Line`], and [`Text`].
//!
//! Up to now the only way to put text on screen was an unstyled `&str`. Real
//! UIs need *runs* of differently-styled text on a line (a green ` ✓ ` next to
//! a default label), lines with their own alignment, and multi-line blocks.
//! These three types are that model, and nearly every richer component
//! (paragraphs, list rows, table cells, tab labels, styled block titles)
//! is built by composing them:
//!
//! - [`Span`] — one run of text sharing a single [`Style`]. The atom.
//! - [`Line`] — a horizontal sequence of [`Span`]s plus an optional
//!   [`Alignment`], i.e. one visual row.
//! - [`Text`] — a vertical sequence of [`Line`]s, the whole block.
//!
//! # One model, on purpose
//!
//! The most adopted TUI library today ships *two* parallel styled-text models
//! (an immutable chunk list and a separately-styled retained node tree) and
//! its own usage shows the resulting "which one wins?" reconciliation is a
//! recurring pain point. rstui deliberately commits to a single flat,
//! data-driven model — the one ratatui has proven — so there is exactly one
//! way to describe styled text and one set of conversions to learn.
//!
//! # Style cascade
//!
//! Styles inherit predictably and explicitly: a cell's final look is
//! `text.style` patched by its `line.style` patched by its `span.style`. An
//! unset color or modifier falls through to the enclosing level; a set one
//! overrides it. This is the same [`Style::patch`] model themes and selection
//! highlights already rely on, so a `Text` styled green with one
//! `Span::styled(_, red)` inside it renders red there and green everywhere
//! else with no special-casing.
//!
//! # Deliberate scope
//!
//! Content is a [`Cow<str>`](std::borrow::Cow): a literal borrows (no
//! allocation in the per-frame render path), an owned `String` is taken as-is.
//! This is `std` only, so the core stays dependency-free.
//!
//! Width is a [`char`] count, *not* a Unicode display width — the same
//! single-`char`-cell simplification [`Buffer`](crate::Buffer) already makes;
//! grapheme clustering and double-width handling remain one deferred renderer
//! concern rather than leaking into the text model. Word wrapping, scrolling,
//! and trimming belong to a future `Paragraph` widget, not these primitives: a
//! [`Line`] is exactly one row and clips at the area's right edge.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Color, Line, Position, Rect, Span, Style, Widget};
//!
//! let mut buf = Buffer::empty(Rect::new(0, 0, 7, 1));
//! Line::from(vec![
//!     Span::styled("ok", Style::new().fg(Color::Green)),
//!     Span::raw(" done"),
//! ])
//! .render(buf.area(), &mut buf);
//!
//! assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'o');
//! assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Green);
//! assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, 'd');
//! assert_eq!(buf.get(Position::new(3, 0)).unwrap().fg, Color::Reset);
//! ```

use std::borrow::Cow;

use crate::buffer::Buffer;
use crate::geometry::{Position, Rect};
use crate::layout::Alignment;
use crate::style::Style;
use crate::widget::Widget;

/// A single run of text drawn with one [`Style`].
///
/// The atom of the text model. Build one with [`Span::raw`] (default style) or
/// [`Span::styled`], or convert from any string with [`From`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Span<'a> {
    /// How this run is styled. Patched over the enclosing line/text style.
    pub style: Style,
    /// The text of this run. A literal borrows; a `String` is owned.
    pub content: Cow<'a, str>,
}

impl<'a> Span<'a> {
    /// A span of `content` with the default (inherit-everything) [`Style`].
    pub fn raw(content: impl Into<Cow<'a, str>>) -> Self {
        Self {
            style: Style::new(),
            content: content.into(),
        }
    }

    /// A span of `content` drawn with `style`.
    pub fn styled(content: impl Into<Cow<'a, str>>, style: Style) -> Self {
        Self {
            style,
            content: content.into(),
        }
    }

    /// Replaces this span's style.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Overlays `style` on top of this span's current style.
    #[must_use]
    pub fn patch_style(mut self, style: Style) -> Self {
        self.style = self.style.patch(style);
        self
    }

    /// The number of [`char`]s in this span (its column count in rstui's
    /// single-`char`-cell model).
    #[must_use]
    pub fn width(&self) -> usize {
        self.content.chars().count()
    }
}

impl<'a> From<&'a str> for Span<'a> {
    fn from(s: &'a str) -> Self {
        Self::raw(s)
    }
}

impl From<String> for Span<'_> {
    fn from(s: String) -> Self {
        Self::raw(s)
    }
}

impl<'a> From<Cow<'a, str>> for Span<'a> {
    fn from(s: Cow<'a, str>) -> Self {
        Self::raw(s)
    }
}

impl Widget for Span<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Line::from(self).render(area, buf);
    }
}

/// One visual row: a sequence of [`Span`]s with an optional [`Alignment`].
///
/// A `Line` is always a single row; embedded newlines are the caller's
/// concern — use [`Text`] for multi-line content. Content wider than the area
/// is clipped at the right edge (alignment positions only what fits).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Line<'a> {
    /// Base style for the whole row, under each span's own style.
    pub style: Style,
    /// Horizontal placement; `None` inherits from the enclosing [`Text`]
    /// (defaulting to [`Alignment::Left`]).
    pub alignment: Option<Alignment>,
    /// The runs that make up this row, left to right.
    pub spans: Vec<Span<'a>>,
}

impl<'a> Line<'a> {
    /// A single-span line of `content` with the default style.
    pub fn raw(content: impl Into<Cow<'a, str>>) -> Self {
        Self::from(Span::raw(content))
    }

    /// A single-span line of `content` drawn with `style` as the line style.
    pub fn styled(content: impl Into<Cow<'a, str>>, style: Style) -> Self {
        Self::from(Span::raw(content)).style(style)
    }

    /// Replaces the spans with those produced by `spans`.
    #[must_use]
    pub fn spans<I, S>(mut self, spans: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Span<'a>>,
    {
        self.spans = spans.into_iter().map(Into::into).collect();
        self
    }

    /// Appends one span to the end of the row.
    pub fn push_span(&mut self, span: impl Into<Span<'a>>) {
        self.spans.push(span.into());
    }

    /// Replaces the line's base style.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the horizontal alignment of this row.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Left-aligns this row.
    #[must_use]
    pub fn left_aligned(self) -> Self {
        self.alignment(Alignment::Left)
    }

    /// Centers this row.
    #[must_use]
    pub fn centered(self) -> Self {
        self.alignment(Alignment::Center)
    }

    /// Right-aligns this row.
    #[must_use]
    pub fn right_aligned(self) -> Self {
        self.alignment(Alignment::Right)
    }

    /// The row's total width: the sum of its spans' [`char`] counts.
    #[must_use]
    pub fn width(&self) -> usize {
        self.spans.iter().map(Span::width).sum()
    }
}

impl<'a> From<&'a str> for Line<'a> {
    fn from(s: &'a str) -> Self {
        Self::from(Span::raw(s))
    }
}

impl From<String> for Line<'_> {
    fn from(s: String) -> Self {
        Self::from(Span::raw(s))
    }
}

impl<'a> From<Cow<'a, str>> for Line<'a> {
    fn from(s: Cow<'a, str>) -> Self {
        Self::from(Span::raw(s))
    }
}

impl<'a> From<Span<'a>> for Line<'a> {
    fn from(span: Span<'a>) -> Self {
        Self {
            spans: vec![span],
            ..Self::default()
        }
    }
}

impl<'a> From<Vec<Span<'a>>> for Line<'a> {
    fn from(spans: Vec<Span<'a>>) -> Self {
        Self {
            spans,
            ..Self::default()
        }
    }
}

impl Widget for Line<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let row = area.top();
        // Fill just this row with the line style so a background/selection
        // highlight covers the full width, including alignment padding.
        buf.set_style(Rect::new(area.x, row, area.width, 1), self.style);

        let avail = area.width as usize;
        let drawn = self.width().min(avail) as u16;
        let start = match self.alignment.unwrap_or_default() {
            Alignment::Left => area.left(),
            Alignment::Right => area.right().saturating_sub(drawn),
            Alignment::Center => area
                .left()
                .saturating_add((area.width.saturating_sub(drawn)) / 2),
        };

        let right = area.right();
        let mut x = start;
        for span in self.spans {
            let style = self.style.patch(span.style);
            for ch in span.content.chars() {
                if x >= right {
                    return;
                }
                buf.set_cell(Position::new(x, row), ch, style);
                x = x.saturating_add(1);
            }
        }
    }
}

/// A block of text: a vertical sequence of [`Line`]s.
///
/// `Text::raw`/`From<&str>` split on `\n` into lines. The text-level style and
/// alignment are defaults each line inherits unless it sets its own.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Text<'a> {
    /// Base style for the whole block, under each line's own style.
    pub style: Style,
    /// Default alignment for lines that do not set their own.
    pub alignment: Option<Alignment>,
    /// The rows, top to bottom.
    pub lines: Vec<Line<'a>>,
}

impl<'a> Text<'a> {
    /// Text from `content`, split into one [`Line`] per `\n`-separated row,
    /// with the default style.
    pub fn raw(content: impl Into<Cow<'a, str>>) -> Self {
        Self::from(content.into())
    }

    /// Like [`Text::raw`] but with `style` as the block's base style.
    pub fn styled(content: impl Into<Cow<'a, str>>, style: Style) -> Self {
        Self::raw(content).style(style)
    }

    /// Replaces the lines with those produced by `lines`.
    #[must_use]
    pub fn lines<I, L>(mut self, lines: I) -> Self
    where
        I: IntoIterator<Item = L>,
        L: Into<Line<'a>>,
    {
        self.lines = lines.into_iter().map(Into::into).collect();
        self
    }

    /// Appends one line to the bottom of the block.
    pub fn push_line(&mut self, line: impl Into<Line<'a>>) {
        self.lines.push(line.into());
    }

    /// Replaces the block's base style.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the default alignment for lines without their own.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Left-aligns lines that do not set their own alignment.
    #[must_use]
    pub fn left_aligned(self) -> Self {
        self.alignment(Alignment::Left)
    }

    /// Centers lines that do not set their own alignment.
    #[must_use]
    pub fn centered(self) -> Self {
        self.alignment(Alignment::Center)
    }

    /// Right-aligns lines that do not set their own alignment.
    #[must_use]
    pub fn right_aligned(self) -> Self {
        self.alignment(Alignment::Right)
    }

    /// The width of the widest line.
    #[must_use]
    pub fn width(&self) -> usize {
        self.lines.iter().map(Line::width).max().unwrap_or(0)
    }

    /// The number of lines (its row count).
    #[must_use]
    pub fn height(&self) -> usize {
        self.lines.len()
    }
}

impl<'a> From<&'a str> for Text<'a> {
    fn from(s: &'a str) -> Self {
        Self {
            lines: s.split('\n').map(Line::from).collect(),
            ..Self::default()
        }
    }
}

impl From<String> for Text<'_> {
    fn from(s: String) -> Self {
        Self {
            lines: s.split('\n').map(|l| Line::from(l.to_owned())).collect(),
            ..Self::default()
        }
    }
}

impl<'a> From<Cow<'a, str>> for Text<'a> {
    fn from(s: Cow<'a, str>) -> Self {
        // Borrowed input keeps each line borrowed (zero-alloc); owned input
        // must own each line since the split slices borrow a local `String`.
        match s {
            Cow::Borrowed(s) => Self::from(s),
            Cow::Owned(s) => Self::from(s),
        }
    }
}

impl<'a> From<Span<'a>> for Text<'a> {
    fn from(span: Span<'a>) -> Self {
        Self::from(Line::from(span))
    }
}

impl<'a> From<Line<'a>> for Text<'a> {
    fn from(line: Line<'a>) -> Self {
        Self {
            lines: vec![line],
            ..Self::default()
        }
    }
}

impl<'a> From<Vec<Line<'a>>> for Text<'a> {
    fn from(lines: Vec<Line<'a>>) -> Self {
        Self {
            lines,
            ..Self::default()
        }
    }
}

impl Widget for Text<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        for (i, mut line) in self.lines.into_iter().enumerate() {
            if i >= area.height as usize {
                break;
            }
            // The line inherits the block's style and, if it set none of its
            // own, the block's alignment.
            line.style = self.style.patch(line.style);
            if line.alignment.is_none() {
                line.alignment = self.alignment;
            }
            let row = Rect::new(area.x, area.y.saturating_add(i as u16), area.width, 1);
            line.render(row, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Position;
    use crate::style::{Color, Modifier};

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
    fn span_width_counts_chars_not_bytes() {
        // 'é' is two UTF-8 bytes but one cell in the single-`char` model.
        assert_eq!(Span::raw("café").width(), 4);
        assert_eq!(Span::raw("").width(), 0);
    }

    #[test]
    fn line_width_is_the_sum_of_its_spans() {
        let line = Line::from(vec![Span::raw("ab"), Span::raw("cde")]);
        assert_eq!(line.width(), 5);
    }

    #[test]
    fn text_width_is_the_widest_line_and_height_is_the_line_count() {
        let text = Text::raw("a\nbbb\ncc");
        assert_eq!(text.height(), 3);
        assert_eq!(text.width(), 3);
        assert_eq!(Text::default().width(), 0);
    }

    #[test]
    fn from_str_splits_text_into_lines_but_not_a_single_line() {
        assert_eq!(Text::from("x\ny\nz").lines.len(), 3);
        // A Line is one row: a newline is just content, never a split.
        let line = Line::from("x\ny");
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "x\ny");
    }

    #[test]
    fn line_renders_spans_left_to_right_and_clips_at_the_right_edge() {
        let line = Line::from(vec![Span::raw("ab"), Span::raw("cdef")]);
        // Area is only 4 wide: "abcd", "ef" is clipped.
        assert_eq!(lines(line, 4, 1), "abcd\n");
    }

    #[test]
    fn line_alignment_positions_content_within_the_area() {
        assert_eq!(lines(Line::raw("hi"), 6, 1), "hi    \n");
        assert_eq!(lines(Line::raw("hi").right_aligned(), 6, 1), "    hi\n");
        assert_eq!(lines(Line::raw("hi").centered(), 6, 1), "  hi  \n");
        // Odd remainder biases toward the start (matches Block title/Alignment).
        assert_eq!(lines(Line::raw("hi").centered(), 7, 1), "  hi   \n");
    }

    #[test]
    fn each_span_keeps_its_own_style_over_the_line_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        Line::from(vec![
            Span::styled("ok", Style::new().fg(Color::Green)),
            Span::raw("!"),
        ])
        .style(Style::new().bg(Color::Blue))
        .render(buf.area(), &mut buf);

        let o = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(o.symbol, 'o');
        assert_eq!(o.fg, Color::Green); // span wins for fg
        assert_eq!(o.bg, Color::Blue); // line style inherited for bg

        let bang = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(bang.fg, Color::Reset); // span has no fg → not green
        assert_eq!(bang.bg, Color::Blue); // still gets the line bg

        // Alignment padding still gets the line background fill.
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().bg, Color::Blue);
    }

    #[test]
    fn text_stacks_lines_and_clips_to_the_area_height() {
        let text = Text::raw("one\ntwo\nthree");
        // Only two rows tall: the third line is dropped, not panicked on.
        assert_eq!(lines(text, 5, 2), "one  \ntwo  \n");
    }

    #[test]
    fn text_style_and_alignment_cascade_into_lines_unless_overridden() {
        let text = Text::default()
            .style(Style::new().fg(Color::Red))
            .right_aligned()
            .lines(vec![
                Line::raw("a"),
                // This line overrides alignment and color locally.
                Line::raw("b")
                    .left_aligned()
                    .style(Style::new().fg(Color::Cyan)),
            ]);
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        text.render(buf.area(), &mut buf);

        // Row 0 inherits: right-aligned, red.
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'a');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().fg, Color::Red);
        // Row 1 overrides: left-aligned, cyan.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'b');
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().fg, Color::Cyan);
    }

    #[test]
    fn conversions_cover_the_common_constructors() {
        let _: Span = "lit".into();
        let _: Span = String::from("owned").into();
        let _: Line = "lit".into();
        let _: Line = Span::raw("s").into();
        let _: Line = vec![Span::raw("a"), Span::raw("b")].into();
        let _: Text = "a\nb".into();
        let _: Text = Line::raw("x").into();
        let _: Text = vec![Line::raw("p"), Line::raw("q")].into();

        let mut t = Text::default();
        t.push_line("first");
        t.push_line(Line::raw("second"));
        assert_eq!(t.height(), 2);

        let mut l = Line::default();
        l.push_span("x");
        l.push_span(Span::styled("y", Style::new().add_modifier(Modifier::BOLD)));
        assert_eq!(l.width(), 2);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Text::raw("hello").render(Rect::new(0, 0, 0, 0), &mut buf);
        Line::raw("hello").render(Rect::new(0, 0, 4, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
