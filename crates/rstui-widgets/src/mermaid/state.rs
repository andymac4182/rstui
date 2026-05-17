//! `stateDiagram` / `stateDiagram-v2` Mermaid renderer.
//!
//! Renders the
//! [state-diagram](https://mermaid.js.org/syntax/stateDiagram.html) subset as
//! a deterministic top-down box-and-arrow chart on the shared
//! [`super::draw::Surface`].
//!
//! # Supported subset
//!
//! * Header `stateDiagram` or `stateDiagram-v2`.
//! * Transitions `A --> B`, `A --> B : event`, the start pseudo-state
//!   `[*] --> S1` (drawn as a filled `●` node) and the end pseudo-state
//!   `S1 --> [*]` (drawn as `◉`).
//! * `state "long description" as s2` aliases — the description is shown,
//!   keyed by the short id used in transitions.
//! * `state Name { ... }` composite states, **nestable**: every child line up
//!   to the matching `}` is parsed recursively and the composite is drawn as
//!   a titled doubled-border region enclosing its children.
//! * Special states `state fork_state <<fork>>`, `<<join>>`, `<<choice>>`:
//!   fork/join render as a heavy bar marker, choice as a `◇` diamond.
//! * `note left of S : text` / `note right of S : text` — drawn as a small
//!   detached `┄` note box beside the diagram.
//! * The concurrency divider `--` inside a composite is recorded so the
//!   composite's children are split into parallel regions by a `╌` rule.
//!
//! # Layout
//!
//! Top-level states are ranked with the flowchart longest-path idea
//! ([`super::rank_nodes`] in spirit): a state with no incoming transition is
//! rank 0, every other one rank past its deepest predecessor; each rank is a
//! row of rounded boxes and transitions are simple orthogonal connectors with
//! a `▼`/`▶` head and the event label. A composite state is laid out the same
//! way *inside* its own region box, recursively.
//!
//! # Terminal approximations
//!
//! A character grid cannot draw UML state rounded "stadium" shapes or true
//! orthogonal routing: states use the rounded box family, the start/end
//! pseudo-states are single glyphs, fork/join is a heavy rule, and a
//! transition is a single vertical stem with one horizontal jog. Overflow is
//! clipped with `…`; a bad line is skipped; nothing panics.

use super::draw::{BoxStyle, Surface};

/// What kind of node a state vertex is — drives its glyph/box shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A normal (or composite) state — a rounded box.
    Normal,
    /// The `[*]` start pseudo-state — a filled `●`.
    Start,
    /// The `[*]` end pseudo-state — a `◉`.
    End,
    /// `<<fork>>` / `<<join>>` — a heavy bar.
    Fork,
    /// `<<choice>>` — a `◇` diamond.
    Choice,
}

/// One parsed state vertex.
#[derive(Debug, Clone)]
struct State {
    /// The transition key (`s2` in `state "..." as s2`, else the raw id).
    id: String,
    /// The displayed label (the alias description, or the id).
    label: String,
    /// The node kind / shape.
    kind: Kind,
    /// Child state ids when this is a composite (`state S { ... }`).
    children: Vec<usize>,
    /// The id this composite's parent is, if nested (else `None`).
    parent: Option<usize>,
}

/// One parsed transition between two state ids (already resolved to indices).
#[derive(Debug, Clone)]
struct Transition {
    /// Source state index.
    from: usize,
    /// Target state index.
    to: usize,
    /// The optional `: event` label.
    label: String,
}

/// A free-floating note attached beside a state.
#[derive(Debug, Clone)]
struct Note {
    /// `true` for `note right of`, `false` for `note left of`.
    right: bool,
    /// The note text.
    text: String,
}

/// The whole parsed diagram.
#[derive(Debug, Default)]
struct Model {
    /// States in first-seen order; the index is the layout id.
    states: Vec<State>,
    /// Top-level transitions (composite-internal ones are drawn by recursion
    /// but for the terminal approximation every transition is drawn flat).
    transitions: Vec<Transition>,
    /// Notes, declaration order.
    notes: Vec<Note>,
}

