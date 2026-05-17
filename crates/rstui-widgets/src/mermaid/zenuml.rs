//! `zenuml` Mermaid renderer — a self-contained parser for the ZenUML
//! sequence DSL plus the same deterministic lifeline layout the
//! `sequenceDiagram` renderer uses, drawn onto the shared
//! [`super::draw::Surface`].
//!
//! ZenUML is sequence-like but its own dialect, so this module parses and
//! lays it out independently (it does *not* call the `sequence` module) while
//! keeping the visual — participant boxes, dashed lifelines, horizontal
//! arrows, bordered fragment regions — consistent with it.
//!
//! # Supported subset
//!
//! - Declarations: `@Actor`, `@Boundary`, `@Database`, … (any `@Word`), and
//!   `@Starter(User)` which both declares `User` and makes it the initiating
//!   sender.
//! - Messages:
//!   - `A.method()` — a call from the current sender to `A`, label `method`.
//!   - `A->B.method()` — an explicit call from `A` to `B`, label `method`.
//!   - `A->B: text` — an explicit message from `A` to `B`, label `text`.
//!   - `return x` / `@return x` — a dashed return arrow back to the previous
//!     sender, label `x`.
//! - Blocks: `if(cond){ … }`, `while(cond){ … }`, `for(cond){ … }`,
//!   `opt { … }`, `par { … }`, `loop { … }`, `try { … }` — a labelled
//!   bordered fragment spanning the involved lifelines, closed by the
//!   matching `}` (nestable). A lone `}` closes the innermost block.
//!
//! # Layout & approximations
//!
//! Identical to the `sequenceDiagram` renderer's: participant boxes in a top
//! row, dashed lifelines (`┊`), one row per message with the label centred
//! above a horizontal arrow (`►` solid call, `┄`/`◄` dashed return), and
//! fragments as plain bordered rectangles with a `[kw] cond` tag — a legible
//! terminal subset, not a pixel-faithful renderer. Everything is integer,
//! source-order and content-sized; the blit centres/clips it like every
//! other diagram.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// A parsed ZenUML statement in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stmt {
    /// A call `from → to` labelled `text`; `ret` marks a dashed return.
    Message {
        /// The sending participant index.
        from: usize,
        /// The receiving participant index.
        to: usize,
        /// The message / method label.
        text: String,
        /// Whether this is a dashed return arrow.
        ret: bool,
    },
    /// The opening of a block fragment (`if` / `while` / `opt` / …).
    BlockStart {
        /// The keyword tag (e.g. `if`).
        tag: String,
        /// The condition / label in parentheses, if any.
        label: String,
    },
    /// The `}` that closes the innermost open block.
    BlockEnd,
}

/// The parsed diagram: participants in declaration/first-use order, their
/// labels and the statement stream.
#[derive(Debug, Default)]
struct Zen {
    /// Stable participant ids, first-seen order.
    ids: Vec<String>,
    /// Display label per participant (currently the id).
    labels: Vec<String>,
    /// The ordered statement stream.
    stmts: Vec<Stmt>,
}

impl Zen {
    /// Index of participant `id`, declaring it in source order on first use.
    fn participant(&mut self, id: &str) -> usize {
        let id = id.trim();
        if let Some(i) = self.ids.iter().position(|p| p == id) {
            return i;
        }
        self.ids.push(id.to_owned());
        self.labels.push(id.to_owned());
        self.ids.len() - 1
    }
}

/// The block keywords ZenUML opens a `{ … }` fragment with.
const BLOCK_KW: &[&str] = &[
    "if", "else", "while", "for", "forEach", "loop", "opt", "par", "try", "catch", "finally",
    "critical", "group", "section",
];

/// Splits a leading identifier (letters/digits/`_`) off `s`, returning
/// `(ident, rest)` with `rest` still holding any trailing syntax.
fn lead_ident(s: &str) -> (&str, &str) {
    let end = s
        .char_indices()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    (&s[..end], &s[end..])
}

