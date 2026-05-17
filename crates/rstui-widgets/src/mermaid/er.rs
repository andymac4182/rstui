//! `erDiagram` Mermaid entity-relationship renderer.
//!
//! Renders the [ER-diagram](https://mermaid.js.org/syntax/entityRelationshipDiagram.html)
//! subset as a deterministic grid of attribute tables joined by
//! crow's-foot-ish connectors, drawn on the shared [`super::draw::Surface`].
//!
//! # Supported subset
//!
//! * Header `erDiagram`.
//! * Relationship `CUSTOMER ||--o{ ORDER : places` — the left/right
//!   cardinality tokens `||` (exactly one), `|o` / `o|` (zero or one),
//!   `}o` / `o{` (zero or many), `}|` / `|{` (one or many) and the verb
//!   label after the `:`.
//! * Entity attribute block:
//!   `CUSTOMER { string name PK \n string email "comment" \n int age FK }` —
//!   each line is `type name [PK|FK|UK] ["comment"]`.
//!
//! # Layout
//!
//! Entities are ranked with the flowchart longest-path idea
//! ([`super::rank_nodes`] in spirit) over the relationship graph: an entity
//! with no incoming relationship is rank 0, every other one rank past its
//! deepest predecessor. Each rank is a row of equal-height tables (a title
//! row, a rule, then one row per attribute). Relationship connectors are
//! drawn vertically between the two tables with an ASCII crow's-foot end
//! marker at each side and the verb centred on the stem.
//!
//! # Terminal approximations
//!
//! A character grid cannot draw true crow's-foot notation: "exactly one" is
//! `┼`, the "many" fan is `△`/`▽`, and an optional ("zero …") end adds a
//! leading `o` on the stem. Connectors are single vertical stems with one
//! horizontal jog. Over-long attribute rows are clipped with `…`; a bad line
//! is skipped; nothing panics.

use super::draw::{BoxStyle, Surface};

/// One ER cardinality end, parsed from a one-or-two-char token on its side of
/// the relationship operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Card {
    /// `||` — exactly one.
    One,
    /// `|o` / `o|` — zero or one.
    ZeroOne,
    /// `}|` / `|{` — one or many.
    OneMany,
    /// `}o` / `o{` — zero or many.
    ZeroMany,
}

impl Card {
    /// The end marker glyph where the connector meets the entity. `facing` is
    /// the crow's-foot fan glyph pointing toward that entity.
    const fn marker(self, facing: char) -> char {
        match self {
            // Exactly one: a crossbar.
            Self::One => '┼',
            // Zero or one: a small circle.
            Self::ZeroOne => 'o',
            // One/zero or many: the crow's-foot fan (optionality is the
            // separate leading `o`, drawn by the caller).
            Self::OneMany | Self::ZeroMany => facing,
        }
    }

    /// `true` when this end is optional (zero allowed) — drawn with a leading
    /// `o` on the stem so the optionality reads even in one cell.
    const fn optional(self) -> bool {
        matches!(self, Self::ZeroOne | Self::ZeroMany)
    }
}

/// Parses a cardinality token (either side): one of `||`, `|o`, `o|`, `}o`,
/// `}|`, `o{`, `|{`.
fn parse_card(tok: &str) -> Option<Card> {
    match tok {
        "||" => Some(Card::One),
        "|o" | "o|" => Some(Card::ZeroOne),
        "}|" | "|{" => Some(Card::OneMany),
        "}o" | "o{" => Some(Card::ZeroMany),
        _ => None,
    }
}

/// One attribute row inside an entity table.
#[derive(Debug, Clone)]
struct Attr {
    /// The declared type (`string`, `int`, …).
    ty: String,
    /// The attribute name.
    name: String,
    /// The key tag (`PK` / `FK` / `UK`), empty when none.
    key: String,
}

/// One parsed entity (a table).
#[derive(Debug, Default, Clone)]
struct Entity {
    /// The entity name (the table title).
    name: String,
    /// The attribute rows, declaration order.
    attrs: Vec<Attr>,
}

