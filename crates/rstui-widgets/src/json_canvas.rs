//! [`JsonCanvas`] — a read-only [JSON Canvas 1.0](https://jsoncanvas.org/)
//! renderer: the open infinite-canvas format where **the author places
//! every node at an explicit `(x, y, width, height)`**.
//!
//! # Why this exists — explicit placement for an LLM
//!
//! Mermaid and the Structurizr DSL are *auto-layout*: they describe
//! structure and a layout engine positions it; a model cannot say "put this
//! box here". [JSON Canvas](https://jsoncanvas.org/spec/1.0/) is the
//! complement — a tiny JSON document of `nodes` and `edges` where each node
//! carries integer pixel coordinates, so an AI tool that *wants* to control
//! the layout emits JSON Canvas instead. It is the format Obsidian Canvas
//! writes, so models already know it.
//!
//! # Why a hand-written parser
//!
//! rstui stays dependency-free below the backend
//! ([ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)
//! §4): a widget never pulls a crate (here `serde_json`) pre-emptively, and
//! JSON Canvas is a fixed, shallow schema. So — exactly like
//! [`Mermaid`](crate::Mermaid) and [`Structurizr`](crate::Structurizr) —
//! this carries a small **total** hand-written JSON scanner (objects,
//! arrays, strings with escapes, numbers, `true`/`false`/`null`) and a
//! lenient mapping onto the JSON Canvas schema; it shares the deterministic
//! crate-internal `diagram` `Surface` the other diagram widgets render onto.
//! Malformed input never panics: a bad node/edge is skipped, and a
//! document that is not a JSON object renders a clear placeholder.
//!
//! # Supported subset (the whole 1.0 spec)
//!
//! - Top level `{ "nodes": [...], "edges": [...] }` (both optional).
//! - **Nodes**: the generic `id`/`x`/`y`/`width`/`height`/`color`, and all
//!   four types — `text` (markdown shown as plain text, wrapped/clipped),
//!   `file` (path, with optional `#subpath`), `link` (url), and `group`
//!   (an enclosing labelled box, drawn behind its members per the spec's
//!   array = ascending z-index rule).
//! - **Edges**: `fromNode`/`toNode`, optional `fromSide`/`toSide`
//!   (`top`/`right`/`bottom`/`left`, else the facing side is chosen),
//!   `fromEnd`/`toEnd` (`none`/`arrow`, defaulting `none`/`arrow` per
//!   spec), `label`, and `color`.
//! - **Colour**: the presets `"1"`–`"6"` (red/orange/yellow/green/cyan/
//!   purple) and `"#rrggbb"` hex (snapped to the nearest terminal colour
//!   via the shared [`css_color`] rule).
//!
//! # The pixel canvas → a character grid
//!
//! A JSON Canvas is an unbounded pixel plane; a terminal is a small cell
//! grid. The renderer computes the bounding box of every node and scales it
//! to fit the draw area (independent x/y scale — a deterministic overview,
//! documented, not pixel-faithful), so the *relative* placement the author
//! chose is preserved and snapshot-testable through a [`Buffer`].
//!
//! ```
//! use rstui_core::{Buffer, Rect, Widget};
//! use rstui_widgets::JsonCanvas;
//!
//! let src = r#"{"nodes":[
//!   {"id":"a","type":"text","text":"Start","x":0,"y":0,"width":120,"height":60},
//!   {"id":"b","type":"text","text":"End","x":300,"y":0,"width":120,"height":60}
//! ],"edges":[{"id":"e","fromNode":"a","toNode":"b"}]}"#;
//! let canvas = JsonCanvas::parse(src).unwrap();
//! assert_eq!(canvas.nodes.len(), 2);
//! assert_eq!(canvas.edges.len(), 1);
//!
//! let mut buf = Buffer::empty(Rect::new(0, 0, 40, 10));
//! JsonCanvas::new(src).render(buf.area(), &mut buf);
//! ```

use std::borrow::Cow;

use crate::block::Block;
use crate::diagram::{BoxStyle, Surface};
use crate::mermaid::css_color;
use rstui_core::{Buffer, Color, Rect, Style, Widget};

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

/// A JSON Canvas colour: a preset slot `1..=6` or a 24-bit hex, mapped to
/// the nearest terminal [`Color`] deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasColor {
    /// One of the six spec presets (`"1"`..`"6"`).
    Preset(u8),
    /// A `#rrggbb` value, already snapped to the nearest ANSI [`Color`].
    Rgb(Color),
}

