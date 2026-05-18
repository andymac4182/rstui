//! [`PromptInput`] — the chat composer, the rstui translation of the
//! ai-elements `PromptInput` / `PromptInputTextarea` /
//! `PromptInputSubmit` / `PromptInputAttachments` family
//! (`prompt-input.tsx`).
//!
//! # A pure projection of a caller-owned [`TextArea`] + [`ChatStatus`]
//!
//! ai-elements' composer is a `<form>` with a growing `<textarea>`, an
//! attachments strip, and a submit button whose icon follows the
//! `status`. In rstui the multi-line input model is
//! [`rstui_core::TextArea`] — a caller-owned value the **reducer** edits
//! in `update` (insert on `Char`, [`TextArea::insert_newline`] on
//! Shift+Enter, the `move_*` family on the arrows), exactly as
//! [`Editor`] projects one. [`PromptInput`] only *reads* the `TextArea`,
//! the [`ChatStatus`], and the attachment chips (ADR 0012 §P1); it owns
//! nothing and never edits at render time.
//!
//! # What it draws
//!
//! A bordered panel: optional **attachment chips** on the first inner
//! row (`📎 name ✕` pills, the ai-elements `PromptInputAttachments`), the
//! [`Editor`] below, and a 1-cell **action
//! glyph** at the panel's top-right whose symbol follows the
//! [`ChatStatus`] — the ai-elements `PromptInputSubmit`:
//!
//! - [`ChatStatus::Ready`] → `➤` (send);
//! - [`ChatStatus::Submitted`] / [`ChatStatus::Streaming`] → `■` (stop —
//!   [`ChatStatus::is_busy`]);
//! - [`ChatStatus::Error`] → `⚠` (the error affordance).
//!
//! # Intent, not callbacks
//!
//! There is **no callback**. [`PromptInput::action_rect`] is the
//! send/stop hit rect and [`PromptInput::attachment_rects`] are the
//! per-chip remove (`✕`) hit rects; the reducer maps a click/key to a
//! [`PromptInputIntent`] (and the spec's keymap is the reducer's:
//! **Enter = [`Submit`](PromptInputIntent::Submit)**, **Shift+Enter =
//! newline** via [`TextArea::insert_newline`]) — the pure-projection
//! rule (ADR 0012 §P1), the same seam
//! [`SplitPane`](rstui_widgets::SplitPane) uses.
//! [`PromptInput::intent_at`] is the convenience hit-test.
//!
//! # Total, never a panic
//!
//! An empty area, a zero-size area, no attachments, more chips than fit,
//! and a document taller than the panel are all safe clips/no-ops (the
//! [`Gauge`](rstui_widgets::Gauge) totality rule).

use rstui_code::Editor; // ADR 0024: Editor moved to rstui-code
use rstui_core::{Buffer, Color, Line, Position, Rect, Span, Style, TextArea, Widget};
use rstui_widgets::{Block, Borders};

use crate::model::ChatStatus;

/// One attachment chip in the composer strip (the ai-elements
/// `PromptInputAttachment`). A plain value type the caller owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// The display name (e.g. a file name).
    pub name: String,
}

impl Attachment {
    /// An attachment chip named `name`.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// What a click/key on the composer means — the reducer derives one from
/// the hit rects and applies it in `update` (there is **no** callback;
/// ADR 0012 §P1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptInputIntent {
    /// Send the prompt (the action glyph clicked while
    /// [`ChatStatus::Ready`], or the reducer's Enter mapping).
    Submit,
    /// Stop the in-flight turn (the action glyph clicked while
    /// [`ChatStatus::is_busy`]).
    Stop,
    /// Remove the attachment at this index (its `✕` clicked).
    RemoveAttachment(usize),
}

/// The composer action glyph for a [`ChatStatus`] (the ai-elements
/// `PromptInputSubmit` icon: `➤` ready, `■` busy, `⚠` error).
#[must_use]
pub fn action_glyph(status: ChatStatus) -> char {
    match status {
        ChatStatus::Ready => '➤',
        ChatStatus::Submitted | ChatStatus::Streaming => '■',
        ChatStatus::Error => '⚠',
    }
}

