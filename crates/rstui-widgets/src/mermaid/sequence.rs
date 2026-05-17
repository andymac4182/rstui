//! `sequenceDiagram` Mermaid renderer — a hand-written line parser plus a
//! deterministic integer layout drawn onto the shared [`super::draw::Surface`].
//!
//! # Supported subset
//!
//! - `participant Alice`, `participant A as Alice`, `actor Bob`. A participant
//!   used in a message before it is declared is auto-declared in source order.
//! - Messages: `A->>B: t` (solid `►`), `A-->>B: t` (dashed return `┄►`),
//!   `A->B` / `A-->B` (open, no head), `A-)B` / `A--)B` (async `▻`), and a
//!   self-message `A->>A: t` (a small loop on the lifeline). A trailing `+` /
//!   leading `-` activation suffix on the target/source is honoured.
//! - `Note right of A: t`, `Note left of A: t`, `Note over A,B: t` → a small
//!   bordered note box on / beside the involved lifeline(s).
//! - Combined fragments `loop` / `alt` … `else` / `opt` / `par` … `and` /
//!   `break` / `critical` … `option`, each closed by a matching `end`
//!   (nestable) → a labelled bordered region spanning the involved lifelines
//!   with the keyword tag in its top-left corner.
//! - `activate A` / `deactivate A` (and the `+` / `-` arrow suffixes) draw a
//!   thin activation bar on the lifeline.
//! - `autonumber` prefixes every following message with a sequence number.
//! - `title X` (or a `title:` line in `--- … ---` frontmatter) is centred on
//!   the top row.
//!
//! # Layout
//!
//! Participant boxes sit in a row at the top, each centred on a fixed column
//! `x`. A dashed lifeline (`┊`, themed `edge`) drops from every box. Each
//! message, note and fragment edge consumes its own row, advancing `y`; an
//! arrow is a horizontal run between the two lifelines with the label centred
//! on the row just above it. The whole image is built on a content-sized
//! `Surface` and blitted once, so out-of-area output clips instead of
//! panicking.
//!
//! # Terminal approximations
//!
//! The terminal has no curves or true diagonals: a self-message is drawn as a
//! small right-side rectangular loop rather than an arc, dashed lines use the
//! `┄`/`┊` boxdraw glyphs, and arrowheads are the single glyphs `►`/`▻`
//! (`◄`/`◁` when the message points leftward). Fragments are plain bordered
//! rectangles with a text tag, not shaded SVG containers. This is an honest
//! legible subset, not a pixel-faithful Mermaid engine.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// The drawn style of a message arrow, selected by its operator token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrowKind {
    /// `->>` — a solid line with a filled arrowhead.
    Solid,
    /// `-->>` — a dashed line with a filled arrowhead (a return message).
    DashedSolid,
    /// `->` — a solid line, no arrowhead (an open call).
    Open,
    /// `-->` — a dashed line, no arrowhead (an open return).
    DashedOpen,
    /// `-)` — a solid line with an open arrowhead (an async message).
    Async,
    /// `--)` — a dashed line with an open arrowhead (an async return).
    DashedAsync,
}

impl ArrowKind {
    /// Whether the connector line is drawn dashed rather than solid.
    const fn dashed(self) -> bool {
        matches!(
            self,
            Self::DashedSolid | Self::DashedOpen | Self::DashedAsync
        )
    }

    /// The arrowhead glyph for a rightward message (`None` = no head, an open
    /// link). `left` swaps it for the mirrored leftward glyph.
    const fn head(self, left: bool) -> Option<char> {
        match self {
            Self::Solid | Self::DashedSolid => Some(if left { '◄' } else { '►' }),
            Self::Async | Self::DashedAsync => Some(if left { '◁' } else { '▻' }),
            Self::Open | Self::DashedOpen => None,
        }
    }
}

/// A single parsed sequence statement, in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stmt {
    /// A message `from → to` with its arrow style and (already trimmed) text.
    Message {
        /// Index of the source participant.
        from: usize,
        /// Index of the target participant.
        to: usize,
        /// The arrow operator's drawn style.
        kind: ArrowKind,
        /// The message label (may be empty).
        text: String,
    },
    /// A note box anchored to one or two participants.
    Note {
        /// The note placement.
        place: NotePlace,
        /// Index of the first (or only) anchored participant.
        a: usize,
        /// Index of the second anchored participant for `over A,B`.
        b: usize,
        /// The note text.
        text: String,
    },
    /// The opening of a combined fragment (`loop` / `alt` / …).
    FragStart {
        /// The keyword tag shown in the corner (e.g. `loop`).
        tag: String,
        /// The fragment's title (the text after the keyword).
        label: String,
    },
    /// A fragment divider (`else` / `and` / `option`).
    FragElse {
        /// The divider keyword.
        tag: String,
        /// The divider's label.
        label: String,
    },
    /// The `end` that closes the innermost open fragment.
    FragEnd,
    /// `activate P` — open an activation bar on participant `P`.
    Activate(usize),
    /// `deactivate P` — close the activation bar on participant `P`.
    Deactivate(usize),
}

/// Where a [`Stmt::Note`] sits relative to its anchor participant(s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotePlace {
    /// `Note right of A`.
    RightOf,
    /// `Note left of A`.
    LeftOf,
    /// `Note over A` / `Note over A,B`.
    Over,
}

/// The fully parsed diagram: participants in declaration order, their display
/// labels, the optional title, the statement stream and whether `autonumber`
/// is active.
#[derive(Debug, Default)]
struct Seq {
    /// Stable participant ids in first-seen order.
    ids: Vec<String>,
    /// Display label per participant (the `as` alias, or the id).
    labels: Vec<String>,
    /// The diagram title, if any.
    title: Option<String>,
    /// The ordered statement stream.
    stmts: Vec<Stmt>,
    /// Whether messages are prefixed with sequence numbers.
    autonumber: bool,
}

impl Seq {
    /// Returns the index of participant `id`, declaring it (with `label`, or
    /// the id when `label` is `None`) in source order on first use.
    fn participant(&mut self, id: &str, label: Option<&str>) -> usize {
        if let Some(i) = self.ids.iter().position(|p| p == id) {
            if let Some(l) = label {
                self.labels[i] = l.to_owned();
            }
            return i;
        }
        self.ids.push(id.to_owned());
        self.labels.push(label.unwrap_or(id).to_owned());
        self.ids.len() - 1
    }
}

