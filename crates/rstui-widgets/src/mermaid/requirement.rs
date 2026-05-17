//! `requirementDiagram` Mermaid renderer.
//!
//! A requirement diagram models systems-engineering artefacts: typed
//! *requirements* (`requirement` / `functionalRequirement` /
//! `performanceRequirement` / `interfaceRequirement` / `physicalRequirement` /
//! `designConstraint`), *elements* (`element`), and the traceability
//! *relationships* between them (`contains`, `copies`, `derives`, `satisfies`,
//! `verifies`, `refines`, `traces`).
//!
//! A requirement block carries `id`, `text`, `risk`, and `verifymethod` rows;
//! an element block carries `type` and `docref`. Relationships are written
//! either inline (`a - satisfies -> b`) or in a `{ type: … \n source: … \n
//! target: … }` block.
//!
//! # Terminal layout approximation
//!
//! Mermaid auto-lays the graph; a terminal cannot, so this draws a
//! deterministic, source-ordered approximation that preserves every node and
//! relationship:
//!
//! - Each requirement is a box: a `«requirement»` (or
//!   `«physicalRequirement»`, …) stereotype header, the bold name, then the
//!   `id` / `text` / `risk` / `verifymethod` rows that were given.
//! - Each element is a box with an `«element»` header then `type` / `docref`.
//! - Nodes pack into a deterministic integer grid (a square-ish, capped
//!   column count) in source order.
//! - Relationships are listed beneath as labelled arrows — `src ──▸ tgt
//!   «satisfies»` — dashed (`╴╴▸`) for the verification kinds (`traces`,
//!   `verifies`) and solid otherwise.
//!
//! Parsing is lenient: a malformed line is skipped, never fatal; a source with
//! nothing parseable falls through to [`super::diagram_placeholder`].

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// One requirement node (`requirement` and its typed variants).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Requirement {
    /// The block name (`requirement <name> { … }`).
    name: String,
    /// The stereotype shown on the header (`requirement`,
    /// `physicalRequirement`, …).
    kind: String,
    /// The `id:` field, if given.
    id: Option<String>,
    /// The `text:` field, if given.
    text: Option<String>,
    /// The `risk:` field, if given.
    risk: Option<String>,
    /// The `verifymethod:` field, if given.
    verify: Option<String>,
}

/// One `element <name> { type: … \n docref: … }` node.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ElementNode {
    /// The block name.
    name: String,
    /// The `type:` field, if given.
    kind: Option<String>,
    /// The `docref:` field, if given.
    docref: Option<String>,
}

/// One traceability relationship between two named nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Relationship {
    /// The source node name.
    src: String,
    /// The target node name.
    tgt: String,
    /// The verb (`satisfies`, `derives`, `traces`, …).
    verb: String,
}

/// The whole parsed requirement diagram in source order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Model {
    /// Every requirement node, in source order.
    requirements: Vec<Requirement>,
    /// Every element node, in source order.
    elements: Vec<ElementNode>,
    /// Every relationship, in source order.
    relationships: Vec<Relationship>,
}

impl Model {
    /// Whether nothing parseable was found (drives the placeholder fallback).
    fn is_empty(&self) -> bool {
        self.requirements.is_empty() && self.elements.is_empty() && self.relationships.is_empty()
    }
}

/// The recognised requirement-block keywords mapped to their stereotype text.
fn req_kind(kw: &str) -> Option<&'static str> {
    match kw {
        "requirement" => Some("requirement"),
        "functionalRequirement" => Some("functionalRequirement"),
        "performanceRequirement" => Some("performanceRequirement"),
        "interfaceRequirement" => Some("interfaceRequirement"),
        "physicalRequirement" => Some("physicalRequirement"),
        "designConstraint" => Some("designConstraint"),
        _ => None,
    }
}

/// The recognised relationship verbs.
fn is_verb(v: &str) -> bool {
    matches!(
        v,
        "contains" | "copies" | "derives" | "satisfies" | "verifies" | "refines" | "traces"
    )
}

/// Strips a single pair of surrounding double quotes (and whitespace).
fn unquote(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_prefix('"').unwrap_or(t);
    let t = t.strip_suffix('"').unwrap_or(t);
    t.trim().to_string()
}

/// What the line-scanner is currently inside.
enum State {
    /// Not inside any block.
    Top,
    /// Inside `requirement <name> { … }` (the in-progress node).
    Req(Requirement),
    /// Inside `element <name> { … }`.
    Elem(ElementNode),
    /// Inside an inline relationship block `{ type/source/target }`.
    RelBlock {
        /// Parsed `type:` verb.
        verb: Option<String>,
        /// Parsed `source:` node.
        src: Option<String>,
        /// Parsed `target:` node.
        tgt: Option<String>,
    },
}

