//! `block-beta` Mermaid diagram renderer.
//!
//! Mermaid's *block* diagram packs labelled boxes into a fixed-width grid and
//! optionally connects their centres with arrows. The dispatcher in [`super`]
//! routes a `block-beta` (or `block`) source here; this module owns the
//! hand-written line parser, a deterministic row-major integer grid layout,
//! and the [`Surface`] render that the shared blit centres into `area`.
//!
//! # Supported subset
//!
//! * `columns N` — sets the grid width (cells per row); defaults to `1`.
//! * A block declaration token: a bare id `A`, a labelled square
//!   `B["Label"]`, a round node `id(("round"))`, a diamond `C{"diamond"}`.
//! * `space` / `space:2` — one (or `N`) empty grid cells.
//! * `A:2` — a block that spans `2` columns of the grid.
//! * `block:groupId:2 ... end` — a nested sub-grid drawn as a titled bordered
//!   region (optionally spanning `2` columns).
//! * `A --> B` / `A -- "label" --> B` — an arrow between two block centres
//!   with an optional mid-point label.
//!
//! # Leniency
//!
//! Every parse step is best-effort: an unrecognised line is skipped, never a
//! panic. A source that yields no blocks falls back to
//! [`super::diagram_placeholder`]. Layout is integer-only and follows source
//! order so the rendered image is a stable snapshot.

use std::collections::HashMap;

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// The shape a block declaration asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// `A` / `A["x"]` — a square box.
    Square,
    /// `A(("x"))` — rounded corners.
    Round,
    /// `A{"x"}` — a diamond (drawn square with a `◇` marker prefix).
    Diamond,
}

/// One laid-out leaf block.
#[derive(Debug, Clone)]
struct Block {
    /// The id used by arrow endpoints.
    id: String,
    /// The display label (defaults to the id).
    label: String,
    /// The requested shape.
    shape: Shape,
    /// Column span in grid cells (`>= 1`).
    span: i32,
    /// Grid column of the block's left edge.
    col: i32,
    /// Grid row.
    row: i32,
}

/// A nested `block:id:span ... end` group: its own grid of child blocks drawn
/// in a titled bordered region.
#[derive(Debug, Clone)]
struct Group {
    /// The group title (its id).
    title: String,
    /// Column span of the group in the *outer* grid.
    span: i32,
    /// Grid column of the group's left edge in the outer grid.
    col: i32,
    /// Grid row in the outer grid.
    row: i32,
    /// The group's own inner column count.
    inner_columns: i32,
    /// Leaf blocks placed inside the group's inner grid.
    children: Vec<Block>,
}

/// An arrow between two block ids with an optional mid-label.
#[derive(Debug, Clone)]
struct Arrow {
    /// Source block id.
    from: String,
    /// Target block id.
    to: String,
    /// Optional label drawn at the arrow's midpoint.
    label: Option<String>,
}

/// The whole parsed diagram.
#[derive(Debug, Default)]
struct Diagram {
    /// Top-level leaf blocks (placed in the outer grid).
    blocks: Vec<Block>,
    /// Top-level groups (placed in the outer grid).
    groups: Vec<Group>,
    /// Arrows between any two ids.
    arrows: Vec<Arrow>,
    /// Outer grid width in columns.
    columns: i32,
}

/// Strips a Mermaid preamble from `src`, yielding the significant body lines
/// (trimmed, `\r` removed, no frontmatter / `%%` comments / header).
fn body_lines(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_front = false;
    let mut seen_header = false;
    for raw in src.split('\n') {
        let line = raw.trim_end_matches('\r').trim();
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
        if !seen_header {
            // The `block-beta` / `block` keyword line (possibly with trailing
            // tokens we ignore).
            seen_header = true;
            let word: String = line.chars().take_while(|c| !c.is_whitespace()).collect();
            if word == "block-beta" || word == "block" {
                continue;
            }
            // No recognised header — treat this as a body line anyway.
        }
        out.push(line.to_string());
    }
    out
}

