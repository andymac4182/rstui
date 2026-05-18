//! Shared scaffolding for the two **Agent UI** screens ([`a2ui_demo`],
//! [`json_render_demo`](super::json_render_demo)): the worked example
//! documents an agent would send, the split renderer (the raw agent
//! response on the left, its live `rstui-jsonui` projection on the
//! right), and a [`Widget`] adapter so a parsed
//! [`UiNode`](rstui_jsonui::tree::UiNode) drops straight into a
//! [`Frame`].
//!
//! This is a normal kitchen-sink scene, so it obeys the same rules: the
//! screen owns only `(example, scroll)` as caller state; `view` re-parses
//! the selected document and re-projects it every frame (pure projection,
//! no retained UI tree — exactly how the ACP client renders agent UI).

use rstui_core::{Buffer, Constraint, Layout, Line, Position, Rect, Widget};
use rstui_jsonui::a2ui::A2uiSurface;
use rstui_jsonui::jsonrender::JsonRenderDoc;
use rstui_jsonui::tree::{HitMap, UiNode};
use rstui_runtime::Frame;
use rstui_widgets::{Block, BorderType, Paragraph, Wrap};

use crate::theme::Theme;

/// One worked agent document: a short label and the verbatim payload an
/// agent would send (an A2UI JSONL stream, or a json-render spec).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sample {
    /// The short name shown in the header.
    pub(crate) name: &'static str,
    /// The exact bytes the "agent" sent (rendered on the left).
    pub(crate) source: &'static str,
}

/// A `rstui-jsonui` [`UiNode`] as a [`Widget`], so a projected agent
/// document renders through the normal `frame.render_widget` path. Hits
/// are discarded — the demo is read-only (the live ACP client is what
/// wires interaction back to the agent).
struct NodeView(UiNode);

impl Widget for NodeView {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut hits = HitMap::new();
        self.0.render(area, buf, &mut hits);
    }
}

/// A rounded, titled panel in the active theme (the kitchen-sink house
/// frame, kept local so this scene stays self-contained).
fn framed(theme: &Theme, title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {title} ")).style(theme.caption()))
        .border_style(theme.border())
        .style(theme.body())
}

/// Parses an A2UI server→client JSONL stream into its projected node
/// (total — a malformed line degrades to a placeholder, never panics).
pub(crate) fn a2ui_node(source: &str) -> UiNode {
    let mut surface = A2uiSurface::new();
    surface.apply_stream(source);
    surface.project()
}

/// Parses a json-render flat spec into its projected node (total).
pub(crate) fn json_render_node(source: &str) -> UiNode {
    match serde_json::from_str::<serde_json::Value>(source) {
        Ok(spec) => JsonRenderDoc::from_flat_value(&spec).view(),
        Err(_) => UiNode::Placeholder("invalid json-render document".to_owned()),
    }
}

/// Draws the scene: a one-row header (which example, how to drive it),
/// then the body split 50/50 — the verbatim agent response (scrollable)
/// on the left, its live projection on the right.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_split(
    theme: &Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    format_label: &str,
    samples: &[Sample],
    example: usize,
    scroll: u16,
    node: UiNode,
) {
    let [header, body] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);

    let index = example.min(samples.len().saturating_sub(1));
    let name = samples.get(index).map(|s| s.name).unwrap_or("");
    let header_line = Line::from(format!(
        " {format_label}  ·  example {}/{}: {name}   ←/→ switch · ↑/↓ scroll source ",
        index + 1,
        samples.len().max(1),
    ));
    frame.render_widget(Paragraph::new(header_line).style(theme.caption()), header);

    let [left, right] =
        Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).areas(body);

    // Left: exactly what the agent sent.
    let left_block = framed(theme, &format!("Agent response · {format_label}"));
    let left_inner = left_block.inner(left);
    frame.render_widget(left_block, left);
    let source = samples.get(index).map(|s| s.source).unwrap_or("");
    frame.render_widget(
        Paragraph::new(source)
            .wrap(Wrap { trim: false })
            .scroll(Position::new(0, scroll))
            .style(theme.body()),
        left_inner,
    );

    // Right: the live rstui-jsonui projection of that response.
    let right_block = framed(theme, "Rendered output");
    let right_inner = right_block.inner(right);
    frame.render_widget(right_block, right);
    frame.render_widget(NodeView(node), right_inner);
}

