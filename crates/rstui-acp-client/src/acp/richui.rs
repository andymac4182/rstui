//! Detecting and rendering agent-authored declarative UI (A2UI /
//! json-render) inside the transcript.
//!
//! The client advertises (in the ACP `initialize` client capabilities,
//! see [`render_capability_meta`]) that it can render A2UI and
//! json-render documents. When an agent then sends one — as a content
//! block that is a self-contained JSON document or a fenced
//! ` ```a2ui ` / ` ```json-render ` / ` ```spec ` block — [`detect`]
//! classifies it and [`render_lines`] projects it through
//! [`rstui_jsonui`] into transcript [`Line`]s.
//!
//! Detection is **conservative and total**: it only fires on a complete,
//! parseable document (a half-streamed token chunk fails the JSON parse
//! and falls through to normal text), and it never panics. Rendering is
//! a pure projection (ADR 0012): the parsed document is re-derived from
//! the stored source every frame — there is no retained UI tree — so it
//! composes with the existing immediate-mode transcript with no new
//! lifecycle.

use rstui_core::{Buffer, Line, Rect, Span, Style};
use rstui_jsonui::a2ui::A2uiSurface;
use rstui_jsonui::jsonrender::JsonRenderDoc;
use rstui_jsonui::tree::HitMap;
use serde_json::{Map, Value};

/// Which declarative-UI format an agent payload is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RichUiFormat {
    /// Google A2UI (a server→client envelope or a JSONL stream of them).
    A2ui,
    /// Vercel json-render (a flat `{root,elements,state}` spec).
    JsonRender,
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

/// Strips a fenced ` ```<tag> … ``` ` wrapper, returning `(tag, body)`
/// when `text` is exactly one fenced block (the way an agent embeds a UI
/// doc in a chat message), else `None`.
fn fenced_block(text: &str) -> Option<(String, &str)> {
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix("```")?;
    let body = rest.strip_suffix("```")?;
    let (info, content) = body.split_once('\n')?;
    Some((info.trim().to_ascii_lowercase(), content))
}

/// Classifies a content block as an A2UI / json-render document, or
/// `None` for ordinary prose. **Total** — any parse failure (e.g. a
/// partial streamed chunk) is simply `None`, so normal text is
/// unaffected.
#[must_use]
pub fn detect(text: &str) -> Option<RichUiPayload> {
    // An explicit fenced block is the unambiguous signal.
    if let Some((tag, body)) = fenced_block(text) {
        let format = match tag.as_str() {
            "a2ui" => Some(RichUiFormat::A2ui),
            "json-render" | "jsonrender" | "jsonui" | "spec" => Some(RichUiFormat::JsonRender),
            _ => None,
        };
        if let Some(format) = format {
            return Some(RichUiPayload {
                format,
                source: body.to_owned(),
            });
        }
    }

    let trimmed = text.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    let parsed: Value = serde_json::from_str(trimmed).ok()?;

    // A2UI: an envelope object/array carrying a `version` + one of the
    // six message keys (or a `messages` wrapper).
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

    // json-render: a flat spec — a `root` string + an `elements` object.
    if parsed.get("root").and_then(Value::as_str).is_some()
        && parsed.get("elements").map(Value::is_object) == Some(true)
    {
        return Some(RichUiPayload {
            format: RichUiFormat::JsonRender,
            source: trimmed.to_owned(),
        });
    }
    None
}

/// Projects a detected payload to transcript [`Line`]s, `width` columns
/// wide and at most `max_height` rows (it renders the document into a
/// scratch [`Buffer`] then converts the painted rows — the same
/// embed-a-widget-in-a-line-view technique the streaming-markdown view
/// uses for diagrams). Always total: a malformed document degrades to
/// the engine's own placeholder, never a panic.
#[must_use]
pub fn render_lines(payload: &RichUiPayload, width: u16, max_height: u16) -> Vec<Line<'static>> {
    let width = width.max(1);
    let height = max_height.max(1);
    let node = match payload.format {
        RichUiFormat::A2ui => {
            let mut surface = A2uiSurface::new();
            surface.apply_stream(&payload.source);
            surface.project()
        }
        RichUiFormat::JsonRender => match serde_json::from_str::<Value>(&payload.source) {
            Ok(spec) => JsonRenderDoc::from_flat_value(&spec).view(),
            Err(_) => return vec![Line::raw("[invalid json-render document]")],
        },
    };

    let mut scratch = Buffer::empty(Rect::new(0, 0, width, height));
    let mut hits = HitMap::new();
    node.render(scratch.area(), &mut scratch, &mut hits);

    // Convert painted rows to owned Lines, trimming the trailing blank
    // rows so an over-tall scratch does not pad the transcript.
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
}
