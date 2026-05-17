//! C4 model Mermaid diagram renderer (`C4Context` / `C4Container` /
//! `C4Component` / `C4Dynamic` / `C4Deployment`).
//!
//! The C4 family describes software architecture at four zoom levels with a
//! shared grammar: *elements* (`Person`, `System`, `Container`, `Component`,
//! `Node`, …, each optionally an `_Ext` external or a `Db` datastore variant),
//! *boundaries* (`Enterprise_Boundary` / `System_Boundary` /
//! `Container_Boundary`, brace-delimited and nestable), and *relations*
//! (`Rel`, `BiRel`, and the directional `Rel_U/D/L/R`).
//!
//! # Terminal layout approximation
//!
//! Mermaid's web renderer auto-lays the graph; a terminal cannot run that
//! solver, so this draws a deterministic, source-ordered approximation that
//! preserves every element, its stereotype, and every relation:
//!
//! - Boundaries become titled dashed bordered regions; nesting is honoured by
//!   indenting an inner boundary inside its parent.
//! - Each element is a box: a `«stereotype»` line (`«Person»`,
//!   `«System»`, `«Container: tech»`, …), the bold name, then the wrapped
//!   description.
//! - Elements inside a boundary pack into a deterministic integer grid;
//!   top-level elements (no enclosing boundary) pack the same way below the
//!   boundaries.
//! - Relations are listed beneath as labelled arrows — `from ──▸ to  «label»`
//!   (a `BiRel` is double-headed `◂──▸`). This is the honest terminal
//!   stand-in for the web router's routed connectors; every relation is still
//!   shown, deterministically, in source order.
//!
//! Parsing is lenient: a malformed line is skipped, never fatal; a source with
//! no parseable element or relation falls through to
//! [`super::diagram_placeholder`].

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// One C4 element (a person/system/container/component/node box).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Element {
    /// The identifier referenced by relations.
    id: String,
    /// The `«stereotype»` shown on the box's first line.
    stereotype: String,
    /// The display name (the first quoted argument).
    name: String,
    /// The wrapped description (the last quoted argument), if any.
    desc: Option<String>,
    /// The enclosing boundary index in [`Model::boundaries`], if any.
    boundary: Option<usize>,
}

/// One C4 boundary (`Enterprise_Boundary` / `System_Boundary` /
/// `Container_Boundary`), possibly nested.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Boundary {
    /// The boundary identifier.
    id: String,
    /// The display title (the quoted name argument), falling back to the id.
    title: String,
    /// The C4 kind word (`Enterprise` / `System` / `Container`).
    kind: String,
    /// The parent boundary index, if this boundary is nested.
    parent: Option<usize>,
}

/// One C4 relation (`Rel` / `BiRel` / directional `Rel_U/D/L/R`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Relation {
    /// The source element id.
    from: String,
    /// The destination element id.
    to: String,
    /// The relation label (first quoted argument), if any.
    label: Option<String>,
    /// The technology annotation (second quoted argument), if any.
    tech: Option<String>,
    /// Whether the relation is bidirectional (`BiRel`).
    bi: bool,
}

/// The whole parsed C4 diagram in source order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Model {
    /// Every boundary, in source order.
    boundaries: Vec<Boundary>,
    /// Every element, in source order.
    elements: Vec<Element>,
    /// Every relation, in source order.
    relations: Vec<Relation>,
}

impl Model {
    /// Whether nothing parseable was found (drives the placeholder fallback).
    fn is_empty(&self) -> bool {
        self.elements.is_empty() && self.relations.is_empty() && self.boundaries.is_empty()
    }
}

/// Splits a C4 argument list on top-level commas, honouring `"…"` quotes and
/// `(`/`)` nesting so a comma inside a quoted label or a nested call does not
/// split. Surrounding quotes/whitespace are trimmed from each argument.
fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_q = false;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '"' => {
                in_q = !in_q;
                cur.push(ch);
            }
            '(' if !in_q => {
                depth += 1;
                cur.push(ch);
            }
            ')' if !in_q => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if !in_q && depth == 0 => {
                out.push(trim_arg(&cur));
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(trim_arg(&cur));
    }
    out
}

/// Trims whitespace then a single pair of surrounding double quotes.
fn trim_arg(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_prefix('"').unwrap_or(t);
    let t = t.strip_suffix('"').unwrap_or(t);
    t.trim().to_string()
}

