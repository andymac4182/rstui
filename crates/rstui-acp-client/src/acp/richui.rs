//! Detecting and rendering agent-authored declarative UI (A2UI /
//! json-render) inside the transcript.
//!
//! The client advertises (in the ACP `initialize` client capabilities,
//! see [`render_capability_meta`]) that it can render A2UI / json-render
//! documents and the Mermaid / Structurizr-C4 / JSON-Canvas diagram
//! DSLs. When an agent sends one — a self-contained JSON document, or a
//! fenced ` ```a2ui ` / ` ```json-render ` / ` ```mermaid ` /
//! ` ```structurizr ` / ` ```canvas ` block — [`detect`] classifies it
//! and [`render_lines`] projects it inline through `rstui-jsonui`
//! (declarative UI) or the same `rstui-widgets` diagram widgets the
//! kitchen-sink Rich Text screen uses, into transcript [`Line`]s.
//!
//! Detection is **conservative and total** and never panics. A real
//! agent streams the document wrapped in prose across many
//! `agent_message_chunk`s, so per-chunk [`detect`] (each chunk an
//! incomplete fragment) cannot see it; the reducer instead runs
//! [`split_message`] on the **assembled** message at turn end, finding
//! an embedded fenced ` ```json-render ` / ` ```a2ui ` block inside
//! ordinary prose and splitting it into `[prose] [rendered UI]
//! [prose]`. A message with no document is left as normal text.
//! Rendering is
//! a pure projection (ADR 0012): the parsed document is re-derived from
//! the stored source every frame — there is no retained UI tree — so it
//! composes with the existing immediate-mode transcript with no new
//! lifecycle.

use rstui_core::{Buffer, Line, Position, Rect, Span, Style, Widget};
use rstui_jsonui::a2ui::{A2uiClientAction, A2uiSurface};
use rstui_jsonui::jsonrender::{ActionEffect, JsonRenderDoc};
use rstui_jsonui::tree::HitMap;
use rstui_widgets::{JsonCanvas, Mermaid, Structurizr};
use serde_json::{Map, Value, json};

/// Which renderable format an agent block is. The two declarative-UI
/// engines, plus the three diagram DSLs the client already advertises
/// ([`diagram_capability`](rstui_jsonui::capability::diagram_capability))
/// — all rendered inline in the transcript through the same widgets the
/// kitchen sink uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RichUiFormat {
    /// Google A2UI (a server→client envelope or a JSONL stream of them).
    A2ui,
    /// Vercel json-render (a flat `{root,elements,state}` spec).
    JsonRender,
    /// A Mermaid diagram (any type) → [`rstui_widgets::Mermaid`].
    Mermaid,
    /// A Structurizr DSL / C4 workspace → [`rstui_widgets::Structurizr`].
    Structurizr,
    /// A JSON Canvas document → [`rstui_widgets::JsonCanvas`].
    JsonCanvas,
}

/// A detected agent-authored UI document: its [`RichUiFormat`] and the
/// raw source (kept verbatim so the renderer can re-project it every
/// frame — pure projection, no retained tree).
#[derive(Debug, Clone, PartialEq)]
pub struct RichUiPayload {
    /// The detected format.
    pub format: RichUiFormat,
    /// The verbatim document source (JSON object / JSONL stream).
    pub source: String,
}

/// The recognised fenced-block info tags that carry a UI document.
fn fence_format(tag: &str) -> Option<RichUiFormat> {
    match tag.trim().to_ascii_lowercase().as_str() {
        "a2ui" => Some(RichUiFormat::A2ui),
        "json-render" | "jsonrender" | "jsonui" | "spec" => Some(RichUiFormat::JsonRender),
        "mermaid" => Some(RichUiFormat::Mermaid),
        "structurizr" | "c4" => Some(RichUiFormat::Structurizr),
        "canvas" | "jsoncanvas" | "json-canvas" => Some(RichUiFormat::JsonCanvas),
        _ => None,
    }
}

