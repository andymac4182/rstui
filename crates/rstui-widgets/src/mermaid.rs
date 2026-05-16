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
//! edge labels — is a line-oriented scan, and a layered tree/DAG layout is a
//! longest-path ranking plus deterministic box placement. Both are a few
//! hundred lines, the same way [`Markdown`](crate::Markdown)'s parser and
//! [`Paragraph`](crate::Paragraph)'s wrap composer are hand-written rather
//! than pulling a crate. So `Mermaid` is a plain
//! [`Widget`] module here, zero new dependencies.
//!
//! # Progressive fidelity, not a fake renderer
//!
//! This is a real, tested subset — not a placeholder that pretends to be a
//! complete Mermaid engine. Supported now:
//!
//! - **Header**: `graph TD` / `graph TB` / `graph LR` (and the `flowchart`
//!   keyword). `TD` and `TB` are top-down; `LR` is left-to-right. `BT`
//!   (bottom-top) and `RL` (right-left) parse but are mapped to `TD`/`LR`
//!   respectively with no axis flip — a documented deferral.
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
//! - `%%` line comments and blank lines are ignored.
//!
//! Deliberately out of scope for this slice (each an additive follow-up that
//! does not change this shape): subgraphs, `classDef`/`class`/`style`
//! directives, click/href interactions, and a true `BT`/`RL` axis flip.
//! Malformed lines never panic — an unparseable line is skipped, and a graph
//! with no parseable nodes renders a clear placeholder.
//!
//! # Layout
//!
//! Nodes are assigned integer ranks by a longest-path layering from the roots
//! (nodes with no incoming edge); a graph that is all cycles falls back to
//! declaration order for its roots, and back-edges into an already-ranked
//! node are drawn but do not deepen the layering. For a top-down graph each
//! rank is a centered row of boxes; for left-right each rank is a column.
//!
//! Connectors are routed orthogonally through a shared connection grid: a
//! parent's forward edges leave on one *bus* (the row just below it, or the
//! column just right of it for left-right), then each edge turns into its
//! child's own distinct column/row and meets the box with a single `▼` (`▶`
//! for left-right) arrowhead. Because every cell's glyph is chosen from the
//! merged set of segments crossing it, corners, tees, and crossings resolve
//! to the exact `┌┐└┘├┤┬┴┼` — a fan-out splits cleanly, a fan-in converges on
//! a `┬`, and each edge's label rides its own child column/row so sibling
//! labels never overprint (the historic `yes`/`no` → `nos` collision is
//! fixed). Spacing is fixed so the same source and area always produce the
//! same cells — output is snapshot-testable through [`Buffer`] exactly like
//! every other widget.
//!
//! Honest remaining limits (each a clean follow-up, not a redesign): a
//! back-edge or self-loop is a short `↺` stub, not a routed return path; an
//! edge that skips ranks drops straight down its child's column and may pass
//! a box between those ranks; and two children of one parent that happen to
//! land in the same column (different ranks, equal centring) would share a
//! label cell. Clean trees, DAGs, and the common branch/merge shapes lay out
//! well.
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
use std::collections::BTreeMap;

use crate::block::Block;
use rstui_core::{Buffer, Color, Position, Rect, Style, Widget};

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

/// The flow direction declared by the `graph`/`flowchart` header.
///
/// Mermaid's four directions collapse to the two axes this slice lays out:
/// `TD`/`TB` are [`TopDown`](Self::TopDown) and `LR`/`RL` are
/// [`LeftRight`](Self::LeftRight). `BT` and `RL` are accepted but *not* axis
/// flipped (a documented deferral) — they render as `TD`/`LR`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    /// Roots at the top, edges flowing downward (`graph TD` / `graph TB`).
    #[default]
    TopDown,
    /// Roots at the left, edges flowing rightward (`graph LR`).
    LeftRight,
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

/// One declared node: a stable `id`, its display `label`, and its [`Shape`].
///
/// The first declaration of an id fixes its shape and label; a later bare
/// reference (e.g. the `B` in a second `B --> C`) reuses it and does not
/// overwrite either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The identifier used to reference this node in edges.
    pub id: String,
    /// The text drawn inside the node's box.
    pub label: String,
    /// The node's drawn outline.
    pub shape: Shape,
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

/// The parsed flowchart: its [`Direction`] and the declared [`Node`]s and
/// [`Edge`]s, in source order.
///
/// This is the public parse result so a caller or test can assert the parse
/// independently of the layout. Nodes appear in first-declaration order;
/// edges in source order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MermaidGraph {
    /// The declared flow direction.
    pub direction: Direction,
    /// Every node, in first-declaration order.
    pub nodes: Vec<Node>,
    /// Every edge, in source order.
    pub edges: Vec<Edge>,
}