/// Splits a participant token off the front of `s`, returning
/// `(id, label, rest)`. Handles `A as Alice` (label = `Alice`) and a leading
/// `+`/`-` activation marker, which is stripped from the id.
fn take_actor(decl: &str) -> (String, Option<String>) {
    let decl = decl.trim();
    if let Some((id, alias)) = decl.split_once(" as ") {
        (
            id.trim().trim_start_matches(['+', '-']).to_owned(),
            Some(alias.trim().to_owned()),
        )
    } else {
        (decl.trim_start_matches(['+', '-']).to_owned(), None)
    }
}

/// Recognises the message arrow operator inside `line` and returns
/// `(before, ArrowKind, after)` split around it, longest operator first so
/// `-->>` is not mis-split as `-->`.
fn split_arrow(line: &str) -> Option<(&str, ArrowKind, &str)> {
    // Longest tokens first; each maps to its drawn style.
    const OPS: &[(&str, ArrowKind)] = &[
        ("-->>", ArrowKind::DashedSolid),
        ("--)", ArrowKind::DashedAsync),
        ("-->", ArrowKind::DashedOpen),
        ("->>", ArrowKind::Solid),
        ("-)", ArrowKind::Async),
        ("->", ArrowKind::Open),
    ];
    for (op, kind) in OPS {
        if let Some(i) = line.find(op) {
            return Some((&line[..i], *kind, &line[i + op.len()..]));
        }
    }
    None
}

/// Parses one `Note …` line into a [`Stmt::Note`], or `None` if malformed.
fn parse_note(rest: &str, seq: &mut Seq) -> Option<Stmt> {
    let (place, body) = if let Some(b) = rest.strip_prefix("right of ") {
        (NotePlace::RightOf, b)
    } else if let Some(b) = rest.strip_prefix("left of ") {
        (NotePlace::LeftOf, b)
    } else if let Some(b) = rest.strip_prefix("over ") {
        (NotePlace::Over, b)
    } else {
        return None;
    };
    let (anchors, text) = body.split_once(':')?;
    let mut parts = anchors.split(',').map(str::trim).filter(|p| !p.is_empty());
    let a_id = parts.next()?;
    let a = seq.participant(a_id, None);
    let b = match parts.next() {
        Some(b_id) => seq.participant(b_id, None),
        None => a,
    };
    Some(Stmt::Note {
        place,
        a,
        b,
        text: text.trim().to_owned(),
    })
}

/// The fragment keywords that open a region (`end`-closed) and the dividers.
const FRAG_OPEN: &[&str] = &["loop", "alt", "opt", "par", "break", "critical", "rect"];
const FRAG_ELSE: &[&str] = &["else", "and", "option"];

