//! [`Diff`] — a read-only widget that parses a unified diff and renders it
//! into the styled-text model with a line-number gutter, a three-color change
//! scheme, and intra-line word highlighting, terminal-width aware.
//!
//! # Why a hand-written scanner
//!
//! rstui is deliberately dependency-free below the backend (see
//! [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)
//! §4: a widget that pulls a transitive dependency is feature-gated, and an
//! own-crate split is reserved for *heavy, optional, conceptually alien*
//! engines — never pre-emptively). A unified-diff *grammar* is none of those:
//! it is a handful of line-oriented prefixes (`diff --git`, `--- `/`+++ `,
//! `@@ … @@`, then a one-char body sign), and the only real algorithm — the
//! word-level intra-line diff — is a textbook LCS over tokens, the same way
//! [`Markdown`](crate::Markdown)'s parser is hand-written rather than pulling a
//! CommonMark crate. So `Diff` is a plain [`Widget`](rstui_core::Widget)
//! module here, zero new dependencies.
//!
//! # A real subset, not a fake renderer
//!
//! This is a real, tested subset of the unified-diff format — not a
//! placeholder that pretends to be complete. Supported now:
//!
//! - patches split into files on `diff --git` or a fresh `--- ` header
//! - file headers `--- path` / `+++ path` (a leading `a/`/`b/` is stripped);
//!   a `/dev/null` side marks an added or deleted file
//! - hunk headers `@@ -l,s +l,s @@ optional section`, with the counts
//!   omittable (`@@ -1 +1 @@` ⇒ count 1), and the trailing section label
//!   echoed on the hunk row
//! - body lines typed by their first column: space → context, `+` → added,
//!   `-` → deleted, `\` → the "no newline at end of file" marker
//! - a left gutter of old line number, new line number, and the change sign,
//!   padded to the widest number so columns stay aligned
//! - intra-line **word highlight**: within a change group a deletion is paired
//!   positionally with its addition, a token-level LCS marks the differing
//!   runs, and only those runs get a strengthened emphasis background — so a
//!   one-word edit reads as one word changed, not two whole lines
//! - a trailing `\r` is stripped so a CRLF diff renders clean; trailing blank
//!   lines are dropped before parsing
//!
//! Deliberately out of scope for this slice (each an additive follow-up that
//! does not change this shape): side-by-side / split layout, syntax
//! highlighting of the code itself, combined (merge, `@@@`) diffs, `git`
//! `rename`/`copy`/`mode`/`index` metadata lines (they are simply ignored, not
//! rendered), and binary-file patches.
//!
//! Rendering is deterministic and width-aware: the same patch and area always
//! produce the same cells, so output is snapshot-testable through
//! [`Buffer`](rstui_core::Buffer) exactly like every other widget. Malformed
//! input never panics — an unparseable line renders best-effort as context.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Position, Rect, Widget};
//! use rstui_widgets::Diff;
//!
//! let patch = "\
//! --- a/greet.txt
//! +++ b/greet.txt
//! @@ -1 +1 @@
//! -hello
//! +hallo
//! ";
//! let mut buf = Buffer::empty(Rect::new(0, 0, 12, 4));
//! Diff::new(patch).render(buf.area(), &mut buf);
//!
//! // Row 0 is the file header, rows 2/3 the changed lines; the body sign
//! // sits in the gutter, the content follows it.
//! let row3: String = (0..12)
//!     .map(|x| buf.get(Position::new(x, 3)).unwrap().symbol)
//!     .collect();
//! assert!(row3.contains('+'));
//! assert!(row3.contains("hallo"));
//! ```

use std::borrow::Cow;

use crate::block::Block;
use rstui_core::{Buffer, Color, Line, Modifier, Rect, Span, Style, Widget};

/// Lines longer than this skip the (quadratic) intra-line word diff and fall
/// back to a whole-line highlight. A pathological minified line should not cost
/// an LCS table proportional to its length squared.
const INTRA_LINE_MAX: usize = 2000;