/// Finds the **first** fenced ` ```<tag> … ``` ` UI-document block
/// embedded anywhere in `text`, returning `(format, body, fence-byte-
/// range)`. A real agent wraps the block in prose and streams it
/// token-by-token (e.g. `"Here is your dashboard:\n```json-render\n…\n```"`),
/// so requiring the *whole* message to be exactly one fence — the
/// original bug — meant this never fired in practice. Non-UI fences
/// (` ```rust ` …) are skipped, not matched. ASCII anchors only, so
/// byte slicing stays on char boundaries; total.
fn find_fenced_doc(text: &str) -> Option<(RichUiFormat, String, std::ops::Range<usize>)> {
    let mut search = 0;
    while let Some(rel) = text[search..].find("```") {
        let open = search + rel;
        let after_ticks = open + 3;
        let line_end = after_ticks + text[after_ticks..].find('\n')?;
        let body_start = line_end + 1;
        let close = body_start + text[body_start..].find("```")?;
        if let Some(format) = fence_format(&text[after_ticks..line_end]) {
            return Some((
                format,
                text[body_start..close].to_owned(),
                open..(close + 3).min(text.len()),
            ));
        }
        search = close + 3; // skip this whole (non-UI) fence, keep scanning
    }
    None
}

/// Classifies a content block as an A2UI / json-render document, or
/// `None` for ordinary prose. **Total** — any parse failure (e.g. a
/// partial streamed chunk) is simply `None`, so normal text is
/// unaffected.
#[must_use]
pub fn detect(text: &str) -> Option<RichUiPayload> {
    // An explicit fenced block is the unambiguous signal — found even
    // when the agent wrapped it in prose.
    if let Some((format, body, _)) = find_fenced_doc(text) {
        return Some(RichUiPayload {
            format,
            source: body,
        });
    }

    let trimmed = text.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }

    // An A2UI envelope: `version` + one of the six message keys.
    let is_a2ui_envelope = |entry: &Value| {
        entry.get("version").and_then(Value::as_str).is_some()
            && [
                "createSurface",
                "updateComponents",
                "updateDataModel",
                "deleteSurface",
                "callFunction",
                "actionResponse",
            ]
            .iter()
            .any(|key| entry.get(*key).is_some())
    };

    // Single JSON value: a json-render flat spec, or a one-shot A2UI
    // object / array / `{messages:[…]}` wrapper.
    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
        let a2ui = match &parsed {
            Value::Array(items) => items.iter().any(&is_a2ui_envelope),
            Value::Object(map) => {
                is_a2ui_envelope(&parsed)
                    || map
                        .get("messages")
                        .and_then(Value::as_array)
                        .is_some_and(|items| items.iter().any(&is_a2ui_envelope))
            }
            _ => false,
        };
        if a2ui {
            return Some(RichUiPayload {
                format: RichUiFormat::A2ui,
                source: trimmed.to_owned(),
            });
        }
        // json-render: a flat spec — a `root` string + `elements` object.
        if parsed.get("root").and_then(Value::as_str).is_some()
            && parsed.get("elements").map(Value::is_object) == Some(true)
        {
            return Some(RichUiPayload {
                format: RichUiFormat::JsonRender,
                source: trimmed.to_owned(),
            });
        }
        return None;
    }

    // Not a single value — A2UI is **JSONL** (one server→client envelope
    // per line). `serde_json::from_str` rejects multi-object input, so a
    // raw/fence-stripped A2UI stream was never detected and fell back to
    // raw text. Recognise it line-by-line; `A2uiSurface::apply_stream`
    // consumes the whole stream.
    let is_a2ui_stream = trimmed.lines().any(|line| {
        serde_json::from_str::<Value>(line.trim())
            .ok()
            .as_ref()
            .is_some_and(&is_a2ui_envelope)
    });
    is_a2ui_stream.then(|| RichUiPayload {
        format: RichUiFormat::A2ui,
        source: trimmed.to_owned(),
    })
}