/// Returns the `(keyword, args_inside_parens)` of a `Keyword(arg, …)` call, or
/// `None` if the line is not a call. The args slice excludes the outer parens.
fn split_call(line: &str) -> Option<(&str, &str)> {
    let open = line.find('(')?;
    let kw = line[..open].trim();
    if kw.is_empty() {
        return None;
    }
    // Last ')' so a trailing `{` or comment after the call is tolerated.
    let close = line.rfind(')')?;
    if close < open {
        return None;
    }
    Some((kw, &line[open + 1..close]))
}

/// Maps an element keyword to its C4 stereotype, given the parsed `tech`
/// (used by `Container`/`Component` to render `«Container: tech»`).
fn stereotype_for(kw: &str, tech: Option<&str>) -> String {
    let base = match kw {
        "Person" => "Person",
        "Person_Ext" => "Person, external",
        "System" => "System",
        "System_Ext" => "System, external",
        "SystemDb" => "System, database",
        "SystemDb_Ext" => "System, external database",
        "SystemQueue" => "System, queue",
        "Container" => "Container",
        "Container_Ext" => "Container, external",
        "ContainerDb" => "Container, database",
        "ContainerDb_Ext" => "Container, external database",
        "ContainerQueue" => "Container, queue",
        "Component" => "Component",
        "Component_Ext" => "Component, external",
        "ComponentDb" => "Component, database",
        "ComponentQueue" => "Component, queue",
        "Node" | "Node_L" | "Node_R" => "Node",
        "Deployment_Node" => "Deployment Node",
        _ => kw,
    };
    match tech {
        Some(t) if !t.is_empty() => format!("{base}: {t}"),
        _ => base.to_string(),
    }
}

/// Whether `kw` names a C4 element-declaring call.
fn is_element_kw(kw: &str) -> bool {
    matches!(
        kw,
        "Person"
            | "Person_Ext"
            | "System"
            | "System_Ext"
            | "SystemDb"
            | "SystemDb_Ext"
            | "SystemQueue"
            | "Container"
            | "Container_Ext"
            | "ContainerDb"
            | "ContainerDb_Ext"
            | "ContainerQueue"
            | "Component"
            | "Component_Ext"
            | "ComponentDb"
            | "ComponentQueue"
            | "Node"
            | "Node_L"
            | "Node_R"
            | "Deployment_Node"
    )
}

/// Whether `kw` names a relation call; returns `Some(bi)` with `bi` true for
/// `BiRel`.
fn rel_kind(kw: &str) -> Option<bool> {
    match kw {
        "Rel" | "Rel_U" | "Rel_D" | "Rel_L" | "Rel_R" | "Rel_Up" | "Rel_Down" | "Rel_Left"
        | "Rel_Right" | "Rel_Back" => Some(false),
        "BiRel" | "BiRel_U" | "BiRel_D" | "BiRel_L" | "BiRel_R" => Some(true),
        _ => None,
    }
}

