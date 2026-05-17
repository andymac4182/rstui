//! `mindmap` Mermaid diagram renderer.
//!
//! A Mermaid `mindmap` is an *indentation-significant* outline: every line
//! after the `mindmap` header is one node, and a line indented more deeply
//! than the line above it is a **child** of the nearest shallower line. There
//! is no `-->`/`---` edge syntax — the tree is the whitespace.
//!
//! # Why a left-rooted bracket tree, not a radial map
//!
//! The Mermaid web renderer draws a mindmap *radially* (the root in the
//! centre, branches fanning out at angles). A character grid has no usable
//! sub-cell angular resolution, so a faithful radial layout is not
//! text-feasible — it would collapse to an unreadable smear. Instead this
//! renderer projects the same tree as a deterministic **left-rooted indented
//! bracket tree**: the root box at the far left, each child one or more rows
//! to its right, joined by `├`/`└`/`│` connectors — exactly the
//! terminal-native idiom [`crate::Tree`] and the flowchart connectors use, and
//! the only shape that stays legible and snapshot-testable in a TUI.
//!
//! Parsing is lenient: a blank or comment line is skipped, a malformed node
//! shape degrades to its raw text, and a source with no nodes at all renders
//! the shared honest [`super::diagram_placeholder`] rather than panicking.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// The visual shape a mindmap node was written with.
///
/// Mermaid mindmap node syntax wraps the label in a delimiter pair; the
/// delimiter picks the shape. An undelimited line is [`Bare`](Self::Bare)
/// text. Unknown/short delimiters fall back to [`Square`](Self::Square).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// `id[Square]` — a square-cornered box.
    Square,
    /// `id(Round)` — a rounded box.
    Round,
    /// `id((Circle))` / `root((Root))` — a doubled "circle" box.
    Circle,
    /// `id{{Hexagon}}` — a hexagon, drawn as a heavy box (terminal proxy).
    Hexagon,
    /// `id))Bang((` — a "bang" cloud burst, drawn heavy.
    Bang,
    /// `id)Cloud(` — a cloud, drawn rounded.
    Cloud,
    /// Bare text with no delimiters — drawn as a plain square box.
    Bare,
}

impl Shape {
    /// The [`BoxStyle`] this shape renders its box with.
    const fn box_style(self) -> BoxStyle {
        match self {
            Self::Square | Self::Bare => BoxStyle::Square,
            Self::Round | Self::Cloud => BoxStyle::Round,
            Self::Circle => BoxStyle::Double,
            Self::Hexagon | Self::Bang => BoxStyle::Heavy,
        }
    }
}

/// One parsed mindmap node: its indentation `depth` (normalised to a rank),
/// the display `label`, its [`Shape`], and its `children` indices in source
/// order.
#[derive(Debug, Clone)]
struct MindNode {
    /// Normalised indentation rank (0 = root); deeper = descendant.
    depth: usize,
    /// The text drawn inside the node's box.
    label: String,
    /// The shape the node was written with.
    shape: Shape,
    /// Indices of this node's children, in source order.
    children: Vec<usize>,
}

/// Strips `\r` and a trailing `%%` comment from a raw line **without**
/// touching leading whitespace — mindmap indentation is significant, so the
/// usual `trim` is deliberately not applied here.
fn clean_line(raw: &str) -> &str {
    let no_cr = raw.strip_suffix('\r').unwrap_or(raw);
    match no_cr.find("%%") {
        Some(i) => &no_cr[..i],
        None => no_cr,
    }
}

