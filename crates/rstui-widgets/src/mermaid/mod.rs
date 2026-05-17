//! [`Mermaid`] — a read-only widget that parses a narrow but real subset of
//! [Mermaid](https://mermaid.js.org/) flowchart syntax and renders a
//! deterministic box-and-arrow diagram in Unicode/ASCII.
//!
//! # Why a hand-written parser and layout
//!
//! rstui is deliberately dependency-free below the backend (see
//! [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)
//! §4: a graph/layout crate is exactly the kind of transitive dependency a
//! widget must not pull pre-emptively). The subset of Mermaid real terminal
//! docs need — a header, shaped node declarations, a handful of edge styles,
//! edge labels, subgraph clusters, `classDef`/`class`/`style` skinning, and
//! `click` activation — is a line-oriented scan, and a layered tree/DAG
//! layout is a longest-path ranking plus deterministic box placement and
//! orthogonal routing. All of it is a few hundred lines, the same way
//! [`Markdown`](crate::Markdown)'s parser and [`Paragraph`](crate::Paragraph)'s
//! wrap composer are hand-written rather than pulling a crate. So `Mermaid`
//! is a plain [`Widget`] module here, zero new dependencies.
//!
//! # Progressive fidelity, not a fake renderer
//!
//! This is a real, tested subset — not a placeholder that pretends to be a
//! complete Mermaid engine. Supported now:
//!
//! - **Header**: `graph TD` / `graph TB` / `graph LR` / `graph BT` /
//!   `graph RL` (and the `flowchart` keyword). `TD`/`TB` are top-down, `LR`
//!   left-to-right, `BT` bottom-to-top (the axis is genuinely inverted —
//!   ranks stack upward and the arrowhead points up, `▲`), `RL`
//!   right-to-left (columns mirrored, the arrowhead points left, `◀`).
//! - **Node shapes**: `A[Rectangle]`, `A(Round)`, `A{Diamond}`,
//!   `A((Circle))`, and a bare `A` (the id doubles as the label). Quoted
//!   labels keep their spaces and brackets: `A["a, [b]"]`.
//! - **Edges**: `A --> B` (arrow), `A --- B` (open, no arrowhead),
//!   `A -.-> B` (dotted), `A ==> B` (thick), with edge labels written either
//!   as `A -->|text| B` or `A -- text --> B`. A node is declared on first use
//!   and may be re-referenced by id to add edges. An arrow-like sequence
//!   *inside* a bracketed/quoted label (`A["x --> y"]`) is not mistaken for
//!   an operator.
//! - **Chained edges** on one line: `A --> B --> C` records both links and
//!   declares the shared middle node once; each hop may carry its own label
//!   (`A -->|go| B --> C`).
//! - **`&` group shorthand**: `A & B --> C` and `A --> B & C` (and both at
//!   once) expand to the Cartesian set of links, a shared label riding every
//!   one.
//! - **Subgraphs**: `subgraph Title ... end` (nestable). Members are laid
//!   out grouped and wrapped in a labelled bordered cluster box; an edge that
//!   crosses a cluster border still routes cleanly. Parsed into
//!   [`MermaidGraph::subgraphs`] with id/title/member ids and a parent link.
//! - **`classDef`/`class`/`style`**: `classDef name fill:#rgb,stroke:#rgb,`
//!   `color:#rgb,stroke-width:..`, `class A,B name`, the `A:::name`
//!   shorthand, and a per-node `style A fill:#rgb,...`. CSS-ish colors map
//!   deterministically to the nearest [`rstui_core::Color`] (see
//!   [`MermaidGraph::class_defs`] / [`Node::style`]).
//! - **`click`/`href`**: `click NODE "url" "tooltip"` and
//!   `click NODE href "url"` register an activation target, exposed exactly
//!   like [`Markdown`](crate::Markdown)'s links via [`Mermaid::links`],
//!   [`Mermaid::link_regions`], and [`Mermaid::link_at`].
//! - `%%` line comments and blank lines are ignored.
//!
//! Malformed lines never panic — an unparseable line is skipped, and a graph
//! with no parseable nodes renders a clear placeholder.
//!
//! # Layout
//!
//! Nodes are assigned integer ranks by a longest-path layering from the roots
//! (nodes with no incoming edge); a graph that is all cycles falls back to
//! declaration order for its roots, and back-edges into an already-ranked
//! node are drawn but do not deepen the layering. The canonical layout is
//! top-down; `LR` is its transpose, and `BT`/`RL` are the same layout with
//! the rank axis genuinely inverted (a `BT` root sits at the bottom with its
//! arrowheads pointing up, an `RL` root at the right pointing left).
//!
//! Connectors are routed orthogonally through a shared connection grid:
//! a parent's forward edges leave on a per-parent *bus*, each turning into
//! its child's own distinct routing channel and meeting the box with a single
//! arrowhead. Because every cell's glyph is chosen from the merged set of
//! segments crossing it, corners, tees, and crossings resolve to the exact
//! `┌┐└┘├┤┬┴┼`. A back-edge or self-loop is a fully routed return path (out
//! the side, down a reserved side channel, back into the target with a proper
//! arrowhead — a self-loop is a small routed loop on the node itself), an
//! edge that skips ranks jogs into a free inter-column channel rather than
//! dropping through an intervening box, and every edge label is reserved its
//! own free cell(s) so two labels can never share a cell. Spacing is fixed so
//! the same source and area always produce the same cells — output is
//! snapshot-testable through [`Buffer`] exactly like every other widget.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Rect, Widget};
//! use rstui_widgets::Mermaid;
//!
//! let graph = Mermaid::parse("graph TD\n  A[Start] --> B[Stop]").unwrap();
//! assert_eq!(graph.nodes.len(), 2);
//! assert_eq!(graph.edges.len(), 1);
//!
//! let mut buf = Buffer::empty(Rect::new(0, 0, 24, 9));
//! Mermaid::new("graph TD\n  A[Start] --> B[Stop]").render(buf.area(), &mut buf);
//! ```

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use crate::block::Block;
use crate::link::Link;
use rstui_core::{Buffer, Color, Position, Rect, Style, Widget};

// The flowchart renderer is the original implementation, kept verbatim in
// this module. Every *other* Mermaid diagram type is a self-contained sibling
// module that parses its own dialect and renders onto the shared
// [`draw::Surface`]; the [`Mermaid`] widget dispatches on the header keyword
// (see [`DiagramKind`]) so one widget renders any Mermaid source.
mod draw;

mod architecture;
mod block_diagram;
mod c4;
mod class_diagram;
mod er;
mod gantt;
mod gitgraph;
mod journey;
mod kanban;
mod mindmap;
mod packet;
mod pie;
mod quadrant;
mod radar;
mod requirement;
mod sankey;
mod sequence;
mod state;
mod timeline;
mod xychart;
mod zenuml;

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

/// The flow direction declared by the `graph`/`flowchart` header.
///
/// All four Mermaid directions are distinct here: `TD`/`TB` are
/// [`TopDown`](Self::TopDown), `LR` is [`LeftRight`](Self::LeftRight), `BT` is
/// [`BottomTop`](Self::BottomTop) (the rank axis is genuinely inverted —
/// ranks stack upward, arrowheads point up), and `RL` is
/// [`RightLeft`](Self::RightLeft) (columns mirrored, arrowheads point left).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    /// Roots at the top, edges flowing downward (`graph TD` / `graph TB`).
    #[default]
    TopDown,
    /// Roots at the left, edges flowing rightward (`graph LR`).
    LeftRight,
    /// Roots at the bottom, edges flowing upward (`graph BT`).
    BottomTop,
    /// Roots at the right, edges flowing leftward (`graph RL`).
    RightLeft,
}

impl Direction {
    /// Whether this direction lays ranks along the vertical axis (rows of
    /// boxes) rather than the horizontal axis (columns).
    const fn is_vertical(self) -> bool {
        matches!(self, Self::TopDown | Self::BottomTop)
    }

    /// Whether the rank axis is inverted relative to the canonical
    /// top-down / left-right layout (`BT` stacks upward, `RL` mirrors right
    /// to left).
    const fn is_reversed(self) -> bool {
        matches!(self, Self::BottomTop | Self::RightLeft)
    }
}

/// The drawn outline of a node, chosen by the bracket style in its
/// declaration.
///
/// The terminal has no curves, so [`Round`](Self::Round),
/// [`Diamond`](Self::Diamond), and [`Circle`](Self::Circle) approximate their
/// SVG shapes with distinct box glyphs (rounded corners, a `◇` marker prefix,
/// a doubled border) rather than literal geometry — enough to tell the shapes
/// apart at a glance, documented as an approximation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `A[label]` — a square-cornered box.
    Rectangle,
    /// `A(label)` — a rounded-corner box (a stadium/round node).
    Round,
    /// `A{label}` — a decision: square box with a leading `◇` marker.
    Diamond,
    /// `A((label))` — a circle: a doubled-line box.
    Circle,
}

impl Shape {
    /// The four corner glyphs `(top_left, top_right, bottom_left,
    /// bottom_right)` and the `(horizontal, vertical)` edge glyphs this shape
    /// draws its box with.
    const fn glyphs(self) -> (char, char, char, char, char, char) {
        match self {
            Self::Rectangle | Self::Diamond => ('┌', '┐', '└', '┘', '─', '│'),
            Self::Round => ('╭', '╮', '╰', '╯', '─', '│'),
            Self::Circle => ('╔', '╗', '╚', '╝', '═', '║'),
        }
    }
}

/// The line style of an edge, chosen by its arrow token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// `A --> B` — a solid line with an arrowhead.
    Arrow,
    /// `A --- B` — a solid line with no arrowhead (an open/undirected link).
    Open,
    /// `A -.-> B` — a dotted line with an arrowhead.
    Dotted,
    /// `A ==> B` — a thick line with an arrowhead.
    Thick,
}

impl EdgeKind {
    /// Whether this edge draws an arrowhead at its destination.
    const fn has_head(self) -> bool {
        !matches!(self, Self::Open)
    }

    /// The glyph this edge draws its straight run with on the flow axis.
    const fn line(self, vertical: bool) -> char {
        match self {
            Self::Dotted => {
                if vertical {
                    '┊'
                } else {
                    '┄'
                }
            }
            Self::Thick => {
                if vertical {
                    '┃'
                } else {
                    '━'
                }
            }
            Self::Arrow | Self::Open => {
                if vertical {
                    '│'
                } else {
                    '─'
                }
            }
        }
    }
}

/// A per-node visual override resolved from a `classDef`/`class`/`style`
/// directive: the colors to draw the node's border and label with.
///
/// Each field is `None` when the directive did not set it, so it layers as a
/// patch over the [`MermaidTheme`] exactly like the rest of the style
/// cascade. CSS-ish hex (`#rgb` / `#rrggbb`) and the named CSS colors map to
/// the nearest [`Color`] by the deterministic rule in [`css_color`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeStyle {
    /// `fill:` — the node's background color.
    pub fill: Option<Color>,
    /// `stroke:` — the node's border color.
    pub stroke: Option<Color>,
    /// `color:` — the node's label text color.
    pub text: Option<Color>,
}

impl NodeStyle {
    /// Whether any field is set (an empty style applies nothing).
    const fn is_empty(self) -> bool {
        self.fill.is_none() && self.stroke.is_none() && self.text.is_none()
    }

    /// Layers `other` over `self` (set fields in `other` win) — the cascade
    /// of `classDef` then a later `class`/`:::`/`style` on the same node.
    fn patch(self, other: Self) -> Self {
        Self {
            fill: other.fill.or(self.fill),
            stroke: other.stroke.or(self.stroke),
            text: other.text.or(self.text),
        }
    }
}

/// One declared node: a stable `id`, its display `label`, its [`Shape`], and
/// any resolved style.
///
/// The first declaration of an id fixes its shape and label; a later bare
/// reference (e.g. the `B` in a second `B --> C`) reuses it and does not
/// overwrite either. `class` is the last class name attached to the node (the
/// `class`/`:::` directive) and `style` is the fully resolved
/// [`NodeStyle`] (its `classDef` patched by any per-node `style`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The identifier used to reference this node in edges.
    pub id: String,
    /// The text drawn inside the node's box.
    pub label: String,
    /// The node's drawn outline.
    pub shape: Shape,
    /// The class name attached via `class A name` or `A:::name`, if any.
    pub class: Option<String>,
    /// The resolved colors to draw this node with (its `classDef` patched by
    /// any per-node `style`), empty when nothing skinned it.
    pub style: NodeStyle,
}

/// One directed connection between two node ids.
///
/// `from` and `to` are ids that are guaranteed to exist in
/// [`MermaidGraph::nodes`] (a node is auto-declared on first use). `label` is
/// the optional text drawn on the connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// The source node id.
    pub from: String,
    /// The destination node id.
    pub to: String,
    /// The connector's optional label.
    pub label: Option<String>,
    /// The connector's line style.
    pub kind: EdgeKind,
}

/// One `subgraph Title ... end` cluster: a stable `id`, its display `title`,
/// the node ids that belong directly to it, and its enclosing parent (for a
/// nested `subgraph`).
///
/// Membership is *direct*: a node nested two clusters deep lists only in the
/// inner cluster; the outer cluster's [`members`](Self::members) holds the
/// inner subgraph through [`MermaidGraph::cluster_members`]. Clusters appear
/// in first-`subgraph`-line order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subgraph {
    /// The identifier (an explicit `subgraph id [Title]` id, else the title).
    pub id: String,
    /// The text drawn on the cluster's border.
    pub title: String,
    /// The node ids declared directly inside this cluster, in source order.
    pub members: Vec<String>,
    /// The id of the enclosing [`Subgraph`], if this one is nested.
    pub parent: Option<String>,
}

/// One `classDef name fill:#..,stroke:#..,color:#..` style class: the name it
/// is referenced by and the [`NodeStyle`] it resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassDef {
    /// The class name used by `class A name` / `A:::name`.
    pub name: String,
    /// The colors this class applies to a node it is attached to.
    pub style: NodeStyle,
}

/// The parsed flowchart: its [`Direction`], the declared [`Node`]s, [`Edge`]s,
/// [`Subgraph`]s, [`ClassDef`]s, and `click` registry, in source order.
///
/// This is the public parse result so a caller or test can assert the parse
/// independently of the layout. Nodes appear in first-declaration order;
/// edges, subgraphs, and class definitions in source order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MermaidGraph {
    /// The declared flow direction.
    pub direction: Direction,
    /// Every node, in first-declaration order.
    pub nodes: Vec<Node>,
    /// Every edge, in source order.
    pub edges: Vec<Edge>,
    /// Every `subgraph` cluster, in source order (nestable via `parent`).
    pub subgraphs: Vec<Subgraph>,
    /// Every `classDef`, in source order.
    pub class_defs: Vec<ClassDef>,
    /// `click NODE "url"` activation targets, in source order: `(node id,
    /// href)`. The public registry behind [`Mermaid::links`].
    pub clicks: Vec<(String, String)>,
}

impl MermaidGraph {
    /// The index into [`nodes`](Self::nodes) of the node with `id`, if any.
    fn index_of(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    /// The [`ClassDef`] named `name`, if defined.
    fn class_def(&self, name: &str) -> Option<&ClassDef> {
        self.class_defs.iter().find(|c| c.name == name)
    }

    /// The [`Subgraph`] with `id`, if any.
    fn subgraph(&self, id: &str) -> Option<&Subgraph> {
        self.subgraphs.iter().find(|s| s.id == id)
    }

    /// The direct member node ids of the cluster `id`, recursively flattened
    /// to include the members of any nested cluster — every node that renders
    /// inside the cluster box, in source order.
    ///
    /// A handle for a host that wants the full membership of a cluster
    /// without walking [`subgraphs`](Self::subgraphs) and `parent` itself.
    #[must_use]
    pub fn cluster_members(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_members(id, &mut out);
        out
    }

    fn collect_members(&self, id: &str, out: &mut Vec<String>) {
        let Some(sg) = self.subgraph(id) else {
            return;
        };
        for m in &sg.members {
            if self.subgraph(m).is_some() {
                self.collect_members(m, out);
            } else if !out.iter().any(|x| x == m) {
                out.push(m.clone());
            }
        }
    }
}

/// Why [`Mermaid::parse`] could not produce a graph.
///
/// Parsing is intentionally lenient — individual malformed lines are skipped,
/// not reported — so the only hard errors are a missing/unrecognised header
/// and a source with no parseable nodes at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MermaidError {
    /// The first non-blank, non-comment line was not a
    /// `graph`/`flowchart <DIR>` header.
    MissingHeader,
    /// The header parsed but no node was ever declared or referenced.
    EmptyGraph,
}

