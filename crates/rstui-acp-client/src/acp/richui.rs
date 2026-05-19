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
use rstui_jsonui::tree::{HitMap, UiNode};
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
/// Paints one projected [`UiNode`] into a scratch [`Buffer`] + its
/// [`HitMap`]. The scratch is sized to the node's *content* height
/// (clamped by `cap`), not `cap` itself — otherwise a bordered
/// container expands to fill the cap and, with the transcript's
/// sticky-bottom autoscroll, the content scrolls out of view. The one
/// place a node is rasterised, so render and hit-test cannot drift.
fn node_paint(node: &UiNode, width: u16, cap: u16) -> (Buffer, HitMap) {
    let height = node.measure_height(width).clamp(1, cap.max(1));
    let mut buffer = Buffer::empty(Rect::new(0, 0, width.max(1), height));
    let mut hits = HitMap::new();
    node.render(buffer.area(), &mut buffer, &mut hits);
    (buffer, hits)
}

/// Converts a painted scratch to indented transcript [`Line`]s,
/// trimming trailing blank rows so an over-tall scratch does not pad
/// the transcript. The `"  "` indent is why a click's local column is
/// `screen_x - inner.x - 2` (see `rich_hit`).
fn buffer_to_lines(scratch: &Buffer, width: u16) -> Vec<Line<'static>> {
    let mut rows: Vec<Line<'static>> = Vec::new();
    for y in 0..scratch.area().height {
        let mut spans: Vec<Span<'static>> = vec![Span::raw("  ")];
        let text: String = (0..width)
            .map(|x| {
                scratch
                    .get(Position::new(x, y))
                    .map_or(' ', |cell| cell.symbol)
            })
            .collect();
        spans.push(Span::styled(text.trim_end().to_owned(), Style::new()));
        rows.push(Line::from(spans));
    }
    while rows
        .last()
        .is_some_and(|line| line.spans.iter().all(|span| span.content.trim().is_empty()))
    {
        rows.pop();
    }
    rows
}

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
            let (buffer, node_hits) = node_paint(&node, width, cap);
            hits = node_hits;
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
    match paint(payload, width, max_height) {
        Some((scratch, _hits)) => buffer_to_lines(&scratch, width),
        None => vec![Line::raw("[invalid json-render document]")],
    }
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

/// A caller-owned, **stateful** rendered document (ADR 0012): the
/// reducer keeps one per interactive `Role::RichUi` entry so a clicked
/// checkbox / switched tab / `setState` **persists** across redraws —
/// `view` re-projects from it, `update` mutates it on a click. Diagrams
/// are static and are *not* stored as a `RichDoc` (rendered from text).
/// No `Debug` — `JsonRenderDoc` is not `Debug`, and `ChatApp` isn't
/// either, so none is required.
pub enum RichDoc {
    /// A live A2UI surface (its data model / selection persist).
    A2ui(A2uiSurface),
    /// A live json-render document (its state model persists).
    Json(JsonRenderDoc),
}

impl RichDoc {
    /// Builds a stateful doc for an interactive payload. `None` for a
    /// diagram (Mermaid/Structurizr/JSON-Canvas) — those are static and
    /// keep being rendered from their source text.
    #[must_use]
    pub fn build(payload: &RichUiPayload) -> Option<Self> {
        match payload.format {
            RichUiFormat::A2ui => {
                let mut surface = A2uiSurface::new();
                surface.apply_stream(&payload.source);
                Some(Self::A2ui(surface))
            }
            RichUiFormat::JsonRender => serde_json::from_str::<Value>(&payload.source)
                .ok()
                .map(|spec| Self::Json(JsonRenderDoc::from_flat_value(&spec))),
            RichUiFormat::Mermaid | RichUiFormat::Structurizr | RichUiFormat::JsonCanvas => None,
        }
    }

    /// The projected node (pure — re-derived from the owned, possibly
    /// mutated, state every call). Public so the interactive right pane
    /// can render it at the pane's own size and own the [`HitMap`].
    #[must_use]
    pub fn node(&self) -> UiNode {
        match self {
            Self::A2ui(surface) => surface.project(),
            Self::Json(doc) => doc.view(),
        }
    }

