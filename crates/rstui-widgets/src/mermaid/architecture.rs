//! `architecture-beta` Mermaid diagram renderer.
//!
//! Mermaid's architecture diagram describes a cloud/service topology: named
//! *groups* (regions, optionally with a parenthesised icon name), *services*
//! that live in a group or free-standing, *junction* fan-out points, and
//! *edges* that attach to a named side (`:L`/`:R`/`:T`/`:B`) of each endpoint,
//! optionally arrowed (`-->`).
//!
//! # Terminal layout approximation
//!
//! The web renderer is a force-directed graph; a terminal cannot honour the
//! edge *side* selectors as literal geometry without an unbounded router, so
//! this draws a deterministic, source-ordered approximation that keeps the
//! information legible:
//!
//! - Each group is a titled bordered region. Its services are packed into a
//!   fixed-width integer grid (a deterministic number of columns derived from
//!   the service count) so the same source always produces the same image.
//! - Free-standing services (no `in <group>`) sit in their own untitled region
//!   below the groups, packed with the same grid rule.
//! - Every service is a small box: its `[label]` centred, and its
//!   parenthesised icon name shown as a dim `(icon)` tag on the line below
//!   when the box is tall enough. A junction renders as a tiny `◇` marker box.
//! - Edges are listed beneath the regions as `a R──▸ L b` rows: the endpoint
//!   ids with their declared sides and an arrowhead when the edge was `-->`.
//!   This is the honest terminal stand-in for the web router's free curves —
//!   every relationship is still shown, deterministically, in source order.
//!
//! Parsing is lenient: an unrecognised line is skipped, never fatal; a source
//! with nothing parseable falls through to [`super::diagram_placeholder`].

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// One service / junction node parsed from an `architecture-beta` source.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Service {
    /// The identifier used by edges.
    id: String,
    /// The display label (the `[label]`), falling back to the id.
    label: String,
    /// The parenthesised icon name (`(database)`), if any.
    icon: Option<String>,
    /// The owning group id, if the service declared `in <group>`.
    group: Option<String>,
    /// Whether this node is a `junction` (a tiny fan-out marker).
    junction: bool,
}

/// One `group <id>(icon)[label]` region.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Group {
    /// The identifier referenced by `in <group>`.
    id: String,
    /// The display title (the `[label]`), falling back to the id.
    title: String,
}

/// One parsed edge: `a:R -- L:b` or arrowed `a:R --> L:b`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Edge {
    /// The left endpoint id.
    from: String,
    /// The side glyph the left endpoint attaches on (`L`/`R`/`T`/`B`).
    from_side: char,
    /// The right endpoint id.
    to: String,
    /// The side glyph the right endpoint attaches on.
    to_side: char,
    /// Whether the edge drew a `-->` arrowhead at the destination.
    arrow: bool,
}

/// The whole parsed diagram in source order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Model {
    /// Every `group`, in source order.
    groups: Vec<Group>,
    /// Every service / junction, in source order.
    services: Vec<Service>,
    /// Every edge, in source order.
    edges: Vec<Edge>,
}

impl Model {
    /// Whether nothing parseable was found (drives the placeholder fallback).
    fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.services.is_empty() && self.edges.is_empty()
    }
}

/// Splits an `id(icon)[label]` head into `(id, icon, label)`. The icon and
/// label segments are both optional and may appear in either order in the
/// wild; we accept whichever delimiter comes first and treat the bare leading
/// token as the id.
fn split_head(s: &str) -> (String, Option<String>, Option<String>) {
    let s = s.trim();
    // The id is the run up to the first '(' or '['.
    let id_end = s.find(['(', '[']).unwrap_or(s.len());
    let id = s[..id_end].trim().to_string();
    let rest = &s[id_end..];
    let mut icon = None;
    let mut label = None;
    let mut cur = rest;
    loop {
        let cur_t = cur.trim_start();
        if let Some(inner) = cur_t.strip_prefix('(') {
            if let Some(end) = inner.find(')') {
                icon = Some(inner[..end].trim().to_string());
                cur = &inner[end + 1..];
                continue;
            }
        }
        if let Some(inner) = cur_t.strip_prefix('[') {
            if let Some(end) = inner.find(']') {
                label = Some(inner[..end].trim().to_string());
                cur = &inner[end + 1..];
                continue;
            }
        }
        break;
    }
    (id, icon, label)
}