/// The number of leading space/tab columns of `line` (a tab counts as one
/// column — Mermaid's own tokenizer treats indentation by relative depth, not
/// absolute width, so only the *ordering* of these counts matters).
fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// Splits the optional `:::class` suffix and any `::icon(...)` decoration off
/// the node body, returning `(body, class)`. The icon is dropped (its glyph
/// is not available in a terminal); the class is returned but not separately
/// themed.
fn split_decorations(s: &str) -> (&str, Option<&str>) {
    let mut body = s;
    // `::icon(fa fa-book)` — drop everything from the icon marker on.
    if let Some(i) = body.find("::icon(") {
        body = body[..i].trim_end();
    }
    // `:::className` — a triple-colon class tag at the end.
    if let Some(i) = body.find(":::") {
        let class = body[i + 3..].split_whitespace().next();
        body = body[..i].trim_end();
        return (body.trim(), class);
    }
    (body.trim(), None)
}

/// Drops one matched pair of surrounding ASCII double quotes, if present.
fn unquote(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_owned()
    } else {
        t.to_owned()
    }
}

/// Parses one node body (already de-indented, decorations stripped) into its
/// `(label, shape)`. Recognises the delimiter pairs Mermaid mindmap allows;
/// an unmatched or empty delimiter degrades to the raw text as a bare node.
fn parse_node_body(body: &str) -> (String, Shape) {
    let b = body.trim();
    if b.is_empty() {
        return (String::new(), Shape::Bare);
    }
    // An optional leading id precedes the delimiter (`root((X))`, `id[X]`);
    // the id ends at the first opening delimiter. No delimiter ⇒ bare text.
    let delim_starts = ['[', '(', '{', ')', '}'];
    let Some(open) = b.find(|c| delim_starts.contains(&c)) else {
        return (b.to_owned(), Shape::Bare);
    };
    let inner = &b[open..];
    // Order matters: test the longest/most specific delimiters first.
    let pairs: [(&str, &str, Shape); 6] = [
        ("((", "))", Shape::Circle),
        ("{{", "}}", Shape::Hexagon),
        ("))", "((", Shape::Bang),
        ("[", "]", Shape::Square),
        (")", "(", Shape::Cloud),
        ("(", ")", Shape::Round),
    ];
    for (lhs, rhs, shape) in pairs {
        if let Some(rest) = inner.strip_prefix(lhs)
            && let Some(label) = rest.strip_suffix(rhs)
            && !label.is_empty()
        {
            return (unquote(label.trim()), shape);
        }
    }
    // A delimiter was present but did not form a known matched pair — keep
    // the whole line as bare text rather than dropping it.
    (b.to_owned(), Shape::Bare)
}

/// Parses `src` into a flat `Vec<MindNode>` in source order with `children`
/// populated by indentation: a line deeper than the running stack top is a
/// child of it; an equal/shallower line pops back to the matching ancestor.
///
/// The first significant line is the `mindmap` header and is consumed. The
/// returned vector is empty when nothing parseable remains.
fn parse(src: &str) -> Vec<MindNode> {
    let mut nodes: Vec<MindNode> = Vec::new();
    // (node_index, indent_columns) of the open ancestors, outermost first.
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut seen_header = false;

    for raw in src.split('\n') {
        let line = clean_line(raw);
        if line.trim().is_empty() {
            continue;
        }
        let indent = indent_of(line);
        let content = line.trim();
        if content.starts_with("%%") {
            continue;
        }
        if !seen_header {
            // The header is the first non-blank line. Mermaid wants it alone
            // on its line; consume just it. If the first line is *not* the
            // keyword, be lenient and treat it as the root anyway.
            if content == "mindmap" || content.starts_with("mindmap ") {
                seen_header = true;
                continue;
            }
            seen_header = true;
        }

        let (body, _class) = split_decorations(content);
        let (label, shape) = parse_node_body(body);
        let label = if label.is_empty() {
            body.to_owned()
        } else {
            label
        };

        // Pop ancestors that are not strictly shallower than this line.
        while let Some(&(_, ind)) = stack.last() {
            if ind >= indent {
                stack.pop();
            } else {
                break;
            }
        }
        let idx = nodes.len();
        let depth = stack.len();
        if let Some(&(parent, _)) = stack.last() {
            nodes[parent].children.push(idx);
        }
        nodes.push(MindNode {
            depth,
            label,
            shape,
            children: Vec::new(),
        });
        stack.push((idx, indent));
    }
    nodes
}