/// Splits a body line into block-declaration tokens, keeping `["..."]`,
/// `("...")`, `{"..."}` and `(("..."))` groups intact (whitespace inside a
/// bracket does not split). Arrow operators come back as their own tokens.
fn tokenize(line: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for c in line.chars() {
        match c {
            '[' | '(' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ']' | ')' | '}' => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    toks.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    toks
}

/// Returns `true` if `tok` is an arrow operator (`-->`, `---`, `-.->`, `==>`,
/// `--x`, `--o`, …).
fn is_arrow(tok: &str) -> bool {
    let t = tok.trim();
    if t.chars().count() < 2 {
        return false;
    }
    t.chars()
        .all(|c| matches!(c, '-' | '>' | '<' | '.' | '=' | 'o' | 'x' | '~'))
        && t.contains(['-', '=', '~'])
}

/// Parses a single block token into `(id, label, shape, span)`.
///
/// Recognises `id`, `id["label"]`, `id(("label"))`, `id{"label"}` and a
/// trailing `:N` span (`A:2`, `B["x"]:3`). Quotes around a label are stripped.
fn parse_block_token(tok: &str) -> Option<(String, String, Shape, i32)> {
    let tok = tok.trim();
    if tok.is_empty() {
        return None;
    }
    let chars: Vec<char> = tok.chars().collect();
    let open = chars.iter().position(|&c| matches!(c, '[' | '(' | '{'));
    match open {
        None => {
            // `id` or `id:span`.
            let id: String = chars.iter().collect();
            if let Some((base, n)) = split_span(&id) {
                Some((base.clone(), base, Shape::Square, n))
            } else {
                Some((id.clone(), id, Shape::Square, 1))
            }
        }
        Some(o) => {
            let id: String = chars[..o].iter().collect();
            let id = id.trim().to_string();
            if id.is_empty() {
                return None;
            }
            let rest: String = chars[o..].iter().collect();
            let (inner, shape, after) = strip_shape(&rest)?;
            let label = if inner.is_empty() {
                id.clone()
            } else {
                unquote(&inner)
            };
            let span = after
                .trim()
                .strip_prefix(':')
                .and_then(|s| s.trim().parse::<i32>().ok())
                .filter(|n| *n >= 1)
                .unwrap_or(1);
            Some((id, label, shape, span))
        }
    }
}

/// Splits a trailing `:N` off `id`, returning `(id, n)` when present and a
/// positive integer.
fn split_span(id: &str) -> Option<(String, i32)> {
    let (lhs, rhs) = id.rsplit_once(':')?;
    let n: i32 = rhs.trim().parse().ok()?;
    if n < 1 || lhs.is_empty() {
        return None;
    }
    Some((lhs.to_string(), n))
}

/// Given the bracketed remainder (starting at the opener), returns the inner
/// text, the [`Shape`] and any text after the matching close (e.g. `:2`).
fn strip_shape(rest: &str) -> Option<(String, Shape, String)> {
    let chars: Vec<char> = rest.chars().collect();
    let first = *chars.first()?;
    let (shape, want_close) = match first {
        '[' => (Shape::Square, ']'),
        '(' => (Shape::Round, ')'),
        '{' => (Shape::Diamond, '}'),
        _ => return None,
    };
    // The last matching close keeps `(("x"))`'s inner quotes intact.
    let close_idx = chars.iter().rposition(|&c| c == want_close)?;
    if close_idx == 0 {
        return None;
    }
    let inner: String = chars[1..close_idx].iter().collect();
    let after: String = chars[close_idx + 1..].iter().collect();
    // Trim a doubled bracket layer: `("x")` inside `(("x"))`.
    let inner = inner
        .trim()
        .trim_start_matches(['(', '[', '{'])
        .trim_end_matches([')', ']', '}'])
        .to_string();
    Some((inner, shape, after))
}

/// Removes one layer of surrounding single/double quotes (and trims).
fn unquote(s: &str) -> String {
    let t = s.trim();
    let t = t
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| t.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(t);
    t.trim().to_string()
}

/// Parses `block-beta` body lines into a [`Diagram`].
fn parse(src: &str) -> Diagram {
    let mut d = Diagram {
        columns: 1,
        ..Diagram::default()
    };
    let lines = body_lines(src);
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        i += 1;
        let toks = tokenize(line);
        if toks.is_empty() {
            continue;
        }
        if toks[0] == "columns" {
            if let Some(n) = toks.get(1).and_then(|s| s.parse::<i32>().ok()) {
                d.columns = n.max(1);
            }
            continue;
        }
        if toks[0] == "end" {
            continue;
        }
        if let Some(spec) = toks[0].strip_prefix("block:") {
            // `block:groupId:span` — collect until a matching `end`.
            let (gid, gspan) = match spec.rsplit_once(':') {
                Some((a, b)) if b.parse::<i32>().is_ok() && !a.is_empty() => {
                    (a.to_string(), b.parse::<i32>().unwrap_or(1).max(1))
                }
                _ => (spec.to_string(), 1),
            };
            let mut inner_lines: Vec<String> = Vec::new();
            let mut inner_cols = 1i32;
            while i < lines.len() {
                let l = &lines[i];
                i += 1;
                let lt = tokenize(l);
                if lt.first().map(String::as_str) == Some("end") {
                    break;
                }
                if lt.first().map(String::as_str) == Some("columns") {
                    if let Some(n) = lt.get(1).and_then(|s| s.parse::<i32>().ok()) {
                        inner_cols = n.max(1);
                    }
                    continue;
                }
                inner_lines.push(l.clone());
            }
            let children = parse_blocks(&inner_lines);
            d.groups.push(Group {
                title: gid,
                span: gspan.max(1),
                col: 0,
                row: 0,
                inner_columns: inner_cols,
                children,
            });
            continue;
        }
        // Arrow line `A --> B` / `A -- "x" --> B`?
        if toks.iter().any(|t| is_arrow(t)) {
            collect_arrows(&toks, &mut d);
            continue;
        }
        // Otherwise: a row of block declarations.
        d.blocks.extend(parse_blocks(std::slice::from_ref(line)));
    }
    place(&mut d);
    d
}

/// Parses every block token across `lines` into ordered [`Block`]s, treating
/// `space` / `space:N` as that many blank cells (span carried, empty id).
fn parse_blocks(lines: &[String]) -> Vec<Block> {
    let mut out = Vec::new();
    for line in lines {
        for tok in tokenize(line) {
            if is_arrow(&tok) {
                continue;
            }
            if tok == "space" || tok.starts_with("space:") {
                let n = tok
                    .strip_prefix("space:")
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(1)
                    .max(1);
                out.push(Block {
                    id: String::new(),
                    label: String::new(),
                    shape: Shape::Square,
                    span: n,
                    col: 0,
                    row: 0,
                });
                continue;
            }
            if let Some((id, label, shape, span)) = parse_block_token(&tok) {
                out.push(Block {
                    id,
                    label,
                    shape,
                    span: span.max(1),
                    col: 0,
                    row: 0,
                });
            }
        }
    }
    out
}

/// Pulls `A -- "label" --> B` (and bare `A --> B`) endpoints out of a token
/// row, appending the arrow.
fn collect_arrows(toks: &[String], d: &mut Diagram) {
    let mut idx = 0;
    while idx < toks.len() {
        if is_arrow(&toks[idx]) {
            idx += 1;
            continue;
        }
        if idx + 1 < toks.len() && is_arrow(&toks[idx + 1]) {
            let from = bare_id(&toks[idx]);
            let mut j = idx + 1;
            let mut label = None;
            // `A -- "x" --> B`: label sits between two arrow tokens.
            if j + 2 < toks.len() && is_arrow(&toks[j]) && is_arrow(&toks[j + 2]) {
                label = Some(unquote(&toks[j + 1]));
                j += 3;
            } else {
                j += 1;
            }
            if let Some(t) = toks.get(j) {
                let to = bare_id(t);
                if !from.is_empty() && !to.is_empty() {
                    d.arrows.push(Arrow { from, to, label });
                }
            }
            // Resume *at* the target token so the same chain isn't re-scanned
            // (a quoted label would otherwise read as a spurious endpoint).
            idx = j;
            continue;
        }
        idx += 1;
    }
}

/// The id portion of a token (drops any `["label"]` / span so an arrow
/// endpoint matches a declared block).
fn bare_id(tok: &str) -> String {
    parse_block_token(tok)
        .map(|(id, ..)| id)
        .unwrap_or_default()
}

/// Assigns `(col, row)` to every top-level block then group by packing them
/// row-major into `columns`-wide rows, honouring spans and `space`.
fn place(d: &mut Diagram) {
    let cols = d.columns.max(1);
    let mut col = 0i32;
    let mut row = 0i32;
    let mut step = |span: i32, slot: &mut (i32, i32)| {
        let span = span.clamp(1, cols);
        if col + span > cols {
            col = 0;
            row += 1;
        }
        *slot = (col, row);
        col += span;
        if col >= cols {
            col = 0;
            row += 1;
        }
    };
    for b in &mut d.blocks {
        let mut slot = (0, 0);
        step(b.span, &mut slot);
        b.col = slot.0;
        b.row = slot.1;
    }
    for g in &mut d.groups {
        let mut slot = (0, 0);
        step(g.span, &mut slot);
        g.col = slot.0;
        g.row = slot.1;
    }
}

/// Fixed per-cell box geometry on the [`Surface`] grid.
const CELL_W: i32 = 14;
/// Per-cell box height (a border row, a label row, a border row).
const CELL_H: i32 = 3;
/// Horizontal gap between grid cells.
const GAP_X: i32 = 2;
/// Vertical gap between grid rows.
const GAP_Y: i32 = 1;

/// The pixel `x` of grid column `col`.
const fn col_x(col: i32) -> i32 {
    col * (CELL_W + GAP_X)
}

/// The pixel width spanned by `span` grid columns.
const fn span_w(span: i32) -> i32 {
    span * CELL_W + (span - 1) * GAP_X
}

/// The number of inner grid rows a group's children occupy.
fn group_rows(g: &Group) -> i32 {
    let n = g.children.len() as i32;
    let ic = g.inner_columns.max(1);
    (n + ic - 1) / ic
}

/// Renders a `block-beta` Mermaid diagram from `src` into `area`.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    if area.is_empty() {
        return;
    }
    let d = parse(src);
    if d.blocks.is_empty() && d.groups.is_empty() {
        super::diagram_placeholder("block", "no blocks", area, buf, base, theme);
        return;
    }

    let max_row = d
        .blocks
        .iter()
        .map(|b| b.row)
        .chain(d.groups.iter().map(|g| g.row))
        .max()
        .unwrap_or(0);
    let cols = d.columns.max(1);

    // A group row needs extra height for its inner grid + title border.
    let row_extra: Vec<i32> = (0..=max_row)
        .map(|r| {
            d.groups
                .iter()
                .filter(|g| g.row == r)
                .map(|g| (group_rows(g) * (CELL_H + GAP_Y) + 1 - CELL_H).max(0))
                .max()
                .unwrap_or(0)
        })
        .collect();

    let grid_w = cols * CELL_W + (cols - 1) * GAP_X;
    let grid_h: i32 = (0..=max_row)
        .map(|r| CELL_H + GAP_Y + row_extra[r as usize])
        .sum::<i32>()
        - GAP_Y;
    let w = grid_w.max(2);
    let h = grid_h.max(2);
    let mut s = Surface::new(w, h);

    let border = base.patch(theme.node_border);
    let label = base.patch(theme.node_label);
    let cluster = base.patch(theme.cluster);
    let edge = base.patch(theme.edge);
    let elabel = base.patch(theme.edge_label);

    // Y offset for each grid row, accounting for tall group rows.
    let row_y: Vec<i32> = {
        let mut ys = Vec::with_capacity((max_row + 1) as usize);
        let mut y = 0;
        for r in 0..=max_row {
            ys.push(y);
            y += CELL_H + GAP_Y + row_extra[r as usize];
        }
        ys
    };

    // Each block id's centre, for arrow routing.
    let mut centers: HashMap<String, (i32, i32)> = HashMap::new();

    for b in &d.blocks {
        let x = col_x(b.col);
        let y = row_y[b.row as usize];
        let bw = span_w(b.span.clamp(1, cols));
        let kind = match b.shape {
            Shape::Square | Shape::Diamond => BoxStyle::Square,
            Shape::Round => BoxStyle::Round,
        };
        let text = if b.shape == Shape::Diamond {
            format!("◇ {}", b.label)
        } else {
            b.label.clone()
        };
        s.labeled_box(x, y, bw, CELL_H, kind, &text, border, label);
        if !b.id.is_empty() {
            centers.insert(b.id.clone(), (x + bw / 2, y + CELL_H / 2));
        }
    }

    for g in &d.groups {
        let x = col_x(g.col);
        let y = row_y[g.row as usize];
        let gw = span_w(g.span.clamp(1, cols));
        let gh = (group_rows(g) * (CELL_H + GAP_Y) - GAP_Y + 2).max(CELL_H + 2);
        s.rect(x, y, gw, gh, BoxStyle::Round, cluster);
        s.text_clipped(x + 2, y, &g.title, gw - 4, cluster);
        let ic = g.inner_columns.max(1);
        let inner_cell_w = ((gw - 4) / ic).max(2);
        for (k, c) in g.children.iter().enumerate() {
            let cc = k as i32 % ic;
            let cr = k as i32 / ic;
            let cx = x + 2 + cc * inner_cell_w;
            let cy = y + 1 + cr * (CELL_H + GAP_Y);
            let kind = match c.shape {
                Shape::Round => BoxStyle::Round,
                _ => BoxStyle::Square,
            };
            let bw = (inner_cell_w - 1).max(2);
            s.labeled_box(cx, cy, bw, CELL_H, kind, &c.label, border, label);
            if !c.id.is_empty() {
                centers.insert(c.id.clone(), (cx + bw / 2, cy + CELL_H / 2));
            }
        }
    }

    for a in &d.arrows {
        let (Some(&(fx, fy)), Some(&(tx, ty))) = (centers.get(&a.from), centers.get(&a.to)) else {
            continue;
        };
        draw_arrow(&mut s, fx, fy, tx, ty, edge);
        if let Some(lbl) = &a.label {
            let mx = (fx + tx) / 2;
            let my = (fy + ty) / 2;
            s.text(mx - lbl.chars().count() as i32 / 2, my, lbl, elabel);
        }
    }

    s.blit(area, buf, base);
}