/// Parses the whole source into a [`Seq`]. Lenient: an unrecognised line is
/// skipped, never an error.
fn parse(src: &str) -> Seq {
    let mut seq = Seq::default();
    let mut lines = src
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .map(str::trim);

    // Drop a leading `--- … ---` frontmatter block, harvesting `title:`.
    let mut pending: Vec<String> = Vec::new();
    let collected: Vec<&str> = lines.by_ref().collect();
    let mut it = collected.iter().copied().peekable();
    // Skip blank/comment lines to find the first significant line.
    while matches!(it.peek(), Some(l) if l.is_empty() || l.starts_with("%%")) {
        it.next();
    }
    if it.peek() == Some(&"---") {
        it.next();
        for l in it.by_ref() {
            if l == "---" {
                break;
            }
            if let Some(t) = l.trim().strip_prefix("title:") {
                seq.title = Some(t.trim().trim_matches(['"', '\'']).to_owned());
            }
        }
    }
    for l in it {
        pending.push(l.to_owned());
    }

    let mut header_seen = false;
    for raw in &pending {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        if !header_seen {
            // The `sequenceDiagram` header (the dispatcher guarantees it).
            if line.starts_with("sequenceDiagram") {
                header_seen = true;
                continue;
            }
            header_seen = true;
        }

        if let Some(t) = line.strip_prefix("title ") {
            seq.title = Some(t.trim().to_owned());
            continue;
        }
        if line == "autonumber" {
            seq.autonumber = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("participant ") {
            let (id, label) = take_actor(rest);
            seq.participant(&id, label.as_deref());
            continue;
        }
        if let Some(rest) = line.strip_prefix("actor ") {
            let (id, label) = take_actor(rest);
            seq.participant(&id, label.as_deref());
            continue;
        }
        if let Some(rest) = line.strip_prefix("activate ") {
            let i = seq.participant(rest.trim(), None);
            seq.stmts.push(Stmt::Activate(i));
            continue;
        }
        if let Some(rest) = line.strip_prefix("deactivate ") {
            let i = seq.participant(rest.trim(), None);
            seq.stmts.push(Stmt::Deactivate(i));
            continue;
        }
        if line == "end" {
            seq.stmts.push(Stmt::FragEnd);
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("Note ")
            .or_else(|| line.strip_prefix("note "))
        {
            if let Some(n) = parse_note(rest.trim(), &mut seq) {
                seq.stmts.push(n);
            }
            continue;
        }
        // Fragment open / divider keywords (first word).
        let kw = line.split_whitespace().next().unwrap_or("");
        if FRAG_OPEN.contains(&kw) {
            let label = line[kw.len()..].trim().to_owned();
            seq.stmts.push(Stmt::FragStart {
                tag: kw.to_owned(),
                label,
            });
            continue;
        }
        if FRAG_ELSE.contains(&kw) {
            let label = line[kw.len()..].trim().to_owned();
            seq.stmts.push(Stmt::FragElse {
                tag: kw.to_owned(),
                label,
            });
            continue;
        }
        // Otherwise: a message `A op B : text`.
        if let Some((lhs, kind, rhs)) = split_arrow(line) {
            let (rhs_actor, text) = match rhs.split_once(':') {
                Some((a, t)) => (a.trim(), t.trim().to_owned()),
                None => (rhs.trim(), String::new()),
            };
            // Strip activation markers (`+`/`-`) from both endpoints.
            let from_id = lhs.trim().trim_end_matches(['+', '-']).trim();
            let to_clean = rhs_actor.trim_start_matches(['+', '-']).trim();
            if from_id.is_empty() || to_clean.is_empty() {
                continue;
            }
            let from = seq.participant(from_id, None);
            let to = seq.participant(to_clean, None);
            seq.stmts.push(Stmt::Message {
                from,
                to,
                kind,
                text,
            });
        }
        // Anything else: silently skipped.
    }
    seq
}

/// Per-participant horizontal geometry resolved before drawing.
struct Cols {
    /// The centre `x` of each participant's lifeline.
    center: Vec<i32>,
    /// Each participant box's left `x`.
    left: Vec<i32>,
    /// Each participant box's width.
    box_w: Vec<i32>,
    /// The whole surface width.
    width: i32,
}

/// The minimum number of cells between two adjacent lifelines (room for a
/// short label centred over an arrow plus its head).
const MIN_GAP: i32 = 8;
/// Inner horizontal padding inside a participant / note box.
const PAD: i32 = 1;
/// How far a self-message loop extends to the right of its lifeline.
const SELF_W: i32 = 4;
/// How far a fragment border / divider extends beyond the outermost
/// participant lifeline it spans.
const FRAG_PAD: i32 = 4;

/// Computes the column geometry for `seq`.
///
/// Each participant box is sized to its label; the centre-to-centre spacing of
/// adjacent lifelines is widened so the longest message label between them
/// fits over the arrow. The layout is fully determined by the source (no area
/// dependence), so the surface is content-sized and the blit centres/clips it.
fn columns(seq: &Seq) -> Cols {
    let n = seq.ids.len().max(1);
    let mut box_w = Vec::with_capacity(seq.labels.len());
    for l in &seq.labels {
        let w = l.chars().count() as i32 + 2 * PAD + 2;
        box_w.push(w.max(5));
    }
    if box_w.is_empty() {
        box_w.push(5);
    }
    // The widest label that has to span between columns `i` and `i+1`
    // (any message whose endpoints straddle that boundary).
    let mut gap_need = vec![MIN_GAP; n.saturating_sub(1).max(1)];
    for st in &seq.stmts {
        let (lo, hi, tw) = match st {
            Stmt::Message { from, to, text, .. } if from != to => (
                (*from).min(*to),
                (*from).max(*to),
                text.chars().count() as i32,
            ),
            _ => continue,
        };
        let spans = (hi - lo).max(1) as i32;
        for b in lo..hi {
            if b < gap_need.len() {
                // The label sits over the whole span; give every crossed gap
                // a fair share of its width so the centred label always fits.
                let share = (tw + 4) / spans + MIN_GAP;
                gap_need[b] = gap_need[b].max(share);
            }
        }
    }
    let mut center = Vec::with_capacity(box_w.len());
    let mut left = Vec::with_capacity(box_w.len());
    // First box's left edge is column 0.
    let mut cx = box_w[0] / 2;
    for i in 0..box_w.len() {
        center.push(cx);
        left.push(cx - box_w[i] / 2);
        if i + 1 < box_w.len() {
            let need_gap = gap_need.get(i).copied().unwrap_or(MIN_GAP).max(MIN_GAP);
            // Spacing must clear both half-boxes plus the required gap.
            let step = box_w[i] / 2 + need_gap + box_w[i + 1] / 2;
            cx += step;
        }
    }
    let last_w = *box_w.last().unwrap();
    let width = *center.last().unwrap_or(&0) + last_w / 2 + (last_w % 2);
    Cols {
        center,
        left,
        box_w,
        width,
    }
}

/// Renders a `sequenceDiagram` from `src` into `area`.
///
/// Parses the supported subset, lays it out deterministically and blits a
/// single content-sized [`Surface`]. When nothing parses (no participants and
/// no statements) it draws the shared honest placeholder and returns.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let seq = parse(src);
    if seq.ids.is_empty() {
        super::diagram_placeholder("sequence", "no participants", area, buf, base, theme);
        return;
    }

    let mut cols = columns(&seq);
    // Reserve horizontal margins for everything that overhangs the outer
    // lifelines so nothing is clipped: self-loops to the right of their
    // lifeline, fragment borders `±FRAG_PAD` beyond the outermost involved
    // lifelines, and `right of` / `left of` / wide `over` notes.
    let last = cols.center.len() - 1;
    let mut left_margin = 0;
    let mut right_margin = 0;
    let widest_self = seq
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::Message { from, to, text, .. } if from == to => {
                Some((*from, text.chars().count() as i32))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    for (p, tw) in &widest_self {
        // Self-loop reaches cx+SELF_W; its label reaches further right still.
        let reach = SELF_W + 2 + tw;
        let over = cols.center[*p] + reach - cols.width + 1;
        right_margin = right_margin.max(over);
        if *p == last {
            right_margin = right_margin.max(over);
        }
    }
    // Any fragment overhangs the outer involved lifelines by FRAG_PAD; with no
    // tracked involvement a fragment can span all participants, so reserve it
    // unconditionally when any fragment exists.
    if seq
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FragStart { .. }))
    {
        left_margin = left_margin.max(FRAG_PAD);
        right_margin = right_margin.max(FRAG_PAD);
    }
    for st in &seq.stmts {
        if let Stmt::Note { place, a, b, text } = st {
            let tw = text.chars().count() as i32 + 2 + 2 * PAD;
            match place {
                NotePlace::LeftOf => {
                    let need = (cols.center[*a] - 2 - tw).min(0).abs();
                    left_margin = left_margin.max(need + FRAG_PAD);
                }
                NotePlace::RightOf => {
                    let over = cols.center[*a] + 2 + tw - cols.width + 1;
                    right_margin = right_margin.max(over);
                }
                NotePlace::Over => {
                    let lo = cols.center[*a].min(cols.center[*b]);
                    let hi = cols.center[*a].max(cols.center[*b]);
                    let span = (hi - lo) + 1;
                    if tw > span {
                        let half = (tw - span + 1) / 2;
                        left_margin = left_margin.max((lo - half).min(0).abs());
                        right_margin = right_margin.max(hi + half - cols.width + 1);
                    }
                }
            }
        }
    }
    let left_margin = left_margin.max(0);
    let right_margin = right_margin.max(0);
    for c in &mut cols.center {
        *c += left_margin;
    }
    for l in &mut cols.left {
        *l += left_margin;
    }
    cols.width += left_margin + right_margin;

    let border = base.patch(theme.node_border);
    let label_st = base.patch(theme.node_label);
    let edge = base.patch(theme.edge);
    let msg_st = base.patch(theme.edge_label);
    let frag_st = base.patch(theme.cluster);

    // --- vertical layout pass: assign a row to every statement -------------
    let mut y = 0;
    if seq.title.is_some() {
        y += 2;
    }
    let header_top = y;
    let header_h = 3;
    y += header_h;
    let lifeline_top = y;

    // A row plan so the surface can be sized before drawing.
    #[derive(Clone)]
    enum Row {
        Msg {
            from: usize,
            to: usize,
            kind: ArrowKind,
            text: String,
            num: Option<usize>,
            y: i32,
            self_msg: bool,
        },
        Note {
            place: NotePlace,
            a: usize,
            b: usize,
            text: String,
            y: i32,
            h: i32,
        },
        FragOpen {
            tag: String,
            label: String,
            y: i32,
            depth: usize,
        },
        FragDiv {
            tag: String,
            label: String,
            y: i32,
        },
        FragClose {
            y: i32,
            open_y: i32,
            lo: usize,
            hi: usize,
            depth: usize,
        },
        Act {
            p: usize,
            y: i32,
            on: bool,
        },
    }

    let mut rows: Vec<Row> = Vec::new();
    let mut number = 1usize;
    // Track open fragments: (open_row_index, min_part, max_part).
    let mut frag_stack: Vec<(usize, usize, usize)> = Vec::new();

    for st in &seq.stmts {
        match st {
            Stmt::Message {
                from,
                to,
                kind,
                text,
            } => {
                let self_msg = from == to;
                let num = if seq.autonumber {
                    let n = number;
                    number += 1;
                    Some(n)
                } else {
                    None
                };
                // A labelled arrow needs a label row above it.
                if !text.is_empty() || num.is_some() {
                    y += 1;
                }
                let row_y = y;
                y += if self_msg { 3 } else { 1 };
                y += 1; // breathing room
                rows.push(Row::Msg {
                    from: *from,
                    to: *to,
                    kind: *kind,
                    text: text.clone(),
                    num,
                    y: row_y,
                    self_msg,
                });
                // Widen any open fragment to include both endpoints.
                for f in &mut frag_stack {
                    f.1 = f.1.min(*from).min(*to);
                    f.2 = f.2.max(*from).max(*to);
                }
            }
            Stmt::Note { place, a, b, text } => {
                let h = 3;
                let row_y = y;
                y += h + 1;
                rows.push(Row::Note {
                    place: *place,
                    a: *a,
                    b: *b,
                    text: text.clone(),
                    y: row_y,
                    h,
                });
                for f in &mut frag_stack {
                    f.1 = f.1.min(*a).min(*b);
                    f.2 = f.2.max(*a).max(*b);
                }
            }
            Stmt::FragStart { tag, label } => {
                y += 1;
                let idx = rows.len();
                rows.push(Row::FragOpen {
                    tag: tag.clone(),
                    label: label.clone(),
                    y,
                    depth: frag_stack.len(),
                });
                y += 1;
                frag_stack.push((idx, usize::MAX, 0));
            }
            Stmt::FragElse { tag, label } => {
                y += 1;
                rows.push(Row::FragDiv {
                    tag: tag.clone(),
                    label: label.clone(),
                    y,
                });
                y += 1;
            }
            Stmt::FragEnd => {
                if let Some((open_idx, mut lo, mut hi)) = frag_stack.pop() {
                    if lo == usize::MAX {
                        // An empty fragment: span all participants.
                        lo = 0;
                        hi = seq.ids.len().saturating_sub(1);
                    }
                    let (open_y, depth) = match &rows[open_idx] {
                        Row::FragOpen { y, depth, .. } => (*y, *depth),
                        _ => (0, 0),
                    };
                    y += 1;
                    rows.push(Row::FragClose {
                        y,
                        open_y,
                        lo,
                        hi,
                        depth,
                    });
                    y += 1;
                }
            }
            Stmt::Activate(p) => {
                rows.push(Row::Act { p: *p, y, on: true });
            }
            Stmt::Deactivate(p) => {
                rows.push(Row::Act {
                    p: *p,
                    y,
                    on: false,
                });
            }
        }
    }
    // Any unclosed fragment is closed at the bottom (lenient).
    while let Some((open_idx, mut lo, mut hi)) = frag_stack.pop() {
        if lo == usize::MAX {
            lo = 0;
            hi = seq.ids.len().saturating_sub(1);
        }
        let (open_y, depth) = match &rows[open_idx] {
            Row::FragOpen { y, depth, .. } => (*y, *depth),
            _ => (0, 0),
        };
        y += 1;
        rows.push(Row::FragClose {
            y,
            open_y,
            lo,
            hi,
            depth,
        });
        y += 1;
    }

    let lifeline_bottom = y.max(lifeline_top + 1);
    let total_h = lifeline_bottom;
    // Content-sized surface: the blit centres it in (and clips it to) `area`,
    // exactly like `diagram_placeholder`.
    let surf_w = cols.width.max(2);
    let mut s = Surface::new(surf_w, total_h.max(1));

    // --- title -------------------------------------------------------------
    if let Some(t) = &seq.title {
        s.text_centered(0, 0, surf_w, t, label_st);
    }

    // --- lifelines + participant boxes ------------------------------------
    for i in 0..seq.ids.len() {
        let cx = cols.center[i];
        // Dashed lifeline first (boxes overpaint their slice).
        for ly in lifeline_top..lifeline_bottom {
            s.set(cx, ly, '┊', edge);
        }
        let lx = cols.left[i];
        let bw = cols.box_w[i];
        s.labeled_box(
            lx,
            header_top,
            bw,
            header_h,
            BoxStyle::Round,
            &seq.labels[i],
            border,
            label_st,
        );
    }

    // --- activation bars ---------------------------------------------------
    // Pair activate/deactivate per participant in row order; an unmatched
    // open runs to the bottom.
    {
        let mut open: Vec<Option<i32>> = vec![None; seq.ids.len()];
        let mut bars: Vec<(usize, i32, i32)> = Vec::new();
        for r in &rows {
            if let Row::Act { p, y, on } = r {
                if *on {
                    open[*p] = Some(*y);
                } else if let Some(sy) = open[*p].take() {
                    bars.push((*p, sy, *y));
                }
            }
        }
        for (p, sy) in open.iter().enumerate() {
            if let Some(sy) = sy {
                bars.push((p, *sy, lifeline_bottom - 1));
            }
        }
        for (p, sy, ey) in bars {
            let cx = cols.center[p];
            for by in sy..=ey.max(sy) {
                s.set(cx, by, '┃', edge);
            }
        }
    }

    // --- statement rows ----------------------------------------------------
    for r in &rows {
        match r {
            Row::Msg {
                from,
                to,
                kind,
                text,
                num,
                y,
                self_msg,
            } => {
                let label = match num {
                    Some(n) => {
                        if text.is_empty() {
                            format!("{n}.")
                        } else {
                            format!("{n}. {text}")
                        }
                    }
                    None => text.clone(),
                };
                if *self_msg {
                    draw_self(&mut s, cols.center[*from], *y, *kind, &label, edge, msg_st);
                } else {
                    draw_arrow(
                        &mut s,
                        cols.center[*from],
                        cols.center[*to],
                        *y,
                        *kind,
                        &label,
                        edge,
                        msg_st,
                    );
                }
            }
            Row::Note {
                place,
                a,
                b,
                text,
                y,
                h,
            } => {
                draw_note(&mut s, &cols, *place, *a, *b, text, *y, *h, border, msg_st);
            }
            Row::FragOpen { .. }
            | Row::FragDiv { .. }
            | Row::FragClose { .. }
            | Row::Act { .. } => {}
        }
    }

    // --- fragment regions (drawn last so borders sit over lifelines) -------
    // Match each open with its close by scanning; nesting handled by depth.
    let mut open_for_depth: Vec<Option<(String, String, i32)>> = Vec::new();
    for r in &rows {
        match r {
            Row::FragOpen {
                tag,
                label,
                y,
                depth,
            } => {
                if open_for_depth.len() <= *depth {
                    open_for_depth.resize(*depth + 1, None);
                }
                open_for_depth[*depth] = Some((tag.clone(), label.clone(), *y));
            }
            Row::FragClose {
                y,
                open_y,
                lo,
                hi,
                depth,
            } => {
                let (tag, label) = match open_for_depth.get(*depth).and_then(|o| o.clone()) {
                    Some((t, l, _)) => (t, l),
                    None => (String::from("frag"), String::new()),
                };
                draw_fragment(
                    &mut s, &cols, *lo, *hi, *open_y, *y, &tag, &label, frag_st, msg_st,
                );
                let _ = open_y;
            }
            Row::FragDiv { tag, label, y } => {
                // A divider line across the innermost open fragment.
                draw_divider(
                    &mut s,
                    &cols,
                    &open_for_depth,
                    tag,
                    label,
                    *y,
                    frag_st,
                    msg_st,
                );
            }
            _ => {}
        }
    }

    s.blit(area, buf, base);
}