    /// Set the active theme-token colour palette (the client maps its
    /// live theme into it) so the doc's `"color"` props and chart
    /// series render in the user's theme. Pure projection reads it on
    /// every re-render — set at build and on a theme change.
    pub fn set_palette(&mut self, palette: rstui_jsonui::color::Palette) {
        match self {
            Self::A2ui(surface) => surface.set_palette(palette),
            Self::Json(doc) => doc.set_palette(palette),
        }
    }

    /// Close the interactive loop: fold an agent **follow-up** message
    /// into this *live* doc instead of spawning a new transcript entry,
    /// so the agent's response to a submitted action updates the open
    /// pane in place. Accepts only an A2UI update stream — one with no
    /// `createSurface` (an `updateComponents`/`updateDataModel`/
    /// `actionResponse`, the spec's response shape; `A2uiSurface`
    /// itself ignores any message not addressed to its surface).
    /// Returns `true` when it was absorbed (the caller then adds no new
    /// entry). A `createSurface` (a brand-new UI) or a json-render
    /// payload returns `false` — a fresh doc.
    #[must_use]
    pub fn merge_followup(&mut self, payload: &RichUiPayload) -> bool {
        match self {
            Self::A2ui(surface)
                if payload.format == RichUiFormat::A2ui
                    && !payload.source.contains("createSurface") =>
            {
                surface.apply_stream(&payload.source);
                true
            }
            _ => false,
        }
    }

