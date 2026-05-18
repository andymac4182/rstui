//! [`Diagram`] — the DSL an AI tool emits to *output a diagram*, rendered.
//!
//! # The diagram DSL we expose to agents
//!
//! "Is there a DSL we can expose for the diagram concept to let AI tools
//! output a diagram?" — yes, and it is the one models already speak:
//!
//! - **[`Mermaid`]** — every Mermaid diagram type
//!   (`flowchart`/`graph`, `sequenceDiagram`, `classDiagram`,
//!   `stateDiagram-v2`, `erDiagram`, `gantt`, `pie`, `gitGraph`, `mindmap`,
//!   `timeline`, `journey`, `quadrantChart`, `requirementDiagram`,
//!   `sankey-beta`, `xychart-beta`, `block-beta`, `packet-beta`, `kanban`,
//!   `architecture-beta`, `radar-beta`, `C4*`, `zenuml`). This is the
//!   de-facto LLM diagram DSL — a model emits a ```` ```mermaid ```` fenced
//!   block unprompted.
//! - **[`Structurizr`]** — the Structurizr DSL /
//!   C4 model (`workspace { model { … } views { … } }`), in a
//!   ```` ```structurizr ```` block.
//! - **[`JsonCanvas`]** — [JSON Canvas 1.0](https://jsoncanvas.org/), the
//!   *explicit-placement* answer: Mermaid and Structurizr are auto-layout
//!   (a model cannot say "put this box here"); JSON Canvas is a tiny
//!   `{ "nodes": [...], "edges": [...] }` document where every node carries
//!   integer `x`/`y`/`width`/`height`, so a model that *wants* to control
//!   the layout emits it (in a ```` ```canvas ````/```` ```jsoncanvas ````
//!   block, or just a JSON body with `nodes`/`edges`). It is the format
//!   Obsidian Canvas writes, so models already know it.
//!
//! An AI tool/agent "outputs a diagram" by emitting that DSL — typically a
//! fenced code block inside a [`Text`](crate::model::UiPart::Text) part or a
//! tool result. [`Diagram::extract`] pulls the first such block out of
//! arbitrary agent prose; [`Diagram::new`] takes a bare or fenced source and
//! auto-detects the language. The contract is *advertised* to the agent via
//! `rstui_jsonui::capability` so the model knows it may answer with a
//! diagram instead of describing one in words.
//!
//! # A pure projection, total, deterministic
//!
//! [`Diagram`] owns no parser: it is a thin pure projection
//! ([ADR 0012](https://github.com/andymac4182/rstui/blob/main/docs/composition.md))
//! that delegates to the existing deterministic, panic-free
//! [`Mermaid`]/[`Structurizr`]
//! widgets — the ADR 0002 §4 / ADR 0017 "new behavior over an existing
//! parser" precedent, exactly like [`stream_markdown`](crate::stream_markdown)
//! projects Mermaid inside prose. Hostile, truncated, or empty agent output
//! degrades to a visible placeholder, never a panic, so a *streaming*
//! diagram (the closing fence not yet arrived) still renders.
//!
//! ```
//! use rstui_core::{Buffer, Rect, Widget};
//! use rstui_ai::diagram::{Diagram, DiagramLanguage};
//!
//! // The shape an agent emits in a chat turn:
//! let turn = "Here is the flow:\n\n```mermaid\ngraph TD\n  A --> B\n```\n";
//! let d = Diagram::extract(turn).expect("a fenced diagram");
//! assert_eq!(d.language(), DiagramLanguage::Mermaid);
//!
//! let mut buf = Buffer::empty(Rect::new(0, 0, 24, 9));
//! d.render(buf.area(), &mut buf);
//! ```

use std::borrow::Cow;

use rstui_core::{Buffer, Rect, Style, Widget};
use rstui_widgets::{Block, JsonCanvas, Mermaid, Structurizr};

/// Which diagram DSL a [`Diagram`] source is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramLanguage {
    /// Mermaid — any of the diagram types [`rstui_widgets::Mermaid`]
    /// renders. The default when a model emits a diagram without saying
    /// which DSL.
    Mermaid,
    /// The Structurizr DSL (C4 model), rendered by
    /// [`rstui_widgets::Structurizr`].
    Structurizr,
    /// [JSON Canvas 1.0](https://jsoncanvas.org/) — the *explicit
    /// placement* format (every node carries `x`/`y`/`width`/`height`), the
    /// one a model emits when it wants to control the layout instead of
    /// leaving it to auto-layout. Rendered by [`rstui_widgets::JsonCanvas`].
    JsonCanvas,
}

