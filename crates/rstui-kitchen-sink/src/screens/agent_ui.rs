//! Shared scaffolding for the two **Agent UI** screens ([`a2ui_demo`],
//! [`json_render_demo`](super::json_render_demo)): an editable code
//! editor holding the document an agent would send on the left, its
//! **live** `rstui-jsonui` projection on the right.
//!
//! The left pane is the real [`Editor`] code-editor widget over a
//! caller-owned [`TextArea`] (line-number gutter, caret, typing, paste —
//! the same composition the IDE scene uses). The right pane re-parses
//! that buffer and re-projects it **every frame**, so editing the JSON
//! live-updates the rendered UI — pure projection, no retained tree,
//! exactly how the ACP client renders agent UI. `PgUp`/`PgDn` switch
//! between the worked examples (edits persist per example).

use rstui_core::{
    Buffer, Constraint, KeyCode, Layout, Line, Position, Rect, Style, TextArea, Widget,
};
use rstui_jsonui::a2ui::A2uiSurface;
use rstui_jsonui::jsonrender::JsonRenderDoc;
use rstui_jsonui::tree::{HitMap, UiNode};
use rstui_runtime::Frame;
use rstui_widgets::{Block, BorderType, Editor, LineNumberGutter, Paragraph};

use crate::screens::ScreenOutcome;
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

/// Which agent-UI format a [`Scene`] hosts: it picks the projector, the
/// header label, and the seed examples.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Format {
    /// Google A2UI v0.10 (a server→client JSONL envelope stream).
    A2ui,
    /// Vercel json-render (a flat `{root,elements}` spec).
    JsonRender,
}

impl Format {
    /// The header / panel label.
    fn label(self) -> &'static str {
        match self {
            Self::A2ui => "A2UI v0.10",
            Self::JsonRender => "json-render",
        }
    }

    /// The worked examples that seed the editable buffers.
    fn samples(self) -> &'static [Sample] {
        match self {
            Self::A2ui => A2UI_SAMPLES,
            Self::JsonRender => JSON_RENDER_SAMPLES,
        }
    }

    /// Project a document's current text to a renderable node (total).
    fn project(self, source: &str) -> UiNode {
        match self {
            Self::A2ui => a2ui_node(source),
            Self::JsonRender => json_render_node(source),
        }
    }
}

/// The shared **Agent UI** scene: an editable code editor of the agent
/// document on the left, its live `rstui-jsonui` projection on the
/// right. Caller-owned state in the IDE pattern — one editable
/// [`TextArea`] per worked example, edits persist when you switch back.
#[derive(Debug)]
pub(crate) struct Scene {
    format: Format,
    docs: Vec<(&'static str, TextArea)>,
    active: usize,
}

impl Scene {
    /// A scene seeded from the format's worked examples, opened on the
    /// first one with the caret at the top.
    pub(crate) fn new(format: Format) -> Self {
        let docs = format
            .samples()
            .iter()
            .map(|sample| {
                let mut doc = TextArea::from_value(sample.source);
                doc.set_cursor(0, 0);
                (sample.name, doc)
            })
            .collect();
        Self {
            format,
            docs,
            active: 0,
        }
    }

    fn doc(&mut self) -> &mut TextArea {
        &mut self.docs[self.active].1
    }

    /// `PgUp`/`PgDn` switch examples (edits persist per example); arrows
    /// move the caret; typing / `Enter` / `Backspace` edit the buffer —
    /// the right pane re-projects it live. Like a real editor, `←` is a
    /// caret move, not a rail fall-back (use the rail / palette to leave).
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::PageUp => self.active = self.active.saturating_sub(1),
            KeyCode::PageDown => {
                self.active = (self.active + 1).min(self.docs.len().saturating_sub(1));
            }
            KeyCode::Left => {
                self.doc().move_left();
            }
            KeyCode::Right => {
                self.doc().move_right();
            }
            KeyCode::Up => {
                self.doc().move_up();
            }
            KeyCode::Down => {
                self.doc().move_down();
            }
            KeyCode::Enter => self.doc().insert_newline(),
            KeyCode::Backspace => {
                self.doc().delete_backward();
            }
            KeyCode::Char(c) => self.doc().insert_char(c),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Pasted text is inserted at the caret.
    pub(crate) fn on_paste(&mut self, text: &str) {
        self.doc().insert_str(text);
    }

    /// Cut `sel` out of the active buffer.
    pub(crate) fn cut(&mut self, sel: &str) -> bool {
        crate::screens::cut_area(self.doc(), sel)
    }

    /// Wheel scroll nudges the caret a line at a time (the IDE idiom).
    pub(crate) fn on_scroll(&mut self, up: bool) {
        if up {
            self.doc().move_up();
        } else {
            self.doc().move_down();
        }
    }

    /// The editor's text rect — after the header split, the left half,
    /// the frame, and the line-number gutter — so a drag-select stays
    /// inside the buffer and never the gutter or the rendered pane.
    /// Mirrors [`view`](Self::view)'s composition exactly.
    pub(crate) fn selection_region(&self, pos: Position, content: Rect) -> Option<Rect> {
        let [_, body] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(content);
        let [left, _] =
            Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).areas(body);
        if !left.contains(pos) {
            return None;
        }
        let ia = crate::screens::block_inner(left);
        let rows = self.docs[self.active].1.row_count();
        Some(LineNumberGutter::new(1, rows).min_number_width(3).inner(ia))
    }

    /// Draw the editor ⇆ live-projection split.
    pub(crate) fn view(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let [header, body] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
        let (name, doc) = &self.docs[self.active];

        frame.render_widget(
            Paragraph::new(Line::from(format!(
                " {}  ·  example {}/{}: {name}   PgUp/PgDn switch · \
                 type to edit — the output re-renders live ",
                self.format.label(),
                self.active + 1,
                self.docs.len(),
            )))
            .style(theme.caption()),
            header,
        );

        let [left, right] =
            Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).areas(body);

        // Left: the editable agent document (line-number gutter + Editor).
        let left_block = framed(
            theme,
            &format!("Agent response · {} (editable)", self.format.label()),
        );
        let ia = left_block.inner(left);
        frame.render_widget(left_block, left);
        let gutter = LineNumberGutter::new(1, doc.row_count())
            .style(theme.caption())
            .min_number_width(3);
        let text_rect = gutter.inner(ia);
        frame.render_widget(gutter, ia);
        frame.render_widget(
            Editor::new(doc)
                .focused(true)
                .style(theme.body())
                .focus_style(theme.border_focused())
                .cursor_style(Style::new().fg(theme.base).bg(theme.accent)),
            text_rect,
        );

        // Right: the live projection of whatever is in the buffer *now*
        // (re-parsed every frame — edit the JSON, watch it re-render).
        let right_block = framed(theme, "Rendered output (live)");
        let right_inner = right_block.inner(right);
        frame.render_widget(right_block, right);
        let source = doc.lines().join("\n");
        frame.render_widget(NodeView(self.format.project(&source)), right_inner);
    }
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