/// Strips a trailing `{` (a block opener glued to the statement) and returns
/// whether one was present.
fn split_open_brace(line: &str) -> (&str, bool) {
    match line.strip_suffix('{') {
        Some(head) => (head.trim_end(), true),
        None => (line, false),
    }
}

/// Parses the whole source into a [`Zen`]. Lenient: an unrecognised line is
/// skipped, never an error.
fn parse(src: &str) -> Zen {
    let mut zen = Zen::default();
    // The implicit current sender; ZenUML threads calls from it.
    let mut sender: Option<usize> = None;
    // The stack of senders so a closing `}` / `return` can pop back.
    let mut sender_stack: Vec<Option<usize>> = Vec::new();

    let mut header_seen = false;
    let mut frontmatter = false;
    for raw in src.split('\n') {
        let mut line = raw.strip_suffix('\r').unwrap_or(raw).trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        // Drop a leading `--- … ---` frontmatter block.
        if !header_seen && line == "---" {
            frontmatter = !frontmatter;
            continue;
        }
        if frontmatter {
            continue;
        }
        if !header_seen {
            if line.starts_with("zenuml") {
                header_seen = true;
                continue;
            }
            header_seen = true;
        }

        // Trailing line comment `// …`.
        if let Some(i) = line.find("//") {
            line = line[..i].trim();
            if line.is_empty() {
                continue;
            }
        }

        // A bare `}` closes the innermost block and pops the sender.
        if line == "}" {
            zen.stmts.push(Stmt::BlockEnd);
            if let Some(prev) = sender_stack.pop() {
                sender = prev;
            }
            continue;
        }
        // `} else {` / `} catch(e) {` — close then reopen on the same line.
        if let Some(rest) = line.strip_prefix('}') {
            zen.stmts.push(Stmt::BlockEnd);
            if let Some(prev) = sender_stack.pop() {
                sender = prev;
            }
            line = rest.trim();
            if line.is_empty() {
                continue;
            }
        }

        // `@Starter(User)` — declare and make `User` the initiating sender.
        if let Some(rest) = line.strip_prefix("@Starter(") {
            if let Some(close) = rest.find(')') {
                let id = rest[..close].trim();
                if !id.is_empty() {
                    let i = zen.participant(id);
                    sender = Some(i);
                }
            }
            continue;
        }
        // `@return x` / `return x` — a return arrow to the previous sender.
        let ret_body = line.strip_prefix("@return").or_else(|| {
            if line == "return" || line.starts_with("return ") {
                Some(&line["return".len()..])
            } else {
                None
            }
        });
        if let Some(rest) = ret_body {
            if let Some(cur) = sender {
                let target = sender_stack.iter().rev().find_map(|s| *s).unwrap_or(cur);
                zen.stmts.push(Stmt::Message {
                    from: cur,
                    to: target,
                    text: rest.trim().trim_end_matches(';').trim().to_owned(),
                    ret: true,
                });
            }
            continue;
        }
        // A participant declaration:
        //   `@Name`                 → participant `Name`
        //   `@Stereotype Name`      → participant `Name` (stereotype dropped)
        //   `@Stereotype "Disp" as Alias` → participant `Alias`
        // Anything past that we don't model is ignored, but a trailing
        // message body on the same line is still parsed.
        if let Some(rest) = line.strip_prefix('@') {
            let (first, tail) = lead_ident(rest);
            let tail = tail.trim();
            if !first.is_empty() {
                if tail.is_empty() {
                    zen.participant(first);
                    continue;
                }
                // `@Stereotype Name …` — the name is the next identifier.
                let (name, more) = lead_ident(tail);
                if !name.is_empty() {
                    let alias = more
                        .trim()
                        .strip_prefix("as ")
                        .map(|a| lead_ident(a.trim()).0);
                    zen.participant(alias.filter(|a| !a.is_empty()).unwrap_or(name));
                    continue;
                }
                // Fall through: parse whatever follows `@first ` as a body.
                line = tail;
            }
        }

        // A block opener: `kw(cond){` or `kw {` or `kw{`.
        let (head, opened) = split_open_brace(line);
        let (kw, after_kw) = lead_ident(head);
        if BLOCK_KW.contains(&kw) && (opened || after_kw.trim_start().starts_with('(')) {
            let label = {
                let a = after_kw.trim();
                if let Some(inner) = a.strip_prefix('(') {
                    inner
                        .rsplit_once(')')
                        .map(|(c, _)| c)
                        .unwrap_or(inner)
                        .trim()
                        .to_owned()
                } else {
                    String::new()
                }
            };
            zen.stmts.push(Stmt::BlockStart {
                tag: kw.to_owned(),
                label,
            });
            sender_stack.push(sender);
            continue;
        }

        // A message. Strip an opener brace if the call also starts a block
        // (`A.method() {`), then parse the call forms.
        let (mut body, msg_opens) = split_open_brace(line);
        body = body.trim();

        // `A->B: text` or `A->B.method()`.
        if let Some((lhs, rhs)) = body.split_once("->") {
            let from = zen.participant(lhs.trim());
            let (to, label) = parse_target(rhs.trim());
            let to_i = zen.participant(&to);
            zen.stmts.push(Stmt::Message {
                from,
                to: to_i,
                text: label,
                ret: false,
            });
            sender = Some(to_i);
            if msg_opens {
                zen.stmts.push(Stmt::BlockStart {
                    tag: "alt".to_owned(),
                    label: String::new(),
                });
                sender_stack.push(Some(from));
            }
            continue;
        }

        // `A.method()` — current sender calls `A`.
        if let Some(dot) = body.find('.') {
            let to = body[..dot].trim();
            if !to.is_empty() && to.chars().all(|c| c.is_alphanumeric() || c == '_') {
                let to_i = zen.participant(to);
                let label = clean_call(&body[dot + 1..]);
                let from = sender.unwrap_or(to_i);
                zen.stmts.push(Stmt::Message {
                    from,
                    to: to_i,
                    text: label,
                    ret: false,
                });
                sender = Some(to_i);
                if msg_opens {
                    zen.stmts.push(Stmt::BlockStart {
                        tag: "alt".to_owned(),
                        label: String::new(),
                    });
                    sender_stack.push(Some(from));
                }
                continue;
            }
        }
        // Anything else: silently skipped (lenient).
    }
    zen
}