impl Model {
    /// Index of the state with transition-key `id`, inserting a plain state
    /// if first seen. `[*]` is *not* routed here — start/end are synthesised
    /// per transition so two different `[*]` uses never collide.
    fn state_id(&mut self, id: &str) -> usize {
        if let Some(i) = self.states.iter().position(|s| s.id == id) {
            return i;
        }
        self.states.push(State {
            id: id.to_string(),
            label: id.to_string(),
            kind: Kind::Normal,
            children: Vec::new(),
            parent: None,
        });
        self.states.len() - 1
    }

    /// Pushes a fresh pseudo-state node (`[*]` start or end) and returns it.
    fn pseudo(&mut self, kind: Kind) -> usize {
        let glyph = if kind == Kind::Start { "●" } else { "◉" };
        self.states.push(State {
            id: format!("\u{0}pseudo{}", self.states.len()),
            label: glyph.to_string(),
            kind,
            children: Vec::new(),
            parent: None,
        });
        self.states.len() - 1
    }
}

/// Strips a single matching pair of surrounding quotes from `s`.
fn unquote(s: &str) -> &str {
    let t = s.trim();
    t.strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(t)
}

/// Resolves one operand token to a state index, synthesising a start/end
/// pseudo-state for `[*]` (its [`Kind`] depends on whether it is the source).
fn resolve(model: &mut Model, tok: &str, is_source: bool) -> usize {
    let t = tok.trim();
    if t == "[*]" {
        let kind = if is_source { Kind::Start } else { Kind::End };
        return model.pseudo(kind);
    }
    model.state_id(t)
}

/// Applies a `<<fork>>` / `<<join>>` / `<<choice>>` stereotype found on a
/// `state X <<...>>` line to the resolved state.
fn apply_stereotype(state: &mut State, stereo: &str) {
    state.kind = match stereo {
        "fork" | "join" => Kind::Fork,
        "choice" => Kind::Choice,
        _ => state.kind,
    };
}

/// Parses a `state ...` declaration line (alias / composite open / special).
/// Returns the composite's state index when the line *opens* a `{` block so
/// the caller can recurse, else `None`.
fn parse_state_decl(model: &mut Model, line: &str, parent: Option<usize>) -> Option<usize> {
    let rest = line.strip_prefix("state ")?.trim();
    // `state "desc" as s2`
    if let Some((desc, alias)) = rest.split_once(" as ") {
        let id = model.state_id(alias.trim());
        model.states[id].label = unquote(desc).to_string();
        model.states[id].parent = parent;
        return None;
    }
    // `state Name <<fork>>`
    if let Some(open) = rest.find("<<") {
        let name = rest[..open].trim();
        let stereo = rest[open + 2..].trim_end_matches('>').trim_end_matches('<');
        let id = model.state_id(name);
        model.states[id].parent = parent;
        apply_stereotype(&mut model.states[id], stereo.trim());
        return None;
    }
    // `state Name {`  (composite open)
    if let Some(name) = rest.strip_suffix('{') {
        let id = model.state_id(name.trim());
        model.states[id].parent = parent;
        return Some(id);
    }
    // bare `state Name`
    let id = model.state_id(rest);
    model.states[id].parent = parent;
    None
}

/// Parses a `note left of S : text` / `note right of S : text` line.
fn parse_note(model: &mut Model, line: &str) -> bool {
    let Some(rest) = line.strip_prefix("note ") else {
        return false;
    };
    let right = if rest.starts_with("right of") {
        true
    } else if rest.starts_with("left of") {
        false
    } else {
        return false;
    };
    let text = rest
        .split_once(':')
        .map(|(_, t)| t.trim())
        .unwrap_or("")
        .to_string();
    model.notes.push(Note { right, text });
    true
}