/// The chat composer — a pure projection of a borrowed [`TextArea`], a
/// [`ChatStatus`], and caller-owned attachment chips.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, TextArea, Widget};
/// use rstui_ai::model::ChatStatus;
/// use rstui_ai::prompt_input::{Attachment, PromptInput};
///
/// // The TextArea is caller-owned model state the reducer edits.
/// let doc = TextArea::from_value("Ask me anything");
/// let chips = [Attachment::new("report.pdf")];
/// let composer = PromptInput::new(&doc, ChatStatus::Ready)
///     .attachments(&chips)
///     .focused(true);
/// let mut buf = Buffer::empty(Rect::new(0, 0, 30, 5));
/// composer.render(buf.area(), &mut buf);
/// ```
#[derive(Debug, Clone)]
pub struct PromptInput<'a> {
    model: &'a TextArea,
    status: ChatStatus,
    attachments: &'a [Attachment],
    focused: bool,
    scroll: (usize, usize),
    style: Style,
    placeholder: &'a str,
}

impl<'a> PromptInput<'a> {
    /// A composer projecting `model` at `status`, no attachments,
    /// unfocused, unscrolled, unstyled, with a default placeholder.
    #[must_use]
    pub fn new(model: &'a TextArea, status: ChatStatus) -> Self {
        Self {
            model,
            status,
            attachments: &[],
            focused: false,
            scroll: (0, 0),
            style: Style::new(),
            placeholder: "Send a message…",
        }
    }

    /// Sets the caller-owned attachment chips (the ai-elements
    /// `PromptInputAttachments`; the reducer owns the list).
    #[must_use]
    pub fn attachments(mut self, attachments: &'a [Attachment]) -> Self {
        self.attachments = attachments;
        self
    }

    /// Sets whether the editor is focused — caller-owned state (move it in
    /// `update`, typically via a `FocusRing`); passed straight to the
    /// [`Editor`] for its caret.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Sets the caller-owned 2D editor scroll `(row, col)` (the reducer
    /// drives it from the cursor, exactly as with
    /// [`Editor::scroll`](rstui_code::Editor::scroll)).
    #[must_use]
    pub fn scroll(mut self, scroll: (usize, usize)) -> Self {
        self.scroll = scroll;
        self
    }

    /// Sets the base [`Style`] (also fills the panel region).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the empty-document placeholder hint.
    #[must_use]
    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// The framing [`Block`] — the single definition the rects and
    /// [`render`](Widget::render) share so they never disagree.
    fn frame() -> Block<'static> {
        Block::new().borders(Borders::ALL)
    }

    /// The 1-cell action-glyph hit rect (panel top-right, the send/stop
    /// affordance), or `None` for an empty area. A pure function of
    /// `area` — the reducer maps a click here to
    /// [`Submit`](PromptInputIntent::Submit)/[`Stop`](PromptInputIntent::Stop)
    /// (per [`ChatStatus::is_busy`]).
    #[must_use]
    pub fn action_rect(&self, area: Rect) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        let inner = Self::frame().inner(area);
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

    /// The attachment-strip rect (the first inner row), or `None` when
    /// there are no attachments or no room. The chips live here; their
    /// per-chip remove rects are [`attachment_rects`](Self::attachment_rects).
    #[must_use]
    pub fn attachments_rect(&self, area: Rect) -> Option<Rect> {
        if self.attachments.is_empty() || area.is_empty() {
            return None;
        }
        let inner = Self::frame().inner(area);
        if inner.is_empty() {
            return None;
        }
        // Leave the action glyph its cell at the right.
        let width = inner.width.saturating_sub(1);
        if width == 0 {
            return None;
        }
        Some(Rect::new(inner.left(), inner.top(), width, 1))
    }

    /// The editor rect (below the attachment strip if any), or `None`
    /// when there is no room. A pure function of `area`.
    #[must_use]
    pub fn editor_rect(&self, area: Rect) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        let inner = Self::frame().inner(area);
        if inner.is_empty() {
            return None;
        }
        let strip = u16::from(self.attachments_rect(area).is_some());
        let top = inner.top().saturating_add(strip);
        if top >= inner.bottom() {
            return None;
        }
        Some(Rect::new(
            inner.left(),
            top,
            inner.width,
            inner.bottom().saturating_sub(top),
        ))
    }

    /// The per-chip `✕`-remove hit rects, in attachment order (only those
    /// that fit in the strip). The reducer maps a click on rect `i` to
    /// [`RemoveAttachment(i)`](PromptInputIntent::RemoveAttachment) — no
    /// callback (ADR 0012 §P1).
    #[must_use]
    pub fn attachment_rects(&self, area: Rect) -> Vec<Rect> {
        let Some(strip) = self.attachments_rect(area) else {
            return Vec::new();
        };
        let mut rects = Vec::new();
        let mut x = strip.left();
        for chip in self.attachments {
            // "📎 name ✕ " — the ✕ sits just before the trailing space.
            let chip_w = chip_width(&chip.name);
            if x.saturating_add(chip_w) > strip.right() {
                break;
            }
            // The ✕ is the second-to-last glyph of the chip run.
            let cross_x = x
                .saturating_add(chip_w)
                .saturating_sub(2)
                .min(strip.right().saturating_sub(1));
            rects.push(Rect::new(cross_x, strip.top(), 1, 1));
            x = x.saturating_add(chip_w);
        }
        rects
    }

    /// Hit-tests `position` against the action glyph and the attachment
    /// `✕`s, returning the [`PromptInputIntent`] (if any). The
    /// convenience wrapper over [`action_rect`](Self::action_rect) /
    /// [`attachment_rects`](Self::attachment_rects) the reducer calls on
    /// a click.
    #[must_use]
    pub fn intent_at(&self, position: Position, area: Rect) -> Option<PromptInputIntent> {
        if let Some(rect) = self.action_rect(area) {
            if rect.contains(position) {
                return Some(if self.status.is_busy() {
                    PromptInputIntent::Stop
                } else {
                    PromptInputIntent::Submit
                });
            }
        }
        for (i, rect) in self.attachment_rects(area).into_iter().enumerate() {
            if rect.contains(position) {
                return Some(PromptInputIntent::RemoveAttachment(i));
            }
        }
        None
    }
}