/// One parsed relationship between two entities.
#[derive(Debug, Clone)]
struct Rel {
    /// Index of the left entity in [`Model::entities`].
    left: usize,
    /// Index of the right entity.
    right: usize,
    /// The left-side cardinality.
    lcard: Card,
    /// The right-side cardinality.
    rcard: Card,
    /// The verb label (`places`), empty when absent.
    verb: String,
}

/// The whole parsed diagram.
#[derive(Debug, Default)]
struct Model {
    /// Entities in first-seen order; the index is the layout id.
    entities: Vec<Entity>,
    /// Relationships, declaration order.
    rels: Vec<Rel>,
}

impl Model {
    /// Index of `name`, inserting an empty entity if first seen.
    fn entity_id(&mut self, name: &str) -> usize {
        if let Some(i) = self.entities.iter().position(|e| e.name == name) {
            return i;
        }
        self.entities.push(Entity {
            name: name.to_string(),
            ..Entity::default()
        });
        self.entities.len() - 1
    }
}

/// Splits an attribute body line `type name [KEY] ["comment"]` into an
/// [`Attr`] (the comment is parsed off but not displayed — the table only has
/// type/name/key columns in the terminal approximation).
fn parse_attr(line: &str) -> Option<Attr> {
    // Drop a trailing quoted comment.
    let body = match line.find('"') {
        Some(i) => line[..i].trim(),
        None => line.trim(),
    };
    let mut it = body.split_whitespace();
    let ty = it.next()?.to_string();
    let name = it.next()?.to_string();
    let key = it
        .next()
        .filter(|k| matches!(*k, "PK" | "FK" | "UK"))
        .unwrap_or("")
        .to_string();
    Some(Attr { ty, name, key })
}

/// Splits the relationship body around the stem (`--` solid / `..`
/// non-identifying) into `(left_name, left_card, right_card, right_name)`, or
/// `None` when no operator is present.
fn split_operator(body: &str) -> Option<(&str, &str, &str, &str)> {
    for stem in ["--", ".."] {
        if let Some(pos) = body.find(stem) {
            let lhs = body[..pos].trim_end();
            let rhs = body[pos + stem.len()..].trim_start();
            let (lname, lcard) = split_trailing_card(lhs);
            let (rcard, rname) = split_leading_card(rhs);
            if lname.is_empty() || rname.is_empty() {
                continue;
            }
            return Some((lname, lcard, rcard, rname));
        }
    }
    None
}

/// Splits `CUSTOMER ||` into `("CUSTOMER", "||")` — the cardinality is the
/// maximal trailing run of `|`, `o`, `{`, `}`.
fn split_trailing_card(lhs: &str) -> (&str, &str) {
    let t = lhs.trim();
    let cut = t
        .char_indices()
        .rev()
        .take_while(|&(_, c)| matches!(c, '|' | 'o' | '{' | '}'))
        .last()
        .map(|(i, _)| i);
    match cut {
        Some(i) => (t[..i].trim_end(), &t[i..]),
        None => (t, ""),
    }
}

/// Splits `o{ ORDER` into `("o{", "ORDER")` — the leading cardinality run.
fn split_leading_card(rhs: &str) -> (&str, &str) {
    let t = rhs.trim();
    let cut = t
        .char_indices()
        .take_while(|&(_, c)| matches!(c, '|' | 'o' | '{' | '}'))
        .last()
        .map(|(i, c)| i + c.len_utf8());
    match cut {
        Some(i) => (&t[..i], t[i..].trim_start()),
        None => ("", t),
    }
}

/// Parses one relationship line `LEFT <lcard><stem><rcard> RIGHT [: verb]`.
fn parse_rel(model: &mut Model, line: &str) -> bool {
    let (body, verb) = match line.split_once(':') {
        Some((b, v)) => (b.trim(), v.trim().to_string()),
        None => (line.trim(), String::new()),
    };
    let Some((lname, lc, rc, rname)) = split_operator(body) else {
        return false;
    };
    let (Some(lcard), Some(rcard)) = (parse_card(lc), parse_card(rc)) else {
        return false;
    };
    let left = model.entity_id(lname.trim());
    let right = model.entity_id(rname.trim());
    model.rels.push(Rel {
        left,
        right,
        lcard,
        rcard,
        verb,
    });
    true
}