/// Parses one already-trimmed, comment-free statement into `model`, tracking
/// the open-boundary stack so element membership and nesting are recorded.
fn parse_statement(line: &str, model: &mut Model, stack: &mut Vec<usize>) {
    // A boundary's closing brace (possibly on its own line).
    if line == "}" {
        stack.pop();
        return;
    }
    let Some((kw, args_str)) = split_call(line) else {
        return;
    };
    let args = split_args(args_str);

    if kw.ends_with("_Boundary") || kw == "Boundary" {
        // `Enterprise_Boundary(id, "name"[, type]) {`
        let id = args.first().cloned().unwrap_or_default();
        if id.is_empty() {
            return;
        }
        let title = args
            .get(1)
            .cloned()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| id.clone());
        let kind = kw.strip_suffix("_Boundary").unwrap_or("").to_string();
        let idx = model.boundaries.len();
        model.boundaries.push(Boundary {
            id,
            title,
            kind,
            parent: stack.last().copied(),
        });
        // The opening brace may be on this line or follow; either way the
        // boundary is now open until a lone `}`.
        stack.push(idx);
        return;
    }

    if is_element_kw(kw) {
        let id = args.first().cloned().unwrap_or_default();
        if id.is_empty() {
            return;
        }
        let name = args
            .get(1)
            .cloned()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| id.clone());
        // `Container(id,"L","tech","d")` has tech at index 2 and desc at 3;
        // `System(id,"L","d")` has desc at 2 and no tech. Detect by keyword.
        let has_tech = matches!(
            kw,
            "Container"
                | "Container_Ext"
                | "ContainerDb"
                | "ContainerDb_Ext"
                | "ContainerQueue"
                | "Component"
                | "Component_Ext"
                | "ComponentDb"
                | "ComponentQueue"
                | "Deployment_Node"
                | "Node"
                | "Node_L"
                | "Node_R"
        );
        let (tech, desc) = if has_tech {
            (
                args.get(2).cloned().filter(|t| !t.is_empty()),
                args.get(3).cloned().filter(|d| !d.is_empty()),
            )
        } else {
            (None, args.get(2).cloned().filter(|d| !d.is_empty()))
        };
        let stereotype = stereotype_for(kw, tech.as_deref());
        model.elements.push(Element {
            id,
            stereotype,
            name,
            desc,
            boundary: stack.last().copied(),
        });
        return;
    }

    if let Some(bi) = rel_kind(kw) {
        // `Rel(from, to, "label", "tech")`
        let from = args.first().cloned().unwrap_or_default();
        let to = args.get(1).cloned().unwrap_or_default();
        if from.is_empty() || to.is_empty() {
            return;
        }
        model.relations.push(Relation {
            from,
            to,
            label: args.get(2).cloned().filter(|l| !l.is_empty()),
            tech: args.get(3).cloned().filter(|t| !t.is_empty()),
            bi,
        });
    }
}

/// Parses a C4 source: split lines, strip `\r`, drop a leading `--- … ---`
/// frontmatter block and `%%` comment/directive lines, skip the header, then
/// feed every remaining trimmed line to [`parse_statement`]. A trailing `{`
/// after a boundary call is tolerated (the brace stack only pops on a lone
/// `}`). Lenient — a bad line is skipped, never fatal.
fn parse(src: &str) -> Model {
    let mut model = Model::default();
    let mut stack: Vec<usize> = Vec::new();
    let mut in_front = false;
    let mut header_seen = false;
    for raw in src.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw).trim();
        if line.is_empty() {
            continue;
        }
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
            // The first significant line is the `C4*` header.
            header_seen = true;
            continue;
        }
        // A `}` may share a line with a following statement; handle the common
        // standalone forms by trimming a trailing `{` off boundary calls.
        let stmt = line.strip_suffix('{').map_or(line, str::trim_end);
        parse_statement(stmt, &mut model, &mut stack);
    }
    model
}