/// The display width of one attachment chip: `📎 name ✕ ` — the leaf
/// glyph + a space, the name, a space, the `✕`, and a trailing space.
fn chip_width(name: &str) -> u16 {
    let chars = 2 + name.chars().count() + 3; // "📎 " + name + " ✕ "
    u16::try_from(chars).unwrap_or(u16::MAX)
}

impl Widget for PromptInput<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let base = self.style;
        let frame = Self::frame();
        let inner = frame.inner(area);
        frame.style(base).render(area, buf);
        if inner.is_empty() {
            return;
        }
        buf.set_style(inner, base);

        // The attachment strip (chips), if any.
        if let Some(strip) = self.attachments_rect(area) {
            buf.set_style(strip, base);
            let chip_style = base.fg(Color::Cyan);
            let mut x = strip.left();
            for chip in self.attachments {
                let chip_w = chip_width(&chip.name);
                if x.saturating_add(chip_w) > strip.right() {
                    break;
                }
                let run = format!("📎 {} ✕ ", chip.name);
                Line::from(Span::styled(run, chip_style))
                    .render(Rect::new(x, strip.top(), chip_w, 1), buf);
                x = x.saturating_add(chip_w);
            }
        }

        // The editor.
        if let Some(editor_area) = self.editor_rect(area) {
            Editor::new(self.model)
                .focused(self.focused)
                .scroll(self.scroll)
                .style(base)
                .placeholder(self.placeholder)
                .placeholder_style(base.fg(Color::DarkGray))
                .render(editor_area, buf);
        }

        // The action glyph (send / stop / error), top-right.
        if let Some(action) = self.action_rect(area) {
            let color = match self.status {
                ChatStatus::Ready => Color::Green,
                ChatStatus::Submitted | ChatStatus::Streaming => Color::Yellow,
                ChatStatus::Error => Color::Red,
            };
            buf.set_cell(action.position(), action_glyph(self.status), base.fg(color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(buf: &Buffer, y: u16, w: u16) -> String {
        (0..w)
            .map(|x| buf.get(Position::new(x, y)).unwrap().symbol)
            .collect()
    }

    fn dump(buf: &Buffer, w: u16, h: u16) -> String {
        let mut out = String::new();
        for y in 0..h {
            out.push_str(&row(buf, y, w));
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_action_glyph_follows_the_chat_status() {
        assert_eq!(action_glyph(ChatStatus::Ready), '➤');
        assert_eq!(action_glyph(ChatStatus::Submitted), '■');
        assert_eq!(action_glyph(ChatStatus::Streaming), '■');
        assert_eq!(action_glyph(ChatStatus::Error), '⚠');
    }

    #[test]
    fn it_renders_the_editor_text_and_a_send_glyph() {
        let doc = TextArea::from_value("hello");
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        PromptInput::new(&doc, ChatStatus::Ready).render(buf.area(), &mut buf);
        let text = dump(&buf, 20, 4);
        assert!(text.contains("hello"), "editor text: {text}");
        assert!(text.contains('➤'), "send glyph: {text}");
    }

    #[test]
    fn a_busy_status_shows_the_stop_glyph() {
        let doc = TextArea::new();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        PromptInput::new(&doc, ChatStatus::Streaming).render(buf.area(), &mut buf);
        assert!(dump(&buf, 20, 4).contains('■'), "stop glyph expected");
    }

    #[test]
    fn an_empty_document_shows_the_placeholder() {
        let doc = TextArea::new();
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 4));
        PromptInput::new(&doc, ChatStatus::Ready)
            .placeholder("Ask…")
            .render(buf.area(), &mut buf);
        assert!(dump(&buf, 24, 4).contains("Ask…"), "placeholder expected");
    }

    #[test]
    fn attachment_chips_render_on_the_first_inner_row() {
        let doc = TextArea::from_value("body");
        let chips = [Attachment::new("a.pdf"), Attachment::new("b.png")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 5));
        PromptInput::new(&doc, ChatStatus::Ready)
            .attachments(&chips)
            .render(buf.area(), &mut buf);
        // Row 1 (inside the top border) is the chip strip.
        let strip = row(&buf, 1, 30);
        assert!(strip.contains("📎 a.pdf"), "{strip:?}");
        assert!(strip.contains("📎 b.png"), "{strip:?}");
        // The editor body is pushed below the strip (row 2).
        assert!(row(&buf, 2, 30).contains("body"), "editor below strip");
    }

    #[test]
    fn intent_at_maps_the_action_glyph_to_submit_or_stop() {
        let doc = TextArea::from_value("x");
        let area = Rect::new(0, 0, 20, 4);
        let ready = PromptInput::new(&doc, ChatStatus::Ready);
        let action = ready.action_rect(area).unwrap();
        assert_eq!(
            ready.intent_at(action.position(), area),
            Some(PromptInputIntent::Submit)
        );
        let busy = PromptInput::new(&doc, ChatStatus::Submitted);
        assert_eq!(
            busy.intent_at(action.position(), area),
            Some(PromptInputIntent::Stop)
        );
        // A click in the editor body is not an action.
        assert_eq!(ready.intent_at(Position::new(1, 1), area), None);
    }

    #[test]
    fn intent_at_maps_a_chip_cross_to_remove_that_attachment() {
        let doc = TextArea::new();
        let chips = [Attachment::new("a"), Attachment::new("b")];
        let area = Rect::new(0, 0, 30, 4);
        let composer = PromptInput::new(&doc, ChatStatus::Ready).attachments(&chips);
        let rects = composer.attachment_rects(area);
        assert_eq!(rects.len(), 2);
        // The ✕ cell of each chip resolves to RemoveAttachment(i).
        assert_eq!(
            composer.intent_at(rects[0].position(), area),
            Some(PromptInputIntent::RemoveAttachment(0))
        );
        assert_eq!(
            composer.intent_at(rects[1].position(), area),
            Some(PromptInputIntent::RemoveAttachment(1))
        );
        // And those cells actually render a ✕.
        let mut buf = Buffer::empty(area);
        composer.render(area, &mut buf);
        assert_eq!(buf.get(rects[0].position()).unwrap().symbol, '✕');
    }

    #[test]
    fn editor_rect_is_the_whole_inner_when_there_are_no_attachments() {
        let doc = TextArea::new();
        let area = Rect::new(0, 0, 20, 5);
        let composer = PromptInput::new(&doc, ChatStatus::Ready);
        assert_eq!(composer.attachments_rect(area), None);
        let er = composer.editor_rect(area).unwrap();
        let inner = PromptInput::frame().inner(area);
        assert_eq!(er, inner);
    }

    #[test]
    fn many_chips_clip_to_the_strip_without_a_panic() {
        let doc = TextArea::new();
        let chips: Vec<_> = (0..30)
            .map(|i| Attachment::new(format!("file{i}")))
            .collect();
        let area = Rect::new(0, 0, 24, 4);
        let composer = PromptInput::new(&doc, ChatStatus::Ready).attachments(&chips);
        // Only the chips that fit get a remove rect; never a panic.
        let rects = composer.attachment_rects(area);
        assert!(rects.len() < chips.len());
        let mut buf = Buffer::empty(area);
        composer.render(area, &mut buf);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let doc = TextArea::from_value("x");
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        PromptInput::new(&doc, ChatStatus::Ready).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        let composer = PromptInput::new(&doc, ChatStatus::Ready);
        assert_eq!(composer.action_rect(Rect::new(0, 0, 0, 0)), None);
        assert_eq!(composer.editor_rect(Rect::new(0, 0, 0, 0)), None);
        assert!(composer.attachment_rects(Rect::new(0, 0, 0, 0)).is_empty());
    }

    #[test]
    fn a_tiny_area_with_no_inner_is_total() {
        let doc = TextArea::from_value("x");
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        PromptInput::new(&doc, ChatStatus::Ready).render(buf.area(), &mut buf);
        assert_eq!(
            PromptInput::new(&doc, ChatStatus::Ready).action_rect(Rect::new(0, 0, 2, 2)),
            None
        );
    }
}
