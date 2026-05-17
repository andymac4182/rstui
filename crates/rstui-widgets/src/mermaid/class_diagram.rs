//! `classDiagram` Mermaid diagram renderer.
//!
//! Renders the [`classDiagram`](https://mermaid.js.org/syntax/classDiagram.html)
//! subset as a deterministic box-and-line UML class diagram, drawn onto the
//! shared [`super::draw::Surface`] grid then blitted once into the widget area.
//!
//! # Supported subset
//!
//! * Header `classDiagram` or `classDiagram-v2`.
//! * Block bodies: `class Animal { +int age \n +String name \n +isMammal()
//!   \n #protected() }` — a brace-delimited member list, one member per line.
//! * One-liner members: `Animal : +int age` and `Animal : +run()`.
//! * `class List~T~` generic type parameters (the `~T~` is folded into the
//!   displayed class name as `List<T>`).
//! * Visibility prefixes `+` public, `-` private, `#` protected, `~` package
//!   are kept verbatim in the member text; a member containing `(` is treated
//!   as a method, everything else as an attribute, so the two compartments
//!   stay separated even from one-liner declarations.
//! * Relations between two class ids with the Mermaid arrow tokens:
//!   `<|--` inheritance, `*--` composition, `o--` aggregation, `-->`
//!   association, `..>` dependency (dashed), `..|>` realization, plus the bare
//!   `--`/`..` link. An optional ` : label` and quoted multiplicities
//!   (`"1" --> "*"`) are parsed and drawn on the connector.
//!
//! # Layout
//!
//! Classes are ranked with the same longest-path idea the flowchart layout
//! uses ([`super::rank_nodes`] in spirit, reimplemented locally over the
//! parsed relation DAG): a class with no incoming relation is rank 0, every
//! other class sits one rank past its deepest predecessor. Each rank becomes a
//! row of equal-width 3-compartment boxes (name / attributes / methods,
//! separated by `├──┤` rules). Connectors are drawn as simple orthogonal
//! lines from the bottom of the parent box to the top of the child box with
//! the relation's end glyph and any label.
//!
//! # Terminal approximations
//!
//! A character grid cannot draw true UML adornments. Inheritance/realization
//! use `▷`, composition `◆`, aggregation `◇`, association/dependency `▶`;
//! dashed relations (`..`) draw the stem with `·` instead of `│`. Boxes are a
//! fixed three-row-minimum height regardless of member count past what fits;
//! overflow is clipped with `…` exactly like every other shared-surface
//! renderer, never wrapped and never panicking.

use rstui_core::Color;

use super::draw::{BoxStyle, Surface};

/// How a relation's parent end is drawn and whether its stem is dashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelKind {
    /// `<|--` — generalization/inheritance (hollow triangle `▷`).
    Inheritance,
    /// `..|>` — realization (hollow triangle `▷`, dashed stem).
    Realization,
    /// `*--` — composition (filled diamond `◆`).
    Composition,
    /// `o--` — aggregation (hollow diamond `◇`).
    Aggregation,
    /// `-->` — association (filled arrow `▶`).
    Association,
    /// `..>` — dependency (filled arrow `▶`, dashed stem).
    Dependency,
    /// A bare `--` / `..` link with no head.
    Link,
}

impl RelKind {
    /// The glyph drawn where the connector meets the parent (top) box.
    const fn head(self) -> char {
        match self {
            Self::Inheritance | Self::Realization => '▷',
            Self::Composition => '◆',
            Self::Aggregation => '◇',
            Self::Association | Self::Dependency => '▶',
            Self::Link => '┴',
        }
    }

    /// `true` when the relation's stem is dashed (`..` family).
    const fn dashed(self) -> bool {
        matches!(self, Self::Realization | Self::Dependency)
    }
}

/// One parsed class: its display `name`, its `attrs` and `methods`
/// compartments (member text kept verbatim, including the visibility prefix).
#[derive(Debug, Default, Clone)]
struct Class {
    /// The class name as displayed (generics folded to `Name<T>`).
    name: String,
    /// Attribute members (no `(` in the member text).
    attrs: Vec<String>,
    /// Method members (the member text contains `(`).
    methods: Vec<String>,
}

