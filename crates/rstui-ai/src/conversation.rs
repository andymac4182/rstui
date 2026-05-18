//! [`Conversation`] — an auto-stick-to-bottom transcript, the rstui
//! translation of the ai-elements `Conversation` /
//! `ConversationContent` / `ConversationScrollButton` /
//! `messagesToMarkdown` family (`conversation.tsx`).
//!
//! # A pure projection of `&[UiMessage]` + a caller-owned [`ScrollState`]
//!
//! ai-elements wraps the transcript in `use-stick-to-bottom`: the view
//! stays pinned to the newest message while streaming, until the user
//! scrolls up. In rstui that "sticky-bottom intent" is exactly
//! [`rstui_core::ScrollState`] — a caller-owned value the **reducer**
//! drives in `update` (call
//! [`ScrollState::on_content_change`](rstui_core::ScrollState::on_content_change)
//! when a message/chunk arrives for sticky-bottom-while-streaming;
//! [`ScrollState::scroll_by`](rstui_core::ScrollState::scroll_by) for
//! wheel/keys; [`ScrollState::scroll_to_end`](rstui_core::ScrollState::scroll_to_end)
//! for the "jump to bottom" affordance). The widget only *reads*
//! [`ScrollState::offset`](rstui_core::ScrollState::offset) (ADR 0012 §P0)
//! — it never scrolls itself, exactly as `List`/`ScrollView` project a
//! caller-owned offset.
//!
//! # Row-indexed scrolling + caller-side windowing
//!
//! The transcript is flattened to **rows** (each [`Message`]'s
//! [`height`](Message::height) at the content width, plus a 1-row gap
//! between turns — the ai-elements `gap-8`). The offset is a row index;
//! [`Conversation`] renders **only the messages that intersect the
//! visible window** (it skips turns entirely above/below it) — the
//! pure-projection answer to virtualization
//! ([`docs/composition.md`](https://github.com/andymac4182/rstui/blob/main/docs/composition.md)
//! "build only the visible item range"). A 10 000-message transcript
//! costs one [`Markdown`](rstui_widgets::Markdown) layout per *visible*
//! turn, not per turn.
//!
//! # Affordances, as state not callbacks
//!
//! [`Conversation::is_at_bottom`] (ai-elements `isAtBottom`, gating the
//! scroll-to-bottom button) and [`Conversation::content_rows`] are pure
//! accessors; the "scroll to bottom" action is just the reducer calling
//! [`ScrollState::scroll_to_end`](rstui_core::ScrollState::scroll_to_end)
//! (no callback — ADR 0012 §P1). [`messages_to_markdown`] is the
//! ai-elements export verbatim (for copy / download).
//!
//! # Total, never a panic
//!
//! An empty area, a zero-size area, an empty transcript, an offset past
//! the end, and a turn taller than the viewport are all safe
//! clips/no-ops (the [`Gauge`](rstui_widgets::Gauge) totality rule).

use rstui_core::scroll::ScrollState;
use rstui_core::{Buffer, Color, Line, Rect, Style, Widget};

use crate::conversation_cache::ConversationCache;
use crate::message::{Message, message_body_markdown, role_label};
use crate::model::UiMessage;

/// The ai-elements `defaultFormatMessage`: `**Role:** body-text` for one
/// turn (the body is its concatenated text/part markdown).
#[must_use]
pub fn format_message(message: &UiMessage) -> String {
    format!(
        "**{}:** {}",
        role_label(message.role),
        message_body_markdown(message)
    )
}

