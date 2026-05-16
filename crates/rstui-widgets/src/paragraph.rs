//! [`Paragraph`] — the multi-line text widget for logs, help text,
//! descriptions, and any scrollable read-only copy.
//!
//! [`Text`](rstui_core::Text) / [`Line`](rstui_core::Line) /
//! [`Span`](rstui_core::Span) render exactly as written and clip at the right
//! edge — that is the whole core text model. `Paragraph` is the widget that
//! adds what real content panes need on top of it: soft word [`Wrap`], a
//! vertical/horizontal scroll offset, per-block alignment, and an optional
//! framing [`Block`] — none of which leaks back into the text primitives.

use crate::block::Block;
use rstui_core::{Alignment, Buffer, Position, Rect, Style, Text, Widget};

/// How [`Paragraph`] reflows lines too wide for its area.
///
/// `trim` controls leading whitespace on every wrapped row: `true` reflows
/// `"  - a long bullet …"` flush-left on each row, `false` keeps the original
/// indentation so continuations line up under the text rather than the bullet.
/// Trailing whitespace at a wrap point is always dropped so alignment stays
/// exact regardless of `trim`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Wrap {
    /// Strip leading whitespace from every wrapped row.
    pub trim: bool,
}

/// A multi-line text widget with optional word [`Wrap`], scroll, alignment,
/// and a framing [`Block`].
///
/// [`Text`](rstui_core::Text) / [`Line`](rstui_core::Line) /
/// [`Span`](rstui_core::Span) render exactly as written and clip at the right
/// edge — that is the whole text model. `Paragraph` is the widget that adds
/// what real content panes need on top of it: soft word wrapping to the
/// available width, a vertical/horizontal scroll offset, per-block alignment,
/// and an optional surrounding [`Block`]. It is the basis for logs, help text,
/// descriptions, and any scrollable read-only copy.
///
/// Styling cascades paragraph → text → line → span (the same
/// [`Style::patch`](rstui_core::Style) model [`Text`](rstui_core::Text) uses);
/// the paragraph style also fills the content area so a background covers the
/// whole region.
///
/// Without [`wrap`](Self::wrap) each [`Line`](rstui_core::Line) is one row
/// (offset by the horizontal scroll, clipped at the right edge). With it,
/// lines too wide for the area reflow at word boundaries and a single word
/// wider than the area is hard split across rows. A blank source line always
/// occupies a row so vertical spacing is preserved.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Paragraph, Wrap};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
/// Paragraph::new("the quick brown")
///     .wrap(Wrap { trim: true })
///     .render(buf.area(), &mut buf);
///
/// // Soft-wrapped at word boundaries to the 6-cell width.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 't'); // "the"
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'q'); // "quick"
/// assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, 'b'); // "brown"
/// ```
#[derive(Debug, Default, Clone)]
pub struct Paragraph<'a> {
    text: Text<'a>,
    block: Option<Block<'a>>,
    style: Style,
    wrap: Option<Wrap>,
    scroll: Position,
    alignment: Option<Alignment>,
}

