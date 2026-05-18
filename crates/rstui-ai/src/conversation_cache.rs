//! [`ConversationCache`] — the caller-owned per-message height memo for
//! [`Conversation`](crate::conversation::Conversation) /
//! [`Message`](crate::message::Message) (the UI-1/MD-1 caller-owned-cache
//! model, ADR 0012 §P1, the [`ScrollState`](rstui_core::ScrollState)
//! precedent).
//!
//! # The cost this removes
//!
//! [`Message::height`](crate::message::Message::height) is
//! `1 + Markdown::new(message_body_markdown(msg)).lines(width).len()` — it
//! builds the body `String` from the parts **and runs the full Markdown
//! parser + line layout**. `Conversation` calls it for **every** message,
//! **every frame**: [`content_rows`](crate::conversation::Conversation::content_rows),
//! the private `turn_starts` prefix sum, and the render loop's windowing
//! math each walk the whole slice. So an *N*-message transcript pays *N* ×
//! a `widget/markdown/render`-class parse (~1.48 ms each) per frame just to
//! place the scroll window — unbounded in *N*, independent of how few turns
//! are on screen (perf-review-2 finding R2-AI-1; the review-1 root-cause B
//! re-introduced). The *rendering* of visible turns is already windowed;
//! only this **measurement** pass is not.
//!
//! # The contract (acp-client `refresh_md_cache`, verbatim)
//!
//! The reducer calls [`sync`](ConversationCache::sync) **once per
//! `update`** (never in `view`); the widgets only *read* the cache (a
//! shared `&ConversationCache`, no interior mutability). **Only the last
//! message can change** in an append-only chat transcript — a streamed
//! turn appends to the last message, older turns are immutable. So:
//!
//! - every **non-last** message with a stable non-empty
//!   [`id`](crate::model::UiMessage::id) is measured **once** per
//!   `(id, fingerprint, width)` and reused forever;
//! - the **last** (still-streaming) message is **never cached** — the
//!   widget re-measures it fresh every frame, so the output is
//!   **byte-identical** to the no-cache path;
//! - a message with an empty `id` is never cached (no stable key) — also
//!   re-measured fresh;
//! - a cache miss (no slot / different width / changed fingerprint)
//!   recomputes **exactly** [`Message::height`](crate::message::Message::height)'s
//!   formula, so a hit and a miss return the same number.
//!
//! Without a cache attached the widgets behave exactly as before (this is
//! a purely additive, opt-in optimization). The
//! `cache_height_equals_a_fresh_measure_for_every_non_last_message` and
//! the conversation byte-identical tests gate-enforce the exactness.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use rstui_widgets::Markdown;

use crate::message::message_body_markdown;
use crate::model::{Role, UiMessage, UiPart};

/// One memoized measurement: the message identity it was computed for and
/// the row height at a width. Co-located by `id` so a front-drain
/// (`cap_transcript`-style history trim) drops only the evicted ids.
#[derive(Debug, Clone)]
struct Slot {
    /// Cheap structural fingerprint of the message (see [`fingerprint`]).
    fp: u64,
    /// The width the height was measured at.
    width: u16,
    /// `Message::height(width)` for the message at that width.
    height: u16,
}

/// A coarse, allocation-free fingerprint of a message: its role, part
/// count, and per-part `(kind tag, text length)`. O(`parts`), no `String`
/// build — cheap enough to evaluate on every cache *read* without
/// reintroducing the per-frame cost the cache exists to remove.
///
/// In the append-only transcript a non-last message never changes, so this
/// is mostly defense-in-depth (truncation, an out-of-band edit, an `id`
/// reused with new content); a missed same-length edit only yields a
/// slightly stale *height* (never a render mismatch — the visible turns
/// render from the real parts; the cache only feeds the windowing integer)
/// and self-corrects on the next [`sync`](ConversationCache::sync).
#[must_use]
fn fingerprint(message: &UiMessage) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (match message.role {
        Role::System => 0u8,
        Role::User => 1,
        Role::Assistant => 2,
    })
    .hash(&mut h);
    message.parts.len().hash(&mut h);
    for part in &message.parts {
        part_tag(part).hash(&mut h);
        part.as_text().len().hash(&mut h);
    }
    h.finish()
}