/// Wraps `text` into lines of at most `width` chars on whitespace boundaries
/// (a single over-long word is hard-split). Deterministic, no alloc/locale
/// surprises.
fn wrap(text: &str, width: i32) -> Vec<String> {
    if width <= 0 {
        return Vec::new();
    }
    let w = width as usize;
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if word.chars().count() > w {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            let mut chunk = String::new();
            for ch in word.chars() {
                if chunk.chars().count() == w {
                    out.push(std::mem::take(&mut chunk));
                }
                chunk.push(ch);
            }
            if !chunk.is_empty() {
                cur = chunk;
            }
            continue;
        }
        let add = if cur.is_empty() {
            word.chars().count()
        } else {
            cur.chars().count() + 1 + word.chars().count()
        };
        if add > w {
            out.push(std::mem::take(&mut cur));
            cur.push_str(word);
        } else {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The fixed cell size of an element box and the inter-box gutter.
const EL_W: i32 = 22;
const EL_H: i32 = 6;
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

/// Draws one element box at `(x, y)` into `s`.
fn draw_element(s: &mut Surface, x: i32, y: i32, el: &Element, theme: &MermaidTheme, base: Style) {
    let border = base.patch(theme.node_border);
    let text = base.patch(theme.node_label);
    let stereo = base.patch(theme.edge_label);
    s.rect(x, y, EL_W, EL_H, BoxStyle::Square, border);
    let iw = EL_W - 2;
    s.text_centered(x + 1, y + 1, iw, &format!("«{}»", el.stereotype), stereo);
    s.text_centered(x + 1, y + 2, iw, &el.name, text);
    if let Some(d) = &el.desc {
        for (i, l) in wrap(d, iw).into_iter().take(2).enumerate() {
            s.text_centered(x + 1, y + 3 + i as i32, iw, &l, text);
        }
    }
}

/// Renders a C4 Mermaid diagram from `src` into `area`.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let model = parse(src);
    if model.is_empty() {
        super::diagram_placeholder("C4", "no elements", area, buf, base, theme);
        return;
    }

    // Each boundary becomes one stacked, full-width region; top-level
    // elements (no boundary) get an untitled region below. Nested boundaries
    // are listed in source order with an indent marker in the title.
    let in_boundary = |bi: usize| -> Vec<&Element> {
        model
            .elements
            .iter()
            .filter(|e| e.boundary == Some(bi))
            .collect()
    };
    let top: Vec<&Element> = model
        .elements
        .iter()
        .filter(|e| e.boundary.is_none())
        .collect();

    let mut max_cols = 1;
    for bi in 0..model.boundaries.len() {
        max_cols = max_cols.max(grid_cols(in_boundary(bi).len()));
    }
    if !top.is_empty() {
        max_cols = max_cols.max(grid_cols(top.len()));
    }
    let inner_w = max_cols * (EL_W + GUT) - GUT;
    let region_w = inner_w + 4;

    let mut total_h = 1;
    for bi in 0..model.boundaries.len() {
        let n = in_boundary(bi).len();
        let cols = grid_cols(n);
        let rows = div_ceil_i32(n as i32, cols).max(1);
        total_h += rows * (EL_H + GUT) - GUT + 3;
    }
    if !top.is_empty() {
        let cols = grid_cols(top.len());
        let rows = div_ceil_i32(top.len() as i32, cols).max(1);
        total_h += rows * (EL_H + GUT) - GUT + 3;
    }

    let mut surf_w = region_w;
    if !model.relations.is_empty() {
        total_h += model.relations.len() as i32 + 2;
        for r in &model.relations {
            let arrow = if r.bi { "◂──▸" } else { "──▸" };
            let mut l = format!("{} {arrow} {}", r.from, r.to);
            if let Some(lbl) = &r.label {
                l.push_str(&format!("  «{lbl}»"));
            }
            surf_w = surf_w.max(l.chars().count() as i32 + 2);
        }
    }

    let mut s = Surface::new(surf_w, total_h.max(1));
    let cluster = base.patch(theme.cluster);
    let edge_st = base.patch(theme.edge);
    let edge_lbl = base.patch(theme.edge_label);

    let mut y = 0;
    for (bi, b) in model.boundaries.iter().enumerate() {
        let els = in_boundary(bi);
        let cols = grid_cols(els.len());
        let rows = div_ceil_i32(els.len() as i32, cols).max(1);
        let inner_h = rows * (EL_H + GUT) - GUT;
        let region_h = inner_h + 3;
        // A nested boundary is drawn inset by its depth so containment reads.
        let depth = boundary_depth(&model, bi);
        let inset = (depth * 2).min(region_w / 4);
        s.rect(
            inset,
            y,
            (region_w - inset).max(2),
            region_h,
            BoxStyle::Double,
            cluster,
        );
        let kind = if b.kind.is_empty() {
            String::new()
        } else {
            format!(" [{}]", b.kind)
        };
        let title = format!(" {}{kind} ", b.title);
        s.text_clipped(inset + 2, y, &title, (region_w - inset - 4).max(1), cluster);
        draw_grid(&mut s, inset + 2, y + 2, &els, cols, theme, base);
        y += region_h + 1;
    }

    if !top.is_empty() {
        let cols = grid_cols(top.len());
        let rows = div_ceil_i32(top.len() as i32, cols).max(1);
        let inner_h = rows * (EL_H + GUT) - GUT;
        let region_h = inner_h + 3;
        s.rect(0, y, region_w, region_h, BoxStyle::Round, cluster);
        s.text_clipped(2, y, " context ", region_w - 4, cluster);
        draw_grid(&mut s, 2, y + 2, &top, cols, theme, base);
        y += region_h + 1;
    }

    if !model.relations.is_empty() {
        s.text(0, y, "relations:", edge_lbl);
        y += 1;
        for r in &model.relations {
            let arrow = if r.bi { "◂──▸" } else { "──▸" };
            let mut l = format!("{} {arrow} {}", r.from, r.to);
            if let Some(lbl) = &r.label {
                l.push_str(&format!("  «{lbl}»"));
            }
            if let Some(t) = &r.tech {
                l.push_str(&format!(" [{t}]"));
            }
            s.text(1, y, &l, edge_st);
            y += 1;
        }
    }

    s.blit(area, buf, base);
}

/// The nesting depth of boundary `bi` (0 = top-level), walking `parent`.
fn boundary_depth(model: &Model, bi: usize) -> i32 {
    let mut d = 0;
    let mut cur = model.boundaries[bi].parent;
    while let Some(p) = cur {
        d += 1;
        cur = model.boundaries.get(p).and_then(|b| b.parent);
        if d > 8 {
            break; // defensive: a malformed cycle never loops forever
        }
    }
    d
}

/// Lays a slice of elements into a grid starting at `(x0, y0)`.
fn draw_grid(
    s: &mut Surface,
    x0: i32,
    y0: i32,
    els: &[&Element],
    cols: i32,
    theme: &MermaidTheme,
    base: Style,
) {
    let mut row = 0;
    let mut col = 0;
    for el in els {
        let bx = x0 + col * (EL_W + GUT);
        let by = y0 + row * (EL_H + GUT);
        draw_element(s, bx, by, el, theme, base);
        col += 1;
        if col >= cols {
            col = 0;
            row += 1;
        }
    }
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
    fn parses_person_system_with_desc() {
        let m = parse(
            "C4Context\n\
             Person(user, \"User\", \"a person\")\n\
             System(sys, \"My System\", \"the system\")",
        );
        assert_eq!(m.elements.len(), 2);
        assert_eq!(m.elements[0].id, "user");
        assert_eq!(m.elements[0].name, "User");
        assert_eq!(m.elements[0].stereotype, "Person");
        assert_eq!(m.elements[0].desc.as_deref(), Some("a person"));
        assert_eq!(m.elements[1].stereotype, "System");
    }

    #[test]
    fn external_and_db_variants_get_stereotypes() {
        let m = parse(
            "C4Context\n\
             Person_Ext(e, \"Ext User\")\n\
             SystemDb(d, \"DB\")\n\
             System_Ext(x, \"Ext Sys\")",
        );
        assert_eq!(m.elements[0].stereotype, "Person, external");
        assert_eq!(m.elements[1].stereotype, "System, database");
        assert_eq!(m.elements[2].stereotype, "System, external");
    }

    #[test]
    fn container_has_tech_in_stereotype() {
        let m = parse("C4Container\nContainer(api, \"API\", \"Rust\", \"the api\")");
        assert_eq!(m.elements[0].stereotype, "Container: Rust");
        assert_eq!(m.elements[0].name, "API");
        assert_eq!(m.elements[0].desc.as_deref(), Some("the api"));
    }

    #[test]
    fn comma_inside_quotes_does_not_split() {
        let m = parse("C4Context\nSystem(s, \"A, B and C\", \"x, y\")");
        assert_eq!(m.elements[0].name, "A, B and C");
        assert_eq!(m.elements[0].desc.as_deref(), Some("x, y"));
    }

    #[test]
    fn boundary_nesting_and_membership() {
        let m = parse(
            "C4Context\n\
             Enterprise_Boundary(e, \"Ent\") {\n\
             System_Boundary(sb, \"Inner\") {\n\
             System(s, \"S\")\n\
             }\n\
             }\n\
             Person(p, \"P\")",
        );
        assert_eq!(m.boundaries.len(), 2);
        assert_eq!(m.boundaries[0].kind, "Enterprise");
        assert_eq!(m.boundaries[1].kind, "System");
        assert_eq!(m.boundaries[1].parent, Some(0));
        // The system is in the inner boundary; the person is top-level.
        assert_eq!(m.elements[0].boundary, Some(1));
        assert!(m.elements[1].boundary.is_none());
    }

    #[test]
    fn relations_birel_and_directional() {
        let m = parse(
            "C4Context\n\
             Rel(a, b, \"calls\", \"HTTP\")\n\
             BiRel(b, c, \"syncs\")\n\
             Rel_U(c, a)",
        );
        assert_eq!(m.relations.len(), 3);
        assert_eq!(m.relations[0].from, "a");
        assert_eq!(m.relations[0].to, "b");
        assert_eq!(m.relations[0].label.as_deref(), Some("calls"));
        assert_eq!(m.relations[0].tech.as_deref(), Some("HTTP"));
        assert!(!m.relations[0].bi);
        assert!(m.relations[1].bi);
        assert_eq!(m.relations[2].from, "c");
    }

    #[test]
    fn lenient_skips_garbage() {
        let m = parse("C4Context\nnot a call\nPerson(p, \"P\")\n)))(((");
        assert_eq!(m.elements.len(), 1);
    }

    #[test]
    fn empty_or_header_only_is_empty() {
        assert!(parse("").is_empty());
        assert!(parse("C4Context").is_empty());
        assert!(parse("C4Context\n%% just a comment").is_empty());
    }

    #[test]
    fn wrap_splits_on_whitespace_and_hard_splits_long_words() {
        assert_eq!(wrap("a b c", 3), vec!["a b", "c"]);
        assert_eq!(wrap("abcdef", 3), vec!["abc", "def"]);
        assert!(wrap("x", 0).is_empty());
    }

    // --- render snapshot tests --------------------------------------------

    #[test]
    fn empty_renders_placeholder() {
        let out = lines(Mermaid::new("C4Context"), 40, 5);
        assert!(out.contains("mermaid · C4: no elements"), "{out}");
    }

    #[test]
    fn person_and_system_render_boxes_with_stereotypes() {
        let src = "C4Context\nPerson(u, \"User\", \"end user\")\nSystem(s, \"Sys\", \"the app\")";
        let out = lines(Mermaid::new(src), 60, 16);
        assert!(out.contains("«Person»"), "{out}");
        assert!(out.contains("«System»"), "{out}");
        assert!(out.contains("User"), "{out}");
        assert!(out.contains("Sys"), "{out}");
        assert!(out.contains("end user"), "{out}");
        assert!(out.contains('┌'), "{out}");
    }

    #[test]
    fn boundary_renders_titled_dashed_region() {
        let src = "C4Context\nSystem_Boundary(b, \"Bnd\") {\nSystem(s, \"S\")\n}";
        let out = lines(Mermaid::new(src), 40, 14);
        assert!(out.contains("Bnd"), "{out}");
        assert!(out.contains("[System]"), "{out}");
        // A doubled boundary corner glyph.
        assert!(out.contains('╔'), "{out}");
    }

    #[test]
    fn relations_listed_with_arrows() {
        let src = "C4Context\nPerson(a, \"A\")\nSystem(b, \"B\")\nRel(a, b, \"uses\")\nBiRel(b, a, \"syncs\")";
        let out = lines(Mermaid::new(src), 60, 22);
        assert!(out.contains("relations:"), "{out}");
        assert!(out.contains("──▸"), "{out}");
        assert!(out.contains("◂──▸"), "{out}");
        assert!(out.contains("«uses»"), "{out}");
    }

    #[test]
    fn container_box_shows_tech_in_stereotype() {
        let src = "C4Container\nContainer(api, \"API\", \"Rust\", \"serves it\")";
        let out = lines(Mermaid::new(src), 40, 10);
        assert!(out.contains("«Container: Rust»"), "{out}");
        assert!(out.contains("API"), "{out}");
    }

    #[test]
    fn tiny_area_does_not_panic() {
        let src = "C4Context\nPerson(p, \"P\", \"d\")\nSystem(s, \"S\")\nRel(p, s, \"x\")";
        let _ = lines(Mermaid::new(src), 1, 1);
        let _ = lines(Mermaid::new(src), 4, 1);
        let _ = lines(Mermaid::new(src), 6, 3);
    }

    #[test]
    fn deterministic_across_repeated_renders() {
        let src = "C4Context\n\
                   Enterprise_Boundary(e, \"E\") {\n\
                   Person(u, \"U\", \"a user\")\n\
                   System(s, \"S\", \"a system\")\n\
                   }\n\
                   Rel(u, s, \"uses\", \"HTTP\")";
        let a = lines(Mermaid::new(src), 60, 24);
        let b = lines(Mermaid::new(src), 60, 24);
        assert_eq!(a, b);
    }
}