impl MermaidGraph {
    /// The index into [`nodes`](Self::nodes) of the node with `id`, if any.
    fn index_of(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }
}

/// Why [`Mermaid::parse`] could not produce a graph.
///
/// Parsing is intentionally lenient — individual malformed lines are skipped,
/// not reported — so the only hard errors are a missing/!unrecognised header
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
// Theme
// ---------------------------------------------------------------------------

/// The styles [`Mermaid`] applies to the pieces of the diagram.
///
/// Every field is a *patch* layered over the widget base style (itself layered
/// over the framing [`Block`] fill), so an unset color falls through rather
/// than overriding the surrounding theme — the same
/// [`Style::patch`](rstui_core::Style) cascade the text model uses. Construct
/// the tuned terminal default with [`MermaidTheme::default`] and override only
/// the fields you care about.
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
            placeholder: Style::new().fg(Color::Red),
        }
    }
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// A read-only Mermaid flowchart view: parses its source and draws the
/// supported subset as a deterministic box-and-arrow diagram.
///
/// The source is a [`Cow<str>`](std::borrow::Cow) (a literal borrows, a
/// `String` is owned). An optional framing [`Block`], a base [`Style`] that
/// also fills the content area, and a [`MermaidTheme`] are the only knobs —
/// everything else is derived from the diagram. Parsing is exposed separately
/// via [`Mermaid::parse`] so callers and tests can assert the graph
/// independently of how it is laid out.
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
    /// or a test can inspect the graph (node/edge structure, shapes, labels,
    /// direction) independently of layout. Malformed lines are skipped; the
    /// only errors are a missing header ([`MermaidError::MissingHeader`]) or a
    /// graph with no nodes ([`MermaidError::EmptyGraph`]).
    pub fn parse(source: impl AsRef<str>) -> Result<MermaidGraph, MermaidError> {
        parse_graph(source.as_ref())
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
        if let Some(b) = self.block.clone() {
            b.render(area, buf);
        }
        if inner.is_empty() {
            return;
        }
        buf.set_style(inner, self.style);

        match parse_graph(self.source.as_ref()) {
            Ok(graph) => {
                let canvas = lay_out(&graph);
                canvas.blit(inner, buf, self.style, &self.theme);
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

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parses `src` into a [`MermaidGraph`]: a header line then line-oriented
/// node/edge statements. Lenient — an unparseable statement line is skipped.
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
    for line in lines {
        parse_statement(line.trim(), &mut graph);
    }
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
        // `TB`/`TD` top-down; `BT` is accepted but not flipped (deferred).
        "" | "TD" | "TB" | "BT" => Direction::TopDown,
        // `LR`; `RL` is accepted but not flipped (deferred).
        "LR" | "RL" => Direction::LeftRight,
        // An unknown suffix is treated as the default rather than rejected,
        // so a future direction does not make the whole diagram an error.
        _ => Direction::TopDown,
    })
}

/// Parses one statement line: a chain of one or more edges or a lone node
/// declaration. Unparseable input is ignored.
///
/// An endpoint may be a `&`-joined group (`A & B`), so each operator connects
/// every node on its left to every node on its right (a Cartesian fan); a
/// chained line `A --> B --> C` is split operator by operator, the right group
/// of one edge becoming the left group of the next, so the middle node is
/// declared exactly once and both links are recorded.
fn parse_statement(line: &str, graph: &mut MermaidGraph) {
    if line.is_empty() {
        return;
    }
    // Walk the operators left to right. `left_ids` is the resolved id list of
    // the group the next operator starts from; on the first pass it is parsed
    // from the head of the line. Each loop pins `rest` to the start of the
    // current left group so [`split_edge`] always has a valid left side.
    let mut rest = line;
    let mut left_ids: Option<Vec<String>> = None;
    let mut produced_edge = false;
    while let Some((left, edge, tail)) = split_edge(rest) {
        let from_ids = match left_ids.take() {
            Some(ids) => ids,
            None => group_ids(graph, left),
        };
        // The right group runs up to the *next* operator (so a chain does not
        // fold the following link's source into this label-less target), else
        // to the end of the line. On a chain the right group *is* the next
        // edge's left group, so the loop re-scans `tail` and reuses these ids
        // rather than re-parsing the shared middle node.
        let (right, next_rest) = match split_edge(tail) {
            Some((r, _, _)) => (&tail[..r.len()], Some(tail)),
            None => (tail, None),
        };
        let to_ids = group_ids(graph, right);
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
        upsert_node(graph, line);
    }
}