impl std::fmt::Display for MermaidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingHeader => {
                f.write_str("expected a `graph`/`flowchart <TD|LR|...>` header line")
            }
            Self::EmptyGraph => f.write_str("no nodes were declared"),
        }
    }
}

impl std::error::Error for MermaidError {}

// ---------------------------------------------------------------------------
// CSS-ish color mapping
// ---------------------------------------------------------------------------

/// Maps a CSS-ish color token (`#rgb`, `#rrggbb`, or a named CSS color) to the
/// nearest [`Color`], deterministically.
///
/// A hex value is expanded to 8-bit RGB and snapped to the closest of the 16
/// ANSI palette colors by squared-distance in RGB space (ties resolve to the
/// earlier palette entry, so the mapping is total and stable); a recognised
/// CSS color name maps to its palette twin directly. An unparseable token
/// yields `None` (the field stays unset and falls through the theme cascade).
/// This is the single documented rule the `classDef`/`style` skinning uses.
#[must_use]
pub fn css_color(token: &str) -> Option<Color> {
    let t = token.trim();
    if let Some(hex) = t.strip_prefix('#') {
        let (r, g, b) = parse_hex(hex)?;
        return Some(nearest_ansi(r, g, b));
    }
    let named = match t.to_ascii_lowercase().as_str() {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "lime" => (0, 255, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" | "aqua" => (0, 255, 255),
        "magenta" | "fuchsia" => (255, 0, 255),
        "gray" | "grey" | "silver" => (192, 192, 192),
        "darkgray" | "darkgrey" => (128, 128, 128),
        "orange" => (255, 165, 0),
        "purple" => (128, 0, 128),
        "navy" => (0, 0, 128),
        "teal" => (0, 128, 128),
        "olive" => (128, 128, 0),
        "maroon" => (128, 0, 0),
        "pink" => (255, 192, 203),
        _ => return None,
    };
    Some(nearest_ansi(named.0, named.1, named.2))
}

/// Parses a 3- or 6-digit hex body (no leading `#`) into 8-bit `(r, g, b)`.
fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim();
    let nib = |c: char| c.to_digit(16).map(|d| d as u8);
    match h.len() {
        3 => {
            let mut it = h.chars();
            let r = nib(it.next()?)?;
            let g = nib(it.next()?)?;
            let b = nib(it.next()?)?;
            Some((r * 17, g * 17, b * 17))
        }
        6 => {
            let r = u8::from_str_radix(&h[0..2], 16).ok()?;
            let g = u8::from_str_radix(&h[2..4], 16).ok()?;
            let b = u8::from_str_radix(&h[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

/// The closest of the 16 ANSI palette colors to `(r, g, b)` by squared RGB
/// distance; ties resolve to the earlier palette entry so the result is
/// stable and order-independent.
fn nearest_ansi(r: u8, g: u8, b: u8) -> Color {
    // The canonical 16-color ANSI RGB approximations, in palette order.
    const PALETTE: [(Color, (i32, i32, i32)); 16] = [
        (Color::Black, (0, 0, 0)),
        (Color::Red, (205, 0, 0)),
        (Color::Green, (0, 205, 0)),
        (Color::Yellow, (205, 205, 0)),
        (Color::Blue, (0, 0, 238)),
        (Color::Magenta, (205, 0, 205)),
        (Color::Cyan, (0, 205, 205)),
        (Color::Gray, (229, 229, 229)),
        (Color::DarkGray, (127, 127, 127)),
        (Color::LightRed, (255, 0, 0)),
        (Color::LightGreen, (0, 255, 0)),
        (Color::LightYellow, (255, 255, 0)),
        (Color::LightBlue, (92, 92, 255)),
        (Color::LightMagenta, (255, 0, 255)),
        (Color::LightCyan, (0, 255, 255)),
        (Color::White, (255, 255, 255)),
    ];
    let (tr, tg, tb) = (r as i32, g as i32, b as i32);
    let mut best = Color::Black;
    let mut best_d = i32::MAX;
    for (col, (pr, pg, pb)) in PALETTE {
        let d = (tr - pr).pow(2) + (tg - pg).pow(2) + (tb - pb).pow(2);
        if d < best_d {
            best_d = d;
            best = col;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

/// The styles [`Mermaid`] applies to the pieces of the diagram.
///
/// Every field is a *patch* layered over the widget base style (itself layered
/// over the framing [`Block`] fill), so an unset color falls through rather
/// than overriding the surrounding theme — the same
/// [`Style::patch`](rstui_core::Style) cascade the text model uses. A
/// per-node [`NodeStyle`] resolved from `classDef`/`class`/`style` is layered
/// *over* [`node_border`](Self::node_border)/[`node_label`](Self::node_label)
/// for that node only, so skinned nodes stand out while the rest keep the
/// theme. Construct the tuned terminal default with [`MermaidTheme::default`]
/// and override only the fields you care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MermaidTheme {
    /// The box border glyphs around every node.
    pub node_border: Style,
    /// The label text drawn inside a node.
    pub node_label: Style,
    /// The connector lines and arrowheads between nodes.
    pub edge: Style,
    /// An edge's label text.
    pub edge_label: Style,
    /// A subgraph cluster's border and its title.
    pub cluster: Style,
    /// The placeholder shown when the source has no parseable graph.
    pub placeholder: Style,
}

impl Default for MermaidTheme {
    fn default() -> Self {
        Self {
            node_border: Style::new().fg(Color::Cyan),
            node_label: Style::new(),
            edge: Style::new().fg(Color::DarkGray),
            edge_label: Style::new().fg(Color::Yellow),
            cluster: Style::new().fg(Color::Magenta),
            placeholder: Style::new().fg(Color::Red),
        }
    }
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// Where a clickable node's box landed on screen, returned by
/// [`Mermaid::link_regions`].
///
/// `index` is the key into [`Mermaid::links`] (the activation registry);
/// `rect` is the screen cells the node's box covers. Mirrors
/// [`markdown::LinkRegion`](crate::LinkRegion) so a host hit-tests a Mermaid
/// node exactly the way it hit-tests a Markdown link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkRegion {
    /// The link's position in [`Mermaid::links`].
    pub index: usize,
    /// The screen rectangle the clickable node's box covers.
    pub rect: Rect,
}

/// A read-only Mermaid flowchart view: parses its source and draws the
/// supported subset as a deterministic box-and-arrow diagram.
///
/// The source is a [`Cow<str>`](std::borrow::Cow) (a literal borrows, a
/// `String` is owned). An optional framing [`Block`], a base [`Style`] that
/// also fills the content area, and a [`MermaidTheme`] are the only knobs —
/// everything else is derived from the diagram. Parsing is exposed separately
/// via [`Mermaid::parse`] so callers and tests can assert the graph
/// independently of how it is laid out, and `click` targets are exposed via
/// [`Mermaid::links`]/[`Mermaid::link_at`] exactly like
/// [`Markdown`](crate::Markdown).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Block, Mermaid};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 16, 5));
/// Mermaid::new("graph LR\n  A --> B")
///     .block(Block::bordered())
///     .render(buf.area(), &mut buf);
///
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌'); // block frame
/// ```
#[derive(Debug, Clone)]
pub struct Mermaid<'a> {
    source: Cow<'a, str>,
    block: Option<Block<'a>>,
    style: Style,
    theme: MermaidTheme,
}

impl<'a> Mermaid<'a> {
    /// A Mermaid view of `source` with the default theme, no block.
    pub fn new(source: impl Into<Cow<'a, str>>) -> Self {
        Self {
            source: source.into(),
            block: None,
            style: Style::new(),
            theme: MermaidTheme::default(),
        }
    }

    /// Frames the diagram in `block`; content renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`] beneath the theme cascade. It also fills the
    /// content area so a background covers the whole region.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Replaces the [`MermaidTheme`].
    #[must_use]
    pub fn theme(mut self, theme: MermaidTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Parses `source` into a [`MermaidGraph`] without rendering.
    ///
    /// This is the same parse the widget runs at draw time, exposed so a host
    /// or a test can inspect the graph (node/edge/subgraph/class structure,
    /// shapes, labels, direction, click targets) independently of layout.
    /// Malformed lines are skipped; the only errors are a missing header
    /// ([`MermaidError::MissingHeader`]) or a graph with no nodes
    /// ([`MermaidError::EmptyGraph`]).
    pub fn parse(source: impl AsRef<str>) -> Result<MermaidGraph, MermaidError> {
        parse_graph(source.as_ref())
    }

    /// The diagram's `click` targets, in source order — the activation
    /// registry, exactly the shape [`Markdown::links`](crate::Markdown::links)
    /// returns.
    ///
    /// The index into this list is the focus key: a host tracks a focused
    /// index in its own state and the reducer turns Enter or a click into a
    /// [`LinkActivation`](crate::link::LinkActivation) via
    /// [`Link::activate`](crate::Link::activate). The link `label` is the
    /// node's display label so a host can show what was clicked.
    /// Width-independent, so it can be called once per frame.
    #[must_use]
    pub fn links(&self) -> Vec<Link<'static>> {
        let Ok(graph) = parse_graph(self.source.as_ref()) else {
            return Vec::new();
        };
        graph
            .clicks
            .iter()
            .map(|(id, href)| {
                let label = graph
                    .index_of(id)
                    .map(|i| graph.nodes[i].label.clone())
                    .unwrap_or_else(|| id.clone());
                Link::new(label, href.clone())
            })
            .collect()
    }

    /// The screen rectangles every clickable node's box occupies when this
    /// widget is rendered into `area` (same block/centring as
    /// [`render`](Widget::render)) — the geometry half of clickable nodes.
    ///
    /// Deterministic and side-effect-free: it re-runs the exact parse and
    /// layout `render` uses and maps each clicked node's placed box through
    /// the same centring offsets and clip, so a region is reported only for
    /// the part of the box actually on screen. One [`LinkRegion`] per
    /// clickable node, in [`links`](Self::links) order.
    #[must_use]
    pub fn link_regions(&self, area: Rect) -> Vec<LinkRegion> {
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if inner.is_empty() {
            return Vec::new();
        }
        let Ok(graph) = parse_graph(self.source.as_ref()) else {
            return Vec::new();
        };
        if graph.clicks.is_empty() {
            return Vec::new();
        }
        let layout = lay_out(&graph);
        let off_x = ((inner.width as i32 - layout.canvas.w) / 2).max(0);
        let off_y = ((inner.height as i32 - layout.canvas.h) / 2).max(0);
        let mut out = Vec::new();
        for (index, (id, _)) in graph.clicks.iter().enumerate() {
            let Some(ni) = graph.index_of(id) else {
                continue;
            };
            let Some(b) = layout.boxes.get(&ni) else {
                continue;
            };
            // The box in screen space, then clipped to the visible inner area.
            let bx0 = inner.x as i32 + off_x + b.x;
            let by0 = inner.y as i32 + off_y + b.y;
            let bx1 = bx0 + b.w;
            let by1 = by0 + b.h;
            let cx0 = bx0.max(inner.x as i32);
            let cy0 = by0.max(inner.y as i32);
            let cx1 = bx1.min(inner.right() as i32);
            let cy1 = by1.min(inner.bottom() as i32);
            if cx1 > cx0 && cy1 > cy0 {
                out.push(LinkRegion {
                    index,
                    rect: Rect::new(
                        cx0 as u16,
                        cy0 as u16,
                        (cx1 - cx0) as u16,
                        (cy1 - cy0) as u16,
                    ),
                });
            }
        }
        out
    }

    /// The registry index of the clickable node whose box covers `position`
    /// (screen coordinates, the same `area` passed to [`render`](Widget::render)),
    /// or `None`.
    ///
    /// The mouse half of activation as a raw index. Prefer
    /// [`link_activation_at`](Self::link_activation_at), which returns the
    /// resolved [`LinkActivation`](crate::link::LinkActivation) in one call.
    #[must_use]
    pub fn link_at(&self, position: Position, area: Rect) -> Option<usize> {
        self.link_regions(area).into_iter().find_map(|r| {
            let in_x = position.x >= r.rect.x && position.x < r.rect.x.saturating_add(r.rect.width);
            let in_y =
                position.y >= r.rect.y && position.y < r.rect.y.saturating_add(r.rect.height);
            (in_x && in_y).then_some(r.index)
        })
    }

    /// Resolve a click `position` straight to the
    /// [`LinkActivation`](crate::link::LinkActivation) (index + owned `href`)
    /// of the `click`-directive node it activates, or `None`.
    /// Hit-test and `href` come from the same parse of the same immutable
    /// source, so the index/`links()` desync the raw
    /// [`link_at`](Self::link_at) pattern invites cannot happen.
    #[must_use]
    pub fn link_activation_at(
        &self,
        position: Position,
        area: Rect,
    ) -> Option<crate::link::LinkActivation> {
        let index = self.link_at(position, area)?;
        self.links().get(index).map(|link| link.activate(index))
    }
}

impl Widget for Mermaid<'_> {
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

        let src = self.source.as_ref();
        match diagram_kind(src) {
            DiagramKind::Sequence => sequence::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Class => class_diagram::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::State => state::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Er => er::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Journey => journey::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Gantt => gantt::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Pie => pie::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Quadrant => quadrant::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Requirement => {
                requirement::render(src, inner, buf, self.style, &self.theme)
            }
            DiagramKind::GitGraph => gitgraph::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Mindmap => mindmap::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Timeline => timeline::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Sankey => sankey::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::XyChart => xychart::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Block => {
                block_diagram::render(src, inner, buf, self.style, &self.theme)
            }
            DiagramKind::Packet => packet::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Kanban => kanban::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::Architecture => {
                architecture::render(src, inner, buf, self.style, &self.theme)
            }
            DiagramKind::Radar => radar::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::C4 => c4::render(src, inner, buf, self.style, &self.theme),
            DiagramKind::ZenUml => zenuml::render(src, inner, buf, self.style, &self.theme),
            // Flowchart *and* an unrecognised header both take the original
            // path verbatim: a real `graph`/`flowchart` lays out, anything
            // else yields the long-standing `missing graph header`
            // placeholder. The legacy behaviour and its exact messages are
            // preserved unchanged for backward compatibility.
            DiagramKind::Flowchart | DiagramKind::Unknown(_) => {
                match parse_graph(src) {
                    Ok(graph) => {
                        let layout = lay_out(&graph);
                        layout.blit_into(inner, buf, self.style, &self.theme);
                    }
                    Err(err) => {
                        let msg = match err {
                            MermaidError::MissingHeader => "[mermaid: missing graph header]",
                            MermaidError::EmptyGraph => "[mermaid: empty graph]",
                        };
                        let style = self.style.patch(self.theme.placeholder);
                        buf.set_str(Position::new(inner.x, inner.y), msg, style);
                    }
                }
            }
        }
    }
}

/// Draws a centred, framed `[mermaid: <note>]` message — the shared fallback
/// every non-flowchart renderer uses when its source has nothing parseable
/// (or while a renderer is a stub), so a bad diagram is an honest, legible
/// box rather than a blank area or a panic. `kind` is the diagram label,
/// `note` the short reason (e.g. `"no data"`).
pub(crate) fn diagram_placeholder(
    kind: &str,
    note: &str,
    area: Rect,
    buf: &mut Buffer,
    base: Style,
    theme: &MermaidTheme,
) {
    let msg = format!("mermaid · {kind}: {note}");
    let w = (msg.chars().count() as i32 + 4).min(area.width as i32).max(2);
    let h = 3.min(area.height as i32).max(1);
    let mut s = draw::Surface::new(w, h);
    let border = base.patch(theme.cluster);
    let text = base.patch(theme.placeholder);
    if h >= 3 && w >= 4 {
        s.labeled_box(0, 0, w, h, draw::BoxStyle::Round, &msg, border, text);
    } else {
        s.text_clipped(0, 0, &msg, w, text);
    }
    s.blit(area, buf, base);
}

// ---------------------------------------------------------------------------
// Diagram-type dispatch
// ---------------------------------------------------------------------------

/// Which Mermaid diagram a source declares.
///
/// Detected from the first significant line's leading keyword after skipping
/// blank lines, `%%` comments / `%%{init}%%` directives, and an optional
/// leading `--- … ---` YAML frontmatter block — the same preamble Mermaid
/// itself tolerates. Drives the [`Mermaid`] widget's per-type render
/// dispatch; [`Unknown`](Self::Unknown) falls through to the original
/// flowchart path so its long-standing placeholder text is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DiagramKind {
    /// `graph` / `flowchart` — the original box-and-arrow renderer.
    Flowchart,
    /// `sequenceDiagram`.
    Sequence,
    /// `classDiagram` (and `classDiagram-v2`).
    Class,
    /// `stateDiagram` (and `stateDiagram-v2`).
    State,
    /// `erDiagram`.
    Er,
    /// `journey`.
    Journey,
    /// `gantt`.
    Gantt,
    /// `pie`.
    Pie,
    /// `quadrantChart`.
    Quadrant,
    /// `requirementDiagram`.
    Requirement,
    /// `gitGraph`.
    GitGraph,
    /// `mindmap`.
    Mindmap,
    /// `timeline`.
    Timeline,
    /// `sankey-beta`.
    Sankey,
    /// `xychart-beta`.
    XyChart,
    /// `block-beta`.
    Block,
    /// `packet-beta` / `packet`.
    Packet,
    /// `kanban`.
    Kanban,
    /// `architecture-beta`.
    Architecture,
    /// `radar-beta` / `radar`.
    Radar,
    /// `C4Context` / `C4Container` / `C4Component` / `C4Dynamic` /
    /// `C4Deployment`.
    C4,
    /// `zenuml`.
    ZenUml,
    /// A header we do not recognise — routed through the legacy flowchart
    /// path so the existing `missing graph header` placeholder is preserved.
    Unknown(String),
}

/// Detects the [`DiagramKind`] of `src`: skip blanks, `%%` comment/directive
/// lines, and an optional leading `--- … ---` frontmatter block, then match
/// the first significant line's leading keyword.
fn diagram_kind(src: &str) -> DiagramKind {
    let mut lines = src
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .map(strip_comment)
        .map(str::trim)
        .filter(|l| !l.is_empty());

    let Some(mut first) = lines.next() else {
        return DiagramKind::Unknown(String::new());
    };
    // An optional YAML frontmatter block (`---` … `---`) precedes the header.
    if first == "---" {
        for l in lines.by_ref() {
            if l == "---" {
                break;
            }
        }
        let Some(next) = lines.next() else {
            return DiagramKind::Unknown(String::new());
        };
        first = next;
    }

    let word: String = first
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != ':' && *c != '{')
        .collect();
    match word.as_str() {
        "graph" | "flowchart" => DiagramKind::Flowchart,
        "sequenceDiagram" => DiagramKind::Sequence,
        w if w.starts_with("classDiagram") => DiagramKind::Class,
        w if w.starts_with("stateDiagram") => DiagramKind::State,
        "erDiagram" => DiagramKind::Er,
        "journey" => DiagramKind::Journey,
        "gantt" => DiagramKind::Gantt,
        "pie" => DiagramKind::Pie,
        "quadrantChart" => DiagramKind::Quadrant,
        "requirementDiagram" => DiagramKind::Requirement,
        "gitGraph" => DiagramKind::GitGraph,
        "mindmap" => DiagramKind::Mindmap,
        "timeline" => DiagramKind::Timeline,
        "sankey-beta" | "sankey" => DiagramKind::Sankey,
        "xychart-beta" | "xychart" => DiagramKind::XyChart,
        "block-beta" | "block" => DiagramKind::Block,
        "packet-beta" | "packet" => DiagramKind::Packet,
        "kanban" => DiagramKind::Kanban,
        "architecture-beta" | "architecture" => DiagramKind::Architecture,
        "radar-beta" | "radar" => DiagramKind::Radar,
        w if w.starts_with("C4") => DiagramKind::C4,
        "zenuml" => DiagramKind::ZenUml,
        other => DiagramKind::Unknown(other.to_owned()),
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parses `src` into a [`MermaidGraph`]: a header line then line-oriented
/// node/edge/subgraph/directive statements. Lenient — an unparseable
/// statement line is skipped.
fn parse_graph(src: &str) -> Result<MermaidGraph, MermaidError> {
    let mut lines = src
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .map(strip_comment)
        .filter(|l| !l.trim().is_empty());

    let header = lines.next().ok_or(MermaidError::MissingHeader)?;
    let direction = parse_header(header.trim()).ok_or(MermaidError::MissingHeader)?;

    let mut graph = MermaidGraph {
        direction,
        ..MermaidGraph::default()
    };
    // The stack of open `subgraph` ids; a node declared while the stack is
    // non-empty joins the innermost cluster.
    let mut cluster_stack: Vec<String> = Vec::new();
    for line in lines {
        parse_statement(line.trim(), &mut graph, &mut cluster_stack);
    }
    apply_classes(&mut graph);
    if graph.nodes.is_empty() {
        return Err(MermaidError::EmptyGraph);
    }
    Ok(graph)
}

/// Drops a `%%` line comment (everything from the first `%%` to end of line).
fn strip_comment(line: &str) -> &str {
    match line.find("%%") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// Parses the `graph`/`flowchart <DIR>` header into a [`Direction`], or
/// `None` if the line is not a recognised header.
fn parse_header(line: &str) -> Option<Direction> {
    let rest = line
        .strip_prefix("graph")
        .or_else(|| line.strip_prefix("flowchart"))?;
    // The keyword must be a whole word: `graphic` is not `graph`.
    if !rest.is_empty() && !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let dir = rest.trim();
    Some(match dir {
        "" | "TD" | "TB" => Direction::TopDown,
        "LR" => Direction::LeftRight,
        "BT" => Direction::BottomTop,
        "RL" => Direction::RightLeft,
        // An unknown suffix is treated as the default rather than rejected,
        // so a future direction does not make the whole diagram an error.
        _ => Direction::TopDown,
    })
}

/// Parses one statement line: a `subgraph`/`end`, a `classDef`/`class`/
/// `style`/`click` directive, a chain of one or more edges, or a lone node
/// declaration. Unparseable input is ignored.
fn parse_statement(line: &str, graph: &mut MermaidGraph, cluster_stack: &mut Vec<String>) {
    if line.is_empty() {
        return;
    }
    // `end` closes the innermost open subgraph.
    if line == "end" {
        cluster_stack.pop();
        return;
    }
    if let Some(rest) = strip_keyword(line, "subgraph") {
        let (id, title) = parse_subgraph_header(rest);
        let parent = cluster_stack.last().cloned();
        if let Some(p) = &parent {
            if let Some(sg) = graph.subgraphs.iter_mut().find(|s| &s.id == p) {
                if !sg.members.iter().any(|m| m == &id) {
                    sg.members.push(id.clone());
                }
            }
        }
        if graph.subgraph(&id).is_none() {
            graph.subgraphs.push(Subgraph {
                id: id.clone(),
                title,
                members: Vec::new(),
                parent,
            });
        }
        cluster_stack.push(id);
        return;
    }
    if let Some(rest) = strip_keyword(line, "classDef") {
        parse_class_def(rest, graph);
        return;
    }
    if let Some(rest) = strip_keyword(line, "class") {
        parse_class_apply(rest, graph);
        return;
    }
    if let Some(rest) = strip_keyword(line, "style") {
        parse_style_directive(rest, graph);
        return;
    }
    if let Some(rest) = strip_keyword(line, "click") {
        parse_click(rest, graph);
        return;
    }
    parse_edges_or_node(line, graph, cluster_stack.last());
}

/// Strips a leading bare keyword (`subgraph`, `class`, …) and the whitespace
/// after it, returning the remainder; `None` if the line is not that keyword
/// as a whole word.
fn strip_keyword<'a>(line: &'a str, kw: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(kw)?;
    if rest.is_empty() {
        return Some(rest);
    }
    rest.starts_with(|c: char| c.is_whitespace())
        .then(|| rest.trim_start())
}

/// Parses a `subgraph` header into `(id, title)`. `subgraph id [Title]` gives
/// an explicit id; `subgraph "A title"` or `subgraph A title` uses the text
/// as both id and title (id de-spaced).
fn parse_subgraph_header(rest: &str) -> (String, String) {
    let rest = rest.trim();
    if rest.is_empty() {
        return ("sg".to_owned(), String::new());
    }
    // `id [Title]` / `id (Title)` / `id {Title}` — explicit id then a shaped
    // title.
    if let Some(open) = rest.find(['[', '(', '{']) {
        let id = rest[..open].trim();
        if !id.is_empty() && id.chars().all(is_id_char) {
            if let Some((title, _)) = parse_shape(&rest[open..]) {
                return (id.to_owned(), title);
            }
        }
    }
    let text = unquote(rest);
    let id: String = text
        .chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect();
    (id, text)
}

/// Parses `classDef name fill:#rgb,stroke:#rgb,color:#rgb,...` into a
/// [`ClassDef`] (first definition of a name wins; later ones are ignored).
fn parse_class_def(rest: &str, graph: &mut MermaidGraph) {
    let rest = rest.trim().trim_end_matches(';');
    let mut it = rest.splitn(2, char::is_whitespace);
    let Some(name) = it.next().map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    let style = parse_style_props(it.next().unwrap_or(""));
    if graph.class_def(name).is_none() {
        graph.class_defs.push(ClassDef {
            name: name.to_owned(),
            style,
        });
    }
}

/// Parses `class A,B,C name`: attaches the class `name` to each listed node
/// (declaring a bare node if it is referenced here first).
fn parse_class_apply(rest: &str, graph: &mut MermaidGraph) {
    let rest = rest.trim().trim_end_matches(';');
    let Some(sp) = rest.rfind(char::is_whitespace) else {
        return;
    };
    let (ids, name) = (rest[..sp].trim(), rest[sp..].trim());
    if name.is_empty() {
        return;
    }
    for id in ids.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(real) = upsert_node(graph, id) {
            if let Some(i) = graph.index_of(&real) {
                graph.nodes[i].class = Some(name.to_owned());
            }
        }
    }
}

/// Parses `style A fill:#rgb,stroke:#rgb,...`: a per-node style override
/// layered over the node's class (resolved in [`apply_classes`]).
fn parse_style_directive(rest: &str, graph: &mut MermaidGraph) {
    let rest = rest.trim().trim_end_matches(';');
    let mut it = rest.splitn(2, char::is_whitespace);
    let Some(id) = it.next().map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    let style = parse_style_props(it.next().unwrap_or(""));
    if let Some(real) = upsert_node(graph, id) {
        if let Some(i) = graph.index_of(&real) {
            // The per-node `style` patches over whatever the class set.
            graph.nodes[i].style = graph.nodes[i].style.patch(style);
        }
    }
}

/// Parses a `fill:#rgb,stroke:#rgb,color:#rgb,stroke-width:2px` property list
/// into a [`NodeStyle`] (unknown keys ignored, every color via [`css_color`]).
fn parse_style_props(props: &str) -> NodeStyle {
    let mut s = NodeStyle::default();
    for prop in props.split([',', ';']) {
        let Some((k, v)) = prop.split_once(':') else {
            continue;
        };
        match k.trim().to_ascii_lowercase().as_str() {
            "fill" => s.fill = css_color(v),
            "stroke" => s.stroke = css_color(v),
            "color" => s.text = css_color(v),
            // `stroke-width`, `stroke-dasharray`, … have no terminal analog
            // and are accepted-and-ignored rather than rejected.
            _ => {}
        }
    }
    s
}

/// Resolves every node's [`NodeStyle`]: its attached `class`'s `classDef`,
/// then any per-node `style` already patched on top. Called once after the
/// whole source is parsed so a `classDef` may appear after the `class`.
fn apply_classes(graph: &mut MermaidGraph) {
    for i in 0..graph.nodes.len() {
        if let Some(name) = graph.nodes[i].class.clone() {
            if let Some(cd) = graph.class_def(&name) {
                // Class first, then the per-node `style` already stored wins.
                graph.nodes[i].style = cd.style.patch(graph.nodes[i].style);
            }
        }
    }
}

/// Parses `click NODE "url" "tooltip"` or `click NODE href "url"` into a
/// `(node id, href)` entry (the tooltip, if any, is dropped — the terminal
/// has no hover). The node is declared if referenced here first.
fn parse_click(rest: &str, graph: &mut MermaidGraph) {
    let rest = rest.trim();
    let mut it = rest.splitn(2, char::is_whitespace);
    let Some(id) = it.next().map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    let mut tail = it.next().unwrap_or("").trim();
    // Optional `href` / `call` keyword before the quoted url.
    if let Some(after) = tail.strip_prefix("href") {
        tail = after.trim_start();
    }
    let href = first_quoted(tail).or_else(|| {
        // Bare (unquoted) url up to the next whitespace.
        tail.split_whitespace().next().map(str::to_owned)
    });
    let Some(href) = href.filter(|h| !h.is_empty()) else {
        return;
    };
    if let Some(real) = upsert_node(graph, id) {
        graph.clicks.push((real, href));
    }
}

/// The contents of the first `"..."` span in `s`, if any.
fn first_quoted(s: &str) -> Option<String> {
    let start = s.find('"')? + 1;
    let end = s[start..].find('"')? + start;
    Some(s[start..end].to_owned())
}

/// Parses an edge-chain or lone-node statement, declaring any new node into
/// `cluster` (the innermost open `subgraph`) when given.
///
/// An endpoint may be a `&`-joined group (`A & B`), so each operator connects
/// every node on its left to every node on its right (a Cartesian fan); a
/// chained line `A --> B --> C` is split operator by operator, the right group
/// of one edge becoming the left group of the next, so the middle node is
/// declared exactly once and both links are recorded.
fn parse_edges_or_node(line: &str, graph: &mut MermaidGraph, cluster: Option<&String>) {
    let mut rest = line;
    let mut left_ids: Option<Vec<String>> = None;
    let mut produced_edge = false;
    while let Some((left, edge, tail)) = split_edge(rest) {
        let from_ids = match left_ids.take() {
            Some(ids) => ids,
            None => group_ids(graph, left, cluster),
        };
        let (right, next_rest) = match split_edge(tail) {
            Some((r, _, _)) => (&tail[..r.len()], Some(tail)),
            None => (tail, None),
        };
        let to_ids = group_ids(graph, right, cluster);
        for from in &from_ids {
            for to in &to_ids {
                graph.edges.push(Edge {
                    from: from.clone(),
                    to: to.clone(),
                    label: edge.label.clone(),
                    kind: edge.kind,
                });
                produced_edge = true;
            }
        }
        match next_rest {
            Some(after) => {
                left_ids = Some(to_ids);
                rest = after;
            }
            None => break,
        }
    }
    if !produced_edge {
        if let Some(id) = upsert_node(graph, line) {
            register_member(graph, &id, cluster);
        }
    }
}

/// Records `id` as a direct member of cluster `cluster` (if any and not
/// already listed).
fn register_member(graph: &mut MermaidGraph, id: &str, cluster: Option<&String>) {
    if let Some(c) = cluster {
        if let Some(sg) = graph.subgraphs.iter_mut().find(|s| &s.id == c) {
            if !sg.members.iter().any(|m| m == id) {
                sg.members.push(id.to_owned());
            }
        }
    }
}

/// Resolves a `&`-joined endpoint group (`A & B[x] & C`) into its node ids,
/// declaring each on first use and registering it into `cluster`.
fn group_ids(graph: &mut MermaidGraph, group: &str, cluster: Option<&String>) -> Vec<String> {
    split_top_level(group, '&')
        .into_iter()
        .filter_map(|tok| {
            let id = upsert_node(graph, tok.trim())?;
            register_member(graph, &id, cluster);
            Some(id)
        })
        .collect()
}

/// Splits `s` at every `sep` that sits at bracket depth 0 and outside a
/// double-quoted span, so a separator buried in a node label is preserved.
fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if in_quote {
            if c == '"' {
                in_quote = false;
            }
            continue;
        }
        match c {
            '"' => in_quote = true,
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = (depth - 1).max(0),
            _ if c == sep && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// An edge token's resolved style and label, between its two endpoints.
struct EdgeToken {
    kind: EdgeKind,
    label: Option<String>,
}

/// Splits a statement at its *first* edge operator into
/// `(left_node, edge, right_node)`, or `None` if the line has no edge.
///
/// Recognises `-->`, `---`, `-.->`, `==>`, the inline-label forms
/// `-->|text|` / `-- text -->`, and their thick/dotted variants. The scan
/// skips over bracketed (`[]` `()` `{}`) and double-quoted node-label spans so
/// an arrow-like sequence *inside* a label (`A["x-->y"]`) is never mistaken
/// for an operator. [`parse_edges_or_node`] re-applies this on the tail to
/// walk a chained `A --> B --> C` link by link.
fn split_edge(line: &str) -> Option<(&str, EdgeToken, &str)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut depth = 0i32;
    let mut in_quote = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_quote {
            if c == '"' {
                in_quote = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_quote = true;
                i += 1;
                continue;
            }
            '[' | '(' | '{' => {
                depth += 1;
                i += 1;
                continue;
            }
            ']' | ')' | '}' => {
                depth = (depth - 1).max(0);
                i += 1;
                continue;
            }
            _ => {}
        }
        if depth == 0 && (c == '-' || c == '=') && i > 0 {
            if let Some((op_end, kind, label)) = scan_operator(line, i) {
                let left = &line[..i];
                let right = &line[op_end..];
                if !left.trim().is_empty() && !right.trim().is_empty() {
                    return Some((left, EdgeToken { kind, label }, right));
                }
            }
        }
        i += 1;
    }
    None
}

/// Tries to read a full edge operator starting at byte `start` in `line`,
/// returning `(byte_after_operator, kind, label)`.
fn scan_operator(line: &str, start: usize) -> Option<(usize, EdgeKind, Option<String>)> {
    let rest = &line[start..];
    let lead = rest.chars().next()?;

    if rest.starts_with("-.") {
        if let Some(after) = rest.strip_prefix("-.->") {
            let (after, label) = take_pipe_label(after);
            return Some((line.len() - after.len(), EdgeKind::Dotted, label));
        }
        if let Some(body) = rest.strip_prefix("-.") {
            if let Some(end) = body.find(".->") {
                let label = clean_label(&body[..end]);
                let after = &body[end + 3..];
                return Some((line.len() - after.len(), EdgeKind::Dotted, label));
            }
        }
        return None;
    }

    if lead == '=' {
        let run = rest.chars().take_while(|&c| c == '=').count();
        if run >= 2 {
            let body = &rest[run..];
            if let Some(after) = body.strip_prefix('>') {
                let (after, label) = take_pipe_label(after);
                return Some((line.len() - after.len(), EdgeKind::Thick, label));
            }
            if let Some(close) = body.find("==>") {
                let label = clean_label(&body[..close]);
                let after = &body[close + 3..];
                return Some((line.len() - after.len(), EdgeKind::Thick, label));
            }
            if let Some(close) = body.find("==") {
                let label = clean_label(&body[..close]);
                let after = &body[close + 2..];
                return Some((line.len() - after.len(), EdgeKind::Thick, label));
            }
        }
        return None;
    }

    if lead == '-' {
        let run = rest.chars().take_while(|&c| c == '-').count();
        if run >= 2 {
            let body = &rest[run..];
            if let Some(after) = body.strip_prefix('>') {
                let (after, label) = take_pipe_label(after);
                return Some((line.len() - after.len(), EdgeKind::Arrow, label));
            }
            if let Some(close) = body.find("-->") {
                let label = clean_label(&body[..close]);
                let after = &body[close + 3..];
                return Some((line.len() - after.len(), EdgeKind::Arrow, label));
            }
            if let Some(close) = body.find("---") {
                let label = clean_label(&body[..close]);
                let after = &body[close + 3..];
                return Some((line.len() - after.len(), EdgeKind::Open, label));
            }
            if run >= 3 || body.trim_start().starts_with(|c: char| c != '-') {
                return Some((start + run, EdgeKind::Open, None));
            }
        }
    }
    None
}

/// Consumes a `|label|` immediately following an arrow, returning the
/// remaining text and the cleaned label (if present).
fn take_pipe_label(after: &str) -> (&str, Option<String>) {
    let trimmed = after.trim_start();
    if let Some(body) = trimmed.strip_prefix('|') {
        if let Some(end) = body.find('|') {
            let label = clean_label(&body[..end]);
            return (&body[end + 1..], label);
        }
    }
    (after, None)
}

/// Trims and unquotes an edge label; an empty result is `None`.
fn clean_label(raw: &str) -> Option<String> {
    let t = raw.trim();
    let t = t
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(t);
    let t = t.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_owned())
    }
}