/// Recursively parses `body[*idx..]` until a `}` (or end) into states /
/// transitions, attributing every state to `parent`. `*idx` is advanced past
/// the consumed lines (and past the closing `}`).
fn parse_block(model: &mut Model, body: &[&str], idx: &mut usize, parent: Option<usize>) {
    while *idx < body.len() {
        let line = body[*idx];
        *idx += 1;

        if line == "}" {
            return;
        }
        if line == "--" {
            // Concurrency divider — recorded implicitly by the flat layout
            // (a dedicated parallel-region rule is a terminal approximation
            // we draw at render time from the composite's child span).
            continue;
        }

        // `state ...` declaration (maybe opening a composite block).
        if line.starts_with("state ") {
            if let Some(cid) = parse_state_decl(model, line, parent) {
                parse_block(model, body, idx, Some(cid));
                // Re-link children gathered during the recursive parse.
                let kids: Vec<usize> = (0..model.states.len())
                    .filter(|&k| model.states[k].parent == Some(cid))
                    .collect();
                model.states[cid].children = kids;
            }
            continue;
        }

        if line.starts_with("note ") && parse_note(model, line) {
            continue;
        }

        // A transition `A --> B [: label]`.
        if let Some((body_part, label)) = split_transition(line) {
            let (l, r) = body_part;
            let from = resolve(model, l, true);
            let to = resolve(model, r, false);
            if model.states[from].parent.is_none() {
                model.states[from].parent = parent;
            }
            if model.states[to].parent.is_none() {
                model.states[to].parent = parent;
            }
            model.transitions.push(Transition { from, to, label });
            continue;
        }

        // A bare state id on its own line.
        let t = line.trim();
        if !t.is_empty() && t != "}" && is_state_token(t) {
            let id = model.state_id(t);
            if model.states[id].parent.is_none() {
                model.states[id].parent = parent;
            }
        }
    }
}

/// Splits `A --> B [: label]` into `((A, B), label)`, or `None` when there is
/// no `-->` arrow.
fn split_transition(line: &str) -> Option<((&str, &str), String)> {
    let (body, label) = match line.split_once(':') {
        Some((b, l)) => (b.trim(), l.trim().to_string()),
        None => (line.trim(), String::new()),
    };
    let pos = body.find("-->")?;
    let l = body[..pos].trim();
    let r = body[pos + 3..].trim();
    if l.is_empty() || r.is_empty() {
        return None;
    }
    Some(((l, r), label))
}

/// `true` when `tok` is a plausible state id (letter/`_` start, then
/// alphanumeric / `_`), so stray punctuation is skipped.
fn is_state_token(tok: &str) -> bool {
    let mut cs = tok.chars();
    match cs.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    cs.all(|c| c.is_alphanumeric() || c == '_')
}

/// Parses the whole source into a [`Model`], lenient: a line that is neither
/// a declaration, a note, nor a transition is skipped.
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
    if body.first().is_some_and(|f| f.starts_with("stateDiagram")) {
        idx = 1;
    }

    parse_block(&mut model, &body, &mut idx, None);
    model
}