impl DiagramLanguage {
    /// The canonical fenced-code info string an agent should use for this
    /// language (`mermaid` / `structurizr` / `canvas`).
    #[must_use]
    pub const fn fence_tag(self) -> &'static str {
        match self {
            Self::Mermaid => "mermaid",
            Self::Structurizr => "structurizr",
            Self::JsonCanvas => "canvas",
        }
    }
}

/// A read-only view of an agent-produced diagram DSL.
///
/// The source is a [`Cow<str>`](std::borrow::Cow) — a borrowed turn string
/// or an owned block lifted by [`extract`](Self::extract). An optional
/// framing [`Block`] and a base [`Style`] are the only knobs; the language
/// is auto-detected (or forced via [`mermaid`](Self::mermaid) /
/// [`structurizr`](Self::structurizr)). A leading fenced ```` ``` ```` block
/// is unwrapped, so the *raw text an LLM emits* renders directly.
#[derive(Debug, Clone)]
pub struct Diagram<'a> {
    source: Cow<'a, str>,
    forced: Option<DiagramLanguage>,
    block: Option<Block<'a>>,
    style: Style,
}

impl<'a> Diagram<'a> {
    /// A diagram from `source` — a bare DSL body *or* a fenced
    /// ```` ```mermaid ```` / ```` ```structurizr ```` block (the language
    /// and body are auto-detected).
    #[must_use]
    pub fn new(source: impl Into<Cow<'a, str>>) -> Self {
        Self {
            source: source.into(),
            forced: None,
            block: None,
            style: Style::new(),
        }
    }

    /// Forces the source to be read as Mermaid (skip detection).
    #[must_use]
    pub fn mermaid(source: impl Into<Cow<'a, str>>) -> Self {
        Self {
            source: source.into(),
            forced: Some(DiagramLanguage::Mermaid),
            block: None,
            style: Style::new(),
        }
    }

    /// Forces the source to be read as the Structurizr DSL.
    #[must_use]
    pub fn structurizr(source: impl Into<Cow<'a, str>>) -> Self {
        Self {
            source: source.into(),
            forced: Some(DiagramLanguage::Structurizr),
            block: None,
            style: Style::new(),
        }
    }

    /// Frames the diagram in `block`; it renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`] passed through to the underlying renderer.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The detected (or forced) DSL.
    #[must_use]
    pub fn language(&self) -> DiagramLanguage {
        self.resolved().1
    }

    /// The diagram body actually rendered (the fenced block unwrapped).
    #[must_use]
    pub fn source(&self) -> &str {
        self.resolved().0
    }

    /// The fence-unwrapped body and the resolved language. Linear, total.
    fn resolved(&self) -> (&str, DiagramLanguage) {
        resolve(self.source.as_ref(), self.forced)
    }

    /// Extracts the first fenced diagram block from arbitrary agent text —
    /// a ```` ```mermaid ````, ```` ```mmd ````, ```` ```structurizr ````,
    /// ```` ```c4 ````, or ```` ```dsl ```` (or an unlabelled block whose
    /// body sniffs as a diagram) — into an owned [`Diagram`]. Returns
    /// `None` when the text has no diagram block (it is plain prose). This
    /// is the seam that turns an assistant
    /// [`Text`](crate::model::UiPart::Text) part or a tool result into a
    /// first-class rendered diagram.
    #[must_use]
    pub fn extract(text: &str) -> Option<Diagram<'static>> {
        let (body, lang) = first_fenced_diagram(text)?;
        Some(Diagram {
            source: Cow::Owned(body.to_string()),
            forced: Some(lang),
            block: None,
            style: Style::new(),
        })
    }
}

impl Widget for Diagram<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Destructure so the `body` borrow (into `source`) and the `block`
        // move are independent locals, not overlapping borrows of `self`.
        let Diagram {
            source,
            forced,
            block,
            style,
        } = self;
        let (body, lang) = resolve(source.as_ref(), forced);
        match lang {
            DiagramLanguage::Mermaid => {
                let mut w = Mermaid::new(body).style(style);
                if let Some(b) = block {
                    w = w.block(b);
                }
                w.render(area, buf);
            }
            DiagramLanguage::Structurizr => {
                let mut w = Structurizr::new(body).style(style);
                if let Some(b) = block {
                    w = w.block(b);
                }
                w.render(area, buf);
            }
            DiagramLanguage::JsonCanvas => {
                let mut w = JsonCanvas::new(body).style(style);
                if let Some(b) = block {
                    w = w.block(b);
                }
                w.render(area, buf);
            }
        }
    }
}