/// Parses a node token (`A`, `A[label]`, `A(round)`, `A{dec}`, `A((c))`, and
/// the `A:::class` skin shorthand), inserts it if new (first declaration wins
/// for shape/label), records a `:::` class, and returns its id. A blank token
/// yields `None`.
fn upsert_node(graph: &mut MermaidGraph, token: &str) -> Option<String> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    // `A:::name` / `A[x]:::name` — split the class off before shape parsing
    // (the `:::` is outside any bracket span).
    let (body, class) = split_class_shorthand(token);
    let (id, label, shape) = parse_node(body)?;
    match graph.index_of(&id) {
        Some(idx) => {
            if graph.nodes[idx].shape == Shape::Rectangle
                && graph.nodes[idx].label == graph.nodes[idx].id
                && (shape != Shape::Rectangle || label != id)
            {
                graph.nodes[idx].label = label;
                graph.nodes[idx].shape = shape;
            }
            if let Some(c) = class {
                graph.nodes[idx].class = Some(c);
            }
        }
        None => graph.nodes.push(Node {
            id: id.clone(),
            label,
            shape,
            class,
            style: NodeStyle::default(),
        }),
    }
    Some(id)
}

/// Splits a trailing `:::class` skin shorthand off a node token, returning
/// `(token_without_class, class_name)`. A `:::` inside a bracket/quote span is
/// left alone (it is label text, not a class).
fn split_class_shorthand(token: &str) -> (&str, Option<String>) {
    let mut depth = 0i32;
    let mut in_quote = false;
    let bytes = token.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        let c = bytes[i] as char;
        if in_quote {
            if c == '"' {
                in_quote = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => in_quote = true,
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = (depth - 1).max(0),
            ':' if depth == 0 && &token[i..i + 3] == ":::" => {
                let name = token[i + 3..].trim();
                if !name.is_empty() {
                    return (token[..i].trim_end(), Some(name.to_owned()));
                }
            }
            _ => {}
        }
        i += 1;
    }
    (token, None)
}