/// `true` when `tok` is a plausible entity name (letter/`_` start, then
/// alphanumeric / `_`), so stray punctuation is skipped.
fn is_entity_token(tok: &str) -> bool {
    let mut cs = tok.chars();
    match cs.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    cs.all(|c| c.is_alphanumeric() || c == '_')
}

/// Parses the whole source into a [`Model`], lenient: a line that is neither
/// an entity block, an attribute, nor a relationship is skipped.
fn parse(src: &str) -> Model {
    let mut model = Model::default();
    let raw: Vec<&str> = src
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .map(super::strip_comment)
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    // Optional leading `--- … ---` frontmatter block.
    let body: Vec<&str> = if raw.first() == Some(&"---") {
        let end = raw
            .iter()
            .skip(1)
            .position(|l| *l == "---")
            .map_or(raw.len(), |p| p + 2);
        raw[end..].to_vec()
    } else {
        raw
    };

    let mut idx = 0;
    if body.first().is_some_and(|f| f.starts_with("erDiagram")) {
        idx = 1;
    }

    while idx < body.len() {
        let line = body[idx];
        idx += 1;

        // `ENTITY { ... }` attribute block (body may span following lines).
        if let Some(open) = line.find('{') {
            let head = line[..open].trim();
            if is_entity_token(head) {
                let id = model.entity_id(head);
                let mut inner = line[open + 1..].to_string();
                while !inner.contains('}') && idx < body.len() {
                    inner.push('\n');
                    inner.push_str(body[idx]);
                    idx += 1;
                }
                let inner = inner.split('}').next().unwrap_or("");
                for row in inner.split('\n') {
                    let row = row.trim();
                    if row.is_empty() {
                        continue;
                    }
                    if let Some(a) = parse_attr(row) {
                        model.entities[id].attrs.push(a);
                    }
                }
                continue;
            }
        }

        // A relationship line.
        if parse_rel(&mut model, line) {
            continue;
        }

        // A bare entity declaration.
        if is_entity_token(line) {
            model.entity_id(line);
        }
    }

    model
}