/// Draws a simple orthogonal connector from `(fx, fy)` to `(tx, ty)` with an
/// arrowhead at the target. A horizontal leg then a vertical leg keeps the
/// integer routing deterministic and legible on the grid.
fn draw_arrow(s: &mut Surface, fx: i32, fy: i32, tx: i32, ty: i32, style: Style) {
    if fx == tx && fy == ty {
        return;
    }
    let (x0, x1) = (fx.min(tx), fx.max(tx));
    for x in x0..=x1 {
        if s.glyph(x, fy) == ' ' {
            s.set(x, fy, '─', style);
        }
    }
    let (y0, y1) = (fy.min(ty), fy.max(ty));
    for y in y0..=y1 {
        let g = s.glyph(tx, y);
        if g == ' ' {
            s.set(tx, y, '│', style);
        } else if g == '─' {
            s.set(tx, y, '┼', style);
        }
    }
    let head = if ty == fy {
        if tx >= fx { '▶' } else { '◀' }
    } else if ty > fy {
        '▼'
    } else {
        '▲'
    };
    if ty == fy {
        let hx = if tx >= fx { tx - 1 } else { tx + 1 };
        s.set(hx, ty, head, style);
    } else {
        let hy = if ty > fy { ty - 1 } else { ty + 1 };
        s.set(tx, hy, head, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mermaid;
    use rstui_core::{Position, Widget};

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

    // --- Parser -----------------------------------------------------------

    #[test]
    fn parses_columns_and_simple_blocks() {
        let d = parse("block-beta\ncolumns 3\nA B C");
        assert_eq!(d.columns, 3);
        assert_eq!(d.blocks.len(), 3);
        assert_eq!(d.blocks[0].id, "A");
        assert_eq!(d.blocks[1].id, "B");
        assert_eq!(d.blocks[2].id, "C");
        assert_eq!((d.blocks[0].col, d.blocks[0].row), (0, 0));
        assert_eq!((d.blocks[1].col, d.blocks[1].row), (1, 0));
        assert_eq!((d.blocks[2].col, d.blocks[2].row), (2, 0));
    }

    #[test]
    fn wraps_into_grid_rows() {
        let d = parse("block-beta\ncolumns 2\nA B C D E");
        assert_eq!((d.blocks[2].col, d.blocks[2].row), (0, 1));
        assert_eq!((d.blocks[4].col, d.blocks[4].row), (0, 2));
    }

    #[test]
    fn parses_labels_and_shapes() {
        let d = parse("block-beta\nA[\"Hello\"]\nB((\"round\"))\nC{\"diamond\"}");
        assert_eq!(d.blocks[0].label, "Hello");
        assert_eq!(d.blocks[0].shape, Shape::Square);
        assert_eq!(d.blocks[1].label, "round");
        assert_eq!(d.blocks[1].shape, Shape::Round);
        assert_eq!(d.blocks[2].label, "diamond");
        assert_eq!(d.blocks[2].shape, Shape::Diamond);
    }

    #[test]
    fn parses_span_and_space() {
        let d = parse("block-beta\ncolumns 3\nA:2 space\nB space:2");
        assert_eq!(d.blocks[0].id, "A");
        assert_eq!(d.blocks[0].span, 2);
        assert!(d.blocks[1].id.is_empty());
        assert_eq!(d.blocks[1].span, 1);
        assert_eq!(d.blocks[2].id, "B");
        assert!(d.blocks[3].id.is_empty());
        assert_eq!(d.blocks[3].span, 2);
    }

    #[test]
    fn span_pushes_next_block_to_new_row() {
        let d = parse("block-beta\ncolumns 2\nA:2\nB");
        assert_eq!((d.blocks[0].col, d.blocks[0].row), (0, 0));
        assert_eq!(d.blocks[1].row, 1);
    }

    #[test]
    fn parses_arrows_with_and_without_label() {
        let d = parse("block-beta\nA\nB\nA --> B\nA -- \"yes\" --> B");
        assert_eq!(d.arrows.len(), 2);
        assert_eq!(d.arrows[0].from, "A");
        assert_eq!(d.arrows[0].to, "B");
        assert_eq!(d.arrows[0].label, None);
        assert_eq!(d.arrows[1].label.as_deref(), Some("yes"));
    }

    #[test]
    fn parses_group_block() {
        let d = parse("block-beta\ncolumns 1\nblock:grp:1\n  columns 2\n  X\n  Y\nend\nZ");
        assert_eq!(d.groups.len(), 1);
        assert_eq!(d.groups[0].title, "grp");
        assert_eq!(d.groups[0].inner_columns, 2);
        assert_eq!(d.groups[0].children.len(), 2);
        assert_eq!(d.groups[0].children[0].id, "X");
        assert_eq!(d.blocks.len(), 1);
        assert_eq!(d.blocks[0].id, "Z");
    }

    #[test]
    fn skips_frontmatter_comments_and_blank_lines() {
        let d = parse("---\ntitle: T\n---\nblock-beta\n%% a comment\n\nA\nB");
        assert_eq!(d.blocks.len(), 2);
        assert_eq!(d.blocks[0].id, "A");
    }

    #[test]
    fn lenient_on_garbage_lines_no_panic() {
        let d = parse("block-beta\n!!! &&&\nA\n%%{init: {}}%%\nB");
        assert!(d.blocks.iter().any(|b| b.id == "A"));
        assert!(d.blocks.iter().any(|b| b.id == "B"));
    }

    #[test]
    fn header_optional_is_lenient() {
        // No `block-beta` header at all — first line is still a block.
        let d = parse("A B");
        assert_eq!(d.blocks.len(), 2);
    }

    // --- Render snapshots -------------------------------------------------

    #[test]
    fn empty_source_renders_placeholder() {
        let out = lines(Mermaid::new("block-beta"), 40, 3);
        assert!(out.contains("mermaid"), "{out}");
        assert!(out.contains("block"));
    }

    #[test]
    fn nothing_parseable_renders_placeholder() {
        let out = lines(Mermaid::new("block-beta\ncolumns 2"), 40, 3);
        assert!(out.contains("no blocks"), "{out}");
    }

    #[test]
    fn single_block_snapshot() {
        let out = lines(Mermaid::new("block-beta\nA[\"Hi\"]"), 18, 5);
        let expect = [
            "                  ",
            "  ┌────────────┐  ",
            "  │     Hi     │  ",
            "  └────────────┘  ",
            "                  ",
            "",
        ]
        .join("\n");
        assert_eq!(out, expect, "got:\n{out}");
    }

    #[test]
    fn round_block_snapshot() {
        let out = lines(Mermaid::new("block-beta\nA((\"R\"))"), 18, 5);
        assert!(out.contains('╭'), "{out}");
        assert!(out.contains('╮'));
        assert!(out.contains('R'));
    }

    #[test]
    fn diamond_block_has_marker() {
        let out = lines(Mermaid::new("block-beta\nA{\"D\"}"), 18, 5);
        assert!(out.contains('◇'), "{out}");
        assert!(out.contains('D'));
    }

    #[test]
    fn two_columns_render_side_by_side() {
        let out = lines(Mermaid::new("block-beta\ncolumns 2\nA B"), 32, 5);
        let box_top = out.lines().find(|r| r.contains('┌')).unwrap();
        assert_eq!(box_top.matches('┌').count(), 2, "{out}");
        assert!(out.contains('A') && out.contains('B'));
    }

    #[test]
    fn arrow_connects_two_blocks() {
        let out = lines(
            Mermaid::new("block-beta\ncolumns 2\nA[\"A\"] B[\"B\"]\nA --> B"),
            34,
            6,
        );
        assert!(
            out.contains('▶') || out.contains('◀') || out.contains('▼') || out.contains('▲'),
            "expected an arrowhead in:\n{out}"
        );
    }

    #[test]
    fn labeled_arrow_draws_label() {
        let out = lines(
            Mermaid::new("block-beta\ncolumns 1\nA[\"A\"]\nB[\"B\"]\nA -- \"go\" --> B"),
            20,
            9,
        );
        assert!(out.contains("go"), "expected edge label in:\n{out}");
    }

    #[test]
    fn group_renders_titled_region() {
        let out = lines(
            Mermaid::new(
                "block-beta\ncolumns 1\nblock:G:1\n  columns 2\n  X[\"X\"]\n  Y[\"Y\"]\nend",
            ),
            34,
            8,
        );
        assert!(out.contains('G'), "group title missing:\n{out}");
        assert!(out.contains('X') && out.contains('Y'));
        assert!(out.contains('╭') && out.contains('╯'), "{out}");
    }

    #[test]
    fn tiny_area_does_not_panic_and_clips() {
        let out = lines(Mermaid::new("block-beta\nA B C"), 4, 2);
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn one_by_one_area_is_safe() {
        let _ = lines(Mermaid::new("block-beta\nA"), 1, 1);
    }
}