/// One parsed relation between two class names.
#[derive(Debug, Clone)]
struct Relation {
    /// Index into [`Model::classes`] of the parent (arrow-head) class.
    parent: usize,
    /// Index into [`Model::classes`] of the child class.
    child: usize,
    /// The relation kind / head glyph.
    kind: RelKind,
    /// The optional relation label (`A --> B : owns`).
    label: String,
    /// The parent-side multiplicity (`"1"`), empty when absent.
    mult_parent: String,
    /// The child-side multiplicity (`"*"`), empty when absent.
    mult_child: String,
}

/// The whole parsed diagram: classes in declaration order plus relations.
#[derive(Debug, Default)]
struct Model {
    /// Classes in first-seen order; the index is the layout id.
    classes: Vec<Class>,
    /// Relations between classes, declaration order.
    relations: Vec<Relation>,
}

impl Model {
    /// Index of `name`, inserting an empty class if first seen — keeps a
    /// relation that names a class before its body still resolvable.
    fn class_id(&mut self, name: &str) -> usize {
        if let Some(i) = self.classes.iter().position(|c| c.name == name) {
            return i;
        }
        self.classes.push(Class {
            name: name.to_string(),
            ..Class::default()
        });
        self.classes.len() - 1
    }
}

/// Folds a raw class token into its displayed name: `List~T~` → `List<T>`,
/// and a trailing/leading whitespace trimmed. A bare `~` with no closer is
/// left as written (lenient).
fn display_name(raw: &str) -> String {
    let t = raw.trim();
    if let Some(open) = t.find('~') {
        if let Some(close_rel) = t[open + 1..].find('~') {
            let close = open + 1 + close_rel;
            return format!("{}<{}>{}", &t[..open], &t[open + 1..close], &t[close + 1..]);
        }
    }
    t.to_string()
}

/// Sorts a member into its compartment: a `(` anywhere marks a method (so
/// `+run()` and `+get(x) int` are methods), everything else an attribute.
fn add_member(class: &mut Class, member: &str) {
    let m = member.trim();
    if m.is_empty() {
        return;
    }
    if m.contains('(') {
        class.methods.push(m.to_string());
    } else {
        class.attrs.push(m.to_string());
    }
}

/// The relation arrow tokens, longest first so `<|--` wins over `--`. Each is
/// `(token, kind, head_on_left)`: `head_on_left` is `true` when the arrow head
/// sits on the *left* operand (`A <|-- B` → `B` inherits `A`, head on `A`).
const ARROWS: &[(&str, RelKind, bool)] = &[
    ("<|..", RelKind::Realization, true),
    ("..|>", RelKind::Realization, false),
    ("<|--", RelKind::Inheritance, true),
    ("--|>", RelKind::Inheritance, false),
    ("*--", RelKind::Composition, true),
    ("--*", RelKind::Composition, false),
    ("o--", RelKind::Aggregation, true),
    ("--o", RelKind::Aggregation, false),
    ("<--", RelKind::Association, true),
    ("-->", RelKind::Association, false),
    ("<..", RelKind::Dependency, true),
    ("..>", RelKind::Dependency, false),
    ("..", RelKind::Link, false),
    ("--", RelKind::Link, false),
];

/// Splits `"1"` style multiplicity quotes off an operand, returning
/// `(class_token, multiplicity)` with the quotes stripped.
fn split_mult(operand: &str) -> (String, String) {
    let t = operand.trim();
    if let Some(rest) = t.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            let mult = rest[..end].to_string();
            let class = rest[end + 1..].trim().to_string();
            return (class, mult);
        }
    }
    if let Some(stripped) = t.strip_suffix('"') {
        if let Some(start) = stripped.rfind('"') {
            let mult = stripped[start + 1..].to_string();
            let class = stripped[..start].trim().to_string();
            return (class, mult);
        }
    }
    (t.to_string(), String::new())
}