/// Splits an **assembled** (post-stream) agent message into
/// `(before, payload, after)` — the prose before the embedded UI
/// document, the document, and the prose after — or `None` when there
/// is no document.
///
/// This is what the reducer runs at *turn end* on the full assembled
/// agent message. Per-chunk `detect` cannot see a streamed document
/// (a real agent emits `"Here is your dashboard:\n```json-render\n…"`
/// across many `agent_message_chunk`s, each an incomplete fragment), so
/// the streamed reply rendered as raw text. Scanning the assembled
/// message for an embedded fence — keeping the surrounding prose —
/// fixes that. A whole-message bare doc (single-shot, no prose) maps to
/// `("", payload, "")`.
#[must_use]
pub fn split_message(text: &str) -> Option<(String, RichUiPayload, String)> {
    if let Some((format, body, range)) = find_fenced_doc(text) {
        return Some((
            text[..range.start].trim().to_owned(),
            RichUiPayload {
                format,
                source: body,
            },
            text[range.end..].trim().to_owned(),
        ));
    }
    detect(text).map(|payload| (String::new(), payload, String::new()))
}

/// One ordered piece of an assembled agent message.
#[derive(Debug, Clone, PartialEq)]
pub enum MessageSegment {
    /// Markdown prose — rendered with the `Markdown` widget, exactly as
    /// any ordinary agent reply is.
    Prose(String),
    /// An extracted renderable block — rendered inline as the live UI /
    /// diagram (json-render, A2UI, Mermaid, Structurizr, JSON Canvas).
    Rich(RichUiPayload),
}

/// Splits an assembled agent message into its ordered segments —
/// markdown prose interleaved with **every** embedded fenced UI/diagram
/// block (and a whole-message bare A2UI/json-render document). This is
/// the answer to "use markdown for the message **and** turn the embedded
/// json-render / A2UI / diagram into a UI": one pass over the assembled
/// message, any number of blocks, the prose between them preserved as
/// markdown. Total; a message with no blocks is a single
/// [`Prose`](MessageSegment::Prose) (the caller then leaves it as a
/// normal markdown agent entry).
#[must_use]
pub fn segments(text: &str) -> Vec<MessageSegment> {
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        if let Some((format, body, range)) = find_fenced_doc(rest) {
            let before = rest[..range.start].trim();
            if !before.is_empty() {
                out.push(MessageSegment::Prose(before.to_owned()));
            }
            out.push(MessageSegment::Rich(RichUiPayload {
                format,
                source: body,
            }));
            rest = &rest[range.end..];
            continue;
        }
        let tail = rest.trim();
        if !tail.is_empty() {
            // No more fences — a whole-remaining *bare* A2UI/json-render
            // doc still becomes a UI; otherwise it is markdown prose.
            match detect(tail) {
                Some(payload) => out.push(MessageSegment::Rich(payload)),
                None => out.push(MessageSegment::Prose(tail.to_owned())),
            }
        }
        return out;
    }
}

