//! [`Markdown`] — a read-only widget that renders a CommonMark-ish subset of
//! markdown into the styled-text model, terminal-width aware.
//!
//! # Why a hand-written parser
//!
//! rstui is deliberately dependency-free below the backend (see
//! [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)
//! §4: a widget that pulls a transitive dependency is feature-gated, and an
//! own-crate split is reserved for *heavy, optional, conceptually alien*
//! engines — never pre-emptively). A markdown *grammar* is none of those: the
//! subset real terminal docs need (headings, emphasis, code, quotes, lists,
//! rules) is a few hundred lines of line-oriented scanning, the same way
//! [`Paragraph`](crate::Paragraph)'s wrap composer is hand-written rather than
//! pulling a text-layout crate. So `Markdown` is a plain
//! [`Widget`] module here, zero new dependencies.
//!
//! # Progressive fidelity, not a fake renderer
//!
//! This is a real, tested subset — not a placeholder that pretends to be
//! complete. Supported now:
//!
//! - ATX headings `#`…`######` (rendered bold, brighter per level)
//! - paragraphs with soft line-join and word wrap to the area width
//! - inline **strong** (`**`/`__`), *emphasis* (`*`/`_`),
//!   `` `code` ``, and backslash escapes
//! - fenced code blocks (``` ``` ``` / `~~~`) with an info string, drawn with
//!   a filled background and never reflowed
//! - block quotes (`>`), nesting recursively, drawn with a `│ ` rail
//! - bullet (`-`/`*`/`+`) and ordered (`1.`/`1)`) lists, including nested
//!   lists and multi-line items via indentation
//! - thematic breaks (`---`/`***`/`___`)
//! - GFM pipe tables with a `:`-aligned delimiter row, drawn as a
//!   width-fitted box-drawing grid with per-column alignment
//!
//! Deliberately out of scope for this slice (each an additive follow-up that
//! does not change this shape): links/images (the link-span slice owns
//! activation), indented code blocks, setext headings, HTML passthrough,
//! reference definitions.
//!
//! Rendering is deterministic and width-aware: the same source and area always
//! produce the same cells, so output is snapshot-testable through
//! [`Buffer`] exactly like every other widget.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Position, Rect, Widget};
//! use rstui_widgets::Markdown;
//!
//! let mut buf = Buffer::empty(Rect::new(0, 0, 12, 3));
//! Markdown::new("# Title\n\nbody **bold**").render(buf.area(), &mut buf);
//!
//! assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'T'); // heading
//! // Row 1 is the blank spacer that separates blocks; the paragraph is row 2.
//! assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, 'b');
//! ```

use std::borrow::Cow;

use crate::block::Block;
use rstui_core::{Alignment, Buffer, Color, Line, Modifier, Rect, Span, Style, Widget};

/// The styles [`Markdown`] applies to each kind of element.
///
/// Every field is a *patch* layered over the widget base style (itself layered
/// over the framing [`Block`] fill), so an unset color or modifier falls
/// through rather than overriding the surrounding theme — the same
/// [`Style::patch`](rstui_core::Style) cascade the text model uses. Construct
/// the tuned terminal default with [`MarkdownTheme::default`] and override only
/// the fields you care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkdownTheme {
    /// Applied to every heading level (e.g. bold). Per-level color is added on
    /// top from [`heading_colors`](Self::heading_colors).
    pub heading: Style,
    /// Foreground color for heading levels 1..=6 (index `level - 1`); levels
    /// past the array reuse the last entry.
    pub heading_colors: [Color; 6],
    /// Inline `` `code` `` and fenced code block content.
    pub code: Style,
    /// `**strong**` emphasis.
    pub strong: Style,
    /// `*emphasis*`.
    pub emphasis: Style,
    /// The `│ ` rail drawn down the left of a block quote, and the quoted text.
    pub quote: Style,
    /// The bullet/number marker that leads a list item.
    pub marker: Style,
    /// The `─` glyphs of a thematic break.
    pub rule: Style,
}

impl Default for MarkdownTheme {
    fn default() -> Self {
        Self {
            heading: Style::new().add_modifier(Modifier::BOLD),
            heading_colors: [
                Color::Cyan,
                Color::Cyan,
                Color::Blue,
                Color::Blue,
                Color::Magenta,
                Color::Magenta,
            ],
            code: Style::new().fg(Color::Yellow),
            strong: Style::new().add_modifier(Modifier::BOLD),
            emphasis: Style::new().add_modifier(Modifier::ITALIC),
            quote: Style::new().fg(Color::Green),
            marker: Style::new().fg(Color::Cyan),
            rule: Style::new().fg(Color::DarkGray),
        }
    }
}

impl MarkdownTheme {
    /// The foreground color for a 1-based heading `level`, clamped to the
    /// configured palette.
    fn heading_color(&self, level: u8) -> Color {
        let idx = (level.max(1) as usize - 1).min(self.heading_colors.len() - 1);
        self.heading_colors[idx]
    }
}

/// A read-only markdown view: parses its source once at render time and draws
/// the supported subset into the area, width-aware and deterministic.
///
/// The source is a [`Cow<str>`](std::borrow::Cow) (a literal borrows, a
/// `String` is owned). Parsing produces owned display lines, so the rendered
/// spans are independent of the source lifetime. An optional framing
/// [`Block`], a base [`Style`] that also fills the content area, a vertical
/// scroll offset, and a [`MarkdownTheme`] are the only knobs — everything else
/// is derived from the markdown.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Block, Markdown};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 14, 4));
/// Markdown::new("- one\n- two")
///     .block(Block::bordered())
///     .render(buf.area(), &mut buf);
///
/// // Framed, with a bullet marker inside the border.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
/// assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, '•');
/// ```
#[derive(Debug, Clone)]
pub struct Markdown<'a> {
    source: Cow<'a, str>,
    block: Option<Block<'a>>,
    style: Style,
    scroll: u16,
    theme: MarkdownTheme,
}