/// The styles [`Diff`] applies to each kind of row.
///
/// Every field is a *patch* layered over the widget base style (itself layered
/// over the framing [`Block`] fill), so an unset color or modifier falls
/// through rather than overriding the surrounding theme — the same
/// [`Style::patch`](rstui_core::Style) cascade the text model uses. Construct
/// the tuned terminal default with [`DiffTheme::default`] and override only the
/// fields you care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffTheme {
    /// An added (`+`) line, gutter and content.
    pub addition: Style,
    /// A deleted (`-`) line, gutter and content.
    pub deletion: Style,
    /// An unchanged context line and its gutter.
    pub context: Style,
    /// A `@@ … @@` hunk header row.
    pub hunk: Style,
    /// A file header row (the `--- `/`+++ ` paths).
    pub file: Style,
    /// The gutter columns (line numbers + sign). Layered *under* the row's
    /// add/del/context style so the numbers stay legible but tinted.
    pub gutter: Style,
    /// Painted on top of an added line's changed word runs (the intra-line
    /// emphasis). A background that strengthens the addition color.
    pub word_added: Style,
    /// Painted on top of a deleted line's changed word runs.
    pub word_deleted: Style,
}

impl Default for DiffTheme {
    fn default() -> Self {
        Self {
            addition: Style::new().fg(Color::Green),
            deletion: Style::new().fg(Color::Red),
            context: Style::new().add_modifier(Modifier::DIM),
            hunk: Style::new().fg(Color::Cyan),
            file: Style::new().add_modifier(Modifier::BOLD),
            gutter: Style::new().fg(Color::DarkGray),
            word_added: Style::new()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
            word_deleted: Style::new()
                .bg(Color::Red)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        }
    }
}

/// A read-only unified-diff view: parses its source once at render time and
/// draws the supported subset into the area, width-aware and deterministic.
///
/// The source is a [`Cow<str>`](std::borrow::Cow) (a literal borrows, a
/// `String` is owned). Parsing produces owned display lines, so the rendered
/// spans are independent of the source lifetime. An optional framing
/// [`Block`], a base [`Style`] that also fills the content area, a vertical
/// scroll offset, and a [`DiffTheme`] are the only knobs — everything else is
/// derived from the patch.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Block, Diff};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 18, 4));
/// Diff::new("@@ -1 +1 @@\n-old\n+new")
///     .block(Block::bordered())
///     .render(buf.area(), &mut buf);
///
/// // Framed, with the hunk header on the first inner row.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
/// assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, '@');
/// ```
#[derive(Debug, Clone)]
pub struct Diff<'a> {
    source: Cow<'a, str>,
    block: Option<Block<'a>>,
    style: Style,
    scroll: u16,
    theme: DiffTheme,
}

impl<'a> Diff<'a> {
    /// A diff view of `source` with the default theme, no block, no scroll.
    pub fn new(source: impl Into<Cow<'a, str>>) -> Self {
        Self {
            source: source.into(),
            block: None,
            style: Style::new(),
            scroll: 0,
            theme: DiffTheme::default(),
        }
    }

    /// Frames the patch in `block`; content renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`] beneath the theme cascade. It also fills the
    /// content area so a background covers the whole region.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Skips the first `offset` composed rows: the vertical scroll position
    /// for a patch taller than its area.
    #[must_use]
    pub fn scroll(mut self, offset: u16) -> Self {
        self.scroll = offset;
        self
    }

    /// Replaces the [`DiffTheme`].
    #[must_use]
    pub fn theme(mut self, theme: DiffTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Parses the source and lays it out to display rows for a content area
    /// `width` columns wide. Public so a host can measure a patch (its row
    /// count) for scroll math or a surrounding scrollbar without re-rendering.
    ///
    /// `width` of zero yields no rows.
    #[must_use]
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 {
            return Vec::new();
        }
        let rows = parse_rows(self.source.as_ref());
        layout_rows(&rows, width as usize, &self.theme)
    }
}

impl Widget for Diff<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if let Some(b) = self.block.clone() {
            b.render(area, buf);
        }
        if inner.is_empty() {
            return;
        }
        buf.set_style(inner, self.style);

        let rows = self.lines(inner.width);
        for (i, mut line) in rows
            .into_iter()
            .skip(self.scroll as usize)
            .take(inner.height as usize)
            .enumerate()
        {
            // Each composed line inherits the widget base beneath its own
            // (theme-derived) style — the same patch cascade Text uses.
            line.style = self.style.patch(line.style);
            let row = Rect::new(inner.x, inner.y.saturating_add(i as u16), inner.width, 1);
            line.render(row, buf);
        }
    }
}

