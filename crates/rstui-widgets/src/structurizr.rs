//! [`Structurizr`] — a read-only widget that parses a real subset of the
//! [Structurizr DSL](https://docs.structurizr.com/dsl/language) and renders
//! its **C4 model** as a deterministic terminal diagram.
//!
//! # Why a hand-written parser, and why its own widget
//!
//! Structurizr is *not* Mermaid — it is a separate architecture-as-code
//! language with its own grammar (a `workspace` of a `model` and `views`),
//! so it is a separate widget, not a Mermaid diagram type. rstui stays
//! dependency-free below the backend, so — exactly like
//! [`Markdown`](crate::Markdown) and [`Mermaid`](crate::Mermaid) — the parser
//! is hand-written line/brace scanning rather than a pulled crate.
//!
//! # Supported subset (progressive fidelity, not a fake)
//!
//! Parsed from the DSL, faithfully to the official `structurizr-dsl`
//! grammar:
//!
//! - **`workspace ["name"] ["description"] { … }`** — the root, with an
//!   optional `model { … }` and `views { … }`.
//! - **Model elements**: `person`, `softwareSystem`, `container`,
//!   `component`, `group`, `deploymentEnvironment` / `deploymentNode` /
//!   `infrastructureNode` / `softwareSystemInstance` / `containerInstance`,
//!   with the exact argument orders (`person <name> [description] [tags]`,
//!   `container <name> [description] [technology] [tags]`, …), an optional
//!   `id = ` assignment, nesting via `{ … }`, and the in-block
//!   `description` / `technology` / `tags` / `url` properties.
//! - **Relationships**: `source -> destination [description] [technology]
//!   [tags]`, the implicit `-> destination …` form inside an element, and
//!   `this`.
//! - **Views**: `systemLandscape`, `systemContext <id>`, `container <id>`,
//!   `component <id>`, `deployment <*|id> <env>`, `dynamic`, `filtered`,
//!   `custom`, with `include *|<ids>`, `exclude <ids>`,
//!   `autolayout [tb|bt|lr|rl]`, `title`, `description`, `default`.
//! - Comments (`//`, `#`, `/* … */`) and `"quoted"` tokens with `\"`.
//!
//! `styles`, `theme(s)`, `configuration`, `properties`, `!`-directives and
//! the like are parsed-past (skipped) rather than erroring — the renderer
//! needs structure, not skinning. Any unparseable line is skipped, never a
//! panic; a source with no model renders a clear placeholder.
//!
//! # Rendering
//!
//! One C4 **view** is rendered at a time (select with [`Structurizr::view`];
//! the default is the `default`-marked view, else the first). The chosen
//! view's scope resolves which elements/relationships are visible — a System
//! Context centres the subject system with the people/systems it talks to; a
//! Container view draws the subject system as a **boundary box** around its
//! containers; a Component view the subject container around its components;
//! a System Landscape every top-level element; a Deployment view the
//! environment's nested nodes. Elements are laid out by a deterministic
//! longest-path ranking along the `autolayout` axis and drawn as
//! C4 cards (a `«stereotype»` line, the name, the technology, a clipped
//! description), externals dimmed, with labelled relationship arrows. A
//! header names the workspace and view and pages `‹ k/n ›` when there are
//! several. Spacing is fixed, so the result is snapshot-testable through a
//! [`Buffer`] exactly like every other widget.
//!
//! ```
//! use rstui_core::{Buffer, Rect, Widget};
//! use rstui_widgets::Structurizr;
//!
//! let src = "workspace {\n model {\n u = person \"User\"\n \
//!            s = softwareSystem \"System\"\n u -> s \"Uses\"\n }\n \
//!            views {\n systemContext s {\n include *\n }\n }\n}";
//! let ws = Structurizr::parse(src).unwrap();
//! assert_eq!(ws.elements.len(), 2);
//! assert_eq!(ws.relationships.len(), 1);
//!
//! let mut buf = Buffer::empty(Rect::new(0, 0, 40, 12));
//! Structurizr::new(src).render(buf.area(), &mut buf);
//! ```

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::block::Block;
use crate::diagram::{BoxStyle, Surface};
use rstui_core::{Buffer, Color, Position, Rect, Style, Widget};

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

/// The kind of a C4 model [`Element`], fixing how it is drawn and which view
/// scopes include it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    /// `person` — a human user/actor (drawn as a rounded card).
    Person,
    /// `softwareSystem` — the highest level of abstraction.
    SoftwareSystem,
    /// `container` — an application/data-store inside a software system.
    Container,
    /// `component` — a grouping of code inside a container.
    Component,
    /// `group` — a non-C4 visual grouping of elements.
    Group,
    /// `deploymentNode` — infrastructure that hosts instances.
    DeploymentNode,
    /// `infrastructureNode` — supporting infrastructure (load balancer, …).
    InfrastructureNode,
    /// `softwareSystemInstance` / `containerInstance` — a deployed instance.
    Instance,
}

impl ElementKind {
    /// The C4 `«stereotype»` text drawn on the element's first card row.
    const fn stereotype(self) -> &'static str {
        match self {
            Self::Person => "«Person»",
            Self::SoftwareSystem => "«Software System»",
            Self::Container => "«Container»",
            Self::Component => "«Component»",
            Self::Group => "«Group»",
            Self::DeploymentNode => "«Deployment Node»",
            Self::InfrastructureNode => "«Infrastructure Node»",
            Self::Instance => "«Instance»",
        }
    }

    /// Whether this kind can contain children (drives boundary boxes).
    const fn is_container_like(self) -> bool {
        matches!(
            self,
            Self::SoftwareSystem | Self::Container | Self::Group | Self::DeploymentNode
        )
    }
}

/// One declared model element: a stable `id`, its [`ElementKind`], the
/// `name`/`description`/`technology`/`tags` from the DSL, and its enclosing
/// `parent` (for a nested `container`/`component`/node or a `group`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// The identifier used to reference this element in relationships/views
    /// (the explicit `id =`, else a stable generated `e<N>`).
    pub id: String,
    /// What the element is.
    pub kind: ElementKind,
    /// The display name.
    pub name: String,
    /// The optional description.
    pub description: Option<String>,
    /// The optional technology (containers/components/nodes).
    pub technology: Option<String>,
    /// Comma-split tags (`"Existing System"`, `"External"`, …).
    pub tags: Vec<String>,
    /// The index into [`Workspace::elements`] of the enclosing element.
    pub parent: Option<usize>,
}

impl Element {
    /// Whether the element carries the conventional `External` tag.
    fn is_external(&self) -> bool {
        self.tags.iter().any(|t| t.eq_ignore_ascii_case("external"))
    }
}

/// One directed relationship `source -> destination`, with the optional
/// description/technology drawn on the connector. Endpoints are indices into
/// [`Workspace::elements`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// Source element index.
    pub source: usize,
    /// Destination element index.
    pub destination: usize,
    /// The optional connector label.
    pub description: Option<String>,
    /// The optional connector technology.
    pub technology: Option<String>,
}