/// Draws a horizontal message arrow between lifelines `cx_from` and `cx_to`
/// on row `y`, with `label` centred on the row directly above it.
#[allow(clippy::too_many_arguments)]
fn draw_arrow(
    s: &mut Surface,
    cx_from: i32,
    cx_to: i32,
    y: i32,
    kind: ArrowKind,
    label: &str,
    edge: Style,
    text: Style,
) {
    let left = cx_to < cx_from;
    let (a, b) = if left {
        (cx_to, cx_from)
    } else {
        (cx_from, cx_to)
    };
    let line_ch = if kind.dashed() { '┄' } else { '─' };
    // The line spans the cells strictly between the two lifelines.
    let lo = a + 1;
    let hi = b - 1;
    if hi >= lo {
        s.hline(lo, y, hi - lo + 1, line_ch, edge);
    }
    // Arrowhead lands on the cell just inside the target lifeline.
    if let Some(h) = kind.head(left) {
        let hx = if left { a + 1 } else { b - 1 };
        s.set(hx.clamp(a, b), y, h, edge);
    }
    if !label.is_empty() {
        let width = (b - a - 1).max(1);
        s.text_centered(a + 1, y - 1, width, label, text);
    }
}

/// Draws a self-message: a small right-side rectangular loop on the lifeline
/// at `cx`, occupying rows `y..y+3`, label to the loop's right.
fn draw_self(
    s: &mut Surface,
    cx: i32,
    y: i32,
    kind: ArrowKind,
    label: &str,
    edge: Style,
    text: Style,
) {
    let line_ch = if kind.dashed() { '┄' } else { '─' };
    let rx = cx + SELF_W;
    // Out leg, down the side, back in. The legs stop one cell short of `rx`
    // so they do not overwrite the corner glyphs.
    s.hline(cx + 1, y, rx - cx - 1, line_ch, edge);
    s.hline(cx + 2, y + 2, rx - cx - 2, line_ch, edge);
    s.set(rx, y, '┐', edge);
    s.set(rx, y + 1, '│', edge);
    s.set(rx, y + 2, '┘', edge);
    if let Some(h) = kind.head(true) {
        s.set(cx + 1, y + 2, h, edge);
    }
    if !label.is_empty() {
        s.text(rx + 2, y + 1, label, text);
    }
}