// ---------------------------------------------------------------------------
// Parse model
// ---------------------------------------------------------------------------

/// One parsed source row, classified by the unified-diff grammar. Layout
/// (gutter widths, intra-line word marking) happens later in [`layout_rows`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum DiffRow {
    /// A file header: the path to show, and whether the *other* side is
    /// `/dev/null` (added/deleted file) — purely informational for the label.
    File { path: String },
    /// A hunk header with its starting line numbers and optional section.
    Hunk {
        old_start: u32,
        new_start: u32,
        section: String,
    },
    /// A body line. `old_no`/`new_no` are the 1-based numbers that apply to
    /// this row (a deletion has no new number, an addition no old number).
    Body {
        kind: ChangeKind,
        old_no: Option<u32>,
        new_no: Option<u32>,
        content: String,
    },
    /// The `\ No newline at end of file` marker line.
    NoNewline { text: String },
}

/// The three body-line kinds, from the leading sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangeKind {
    /// An unchanged ` ` context line.
    Context,
    /// An added `+` line.
    Addition,
    /// A deleted `-` line.
    Deletion,
}

/// Splits `src` into classified [`DiffRow`]s, tracking line numbers across
/// hunks. Line-oriented, single pass; an unrecognised line outside any hunk is
/// ignored (e.g. `index`/`rename` git metadata), one inside renders as
/// context so no content is ever silently dropped.
fn parse_rows(src: &str) -> Vec<DiffRow> {
    let mut lines: Vec<&str> = src
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    // Trailing blank lines (e.g. a final newline's empty tail) are not content.
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }

    let mut out = Vec::new();
    let mut old_no: u32 = 0;
    let mut new_no: u32 = 0;
    let mut in_hunk = false;
    let mut pending_minus: Option<&str> = None; // last `--- ` awaiting its `+++`

    for line in lines {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            // `diff --git a/x b/x` — start a file; the `--- `/`+++ ` pair that
            // follows refines the path, so just note it and reset hunk state.
            let _ = rest;
            in_hunk = false;
            pending_minus = None;
            continue;
        }

        if let Some(path) = line.strip_prefix("--- ") {
            pending_minus = Some(path);
            in_hunk = false;
            continue;
        }

        if let Some(path) = line.strip_prefix("+++ ") {
            let shown = file_label(pending_minus.unwrap_or("/dev/null"), path);
            out.push(DiffRow::File { path: shown });
            pending_minus = None;
            in_hunk = false;
            continue;
        }

        if let Some(h) = parse_hunk_header(line) {
            old_no = h.old_start;
            new_no = h.new_start;
            in_hunk = true;
            out.push(DiffRow::Hunk {
                old_start: h.old_start,
                new_start: h.new_start,
                section: h.section,
            });
            continue;
        }

        if line.starts_with('\\') {
            // `\ No newline at end of file` — attaches to whichever side it
            // follows; it is not a numbered body row.
            out.push(DiffRow::NoNewline {
                text: line.to_owned(),
            });
            continue;
        }

        if in_hunk {
            let (kind, content) = match line.chars().next() {
                Some('+') => (ChangeKind::Addition, &line[1..]),
                Some('-') => (ChangeKind::Deletion, &line[1..]),
                Some(' ') => (ChangeKind::Context, &line[1..]),
                // An empty line inside a hunk is an empty context line.
                None => (ChangeKind::Context, ""),
                // Anything else inside a hunk is treated as context so the
                // text is preserved rather than dropped.
                Some(_) => (ChangeKind::Context, line),
            };
            let (row_old, row_new) = match kind {
                ChangeKind::Context => {
                    let o = old_no;
                    let n = new_no;
                    old_no += 1;
                    new_no += 1;
                    (Some(o), Some(n))
                }
                ChangeKind::Deletion => {
                    let o = old_no;
                    old_no += 1;
                    (Some(o), None)
                }
                ChangeKind::Addition => {
                    let n = new_no;
                    new_no += 1;
                    (None, Some(n))
                }
            };
            out.push(DiffRow::Body {
                kind,
                old_no: row_old,
                new_no: row_new,
                content: content.to_owned(),
            });
            continue;
        }

        // Outside any hunk and not a header we recognise: git metadata
        // (`index`, `old mode`, `rename from`, …). Deliberately ignored.
    }

    out
}

