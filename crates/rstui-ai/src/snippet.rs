//! [`Snippet`] — a single-line read-only command with a copy affordance: the
//! `npm i …` / `cargo add …` chip an agent emits for the user to run.
//!
//! # A pure projection; the copy is an intent, not a callback
//!
//! The ai-elements `Snippet` is a read-only input plus a copy button that
//! flips to a tick for a moment. rstui forbids callbacks and a wall clock in
//! `view` ([ADR 0012](https://github.com/andymac4182/rstui/blob/main/docs/composition.md)).
//! So `Snippet` owns nothing: it projects the caller's command `&str`, and
//! the "was it just copied" feedback is caller-owned state
//! ([`copied`](Snippet::copied), which the reducer sets on the copy and
//! clears on a later tick). The copy control surfaces as a pure hit-test
//! [`copy_rect`](Snippet::copy_rect) the host maps a click to, yielding the
//! reducer-consumed [`SnippetIntent::Copy`] — the same seam
//! [`Button`](rstui_widgets::Button) and the tool cards use, never an
//! `onClick`.
//!
//! # One row, framed like a chip
//!
//! It is one row inside an optional [`Block`]: a
//! `$ ` prompt, the command (clipped), then a right-aligned copy glyph
//! (`⧉`, or `✓` while [`copied`](Snippet::copied)).
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area
//! clips and [`copy_rect`](Snippet::copy_rect) returns `None` — never a
//! panic.

use rstui_core::{Buffer, Position, Rect, Style, Widget};
use rstui_widgets::Block;

/// The reducer-consumed intent a [`Snippet`] surfaces — the host maps a click
/// in [`copy_rect`](Snippet::copy_rect) to this and the reducer copies the
/// command to the clipboard and flips its `copied` state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetIntent {
    /// The copy affordance was activated — copy the command.
    Copy,
}

/// A single-line read-only command with a trailing copy affordance.
///
/// One row inside an optional framing [`Block`](Self::block): a `$ ` prompt,
/// the [`command`](Self::new) (clipped at the copy glyph), then a
/// right-aligned glyph — `⧉` normally, `✓` when [`copied`](Self::copied) (the
/// caller-owned "just copied" feedback the reducer owns). `Snippet` owns no
/// state — see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::snippet::{Snippet, SnippetIntent};
///
/// let snip = Snippet::new("npm i ai");
/// let area = Rect::new(0, 0, 16, 1);
/// // The copy glyph sits on the last column — the host hit-tests this.
/// assert_eq!(snip.copy_rect(area), Some(Rect::new(15, 0, 1, 1)));
///
/// let mut buf = Buffer::empty(area);
/// snip.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '$');
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'n');
/// assert_eq!(buf.get(Position::new(15, 0)).unwrap().symbol, '⧉');
/// ```
#[derive(Debug, Clone)]
pub struct Snippet<'a> {
    command: &'a str,
    block: Option<Block<'a>>,
    copied: bool,
    prompt: &'a str,
    style: Style,
    command_style: Style,
    copy_style: Style,
}

impl<'a> Snippet<'a> {
    /// A snippet displaying `command`, unframed, with the default `$ `
    /// prompt and not-yet-copied.
    #[must_use]
    pub fn new(command: &'a str) -> Self {
        Self {
            command,
            block: None,
            copied: false,
            prompt: "$ ",
            style: Style::new(),
            command_style: Style::new(),
            copy_style: Style::new(),
        }
    }