/// The fence-unwrapped body of `src` and its resolved language: a forced
/// language wins, else a fenced info string, else a content sniff. Linear
/// and total — the shared core of [`Diagram::language`]/[`source`](Diagram::source)
/// and [`render`](Diagram::render).
fn resolve(src: &str, forced: Option<DiagramLanguage>) -> (&str, DiagramLanguage) {
    let (body, fenced_lang) = unfence(src);
    let lang = forced.or(fenced_lang).unwrap_or_else(|| sniff(body));
    (body, lang)
}

/// Maps a fenced-code info string to a diagram language, if it names one.
fn lang_from_info(info: &str) -> Option<DiagramLanguage> {
    match info.trim().to_ascii_lowercase().as_str() {
        "mermaid" | "mmd" => Some(DiagramLanguage::Mermaid),
        "structurizr" | "c4" | "dsl" | "workspace" => Some(DiagramLanguage::Structurizr),
        "canvas" | "jsoncanvas" => Some(DiagramLanguage::JsonCanvas),
        _ => None,
    }
}

/// Whether a body looks like a JSON Canvas document: a JSON object that
/// carries a `nodes`/`edges` array.
fn is_json_canvas(body: &str) -> bool {
    let t = body.trim_start();
    t.starts_with('{') && (t.contains("\"nodes\"") || t.contains("\"edges\""))
}

/// Sniffs a *bare* (unfenced) body's language: a JSON object with
/// `nodes`/`edges` is JSON Canvas; a body opening with `workspace` is the
/// Structurizr DSL; everything else is treated as Mermaid (its own
/// dispatcher handles the 22 types and degrades an unknown header to a
/// placeholder — and an unlabelled LLM diagram is overwhelmingly Mermaid).
fn sniff(body: &str) -> DiagramLanguage {
    if is_json_canvas(body) {
        return DiagramLanguage::JsonCanvas;
    }
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") || t.starts_with('#') || t.starts_with("%%") {
            continue;
        }
        let first: String = t
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '{')
            .collect();
        return if first == "workspace" {
            DiagramLanguage::Structurizr
        } else {
            DiagramLanguage::Mermaid
        };
    }
    DiagramLanguage::Mermaid
}

/// If `src` is *exactly* a single fenced block (optionally surrounded by
/// blank lines), returns its body and the language its info string names;
/// otherwise returns `src` unchanged with `None`. Linear and total: an
/// unterminated fence (still streaming) takes everything after the opener.
fn unfence(src: &str) -> (&str, Option<DiagramLanguage>) {
    let trimmed = src.trim_matches(|c: char| c == '\n' || c == '\r');
    let Some(rest) = trimmed.strip_prefix("```") else {
        return (src, None);
    };
    // The opening fence's info string is the remainder of that first line.
    let nl = rest.find('\n').unwrap_or(rest.len());
    let info = &rest[..nl];
    let after = rest.get(nl + 1..).unwrap_or("");
    let lang = lang_from_info(info);
    // Body is everything up to a closing fence line (``` alone), else EOF.
    let body = match find_closing_fence(after) {
        Some(end) => &after[..end],
        None => after,
    };
    (body.trim_matches('\n'), lang)
}

/// Byte offset in `s` of the start of the line that is just a ```` ``` ````
/// closing fence, if any.
fn find_closing_fence(s: &str) -> Option<usize> {
    let mut idx = 0;
    for line in s.split_inclusive('\n') {
        if line.trim_end_matches(['\n', '\r']).trim() == "```" {
            return Some(idx);
        }
        idx += line.len();
    }
    None
}

/// Scans arbitrary text for the first fenced diagram block: a ```` ``` ````
/// opener whose info string names a diagram language, or an unlabelled
/// block whose body sniffs as a diagram. Returns `(body, language)`.
/// Linear and total.
fn first_fenced_diagram(text: &str) -> Option<(&str, DiagramLanguage)> {
    let mut offset = 0;
    let bytes = text.as_bytes();
    while offset < text.len() {
        // Find the next fence opener at a line start.
        let rel = text[offset..].find("```")?;
        let at = offset + rel;
        let line_start = at == 0 || bytes[at - 1] == b'\n';
        if !line_start {
            offset = at + 3;
            continue;
        }
        let rest = &text[at + 3..];
        let nl = rest.find('\n').unwrap_or(rest.len());
        let info = &rest[..nl];
        let after = rest.get(nl + 1..).unwrap_or("");
        let body = match find_closing_fence(after) {
            Some(end) => &after[..end],
            None => after,
        };
        let body = body.trim_matches('\n');
        let lang = lang_from_info(info).or_else(|| {
            // An unlabelled / prose block: only treat it as a diagram when
            // its first line is unmistakably one (a Mermaid header keyword
            // or a Structurizr `workspace`), so plain code stays code.
            (!body.is_empty() && looks_like_diagram(body)).then(|| sniff(body))
        });
        if let Some(lang) = lang {
            if !body.is_empty() {
                return Some((body, lang));
            }
        }
        // Not a diagram block — resume scanning after this opener.
        offset = at + 3;
    }
    None
}