/// The label shown on a file-header row, given the raw `--- ` and `+++ `
/// paths. `/dev/null` on one side names an added/deleted file; otherwise the
/// (cleaned) new path is canonical, falling back to the old one.
fn file_label(minus: &str, plus: &str) -> String {
    let old = clean_path(minus);
    let new = clean_path(plus);
    let old_null = is_dev_null(minus);
    let new_null = is_dev_null(plus);
    if new_null {
        format!("{old} (deleted)")
    } else if old_null {
        format!("{new} (added)")
    } else if old == new {
        new
    } else {
        format!("{old} → {new}")
    }
}

/// Strips a trailing tab + timestamp (the `--- file\t2024-…` form), a leading
/// `a/`/`b/` prefix, and surrounding quotes from a header path.
fn clean_path(raw: &str) -> String {
    // Git/diff timestamps follow a tab; keep only the path before it.
    let path = raw.split('\t').next().unwrap_or(raw).trim();
    let path = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    path.trim_matches('"').to_owned()
}

/// Whether a raw header path names the empty `/dev/null` side.
fn is_dev_null(raw: &str) -> bool {
    let path = raw.split('\t').next().unwrap_or(raw).trim();
    path == "/dev/null"
}

/// A parsed `@@ … @@` hunk header.
struct HunkHeader {
    old_start: u32,
    new_start: u32,
    section: String,
}

/// Parses `@@ -<l>[,<s>] +<l>[,<s>] @@[ section]`. Omitted counts default to
/// 1 (the unified-diff convention); the section label is the free text after
/// the closing `@@`. Returns `None` if the shape does not match.
fn parse_hunk_header(line: &str) -> Option<HunkHeader> {
    let rest = line.strip_prefix("@@ ")?;
    let close = rest.find(" @@")?;
    let ranges = &rest[..close];
    let section = rest[close + 3..].trim().to_owned();

    let mut parts = ranges.split(' ');
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    if parts.next().is_some() {
        return None;
    }
    let (old_start, _old_count) = parse_range(old)?;
    let (new_start, _new_count) = parse_range(new)?;
    Some(HunkHeader {
        old_start,
        new_start,
        section,
    })
}

