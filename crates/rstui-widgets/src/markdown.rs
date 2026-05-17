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
//! - fenced code blocks (``` ``` ``` / `~~~`); the info string's language is
//!   shown as a dim caption and the code gets a deterministic,
//!   dependency-free generic syntax highlight (strings/numbers/comments/a
//!   keyword core), drawn on a filled background and never reflowed
//! - block quotes (`>`), nesting recursively, drawn with a `│ ` rail
//! - bullet (`-`/`*`/`+`) and ordered (`1.`/`1)`) lists, including nested
//!   lists and multi-line items via indentation
//! - thematic breaks (`---`/`***`/`___`)
//! - GFM pipe tables with a `:`-aligned delimiter row, drawn as a
//!   width-fitted box-drawing grid with per-column alignment
//! - `[text](href)` links and `<autolink>`s: the label is styled and the
//!   targets are exposed in reading order by [`Markdown::links`] (the
//!   [`Link`] activation registry), keeping the href out of the
//!   rendered glyphs; [`Markdown::focused_link`] highlights one for keyboard
//!   focus and [`Markdown::link_at`] / [`Markdown::link_regions`] give the
//!   deterministic screen geometry for mouse clicks — the full
//!   registry → focus/click → activation loop, all in-stream (no runtime
//!   geometry coupling needed)
//! - `![alt](src)` images — terminals can't show the bitmap, so the alt text
//!   stands in for it, distinctly styled (and itself inline-parsed)
//! - 4-space **indented code blocks** (kept verbatim, like fenced code)
//! - **setext headings** (`===` → H1, `---` → H2 underline), disambiguated
//!   from thematic breaks and list markers
//! - **HTML passthrough** (terminal-appropriate): named/numeric entities are
//!   decoded, comments removed, `<b>/<strong>/<i>/<em>/<code>` mapped to their
//!   markdown styling, `<br>` is a hard line break, and every other tag is
//!   stripped with its text kept
//! - **reference-style links/images** — full `[text][label]`, collapsed
//!   `[text][]`, and shortcut `[text]` (and their `![…]` forms) resolved
//!   against `[label]: dest "title"` definitions (document-scoped, not
//!   rendered, fence-aware)
//! - **loose vs tight lists** — a blank line between items (or a multi-block
//!   item) renders inter-item spacer rows; a tight list stays compact
//!
//! Every CommonMark-ish construct this renderer aims at is now implemented;
//! there are no deferred markdown features.
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
use crate::link::Link;
use rstui_core::{Alignment, Buffer, Color, Line, Modifier, Position, Rect, Span, Style, Widget};

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
    /// A `[text](href)` / autolink label. The href is kept out of the render
    /// (retrieve it via [`Markdown::links`]); only the label is shown, styled.
    pub link: Style,
    /// The label of the link at [`Markdown::focused_link`] — patched over
    /// [`link`](Self::link) so the focused reference reads as selected (the
    /// same "selection bar" idea [`List`](crate::List) uses, for links).
    pub link_focused: Style,
    /// An `![alt](src)` image's substitute text (terminals can't show the
    /// bitmap, so the alt text stands in for it, distinctly styled).
    pub image: Style,
    /// The dim language caption above a fenced code block (its info string).
    pub code_lang: Style,
    /// Generic syntax accents inside a code block, patched over [`code`](Self::code):
    /// string literals.
    pub code_string: Style,
    /// …numeric literals.
    pub code_number: Style,
    /// …line/block comments.
    pub code_comment: Style,
    /// …a common cross-language keyword core.
    pub code_keyword: Style,
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
            link: Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
            link_focused: Style::new().add_modifier(Modifier::REVERSED),
            image: Style::new()
                .fg(Color::Magenta)
                .add_modifier(Modifier::ITALIC),
            code_lang: Style::new()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
            code_string: Style::new().fg(Color::Green),
            code_number: Style::new().fg(Color::Magenta),
            code_comment: Style::new()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
            code_keyword: Style::new().fg(Color::Blue),
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
    focused_link: Option<usize>,
}