/// Draws a note box anchored per [`NotePlace`]; `over A,B` spans both
/// lifelines, `right of` / `left of` sits beside the single anchor.
#[allow(clippy::too_many_arguments)]
fn draw_note(
    s: &mut Surface,
    cols: &Cols,
    place: NotePlace,
    a: usize,
    b: usize,
    text: &str,
    y: i32,
    h: i32,
    border: Style,
    body: Style,
) {
    let tw = text.chars().count() as i32;
    let (x, w) = match place {
        NotePlace::Over => {
            let lo = cols.center[a].min(cols.center[b]);
            let hi = cols.center[a].max(cols.center[b]);
            let span = (hi - lo) + 1;
            let w = (tw + 2 + 2 * PAD).max(span).max(5);
            (lo - (w - span) / 2, w)
        }
        NotePlace::RightOf => {
            let w = (tw + 2 + 2 * PAD).max(5);
            (cols.center[a] + 2, w)
        }
        NotePlace::LeftOf => {
            let w = (tw + 2 + 2 * PAD).max(5);
            (cols.center[a] - 2 - w, w)
        }
    };
    s.labeled_box(x, y, w, h, BoxStyle::Square, text, border, body);
}

/// Draws a labelled fragment region spanning participant indices `lo..=hi`
/// from row `open_y` to `close_y`, with `tag` (`[label]`) in the top-left.
#[allow(clippy::too_many_arguments)]
fn draw_fragment(
    s: &mut Surface,
    cols: &Cols,
    lo: usize,
    hi: usize,
    open_y: i32,
    close_y: i32,
    tag: &str,
    label: &str,
    border: Style,
    text: Style,
) {
    let lo = lo.min(cols.center.len().saturating_sub(1));
    let hi = hi.min(cols.center.len().saturating_sub(1));
    let x0 = cols.center[lo] - FRAG_PAD;
    let x1 = cols.center[hi] + FRAG_PAD;
    let w = (x1 - x0 + 1).max(2);
    let h = (close_y - open_y + 1).max(2);
    s.rect(x0, open_y, w, h, BoxStyle::Square, border);
    // The keyword tag in the top-left corner, then its bracketed label.
    let tagtext = if label.is_empty() {
        format!("[{tag}]")
    } else {
        format!("[{tag}] {label}")
    };
    s.text_clipped(x0 + 2, open_y, &tagtext, w - 3, text);
}