/// Parses an inline relationship `src - verb -> tgt`. Returns `None` if the
/// line is not in that shape.
fn parse_inline_rel(line: &str) -> Option<Relationship> {
    // `<src> - <verb> -> <tgt>` (also tolerant of `->` spacing).
    let (src, rest) = line.split_once(" - ")?;
    let (verb, tgt) = rest.split_once("->")?;
    let verb = verb.trim();
    if !is_verb(verb) {
        return None;
    }
    let src = src.trim().to_string();
    let tgt = tgt.trim().to_string();
    if src.is_empty() || tgt.is_empty() {
        return None;
    }
    Some(Relationship {
        src,
        tgt,
        verb: verb.to_string(),
    })
}

/// Splits a source into logical statements. Mermaid writes a block either
/// multi-line *or* on one line with the fields separated by a literal `\n`
/// escape (`requirement r { id: 1 \n text: "t" }`); the braces may also share
/// a line with the head and the first/last field. Normalising up-front — real
/// newlines, the two-char `\n` escape, and the brace glyphs all become
/// statement breaks (the braces kept as their own `{` / `}` tokens) — lets the
/// state machine treat every shape identically. Empty tokens are dropped.
fn tokenize(src: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let flush_cur = |cur: &mut String, toks: &mut Vec<String>| {
        let t = cur.trim();
        if !t.is_empty() {
            toks.push(t.to_string());
        }
        cur.clear();
    };
    let mut chars = src.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {}
            '\n' => flush_cur(&mut cur, &mut toks),
            '\\' if chars.peek() == Some(&'n') => {
                chars.next();
                flush_cur(&mut cur, &mut toks);
            }
            '{' | '}' => {
                flush_cur(&mut cur, &mut toks);
                toks.push(ch.to_string());
            }
            _ => cur.push(ch),
        }
    }
    flush_cur(&mut cur, &mut toks);
    toks
}

/// Parses a `requirementDiagram` source: tokenise into logical statements
/// (handling single-line, `\n`-separated, and multi-line block shapes
/// uniformly), drop a leading `--- … ---` frontmatter block and `%%`
/// comment/directive lines, skip the header, then scan a tiny block state
/// machine. Lenient — a bad statement is skipped and an unterminated block is
/// flushed at EOF, never fatal.
fn parse(src: &str) -> Model {
    let mut model = Model::default();
    let mut state = State::Top;
    let mut in_front = false;
    let mut header_seen = false;

    for tok in tokenize(src) {
        let line = tok.as_str();
        if line == "---" {
            in_front = !in_front;
            continue;
        }
        if in_front {
            continue;
        }
        if line.starts_with("%%") {
            continue;
        }
        if !header_seen {
            header_seen = true;
            continue;
        }

        // An opening brace starts the block named by the previous head token;
        // if we are already `Top` it opens an inline relationship block.
        if line == "{" {
            if let State::Top = state {
                state = State::RelBlock {
                    verb: None,
                    src: None,
                    tgt: None,
                };
            }
            continue;
        }
        // A closing brace commits whatever block we were in.
        if line == "}" {
            flush(&mut state, &mut model);
            continue;
        }

        match &mut state {
            State::Top => {
                if let Some((kw, rest)) = line.split_once(char::is_whitespace) {
                    if let Some(kind) = req_kind(kw) {
                        state = State::Req(Requirement {
                            name: rest.trim().to_string(),
                            kind: kind.to_string(),
                            id: None,
                            text: None,
                            risk: None,
                            verify: None,
                        });
                        continue;
                    }
                    if kw == "element" {
                        state = State::Elem(ElementNode {
                            name: rest.trim().to_string(),
                            kind: None,
                            docref: None,
                        });
                        continue;
                    }
                }
                // An inline `src - verb -> tgt`.
                if let Some(r) = parse_inline_rel(line) {
                    model.relationships.push(r);
                }
            }
            State::Req(r) => {
                if let Some((k, v)) = line.split_once(':') {
                    let v = unquote(v);
                    match k.trim() {
                        "id" => r.id = Some(v),
                        "text" => r.text = Some(v),
                        "risk" => r.risk = Some(v),
                        "verifymethod" => r.verify = Some(v),
                        _ => {}
                    }
                }
            }
            State::Elem(e) => {
                if let Some((k, v)) = line.split_once(':') {
                    let v = unquote(v);
                    match k.trim() {
                        "type" => e.kind = Some(v),
                        "docref" => e.docref = Some(v),
                        _ => {}
                    }
                }
            }
            State::RelBlock { verb, src, tgt } => {
                if let Some((k, v)) = line.split_once(':') {
                    let v = unquote(v);
                    match k.trim() {
                        "type" => *verb = Some(v),
                        "source" => *src = Some(v),
                        "target" => *tgt = Some(v),
                        _ => {}
                    }
                }
            }
        }
    }
    // Flush a block left open by a missing closing brace.
    flush(&mut state, &mut model);
    model
}