/// Parses one already-trimmed, comment-free statement into `model`.
fn parse_statement(line: &str, model: &mut Model) {
    if let Some(rest) = line.strip_prefix("group ") {
        let (id, _icon, label) = split_head(rest);
        if id.is_empty() {
            return;
        }
        let title = label.unwrap_or_else(|| id.clone());
        model.groups.push(Group { id, title });
        return;
    }
    if let Some(rest) = line.strip_prefix("service ") {
        // `<id>(<icon>)[<label>] in <group>` — the ` in ` tail is optional.
        let (head, group) = match rest.rsplit_once(" in ") {
            Some((h, g)) => (h, Some(g.trim().to_string())),
            None => (rest, None),
        };
        let (id, icon, label) = split_head(head);
        if id.is_empty() {
            return;
        }
        let label = label.unwrap_or_else(|| id.clone());
        model.services.push(Service {
            id,
            label,
            icon,
            group,
            junction: false,
        });
        return;
    }
    if let Some(rest) = line.strip_prefix("junction ") {
        let (head, group) = match rest.rsplit_once(" in ") {
            Some((h, g)) => (h, Some(g.trim().to_string())),
            None => (rest, None),
        };
        let id = head.trim();
        if id.is_empty() {
            return;
        }
        model.services.push(Service {
            id: id.to_string(),
            label: id.to_string(),
            icon: None,
            group,
            junction: true,
        });
        return;
    }
    parse_edge(line, model);
}

/// Parses an edge statement of the form `<l>{:S} -- {S:}<r>` (or `-->`).
/// Either side selector is optional; an unparseable line is skipped.
fn parse_edge(line: &str, model: &mut Model) {
    let (sep, arrow) = if line.contains("-->") {
        ("-->", true)
    } else if line.contains("--") {
        ("--", false)
    } else {
        return;
    };
    let Some((left, right)) = line.split_once(sep) else {
        return;
    };
    let (from, from_side) = split_endpoint(left.trim(), true);
    let (to, to_side) = split_endpoint(right.trim(), false);
    if from.is_empty() || to.is_empty() {
        return;
    }
    model.edges.push(Edge {
        from,
        from_side,
        to,
        to_side,
        arrow,
    });
}

/// Splits one edge endpoint into `(id, side)`. On the left of the arrow the
/// side trails (`db:R`); on the right it leads (`L:server`). A missing
/// selector defaults to `·` (unspecified).
fn split_endpoint(s: &str, left: bool) -> (String, char) {
    if left {
        if let Some((id, side)) = s.rsplit_once(':') {
            return (id.trim().to_string(), side_glyph(side));
        }
        (s.to_string(), '·')
    } else {
        if let Some((side, id)) = s.split_once(':') {
            return (id.trim().to_string(), side_glyph(side));
        }
        (s.to_string(), '·')
    }
}

/// Maps a `L`/`R`/`T`/`B` selector (any case) to its display glyph.
fn side_glyph(s: &str) -> char {
    match s.trim() {
        "L" | "l" => 'L',
        "R" | "r" => 'R',
        "T" | "t" => 'T',
        "B" | "b" => 'B',
        _ => '·',
    }
}

/// Parses an `architecture-beta` source: split lines, strip `\r`, drop a
/// leading `--- … ---` frontmatter block and `%%` comment/directive lines,
/// skip the header, and feed every remaining trimmed line to
/// [`parse_statement`]. Lenient — a bad line is skipped, never fatal.
fn parse(src: &str) -> Model {
    let mut model = Model::default();
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
            // The first significant line is the `architecture-beta` header.
            header_seen = true;
            continue;
        }
        parse_statement(line, &mut model);
    }
    model
}

/// The fixed cell size of a service box and the inter-box gutter.
const SVC_W: i32 = 16;
const SVC_H: i32 = 4;
const GUT: i32 = 1;

/// Ceiling of `a / b` for non-negative `i32`s (`i32::div_ceil` is unstable on
/// the pinned toolchain, so this keeps the layout integer-only and total).
fn div_ceil_i32(a: i32, b: i32) -> i32 {
    if b <= 0 {
        return a;
    }
    (a + b - 1) / b
}