/// Longest-path rank over the relationship graph (left → right). Mirrors the
/// flowchart [`super::rank_nodes`] fixpoint; self relationships are ignored
/// and the iteration is bounded by the entity count so a cycle terminates.
fn rank_entities(model: &Model) -> Vec<usize> {
    let n = model.entities.len();
    let mut rank = vec![0usize; n];
    if n == 0 {
        return rank;
    }
    let mut has_incoming = vec![false; n];
    for r in &model.rels {
        if r.left != r.right {
            has_incoming[r.right] = true;
        }
    }
    let any_root = has_incoming.iter().any(|&v| !v);
    for _ in 0..n {
        let mut changed = false;
        for r in &model.rels {
            if r.left == r.right {
                continue;
            }
            let cand = rank[r.left] + 1;
            if cand > rank[r.right] && (any_root || r.right != 0) {
                rank[r.right] = cand;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    rank
}

/// A placed entity table: grid rectangle.
#[derive(Debug, Clone, Copy, Default)]
struct Placed {
    /// Left edge on the grid.
    x: i32,
    /// Top edge on the grid.
    y: i32,
    /// Table width in cells.
    w: i32,
    /// Table height in cells.
    h: i32,
}

/// The displayed text of an attribute row: `type name [KEY]`.
fn attr_text(a: &Attr) -> String {
    if a.key.is_empty() {
        format!("{} {}", a.ty, a.name)
    } else {
        format!("{} {} {}", a.ty, a.name, a.key)
    }
}

/// The on-grid width an entity table needs: the widest of its name and every
/// attribute row, plus border + padding, clamped to a sane range.
fn entity_width(e: &Entity) -> i32 {
    let mut w = e.name.chars().count() as i32;
    for a in &e.attrs {
        w = w.max(attr_text(a).chars().count() as i32);
    }
    (w + 4).clamp(10, 44)
}

/// The on-grid height: title row + rule + one row per attribute (>=1 blank
/// row when the entity has none) + the two borders.
fn entity_height(e: &Entity) -> i32 {
    let rows = e.attrs.len().max(1) as i32;
    // top border, title, rule, rows, bottom border
    1 + 1 + 1 + rows + 1
}

/// Draws one entity table at its placed rectangle (title row, a `├──┤` rule,
/// then one clipped row per attribute).
fn draw_entity(
    s: &mut Surface,
    p: Placed,
    e: &Entity,
    border: rstui_core::Style,
    text: rstui_core::Style,
) {
    s.rect(p.x, p.y, p.w, p.h, BoxStyle::Square, border);
    let inner_w = p.w - 2;
    s.text_centered(p.x + 1, p.y + 1, inner_w, &e.name, text);
    // Title/body separator.
    s.set(p.x, p.y + 2, '├', border);
    s.hline(p.x + 1, p.y + 2, inner_w, '─', border);
    s.set(p.x + p.w - 1, p.y + 2, '┤', border);
    for (i, a) in e.attrs.iter().enumerate() {
        s.text_clipped(p.x + 1, p.y + 3 + i as i32, &attr_text(a), inner_w, text);
    }
}

/// A vertical relationship connector between two tables with a crow's-foot-ish
/// end marker at each side and the verb centred on the stem — the terminal
/// approximation of an ER relationship line.
fn draw_rel(
    s: &mut Surface,
    rel: &Rel,
    lp: Placed,
    rp: Placed,
    edge: rstui_core::Style,
    edge_label: rstui_core::Style,
) {
    let left_on_top = lp.y <= rp.y;
    let (top, bot) = if left_on_top { (lp, rp) } else { (rp, lp) };
    let (tcard, bcard) = if left_on_top {
        (rel.lcard, rel.rcard)
    } else {
        (rel.rcard, rel.lcard)
    };
    let tx = top.x + top.w / 2;
    let bx = bot.x + bot.w / 2;
    let y0 = top.y + top.h;
    let y1 = bot.y - 1;
    if y1 < y0 {
        return;
    }
    s.vline(tx, y0, y1 - y0 + 1, '│', edge);
    if tx != bx {
        let (lo, hi) = (tx.min(bx), tx.max(bx));
        for x in lo..=hi {
            s.set(x, y1, '─', edge);
        }
        s.set(tx, y1, '┐', edge);
        s.set(bx, y1, '┘', edge);
    }
    // End markers: the fan points *toward* the entity it touches.
    s.set(tx, y0, tcard.marker('▽'), edge);
    s.set(bx, y1, bcard.marker('△'), edge);
    // A leading optionality circle one cell in from each table edge.
    if tcard.optional() && y0 < y1 {
        s.set(tx, y0 + 1, 'o', edge);
    }
    if bcard.optional() && y1 > y0 {
        s.set(bx, y1 - 1, 'o', edge);
    }
    if !rel.verb.is_empty() {
        let my = ((y0 + y1) / 2).clamp(y0, y1);
        s.text(tx + 1, my, &rel.verb, edge_label);
    }
}

/// Renders an `erDiagram` Mermaid diagram from `src` into `area`.
pub(crate) fn render(
    src: &str,
    area: rstui_core::Rect,
    buf: &mut rstui_core::Buffer,
    base: rstui_core::Style,
    theme: &super::MermaidTheme,
) {
    let model = parse(src);
    if model.entities.is_empty() {
        super::diagram_placeholder("er diagram", "no entities", area, buf, base, theme);
        return;
    }

    let rank = rank_entities(&model);
    let max_rank = *rank.iter().max().unwrap_or(&0);
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for (i, &r) in rank.iter().enumerate() {
        rows[r].push(i);
    }

    const HGAP: i32 = 4;
    const VGAP: i32 = 2;
    let mut placed: Vec<Placed> = vec![Placed::default(); model.entities.len()];

    let mut row_y = 0;
    let mut content_w = 0;
    for row in &rows {
        let mut x = 0;
        let mut row_h = 0;
        for &ei in row {
            let e = &model.entities[ei];
            let w = entity_width(e);
            let h = entity_height(e);
            placed[ei] = Placed { x, y: row_y, w, h };
            x += w + HGAP;
            row_h = row_h.max(h);
        }
        content_w = content_w.max(x - HGAP);
        row_y += row_h + VGAP;
    }
    let widest_verb = model
        .rels
        .iter()
        .map(|r| r.verb.chars().count())
        .max()
        .unwrap_or(0);
    let label_margin = if widest_verb == 0 {
        0
    } else {
        widest_verb as i32 + 1
    };
    let total_w = (content_w + label_margin).max(1);
    let total_h = (row_y - VGAP).max(1);

    let mut s = Surface::new(total_w, total_h);
    let border = base.patch(theme.node_border);
    let text = base.patch(theme.node_label);
    let edge = base.patch(theme.edge);
    let edge_label = base.patch(theme.edge_label);

    // Tables first so a passing connector never crosses a border; the
    // connectors then fill the blank inter-rank gap.
    for (i, e) in model.entities.iter().enumerate() {
        draw_entity(&mut s, placed[i], e, border, text);
    }
    for rel in &model.rels {
        if rel.left == rel.right {
            continue;
        }
        draw_rel(
            &mut s,
            rel,
            placed[rel.left],
            placed[rel.right],
            edge,
            edge_label,
        );
    }

    s.blit(area, buf, base);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Buffer, Position, Rect, Style};

    /// Renders `render(...)` into a fresh `w`×`h` buffer with the default
    /// theme and returns the glyphs joined one newline-terminated row per
    /// line — the sibling of mod.rs `tests::lines`.
    fn lines(src: &str, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        render(
            src,
            buf.area(),
            &mut buf,
            Style::new(),
            &super::super::MermaidTheme::default(),
        );
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    // --- parser ------------------------------------------------------------

    #[test]
    fn parses_relationship_cardinalities_and_verb() {
        let m = parse("erDiagram\nCUSTOMER ||--o{ ORDER : places");
        assert_eq!(m.entities.len(), 2);
        assert_eq!(m.entities[0].name, "CUSTOMER");
        assert_eq!(m.entities[1].name, "ORDER");
        assert_eq!(m.rels.len(), 1);
        let r = &m.rels[0];
        assert_eq!(r.lcard, Card::One);
        assert_eq!(r.rcard, Card::ZeroMany);
        assert_eq!(r.verb, "places");
    }

    #[test]
    fn parses_every_cardinality_token() {
        let m = parse("erDiagram\nA |o--|| B\nC }|--|{ D\nE }o--o{ F\nG ||--|o H");
        assert_eq!(m.rels[0].lcard, Card::ZeroOne);
        assert_eq!(m.rels[0].rcard, Card::One);
        assert_eq!(m.rels[1].lcard, Card::OneMany);
        assert_eq!(m.rels[1].rcard, Card::OneMany);
        assert_eq!(m.rels[2].lcard, Card::ZeroMany);
        assert_eq!(m.rels[2].rcard, Card::ZeroMany);
        assert_eq!(m.rels[3].lcard, Card::One);
        assert_eq!(m.rels[3].rcard, Card::ZeroOne);
    }

    #[test]
    fn parses_attribute_block_with_keys_and_comment() {
        let m = parse(
            "erDiagram\nCUSTOMER {\nstring name PK\nstring email \"the contact\"\nint age FK\n}",
        );
        let e = &m.entities[0];
        assert_eq!(e.attrs.len(), 3);
        assert_eq!(e.attrs[0].ty, "string");
        assert_eq!(e.attrs[0].name, "name");
        assert_eq!(e.attrs[0].key, "PK");
        // The quoted comment is parsed off, not kept in the name.
        assert_eq!(e.attrs[1].name, "email");
        assert_eq!(e.attrs[1].key, "");
        assert_eq!(e.attrs[2].key, "FK");
    }

    #[test]
    fn relationship_then_attribute_block_merge_into_one_entity() {
        let m = parse("erDiagram\nCUSTOMER ||--o{ ORDER : places\nORDER {\nint id PK\n}");
        // ORDER is one entity, gaining the attribute from its later block.
        assert_eq!(m.entities.iter().filter(|e| e.name == "ORDER").count(), 1);
        let o = m.entities.iter().find(|e| e.name == "ORDER").unwrap();
        assert_eq!(o.attrs.len(), 1);
        assert_eq!(o.attrs[0].name, "id");
    }

    #[test]
    fn non_identifying_dotted_stem_is_accepted() {
        let m = parse("erDiagram\nA |o..o| B");
        assert_eq!(m.rels.len(), 1);
        assert_eq!(m.rels[0].lcard, Card::ZeroOne);
        assert_eq!(m.rels[0].rcard, Card::ZeroOne);
    }

    #[test]
    fn lenient_skips_garbage_and_frontmatter() {
        let m = parse("---\ntitle: T\n---\nerDiagram\n%% a comment\n###bad###\nA ||--|| B");
        assert_eq!(m.entities.len(), 2);
        assert_eq!(m.rels.len(), 1);
    }

    #[test]
    fn rank_is_longest_path() {
        let m = parse("erDiagram\nA ||--|| B\nB ||--|| C");
        let r = rank_entities(&m);
        let ia = m.entities.iter().position(|e| e.name == "A").unwrap();
        let ib = m.entities.iter().position(|e| e.name == "B").unwrap();
        let ic = m.entities.iter().position(|e| e.name == "C").unwrap();
        assert_eq!(r[ia], 0);
        assert_eq!(r[ib], 1);
        assert_eq!(r[ic], 2);
    }

    // --- render snapshots --------------------------------------------------

    #[test]
    fn empty_source_is_placeholder() {
        let out = lines("erDiagram", 40, 3);
        assert!(out.contains("mermaid"));
        assert!(out.contains("er diagram"));
        assert!(out.contains("no entities"));
    }

    #[test]
    fn only_comment_is_placeholder() {
        let out = lines("erDiagram\n%% nothing", 40, 3);
        assert!(out.contains("no entities"));
    }

    #[test]
    fn single_entity_table_snapshot() {
        // "USER"(4) / widest row "string name PK"(14) → width 14+4 = 18,
        // height = top + title + rule + 2 attrs + bottom = 6. Buffer sized to
        // the surface so there is no centring offset.
        let out = lines("erDiagram\nUSER {\nstring name PK\nint age\n}", 18, 6);
        assert_eq!(
            out,
            "┌────────────────┐\n\
             │      USER      │\n\
             ├────────────────┤\n\
             │string name PK  │\n\
             │int age         │\n\
             └────────────────┘\n"
        );
    }

    #[test]
    fn empty_entity_has_blank_row_snapshot() {
        // A bare entity still gets one blank attribute row so the table is
        // legible. Min width is 10.
        let out = lines("erDiagram\nLONE", 10, 5);
        assert_eq!(
            out,
            "┌────────┐\n\
             │  LONE  │\n\
             ├────────┤\n\
             │        │\n\
             └────────┘\n"
        );
    }

    #[test]
    fn relationship_connector_has_markers_and_verb() {
        let out = lines("erDiagram\nCUSTOMER ||--o{ ORDER : places", 60, 16);
        assert!(out.contains("places"), "verb present:\n{out}");
        assert!(out.contains("CUSTOMER") && out.contains("ORDER"));
        // The "exactly one" end is a crossbar, the "zero or many" end a fan.
        assert!(out.contains('┼'), "one marker present:\n{out}");
        assert!(
            out.contains('△') || out.contains('▽'),
            "many fan present:\n{out}"
        );
        assert!(out.contains('o'), "optional circle present:\n{out}");
    }

    #[test]
    fn two_related_entities_stack_by_rank() {
        let out = lines("erDiagram\nA ||--|| B", 30, 14);
        // A (rank 0) above B (rank 1).
        let a_row = out.lines().position(|l| l.contains('A')).unwrap();
        let b_row = out.lines().position(|l| l.contains('B')).unwrap();
        assert!(a_row < b_row, "A above B:\n{out}");
    }

    #[test]
    fn long_attribute_row_is_clipped() {
        let out = lines(
            "erDiagram\nT {\nstring anextremelylongattributenamethatwillnotfit PK\n}",
            60,
            7,
        );
        assert!(out.contains('…'), "overflow clipped:\n{out}");
    }

    #[test]
    fn tiny_area_does_not_panic() {
        for (w, h) in [(0, 0), (1, 1), (2, 1), (3, 3), (1, 30), (30, 1)] {
            let _ = lines(
                "erDiagram\nCUSTOMER ||--o{ ORDER : places\nCUSTOMER {\nstring name PK\n}",
                w,
                h,
            );
        }
    }
}