/// Commits the in-progress block in `state` to `model` and resets to
/// [`State::Top`]. A relationship block only commits when its three fields
/// were all present and the verb is recognised.
fn flush(state: &mut State, model: &mut Model) {
    match std::mem::replace(state, State::Top) {
        State::Top => {}
        State::Req(r) => {
            if !r.name.is_empty() {
                model.requirements.push(r);
            }
        }
        State::Elem(e) => {
            if !e.name.is_empty() {
                model.elements.push(e);
            }
        }
        State::RelBlock { verb, src, tgt } => {
            if let (Some(v), Some(s), Some(t)) = (verb, src, tgt) {
                if is_verb(&v) {
                    model.relationships.push(Relationship {
                        src: s,
                        tgt: t,
                        verb: v,
                    });
                }
            }
        }
    }
}

/// The fixed cell size of a node box and the inter-box gutter.
const N_W: i32 = 24;
const N_H: i32 = 7;
const GUT: i32 = 1;

/// Ceiling of `a / b` for non-negative `i32`s (`i32::div_ceil` is unstable on
/// the pinned toolchain).
fn div_ceil_i32(a: i32, b: i32) -> i32 {
    if b <= 0 {
        return a;
    }
    (a + b - 1) / b
}

/// A roughly-square, capped column count for `n` boxes packed in a grid.
fn grid_cols(n: usize) -> i32 {
    if n == 0 {
        return 1;
    }
    let mut c = 1;
    while c * c < n {
        c += 1;
    }
    (c as i32).clamp(1, 3)
}

/// Draws one requirement box at `(x, y)`.
fn draw_req(s: &mut Surface, x: i32, y: i32, r: &Requirement, theme: &MermaidTheme, base: Style) {
    let border = base.patch(theme.node_border);
    let text = base.patch(theme.node_label);
    let stereo = base.patch(theme.edge_label);
    s.rect(x, y, N_W, N_H, BoxStyle::Square, border);
    let iw = N_W - 2;
    s.text_centered(x + 1, y + 1, iw, &format!("«{}»", r.kind), stereo);
    s.text_centered(x + 1, y + 2, iw, &r.name, text);
    let mut row = 3;
    let mut put = |s: &mut Surface, label: &str, val: &Option<String>| {
        if let Some(v) = val {
            if row < N_H - 1 {
                s.text_clipped(x + 1, y + row, &format!("{label}: {v}"), iw, text);
                row += 1;
            }
        }
    };
    put(s, "id", &r.id);
    put(s, "text", &r.text);
    put(s, "risk", &r.risk);
    put(s, "verify", &r.verify);
}

/// Draws one element box at `(x, y)`.
fn draw_elem(s: &mut Surface, x: i32, y: i32, e: &ElementNode, theme: &MermaidTheme, base: Style) {
    let border = base.patch(theme.node_border);
    let text = base.patch(theme.node_label);
    let stereo = base.patch(theme.edge_label);
    s.rect(x, y, N_W, N_H, BoxStyle::Round, border);
    let iw = N_W - 2;
    s.text_centered(x + 1, y + 1, iw, "«element»", stereo);
    s.text_centered(x + 1, y + 2, iw, &e.name, text);
    let mut row = 3;
    if let Some(t) = &e.kind {
        s.text_clipped(x + 1, y + row, &format!("type: {t}"), iw, text);
        row += 1;
    }
    if let Some(d) = &e.docref {
        if row < N_H - 1 {
            s.text_clipped(x + 1, y + row, &format!("docref: {d}"), iw, text);
        }
    }
}