/// Draws an `else` / `and` / `option` divider line across the innermost open
/// fragment at row `y`, labelled with `tag`/`label`.
#[allow(clippy::too_many_arguments)]
fn draw_divider(
    s: &mut Surface,
    cols: &Cols,
    open_for_depth: &[Option<(String, String, i32)>],
    tag: &str,
    label: &str,
    y: i32,
    border: Style,
    text: Style,
) {
    // The innermost currently-open fragment is the deepest Some entry.
    let depth = open_for_depth.iter().rposition(|o| o.is_some());
    let Some(_d) = depth else { return };
    // Span all participants (a conservative, deterministic width).
    let x0 = cols.center.first().copied().unwrap_or(0) - FRAG_PAD;
    let x1 = cols.center.last().copied().unwrap_or(0) + FRAG_PAD;
    if x1 > x0 {
        s.hline(x0 + 1, y, x1 - x0 - 1, '┄', border);
    }
    let tagtext = if label.is_empty() {
        format!("[{tag}]")
    } else {
        format!("[{tag}] {label}")
    };
    s.text_clipped(x0 + 2, y, &tagtext, x1 - x0 - 3, text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Buffer, Position, Rect, Style};

    /// Renders `src` into a `w`×`h` buffer with the default theme and returns
    /// the glyphs as one newline-terminated line per row (the shared snapshot
    /// idiom from `mermaid::tests::lines`).
    fn lines(src: &str, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        render(
            src,
            Rect::new(0, 0, w, h),
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

    /// Renders `src` into a buffer comfortably larger than any content here,
    /// then crops to the non-blank bounding box and returns it as one
    /// newline-terminated line per row. Cropping the surrounding blank cells
    /// makes an exact snapshot independent of the [`Surface`] blit's centring,
    /// so a pinned image asserts only the drawn diagram.
    fn tight(src: &str) -> String {
        const W: u16 = 120;
        const H: u16 = 80;
        let mut buf = Buffer::empty(Rect::new(0, 0, W, H));
        render(
            src,
            Rect::new(0, 0, W, H),
            &mut buf,
            Style::new(),
            &MermaidTheme::default(),
        );
        let glyph = |x: u16, y: u16| buf.get(Position::new(x, y)).unwrap().symbol;
        let (mut x0, mut y0, mut x1, mut y1) = (W, H, 0u16, 0u16);
        let mut any = false;
        for y in 0..H {
            for x in 0..W {
                if glyph(x, y) != ' ' {
                    any = true;
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        if !any {
            return String::new();
        }
        let mut out = String::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                out.push(glyph(x, y));
            }
            out.push('\n');
        }
        out
    }

    /// Joins expected `rows` into the exact form [`tight`] returns (each row
    /// newline-terminated). Written as a slice so significant leading/trailing
    /// spaces in a row are preserved verbatim (a `"\<newline>"` string
    /// continuation would silently eat them).
    fn snap(rows: &[&str]) -> String {
        let mut s = rows.join("\n");
        s.push('\n');
        s
    }

    // --- parser unit tests -------------------------------------------------

    #[test]
    fn participants_declared_in_source_order() {
        let q = parse("sequenceDiagram\nparticipant Alice\nparticipant Bob");
        assert_eq!(q.ids, ["Alice", "Bob"]);
        assert_eq!(q.labels, ["Alice", "Bob"]);
    }

    #[test]
    fn participant_alias_sets_label_not_id() {
        let q = parse("sequenceDiagram\nparticipant A as Alice\nA->>A: hi");
        assert_eq!(q.ids, ["A"]);
        assert_eq!(q.labels, ["Alice"]);
    }

    #[test]
    fn actor_keyword_declares_participant() {
        let q = parse("sequenceDiagram\nactor Bob");
        assert_eq!(q.ids, ["Bob"]);
    }

    #[test]
    fn message_auto_declares_in_source_order() {
        let q = parse("sequenceDiagram\nAlice->>Bob: Hi\nBob-->>Alice: Yo");
        assert_eq!(q.ids, ["Alice", "Bob"]);
        assert_eq!(q.stmts.len(), 2);
        match &q.stmts[0] {
            Stmt::Message {
                from,
                to,
                kind,
                text,
            } => {
                assert_eq!((*from, *to), (0, 1));
                assert_eq!(*kind, ArrowKind::Solid);
                assert_eq!(text, "Hi");
            }
            _ => panic!("expected message"),
        }
        match &q.stmts[1] {
            Stmt::Message { kind, .. } => assert_eq!(*kind, ArrowKind::DashedSolid),
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn all_arrow_operators_are_recognised() {
        let cases = [
            ("A->>B: x", ArrowKind::Solid),
            ("A-->>B: x", ArrowKind::DashedSolid),
            ("A->B: x", ArrowKind::Open),
            ("A-->B: x", ArrowKind::DashedOpen),
            ("A-)B: x", ArrowKind::Async),
            ("A--)B: x", ArrowKind::DashedAsync),
        ];
        for (line, want) in cases {
            let q = parse(&format!("sequenceDiagram\n{line}"));
            match &q.stmts[0] {
                Stmt::Message { kind, .. } => assert_eq!(*kind, want, "{line}"),
                _ => panic!("expected message for {line}"),
            }
        }
    }

    #[test]
    fn activation_markers_stripped_from_endpoints() {
        let q = parse("sequenceDiagram\nA->>+B: go\nB-->>-A: back");
        assert_eq!(q.ids, ["A", "B"]);
        match &q.stmts[0] {
            Stmt::Message { from, to, .. } => assert_eq!((*from, *to), (0, 1)),
            _ => panic!(),
        }
    }

    #[test]
    fn note_over_two_participants_parses_anchors() {
        let q = parse("sequenceDiagram\nAlice->>Bob: hi\nNote over Alice,Bob: shared");
        match q.stmts.last().unwrap() {
            Stmt::Note { place, a, b, text } => {
                assert_eq!(*place, NotePlace::Over);
                assert_eq!((*a, *b), (0, 1));
                assert_eq!(text, "shared");
            }
            _ => panic!("expected note"),
        }
    }

    #[test]
    fn note_right_and_left_of_parse() {
        let q = parse("sequenceDiagram\nNote right of A: r\nNote left of A: l");
        assert!(matches!(
            q.stmts[0],
            Stmt::Note {
                place: NotePlace::RightOf,
                ..
            }
        ));
        assert!(matches!(
            q.stmts[1],
            Stmt::Note {
                place: NotePlace::LeftOf,
                ..
            }
        ));
    }

    #[test]
    fn fragments_open_divide_and_close() {
        let q = parse("sequenceDiagram\nalt ok\nA->>B: yes\nelse no\nA->>B: nope\nend");
        let kinds: Vec<&str> = q
            .stmts
            .iter()
            .map(|s| match s {
                Stmt::FragStart { .. } => "start",
                Stmt::FragElse { .. } => "else",
                Stmt::FragEnd => "end",
                Stmt::Message { .. } => "msg",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["start", "msg", "else", "msg", "end"]);
        match &q.stmts[0] {
            Stmt::FragStart { tag, label } => {
                assert_eq!(tag, "alt");
                assert_eq!(label, "ok");
            }
            _ => panic!("expected FragStart"),
        }
        match &q.stmts[2] {
            Stmt::FragElse { tag, label } => {
                assert_eq!(tag, "else");
                assert_eq!(label, "no");
            }
            _ => panic!("expected FragElse"),
        }
    }

    #[test]
    fn autonumber_flag_set() {
        let q = parse("sequenceDiagram\nautonumber\nA->>B: x");
        assert!(q.autonumber);
    }

    #[test]
    fn title_keyword_and_frontmatter_title() {
        let q = parse("sequenceDiagram\ntitle My Flow\nA->>B: x");
        assert_eq!(q.title.as_deref(), Some("My Flow"));
        let q2 = parse("---\ntitle: Front\n---\nsequenceDiagram\nA->>B: x");
        assert_eq!(q2.title.as_deref(), Some("Front"));
    }

    #[test]
    fn comments_and_blank_lines_skipped() {
        let q = parse("sequenceDiagram\n\n%% a comment\nA->>B: x\n");
        assert_eq!(q.ids, ["A", "B"]);
        assert_eq!(q.stmts.len(), 1);
    }

    #[test]
    fn unparseable_lines_are_skipped_not_panic() {
        let q = parse("sequenceDiagram\nthis is gibberish\nA->>B: ok\n???");
        assert_eq!(q.ids, ["A", "B"]);
        assert_eq!(q.stmts.len(), 1);
    }

    #[test]
    fn activate_deactivate_recorded() {
        let q = parse("sequenceDiagram\nactivate A\ndeactivate A");
        assert!(matches!(q.stmts[0], Stmt::Activate(0)));
        assert!(matches!(q.stmts[1], Stmt::Deactivate(0)));
    }

    // --- full-render snapshot tests ----------------------------------------

    #[test]
    fn empty_source_renders_placeholder() {
        let out = lines("sequenceDiagram\n", 40, 3);
        assert!(out.contains("sequence"), "{out}");
        assert!(out.contains("no participants"), "{out}");
    }

    #[test]
    fn nonsense_only_renders_placeholder() {
        let out = lines("sequenceDiagram\n%% nothing here\n", 40, 3);
        assert!(out.contains("no participants"), "{out}");
    }

    #[test]
    fn zero_area_does_not_panic() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 0, 0));
        render(
            "sequenceDiagram\nA->>B: hi",
            Rect::new(0, 0, 0, 0),
            &mut buf,
            Style::new(),
            &MermaidTheme::default(),
        );
    }

    #[test]
    fn tiny_area_does_not_panic() {
        for (w, h) in [(1, 1), (2, 1), (1, 3), (3, 2), (5, 4)] {
            let _ = lines(
                "sequenceDiagram\nAlice->>Bob: hello\nNote over Alice,Bob: x",
                w,
                h,
            );
        }
    }

    #[test]
    fn simple_two_participant_message_snapshot() {
        let out = lines("sequenceDiagram\nAlice->>Bob: Hello", 30, 10);
        // Two boxes on the top rows, a dashed lifeline, and the message.
        assert!(out.contains("Alice"), "{out}");
        assert!(out.contains("Bob"), "{out}");
        assert!(out.contains("Hello"), "{out}");
        assert!(out.contains('┊'), "lifeline missing:\n{out}");
        assert!(out.contains('►'), "arrowhead missing:\n{out}");
    }

    #[test]
    fn exact_snapshot_single_message() {
        let out = tight("sequenceDiagram\nA->>B: hi");
        let expected = snap(&[
            "╭───╮             ╭───╮",
            "│ A │             │ B │",
            "╰───╯             ╰───╯",
            "  ┊       hi        ┊  ",
            "  ┊────────────────►┊  ",
            "  ┊                 ┊  ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn exact_snapshot_open_arrow_no_head() {
        let out = tight("sequenceDiagram\nA->B: call");
        let expected = snap(&[
            "╭───╮               ╭───╮",
            "│ A │               │ B │",
            "╰───╯               ╰───╯",
            "  ┊       call        ┊  ",
            "  ┊───────────────────┊  ",
            "  ┊                   ┊  ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
        assert!(!out.contains('►'), "open arrow must have no head:\n{out}");
    }

    #[test]
    fn exact_snapshot_async_arrow() {
        let out = tight("sequenceDiagram\nA-)B: ev");
        let expected = snap(&[
            "╭───╮             ╭───╮",
            "│ A │             │ B │",
            "╰───╯             ╰───╯",
            "  ┊       ev        ┊  ",
            "  ┊────────────────▻┊  ",
            "  ┊                 ┊  ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn exact_snapshot_leftward_message() {
        let out = tight("sequenceDiagram\nparticipant A\nparticipant B\nB->>A: back");
        let expected = snap(&[
            "╭───╮               ╭───╮",
            "│ A │               │ B │",
            "╰───╯               ╰───╯",
            "  ┊       back        ┊  ",
            "  ┊◄──────────────────┊  ",
            "  ┊                   ┊  ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn exact_snapshot_message_without_text() {
        let out = tight("sequenceDiagram\nA->>B");
        let expected = snap(&[
            "╭───╮           ╭───╮",
            "│ A │           │ B │",
            "╰───╯           ╰───╯",
            "  ┊──────────────►┊  ",
            "  ┊               ┊  ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn dashed_return_uses_dashed_line() {
        let out = lines("sequenceDiagram\nA-->>B: ok", 24, 9);
        assert!(out.contains('┄'), "dashed line missing:\n{out}");
        assert!(out.contains('►'), "head missing:\n{out}");
    }

    #[test]
    fn exact_snapshot_self_message_loop() {
        let out = tight("sequenceDiagram\nA->>A: think");
        let expected = snap(&[
            "╭───╮        ",
            "│ A │        ",
            "╰───╯        ",
            "  ┊          ",
            "  ┊───┐      ",
            "  ┊   │ think",
            "  ┊◄──┘      ",
            "  ┊          ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn exact_snapshot_note_over_two_participants() {
        let out = tight("sequenceDiagram\nAlice->>Bob: hi\nNote over Alice,Bob: shared");
        let expected = snap(&[
            "╭───────╮             ╭─────╮",
            "│ Alice │             │ Bob │",
            "╰───────╯             ╰─────╯",
            "    ┊         hi         ┊   ",
            "    ┊───────────────────►┊   ",
            "    ┊                    ┊   ",
            "    ┌────────────────────┐   ",
            "    │       shared       │   ",
            "    └────────────────────┘   ",
            "    ┊                    ┊   ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn exact_snapshot_note_right_of() {
        let out = tight("sequenceDiagram\nNote right of A: hi there");
        let expected = snap(&[
            "╭───╮           ",
            "│ A │           ",
            "╰───╯           ",
            "  ┊ ┌──────────┐",
            "  ┊ │ hi there │",
            "  ┊ └──────────┘",
            "  ┊             ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn note_left_of_draws_box_to_the_left() {
        let out = tight("sequenceDiagram\nparticipant A\nparticipant B\nNote left of B: x");
        assert!(
            out.contains('┌') && out.contains('┘'),
            "note box missing:\n{out}"
        );
        // The note sits left of B's lifeline (it appears before the line in
        // reading order on the note rows).
        let row = out.lines().nth(4).unwrap_or("");
        let box_col = row.find('│').unwrap_or(usize::MAX);
        let life_col = row.rfind('┊').unwrap_or(0);
        assert!(box_col < life_col, "note not left of lifeline:\n{out}");
    }

    #[test]
    fn exact_snapshot_loop_fragment() {
        let out = tight("sequenceDiagram\nloop every minute\nA->>B: poll\nend");
        let expected = snap(&[
            "  ╭───╮               ╭───╮  ",
            "  │ A │               │ B │  ",
            "  ╰───╯               ╰───╯  ",
            "    ┊                   ┊    ",
            "┌─[loop] every minute───────┐",
            "│   ┊       poll        ┊   │",
            "│   ┊──────────────────►┊   │",
            "│   ┊                   ┊   │",
            "│   ┊                   ┊   │",
            "└───────────────────────────┘",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn alt_else_fragment_has_divider() {
        let out = tight("sequenceDiagram\nalt is ok\nA->>B: yes\nelse not ok\nA->>B: no\nend");
        assert!(out.contains("[alt] is ok"), "alt tag missing:\n{out}");
        assert!(
            out.contains("[else] not ok"),
            "else divider missing:\n{out}"
        );
        assert!(out.contains("yes") && out.contains("no"), "{out}");
        // The else divider is a dashed line across the fragment.
        let div = out.lines().find(|l| l.contains("[else]")).unwrap_or("");
        assert!(div.contains('┄'), "divider not dashed: {div:?}");
    }

    #[test]
    fn nested_fragments_render() {
        let out = tight("sequenceDiagram\nloop outer\nopt inner\nA->>B: x\nend\nend");
        assert!(out.contains("[loop] outer"), "{out}");
        assert!(out.contains("[opt] inner"), "{out}");
        assert!(out.contains('x'), "{out}");
        // Outer box fully encloses the inner one (first/last rows are the
        // outer border).
        let first = out.lines().find(|l| l.contains("[loop]")).unwrap();
        assert!(first.starts_with('┌'), "outer top: {first:?}");
        assert!(out.lines().last().unwrap().starts_with('└'), "{out}");
    }

    #[test]
    fn exact_snapshot_autonumber() {
        let out = tight("sequenceDiagram\nautonumber\nA->>B: one\nB->>A: two");
        let expected = snap(&[
            "╭───╮              ╭───╮",
            "│ A │              │ B │",
            "╰───╯              ╰───╯",
            "  ┊      1. one      ┊  ",
            "  ┊─────────────────►┊  ",
            "  ┊                  ┊  ",
            "  ┊      2. two      ┊  ",
            "  ┊◄─────────────────┊  ",
            "  ┊                  ┊  ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn title_is_centered_above_the_diagram() {
        let out = tight("sequenceDiagram\ntitle Login\nA->>B: hi");
        let first = out.lines().next().unwrap_or("");
        assert!(first.contains("Login"), "title row was: {first:?}\n{out}");
        // The title row sits above the participant boxes.
        assert!(
            out.lines().nth(2).is_some_and(|l| l.contains('╭')),
            "boxes not below title:\n{out}"
        );
    }

    #[test]
    fn activate_deactivate_draws_bar() {
        let out = tight("sequenceDiagram\nA->>B: go\nactivate B\nB-->>A: done\ndeactivate B");
        assert!(out.contains('┃'), "activation bar missing:\n{out}");
        // The bar is on B's lifeline (the right column).
        let bar_row = out.lines().find(|l| l.contains('┃')).unwrap();
        let bar_col = bar_row.find('┃').unwrap();
        let life_col = bar_row.find('┊').unwrap_or(0);
        assert!(bar_col > life_col, "bar not on right lifeline:\n{out}");
    }

    #[test]
    fn exact_snapshot_three_participants() {
        let out = tight(
            "sequenceDiagram\nparticipant A\nparticipant B\nparticipant C\nA->>B: x\nB->>C: y",
        );
        let expected = snap(&[
            "╭───╮            ╭───╮            ╭───╮",
            "│ A │            │ B │            │ C │",
            "╰───╯            ╰───╯            ╰───╯",
            "  ┊       x        ┊                ┊  ",
            "  ┊───────────────►┊                ┊  ",
            "  ┊                ┊                ┊  ",
            "  ┊                ┊       y        ┊  ",
            "  ┊                ┊───────────────►┊  ",
            "  ┊                ┊                ┊  ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn frontmatter_block_is_dropped_before_header() {
        let out = tight("---\ntitle: Auth\n---\nsequenceDiagram\nA->>B: login");
        assert!(out.contains("login"), "{out}");
        let first = out.lines().next().unwrap_or("");
        assert!(
            first.contains("Auth"),
            "frontmatter title missing: {first:?}\n{out}"
        );
    }

    #[test]
    fn message_without_text_still_draws_line() {
        let out = lines("sequenceDiagram\nA->>B", 24, 9);
        assert!(out.contains('►'), "{out}");
        assert!(out.contains('─'), "{out}");
    }

    #[test]
    fn participant_alias_label_is_rendered() {
        let out = tight("sequenceDiagram\nparticipant A as Alice\nA->>A: hi");
        assert!(out.contains("Alice"), "alias label missing:\n{out}");
        assert!(!out.contains("│ A │"), "raw id should not show:\n{out}");
    }

    #[test]
    fn par_and_fragment_renders() {
        let out = tight("sequenceDiagram\npar task one\nA->>B: a\nand task two\nA->>B: b\nend");
        assert!(out.contains("[par] task one"), "{out}");
        assert!(out.contains("[and] task two"), "{out}");
    }
}