impl CanvasColor {
    /// Parses a JSON Canvas `canvasColor` string (a preset digit or hex).
    fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(d) = s.strip_prefix('#') {
            // Reuse the shared CSS→ANSI snap so colours match the other
            // diagram widgets.
            return css_color(&format!("#{d}")).map(Self::Rgb);
        }
        match s.parse::<u8>() {
            Ok(n @ 1..=6) => Some(Self::Preset(n)),
            _ => None,
        }
    }

    /// The terminal [`Color`] this maps to (preset slots use the
    /// spec's red/orange/yellow/green/cyan/purple order).
    fn color(self) -> Color {
        match self {
            Self::Preset(1) => Color::Red,
            Self::Preset(2) => Color::Yellow, // "orange" → nearest ANSI
            Self::Preset(3) => Color::LightYellow,
            Self::Preset(4) => Color::Green,
            Self::Preset(5) => Color::Cyan,
            Self::Preset(6) => Color::Magenta,
            Self::Preset(_) => Color::Gray,
            Self::Rgb(c) => c,
        }
    }
}

/// What a [`CanvasNode`] is — the four JSON Canvas node types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// `text` — plain text with Markdown syntax.
    Text(String),
    /// `file` — a file path with an optional `#subpath`.
    File {
        /// The file path.
        file: String,
        /// An optional in-file anchor (`#heading`/`#^block`).
        subpath: Option<String>,
    },
    /// `link` — an external URL.
    Link(String),
    /// `group` — an enclosing region with an optional label.
    Group {
        /// The optional group label drawn on its border.
        label: Option<String>,
    },
}

/// One placed node: its `id`, [`NodeKind`], the explicit integer
/// `x`/`y`/`width`/`height` (the author's chosen placement; `x`/`y` may be
/// negative), and an optional [`CanvasColor`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasNode {
    /// The unique node id (edges reference it).
    pub id: String,
    /// The node's type-specific payload.
    pub kind: NodeKind,
    /// Left edge, in canvas pixels.
    pub x: i64,
    /// Top edge, in canvas pixels.
    pub y: i64,
    /// Width, in canvas pixels.
    pub width: i64,
    /// Height, in canvas pixels.
    pub height: i64,
    /// The node's colour, if set.
    pub color: Option<CanvasColor>,
}

/// A node side an edge attaches to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Top edge.
    Top,
    /// Right edge.
    Right,
    /// Bottom edge.
    Bottom,
    /// Left edge.
    Left,
}

impl Side {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "top" => Some(Self::Top),
            "right" => Some(Self::Right),
            "bottom" => Some(Self::Bottom),
            "left" => Some(Self::Left),
            _ => None,
        }
    }
}

/// An edge endpoint shape (`none`/`arrow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// No arrowhead.
    None,
    /// An arrowhead.
    Arrow,
}

/// One connection between two nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasEdge {
    /// The unique edge id.
    pub id: String,
    /// The source node id.
    pub from_node: String,
    /// The side the edge leaves the source from, if pinned.
    pub from_side: Option<Side>,
    /// The source endpoint shape (spec default `none`).
    pub from_end: Endpoint,
    /// The destination node id.
    pub to_node: String,
    /// The side the edge enters the destination on, if pinned.
    pub to_side: Option<Side>,
    /// The destination endpoint shape (spec default `arrow`).
    pub to_end: Endpoint,
    /// The edge colour, if set.
    pub color: Option<CanvasColor>,
    /// The optional edge label.
    pub label: Option<String>,
}

/// A parsed JSON Canvas document — the public parse result, so a host or
/// test can assert structure independently of layout. Nodes are in document
/// order (= ascending z-index per the spec: later nodes draw on top).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Canvas {
    /// Every node, in document/z order.
    pub nodes: Vec<CanvasNode>,
    /// Every edge, in document order.
    pub edges: Vec<CanvasEdge>,
}

impl Canvas {
    /// The node with `id`, if present.
    fn node(&self, id: &str) -> Option<&CanvasNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

/// Why [`JsonCanvas::parse`] could not produce a [`Canvas`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonCanvasError {
    /// The source was not a JSON object (the only hard error — a
    /// well-formed object with no usable nodes still parses, to an empty
    /// canvas, and renders a placeholder).
    NotAnObject,
}

impl std::fmt::Display for JsonCanvasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("expected a JSON object with `nodes`/`edges`")
    }
}

impl std::error::Error for JsonCanvasError {}