/// Parses an arrow target `B.method()` / `B: text` / `B` into
/// `(participant, label)`.
fn parse_target(rhs: &str) -> (String, String) {
    if let Some((p, t)) = rhs.split_once(':') {
        return (p.trim().to_owned(), t.trim().to_owned());
    }
    if let Some(dot) = rhs.find('.') {
        let p = rhs[..dot].trim().to_owned();
        return (p, clean_call(&rhs[dot + 1..]));
    }
    (rhs.trim().to_owned(), String::new())
}

/// Normalises a method call body `method(args);` to a compact `method(args)`
/// label (drops a trailing `;` and surrounding whitespace).
fn clean_call(s: &str) -> String {
    s.trim().trim_end_matches(';').trim().to_owned()
}

/// Per-participant horizontal geometry.
struct Cols {
    /// Centre `x` of each lifeline.
    center: Vec<i32>,
    /// Left `x` of each participant box.
    left: Vec<i32>,
    /// Width of each participant box.
    box_w: Vec<i32>,
    /// Total surface width.
    width: i32,
}

/// Minimum centre-to-centre lifeline gap (room for a short label + head).
const MIN_GAP: i32 = 8;
/// Inner box padding.
const PAD: i32 = 1;
/// Fragment border overhang beyond the outer involved lifeline.
const FRAG_PAD: i32 = 4;

