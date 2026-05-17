//! [`TerminalView`] — a command-output viewer with a streaming cursor and
//! autoscroll: the scrollback panel a shell/exec tool streams its stdout
//! into.
//!
//! # A pure projection of `&str` + a caller-owned `ScrollState`
//!
//! The ai-elements `Terminal` renders streamed command output, sticking to
//! the bottom while it grows and showing a cursor while live. Scroll position
//! is ordinary application state — the documented
//! [`ScrollState`](rstui_core::ScrollState) seam (the reducer calls
//! [`on_content_change`](rstui_core::ScrollState::on_content_change) for
//! sticky-bottom-while-streaming). So `TerminalView` owns nothing: it
//! projects the caller's output `&str`, a caller-owned
//! [`scroll`](TerminalView::scroll) offset, and a
//! [`streaming`](TerminalView::streaming) flag (a trailing block cursor on
//! the last line while set).
//!
//! Lines are rendered **verbatim** — this is deliberately *not* an ANSI
//! parser (the brief's scope line): an escape sequence is just text here; a
//! real ANSI layer is an additive follow-up over this shape, not smuggled in.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area
//! clips, an out-of-range offset clamps to the last page, and empty output
//! is a blank pane — never a panic.

use rstui_core::{Buffer, Modifier, Position, Rect, Style, Widget};
use rstui_widgets::Block;

/// The block cursor glyph drawn at the output tail while
/// [`streaming`](TerminalView::streaming).
const CURSOR: char = '▌';

/// A command-output viewer with a streaming cursor and autoscroll.
///
/// Projects the caller's `output`, a caller-owned [`scroll`](Self::scroll)
/// row offset, and a [`streaming`](Self::streaming) flag. Inside an optional
/// [`block`](Self::block) it shows the line window
/// `[scroll, scroll + height)` (the offset clamped so the last page is the
/// floor); while [`streaming`](Self::streaming) a `▌` cursor follows the
/// last line. Lines are drawn verbatim — see the [module docs](self).
/// `TerminalView` owns no state.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::terminal_view::TerminalView;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 2));
/// TerminalView::new("$ ls\nsrc lib")
///     .streaming(true)
///     .render(buf.area(), &mut buf);
///
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '$');
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 's'); // "src lib"
/// // The streaming cursor trails the last line.
/// assert_eq!(buf.get(Position::new(7, 1)).unwrap().symbol, '▌');
/// ```
#[derive(Debug, Clone)]
pub struct TerminalView<'a> {
    output: &'a str,
    block: Option<Block<'a>>,
    scroll: usize,
    streaming: bool,
    style: Style,
}

impl<'a> TerminalView<'a> {
    /// A viewer of `output`, unframed, scrolled to the top, not streaming.
    #[must_use]
    pub fn new(output: &'a str) -> Self {
        Self {
            output,
            block: None,
            scroll: 0,
            streaming: false,
            style: Style::new(),
        }
    }

    /// Frames the viewer in `block`; output is drawn in
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the caller-owned first visible row (the reducer owns it — e.g.
    /// via [`ScrollState`](rstui_core::ScrollState)); clamped so the last
    /// page is the floor.
    #[must_use]
    pub fn scroll(mut self, scroll: usize) -> Self {
        self.scroll = scroll;
        self
    }

    /// Sets the streaming flag; a `▌` block cursor trails the last output
    /// line while set.
    #[must_use]
    pub fn streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }

    /// Sets the [`Style`] the output is drawn with.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The total line count of the output (`1` for empty output, since an
    /// empty terminal still has a cursor row).
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.output.lines().count().max(1)
    }

    /// The content area, inside [`block`](Self::block) if present.
    fn inner(&self, area: Rect) -> Rect {
        match &self.block {
            Some(b) => b.inner(area),
            None => area,
        }
    }
}

impl Widget for TerminalView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let inner = self.inner(area);
        if let Some(b) = self.block.clone() {
            b.render(area, buf);
        }
        if inner.is_empty() {
            return;
        }
        buf.set_style(inner, self.style);

        let lines: Vec<&str> = if self.output.is_empty() {
            vec![""]
        } else {
            self.output.lines().collect()
        };
        let viewport = inner.height as usize;
        // Clamp the offset so the last page is the floor (autoscroll-friendly).
        let max_off = lines.len().saturating_sub(viewport);
        let off = self.scroll.min(max_off);
        let last_idx = lines.len().saturating_sub(1);

        for (row, line_idx) in (off..lines.len()).take(viewport).enumerate() {
            let y = inner.top().saturating_add(row as u16);
            let mut x = inner.left();
            for ch in lines[line_idx].chars() {
                if x >= inner.right() {
                    break;
                }
                buf.set_cell(Position::new(x, y), ch, self.style);
                x = x.saturating_add(1);
            }
            // The streaming cursor trails the final line.
            if self.streaming && line_idx == last_idx && x < inner.right() {
                buf.set_cell(
                    Position::new(x, y),
                    CURSOR,
                    self.style.add_modifier(Modifier::SLOW_BLINK),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(widget: TerminalView<'_>, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn output_renders_one_line_per_row_verbatim() {
        // An escape sequence is just text — not parsed.
        assert_eq!(
            lines(TerminalView::new("a\n\x1b[31mb"), 6, 2),
            "a     \n\u{1b}[31mb\n"
        );
    }

    #[test]
    fn the_streaming_cursor_trails_the_last_line() {
        assert_eq!(
            lines(TerminalView::new("hi").streaming(true), 5, 1),
            "hi▌  \n"
        );
        // Not streaming → no cursor.
        assert_eq!(lines(TerminalView::new("hi"), 5, 1), "hi   \n");
    }

    #[test]
    fn the_offset_clamps_to_the_last_page() {
        // 4 lines, height 2, offset 99 → clamps to show lines 2..4.
        assert_eq!(
            lines(TerminalView::new("l0\nl1\nl2\nl3").scroll(99), 3, 2),
            "l2 \nl3 \n"
        );
        // A valid offset scrolls normally.
        assert_eq!(
            lines(TerminalView::new("l0\nl1\nl2\nl3").scroll(1), 3, 2),
            "l1 \nl2 \n"
        );
    }

    #[test]
    fn line_count_is_at_least_one() {
        assert_eq!(TerminalView::new("").line_count(), 1);
        assert_eq!(TerminalView::new("a\nb\nc").line_count(), 3);
    }

    #[test]
    fn empty_output_streaming_is_just_a_cursor() {
        assert_eq!(lines(TerminalView::new("").streaming(true), 3, 1), "▌  \n");
    }

    #[test]
    fn a_block_frames_the_output() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        TerminalView::new("ok")
            .block(Block::bordered())
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'o');
    }

    #[test]
    fn long_lines_clip_at_the_right_edge() {
        assert_eq!(lines(TerminalView::new("overlong"), 4, 1), "over\n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        TerminalView::new("x").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