/// The inner text width a node's box gets: its label length, clamped so a
/// very long label cannot blow the layout out (it is ellipsised on draw).
fn label_w(label: &str) -> i32 {
    (label.chars().count() as i32).clamp(1, 24)
}

/// The full outer box width for a label (text + padding + borders).
fn box_w(label: &str) -> i32 {
    label_w(label) + 4
}

/// A laid-out node box: pixel origin, size, and the source node index.
#[derive(Debug, Clone, Copy)]
struct Placed {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    node: usize,
}

/// The mutable layout context threaded through the recursive
/// [`Layouter::place`] pass: the immutable tree + per-depth x columns + the
/// growing leaf-row cursor and the placed-box accumulator.
struct Layouter<'a> {
    /// The parsed nodes (immutable during layout).
    nodes: &'a [MindNode],
    /// The fixed left edge of each depth column.
    col_x: &'a [i32],
    /// Row slots per leaf (3-row box + 1 blank separator).
    row_h: i32,
    /// The next free leaf row; bumped as leaves are placed.
    next_row: i32,
    /// Every laid-out box, in post-order.
    placed: Vec<Placed>,
}

impl Layouter<'_> {
    /// Recursively assigns each subtree a vertical band and a box,
    /// left-rooted: `x` is fixed by `depth` (via `col_x`), every leaf
    /// consumes one `row_h` slot, and a parent is vertically centred across
    /// the span of its children. Returns this subtree's
    /// `(top_mid, bottom_mid)` child mid-rows and records its box.
    fn place(&mut self, idx: usize, depth: usize) -> (i32, i32) {
        let x = self.col_x.get(depth).copied().unwrap_or(0);
        let w = box_w(&self.nodes[idx].label);
        if self.nodes[idx].children.is_empty() {
            let y = self.next_row;
            self.next_row += self.row_h;
            let mid = y + 1;
            self.placed.push(Placed {
                x,
                y,
                w,
                h: 3,
                node: idx,
            });
            return (mid, mid);
        }
        let mut first_c = i32::MAX;
        let mut last_c = 0;
        // `children` is read here while `self` is borrowed mutably for the
        // recursive call; clone the small index list to satisfy the borrow.
        let kids = self.nodes[idx].children.clone();
        for c in kids {
            let (cy0, cy1) = self.place(c, depth + 1);
            first_c = first_c.min(cy0);
            last_c = last_c.max(cy1);
        }
        let mid = (first_c + last_c) / 2;
        self.placed.push(Placed {
            x,
            y: mid - 1,
            w,
            h: 3,
            node: idx,
        });
        (mid, mid)
    }
}

