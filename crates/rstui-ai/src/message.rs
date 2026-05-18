//! [`Message`] — one chat turn / bubble, the rstui translation of the
//! ai-elements `Message` / `MessageContent` / `MessageBranch` family
//! (`message.tsx`).
//!
//! # A pure projection of a borrowed [`UiMessage`]
//!
//! ai-elements styles a turn by `from` (user bubbles align right with a
//! secondary background; assistant turns are flush text) and renders each
//! part. Here the turn is caller-owned model data
//! ([`crate::model::UiMessage`], parsed from the wire by
//! [`crate::model`]); [`Message`] only *reads* it (ADR 0012 §P1) and
//! draws:
//!
//! - a **role line** — `▸ You` / `▸ Assistant` / `▸ System`, accented per
//!   [`Role`] (the ai-elements `is-user` / `is-assistant` distinction);
//! - the **body**, each [`UiPart`] in order:
//!   [`Text`](crate::model::UiPart::Text) and
//!   [`Reasoning`](crate::model::UiPart::Reasoning) via
//!   [`Markdown`] (the house renderer — do not
//!   reinvent it), a [`Tool`](crate::model::UiPart::Tool) as a compact
//!   one-line summary (`🔧 name [state]` — the full card is
//!   [`crate::tool::Tool`], a turn lists them compactly), a
//!   [`SourceUrl`](crate::model::UiPart::SourceUrl) /
//!   [`SourceDocument`](crate::model::UiPart::SourceDocument) as a `↗`
//!   citation line, a [`File`](crate::model::UiPart::File) as a `📎` chip,
//!   a [`Data`](crate::model::UiPart::Data) / [`Unknown`](crate::model::UiPart::Unknown)
//!   as a dim debug line (totality — an unknown part is still surfaced,
//!   never dropped), and [`StepStart`](crate::model::UiPart::StepStart) as
//!   a thin rule.
//!
//! Height is content-driven: [`Message::height`] is a pure measurement
//! (the role line + the wrapped parts at a given width) so the caller —
//! e.g. [`crate::conversation::Conversation`] — can lay turns out and
//! window them without rendering, exactly as
//! [`Editor::content_height`](rstui_widgets::Editor::content_height) does.
//!
//! # [`MessageBranch`]: the regenerate selector
//!
//! ai-elements' `MessageBranch` cycles alternative responses with
//! `‹`/`›`. [`MessageBranchState`] is the tiny caller-owned `{ current,
//! total }` the reducer mutates; [`MessageBranch`] projects it as a
//! `‹ n/m ›` selector and exposes [`MessageBranch::prev_rect`] /
//! [`MessageBranch::next_rect`] hit rects — no callback, the
//! pure-projection rule (ADR 0012 §P1).
//!
//! # Total, never a panic
//!
//! An empty area, a zero-size area, an empty `parts`, an unknown part,
//! and a body taller than the area are all safe clips/no-ops (the
//! [`Gauge`](rstui_widgets::Gauge) totality rule).

use rstui_core::{Buffer, Color, Line, Modifier, Rect, Span, Style, Widget};
use rstui_widgets::Markdown;

use crate::conversation_cache::{ConversationCache, measure_height};
use crate::model::{Role, UiMessage, UiPart};

/// The display label for a [`Role`] (the ai-elements role text, title-cased).
#[must_use]
pub fn role_label(role: Role) -> &'static str {
    match role {
        Role::System => "System",
        Role::User => "You",
        Role::Assistant => "Assistant",
    }
}

/// The accent [`Color`] for a [`Role`]'s name (the ai-elements
/// user/assistant/system visual distinction).
#[must_use]
pub fn role_color(role: Role) -> Color {
    match role {
        Role::System => Color::Yellow,
        Role::User => Color::Cyan,
        Role::Assistant => Color::Green,
    }
}

/// Renders the body parts of `message` into the markdown-ish lines a
/// [`Message`] / [`crate::conversation::Conversation`] draws, one logical
/// block per part. Public so the conversation can measure/window a turn
/// without a [`Buffer`] (the pure-measurement seam).
#[must_use]
pub fn part_to_markdown(part: &UiPart) -> String {
    match part {
        UiPart::Text { text, .. } => text.clone(),
        UiPart::Reasoning { text, .. } => format!("> 🧠 {text}"),
        UiPart::Tool(tool) => {
            format!("`🔧 {} [{}]`", tool.tool_name, tool.state.label())
        }
        UiPart::SourceUrl { url, title, .. } => {
            let label = title.as_deref().unwrap_or(url.as_str());
            format!("↗ [{label}]({url})")
        }
        UiPart::SourceDocument {
            title, filename, ..
        } => {
            let name = filename.as_deref().unwrap_or(title.as_str());
            format!("↗ {title} ({name})")
        }
        UiPart::File {
            filename,
            media_type,
            ..
        } => {
            let name = filename.as_deref().unwrap_or(media_type.as_str());
            format!("`📎 {name}`")
        }
        UiPart::StepStart => "---".to_owned(),
        UiPart::Data { name, .. } => format!("`data-{name}`"),
        UiPart::Unknown(value) => format!("`?{value}`"),
    }
}