    /// Frames the snippet in `block`; the row is drawn in
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the caller-owned "just copied" feedback (the reducer sets it on a
    /// copy and clears it on a later tick); the copy glyph becomes `✓`.
    #[must_use]
    pub fn copied(mut self, copied: bool) -> Self {
        self.copied = copied;
        self
    }

    /// Sets the leading prompt string (default `"$ "`).
    #[must_use]
    pub fn prompt(mut self, prompt: &'a str) -> Self {
        self.prompt = prompt;
        self
    }

    /// Sets the base [`Style`], beneath the command and copy styles.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] the command text is drawn with (over the base).
    #[must_use]
    pub fn command_style(mut self, command_style: Style) -> Self {
        self.command_style = command_style;
        self
    }

    /// Sets the [`Style`] the trailing copy glyph is drawn with (over the
    /// base).
    #[must_use]
    pub fn copy_style(mut self, copy_style: Style) -> Self {
        self.copy_style = copy_style;
        self
    }

    /// The content row, inside [`block`](Self::block) if present.
    fn inner(&self, area: Rect) -> Rect {
        match &self.block {
            Some(b) => b.inner(area),
            None => area,
        }
    }

    /// The 1×1 [`Rect`] of the copy affordance (the last inner column), or
    /// `None` if the area is too small to hold it. The host hit-tests a
    /// click against this and yields [`SnippetIntent::Copy`].
    #[must_use]
    pub fn copy_rect(&self, area: Rect) -> Option<Rect> {
        let inner = self.inner(area);
        if inner.is_empty() {
            return None;
        }
        Some(Rect::new(
            inner.right().saturating_sub(1),
            inner.top(),
            1,
            1,
        ))
    }
}

impl Widget for Snippet<'_> {
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

        let y = inner.top();
        let base = self.style;
        buf.set_style(Rect::new(inner.left(), y, inner.width, 1), base);

        // The copy glyph occupies the last column; the command stops before
        // it (with a one-column gap).
        let glyph = if self.copied { '✓' } else { '⧉' };
        let glyph_x = inner.right().saturating_sub(1);
        buf.set_cell(
            Position::new(glyph_x, y),
            glyph,
            base.patch(self.copy_style),
        );
        let command_right = glyph_x.saturating_sub(1);

        let mut x = inner.left();
        let cmd_style = base.patch(self.command_style);
        for ch in self.prompt.chars() {
            if x >= command_right {
                return;
            }
            buf.set_cell(Position::new(x, y), ch, base);
            x = x.saturating_add(1);
        }
        for ch in self.command.chars() {
            if x >= command_right {
                break;
            }
            buf.set_cell(Position::new(x, y), ch, cmd_style);
            x = x.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier};

    fn row(widget: Snippet<'_>, width: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
        widget.render(buf.area(), &mut buf);
        (0..width)
            .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
            .collect()
    }

    #[test]
    fn a_snippet_is_prompt_command_then_a_copy_glyph() {
        assert_eq!(row(Snippet::new("ls -a"), 12), "$ ls -a    ⧉");
    }

    #[test]
    fn copied_swaps_the_glyph_to_a_tick() {
        assert_eq!(row(Snippet::new("ok").copied(true), 8), "$ ok   ✓");
    }

    #[test]
    fn copy_rect_is_the_last_inner_column() {
        let area = Rect::new(0, 0, 10, 1);
        assert_eq!(
            Snippet::new("x").copy_rect(area),
            Some(Rect::new(9, 0, 1, 1))
        );
        // Framed: inside the block's inner area (needs ≥2 rows for a border).
        let framed = Snippet::new("x").block(Block::bordered());
        assert_eq!(
            framed.copy_rect(Rect::new(0, 0, 10, 3)),
            Some(Rect::new(8, 1, 1, 1))
        );
        // A 1-row area can't hold a bordered inner → None (totality).
        assert_eq!(framed.copy_rect(area), None);
    }

    #[test]
    fn a_long_command_clips_before_the_copy_glyph() {
        // The glyph always survives on the last column.
        assert_eq!(row(Snippet::new("aaaaaaaaaaaa"), 8), "$ aaaa ⧉");
    }

    #[test]
    fn a_block_frames_the_row() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 9, 3));
        Snippet::new("hi")
            .block(Block::bordered())
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, '$');
        assert_eq!(buf.get(Position::new(7, 1)).unwrap().symbol, '⧉');
    }

    #[test]
    fn styles_cascade_command_over_base() {
        let widget = Snippet::new("c")
            .style(Style::new().add_modifier(Modifier::BOLD))
            .command_style(Style::new().fg(Color::Green));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        widget.render(buf.area(), &mut buf);
        let c = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(c.symbol, 'c');
        assert_eq!(c.fg, Color::Green);
        assert!(c.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn tiny_and_zero_areas_are_safe() {
        assert_eq!(Snippet::new("x").copy_rect(Rect::new(0, 0, 0, 0)), None);
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Snippet::new("x").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