/// Splits a node token into `(id, label, shape)`. The id is the leading run
/// before any bracket; the bracket style picks the shape; a missing bracket
/// reuses the id as the label.
fn parse_node(token: &str) -> Option<(String, String, Shape)> {
    let token = token.trim();
    let open = token.find(['[', '(', '{']);
    let (id, rest) = match open {
        Some(i) => (token[..i].trim(), &token[i..]),
        None => (token, ""),
    };
    if id.is_empty() || !id.chars().all(is_id_char) {
        return None;
    }
    if rest.is_empty() {
        return Some((id.to_owned(), id.to_owned(), Shape::Rectangle));
    }
    let (label, shape) = parse_shape(rest)?;
    Some((id.to_owned(), label, shape))
}

/// Parses the bracketed `rest` of a node token (`[..]`, `(..)`, `{..}`,
/// `((..))`) into its `(label, shape)`.
fn parse_shape(rest: &str) -> Option<(String, Shape)> {
    let (shape, open_len, close): (Shape, usize, &str) = if rest.starts_with("((") {
        (Shape::Circle, 2, "))")
    } else if let Some(stripped) = rest.strip_prefix('[') {
        let _ = stripped;
        (Shape::Rectangle, 1, "]")
    } else if rest.starts_with('(') {
        (Shape::Round, 1, ")")
    } else if rest.starts_with('{') {
        (Shape::Diamond, 1, "}")
    } else {
        return None;
    };
    let body = &rest[open_len..];
    let end = body.rfind(close)?;
    let inner = &body[..end];
    Some((unquote(inner.trim()), shape))
}

/// Strips a single pair of surrounding double quotes, if present.
fn unquote(s: &str) -> String {
    s.strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(s)
        .to_owned()
}

/// Whether `c` may appear in a node id (alphanumerics plus `_`/`-`/`.`, the
/// practical subset Mermaid ids use).
fn is_id_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.')
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// One placed box: its top-left cell, drawn size, label, and shape. The
/// resolved per-node [`NodeStyle`] is carried separately on [`Layout::styles`]
/// (indexed by node) so it survives the blit without duplicating it here.
struct PlacedBox {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    label: String,
    shape: Shape,
}

impl PlacedBox {
    fn bottom_center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h - 1)
    }
    fn top_center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y)
    }
    fn right_center(&self) -> (i32, i32) {
        (self.x + self.w - 1, self.y + self.h / 2)
    }
    fn left_center(&self) -> (i32, i32) {
        (self.x, self.y + self.h / 2)
    }
}

/// A character grid the diagram is rendered into before being blitted to the
/// [`Buffer`], so layout math is plain integer arithmetic and the result is a
/// single deterministic snapshot.
struct Canvas {
    w: i32,
    h: i32,
    cells: Vec<(char, CellRole)>,
}

/// Which theme style a painted cell takes — kept abstract so the canvas does
/// not depend on a concrete [`Style`]. A node-bordered/labelled cell carries
/// the index of its node so a per-node [`NodeStyle`] can be resolved at blit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CellRole {
    Blank,
    NodeBorder(usize),
    NodeLabel(usize),
    Edge,
    EdgeLabel,
    Cluster,
}

impl Canvas {
    fn new(w: i32, h: i32) -> Self {
        let w = w.max(0);
        let h = h.max(0);
        Self {
            w,
            h,
            cells: vec![(' ', CellRole::Blank); (w * h).max(0) as usize],
        }
    }

    /// Paints `ch` at `(x, y)` with `role`, ignoring out-of-bounds writes.
    fn put(&mut self, x: i32, y: i32, ch: char, role: CellRole) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        self.cells[(y * self.w + x) as usize] = (ch, role);
    }

    /// Paints `text` left-to-right from `(x, y)` with `role`.
    fn put_str(&mut self, x: i32, y: i32, text: &str, role: CellRole) {
        for (i, ch) in text.chars().enumerate() {
            self.put(x + i as i32, y, ch, role);
        }
    }

    /// The glyph already painted at `(x, y)`, or a space if out of bounds.
    #[cfg(test)]
    fn glyph(&self, x: i32, y: i32) -> char {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return ' ';
        }
        self.cells[(y * self.w + x) as usize].0
    }

    /// Draws a subgraph cluster's bordered, titled box.
    fn draw_cluster(&mut self, x: i32, y: i32, w: i32, h: i32, title: &str) {
        if w < 2 || h < 2 {
            return;
        }
        let (x1, y1) = (x + w - 1, y + h - 1);
        for cx in x..=x1 {
            self.put(cx, y, '─', CellRole::Cluster);
            self.put(cx, y1, '─', CellRole::Cluster);
        }
        for cy in y..=y1 {
            self.put(x, cy, '│', CellRole::Cluster);
            self.put(x1, cy, '│', CellRole::Cluster);
        }
        self.put(x, y, '┌', CellRole::Cluster);
        self.put(x1, y, '┐', CellRole::Cluster);
        self.put(x, y1, '└', CellRole::Cluster);
        self.put(x1, y1, '┘', CellRole::Cluster);
        if !title.is_empty() {
            let inner = (w - 2).max(0) as usize;
            let shown: String = std::iter::once(' ')
                .chain(title.chars())
                .chain(std::iter::once(' '))
                .take(inner)
                .collect();
            self.put_str(x + 1, y, &shown, CellRole::Cluster);
        }
    }

    /// Draws a node's box with its shape glyphs and centred (clipped) label,
    /// tagging every cell with `node` so its [`NodeStyle`] resolves at blit.
    fn draw_box(&mut self, node: usize, b: &PlacedBox) {
        let (tl, tr, bl, br, horiz, vert) = b.shape.glyphs();
        let (x0, y0, x1, y1) = (b.x, b.y, b.x + b.w - 1, b.y + b.h - 1);
        for x in x0..=x1 {
            self.put(x, y0, horiz, CellRole::NodeBorder(node));
            self.put(x, y1, horiz, CellRole::NodeBorder(node));
        }
        for y in y0..=y1 {
            self.put(x0, y, vert, CellRole::NodeBorder(node));
            self.put(x1, y, vert, CellRole::NodeBorder(node));
        }
        self.put(x0, y0, tl, CellRole::NodeBorder(node));
        self.put(x1, y0, tr, CellRole::NodeBorder(node));
        self.put(x0, y1, bl, CellRole::NodeBorder(node));
        self.put(x1, y1, br, CellRole::NodeBorder(node));

        let mut text: Cow<'_, str> = Cow::Borrowed(b.label.as_str());
        if b.shape == Shape::Diamond {
            text = Cow::Owned(format!("◇ {}", b.label));
        }
        let inner_w = (b.w - 2).max(0) as usize;
        let shown: String = text.chars().take(inner_w).collect();
        let pad = inner_w.saturating_sub(shown.chars().count());
        let lx = b.x + 1 + (pad / 2) as i32;
        // Fill the interior so a node fill color covers the whole box, not
        // just the label glyphs.
        for x in (x0 + 1)..x1 {
            self.put(x, b.y + b.h / 2, ' ', CellRole::NodeLabel(node));
        }
        self.put_str(lx, b.y + b.h / 2, &shown, CellRole::NodeLabel(node));
    }

    /// Blits the canvas into `area` of `buf`, centred when smaller than the
    /// area and clipped when larger, resolving each [`CellRole`] to a style.
    fn blit(
        &self,
        area: Rect,
        buf: &mut Buffer,
        base: Style,
        theme: &MermaidTheme,
        styles: &[NodeStyle],
    ) {
        if self.w == 0 || self.h == 0 {
            return;
        }
        let off_x = ((area.width as i32 - self.w) / 2).max(0);
        let off_y = ((area.height as i32 - self.h) / 2).max(0);
        for cy in 0..self.h {
            for cx in 0..self.w {
                let (ch, role) = self.cells[(cy * self.w + cx) as usize];
                if role == CellRole::Blank {
                    continue;
                }
                let px = area.x as i32 + off_x + cx;
                let py = area.y as i32 + off_y + cy;
                if px < area.x as i32
                    || py < area.y as i32
                    || px >= area.right() as i32
                    || py >= area.bottom() as i32
                {
                    continue;
                }
                // Per-node skin: a node's resolved `NodeStyle` overlays the
                // themed border/label for that node only.
                let node_skin = |idx: usize, border: bool| -> Style {
                    let ns = styles.get(idx).copied().unwrap_or_default();
                    if ns.is_empty() {
                        return Style::new();
                    }
                    let mut s = Style::new();
                    if border {
                        if let Some(c) = ns.stroke {
                            s = s.fg(c);
                        }
                    } else if let Some(c) = ns.text {
                        s = s.fg(c);
                    }
                    if let Some(c) = ns.fill {
                        s = s.bg(c);
                    }
                    s
                };
                let style = match role {
                    CellRole::Blank => base,
                    CellRole::NodeBorder(i) => {
                        base.patch(theme.node_border).patch(node_skin(i, true))
                    }
                    CellRole::NodeLabel(i) => {
                        base.patch(theme.node_label).patch(node_skin(i, false))
                    }
                    CellRole::Edge => base.patch(theme.edge),
                    CellRole::EdgeLabel => base.patch(theme.edge_label),
                    CellRole::Cluster => base.patch(theme.cluster),
                };
                buf.set_cell(Position::new(px as u16, py as u16), ch, style);
            }
        }
    }
}