impl<'a> Markdown<'a> {
    /// A markdown view of `source` with the default theme, no block, no scroll.
    pub fn new(source: impl Into<Cow<'a, str>>) -> Self {
        Self {
            source: source.into(),
            block: None,
            style: Style::new(),
            scroll: 0,
            theme: MarkdownTheme::default(),
        }
    }

    /// Frames the document in `block`; content renders into
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

    /// Skips the first `offset` composed (post-wrap) rows: the vertical scroll
    /// position for a document taller than its area.
    #[must_use]
    pub fn scroll(mut self, offset: u16) -> Self {
        self.scroll = offset;
        self
    }

    /// Replaces the [`MarkdownTheme`].
    #[must_use]
    pub fn theme(mut self, theme: MarkdownTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Parses the source and lays it out to display rows for a content area
    /// `width` columns wide. Public so a host can measure a document (its row
    /// count) for scroll math or a surrounding scrollbar without re-rendering.
    ///
    /// `width` of zero yields no rows.
    #[must_use]
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 {
            return Vec::new();
        }
        let blocks = parse_blocks(self.source.as_ref());
        let mut rows = Vec::new();
        layout_blocks(&blocks, width as usize, &self.theme, true, &mut rows);
        rows
    }
}

impl Widget for Markdown<'_> {
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
// Block model
// ---------------------------------------------------------------------------

/// One parsed top-level construct. Inline text is already resolved to styled
/// [`Span`]s; layout (wrapping, prefixes, rails) happens in [`layout_blocks`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum MdBlock {
    Heading {
        level: u8,
        spans: Vec<Span<'static>>,
    },
    Paragraph(Vec<Span<'static>>),
    Code {
        lines: Vec<String>,
    },
    Quote(Vec<MdBlock>),
    List {
        ordered: bool,
        start: u64,
        items: Vec<Vec<MdBlock>>,
    },
    /// A GFM pipe table: per-column [`Alignment`] from the delimiter row, a
    /// header row, then body rows. Each cell is already inline-parsed spans.
    Table {
        aligns: Vec<Alignment>,
        header: Vec<Vec<Span<'static>>>,
        rows: Vec<Vec<Vec<Span<'static>>>>,
    },
    Rule,
}

/// Splits `src` into [`MdBlock`]s. Line-oriented, single pass, no lookahead
/// beyond fence/list continuation scanning.
fn parse_blocks(src: &str) -> Vec<MdBlock> {
    let lines: Vec<&str> = src
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        if let Some((fence, info)) = fence_open(trimmed) {
            i += 1;
            let mut body = Vec::new();
            while i < lines.len() {
                let l = lines[i];
                if fence_close(l.trim_start(), fence) {
                    i += 1;
                    break;
                }
                body.push(l.to_owned());
                i += 1;
            }
            let _ = info; // info string (language) is reserved for a later slice
            out.push(MdBlock::Code { lines: body });
            continue;
        }

        if is_thematic_break(trimmed) {
            out.push(MdBlock::Rule);
            i += 1;
            continue;
        }

        if let Some(level) = atx_heading_level(trimmed) {
            let text = atx_heading_text(trimmed, level);
            out.push(MdBlock::Heading {
                level,
                spans: parse_inline(text),
            });
            i += 1;
            continue;
        }

        if trimmed.starts_with('>') {
            let mut quoted = Vec::new();
            while i < lines.len() {
                let t = lines[i].trim_start();
                if t.starts_with('>') {
                    quoted.push(strip_quote_marker(t));
                    i += 1;
                } else if t.is_empty() {
                    break;
                } else {
                    // Lazy continuation: a non-blank, non-`>` line still
                    // belongs to the quote paragraph (CommonMark laziness).
                    quoted.push(lines[i].to_owned());
                    i += 1;
                }
            }
            out.push(MdBlock::Quote(parse_blocks(&quoted.join("\n"))));
            continue;
        }

        if starts_table(&lines, i) {
            let aligns = table_delim_aligns(lines[i + 1]).expect("starts_table verified it");
            let ncols = aligns.len();
            let header = normalize_row(split_table_row(line), ncols)
                .into_iter()
                .map(|c| parse_inline(&c))
                .collect();
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() {
                let row = lines[i];
                let t = row.trim();
                if t.is_empty() || !t.contains('|') || table_delim_aligns(row).is_some() {
                    break;
                }
                rows.push(
                    normalize_row(split_table_row(row), ncols)
                        .into_iter()
                        .map(|c| parse_inline(&c))
                        .collect(),
                );
                i += 1;
            }
            out.push(MdBlock::Table {
                aligns,
                header,
                rows,
            });
            continue;
        }

        if let Some(marker) = list_marker(line) {
            let (block, next) = parse_list(&lines, i, marker);
            out.push(block);
            i = next;
            continue;
        }

        // Paragraph: gather following lines until a blank line or a line that
        // begins a different construct. Soft breaks join with a space.
        let mut buf = String::new();
        while i < lines.len() {
            let l = lines[i];
            let t = l.trim_start();
            if t.is_empty()
                || is_thematic_break(t)
                || atx_heading_level(t).is_some()
                || fence_open(t).is_some()
                || t.starts_with('>')
                || list_marker(l).is_some()
                || starts_table(&lines, i)
            {
                break;
            }
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(t);
            i += 1;
        }
        out.push(MdBlock::Paragraph(parse_inline(&buf)));
    }
    out
}