/// The whole body of `message` as one markdown document (the parts joined
/// by blank lines), the input [`Message`]/[`crate::conversation`] render
/// and measure.
#[must_use]
pub fn message_body_markdown(message: &UiMessage) -> String {
    message
        .parts
        .iter()
        .map(part_to_markdown)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// One chat turn rendered as a pure projection of a borrowed
/// [`UiMessage`]: a role line then the body parts.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_ai::message::Message;
/// use rstui_ai::model::UiMessage;
/// use serde_json::json;
///
/// let msg = UiMessage::from_value(&json!({
///     "id": "m1", "role": "assistant",
///     "parts": [{ "type": "text", "text": "Hello!" }]
/// }));
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
/// Message::new(&msg).render(buf.area(), &mut buf);
/// ```
#[derive(Debug, Clone)]
pub struct Message<'a> {
    message: &'a UiMessage,
    style: Style,
    cache: Option<&'a ConversationCache>,
}

impl<'a> Message<'a> {
    /// A turn projecting `message`, unstyled.
    #[must_use]
    pub fn new(message: &'a UiMessage) -> Self {
        Self {
            message,
            style: Style::new(),
            cache: None,
        }
    }

    /// Sets the base [`Style`] (also fills the turn region).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Attaches a caller-owned [`ConversationCache`] so
    /// [`height`](Self::height) can reuse a memoized measurement instead
    /// of re-parsing the body markdown every call (the UI-1/MD-1 model).
    /// Purely an optimization: a cache miss measures fresh and returns the
    /// same number, so a cached turn renders byte-identically.
    #[must_use]
    pub fn cache(mut self, cache: &'a ConversationCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Like [`cache`](Self::cache) but for an already-optional cache (the
    /// seam [`Conversation`](crate::conversation::Conversation) threads its
    /// own optional cache through without re-branching at each call site).
    #[must_use]
    pub(crate) fn cache_opt(mut self, cache: Option<&'a ConversationCache>) -> Self {
        self.cache = cache;
        self
    }

    /// The number of rows this turn needs at `width` columns: the role
    /// line (1) plus the wrapped body. A **pure measurement** of the
    /// borrowed model owning no state and touching no [`Buffer`], exactly
    /// like [`Editor::content_height`](rstui_widgets::Editor::content_height)
    /// — what [`crate::conversation::Conversation`] uses to lay out and
    /// window turns. `width == 0` yields `1` (just the role line);
    /// saturates at [`u16::MAX`].
    ///
    /// With a [`cache`](Self::cache) attached this is an O(1) memoized
    /// read on a hit (the common, non-streaming case); a miss falls back
    /// to the same fresh measurement, so the result is identical either
    /// way. Without a cache it is always the fresh measurement (the
    /// pre-cache behavior, unchanged).
    #[must_use]
    pub fn height(&self, width: u16) -> u16 {
        if let Some(cache) = self.cache {
            if let Some(h) = cache.height(self.message, width) {
                return h;
            }
        }
        measure_height(self.message, width)
    }

    /// The role line [`Line`]: `▸ You` / `▸ Assistant` / `▸ System`,
    /// accented per [`Role`].
    fn role_line(&self, base: Style) -> Line<'static> {
        let role = self.message.role;
        let accent = base.fg(role_color(role)).add_modifier(Modifier::BOLD);
        Line::from(vec![
            Span::styled("▸ ", base.add_modifier(Modifier::DIM)),
            Span::styled(role_label(role), accent),
        ])
    }
}

impl Widget for Message<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let base = self.style;
        buf.set_style(area, base);

        let role_row = Rect::new(area.left(), area.top(), area.width, 1);
        self.role_line(base).render(role_row, buf);

        if area.height < 2 {
            return;
        }
        let body = Rect::new(
            area.left(),
            area.top().saturating_add(1),
            area.width,
            area.height.saturating_sub(1),
        );
        Markdown::new(message_body_markdown(self.message))
            .style(base)
            .render(body, buf);
    }
}