/// Projects a detected payload to transcript [`Line`]s, `width` columns
/// wide and at most `max_height` rows (it renders the document into a
/// scratch [`Buffer`] then converts the painted rows — the same
/// embed-a-widget-in-a-line-view technique the streaming-markdown view
/// uses for diagrams). Always total: a malformed document degrades to
/// the engine's own placeholder, never a panic.
#[must_use]
/// Paints `payload` into a scratch [`Buffer`] (the exact projection the
/// transcript draws) and returns it with the interactive-node
/// [`HitMap`]. `None` only for an unparseable json-render document.
/// Shared by [`render_lines`] (ignores the hit map) and [`click`] (uses
/// it) so what is drawn and what a click resolves to cannot drift.
fn paint(payload: &RichUiPayload, width: u16, max_height: u16) -> Option<(Buffer, HitMap)> {
    let width = width.max(1);
    let cap = max_height.max(1);
    let mut hits = HitMap::new();
    let scratch = match payload.format {
        RichUiFormat::A2ui | RichUiFormat::JsonRender => {
            let node = if payload.format == RichUiFormat::A2ui {
                let mut surface = A2uiSurface::new();
                surface.apply_stream(&payload.source);
                surface.project()
            } else {
                let spec = serde_json::from_str::<Value>(&payload.source).ok()?;
                JsonRenderDoc::from_flat_value(&spec).view()
            };
            // Size the scratch to the document's *content* height
            // (clamped by the caller's cap), not the cap itself —
            // otherwise a bordered container expands to fill
            // `max_height` and, with the transcript's sticky-bottom
            // autoscroll, the content scrolls out of view.
            let height = node.measure_height(width).clamp(1, cap);
            let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
            node.render(buffer.area(), &mut buffer, &mut hits);
            buffer
        }
        // The diagram DSLs: render the *same* widget the kitchen-sink
        // Rich Text screen uses, into a capped scratch (trailing blank
        // rows are trimmed below, so a small diagram stays small). Each
        // widget is total — invalid/streaming-truncated source degrades
        // to its own placeholder, never a panic. Diagrams are static
        // (no hit map).
        RichUiFormat::Mermaid => {
            let mut buffer = Buffer::empty(Rect::new(0, 0, width, cap));
            Mermaid::new(payload.source.as_str()).render(buffer.area(), &mut buffer);
            buffer
        }
        RichUiFormat::Structurizr => {
            let mut buffer = Buffer::empty(Rect::new(0, 0, width, cap));
            Structurizr::new(payload.source.as_str()).render(buffer.area(), &mut buffer);
            buffer
        }
        RichUiFormat::JsonCanvas => {
            let mut buffer = Buffer::empty(Rect::new(0, 0, width, cap));
            JsonCanvas::new(payload.source.as_str()).render(buffer.area(), &mut buffer);
            buffer
        }
    };
    Some((scratch, hits))
}

pub fn render_lines(payload: &RichUiPayload, width: u16, max_height: u16) -> Vec<Line<'static>> {
    let width = width.max(1);
    let Some((scratch, _hits)) = paint(payload, width, max_height) else {
        return vec![Line::raw("[invalid json-render document]")];
    };

    // Convert painted rows to owned Lines, trimming the trailing blank
    // rows so an over-tall scratch does not pad the transcript.
    let height = scratch.area().height;
    let mut rows: Vec<Line<'static>> = Vec::new();
    for y in 0..height {
        let mut spans: Vec<Span<'static>> = vec![Span::raw("  ")];
        let mut text = String::new();
        for x in 0..width {
            text.push(
                scratch
                    .get(rstui_core::Position::new(x, y))
                    .map_or(' ', |cell| cell.symbol),
            );
        }
        spans.push(Span::styled(text.trim_end().to_owned(), Style::new()));
        rows.push(Line::from(spans));
    }
    while rows
        .last()
        .is_some_and(|line| line.spans.iter().all(|s| s.content.trim().is_empty()))
    {
        rows.pop();
    }
    rows
}

/// Renders a stored rich-UI document source (a `Role::RichUi` entry's
/// `text`) to transcript [`Line`]s, re-detecting the format each frame
/// (pure projection — no retained UI tree). Falls back to showing the
/// raw source if it no longer detects (it always will, by construction).
#[must_use]
pub fn render_source(source: &str, width: u16, max_height: u16) -> Vec<Line<'static>> {
    match detect(source) {
        Some(payload) => render_lines(&payload, width, max_height),
        None => source
            .lines()
            .map(|raw| Line::from(vec![Span::raw("  "), Span::raw(raw.to_owned())]))
            .collect(),
    }
}

/// What a click on a rendered block resolves to — the reducer performs
/// it (no callback; ADR 0012 §P1).
#[derive(Debug, Clone, PartialEq)]
pub enum RichAction {
    /// Send this text to the agent as a turn: an A2UI client-action
    /// envelope (`{"version":…,"action":{name,context,…}}`) or a
    /// json-render custom-action `{action,params}` — so the agent can
    /// react and stream the next surface.
    ToAgent(String),
    /// Open this URL (an A2UI `openUrl` function-call / a `Link`).
    OpenUrl(String),
    /// A local-only effect (a two-way input / `setState`); carries a
    /// short human description for a transcript breadcrumb. Persisting
    /// local UI state across redraws is a separate follow-up; the agent
    /// round-trip — the common, asked-for case — is unaffected.
    Local(String),
}