    /// Re-project the owned (mutated) state to transcript lines — the
    /// renderer calls this every frame (pure projection).
    #[must_use]
    pub fn render_lines(&self, width: u16, max_height: u16) -> Vec<Line<'static>> {
        let width = width.max(1);
        let (scratch, _) = node_paint(&self.node(), width, max_height.max(1));
        buffer_to_lines(&scratch, width)
    }

    /// Render the doc into a `width`×`height` scratch [`Buffer`] at a
    /// pane-local origin, returning it with the interactive [`HitMap`].
    /// The interactive right pane blits the buffer into the frame and
    /// keeps the hit map for its focus ring / clicks (pure projection,
    /// pane-local coords — the caller offsets by the pane rect, exactly
    /// the `rich_hit` discipline; no retained tree, ADR 0012).
    #[must_use]
    pub fn render_pane(&self, width: u16, height: u16) -> (Buffer, HitMap) {
        let mut buffer = Buffer::empty(Rect::new(0, 0, width.max(1), height.max(1)));
        let mut hits = HitMap::new();
        self.node().render(buffer.area(), &mut buffer, &mut hits);
        (buffer, hits)
    }

    /// The current text of an editable field (`TextField`) by its
    /// projected node id, or `None` if that id is not a text field —
    /// the reducer reads this to append/erase a typed character.
    #[must_use]
    pub fn field_text(&self, node_id: &str) -> Option<String> {
        find_field(&self.node(), node_id).map(str::to_owned)
    }

    /// `true` when `node_id` is an editable text field (so the reducer
    /// routes a typed character into it instead of treating it as an
    /// activation key).
    #[must_use]
    pub fn is_text_field(&self, node_id: &str) -> bool {
        find_field(&self.node(), node_id).is_some()
    }

    /// Two-way write-back: store `text` as the field's bound value so the
    /// next projection shows it **and** a later submit's resolved
    /// `context`/params include it (the spec round-trip). A2UI resolves
    /// the component's `{path}` binding via
    /// [`A2uiSurface::text_binding`]; json-render's projected field id is
    /// itself the `$bindState` write-back pointer. Total: an unbound
    /// field is a no-op.
    pub fn set_field_text(&mut self, node_id: &str, text: &str) {
        match self {
            Self::A2ui(surface) => {
                if let Some(pointer) = surface.text_binding(node_id) {
                    surface
                        .model_mut()
                        .set(&pointer, Value::String(text.to_owned()));
                }
            }
            Self::Json(doc) => {
                doc.write_binding(node_id, Value::String(text.to_owned()));
            }
        }
    }

    /// Apply a click at pane-local `pos` (render at `width`×`height`,
    /// resolve the hit node, then [`act_node`](Self::act_node)).
    #[must_use]
    pub fn act(
        &mut self,
        width: u16,
        max_height: u16,
        pos: Position,
        timestamp: &str,
    ) -> Option<RichAction> {
        let (_, hits) = node_paint(&self.node(), width.max(1), max_height.max(1));
        let node_id = hits.at(pos)?.to_owned();
        self.act_node(&node_id, timestamp)
    }

    /// Activate an interactive node **by id** (a keyboard Enter/Space, or
    /// a resolved click): **mutate** the owned state for a local effect
    /// (a toggled checkbox / switched tab / `setState` persists and
    /// re-renders) and return what the reducer must still do (round-trip
    /// the spec envelope to the agent / open a URL). Total — an unknown
    /// or non-actionable id is `None`.
    #[must_use]
    pub fn act_node(&mut self, node_id: &str, timestamp: &str) -> Option<RichAction> {
        match self {
            Self::A2ui(surface) => {
                // Slider stepper `"<ptr>#slider:<±step>:<min>:<max>"`:
                // clamp `value ± step` and write it back two-way.
                if let Some((ptr, delta, min, max)) = parse_slider_id(node_id) {
                    let cur = surface
                        .model()
                        .get(ptr)
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    let next = (cur + delta).clamp(min, max);
                    surface.model_mut().set(ptr, json!(next));
                    return Some(RichAction::Local(format!("{ptr} = {next}")));
                }
                // A2UI `Tabs` header: `"<tabsId>#tab:<index>"` switches
                // the reducer-owned active tab (selection state, not the
                // data model) — the spec's tab model.
                if let Some((base, index)) = parse_tab_id(node_id) {
                    surface
                        .selection_mut()
                        .active_tab
                        .insert(base.to_owned(), index);
                    return Some(RichAction::Local(format!("tab {index}")));
                }
                let surface_id = surface.surface_id().unwrap_or_default().to_owned();
                match surface.action_for(node_id)? {
                    A2uiClientAction::OpenUrl(url) => Some(RichAction::OpenUrl(url)),
                    A2uiClientAction::SetData { pointer, value } => {
                        // Persisted — the next `project()` reflects it.
                        surface.model_mut().set(&pointer, value);
                        Some(RichAction::Local(format!("set {pointer}")))
                    }
                    event => event.to_client_json(&surface_id, timestamp).map(|value| {
                        RichAction::ToAgent(wrap_submission(SubmissionFormat::A2ui, &value))
                    }),
                }
            }
            Self::Json(doc) => {
                // Slider stepper: clamp + two-way write-back.
                if let Some((ptr, delta, min, max)) = parse_slider_id(node_id) {
                    let cur = doc.model().get(ptr).and_then(Value::as_f64).unwrap_or(0.0);
                    let next = (cur + delta).clamp(min, max);
                    doc.write_binding(ptr, json!(next));
                    return Some(RichAction::Local(format!("{ptr} = {next}")));
                }
                // A `Checkbox`/`Switch` is two-way (no `on.press`): its
                // id is the `$bindState` pointer — flip the bound bool.
                if let Some(checked) = find_checkbox(&doc.view(), node_id) {
                    doc.write_binding(node_id, Value::Bool(!checked));
                    return Some(RichAction::Local(format!("{node_id} = {}", !checked)));
                }
                // `dispatch` mutates the owned data model in place, so a
                // `setState`/`pushState` persists and re-renders.
                let mut to_agent = None;
                let mut local = None;
                for effect in doc.dispatch(&node_id.to_owned(), "press") {
                    match effect {
                        ActionEffect::Unhandled(action) => {
                            to_agent = Some(wrap_submission(
                                SubmissionFormat::JsonRender,
                                &json!({ "action": action.action, "params": action.params }),
                            ));
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
        }
    }
}

/// Which interactive format produced a submission — picks the agent-
/// facing label/instruction for the wrapped user message.
#[derive(Debug, Clone, Copy)]
enum SubmissionFormat {
    /// Google A2UI v0.10 client→server action envelope.
    A2ui,
    /// Vercel json-render host `{action,params}`.
    JsonRender,
}

impl SubmissionFormat {
    const fn label(self) -> &'static str {
        match self {
            Self::A2ui => "A2UI",
            Self::JsonRender => "json-render",
        }
    }
}

/// Wrap a resolved submission envelope into the **user message** the
/// agent receives when a rendered form is submitted. A bare JSON blob
/// in chat is opaque to an LLM; this marker + a brief instruction +
/// the pretty-printed JSON inside a ` ```json ` fence makes the
/// contract unambiguous (the [`json_render_prompt`](
/// rstui_jsonui::capability::json_render_prompt) and A2UI
/// `submissionConvention` in [`render_capability_summary`](
/// rstui_jsonui::capability::render_capability_summary) document the
/// same shape so a capability-aware agent recognises it).
fn wrap_submission(format: SubmissionFormat, envelope: &Value) -> String {
    let pretty = serde_json::to_string_pretty(envelope).unwrap_or_else(|_| envelope.to_string());
    format!(
        "[{label} form submission]\n\n\
         The user submitted the rendered {label} form. Process the action below and respond.\n\n\
         ```json\n{pretty}\n```",
        label = format.label(),
    )
}

/// Parses an A2UI tab-header id `"<tabsId>#tab:<index>"` into
/// `(tabsId, index)`. `None` for any other id (total).
fn parse_tab_id(node_id: &str) -> Option<(&str, usize)> {
    let (base, rest) = node_id.split_once("#tab:")?;
    Some((base, rest.parse::<usize>().ok()?))
}

/// Parses a slider-stepper id `"<ptr>#slider:<±step>:<min>:<max>"` into
/// `(ptr, delta, min, max)`. `None` for any other id (total) — the
/// shared format ([`rstui_jsonui::tree::slider_row`]) both A2UI and
/// json-render sliders emit.
fn parse_slider_id(node_id: &str) -> Option<(&str, f64, f64, f64)> {
    let (ptr, rest) = node_id.split_once("#slider:")?;
    let mut it = rest.split(':');
    let delta = it.next()?.parse::<f64>().ok()?;
    let min = it.next()?.parse::<f64>().ok()?;
    let max = it.next()?.parse::<f64>().ok()?;
    Some((ptr, delta, min, max))
}

/// Depth-first search for a `Checkbox` with `id`, returning its current
/// `checked`. Total over the projected tree (mirrors [`find_field`]);
/// used to two-way-toggle a json-render `Checkbox`/`Switch` (its id is
/// the `$bindState` pointer; it has no `on.press`).
fn find_checkbox(node: &UiNode, id: &str) -> Option<bool> {
    match node {
        UiNode::Checkbox {
            id: cid, checked, ..
        } if cid == id => Some(*checked),
        UiNode::Column { children, .. } | UiNode::Row { children, .. } => {
            children.iter().find_map(|child| find_checkbox(child, id))
        }
        UiNode::Stack(children) => children.iter().find_map(|child| find_checkbox(child, id)),
        UiNode::Card { child, .. } | UiNode::Scroll { child, .. } => find_checkbox(child, id),
        _ => None,
    }
}

/// Depth-first search for an editable `TextField` with `id`, returning
/// its current value. Total over the projected tree (the pure model —
/// no retained tree); used to route typed text to the focused field.
fn find_field<'tree>(node: &'tree UiNode, id: &str) -> Option<&'tree str> {
    match node {
        UiNode::TextField { id: fid, value, .. } if fid == id => Some(value.as_str()),
        UiNode::Column { children, .. } | UiNode::Row { children, .. } => {
            children.iter().find_map(|child| find_field(child, id))
        }
        UiNode::Stack(children) => children.iter().find_map(|child| find_field(child, id)),
        UiNode::Card { child, .. } | UiNode::Scroll { child, .. } => find_field(child, id),
        _ => None,
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

    /// Extract the spec envelope from a wrapped submission user
    /// message (the `[<fmt> form submission]` + fenced json + prose
    /// the agent receives — see `wrap_submission`). Tests need it to
    /// assert the underlying envelope still matches the spec shape.
    fn embedded_json(wrapped: &str) -> Value {
        let open = wrapped
            .find("```json\n")
            .map(|i| i + "```json\n".len())
            .expect("wrapped submission has a ```json fence");
        let close = wrapped[open..]
            .find("\n```")
            .map(|n| open + n)
            .expect("the ```json fence is closed");
        serde_json::from_str(&wrapped[open..close]).expect("the fenced payload is JSON")
    }

    fn row_of(doc: &RichDoc, needle: &str) -> Option<u16> {
        doc.render_lines(80, 40)
            .iter()
            .position(|line| {
                let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                text.contains(needle)
            })
            .map(|y| y as u16)
    }

    #[test]
    fn rich_doc_act_persists_a_json_render_setstate_across_reprojection() {
        // The Phase-2 unit contract: `act` mutates the *owned* doc, so a
        // later `render_lines` (the every-frame re-projection) shows the
        // new state — not a re-parse of the immutable source.
        let spec = r#"{"root":"col","elements":{
            "col":{"type":"Box","children":["status","btn"]},
            "status":{"type":"Text","props":{"text":{"$cond":{"$state":"/done"},"$then":"STATE=DONE","$else":"STATE=PENDING"}}},
            "btn":{"type":"ConfirmInput","props":{"message":"Mark?","yesLabel":"YesGo"},"on":{"confirm":{"action":"setState","params":{"statePath":"/done","value":true}}}}
        },"state":{"done":false}}"#;
        let payload = RichUiPayload {
            format: RichUiFormat::JsonRender,
            source: spec.to_owned(),
        };
        let mut doc = RichDoc::build(&payload).expect("json-render builds a stateful doc");
        assert!(row_of(&doc, "STATE=PENDING").is_some(), "starts PENDING");

        // The Yes button is on the row that holds the message; sweep it.
        let by = row_of(&doc, "YesGo").expect("the Yes button rendered");
        let acted = (0..80).any(|x| doc.act(80, 40, Position::new(x, by), "t").is_some());
        assert!(acted, "a click on the Yes button row resolved an action");

        // Re-project the *same owned doc*: the state persisted.
        assert!(
            row_of(&doc, "STATE=DONE").is_some() && row_of(&doc, "STATE=PENDING").is_none(),
            "the setState persisted across re-projection: {:?}",
            doc.render_lines(80, 40)
                .iter()
                .map(|l| l
                    .spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<String>())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a2ui_form_two_way_submit_builds_the_exact_spec_envelope() {
        // The hook contract: type into a bound A2UI TextField, press the
        // submit Button, and the agent receives the **spec** client→
        // server action envelope with the typed value resolved into the
        // event `context` (`{path}` → model value).
        let a2ui = concat!(
            r#"{"version":"v0.10","createSurface":{"surfaceId":"s1","catalogId":"c"}}"#,
            "\n",
            r#"{"version":"v0.10","updateComponents":{"surfaceId":"s1","components":["#,
            r#"{"id":"root","component":"Column","children":["name","submit"]},"#,
            r#"{"id":"name","component":"TextField","label":"Name","value":{"path":"/who"}},"#,
            r#"{"id":"submit","component":"Button","child":"sl","action":{"event":{"name":"save","context":{"who":{"path":"/who"}}}}},"#,
            r#"{"id":"sl","component":"Text","text":"Save"}"#,
            r#"]}}"#,
        );
        let mut doc = RichDoc::build(&RichUiPayload {
            format: RichUiFormat::A2ui,
            source: a2ui.to_owned(),
        })
        .expect("A2UI builds");

        assert!(doc.is_text_field("name"), "the bound TextField is editable");
        doc.set_field_text("name", "Ada");
        assert_eq!(
            doc.field_text("name").as_deref(),
            Some("Ada"),
            "two-way echo"
        );

        let action = doc
            .act_node("submit", "2026-05-19T00:00:00Z")
            .expect("submit resolves");
        let RichAction::ToAgent(json) = action else {
            panic!("submit must round-trip to the agent, got {action:?}");
        };
        let v: Value = embedded_json(&json);
        assert_eq!(v["version"], "v0.10");
        assert_eq!(v["action"]["name"], "save");
        assert_eq!(v["action"]["surfaceId"], "s1");
        assert_eq!(v["action"]["sourceComponentId"], "submit");
        assert_eq!(
            v["action"]["context"]["who"], "Ada",
            "the typed value is resolved into the event context (spec two-way): {json}"
        );
    }

    #[test]
    fn json_render_form_two_way_submit_sends_resolved_params() {
        // The same contract for json-render: a `$bindState` TextInput +
        // a host action whose params resolve the typed state, so the
        // agent receives `{action, params:{... typed ...}}`.
        let spec = r#"{"root":"form","elements":{
            "form":{"type":"Box","children":["who","go"]},
            "who":{"type":"TextInput","props":{"label":"Who","value":{"$bindState":"/who"}}},
            "go":{"type":"ConfirmInput","props":{"message":"Send?","yesLabel":"Send"},"on":{"confirm":{"action":"submitForm","params":{"who":{"$state":"/who"}}}}}
        },"state":{"who":""}}"#;
        let mut doc = RichDoc::build(&RichUiPayload {
            format: RichUiFormat::JsonRender,
            source: spec.to_owned(),
        })
        .expect("json-render builds");

        // The projected TextField id is the `$bindState` write-back
        // pointer ("/who"); typing writes it back through the model.
        assert!(doc.is_text_field("/who"), "the bound TextInput is editable");
        doc.set_field_text("/who", "Bob");
        assert_eq!(doc.field_text("/who").as_deref(), Some("Bob"));

        let action = doc
            .act_node("go#confirm", "t")
            .expect("the Send button resolves");
        let RichAction::ToAgent(json) = action else {
            panic!("a host action must round-trip, got {action:?}");
        };
        let v: Value = embedded_json(&json);
        assert_eq!(v["action"], "submitForm");
        assert_eq!(
            v["params"]["who"], "Bob",
            "the typed state is resolved into the action params: {json}"
        );
    }

    #[test]
    fn json_render_button_press_round_trips_to_the_agent() {
        // The reported gap: a json-render `Button` was advertised but
        // unimplemented (→ a dead `[unsupported: Button]`). It must
        // project to a real button and its `on.press` host action must
        // round-trip the spec `{action,params}` to the agent, with a
        // bound field's value resolved in.
        let spec = r#"{"root":"form","elements":{
            "form":{"type":"Box","children":["q","send"]},
            "q":{"type":"TextInput","props":{"label":"Q","value":{"$bindState":"/q"}}},
            "send":{"type":"Button","props":{"label":"Send","variant":"primary"},"on":{"press":{"action":"submitForm","params":{"q":{"$state":"/q"}}}}}
        },"state":{"q":""}}"#;
        let mut doc = RichDoc::build(&RichUiPayload {
            format: RichUiFormat::JsonRender,
            source: spec.to_owned(),
        })
        .expect("json-render builds");
        doc.set_field_text("/q", "hello");

        let action = doc
            .act_node("send", "t")
            .expect("the Button resolves an action");
        let RichAction::ToAgent(json) = action else {
            panic!("a json-render Button host action must round-trip, got {action:?}");
        };
        let v: Value = embedded_json(&json);
        assert_eq!(v["action"], "submitForm");
        assert_eq!(
            v["params"]["q"], "hello",
            "the bound field resolves into the submitted params: {json}"
        );

        // A Button whose press is a builtin stays LOCAL (spec: builtin
        // actions mutate local UI state, they don't message the agent).
        let local = r#"{"root":"b","elements":{"b":{"type":"Button","props":{"label":"+"},"on":{"press":{"action":"setState","params":{"statePath":"/n","value":1}}}}},"state":{"n":0}}"#;
        let mut d2 = RichDoc::build(&RichUiPayload {
            format: RichUiFormat::JsonRender,
            source: local.to_owned(),
        })
        .expect("builds");
        assert!(
            matches!(d2.act_node("b", "t"), Some(RichAction::Local(_))),
            "a builtin-action Button stays local, not a round-trip"
        );
    }

    /// The projected interactive node ids (the focus ring) — proves a
    /// form element is actually hit-testable, and lets a test find the
    /// real stepper ids without re-deriving the `#slider:` format.
    fn ring_ids(doc: &RichDoc) -> Vec<String> {
        let (_, hits) = doc.render_pane(80, 24);
        hits.entries().iter().map(|h| h.id.clone()).collect()
    }

    #[test]
    fn json_render_checkbox_and_slider_are_interactive_and_submit() {
        // Every form element must be usable + reach the agent: a
        // Checkbox toggles two-way, a Slider steps within bounds, and a
        // submit Button's params resolve the live state.
        let spec = r#"{"root":"f","elements":{
            "f":{"type":"Box","children":["a","q","go"]},
            "a":{"type":"Checkbox","props":{"label":"Agree","value":{"$bindState":"/agree"}}},
            "q":{"type":"Slider","props":{"label":"Qty","value":{"$bindState":"/qty"},"min":0,"max":10,"step":2}},
            "go":{"type":"Button","props":{"label":"Go"},"on":{"press":{"action":"submit","params":{"agree":{"$state":"/agree"},"qty":{"$state":"/qty"}}}}}
        },"state":{"agree":false,"qty":4}}"#;
        let mut doc = RichDoc::build(&RichUiPayload {
            format: RichUiFormat::JsonRender,
            source: spec.to_owned(),
        })
        .expect("builds");

        let ids = ring_ids(&doc);
        assert!(
            ids.iter().any(|i| i == "/agree"),
            "the Checkbox is hit-testable (focus ring): {ids:?}"
        );
        let inc = ids
            .iter()
            .find(|i| i.contains("#slider:") && !i.contains("#slider:-"))
            .cloned()
            .expect("the Slider [+] stepper is hit-testable");

        // Checkbox: false → true.
        assert!(matches!(
            doc.act_node("/agree", "t"),
            Some(RichAction::Local(_))
        ));
        // Slider: 4 → 6 → 8 → 10 → 10 (clamped at max).
        for _ in 0..4 {
            let _ = doc.act_node(&inc, "t");
        }

        let RichAction::ToAgent(json) = doc.act_node("go", "t").expect("submit") else {
            panic!("submit must round-trip");
        };
        let v: Value = embedded_json(&json);
        assert_eq!(v["params"]["agree"], true, "checkbox toggled into submit");
        assert_eq!(
            v["params"]["qty"], 10.0,
            "slider stepped + clamped into submit: {json}"
        );
    }

    #[test]
    fn a2ui_slider_steps_and_a_button_event_submits_it() {
        let a2ui = concat!(
            r#"{"version":"v0.10","createSurface":{"surfaceId":"s","catalogId":"c"}}"#,
            "\n",
            r#"{"version":"v0.10","updateComponents":{"surfaceId":"s","components":["#,
            r#"{"id":"root","component":"Column","children":["n","ok"]},"#,
            r#"{"id":"n","component":"Slider","value":{"path":"/n"},"min":0,"max":9,"step":3,"label":"N"},"#,
            r#"{"id":"ok","component":"Button","child":"l","action":{"event":{"name":"save","context":{"n":{"path":"/n"}}}}},"#,
            r#"{"id":"l","component":"Text","text":"OK"}"#,
            r#"]}}"#,
        );
        let mut doc = RichDoc::build(&RichUiPayload {
            format: RichUiFormat::A2ui,
            source: a2ui.to_owned(),
        })
        .expect("A2UI builds");
        let inc = ring_ids(&doc)
            .into_iter()
            .find(|i| i.contains("#slider:") && !i.contains("#slider:-"))
            .expect("A2UI Slider is interactive (focus ring)");
        // 0 → 3 → 6 → 9 → 9 (clamp at max=9).
        for _ in 0..4 {
            let _ = doc.act_node(&inc, "t");
        }
        let RichAction::ToAgent(json) = doc.act_node("ok", "2026-01-01T00:00:00Z").expect("submit")
        else {
            panic!("the Button event must round-trip");
        };
        let v: Value = embedded_json(&json);
        assert_eq!(
            v["action"]["context"]["n"], 9.0,
            "the stepped slider value is in the submitted A2UI context: {json}"
        );
    }

    #[test]
    fn submitted_form_is_a_user_message_wrapped_with_the_format_marker() {
        // A bare JSON blob on the prompt channel is opaque to an LLM.
        // A submit's RichAction::ToAgent payload must be a real user
        // message: a `[<fmt> form submission]` marker + instruction +
        // a ```json fence carrying the spec envelope. apply_rich_action
        // pushes it as a Role::User entry AND sends it via the prompt
        // channel — the agent receives a clearly-marked user message.
        let js = r#"{"root":"b","elements":{"b":{"type":"Button","props":{"label":"Go"},"on":{"press":{"action":"submit","params":{"q":1}}}}}}"#;
        let RichAction::ToAgent(text) = RichDoc::build(&RichUiPayload {
            format: RichUiFormat::JsonRender,
            source: js.to_owned(),
        })
        .unwrap()
        .act_node("b", "t")
        .unwrap() else {
            panic!("Button must round-trip");
        };
        assert!(
            text.starts_with("[json-render form submission]"),
            "the user message is marked: {text}"
        );
        assert!(text.contains("```json\n"), "the envelope is fenced JSON");
        assert_eq!(embedded_json(&text)["action"], "submit");

        let a = concat!(
            r#"{"version":"v0.10","createSurface":{"surfaceId":"s","catalogId":"c"}}"#,
            "\n",
            r#"{"version":"v0.10","updateComponents":{"surfaceId":"s","components":["#,
            r#"{"id":"root","component":"Button","child":"l","action":{"event":{"name":"go"}}},"#,
            r#"{"id":"l","component":"Text","text":"Go"}"#,
            r#"]}}"#,
        );
        let RichAction::ToAgent(text) = RichDoc::build(&RichUiPayload {
            format: RichUiFormat::A2ui,
            source: a.to_owned(),
        })
        .unwrap()
        .act_node("root", "2026-01-01T00:00:00Z")
        .unwrap() else {
            panic!("A2UI Button must round-trip");
        };
        assert!(text.starts_with("[A2UI form submission]"), "marker: {text}");
        let v = embedded_json(&text);
        assert_eq!(v["version"], "v0.10");
        assert_eq!(v["action"]["name"], "go");
    }

    #[test]
    fn a2ui_tabs_switch_via_act_node_persists() {
        // A2UI `Tabs` headers are now interactive (`<id>#tab:<n>`):
        // activating one switches the reducer-owned active tab and the
        // re-projection shows that tab's child.
        let a2ui = concat!(
            r#"{"version":"v0.10","createSurface":{"surfaceId":"s","catalogId":"c"}}"#,
            "\n",
            r#"{"version":"v0.10","updateComponents":{"surfaceId":"s","components":["#,
            r#"{"id":"root","component":"Tabs","tabs":[{"title":"One","child":"a"},{"title":"Two","child":"b"}]},"#,
            r#"{"id":"a","component":"Text","text":"FIRST"},"#,
            r#"{"id":"b","component":"Text","text":"SECOND"}"#,
            r#"]}}"#,
        );
        let mut doc = RichDoc::build(&RichUiPayload {
            format: RichUiFormat::A2ui,
            source: a2ui.to_owned(),
        })
        .expect("A2UI builds");
        assert!(row_of(&doc, "FIRST").is_some(), "tab 0 child shows first");

        let acted = doc.act_node("root#tab:1", "t");
        assert!(
            matches!(acted, Some(RichAction::Local(_))),
            "activating a tab header is a local selection change: {acted:?}"
        );
        assert!(
            row_of(&doc, "SECOND").is_some() && row_of(&doc, "FIRST").is_none(),
            "the active tab persisted across re-projection"
        );
    }
}