/// Computes column geometry, widening any gap a message label has to span.
fn columns(zen: &Zen) -> Cols {
    let mut box_w: Vec<i32> = zen
        .labels
        .iter()
        .map(|l| (l.chars().count() as i32 + 2 * PAD + 2).max(5))
        .collect();
    if box_w.is_empty() {
        box_w.push(5);
    }
    let n = box_w.len();
    let mut gap_need = vec![MIN_GAP; n.saturating_sub(1).max(1)];
    for st in &zen.stmts {
        if let Stmt::Message { from, to, text, .. } = st {
            if from == to {
                continue;
            }
            let (lo, hi) = ((*from).min(*to), (*from).max(*to));
            let spans = (hi - lo).max(1) as i32;
            for b in lo..hi {
                if b < gap_need.len() {
                    let share = (text.chars().count() as i32 + 4) / spans + MIN_GAP;
                    gap_need[b] = gap_need[b].max(share);
                }
            }
        }
    }
    let mut center = Vec::with_capacity(n);
    let mut left = Vec::with_capacity(n);
    let mut cx = box_w[0] / 2;
    for i in 0..n {
        center.push(cx);
        left.push(cx - box_w[i] / 2);
        if i + 1 < n {
            let g = gap_need.get(i).copied().unwrap_or(MIN_GAP).max(MIN_GAP);
            cx += box_w[i] / 2 + g + box_w[i + 1] / 2;
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

/// Renders a `zenuml` diagram from `src` into `area`.
///
/// Parses the supported subset, lays it out deterministically and blits a
/// single content-sized [`Surface`]. With no participants it draws the shared
/// honest placeholder and returns.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let zen = parse(src);
    if zen.ids.is_empty() {
        super::diagram_placeholder("zenuml", "no participants", area, buf, base, theme);
        return;
    }

    let mut cols = columns(&zen);
    // Reserve fragment overhang on both sides when any block exists, and
    // enough room to the right for the widest self-call loop + its label so
    // it is never clipped.
    let mut margin = 0;
    if zen
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::BlockStart { .. }))
    {
        margin = FRAG_PAD;
    }
    let mut right_margin = margin;
    for st in &zen.stmts {
        if let Stmt::Message { from, to, text, .. } = st {
            if from == to {
                let reach = cols.center[*from] + 4 + 2 + text.chars().count() as i32;
                right_margin = right_margin.max(reach - cols.width + 1);
            }
        }
    }
    for c in &mut cols.center {
        *c += margin;
    }
    for l in &mut cols.left {
        *l += margin;
    }
    cols.width += margin + right_margin.max(margin);

    let border = base.patch(theme.node_border);
    let label_st = base.patch(theme.node_label);
    let edge = base.patch(theme.edge);
    let msg_st = base.patch(theme.edge_label);
    let frag_st = base.patch(theme.cluster);

    let header_top = 0;
    let header_h = 3;
    let lifeline_top = header_h;

    // Plan a row per statement so the surface can be sized first.
    #[derive(Clone)]
    enum Row {
        Msg {
            from: usize,
            to: usize,
            text: String,
            ret: bool,
            y: i32,
        },
        Open {
            tag: String,
            label: String,
            y: i32,
            depth: usize,
        },
        Close {
            y: i32,
            open_y: i32,
            lo: usize,
            hi: usize,
            depth: usize,
        },
    }

    let mut rows: Vec<Row> = Vec::new();
    let mut y = lifeline_top;
    // (open_row_index, min_part, max_part) per open block.
    let mut stack: Vec<(usize, usize, usize)> = Vec::new();

    for st in &zen.stmts {
        match st {
            Stmt::Message {
                from,
                to,
                text,
                ret,
            } => {
                let self_msg = from == to;
                if !text.is_empty() {
                    y += 1;
                }
                let row_y = y;
                y += if self_msg { 3 } else { 1 };
                y += 1;
                rows.push(Row::Msg {
                    from: *from,
                    to: *to,
                    text: text.clone(),
                    ret: *ret,
                    y: row_y,
                });
                for f in &mut stack {
                    f.1 = f.1.min(*from).min(*to);
                    f.2 = f.2.max(*from).max(*to);
                }
            }
            Stmt::BlockStart { tag, label } => {
                y += 1;
                let idx = rows.len();
                rows.push(Row::Open {
                    tag: tag.clone(),
                    label: label.clone(),
                    y,
                    depth: stack.len(),
                });
                y += 1;
                stack.push((idx, usize::MAX, 0));
            }
            Stmt::BlockEnd => {
                if let Some((open_idx, mut lo, mut hi)) = stack.pop() {
                    if lo == usize::MAX {
                        lo = 0;
                        hi = zen.ids.len().saturating_sub(1);
                    }
                    let (open_y, depth) = match &rows[open_idx] {
                        Row::Open { y, depth, .. } => (*y, *depth),
                        _ => (0, 0),
                    };
                    y += 1;
                    rows.push(Row::Close {
                        y,
                        open_y,
                        lo,
                        hi,
                        depth,
                    });
                    y += 1;
                }
            }
        }
    }
    // Close any block left open at EOF (lenient).
    while let Some((open_idx, mut lo, mut hi)) = stack.pop() {
        if lo == usize::MAX {
            lo = 0;
            hi = zen.ids.len().saturating_sub(1);
        }
        let (open_y, depth) = match &rows[open_idx] {
            Row::Open { y, depth, .. } => (*y, *depth),
            _ => (0, 0),
        };
        y += 1;
        rows.push(Row::Close {
            y,
            open_y,
            lo,
            hi,
            depth,
        });
        y += 1;
    }

    let lifeline_bottom = y.max(lifeline_top + 1);
    let surf_w = cols.width.max(2);
    let mut s = Surface::new(surf_w, lifeline_bottom.max(1));

    // Lifelines then participant boxes (boxes overpaint their slice).
    for i in 0..zen.ids.len() {
        let cx = cols.center[i];
        for ly in lifeline_top..lifeline_bottom {
            s.set(cx, ly, '┊', edge);
        }
        s.labeled_box(
            cols.left[i],
            header_top,
            cols.box_w[i],
            header_h,
            BoxStyle::Round,
            &zen.labels[i],
            border,
            label_st,
        );
    }

    // Messages.
    for r in &rows {
        if let Row::Msg {
            from,
            to,
            text,
            ret,
            y,
        } = r
        {
            if from == to {
                draw_self(&mut s, cols.center[*from], *y, *ret, text, edge, msg_st);
            } else {
                draw_arrow(
                    &mut s,
                    cols.center[*from],
                    cols.center[*to],
                    *y,
                    *ret,
                    text,
                    edge,
                    msg_st,
                );
            }
        }
    }

    // Fragment regions last so their borders sit over the lifelines.
    let mut open_for_depth: Vec<Option<(String, String)>> = Vec::new();
    for r in &rows {
        match r {
            Row::Open {
                tag, label, depth, ..
            } => {
                if open_for_depth.len() <= *depth {
                    open_for_depth.resize(*depth + 1, None);
                }
                open_for_depth[*depth] = Some((tag.clone(), label.clone()));
            }
            Row::Close {
                y,
                open_y,
                lo,
                hi,
                depth,
            } => {
                let (tag, label) = open_for_depth
                    .get(*depth)
                    .and_then(|o| o.clone())
                    .unwrap_or_else(|| (String::from("block"), String::new()));
                draw_fragment(
                    &mut s, &cols, *lo, *hi, *open_y, *y, &tag, &label, frag_st, msg_st,
                );
            }
            Row::Msg { .. } => {}
        }
    }

    s.blit(area, buf, base);
}