/// The ai-elements `messagesToMarkdown` export: every turn formatted by
/// [`format_message`], blank-line separated — for the copy / download
/// affordance. A pure function of the borrowed slice.
#[must_use]
pub fn messages_to_markdown(messages: &[UiMessage]) -> String {
    messages
        .iter()
        .map(format_message)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// One row between consecutive turns (the ai-elements `gap-8`).
const TURN_GAP: u16 = 1;

/// An auto-stick-to-bottom transcript of `&[UiMessage]`, a pure
/// projection of the slice plus a caller-owned [`ScrollState`] (`row`
/// offset).
///
/// # Example
///
/// ```
/// use rstui_core::scroll::ScrollState;
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_ai::conversation::Conversation;
/// use rstui_ai::model::UiMessage;
/// use serde_json::json;
///
/// let msgs = vec![UiMessage::from_value(&json!({
///     "role": "user", "parts": [{ "type": "text", "text": "hi" }]
/// }))];
/// // `scroll` is caller-owned model state the reducer drives in `update`.
/// let scroll = ScrollState::new();
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 6));
/// Conversation::new(&msgs, &scroll).render(buf.area(), &mut buf);
/// ```
#[derive(Debug, Clone)]
pub struct Conversation<'a> {
    messages: &'a [UiMessage],
    scroll: &'a ScrollState,
    style: Style,
    empty_text: &'a str,
    cache: Option<&'a ConversationCache>,
}

impl<'a> Conversation<'a> {
    /// A transcript projecting `messages` at `scroll`'s offset, unstyled,
    /// with the ai-elements default empty-state text.
    #[must_use]
    pub fn new(messages: &'a [UiMessage], scroll: &'a ScrollState) -> Self {
        Self {
            messages,
            scroll,
            style: Style::new(),
            empty_text: "No messages yet",
            cache: None,
        }
    }

    /// Sets the base [`Style`] (also fills the transcript region).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the centred empty-state text shown when there are no messages
    /// (the ai-elements `ConversationEmptyState`).
    #[must_use]
    pub fn empty_text(mut self, empty_text: &'a str) -> Self {
        self.empty_text = empty_text;
        self
    }

    /// Attaches a caller-owned [`ConversationCache`] (the UI-1/MD-1
    /// model). The per-frame windowing math —
    /// [`content_rows`](Self::content_rows), `turn_starts`, and the render
    /// loop — measures **every** turn's [`Message::height`] every frame; a
    /// transcript of *N* turns otherwise pays *N* full Markdown re-parses
    /// per frame regardless of how few are on screen (perf-review-2
    /// R2-AI-1). With a cache the reducer measures each immutable turn
    /// **once** (in `update`, via [`ConversationCache::sync`]); the widget
    /// only reads it here. A miss measures fresh and yields the same
    /// number, so a cached transcript renders **byte-identically** — this
    /// is a pure, opt-in optimization (ADR 0012 §P1).
    #[must_use]
    pub fn cache(mut self, cache: &'a ConversationCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// The total content height in rows at `width`: every turn's
    /// [`Message::height`] plus a one-row gap between turns. The
    /// `content_len` the caller passes to [`ScrollState`] (a pure
    /// measurement; saturates at [`u16::MAX`]).
    #[must_use]
    pub fn content_rows(&self, width: u16) -> u16 {
        let mut rows: u32 = 0;
        for (i, message) in self.messages.iter().enumerate() {
            if i > 0 {
                rows += u32::from(TURN_GAP);
            }
            rows += u32::from(Message::new(message).cache_opt(self.cache).height(width));
        }
        u16::try_from(rows).unwrap_or(u16::MAX)
    }

    /// Whether the offset is at the last full window — i.e. the newest
    /// turn is visible (the ai-elements `isAtBottom`, which gates the
    /// scroll-to-bottom button). A pure function of the caller-owned
    /// scroll and the layout.
    #[must_use]
    pub fn is_at_bottom(&self, area: Rect) -> bool {
        self.scroll
            .at_end(self.content_rows(area.width) as usize, area.height as usize)
    }

    /// The starting row of every turn at `width` (a prefix sum over
    /// [`Message::height`] + the one-row turn gap). Internal layout shared
    /// by [`content_rows`](Self::content_rows) and
    /// [`render`](Widget::render) so they never disagree.
    fn turn_starts(&self, width: u16) -> Vec<u16> {
        let mut starts = Vec::with_capacity(self.messages.len());
        let mut y: u32 = 0;
        for (i, message) in self.messages.iter().enumerate() {
            if i > 0 {
                y += u32::from(TURN_GAP);
            }
            starts.push(u16::try_from(y).unwrap_or(u16::MAX));
            y += u32::from(Message::new(message).cache_opt(self.cache).height(width));
        }
        starts
    }
}

impl Widget for Conversation<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let base = self.style;
        buf.set_style(area, base);