/// Parses one relation line of the form `LEFT <arrow> RIGHT [: label]`.
/// Returns `None` when no arrow token is present so the caller can fall back
/// to a member declaration.
fn parse_relation(model: &mut Model, line: &str) -> Option<()> {
    let (body, label) = match line.split_once(':') {
        Some((b, l)) => (b.trim(), l.trim().to_string()),
        None => (line.trim(), String::new()),
    };
    for &(tok, kind, head_left) in ARROWS {
        if let Some(pos) = body.find(tok) {
            let left = &body[..pos];
            let right = &body[pos + tok.len()..];
            if left.trim().is_empty() || right.trim().is_empty() {
                continue;
            }
            let (lc, lm) = split_mult(left);
            let (rc, rm) = split_mult(right);
            let li = model.class_id(&display_name(&lc));
            let ri = model.class_id(&display_name(&rc));
            // `head_left` means the arrow head (the "parent"/general end)
            // sits on the left operand.
            let (parent, child, mult_parent, mult_child) = if head_left {
                (li, ri, lm, rm)
            } else {
                (ri, li, rm, lm)
            };
            model.relations.push(Relation {
                parent,
                child,
                kind,
                label,
                mult_parent,
                mult_child,
            });
            return Some(());
        }
    }
    None
}

/// Parses the whole source into a [`Model`], lenient: a line that is neither
/// a class block, a member, nor a relation is skipped.
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
    // Skip the header line (`classDiagram` / `classDiagram-v2`).
    if let Some(first) = body.first() {
        if first.starts_with("classDiagram") {
            idx = 1;
        }
    }

    while idx < body.len() {
        let line = body[idx];
        idx += 1;
        if line.is_empty() {
            continue;
        }

        // `class Name { ... }` — possibly the body spans following lines.
        if let Some(rest) = line.strip_prefix("class ") {
            let rest = rest.trim();
            if let Some(open) = rest.find('{') {
                let head = display_name(&rest[..open]);
                let id = model.class_id(&head);
                let mut inner = rest[open + 1..].to_string();
                // Pull in following lines until the closing brace.
                while !inner.contains('}') && idx < body.len() {
                    inner.push('\n');
                    inner.push_str(body[idx]);
                    idx += 1;
                }
                let inner = inner.split('}').next().unwrap_or("");
                for raw in inner.split('\n') {
                    for member in raw.split(';') {
                        add_member(&mut model.classes[id], member);
                    }
                }
            } else {
                // Bare `class Name` declaration (registers it, no members).
                model.class_id(&display_name(rest));
            }
            continue;
        }

        // A relation line wins over a one-liner member only when an arrow
        // token is present.
        if parse_relation(&mut model, line).is_some() {
            continue;
        }

        // `Name : member` one-liner.
        if let Some((name, member)) = line.split_once(':') {
            let id = model.class_id(&display_name(name));
            add_member(&mut model.classes[id], member);
            continue;
        }

        // A bare identifier on its own line declares an empty class. Only a
        // real class-name token qualifies, so stray punctuation
        // (`!!garbage!!`) is skipped rather than turned into a phantom class.
        if is_class_token(line) {
            model.class_id(&display_name(line));
        }
    }

    model
}

/// `true` when `tok` is a plausible class name: starts with a letter or `_`,
/// every other char alphanumeric / `_` / `~` (generic) / `.` (qualified) and
/// no whitespace — the lenient gate for a bare-identifier class line.
fn is_class_token(tok: &str) -> bool {
    let mut cs = tok.chars();
    match cs.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    cs.all(|c| c.is_alphanumeric() || matches!(c, '_' | '~' | '.'))
}