/// Assigns each node a rank by longest-path layering from the roots, breaking
/// cycles deterministically.
///
/// A root is a node with no incoming forward edge; if every node has an
/// incoming edge (a pure cycle) the first declared node seeds rank 0. The
/// longest-path relaxation iterates a bounded number of times (node count) so
/// a back-edge cannot loop forever — it simply does not deepen the layering.
fn rank_nodes(graph: &MermaidGraph) -> Vec<usize> {
    let n = graph.nodes.len();
    let mut rank = vec![0usize; n];
    if n == 0 {
        return rank;
    }
    let idx = |id: &str| graph.index_of(id);
    let mut has_incoming = vec![false; n];
    for e in &graph.edges {
        if let (Some(a), Some(b)) = (idx(&e.from), idx(&e.to)) {
            if a != b {
                has_incoming[b] = true;
            }
        }
    }
    let any_root = has_incoming.iter().any(|&v| !v);
    for _ in 0..n {
        let mut changed = false;
        for e in &graph.edges {
            if let (Some(a), Some(b)) = (idx(&e.from), idx(&e.to)) {
                if a == b {
                    continue;
                }
                let candidate = rank[a] + 1;
                if candidate > rank[b] && (any_root || b != 0) {
                    rank[b] = candidate;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    rank
}

/// A laid-out diagram: the rendered [`Canvas`], the placed boxes keyed by node
/// index (so [`Mermaid::link_regions`] can hit-test them), and each node's
/// resolved [`NodeStyle`] (parallel to the canvas's per-cell node index).
struct Layout {
    canvas: Canvas,
    boxes: BTreeMap<usize, PlacedBox>,
    styles: Vec<NodeStyle>,
}

impl Layout {
    /// Renders the canvas into `area` with the per-node skin applied.
    fn blit_into(&self, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
        self.canvas.blit(area, buf, base, theme, &self.styles);
    }
}

/// Lays the parsed graph out into a [`Layout`]: rank the nodes (inverting the
/// axis for `BT`/`RL`), place each rank's boxes, draw any subgraph cluster
/// boxes, then route the connectors.
fn lay_out(graph: &MermaidGraph) -> Layout {
    let n = graph.nodes.len();
    let styles: Vec<NodeStyle> = graph.nodes.iter().map(|nd| nd.style).collect();
    if n == 0 {
        return Layout {
            canvas: Canvas::new(0, 0),
            boxes: BTreeMap::new(),
            styles,
        };
    }
    let base_rank = rank_nodes(graph);
    let max_rank = *base_rank.iter().max().unwrap_or(&0);
    // `BT`/`RL` invert the rank axis: a root ends up on the far side with its
    // arrowheads pointing back toward rank 0.
    let rank: Vec<usize> = if graph.direction.is_reversed() {
        base_rank.iter().map(|&r| max_rank - r).collect()
    } else {
        base_rank.clone()
    };

    let mut ranks: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for (i, &r) in rank.iter().enumerate() {
        ranks[r].push(i);
    }
    // Group subgraph members so a cluster's nodes are contiguous and on a
    // consistent side at *every* rank it spans — its bounding box is then a
    // clean band that never swallows a foreign node. The key is the node's
    // outermost cluster's source position (ungrouped nodes sort after, so
    // they sit outside every cluster), declaration order breaking ties so
    // the layout stays deterministic.
    if !graph.subgraphs.is_empty() {
        let group_key = |i: usize| -> usize {
            outermost_cluster(graph, &graph.nodes[i].id)
                .and_then(|id| graph.subgraphs.iter().position(|s| s.id == id))
                .unwrap_or(usize::MAX)
        };
        for members in &mut ranks {
            members.sort_by_key(|&i| (group_key(i), i));
        }
    }

    let box_w = |i: usize| -> i32 {
        let node = &graph.nodes[i];
        let label = node.label.chars().count() as i32;
        let marker = if node.shape == Shape::Diamond { 2 } else { 0 };
        (label + marker + 4).max(5)
    };
    let box_h = 3i32;

    // Channel reservation: an edge that skips a rank needs a free routing
    // channel beside the columns so it never drops through an intervening
    // box. Widen the inter-rank gap proportionally to how many skip edges
    // the graph has (capped so a pathological graph still fits).
    let idx = |id: &str| graph.index_of(id);
    let is_skip = |e: &Edge| -> bool {
        matches!(
            (idx(&e.from), idx(&e.to)),
            (Some(a), Some(b)) if base_rank[b] > base_rank[a] + 1
        )
    };
    let skip_channels = graph.edges.iter().filter(|e| is_skip(e)).count().min(4) as i32;
    // Whether the graph has any back-edge / self-loop, which routes through a
    // reserved side channel on the trailing edge of the canvas.
    let has_return = graph.edges.iter().any(|e| {
        if let (Some(a), Some(b)) = (idx(&e.from), idx(&e.to)) {
            base_rank[a] >= base_rank[b]
        } else {
            false
        }
    });

    let h_gap = 3i32;
    // Extra rank gap so skip-rank edges have a free lane and labels each get
    // their own reserved row/column.
    let v_gap = 3i32 + skip_channels.min(3);

    // Subgraph clusters draw a border *around* their members, so every box is
    // inset by the deepest nesting's accumulated border thickness (2 cols /
    // 1 row per level, plus one cell of breathing room) and the canvas grows
    // to match — without this an outermost cluster's border would clip off
    // the left/top edge.
    let max_depth = graph
        .subgraphs
        .iter()
        .map(|s| subgraph_depth(graph, s))
        .max()
        .map(|d| d + 1)
        .unwrap_or(0) as i32;
    let inset_x = max_depth * 3;
    let inset_y = max_depth * 2;
    let cluster_w = inset_x * 2;
    let cluster_h = inset_y * 2;

    let mut boxes: BTreeMap<usize, PlacedBox> = BTreeMap::new();

    // The cluster band each node belongs to: its outermost subgraph's source
    // index, or `usize::MAX` for an ungrouped node (which therefore sits in
    // its own band right of every cluster). Bands are laid out left to right
    // at fixed x-origins so a cluster occupies one contiguous horizontal
    // block at *every* rank it spans — its bounding box can never enclose a
    // foreign node, and an edge in or out simply crosses the band's border.
    let band_of = |i: usize| -> usize {
        if graph.subgraphs.is_empty() {
            return 0;
        }
        outermost_cluster(graph, &graph.nodes[i].id)
            .and_then(|id| graph.subgraphs.iter().position(|s| s.id == id))
            .unwrap_or(usize::MAX)
    };

    let canvas = if graph.direction.is_vertical() {
        // Per (rank, band) row width, and each band's max width across ranks.
        let mut band_keys: Vec<usize> = ranks
            .iter()
            .flat_map(|m| m.iter().map(|&i| band_of(i)))
            .collect();
        band_keys.sort_unstable();
        band_keys.dedup();
        let band_row_w = |members: &[usize], key: usize| -> i32 {
            let ms: Vec<usize> = members
                .iter()
                .copied()
                .filter(|&i| band_of(i) == key)
                .collect();
            if ms.is_empty() {
                return 0;
            }
            ms.iter().map(|&i| box_w(i)).sum::<i32>() + h_gap * (ms.len().saturating_sub(1) as i32)
        };
        let mut band_w: BTreeMap<usize, i32> = BTreeMap::new();
        for &k in &band_keys {
            let w = ranks
                .iter()
                .map(|m| band_row_w(m, k))
                .max()
                .unwrap_or(0)
                .max(1);
            band_w.insert(k, w);
        }
        // Band x-origins (inside the cluster inset), left to right.
        let mut band_x: BTreeMap<usize, i32> = BTreeMap::new();
        let mut acc = inset_x;
        for &k in &band_keys {
            band_x.insert(k, acc);
            acc += band_w[&k] + h_gap * 2;
        }
        let box_region = (acc - h_gap * 2 - inset_x).max(1);
        let label_pad = graph
            .edges
            .iter()
            .filter_map(|e| e.label.as_ref())
            .map(|l| l.chars().count() as i32 + 2)
            .max()
            .unwrap_or(0);
        // A dedicated band of channel columns just right of the boxes so a
        // skip-rank edge has its own lane clear of labels and never drops
        // through a box (one column per skip edge, deterministically), plus a
        // side channel for routed back-edges/self-loops.
        let skip_band = skip_channels * 2;
        let side = if has_return { 4i32 } else { 0 };
        let canvas_w = box_region + skip_band + label_pad + side + cluster_w;
        let canvas_h = (max_rank as i32 + 1) * box_h + max_rank as i32 * v_gap + cluster_h;
        for (r, members) in ranks.iter().enumerate() {
            let y = r as i32 * (box_h + v_gap) + inset_y;
            for &k in &band_keys {
                let row: Vec<usize> = members
                    .iter()
                    .copied()
                    .filter(|&i| band_of(i) == k)
                    .collect();
                if row.is_empty() {
                    continue;
                }
                // Centre this rank's members of the band within the band.
                let mut x = band_x[&k] + (band_w[&k] - band_row_w(members, k)) / 2;
                for i in row {
                    let w = box_w(i);
                    boxes.insert(
                        i,
                        PlacedBox {
                            x,
                            y,
                            w,
                            h: box_h,
                            label: graph.nodes[i].label.clone(),
                            shape: graph.nodes[i].shape,
                        },
                    );
                    x += w + h_gap;
                }
            }
        }
        let mut canvas = Canvas::new(canvas_w, canvas_h.max(box_h));
        draw_clusters(&mut canvas, graph, &boxes);
        for (&i, b) in &boxes {
            canvas.draw_box(i, b);
        }
        route_vertical(&mut canvas, graph, &boxes, &base_rank);
        canvas
    } else {
        let col_h = |members: &[usize]| -> i32 {
            box_h * members.len() as i32 + v_gap * (members.len().saturating_sub(1) as i32)
        };
        // A skip-rank band of routing rows just below the boxes, one row per
        // skip edge, plus the back-edge side channel.
        let skip_band = skip_channels * 2;
        let side = if has_return { 4i32 } else { 0 };
        let cols_h = ranks.iter().map(|m| col_h(m)).max().unwrap_or(0).max(box_h);
        let canvas_h = cols_h + skip_band + side + cluster_h;
        let mut col_x = vec![inset_x; ranks.len()];
        let mut acc = inset_x;
        for (r, members) in ranks.iter().enumerate() {
            col_x[r] = acc;
            let widest = members.iter().map(|&i| box_w(i)).max().unwrap_or(5);
            acc += widest + h_gap * 2;
        }
        let canvas_w = (acc + inset_x).max(1);
        for (r, members) in ranks.iter().enumerate() {
            let total = col_h(members);
            let mut y = (cols_h - total) / 2 + inset_y;
            for &i in members {
                let w = box_w(i);
                boxes.insert(
                    i,
                    PlacedBox {
                        x: col_x[r],
                        y,
                        w,
                        h: box_h,
                        label: graph.nodes[i].label.clone(),
                        shape: graph.nodes[i].shape,
                    },
                );
                y += box_h + v_gap;
            }
        }
        let mut canvas = Canvas::new(canvas_w, canvas_h);
        draw_clusters(&mut canvas, graph, &boxes);
        for (&i, b) in &boxes {
            canvas.draw_box(i, b);
        }
        route_horizontal(&mut canvas, graph, &boxes, &base_rank);
        canvas
    };

    Layout {
        canvas,
        boxes,
        styles,
    }
}

/// The id of the *outermost* cluster the node `id` belongs to (walking up
/// the `parent` chain from its directly-enclosing `subgraph`), or `None` if
/// the node is in no cluster. The grouping key that keeps a cluster's nodes
/// contiguous across ranks.
fn outermost_cluster(graph: &MermaidGraph, id: &str) -> Option<String> {
    let direct = graph
        .subgraphs
        .iter()
        .find(|s| s.members.iter().any(|m| m == id))?;
    let mut cur = direct;
    while let Some(p) = &cur.parent {
        match graph.subgraph(p) {
            Some(parent) => cur = parent,
            None => break,
        }
    }
    Some(cur.id.clone())
}

/// The nesting depth of `sg` (0 for an outermost cluster, +1 per enclosing
/// `subgraph`), used to inset boxes and to nest cluster borders.
fn subgraph_depth(graph: &MermaidGraph, sg: &Subgraph) -> usize {
    let mut d = 0;
    let mut cur = sg.parent.clone();
    while let Some(p) = cur {
        d += 1;
        cur = graph.subgraph(&p).and_then(|s| s.parent.clone());
    }
    d
}

/// Draws each subgraph's labelled bordered cluster box around the bounding
/// rectangle of its (recursively flattened) members, outermost first so a
/// nested cluster's border sits strictly inside its parent's (a deeper level
/// is pulled in by one border thickness so the boxes never collide). A
/// cluster with no placed members is skipped.
fn draw_clusters(canvas: &mut Canvas, graph: &MermaidGraph, boxes: &BTreeMap<usize, PlacedBox>) {
    let mut order: Vec<&Subgraph> = graph.subgraphs.iter().collect();
    order.sort_by_key(|s| subgraph_depth(graph, s));
    for sg in order {
        let members = graph.cluster_members(&sg.id);
        let rects: Vec<&PlacedBox> = members
            .iter()
            .filter_map(|m| graph.index_of(m).and_then(|i| boxes.get(&i)))
            .collect();
        if rects.is_empty() {
            continue;
        }
        let mx0 = rects.iter().map(|b| b.x).min().unwrap();
        let my0 = rects.iter().map(|b| b.y).min().unwrap();
        let mx1 = rects.iter().map(|b| b.x + b.w).max().unwrap();
        let my1 = rects.iter().map(|b| b.y + b.h).max().unwrap();
        // Margin shrinks with depth so a nested border sits inside its
        // parent's: an outermost cluster keeps a 2-col / 1-row gap to its
        // members; that gap is what the enclosing levels' borders occupy.
        // A 1-cell gap between the members' bounding box and the cluster
        // border on every side keeps a nested cluster's border clear of its
        // members and of any enclosing cluster.
        let x0 = (mx0 - 2).max(0);
        let y0 = (my0 - 2).max(0);
        let x1 = mx1 + 1;
        let y1 = my1 + 1;
        canvas.draw_cluster(x0, y0, (x1 - x0).max(2), (y1 - y0 + 1).max(2), &sg.title);
    }
}

/// The four orthogonal neighbours a routed cell connects to, plus the texture
/// of the straight run that owns it.
#[derive(Clone, Copy, Default)]
struct Conn {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    kind: Option<EdgeKind>,
}

impl Conn {
    /// The box-drawing glyph for this connection set. A straight run keeps its
    /// own edge texture; every corner or junction falls back to the solid
    /// set, which reads cleanly there.
    fn glyph(self) -> char {
        let kind = self.kind.unwrap_or(EdgeKind::Arrow);
        match (self.up, self.down, self.left, self.right) {
            (true, true, false, false) => kind.line(true),
            (false, false, true, true) => kind.line(false),
            (false, true, false, true) => '┌',
            (false, true, true, false) => '┐',
            (true, false, false, true) => '└',
            (true, false, true, false) => '┘',
            (true, true, false, true) => '├',
            (true, true, true, false) => '┤',
            (false, true, true, true) => '┬',
            (true, false, true, true) => '┴',
            (true, true, true, true) => '┼',
            (true, false, false, false) | (false, true, false, false) => kind.line(true),
            (false, false, true, false) | (false, false, false, true) => kind.line(false),
            (false, false, false, false) => ' ',
        }
    }
}

/// Accumulates every edge's orthogonal segments into one connection grid
/// before any glyph is chosen, so overlapping runs merge into the correct
/// junction glyph instead of one edge overwriting another.
#[derive(Default)]
struct Router {
    conn: BTreeMap<(i32, i32), Conn>,
    marks: BTreeMap<(i32, i32), char>,
}

impl Router {
    /// Adds one orthogonal `kind`-textured segment between two cells on a
    /// shared row or column.
    fn segment(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, kind: EdgeKind) {
        if y0 == y1 {
            let (lo, hi) = (x0.min(x1), x0.max(x1));
            for x in lo..=hi {
                let c = self.conn.entry((x, y0)).or_default();
                if x > lo {
                    c.left = true;
                }
                if x < hi {
                    c.right = true;
                }
                c.kind = Some(kind);
            }
        } else if x0 == x1 {
            let (lo, hi) = (y0.min(y1), y0.max(y1));
            for y in lo..=hi {
                let c = self.conn.entry((x0, y)).or_default();
                if y > lo {
                    c.up = true;
                }
                if y < hi {
                    c.down = true;
                }
                c.kind = Some(kind);
            }
        }
    }

    /// Records the connection arm by which a node box touches a routed cell.
    fn tap(&mut self, x: i32, y: i32, arm: impl FnOnce(&mut Conn)) {
        arm(self.conn.entry((x, y)).or_default());
    }

    /// Flushes the grid onto `canvas`: every connected cell as its merged
    /// glyph, then the take-over marks on top — but never writing a cell in
    /// `reserved` (the cells an edge label has claimed), so a routed line can
    /// never overprint a label.
    fn flush_except(self, canvas: &mut Canvas, reserved: &BTreeSet<(i32, i32)>) {
        for (&(x, y), &c) in &self.conn {
            if self.marks.contains_key(&(x, y)) || reserved.contains(&(x, y)) {
                continue;
            }
            canvas.put(x, y, c.glyph(), CellRole::Edge);
        }
        for (&(x, y), &ch) in &self.marks {
            if reserved.contains(&(x, y)) {
                continue;
            }
            canvas.put(x, y, ch, CellRole::Edge);
        }
    }
}

/// Deterministically reserves the free label cells for one edge label,
/// returning the chosen top-left and recording every covered cell so a later
/// label can never be placed over it.
///
/// The label is tried at its natural anchor; if any cell there is already
/// reserved (by an earlier label or a node/edge cell, tracked in `taken`) it
/// is pushed down one row and retried, up to a bounded number of rows, so two
/// labels never share a cell. The chosen run is then added to `taken`.
fn reserve_label(
    taken: &mut BTreeSet<(i32, i32)>,
    occupied: &dyn Fn(i32, i32) -> bool,
    x: i32,
    y: i32,
    len: i32,
) -> (i32, i32) {
    let mut ly = y;
    for _ in 0..8 {
        let clash = (0..len).any(|d| taken.contains(&(x + d, ly)) || occupied(x + d, ly));
        if !clash {
            break;
        }
        ly += 1;
    }
    for d in 0..len {
        taken.insert((x + d, ly));
    }
    (x, ly)
}

/// One edge label awaiting placement: its text and the natural anchor cell it
/// would like its first glyph at (deterministically de-conflicted later).
struct PendingLabel<'a> {
    text: &'a str,
    x: i32,
    y: i32,
}

/// Routes every edge of a vertical (`TD`/`BT`) graph.
///
/// Forward edges out of one node share a single horizontal *bus* on the row
/// just past the parent (below for `TD`, above for `BT`), then each runs into
/// its child's own column with a directional arrowhead — so every label sits
/// on its child's distinct column and a fan-out's labels never overprint. A
/// skip-rank edge jogs into a free inter-column channel rather than dropping
/// through an intervening box; a back-edge/self-loop is a fully routed return
/// path out the side, down a reserved side channel, and back into the target.
///
/// All segments land in one [`Router`] grid first so junctions resolve to
/// exact `┌┐└┘├┤┬┴┼`; only then are labels reserved (avoiding every box *and*
/// every routed cell so a label can never share a cell or be overwritten),
/// the router flushed (skipping reserved label cells), and the labels drawn.
fn route_vertical(
    canvas: &mut Canvas,
    graph: &MermaidGraph,
    boxes: &BTreeMap<usize, PlacedBox>,
    base_rank: &[usize],
) {
    let idx = |id: &str| graph.index_of(id);
    let mut r = Router::default();
    let box_cells = box_occupancy(boxes, canvas.w, canvas.h);
    let up = graph.direction == Direction::BottomTop; // arrowhead points up
    let head = if up { '▲' } else { '▼' };
    let side_x = canvas.w - 2;
    let mut return_lane = 0i32;
    // Skip-rank edges route through their own dedicated column band just
    // right of every box, one column per edge — clear of both boxes and the
    // label band further right.
    let skip_x0 = boxes.values().map(|b| b.x + b.w).max().unwrap_or(0) + 1;
    let mut skip_lane = 0i32;
    let mut pending: Vec<PendingLabel<'_>> = Vec::new();

    for e in &graph.edges {
        let (Some(a), Some(b)) = (idx(&e.from), idx(&e.to)) else {
            continue;
        };
        let (Some(pa), Some(pb)) = (boxes.get(&a), boxes.get(&b)) else {
            continue;
        };

        // Self-loop: a small routed loop out the side and back in.
        if a == b {
            route_self_loop(&mut r, pa, e.kind, true, head);
            continue;
        }

        // Back-edge (target at/behind the source in flow order): a real
        // orthogonal return path through a reserved side lane.
        if base_rank[a] >= base_rank[b] {
            let lane = side_x - return_lane;
            return_lane = (return_lane + 2).min(side_x - 2).max(0);
            let (sx, sy) = if up {
                (pa.x + pa.w / 2, pa.y)
            } else {
                (pa.x + pa.w / 2, pa.y + pa.h - 1)
            };
            let (tx, ty) = (pb.x + pb.w - 1, pb.y + pb.h / 2);
            let exit_y = if up { sy - 1 } else { sy + 1 };
            r.tap(sx, exit_y, |c| {
                if up {
                    c.down = true;
                } else {
                    c.up = true;
                }
            });
            r.segment(sx, exit_y, lane, exit_y, e.kind);
            r.segment(lane, exit_y, lane, ty, e.kind);
            r.segment(lane, ty, tx + 1, ty, e.kind);
            if e.kind.has_head() {
                r.marks.insert((tx + 1, ty), '◀');
            }
            r.tap(tx + 1, ty, |c| c.right = true);
            if let Some(label) = &e.label {
                let len = label.chars().count() as i32;
                pending.push(PendingLabel {
                    text: label,
                    x: lane - len - 1,
                    y: exit_y,
                });
            }
            continue;
        }

        let (sx, sy) = if up {
            (pa.x + pa.w / 2, pa.y)
        } else {
            pa.bottom_center()
        };
        let (tx, ty) = if up {
            (pb.x + pb.w / 2, pb.y + pb.h - 1)
        } else {
            pb.top_center()
        };
        let by = if up { sy - 1 } else { sy + 1 }; // the per-parent bus row
        r.tap(sx, by, |c| {
            if up {
                c.down = true;
            } else {
                c.up = true;
            }
        });
        r.tap(tx, by, |c| {
            if up {
                c.up = true;
            } else {
                c.down = true;
            }
        });
        r.segment(sx, by, tx, by, e.kind);

        let skip = base_rank[b] > base_rank[a] + 1;
        if skip {
            // Jog out to this edge's own channel column in the skip band so
            // the descent never crosses an intervening box, then drop and
            // come back to the child's column at its entry row.
            let chan = skip_x0 + skip_lane * 2;
            skip_lane += 1;
            r.segment(tx, by, chan, by, e.kind);
            r.segment(chan, by, chan, near_row(ty, up), e.kind);
            r.segment(chan, near_row(ty, up), tx, near_row(ty, up), e.kind);
            r.segment(tx, near_row(ty, up), tx, ty_head(ty, up), e.kind);
        } else {
            r.segment(tx, by, tx, ty_head(ty, up), e.kind);
        }
        let hy = ty_head(ty, up);
        if (up && hy <= by) || (!up && hy >= by) {
            if e.kind.has_head() {
                r.marks.insert((tx, hy), head);
            }
            r.tap(tx, hy, |c| {
                if up {
                    c.up = true;
                } else {
                    c.down = true;
                }
            });
        }
        if let Some(label) = &e.label {
            // The label rides just right of the child's column, one row past
            // the bus — distinct per child, then de-conflicted below.
            pending.push(PendingLabel {
                text: label,
                x: tx + 1,
                y: if up { by - 1 } else { by + 1 },
            });
        }
    }

    place_labels_and_flush(canvas, r, &box_cells, &pending);
}

/// Reserves every pending label a free run of cells (avoiding boxes and the
/// routed grid, shifting deterministically on a clash so two labels never
/// share a cell), flushes the router *skipping* those reserved cells (so an
/// edge glyph can never overwrite a label), and finally paints the labels.
fn place_labels_and_flush(
    canvas: &mut Canvas,
    r: Router,
    box_cells: &dyn Fn(i32, i32) -> bool,
    pending: &[PendingLabel<'_>],
) {
    // A cell is occupied for label purposes if it is a box cell or a routed
    // line/mark cell — labels must stand entirely clear of both.
    let routed = |x: i32, y: i32| r.conn.contains_key(&(x, y)) || r.marks.contains_key(&(x, y));
    let occupied = |x: i32, y: i32| box_cells(x, y) || routed(x, y);

    let mut taken: BTreeSet<(i32, i32)> = BTreeSet::new();
    let mut placed: Vec<(i32, i32, &str)> = Vec::new();
    for p in pending {
        let len = p.text.chars().count() as i32;
        let (lx, ly) = reserve_label(&mut taken, &occupied, p.x.max(0), p.y.max(0), len);
        placed.push((lx, ly, p.text));
    }
    // Flush the routed grid, but never over a reserved label cell.
    r.flush_except(canvas, &taken);
    for (lx, ly, text) in placed {
        canvas.put_str(lx, ly, text, CellRole::EdgeLabel);
    }
}

/// The cell one step toward the bus from the child's entry (so the descent
/// stops just short of the box and the arrowhead is the only thing touching
/// it).
fn near_row(ty: i32, up: bool) -> i32 {
    if up { ty + 1 } else { ty - 1 }
}

/// The row the arrowhead sits on (one cell off the box on the bus side).
fn ty_head(ty: i32, up: bool) -> i32 {
    if up { ty + 1 } else { ty - 1 }
}

/// A small routed loop on a node (out the trailing edge, around, and back in
/// one row offset) with a proper arrowhead — a real self-loop, not a stub.
fn route_self_loop(r: &mut Router, b: &PlacedBox, kind: EdgeKind, vertical: bool, head: char) {
    if vertical {
        let (rx, ry) = b.right_center();
        let ox = rx + 2;
        let top = ry - 1;
        let bot = ry + 1;
        r.tap(rx + 1, top, |c| c.left = true);
        r.segment(rx + 1, top, ox, top, kind);
        r.segment(ox, top, ox, bot, kind);
        r.segment(ox, bot, rx + 1, bot, kind);
        if kind.has_head() {
            r.marks.insert((rx + 1, bot), '◀');
        }
        r.tap(rx, bot, |c| c.right = true);
        let _ = head;
    } else {
        let (bx, by) = b.bottom_center();
        let oy = by + 2;
        let left = bx - 1;
        let right = bx + 1;
        r.tap(left, by + 1, |c| c.up = true);
        r.segment(left, by + 1, left, oy, kind);
        r.segment(left, oy, right, oy, kind);
        r.segment(right, oy, right, by + 1, kind);
        if kind.has_head() {
            r.marks.insert((right, by + 1), head);
        }
    }
}

/// Routes every edge of a horizontal (`LR`/`RL`) graph — the column-wise
/// transpose of [`route_vertical`], with `RL` flipping the bus to the left of
/// the parent and the arrowhead to `◀`.
fn route_horizontal(
    canvas: &mut Canvas,
    graph: &MermaidGraph,
    boxes: &BTreeMap<usize, PlacedBox>,
    base_rank: &[usize],
) {
    let idx = |id: &str| graph.index_of(id);
    let mut r = Router::default();
    let box_cells = box_occupancy(boxes, canvas.w, canvas.h);
    let left = graph.direction == Direction::RightLeft; // arrowhead points left
    let head = if left { '◀' } else { '▶' };
    let side_y = canvas.h - 2;
    let mut return_lane = 0i32;
    // Skip-rank edges route through their own dedicated row band just below
    // every box, one row per edge — clear of boxes and labels.
    let skip_y0 = boxes.values().map(|b| b.y + b.h).max().unwrap_or(0) + 1;
    let mut skip_lane = 0i32;
    let mut pending: Vec<PendingLabel<'_>> = Vec::new();

    for e in &graph.edges {
        let (Some(a), Some(b)) = (idx(&e.from), idx(&e.to)) else {
            continue;
        };
        let (Some(pa), Some(pb)) = (boxes.get(&a), boxes.get(&b)) else {
            continue;
        };

        if a == b {
            route_self_loop(&mut r, pa, e.kind, false, '▼');
            continue;
        }

        if base_rank[a] >= base_rank[b] {
            let lane = side_y - return_lane;
            return_lane = (return_lane + 2).min(side_y - 2).max(0);
            let (sx, sy) = if left {
                (pa.x, pa.y + pa.h / 2)
            } else {
                (pa.x + pa.w - 1, pa.y + pa.h / 2)
            };
            let (tx, ty) = (pb.x + pb.w / 2, pb.y + pb.h - 1);
            let exit_x = if left { sx - 1 } else { sx + 1 };
            r.tap(exit_x, sy, |c| {
                if left {
                    c.right = true;
                } else {
                    c.left = true;
                }
            });
            r.segment(exit_x, sy, exit_x, lane, e.kind);
            r.segment(exit_x, lane, tx, lane, e.kind);
            r.segment(tx, lane, tx, ty + 1, e.kind);
            if e.kind.has_head() {
                r.marks.insert((tx, ty + 1), '▼');
            }
            r.tap(tx, ty + 1, |c| c.down = true);
            if let Some(label) = &e.label {
                pending.push(PendingLabel {
                    text: label,
                    x: exit_x + 1,
                    y: lane - 1,
                });
            }
            continue;
        }

        let (sx, sy) = if left {
            (pa.x, pa.y + pa.h / 2)
        } else {
            pa.right_center()
        };
        let (tx, ty) = if left {
            (pb.x + pb.w - 1, pb.y + pb.h / 2)
        } else {
            pb.left_center()
        };
        let bx = if left { sx - 1 } else { sx + 1 }; // the per-parent bus col
        r.tap(bx, sy, |c| {
            if left {
                c.right = true;
            } else {
                c.left = true;
            }
        });
        r.tap(bx, ty, |c| {
            if left {
                c.left = true;
            } else {
                c.right = true;
            }
        });
        r.segment(bx, sy, bx, ty, e.kind);

        let skip = base_rank[b] > base_rank[a] + 1;
        if skip {
            let chan = skip_y0 + skip_lane * 2;
            skip_lane += 1;
            r.segment(bx, ty, bx, chan, e.kind);
            r.segment(bx, chan, near_col(tx, left), chan, e.kind);
            r.segment(near_col(tx, left), chan, near_col(tx, left), ty, e.kind);
            r.segment(near_col(tx, left), ty, tx_head(tx, left), ty, e.kind);
        } else {
            r.segment(bx, ty, tx_head(tx, left), ty, e.kind);
        }
        // The head sits one cell off the child's entry edge, reached only if
        // it is on the flow side of the bus (leftward for `RL`, else right).
        let hx = tx_head(tx, left);
        if (left && hx <= bx) || (!left && hx >= bx) {
            if e.kind.has_head() {
                r.marks.insert((hx, ty), head);
            }
            r.tap(hx, ty, |c| {
                if left {
                    c.left = true;
                } else {
                    c.right = true;
                }
            });
        }
        if let Some(label) = &e.label {
            let len = label.chars().count() as i32;
            // Sit the label above the child's entry row, between the bus and
            // the child (clamped so it never starts left of the bus).
            let lx0 = if left {
                (bx + 1).min(tx - 1 - len)
            } else {
                (bx + 1).max(tx - 1 - len)
            };
            pending.push(PendingLabel {
                text: label,
                x: lx0.max(0),
                y: ty.saturating_sub(1).max(0),
            });
        }
    }

    place_labels_and_flush(canvas, r, &box_cells, &pending);
}

/// Horizontal analog of [`near_row`].
fn near_col(tx: i32, left: bool) -> i32 {
    if left { tx - 1 } else { tx + 1 }
}

/// Horizontal analog of [`ty_head`].
fn tx_head(tx: i32, left: bool) -> i32 {
    if left { tx + 1 } else { tx - 1 }
}

/// A predicate over the box interiors+borders so routing and label placement
/// can avoid drawing through a node, captured once per layout.
fn box_occupancy(
    boxes: &BTreeMap<usize, PlacedBox>,
    w: i32,
    h: i32,
) -> impl Fn(i32, i32) -> bool + '_ {
    let mut grid = vec![false; (w.max(0) * h.max(0)).max(0) as usize];
    for b in boxes.values() {
        for y in b.y..(b.y + b.h) {
            for x in b.x..(b.x + b.w) {
                if x >= 0 && y >= 0 && x < w && y < h {
                    grid[(y * w + x) as usize] = true;
                }
            }
        }
    }
    move |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= w || y >= h {
            false
        } else {
            grid[(y * w + x) as usize]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The canvas glyphs as one newline-terminated string per row (test-only,
    /// for layout assertions independent of the buffer blit).
    fn canvas_text(layout: &Layout) -> String {
        let c = &layout.canvas;
        let mut s = String::new();
        for y in 0..c.h {
            for x in 0..c.w {
                s.push(c.glyph(x, y));
            }
            s.push('\n');
        }
        s
    }

    // --- parse-level tests -------------------------------------------------

    #[test]
    fn header_sets_direction_and_accepts_flowchart_keyword() {
        assert_eq!(
            Mermaid::parse("graph TD\nA-->B").unwrap().direction,
            Direction::TopDown
        );
        assert_eq!(
            Mermaid::parse("flowchart LR\nA-->B").unwrap().direction,
            Direction::LeftRight
        );
        assert_eq!(
            Mermaid::parse("graph TB\nA-->B").unwrap().direction,
            Direction::TopDown
        );
        // BT/RL are now genuinely their own directions, not aliases.
        assert_eq!(
            Mermaid::parse("graph BT\nA-->B").unwrap().direction,
            Direction::BottomTop
        );
        assert_eq!(
            Mermaid::parse("graph RL\nA-->B").unwrap().direction,
            Direction::RightLeft
        );
    }

    #[test]
    fn missing_or_unrecognised_header_is_an_error() {
        assert_eq!(Mermaid::parse(""), Err(MermaidError::MissingHeader));
        assert_eq!(Mermaid::parse("A --> B"), Err(MermaidError::MissingHeader));
        assert_eq!(
            Mermaid::parse("graphic TD\nA-->B"),
            Err(MermaidError::MissingHeader)
        );
        assert_eq!(
            Mermaid::parse("graph TD\n%% only a comment"),
            Err(MermaidError::EmptyGraph)
        );
    }

    #[test]
    fn node_shapes_parse_from_bracket_style() {
        let g = Mermaid::parse("graph TD\nA[Rect]\nB(Round)\nC{Dec}\nD((Circ))\nE").unwrap();
        let by = |id: &str| g.nodes.iter().find(|n| n.id == id).unwrap();
        assert_eq!(by("A").shape, Shape::Rectangle);
        assert_eq!(by("A").label, "Rect");
        assert_eq!(by("B").shape, Shape::Round);
        assert_eq!(by("C").shape, Shape::Diamond);
        assert_eq!(by("D").shape, Shape::Circle);
        assert_eq!(by("D").label, "Circ");
        assert_eq!(by("E").shape, Shape::Rectangle);
        assert_eq!(by("E").label, "E");
    }

    #[test]
    fn quoted_label_keeps_spaces_and_brackets() {
        let g = Mermaid::parse("graph TD\nA[\"a, [b] c\"]").unwrap();
        assert_eq!(g.nodes[0].label, "a, [b] c");
    }

    #[test]
    fn edge_kinds_and_inline_label_parse() {
        let g = Mermaid::parse(
            "graph TD\n\
             A --> B\n\
             B --- C\n\
             C -.-> D\n\
             D ==> E\n\
             E -->|yes| F\n\
             F -- no --> G",
        )
        .unwrap();
        let kinds: Vec<_> = g.edges.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                EdgeKind::Arrow,
                EdgeKind::Open,
                EdgeKind::Dotted,
                EdgeKind::Thick,
                EdgeKind::Arrow,
                EdgeKind::Arrow,
            ]
        );
        assert_eq!(g.edges[4].label.as_deref(), Some("yes"));
        assert_eq!(g.edges[5].label.as_deref(), Some("no"));
        assert_eq!(g.nodes.len(), 7);
    }

    #[test]
    fn inline_declaration_on_first_use_then_reference_by_id() {
        let g = Mermaid::parse("graph TD\nA[Start] --> B[Mid]\nB --> C[End]").unwrap();
        assert_eq!(g.nodes.len(), 3);
        let b = g.nodes.iter().find(|n| n.id == "B").unwrap();
        assert_eq!(b.label, "Mid");
        assert_eq!(g.edges.len(), 2);
        assert_eq!(g.edges[1].from, "B");
        assert_eq!(g.edges[1].to, "C");
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let g = Mermaid::parse(
            "graph TD\n\
             %% a full-line comment\n\
             \n\
             A --> B  %% trailing comment\n",
        )
        .unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn malformed_lines_are_skipped_not_panicked() {
        let g = Mermaid::parse(
            "graph TD\n\
             --> B\n\
             ![oops\n\
             {}\n\
             A --> B",
        )
        .unwrap();
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].from, "A");
        assert_eq!(g.edges[0].to, "B");
    }

    #[test]
    fn dash_label_form_keeps_open_vs_arrow_kind() {
        let g = Mermaid::parse("graph TD\nA -- plain --- B\nA -- go --> C").unwrap();
        assert_eq!(g.edges[0].kind, EdgeKind::Open);
        assert_eq!(g.edges[0].label.as_deref(), Some("plain"));
        assert_eq!(g.edges[1].kind, EdgeKind::Arrow);
        assert_eq!(g.edges[1].label.as_deref(), Some("go"));
    }

    // --- chained / `&` shorthand parse tests ------------------------------

    #[test]
    fn chained_edges_on_one_line_parse_to_consecutive_links() {
        let g = Mermaid::parse("graph TD\nA --> B --> C").unwrap();
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(
            g.edges
                .iter()
                .map(|e| (e.from.as_str(), e.to.as_str()))
                .collect::<Vec<_>>(),
            vec![("A", "B"), ("B", "C")]
        );
        let g = Mermaid::parse("graph TD\nA[Start] -->|go| B(Mid) --> C{End}").unwrap();
        assert_eq!(g.edges[0].label.as_deref(), Some("go"));
        assert_eq!(g.edges[1].label, None);
        let mid = g.nodes.iter().find(|n| n.id == "B").unwrap();
        assert_eq!(mid.label, "Mid");
        assert_eq!(mid.shape, Shape::Round);
        assert_eq!(g.edges.last().unwrap().to, "C");
    }

    #[test]
    fn ampersand_shorthand_expands_to_a_cartesian_fan() {
        let g = Mermaid::parse("graph TD\nA & B --> C\nD --> E & F").unwrap();
        let pairs: Vec<_> = g
            .edges
            .iter()
            .map(|e| (e.from.as_str(), e.to.as_str()))
            .collect();
        assert_eq!(pairs, vec![("A", "C"), ("B", "C"), ("D", "E"), ("D", "F")]);
        let g = Mermaid::parse("graph TD\nA & B -->|link| C & D").unwrap();
        assert_eq!(g.edges.len(), 4);
        assert!(g.edges.iter().all(|e| e.label.as_deref() == Some("link")));
        assert_eq!(g.nodes.len(), 4);
    }

    #[test]
    fn arrow_inside_a_quoted_label_is_not_an_operator() {
        let g = Mermaid::parse("graph TD\nA[\"x --> y\"] --> B").unwrap();
        assert_eq!(g.nodes.len(), 2);
        let a = g.nodes.iter().find(|n| n.id == "A").unwrap();
        assert_eq!(a.label, "x --> y");
        assert_eq!(g.edges.len(), 1);
        assert_eq!(
            (g.edges[0].from.as_str(), g.edges[0].to.as_str()),
            ("A", "B")
        );
    }

    #[test]
    fn ampersand_inside_a_label_is_not_a_group_separator() {
        let g = Mermaid::parse("graph TD\nA[\"R & D\"] --> B & C").unwrap();
        let a = g.nodes.iter().find(|n| n.id == "A").unwrap();
        assert_eq!(a.label, "R & D");
        let pairs: Vec<_> = g
            .edges
            .iter()
            .map(|e| (e.from.as_str(), e.to.as_str()))
            .collect();
        assert_eq!(pairs, vec![("A", "B"), ("A", "C")]);
    }

    #[test]
    fn cycle_is_broken_deterministically_without_hanging() {
        let g = Mermaid::parse("graph TD\nA --> B\nB --> A").unwrap();
        let r = rank_nodes(&g);
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|&x| x <= 1));
    }

    // --- subgraph parse + render tests ------------------------------------

    #[test]
    fn subgraph_parses_members_title_and_nesting() {
        let g = Mermaid::parse(
            "graph TD\n\
             subgraph Outer\n\
             A --> B\n\
             subgraph Inner\n\
             C --> D\n\
             end\n\
             end\n\
             B --> C",
        )
        .unwrap();
        assert_eq!(g.subgraphs.len(), 2);
        let outer = g.subgraph("Outer").unwrap();
        assert_eq!(outer.title, "Outer");
        assert_eq!(outer.parent, None);
        // Outer's direct members are A, B, and the nested Inner subgraph.
        assert_eq!(outer.members, vec!["A", "B", "Inner"]);
        let inner = g.subgraph("Inner").unwrap();
        assert_eq!(inner.parent.as_deref(), Some("Outer"));
        assert_eq!(inner.members, vec!["C", "D"]);
        // Recursive flatten: Outer contains every leaf node.
        assert_eq!(g.cluster_members("Outer"), vec!["A", "B", "C", "D"]);
        // The cross-cluster edge B --> C still exists.
        assert!(g.edges.iter().any(|e| e.from == "B" && e.to == "C"));
    }

    #[test]
    fn subgraph_explicit_id_with_shaped_title() {
        let g = Mermaid::parse("graph TD\nsubgraph s1[Pipeline]\nA --> B\nend").unwrap();
        let sg = &g.subgraphs[0];
        assert_eq!(sg.id, "s1");
        assert_eq!(sg.title, "Pipeline");
        assert_eq!(sg.members, vec!["A", "B"]);
    }

    #[test]
    fn subgraph_draws_a_titled_cluster_box_around_members() {
        let g = Mermaid::parse("graph TD\nsubgraph G1\nA --> B\nend").unwrap();
        let txt = canvas_text(&lay_out(&g));
        assert_eq!(
            txt,
            " ┌ G1 ──┐  \n\
             \u{20}│      │  \n\
             \u{20}│ ┌───┐│  \n\
             \u{20}│ │ A ││  \n\
             \u{20}│ └───┘│  \n\
             \u{20}│   │  │  \n\
             \u{20}│   │  │  \n\
             \u{20}│   ▼  │  \n\
             \u{20}│ ┌───┐│  \n\
             \u{20}│ │ B ││  \n\
             \u{20}│ └───┘│  \n\
             \u{20}│      │  \n\
             \u{20}└──────┘  \n"
        );
        assert!(txt.contains("┌ G1 "));
        assert!(txt.contains("│ A │") && txt.contains("│ B │"));
    }

    // --- classDef / class / style tests -----------------------------------

    #[test]
    fn class_def_and_class_directive_resolve_node_style() {
        let g = Mermaid::parse(
            "graph TD\n\
             classDef warn fill:#f00,stroke:#ff0,color:#000\n\
             A --> B\n\
             class A warn",
        )
        .unwrap();
        assert_eq!(g.class_defs.len(), 1);
        let a = g.nodes.iter().find(|n| n.id == "A").unwrap();
        assert_eq!(a.class.as_deref(), Some("warn"));
        // #f00 -> nearest ANSI is LightRed, #ff0 -> LightYellow, #000 -> Black.
        assert_eq!(a.style.fill, Some(Color::LightRed));
        assert_eq!(a.style.stroke, Some(Color::LightYellow));
        assert_eq!(a.style.text, Some(Color::Black));
        // B is untouched.
        let b = g.nodes.iter().find(|n| n.id == "B").unwrap();
        assert_eq!(b.style, NodeStyle::default());
    }

    #[test]
    fn triple_colon_shorthand_and_style_directive() {
        let g = Mermaid::parse(
            "graph TD\n\
             classDef ok fill:#0f0\n\
             A:::ok --> B\n\
             style B fill:#00f,stroke:#fff",
        )
        .unwrap();
        let a = g.nodes.iter().find(|n| n.id == "A").unwrap();
        assert_eq!(a.class.as_deref(), Some("ok"));
        assert_eq!(a.style.fill, Some(Color::LightGreen));
        let b = g.nodes.iter().find(|n| n.id == "B").unwrap();
        assert_eq!(b.style.fill, Some(Color::Blue));
        assert_eq!(b.style.stroke, Some(Color::White));
    }

    #[test]
    fn css_color_maps_deterministically_to_nearest_ansi() {
        assert_eq!(css_color("#000"), Some(Color::Black));
        assert_eq!(css_color("#ffffff"), Some(Color::White));
        assert_eq!(css_color("#ff0000"), Some(Color::LightRed));
        assert_eq!(css_color("red"), Some(Color::LightRed));
        assert_eq!(css_color("navy"), Some(Color::Blue));
        assert_eq!(css_color("not-a-color"), None);
        assert_eq!(css_color("#xyz"), None);
    }

    #[test]
    fn styled_node_paints_its_fill_and_stroke_through_the_blit() {
        let g = Mermaid::parse(
            "graph TD\n\
             classDef hot fill:#f00,color:#000\n\
             A:::hot",
        )
        .unwrap();
        let layout = lay_out(&g);
        let mut buf = Buffer::empty(Rect::new(0, 0, 9, 3));
        layout.blit_into(buf.area(), &mut buf, Style::new(), &MermaidTheme::default());
        // The border carries the node skin's fill bg; the label cell its text
        // fg + fill bg.
        let border = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(border.symbol, '┌');
        assert_eq!(border.bg, Color::LightRed);
        let label = buf.get(Position::new(4, 1)).unwrap();
        assert_eq!(label.symbol, 'A');
        assert_eq!(label.fg, Color::Black);
        assert_eq!(label.bg, Color::LightRed);
    }

    // --- click / link registry tests --------------------------------------

    #[test]
    fn click_directive_registers_links_in_source_order() {
        let g = Mermaid::parse(
            "graph TD\n\
             A --> B\n\
             click A \"https://a.example\" \"tip\"\n\
             click B href \"https://b.example\"",
        )
        .unwrap();
        assert_eq!(
            g.clicks,
            vec![
                ("A".to_owned(), "https://a.example".to_owned()),
                ("B".to_owned(), "https://b.example".to_owned()),
            ]
        );
        let m = Mermaid::new(
            "graph TD\n\
             A[Start] --> B[Stop]\n\
             click A \"https://a.example\"\n\
             click B href \"https://b.example\"",
        );
        let links = m.links();
        assert_eq!(
            links,
            vec![
                Link::new("Start", "https://a.example"),
                Link::new("Stop", "https://b.example"),
            ]
        );
        // The activation shape mirrors Markdown's.
        let ev = links[1].activate(1);
        assert_eq!(ev.index, 1);
        assert_eq!(ev.href, "https://b.example");
    }

    #[test]
    fn link_at_hit_tests_a_clicked_node_box_and_misses_elsewhere() {
        let src = "graph TD\nA[Start] --> B[Stop]\nclick A \"u1\"\nclick B \"u2\"";
        let m = Mermaid::new(src);
        let area = Rect::new(0, 0, 24, 11);
        let regions = m.link_regions(area);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].index, 0);
        assert_eq!(regions[1].index, 1);
        // A point inside A's box hits index 0; inside B's box hits index 1.
        let a_rect = regions[0].rect;
        let inside_a = Position::new(a_rect.x + 1, a_rect.y + 1);
        assert_eq!(m.link_at(inside_a, area), Some(0));
        let b_rect = regions[1].rect;
        let inside_b = Position::new(b_rect.x + 1, b_rect.y + 1);
        assert_eq!(m.link_at(inside_b, area), Some(1));
        // The gap between the boxes is not a link.
        assert_eq!(m.link_at(Position::new(0, 0), area), None);
        // No clicks at all → no regions, no panic.
        assert!(
            Mermaid::new("graph TD\nA --> B")
                .link_regions(area)
                .is_empty()
        );
        assert_eq!(
            Mermaid::new("graph TD\nA --> B").link_at(Position::new(1, 1), area),
            None
        );
    }

    #[test]
    fn link_activation_at_resolves_a_clicked_node_to_its_href() {
        let src = "graph TD\nA[Start] --> B[Stop]\nclick A \"u1\"\nclick B \"u2\"";
        let m = Mermaid::new(src);
        let area = Rect::new(0, 0, 24, 11);
        let regions = m.link_regions(area);
        let inside_a = Position::new(regions[0].rect.x + 1, regions[0].rect.y + 1);
        let inside_b = Position::new(regions[1].rect.x + 1, regions[1].rect.y + 1);

        assert_eq!(
            m.link_activation_at(inside_a, area),
            Some(m.links()[0].activate(0))
        );
        assert_eq!(
            m.link_activation_at(inside_b, area),
            Some(m.links()[1].activate(1))
        );
        assert_eq!(m.link_activation_at(Position::new(0, 0), area), None);
        // No-desync: href tracks the resolved index.
        if let Some(act) = m.link_activation_at(inside_b, area) {
            assert_eq!(act.href, m.links()[act.index].href);
            assert_eq!(act.index, 1);
        }
    }

    #[test]
    fn link_regions_through_a_block_are_in_screen_coords() {
        let m = Mermaid::new("graph TD\nA[X]\nclick A \"u\"").block(Block::bordered());
        let area = Rect::new(0, 0, 12, 7);
        let regions = m.link_regions(area);
        assert_eq!(regions.len(), 1);
        // The region sits strictly inside the border frame.
        let r = regions[0].rect;
        assert!(r.x >= 1 && r.y >= 1);
        assert!(r.right() < area.right() && r.bottom() < area.bottom());
        // A click on the frame is not the node.
        assert_eq!(m.link_at(Position::new(0, 0), area), None);
    }

    // --- render snapshot tests --------------------------------------------

    #[test]
    fn two_node_top_down_renders_boxes_and_a_down_arrow() {
        let out = lines(Mermaid::new("graph TD\nA --> B"), 9, 9);
        assert_eq!(
            out,
            "  ┌───┐  \n\
             \u{20}\u{20}│ A │  \n\
             \u{20}\u{20}└───┘  \n\
             \u{20}\u{20}\u{20}\u{20}│    \n\
             \u{20}\u{20}\u{20}\u{20}│    \n\
             \u{20}\u{20}\u{20}\u{20}▼    \n\
             \u{20}\u{20}┌───┐  \n\
             \u{20}\u{20}│ B │  \n\
             \u{20}\u{20}└───┘  \n"
        );
    }

    #[test]
    fn branch_graph_lays_children_side_by_side() {
        let g = Mermaid::parse("graph TD\nA --> B\nA --> C").unwrap();
        let layout = lay_out(&g);
        let txt = canvas_text(&layout);
        assert!(txt.contains("│ B │") && txt.contains("│ C │"));
        assert!(txt.lines().next().unwrap().contains("┌───┐"));
        assert_eq!(txt.matches('▼').count(), 2);
    }

    #[test]
    fn left_right_direction_uses_columns_and_a_right_arrow() {
        let out = lines(Mermaid::new("graph LR\nA --> B"), 18, 5);
        assert!(out.contains('▶'));
        let row: &str = out.lines().nth(2).unwrap();
        let a = row.find('A').unwrap();
        let b = row.find('B').unwrap();
        assert!(a < b, "A column must be left of B column: {row:?}");
    }

    #[test]
    fn edge_label_is_drawn_on_the_connector() {
        let out = lines(Mermaid::new("graph TD\nA -->|yes| B"), 12, 9);
        assert!(out.contains("yes"), "edge label missing:\n{out}");
    }

    #[test]
    fn diamond_node_gets_a_decision_marker() {
        let out = lines(Mermaid::new("graph TD\nA{Go?}"), 14, 3);
        assert!(out.contains("◇ Go?"), "diamond marker missing:\n{out}");
    }

    #[test]
    fn malformed_source_renders_a_placeholder_not_a_panic() {
        let out = lines(Mermaid::new("not mermaid at all"), 32, 1);
        assert_eq!(out, "[mermaid: missing graph header] \n");
        let out = lines(Mermaid::new("graph TD\n%% nothing"), 24, 1);
        assert_eq!(out, "[mermaid: empty graph]  \n");
    }

    #[test]
    fn block_frames_the_diagram_in_the_inner_area() {
        let out = lines(
            Mermaid::new("graph TD\nA --> B").block(Block::bordered()),
            11,
            11,
        );
        assert!(out.starts_with("┌─────────┐\n"));
        assert!(out.ends_with("└─────────┘\n"));
        assert!(out.contains("│ A │"));
    }

    #[test]
    fn zero_area_and_tiny_area_are_no_ops() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        Mermaid::new("graph TD\nA --> B").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        let out = lines(
            Mermaid::new("graph TD\nA --> B").block(Block::bordered()),
            2,
            2,
        );
        assert_eq!(out, "┌┐\n└┘\n");
    }

    #[test]
    fn fan_out_labels_render_separately_not_overprinted() {
        let g = Mermaid::parse("graph TD\nA{Go?}\nA -->|yes| B\nA -->|no| C").unwrap();
        let txt = canvas_text(&lay_out(&g));
        assert_eq!(
            txt,
            "  ┌───────┐       \n\
             \u{20}\u{20}│ ◇ Go? │       \n\
             \u{20}\u{20}└───────┘       \n\
             \u{20}\u{20}┌───┴───┐       \n\
             \u{20}\u{20}│yes    │no     \n\
             \u{20}\u{20}▼       ▼       \n\
             ┌───┐   ┌───┐     \n\
             │ B │   │ C │     \n\
             └───┘   └───┘     \n"
        );
        assert!(txt.contains("│yes "));
        assert!(txt.contains("│no "));
        assert!(!txt.contains("nos"));
        assert!(txt.contains("┌───┴───┐"));
        assert_eq!(txt.matches('▼').count(), 2);
    }

    #[test]
    fn fan_in_converges_with_a_tee_not_an_overwrite() {
        let g = Mermaid::parse("graph TD\nA --> C\nB --> C").unwrap();
        let txt = canvas_text(&lay_out(&g));
        assert!(txt.contains("└───┬───┘"), "fan-in tee missing:\n{txt}");
        assert_eq!(txt.matches('▼').count(), 1);
    }

    #[test]
    fn left_right_fan_out_labels_render_separately() {
        let g = Mermaid::parse("graph LR\nA -->|hot| B\nA -->|cold| C").unwrap();
        let txt = canvas_text(&lay_out(&g));
        assert_eq!(
            txt,
            "       hot ┌───┐      \n\
             \u{20}\u{20}\u{20}\u{20}\u{20}┌────▶│ B │      \n\
             \u{20}\u{20}\u{20}\u{20}\u{20}│     └───┘      \n\
             ┌───┐│                \n\
             │ A │┤                \n\
             └───┘│                \n\
             \u{20}\u{20}\u{20}\u{20}\u{20}│cold ┌───┐      \n\
             \u{20}\u{20}\u{20}\u{20}\u{20}└────▶│ C │      \n\
             \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}└───┘      \n"
        );
        assert!(txt.contains("hot") && txt.contains("cold"));
        assert_eq!(txt.matches('▶').count(), 2);
    }

    #[test]
    fn chained_edges_render_as_a_three_box_column() {
        let out = lines(Mermaid::new("graph TD\nA --> B --> C"), 9, 15);
        assert!(out.contains("│ A │"));
        assert!(out.contains("│ B │"));
        assert!(out.contains("│ C │"));
        assert_eq!(out.matches('▼').count(), 2);
    }

    // --- BT / RL true-flip tests ------------------------------------------

    #[test]
    fn bottom_top_inverts_the_axis_with_an_up_arrow() {
        // `A --> B` in BT: A (the root) sits at the BOTTOM, B above it, with
        // the arrowhead pointing UP into B.
        let g = Mermaid::parse("graph BT\nA --> B").unwrap();
        let txt = canvas_text(&lay_out(&g));
        let a_row = txt.lines().position(|l| l.contains("│ A │")).unwrap();
        let b_row = txt.lines().position(|l| l.contains("│ B │")).unwrap();
        assert!(a_row > b_row, "BT: root A must be below B:\n{txt}");
        assert!(txt.contains('▲'), "BT must use an up arrowhead:\n{txt}");
        assert!(!txt.contains('▼'));
    }

    #[test]
    fn right_left_inverts_the_axis_with_a_left_arrow() {
        // `A --> B` in RL: A on the RIGHT, B on the LEFT, arrowhead `◀`.
        let g = Mermaid::parse("graph RL\nA --> B").unwrap();
        let txt = canvas_text(&lay_out(&g));
        let row = txt
            .lines()
            .find(|l| l.contains('A') && l.contains('B'))
            .unwrap();
        let a = row.find('A').unwrap();
        let b = row.find('B').unwrap();
        assert!(a > b, "RL: root A must be right of B: {row:?}");
        assert!(txt.contains('◀'), "RL must use a left arrowhead:\n{txt}");
        assert!(!txt.contains('▶'));
    }

    // --- routed back-edge / self-loop tests -------------------------------

    #[test]
    fn back_edge_is_a_routed_return_path_not_a_stub() {
        // B --> A is a back-edge: it must route as a real orthogonal path
        // with a proper arrowhead back into A, never the old `↺` stub.
        let g = Mermaid::parse("graph TD\nA --> B\nB --> A").unwrap();
        let txt = canvas_text(&lay_out(&g));
        assert!(!txt.contains('↺'), "no stub glyph allowed:\n{txt}");
        // The forward edge keeps its ▼ head; the return path adds a ◀ head
        // into A's side and routes through corners.
        assert!(txt.contains('▼'), "forward head missing:\n{txt}");
        assert!(txt.contains('◀'), "return arrowhead missing:\n{txt}");
        // A routed path bends — at least one corner glyph is present.
        assert!(
            txt.contains('┐') || txt.contains('┘') || txt.contains('└') || txt.contains('┌'),
            "return path must bend orthogonally:\n{txt}"
        );
    }

    #[test]
    fn self_loop_is_a_small_routed_loop_not_a_stub() {
        let g = Mermaid::parse("graph TD\nA --> A").unwrap();
        let txt = canvas_text(&lay_out(&g));
        assert!(!txt.contains('↺'), "no stub glyph allowed:\n{txt}");
        // A real loop: corners around the node and an arrowhead back in.
        assert!(txt.contains('◀'), "self-loop arrowhead missing:\n{txt}");
        assert!(
            txt.contains('┐') && txt.contains('┘'),
            "self-loop must form a routed rectangle:\n{txt}"
        );
    }

    // --- skip-rank routing test -------------------------------------------

    #[test]
    fn skip_rank_edge_routes_around_the_intervening_box() {
        // A --> B --> C plus A --> C: the A→C edge spans two ranks and must
        // jog into a free channel instead of dropping through B's box.
        let g = Mermaid::parse("graph TD\nA --> B\nB --> C\nA --> C").unwrap();
        let layout = lay_out(&g);
        let txt = canvas_text(&layout);
        // Locate B's box and assert no edge glyph is painted inside it.
        let bi = g.index_of("B").unwrap();
        let bb = layout.boxes.get(&bi).unwrap();
        for y in (bb.y + 1)..(bb.y + bb.h - 1) {
            for x in (bb.x + 1)..(bb.x + bb.w - 1) {
                let ch = layout.canvas.glyph(x, y);
                assert!(
                    ch == ' ' || ch == 'B',
                    "skip edge passed through B at ({x},{y})={ch:?}:\n{txt}"
                );
            }
        }
        // C still receives both incoming edges (a ┬ fan-in) and one head.
        assert!(txt.contains('▼'));
    }

    // --- label-cell sharing test ------------------------------------------

    #[test]
    fn two_children_in_one_column_get_distinct_label_cells() {
        // A skip layout that previously could land two labels in one cell:
        // every label cell must be unique.
        let g = Mermaid::parse("graph TD\nA -->|one| B\nB --> C\nA -->|two| C").unwrap();
        let layout = lay_out(&g);
        let c = &layout.canvas;
        // Collect the cells of every edge-label glyph and assert the labels
        // `one` and `two` are both fully present and never share a cell.
        let txt = canvas_text(&layout);
        assert!(txt.contains("one"), "label `one` missing:\n{txt}");
        assert!(txt.contains("two"), "label `two` missing:\n{txt}");
        // The two labels occupy different rows (deterministic de-conflict),
        // so neither overwrote the other into a fused string.
        let row_one =
            (0..c.h).find(|&y| (0..c.w).any(|x| c.glyph(x, y) == 'o' && c.glyph(x + 1, y) == 'n'));
        let row_two =
            (0..c.h).find(|&y| (0..c.w).any(|x| c.glyph(x, y) == 't' && c.glyph(x + 1, y) == 'w'));
        assert!(row_one.is_some() && row_two.is_some());
        assert_ne!(
            row_one, row_two,
            "the two labels must not share a row/cell:\n{txt}"
        );
    }

    // --- combined smoke test ----------------------------------------------

    #[test]
    fn subgraph_with_classdef_and_click_renders_without_panic() {
        let src = "graph TD\n\
                   classDef hot fill:#f00\n\
                   subgraph Pipe[Pipeline]\n\
                   A[Start]:::hot --> B[Stop]\n\
                   end\n\
                   click A \"https://example\"";
        let out = lines(Mermaid::new(src).block(Block::bordered()), 30, 16);
        assert!(out.contains("Pipeline"), "cluster title missing:\n{out}");
        assert!(out.contains("Start") && out.contains("Stop"));
        let m = Mermaid::new(src);
        assert_eq!(m.links(), vec![Link::new("Start", "https://example")]);
    }
}