/// Chooses a deterministic column count for `n` boxes packed in a grid: a
/// roughly square arrangement, capped so a single group never gets absurdly
/// wide.
fn grid_cols(n: usize) -> i32 {
    if n == 0 {
        return 1;
    }
    let mut c = 1;
    while c * c < n {
        c += 1;
    }
    (c as i32).clamp(1, 4)
}

/// Draws one service / junction box at `(x, y)` into `s`.
fn draw_service(s: &mut Surface, x: i32, y: i32, svc: &Service, theme: &MermaidTheme, base: Style) {
    let border = base.patch(theme.node_border);
    let text = base.patch(theme.node_label);
    let tag = base.patch(theme.edge_label);
    if svc.junction {
        s.rect(x, y, SVC_W, SVC_H, BoxStyle::Round, border);
        s.text_centered(x + 1, y + SVC_H / 2, SVC_W - 2, "◇ junction", text);
        return;
    }
    s.rect(x, y, SVC_W, SVC_H, BoxStyle::Square, border);
    s.text_centered(x + 1, y + 1, SVC_W - 2, &svc.label, text);
    if let Some(icon) = &svc.icon {
        let line = format!("({icon})");
        s.text_centered(x + 1, y + 2, SVC_W - 2, &line, tag);
    }
}

/// Lays a slice of services into a grid starting at `(x0, y0)`, returning the
/// total height consumed.
fn draw_grid(
    s: &mut Surface,
    x0: i32,
    y0: i32,
    svcs: &[&Service],
    cols: i32,
    theme: &MermaidTheme,
    base: Style,
) -> i32 {
    if svcs.is_empty() {
        return 0;
    }
    let mut row = 0;
    let mut col = 0;
    for svc in svcs {
        let bx = x0 + col * (SVC_W + GUT);
        let by = y0 + row * (SVC_H + GUT);
        draw_service(s, bx, by, svc, theme, base);
        col += 1;
        if col >= cols {
            col = 0;
            row += 1;
        }
    }
    let rows = if col == 0 { row } else { row + 1 };
    rows * (SVC_H + GUT) - GUT
}