/// Longest-path rank of every class from the relation DAG: a class with no
/// incoming relation is rank 0, every other is one past its deepest
/// predecessor. Mirrors the flowchart [`super::rank_nodes`] longest-path
/// fixpoint but over the local [`Model`]; self relations are ignored and a
/// cycle is bounded by the class count so it always terminates.
fn rank_classes(model: &Model) -> Vec<usize> {
    let n = model.classes.len();
    let mut rank = vec![0usize; n];
    if n == 0 {
        return rank;
    }
    let mut has_incoming = vec![false; n];
    for r in &model.relations {
        if r.parent != r.child {
            has_incoming[r.child] = true;
        }
    }
    let any_root = has_incoming.iter().any(|&v| !v);
    for _ in 0..n {
        let mut changed = false;
        for r in &model.relations {
            if r.parent == r.child {
                continue;
            }
            let cand = rank[r.parent] + 1;
            if cand > rank[r.child] && (any_root || r.child != 0) {
                rank[r.child] = cand;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    rank
}

/// A placed class box: grid origin and computed size.
#[derive(Debug, Clone, Copy)]
struct Placed {
    /// Left edge on the grid.
    x: i32,
    /// Top edge on the grid.
    y: i32,
    /// Box width in cells.
    w: i32,
    /// Box height in cells.
    h: i32,
}

/// The natural (unclipped) width a class needs: the widest of its name and
/// every member, plus the border and one cell of padding each side, clamped
/// to a sane minimum.
fn class_width(c: &Class) -> i32 {
    let mut w = c.name.chars().count() as i32;
    for m in c.attrs.iter().chain(c.methods.iter()) {
        w = w.max(m.chars().count() as i32);
    }
    (w + 4).clamp(8, 40)
}

/// The natural height: title row + a rule + attrs (>=1 row) + a rule +
/// methods (>=1 row) + the two borders.
fn class_height(c: &Class) -> i32 {
    let a = c.attrs.len().max(1) as i32;
    let m = c.methods.len().max(1) as i32;
    // top border, name, sep, attrs, sep, methods, bottom border
    1 + 1 + 1 + a + 1 + m + 1
}

/// Draws one class box (name / attributes / methods compartments separated by
/// `├──┤` rules) at its placed rectangle, clipping over-long member rows.
fn draw_class(
    s: &mut Surface,
    p: Placed,
    c: &Class,
    border: rstui_core::Style,
    text: rstui_core::Style,
) {
    s.rect(p.x, p.y, p.w, p.h, BoxStyle::Square, border);
    let inner_w = p.w - 2;
    // Name.
    s.text_centered(p.x + 1, p.y + 1, inner_w, &c.name, text);
    // Separator helper drawing `├───┤` across the box at row `ry`.
    let sep = |s: &mut Surface, ry: i32| {
        s.set(p.x, ry, '├', border);
        s.hline(p.x + 1, ry, inner_w, '─', border);
        s.set(p.x + p.w - 1, ry, '┤', border);
    };
    let attr_top = p.y + 2;
    sep(s, attr_top);
    let mut row = attr_top + 1;
    let attr_rows = c.attrs.len().max(1) as i32;
    for (i, a) in c.attrs.iter().enumerate() {
        if (i as i32) >= attr_rows {
            break;
        }
        s.text_clipped(p.x + 1, row + i as i32, a, inner_w, text);
    }
    row += attr_rows;
    sep(s, row);
    row += 1;
    for (i, m) in c.methods.iter().enumerate() {
        s.text_clipped(p.x + 1, row + i as i32, m, inner_w, text);
    }
}

/// A vertical connector between the two boxes, drawn strictly in the gap
/// *between* them (never over a border). The relation's head glyph sits on
/// the first stem cell adjacent to the **parent** (general) box; an optional
/// label and the two multiplicities are placed beside the stem. This is the
/// terminal approximation of an orthogonal UML connector — a single jog
/// links the two centre columns when they differ.
fn draw_relation(
    s: &mut Surface,
    rel: &Relation,
    pp: Placed,
    cp: Placed,
    edge: rstui_core::Style,
    edge_label: rstui_core::Style,
) {
    let dashed = rel.kind.dashed();
    let stem = if dashed { '·' } else { '│' };
    let dash_h = if dashed { '·' } else { '─' };
    // Whichever box is higher is the visual top; the connector lives in the
    // open rows `y0..=y1` between the two box borders.
    let parent_on_top = pp.y <= cp.y;
    let (top, bot) = if parent_on_top { (pp, cp) } else { (cp, pp) };
    let tx = top.x + top.w / 2; // top box centre column
    let bx = bot.x + bot.w / 2; // bottom box centre column
    let y0 = top.y + top.h; // first free row under the top box
    let y1 = bot.y - 1; // last free row above the bottom box
    if y1 < y0 {
        return; // boxes touch / overlap — nothing to route
    }
    let head = rel.kind.head();
    // Vertical run on the top box's centre column.
    s.vline(tx, y0, y1 - y0 + 1, stem, edge);
    // Horizontal jog on the last free row to reach the bottom column.
    if tx != bx {
        let (lo, hi) = (tx.min(bx), tx.max(bx));
        for x in lo..=hi {
            s.set(x, y1, dash_h, edge);
        }
        s.set(tx, y1, '┐', edge);
        s.set(bx, y1, '┘', edge);
        s.set(bx, y1.max(y0), stem, edge);
    }
    // Head glyph on the stem cell touching the parent box.
    if parent_on_top {
        s.set(tx, y0, head, edge);
    } else {
        s.set(bx, y1, head, edge);
    }
    // Label centred vertically beside the stem.
    if !rel.label.is_empty() {
        let my = ((y0 + y1) / 2).clamp(y0, y1);
        s.text(tx + 1, my, &rel.label, edge_label);
    }
    // Multiplicities sit next to their own box's stem end.
    let (pm_x, pm_y, cm_x, cm_y) = if parent_on_top {
        (tx + 1, y0, bx + 1, y1)
    } else {
        (bx + 1, y1, tx + 1, y0)
    };
    if !rel.mult_parent.is_empty() {
        s.text(pm_x, pm_y, &rel.mult_parent, edge_label);
    }
    if !rel.mult_child.is_empty() {
        s.text(cm_x, cm_y, &rel.mult_child, edge_label);
    }
}

/// Renders a `classDiagram` Mermaid diagram from `src` into `area`.
pub(crate) fn render(
    src: &str,
    area: rstui_core::Rect,
    buf: &mut rstui_core::Buffer,
    base: rstui_core::Style,
    theme: &super::MermaidTheme,
) {
    let model = parse(src);
    if model.classes.is_empty() {
        super::diagram_placeholder("class diagram", "no classes", area, buf, base, theme);
        return;
    }

    let rank = rank_classes(&model);
    let max_rank = *rank.iter().max().unwrap_or(&0);
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for (i, &r) in rank.iter().enumerate() {
        rows[r].push(i);
    }

    // Geometry: 3 cols of inter-box gutter, 2 rows between ranks. A right
    // margin wide enough for the longest relation label keeps a connector
    // label/head (drawn one cell right of a box's centre) on the surface.
    const HGAP: i32 = 3;
    const VGAP: i32 = 2;
    let widest_label = model
        .relations
        .iter()
        .map(|r| {
            r.label
                .chars()
                .count()
                .max(r.mult_parent.chars().count())
                .max(r.mult_child.chars().count())
        })
        .max()
        .unwrap_or(0);
    // One cell sits between the stem and the text; only reserve the column
    // band when some relation actually has a label/multiplicity to draw.
    let label_margin = if widest_label == 0 {
        0
    } else {
        widest_label as i32 + 1
    };
    let mut placed: Vec<Placed> = vec![
        Placed {
            x: 0,
            y: 0,
            w: 0,
            h: 0
        };
        model.classes.len()
    ];

    let mut row_y = 0;
    let mut content_w = 0;
    for row in &rows {
        let mut x = 0;
        let mut row_h = 0;
        for &ci in row {
            let c = &model.classes[ci];
            let w = class_width(c);
            let h = class_height(c);
            placed[ci] = Placed { x, y: row_y, w, h };
            x += w + HGAP;
            row_h = row_h.max(h);
        }
        content_w = content_w.max(x - HGAP);
        row_y += row_h + VGAP;
    }
    let total_h = (row_y - VGAP).max(1);
    let total_w = (content_w + label_margin).max(1);

    let mut s = Surface::new(total_w, total_h);
    let border = base.patch(theme.node_border);
    let text = base.patch(theme.node_label);
    let edge = base.patch(theme.edge);
    let edge_label = base.patch(theme.edge_label);

    // Boxes first; connectors then fill the blank inter-rank gap so a head
    // glyph adjacent to a box is never clobbered by a border.
    for (i, c) in model.classes.iter().enumerate() {
        draw_class(&mut s, placed[i], c, border, text);
    }
    for rel in &model.relations {
        if rel.parent == rel.child {
            continue;
        }
        draw_relation(
            &mut s,
            rel,
            placed[rel.parent],
            placed[rel.child],
            edge,
            edge_label,
        );
    }

    s.blit(area, buf, base);
}

/// `classDef`/`style` colour parsing is shared with the flowchart path; this
/// is referenced so the import stays meaningful when a future revision skins
/// class boxes from a `style` line.
#[allow(dead_code)]
fn _color(token: &str) -> Option<Color> {
    super::css_color(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Buffer, Position, Rect, Style};

    /// Renders `render(...)` into a fresh `w`×`h` buffer with the default
    /// theme and returns the glyphs as one newline-terminated row per line —
    /// the sibling of mod.rs `tests::lines` for a free-function renderer.
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
    fn parses_block_members_into_compartments() {
        let m = parse(
            "classDiagram\nclass Animal {\n+int age\n+String name\n+isMammal()\n#protected()\n}",
        );
        assert_eq!(m.classes.len(), 1);
        assert_eq!(m.classes[0].name, "Animal");
        assert_eq!(m.classes[0].attrs, vec!["+int age", "+String name"]);
        assert_eq!(m.classes[0].methods, vec!["+isMammal()", "#protected()"]);
    }

    #[test]
    fn parses_one_liner_members() {
        let m = parse("classDiagram\nAnimal : +int age\nAnimal : +run()");
        assert_eq!(m.classes.len(), 1);
        assert_eq!(m.classes[0].attrs, vec!["+int age"]);
        assert_eq!(m.classes[0].methods, vec!["+run()"]);
    }

    #[test]
    fn folds_generic_type_parameter() {
        let m = parse("classDiagram\nclass List~T~");
        assert_eq!(m.classes[0].name, "List<T>");
    }

    #[test]
    fn parses_inline_brace_body() {
        let m = parse("classDiagram\nclass Point { +int x ; +int y }");
        assert_eq!(m.classes[0].attrs, vec!["+int x", "+int y"]);
    }

    #[test]
    fn parses_all_relation_kinds_and_orientation() {
        let m = parse(
            "classDiagram\nAnimal <|-- Dog\nCar *-- Engine\nLib o-- Book\nA --> B\nC ..> D\nIface ..|> Impl",
        );
        // `Animal <|-- Dog`: head on the left operand (Animal is parent).
        let r = &m.relations[0];
        assert_eq!(m.classes[r.parent].name, "Animal");
        assert_eq!(m.classes[r.child].name, "Dog");
        assert_eq!(r.kind, RelKind::Inheritance);
        assert_eq!(m.relations[1].kind, RelKind::Composition);
        assert_eq!(m.relations[2].kind, RelKind::Aggregation);
        assert_eq!(m.relations[3].kind, RelKind::Association);
        assert_eq!(m.relations[4].kind, RelKind::Dependency);
        assert_eq!(m.relations[5].kind, RelKind::Realization);
        // `Iface ..|> Impl`: the `|>` head is on the *right* operand, so
        // `Impl` is the realized parent and `Iface` the realizing child.
        let rr = &m.relations[5];
        assert_eq!(m.classes[rr.parent].name, "Impl");
        assert_eq!(m.classes[rr.child].name, "Iface");
    }

    #[test]
    fn parses_label_and_multiplicities() {
        let m = parse("classDiagram\nCustomer \"1\" --> \"*\" Order : places");
        let r = &m.relations[0];
        assert_eq!(m.classes[r.child].name, "Customer");
        assert_eq!(m.classes[r.parent].name, "Order");
        assert_eq!(r.label, "places");
        // child is Customer (mult "1"), parent is Order (mult "*").
        assert_eq!(r.mult_child, "1");
        assert_eq!(r.mult_parent, "*");
    }

    #[test]
    fn lenient_skips_garbage_and_drops_frontmatter_and_comments() {
        let m = parse("---\ntitle: X\n---\nclassDiagram-v2\n%% a comment\n!!garbage!!\nclass A");
        assert_eq!(m.classes.len(), 1);
        assert_eq!(m.classes[0].name, "A");
    }

    #[test]
    fn rank_is_longest_path() {
        let m = parse("classDiagram\nA <|-- B\nB <|-- C");
        let r = rank_classes(&m);
        // A is parent of B, B is parent of C → ranks 0,1,2.
        let ia = m.classes.iter().position(|c| c.name == "A").unwrap();
        let ib = m.classes.iter().position(|c| c.name == "B").unwrap();
        let ic = m.classes.iter().position(|c| c.name == "C").unwrap();
        assert_eq!(r[ia], 0);
        assert_eq!(r[ib], 1);
        assert_eq!(r[ic], 2);
    }

    #[test]
    fn self_relation_does_not_panic_or_rank() {
        let m = parse("classDiagram\nNode --> Node : next");
        let r = rank_classes(&m);
        assert_eq!(r, vec![0]);
    }

    // --- render snapshots --------------------------------------------------

    #[test]
    fn empty_source_is_placeholder() {
        let out = lines("classDiagram", 40, 3);
        assert!(out.contains("mermaid"));
        assert!(out.contains("class diagram"));
        assert!(out.contains("no classes"));
    }

    #[test]
    fn no_parseable_content_is_placeholder() {
        let out = lines("classDiagram\n%% only a comment\n", 40, 3);
        assert!(out.contains("no classes"));
    }

    #[test]
    fn single_class_three_compartments_snapshot() {
        // "Animal"(6) / widest member "+int age"(8) → box width 8+4 = 12,
        // height = 7 (border, name, sep, 1 attr, sep, 1 method, border). The
        // buffer is sized to the surface so there is no centring offset.
        let out = lines("classDiagram\nclass Animal {\n+int age\n+run()\n}", 12, 7);
        assert_eq!(
            out,
            "┌──────────┐\n\
             │  Animal  │\n\
             ├──────────┤\n\
             │+int age  │\n\
             ├──────────┤\n\
             │+run()    │\n\
             └──────────┘\n"
        );
    }

    #[test]
    fn empty_class_has_blank_compartments_snapshot() {
        // Min box width is 8; an empty class still gets one blank attribute
        // row and one blank method row so its three compartments stay legible.
        let out = lines("classDiagram\nclass A", 8, 7);
        assert_eq!(
            out,
            "┌──────┐\n\
             │  A   │\n\
             ├──────┤\n\
             │      │\n\
             ├──────┤\n\
             │      │\n\
             └──────┘\n"
        );
    }

    #[test]
    fn two_classes_inheritance_has_head_glyph() {
        // Animal (rank 0) above Dog (rank 1) with a `▷` parent head.
        let out = lines("classDiagram\nAnimal <|-- Dog", 60, 24);
        assert!(out.contains('▷'), "inheritance head present:\n{out}");
        assert!(out.contains("Animal"));
        assert!(out.contains("Dog"));
    }

    #[test]
    fn association_label_is_drawn() {
        let out = lines("classDiagram\nA --> B : uses", 60, 24);
        assert!(out.contains("uses"), "relation label present:\n{out}");
        assert!(out.contains('▶'), "association head present:\n{out}");
    }

    #[test]
    fn composition_and_aggregation_glyphs() {
        let comp = lines("classDiagram\nCar *-- Engine", 60, 24);
        assert!(comp.contains('◆'), "composition diamond:\n{comp}");
        let agg = lines("classDiagram\nLib o-- Book", 60, 24);
        assert!(agg.contains('◇'), "aggregation diamond:\n{agg}");
    }

    #[test]
    fn tiny_area_does_not_panic() {
        for (w, h) in [(0, 0), (1, 1), (2, 1), (3, 3), (1, 20), (20, 1)] {
            let _ = lines("classDiagram\nclass A {\n+x\n}\nA <|-- B", w, h);
        }
    }

    #[test]
    fn long_member_is_clipped_with_ellipsis() {
        let out = lines(
            "classDiagram\nA : +averyveryveryverylongmembernamethatoverflows int",
            60,
            10,
        );
        assert!(out.contains('…'), "overflow clipped:\n{out}");
    }
}