        // Empty state: the centred ai-elements `ConversationEmptyState`.
        if self.messages.is_empty() {
            let y = area.top() + area.height / 2;
            let line =
                Line::styled(self.empty_text.to_owned(), base.fg(Color::DarkGray)).centered();
            line.render(Rect::new(area.left(), y, area.width, 1), buf);
            return;
        }

        let width = area.width;
        let offset = self.scroll.offset();
        let view_top = offset;
        let view_bottom = offset.saturating_add(area.height as usize);
        let starts = self.turn_starts(width);

        // Caller-side windowing: render only the turns that intersect the
        // visible row window [view_top, view_bottom). A turn above/below
        // it is skipped entirely (no Markdown layout) — the pure
        // virtualization model (docs/composition.md).
        for (i, message) in self.messages.iter().enumerate() {
            let start = starts[i] as usize;
            let h = Message::new(message).cache_opt(self.cache).height(width) as usize;
            let end = start + h;
            if end <= view_top || start >= view_bottom {
                continue;
            }

            // Where this turn lands on screen relative to the offset.
            // `clip_top` rows of the turn are above the viewport.
            let clip_top = view_top.saturating_sub(start);
            let screen_y = area.top() as usize + start.saturating_sub(view_top);
            let visible_h = h
                .saturating_sub(clip_top)
                .min((area.bottom() as usize).saturating_sub(screen_y));
            if visible_h == 0 {
                continue;
            }

            let turn_area = Rect::new(area.left(), screen_y as u16, width, visible_h as u16);
            // A turn partly scrolled off the top: render it at full
            // height into a scratch rect, then blit the visible slice.
            if clip_top == 0 {
                Message::new(message).style(base).render(turn_area, buf);
            } else {
                render_clipped_turn(message, base, clip_top as u16, turn_area, buf);
            }
        }
    }
}