/// Draws a horizontal call/return arrow between two lifelines on row `y`,
/// label centred on the row above. `ret` makes it a dashed `┄`/head return.
#[allow(clippy::too_many_arguments)]
fn draw_arrow(
    s: &mut Surface,
    cx_from: i32,
    cx_to: i32,
    y: i32,
    ret: bool,
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
    let line_ch = if ret { '┄' } else { '─' };
    let lo = a + 1;
    let hi = b - 1;
    if hi >= lo {
        s.hline(lo, y, hi - lo + 1, line_ch, edge);
    }
    let head = if left { '◄' } else { '►' };
    let hx = if left { a + 1 } else { b - 1 };
    s.set(hx.clamp(a, b), y, head, edge);
    if !label.is_empty() {
        let width = (b - a - 1).max(1);
        s.text_centered(a + 1, y - 1, width, label, text);
    }
}

/// Draws a self-call as a small right-side rectangular loop on the lifeline
/// at `cx`, occupying rows `y..y+3`, label to its right.
fn draw_self(s: &mut Surface, cx: i32, y: i32, ret: bool, label: &str, edge: Style, text: Style) {
    let line_ch = if ret { '┄' } else { '─' };
    let rx = cx + 4;
    // Legs stop one cell short of `rx` so the corner glyphs are not
    // overwritten by the horizontal runs.
    s.hline(cx + 1, y, rx - cx - 1, line_ch, edge);
    s.hline(cx + 2, y + 2, rx - cx - 2, line_ch, edge);
    s.set(rx, y, '┐', edge);
    s.set(rx, y + 1, '│', edge);
    s.set(rx, y + 2, '┘', edge);
    // The return arrowhead points back into the lifeline.
    s.set(cx + 1, y + 2, '◄', edge);
    if !label.is_empty() {
        s.text(rx + 2, y + 1, label, text);
    }
}