/// Parses a `start[,count]` range; a missing count defaults to 1.
fn parse_range(s: &str) -> Option<(u32, u32)> {
    match s.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((s.parse().ok()?, 1)),
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Lays the parsed rows out to display [`Line`]s for a content area `width`
/// wide: computes the gutter width from the largest line number, pairs change
/// groups for the intra-line word diff, then renders every row.
fn layout_rows(rows: &[DiffRow], width: usize, theme: &DiffTheme) -> Vec<Line<'static>> {
    // Gutter width: the widest old/new number, min 1, so a no-number side
    // still reserves a column and the sign stays aligned.
    let max_no = rows
        .iter()
        .filter_map(|r| match r {
            DiffRow::Body { old_no, new_no, .. } => {
                Some(old_no.unwrap_or(0).max(new_no.unwrap_or(0)))
            }
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let num_w = digits(max_no).max(1);

    // Which body rows are part of a change group, paired for intra-line marks:
    // index → the changed-char mask for that row (empty = no per-word marks).
    let marks = intra_line_marks(rows);

    let mut out = Vec::with_capacity(rows.len());
    for (idx, row) in rows.iter().enumerate() {
        out.push(render_row(idx, row, num_w, &marks, width, theme));
    }
    out
}

/// The decimal digit count of `n` (at least 1, for `0`).
fn digits(n: u32) -> usize {
    if n == 0 { 1 } else { (n.ilog10() + 1) as usize }
}

/// Renders one parsed row into a display [`Line`], padding its content with
/// trailing spaces so a row background spans the full `width`.
fn render_row(
    idx: usize,
    row: &DiffRow,
    num_w: usize,
    marks: &[Option<Vec<bool>>],
    width: usize,
    theme: &DiffTheme,
) -> Line<'static> {
    match row {
        DiffRow::File { path } => full_width_line(&format!("─── {path} "), width, theme.file),
        DiffRow::Hunk {
            old_start,
            new_start,
            section,
        } => {
            let mut head = format!("@@ -{old_start} +{new_start} @@");
            if !section.is_empty() {
                head.push(' ');
                head.push_str(section);
            }
            full_width_line(&head, width, theme.hunk)
        }
        DiffRow::NoNewline { text } => full_width_line(text, width, theme.context),
        DiffRow::Body {
            kind,
            old_no,
            new_no,
            content,
        } => {
            let (sign, row_style, word_style) = match kind {
                ChangeKind::Addition => ('+', theme.addition, theme.word_added),
                ChangeKind::Deletion => ('-', theme.deletion, theme.word_deleted),
                ChangeKind::Context => (' ', theme.context, theme.context),
            };
            let gutter = format!(
                "{old:>w$} {new:>w$} {sign} ",
                old = num_str(*old_no),
                new = num_str(*new_no),
                w = num_w,
            );
            let gutter_w = gutter.chars().count();

            let mut spans = vec![Span::styled(gutter, theme.gutter.patch(row_style))];

            // Body content cells, with the per-word mask painted on top.
            let body_w = width.saturating_sub(gutter_w);
            let mask = marks.get(idx).and_then(Option::as_ref);
            let mut col = 0usize;
            let mut run = String::new();
            let mut run_marked = false;
            let chars: Vec<char> = content.chars().collect();
            for (i, &ch) in chars.iter().enumerate() {
                if col >= body_w {
                    break;
                }
                let marked = mask.map(|m| m.get(i).copied().unwrap_or(false)) == Some(true);
                if !run.is_empty() && marked != run_marked {
                    spans.push(body_span(&run, run_marked, row_style, word_style));
                    run.clear();
                }
                run.push(ch);
                run_marked = marked;
                col += 1;
            }
            if !run.is_empty() {
                spans.push(body_span(&run, run_marked, row_style, word_style));
            }

            // Pad to full width so the row background reads as a block.
            if col < body_w {
                spans.push(Span::styled(" ".repeat(body_w - col), row_style));
            }
            Line::from(spans).style(row_style)
        }
    }
}

/// A body content span: the changed-word emphasis layered on top of the row
/// style when `marked`, otherwise just the row style.
fn body_span(text: &str, marked: bool, row_style: Style, word_style: Style) -> Span<'static> {
    let style = if marked {
        row_style.patch(word_style)
    } else {
        row_style
    };
    Span::styled(text.to_owned(), style)
}

/// A header row: `text` clipped to `width`, padded with trailing spaces so the
/// header background spans the full row.
fn full_width_line(text: &str, width: usize, style: Style) -> Line<'static> {
    let mut s: String = text.chars().take(width).collect();
    while s.chars().count() < width {
        s.push(' ');
    }
    Line::from(Span::styled(s, style)).style(style)
}