/// A stable small tag per [`UiPart`] kind (the discriminant, by hand so it
/// does not depend on `Discriminant: Hash` across toolchains).
#[must_use]
fn part_tag(part: &UiPart) -> u8 {
    match part {
        UiPart::Text { .. } => 0,
        UiPart::Reasoning { .. } => 1,
        UiPart::Tool(_) => 2,
        UiPart::SourceUrl { .. } => 3,
        UiPart::SourceDocument { .. } => 4,
        UiPart::File { .. } => 5,
        UiPart::StepStart => 6,
        UiPart::Data { .. } => 7,
        UiPart::Unknown(_) => 8,
    }
}

/// `Message::height`'s exact formula, factored so a cache miss and
/// [`sync`](ConversationCache::sync) compute byte-identical heights.
#[must_use]
pub(crate) fn measure_height(message: &UiMessage, width: u16) -> u16 {
    if width == 0 {
        return 1;
    }
    let body = message_body_markdown(message);
    let body_rows = Markdown::new(body).lines(width).len();
    u16::try_from(1 + body_rows).unwrap_or(u16::MAX)
}

/// The caller-owned per-message height memo a
/// [`Conversation`](crate::conversation::Conversation) /
/// [`Message`](crate::message::Message) optionally projects (the UI-1/MD-1
/// model). Owned by the app's model like a
/// [`ScrollState`](rstui_core::ScrollState); the reducer drives it with
/// [`sync`](Self::sync) in `update`, the widget reads it in `view`.
///
/// # Example
///
/// ```
/// use rstui_core::scroll::ScrollState;
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_ai::conversation::Conversation;
/// use rstui_ai::conversation_cache::ConversationCache;
/// use rstui_ai::model::UiMessage;
/// use serde_json::json;
///
/// let msgs: Vec<UiMessage> = (0..3)
///     .map(|i| UiMessage::from_value(&json!({
///         "id": format!("m{i}"), "role": "assistant",
///         "parts": [{ "type": "text", "text": "hello" }]
///     })))
///     .collect();
///
/// // Owned by the model; the reducer calls this once per `update`.
/// let mut cache = ConversationCache::new();
/// let content_width = 40;
/// cache.sync(&msgs, content_width);
///
/// // The widget only reads it (byte-identical to no cache).
/// let scroll = ScrollState::new();
/// let mut buf = Buffer::empty(Rect::new(0, 0, content_width, 8));
/// Conversation::new(&msgs, &scroll)
///     .cache(&cache)
///     .render(buf.area(), &mut buf);
/// ```
#[derive(Debug, Default, Clone)]
pub struct ConversationCache {
    /// `id` → its memoized measurement. `HashMap` so the per-frame
    /// per-message read the widget does is O(1), not O(history).
    slots: HashMap<String, Slot>,
}