/// The kind and indent of a list item's marker line.
#[derive(Debug, Clone, Copy)]
struct ListMarker {
    /// Columns of leading whitespace before the marker.
    indent: usize,
    /// `Some(n)` for an ordered item starting at `n`; `None` for a bullet.
    ordered: Option<u64>,
    /// Column count of marker + the single required space after it.
    width: usize,
}

/// Recognises a list item at the start of `line`, returning its marker shape.
fn list_marker(line: &str) -> Option<ListMarker> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;
    if matches!(first, '-' | '*' | '+') {
        // `* * *` is a thematic break, not a one-item list.
        if is_thematic_break(rest) {
            return None;
        }
        let after = &rest[first.len_utf8()..];
        if after.starts_with(' ') || after.is_empty() {
            return Some(ListMarker {
                indent,
                ordered: None,
                width: 2,
            });
        }
        return None;
    }
    // Ordered: one or more digits then `.` or `)` then a space.
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 9 {
        return None;
    }
    let delim = rest[digits.len()..].chars().next()?;
    if delim != '.' && delim != ')' {
        return None;
    }
    let after = &rest[digits.len() + 1..];
    if after.starts_with(' ') || after.is_empty() {
        Some(ListMarker {
            indent,
            ordered: Some(digits.parse().unwrap_or(1)),
            width: digits.len() + 2,
        })
    } else {
        None
    }
}

/// Parses one whole list starting at `lines[start]`. Returns the block and the
/// index of the first line after it. Items collect their own marker line plus
/// any following lines indented past the marker (nested lists, lazy
/// continuation), which are recursively parsed as block content.
fn parse_list(lines: &[&str], start: usize, first: ListMarker) -> (MdBlock, usize) {
    let ordered = first.ordered.is_some();
    let list_start = first.ordered.unwrap_or(1);
    let mut items: Vec<Vec<MdBlock>> = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let Some(m) = list_marker(lines[i]) else {
            break;
        };
        if m.indent != first.indent || m.ordered.is_some() != ordered {
            break;
        }
        // The item body: text after the marker, then continuation lines that
        // are blank or indented at least to the content column.
        let content_col = m.indent + m.width;
        let mut body = Vec::new();
        let head = &lines[i][lines[i].len().min(content_col)..];
        body.push(head.to_owned());
        i += 1;
        let mut saw_blank = false;
        let mut end_list = false;
        while i < lines.len() {
            let l = lines[i];
            if l.trim().is_empty() {
                saw_blank = true;
                body.push(String::new());
                i += 1;
                continue;
            }
            let lead = l.len() - l.trim_start().len();
            if lead >= content_col {
                body.push(l[content_col.min(l.len())..].to_owned());
                i += 1;
            } else if list_marker(l).is_some() {
                break;
            } else if saw_blank {
                // A non-indented line after a blank starts a new block: it
                // ends this item *and* the list (no lazy continuation across
                // a paragraph break — that is what made `---` get swallowed).
                end_list = true;
                break;
            } else {
                // Lazy paragraph continuation of the current item.
                body.push(l.trim_start().to_owned());
                i += 1;
            }
        }
        while body.last().is_some_and(|s| s.is_empty()) {
            body.pop();
        }
        items.push(parse_blocks(&body.join("\n")));
        if end_list {
            break;
        }
    }
    (
        MdBlock::List {
            ordered,
            start: list_start,
            items,
        },
        i,
    )
}

/// `Some((fence_char, info))` if `trimmed` opens a fenced code block.
fn fence_open(trimmed: &str) -> Option<(char, String)> {
    for fence in ['`', '~'] {
        let run = trimmed.chars().take_while(|&c| c == fence).count();
        if run >= 3 {
            let info = trimmed[run..].trim().to_owned();
            // An info string may not itself contain a backtick fence char.
            if fence == '`' && info.contains('`') {
                continue;
            }
            return Some((fence, info));
        }
    }
    None
}

/// Whether `trimmed` closes a fence opened with `fence` (a bare run of ≥3).
fn fence_close(trimmed: &str, fence: char) -> bool {
    let run = trimmed.chars().take_while(|&c| c == fence).count();
    run >= 3 && trimmed[run..].trim().is_empty()
}

/// ATX heading level (count of leading `#`, 1..=6) if `trimmed` is one.
fn atx_heading_level(trimmed: &str) -> Option<u8> {
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) {
        let after = &trimmed[hashes..];
        if after.is_empty() || after.starts_with(' ') {
            return Some(hashes as u8);
        }
    }
    None
}

/// The inline text of an ATX heading, with the leading `#`s and any optional
/// closing run of `#`s removed.
fn atx_heading_text(trimmed: &str, level: u8) -> &str {
    let body = trimmed[level as usize..].trim();
    body.trim_end_matches('#').trim_end()
}

/// Whether `trimmed` is a thematic break: ≥3 of one of `-`/`*`/`_`, only those
/// chars and spaces.
fn is_thematic_break(trimmed: &str) -> bool {
    for marker in ['-', '*', '_'] {
        let count = trimmed.chars().filter(|&c| c == marker).count();
        if count >= 3 && trimmed.chars().all(|c| c == marker || c == ' ') {
            return true;
        }
    }
    false
}

/// Drops a `>` quote marker and the single optional space after it.
fn strip_quote_marker(trimmed: &str) -> String {
    let rest = &trimmed[1..];
    rest.strip_prefix(' ').unwrap_or(rest).to_owned()
}

/// A GFM pipe table starts at `lines[i]` iff that line has a `|` and the next
/// line is a valid alignment delimiter row. Requiring the delimiter row is
/// what keeps an ordinary `a | b` paragraph from being misread as a table.
fn starts_table(lines: &[&str], i: usize) -> bool {
    i + 1 < lines.len()
        && lines[i].contains('|')
        && !lines[i].trim().is_empty()
        && table_delim_aligns(lines[i + 1]).is_some()
}