/// The kind of a C4 [`View`], fixing which elements its scope resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    /// `systemLandscape` — every top-level person and software system.
    SystemLandscape,
    /// `systemContext <id>` — the subject system and its direct neighbours.
    SystemContext,
    /// `container <id>` — the subject system's containers, boundary-boxed.
    Container,
    /// `component <id>` — the subject container's components, boundary-boxed.
    Component,
    /// `deployment <*|id> <env>` — the environment's deployment nodes.
    Deployment,
    /// `dynamic` — collaboration view (rendered as its element set).
    Dynamic,
    /// `filtered` / `custom` / `image` — rendered as their element set.
    Other,
}

/// The `autolayout` rank axis/direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Autolayout {
    /// `tb` — ranks stack downward (the C4 default).
    #[default]
    TopBottom,
    /// `bt` — ranks stack upward.
    BottomTop,
    /// `lr` — ranks flow left to right.
    LeftRight,
    /// `rl` — ranks flow right to left.
    RightLeft,
}

impl Autolayout {
    /// Whether ranks are laid out on the vertical axis (rows of cards).
    const fn is_vertical(self) -> bool {
        matches!(self, Self::TopBottom | Self::BottomTop)
    }
}

/// One `views { … }` entry: its [`ViewKind`], the `scope` element it is
/// about (the `<id>` argument, if any), `key`/`title`/`description`, the
/// `include`/`exclude` lists, the `autolayout` axis, and the deployment
/// `environment`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// What kind of view this is.
    pub kind: ViewKind,
    /// The subject element index (`systemContext`/`container`/`component`,
    /// and `deployment <id>`), if the argument resolved.
    pub scope: Option<usize>,
    /// The view key (explicit, else generated).
    pub key: String,
    /// The optional `title`.
    pub title: Option<String>,
    /// The optional `description`.
    pub description: Option<String>,
    /// Identifiers/`*` from `include` lines.
    pub include: Vec<String>,
    /// Identifiers from `exclude` lines.
    pub exclude: Vec<String>,
    /// The `autolayout` axis.
    pub autolayout: Autolayout,
    /// `deployment` environment name.
    pub environment: Option<String>,
    /// Whether this view is the `default`.
    pub is_default: bool,
}

/// A parsed Structurizr workspace: its `name`/`description`, the model
/// `elements` (in first-declaration order, with `parent` links) and
/// `relationships`, and the declared `views`.
///
/// Exposed as the public parse result so a host or test can assert structure
/// independently of layout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Workspace {
    /// The workspace name (the first `workspace` argument).
    pub name: Option<String>,
    /// The workspace description (the second `workspace` argument).
    pub description: Option<String>,
    /// Every element, first-declaration order.
    pub elements: Vec<Element>,
    /// Every relationship, source order.
    pub relationships: Vec<Relationship>,
    /// Every view, source order.
    pub views: Vec<View>,
}

impl Workspace {
    /// The index of the element whose id matches `id` (case-sensitive, the
    /// DSL's rule), falling back to a unique case-insensitive name match so a
    /// `source -> "Some Name"` style reference still resolves.
    fn resolve(&self, id: &str) -> Option<usize> {
        if let Some(i) = self.elements.iter().position(|e| e.id == id) {
            return Some(i);
        }
        let mut hit = None;
        for (i, e) in self.elements.iter().enumerate() {
            if e.name.eq_ignore_ascii_case(id) {
                if hit.is_some() {
                    return None;
                }
                hit = Some(i);
            }
        }
        hit
    }

    /// Direct children of element `idx`, in declaration order.
    fn children(&self, idx: usize) -> Vec<usize> {
        self.elements
            .iter()
            .enumerate()
            .filter(|(_, e)| e.parent == Some(idx))
            .map(|(i, _)| i)
            .collect()
    }
}

/// Why [`Structurizr::parse`] could not produce a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructurizrError {
    /// The first significant line was not a `workspace` declaration.
    MissingWorkspace,
    /// The workspace parsed but declared no model elements.
    EmptyModel,
}

impl std::fmt::Display for StructurizrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingWorkspace => f.write_str("expected a `workspace { … }` declaration"),
            Self::EmptyModel => f.write_str("no model elements were declared"),
        }
    }
}

impl std::error::Error for StructurizrError {}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

/// The styles [`Structurizr`] paints the C4 diagram pieces with.
///
/// Every field is a *patch* layered over the widget base style (itself over
/// the framing [`Block`] fill), the same cascade the rest of the catalog
/// uses — an unset colour falls through to the surrounding theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructurizrTheme {
    /// A `person` card's border + name.
    pub person: Style,
    /// A `softwareSystem` card's border + name.
    pub software_system: Style,
    /// A `container` card's border + name.
    pub container: Style,
    /// A `component` card's border + name.
    pub component: Style,
    /// An external / out-of-scope element (dimmed).
    pub external: Style,
    /// The `«stereotype»` and technology lines inside a card.
    pub stereotype: Style,
    /// A boundary box border + its title.
    pub boundary: Style,
    /// A relationship connector and its arrowhead.
    pub relationship: Style,
    /// A relationship's label text.
    pub relationship_label: Style,
    /// The workspace/view header band.
    pub header: Style,
    /// The placeholder shown when the source has no parseable workspace.
    pub placeholder: Style,
}

impl Default for StructurizrTheme {
    fn default() -> Self {
        Self {
            person: Style::new().fg(Color::Cyan),
            software_system: Style::new().fg(Color::Blue),
            container: Style::new().fg(Color::Green),
            component: Style::new().fg(Color::Magenta),
            external: Style::new().fg(Color::DarkGray),
            stereotype: Style::new().fg(Color::DarkGray),
            boundary: Style::new().fg(Color::Yellow),
            relationship: Style::new().fg(Color::DarkGray),
            relationship_label: Style::new().fg(Color::Yellow),
            header: Style::new().fg(Color::White),
            placeholder: Style::new().fg(Color::Red),
        }
    }
}