/// Renders a `mindmap` Mermaid diagram from `src` into `area`.
///
/// Builds the indentation tree, lays it out left-to-right with one row slot
/// per leaf, draws every node as a shaped box, then joins each parent to its
/// children with `├`/`└`/`│` connectors before a single centred blit. An
/// empty/header-only source falls back to the shared placeholder.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let nodes = parse(src);
    if nodes.is_empty() {
        super::diagram_placeholder("mindmap", "no nodes", area, buf, base, theme);
        return;
    }

    // Column x of each depth = previous column right edge + connector gap.
    let max_depth = nodes.iter().map(|n| n.depth).max().unwrap_or(0);
    let mut col_x = Vec::with_capacity(max_depth + 1);
    let mut x = 0;
    for d in 0..=max_depth {
        col_x.push(x);
        let wmax = nodes
            .iter()
            .filter(|n| n.depth == d)
            .map(|n| box_w(&n.label))
            .max()
            .unwrap_or(6);
        x += wmax + 3; // +3 columns for the connector elbow + a gap.
    }

    // Roots are every depth-0 node (a well-formed mindmap has exactly one,
    // but be lenient about a forest).
    let roots: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].depth == 0).collect();
    let mut layouter = Layouter {
        nodes: &nodes,
        col_x: &col_x,
        row_h: 4, // 3-row box + 1 blank separator row.
        next_row: 0,
        placed: Vec::new(),
    };
    for &r in &roots {
        layouter.place(r, 0);
    }
    let placed = layouter.placed;
    if placed.is_empty() {
        super::diagram_placeholder("mindmap", "no nodes", area, buf, base, theme);
        return;
    }

    let sw = placed.iter().map(|p| p.x + p.w).max().unwrap_or(1).max(1);
    let sh = placed.iter().map(|p| p.y + p.h).max().unwrap_or(1).max(1);
    let mut s = Surface::new(sw, sh);
    // A degenerate (zero-area) layout cannot draw anything legible — fall
    // back to the honest placeholder instead of blitting an empty surface.
    if s.width() == 0 || s.height() == 0 {
        super::diagram_placeholder("mindmap", "no nodes", area, buf, base, theme);
        return;
    }

    let border = base.patch(theme.node_border);
    let label_st = base.patch(theme.node_label);
    let edge = base.patch(theme.edge);
    let root_st = base.patch(theme.cluster);

    // source node index -> its placed box.
    let mut box_of = vec![None; nodes.len()];
    for p in &placed {
        box_of[p.node] = Some(*p);
    }

    // Connectors first so boxes paint cleanly over any overlap.
    for p in &placed {
        let kids = &nodes[p.node].children;
        if kids.is_empty() {
            continue;
        }
        let stub_x = p.x + p.w; // one column right of the parent box.
        let trunk_x = stub_x + 1; // the vertical bus column.
        let parent_mid = p.y + p.h / 2;
        let child_boxes: Vec<Placed> = kids.iter().filter_map(|&c| box_of[c]).collect();
        if child_boxes.is_empty() {
            continue;
        }
        let mids: Vec<i32> = child_boxes.iter().map(|cb| cb.y + cb.h / 2).collect();
        let top = *mids.iter().min().unwrap();
        let bot = *mids.iter().max().unwrap();

        // A short stub out of the parent's right side at its mid row.
        s.set(stub_x, parent_mid, '─', edge);
        // The vertical trunk spanning the children's mid rows.
        for y in top..=bot {
            s.set(trunk_x, y, '│', edge);
        }
        // The parent stub joins the trunk with a tee.
        if (top..=bot).contains(&parent_mid) {
            s.set(trunk_x, parent_mid, '├', edge);
        } else {
            // Parent is above/below every child: extend the trunk to reach
            // the parent's mid row so the connector is unbroken.
            let (a, b) = if parent_mid < top {
                (parent_mid, top)
            } else {
                (bot, parent_mid)
            };
            for y in a..=b {
                s.set(trunk_x, y, '│', edge);
            }
        }
        // Each child gets an elbow off the trunk and a horizontal lead-in.
        for cb in &child_boxes {
            let cy = cb.y + cb.h / 2;
            let elbow = if cy == top && cy == bot {
                '─'
            } else if cy == top {
                '┌'
            } else if cy == bot {
                '└'
            } else {
                '├'
            };
            let g = if cy == parent_mid && !(cy == top && cy == bot) {
                '┼'
            } else {
                elbow
            };
            s.set(trunk_x, cy, g, edge);
            for hx in (trunk_x + 1)..cb.x {
                s.set(hx, cy, '─', edge);
            }
        }
    }

    // Boxes on top.
    for p in &placed {
        let n = &nodes[p.node];
        let (bd, tx) = if n.depth == 0 {
            (root_st, root_st)
        } else {
            (border, label_st)
        };
        s.labeled_box(p.x, p.y, p.w, p.h, n.shape.box_style(), &n.label, bd, tx);
    }

    s.blit(area, buf, base);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Position;

    /// Renders `src` as a `mindmap` into a fresh `w`×`h` buffer and returns
    /// the glyphs as one newline-terminated line per row (the shared mermaid
    /// snapshot idiom).
    fn lines(src: &str, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        render(
            src,
            buf.area(),
            &mut buf,
            Style::new(),
            &MermaidTheme::default(),
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

    /// Each [`Shape`] maps to the documented [`BoxStyle`] glyph family —
    /// asserted directly on a [`Surface`] via the shared `draw::dump` idiom.
    #[test]
    fn shape_box_style_glyph_families_are_correct() {
        let cases = [
            (Shape::Square, '┌'),
            (Shape::Bare, '┌'),
            (Shape::Round, '╭'),
            (Shape::Cloud, '╭'),
            (Shape::Circle, '╔'),
            (Shape::Hexagon, '┏'),
            (Shape::Bang, '┏'),
        ];
        for (shape, top_left) in cases {
            let mut s = Surface::new(5, 3);
            s.labeled_box(
                0,
                0,
                5,
                3,
                shape.box_style(),
                "x",
                Style::new(),
                Style::new(),
            );
            let dumped = super::super::draw::dump(&s);
            assert_eq!(
                dumped.chars().next(),
                Some(top_left),
                "shape {shape:?} should start with {top_left:?}, got:\n{dumped}"
            );
        }
    }

    // --- parser tests ------------------------------------------------------

    #[test]
    fn header_is_consumed_and_root_parsed() {
        let n = parse("mindmap\n  root((Root))");
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].label, "Root");
        assert_eq!(n[0].shape, Shape::Circle);
        assert_eq!(n[0].depth, 0);
    }

    #[test]
    fn indentation_builds_parent_child_tree() {
        let n = parse("mindmap\nroot\n  A\n    A1\n  B");
        assert_eq!(n.len(), 4);
        assert_eq!(n[0].label, "root");
        assert_eq!(n[0].children, vec![1, 3]); // A and B
        assert_eq!(n[1].label, "A");
        assert_eq!(n[1].children, vec![2]); // A1
        assert_eq!(n[2].depth, 2);
        assert_eq!(n[3].label, "B");
        assert_eq!(n[3].depth, 1);
        assert!(n[3].children.is_empty());
    }

    #[test]
    fn every_node_shape_is_recognised() {
        let src = "mindmap\n\
             root((R))\n\
               a[Square]\n\
               b(Round)\n\
               c{{Hex}}\n\
               d))Bang((\n\
               e)Cloud(\n\
               f bare text";
        let n = parse(src);
        assert_eq!(n[0].shape, Shape::Circle);
        assert_eq!(n[1].shape, Shape::Square);
        assert_eq!(n[1].label, "Square");
        assert_eq!(n[2].shape, Shape::Round);
        assert_eq!(n[3].shape, Shape::Hexagon);
        assert_eq!(n[3].label, "Hex");
        assert_eq!(n[4].shape, Shape::Bang);
        assert_eq!(n[4].label, "Bang");
        assert_eq!(n[5].shape, Shape::Cloud);
        assert_eq!(n[5].label, "Cloud");
        assert_eq!(n[6].shape, Shape::Bare);
        assert_eq!(n[6].label, "f bare text");
    }

    #[test]
    fn class_and_icon_decorations_are_stripped() {
        let n = parse("mindmap\nroot\n  Idea :::urgent ::icon(fa fa-book)");
        assert_eq!(n.len(), 2);
        assert_eq!(n[1].label, "Idea");
    }

    #[test]
    fn quoted_label_keeps_inner_punctuation() {
        let n = parse("mindmap\nroot[\"a, b: c\"]");
        assert_eq!(n[0].label, "a, b: c");
    }

    #[test]
    fn comment_and_blank_lines_are_skipped() {
        let n = parse("mindmap\n\nroot\n  %% a comment\n  Child");
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].label, "root");
        assert_eq!(n[1].label, "Child");
    }

    #[test]
    fn deeper_then_shallower_pops_to_correct_ancestor() {
        // root > a > a1 ; then b at root depth, c under b.
        let n = parse("mindmap\nroot\n    a\n        a1\n    b\n        c");
        assert_eq!(n[0].children, vec![1, 3]);
        assert_eq!(n[1].children, vec![2]);
        assert_eq!(n[3].children, vec![4]);
        assert_eq!(n[4].depth, 2);
    }

    #[test]
    fn missing_explicit_keyword_is_lenient() {
        // No `mindmap` header — first line still becomes the root.
        let n = parse("root\n  Child");
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].label, "root");
        assert_eq!(n[1].label, "Child");
    }

    // --- render snapshot tests --------------------------------------------

    #[test]
    fn empty_source_renders_placeholder() {
        let out = lines("mindmap\n", 40, 3);
        assert!(out.contains("mermaid"), "got:\n{out}");
        assert!(out.contains("mindmap"), "got:\n{out}");
        assert!(out.contains("no nodes"), "got:\n{out}");
    }

    #[test]
    fn whitespace_only_source_renders_placeholder() {
        let out = lines("mindmap\n   \n  %% c\n", 40, 3);
        assert!(out.contains("no nodes"), "got:\n{out}");
    }

    #[test]
    fn single_root_renders_one_double_box() {
        let out = lines("mindmap\nroot((Hi))", 12, 5);
        // A doubled-line box (Circle shape).
        assert!(out.contains('╔'), "got:\n{out}");
        assert!(out.contains('╗'), "got:\n{out}");
        assert!(out.contains("Hi"), "got:\n{out}");
    }

    #[test]
    fn root_with_two_children_draws_bracket_connectors() {
        let out = lines("mindmap\nroot((R))\n  A\n  B", 28, 9);
        assert!(out.contains('R'), "got:\n{out}");
        assert!(out.contains('A'), "got:\n{out}");
        assert!(out.contains('B'), "got:\n{out}");
        assert!(
            out.contains('│') || out.contains('├') || out.contains('┌'),
            "expected connectors, got:\n{out}"
        );
        // Children are square boxes.
        assert!(out.contains('┌') && out.contains('┐'), "got:\n{out}");
        // Root is a doubled box.
        assert!(out.contains('╔'), "got:\n{out}");
    }

    #[test]
    fn exact_render_root_two_leaves() {
        // Full deterministic snapshot of the canonical small mindmap.
        let out = lines("mindmap\nR\n  A\n  B", 22, 9);
        let expected = [
            "                      ",
            "            ┌───┐     ",
            "          ┌─│ A │     ",
            "    ┌───┐ │ └───┘     ",
            "    │ R │─├           ",
            "    └───┘ │ ┌───┐     ",
            "          └─│ B │     ",
            "            └───┘     ",
            "                      ",
        ]
        .join("\n")
            + "\n";
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn tiny_area_clips_without_panic() {
        // Far too small for the tree — must clip, not panic.
        let out = lines("mindmap\nroot\n  child\n    grand", 4, 2);
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn deep_chain_indents_each_level_rightward() {
        let out = lines("mindmap\nA\n  B\n    C\n      D", 48, 7);
        // Each successive label appears strictly further right.
        let col_of = |needle: char| {
            out.lines()
                .find_map(|l| l.chars().position(|c| c == needle).map(|c| c as i32))
        };
        let (a, b, c, d) = (
            col_of('A').unwrap(),
            col_of('B').unwrap(),
            col_of('C').unwrap(),
            col_of('D').unwrap(),
        );
        assert!(a < b && b < c && c < d, "a={a} b={b} c={c} d={d}\n{out}");
    }
}