/// Draws a labelled fragment rectangle spanning participants `lo..=hi` from
/// row `open_y` to `close_y`, `[tag] label` in the top-left corner.
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
    let tagtext = if label.is_empty() {
        format!("[{tag}]")
    } else {
        format!("[{tag}] {label}")
    };
    s.text_clipped(x0 + 2, open_y, &tagtext, w - 3, text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Buffer, Position, Rect, Style};

    /// Renders `src` into a `w`×`h` buffer with the default theme and reads
    /// the glyphs back as one newline-terminated line per row (the shared
    /// `mermaid::tests::lines` snapshot idiom).
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

    /// Renders into a generous buffer, then crops to the non-blank bounding
    /// box so an exact snapshot is independent of the blit's centring.
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
        let g = |x: u16, y: u16| buf.get(Position::new(x, y)).unwrap().symbol;
        let (mut x0, mut y0, mut x1, mut y1) = (W, H, 0u16, 0u16);
        let mut any = false;
        for y in 0..H {
            for x in 0..W {
                if g(x, y) != ' ' {
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
                out.push(g(x, y));
            }
            out.push('\n');
        }
        out
    }

    /// Joins expected `rows` exactly as [`tight`] returns them (each row
    /// newline-terminated), preserving significant trailing spaces.
    fn snap(rows: &[&str]) -> String {
        let mut s = rows.join("\n");
        s.push('\n');
        s
    }

    // --- parser unit tests -------------------------------------------------

    #[test]
    fn at_declaration_registers_participant() {
        let z = parse("zenuml\n@Actor User\n@Database DB");
        assert_eq!(z.ids, ["User", "DB"]);
    }

    #[test]
    fn starter_sets_initiating_sender() {
        let z = parse("zenuml\n@Starter(User)\nA.run()");
        // User is declared first, then A; the call is User -> A.
        assert_eq!(z.ids, ["User", "A"]);
        match &z.stmts[0] {
            Stmt::Message { from, to, text, .. } => {
                assert_eq!((*from, *to), (0, 1));
                assert_eq!(text, "run()");
            }
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn dot_call_threads_from_previous_sender() {
        let z = parse("zenuml\n@Starter(U)\nA.first()\nB.second()");
        // U->A, then A->B (A became the sender after the first call).
        assert_eq!(z.ids, ["U", "A", "B"]);
        match (&z.stmts[0], &z.stmts[1]) {
            (
                Stmt::Message {
                    from: f0, to: t0, ..
                },
                Stmt::Message {
                    from: f1, to: t1, ..
                },
            ) => {
                assert_eq!((*f0, *t0), (0, 1));
                assert_eq!((*f1, *t1), (1, 2));
            }
            _ => panic!("expected two messages"),
        }
    }

    #[test]
    fn explicit_arrow_with_method() {
        let z = parse("zenuml\nA->B.process()");
        assert_eq!(z.ids, ["A", "B"]);
        match &z.stmts[0] {
            Stmt::Message {
                from,
                to,
                text,
                ret,
            } => {
                assert_eq!((*from, *to), (0, 1));
                assert_eq!(text, "process()");
                assert!(!ret);
            }
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn explicit_arrow_with_text_message() {
        let z = parse("zenuml\nA->B: hello there");
        match &z.stmts[0] {
            Stmt::Message { from, to, text, .. } => {
                assert_eq!((*from, *to), (0, 1));
                assert_eq!(text, "hello there");
            }
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn return_is_a_dashed_message_back() {
        let z = parse("zenuml\n@Starter(U)\nA.go() {\nreturn done\n}");
        // Inside A's activation, `return done` goes A -> U.
        let ret = z
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::Message {
                    ret: true,
                    from,
                    to,
                    text,
                } => Some((*from, *to, text.clone())),
                _ => None,
            })
            .expect("a return message");
        assert_eq!(ret.0, 1); // from A
        assert_eq!(ret.1, 0); // back to U
        assert_eq!(ret.2, "done");
    }

    #[test]
    fn if_block_opens_and_closes() {
        let z = parse("zenuml\nif(ok) {\nA.x()\n}");
        let kinds: Vec<&str> = z
            .stmts
            .iter()
            .map(|s| match s {
                Stmt::BlockStart { .. } => "open",
                Stmt::BlockEnd => "close",
                Stmt::Message { .. } => "msg",
            })
            .collect();
        assert_eq!(kinds, ["open", "msg", "close"]);
        match &z.stmts[0] {
            Stmt::BlockStart { tag, label } => {
                assert_eq!(tag, "if");
                assert_eq!(label, "ok");
            }
            _ => panic!("expected BlockStart"),
        }
    }

    #[test]
    fn while_and_opt_blocks_recognised() {
        let z = parse("zenuml\nwhile(more) {\nA.tick()\n}\nopt {\nA.maybe()\n}");
        let tags: Vec<String> = z
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::BlockStart { tag, .. } => Some(tag.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(tags, ["while", "opt"]);
    }

    #[test]
    fn nested_blocks_balance() {
        let z = parse("zenuml\nif(a) {\nwhile(b) {\nX.y()\n}\n}");
        let opens = z
            .stmts
            .iter()
            .filter(|s| matches!(s, Stmt::BlockStart { .. }))
            .count();
        let closes = z
            .stmts
            .iter()
            .filter(|s| matches!(s, Stmt::BlockEnd))
            .count();
        assert_eq!((opens, closes), (2, 2));
    }

    #[test]
    fn comments_and_blanks_skipped() {
        let z = parse("zenuml\n\n// a comment\nA.run() // trailing\n");
        assert_eq!(z.ids, ["A"]);
        assert_eq!(z.stmts.len(), 1);
    }

    #[test]
    fn unparseable_lines_skipped_not_panic() {
        let z = parse("zenuml\n!!! junk ???\nA->B: ok\n@@@");
        assert_eq!(z.ids, ["A", "B"]);
        assert_eq!(z.stmts.len(), 1);
    }

    #[test]
    fn frontmatter_block_is_dropped() {
        let z = parse("---\ntitle: T\n---\nzenuml\nA->B: hi");
        assert_eq!(z.ids, ["A", "B"]);
        assert_eq!(z.stmts.len(), 1);
    }

    // --- full-render snapshot tests ----------------------------------------

    #[test]
    fn empty_source_renders_placeholder() {
        let out = lines("zenuml\n", 40, 3);
        assert!(out.contains("zenuml"), "{out}");
        assert!(out.contains("no participants"), "{out}");
    }

    #[test]
    fn only_comments_renders_placeholder() {
        let out = lines("zenuml\n// nothing\n", 40, 3);
        assert!(out.contains("no participants"), "{out}");
    }

    #[test]
    fn zero_area_does_not_panic() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 0, 0));
        render(
            "zenuml\nA.run()",
            Rect::new(0, 0, 0, 0),
            &mut buf,
            Style::new(),
            &MermaidTheme::default(),
        );
    }

    #[test]
    fn tiny_area_does_not_panic() {
        for (w, h) in [(1, 1), (2, 1), (1, 3), (3, 2), (6, 4)] {
            let _ = lines("zenuml\n@Starter(U)\nA.go() {\nB.x()\nreturn ok\n}", w, h);
        }
    }

    #[test]
    fn exact_snapshot_explicit_call() {
        let out = tight("zenuml\nA->B.process()");
        let expected = snap(&[
            "╭───╮                    ╭───╮",
            "│ A │                    │ B │",
            "╰───╯                    ╰───╯",
            "  ┊       process()        ┊  ",
            "  ┊───────────────────────►┊  ",
            "  ┊                        ┊  ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn exact_snapshot_two_step_thread() {
        let out = tight("zenuml\n@Starter(U)\nA.first()\nB.second()");
        let expected = snap(&[
            "╭───╮                  ╭───╮                   ╭───╮",
            "│ U │                  │ A │                   │ B │",
            "╰───╯                  ╰───╯                   ╰───╯",
            "  ┊       first()        ┊                       ┊  ",
            "  ┊─────────────────────►┊                       ┊  ",
            "  ┊                      ┊                       ┊  ",
            "  ┊                      ┊       second()        ┊  ",
            "  ┊                      ┊──────────────────────►┊  ",
            "  ┊                      ┊                       ┊  ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn exact_snapshot_if_block() {
        let out = tight("zenuml\nif(ok) {\nA->B: do it\n}");
        let expected = snap(&[
            "  ╭───╮                ╭───╮  ",
            "  │ A │                │ B │  ",
            "  ╰───╯                ╰───╯  ",
            "    ┊                    ┊    ",
            "┌─[if] ok────────────────────┐",
            "│   ┊       do it        ┊   │",
            "│   ┊───────────────────►┊   │",
            "│   ┊                    ┊   │",
            "│   ┊                    ┊   │",
            "└────────────────────────────┘",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn explicit_text_message_snapshot() {
        let out = tight("zenuml\nA->B: hello");
        let expected = snap(&[
            "╭───╮                ╭───╮",
            "│ A │                │ B │",
            "╰───╯                ╰───╯",
            "  ┊       hello        ┊  ",
            "  ┊───────────────────►┊  ",
            "  ┊                    ┊  ",
        ]);
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn return_arrow_is_dashed() {
        let out = tight("zenuml\n@Starter(U)\nA.go() {\nreturn done\n}");
        assert!(out.contains('┄'), "return not dashed:\n{out}");
        assert!(out.contains("done"), "{out}");
        assert!(out.contains("go()"), "{out}");
    }

    #[test]
    fn while_block_renders_tag() {
        let out = tight("zenuml\nwhile(more) {\nA->B: tick\n}");
        assert!(out.contains("[while] more"), "{out}");
        assert!(out.contains("tick"), "{out}");
    }

    #[test]
    fn nested_blocks_render_enclosed() {
        let out = tight("zenuml\nif(a) {\nwhile(b) {\nX->Y: z\n}\n}");
        assert!(out.contains("[if] a"), "{out}");
        assert!(out.contains("[while] b"), "{out}");
        let first = out.lines().find(|l| l.contains("[if]")).unwrap();
        assert!(first.starts_with('┌'), "outer top: {first:?}\n{out}");
        assert!(out.lines().last().unwrap().starts_with('└'), "{out}");
    }

    #[test]
    fn self_call_draws_loop() {
        let out = tight("zenuml\nA.recurse()");
        assert!(out.contains("recurse()"), "{out}");
        assert!(
            out.contains('┐') && out.contains('┘'),
            "loop missing:\n{out}"
        );
    }

    #[test]
    fn leftward_return_points_left() {
        // U->A then A returns to U: the return arrow points left.
        let out = tight("zenuml\n@Starter(U)\nA.go() {\nreturn r\n}");
        assert!(out.contains('◄'), "left head missing:\n{out}");
    }

    #[test]
    fn three_participants_each_have_a_lifeline() {
        let out = tight("zenuml\n@Starter(U)\nA.a()\nB.b()");
        assert!(
            out.contains("U") && out.contains("A") && out.contains("B"),
            "{out}"
        );
        assert!(
            out.matches('┊').count() >= 3,
            "expected multiple lifelines:\n{out}"
        );
    }
}