/// The three worked A2UI documents (each a v0.10 server→client JSONL
/// stream: `createSurface`, then `updateComponents`, then
/// `updateDataModel` for the bound values).
pub(crate) const A2UI_SAMPLES: &[Sample] = &[
    Sample {
        name: "Sign-up form (data binding + action)",
        source: concat!(
            r#"{"version":"v0.10","createSurface":{"surfaceId":"s","catalogId":"https://a2ui.org/specification/v0_10/basic_catalog.json"}}"#,
            "\n",
            r#"{"version":"v0.10","updateComponents":{"surfaceId":"s","components":["#,
            r#"{"id":"root","component":"Column","children":["h","email","subscribe","go"]},"#,
            r#"{"id":"h","component":"Text","text":"Create your account","variant":"h2"},"#,
            r#"{"id":"email","component":"TextField","label":"Email","value":{"path":"/email"}},"#,
            r#"{"id":"subscribe","component":"CheckBox","label":"Email me product news","value":{"path":"/subscribe"}},"#,
            r#"{"id":"go","component":"Button","variant":"primary","child":"goLabel","action":{"event":{"name":"signup"}}},"#,
            r#"{"id":"goLabel","component":"Text","text":"Sign up"}"#,
            r#"]}}"#,
            "\n",
            r#"{"version":"v0.10","updateDataModel":{"surfaceId":"s","path":"/email","value":"ada@example.com"}}"#,
            "\n",
            r#"{"version":"v0.10","updateDataModel":{"surfaceId":"s","path":"/subscribe","value":true}}"#,
        ),
    },
    Sample {
        name: "Profile card (Card · Row · Divider · variants)",
        source: concat!(
            r#"{"version":"v0.10","createSurface":{"surfaceId":"s","catalogId":"https://a2ui.org/specification/v0_10/basic_catalog.json"}}"#,
            "\n",
            r#"{"version":"v0.10","updateComponents":{"surfaceId":"s","components":["#,
            r#"{"id":"root","component":"Card","child":"col"},"#,
            r#"{"id":"col","component":"Column","children":["name","title","rule","tags"]},"#,
            r#"{"id":"name","component":"Text","text":"Ada Lovelace","variant":"h2"},"#,
            r#"{"id":"title","component":"Text","text":"Mathematician · Analytical Engine","variant":"caption"},"#,
            r#"{"id":"rule","component":"Divider"},"#,
            r#"{"id":"tags","component":"Row","children":["t1","t2","t3"]},"#,
            r#"{"id":"t1","component":"Text","text":"algorithms"},"#,
            r#"{"id":"t2","component":"Text","text":"computing"},"#,
            r#"{"id":"t3","component":"Text","text":"history"}"#,
            r#"]}}"#,
        ),
    },
    Sample {
        name: "Tabs + list (containers)",
        source: concat!(
            r#"{"version":"v0.10","createSurface":{"surfaceId":"s","catalogId":"https://a2ui.org/specification/v0_10/basic_catalog.json"}}"#,
            "\n",
            r#"{"version":"v0.10","updateComponents":{"surfaceId":"s","components":["#,
            r#"{"id":"root","component":"Tabs","tabs":[{"title":"Overview","child":"ov"},{"title":"Steps","child":"steps"}]},"#,
            r#"{"id":"ov","component":"Text","text":"A2UI lets the agent describe the UI; this terminal renders it."},"#,
            r#"{"id":"steps","component":"List","children":["a","b","c"]},"#,
            r#"{"id":"a","component":"Text","text":"1. The client advertises its catalog"},"#,
            r#"{"id":"b","component":"Text","text":"2. The agent sends components + data"},"#,
            r#"{"id":"c","component":"Text","text":"3. rstui-jsonui projects them to widgets"}"#,
            r#"]}}"#,
        ),
    },
];

/// The three worked json-render documents (flat `{root,elements}` specs,
/// pretty-printed so the structure reads on the left).
pub(crate) const JSON_RENDER_SAMPLES: &[Sample] = &[
    Sample {
        name: "Status card (Card · Heading · Badge · StatusLine)",
        source: r##"{
  "root": "card",
  "elements": {
    "card":   { "type": "Card", "props": { "title": "Deployment" }, "children": ["col"] },
    "col":    { "type": "Box", "props": { "flexDirection": "column" },
                "children": ["h", "status", "rule", "note"] },
    "h":      { "type": "Heading", "props": { "text": "api-gateway", "level": "h3" } },
    "status": { "type": "StatusLine", "props": { "text": "Rolled out to 100%", "status": "success" } },
    "rule":   { "type": "Divider", "props": { "title": "details" } },
    "note":   { "type": "Text", "props": { "text": "3 replicas healthy · build #1287" } }
  }
}"##,
    },
    Sample {
        name: "Metrics row (KeyValue · ProgressBar · List)",
        source: r##"{
  "root": "box",
  "elements": {
    "box":  { "type": "Box", "props": { "flexDirection": "column" },
              "children": ["kv", "bar", "list"] },
    "kv":   { "type": "KeyValue", "props": { "label": "Region", "value": "us-east-1" } },
    "bar":  { "type": "ProgressBar", "props": { "progress": 0.72, "label": "Cache hit rate" } },
    "list": { "type": "List", "props": { "items": ["queue: 0", "p99: 84ms", "errors: 0.1%"] } }
  }
}"##,
    },
    Sample {
        name: "Markdown + badge (rich content)",
        source: r##"{
  "root": "box",
  "elements": {
    "box": { "type": "Box", "props": { "flexDirection": "column" },
             "children": ["badge", "md"] },
    "badge": { "type": "Badge", "props": { "label": "experimental", "variant": "warning" } },
    "md":  { "type": "Markdown",
             "props": { "text": "# json-render\n\nThe agent streams a **flat element map**; rstui projects it to widgets — `Markdown` included." } }
  }
}"##,
    },
];