/// Resolves a `&`-joined endpoint group (`A & B[x] & C`) into its node ids,
/// declaring each node on first use. A lone token is a one-element group; an
/// unparseable member is skipped so a stray `&` cannot lose the rest. A `&`
/// *inside* a bracketed/quoted label (`A["a & b"]`) is not a separator.
fn group_ids(graph: &mut MermaidGraph, group: &str) -> Vec<String> {
    split_top_level(group, '&')
        .into_iter()
        .filter_map(|tok| upsert_node(graph, tok.trim()))
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
/// for an operator. [`parse_statement`] re-applies this on the tail to walk a
/// chained `A --> B --> C` link by link.
fn split_edge(line: &str) -> Option<(&str, EdgeToken, &str)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    // Bracket nesting depth and whether we are inside a `"..."` label; an
    // operator is only recognised at depth 0 outside quotes.
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
        // An operator starts at a run of `-` or `=` (after a node char), or a
        // `-.` dotted opener — only at the top level, outside any label.
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
///
/// Handles the two label placements: a `|text|` immediately after the arrow,
/// and a `-- text -->` where the label sits between two dash runs.
fn scan_operator(line: &str, start: usize) -> Option<(usize, EdgeKind, Option<String>)> {
    let rest = &line[start..];
    let lead = rest.chars().next()?;

    // Dotted: `-.->` or `-. text .->`.
    if rest.starts_with("-.") {
        // `-.->`
        if let Some(after) = rest.strip_prefix("-.->") {
            let (after, label) = take_pipe_label(after);
            return Some((line.len() - after.len(), EdgeKind::Dotted, label));
        }
        // `-. text .-> ` (label between the dotted halves)
        if let Some(body) = rest.strip_prefix("-.") {
            if let Some(end) = body.find(".->") {
                let label = clean_label(&body[..end]);
                let after = &body[end + 3..];
                return Some((line.len() - after.len(), EdgeKind::Dotted, label));
            }
        }
        return None;
    }

    // Thick: `==>` or `== text ==>`.
    if lead == '=' {
        let run = rest.chars().take_while(|&c| c == '=').count();
        if run >= 2 {
            let body = &rest[run..];
            if let Some(after) = body.strip_prefix('>') {
                let (after, label) = take_pipe_label(after);
                return Some((line.len() - after.len(), EdgeKind::Thick, label));
            }
            // `== text ==>`
            if let Some(close) = body.find("==>") {
                let label = clean_label(&body[..close]);
                let after = &body[close + 3..];
                return Some((line.len() - after.len(), EdgeKind::Thick, label));
            }
            // `== text ==` (open thick) — rare; treat as thick arrow target.
            if let Some(close) = body.find("==") {
                let label = clean_label(&body[..close]);
                let after = &body[close + 2..];
                return Some((line.len() - after.len(), EdgeKind::Thick, label));
            }
        }
        return None;
    }

    // Solid: `-->`, `---`, or `-- text -->`.
    if lead == '-' {
        let run = rest.chars().take_while(|&c| c == '-').count();
        if run >= 2 {
            let body = &rest[run..];
            // `-->` (arrowhead immediately follows the dash run)
            if let Some(after) = body.strip_prefix('>') {
                let (after, label) = take_pipe_label(after);
                return Some((line.len() - after.len(), EdgeKind::Arrow, label));
            }
            // `-- text -->` / `-- text ---` (label between dash runs)
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
            // Plain `---`/`--` with nothing after is an open link.
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

/// Parses a node token (`A`, `A[label]`, `A(round)`, `A{dec}`, `A((c))`),
/// inserts it if new (first declaration wins for shape/label), and returns
/// its id. A blank token yields `None`.
fn upsert_node(graph: &mut MermaidGraph, token: &str) -> Option<String> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    let (id, label, shape) = parse_node(token)?;
    match graph.index_of(&id) {
        Some(idx) => {
            // A later reference may upgrade a bare node to a shaped/labelled
            // one, but never overwrites an already-shaped declaration.
            if graph.nodes[idx].shape == Shape::Rectangle
                && graph.nodes[idx].label == graph.nodes[idx].id
                && (shape != Shape::Rectangle || label != id)
            {
                graph.nodes[idx].label = label;
                graph.nodes[idx].shape = shape;
            }
        }
        None => graph.nodes.push(Node {
            id: id.clone(),
            label,
            shape,
        }),
    }
    Some(id)
}

/// Splits a node token into `(id, label, shape)`. The id is the leading run
/// before any bracket; the bracket style picks the shape; a missing bracket
/// reuses the id as the label.
fn parse_node(token: &str) -> Option<(String, String, Shape)> {
    let token = token.trim();
    // The id is everything up to the first shape-opening bracket.
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

/// One placed box: its top-left cell, drawn size, label, and shape.
struct PlacedBox {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    label: String,
    shape: Shape,
}

impl PlacedBox {
    /// The cell at the centre of the box's bottom edge (a TD parent's exit).
    fn bottom_center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h - 1)
    }
    /// The cell at the centre of the box's top edge (a TD child's entry).
    fn top_center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y)
    }
    /// The cell at the centre of the box's right edge (an LR parent's exit).
    fn right_center(&self) -> (i32, i32) {
        (self.x + self.w - 1, self.y + self.h / 2)
    }
    /// The cell at the centre of the box's left edge (an LR child's entry).
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
    /// `(glyph, style)` per cell, row-major; the default cell is blank.
    cells: Vec<(char, CellRole)>,
}