impl<'a> Paragraph<'a> {
    /// A left-aligned, unwrapped paragraph of `text` with no block.
    pub fn new(text: impl Into<Text<'a>>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// Frames the paragraph in `block`; content renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`], beneath the text→line→span cascade. It also
    /// fills the content area so a background covers the whole region.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Enables soft word wrapping with the given [`Wrap`] options.
    #[must_use]
    pub fn wrap(mut self, wrap: Wrap) -> Self {
        self.wrap = Some(wrap);
        self
    }

    /// Sets the scroll offset into the composed text: `x` columns from the
    /// left (only meaningful without [`wrap`](Self::wrap)) and `y` rows from
    /// the top. Accepts a [`Position`] or an `(x, y)` tuple.
    #[must_use]
    pub fn scroll(mut self, offset: impl Into<Position>) -> Self {
        self.scroll = offset.into();
        self
    }

    /// Sets the default alignment for lines that do not set their own.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Left-aligns lines without their own alignment.
    #[must_use]
    pub fn left_aligned(self) -> Self {
        self.alignment(Alignment::Left)
    }

    /// Centers lines without their own alignment.
    #[must_use]
    pub fn centered(self) -> Self {
        self.alignment(Alignment::Center)
    }

    /// Right-aligns lines without their own alignment.
    #[must_use]
    pub fn right_aligned(self) -> Self {
        self.alignment(Alignment::Right)
    }
}

/// One composed output row: its resolved glyph cells and alignment.
struct ParaRow {
    cells: Vec<(char, Style)>,
    align: Alignment,
}

/// Trims trailing whitespace off `cur`, then pushes it as a finished row.
///
/// Trailing-whitespace trimming keeps the row's reported width equal to its
/// visible width so right/center alignment positions exactly.
fn flush_row(cur: &mut Vec<(char, Style)>, align: Alignment, out: &mut Vec<ParaRow>) {
    while matches!(cur.last(), Some((c, _)) if c.is_whitespace()) {
        cur.pop();
    }
    out.push(ParaRow {
        cells: std::mem::take(cur),
        align,
    });
}

/// Soft-wraps one source line's `cells` to `width`, appending the rows.
///
/// Word boundaries are maximal runs of (non-)whitespace `char`s. A row is
/// flushed before a word that would overflow; `trim` additionally drops
/// leading whitespace at the start of every row; a single word wider than
/// `width` is hard split across rows. Empty input still yields one row so a
/// blank source line occupies a row.
fn wrap_cells(
    cells: &[(char, Style)],
    width: usize,
    trim: bool,
    align: Alignment,
    out: &mut Vec<ParaRow>,
) {
    if width == 0 {
        out.push(ParaRow {
            cells: Vec::new(),
            align,
        });
        return;
    }
    let mut cur: Vec<(char, Style)> = Vec::new();
    let n = cells.len();
    let mut i = 0;
    while i < n {
        let ws = cells[i].0.is_whitespace();
        let mut j = i;
        while j < n && cells[j].0.is_whitespace() == ws {
            j += 1;
        }
        let token = &cells[i..j];
        i = j;
        if ws {
            if trim && cur.is_empty() {
                // Drop leading whitespace at the start of a row.
            } else if cur.len() + token.len() <= width {
                cur.extend_from_slice(token);
            } else {
                // Whitespace would overflow: end the row, drop the spaces.
                flush_row(&mut cur, align, out);
            }
        } else if token.len() <= width {
            if cur.len() + token.len() > width {
                flush_row(&mut cur, align, out);
            }
            cur.extend_from_slice(token);
        } else {
            // A single word wider than the whole row: hard split it.
            let mut k = 0;
            while k < token.len() {
                if cur.len() == width {
                    flush_row(&mut cur, align, out);
                }
                let take = (width - cur.len()).min(token.len() - k);
                cur.extend_from_slice(&token[k..k + take]);
                k += take;
            }
        }
    }
    flush_row(&mut cur, align, out);
}

impl Widget for Paragraph<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Paragraph {
            text,
            block,
            style,
            wrap,
            scroll,
            alignment,
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

        // Paragraph base fills the content area so a background covers the
        // whole region; glyphs then layer the text→line→span cascade on top.
        buf.set_style(inner, style);

        let width = inner.width as usize;
        let text_base = style.patch(text.style);
        let para_align = text.alignment.or(alignment);

        // Compose every source line into output rows: one per source line, or
        // several when wrapping. Each row carries its source line's resolved
        // alignment so a wrapped continuation stays aligned with it.
        let mut rows: Vec<ParaRow> = Vec::new();
        for line in text.lines {
            let align = line.alignment.or(para_align).unwrap_or_default();
            let line_base = text_base.patch(line.style);
            let mut cells: Vec<(char, Style)> = Vec::new();
            for span in &line.spans {
                let span_style = line_base.patch(span.style);
                cells.extend(span.content.chars().map(|ch| (ch, span_style)));
            }
            match wrap {
                Some(w) => wrap_cells(&cells, width, w.trim, align, &mut rows),
                None => rows.push(ParaRow { cells, align }),
            }
        }