/// Parses a table delimiter row into per-column [`Alignment`]s, or `None` if
/// it is not one. Each cell must be `-`s with an optional leading/trailing `:`
/// (`:--`=left, `:-:`=center, `--:`=right, `---`=left default).
fn table_delim_aligns(line: &str) -> Option<Vec<Alignment>> {
    if !line.contains('|') && !line.contains('-') {
        return None;
    }
    let cells = split_table_row(line);
    if cells.is_empty() {
        return None;
    }
    let mut aligns = Vec::with_capacity(cells.len());
    for cell in &cells {
        let c = cell.trim();
        if c.is_empty() || !c.contains('-') || !c.chars().all(|ch| ch == '-' || ch == ':') {
            return None;
        }
        let left = c.starts_with(':');
        let right = c.ends_with(':');
        // A `:` may only sit at the ends: the middle is all dashes.
        if c[usize::from(left)..c.len() - usize::from(right)]
            .chars()
            .any(|ch| ch != '-')
        {
            return None;
        }
        aligns.push(match (left, right) {
            (true, true) => Alignment::Center,
            (false, true) => Alignment::Right,
            _ => Alignment::Left,
        });
    }
    Some(aligns)
}

/// Splits one table row into trimmed cell strings: a single optional leading
/// and trailing `|` is dropped, the row is split on unescaped `|`, and `\|`
/// is unescaped to a literal pipe.
fn split_table_row(line: &str) -> Vec<String> {
    let mut s = line.trim();
    s = s.strip_prefix('|').unwrap_or(s);
    if s.ends_with('|') && !s.ends_with("\\|") {
        s = &s[..s.len() - 1];
    }
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cur.push('|');
            chars.next();
        } else if c == '|' {
            cells.push(cur.trim().to_owned());
            cur = String::new();
        } else {
            cur.push(c);
        }
    }
    cells.push(cur.trim().to_owned());
    cells
}

/// Pads a row with empty cells (or truncates extras) so it has exactly `ncols`
/// cells — GFM's rule for ragged rows.
fn normalize_row(mut cells: Vec<String>, ncols: usize) -> Vec<String> {
    cells.resize(ncols, String::new());
    cells
}

// ---------------------------------------------------------------------------
// Inline parser
// ---------------------------------------------------------------------------

/// One lexed inline unit.
///
/// Lexing once, *before* emphasis resolution, is what makes a
/// backslash-escaped `*` provably inert: it becomes a [`Lit`](InlineTok::Lit),
/// never a [`Delim`](InlineTok::Delim), so no later pass can revive it as an
/// emphasis marker (the bug a re-scanning two-pass parser invites).
#[derive(Debug, Clone, PartialEq, Eq)]
enum InlineTok {
    /// A literal char: an ordinary char, or the unescaped target of `\X`.
    Lit(char),
    /// An *active* emphasis delimiter — `*` or `_`, unescaped, outside code.
    Delim(char),
    /// A resolved code span's literal content; never re-parsed.
    Code(String),
}

/// Parses inline markdown into owned styled spans.
///
/// Precedence follows CommonMark's shape closely enough for real docs: a
/// backslash escapes the next ASCII punctuation char; `` `code` `` binds
/// tighter than emphasis and is never re-parsed; `**`/`__` is strong, `*`/`_`
/// is emphasis, `***`/`___` is both, matched non-greedily and recursively so
/// `**a *b* c**` nests. `_` does not open or close inside a word so
/// `snake_case` is left alone.
fn parse_inline(text: &str) -> Vec<Span<'static>> {
    let toks = lex_inline(text);
    let theme = MarkdownTheme::default();
    let mut out = Vec::new();
    render_toks(&toks, Style::new(), &theme, &mut out);
    coalesce(out)
}

/// Lexes `text` into [`InlineTok`]s: backslash escapes and code spans are
/// resolved here (highest precedence), so what remains for emphasis resolution
/// is an unambiguous stream of literals and active delimiters.
fn lex_inline(text: &str) -> Vec<InlineTok> {
    let chars: Vec<char> = text.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() && chars[i + 1].is_ascii_punctuation() {
            toks.push(InlineTok::Lit(chars[i + 1]));
            i += 2;
            continue;
        }
        if c == '`' {
            let fence = chars[i..].iter().take_while(|&&c| c == '`').count();
            if let Some(close) = find_backtick_run(&chars, i + fence, fence) {
                let raw: String = chars[i + fence..close].iter().collect();
                toks.push(InlineTok::Code(trim_code_span(&raw)));
                i = close + fence;
                continue;
            }
            // An unmatched backtick run is literal text.
            toks.extend(std::iter::repeat_n(InlineTok::Lit('`'), fence));
            i += fence;
            continue;
        }
        toks.push(if c == '*' || c == '_' {
            InlineTok::Delim(c)
        } else {
            InlineTok::Lit(c)
        });
        i += 1;
    }
    toks
}

