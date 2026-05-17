//! [`WebConsole`] — a leveled console-log list: the `console.log` /
//! `console.warn` / `console.error` strip from the ai-elements `WebPreview`
//! console panel.
//!
//! # Just the portable console list, not an iframe
//!
//! The ai-elements `WebPreview` embeds a live iframe; its *portable* part —
//! the only part a TUI can render — is the console-log list
//! (`{ level, message }` rows, the level tinting the line). That is all
//! [`WebConsole`] is, deliberately (the brief's scope line): no preview pane.
//!
//! # A pure projection of `&[ConsoleLog]` + a caller-owned `ScrollState`
//!
//! Scroll position is the documented
//! [`ScrollState`](rstui_core::ScrollState) seam. So `WebConsole` owns
//! nothing: it projects the caller's `&[ConsoleLog]` and a caller-owned
//! [`offset`](WebConsole::offset) (the reducer owns it), showing the line
//! window `[offset, offset + height)`, each line tinted by its
//! [`ConsoleLevel`].
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area
//! clips, an out-of-range offset clamps, and an empty list is a blank pane —
//! never a panic.

use rstui_core::{Buffer, Color, Position, Rect, Style, Widget};
use rstui_widgets::Block;

/// The severity of a [`ConsoleLog`] line, selecting its accent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleLevel {
    /// A `console.log` line (the default) — the base style.
    #[default]
    Log,
    /// A `console.warn` line — the warning accent.
    Warn,
    /// A `console.error` line — the error accent.
    Error,
}

/// One console line: a [`ConsoleLevel`] and its message text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLog {
    /// The line's severity (drives the accent).
    pub level: ConsoleLevel,
    /// The logged message.
    pub message: String,
}

impl ConsoleLog {
    /// A line at `level` with `message`.
    pub fn new(level: ConsoleLevel, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
        }
    }
}

/// A leveled console-log list.
///
/// Projects the caller's `&[ConsoleLog]` and a caller-owned
/// [`offset`](Self::offset) (clamped so the last page is the floor), drawing
/// the window `[offset, offset + inner_height)` one line per row inside an
/// optional [`block`](Self::block). Each line is tinted by its level —
/// [`warn_style`](Self::warn_style)/[`error_style`](Self::error_style) over
/// the base. `WebConsole` owns no state — see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::web_console::{ConsoleLevel, ConsoleLog, WebConsole};
///
/// let logs = [
///     ConsoleLog::new(ConsoleLevel::Log, "ready"),
///     ConsoleLog::new(ConsoleLevel::Error, "boom"),
/// ];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 2));
/// WebConsole::new(&logs).render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'r'); // ready
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'b'); // boom
/// ```
#[derive(Debug, Clone)]
pub struct WebConsole<'a> {
    logs: &'a [ConsoleLog],
    block: Option<Block<'a>>,
    offset: usize,
    style: Style,
    warn_style: Style,
    error_style: Style,
}

impl<'a> WebConsole<'a> {
    /// A console over `logs`, unframed, scrolled to the top.
    #[must_use]
    pub fn new(logs: &'a [ConsoleLog]) -> Self {
        Self {
            logs,
            block: None,
            offset: 0,
            style: Style::new(),
            warn_style: Style::new().fg(Color::Yellow),
            error_style: Style::new().fg(Color::Red),
        }
    }

    /// Frames the console in `block`; lines are drawn in
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the caller-owned first visible row (the reducer owns it — e.g.
    /// via [`ScrollState`](rstui_core::ScrollState)); clamped to the last
    /// page.
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Sets the base [`Style`] (the [`ConsoleLevel::Log`] style).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the accent [`Style`] for [`ConsoleLevel::Warn`] lines.
    #[must_use]
    pub fn warn_style(mut self, warn_style: Style) -> Self {
        self.warn_style = warn_style;
        self
    }

    /// Sets the accent [`Style`] for [`ConsoleLevel::Error`] lines.
    #[must_use]
    pub fn error_style(mut self, error_style: Style) -> Self {
        self.error_style = error_style;
        self
    }

    /// The accent for `level`, patched over the base.
    fn accent(&self, level: ConsoleLevel) -> Style {
        match level {
            ConsoleLevel::Log => self.style,
            ConsoleLevel::Warn => self.style.patch(self.warn_style),
            ConsoleLevel::Error => self.style.patch(self.error_style),
        }
    }

    /// The content area, inside [`block`](Self::block) if present.
    fn inner(&self, area: Rect) -> Rect {
        match &self.block {
            Some(b) => b.inner(area),
            None => area,
        }
    }
}

impl Widget for WebConsole<'_> {
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

        let viewport = inner.height as usize;
        let max_off = self.logs.len().saturating_sub(viewport);
        let off = self.offset.min(max_off);

        for (row, log) in self.logs.iter().skip(off).take(viewport).enumerate() {
            let y = inner.top().saturating_add(row as u16);
            let style = self.accent(log.level);
            let mut x = inner.left();
            for ch in log.message.chars() {
                if x >= inner.right() {
                    break;
                }
                buf.set_cell(Position::new(x, y), ch, style);
                x = x.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logs() -> Vec<ConsoleLog> {
        vec![
            ConsoleLog::new(ConsoleLevel::Log, "hello"),
            ConsoleLog::new(ConsoleLevel::Warn, "careful"),
            ConsoleLog::new(ConsoleLevel::Error, "boom"),
        ]
    }

    fn lines(widget: WebConsole<'_>, w: u16, h: u16) -> String {
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
    fn it_lists_one_message_per_row() {
        let l = logs();
        assert_eq!(
            lines(WebConsole::new(&l), 7, 3),
            "hello  \ncareful\nboom   \n"
        );
    }

    #[test]
    fn each_level_tints_its_line() {
        let l = logs();
        let mut buf = Buffer::empty(Rect::new(0, 0, 7, 3));
        WebConsole::new(&l).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Reset); // log
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().fg, Color::Yellow); // warn
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().fg, Color::Red); // error
    }

    #[test]
    fn the_offset_clamps_to_the_last_page() {
        let l = logs();
        // 3 lines, height 2, offset 9 → last two lines.
        assert_eq!(
            lines(WebConsole::new(&l).offset(9), 7, 2),
            "careful\nboom   \n"
        );
    }

    #[test]
    fn an_empty_console_is_a_blank_pane() {
        let empty: [ConsoleLog; 0] = [];
        assert_eq!(lines(WebConsole::new(&empty), 4, 2), "    \n    \n");
    }

    #[test]
    fn a_block_frames_the_list() {
        let l = logs();
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        WebConsole::new(&l)
            .block(Block::bordered())
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'h');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let l = logs();
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        WebConsole::new(&l).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