/// Resolves a click at **block-local** `pos` on the rendered `source`
/// to a [`RichAction`], or `None` when the click is not on an
/// interactive node. Re-derives the exact projection the transcript
/// drew (via `paint`) so the hit map matches pixel-for-pixel. The
/// A2UI client-action envelope is stamped with the surface's own id
/// (parsed from the document) and `timestamp`.
#[must_use]
pub fn click(
    source: &str,
    width: u16,
    max_height: u16,
    pos: Position,
    timestamp: &str,
) -> Option<RichAction> {
    let payload = detect(source)?;
    let (_, hits) = paint(&payload, width.max(1), max_height)?;
    let node_id = hits.at(pos)?.to_owned();
    match payload.format {
        RichUiFormat::A2ui => {
            let mut surface = A2uiSurface::new();
            surface.apply_stream(&payload.source);
            let surface_id = surface.surface_id().unwrap_or_default().to_owned();
            match surface.action_for(&node_id)? {
                A2uiClientAction::OpenUrl(url) => Some(RichAction::OpenUrl(url)),
                A2uiClientAction::SetData { pointer, .. } => {
                    Some(RichAction::Local(format!("set {pointer}")))
                }
                // The server `event` variant — the agent round-trip.
                event => event
                    .to_client_json(&surface_id, timestamp)
                    .map(|value| RichAction::ToAgent(value.to_string())),
            }
        }
        RichUiFormat::JsonRender => {
            let spec = serde_json::from_str::<Value>(&payload.source).ok()?;
            let mut doc = JsonRenderDoc::from_flat_value(&spec);
            let mut to_agent: Option<String> = None;
            let mut local: Option<String> = None;
            for effect in doc.dispatch(&node_id, "press") {
                match effect {
                    ActionEffect::Unhandled(action) => {
                        to_agent = Some(
                            json!({ "action": action.action, "params": action.params }).to_string(),
                        );
                    }
                    ActionEffect::StateChanged => {
                        local = local.or_else(|| Some("updated".to_owned()));
                    }
                    ActionEffect::Log(message) => local = Some(message),
                    ActionEffect::Exit(_) => {}
                }
            }
            to_agent
                .map(RichAction::ToAgent)
                .or(local.map(RichAction::Local))
        }
        // Diagrams (Mermaid/Structurizr/JSON-Canvas) are static.
        RichUiFormat::Mermaid | RichUiFormat::Structurizr | RichUiFormat::JsonCanvas => None,
    }
}