/// Which theme style a painted cell takes — kept abstract so the canvas does
/// not depend on a concrete [`Style`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum CellRole {
    Blank,
    NodeBorder,
    NodeLabel,
    Edge,
    EdgeLabel,
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
    /// Used by the layout snapshot tests to assert the canvas independently
    /// of the buffer blit.
    #[cfg(test)]
    fn glyph(&self, x: i32, y: i32) -> char {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return ' ';
        }
        self.cells[(y * self.w + x) as usize].0
    }

    /// Draws a node's box with its shape glyphs and centred (clipped) label.
    fn draw_box(&mut self, b: &PlacedBox) {
        let (tl, tr, bl, br, horiz, vert) = b.shape.glyphs();
        let (x0, y0, x1, y1) = (b.x, b.y, b.x + b.w - 1, b.y + b.h - 1);
        for x in x0..=x1 {
            self.put(x, y0, horiz, CellRole::NodeBorder);
            self.put(x, y1, horiz, CellRole::NodeBorder);
        }
        for y in y0..=y1 {
            self.put(x0, y, vert, CellRole::NodeBorder);
            self.put(x1, y, vert, CellRole::NodeBorder);
        }
        self.put(x0, y0, tl, CellRole::NodeBorder);
        self.put(x1, y0, tr, CellRole::NodeBorder);
        self.put(x0, y1, bl, CellRole::NodeBorder);
        self.put(x1, y1, br, CellRole::NodeBorder);

        // A diamond carries a leading `◇` marker so a decision reads as one
        // even though the terminal box itself stays rectangular.
        let mut text: Cow<'_, str> = Cow::Borrowed(b.label.as_str());
        if b.shape == Shape::Diamond {
            text = Cow::Owned(format!("◇ {}", b.label));
        }
        let inner_w = (b.w - 2).max(0) as usize;
        let shown: String = text.chars().take(inner_w).collect();
        let pad = inner_w.saturating_sub(shown.chars().count());
        let lx = b.x + 1 + (pad / 2) as i32;
        self.put_str(lx, b.y + b.h / 2, &shown, CellRole::NodeLabel);
    }

    /// Blits the canvas into `area` of `buf`, centred when smaller than the
    /// area and clipped when larger, resolving each [`CellRole`] to a style.
    fn blit(&self, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
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
                let style = base.patch(match role {
                    CellRole::Blank => Style::new(),
                    CellRole::NodeBorder => theme.node_border,
                    CellRole::NodeLabel => theme.node_label,
                    CellRole::Edge => theme.edge,
                    CellRole::EdgeLabel => theme.edge_label,
                });
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
    // Longest-path relaxation: rank(to) = max(rank(to), rank(from)+1),
    // bounded to `n` passes so a cycle cannot diverge.
    for _ in 0..n {
        let mut changed = false;
        for e in &graph.edges {
            if let (Some(a), Some(b)) = (idx(&e.from), idx(&e.to)) {
                if a == b {
                    continue;
                }
                // A node with no incoming edge, or (in a pure cycle) the
                // first declared node, anchors the layering.
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

/// Lays the parsed graph out into a [`Canvas`]: rank the nodes, place each
/// rank's boxes, then route the connectors.
fn lay_out(graph: &MermaidGraph) -> Canvas {
    let n = graph.nodes.len();
    if n == 0 {
        return Canvas::new(0, 0);
    }
    let rank = rank_nodes(graph);
    let max_rank = *rank.iter().max().unwrap_or(&0);

    // Group node indices by rank, preserving declaration order within a rank.
    let mut ranks: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for (i, &r) in rank.iter().enumerate() {
        ranks[r].push(i);
    }

    // Box size: label width (+2 border, +pad), fixed height of 3. A diamond
    // reserves room for its `◇ ` marker.
    let box_w = |i: usize| -> i32 {
        let node = &graph.nodes[i];
        let label = node.label.chars().count() as i32;
        let marker = if node.shape == Shape::Diamond { 2 } else { 0 };
        (label + marker + 4).max(5)
    };
    let box_h = 3i32;

    let h_gap = 3i32; // columns between sibling boxes in a TD row
    // 3 blank rows between TD ranks: a drop row, a dedicated edge-label row,
    // and the jog row — so a labelled connector keeps its text clear of the
    // jog corner.
    let v_gap = 3i32;

    let mut boxes: BTreeMap<usize, PlacedBox> = BTreeMap::new();

    match graph.direction {
        Direction::TopDown => {
            // Lay each rank as a centred row; track the widest so the canvas
            // can centre every row to it.
            let mut row_w = vec![0i32; ranks.len()];
            for (r, members) in ranks.iter().enumerate() {
                let total: i32 = members.iter().map(|&i| box_w(i)).sum::<i32>()
                    + h_gap * (members.len().saturating_sub(1) as i32);
                row_w[r] = total;
            }
            let box_region = row_w.iter().copied().max().unwrap_or(0).max(1);
            // A TD edge label is drawn just right of its vertical connector;
            // reserve a margin so the longest one is never clipped at the
            // canvas edge (the box rows still centre within `box_region`).
            let label_pad = graph
                .edges
                .iter()
                .filter_map(|e| e.label.as_ref())
                .map(|l| l.chars().count() as i32 + 2)
                .max()
                .unwrap_or(0);
            let canvas_w = box_region + label_pad;
            let canvas_h = (max_rank as i32 + 1) * box_h + max_rank as i32 * v_gap;
            for (r, members) in ranks.iter().enumerate() {
                let mut x = (box_region - row_w[r]) / 2;
                let y = r as i32 * (box_h + v_gap);
                for &i in members {
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
            let mut canvas = Canvas::new(canvas_w, canvas_h.max(box_h));
            for b in boxes.values() {
                canvas.draw_box(b);
            }
            route_top_down(&mut canvas, graph, &boxes, &rank);
            canvas
        }
        Direction::LeftRight => {
            // Each rank is a column; rows are sized to the tallest column.
            let col_h = |members: &[usize]| -> i32 {
                box_h * members.len() as i32 + v_gap * (members.len().saturating_sub(1) as i32)
            };
            let canvas_h = ranks.iter().map(|m| col_h(m)).max().unwrap_or(0).max(box_h);
            let mut col_x = vec![0i32; ranks.len()];
            let mut acc = 0i32;
            for (r, members) in ranks.iter().enumerate() {
                col_x[r] = acc;
                let widest = members.iter().map(|&i| box_w(i)).max().unwrap_or(5);
                acc += widest + h_gap * 2;
            }
            let canvas_w = acc.max(1);
            for (r, members) in ranks.iter().enumerate() {
                let total = col_h(members);
                let mut y = (canvas_h - total) / 2;
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
            for b in boxes.values() {
                canvas.draw_box(b);
            }
            route_left_right(&mut canvas, graph, &boxes);
            canvas
        }
    }
}

/// The four orthogonal neighbours a routed cell connects to, plus the texture
/// of the straight run that owns it. Choosing the glyph from the *merged* set
/// of arms (rather than per edge) makes a fan-out's junctions always the right
/// `┌┐└┘├┤┬┴┼`, while the kept texture keeps a dotted/thick run intact.
#[derive(Clone, Copy, Default)]
struct Conn {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    /// The line texture of the straight segment that owns this cell, if any.
    /// A junction (more than two arms, or an `L`/`T`/`+`) always uses the
    /// solid set, so this only matters for a plain run.
    kind: Option<EdgeKind>,
}

impl Conn {
    /// The box-drawing glyph for this connection set. A straight run keeps its
    /// own edge texture (a dotted/thick link stays dotted/thick); every corner
    /// or junction falls back to the solid set, which reads cleanly there.
    fn glyph(self) -> char {
        let kind = self.kind.unwrap_or(EdgeKind::Arrow);
        match (self.up, self.down, self.left, self.right) {
            // Straight runs keep the edge texture.
            (true, true, false, false) => kind.line(true),
            (false, false, true, true) => kind.line(false),
            // Corners.
            (false, true, false, true) => '┌',
            (false, true, true, false) => '┐',
            (true, false, false, true) => '└',
            (true, false, true, false) => '┘',
            // Tees.
            (true, true, false, true) => '├',
            (true, true, true, false) => '┤',
            (false, true, true, true) => '┬',
            (true, false, true, true) => '┴',
            // Cross.
            (true, true, true, true) => '┼',
            // A lone stub (one arm only) reads as the run along that arm.
            (true, false, false, false) | (false, true, false, false) => kind.line(true),
            (false, false, true, false) | (false, false, false, true) => kind.line(false),
            (false, false, false, false) => ' ',
        }
    }
}

/// Accumulates every edge's orthogonal segments into one connection grid
/// before any glyph is chosen, so overlapping runs (a fan-out's shared bus, a
/// fan-in's converging column, two edges that cross) merge into the correct
/// junction glyph instead of one edge overwriting another.
///
/// `conn` is the merged line mask per cell; `marks` are the glyphs that *take
/// over* a cell (arrowheads, back-edge `↺`) and are painted last so a head is
/// never buried under a line.
#[derive(Default)]
struct Router {
    conn: BTreeMap<(i32, i32), Conn>,
    marks: BTreeMap<(i32, i32), char>,
}

impl Router {
    /// Adds one orthogonal `kind`-textured segment between two cells on a
    /// shared row or column, setting the connecting arms at every cell along
    /// it (so the run is continuous and merges with anything already there).
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

    /// Records the connection arm by which a node box touches a routed cell
    /// (the parent's exit / the child's entry), so the junction glyph there
    /// reads as joined to the box rather than a dangling stub.
    fn tap(&mut self, x: i32, y: i32, arm: impl FnOnce(&mut Conn)) {
        arm(self.conn.entry((x, y)).or_default());
    }

    /// Flushes the grid onto `canvas`: every connected cell as its merged
    /// glyph (each straight run in its own texture), then the take-over marks
    /// on top.
    fn flush(self, canvas: &mut Canvas) {
        for (&(x, y), &c) in &self.conn {
            if self.marks.contains_key(&(x, y)) {
                continue;
            }
            canvas.put(x, y, c.glyph(), CellRole::Edge);
        }
        for (&(x, y), &ch) in &self.marks {
            canvas.put(x, y, ch, CellRole::Edge);
        }
    }
}

/// Routes every edge of a top-down graph with a per-parent fan-out bus.
///
/// Forward edges out of one node share a single horizontal *bus* on the row
/// just below the parent, then each drops in its child's own column with a
/// `▼` head — so every label sits on its child's distinct column and a
/// fan-out's labels never overprint (the historic `yes`/`no` → `nos` bug).
/// All segments land in one [`Router`] grid first, so corners, tees, a
/// fan-in's converging column, and crossings resolve to exact `┌┐└┘├┤┬┴┼`
/// glyphs while every straight run keeps its own dotted/thick texture.
/// Back-edges and self-loops keep a short `↺` stub (a documented limit: no
/// orthogonal back-edge routing).
fn route_top_down(
    canvas: &mut Canvas,
    graph: &MermaidGraph,
    boxes: &BTreeMap<usize, PlacedBox>,
    rank: &[usize],
) {
    let idx = |id: &str| graph.index_of(id);
    let mut r = Router::default();

    for e in &graph.edges {
        let (Some(a), Some(b)) = (idx(&e.from), idx(&e.to)) else {
            continue;
        };
        let (Some(pa), Some(pb)) = (boxes.get(&a), boxes.get(&b)) else {
            continue;
        };
        if rank[a] >= rank[b] {
            // Self-loop / back-edge: a short stub, not a misleading long line.
            let (sx, sy) = pa.bottom_center();
            r.marks.insert((sx, sy + 1), '↺');
            continue;
        }
        let (sx, sy) = pa.bottom_center();
        let (tx, ty) = pb.top_center();
        let by = sy + 1; // the bus row, shared by this parent's fan-out
        // Parent feeds the bus from above at its column; child taps it from
        // below at its own (distinct) column.
        r.tap(sx, by, |c| c.up = true);
        r.tap(tx, by, |c| c.down = true);
        // The bus hop across to the child column, then the drop into it.
        r.segment(sx, by, tx, by, e.kind);
        r.segment(tx, by, tx, ty - 1, e.kind);
        let head_y = ty - 1;
        if head_y >= by {
            if e.kind.has_head() {
                r.marks.insert((tx, head_y), '▼');
            }
            r.tap(tx, head_y, |c| c.down = true);
        }
        if let Some(label) = &e.label {
            // The label rides the child's own column (distinct per child), one
            // row under the bus — so sibling labels never overprint.
            canvas.put_str(tx + 1, by + 1, label, CellRole::EdgeLabel);
        }
    }
    r.flush(canvas);
}

/// Routes every edge of a left-right graph with a per-parent fan-out bus —
/// the column-wise transpose of [`route_top_down`].
///
/// Forward edges out of one node share a vertical *bus* in the column just
/// right of the parent, then each runs into its child's own row with a `▶`
/// head; each label sits above its child's distinct row so siblings never
/// overprint. Junctions resolve through the same [`Router`] grid.
/// Back-/self-edges keep the `↺` stub (a documented limit).
fn route_left_right(canvas: &mut Canvas, graph: &MermaidGraph, boxes: &BTreeMap<usize, PlacedBox>) {
    let idx = |id: &str| graph.index_of(id);
    let mut r = Router::default();

    for e in &graph.edges {
        let (Some(a), Some(b)) = (idx(&e.from), idx(&e.to)) else {
            continue;
        };
        let (Some(pa), Some(pb)) = (boxes.get(&a), boxes.get(&b)) else {
            continue;
        };
        if pb.x <= pa.x {
            let (sx, sy) = pa.right_center();
            r.marks.insert((sx + 1, sy), '↺');
            continue;
        }
        let (sx, sy) = pa.right_center();
        let (tx, ty) = pb.left_center();
        let bx = sx + 1; // the bus column, shared by this parent's fan-out
        r.tap(bx, sy, |c| c.left = true);
        r.tap(bx, ty, |c| c.right = true);
        r.segment(bx, sy, bx, ty, e.kind);
        r.segment(bx, ty, tx - 1, ty, e.kind);
        let head_x = tx - 1;
        if head_x >= bx {
            if e.kind.has_head() {
                r.marks.insert((head_x, ty), '▶');
            }
            r.tap(head_x, ty, |c| c.right = true);
        }
        if let Some(label) = &e.label {
            // The label rides above the child's own row (distinct per child),
            // so sibling labels never overprint.
            let lx = (bx + 1).max(tx - 1 - label.chars().count() as i32);
            canvas.put_str(lx, ty.saturating_sub(1).max(0), label, CellRole::EdgeLabel);
        }
    }
    r.flush(canvas);
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
        // TB maps to top-down; RL maps to left-right (no flip — documented).
        assert_eq!(
            Mermaid::parse("graph TB\nA-->B").unwrap().direction,
            Direction::TopDown
        );
        assert_eq!(
            Mermaid::parse("graph RL\nA-->B").unwrap().direction,
            Direction::LeftRight
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
        // Header but no nodes at all.
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
        // A bare node reuses its id as the label.
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
        // Seven nodes auto-declared from the edges (A..G).
        assert_eq!(g.nodes.len(), 7);
    }

    #[test]
    fn inline_declaration_on_first_use_then_reference_by_id() {
        let g = Mermaid::parse("graph TD\nA[Start] --> B[Mid]\nB --> C[End]").unwrap();
        assert_eq!(g.nodes.len(), 3);
        // B keeps its first declared label, not overwritten by the bare reuse.
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
        // `-->` with no left side, a stray bracket, an empty node — all
        // tolerated; the one good edge still parses.
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

    // --- render snapshot tests --------------------------------------------

    #[test]
    fn two_node_top_down_renders_boxes_and_a_down_arrow() {
        // 3 box rows, a 3-row connector (drop, drop, ▼ head), 3 box rows —
        // exactly 9 rows, filling the 9-tall buffer with no trailing blank.
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
        // A fans out to B and C: B and C share rank 1, side by side.
        let g = Mermaid::parse("graph TD\nA --> B\nA --> C").unwrap();
        let canvas = lay_out(&g);
        let txt = canvas_text(&canvas);
        // Two boxes on the bottom rank, the root centred above them.
        assert!(txt.contains("│ B │") && txt.contains("│ C │"));
        assert!(txt.lines().next().unwrap().contains("┌───┐"));
        // The fan-out row carries two arrowheads.
        assert_eq!(txt.matches('▼').count(), 2);
    }

    #[test]
    fn left_right_direction_uses_columns_and_a_right_arrow() {
        let out = lines(Mermaid::new("graph LR\nA --> B"), 18, 5);
        // A on the left, B on the right, a `▶` head into B.
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
        // 31-char message in a 32-wide row → exactly one trailing space.
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
        // The frame is intact and a node box sits inside it.
        assert!(out.starts_with("┌─────────┐\n"));
        assert!(out.ends_with("└─────────┘\n"));
        assert!(out.contains("│ A │"));
    }

    #[test]
    fn zero_area_and_tiny_area_are_no_ops() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        Mermaid::new("graph TD\nA --> B").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        // A block too small for an inner area draws the frame, no content,
        // and does not panic.
        let out = lines(
            Mermaid::new("graph TD\nA --> B").block(Block::bordered()),
            2,
            2,
        );
        assert_eq!(out, "┌┐\n└┘\n");
    }

    #[test]
    fn cycle_is_broken_deterministically_without_hanging() {
        // A <-> B is a 2-cycle: ranking must terminate and stay finite.
        let g = Mermaid::parse("graph TD\nA --> B\nB --> A").unwrap();
        let r = rank_nodes(&g);
        assert_eq!(r.len(), 2);
        // The back-edge does not deepen the layering past one step.
        assert!(r.iter().all(|&x| x <= 1));
    }

    // --- chained / `&` shorthand parse tests ------------------------------

    #[test]
    fn chained_edges_on_one_line_parse_to_consecutive_links() {
        // `A --> B --> C` is one statement: two links, the middle node
        // declared exactly once.
        let g = Mermaid::parse("graph TD\nA --> B --> C").unwrap();
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(
            g.edges
                .iter()
                .map(|e| (e.from.as_str(), e.to.as_str()))
                .collect::<Vec<_>>(),
            vec![("A", "B"), ("B", "C")]
        );
        // A chain may carry a per-link label and shaped middle nodes.
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
        // `A & B --> C` is A→C and B→C; `D --> E & F` is D→E and D→F.
        let g = Mermaid::parse("graph TD\nA & B --> C\nD --> E & F").unwrap();
        let pairs: Vec<_> = g
            .edges
            .iter()
            .map(|e| (e.from.as_str(), e.to.as_str()))
            .collect();
        assert_eq!(pairs, vec![("A", "C"), ("B", "C"), ("D", "E"), ("D", "F")]);
        // Both sides may be groups, and a shared label rides every link.
        let g = Mermaid::parse("graph TD\nA & B -->|link| C & D").unwrap();
        assert_eq!(g.edges.len(), 4);
        assert!(g.edges.iter().all(|e| e.label.as_deref() == Some("link")));
        assert_eq!(g.nodes.len(), 4);
    }

    #[test]
    fn arrow_inside_a_quoted_label_is_not_an_operator() {
        // The scan skips bracketed/quoted spans, so `-->` inside a label does
        // not split the node off.
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
        // `&` only groups at the top level; one inside a label stays in it.
        let g = Mermaid::parse("graph TD\nA[\"R & D\"] --> B & C").unwrap();
        let a = g.nodes.iter().find(|n| n.id == "A").unwrap();
        assert_eq!(a.label, "R & D");
        // The trailing `B & C` is still a real two-node group.
        let pairs: Vec<_> = g
            .edges
            .iter()
            .map(|e| (e.from.as_str(), e.to.as_str()))
            .collect();
        assert_eq!(pairs, vec![("A", "B"), ("A", "C")]);
    }

    // --- render snapshot tests (new routing) ------------------------------

    #[test]
    fn fan_out_labels_render_separately_not_overprinted() {
        // The historic v1 bug: a node with two labelled outgoing edges drew
        // `yes`/`no` on the *same* parent row, colliding into `nos`. Each
        // label now rides its own child column, one row under the bus.
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
        // Both labels are present, intact, and on different rows-of-text only
        // by column — never fused.
        assert!(txt.contains("│yes "));
        assert!(txt.contains("│no "));
        assert!(!txt.contains("nos"));
        // Orthogonal junction glyphs are present on the fan-out bus.
        assert!(txt.contains("┌───┴───┐"));
        assert_eq!(txt.matches('▼').count(), 2);
    }

    #[test]
    fn fan_in_converges_with_a_tee_not_an_overwrite() {
        // Two parents into one child must merge into a `┬`, not have one
        // edge's corner stomp the other's.
        let g = Mermaid::parse("graph TD\nA --> C\nB --> C").unwrap();
        let txt = canvas_text(&lay_out(&g));
        assert!(txt.contains("└───┬───┘"), "fan-in tee missing:\n{txt}");
        assert_eq!(txt.matches('▼').count(), 1);
    }

    #[test]
    fn left_right_fan_out_labels_render_separately() {
        // The LR transpose of the fan-out fix: each label above its own
        // child row.
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
        // `A --> B --> C` lays out as three stacked ranks linked top-down.
        let out = lines(Mermaid::new("graph TD\nA --> B --> C"), 9, 15);
        assert!(out.contains("│ A │"));
        assert!(out.contains("│ B │"));
        assert!(out.contains("│ C │"));
        assert_eq!(out.matches('▼').count(), 2);
    }

    /// The canvas glyphs as one newline-terminated string per row (test-only,
    /// for layout assertions independent of the buffer blit).
    fn canvas_text(c: &Canvas) -> String {
        let mut s = String::new();
        for y in 0..c.h {
            for x in 0..c.w {
                s.push(c.glyph(x, y));
            }
            s.push('\n');
        }
        s
    }
}