/// Where a link's label landed on screen, returned by
/// [`Markdown::link_regions`].
///
/// `index` is the key into [`Markdown::links`] (the activation registry);
/// `rect` is the cells the label occupies. A label that soft-wraps yields one
/// [`LinkRegion`] per occupied row, all sharing the same `index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkRegion {
    /// The link's position in [`Markdown::links`].
    pub index: usize,
    /// The screen rectangle (one row tall) the label covers.
    pub rect: Rect,
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
            focused_link: None,
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

    /// Highlights the link at this index into [`links`](Self::links) with the
    /// theme's [`link_focused`](MarkdownTheme::link_focused) style.
    ///
    /// A pure projection of caller-owned focus, exactly like
    /// [`Checkbox`](crate::Checkbox)'s `focused`: the app owns which link is
    /// focused (cycling it with Tab/arrows over the `links()` registry) and
    /// the reducer turns Enter into a
    /// [`LinkActivation`](crate::link::LinkActivation). An out-of-range index
    /// simply highlights nothing — a caller-owned number never panics.
    #[must_use]
    pub fn focused_link(mut self, index: usize) -> Self {
        self.focused_link = Some(index);
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
        let (defs, src) = collect_defs(self.source.as_ref());
        let mut links = Vec::new();
        let blocks = blocks_into(&src, &mut links, self.focused_link, &self.theme, &defs);
        let mut rows = Vec::new();
        layout_blocks(&blocks, width as usize, &self.theme, true, &mut rows);
        rows
    }

    /// The document's links, in reading order — the activation registry.
    ///
    /// The index into this list is the focus key: a host tracks a focused
    /// index in its own state (the [`FocusRing`](rstui_core::FocusRing) /
    /// [`List`](crate::List) selection shape), and the reducer turns Enter or
    /// a click into a [`LinkActivation`](crate::link::LinkActivation) via
    /// [`Link::activate`](crate::Link::activate). Width-independent, so it can
    /// be called once per frame regardless of layout.
    #[must_use]
    pub fn links(&self) -> Vec<Link<'static>> {
        document_links(self.source.as_ref())
    }

    /// The screen rectangles every link label occupies when this widget is
    /// rendered into `area` (same block/scroll/width as
    /// [`render`](Widget::render)) — the geometry half of clickable links.
    ///
    /// Deterministic and side-effect-free: it renders into a scratch buffer
    /// twice with the link style toggled, so the cells that differ are exactly
    /// the link-label cells (no other theme element sets `REVERSED`, and the
    /// widget applies the link style only to labels). Those cells are then
    /// segmented in [`links`](Self::links) order by each label's char length,
    /// so a soft-wrapped label keeps one `index` across its rows and two
    /// back-to-back links split at the right boundary. (Edge: a label with a
    /// space at the exact wrap column loses that trimmed space from its
    /// measured width — a rare under-measure, never a panic.)
    #[must_use]
    pub fn link_regions(&self, area: Rect) -> Vec<LinkRegion> {
        let links = self.links();
        if area.is_empty() || links.is_empty() {
            return Vec::new();
        }
        let probe = |reversed: bool| -> Buffer {
            let mut md = self.clone();
            let mark = if reversed {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                Style::new()
            };
            md.theme.link = mark;
            md.theme.link_focused = mark;
            let mut buf = Buffer::empty(area);
            md.render(area, &mut buf);
            buf
        };
        let plain = probe(false);
        let marked = probe(true);
        let key =
            |buf: &Buffer, p: Position| buf.get(p).map(|c| (c.symbol, c.fg, c.bg, c.modifier));
        let is_link = |p: Position| key(&plain, p) != key(&marked, p);

        let label_len: Vec<usize> = links.iter().map(Link::width).collect();
        let mut cursor = 0usize;
        let mut remaining = label_len.first().copied().unwrap_or(0);
        let mut out = Vec::new();
        for y in area.top()..area.bottom() {
            let mut x = area.left();
            while x < area.right() {
                if !is_link(Position::new(x, y)) {
                    x += 1;
                    continue;
                }
                while cursor < links.len() && remaining == 0 {
                    cursor += 1;
                    remaining = label_len.get(cursor).copied().unwrap_or(0);
                }
                if cursor >= links.len() {
                    break;
                }
                let start = x;
                while x < area.right() && remaining > 0 && is_link(Position::new(x, y)) {
                    remaining -= 1;
                    x += 1;
                }
                out.push(LinkRegion {
                    index: cursor,
                    rect: Rect::new(start, y, x - start, 1),
                });
                if remaining == 0 {
                    cursor += 1;
                    remaining = label_len.get(cursor).copied().unwrap_or(0);
                }
            }
        }
        out
    }

    /// The registry index of the link whose label covers `position` (screen
    /// coordinates, the same `area` passed to [`render`](Widget::render)), or
    /// `None`.
    ///
    /// The mouse half of activation as a raw index. Prefer
    /// [`link_activation_at`](Self::link_activation_at), which returns the
    /// resolved [`LinkActivation`](crate::link::LinkActivation) in one call
    /// (no caller-side index/`links()` desync).
    #[must_use]
    pub fn link_at(&self, position: Position, area: Rect) -> Option<usize> {
        self.link_regions(area).into_iter().find_map(|r| {
            let in_x = position.x >= r.rect.x && position.x < r.rect.x.saturating_add(r.rect.width);
            (in_x && position.y == r.rect.y).then_some(r.index)
        })
    }

    /// Resolve a click `position` straight to the
    /// [`LinkActivation`](crate::link::LinkActivation) (index + owned `href`)
    /// it activates, or `None` for plain text / outside.
    ///
    /// This is the one call a reducer needs for clickable links — the
    /// immediate-mode equivalent of Textual's per-span click meta: the
    /// hit-test and the `href` are taken from the *same* parse of the same
    /// immutable source, so a stale index or a `link_at`/`links()` mismatch
    /// (the foot-gun the raw [`link_at`](Self::link_at) + `links()[i]` pattern
    /// invites) is structurally impossible.
    ///
    /// ```
    /// use rstui_core::{Position, Rect};
    /// use rstui_widgets::Markdown;
    /// let md = Markdown::new("see [docs](https://rstui.dev) here");
    /// let area = Rect::new(0, 0, 40, 3);
    /// if let Some(act) = md.link_activation_at(Position::new(6, 0), area) {
    ///     assert_eq!(act.href, "https://rstui.dev");
    /// }
    /// ```
    #[must_use]
    pub fn link_activation_at(
        &self,
        position: Position,
        area: Rect,
    ) -> Option<crate::link::LinkActivation> {
        let index = self.link_at(position, area)?;
        self.links().get(index).map(|link| link.activate(index))
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
        if let Some(b) = &self.block {
            b.render_ref(area, buf);
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
        /// The fenced info string's first word (the language), or empty for
        /// an indented code block / a bare fence.
        lang: String,
    },
    Quote(Vec<MdBlock>),
    List {
        ordered: bool,
        start: u64,
        items: Vec<Vec<MdBlock>>,
        /// A *loose* list (a blank line between items, or a multi-block item)
        /// renders a blank spacer row between items; a tight list does not.
        loose: bool,
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

/// Splits `src` into [`MdBlock`]s, discarding links and focus — a test-only
/// convenience over [`blocks_into`] (the render path threads links/focus, so
/// this thin wrapper is exercised solely by the block-shape unit tests).
#[cfg(test)]
fn parse_blocks(src: &str) -> Vec<MdBlock> {
    let (defs, src) = collect_defs(src);
    blocks_into(
        &src,
        &mut Vec::new(),
        None,
        &MarkdownTheme::default(),
        &defs,
    )
}

/// Every `[text](href)` / autolink in `src`, in reading order — the registry
/// [`Markdown::links`] exposes for focus and activation. Labels/hrefs are
/// theme-independent, so the default theme is fine here.
fn document_links(src: &str) -> Vec<Link<'static>> {
    let (defs, src) = collect_defs(src);
    let mut links = Vec::new();
    blocks_into(&src, &mut links, None, &MarkdownTheme::default(), &defs);
    links
}

/// Builds the spans for `text`, appending its links to `links` (so
/// `links.len()` is the running document index) and flagging the one at
/// `focused` for the focus style. Inline styling uses the supplied `theme`
/// so a custom [`MarkdownTheme`] actually reaches code/emphasis/link spans.
fn inline(
    text: &str,
    links: &mut Vec<Link<'static>>,
    focused: Option<usize>,
    theme: &MarkdownTheme,
    defs: &LinkDefs,
) -> Vec<Span<'static>> {
    let (spans, mut found) = inline_spans_and_links(text, links.len(), focused, theme, defs);
    links.append(&mut found);
    spans
}