/// Index of the start of the next run of exactly `len` backticks at or after
/// `from`, or `None`. A longer run does not close a shorter fence.
fn find_backtick_run(chars: &[char], from: usize, len: usize) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == '`' {
            let run = chars[i..].iter().take_while(|&&c| c == '`').count();
            if run == len {
                return Some(i);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

/// One leading and one trailing space are stripped from a code span iff it is
/// space-padded on both sides and not all spaces (CommonMark rule).
fn trim_code_span(s: &str) -> String {
    if s.len() >= 2 && s.starts_with(' ') && s.ends_with(' ') && s.chars().any(|c| c != ' ') {
        s[1..s.len() - 1].to_owned()
    } else {
        s.to_owned()
    }
}

/// Recursively resolves emphasis over a token slice, appending styled spans.
///
/// Finds the first balanced delimiter pair, emits the literal text before it,
/// recurses into the emphasised inner slice with the patched style, then
/// recurses over the remainder so trailing emphasis is handled too.
fn render_toks(
    toks: &[InlineTok],
    base: Style,
    theme: &MarkdownTheme,
    out: &mut Vec<Span<'static>>,
) {
    if let Some((open, need, close, _)) = find_emph_tok(toks) {
        emit_literal(&toks[..open], base, theme, out);
        let style = match need {
            n if n >= 3 => base.patch(theme.strong).patch(theme.emphasis),
            2 => base.patch(theme.strong),
            _ => base.patch(theme.emphasis),
        };
        render_toks(&toks[open + need..close], style, theme, out);
        render_toks(&toks[close + need..], base, theme, out);
    } else {
        emit_literal(toks, base, theme, out);
    }
}

/// Emits a delimiter-free token slice as styled spans: literals (and inert
/// stray delimiters) become text under `base`; a code token is its own span.
fn emit_literal(
    toks: &[InlineTok],
    base: Style,
    theme: &MarkdownTheme,
    out: &mut Vec<Span<'static>>,
) {
    let mut buf = String::new();
    for t in toks {
        match t {
            InlineTok::Lit(c) | InlineTok::Delim(c) => buf.push(*c),
            InlineTok::Code(s) => {
                if !buf.is_empty() {
                    out.push(Span::styled(std::mem::take(&mut buf), base));
                }
                out.push(Span::styled(s.clone(), base.patch(theme.code)));
            }
        }
    }
    if !buf.is_empty() {
        out.push(Span::styled(buf, base));
    }
}

/// Whether the token at `idx` is a `Lit` whose char satisfies `f` (a `Delim`
/// or `Code` neighbour counts as neither whitespace nor alphanumeric).
fn lit_is(toks: &[InlineTok], idx: usize, f: impl Fn(char) -> bool) -> bool {
    matches!(toks.get(idx), Some(InlineTok::Lit(c)) if f(*c))
}

/// Finds the first balanced emphasis: `(open, need, close, char)` where `need`
/// is 1 (em), 2 (strong) or 3 (both). A run opens only before a non-space and
/// closes only after a non-space; `_` may not be intra-word.
fn find_emph_tok(toks: &[InlineTok]) -> Option<(usize, usize, usize, char)> {
    let n = toks.len();
    let mut i = 0;
    while i < n {
        if let InlineTok::Delim(c) = toks[i] {
            let run = toks[i..]
                .iter()
                .take_while(|t| matches!(t, InlineTok::Delim(d) if *d == c))
                .count();
            let need = run.min(3);
            // Opener: char after the run is not whitespace; for `_`, the char
            // before the run is not alphanumeric (no intra-word emphasis).
            let opens = !lit_is(toks, i + run, char::is_whitespace)
                && toks.get(i + run).is_some()
                && (c != '_' || i == 0 || !lit_is(toks, i - 1, char::is_alphanumeric));
            if opens {
                if let Some(close) = find_close_tok(toks, i + run, c, need) {
                    return Some((i, need, close, c));
                }
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

/// The index of a closing `c`-run of at least `need` at or after `from`, valid
/// only when preceded by a non-space (and, for `_`, not followed by an
/// alphanumeric so it stays out of words).
fn find_close_tok(toks: &[InlineTok], from: usize, c: char, need: usize) -> Option<usize> {
    let n = toks.len();
    let mut i = from;
    while i < n {
        if matches!(toks[i], InlineTok::Delim(d) if d == c) {
            let here = toks[i..]
                .iter()
                .take_while(|t| matches!(t, InlineTok::Delim(d) if *d == c))
                .count();
            if here >= need
                && i > from
                && !lit_is(toks, i - 1, char::is_whitespace)
                && (c != '_' || !lit_is(toks, i + here, char::is_alphanumeric))
            {
                return Some(i);
            }
            i += here;
        } else {
            i += 1;
        }
    }
    None
}

/// Merges adjacent spans that share a style so output is compact and snapshot
/// diffs stay readable.
fn coalesce(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    let mut out: Vec<Span<'static>> = Vec::with_capacity(spans.len());
    for s in spans {
        if s.content.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.style == s.style => {
                last.content.to_mut().push_str(&s.content);
            }
            _ => out.push(s),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Appends the display rows for `blocks` (a content area `width` wide) to
/// `out`. When `spacing` is set, a blank spacer row separates adjacent blocks
/// so the document breathes the way a rendered page does; list items are laid
/// out *tight* (`spacing` off) so a nested list sits directly under its parent
/// — loose-list blank-line spacing is a deliberately deferred refinement.
fn layout_blocks(
    blocks: &[MdBlock],
    width: usize,
    theme: &MarkdownTheme,
    spacing: bool,
    out: &mut Vec<Line<'static>>,
) {
    for (idx, block) in blocks.iter().enumerate() {
        if spacing && idx > 0 {
            out.push(Line::default());
        }
        match block {
            MdBlock::Heading { level, spans } => {
                let color = theme.heading_color(*level);
                let styled = spans
                    .iter()
                    .map(|s| {
                        Span::styled(s.content.clone(), s.style.patch(theme.heading).fg(color))
                    })
                    .collect::<Vec<_>>();
                wrap_spans(&styled, width, &[], out);
            }
            MdBlock::Paragraph(spans) => wrap_spans(spans, width, &[], out),
            MdBlock::Code { lines } => {
                for l in lines {
                    let mut row: String = l.chars().take(width).collect();
                    // Pad to full width so the code background reads as a block.
                    while row.chars().count() < width {
                        row.push(' ');
                    }
                    out.push(Line::from(Span::styled(row, theme.code)).style(theme.code));
                }
            }
            MdBlock::Quote(inner) => {
                let rail_w = 2.min(width);
                let mut sub = Vec::new();
                layout_blocks(
                    inner,
                    width.saturating_sub(rail_w),
                    theme,
                    spacing,
                    &mut sub,
                );
                for line in sub {
                    let mut spans = vec![Span::styled("│ ", theme.quote)];
                    spans.extend(line.spans);
                    out.push(Line::from(spans));
                }
            }
            MdBlock::List {
                ordered,
                start,
                items,
            } => {
                for (n, item) in items.iter().enumerate() {
                    let label = if *ordered {
                        format!("{}. ", start + n as u64)
                    } else {
                        "• ".to_owned()
                    };
                    let pad = " ".repeat(label.chars().count());
                    let mut sub = Vec::new();
                    // Tight: a nested list/para sits directly under the marker.
                    layout_blocks(
                        item,
                        width.saturating_sub(label.chars().count()),
                        theme,
                        false,
                        &mut sub,
                    );
                    for (k, line) in sub.into_iter().enumerate() {
                        let lead = if k == 0 { label.clone() } else { pad.clone() };
                        let mut spans = vec![Span::styled(lead, theme.marker)];
                        spans.extend(line.spans);
                        out.push(Line::from(spans));
                    }
                }
            }
            MdBlock::Table {
                aligns,
                header,
                rows,
            } => layout_table(aligns, header, rows, width, theme, out),
            MdBlock::Rule => {
                out.push(Line::from(Span::styled("─".repeat(width), theme.rule)));
            }
        }
    }
}

/// The display width of a cell: the `char` count across its spans.
fn cell_width(spans: &[Span<'static>]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// Clips/pads a cell's spans to exactly `colw` columns under `align`. Padding
/// is unstyled spaces so only the text carries the cell's own styling.
fn fit_cell(spans: &[Span<'static>], colw: usize, align: Alignment) -> Vec<Span<'static>> {
    let mut cells: Vec<(char, Style)> = spans
        .iter()
        .flat_map(|s| s.content.chars().map(|c| (c, s.style)))
        .collect();
    cells.truncate(colw);
    let pad = colw - cells.len();
    let (left, right) = match align {
        Alignment::Left => (0, pad),
        Alignment::Right => (pad, 0),
        Alignment::Center => (pad / 2, pad - pad / 2),
    };
    let mut row: Vec<(char, Style)> = Vec::with_capacity(colw);
    row.extend((0..left).map(|_| (' ', Style::new())));
    row.extend(cells);
    row.extend((0..right).map(|_| (' ', Style::new())));
    group_cells(&row)
}

/// Renders a parsed table as a width-fitted box-drawing grid. Column widths
/// are the natural content widths, scaled down proportionally (never below 1,
/// clipping cell text) only when the grid would exceed `width`; a narrower
/// table is left at its natural size rather than stretched.
fn layout_table(
    aligns: &[Alignment],
    header: &[Vec<Span<'static>>],
    rows: &[Vec<Vec<Span<'static>>>],
    width: usize,
    theme: &MarkdownTheme,
    out: &mut Vec<Line<'static>>,
) {
    let ncols = aligns.len();
    // Each column costs colw + 2 padding spaces; plus one vertical rule
    // between/around columns (ncols + 1). Too narrow for even 1-wide columns:
    // skip rather than draw a broken grid.
    let overhead = (ncols + 1) + 2 * ncols;
    if ncols == 0 || width <= overhead {
        return;
    }
    let mut colw: Vec<usize> = (0..ncols)
        .map(|c| {
            let h = cell_width(&header[c]);
            rows.iter()
                .map(|r| cell_width(&r[c]))
                .chain([h])
                .max()
                .unwrap_or(1)
                .max(1)
        })
        .collect();
    let avail = width - overhead;
    let natural: usize = colw.iter().sum();
    if natural > avail {
        let sum = natural.max(1);
        for w in &mut colw {
            *w = (*w * avail / sum).max(1);
        }
        // Flooring can leave us a hair over; trim the widest deterministically.
        while colw.iter().sum::<usize>() > avail {
            let max = *colw.iter().max().unwrap();
            let i = colw.iter().position(|&w| w == max).unwrap();
            colw[i] -= 1;
        }
    }

    let rule = |left: char, mid: char, right: char| -> Line<'static> {
        let mut s = String::new();
        s.push(left);
        for (c, w) in colw.iter().enumerate() {
            s.push_str(&"─".repeat(w + 2));
            s.push(if c + 1 == ncols { right } else { mid });
        }
        Line::from(Span::styled(s, theme.rule))
    };
    let content_row = |cells: &[Vec<Span<'static>>], header: bool| -> Line<'static> {
        let mut spans = Vec::new();
        for c in 0..ncols {
            spans.push(Span::styled("│ ", theme.rule));
            let styled: Vec<Span<'static>> = if header {
                cells[c]
                    .iter()
                    .map(|s| Span::styled(s.content.clone(), s.style.patch(theme.strong)))
                    .collect()
            } else {
                cells[c].clone()
            };
            spans.extend(fit_cell(&styled, colw[c], aligns[c]));
            spans.push(Span::styled(" ", theme.rule));
        }
        spans.push(Span::styled("│", theme.rule));
        Line::from(spans)
    };

    out.push(rule('┌', '┬', '┐'));
    out.push(content_row(header, true));
    out.push(rule('├', '┼', '┤'));
    for r in rows {
        out.push(content_row(r, false));
    }
    out.push(rule('└', '┴', '┘'));
}

/// Word-wraps `spans` to `width`, each row prefixed by `prefix`. Wrapping is
/// done on resolved `(char, style)` cells (the same approach
/// [`Paragraph`](crate::Paragraph) uses) and rebuilt into per-row spans by
/// grouping equal styles, so emphasis runs survive a line break.
fn wrap_spans(
    spans: &[Span<'static>],
    width: usize,
    prefix: &[Span<'static>],
    out: &mut Vec<Line<'static>>,
) {
    let avail = width.max(1);
    let cells: Vec<(char, Style)> = spans
        .iter()
        .flat_map(|s| s.content.chars().map(move |c| (c, s.style)))
        .collect();
    if cells.is_empty() {
        out.push(Line::from(prefix.to_vec()));
        return;
    }
    let mut row: Vec<(char, Style)> = Vec::new();
    let n = cells.len();
    let mut i = 0;
    let flush = |row: &mut Vec<(char, Style)>, out: &mut Vec<Line<'static>>| {
        while matches!(row.last(), Some((c, _)) if c.is_whitespace()) {
            row.pop();
        }
        let mut line_spans: Vec<Span<'static>> = prefix.to_vec();
        line_spans.extend(group_cells(row));
        out.push(Line::from(line_spans));
        row.clear();
    };
    while i < n {
        let ws = cells[i].0.is_whitespace();
        let mut j = i;
        while j < n && cells[j].0.is_whitespace() == ws {
            j += 1;
        }
        let token = &cells[i..j];
        i = j;
        if ws {
            if row.is_empty() {
                // drop leading whitespace at the start of a wrapped row
            } else if row.len() + token.len() <= avail {
                row.extend_from_slice(token);
            } else {
                flush(&mut row, out);
            }
        } else if token.len() <= avail {
            if row.len() + token.len() > avail {
                flush(&mut row, out);
            }
            row.extend_from_slice(token);
        } else {
            let mut k = 0;
            while k < token.len() {
                if row.len() == avail {
                    flush(&mut row, out);
                }
                let take = (avail - row.len()).min(token.len() - k);
                row.extend_from_slice(&token[k..k + take]);
                k += take;
            }
        }
    }
    flush(&mut row, out);
}

/// Groups consecutive equal-style cells back into spans.
fn group_cells(cells: &[(char, Style)]) -> Vec<Span<'static>> {
    let mut out: Vec<Span<'static>> = Vec::new();
    for &(c, st) in cells {
        match out.last_mut() {
            Some(last) if last.style == st => last.content.to_mut().push(c),
            _ => out.push(Span::styled(c.to_string(), st)),
        }
    }
    out
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

    fn span_text(spans: &[Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn heading_is_bold_and_colored_per_level() {
        let blocks = parse_blocks("## Hello");
        assert_eq!(
            blocks,
            vec![MdBlock::Heading {
                level: 2,
                spans: vec![Span::raw("Hello")],
            }]
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        Markdown::new("## Hello").render(buf.area(), &mut buf);
        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.symbol, 'H');
        assert!(cell.modifier.contains(Modifier::BOLD));
        assert_eq!(cell.fg, Color::Cyan);
    }

    #[test]
    fn closing_hashes_and_trailing_space_are_trimmed() {
        let blocks = parse_blocks("# Title ###");
        assert_eq!(
            blocks,
            vec![MdBlock::Heading {
                level: 1,
                spans: vec![Span::raw("Title")],
            }]
        );
    }

    #[test]
    fn strong_emphasis_and_code_resolve_to_styled_spans() {
        let s = parse_inline("a **b** c *d* `e`");
        assert_eq!(span_text(&s), "a b c d e");
        assert!(
            s.iter()
                .any(|x| x.content == "b" && x.style.add_modifier.contains(Modifier::BOLD))
        );
        assert!(
            s.iter()
                .any(|x| x.content == "d" && x.style.add_modifier.contains(Modifier::ITALIC))
        );
        assert!(
            s.iter()
                .any(|x| x.content == "e" && x.style.fg == Some(Color::Yellow))
        );
    }

    #[test]
    fn nested_emphasis_inside_strong() {
        let s = parse_inline("**a *b* c**");
        assert_eq!(span_text(&s), "a b c");
        let b = s.iter().find(|x| x.content == "b").unwrap();
        assert!(b.style.add_modifier.contains(Modifier::BOLD));
        assert!(b.style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn underscore_does_not_emphasize_inside_a_word() {
        let s = parse_inline("call snake_case_name now");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].content, "call snake_case_name now");
        assert!(!s[0].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn backslash_escapes_punctuation() {
        let s = parse_inline(r"not \*bold\* here");
        assert_eq!(span_text(&s), "not *bold* here");
        assert!(
            s.iter()
                .all(|x| !x.style.add_modifier.contains(Modifier::BOLD))
        );
    }

    #[test]
    fn code_span_is_literal_and_strips_one_padding_space() {
        let s = parse_inline("x `a*b*c` y");
        let code = s
            .iter()
            .find(|x| x.style.fg == Some(Color::Yellow))
            .unwrap();
        assert_eq!(code.content, "a*b*c");
        let padded = parse_inline("`` ` ``");
        assert_eq!(padded[0].content, "`");
    }

    #[test]
    fn fenced_code_block_keeps_text_verbatim_and_fills_width() {
        let out = lines(Markdown::new("```\nlet x=*y*;\n```"), 12, 1);
        assert_eq!(out, "let x=*y*;  \n");
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
        Markdown::new("```rust\nfn f(){}\n```").render(buf.area(), &mut buf);
        // Trailing padding still carries the code background style.
        assert_eq!(buf.get(Position::new(11, 0)).unwrap().fg, Color::Yellow);
    }

    #[test]
    fn block_quote_draws_a_rail_and_nests() {
        assert_eq!(lines(Markdown::new("> hi"), 6, 1), "│ hi  \n");
        assert_eq!(lines(Markdown::new("> > deep"), 9, 1), "│ │ deep \n");
    }

    #[test]
    fn bullet_and_ordered_lists_render_markers() {
        assert_eq!(lines(Markdown::new("- a\n- b"), 4, 2), "• a \n• b \n");
        assert_eq!(lines(Markdown::new("3. x\n4. y"), 5, 2), "3. x \n4. y \n");
    }

    #[test]
    fn nested_list_indents_under_its_parent() {
        let src = "- top\n  - child";
        let out = lines(Markdown::new(src), 10, 2);
        // Tight list: the nested item sits directly under its parent, indented
        // by the parent marker's width — no blank spacer between them.
        assert_eq!(out, "• top     \n  • child \n");
    }

    #[test]
    fn a_blank_line_ends_the_list_so_a_following_block_is_not_swallowed() {
        // Regression: `2.\n\n---` once absorbed the rule as lazy continuation
        // of item 2; the blank line must end the list cleanly.
        let blocks = parse_blocks("1. a\n2. b\n\n---");
        assert_eq!(blocks.len(), 2);
        assert!(matches!(blocks[0], MdBlock::List { .. }));
        assert_eq!(blocks[1], MdBlock::Rule);
    }

    #[test]
    fn thematic_break_fills_the_width_and_is_not_a_list() {
        assert_eq!(lines(Markdown::new("---"), 4, 1), "────\n");
        assert_eq!(lines(Markdown::new("* * *"), 4, 1), "────\n");
    }

    #[test]
    fn gfm_table_renders_a_box_drawing_grid() {
        let src = "| A | B |\n| --- | --- |\n| 1 | 2 |";
        assert_eq!(
            lines(Markdown::new(src), 9, 5),
            "┌───┬───┐\n│ A │ B │\n├───┼───┤\n│ 1 │ 2 │\n└───┴───┘\n"
        );
    }

    #[test]
    fn delimiter_row_sets_per_column_alignment() {
        let blocks = parse_blocks("| l | c | r |\n| :-- | :-: | --: |\n| a | b | c |");
        match &blocks[0] {
            MdBlock::Table { aligns, .. } => assert_eq!(
                aligns,
                &[Alignment::Left, Alignment::Center, Alignment::Right]
            ),
            other => panic!("expected a table, got {other:?}"),
        }
        // Right alignment pushes a short value to the cell's right edge.
        assert_eq!(
            lines(Markdown::new("| ab |\n| --: |\n| z |"), 6, 5),
            "┌────┐\n│ ab │\n├────┤\n│  z │\n└────┘\n"
        );
    }

    #[test]
    fn ragged_rows_are_padded_to_the_column_count() {
        let src = "| A | B |\n|---|---|\n| 1 |";
        assert_eq!(
            lines(Markdown::new(src), 9, 5),
            "┌───┬───┐\n│ A │ B │\n├───┼───┤\n│ 1 │   │\n└───┴───┘\n"
        );
    }

    #[test]
    fn a_pipe_line_without_a_delimiter_row_is_just_a_paragraph() {
        // No table without the `---` delimiter row: this stays literal text.
        assert_eq!(lines(Markdown::new("a | b"), 5, 1), "a | b\n");
    }

    #[test]
    fn table_cells_are_inline_parsed() {
        // `code` inside a body cell keeps the code style.
        let mut buf = Buffer::empty(Rect::new(0, 0, 9, 5));
        Markdown::new("| h |\n|---|\n| `c` |").render(buf.area(), &mut buf);
        // Row 3 (data) col content: "│ c │" → 'c' at x=2.
        let cell = buf.get(Position::new(2, 3)).unwrap();
        assert_eq!(cell.symbol, 'c');
        assert_eq!(cell.fg, Color::Yellow);
    }

    #[test]
    fn table_columns_shrink_to_fit_a_narrow_area() {
        // Natural width far exceeds the area; columns scale down, never panic.
        let src = "| long header | another |\n|---|---|\n| value one | value two |";
        let out = lines(Markdown::new(src), 16, 5);
        // Every rendered row is clipped to the 16-cell area, grid stays intact.
        assert!(out.lines().all(|l| l.chars().count() == 16));
        assert!(out.starts_with('┌'));
    }

    #[test]
    fn paragraph_soft_wraps_to_width_and_keeps_emphasis() {
        let md = Markdown::new("the **quick** brown");
        let rows = md.lines(6);
        assert_eq!(rows.len(), 3);
        // "quick" stayed bold across the wrap boundary.
        let quick_row = &rows[1];
        assert!(
            quick_row
                .spans
                .iter()
                .any(|s| s.content.contains("quick")
                    && s.style.add_modifier.contains(Modifier::BOLD))
        );
    }

    #[test]
    fn blocks_are_separated_by_a_spacer_row() {
        // Heading, blank spacer, paragraph.
        assert_eq!(
            lines(Markdown::new("# H\n\nbody"), 4, 3),
            "H   \n    \nbody\n"
        );
    }

    #[test]
    fn scroll_skips_composed_rows() {
        let md = Markdown::new("a\n\nb\n\nc").scroll(2);
        // Rows: "a","", "b","", "c" → skip 2 → start at "b".
        assert_eq!(lines(md, 1, 1), "b\n");
    }

    #[test]
    fn block_frames_content_in_the_inner_area() {
        assert_eq!(
            lines(Markdown::new("hi").block(Block::bordered()), 4, 3),
            "┌──┐\n│hi│\n└──┘\n"
        );
    }

    #[test]
    fn zero_area_and_zero_width_are_no_ops() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Markdown::new("# x").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        assert!(Markdown::new("# x").lines(0).is_empty());
    }
}