impl ConversationCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-measure the cacheable (non-last, stable-id) messages at
    /// `width`, **once per `update`** (the acp-client `refresh_md_cache`
    /// contract). The last message is left uncached (it streams), so a
    /// cached render is byte-identical to an uncached one. Slots whose id
    /// is no longer a non-last message are dropped (a history trim or an
    /// edit), so the cache never unboundedly grows or serves a stale id.
    pub fn sync(&mut self, messages: &[UiMessage], width: u16) {
        let n = messages.len();
        let last_cacheable = n.saturating_sub(1); // [0, n-1): every non-last

        // Drop slots for ids that are no longer a cacheable message.
        let live: std::collections::HashSet<&str> = messages[..last_cacheable]
            .iter()
            .map(|m| m.id.as_str())
            .filter(|s| !s.is_empty())
            .collect();
        self.slots.retain(|id, _| live.contains(id.as_str()));

        for message in &messages[..last_cacheable] {
            if message.id.is_empty() {
                continue; // no stable key — always measured fresh
            }
            let fp = fingerprint(message);
            let fresh = self
                .slots
                .get(&message.id)
                .is_some_and(|s| s.fp == fp && s.width == width);
            if !fresh {
                let height = measure_height(message, width);
                self.slots
                    .insert(message.id.clone(), Slot { fp, width, height });
            }
        }
    }

    /// The memoized `Message::height(width)` for `message`, or `None` for
    /// a miss (uncached id — empty / the streaming last turn / evicted —
    /// or a width/fingerprint change). A miss tells the widget to measure
    /// fresh, which yields the same number (the cache stores exactly
    /// `Message::height`'s formula, so a hit and a miss never disagree).
    #[must_use]
    pub fn height(&self, message: &UiMessage, width: u16) -> Option<u16> {
        if message.id.is_empty() {
            return None;
        }
        let s = self.slots.get(&message.id)?;
        (s.width == width && s.fp == fingerprint(message)).then_some(s.height)
    }

    /// How many messages are currently memoized (the cacheable history
    /// size — every non-last stable-id turn). For tests / introspection.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether nothing is memoized yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use serde_json::json;

    fn msg(id: &str, role: &str, body: &str) -> UiMessage {
        UiMessage::from_value(&json!({
            "id": id, "role": role,
            "parts": [{ "type": "text", "text": body }]
        }))
    }

    /// The gate test (the acp-client
    /// `agent_md_cache_equals_a_fresh_parse_for_every_non_last_entry`
    /// analog): after a `sync`, every non-last message's cached height is
    /// **exactly** a fresh `Message::height`, and the last/streaming
    /// message is never cached.
    #[test]
    fn cache_height_equals_a_fresh_measure_for_every_non_last_message() {
        let msgs = vec![
            msg(
                "a",
                "user",
                "# Heading\n\nA **bold** word and a [link](http://e.com).\n",
            ),
            msg(
                "b",
                "assistant",
                "para one\n\npara two with more words to wrap around\n",
            ),
            msg("c", "user", "- alpha\n- beta\n- gamma\n"),
            msg("d", "assistant", "the streaming last turn, never cached"),
        ];
        let width = 24;
        let mut cache = ConversationCache::new();
        cache.sync(&msgs, width);

        let n = msgs.len();
        for (i, m) in msgs.iter().enumerate() {
            let fresh = Message::new(m).height(width); // no cache attached
            if i + 1 == n {
                assert_eq!(cache.height(m, width), None, "last turn must be uncached");
            } else {
                assert_eq!(
                    cache.height(m, width),
                    Some(fresh),
                    "non-last message {i} cache must equal a fresh measure"
                );
            }
        }
        // Three non-last cacheable turns memoized.
        assert_eq!(cache.len(), n - 1);
    }

    #[test]
    fn a_width_change_invalidates_and_re_measures() {
        let msgs = vec![
            msg(
                "a",
                "assistant",
                "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj",
            ),
            msg("b", "user", "tail"),
        ];
        let mut cache = ConversationCache::new();
        cache.sync(&msgs, 12);
        let narrow = cache.height(&msgs[0], 12);
        // Stale width is a miss (not a wrong number).
        assert_eq!(cache.height(&msgs[0], 40), None);
        cache.sync(&msgs, 40);
        let wide = cache.height(&msgs[0], 40);
        assert_eq!(narrow, Some(Message::new(&msgs[0]).height(12)));
        assert_eq!(wide, Some(Message::new(&msgs[0]).height(40)));
        assert!(narrow.unwrap() >= wide.unwrap(), "narrower wraps taller");
    }

    #[test]
    fn an_empty_id_is_never_cached() {
        let msgs = vec![msg("", "assistant", "no id here"), msg("z", "user", "tail")];
        let mut cache = ConversationCache::new();
        cache.sync(&msgs, 20);
        assert_eq!(cache.height(&msgs[0], 20), None);
        assert!(cache.is_empty(), "no stable key → nothing memoized");
    }

    #[test]
    fn a_history_trim_evicts_dropped_ids() {
        let mut msgs: Vec<_> = (0..5)
            .map(|i| msg(&format!("m{i}"), "assistant", "some body text here"))
            .collect();
        let mut cache = ConversationCache::new();
        cache.sync(&msgs, 30);
        assert_eq!(cache.len(), 4); // m0..m3 (m4 is the last)
        // Drain the oldest two (a cap_transcript-style front trim).
        msgs.drain(0..2);
        cache.sync(&msgs, 30);
        // Only m2, m3 remain cacheable (m4 still last); m0/m1 evicted.
        assert_eq!(cache.len(), 2);
        assert_eq!(
            cache.height(&msgs[0], 30),
            Some(Message::new(&msgs[0]).height(30))
        );
    }

    #[test]
    fn a_changed_fingerprint_is_a_miss_until_resynced() {
        let mut msgs = vec![
            msg("a", "assistant", "original"),
            msg("b", "user", "tail one"),
        ];
        let mut cache = ConversationCache::new();
        cache.sync(&msgs, 20);
        assert!(cache.height(&msgs[0], 20).is_some());
        // An out-of-band edit to a non-last message (rare; defense).
        msgs[0] = msg("a", "assistant", "a much longer replacement body now");
        assert_eq!(
            cache.height(&msgs[0], 20),
            None,
            "stale fp → miss, not wrong"
        );
        cache.sync(&msgs, 20);
        assert_eq!(
            cache.height(&msgs[0], 20),
            Some(Message::new(&msgs[0]).height(20))
        );
    }
}