/// The caller-owned `{ current, total }` a [`MessageBranch`] selector
/// projects (the ai-elements `MessageBranch` state). Mutated only by the
/// reducer (on a [`MessageBranch::prev_rect`]/[`MessageBranch::next_rect`]
/// click); the widget only reads it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MessageBranchState {
    /// The 0-based index of the shown branch.
    pub current: usize,
    /// How many alternative branches exist.
    pub total: usize,
}

impl MessageBranchState {
    /// A selector over `total` branches showing branch `current` (clamped
    /// into range so an out-of-range caller value is still total).
    #[must_use]
    pub fn new(current: usize, total: usize) -> Self {
        Self {
            current: if total == 0 {
                0
            } else {
                current.min(total - 1)
            },
            total,
        }
    }

    /// The previous branch index, wrapping to the last (the ai-elements
    /// `goToPrevious`). `0` when there are no branches.
    #[must_use]
    pub fn prev(self) -> usize {
        if self.total == 0 {
            0
        } else if self.current == 0 {
            self.total - 1
        } else {
            self.current - 1
        }
    }

    /// The next branch index, wrapping to the first (the ai-elements
    /// `goToNext`). `0` when there are no branches.
    #[must_use]
    pub fn next(self) -> usize {
        // No branches, or already on the last → wrap to the first (0).
        if self.total == 0 || self.current + 1 >= self.total {
            0
        } else {
            self.current + 1
        }
    }
}

/// The ai-elements `MessageBranch` selector: a one-row `‹ n/m ›` control,
/// a pure projection of a [`MessageBranchState`].
///
/// [`prev_rect`](Self::prev_rect) / [`next_rect`](Self::next_rect) are the
/// hit rects the reducer maps a click to (then sets `current` from
/// [`MessageBranchState::prev`]/[`next`](MessageBranchState::next)) — no
/// callback (ADR 0012 §P1).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_ai::message::{MessageBranch, MessageBranchState};
///
/// let state = MessageBranchState::new(1, 3); // showing 2/3
/// let mut buf = Buffer::empty(Rect::new(0, 0, 9, 1));
/// MessageBranch::new(state).render(buf.area(), &mut buf);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct MessageBranch {
    state: MessageBranchState,
    style: Style,
}

impl MessageBranch {
    /// A selector projecting `state`, unstyled.
    #[must_use]
    pub fn new(state: MessageBranchState) -> Self {
        Self {
            state,
            style: Style::new(),
        }
    }

    /// Sets the base [`Style`].
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The `‹` hit rect (1 cell at the row's left), or `None` for an
    /// empty area — the reducer maps a click here to
    /// [`MessageBranchState::prev`].
    #[must_use]
    pub fn prev_rect(&self, area: Rect) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        Some(Rect::new(area.left(), area.top(), 1, 1))
    }

    /// The `›` hit rect (1 cell at the label's right edge), or `None` for
    /// an empty area — the reducer maps a click here to
    /// [`MessageBranchState::next`].
    #[must_use]
    pub fn next_rect(&self, area: Rect) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        let label_w = self.label().chars().count() as u16;
        let x = area
            .left()
            .saturating_add(label_w.saturating_sub(1))
            .min(area.right().saturating_sub(1));
        Some(Rect::new(x, area.top(), 1, 1))
    }

    /// The `‹ n/m ›` text (`‹ 0/0 ›` when there are no branches).
    fn label(&self) -> String {
        let shown = if self.state.total == 0 {
            0
        } else {
            self.state.current + 1
        };
        format!("‹ {shown}/{} ›", self.state.total)
    }
}

impl Widget for MessageBranch {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        Line::styled(self.label(), self.style).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Position;
    use serde_json::json;

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

    fn assistant() -> UiMessage {
        UiMessage::from_value(&json!({
            "id": "m1", "role": "assistant",
            "parts": [
                { "type": "text", "text": "Hello there" },
                { "type": "tool-search", "toolCallId": "t1", "state": "output-available" },
                { "type": "source-url", "sourceId": "s1", "url": "https://e.com", "title": "Ex" }
            ]
        }))
    }