impl StructurizrTheme {
    /// The border/name style for an element of `kind`, dimmed when external.
    fn element_style(&self, kind: ElementKind, external: bool) -> Style {
        if external {
            return self.external;
        }
        match kind {
            ElementKind::Person => self.person,
            ElementKind::SoftwareSystem => self.software_system,
            ElementKind::Container | ElementKind::Instance => self.container,
            ElementKind::Component => self.component,
            ElementKind::Group | ElementKind::DeploymentNode | ElementKind::InfrastructureNode => {
                self.boundary
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// A read-only Structurizr DSL view: parses its source and draws the selected
/// C4 view as a deterministic terminal diagram.
///
/// The source is a [`Cow<str>`](std::borrow::Cow). An optional framing
/// [`Block`], a base [`Style`] (which also fills the content area), a
/// [`StructurizrTheme`], and an optional selected view index are the only
/// knobs — everything else is derived. Parsing is exposed via
/// [`Structurizr::parse`] so callers/tests can assert the workspace
/// independently of layout.
#[derive(Debug, Clone)]
pub struct Structurizr<'a> {
    source: Cow<'a, str>,
    block: Option<Block<'a>>,
    style: Style,
    theme: StructurizrTheme,
    view: Option<usize>,
}

impl<'a> Structurizr<'a> {
    /// A Structurizr view of `source` with the default theme, no block, the
    /// default view selected.
    pub fn new(source: impl Into<Cow<'a, str>>) -> Self {
        Self {
            source: source.into(),
            block: None,
            style: Style::new(),
            theme: StructurizrTheme::default(),
            view: None,
        }
    }

    /// Frames the diagram in `block`; content renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`] beneath the theme cascade; it also fills the
    /// content area.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Replaces the [`StructurizrTheme`].
    #[must_use]
    pub fn theme(mut self, theme: StructurizrTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Renders the view at `index` (clamped) instead of the default. Views
    /// are in [`Workspace::views`] order; the index wraps so a host can page
    /// without bounds-checking.
    #[must_use]
    pub fn view(mut self, index: usize) -> Self {
        self.view = Some(index);
        self
    }

    /// Parses `source` into a [`Workspace`] without rendering.
    ///
    /// Lenient: unparseable lines are skipped; the only errors are a missing
    /// `workspace` ([`StructurizrError::MissingWorkspace`]) or a workspace
    /// with no model elements ([`StructurizrError::EmptyModel`]).
    pub fn parse(source: impl AsRef<str>) -> Result<Workspace, StructurizrError> {
        parse_workspace(source.as_ref())
    }
}

impl Widget for Structurizr<'_> {
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

        match parse_workspace(self.source.as_ref()) {
            Ok(ws) => {
                let surface = lay_out(&ws, self.view, &self.theme);
                surface.blit(inner, buf, self.style);
            }
            Err(err) => {
                let msg = match err {
                    StructurizrError::MissingWorkspace => "[structurizr: missing workspace]",
                    StructurizrError::EmptyModel => "[structurizr: empty model]",
                };
                let style = self.style.patch(self.theme.placeholder);
                buf.set_str(Position::new(inner.x, inner.y), msg, style);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// Splits one already comment-stripped line into whitespace-separated tokens,
/// honouring `"quoted strings"` (with `\"`), per `structurizr-dsl`'s
/// `Tokenizer`. A trailing `{` / lone `}` survive as their own tokens.
fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut has = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '\\' && chars.peek() == Some(&'"') {
                cur.push('"');
                chars.next();
            } else if c == '"' {
                quoted = false;
                out.push(std::mem::take(&mut cur));
                has = false;
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            quoted = true;
            has = true;
        } else if c.is_whitespace() {
            if has {
                out.push(std::mem::take(&mut cur));
                has = false;
            }
        } else {
            cur.push(c);
            has = true;
        }
    }
    if has {
        out.push(cur);
    }
    out
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// What brace context the parser is currently inside.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ctx {
    /// Top, before/around `workspace`.
    Top,
    /// Inside `workspace { … }`.
    Workspace,
    /// Inside `model { … }`.
    Model,
    /// Inside an element `{ … }`; carries that element's index so implicit
    /// `-> dest` and `this` resolve and nested children get a `parent`.
    Element(usize),
    /// Inside `views { … }`.
    Views,
    /// Inside a single view `{ … }`; carries that view's index.
    View(usize),
    /// Inside any other `{ … }` we parse past (styles, configuration, …).
    Skip,
}

/// Parses `src` into a [`Workspace`]. Brace-context state machine, lenient:
/// an unrecognised line is skipped, never a panic.
fn parse_workspace(src: &str) -> Result<Workspace, StructurizrError> {
    // Phase 1: a quote/comment-aware scan flattening the source into logical
    // lines — declaration text with `//`/`#`/`/* … */` comments removed and
    // every *unquoted* `{` / `}` as its own line. Single-line blocks
    // (`model { a = person "A" }`), same-line braces, and Allman braces all
    // normalise to the same shape; a brace inside `"…"` is preserved.
    let mut logical: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut in_block = false;
    let mut chars = src.chars().peekable();
    fn flush(cur: &mut String, logical: &mut Vec<String>) {
        let t = cur.trim();
        if !t.is_empty() {
            logical.push(t.to_string());
        }
        cur.clear();
    }
    while let Some(c) = chars.next() {
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            }
            continue;
        }
        if in_quote {
            cur.push(c);
            if c == '\\' {
                if let Some(&n) = chars.peek() {
                    cur.push(n);
                    chars.next();
                }
            } else if c == '"' {
                in_quote = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_quote = true;
                cur.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                while chars.peek().is_some_and(|&n| n != '\n') {
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                in_block = true;
            }
            '#' => {
                while chars.peek().is_some_and(|&n| n != '\n') {
                    chars.next();
                }
            }
            '{' | '}' => {
                flush(&mut cur, &mut logical);
                logical.push(c.to_string());
            }
            '\n' => flush(&mut cur, &mut logical),
            _ => cur.push(c),
        }
    }
    flush(&mut cur, &mut logical);

    // Phase 2: walk the logical lines through the brace-context stack. `{`
    // opens the context the immediately-preceding declaration announced via
    // `pending`; `}` closes; any other line is a declaration/property
    // handled in the current context. A body-less declaration's `pending`
    // is harmlessly reset by the next line, so it never leaks a context.
    let mut ws = Workspace::default();
    let mut stack: Vec<Ctx> = vec![Ctx::Top];
    let mut pending: Option<Ctx> = None;
    let mut next_id = 0usize;
    let mut header_seen = false;

    for line in &logical {
        if line == "}" {
            if stack.len() > 1 {
                stack.pop();
            }
            pending = None;
            continue;
        }
        if line == "{" {
            stack.push(pending.take().unwrap_or(Ctx::Skip));
            continue;
        }
        let toks = tokenize(line);
        if toks.is_empty() {
            continue;
        }
        let ctx = stack.last().cloned().unwrap_or(Ctx::Top);
        pending = None;

        if !header_seen && toks[0] == "workspace" {
            header_seen = true;
            let args: Vec<&String> = toks[1..]
                .iter()
                .filter(|t| t.as_str() != "extends")
                .collect();
            if let Some(n) = args.first() {
                ws.name = Some((*n).clone());
            }
            if let Some(d) = args.get(1) {
                ws.description = Some((*d).clone());
            }
            pending = Some(Ctx::Workspace);
            continue;
        }
        match &ctx {
            Ctx::Workspace => {
                pending = Some(match toks[0].as_str() {
                    "model" => Ctx::Model,
                    "views" => Ctx::Views,
                    _ => Ctx::Skip,
                });
            }
            Ctx::Model | Ctx::Element(_) => {
                let parent = if let Ctx::Element(p) = ctx {
                    Some(p)
                } else {
                    None
                };
                parse_model_line(&toks, &mut ws, parent, &mut next_id, &mut pending);
            }
            Ctx::Views => parse_view_header(&toks, &mut ws, &mut pending),
            Ctx::View(vi) => {
                let vi = *vi;
                parse_view_body(&toks, &mut ws, vi, &mut pending);
            }
            Ctx::Top | Ctx::Skip => pending = Some(Ctx::Skip),
        }
    }

    if !header_seen {
        return Err(StructurizrError::MissingWorkspace);
    }
    if ws.elements.is_empty() {
        return Err(StructurizrError::EmptyModel);
    }
    Ok(ws)
}

/// The element-defining keyword → kind, with its `[description]` /
/// `[technology]` argument shape.
fn element_kind(kw: &str) -> Option<(ElementKind, bool)> {
    // bool = "has a [technology] arg before [tags]".
    Some(match kw {
        "person" => (ElementKind::Person, false),
        "softwareSystem" => (ElementKind::SoftwareSystem, false),
        "container" => (ElementKind::Container, true),
        "component" => (ElementKind::Component, true),
        "deploymentNode" => (ElementKind::DeploymentNode, true),
        "infrastructureNode" => (ElementKind::InfrastructureNode, true),
        "softwareSystemInstance" | "containerInstance" => (ElementKind::Instance, false),
        _ => return None,
    })
}

/// Parses one line inside `model { … }` or an element body: an element
/// declaration (optionally `id = `), a relationship, `group`,
/// `deploymentEnvironment`, or an in-block `description`/`technology`/`tags`.
fn parse_model_line(
    toks: &[String],
    ws: &mut Workspace,
    parent: Option<usize>,
    next_id: &mut usize,
    pending: &mut Option<Ctx>,
) {
    // `id = <keyword> …`
    let (id, rest): (Option<String>, &[String]) = if toks.len() >= 3 && toks[1] == "=" {
        (Some(toks[0].clone()), &toks[2..])
    } else {
        (None, toks)
    };
    if rest.is_empty() {
        return;
    }

    // Relationship: `src -> dst [desc] [tech] …` (explicit) or the implicit
    // `-> dst …` inside an element. `dst_at` is the destination token index
    // — right after the `->` arrow.
    if rest[0] == "->" || (rest.len() >= 2 && rest[1] == "->") {
        let (src_tok, dst_at) = if rest[0] == "->" {
            (None, 1usize)
        } else {
            (Some(rest[0].clone()), 2usize)
        };
        let src = match src_tok {
            Some(s) if s != "this" => ws.resolve(&s),
            _ => parent,
        };
        let Some(dst_id) = rest.get(dst_at) else {
            return;
        };
        // `this` (either endpoint) is the enclosing element.
        let dst = if dst_id == "this" {
            parent
        } else {
            ws.resolve(dst_id)
        };
        let Some(src) = src else { return };
        let Some(dst) = dst else { return };
        let description = rest.get(dst_at + 1).filter(|s| s.as_str() != "{").cloned();
        let technology = rest.get(dst_at + 2).filter(|s| s.as_str() != "{").cloned();
        ws.relationships.push(Relationship {
            source: src,
            destination: dst,
            description,
            technology,
        });
        // A relationship may carry a `{ … }` body (tags/url/…); skip it if a
        // brace opens.
        *pending = Some(Ctx::Skip);
        return;
    }

    // In-block property of the current element.
    if let Some(p) = parent {
        match rest[0].as_str() {
            "description" if rest.len() >= 2 => {
                ws.elements[p].description = Some(rest[1].clone());
                return;
            }
            "technology" if rest.len() >= 2 => {
                ws.elements[p].technology = Some(rest[1].clone());
                return;
            }
            "tags" if rest.len() >= 2 => {
                for t in rest[1..].iter() {
                    for t in t.split(',') {
                        let t = t.trim();
                        if !t.is_empty() {
                            ws.elements[p].tags.push(t.to_string());
                        }
                    }
                }
                return;
            }
            "url" | "properties" | "perspectives" | "!docs" | "!adrs" => {
                *pending = Some(Ctx::Skip);
                return;
            }
            _ => {}
        }
    }

    // `group "Name" {` / `deploymentEnvironment "Name" {`.
    if rest[0] == "group" || rest[0] == "deploymentEnvironment" {
        let kind = if rest[0] == "group" {
            ElementKind::Group
        } else {
            ElementKind::DeploymentNode
        };
        let name = rest.get(1).cloned().unwrap_or_else(|| rest[0].clone());
        let idx = push_element(ws, id, kind, name, parent, next_id);
        *pending = Some(Ctx::Element(idx));
        return;
    }

    // `<keyword> <name> [description] [technology] [tags]`.
    if let Some((kind, has_tech)) = element_kind(&rest[0]) {
        let name = rest.get(1).cloned().unwrap_or_else(|| rest[0].clone());
        let args: Vec<&String> = rest[2..].iter().filter(|t| t.as_str() != "{").collect();
        let description = args.first().map(|s| (*s).clone());
        let (technology, tags_at) = if has_tech {
            (args.get(1).map(|s| (*s).clone()), 2)
        } else {
            (None, 1)
        };
        let tags = args.get(tags_at).map(|t| split_tags(t)).unwrap_or_default();
        let idx = push_element(ws, id, kind, name, parent, next_id);
        ws.elements[idx].description = description;
        ws.elements[idx].technology = technology;
        ws.elements[idx].tags = tags;
        // The element may open a `{ … }` body of children/properties; the
        // caller pushes this only if a brace actually follows.
        *pending = Some(Ctx::Element(idx));
    }
}

/// Comma-splits a `tags` argument into trimmed, non-empty tags.
fn split_tags(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// Appends an [`Element`], registering its id (explicit or generated `e<N>`).
fn push_element(
    ws: &mut Workspace,
    id: Option<String>,
    kind: ElementKind,
    name: String,
    parent: Option<usize>,
    next_id: &mut usize,
) -> usize {
    let id = id.unwrap_or_else(|| {
        *next_id += 1;
        format!("e{next_id}")
    });
    ws.elements.push(Element {
        id,
        kind,
        name,
        description: None,
        technology: None,
        tags: Vec::new(),
        parent,
    });
    ws.elements.len() - 1
}

/// Parses a view-declaration line inside `views { … }`.
fn parse_view_header(toks: &[String], ws: &mut Workspace, pending: &mut Option<Ctx>) {
    let kw = toks[0].as_str();
    let (kind, takes_scope, takes_env) = match kw {
        "systemLandscape" => (ViewKind::SystemLandscape, false, false),
        "systemContext" => (ViewKind::SystemContext, true, false),
        "container" => (ViewKind::Container, true, false),
        "component" => (ViewKind::Component, true, false),
        "deployment" => (ViewKind::Deployment, true, true),
        "dynamic" => (ViewKind::Dynamic, true, false),
        "filtered" | "custom" | "image" => (ViewKind::Other, false, false),
        // `styles`, `theme`, `themes`, `terminology`, `properties`,
        // `branding`, `configuration`, … — parsed past (a brace block, if
        // any, is skipped).
        _ => {
            *pending = Some(Ctx::Skip);
            return;
        }
    };
    let args: Vec<&String> = toks[1..].iter().filter(|t| t.as_str() != "{").collect();
    let mut a = 0;
    let mut scope = None;
    let mut environment = None;
    if takes_scope {
        if let Some(s) = args.get(a) {
            if s.as_str() != "*" {
                scope = ws.resolve(s);
            }
            a += 1;
        }
    }
    if takes_env {
        environment = args.get(a).map(|s| (*s).clone());
        a += 1;
    }
    let key = args
        .get(a)
        .map(|s| (*s).clone())
        .unwrap_or_else(|| format!("{kw}-{}", ws.views.len() + 1));
    let description = args.get(a + 1).map(|s| (*s).clone());
    ws.views.push(View {
        kind,
        scope,
        key,
        title: None,
        description,
        include: Vec::new(),
        exclude: Vec::new(),
        autolayout: Autolayout::default(),
        environment,
        is_default: false,
    });
    let vi = ws.views.len() - 1;
    // A view always opens a `{ … }` body; the caller pushes it only if a
    // brace follows (a body-less view declaration is tolerated).
    *pending = Some(Ctx::View(vi));
}

/// Parses one line inside a view body.
fn parse_view_body(toks: &[String], ws: &mut Workspace, vi: usize, pending: &mut Option<Ctx>) {
    let v = &mut ws.views[vi];
    match toks[0].as_str() {
        "include" => v.include.extend(toks[1..].iter().cloned()),
        "exclude" => v.exclude.extend(toks[1..].iter().cloned()),
        "autolayout" | "autoLayout" => {
            v.autolayout = match toks.get(1).map(String::as_str) {
                Some("bt") => Autolayout::BottomTop,
                Some("lr") => Autolayout::LeftRight,
                Some("rl") => Autolayout::RightLeft,
                _ => Autolayout::TopBottom,
            };
        }
        "title" if toks.len() >= 2 => v.title = Some(toks[1].clone()),
        "description" if toks.len() >= 2 => v.description = Some(toks[1].clone()),
        "default" => v.is_default = true,
        // `animation { … }`, `properties { … }`, … inside a view — skipped.
        _ => *pending = Some(Ctx::Skip),
    }
}

// ---------------------------------------------------------------------------
// View scoping + layout
// ---------------------------------------------------------------------------

/// Resolves which element indices a view shows, honouring its scope kind and
/// any explicit `exclude` (an explicit `include <id>` only adds ids; `include
/// *` and the default are the kind's natural scope).
fn scoped_elements(ws: &Workspace, v: &View) -> Vec<usize> {
    let mut set: Vec<usize> = Vec::new();
    let add = |set: &mut Vec<usize>, i: usize| {
        if !set.contains(&i) {
            set.push(i);
        }
    };
    let related = |a: usize, b: usize| {
        ws.relationships
            .iter()
            .any(|r| (r.source == a && r.destination == b) || (r.source == b && r.destination == a))
    };
    match v.kind {
        ViewKind::SystemLandscape | ViewKind::Other | ViewKind::Dynamic => {
            for (i, e) in ws.elements.iter().enumerate() {
                if e.parent.is_none()
                    && matches!(e.kind, ElementKind::Person | ElementKind::SoftwareSystem)
                {
                    add(&mut set, i);
                }
            }
        }
        ViewKind::SystemContext => {
            if let Some(s) = v.scope {
                add(&mut set, s);
                for (i, e) in ws.elements.iter().enumerate() {
                    if i != s
                        && e.parent.is_none()
                        && matches!(e.kind, ElementKind::Person | ElementKind::SoftwareSystem)
                        && related(i, s)
                    {
                        add(&mut set, i);
                    }
                }
            }
        }
        ViewKind::Container => {
            if let Some(s) = v.scope {
                add(&mut set, s);
                for c in ws.children(s) {
                    add(&mut set, c);
                }
                let inside: Vec<usize> = set.clone();
                for (i, e) in ws.elements.iter().enumerate() {
                    if e.parent.is_none() && i != s && inside.iter().any(|&x| related(i, x)) {
                        add(&mut set, i);
                    }
                }
            }
        }
        ViewKind::Component => {
            if let Some(c) = v.scope {
                add(&mut set, c);
                for k in ws.children(c) {
                    add(&mut set, k);
                }
                let inside: Vec<usize> = set.clone();
                for (i, e) in ws.elements.iter().enumerate() {
                    if i != c && e.parent != Some(c) && inside.iter().any(|&x| related(i, x)) {
                        add(&mut set, i);
                    }
                }
            }
        }
        ViewKind::Deployment => {
            for (i, e) in ws.elements.iter().enumerate() {
                if matches!(
                    e.kind,
                    ElementKind::DeploymentNode
                        | ElementKind::InfrastructureNode
                        | ElementKind::Instance
                ) {
                    add(&mut set, i);
                }
            }
        }
    }
    // Explicit `include <id>` (ignoring `*`) adds elements.
    for inc in &v.include {
        if inc != "*" {
            if let Some(i) = ws.resolve(inc) {
                add(&mut set, i);
            }
        }
    }
    // `exclude <id>` removes them.
    for exc in &v.exclude {
        if let Some(i) = ws.resolve(exc) {
            set.retain(|&x| x != i);
        }
    }
    set
}

/// The boundary element (the subject system/container drawn as an enclosing
/// box) for a Container/Component view, if its children are in scope.
fn boundary_of(ws: &Workspace, v: &View, scope: &[usize]) -> Option<usize> {
    match v.kind {
        ViewKind::Container | ViewKind::Component => {
            let s = v.scope?;
            if ws.children(s).iter().any(|c| scope.contains(c)) {
                Some(s)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// A placed C4 card.
struct Card {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// Card inner text width (clamped) and the wrapped description lines.
fn card_lines(e: &Element) -> (Vec<String>, i32) {
    let mut lines = vec![e.kind.stereotype().to_string(), e.name.clone()];
    if let Some(t) = &e.technology {
        lines.push(format!("[{t}]"));
    }
    if let Some(d) = &e.description {
        lines.push(d.clone());
    }
    let w = lines
        .iter()
        .map(|l| l.chars().count() as i32)
        .max()
        .unwrap_or(4)
        .clamp(8, 26);
    (lines, w + 2)
}

/// Longest-path rank of each scoped element along the relationship DAG
/// (cycle-safe, like the flowchart ranker): a node with no in-scope incoming
/// edge seeds rank 0; relaxation is bounded by the node count.
fn rank(ws: &Workspace, scope: &[usize]) -> BTreeMap<usize, usize> {
    let pos: BTreeMap<usize, usize> = scope.iter().enumerate().map(|(i, &e)| (e, i)).collect();
    let mut rank: BTreeMap<usize, usize> = scope.iter().map(|&e| (e, 0usize)).collect();
    let edges: Vec<(usize, usize)> = ws
        .relationships
        .iter()
        .filter(|r| pos.contains_key(&r.source) && pos.contains_key(&r.destination))
        .map(|r| (r.source, r.destination))
        .collect();
    let any_root = scope.iter().any(|&e| !edges.iter().any(|&(_, d)| d == e));
    for _ in 0..scope.len() {
        let mut changed = false;
        for &(s, d) in &edges {
            if s == d {
                continue;
            }
            let cand = rank[&s] + 1;
            let dr = rank.get_mut(&d).unwrap();
            if cand > *dr && (any_root || pos[&d] != 0) {
                *dr = cand;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    rank
}

/// Lays the workspace's selected view out onto a [`Surface`].
fn lay_out(ws: &Workspace, view: Option<usize>, theme: &StructurizrTheme) -> Surface {
    if ws.views.is_empty() {
        // No explicit view: synthesize a System Landscape so a model-only
        // workspace still renders.
        let synth = View {
            kind: ViewKind::SystemLandscape,
            scope: None,
            key: "landscape".into(),
            title: None,
            description: None,
            include: vec!["*".into()],
            exclude: Vec::new(),
            autolayout: Autolayout::default(),
            environment: None,
            is_default: true,
        };
        return lay_out_view(ws, &synth, 0, 1, theme);
    }
    let default_idx = ws.views.iter().position(|v| v.is_default).unwrap_or(0);
    let n = ws.views.len();
    let idx = view.map(|i| i % n).unwrap_or(default_idx);
    lay_out_view(ws, &ws.views[idx], idx, n, theme)
}

/// Renders one view: header band, ranked C4 cards, optional boundary box, and
/// labelled relationship arrows.
fn lay_out_view(
    ws: &Workspace,
    v: &View,
    idx: usize,
    total: usize,
    theme: &StructurizrTheme,
) -> Surface {
    let scope = scoped_elements(ws, v);
    if scope.is_empty() {
        let msg = "[structurizr: view has no elements]";
        let mut s = Surface::new(msg.chars().count() as i32 + 2, 1);
        s.text(1, 0, msg, theme.placeholder);
        return s;
    }
    let boundary = boundary_of(ws, v, &scope);
    // Lay out everything except the boundary element itself (it becomes the
    // enclosing box); rank the rest.
    let laid: Vec<usize> = scope
        .iter()
        .copied()
        .filter(|&e| Some(e) != boundary)
        .collect();
    let ranks = rank(ws, &laid);
    let max_rank = ranks.values().copied().max().unwrap_or(0);

    // Group element indices by rank, in scope order (stable).
    let mut by_rank: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for &e in &laid {
        by_rank[ranks[&e]].push(e);
    }

    // Card sizes.
    let mut cards: BTreeMap<usize, Card> = BTreeMap::new();
    let mut card_w: BTreeMap<usize, i32> = BTreeMap::new();
    let mut card_h: BTreeMap<usize, i32> = BTreeMap::new();
    for &e in &laid {
        let (lines, w) = card_lines(&ws.elements[e]);
        card_w.insert(e, w);
        card_h.insert(e, lines.len() as i32 + 2);
    }

    let vertical = v.autolayout.is_vertical();
    let gap_major = 4; // between ranks (room for an arrow + label)
    let gap_minor = 3; // between cards within a rank
    let header_h = 3;
    // Boundary inset (the box drawn around child cards).
    let pad = if boundary.is_some() { 2 } else { 0 };

    // Place rank by rank.
    let mut max_x = 0;
    let mut max_y = 0;
    let mut cursor_major = header_h + 1 + pad;
    for row in &by_rank {
        if row.is_empty() {
            continue;
        }
        // Extent of this rank on the minor axis.
        let mut cursor_minor = pad + 1;
        let mut rank_extent_major = 0;
        for &e in row {
            let (cw, ch) = (card_w[&e], card_h[&e]);
            let (x, y, w, h) = if vertical {
                (cursor_minor, cursor_major, cw, ch)
            } else {
                (cursor_major, cursor_minor, cw, ch)
            };
            cards.insert(e, Card { x, y, w, h });
            if vertical {
                cursor_minor += cw + gap_minor;
                rank_extent_major = rank_extent_major.max(ch);
                max_x = max_x.max(x + w);
                max_y = max_y.max(y + h);
            } else {
                cursor_minor += ch + gap_minor;
                rank_extent_major = rank_extent_major.max(cw);
                max_x = max_x.max(x + w);
                max_y = max_y.max(y + h);
            }
        }
        cursor_major += rank_extent_major + gap_major;
    }

    let (hdr_left, hdr_pager) = header_parts(ws, v, idx, total);
    let header_w = hdr_left.chars().count() as i32
        + hdr_pager
            .as_ref()
            .map(|p| p.chars().count() as i32 + 2)
            .unwrap_or(0)
        + 3;
    let bx_extra = if boundary.is_some() { pad + 1 } else { 1 };
    let total_w = (max_x + bx_extra).max(header_w).max(20);
    let total_h = (max_y + bx_extra).max(header_h + 2);
    let mut s = Surface::new(total_w, total_h);

    // Header band: workspace + view, paged (the surface is sized so it fits).
    draw_header(
        &mut s,
        &hdr_left,
        hdr_pager.as_deref(),
        v.description.as_deref(),
        total_w,
        theme,
    );

    // Boundary box around the laid cards (Container/Component views).
    if let Some(b) = boundary {
        let bx0 = 0;
        let by0 = header_h + 1;
        let bw = total_w;
        let bh = total_h - by0;
        s.rect(bx0, by0, bw, bh, BoxStyle::Dashed, theme.boundary);
        let be = &ws.elements[b];
        let label = format!(" {} [{}] ", be.name, c4_label(be.kind));
        s.text_clipped(bx0 + 2, by0, &label, bw - 4, theme.boundary);
    }

    // Relationship arrows (drawn before cards so card borders win overlaps).
    for r in &ws.relationships {
        let (Some(a), Some(c)) = (cards.get(&r.source), cards.get(&r.destination)) else {
            continue;
        };
        draw_relationship(&mut s, a, c, r, vertical, theme);
    }

    // Cards.
    for (&e, card) in &cards {
        draw_card(&mut s, card, &ws.elements[e], boundary, theme);
    }

    s
}

/// The short bracket label for a boundary element.
fn c4_label(kind: ElementKind) -> &'static str {
    match kind {
        ElementKind::SoftwareSystem => "Software System",
        ElementKind::Container => "Container",
        ElementKind::DeploymentNode => "Deployment Node",
        _ => "Boundary",
    }
}

/// Draws the two-line header band: `Workspace — ViewKind: key  ‹ k/n ›` and
/// an underline; the view title/description when present.
/// The header label for a view kind.
fn view_kind_label(kind: ViewKind) -> &'static str {
    match kind {
        ViewKind::SystemLandscape => "System Landscape",
        ViewKind::SystemContext => "System Context",
        ViewKind::Container => "Container",
        ViewKind::Component => "Component",
        ViewKind::Deployment => "Deployment",
        ViewKind::Dynamic => "Dynamic",
        ViewKind::Other => "View",
    }
}

/// The header's left line (`Workspace — Kind: title`) and the optional
/// `‹ k/n ›` pager — also used to size the surface so neither is truncated.
fn header_parts(ws: &Workspace, v: &View, idx: usize, total: usize) -> (String, Option<String>) {
    let name = ws.name.as_deref().unwrap_or("Workspace");
    let titled = v.title.as_deref().unwrap_or(&v.key);
    let left = format!("{name} — {}: {titled}", view_kind_label(v.kind));
    let pager = (total > 1).then(|| format!("‹ {}/{} ›", idx + 1, total));
    (left, pager)
}

/// Draws the header band: the (clipped) left line, a right-aligned pager when
/// there are several views, an optional description row, and an underline.
fn draw_header(
    s: &mut Surface,
    left: &str,
    pager: Option<&str>,
    desc: Option<&str>,
    width: i32,
    theme: &StructurizrTheme,
) {
    let pager_w = pager.map(|p| p.chars().count() as i32 + 1).unwrap_or(0);
    s.text_clipped(1, 0, left, width - 2 - pager_w, theme.header);
    if let Some(p) = pager {
        let px = (width - p.chars().count() as i32 - 1).max(0);
        s.text(px, 0, p, theme.header);
    }
    if let Some(d) = desc {
        s.text_clipped(1, 1, d, width - 2, theme.stereotype);
    }
    s.hline(0, 2, width, '─', theme.header);
}

/// Draws one C4 card: bordered, with the `«stereotype»`, name, optional
/// `[technology]`, and a clipped description. Person cards get rounded
/// corners; externals are dimmed via the theme.
fn draw_card(
    s: &mut Surface,
    c: &Card,
    e: &Element,
    boundary: Option<usize>,
    theme: &StructurizrTheme,
) {
    let external = e.is_external()
        || (boundary.is_some() && e.parent != boundary && !e.kind.is_container_like());
    let border = theme.element_style(e.kind, external);
    let (lines, _) = card_lines(e);
    // Fill the interior so a bg covers the card.
    for yy in (c.y + 1)..(c.y + c.h - 1) {
        for xx in (c.x + 1)..(c.x + c.w - 1) {
            s.set(xx, yy, ' ', border);
        }
    }
    s.rect(c.x, c.y, c.w, c.h, BoxStyle::Square, border);
    if e.kind == ElementKind::Person {
        // Evoke the C4 "person" shape: round the top corners.
        s.set(c.x, c.y, '╭', border);
        s.set(c.x + c.w - 1, c.y, '╮', border);
    }
    let iw = c.w - 2;
    for (i, ln) in lines.iter().enumerate() {
        let yy = c.y + 1 + i as i32;
        if yy >= c.y + c.h - 1 {
            break;
        }
        // Row 0 is the «stereotype», row 1 the (themed/coloured) name, every
        // following row (technology, description) the dim secondary style.
        let st = match i {
            1 => border,
            _ => theme.stereotype,
        };
        s.text_centered(c.x + 1, yy, iw, ln, st);
    }
}

/// Draws a relationship arrow between two cards along the rank axis, with the
/// description (+ `[technology]`) labelled mid-connector. Straight when the
/// cards line up on the major axis; an L-jog otherwise — deterministic and
/// clipped, not a full router.
fn draw_relationship(
    s: &mut Surface,
    a: &Card,
    b: &Card,
    r: &Relationship,
    vertical: bool,
    theme: &StructurizrTheme,
) {
    let st = theme.relationship;
    let label = match (&r.description, &r.technology) {
        (Some(d), Some(t)) => Some(format!("{d} [{t}]")),
        (Some(d), None) => Some(d.clone()),
        (None, Some(t)) => Some(format!("[{t}]")),
        (None, None) => None,
    };
    if vertical {
        let (top, bot) = if a.y <= b.y { (a, b) } else { (b, a) };
        let sx = top.x + top.w / 2;
        let ex = bot.x + bot.w / 2;
        let y0 = top.y + top.h;
        let y1 = bot.y - 1;
        if y1 < y0 {
            return;
        }
        let my = (y0 + y1) / 2;
        s.vline(sx, y0, (my - y0).max(0), '│', st);
        // Horizontal jog at the midpoint if columns differ.
        let (lo, hi) = (sx.min(ex), sx.max(ex));
        s.hline(lo, my, hi - lo + 1, '─', st);
        s.vline(ex, my, (y1 - my).max(0) + 1, '│', st);
        let head = if a.y <= b.y { '▼' } else { '▲' };
        s.set(ex, y1 + 1 - 1, head, st);
        s.set(ex, bot.y - 1, head, st);
        if let Some(l) = label {
            let lx = (sx.min(ex)) + 1;
            s.text_clipped(
                lx,
                my.saturating_sub(0).max(y0),
                &l,
                (hi - lo).max(8) + 8,
                theme.relationship_label,
            );
        }
    } else {
        let (lft, rgt) = if a.x <= b.x { (a, b) } else { (b, a) };
        let sy = lft.y + lft.h / 2;
        let ey = rgt.y + rgt.h / 2;
        let x0 = lft.x + lft.w;
        let x1 = rgt.x - 1;
        if x1 < x0 {
            return;
        }
        let mx = (x0 + x1) / 2;
        s.hline(x0, sy, (mx - x0).max(0), '─', st);
        let (lo, hi) = (sy.min(ey), sy.max(ey));
        s.vline(mx, lo, hi - lo + 1, '│', st);
        s.hline(mx, ey, (x1 - mx).max(0) + 1, '─', st);
        let head = if a.x <= b.x { '▶' } else { '◀' };
        s.set(x1, ey, head, st);
        if let Some(l) = label {
            s.text_clipped(
                x0 + 1,
                sy.saturating_sub(1),
                &l,
                (x1 - x0).max(6),
                theme.relationship_label,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render `widget` into a `w`×`h` buffer; return glyph rows joined by
    /// `\n` (one trailing `\n`) — the same snapshot helper the other widgets
    /// use.
    fn lines(widget: Structurizr<'_>, w: u16, h: u16) -> String {
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

    const BIGBANK: &str = "workspace \"Big Bank\" \"An example.\" {
  model {
    customer = person \"Customer\" \"A bank customer.\"
    bank = softwareSystem \"Internet Banking\" \"Lets customers bank.\" {
      web = container \"Web App\" \"Delivers the SPA.\" \"Java\"
      api = container \"API\" \"Banking logic.\" \"Java\"
      db = container \"Database\" \"Stores accounts.\" \"Oracle\"
    }
    mainframe = softwareSystem \"Mainframe\" \"Core banking.\" \"Existing System,External\"
    customer -> bank \"Uses\"
    web -> api \"Calls\" \"JSON/HTTPS\"
    api -> db \"Reads/writes\" \"JDBC\"
    api -> mainframe \"Uses\" \"XML/HTTPS\"
  }
  views {
    systemContext bank \"Context\" {
      include *
      autolayout tb
    }
    container bank \"Containers\" {
      include *
      autolayout tb
    }
  }
}";

    // --- parser ------------------------------------------------------------

    #[test]
    fn parses_workspace_name_description_and_counts() {
        let ws = Structurizr::parse(BIGBANK).unwrap();
        assert_eq!(ws.name.as_deref(), Some("Big Bank"));
        assert_eq!(ws.description.as_deref(), Some("An example."));
        assert_eq!(ws.elements.len(), 6);
        assert_eq!(ws.relationships.len(), 4);
        assert_eq!(ws.views.len(), 2);
    }

    #[test]
    fn assigns_ids_kinds_parents_and_technology() {
        let ws = Structurizr::parse(BIGBANK).unwrap();
        let web = ws.elements.iter().find(|e| e.id == "web").unwrap();
        assert_eq!(web.kind, ElementKind::Container);
        assert_eq!(web.technology.as_deref(), Some("Java"));
        let bank = ws.resolve("bank").unwrap();
        assert_eq!(web.parent, Some(bank));
        let mf = ws.elements.iter().find(|e| e.id == "mainframe").unwrap();
        assert!(mf.is_external());
    }

    #[test]
    fn resolves_relationships_by_id() {
        let ws = Structurizr::parse(BIGBANK).unwrap();
        let web = ws.resolve("web").unwrap();
        let api = ws.resolve("api").unwrap();
        let r = ws
            .relationships
            .iter()
            .find(|r| r.source == web && r.destination == api)
            .unwrap();
        assert_eq!(r.description.as_deref(), Some("Calls"));
        assert_eq!(r.technology.as_deref(), Some("JSON/HTTPS"));
    }

    #[test]
    fn implicit_relationship_and_this_inside_element() {
        let src = "workspace {
  model {
    u = person \"User\"
    s = softwareSystem \"S\" {
      web = container \"Web\" {
        u -> this \"Uses\"
      }
    }
  }
}";
        let ws = Structurizr::parse(src).unwrap();
        let u = ws.resolve("u").unwrap();
        let web = ws.resolve("web").unwrap();
        assert!(ws.relationships.iter().any(|r| r.source == u
            && r.destination == web
            && r.description.as_deref() == Some("Uses")));
    }

    #[test]
    fn comments_block_line_and_hash_are_stripped() {
        let src = "// header\nworkspace {\n  # a hash comment\n  model {\n    /* block */ a = person \"A\"\n    b = softwareSystem \"B\" /* trailing */\n  }\n}";
        let ws = Structurizr::parse(src).unwrap();
        assert_eq!(ws.elements.len(), 2);
        assert_eq!(ws.elements[0].name, "A");
    }

    #[test]
    fn quoted_escapes_and_in_block_properties() {
        let src = "workspace {
  model {
    a = softwareSystem \"A\" {
      description \"Says \\\"hi\\\"\"
      technology \"Rust\"
      tags \"External, Legacy\"
    }
  }
}";
        let ws = Structurizr::parse(src).unwrap();
        let a = &ws.elements[0];
        assert_eq!(a.description.as_deref(), Some("Says \"hi\""));
        assert_eq!(a.technology.as_deref(), Some("Rust"));
        assert!(a.tags.iter().any(|t| t == "External"));
        assert!(a.tags.iter().any(|t| t == "Legacy"));
    }

    #[test]
    fn missing_workspace_and_empty_model_error() {
        assert_eq!(
            Structurizr::parse("not a workspace"),
            Err(StructurizrError::MissingWorkspace)
        );
        assert_eq!(
            Structurizr::parse("workspace {\n model {\n }\n}"),
            Err(StructurizrError::EmptyModel)
        );
    }

    #[test]
    fn views_parse_scope_autolayout_default_and_key() {
        let src = "workspace {
  model { s = softwareSystem \"S\" \"d\" }
  views {
    systemContext s \"Ctx\" \"the ctx\" {
      include *
      autolayout lr
      default
    }
  }
}";
        let ws = Structurizr::parse(src).unwrap();
        let v = &ws.views[0];
        assert_eq!(v.kind, ViewKind::SystemContext);
        assert_eq!(v.scope, ws.resolve("s"));
        assert_eq!(v.key, "Ctx");
        assert_eq!(v.description.as_deref(), Some("the ctx"));
        assert_eq!(v.autolayout, Autolayout::LeftRight);
        assert!(v.is_default);
    }

    // --- scoping -----------------------------------------------------------

    #[test]
    fn system_context_scope_is_subject_plus_neighbours() {
        let ws = Structurizr::parse(BIGBANK).unwrap();
        let v = ws
            .views
            .iter()
            .find(|v| v.kind == ViewKind::SystemContext)
            .unwrap();
        let scope = scoped_elements(&ws, v);
        let names: Vec<&str> = scope
            .iter()
            .map(|&i| ws.elements[i].name.as_str())
            .collect();
        assert!(names.contains(&"Internet Banking"));
        assert!(names.contains(&"Customer"));
        // Containers are NOT in a context view.
        assert!(!names.contains(&"Web App"));
    }

    #[test]
    fn container_view_boundaries_the_subject_and_shows_children() {
        let ws = Structurizr::parse(BIGBANK).unwrap();
        let v = ws
            .views
            .iter()
            .find(|v| v.kind == ViewKind::Container)
            .unwrap();
        let scope = scoped_elements(&ws, v);
        let b = boundary_of(&ws, v, &scope);
        assert_eq!(b, ws.resolve("bank"));
        let names: Vec<&str> = scope
            .iter()
            .map(|&i| ws.elements[i].name.as_str())
            .collect();
        assert!(
            names.contains(&"Web App") && names.contains(&"API") && names.contains(&"Database")
        );
    }

    // --- render snapshots --------------------------------------------------

    #[test]
    fn missing_and_empty_render_placeholders() {
        assert!(
            lines(Structurizr::new("nope"), 34, 1).contains("[structurizr: missing workspace]")
        );
        assert!(
            lines(Structurizr::new("workspace {\n model {}\n}"), 30, 1)
                .contains("[structurizr: empty model]")
        );
    }

    #[test]
    fn tiny_and_zero_area_never_panic() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        Structurizr::new(BIGBANK).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        // 2×2 must not panic.
        let _ = lines(Structurizr::new(BIGBANK), 2, 2);
    }

    #[test]
    fn system_context_full_render_is_stable_and_c4() {
        let out = lines(Structurizr::new(BIGBANK), 60, 22);
        // Header names workspace + view kind.
        assert!(out.contains("Big Bank — System Context"), "header:\n{out}");
        // Pager (two views).
        assert!(out.contains("1/2"), "pager:\n{out}");
        // C4 stereotypes + the subject + an external neighbour.
        assert!(out.contains("«Software System»"), "stereotype:\n{out}");
        assert!(out.contains("Internet Banking"), "subject:\n{out}");
        assert!(out.contains("Customer"), "person:\n{out}");
        assert!(out.contains("Uses"), "relationship label:\n{out}");
        // Deterministic: identical on a re-render.
        assert_eq!(out, lines(Structurizr::new(BIGBANK), 60, 22));
    }

    #[test]
    fn container_view_draws_a_dashed_boundary_with_children() {
        // Tall enough that every ranked container (the deepest, `Database`,
        // is several ranks down) is on-surface rather than clipped.
        let out = lines(Structurizr::new(BIGBANK).view(1), 64, 44);
        assert!(out.contains("Big Bank — Container"), "header:\n{out}");
        assert!(out.contains("[Software System]"), "boundary label:\n{out}");
        assert!(out.contains('╌'), "dashed boundary:\n{out}");
        assert!(out.contains("«Container»"), "container stereotype:\n{out}");
        assert!(
            out.contains("Web App") && out.contains("Database"),
            "children:\n{out}"
        );
        assert!(out.contains("[Java]"), "technology line:\n{out}");
    }

    #[test]
    fn view_index_wraps_and_selects() {
        // 2 views; index 3 wraps to view 1 (Container).
        let out = lines(Structurizr::new(BIGBANK).view(3), 64, 24);
        assert!(
            out.contains("— Container:"),
            "wrapped to container view:\n{out}"
        );
    }

    #[test]
    fn model_only_workspace_synthesizes_a_landscape() {
        let src = "workspace {
  model {
    u = person \"User\"
    s = softwareSystem \"System\"
    u -> s \"Uses\"
  }
}";
        let out = lines(Structurizr::new(src), 40, 14);
        assert!(out.contains("System Landscape"), "synth landscape:\n{out}");
        assert!(
            out.contains("«Person»") && out.contains("User"),
            "person:\n{out}"
        );
        assert!(
            out.contains("«Software System»") && out.contains("System"),
            "system:\n{out}"
        );
    }

    #[test]
    fn block_frames_the_diagram() {
        let out = lines(Structurizr::new(BIGBANK).block(Block::bordered()), 40, 12);
        assert!(out.starts_with('┌'), "block frame:\n{out}");
        assert!(out.contains("Big Bank"), "content inside block:\n{out}");
    }
}