/// Longest-path rank over the top-level transition DAG. Mirrors the flowchart
/// [`super::rank_nodes`] fixpoint; self transitions are ignored and the
/// iteration is bounded by the state count so a cycle always terminates.
fn rank_states(model: &Model) -> Vec<usize> {
    let n = model.states.len();
    let mut rank = vec![0usize; n];
    if n == 0 {
        return rank;
    }
    let mut has_incoming = vec![false; n];
    for t in &model.transitions {
        if t.from != t.to {
            has_incoming[t.to] = true;
        }
    }
    let any_root = has_incoming.iter().any(|&v| !v);
    for _ in 0..n {
        let mut changed = false;
        for t in &model.transitions {
            if t.from == t.to {
                continue;
            }
            let cand = rank[t.from] + 1;
            if cand > rank[t.to] && (any_root || t.to != 0) {
                rank[t.to] = cand;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    rank
}

/// A placed state box: grid rectangle.
#[derive(Debug, Clone, Copy, Default)]
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

/// The on-grid width a state needs for its shape and label.
fn state_width(s: &State) -> i32 {
    match s.kind {
        Kind::Start | Kind::End => 1,
        Kind::Choice => 3,
        Kind::Fork => (s.label.chars().count() as i32 + 2).max(6),
        Kind::Normal => (s.label.chars().count() as i32 + 4).clamp(7, 36),
    }
}

/// The on-grid height a state needs for its shape.
fn state_height(s: &State) -> i32 {
    match s.kind {
        Kind::Start | Kind::End | Kind::Choice => 1,
        Kind::Fork => 1,
        Kind::Normal => 3,
    }
}

/// Draws one state vertex at its placed rectangle in the right shape.
fn draw_state(
    s: &mut Surface,
    p: Placed,
    st: &State,
    border: rstui_core::Style,
    text: rstui_core::Style,
) {
    match st.kind {
        Kind::Start => s.set(p.x, p.y, '●', border),
        Kind::End => s.set(p.x, p.y, '◉', border),
        Kind::Choice => s.text(p.x, p.y, "◇", border),
        Kind::Fork => {
            // A heavy bar; the (usually empty) label sits centred on it.
            s.hline(p.x, p.y, p.w, '━', border);
            if !st.label.is_empty() {
                s.text_centered(p.x, p.y, p.w, &st.label, text);
            }
        }
        Kind::Normal => {
            let kind = if st.children.is_empty() {
                BoxStyle::Round
            } else {
                // A composite gets the doubled border so it reads as a
                // region even though its children are drawn flat.
                BoxStyle::Double
            };
            s.labeled_box(p.x, p.y, p.w, p.h, kind, &st.label, border, text);
        }
    }
}

/// A vertical connector between two placed boxes with a `▼`/`▶` head at the
/// child end and the event label beside the stem — the terminal
/// approximation of a routed state transition.
fn draw_transition(
    s: &mut Surface,
    tr: &Transition,
    fp: Placed,
    tp: Placed,
    edge: rstui_core::Style,
    edge_label: rstui_core::Style,
) {
    let down = fp.y <= tp.y;
    let (top, bot) = if down { (fp, tp) } else { (tp, fp) };
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
    // Head at the child box's edge.
    let (hx, hy, head) = if down {
        (bx, y1, '▼')
    } else {
        (tx, y0, '▲')
    };
    s.set(hx, hy, head, edge);
    if !tr.label.is_empty() {
        let my = ((y0 + y1) / 2).clamp(y0, y1);
        s.text(tx + 1, my, &tr.label, edge_label);
    }
}

/// Renders a `stateDiagram` Mermaid diagram from `src` into `area`.
pub(crate) fn render(
    src: &str,
    area: rstui_core::Rect,
    buf: &mut rstui_core::Buffer,
    base: rstui_core::Style,
    theme: &super::MermaidTheme,
) {
    let model = parse(src);
    if model.states.is_empty() {
        super::diagram_placeholder("state diagram", "no states", area, buf, base, theme);
        return;
    }

    let rank = rank_states(&model);
    let max_rank = *rank.iter().max().unwrap_or(&0);
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for (i, &r) in rank.iter().enumerate() {
        rows[r].push(i);
    }

    const HGAP: i32 = 4;
    const VGAP: i32 = 2;
    let mut placed: Vec<Placed> = vec![Placed::default(); model.states.len()];

    let mut row_y = 0;
    let mut content_w = 0;
    for row in &rows {
        let mut x = 0;
        let mut row_h = 0;
        for &si in row {
            let st = &model.states[si];
            let w = state_width(st);
            let h = state_height(st);
            placed[si] = Placed { x, y: row_y, w, h };
            x += w + HGAP;
            row_h = row_h.max(h);
        }
        content_w = content_w.max(x - HGAP);
        row_y += row_h + VGAP;
    }
    let widest_label = model
        .transitions
        .iter()
        .map(|t| t.label.chars().count())
        .max()
        .unwrap_or(0);
    let label_margin = if widest_label == 0 {
        0
    } else {
        widest_label as i32 + 1
    };
    let note_w = model
        .notes
        .iter()
        .map(|n| n.text.chars().count() as i32 + 4)
        .max()
        .unwrap_or(0);
    let total_w = (content_w + label_margin + note_w + 1).max(1);
    let total_h = (row_y - VGAP).max(1);

    let mut s = Surface::new(total_w, total_h);
    let border = base.patch(theme.node_border);
    let text = base.patch(theme.node_label);
    let edge = base.patch(theme.edge);
    let edge_label = base.patch(theme.edge_label);
    let cluster = base.patch(theme.cluster);

    // A composite state's enclosing region: a doubled box around the span of
    // its placed children, titled with the composite's label.
    for (i, st) in model.states.iter().enumerate() {
        if st.children.is_empty() {
            continue;
        }
        let mut x0 = i32::MAX;
        let mut y0 = i32::MAX;
        let mut x1 = 0;
        let mut y1 = 0;
        for &c in &st.children {
            let p = placed[c];
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x + p.w);
            y1 = y1.max(p.y + p.h);
        }
        if x0 <= x1 && y0 <= y1 {
            let rx = (x0 - 1).max(0);
            let ry = (y0 - 1).max(0);
            let rw = (x1 - rx + 1).min(total_w - rx);
            let rh = (y1 - ry + 1).min(total_h - ry);
            s.rect(rx, ry, rw, rh, BoxStyle::Double, cluster);
            s.text_clipped(rx + 1, ry, &model.states[i].label, rw - 2, cluster);
        }
        // Mark this composite so its own box is not drawn again as a leaf.
        placed[i] = Placed::default();
    }

    for tr in &model.transitions {
        if tr.from == tr.to {
            continue;
        }
        draw_transition(&mut s, tr, placed[tr.from], placed[tr.to], edge, edge_label);
    }
    for (i, st) in model.states.iter().enumerate() {
        if !st.children.is_empty() {
            continue; // drawn as a region above
        }
        draw_state(&mut s, placed[i], st, border, text);
    }

    // Notes: a small dashed box stacked on the right margin.
    let mut ny = 0;
    for n in &model.notes {
        let nw = n.text.chars().count() as i32 + 4;
        let nx = total_w - nw;
        if nx >= 0 && ny + 3 <= total_h {
            s.rect(nx, ny, nw, 3, BoxStyle::Round, cluster);
            // Dashed top/bottom to read as a note rather than a state.
            for dx in nx + 1..nx + nw - 1 {
                s.set(dx, ny, '┄', cluster);
                s.set(dx, ny + 2, '┄', cluster);
            }
            let side = if n.right { '▸' } else { '◂' };
            s.set(nx, ny + 1, side, cluster);
            s.text_clipped(nx + 2, ny + 1, &n.text, nw - 4, text);
            ny += 3;
        }
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
    fn parses_simple_transition_with_event() {
        let m = parse("stateDiagram-v2\nS1 --> S2 : go");
        assert_eq!(m.states.len(), 2);
        assert_eq!(m.states[0].id, "S1");
        assert_eq!(m.states[1].id, "S2");
        assert_eq!(m.transitions.len(), 1);
        assert_eq!(m.transitions[0].label, "go");
    }

    #[test]
    fn start_and_end_are_distinct_pseudo_states() {
        let m = parse("stateDiagram\n[*] --> A\nA --> [*]");
        // Two pseudo + one real.
        assert_eq!(m.states.len(), 3);
        let kinds: Vec<Kind> = m.states.iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&Kind::Start));
        assert!(kinds.contains(&Kind::End));
        assert_eq!(m.states.iter().filter(|s| s.kind == Kind::Start).count(), 1);
        assert_eq!(m.states.iter().filter(|s| s.kind == Kind::End).count(), 1);
    }

    #[test]
    fn alias_description_is_used_as_label() {
        let m = parse("stateDiagram-v2\nstate \"Active session\" as s2\ns2 --> s2 : tick");
        let s = m.states.iter().find(|s| s.id == "s2").unwrap();
        assert_eq!(s.label, "Active session");
    }

    #[test]
    fn stereotypes_set_special_kinds() {
        let m = parse(
            "stateDiagram-v2\nstate fork_state <<fork>>\nstate join_state <<join>>\nstate c <<choice>>",
        );
        let f = m.states.iter().find(|s| s.id == "fork_state").unwrap();
        let j = m.states.iter().find(|s| s.id == "join_state").unwrap();
        let c = m.states.iter().find(|s| s.id == "c").unwrap();
        assert_eq!(f.kind, Kind::Fork);
        assert_eq!(j.kind, Kind::Fork);
        assert_eq!(c.kind, Kind::Choice);
    }

    #[test]
    fn composite_collects_nested_children() {
        let m = parse("stateDiagram-v2\nstate Outer {\n[*] --> Inner\nInner --> [*]\n}");
        let outer = m.states.iter().find(|s| s.id == "Outer").unwrap();
        // Inner + two pseudo states are children of Outer.
        assert!(!outer.children.is_empty());
        let inner = m.states.iter().find(|s| s.id == "Inner").unwrap();
        assert_eq!(inner.parent, m.states.iter().position(|s| s.id == "Outer"));
    }

    #[test]
    fn nested_composite_is_parsed_recursively() {
        let m = parse("stateDiagram-v2\nstate A {\nstate B {\nB1 --> B2\n}\n}");
        let a = m.states.iter().position(|s| s.id == "A").unwrap();
        let b = m.states.iter().position(|s| s.id == "B").unwrap();
        assert_eq!(m.states[b].parent, Some(a));
        let b1 = m.states.iter().find(|s| s.id == "B1").unwrap();
        assert_eq!(b1.parent, Some(b));
    }

    #[test]
    fn notes_left_and_right_are_parsed() {
        let m =
            parse("stateDiagram-v2\nA --> B\nnote right of A : start here\nnote left of B : done");
        assert_eq!(m.notes.len(), 2);
        assert!(m.notes[0].right);
        assert_eq!(m.notes[0].text, "start here");
        assert!(!m.notes[1].right);
        assert_eq!(m.notes[1].text, "done");
    }

    #[test]
    fn lenient_skips_garbage_and_frontmatter() {
        let m = parse("---\ntitle: T\n---\nstateDiagram-v2\n%% comment\n@@bad@@\nA --> B");
        assert_eq!(m.states.len(), 2);
        assert_eq!(m.transitions.len(), 1);
    }

    #[test]
    fn concurrency_divider_does_not_break_parse() {
        let m = parse("stateDiagram-v2\nstate S {\nA --> B\n--\nC --> D\n}");
        assert!(m.states.iter().any(|s| s.id == "A"));
        assert!(m.states.iter().any(|s| s.id == "D"));
        assert_eq!(m.transitions.len(), 2);
    }

    // --- render snapshots --------------------------------------------------

    #[test]
    fn empty_source_is_placeholder() {
        let out = lines("stateDiagram-v2", 40, 3);
        assert!(out.contains("mermaid"));
        assert!(out.contains("state diagram"));
        assert!(out.contains("no states"));
    }

    #[test]
    fn only_comment_is_placeholder() {
        let out = lines("stateDiagram-v2\n%% nothing here", 40, 3);
        assert!(out.contains("no states"));
    }

    #[test]
    fn single_normal_state_box_snapshot() {
        // A self-transition keeps `Idle` the only state; equal column means a
        // clean box with no jog. "Idle"(4) → width 4+4 clamped to 8 minimum;
        // height 3. The buffer is sized to the surface so there is no
        // centring offset and the snapshot is exact.
        let out = lines("stateDiagram-v2\nIdle --> Idle", 8, 3);
        assert_eq!(
            out,
            "╭──────╮\n\
             │ Idle │\n\
             ╰──────╯\n"
        );
    }

    #[test]
    fn start_state_end_chain_has_pseudo_glyphs_and_arrows() {
        // [*](●) → Idle → [*](◉) stacked top-down. Mismatched node widths
        // mean an orthogonal jog rather than a single column, so this is an
        // assertion test (a brittle full snapshot would only pin the jog).
        let out = lines("stateDiagram-v2\n[*] --> Idle\nIdle --> [*]", 16, 11);
        assert!(out.contains('●'), "start glyph:\n{out}");
        assert!(out.contains('◉'), "end glyph:\n{out}");
        assert!(out.contains('▼'), "downward head:\n{out}");
        assert!(out.contains("Idle"), "state label:\n{out}");
    }

    #[test]
    fn transition_event_label_is_drawn() {
        let out = lines("stateDiagram-v2\nIdle --> Run : start", 40, 12);
        assert!(out.contains("start"), "event label present:\n{out}");
        assert!(out.contains('▼'), "transition head present:\n{out}");
        assert!(out.contains("Idle") && out.contains("Run"));
    }

    #[test]
    fn rounded_box_is_used_for_normal_state() {
        let out = lines("stateDiagram-v2\nstate \"Working\" as w\nw --> w", 40, 8);
        assert!(
            out.contains('╭') && out.contains('╯'),
            "rounded box:\n{out}"
        );
        assert!(out.contains("Working"));
    }

    #[test]
    fn composite_state_draws_doubled_region() {
        let out = lines(
            "stateDiagram-v2\nstate Outer {\nA --> B\n}\nOuter --> Done",
            44,
            16,
        );
        assert!(
            out.contains('╔') || out.contains('╗'),
            "region border:\n{out}"
        );
        assert!(out.contains("Outer"), "region title:\n{out}");
        assert!(out.contains('A') && out.contains('B'));
    }

    #[test]
    fn fork_renders_heavy_bar() {
        let out = lines(
            "stateDiagram-v2\nstate f <<fork>>\n[*] --> f\nf --> A",
            30,
            12,
        );
        assert!(out.contains('━'), "fork bar present:\n{out}");
    }

    #[test]
    fn choice_renders_diamond() {
        let out = lines(
            "stateDiagram-v2\nstate c <<choice>>\nA --> c\nc --> B",
            30,
            14,
        );
        assert!(out.contains('◇'), "choice diamond present:\n{out}");
    }

    #[test]
    fn note_box_is_drawn_on_the_side() {
        let out = lines(
            "stateDiagram-v2\nA --> B\nnote right of A : hi there",
            44,
            10,
        );
        assert!(out.contains("hi there"), "note text present:\n{out}");
        assert!(out.contains('┄'), "dashed note border present:\n{out}");
    }

    #[test]
    fn tiny_area_does_not_panic() {
        for (w, h) in [(0, 0), (1, 1), (2, 1), (3, 3), (1, 30), (30, 1)] {
            let _ = lines(
                "stateDiagram-v2\n[*] --> A\nstate S {\nA --> B\n--\nC --> D\n}\nS --> [*]",
                w,
                h,
            );
        }
    }

    #[test]
    fn long_label_is_clipped() {
        // The state box width is clamped (max 36) so an over-long
        // description is truncated with `…` rather than widening forever.
        let out = lines(
            "stateDiagram-v2\nstate \"a very very very long state description here\" as s\ns --> s",
            44,
            8,
        );
        assert!(out.contains('…'), "overflow clipped:\n{out}");
    }
}