/// Splits `src` into [`MdBlock`]s, appending links to `links` in reading
/// order. Line-oriented, single pass, no lookahead beyond fence/list
/// continuation scanning.
fn blocks_into(
    src: &str,
    links: &mut Vec<Link<'static>>,
    focused: Option<usize>,
    theme: &MarkdownTheme,
    defs: &LinkDefs,
) -> Vec<MdBlock> {
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
            let lang = info.split_whitespace().next().unwrap_or("").to_owned();
            out.push(MdBlock::Code { lines: body, lang });
            continue;
        }

        // Indented code block: ≥4 leading spaces at block start. Checked
        // before thematic/heading/quote/list so a 4-space indent makes those
        // literal code (CommonMark), and exactly 4 spaces are stripped so the
        // code keeps its own relative indentation.
        if line.len() - trimmed.len() >= 4 {
            let mut body = Vec::new();
            while i < lines.len() {
                let l = lines[i];
                if l.trim().is_empty() {
                    body.push(String::new());
                    i += 1;
                } else if l.len() - l.trim_start().len() >= 4 {
                    body.push(l.chars().skip(4).collect());
                    i += 1;
                } else {
                    break;
                }
            }
            while body.last().is_some_and(String::is_empty) {
                body.pop();
            }
            out.push(MdBlock::Code {
                lines: body,
                lang: String::new(),
            });
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
                spans: inline(text, links, focused, theme, defs),
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
            out.push(MdBlock::Quote(blocks_into(
                &quoted.join("\n"),
                links,
                focused,
                theme,
                defs,
            )));
            continue;
        }

        if starts_table(&lines, i) {
            let aligns = table_delim_aligns(lines[i + 1]).expect("starts_table verified it");
            let ncols = aligns.len();
            let mut header = Vec::with_capacity(ncols);
            for c in normalize_row(split_table_row(line), ncols) {
                header.push(inline(&c, links, focused, theme, defs));
            }
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() {
                let row = lines[i];
                let t = row.trim();
                if t.is_empty() || !t.contains('|') || table_delim_aligns(row).is_some() {
                    break;
                }
                let mut cells = Vec::with_capacity(ncols);
                for c in normalize_row(split_table_row(row), ncols) {
                    cells.push(inline(&c, links, focused, theme, defs));
                }
                rows.push(cells);
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
            let (block, next) = parse_list(&lines, i, marker, links, focused, theme, defs);
            out.push(block);
            i = next;
            continue;
        }

        // Paragraph: gather following lines until a blank line or a line that
        // begins a different construct. Soft breaks join with a space. A
        // `===`/`---`-only line right after paragraph text is a setext
        // heading underline (it takes priority over a thematic break in that
        // position, per CommonMark) and turns the paragraph into a heading.
        let mut buf = String::new();
        let mut setext = None;
        while i < lines.len() {
            let l = lines[i];
            let t = l.trim_start();
            if !buf.is_empty() {
                if let Some(level) = setext_level(t) {
                    setext = Some(level);
                    i += 1; // consume the underline
                    break;
                }
            }
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
        let spans = inline(&buf, links, focused, theme, defs);
        out.push(match setext {
            Some(level) => MdBlock::Heading { level, spans },
            None => MdBlock::Paragraph(spans),
        });
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
fn parse_list(
    lines: &[&str],
    start: usize,
    first: ListMarker,
    links: &mut Vec<Link<'static>>,
    focused: Option<usize>,
    theme: &MarkdownTheme,
    defs: &LinkDefs,
) -> (MdBlock, usize) {
    let ordered = first.ordered.is_some();
    let list_start = first.ordered.unwrap_or(1);
    let mut items: Vec<Vec<MdBlock>> = Vec::new();
    let mut loose = false;
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
        let mut interior_blank = false;
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
                // Content resuming after a blank means the item has
                // blank-separated blocks — that makes the whole list loose.
                if saw_blank {
                    interior_blank = true;
                }
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
        let item_blocks = blocks_into(&body.join("\n"), links, focused, theme, defs);
        // CommonMark looseness: a blank line *between* this item's blocks
        // (`interior_blank`), or *between* this item and a following sibling
        // marker, makes the whole list loose. A blank that merely ends the
        // list, or a trailing blank before EOF, stays tight — and a nested
        // sub-list with no blank line before it is still tight.
        let next_is_sibling = i < lines.len()
            && list_marker(lines[i])
                .is_some_and(|m| m.indent == first.indent && m.ordered.is_some() == ordered);
        if interior_blank || (saw_blank && !end_list && next_is_sibling) {
            loose = true;
        }
        items.push(item_blocks);
        if end_list {
            break;
        }
    }
    (
        MdBlock::List {
            ordered,
            start: list_start,
            items,
            loose,
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

/// A setext heading underline: a run of only `=` (level 1) or only `-`
/// (level 2), trailing spaces allowed, at least one marker char.
fn setext_level(t: &str) -> Option<u8> {
    let s = t.trim_end();
    if s.is_empty() {
        return None;
    }
    if s.chars().all(|c| c == '=') {
        Some(1)
    } else if s.chars().all(|c| c == '-') {
        Some(2)
    } else {
        None
    }
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
    /// A resolved `[label](href)` or `<autolink>`. `label` is raw markdown
    /// (re-parsed for display so emphasis inside link text works); `href` is
    /// the literal target, kept out of the rendered glyphs. `focused` is set
    /// only on the one link at [`Markdown::focused_link`].
    Link {
        label: String,
        href: String,
        focused: bool,
    },
    /// A resolved `![alt](src)` image. Terminals can't show the bitmap, so the
    /// `alt` text is rendered (styled) in its place; `src` is intentionally
    /// not surfaced yet (no activation semantics, unlike a link).
    Image { alt: String },
}

/// Parses inline markdown into owned styled spans.
///
/// Precedence follows CommonMark's shape closely enough for real docs: a
/// backslash escapes the next ASCII punctuation char; `` `code` `` binds
/// tighter than emphasis and is never re-parsed; `**`/`__` is strong, `*`/`_`
/// is emphasis, `***`/`___` is both, matched non-greedily and recursively so
/// `**a *b* c**` nests. `_` does not open or close inside a word so
/// `snake_case` is left alone.
/// Plain-text + default-theme inline parse: used to derive a link's registry
/// label (markup stripped) and by the inline unit tests.
fn parse_inline(text: &str) -> Vec<Span<'static>> {
    inline_spans_and_links(text, 0, None, &MarkdownTheme::default(), &LinkDefs::new()).0
}

/// Like [`parse_inline`] but also returns the links it contains, in order,
/// and flags the one whose *document* index (`base` + its local position)
/// equals `focused` so it renders with the focus style. The registry label
/// is the link's *rendered plain text* (markup stripped), which is what a
/// host shows in a "links in this document" affordance.
fn inline_spans_and_links(
    text: &str,
    base: usize,
    focused: Option<usize>,
    theme: &MarkdownTheme,
    defs: &LinkDefs,
) -> (Vec<Span<'static>>, Vec<Link<'static>>) {
    let resolved = resolve_refs(text, defs);
    let prepared = prepare_html(&resolved);
    let mut toks = lex_inline(&prepared);
    let mut links = Vec::new();
    for t in &mut toks {
        if let InlineTok::Link {
            label,
            href,
            focused: is_focused,
        } = t
        {
            let plain: String = parse_inline(label)
                .iter()
                .map(|s| s.content.as_ref())
                .collect();
            if focused == Some(base + links.len()) {
                *is_focused = true;
            }
            links.push(Link::new(plain, href.clone()));
        }
    }
    let mut out = Vec::new();
    render_toks(&toks, Style::new(), theme, &mut out);
    (coalesce(out), links)
}

/// Document-scoped link reference definitions: a normalised label → its
/// destination. Resolved before inline parsing so `[text][label]` and friends
/// become ordinary inline links the existing lexer already handles.
type LinkDefs = std::collections::BTreeMap<String, String>;

/// CommonMark label matching is case-insensitive and whitespace-collapsed.
fn normalize_ref(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Parses a link reference definition line — `[label]: dest "optional title"`
/// with ≤3 leading spaces — into `(label, dest)`, or `None`.
fn parse_def_line(line: &str) -> Option<(String, String)> {
    let indent = line.len() - line.trim_start().len();
    if indent > 3 {
        return None;
    }
    let s = line.trim_start();
    let chars: Vec<char> = s.chars().collect();
    if chars.first() != Some(&'[') {
        return None;
    }
    let mut j = 1;
    while j < chars.len() && chars[j] != ']' {
        if chars[j] == '\\' && j + 1 < chars.len() {
            j += 1;
        }
        j += 1;
    }
    if j >= chars.len() || chars.get(j + 1) != Some(&':') {
        return None;
    }
    let label: String = chars[1..j].iter().collect();
    if label.trim().is_empty() {
        return None;
    }
    let rest: String = chars[j + 2..].iter().collect();
    let rest = rest.trim();
    let dest = rest.split_whitespace().next()?;
    let dest = dest
        .strip_prefix('<')
        .and_then(|d| d.strip_suffix('>'))
        .unwrap_or(dest);
    if dest.is_empty() {
        return None;
    }
    Some((label, dest.to_owned()))
}

/// Scans link reference definitions out of `src`, returning the def map and
/// the source with definition lines blanked so they never render.
fn collect_defs(src: &str) -> (LinkDefs, String) {
    let mut defs = LinkDefs::new();
    let mut kept: Vec<&str> = Vec::new();
    let mut fence: Option<char> = None;
    for raw in src.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = line.trim_start();
        // Definitions inside a fenced code block are code, not definitions.
        match fence {
            Some(f) if fence_close(trimmed, f) => fence = None,
            Some(_) => {}
            None => {
                if let Some((f, _)) = fence_open(trimmed) {
                    fence = Some(f);
                } else if let Some((label, dest)) = parse_def_line(line) {
                    defs.entry(normalize_ref(&label)).or_insert(dest);
                    kept.push("");
                    continue;
                }
            }
        }
        kept.push(line);
    }
    (defs, kept.join("\n"))
}

/// Rewrites reference links/images — full `[t][label]`, collapsed `[t][]`,
/// shortcut `[t]`, and their `![...]` image forms — into ordinary inline
/// `[t](dest)` when the label resolves in `defs`. Unresolved references and
/// inline `[t](url)` links are copied through untouched (CommonMark: an
/// unknown reference renders literally); code spans pass verbatim.
fn resolve_refs(text: &str, defs: &LinkDefs) -> String {
    if defs.is_empty() || !text.contains('[') {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '`' {
            let fence = chars[i..].iter().take_while(|&&x| x == '`').count();
            if let Some(close) = find_backtick_run(&chars, i + fence, fence) {
                out.extend(chars[i..close + fence].iter());
                i = close + fence;
                continue;
            }
        }
        if c == '\\' && i + 1 < n {
            out.push(c);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        let bang = c == '!' && chars.get(i + 1) == Some(&'[');
        if c == '[' || bang {
            let open = if bang { i + 1 } else { i };
            if let Some((text_inner, label, next)) = scan_reference(&chars, open) {
                let key = normalize_ref(if label.is_empty() {
                    &text_inner
                } else {
                    &label
                });
                if let Some(dest) = defs.get(&key) {
                    if bang {
                        out.push('!');
                    }
                    out.push('[');
                    out.push_str(&text_inner);
                    out.push_str("](");
                    out.push_str(dest);
                    out.push(')');
                    i = next;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Scans a `[text]` then optional `[label]` starting at `chars[open] == '['`.
/// Returns `(text, label, next)` where an absent/empty `[label]` yields an
/// empty `label` (shortcut/collapsed). `None` if `[text]` is unbalanced or is
/// immediately an inline link (`](`), which the lexer handles instead.
fn scan_reference(chars: &[char], open: usize) -> Option<(String, String, usize)> {
    let n = chars.len();
    let mut j = open + 1;
    let mut depth = 1;
    while j < n {
        match chars[j] {
            '\\' if j + 1 < n => j += 2,
            '[' => {
                depth += 1;
                j += 1;
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                j += 1;
            }
            _ => j += 1,
        }
    }
    if depth != 0 {
        return None;
    }
    let text: String = chars[open + 1..j].iter().collect();
    // `](` is an inline link — leave it for the lexer.
    if chars.get(j + 1) == Some(&'(') {
        return None;
    }
    if chars.get(j + 1) == Some(&'[') {
        let mut k = j + 2;
        let mut d = 1;
        while k < n {
            match chars[k] {
                '\\' if k + 1 < n => k += 2,
                '[' => {
                    d += 1;
                    k += 1;
                }
                ']' => {
                    d -= 1;
                    if d == 0 {
                        break;
                    }
                    k += 1;
                }
                _ => k += 1,
            }
        }
        if d != 0 {
            return None;
        }
        let label: String = chars[j + 2..k].iter().collect();
        Some((text, label, k + 1))
    } else {
        Some((text, String::new(), j + 1))
    }
}

/// Terminal HTML "passthrough" at the inline layer, applied before lexing:
///
/// - named and numeric (`&#dd;` / `&#xhh;`) entities are decoded to their
///   characters;
/// - `<!-- … -->` comments are removed;
/// - `<b>`/`<strong>` → `**`, `<i>`/`<em>` → `*`, `<code>` → `` ` ``,
///   `<br>`/`<br/>` → a hard line break, so the existing emphasis/code engine
///   renders them;
/// - every other tag is stripped (its text content is kept);
/// - inline code spans pass through verbatim (HTML is never interpreted
///   inside code), backslash escapes are preserved, and `<…>` autolinks are
///   left for the lexer.
fn prepare_html(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '`' {
            let fence = chars[i..].iter().take_while(|&&x| x == '`').count();
            if let Some(close) = find_backtick_run(&chars, i + fence, fence) {
                out.extend(chars[i..close + fence].iter());
                i = close + fence;
                continue;
            }
        }
        if c == '\\' && i + 1 < n {
            out.push(c);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '&' {
            if let Some((decoded, len)) = decode_entity(&chars, i) {
                out.push_str(&decoded);
                i += len;
                continue;
            }
        }
        if c == '<' {
            if scan_autolink(&chars, i).is_some() {
                out.push(c);
                i += 1;
                continue;
            }
            if chars[i..].starts_with(&['<', '!', '-', '-']) {
                let mut j = i + 4;
                while j + 2 < n && !(chars[j] == '-' && chars[j + 1] == '-' && chars[j + 2] == '>')
                {
                    j += 1;
                }
                i = if j + 2 < n { j + 3 } else { n };
                continue;
            }
            if let Some((repl, len)) = map_html_tag(&chars, i) {
                out.push_str(repl);
                i += len;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Decodes an HTML entity beginning at `chars[i] == '&'`. Returns the decoded
/// text and the number of `char`s consumed (including `&` and `;`), or `None`
/// if it is not a well-formed entity (then `&` is a literal).
fn decode_entity(chars: &[char], i: usize) -> Option<(String, usize)> {
    let n = chars.len();
    let semi = (i + 1..n.min(i + 33)).find(|&k| chars[k] == ';')?;
    let body: String = chars[i + 1..semi].iter().collect();
    let len = semi - i + 1;
    if let Some(num) = body.strip_prefix('#') {
        let code = if let Some(hex) = num.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            num.parse::<u32>().ok()?
        };
        let ch = char::from_u32(code)?;
        return Some((ch.to_string(), len));
    }
    let decoded = match body.as_str() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => "\u{00a0}",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "mdash" => "—",
        "ndash" => "–",
        "hellip" => "…",
        "laquo" => "«",
        "raquo" => "»",
        "lsquo" => "‘",
        "rsquo" => "’",
        "ldquo" => "“",
        "rdquo" => "”",
        "deg" => "°",
        "plusmn" => "±",
        "times" => "×",
        "divide" => "÷",
        "micro" => "µ",
        "para" => "¶",
        "sect" => "§",
        "middot" => "·",
        "bull" => "•",
        "dagger" => "†",
        "Dagger" => "‡",
        "frac12" => "½",
        "frac14" => "¼",
        "frac34" => "¾",
        "sup2" => "²",
        "sup3" => "³",
        "larr" => "←",
        "rarr" => "→",
        "uarr" => "↑",
        "darr" => "↓",
        "harr" => "↔",
        "infin" => "∞",
        "ne" => "≠",
        "le" => "≤",
        "ge" => "≥",
        "equiv" => "≡",
        "asymp" => "≈",
        "euro" => "€",
        "pound" => "£",
        "yen" => "¥",
        "cent" => "¢",
        "check" => "✓",
        "cross" => "✗",
        "star" => "★",
        "hearts" => "♥",
        _ => return None,
    };
    Some((decoded.to_owned(), len))
}

/// Maps an HTML tag beginning at `chars[i] == '<'` to its markdown
/// replacement (`""` strips an unrecognised but well-formed tag, keeping its
/// inner text). Returns the replacement and `char`s consumed, or `None` if
/// this is not a well-formed tag (then `<` is a literal).
fn map_html_tag(chars: &[char], i: usize) -> Option<(&'static str, usize)> {
    let n = chars.len();
    let mut j = i + 1;
    if j < n && chars[j] == '/' {
        j += 1;
    }
    let name_start = j;
    while j < n && chars[j].is_ascii_alphanumeric() {
        j += 1;
    }
    if j == name_start {
        return None; // `<` not followed by a tag name
    }
    let name: String = chars[name_start..j].iter().collect();
    // Skip to the closing `>` (attributes/`/` are ignored).
    let close = (j..n).find(|&k| chars[k] == '>')?;
    let len = close - i + 1;
    let repl = match name.to_ascii_lowercase().as_str() {
        "b" | "strong" => "**",
        "i" | "em" => "*",
        "code" => "`",
        "br" => "\n",
        _ => "",
    };
    Some((repl, len))
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
        if c == '!' && chars.get(i + 1) == Some(&'[') {
            // An image is link syntax with a `!` sigil: `![alt](src)`.
            if let Some((alt, _src, next)) = scan_link(&chars, i + 1) {
                toks.push(InlineTok::Image { alt });
                i = next;
                continue;
            }
        }
        if c == '[' {
            if let Some((label, href, next)) = scan_link(&chars, i) {
                toks.push(InlineTok::Link {
                    label,
                    href,
                    focused: false,
                });
                i = next;
                continue;
            }
        }
        if c == '<' {
            if let Some((target, next)) = scan_autolink(&chars, i) {
                let href = if is_email(&target) {
                    format!("mailto:{target}")
                } else {
                    target.clone()
                };
                toks.push(InlineTok::Link {
                    label: target,
                    href,
                    focused: false,
                });
                i = next;
                continue;
            }
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

/// Scans `[label](href)` starting at `chars[i] == '['`. Returns the raw label,
/// the cleaned href, and the index just past the `)`, or `None` if the shape
/// is incomplete (then `[` is treated as a literal). Brackets and parens are
/// balanced; `\]`/`\)` are escaped; an optional `"title"` and `<…>` wrapper on
/// the destination are dropped. Link labels do not contain another link.
fn scan_link(chars: &[char], i: usize) -> Option<(String, String, usize)> {
    let n = chars.len();
    let mut j = i + 1;
    let mut depth = 1;
    while j < n {
        match chars[j] {
            '\\' if j + 1 < n => j += 2,
            '[' => {
                depth += 1;
                j += 1;
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                j += 1;
            }
            _ => j += 1,
        }
    }
    if depth != 0 || j + 1 >= n || chars[j + 1] != '(' {
        return None;
    }
    let label: String = chars[i + 1..j].iter().collect();
    let mut k = j + 2;
    let mut pdepth = 1;
    while k < n {
        match chars[k] {
            '\\' if k + 1 < n => k += 2,
            '(' => {
                pdepth += 1;
                k += 1;
            }
            ')' => {
                pdepth -= 1;
                if pdepth == 0 {
                    break;
                }
                k += 1;
            }
            _ => k += 1,
        }
    }
    if pdepth != 0 {
        return None;
    }
    let dest: String = chars[j + 2..k].iter().collect();
    let dest = dest.trim();
    // Drop an optional `"title"`: the href is the first whitespace-free run.
    let href = dest.split_whitespace().next().unwrap_or("");
    let href = href
        .strip_prefix('<')
        .and_then(|h| h.strip_suffix('>'))
        .unwrap_or(href);
    Some((label, href.to_owned(), k + 1))
}

/// Scans an `<scheme:…>` / `<email>` autolink at `chars[i] == '<'`. Returns
/// the inner target and the index past `>`, or `None` if it is not a valid
/// autolink (no spaces/controls; an absolute scheme or an email shape).
fn scan_autolink(chars: &[char], i: usize) -> Option<(String, usize)> {
    let n = chars.len();
    let mut j = i + 1;
    while j < n && chars[j] != '>' {
        if chars[j].is_whitespace() || chars[j] == '<' {
            return None;
        }
        j += 1;
    }
    if j >= n {
        return None;
    }
    let inner: String = chars[i + 1..j].iter().collect();
    let scheme_uri = inner.split_once(':').is_some_and(|(s, _)| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-')
    });
    if scheme_uri || is_email(&inner) {
        Some((inner, j + 1))
    } else {
        None
    }
}

/// A minimal `local@domain.tld` shape check for autolink emails.
fn is_email(s: &str) -> bool {
    match s.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        }
        None => false,
    }
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
            InlineTok::Link { label, focused, .. } => {
                if !buf.is_empty() {
                    out.push(Span::styled(std::mem::take(&mut buf), base));
                }
                // The label is re-parsed so emphasis/code inside link text
                // works; the link style sits beneath it (the focused link
                // gets the selection patch on top). The href never reaches
                // the glyphs — `Markdown::links()` exposes it instead.
                let link_style = if *focused {
                    base.patch(theme.link).patch(theme.link_focused)
                } else {
                    base.patch(theme.link)
                };
                let inner = lex_inline(label);
                render_toks(&inner, link_style, theme, out);
            }
            InlineTok::Image { alt } => {
                if !buf.is_empty() {
                    out.push(Span::styled(std::mem::take(&mut buf), base));
                }
                // The bitmap can't render in a terminal; the alt text stands
                // in for it, distinctly styled (and itself inline-parsed so
                // emphasis inside alt text still works).
                let inner = lex_inline(alt);
                render_toks(&inner, base.patch(theme.image), theme, out);
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
/// so the document breathes the way a rendered page does. A *tight* list
/// passes `spacing` off so a nested list sits directly under its parent; a
/// *loose* list (parsed per CommonMark) passes it on and additionally emits a
/// spacer row between items.
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
            MdBlock::Code { lines, lang } => {
                // The fenced info string's language, shown as a dim caption.
                if !lang.is_empty() {
                    let mut cap: String = lang.chars().take(width).collect();
                    while cap.chars().count() < width {
                        cap.push(' ');
                    }
                    out.push(
                        Line::from(Span::styled(cap, theme.code.patch(theme.code_lang)))
                            .style(theme.code),
                    );
                }
                // Generic, deterministic syntax accents under the code
                // background; one stateful pass so `/* … */` block comments
                // spanning lines stay one comment.
                for mut spans in highlight_block(lines, theme) {
                    let drawn: usize = spans.iter().map(|s| s.content.chars().count()).sum();
                    if drawn < width {
                        spans.push(Span::styled(" ".repeat(width - drawn), theme.code));
                    }
                    out.push(Line::from(clip_spans(spans, width)).style(theme.code));
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
                loose,
            } => {
                for (n, item) in items.iter().enumerate() {
                    // A loose list breathes: a blank spacer row between items.
                    if *loose && n > 0 {
                        out.push(Line::default());
                    }
                    let label = if *ordered {
                        format!("{}. ", start + n as u64)
                    } else {
                        "• ".to_owned()
                    };
                    let pad = " ".repeat(label.chars().count());
                    let mut sub = Vec::new();
                    // A list item's blocks are laid out tight internally; the
                    // list's own looseness adds the inter-item spacer above.
                    layout_blocks(
                        item,
                        width.saturating_sub(label.chars().count()),
                        theme,
                        *loose,
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

/// A common cross-language keyword core for the code-block highlight — a
/// reading aid, not a parser, so a word that is a keyword in *some* mainstream
/// language is accented. Sorted for `binary_search`.
const CODE_KEYWORDS: &[&str] = &[
    "abstract",
    "and",
    "as",
    "async",
    "await",
    "begin",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "crate",
    "def",
    "default",
    "delete",
    "do",
    "double",
    "dyn",
    "else",
    "end",
    "enum",
    "export",
    "extends",
    "extern",
    "false",
    "final",
    "finally",
    "float",
    "fn",
    "for",
    "from",
    "function",
    "if",
    "impl",
    "implements",
    "import",
    "in",
    "int",
    "interface",
    "is",
    "lambda",
    "let",
    "match",
    "mod",
    "move",
    "mut",
    "new",
    "nil",
    "none",
    "not",
    "null",
    "or",
    "override",
    "package",
    "private",
    "protected",
    "pub",
    "public",
    "ref",
    "return",
    "self",
    "static",
    "string",
    "struct",
    "super",
    "switch",
    "then",
    "this",
    "throw",
    "trait",
    "true",
    "try",
    "type",
    "typeof",
    "unsafe",
    "use",
    "var",
    "virtual",
    "void",
    "where",
    "while",
    "yield",
];

/// Stateful generic syntax highlight for a fenced/indented code block: one
/// left-to-right pass per line carrying `/* … */` block-comment state across
/// lines. Strings (`"`/`'`/`` ` `` with `\` escapes), numbers, `//` and
/// leading-`#` line comments, block comments, and the [`CODE_KEYWORDS`] core
/// are accented over [`MarkdownTheme::code`]; everything else stays base.
fn highlight_block(lines: &[String], theme: &MarkdownTheme) -> Vec<Vec<Span<'static>>> {
    let base = theme.code;
    let kw = base.patch(theme.code_keyword);
    let st = base.patch(theme.code_string);
    let num = base.patch(theme.code_number);
    let com = base.patch(theme.code_comment);
    let mut in_block = false;
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let chars: Vec<char> = line.chars().collect();
        let n = chars.len();
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut buf = String::new();
        let mut style = base;
        let push = |s: &mut Vec<Span<'static>>, buf: &mut String, st: Style| {
            if !buf.is_empty() {
                s.push(Span::styled(std::mem::take(buf), st));
            }
        };
        let mut i = 0;
        while i < n {
            if in_block {
                let cstart = i;
                while i < n && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                let end = if i < n { i + 2 } else { i };
                let seg: String = chars[cstart..end.min(n)].iter().collect();
                spans.push(Span::styled(seg, com));
                if i < n {
                    in_block = false;
                }
                i = end;
                continue;
            }
            let c = chars[i];
            if c == '/' && chars.get(i + 1) == Some(&'/') {
                push(&mut spans, &mut buf, style);
                spans.push(Span::styled(chars[i..].iter().collect::<String>(), com));
                break;
            }
            if c == '/' && chars.get(i + 1) == Some(&'*') {
                push(&mut spans, &mut buf, style);
                in_block = true;
                i += 2;
                let cstart = i;
                while i < n && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                let end = if i < n { i + 2 } else { i };
                let mut seg = String::from("/*");
                seg.extend(chars[cstart..end.min(n)].iter());
                spans.push(Span::styled(seg, com));
                if i < n {
                    in_block = false;
                }
                i = end;
                continue;
            }
            if c == '#' && buf.trim().is_empty() && spans.is_empty() {
                push(&mut spans, &mut buf, style);
                spans.push(Span::styled(chars[i..].iter().collect::<String>(), com));
                break;
            }
            if c == '"' || c == '\'' || c == '`' {
                push(&mut spans, &mut buf, style);
                let mut sbuf = String::from(c);
                i += 1;
                while i < n {
                    sbuf.push(chars[i]);
                    if chars[i] == '\\' && i + 1 < n {
                        sbuf.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    if chars[i] == c {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                spans.push(Span::styled(sbuf, st));
                continue;
            }
            if c.is_ascii_digit() {
                // A bare digit reached here is a number start: an identifier
                // like `a1` was already consumed whole by the word branch.
                push(&mut spans, &mut buf, style);
                let s0 = i;
                while i < n
                    && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
                {
                    i += 1;
                }
                spans.push(Span::styled(chars[s0..i].iter().collect::<String>(), num));
                continue;
            }
            if c.is_ascii_alphabetic() || c == '_' {
                let s0 = i;
                while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[s0..i].iter().collect();
                let is_kw = CODE_KEYWORDS
                    .binary_search(&word.to_ascii_lowercase().as_str())
                    .is_ok();
                push(&mut spans, &mut buf, style);
                spans.push(Span::styled(word, if is_kw { kw } else { base }));
                style = base;
                continue;
            }
            buf.push(c);
            i += 1;
        }
        push(&mut spans, &mut buf, style);
        out.push(spans);
    }
    out
}

/// Truncates `spans` to at most `width` columns, dropping/clipping the span
/// that crosses the edge.
fn clip_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let mut out = Vec::with_capacity(spans.len());
    let mut used = 0;
    for s in spans {
        if used >= width {
            break;
        }
        let w = s.content.chars().count();
        if used + w <= width {
            used += w;
            out.push(s);
        } else {
            let take = width - used;
            let clipped: String = s.content.chars().take(take).collect();
            out.push(Span::styled(clipped, s.style));
            used = width;
        }
    }
    out
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
/// grouping equal styles, so emphasis runs survive a line break. A `\n` cell
/// (an HTML `<br>` hard break) ends the current visual row outright; an empty
/// segment between two breaks still occupies a row, so `<br><br>` leaves a
/// blank line just as a blank source line does.
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
    let mut start = 0;
    for idx in 0..=cells.len() {
        if idx == cells.len() || cells[idx].0 == '\n' {
            wrap_segment(&cells[start..idx], avail, prefix, out);
            start = idx + 1;
        }
    }
}

/// Word-wraps one hard-break-free cell run, appending its rows. An empty run
/// still yields one (prefix-only) row.
fn wrap_segment(
    cells: &[(char, Style)],
    avail: usize,
    prefix: &[Span<'static>],
    out: &mut Vec<Line<'static>>,
) {
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
        // A bare fence (no language) has no caption; glyphs are verbatim
        // (syntax highlight is colour-only) and the row is width-padded.
        let out = lines(Markdown::new("```\nlet x=*y*;\n```"), 12, 1);
        assert_eq!(out, "let x=*y*;  \n");
        // A language fence shows a dim caption row, then the code; the code
        // row's trailing pad still carries the code background.
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 2));
        Markdown::new("```rust\nfn f(){}\n```").render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'r'); // "rust"
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'f'); // code
        assert_eq!(buf.get(Position::new(11, 1)).unwrap().fg, Color::Yellow);
        // `fn` is highlighted as a keyword (colour-only, glyph unchanged).
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().fg, Color::Blue);
    }

    #[test]
    fn fenced_code_syntax_highlight_accents_tokens_deterministically() {
        // Caption row + a line with a keyword, number, string, comment.
        let src = "```js\nlet n = 42 + \"hi\"; // note\n```";
        let mut buf = Buffer::empty(Rect::new(0, 0, 32, 2));
        Markdown::new(src).render(buf.area(), &mut buf);
        let row1: String = (0..32)
            .map(|x| buf.get(Position::new(x, 1)).unwrap().symbol)
            .collect();
        // Glyphs are verbatim (highlight is colour-only).
        assert!(row1.starts_with("let n = 42 + \"hi\"; // note"));
        let fg = |x| buf.get(Position::new(x, 1)).unwrap().fg;
        assert_eq!(fg(0), Color::Blue); // `let` keyword
        assert_eq!(fg(8), Color::Magenta); // `42` number
        assert_eq!(fg(13), Color::Green); // string `"`
        assert_eq!(fg(19), Color::DarkGray); // `//` comment
        // The caption row shows the language.
        let cap: String = (0..2)
            .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
            .collect();
        assert_eq!(cap, "js");
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
    fn link_renders_only_the_label_and_registers_the_href() {
        let md = Markdown::new("see [the docs](https://example.com/d) now");
        assert_eq!(
            md.links(),
            vec![Link::new("the docs", "https://example.com/d")]
        );
        // The href never reaches the glyphs — only the label is drawn.
        assert_eq!(
            lines(Markdown::new("[the docs](https://example.com/d)"), 8, 1),
            "the docs\n"
        );
        // The label carries the link style (underlined cyan).
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        Markdown::new("[the docs](x)").render(buf.area(), &mut buf);
        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.symbol, 't');
        assert_eq!(cell.fg, Color::Cyan);
        assert!(cell.modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn autolink_url_and_email_register_targets() {
        assert_eq!(
            Markdown::new("ping <https://a.test/x>").links(),
            vec![Link::new("https://a.test/x", "https://a.test/x")]
        );
        // A bare email autolink gets a `mailto:` href, label unchanged.
        assert_eq!(
            Markdown::new("mail <her@a.test>").links(),
            vec![Link::new("her@a.test", "mailto:her@a.test")]
        );
    }

    #[test]
    fn link_label_keeps_inline_formatting_but_registry_is_plain() {
        let md = Markdown::new("[**bold** word](u)");
        // Registry label is the rendered plain text (markup stripped).
        assert_eq!(md.links(), vec![Link::new("bold word", "u")]);
        // …while the rendered label keeps the bold run.
        let spans = parse_inline("[**bold** word](u)");
        assert!(
            spans.iter().any(
                |s| s.content.contains("bold") && s.style.add_modifier.contains(Modifier::BOLD)
            )
        );
    }

    #[test]
    fn an_escaped_bracket_is_not_a_link() {
        let md = Markdown::new(r"\[not a link](x)");
        assert!(md.links().is_empty());
        // Width wide enough that the (unwrapped) literal stays on one row.
        assert_eq!(lines(md, 16, 1), "[not a link](x) \n");
    }

    #[test]
    fn links_are_collected_in_reading_order_across_blocks() {
        let src = "# [h](1)\n\npara [p](2)\n\n- [l](3)\n\n| [t](4) |\n| --- |\n| x |";
        assert_eq!(
            Markdown::new(src).links(),
            vec![
                Link::new("h", "1"),
                Link::new("p", "2"),
                Link::new("l", "3"),
                Link::new("t", "4"),
            ]
        );
        assert!(Markdown::new("no links here").links().is_empty());
    }

    #[test]
    fn focused_link_gets_the_selection_style_on_only_that_link() {
        let src = "[a](1) and [b](2)";
        let mut buf = Buffer::empty(Rect::new(0, 0, 9, 1));
        Markdown::new(src)
            .focused_link(1)
            .render(buf.area(), &mut buf);
        let a = buf.get(Position::new(0, 0)).unwrap(); // link 0: "a"
        let b = buf.get(Position::new(6, 0)).unwrap(); // link 1: "b" (focused)
        assert_eq!((a.symbol, b.symbol), ('a', 'b'));
        // Both are links (underlined); only the focused one is reversed.
        assert!(a.modifier.contains(Modifier::UNDERLINED));
        assert!(!a.modifier.contains(Modifier::REVERSED));
        assert!(b.modifier.contains(Modifier::UNDERLINED));
        assert!(b.modifier.contains(Modifier::REVERSED));
        // Focus is purely visual: the registry is unchanged.
        assert_eq!(
            Markdown::new(src).focused_link(1).links(),
            vec![Link::new("a", "1"), Link::new("b", "2")]
        );
    }

    #[test]
    fn an_out_of_range_focused_link_highlights_nothing() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        // Index 9 with one link: no panic, no reversed cell.
        Markdown::new("[a](1)")
            .focused_link(9)
            .render(buf.area(), &mut buf);
        assert!((0..3).all(|x| {
            !buf.get(Position::new(x, 0))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        }));
    }

    #[test]
    fn focus_index_is_the_document_registry_order() {
        // Link 2 ("third") spans two blocks of preceding links; focusing it
        // must land on "third", proving the index is the global reading order.
        let src = "[one](1) [two](2)\n\nthen [three](3)";
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 3));
        Markdown::new(src)
            .focused_link(2)
            .render(buf.area(), &mut buf);
        // Row 0 "one two", row 1 blank spacer, row 2 "then three".
        let t = buf.get(Position::new(5, 2)).unwrap();
        assert_eq!(t.symbol, 't'); // "three" starts at col 5 of "then three"
        assert!(t.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn link_regions_locate_every_label_in_screen_space() {
        // "a and bb" → 'a'@0 (link 0), " and " @1..6, "bb"@6..8 (link 1).
        let area = Rect::new(0, 0, 12, 1);
        let regions = Markdown::new("[a](1) and [bb](2)").link_regions(area);
        assert_eq!(
            regions,
            vec![
                LinkRegion {
                    index: 0,
                    rect: Rect::new(0, 0, 1, 1)
                },
                LinkRegion {
                    index: 1,
                    rect: Rect::new(6, 0, 2, 1)
                },
            ]
        );
    }

    #[test]
    fn link_at_maps_a_click_to_its_link_and_misses_plain_text() {
        let md = Markdown::new("[a](1) and [bb](2)");
        let area = Rect::new(0, 0, 12, 1);
        assert_eq!(md.link_at(Position::new(0, 0), area), Some(0));
        assert_eq!(md.link_at(Position::new(7, 0), area), Some(1)); // 2nd 'b'
        assert_eq!(md.link_at(Position::new(3, 0), area), None); // inside "and"
        // The reducer's one-liner: click → LinkActivation.
        let act = md
            .link_at(Position::new(6, 0), area)
            .and_then(|i| md.links().get(i).map(|l| l.activate(i)));
        assert_eq!(act, Some(md.links()[1].activate(1)));
    }

    #[test]
    fn link_activation_at_resolves_in_one_call_with_no_desync() {
        let md = Markdown::new("[a](1) and [bb](2)");
        let area = Rect::new(0, 0, 12, 1);

        // One call resolves a hit straight to (index, href).
        assert_eq!(
            md.link_activation_at(Position::new(0, 0), area),
            Some(md.links()[0].activate(0))
        );
        assert_eq!(
            md.link_activation_at(Position::new(7, 0), area),
            Some(md.links()[1].activate(1))
        );
        // Plain text and a zero area yield nothing (total).
        assert_eq!(md.link_activation_at(Position::new(3, 0), area), None);
        assert_eq!(
            md.link_activation_at(Position::new(0, 0), Rect::new(0, 0, 0, 0)),
            None
        );

        // The no-desync contract: link_activation_at is exactly the manual
        // link_at + links()[i] pattern, and href always matches its index.
        for x in 0..12u16 {
            let p = Position::new(x, 0);
            let manual = md
                .link_at(p, area)
                .and_then(|i| md.links().get(i).map(|l| l.activate(i)));
            assert_eq!(md.link_activation_at(p, area), manual, "x={x}");
            if let Some(act) = md.link_activation_at(p, area) {
                assert_eq!(act.href, md.links()[act.index].href, "href tracks index");
            }
        }
    }

    #[test]
    fn link_regions_are_in_screen_coords_through_a_block() {
        // A bordered block offsets content to (1,1); regions must reflect that.
        let md = Markdown::new("[x](u)").block(Block::bordered());
        let area = Rect::new(0, 0, 8, 3);
        assert_eq!(
            md.link_regions(area),
            vec![LinkRegion {
                index: 0,
                rect: Rect::new(1, 1, 1, 1)
            }]
        );
        assert_eq!(md.link_at(Position::new(1, 1), area), Some(0));
        assert_eq!(md.link_at(Position::new(0, 0), area), None); // the border
    }

    #[test]
    fn a_soft_wrapped_link_keeps_one_index_across_its_rows() {
        // Label wraps to two rows; both regions carry the same registry index.
        let md = Markdown::new("[one two three](u)");
        let regions = md.link_regions(Rect::new(0, 0, 7, 3));
        assert!(regions.len() >= 2);
        assert!(regions.iter().all(|r| r.index == 0));
        assert!(regions.iter().any(|r| r.rect.y == 0) && regions.iter().any(|r| r.rect.y == 1));
    }

    #[test]
    fn no_links_or_zero_area_yields_no_regions() {
        assert!(
            Markdown::new("plain text")
                .link_regions(Rect::new(0, 0, 10, 1))
                .is_empty()
        );
        assert!(
            Markdown::new("[a](b)")
                .link_regions(Rect::new(0, 0, 0, 0))
                .is_empty()
        );
        assert_eq!(
            Markdown::new("[a](b)").link_at(Position::new(0, 0), Rect::new(0, 0, 0, 0)),
            None
        );
    }

    #[test]
    fn image_renders_styled_alt_text_and_is_not_a_link() {
        let md = Markdown::new("see ![a cat](cat.png) here");
        // An image is not activatable, so it never enters the link registry.
        assert!(md.links().is_empty());
        // The alt text stands in for the bitmap, styled with theme.image; the
        // src never reaches the glyphs.
        let out = lines(Markdown::new("![a cat](cat.png)"), 8, 1);
        assert_eq!(out, "a cat   \n");
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        Markdown::new("![a cat](cat.png)").render(buf.area(), &mut buf);
        let cell = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(cell.symbol, 'a');
        assert_eq!(cell.fg, Color::Magenta);
        assert!(cell.modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn indented_code_block_is_verbatim_and_strips_four_spaces() {
        let blocks = parse_blocks("    let x = *y*;");
        assert_eq!(
            blocks,
            vec![MdBlock::Code {
                lines: vec!["let x = *y*;".to_owned()],
                lang: String::new(),
            }]
        );
        // Verbatim: the `*y*` is NOT italicised, and 4 spaces were stripped.
        assert_eq!(
            lines(Markdown::new("    fn f() {}"), 14, 1),
            "fn f() {}     \n"
        );
    }

    #[test]
    fn an_indented_line_after_paragraph_text_stays_in_the_paragraph() {
        // CommonMark: indented code cannot interrupt a paragraph.
        let blocks = parse_blocks("a paragraph\n    still the same paragraph");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0], MdBlock::Paragraph(_)));
    }

    #[test]
    fn setext_underlines_make_headings() {
        assert_eq!(
            parse_blocks("Big Title\n==="),
            vec![MdBlock::Heading {
                level: 1,
                spans: vec![Span::raw("Big Title")]
            }]
        );
        assert_eq!(
            parse_blocks("A Sub\n-----"),
            vec![MdBlock::Heading {
                level: 2,
                spans: vec![Span::raw("A Sub")]
            }]
        );
    }

    #[test]
    fn a_dash_underline_after_text_beats_the_thematic_break() {
        // `para` then `---` is a setext H2, not a paragraph + rule…
        assert_eq!(
            parse_blocks("para\n---"),
            vec![MdBlock::Heading {
                level: 2,
                spans: vec![Span::raw("para")]
            }]
        );
        // …but a standalone `---` (no preceding paragraph) is still a rule.
        assert_eq!(parse_blocks("---"), vec![MdBlock::Rule]);
        // And a blank line between text and `---` breaks the setext link.
        assert_eq!(
            parse_blocks("para\n\n---"),
            vec![MdBlock::Paragraph(vec![Span::raw("para")]), MdBlock::Rule]
        );
    }

    #[test]
    fn html_entities_are_decoded_named_and_numeric() {
        let s = parse_inline("a &amp; b &lt;x&gt; &#65; &#x42; &copy;");
        assert_eq!(span_text(&s), "a & b <x> A B ©");
    }

    #[test]
    fn html_comments_are_removed_and_unknown_tags_stripped() {
        assert_eq!(lines(Markdown::new("x<!-- hide -->y"), 3, 1), "xy \n");
        assert_eq!(
            lines(Markdown::new("<span class=\"k\">hi</span>"), 4, 1),
            "hi  \n"
        );
    }

    #[test]
    fn html_formatting_tags_map_to_markdown_styling() {
        let s = parse_inline("<b>B</b> <i>I</i> <code>c*d*</code>");
        assert!(
            s.iter()
                .any(|x| x.content == "B" && x.style.add_modifier.contains(Modifier::BOLD))
        );
        assert!(
            s.iter()
                .any(|x| x.content == "I" && x.style.add_modifier.contains(Modifier::ITALIC))
        );
        // <code> is a literal code span: the inner *d* is not italicised.
        let code = s
            .iter()
            .find(|x| x.style.fg == Some(Color::Yellow))
            .unwrap();
        assert_eq!(code.content, "c*d*");
    }

    #[test]
    fn html_br_is_a_hard_line_break_and_code_keeps_entities_literal() {
        assert_eq!(lines(Markdown::new("a<br>b"), 3, 2), "a  \nb  \n");
        // No entity decoding inside a code span.
        let s = parse_inline("`&amp;`");
        assert_eq!(s[0].content, "&amp;");
    }

    #[test]
    fn reference_links_full_collapsed_and_shortcut_resolve() {
        // Full, collapsed, and shortcut forms all resolve to the same link.
        for src in [
            "[t][r]\n\n[r]: http://e.com",
            "[t][]\n\n[t]: http://e.com",
            "[t]\n\n[t]: http://e.com",
        ] {
            assert_eq!(
                Markdown::new(src).links(),
                vec![Link::new("t", "http://e.com")],
                "src = {src:?}"
            );
        }
        // Label matching is case-insensitive and whitespace-collapsed.
        assert_eq!(
            Markdown::new("[Click Here][My  Ref]\n\n[my ref]: u").links(),
            vec![Link::new("Click Here", "u")]
        );
    }

    #[test]
    fn an_unresolved_reference_renders_literally_and_is_no_link() {
        let md = Markdown::new("see [text][missing] end");
        assert!(md.links().is_empty());
        assert_eq!(lines(md, 20, 1), "see [text][missing] \n");
    }

    #[test]
    fn reference_image_resolves_and_definition_is_not_rendered() {
        let md = Markdown::new("![a logo][l]\n\n[l]: logo.png");
        // An image is not a link…
        assert!(md.links().is_empty());
        // …its alt stands in (styled), and the definition line renders nothing.
        let out = lines(md, 7, 1);
        assert_eq!(out, "a logo \n");
    }

    #[test]
    fn a_definition_inside_a_code_fence_is_not_a_definition() {
        // The fenced `[x]: y` is code, so `[x]` stays an unresolved literal.
        let md = Markdown::new("```\n[x]: http://e.com\n```\n\n[x]");
        assert!(md.links().is_empty());
    }

    #[test]
    fn loose_lists_get_inter_item_spacers_tight_lists_do_not() {
        // Tight: no blank line between items → compact.
        assert_eq!(lines(Markdown::new("- a\n- b"), 4, 2), "• a \n• b \n");
        // Loose: a blank line between items → a spacer row between them.
        assert_eq!(
            lines(Markdown::new("- a\n\n- b"), 4, 3),
            "• a \n    \n• b \n"
        );
    }

    #[test]
    fn a_multi_block_item_makes_the_list_loose() {
        let blocks = parse_blocks("- one\n\n  still one\n- two");
        match &blocks[0] {
            MdBlock::List { loose, items, .. } => {
                assert!(*loose);
                assert_eq!(items.len(), 2);
            }
            other => panic!("expected list, got {other:?}"),
        }
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