/// Renders a `requirementDiagram` Mermaid diagram from `src` into `area`.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let model = parse(src);
    if model.is_empty() {
        super::diagram_placeholder("requirement", "no nodes", area, buf, base, theme);
        return;
    }

    let total_nodes = model.requirements.len() + model.elements.len();
    let cols = grid_cols(total_nodes);
    let rows = div_ceil_i32(total_nodes as i32, cols).max(1);

    let grid_w = cols * (N_W + GUT) - GUT;
    let grid_h = rows * (N_H + GUT) - GUT;

    let mut surf_w = grid_w + 2;
    let rel_h = if model.relationships.is_empty() {
        0
    } else {
        model.relationships.len() as i32 + 2
    };
    for r in &model.relationships {
        let l = format!("{} ──▸ {}  «{}»", r.src, r.tgt, r.verb);
        surf_w = surf_w.max(l.chars().count() as i32 + 2);
    }
    let surf_h = grid_h + rel_h + 1;

    let mut s = Surface::new(surf_w, surf_h.max(1));
    let edge_st = base.patch(theme.edge);
    let edge_lbl = base.patch(theme.edge_label);

    // Requirements first, then elements — a stable source-ish order packed
    // into the grid left-to-right, top-to-bottom.
    let mut idx = 0;
    let x0 = 1;
    let y0 = 0;
    for r in &model.requirements {
        let (cx, cy) = (idx % cols, idx / cols);
        draw_req(
            &mut s,
            x0 + cx * (N_W + GUT),
            y0 + cy * (N_H + GUT),
            r,
            theme,
            base,
        );
        idx += 1;
    }
    for e in &model.elements {
        let (cx, cy) = (idx % cols, idx / cols);
        draw_elem(
            &mut s,
            x0 + cx * (N_W + GUT),
            y0 + cy * (N_H + GUT),
            e,
            theme,
            base,
        );
        idx += 1;
    }

    if !model.relationships.is_empty() {
        let mut y = grid_h + 1;
        s.text(0, y, "relationships:", edge_lbl);
        y += 1;
        for r in &model.relationships {
            // Verification kinds are dashed; structural kinds solid.
            let dashed = matches!(r.verb.as_str(), "traces" | "verifies");
            let arrow = if dashed { "╴╴▸" } else { "──▸" };
            let line = format!("{} {arrow} {}  «{}»", r.src, r.tgt, r.verb);
            s.text(1, y, &line, edge_st);
            y += 1;
        }
    }

    s.blit(area, buf, base);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Position, Widget};

    use crate::Mermaid;

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row.
    fn lines<W: Widget>(widget: W, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    // --- parser tests ------------------------------------------------------

    #[test]
    fn parses_requirement_block_fields() {
        let m = parse(
            "requirementDiagram\n\
             requirement test_req {\n\
             id: 1\n\
             text: the text\n\
             risk: high\n\
             verifymethod: test\n\
             }",
        );
        assert_eq!(m.requirements.len(), 1);
        let r = &m.requirements[0];
        assert_eq!(r.name, "test_req");
        assert_eq!(r.kind, "requirement");
        assert_eq!(r.id.as_deref(), Some("1"));
        assert_eq!(r.text.as_deref(), Some("the text"));
        assert_eq!(r.risk.as_deref(), Some("high"));
        assert_eq!(r.verify.as_deref(), Some("test"));
    }

    #[test]
    fn quoted_text_is_unquoted() {
        let m = parse(
            "requirementDiagram\n\
             requirement r {\n\
             text: \"the quoted text\"\n\
             }",
        );
        assert_eq!(m.requirements[0].text.as_deref(), Some("the quoted text"));
    }

    #[test]
    fn typed_requirement_keywords_keep_their_stereotype() {
        let m = parse(
            "requirementDiagram\n\
             physicalRequirement p { id: 2 }\n\
             designConstraint d { id: 3 }",
        );
        assert_eq!(m.requirements[0].kind, "physicalRequirement");
        assert_eq!(m.requirements[1].kind, "designConstraint");
    }

    #[test]
    fn parses_element_block() {
        let m = parse(
            "requirementDiagram\n\
             element test_entity {\n\
             type: simulation\n\
             docref: a.b\n\
             }",
        );
        assert_eq!(m.elements.len(), 1);
        assert_eq!(m.elements[0].name, "test_entity");
        assert_eq!(m.elements[0].kind.as_deref(), Some("simulation"));
        assert_eq!(m.elements[0].docref.as_deref(), Some("a.b"));
    }

    #[test]
    fn parses_inline_relationship() {
        let m = parse("requirementDiagram\ntest_req - satisfies -> test_entity");
        assert_eq!(m.relationships.len(), 1);
        assert_eq!(m.relationships[0].src, "test_req");
        assert_eq!(m.relationships[0].verb, "satisfies");
        assert_eq!(m.relationships[0].tgt, "test_entity");
    }

    #[test]
    fn parses_block_relationship() {
        let m = parse(
            "requirementDiagram\n\
             {\n\
             type: derives\n\
             source: req2\n\
             target: req1\n\
             }",
        );
        assert_eq!(m.relationships.len(), 1);
        assert_eq!(m.relationships[0].verb, "derives");
        assert_eq!(m.relationships[0].src, "req2");
        assert_eq!(m.relationships[0].tgt, "req1");
    }

    #[test]
    fn unterminated_block_is_flushed_at_eof() {
        let m = parse("requirementDiagram\nrequirement r {\nid: 9");
        assert_eq!(m.requirements.len(), 1);
        assert_eq!(m.requirements[0].id.as_deref(), Some("9"));
    }

    #[test]
    fn lenient_skips_garbage_and_bad_verb() {
        let m = parse(
            "requirementDiagram\n\
             total nonsense here\n\
             a - notaverb -> b\n\
             requirement good { id: 1 }",
        );
        assert_eq!(m.requirements.len(), 1);
        assert!(m.relationships.is_empty());
    }

    #[test]
    fn empty_or_header_only_is_empty() {
        assert!(parse("").is_empty());
        assert!(parse("requirementDiagram").is_empty());
        assert!(parse("requirementDiagram\n%% just a comment").is_empty());
    }

    // --- render snapshot tests --------------------------------------------

    #[test]
    fn empty_renders_placeholder() {
        let out = lines(Mermaid::new("requirementDiagram"), 44, 5);
        assert!(out.contains("mermaid · requirement: no nodes"), "{out}");
    }

    #[test]
    fn requirement_box_shows_stereotype_name_and_rows() {
        let src = "requirementDiagram\n\
                   requirement test_req {\n\
                   id: 1\n\
                   text: the text\n\
                   risk: high\n\
                   verifymethod: test\n\
                   }";
        let out = lines(Mermaid::new(src), 40, 12);
        assert!(out.contains("«requirement»"), "{out}");
        assert!(out.contains("test_req"), "{out}");
        assert!(out.contains("id: 1"), "{out}");
        assert!(out.contains("risk: high"), "{out}");
        assert!(out.contains('┌'), "{out}");
    }

    #[test]
    fn element_box_shows_type_and_docref() {
        let src = "requirementDiagram\n\
                   element test_entity {\n\
                   type: simulation\n\
                   docref: a.b\n\
                   }";
        let out = lines(Mermaid::new(src), 40, 10);
        assert!(out.contains("«element»"), "{out}");
        assert!(out.contains("test_entity"), "{out}");
        assert!(out.contains("type: simulation"), "{out}");
        assert!(out.contains("docref: a.b"), "{out}");
        // Rounded element corner.
        assert!(out.contains('╭'), "{out}");
    }

    #[test]
    fn relationship_listed_with_label_and_solid_arrow() {
        let src = "requirementDiagram\n\
                   requirement a { id: 1 }\n\
                   element b { type: t }\n\
                   a - satisfies -> b";
        let out = lines(Mermaid::new(src), 50, 20);
        assert!(out.contains("relationships:"), "{out}");
        assert!(out.contains("«satisfies»"), "{out}");
        assert!(out.contains("──▸"), "{out}");
    }

    #[test]
    fn verification_relationship_is_dashed() {
        let src = "requirementDiagram\n\
                   requirement a { id: 1 }\n\
                   element b { type: t }\n\
                   b - verifies -> a";
        let out = lines(Mermaid::new(src), 50, 20);
        assert!(out.contains("«verifies»"), "{out}");
        assert!(out.contains("╴╴▸"), "{out}");
    }

    #[test]
    fn tiny_area_does_not_panic() {
        let src = "requirementDiagram\n\
                   requirement r { id: 1 \n text: t \n risk: low \n verifymethod: test }\n\
                   element e { type: x \n docref: y }\n\
                   r - satisfies -> e";
        let _ = lines(Mermaid::new(src), 1, 1);
        let _ = lines(Mermaid::new(src), 5, 1);
        let _ = lines(Mermaid::new(src), 7, 3);
    }

    #[test]
    fn deterministic_across_repeated_renders() {
        let src = "requirementDiagram\n\
                   requirement r1 { id: 1 \n text: first \n risk: high \n verifymethod: test }\n\
                   requirement r2 { id: 2 \n text: second }\n\
                   element e { type: sim \n docref: d.e }\n\
                   r1 - derives -> r2\n\
                   e - satisfies -> r1";
        let a = lines(Mermaid::new(src), 60, 28);
        let b = lines(Mermaid::new(src), 60, 28);
        assert_eq!(a, b);
    }
}