        // Vertical scroll, then paint the visible window of rows.
        let top = inner.top();
        let right = inner.right();
        for (i, row) in rows
            .into_iter()
            .skip(scroll.y as usize)
            .take(inner.height as usize)
            .enumerate()
        {
            // Horizontal scroll only applies without wrapping — wrapped text
            // has no off-screen horizontal extent to scroll into.
            let visible: &[(char, Style)] = if wrap.is_none() {
                let off = (scroll.x as usize).min(row.cells.len());
                &row.cells[off..]
            } else {
                &row.cells
            };
            let drawn = visible.len().min(width) as u16;
            let start = match row.align {
                Alignment::Left => inner.left(),
                Alignment::Right => right.saturating_sub(drawn),
                Alignment::Center => inner
                    .left()
                    .saturating_add((inner.width.saturating_sub(drawn)) / 2),
            };
            let y = top.saturating_add(i as u16);
            let mut x = start;
            for &(ch, st) in visible {
                if x >= right {
                    break;
                }
                buf.set_cell(Position::new(x, y), ch, st);
                x = x.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Line, Modifier, Span};

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
    fn paragraph_without_wrap_is_one_clipped_row_per_source_line() {
        // No wrap: each `\n`-split line is exactly one row, clipped at the
        // right edge — the same rule the bare text model already follows.
        assert_eq!(
            lines(Paragraph::new("abcdef\nXY"), 4, 3),
            "abcd\nXY  \n    \n"
        );
    }

    #[test]
    fn wrap_breaks_at_word_boundaries() {
        assert_eq!(
            lines(
                Paragraph::new("the quick brown").wrap(Wrap { trim: true }),
                6,
                3
            ),
            "the   \nquick \nbrown \n"
        );
    }

    #[test]
    fn wrap_trim_controls_leading_whitespace_only() {
        // trim:false keeps the original indentation on the first row…
        assert_eq!(
            lines(Paragraph::new("  ab cd").wrap(Wrap { trim: false }), 4, 2),
            "  ab\ncd  \n"
        );
        // …trim:true strips it. Both wrap "cd" identically (no leading space
        // on a continuation row either way).
        assert_eq!(
            lines(Paragraph::new("  ab cd").wrap(Wrap { trim: true }), 4, 2),
            "ab  \ncd  \n"
        );
    }

    #[test]
    fn wrap_hard_splits_a_word_wider_than_the_area() {
        assert_eq!(
            lines(Paragraph::new("abcdefg").wrap(Wrap { trim: false }), 3, 3),
            "abc\ndef\ng  \n"
        );
    }

    #[test]
    fn the_space_at_a_wrap_point_is_dropped() {
        // "aaa bbb" at width 3 yields two full rows, not "aaa " (width 4) or a
        // leading-space "bbb" — trailing-whitespace trim keeps widths exact.
        assert_eq!(
            lines(Paragraph::new("aaa bbb").wrap(Wrap { trim: false }), 3, 2),
            "aaa\nbbb\n"
        );
    }

    #[test]
    fn a_blank_source_line_still_occupies_a_row() {
        assert_eq!(lines(Paragraph::new("a\n\nb"), 3, 3), "a  \n   \nb  \n");
    }

    #[test]
    fn vertical_scroll_skips_composed_rows() {
        let p = Paragraph::new("l0\nl1\nl2\nl3").scroll((0, 2));
        assert_eq!(lines(p, 2, 2), "l2\nl3\n");
    }

    #[test]
    fn horizontal_scroll_skips_columns_without_wrap() {
        // x scroll drops leading columns of each row (no-wrap only).
        assert_eq!(
            lines(Paragraph::new("abcdef").scroll((2, 0)), 3, 1),
            "cde\n"
        );
    }

    #[test]
    fn block_frames_content_in_the_inner_area() {
        assert_eq!(
            lines(Paragraph::new("hi").block(Block::bordered()), 4, 3),
            "┌──┐\n│hi│\n└──┘\n"
        );
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_content() {
        // inner() collapses to empty; the block still renders, content does
        // not (and nothing panics).
        assert_eq!(
            lines(Paragraph::new("Z").block(Block::bordered()), 2, 2),
            "┌┐\n└┘\n"
        );
    }

    #[test]
    fn alignment_positions_each_row() {
        assert_eq!(lines(Paragraph::new("hi").right_aligned(), 5, 1), "   hi\n");
        assert_eq!(lines(Paragraph::new("hi").centered(), 5, 1), " hi  \n");
    }

    #[test]
    fn style_cascades_paragraph_text_line_span_and_fills_the_area() {
        let p = Paragraph::new(
            Text::from(vec![
                Line::from(vec![
                    Span::styled("X", Style::new().fg(Color::Red)),
                    Span::raw("y"),
                ])
                .style(Style::new().add_modifier(Modifier::BOLD)),
            ])
            .style(Style::new().fg(Color::Green)),
        )
        .style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        p.render(buf.area(), &mut buf);

        // Span fg wins; line BOLD + paragraph bg cascade through.
        let x = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(x.symbol, 'X');
        assert_eq!(x.fg, Color::Red);
        assert_eq!(x.bg, Color::Blue);
        assert!(x.modifier.contains(Modifier::BOLD));

        // The raw span inherits the text-level green (no span fg of its own).
        let y = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(y.symbol, 'y');
        assert_eq!(y.fg, Color::Green);
        assert!(y.modifier.contains(Modifier::BOLD));

        // The paragraph style also fills the empty cell past the text.
        let pad = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(pad.symbol, ' ');
        assert_eq!(pad.bg, Color::Blue);
        assert_eq!(pad.fg, Color::Reset);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Paragraph::new("hello")
            .wrap(Wrap { trim: true })
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