    #[test]
    fn the_role_line_is_labelled_and_accented() {
        let m = assistant();
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 6));
        Message::new(&m).render(buf.area(), &mut buf);
        let role = row(&buf, 0, 30);
        assert!(role.contains("Assistant"), "role line was {role:?}");
        let name = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(name.symbol, 'A');
        assert_eq!(name.fg, Color::Green); // assistant accent
    }

    #[test]
    fn role_labels_and_colors_cover_every_role() {
        assert_eq!(role_label(Role::User), "You");
        assert_eq!(role_label(Role::Assistant), "Assistant");
        assert_eq!(role_label(Role::System), "System");
        assert_eq!(role_color(Role::User), Color::Cyan);
        assert_eq!(role_color(Role::System), Color::Yellow);
    }

    #[test]
    fn the_body_renders_each_part_kind() {
        let m = assistant();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 8));
        Message::new(&m).render(buf.area(), &mut buf);
        let text = dump(&buf, 40, 8);
        assert!(text.contains("Hello there"), "text part: {text}");
        assert!(text.contains("🔧 search"), "compact tool line: {text}");
        assert!(text.contains("Ex"), "citation line: {text}");
    }

    #[test]
    fn an_unknown_part_is_still_surfaced_not_dropped() {
        let m = UiMessage::from_value(&json!({
            "role": "assistant",
            "parts": [{ "type": "future-thing", "x": 1 }]
        }));
        assert!(matches!(m.parts[0], UiPart::Unknown(_)));
        let md = message_body_markdown(&m);
        assert!(md.contains("future-thing"), "unknown kept: {md:?}");
    }

    #[test]
    fn part_to_markdown_maps_each_variant() {
        let m = assistant();
        assert_eq!(part_to_markdown(&m.parts[0]), "Hello there");
        assert_eq!(part_to_markdown(&m.parts[1]), "`🔧 search [Completed]`");
        assert!(part_to_markdown(&m.parts[2]).starts_with("↗ ["));
    }

    #[test]
    fn height_is_role_line_plus_wrapped_body() {
        let m = UiMessage::from_value(&json!({
            "role": "user",
            "parts": [{ "type": "text", "text": "one line" }]
        }));
        // role line (1) + 1 body row at a generous width.
        assert_eq!(Message::new(&m).height(40), 2);
        // width 0 -> just the role line.
        assert_eq!(Message::new(&m).height(0), 1);
    }

    #[test]
    fn message_branch_state_wraps_both_directions() {
        let s = MessageBranchState::new(0, 3);
        assert_eq!(s.prev(), 2); // wrap to last
        assert_eq!(s.next(), 1);
        let s = MessageBranchState::new(2, 3);
        assert_eq!(s.next(), 0); // wrap to first
        assert_eq!(s.prev(), 1);
        // Out-of-range current is clamped (total).
        assert_eq!(MessageBranchState::new(9, 3).current, 2);
        // No branches: everything is 0, never a panic.
        let z = MessageBranchState::new(5, 0);
        assert_eq!((z.current, z.prev(), z.next()), (0, 0, 0));
    }

    #[test]
    fn message_branch_renders_the_selector_and_hit_rects() {
        let state = MessageBranchState::new(1, 3);
        let mb = MessageBranch::new(state);
        let area = Rect::new(0, 0, 9, 1);
        let mut buf = Buffer::empty(area);
        mb.render(area, &mut buf);
        assert_eq!(row(&buf, 0, 9), "‹ 2/3 ›  ");
        // Prev is the leftmost cell, next is at the '›'.
        assert_eq!(mb.prev_rect(area), Some(Rect::new(0, 0, 1, 1)));
        let nr = mb.next_rect(area).unwrap();
        assert_eq!(buf.get(nr.position()).unwrap().symbol, '›');
    }

    #[test]
    fn message_branch_with_no_branches_is_total() {
        let mb = MessageBranch::new(MessageBranchState::new(0, 0));
        let area = Rect::new(0, 0, 9, 1);
        let mut buf = Buffer::empty(area);
        mb.render(area, &mut buf);
        assert_eq!(row(&buf, 0, 9), "‹ 0/0 ›  ");
        assert!(mb.prev_rect(area).is_some());
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let m = assistant();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        Message::new(&m).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        let mb = MessageBranch::new(MessageBranchState::new(0, 2));
        assert_eq!(mb.prev_rect(Rect::new(0, 0, 0, 0)), None);
        assert_eq!(mb.next_rect(Rect::new(0, 0, 0, 0)), None);
    }

    #[test]
    fn a_one_row_area_draws_only_the_role_line() {
        let m = assistant();
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 1));
        Message::new(&m).render(buf.area(), &mut buf);
        assert!(row(&buf, 0, 30).contains("Assistant"));
    }
}