/// The client-capability metadata advertising that this terminal client
/// can render A2UI + json-render, to attach to the ACP `initialize`
/// request's client capabilities (`_meta`). It carries the standard
/// A2UI `a2uiClientCapabilities` (`supportedCatalogIds`) so an
/// A2UI-aware agent negotiates the basic catalog, plus a human/agent
/// readable summary of both formats.
#[must_use]
pub fn render_capability_meta() -> Map<String, Value> {
    let mut meta = Map::new();
    meta.insert(
        "a2uiClientCapabilities".to_owned(),
        rstui_jsonui::capability::client_capabilities(),
    );
    if let Value::Object(summary) = rstui_jsonui::capability::render_capability_summary() {
        for (key, value) in summary {
            meta.insert(key, value);
        }
    }
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a2ui_envelope_and_json_render_spec_but_not_prose() {
        let a2ui = r#"{"version":"v0.10","createSurface":{"surfaceId":"s","catalogId":"c"}}"#;
        assert_eq!(detect(a2ui).map(|p| p.format), Some(RichUiFormat::A2ui));

        let spec = r#"{"root":"a","elements":{"a":{"type":"Text","props":{"text":"hi"}}}}"#;
        assert_eq!(
            detect(spec).map(|p| p.format),
            Some(RichUiFormat::JsonRender)
        );

        assert!(detect("Just a normal answer from the agent.").is_none());
        assert!(detect("{ not valid json").is_none()); // partial stream → text
        assert!(detect("{\"hello\":\"world\"}").is_none()); // JSON, but not a UI doc

        let fenced = "```json-render\n{\"root\":\"a\",\"elements\":{}}\n```";
        assert_eq!(
            detect(fenced).map(|p| p.format),
            Some(RichUiFormat::JsonRender)
        );
    }

    #[test]
    fn split_message_extracts_an_embedded_fenced_doc_from_prose() {
        // What a real agent actually sends: prose, the fenced doc, prose.
        let msg = "Here is your dashboard:\n\n\
                   ```json-render\n\
                   {\"root\":\"a\",\"elements\":{\"a\":{\"type\":\"Text\",\
                   \"props\":{\"text\":\"hi\"}}}}\n\
                   ```\n\nHope that helps!";
        let (before, payload, after) = split_message(msg).expect("embedded doc found");
        assert_eq!(before, "Here is your dashboard:");
        assert_eq!(after, "Hope that helps!");
        assert_eq!(payload.format, RichUiFormat::JsonRender);
        assert!(payload.source.contains("\"root\":\"a\""));
        // The old whole-string-only `fenced_block` would have missed this.

        // A non-UI fence is skipped; a later UI fence still matches.
        let mixed = "```rust\nfn main() {}\n```\nand:\n```a2ui\n\
                     {\"version\":\"v0.10\",\"createSurface\":{}}\n```";
        let (_, p, _) = split_message(mixed).expect("a2ui fence after a rust fence");
        assert_eq!(p.format, RichUiFormat::A2ui);

        // A whole-message bare doc (single-shot, no prose) → no prose.
        let bare = r#"{"root":"a","elements":{}}"#;
        let (b, _, a) = split_message(bare).expect("bare doc");
        assert!(b.is_empty() && a.is_empty());

        // Prose with no document is left alone.
        assert!(split_message("just a normal answer, no UI here").is_none());
    }

    #[test]
    fn renders_total_and_capability_meta_carries_catalog() {
        let payload = RichUiPayload {
            format: RichUiFormat::JsonRender,
            source: r#"{"root":"t","elements":{"t":{"type":"Text","props":{"text":"Hello"}}}}"#
                .to_owned(),
        };
        let lines = render_lines(&payload, 20, 6);
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(joined.contains("Hello"));

        // A malformed doc is total (no panic), and 0-size is total.
        let bad = RichUiPayload {
            format: RichUiFormat::A2ui,
            source: "{ broken".to_owned(),
        };
        let _ = render_lines(&bad, 0, 0);

        let meta = render_capability_meta();
        assert!(meta.contains_key("a2uiClientCapabilities"));
    }

    #[test]
    fn diagram_fences_are_recognised_and_rendered_inline() {
        for (tag, want) in [
            ("mermaid", RichUiFormat::Mermaid),
            ("structurizr", RichUiFormat::Structurizr),
            ("c4", RichUiFormat::Structurizr),
            ("canvas", RichUiFormat::JsonCanvas),
            ("json-canvas", RichUiFormat::JsonCanvas),
        ] {
            let msg = format!("see:\n```{tag}\nflowchart LR\n  A-->B\n```");
            assert_eq!(
                detect(&msg).map(|p| p.format),
                Some(want),
                "```{tag} is a recognised inline-renderable fence"
            );
        }
        // A Mermaid block renders as the diagram (its node label is
        // painted), not as raw fence text.
        let payload = RichUiPayload {
            format: RichUiFormat::Mermaid,
            source: "flowchart LR\n  Start-->Stop".to_owned(),
        };
        let text: String = render_lines(&payload, 60, 20)
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(
            text.contains("Start") && text.contains("Stop"),
            "the Mermaid diagram rendered (node labels painted): {text:?}"
        );
        assert!(!text.contains("```"), "no raw fence in the rendered output");
    }

    #[test]
    fn segments_split_a_message_into_prose_and_every_block_in_order() {
        let msg = "Intro prose.\n\n\
                   ```mermaid\nflowchart LR\n A-->B\n```\n\n\
                   Middle prose.\n\n\
                   ```json-render\n{\"root\":\"x\",\"elements\":{}}\n```\n\n\
                   Closing prose.";
        let segs = segments(msg);
        assert_eq!(
            segs.len(),
            5,
            "prose,mermaid,prose,json-render,prose: {segs:?}"
        );
        assert!(matches!(&segs[0], MessageSegment::Prose(p) if p == "Intro prose."));
        assert!(matches!(&segs[1], MessageSegment::Rich(p) if p.format == RichUiFormat::Mermaid));
        assert!(matches!(&segs[2], MessageSegment::Prose(p) if p == "Middle prose."));
        assert!(
            matches!(&segs[3], MessageSegment::Rich(p) if p.format == RichUiFormat::JsonRender)
        );
        assert!(matches!(&segs[4], MessageSegment::Prose(p) if p == "Closing prose."));

        // Prose-only → one Prose segment (caller leaves it markdown).
        let plain = segments("just a normal answer, no UI here");
        assert_eq!(plain.len(), 1);
        assert!(matches!(&plain[0], MessageSegment::Prose(_)));

        // Whole-message bare doc → a single Rich segment.
        let bare = segments(r#"{"root":"a","elements":{}}"#);
        assert!(matches!(bare.as_slice(), [MessageSegment::Rich(_)]));
    }

    #[test]
    fn click_resolves_a_button_to_an_agent_round_trip() {
        // An A2UI surface whose root is a Button with a server event.
        let a2ui = concat!(
            r#"{"version":"v0.10","createSurface":{"surfaceId":"s1","catalogId":"c"}}"#,
            "\n",
            r#"{"version":"v0.10","updateComponents":{"surfaceId":"s1","components":["#,
            r#"{"id":"root","component":"Button","child":"l","action":{"event":{"name":"signup"}}},"#,
            r#"{"id":"l","component":"Text","text":"Sign up"}"#,
            r#"]}}"#,
        );
        // A click on the button → the client→server `action` envelope.
        let hit =
            (0..6).find_map(|x| click(a2ui, 40, 12, Position::new(x, 0), "2026-05-19T00:00:00Z"));
        match hit {
            Some(RichAction::ToAgent(json)) => {
                assert!(json.contains("\"action\"") || json.contains("signup"));
                assert!(json.contains("signup"), "carries the event name: {json}");
                assert!(json.contains("s1"), "carries the surfaceId: {json}");
            }
            other => panic!("button click should round-trip to the agent, got {other:?}"),
        }
        // A click far outside any node resolves to nothing.
        assert!(click(a2ui, 40, 12, Position::new(39, 11), "t").is_none());

        // An A2UI link `openUrl` → OpenUrl (local, not the agent).
        let url_doc = concat!(
            r#"{"version":"v0.10","createSurface":{"surfaceId":"s","catalogId":"c"}}"#,
            "\n",
            r#"{"version":"v0.10","updateComponents":{"surfaceId":"s","components":["#,
            r#"{"id":"root","component":"Button","child":"l","action":{"functionCall":{"call":"openUrl","args":{"url":"https://example.com"}}}},"#,
            r#"{"id":"l","component":"Text","text":"Docs"}"#,
            r#"]}}"#,
        );
        let url_hit = (0..6).find_map(|x| click(url_doc, 40, 12, Position::new(x, 0), "t"));
        assert!(
            matches!(url_hit, Some(RichAction::OpenUrl(ref u)) if u.contains("example.com")),
            "openUrl resolves to OpenUrl, got {url_hit:?}"
        );

        // Prose / non-doc never resolves.
        assert!(click("just text", 40, 12, Position::new(0, 0), "t").is_none());
    }
}