/// Whether a bare body's first significant token is a recognised diagram
/// header (used only to rescue an *unlabelled* fenced block).
fn looks_like_diagram(body: &str) -> bool {
    if is_json_canvas(body) {
        return true;
    }
    let Some(line) = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("%%"))
    else {
        return false;
    };
    let first: String = line
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '{')
        .collect();
    const HEADS: &[&str] = &[
        "workspace",
        "graph",
        "flowchart",
        "sequenceDiagram",
        "classDiagram",
        "stateDiagram",
        "stateDiagram-v2",
        "erDiagram",
        "journey",
        "gantt",
        "pie",
        "quadrantChart",
        "requirementDiagram",
        "gitGraph",
        "mindmap",
        "timeline",
        "sankey-beta",
        "xychart-beta",
        "block-beta",
        "packet-beta",
        "kanban",
        "architecture-beta",
        "radar-beta",
        "zenuml",
    ];
    HEADS.contains(&first.as_str()) || first.starts_with("C4")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Position;

    /// Render `widget` into a `w`×`h` buffer; glyph rows joined by `\n`.
    fn lines(widget: Diagram<'_>, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    // --- language detection ------------------------------------------------

    #[test]
    fn bare_mermaid_and_structurizr_sniff_correctly() {
        assert_eq!(
            Diagram::new("graph TD\n A-->B").language(),
            DiagramLanguage::Mermaid
        );
        assert_eq!(
            Diagram::new("sequenceDiagram\n A->>B: hi").language(),
            DiagramLanguage::Mermaid
        );
        assert_eq!(
            Diagram::new("workspace {\n model {}\n}").language(),
            DiagramLanguage::Structurizr
        );
        // Unknown header still defaults to Mermaid (its dispatcher handles it).
        assert_eq!(Diagram::new("???").language(), DiagramLanguage::Mermaid);
    }

    #[test]
    fn a_fenced_block_is_unwrapped_and_its_info_string_wins() {
        let d = Diagram::new("```mermaid\ngraph LR\n A-->B\n```");
        assert_eq!(d.language(), DiagramLanguage::Mermaid);
        assert_eq!(d.source(), "graph LR\n A-->B");
        let s = Diagram::new("```structurizr\nworkspace {}\n```");
        assert_eq!(s.language(), DiagramLanguage::Structurizr);
        assert_eq!(s.source(), "workspace {}");
        // Unlabelled fence → sniff the body.
        let u = Diagram::new("```\nworkspace {\n}\n```");
        assert_eq!(u.language(), DiagramLanguage::Structurizr);
    }

    #[test]
    fn forced_language_overrides_detection() {
        assert_eq!(
            Diagram::structurizr("workspace {}").language(),
            DiagramLanguage::Structurizr
        );
        assert_eq!(
            Diagram::mermaid("graph TD\nA-->B").language(),
            DiagramLanguage::Mermaid
        );
        assert_eq!(DiagramLanguage::Mermaid.fence_tag(), "mermaid");
        assert_eq!(DiagramLanguage::Structurizr.fence_tag(), "structurizr");
    }

    /// JSON Canvas — the explicit-placement path.
    const CANVAS: &str = r#"{"nodes":[
      {"id":"a","type":"text","text":"Start","x":0,"y":0,"width":120,"height":60},
      {"id":"b","type":"text","text":"Finish","x":400,"y":0,"width":120,"height":60}],
      "edges":[{"id":"e","fromNode":"a","toNode":"b","label":"go"}]}"#;

    #[test]
    fn json_canvas_is_detected_bare_fenced_and_forced() {
        // A bare JSON object with nodes/edges.
        assert_eq!(Diagram::new(CANVAS).language(), DiagramLanguage::JsonCanvas);
        // A ```canvas / ```jsoncanvas fence.
        assert_eq!(
            Diagram::new("```canvas\n{\"nodes\":[]}\n```").language(),
            DiagramLanguage::JsonCanvas
        );
        assert_eq!(
            Diagram::new("```jsoncanvas\n{\"edges\":[]}\n```").language(),
            DiagramLanguage::JsonCanvas
        );
        assert_eq!(DiagramLanguage::JsonCanvas.fence_tag(), "canvas");
        // Not confused with Mermaid/Structurizr.
        assert_eq!(
            Diagram::new("graph TD\nA-->B").language(),
            DiagramLanguage::Mermaid
        );
    }

    #[test]
    fn json_canvas_extracts_from_prose_and_renders_placed() {
        let turn = "Here's the layout I want:\n\n```canvas\n".to_string()
            + CANVAS
            + "\n```\n\nLooks good?";
        let d = Diagram::extract(&turn).expect("a fenced canvas");
        assert_eq!(d.language(), DiagramLanguage::JsonCanvas);
        let out = lines(Diagram::new(CANVAS), 48, 8);
        assert!(out.contains("Start") && out.contains("Finish"), "{out}");
        // Explicit x: Start (x0) is left of Finish (x400).
        let row = out.lines().find(|l| l.contains("Start")).unwrap();
        let frow = out.lines().find(|l| l.contains("Finish")).unwrap();
        assert!(
            row.find("Start").unwrap() < frow.find("Finish").unwrap(),
            "placement honoured:\n{out}"
        );
    }

    #[test]
    fn an_unterminated_fence_still_resolves_a_body_streaming() {
        // The closing ``` has not arrived yet.
        let d = Diagram::new("```mermaid\ngraph TD\n A-->B");
        assert_eq!(d.language(), DiagramLanguage::Mermaid);
        assert_eq!(d.source(), "graph TD\n A-->B");
    }

    // --- extract from agent prose -----------------------------------------

    #[test]
    fn extract_pulls_the_first_diagram_out_of_a_chat_turn() {
        let turn = "Sure! Here's the architecture:\n\n\
                    ```mermaid\nflowchart LR\n  U[User] --> S[Server]\n```\n\n\
                    Let me know if you want changes.";
        let d = Diagram::extract(turn).expect("a fenced diagram");
        assert_eq!(d.language(), DiagramLanguage::Mermaid);
        assert_eq!(d.source(), "flowchart LR\n  U[User] --> S[Server]");
    }

    #[test]
    fn extract_finds_structurizr_and_unlabelled_diagram_blocks() {
        let t = "```structurizr\nworkspace \"X\" {\n}\n```";
        assert_eq!(
            Diagram::extract(t).unwrap().language(),
            DiagramLanguage::Structurizr
        );
        // Unlabelled block whose body is unmistakably a diagram.
        let u = "look:\n```\nsequenceDiagram\n  A->>B: hi\n```";
        assert_eq!(
            Diagram::extract(u).unwrap().language(),
            DiagramLanguage::Mermaid
        );
    }

    #[test]
    fn extract_returns_none_for_plain_prose_and_plain_code() {
        assert!(Diagram::extract("just some text, no diagram").is_none());
        // A plain code block is NOT a diagram.
        assert!(Diagram::extract("```python\nprint('hi')\n```").is_none());
        assert!(Diagram::extract("```\nlet x = 1;\n```").is_none());
    }

    // --- render (delegation + totality) -----------------------------------

    #[test]
    fn renders_a_mermaid_flowchart_through_the_widget() {
        let out = lines(Diagram::new("graph TD\nA-->B"), 9, 9);
        assert!(out.contains("A") && out.contains("B"), "{out}");
        assert!(out.contains('▼'), "a down arrow:\n{out}");
        // Deterministic.
        assert_eq!(out, lines(Diagram::new("graph TD\nA-->B"), 9, 9));
    }

    #[test]
    fn renders_a_structurizr_workspace_through_the_widget() {
        let src = "workspace \"Sys\" {\n model {\n u = person \"User\"\n \
                   s = softwareSystem \"S\"\n u -> s \"Uses\"\n }\n \
                   views {\n systemContext s {\n include *\n }\n }\n}";
        let out = lines(Diagram::structurizr(src), 60, 20);
        assert!(out.contains("System Context"), "C4 header:\n{out}");
        assert!(out.contains("User"), "person:\n{out}");
    }

    #[test]
    fn empty_and_garbled_input_degrade_to_a_placeholder_not_a_panic() {
        let out = lines(Diagram::new(""), 40, 1);
        assert!(out.contains("mermaid"), "placeholder:\n{out}");
        // Tiny / zero area: no panic.
        let _ = lines(Diagram::new("graph TD\nA-->B"), 2, 2);
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Diagram::new("workspace {}").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn block_frames_the_diagram() {
        let out = lines(
            Diagram::new("graph TD\nA-->B").block(Block::bordered()),
            12,
            9,
        );
        assert!(out.starts_with('┌'), "block frame:\n{out}");
    }
}