// ---------------------------------------------------------------------------
// Minimal, total JSON scanner (zero-dep, ADR 0002 §4)
// ---------------------------------------------------------------------------

/// A parsed JSON value — only what the JSON Canvas schema needs.
#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }
    fn as_i64(&self) -> Option<i64> {
        match self {
            #[allow(clippy::cast_possible_truncation)]
            Self::Num(n) => Some(*n as i64),
            _ => None,
        }
    }
    fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Self::Arr(a) => Some(a),
            _ => None,
        }
    }
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// A linear, total JSON parser over the input bytes. Any malformed token
/// stops the parse and yields what was read so far (the caller treats a
/// failed top-level parse as [`JsonCanvasError::NotAnObject`]); it never
/// panics and never recurses unbounded on hostile input (depth-capped).
struct JsonParser<'a> {
    s: &'a [u8],
    i: usize,
    depth: u32,
}

impl<'a> JsonParser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            s: s.as_bytes(),
            i: 0,
            depth: 0,
        }
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn value(&mut self) -> Option<Json> {
        if self.depth > 64 {
            return None;
        }
        self.ws();
        let c = *self.s.get(self.i)?;
        match c {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(Json::Str),
            b't' | b'f' => self.boolean(),
            b'n' => self.null(),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn object(&mut self) -> Option<Json> {
        self.depth += 1;
        self.i += 1; // {
        let mut out = Vec::new();
        loop {
            self.ws();
            match self.s.get(self.i)? {
                b'}' => {
                    self.i += 1;
                    self.depth -= 1;
                    return Some(Json::Obj(out));
                }
                b',' => {
                    self.i += 1;
                }
                b'"' => {
                    let key = self.string()?;
                    self.ws();
                    if self.s.get(self.i)? != &b':' {
                        return None;
                    }
                    self.i += 1;
                    let val = self.value()?;
                    out.push((key, val));
                }
                _ => return None,
            }
        }
    }

    fn array(&mut self) -> Option<Json> {
        self.depth += 1;
        self.i += 1; // [
        let mut out = Vec::new();
        loop {
            self.ws();
            match self.s.get(self.i)? {
                b']' => {
                    self.i += 1;
                    self.depth -= 1;
                    return Some(Json::Arr(out));
                }
                b',' => {
                    self.i += 1;
                }
                _ => out.push(self.value()?),
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        self.i += 1; // opening "
        let mut out = String::new();
        loop {
            let c = *self.s.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return Some(out),
                b'\\' => {
                    let e = *self.s.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            let hex = self.s.get(self.i..self.i + 4)?;
                            let cp =
                                u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                            self.i += 4;
                            out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                        }
                        _ => return None,
                    }
                }
                // A raw UTF-8 byte: re-decode from the input so multibyte
                // characters survive intact.
                _ => {
                    let start = self.i - 1;
                    while self.i < self.s.len() && self.s[self.i] != b'"' && self.s[self.i] != b'\\'
                    {
                        self.i += 1;
                    }
                    out.push_str(std::str::from_utf8(&self.s[start..self.i]).ok()?);
                }
            }
        }
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.i;
        if self.s.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        while self.i < self.s.len()
            && matches!(
                self.s[self.i],
                b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-'
            )
        {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()?
            .parse::<f64>()
            .ok()
            .map(Json::Num)
    }

    fn boolean(&mut self) -> Option<Json> {
        if self.s[self.i..].starts_with(b"true") {
            self.i += 4;
            Some(Json::Bool(true))
        } else if self.s[self.i..].starts_with(b"false") {
            self.i += 5;
            Some(Json::Bool(false))
        } else {
            None
        }
    }

    fn null(&mut self) -> Option<Json> {
        if self.s[self.i..].starts_with(b"null") {
            self.i += 4;
            Some(Json::Null)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Schema mapping
// ---------------------------------------------------------------------------

/// Maps a parsed [`Json`] document to a [`Canvas`], lenient: a node/edge
/// missing a required field (or of an unknown type) is skipped, not fatal.
fn to_canvas(root: &Json) -> Canvas {
    let mut canvas = Canvas::default();
    if let Some(nodes) = root.get("nodes").and_then(Json::as_arr) {
        for n in nodes {
            if let Some(node) = node_from(n) {
                canvas.nodes.push(node);
            }
        }
    }
    if let Some(edges) = root.get("edges").and_then(Json::as_arr) {
        for e in edges {
            if let Some(edge) = edge_from(e) {
                canvas.edges.push(edge);
            }
        }
    }
    canvas
}

fn node_from(v: &Json) -> Option<CanvasNode> {
    let id = v.get("id")?.as_str()?.to_string();
    let ty = v.get("type")?.as_str()?;
    let kind = match ty {
        "text" => NodeKind::Text(v.get("text")?.as_str()?.to_string()),
        "file" => NodeKind::File {
            file: v.get("file")?.as_str()?.to_string(),
            subpath: v.get("subpath").and_then(Json::as_str).map(str::to_string),
        },
        "link" => NodeKind::Link(v.get("url")?.as_str()?.to_string()),
        "group" => NodeKind::Group {
            label: v.get("label").and_then(Json::as_str).map(str::to_string),
        },
        _ => return None,
    };
    Some(CanvasNode {
        id,
        kind,
        x: v.get("x")?.as_i64()?,
        y: v.get("y")?.as_i64()?,
        width: v.get("width")?.as_i64()?.max(1),
        height: v.get("height")?.as_i64()?.max(1),
        color: v
            .get("color")
            .and_then(Json::as_str)
            .and_then(CanvasColor::parse),
    })
}

fn edge_from(v: &Json) -> Option<CanvasEdge> {
    let end = |k: &str, default: Endpoint| match v.get(k).and_then(Json::as_str) {
        Some("arrow") => Endpoint::Arrow,
        Some("none") => Endpoint::None,
        _ => default,
    };
    Some(CanvasEdge {
        id: v.get("id")?.as_str()?.to_string(),
        from_node: v.get("fromNode")?.as_str()?.to_string(),
        from_side: v
            .get("fromSide")
            .and_then(Json::as_str)
            .and_then(Side::parse),
        from_end: end("fromEnd", Endpoint::None),
        to_node: v.get("toNode")?.as_str()?.to_string(),
        to_side: v.get("toSide").and_then(Json::as_str).and_then(Side::parse),
        to_end: end("toEnd", Endpoint::Arrow),
        color: v
            .get("color")
            .and_then(Json::as_str)
            .and_then(CanvasColor::parse),
        label: v.get("label").and_then(Json::as_str).map(str::to_string),
    })
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

/// The styles [`JsonCanvas`] paints with. Each is a *patch* over the base
/// style (the shared diagram cascade); a node/edge's own
/// [`CanvasColor`] overrides the themed colour for that element only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonCanvasTheme {
    /// A text/file/link node's border.
    pub node: Style,
    /// Node body text.
    pub label: Style,
    /// A group container's border + label.
    pub group: Style,
    /// Edge connector lines + arrowheads.
    pub edge: Style,
    /// An edge's label.
    pub edge_label: Style,
    /// The placeholder shown for a non-object / empty canvas.
    pub placeholder: Style,
}

impl Default for JsonCanvasTheme {
    fn default() -> Self {
        Self {
            node: Style::new().fg(Color::Cyan),
            label: Style::new(),
            group: Style::new().fg(Color::Magenta),
            edge: Style::new().fg(Color::DarkGray),
            edge_label: Style::new().fg(Color::Yellow),
            placeholder: Style::new().fg(Color::Red),
        }
    }
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// A read-only JSON Canvas view: parses its source and draws the placed
/// nodes/edges scaled to fit the area.
#[derive(Debug, Clone)]
pub struct JsonCanvas<'a> {
    source: Cow<'a, str>,
    block: Option<Block<'a>>,
    style: Style,
    theme: JsonCanvasTheme,
    /// A caller-held pre-parsed canvas ([`from_parsed`](Self::from_parsed)):
    /// when set, `render` lays it out directly and **never scans the JSON**
    /// — the parse-free seam (perf-review-3 R3-3, the
    /// [`Mermaid::from_graph`](crate::Mermaid::from_graph) precedent).
    canvas: Option<&'a Canvas>,
}

impl<'a> JsonCanvas<'a> {
    /// A JSON Canvas view of `source`, default theme, no block.
    #[must_use]
    pub fn new(source: impl Into<Cow<'a, str>>) -> Self {
        Self {
            source: source.into(),
            block: None,
            style: Style::new(),
            theme: JsonCanvasTheme::default(),
            canvas: None,
        }
    }

    /// A view of a **pre-parsed** [`Canvas`] the caller holds in its model —
    /// `render` lays it out directly and **never runs the JSON scanner**
    /// (which `new(..)` does every frame). Parse once with
    /// [`parse`](Self::parse) when the source changes, render parse-free
    /// every frame.
    ///
    /// The parse-free seam perf-review-3 R3-3 calls for, identical in shape
    /// to [`Mermaid::from_graph`](crate::Mermaid::from_graph): purely
    /// additive, byte-identical to `new(src)` for a source that parses to
    /// the same canvas (gate-enforced).
    #[must_use]
    pub fn from_parsed(canvas: &'a Canvas) -> Self {
        Self {
            source: Cow::Borrowed(""),
            block: None,
            style: Style::new(),
            theme: JsonCanvasTheme::default(),
            canvas: Some(canvas),
        }
    }

    /// Frames the canvas in `block`; content renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`] beneath the theme cascade.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Replaces the [`JsonCanvasTheme`].
    #[must_use]
    pub fn theme(mut self, theme: JsonCanvasTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Parses `source` into a [`Canvas`] without rendering. The only error
    /// is a source that is not a JSON object
    /// ([`JsonCanvasError::NotAnObject`]); malformed nodes/edges are
    /// skipped, never fatal.
    pub fn parse(source: impl AsRef<str>) -> Result<Canvas, JsonCanvasError> {
        let mut p = JsonParser::new(source.as_ref());
        match p.value() {
            Some(root @ Json::Obj(_)) => Ok(to_canvas(&root)),
            _ => Err(JsonCanvasError::NotAnObject),
        }
    }
}

impl Widget for JsonCanvas<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if let Some(b) = &self.block {
            b.render_ref(area, buf);
        }
        if inner.is_empty() {
            return;
        }
        buf.set_style(inner, self.style);

        // Parse-free fast path: a caller-held pre-parsed canvas lays out
        // directly (the R3-3 seam), byte-identical to the parse path.
        // Otherwise scan the JSON as before.
        let owned;
        let canvas: &Canvas = match self.canvas {
            Some(c) => c,
            None => {
                owned = JsonCanvas::parse(self.source.as_ref()).unwrap_or_default();
                &owned
            }
        };
        let surface = lay_out(canvas, inner.width as i32, inner.height as i32, &self.theme);
        surface.blit(inner, buf, self.style);
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// A node's scaled cell rectangle.
struct Placed {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Placed {
    fn anchor(&self, side: Side) -> (i32, i32) {
        match side {
            Side::Top => (self.x + self.w / 2, self.y),
            Side::Bottom => (self.x + self.w / 2, self.y + self.h - 1),
            Side::Left => (self.x, self.y + self.h / 2),
            Side::Right => (self.x + self.w - 1, self.y + self.h / 2),
        }
    }
    fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

/// Builds the surface: scale the canvas bounding box to fit `w`×`h`, place
/// every node, draw groups (behind) then nodes (front) per z-order, then
/// the edges between scaled side-anchors.
fn lay_out(canvas: &Canvas, w: i32, h: i32, theme: &JsonCanvasTheme) -> Surface {
    if canvas.nodes.is_empty() || w < 2 || h < 2 {
        let msg = "[json-canvas: no placed nodes]";
        let mut s = Surface::new((msg.len() as i32).min(w.max(2)), 1);
        s.text(0, 0, msg, theme.placeholder);
        return s;
    }

    // Canvas-pixel bounding box over every node.
    let min_x = canvas.nodes.iter().map(|n| n.x).min().unwrap();
    let min_y = canvas.nodes.iter().map(|n| n.y).min().unwrap();
    let max_x = canvas.nodes.iter().map(|n| n.x + n.width).max().unwrap();
    let max_y = canvas.nodes.iter().map(|n| n.y + n.height).max().unwrap();
    let span_x = (max_x - min_x).max(1) as f64;
    let span_y = (max_y - min_y).max(1) as f64;

    let surf_w = w;
    let surf_h = h;
    // Independent x/y scale (a deterministic overview — terminal cells are
    // not square; preserving the author's *relative* placement matters more
    // than pixel aspect).
    let sx = (surf_w - 1) as f64 / span_x;
    let sy = (surf_h - 1) as f64 / span_y;

    let map = |nx: i64, ny: i64, nw: i64, nh: i64| -> Placed {
        let px = ((nx - min_x) as f64 * sx).round() as i32;
        let py = ((ny - min_y) as f64 * sy).round() as i32;
        let pw = ((nw as f64 * sx).round() as i32).max(3).min(surf_w);
        let ph = ((nh as f64 * sy).round() as i32).max(3).min(surf_h);
        Placed {
            x: px.clamp(0, (surf_w - pw).max(0)),
            y: py.clamp(0, (surf_h - ph).max(0)),
            w: pw,
            h: ph,
        }
    };

    let mut s = Surface::new(surf_w, surf_h);

    // 1. Connector lines first (node borders then draw cleanly over them).
    for e in &canvas.edges {
        let (Some(a), Some(b)) = (canvas.node(&e.from_node), canvas.node(&e.to_node)) else {
            continue;
        };
        let pa = map(a.x, a.y, a.width, a.height);
        let pb = map(b.x, b.y, b.width, b.height);
        draw_edge_line(&mut s, &pa, &pb, e, theme);
    }

    // 2. Nodes in document order = ascending z (groups are usually first,
    // so they land behind their members, per the spec).
    for n in &canvas.nodes {
        let p = map(n.x, n.y, n.width, n.height);
        draw_node(&mut s, &p, n, theme);
    }

    // 3. Arrowheads last, *on top* of the node borders, so a head landing
    // on the box edge it points into stays visible.
    for e in &canvas.edges {
        let (Some(a), Some(b)) = (canvas.node(&e.from_node), canvas.node(&e.to_node)) else {
            continue;
        };
        let pa = map(a.x, a.y, a.width, a.height);
        let pb = map(b.x, b.y, b.width, b.height);
        let (sx, sy, fs, ex, ey, ts) = edge_geom(&pa, &pb, e);
        let st = match e.color {
            Some(c) => theme.edge.fg(c.color()),
            None => theme.edge,
        };
        if e.to_end == Endpoint::Arrow {
            s.set(ex, ey, arrow_for(ts), st);
        }
        if e.from_end == Endpoint::Arrow {
            s.set(sx, sy, arrow_for(fs), st);
        }
    }
    s
}

/// The two side-anchor points and the sides an edge connects, resolving an
/// unpinned `fromSide`/`toSide` to the facing side.
fn edge_geom(a: &Placed, b: &Placed, e: &CanvasEdge) -> (i32, i32, Side, i32, i32, Side) {
    let fs = e.from_side.unwrap_or_else(|| facing_side(a, b));
    let ts = e.to_side.unwrap_or_else(|| facing_side(b, a));
    let (sx, sy) = a.anchor(fs);
    let (ex, ey) = b.anchor(ts);
    (sx, sy, fs, ex, ey, ts)
}

/// The side of `from`'s rect facing `to` (used when an edge does not pin a
/// `fromSide`/`toSide`).
fn facing_side(from: &Placed, to: &Placed) -> Side {
    let (fx, fy) = from.center();
    let (tx, ty) = to.center();
    if (tx - fx).abs() >= (ty - fy).abs() {
        if tx >= fx { Side::Right } else { Side::Left }
    } else if ty >= fy {
        Side::Bottom
    } else {
        Side::Top
    }
}

/// Draws one edge's orthogonal connector (an L through the x-midpoint) and
/// its optional mid-label. Arrowheads are a separate later pass so they sit
/// on top of node borders.
fn draw_edge_line(
    s: &mut Surface,
    a: &Placed,
    b: &Placed,
    e: &CanvasEdge,
    theme: &JsonCanvasTheme,
) {
    let st = match e.color {
        Some(c) => theme.edge.fg(c.color()),
        None => theme.edge,
    };
    let (sx, sy, _fs, ex, ey, _ts) = edge_geom(a, b, e);
    let mx = (sx + ex) / 2;
    let xa = sx.min(mx);
    let xb = sx.max(mx);
    s.hline(xa, sy, xb - xa + 1, '─', st);
    let (vlo, vhi) = (sy.min(ey), sy.max(ey));
    s.vline(mx, vlo, vhi - vlo + 1, '│', st);
    let (xc, xd) = (mx.min(ex), mx.max(ex));
    s.hline(xc, ey, xd - xc + 1, '─', st);
    if let Some(lbl) = &e.label {
        s.text_clipped(mx + 1, (sy + ey) / 2, lbl, 16, theme.edge_label);
    }
}

/// The arrowhead glyph that points *into* a node entered on `side` (an edge
/// arriving at the left edge points right, etc.).
const fn arrow_for(side: Side) -> char {
    match side {
        Side::Top => '▼',
        Side::Bottom => '▲',
        Side::Left => '▶',
        Side::Right => '◀',
    }
}

/// Draws one node: a `group` is a dashed labelled container; `text`/`file`/
/// `link` are solid boxes carrying their payload, clipped to the box.
fn draw_node(s: &mut Surface, p: &Placed, n: &CanvasNode, theme: &JsonCanvasTheme) {
    let is_group = matches!(n.kind, NodeKind::Group { .. });
    let mut border = if is_group { theme.group } else { theme.node };
    if let Some(c) = n.color {
        border = border.fg(c.color());
    }
    let kind = if is_group {
        BoxStyle::Dashed
    } else {
        BoxStyle::Square
    };
    s.rect(p.x, p.y, p.w, p.h, kind, border);

    let iw = (p.w - 2).max(0);
    match &n.kind {
        NodeKind::Group { label } => {
            if let Some(l) = label {
                let tag: String = std::iter::once(' ')
                    .chain(l.chars())
                    .chain(std::iter::once(' '))
                    .collect();
                s.text_clipped(p.x + 1, p.y, &tag, iw, theme.group);
            }
        }
        NodeKind::Text(t) => {
            for (i, line) in t.lines().enumerate() {
                let yy = p.y + 1 + i as i32;
                if yy >= p.y + p.h - 1 {
                    break;
                }
                s.text_clipped(p.x + 1, yy, line, iw, theme.label);
            }
        }
        NodeKind::File { file, subpath } => {
            let shown = match subpath {
                Some(sp) => format!("▣ {file}{sp}"),
                None => format!("▣ {file}"),
            };
            s.text_clipped(p.x + 1, p.y + p.h / 2, &shown, iw, theme.label);
        }
        NodeKind::Link(url) => {
            s.text_clipped(
                p.x + 1,
                p.y + p.h / 2,
                &format!("🔗 {url}"),
                iw,
                theme.label,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Position;

    fn lines(widget: JsonCanvas<'_>, w: u16, h: u16) -> String {
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

    // `r##"…"##` because `"subpath":"#top"` contains the `"#` sequence
    // that would otherwise close an `r#"…"#` raw string early.
    const SAMPLE: &str = r##"{
      "nodes":[
        {"id":"g","type":"group","x":-300,"y":-460,"width":610,"height":200,"label":"JSON Canvas"},
        {"id":"logo","type":"file","file":"_site/logo.svg","x":-280,"y":-440,"width":217,"height":80},
        {"id":"learn","type":"text","text":"Learn more:\n- Apps\n- Spec","x":40,"y":-440,"width":250,"height":160,"color":"6"},
        {"id":"spec","type":"file","file":"spec/1.0.md","subpath":"#top","x":360,"y":-400,"width":400,"height":400}
      ],
      "edges":[
        {"id":"e1","fromNode":"logo","fromSide":"right","toNode":"learn","toSide":"left","label":"see"}
      ]
    }"##;

    // --- parser ------------------------------------------------------------

    #[test]
    fn parses_nodes_edges_kinds_coords_and_color() {
        let c = JsonCanvas::parse(SAMPLE).unwrap();
        assert_eq!(c.nodes.len(), 4);
        assert_eq!(c.edges.len(), 1);
        let g = &c.nodes[0];
        assert_eq!(g.id, "g");
        assert_eq!(g.x, -300);
        assert_eq!(g.width, 610);
        assert!(
            matches!(&g.kind, NodeKind::Group { label } if label.as_deref() == Some("JSON Canvas"))
        );
        let learn = c.nodes.iter().find(|n| n.id == "learn").unwrap();
        assert_eq!(learn.color, Some(CanvasColor::Preset(6)));
        assert!(matches!(&learn.kind, NodeKind::Text(t) if t.contains("Learn more")));
        let spec = c.nodes.iter().find(|n| n.id == "spec").unwrap();
        assert!(matches!(&spec.kind, NodeKind::File { file, subpath }
                if file == "spec/1.0.md" && subpath.as_deref() == Some("#top")));
        let e = &c.edges[0];
        assert_eq!(e.from_side, Some(Side::Right));
        assert_eq!(e.to_side, Some(Side::Left));
        assert_eq!(e.from_end, Endpoint::None); // spec default
        assert_eq!(e.to_end, Endpoint::Arrow); // spec default
        assert_eq!(e.label.as_deref(), Some("see"));
    }

    #[test]
    fn link_node_hex_color_and_escapes_parse() {
        // `r##"…"##` because the JSON hex colour `"#ff0000"` contains the
        // `"#` sequence that would close an `r#"…"#` raw string.
        let src = r##"{"nodes":[{"id":"a","type":"link","url":"https://x.test","x":0,"y":0,"width":100,"height":40,"color":"#ff0000"},
                     {"id":"b","type":"text","text":"a \"quote\" and é","x":0,"y":80,"width":100,"height":40}]}"##;
        let c = JsonCanvas::parse(src).unwrap();
        assert!(matches!(&c.nodes[0].kind, NodeKind::Link(u) if u == "https://x.test"));
        assert!(matches!(c.nodes[0].color, Some(CanvasColor::Rgb(_))));
        assert!(matches!(&c.nodes[1].kind, NodeKind::Text(t) if t == "a \"quote\" and é"));
    }

    #[test]
    fn malformed_input_is_total_not_a_panic() {
        assert_eq!(
            JsonCanvas::parse("not json"),
            Err(JsonCanvasError::NotAnObject)
        );
        assert_eq!(JsonCanvas::parse("[]"), Err(JsonCanvasError::NotAnObject));
        // A well-formed object with junk nodes → empty canvas, not an error.
        let c = JsonCanvas::parse(r#"{"nodes":[{"id":"x"},{"bad":true}],"edges":[{}]}"#).unwrap();
        assert!(c.nodes.is_empty() && c.edges.is_empty());
        // Unterminated / deep input must not panic.
        let _ = JsonCanvas::parse(r#"{"nodes":[{"id":"a","type":"text","text":"#);
        let _ = JsonCanvas::parse("[".repeat(500));
    }

    // --- render ------------------------------------------------------------

    #[test]
    fn renders_placed_nodes_edges_and_group_deterministically() {
        let out = lines(JsonCanvas::new(SAMPLE), 60, 18);
        assert!(out.contains("JSON Canvas"), "group label:\n{out}");
        assert!(out.contains("Learn more"), "text node:\n{out}");
        assert!(out.contains("spec/1.0.md"), "file node:\n{out}");
        assert!(out.contains('╌'), "dashed group box:\n{out}");
        assert!(out.contains('▶') || out.contains('◀'), "edge arrow:\n{out}");
        // The author's placement is honoured: `logo`/`learn` are left of
        // `spec` (x -280/40 vs 360) — the spec box's column is rightmost.
        assert_eq!(out, lines(JsonCanvas::new(SAMPLE), 60, 18), "deterministic");
    }

    #[test]
    fn explicit_x_controls_horizontal_order() {
        // Two text nodes; `right` has the larger x and must land to the
        // right of `left` (placement, not auto-layout).
        let src = r#"{"nodes":[
          {"id":"L","type":"text","text":"LEFT","x":0,"y":0,"width":80,"height":40},
          {"id":"R","type":"text","text":"RIGHT","x":400,"y":0,"width":80,"height":40}]}"#;
        let out = lines(JsonCanvas::new(src), 40, 6);
        let row = out.lines().find(|l| l.contains("LEFT")).unwrap();
        let lpos = row.find("LEFT").unwrap();
        let rrow = out.lines().find(|l| l.contains("RIGHT")).unwrap();
        let rpos = rrow.find("RIGHT").unwrap();
        assert!(lpos < rpos, "LEFT must be left of RIGHT:\n{out}");
    }

    #[test]
    fn empty_and_tiny_area_degrade_to_placeholder_no_panic() {
        assert!(lines(JsonCanvas::new("{}"), 32, 1).contains("json-canvas"));
        assert!(lines(JsonCanvas::new(r#"{"nodes":[]}"#), 32, 1).contains("json-canvas"));
        let _ = lines(JsonCanvas::new(SAMPLE), 2, 2);
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        JsonCanvas::new(SAMPLE).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn block_frames_the_canvas() {
        let out = lines(JsonCanvas::new(SAMPLE).block(Block::bordered()), 30, 10);
        assert!(out.starts_with('┌'), "block frame:\n{out}");
    }

    /// R3-3: `from_parsed(&canvas)` (the parse-free seam — caller holds the
    /// parse) renders **byte-identical** to `new(src)` (which scans the
    /// JSON every frame), across sizes. The `Mermaid::from_graph`-class
    /// cell-for-cell equivalence.
    #[test]
    fn from_parsed_is_byte_identical_to_new() {
        let canvas = JsonCanvas::parse(SAMPLE).expect("SAMPLE parses");
        for (w, h) in [(30u16, 10u16), (60, 20), (16, 6)] {
            assert_eq!(
                lines(JsonCanvas::from_parsed(&canvas), w, h),
                lines(JsonCanvas::new(SAMPLE), w, h),
                "parse-free == parse at {w}x{h}"
            );
        }
    }
}