/// Renders an `architecture-beta` Mermaid diagram from `src` into `area`.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let model = parse(src);
    if model.is_empty() {
        super::diagram_placeholder("architecture", "no nodes", area, buf, base, theme);
        return;
    }

    // Width: the widest group grid (or the free grid). Each group is its own
    // full-width region stacked vertically; free services follow.
    let group_count = |gid: &str| {
        model
            .services
            .iter()
            .filter(|s| s.group.as_deref() == Some(gid))
            .count()
    };
    let free: Vec<&Service> = model
        .services
        .iter()
        .filter(|s| s.group.is_none())
        .collect();

    let mut max_inner_cols = 1;
    for g in &model.groups {
        max_inner_cols = max_inner_cols.max(grid_cols(group_count(&g.id)));
    }
    if !free.is_empty() {
        max_inner_cols = max_inner_cols.max(grid_cols(free.len()));
    }
    let inner_w = max_inner_cols * (SVC_W + GUT) - GUT;
    let region_w = inner_w + 4; // 1 border + 1 pad each side
    // Edge list rows live below; size the surface for the worst case then
    // clip via blit.
    let mut total_h = 1;
    for g in &model.groups {
        let n = group_count(&g.id);
        let cols = grid_cols(n);
        let rows = div_ceil_i32(n as i32, cols);
        let inner_h = (rows.max(1)) * (SVC_H + GUT) - GUT;
        total_h += inner_h + 4; // border+title+pad
    }
    if !free.is_empty() {
        let cols = grid_cols(free.len());
        let rows = div_ceil_i32(free.len() as i32, cols);
        total_h += rows * (SVC_H + GUT) - GUT + 4;
    }
    let edge_h = if model.edges.is_empty() {
        0
    } else {
        model.edges.len() as i32 + 2
    };
    total_h += edge_h;

    // The widest row is either a region or an edge line; size generously.
    let mut surf_w = region_w;
    for e in &model.edges {
        let l = format!(
            "{} {}──{} {} {}",
            e.from,
            e.from_side,
            if e.arrow { "▸" } else { "─" },
            e.to_side,
            e.to
        );
        surf_w = surf_w.max(l.chars().count() as i32 + 2);
    }

    let mut s = Surface::new(surf_w, total_h.max(1));
    let cluster = base.patch(theme.cluster);
    let edge_st = base.patch(theme.edge);
    let edge_lbl = base.patch(theme.edge_label);

    let mut y = 0;
    for g in &model.groups {
        let svcs: Vec<&Service> = model
            .services
            .iter()
            .filter(|s| s.group.as_deref() == Some(g.id.as_str()))
            .collect();
        let cols = grid_cols(svcs.len());
        let rows = div_ceil_i32(svcs.len() as i32, cols).max(1);
        let inner_h = rows * (SVC_H + GUT) - GUT;
        let region_h = inner_h + 3; // title row + top/bottom border + content
        s.rect(0, y, region_w, region_h, BoxStyle::Round, cluster);
        let title = format!(" {} ", g.title);
        s.text_clipped(2, y, &title, region_w - 4, cluster);
        draw_grid(&mut s, 2, y + 2, &svcs, cols, theme, base);
        y += region_h + 1;
    }

    if !free.is_empty() {
        let cols = grid_cols(free.len());
        let rows = div_ceil_i32(free.len() as i32, cols).max(1);
        let inner_h = rows * (SVC_H + GUT) - GUT;
        let region_h = inner_h + 3;
        s.rect(0, y, region_w, region_h, BoxStyle::Round, cluster);
        s.text_clipped(2, y, " services ", region_w - 4, cluster);
        draw_grid(&mut s, 2, y + 2, &free, cols, theme, base);
        y += region_h + 1;
    }

    if !model.edges.is_empty() {
        s.text(0, y, "edges:", edge_lbl);
        y += 1;
        for e in &model.edges {
            let arrow = if e.arrow { '▸' } else { '─' };
            let line = format!("{} {}──{arrow} {} {}", e.from, e.from_side, e.to_side, e.to);
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
    /// glyphs as one newline-terminated line per row. Mirrors the shared
    /// `tests::lines` helper in [`super::super`].
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
    fn parses_group_service_with_icon_and_membership() {
        let m = parse(
            "architecture-beta\n\
             group api(cloud)[API Group]\n\
             service db(database)[Database] in api\n\
             service plain",
        );
        assert_eq!(m.groups.len(), 1);
        assert_eq!(m.groups[0].id, "api");
        assert_eq!(m.groups[0].title, "API Group");
        assert_eq!(m.services.len(), 2);
        assert_eq!(m.services[0].id, "db");
        assert_eq!(m.services[0].label, "Database");
        assert_eq!(m.services[0].icon.as_deref(), Some("database"));
        assert_eq!(m.services[0].group.as_deref(), Some("api"));
        assert_eq!(m.services[1].id, "plain");
        assert_eq!(m.services[1].label, "plain");
        assert!(m.services[1].group.is_none());
    }

    #[test]
    fn parses_junction_and_edges_with_sides_and_arrow() {
        let m = parse(
            "architecture-beta\n\
             junction j1 in api\n\
             db:R -- L:server\n\
             db:R --> L:server",
        );
        assert_eq!(m.services.len(), 1);
        assert!(m.services[0].junction);
        assert_eq!(m.services[0].id, "j1");
        assert_eq!(m.services[0].group.as_deref(), Some("api"));
        assert_eq!(m.edges.len(), 2);
        assert_eq!(m.edges[0].from, "db");
        assert_eq!(m.edges[0].from_side, 'R');
        assert_eq!(m.edges[0].to, "server");
        assert_eq!(m.edges[0].to_side, 'L');
        assert!(!m.edges[0].arrow);
        assert!(m.edges[1].arrow);
    }

    #[test]
    fn lenient_skips_bad_lines_keeps_good() {
        let m = parse(
            "architecture-beta\n\
             %% a comment\n\
             this is garbage\n\
             group g[G]\n\
             service s in g",
        );
        assert_eq!(m.groups.len(), 1);
        assert_eq!(m.services.len(), 1);
    }

    #[test]
    fn frontmatter_block_is_dropped() {
        let m = parse("---\ntitle: T\n---\narchitecture-beta\ngroup g[G]");
        assert_eq!(m.groups.len(), 1);
        assert_eq!(m.groups[0].title, "G");
    }

    #[test]
    fn edge_without_side_selectors_defaults_to_dot() {
        let m = parse("architecture-beta\na -- b");
        assert_eq!(m.edges.len(), 1);
        assert_eq!(m.edges[0].from, "a");
        assert_eq!(m.edges[0].from_side, '·');
        assert_eq!(m.edges[0].to_side, '·');
        assert_eq!(m.edges[0].to, "b");
    }

    #[test]
    fn empty_or_header_only_parses_to_empty() {
        assert!(parse("").is_empty());
        assert!(parse("architecture-beta").is_empty());
        assert!(parse("architecture-beta\n%% only a comment").is_empty());
    }

    // --- render snapshot tests --------------------------------------------

    #[test]
    fn empty_source_renders_placeholder() {
        let out = lines(Mermaid::new("architecture-beta"), 40, 5);
        assert!(out.contains("architecture"), "{out}");
        assert!(out.contains("no nodes"), "{out}");
    }

    #[test]
    fn garbage_only_renders_placeholder() {
        let out = lines(Mermaid::new("architecture-beta\n???\n!!!"), 44, 5);
        assert!(out.contains("mermaid · architecture: no nodes"), "{out}");
    }

    #[test]
    fn group_with_service_renders_titled_region_and_box() {
        let src = "architecture-beta\ngroup api(cloud)[API]\nservice db(database)[DB] in api";
        let out = lines(Mermaid::new(src), 30, 10);
        // Region border + title.
        assert!(out.contains("API"), "{out}");
        // Service label and icon tag.
        assert!(out.contains("DB"), "{out}");
        assert!(out.contains("(database)"), "{out}");
        // A rounded region corner glyph is present.
        assert!(out.contains('╭'), "{out}");
        // A square service-box corner glyph is present.
        assert!(out.contains('┌'), "{out}");
    }

    #[test]
    fn edge_row_shows_sides_and_arrowhead() {
        let src = "architecture-beta\nservice a\nservice b\na:R --> L:b";
        let out = lines(Mermaid::new(src), 40, 16);
        assert!(out.contains("edges:"), "{out}");
        assert!(out.contains('▸'), "{out}");
        assert!(out.contains('R'), "{out}");
        assert!(out.contains('L'), "{out}");
    }

    #[test]
    fn full_snapshot_single_group_single_service() {
        let src = "architecture-beta\ngroup g[Grp]\nservice s[Svc] in g";
        let out = lines(Mermaid::new(src), 26, 9);
        // Deterministic: assert the exact rows that carry the structure.
        let rows: Vec<&str> = out.lines().collect();
        assert!(
            rows.iter().any(|r| r.contains("Grp")),
            "title row missing:\n{out}"
        );
        assert!(
            rows.iter().any(|r| r.contains("Svc")),
            "service row missing:\n{out}"
        );
        // Stable across runs: same input twice → identical output.
        let again = lines(Mermaid::new(src), 26, 9);
        assert_eq!(out, again);
    }

    #[test]
    fn junction_renders_marker() {
        let src = "architecture-beta\ngroup g[G]\njunction j in g";
        let out = lines(Mermaid::new(src), 28, 9);
        assert!(out.contains('◇'), "{out}");
        assert!(out.contains("junction"), "{out}");
    }

    #[test]
    fn free_services_get_their_own_region() {
        let src = "architecture-beta\nservice a[Alpha]\nservice b[Beta]";
        let out = lines(Mermaid::new(src), 40, 12);
        assert!(out.contains("services"), "{out}");
        assert!(out.contains("Alpha"), "{out}");
        assert!(out.contains("Beta"), "{out}");
    }

    #[test]
    fn tiny_area_does_not_panic() {
        // 1×1 and 3×1 must clip, never panic.
        let src = "architecture-beta\ngroup g[G]\nservice s in g\ns:R --> L:s";
        let _ = lines(Mermaid::new(src), 1, 1);
        let _ = lines(Mermaid::new(src), 3, 1);
        let _ = lines(Mermaid::new(src), 5, 2);
    }

    #[test]
    fn deterministic_across_repeated_renders() {
        let src = "architecture-beta\n\
                   group a[A]\nservice s1 in a\nservice s2 in a\n\
                   group b[B]\nservice s3 in b\n\
                   s1:R --> L:s3";
        let a = lines(Mermaid::new(src), 50, 24);
        let b = lines(Mermaid::new(src), 50, 24);
        assert_eq!(a, b);
    }
}