/// Renders `message` skipping its first `clip_top` rows, into `dest`.
///
/// A turn scrolled partly off the top still shows its lower rows. There
/// is no public "render with a row offset" on [`Message`]; the role line
/// is row 0 and the body is a [`Markdown`](rstui_widgets::Markdown) whose
/// `scroll` skips composed rows, so the slice is reconstructed here from
/// those two pieces. Total: a `clip_top` past the turn draws nothing.
fn render_clipped_turn(
    message: &UiMessage,
    base: Style,
    clip_top: u16,
    dest: Rect,
    buf: &mut Buffer,
) {
    use rstui_widgets::Markdown;

    buf.set_style(dest, base);
    // Row 0 of a turn is the role line; rows 1.. are the markdown body.
    let body = message_body_markdown(message);
    if clip_top == 0 {
        // (Unreachable via render, but keeps this fn total on its own.)
        Message::new(message).style(base).render(dest, buf);
        return;
    }
    // The role line is clipped away; show the body scrolled by
    // (clip_top - 1) of its own rows.
    let body_scroll = clip_top - 1;
    Markdown::new(body)
        .style(base)
        .scroll(body_scroll)
        .render(dest, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Position;
    use serde_json::json;

    fn text_msg(role: &str, body: &str) -> UiMessage {
        UiMessage::from_value(&json!({
            "role": role,
            "parts": [{ "type": "text", "text": body }]
        }))
    }

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
    fn messages_to_markdown_is_the_ai_elements_export() {
        let msgs = vec![text_msg("user", "hi"), text_msg("assistant", "hey")];
        assert_eq!(
            messages_to_markdown(&msgs),
            "**You:** hi\n\n**Assistant:** hey"
        );
        assert_eq!(messages_to_markdown(&[]), "");
    }

    #[test]
    fn an_empty_transcript_shows_the_centred_empty_state() {
        let scroll = ScrollState::new();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 5));
        Conversation::new(&[], &scroll).render(buf.area(), &mut buf);
        // Row 2 (height/2) holds the centred text.
        assert!(row(&buf, 2, 20).contains("No messages yet"));
    }

    #[test]
    fn a_custom_empty_text_is_used() {
        let scroll = ScrollState::new();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
        Conversation::new(&[], &scroll)
            .empty_text("Ask away")
            .render(buf.area(), &mut buf);
        assert!(row(&buf, 1, 20).contains("Ask away"));
    }

    #[test]
    fn it_renders_the_visible_turns_top_anchored() {
        let msgs = vec![text_msg("user", "first"), text_msg("assistant", "second")];
        let scroll = ScrollState::default(); // top-anchored, offset 0
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
        Conversation::new(&msgs, &scroll).render(buf.area(), &mut buf);
        let text = dump(&buf, 20, 8);
        assert!(text.contains("You"), "{text}");
        assert!(text.contains("first"), "{text}");
        assert!(text.contains("Assistant"), "{text}");
        assert!(text.contains("second"), "{text}");
    }

    #[test]
    fn content_rows_sums_turn_heights_plus_gaps() {
        let msgs = vec![text_msg("user", "a"), text_msg("assistant", "b")];
        // Each turn: role line (1) + 1 body row = 2; + 1 gap between = 5.
        assert_eq!(
            Conversation::new(&msgs, &ScrollState::new()).content_rows(20),
            5
        );
        assert_eq!(
            Conversation::new(&[], &ScrollState::new()).content_rows(20),
            0
        );
    }

    #[test]
    fn sticky_bottom_offset_shows_the_newest_turn() {
        // Many short turns; a 4-row viewport. A following ScrollState
        // (the streaming default) snaps to the end on on_content_change.
        let msgs: Vec<_> = (0..10)
            .map(|i| text_msg("assistant", &format!("msg{i}")))
            .collect();
        let area = Rect::new(0, 0, 20, 4);
        let mut scroll = ScrollState::new();
        let total = Conversation::new(&msgs, &scroll).content_rows(area.width) as usize;
        scroll.on_content_change(total, area.height as usize);

        let mut buf = Buffer::empty(area);
        Conversation::new(&msgs, &scroll).render(area, &mut buf);
        let text = dump(&buf, 20, 4);
        assert!(text.contains("msg9"), "newest turn visible: {text}");
        assert!(!text.contains("msg0"), "oldest scrolled away: {text}");
        assert!(Conversation::new(&msgs, &scroll).is_at_bottom(area));
    }

    #[test]
    fn scrolled_up_is_not_at_bottom_and_shows_older_turns() {
        let msgs: Vec<_> = (0..10)
            .map(|i| text_msg("assistant", &format!("msg{i}")))
            .collect();
        let area = Rect::new(0, 0, 20, 4);
        let mut scroll = ScrollState::new();
        let total = Conversation::new(&msgs, &scroll).content_rows(area.width) as usize;
        scroll.on_content_change(total, area.height as usize);
        scroll.scroll_to_top(); // user scrolled to the very top

        assert!(!Conversation::new(&msgs, &scroll).is_at_bottom(area));
        let mut buf = Buffer::empty(area);
        Conversation::new(&msgs, &scroll).render(area, &mut buf);
        assert!(dump(&buf, 20, 4).contains("msg0"), "oldest visible at top");
    }

    #[test]
    fn a_turn_partly_scrolled_off_the_top_still_shows_its_lower_rows() {
        // One 2-row turn (role + body); scroll past its role line by 1.
        let msgs = vec![text_msg("assistant", "bodyline")];
        let area = Rect::new(0, 0, 20, 4);
        let mut scroll = ScrollState::default();
        scroll.set_offset(1);
        scroll.clamp(
            Conversation::new(&msgs, &scroll).content_rows(area.width) as usize,
            area.height as usize,
        );
        // content is 2 rows, viewport 4 -> clamps offset back to 0.
        // Force the clip path with a taller turn instead:
        let tall = vec![text_msg(
            "assistant",
            "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii",
        )];
        let mut scroll = ScrollState::default();
        scroll.set_offset(2); // skip the role line + 1 body row
        let total = Conversation::new(&tall, &scroll).content_rows(8) as usize;
        scroll.clamp(total, 3);
        let area = Rect::new(0, 0, 8, 3);
        let mut buf = Buffer::empty(area);
        Conversation::new(&tall, &scroll).render(area, &mut buf);
        // The role line ("Assistant") is scrolled away; body wrap is shown.
        let text = dump(&buf, 8, 3);
        assert!(!text.contains("Assistant"), "role clipped: {text}");
        assert!(text.contains("cccc") || text.contains("dddd"), "{text}");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let msgs = vec![text_msg("user", "x")];
        let scroll = ScrollState::new();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        Conversation::new(&msgs, &scroll).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn content_rows_is_total_at_zero_width() {
        let msgs = vec![text_msg("user", "x"), text_msg("assistant", "y")];
        // width 0 -> each Message::height is 1, + gap = 3.
        assert_eq!(
            Conversation::new(&msgs, &ScrollState::new()).content_rows(0),
            3
        );
    }

    fn id_msg(id: &str, role: &str, body: &str) -> UiMessage {
        UiMessage::from_value(&json!({
            "id": id, "role": role,
            "parts": [{ "type": "text", "text": body }]
        }))
    }

    /// The R2-AI-1 gate (perf-review-2): an attached, synced
    /// [`ConversationCache`](crate::conversation_cache::ConversationCache)
    /// must change *nothing* observable — the same `content_rows`, the
    /// same `turn_starts`, and a **byte-identical** render at every scroll
    /// position. It only removes the per-frame O(history) re-parse.
    #[test]
    fn a_synced_cache_renders_byte_identical_and_measures_the_same() {
        let msgs = vec![
            id_msg(
                "a",
                "user",
                "# Title\n\nfirst **turn** with [a link](http://e.com) and words to wrap",
            ),
            id_msg(
                "b",
                "assistant",
                "- alpha\n- beta\n- gamma delta epsilon zeta eta theta iota",
            ),
            id_msg("c", "user", "short"),
            id_msg(
                "d",
                "assistant",
                "the still-streaming last turn, a longer body that spans several rows here",
            ),
        ];
        let width = 18;
        let mut cache = ConversationCache::new();
        cache.sync(&msgs, width);

        let scroll = ScrollState::new();
        let plain = Conversation::new(&msgs, &scroll);
        let cached = Conversation::new(&msgs, &scroll).cache(&cache);
        assert_eq!(plain.content_rows(width), cached.content_rows(width));
        assert_eq!(plain.turn_starts(width), cached.turn_starts(width));

        // Top, middle, and clamped-to-bottom offsets exercise the
        // above/inside/below-window branches.
        for off in [0usize, 3, 9999] {
            let mut s = ScrollState::new();
            s.set_offset(off);
            let area = Rect::new(0, 0, width, 6);
            let mut a = Buffer::empty(area);
            let mut b = Buffer::empty(area);
            Conversation::new(&msgs, &s).render(area, &mut a);
            Conversation::new(&msgs, &s)
                .cache(&cache)
                .render(area, &mut b);
            assert_eq!(a, b, "cached render must be byte-identical at offset {off}");
        }
    }
}