/// A line number formatted for the gutter, or spaces when the side does not
/// apply to this row (a deletion has no new number, an addition no old one).
fn num_str(no: Option<u32>) -> String {
    match no {
        Some(n) => n.to_string(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Intra-line word diff
// ---------------------------------------------------------------------------

/// For every body row, the per-char "changed" mask of an intra-line word diff,
/// or `None` when the row is not word-diffed (context, or an unpaired
/// add/delete). A change group is a maximal run of deletions then additions;
/// deletion *i* is paired with addition *i* and their tokens LCS'd.
fn intra_line_marks(rows: &[DiffRow]) -> Vec<Option<Vec<bool>>> {
    let mut marks = vec![None; rows.len()];
    let mut i = 0;
    while i < rows.len() {
        // A change group: consecutive deletions, then consecutive additions.
        let del_start = i;
        while matches!(
            rows.get(i),
            Some(DiffRow::Body {
                kind: ChangeKind::Deletion,
                ..
            })
        ) {
            i += 1;
        }
        let del_end = i;
        let add_start = i;
        while matches!(
            rows.get(i),
            Some(DiffRow::Body {
                kind: ChangeKind::Addition,
                ..
            })
        ) {
            i += 1;
        }
        let add_end = i;

        let dels = del_end - del_start;
        let adds = add_end - add_start;
        // Only pair when both sides exist; pure adds or pure deletes get a
        // whole-line highlight (no per-word marks) — there is nothing to
        // diff against.
        if dels > 0 && adds > 0 {
            for k in 0..dels.min(adds) {
                let di = del_start + k;
                let ai = add_start + k;
                if let (
                    Some(DiffRow::Body { content: d, .. }),
                    Some(DiffRow::Body { content: a, .. }),
                ) = (rows.get(di), rows.get(ai))
                {
                    if d.len() <= INTRA_LINE_MAX && a.len() <= INTRA_LINE_MAX {
                        let (dm, am) = word_diff(d, a);
                        marks[di] = Some(dm);
                        marks[ai] = Some(am);
                    }
                }
            }
        }

        // Ensure forward progress on a row that began no change group.
        if i == del_start {
            i += 1;
        }
    }
    marks
}

/// One token of a line: a maximal run of one class. Splitting on class
/// boundaries (rather than per-char) is what makes the word highlight read as
/// *words* changed, and keeps the LCS table small.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    /// The token's text.
    text: String,
    /// Character offset of the token's start within its line.
    start: usize,
}

/// The class of a `char` for tokenisation: whitespace, word (alphanumeric or
/// `_`), or each punctuation char on its own.
fn class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// Splits `s` into [`Token`]s: maximal runs of whitespace or word chars, and
/// each punctuation char as its own token (so `a;b` ≠ `a,b` differs only at
/// the punctuation).
fn tokenize(s: &str) -> Vec<Token> {
    let chars: Vec<char> = s.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let cls = class(chars[i]);
        if cls == 2 {
            toks.push(Token {
                text: chars[i].to_string(),
                start: i,
            });
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && class(chars[i]) == cls {
            i += 1;
        }
        toks.push(Token {
            text: chars[start..i].iter().collect(),
            start,
        });
    }
    toks
}

/// A token-level LCS between the deletion line `d` and addition line `a`,
/// returning a per-*char* changed mask for each. Tokens not on the longest
/// common subsequence are the changed ones; their char span is marked.
fn word_diff(d: &str, a: &str) -> (Vec<bool>, Vec<bool>) {
    let dt = tokenize(d);
    let at = tokenize(a);
    let d_len = d.chars().count();
    let a_len = a.chars().count();
    let mut d_mask = vec![false; d_len];
    let mut a_mask = vec![false; a_len];

    // Classic LCS DP over token equality.
    let n = dt.len();
    let m = at.len();
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for x in (0..n).rev() {
        for y in (0..m).rev() {
            dp[x][y] = if dt[x].text == at[y].text {
                dp[x + 1][y + 1] + 1
            } else {
                dp[x + 1][y].max(dp[x][y + 1])
            };
        }
    }

    // Walk the table: a matched pair is unchanged; an unmatched token on
    // either side has its whole char span marked.
    let mut x = 0;
    let mut y = 0;
    while x < n && y < m {
        if dt[x].text == at[y].text {
            x += 1;
            y += 1;
        } else if dp[x + 1][y] >= dp[x][y + 1] {
            mark(&mut d_mask, &dt[x]);
            x += 1;
        } else {
            mark(&mut a_mask, &at[y]);
            y += 1;
        }
    }
    while x < n {
        mark(&mut d_mask, &dt[x]);
        x += 1;
    }
    while y < m {
        mark(&mut a_mask, &at[y]);
        y += 1;
    }
    (d_mask, a_mask)
}

/// Marks every char position covered by `tok` in `mask`.
fn mark(mask: &mut [bool], tok: &Token) {
    let len = tok.text.chars().count();
    for slot in mask.iter_mut().skip(tok.start).take(len) {
        *slot = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Position;

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

    #[test]
    fn empty_input_renders_nothing() {
        assert!(Diff::new("").lines(40).is_empty());
        assert_eq!(lines(Diff::new(""), 6, 2), "      \n      \n");
    }

    #[test]
    fn basic_hunk_numbers_context_add_and_delete() {
        let patch = "@@ -1,2 +1,2 @@\n ctx\n-old\n+new";
        let out = lines(Diff::new(patch), 14, 4);
        // Gutter: `<old> <new> <sign> ` (each number right-padded to the
        // widest, here width 1), then the content padded to the row width.
        assert_eq!(
            out,
            "@@ -1 +1 @@   \n1 1   ctx     \n2   - old     \n  2 + new     \n"
        );
    }

    #[test]
    fn omitted_counts_default_to_one() {
        // `@@ -1 +1 @@` (no `,count`) must parse: count defaults to 1.
        let rows = parse_rows("@@ -1 +1 @@\n-a\n+b");
        assert_eq!(
            rows[0],
            DiffRow::Hunk {
                old_start: 1,
                new_start: 1,
                section: String::new(),
            }
        );
        assert!(matches!(
            rows[1],
            DiffRow::Body {
                kind: ChangeKind::Deletion,
                old_no: Some(1),
                new_no: None,
                ..
            }
        ));
    }

    #[test]
    fn hunk_section_label_is_echoed() {
        let rows = parse_rows("@@ -10,3 +12,4 @@ fn render(&self)");
        assert_eq!(
            rows[0],
            DiffRow::Hunk {
                old_start: 10,
                new_start: 12,
                section: "fn render(&self)".to_owned(),
            }
        );
        let out = lines(Diff::new("@@ -10,3 +12,4 @@ fn render"), 24, 1);
        assert_eq!(out, "@@ -10 +12 @@ fn render \n");
    }

    #[test]
    fn added_file_uses_the_plus_path_and_added_marker() {
        let rows = parse_rows("--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1 @@\n+x");
        assert_eq!(
            rows[0],
            DiffRow::File {
                path: "new.rs (added)".to_owned(),
            }
        );
    }

    #[test]
    fn deleted_file_uses_the_minus_path_and_deleted_marker() {
        let rows = parse_rows("--- a/gone.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-x");
        assert_eq!(
            rows[0],
            DiffRow::File {
                path: "gone.rs (deleted)".to_owned(),
            }
        );
    }

    #[test]
    fn no_newline_marker_is_its_own_row_and_not_numbered() {
        let rows = parse_rows("@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b");
        assert_eq!(
            rows[2],
            DiffRow::NoNewline {
                text: "\\ No newline at end of file".to_owned(),
            }
        );
        // The marker did not consume a line number: the addition is still 1.
        assert!(matches!(
            rows[3],
            DiffRow::Body {
                new_no: Some(1),
                ..
            }
        ));
    }

    #[test]
    fn crlf_is_stripped_so_no_stray_carriage_return_renders() {
        let patch = "@@ -1 +1 @@\r\n-a\r\n+b\r\n";
        let out = lines(Diff::new(patch), 12, 3);
        assert!(!out.contains('\r'));
        assert_eq!(out, "@@ -1 +1 @@ \n1   - a     \n  1 + b     \n");
    }

    #[test]
    fn change_group_with_unequal_deletions_and_additions() {
        // 2 deletions, 1 addition: pair (del0,add0); del1 is an unpaired
        // delete (whole-line highlight, no per-word mask).
        let rows = parse_rows("@@ -1,2 +1 @@\n-aaa\n-bbb\n+aaa");
        let marks = intra_line_marks(&rows);
        // rows: [Hunk, Body(-aaa), Body(-bbb), Body(+aaa)]
        assert!(marks[1].is_some()); // del0 paired
        assert!(marks[2].is_none()); // del1 unpaired
        assert!(marks[3].is_some()); // add0 paired
        // del0 vs add0 are identical → no chars marked.
        assert!(marks[1].as_ref().unwrap().iter().all(|&m| !m));
    }

    #[test]
    fn intra_line_marks_only_the_one_changed_token() {
        // "let x = 1;" → "let x = 2;" differs only at the `1`/`2` token.
        let (dm, am) = word_diff("let x = 1;", "let x = 2;");
        let d: String = "let x = 1;"
            .chars()
            .zip(&dm)
            .filter(|&(_, &m)| m)
            .map(|(c, _)| c)
            .collect();
        let a: String = "let x = 2;"
            .chars()
            .zip(&am)
            .filter(|&(_, &m)| m)
            .map(|(c, _)| c)
            .collect();
        assert_eq!(d, "1");
        assert_eq!(a, "2");
    }

    #[test]
    fn intra_line_highlight_paints_the_changed_word_background() {
        let patch = "@@ -1 +1 @@\n-hello world\n+hello there";
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 3));
        Diff::new(patch).render(buf.area(), &mut buf);
        // Layout: y=0 hunk, y=1 `-hello world`, y=2 `+hello there`. The
        // gutter is 6 cols (`  1 + `), so content begins at col 6:
        // cols 6..=10 = "hello", 11 = ' ', cols 12..=16 = the changed word.
        let add_row = 2u16;
        // Inside the unchanged "hello": the row fg is the addition green but
        // there is no strengthened word *background*.
        let unchanged = buf.get(Position::new(8, add_row)).unwrap(); // 'l'
        assert_eq!(unchanged.symbol, 'l');
        assert_ne!(unchanged.bg, Color::Green);
        // Inside the changed "there": the word-added background is painted.
        let changed = buf.get(Position::new(13, add_row)).unwrap(); // 'h'
        assert_eq!(changed.symbol, 'h');
        assert_eq!(changed.bg, Color::Green);
    }

    #[test]
    fn long_lines_skip_the_intra_line_pass() {
        let long_a = "a".repeat(INTRA_LINE_MAX + 1);
        let long_b = format!("{}b", "a".repeat(INTRA_LINE_MAX));
        let patch = format!("@@ -1 +1 @@\n-{long_a}\n+{long_b}");
        let rows = parse_rows(&patch);
        let marks = intra_line_marks(&rows);
        // Over the cap: no per-word marks; the whole-line style still applies.
        assert!(marks[1].is_none());
        assert!(marks[2].is_none());
    }

    #[test]
    fn multi_file_patch_splits_on_each_header() {
        let patch = "\
diff --git a/one.rs b/one.rs
--- a/one.rs
+++ b/one.rs
@@ -1 +1 @@
-a
+b
diff --git a/two.rs b/two.rs
--- a/two.rs
+++ b/two.rs
@@ -1 +1 @@
-c
+d";
        let rows = parse_rows(patch);
        let files: Vec<_> = rows
            .iter()
            .filter_map(|r| match r {
                DiffRow::File { path } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(files, vec!["one.rs", "two.rs"]);
        // Second file's hunk restarts numbering from its own header.
        let hunks = rows
            .iter()
            .filter(|r| matches!(r, DiffRow::Hunk { .. }))
            .count();
        assert_eq!(hunks, 2);
    }

    #[test]
    fn scroll_skips_composed_rows() {
        let patch = "@@ -1,3 +1,3 @@\n a\n b\n c";
        // Rows: [hunk, " a", " b", " c"] → scroll 2 → start at " b".
        let d = Diff::new(patch).scroll(2);
        let out = lines(d, 14, 1);
        assert_eq!(out, "2 2   b       \n");
    }

    #[test]
    fn zero_area_and_zero_width_are_no_ops() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Diff::new("@@ -1 +1 @@\n+x").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        assert!(Diff::new("@@ -1 +1 @@\n+x").lines(0).is_empty());
    }

    #[test]
    fn block_frames_content_in_the_inner_area() {
        let out = lines(Diff::new("@@ -1 +1 @@").block(Block::bordered()), 13, 3);
        assert_eq!(
            out,
            "┌───────────┐\n\
             │@@ -1 +1 @@│\n\
             └───────────┘\n"
        );
    }

    #[test]
    fn git_metadata_lines_outside_a_hunk_are_ignored() {
        let patch = "\
diff --git a/f.rs b/f.rs
index e69de29..4b825dc 100644
--- a/f.rs
+++ b/f.rs
@@ -1 +1 @@
-a
+b";
        let rows = parse_rows(patch);
        // No row carries the `index …` metadata line.
        assert!(!rows.iter().any(|r| matches!(
            r,
            DiffRow::Body { content, .. } if content.contains("index")
        )));
        assert!(matches!(rows[0], DiffRow::File { .. }));
    }

    #[test]
    fn malformed_hunk_header_is_not_parsed_as_a_hunk() {
        // Missing the closing `@@`: must not panic, and is not a Hunk row.
        let rows = parse_rows("@@ -1 +1\n+x");
        assert!(!rows.iter().any(|r| matches!(r, DiffRow::Hunk { .. })));
    }

    #[test]
    fn trailing_blank_lines_are_dropped_before_parsing() {
        let rows = parse_rows("@@ -1 +1 @@\n+x\n\n\n");
        // Only [Hunk, Body(+x)] — the trailing blank tail is not content.
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[1], DiffRow::Body { .. }));
    }

    #[test]
    fn renamed_file_shows_both_paths() {
        let rows = parse_rows("--- a/old/name.rs\n+++ b/new/name.rs\n@@ -1 +1 @@\n ctx");
        assert_eq!(
            rows[0],
            DiffRow::File {
                path: "old/name.rs → new/name.rs".to_owned(),
            }
        );
    }
}
