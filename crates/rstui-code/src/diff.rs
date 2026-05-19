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
//! `@@ … @@`, then a one-char body sign), and the two real algorithms — the
//! word-level intra-line diff (a textbook LCS over tokens) and the optional
//! generic syntax tokenizer (a small character classifier over string,
//! number, comment and keyword runs) — are the same kind of hand-written
//! scanning [`Markdown`](rstui_widgets::Markdown)'s parser uses rather than pulling a
//! CommonMark crate. So `Diff` is a plain [`Widget`]
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
//! - two layouts via [`Diff::layout`] / [`Diff::side_by_side`]: the default
//!   [`DiffLayout::Unified`] (one column, `±` sign in the gutter) and an
//!   opt-in [`DiffLayout::Split`] side-by-side view — old/deletions on the
//!   left, new/additions on the right, each with its own line-number gutter,
//!   a thin `│` separator between them, a change group pairing deletion *i*
//!   with addition *i* on one screen row (the shorter side padded with blank
//!   themed cells so rows stay aligned), context echoed on both sides, and
//!   the same intra-line word highlight on the paired changed lines; file and
//!   hunk headers span the full width in either layout
//! - opt-in **generic syntax highlighting** of the code itself via
//!   [`Diff::syntax`] (default off): a dependency-free, language-agnostic,
//!   deterministic tokenizer tints string literals (`"…"`, `'…'`,
//!   `` `…` ``), numbers, line comments (`//`, `#`, `--`) and `/* … */`
//!   block comments, and a curated common-keyword set (`fn`/`let`/`if`/…).
//!   It is layered *under* the add/del row background and the intra-line
//!   word highlight, so a changed word still wins
//! - **combined merge diffs**: an `@@@ -a,b -c,d +e,f @@@` hunk header (one
//!   `@` and one range per parent) and the matching N-column body sign
//!   prefixes, rendered with an N-wide sign gutter
//! - **`git` metadata rows** — `rename from`/`rename to`,
//!   `copy from`/`copy to`, `old mode`/`new mode`, `similarity index N%`,
//!   `index <oid>..<oid>[ <mode>]`, `new file mode`/`deleted file mode` —
//!   parsed and rendered as themed header rows (shown, never dropped)
//! - **binary patches**: a `Binary files a/x and b/y differ` line or a
//!   `GIT binary patch` block renders as a clear themed "binary file
//!   changed" row instead of being silently dropped
//!
//! Rendering is deterministic and width-aware: the same patch and area always
//! produce the same cells, so output is snapshot-testable through
//! [`Buffer`] exactly like every other widget. Malformed
//! input never panics — an unparseable line renders best-effort as context.
//! A [`DiffLayout::Split`] area too narrow to seat both columns (each a
//! one-digit gutter, one content column, and the separator) degrades to the
//! unified layout rather than panicking or rendering an unreadable sliver.
//!
//! # Example
//!
//! ```
//! use rstui_code::Diff;
//! use rstui_core::{Buffer, Position, Rect, Widget};
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
use std::collections::HashMap;

use crate::changeset::Changeset;
use crate::syntax::{self, Language, LexState, SyntaxStyles};
use crate::treesitter::{Analyzer, TsLanguage};
use rstui_core::{Buffer, Color, Line, Modifier, Rect, Span, Style, Widget};
use rstui_widgets::Block;

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
    /// A themed `git` metadata row (`rename`/`copy`/`mode`/`similarity`/
    /// `index`/`new file`/`deleted file`). Distinct from [`file`](Self::file)
    /// so the path header still stands out above its metadata.
    pub meta: Style,
    /// A "binary file changed" row (a `Binary files … differ` line or a
    /// `GIT binary patch` block). Loud enough not to be missed.
    pub binary: Style,
    /// A string literal (`"…"`, `'…'`, `` `…` ``) when [`Diff::syntax`] is on.
    /// Layered *under* the row add/del background and the word highlight.
    pub syntax_string: Style,
    /// A numeric literal when [`Diff::syntax`] is on. Same under-layering.
    pub syntax_number: Style,
    /// A `//`/`#`/`--` line comment or a `/* … */` block comment when
    /// [`Diff::syntax`] is on. Same under-layering.
    pub syntax_comment: Style,
    /// A common-keyword token (`fn`/`let`/`if`/…) when [`Diff::syntax`] is on.
    /// Same under-layering.
    pub syntax_keyword: Style,
    /// A function / method / macro / constructor name (a *Tier-1* semantic
    /// class — only produced when [`Diff::tree_sitter`] is on; the Tier-0
    /// dependency-free lexer never emits it). Same under-layering as the
    /// legacy four; see [ADR 0024](https://github.com/andymac4182/rstui/blob/main/docs/adr/0024-code-widget-crate-and-treesitter-exemption.md).
    pub syntax_function: Style,
    /// A type / class / enum / trait / builtin-type name (Tier-1 only — see
    /// [`syntax_function`](Self::syntax_function)). Same under-layering.
    pub syntax_type: Style,
    /// A named constant or enum variant (Tier-1 only). Same under-layering.
    pub syntax_constant: Style,
    /// An identifier / parameter / field / property (Tier-1 only). Defaults
    /// to no colour so plain variables keep the row foreground, matching the
    /// editor's Tier-1 palette. Same under-layering.
    pub syntax_variable: Style,
    /// An operator (Tier-1 only). Same under-layering.
    pub syntax_operator: Style,
    /// A bracket / delimiter / punctuation glyph (Tier-1 only). Same
    /// under-layering.
    pub syntax_punctuation: Style,
    /// An attribute / decorator / annotation (Tier-1 only). Same
    /// under-layering.
    pub syntax_attribute: Style,
    /// A module / namespace (Tier-1 only). Same under-layering.
    pub syntax_namespace: Style,
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
            meta: Style::new().fg(Color::Magenta).add_modifier(Modifier::DIM),
            binary: Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            syntax_string: Style::new().fg(Color::Green),
            syntax_number: Style::new().fg(Color::Magenta),
            syntax_comment: Style::new()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
            syntax_keyword: Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD),
            // Tier-1-only semantic classes (only emitted when
            // `Diff::tree_sitter` is on). Distinct, non-empty defaults in the
            // spirit of the existing palette; `syntax_variable` stays unset
            // so plain identifiers keep the row foreground (the editor's
            // Tier-1 default too).
            syntax_function: Style::new().fg(Color::Cyan),
            syntax_type: Style::new().fg(Color::Yellow),
            syntax_constant: Style::new().fg(Color::Magenta),
            syntax_variable: Style::new(),
            syntax_operator: Style::new().fg(Color::DarkGray),
            syntax_punctuation: Style::new().fg(Color::DarkGray),
            syntax_attribute: Style::new().fg(Color::Blue),
            syntax_namespace: Style::new().fg(Color::Cyan),
        }
    }
}

/// How [`Diff`] arranges a hunk's body lines on screen.
///
/// The header rows (file, hunk, the `\ No newline` marker) always span the
/// full width; this only governs the body. The default is [`Unified`].
///
/// [`Unified`]: DiffLayout::Unified
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffLayout {
    /// One column: every body line on its own row, the `+`/`-`/` ` sign in
    /// the gutter — the classic `git diff` reading order. The default.
    #[default]
    Unified,
    /// Side by side: old/deletions in a left column, new/additions in a
    /// right column, each with its own line-number gutter and a thin `│`
    /// separator between them. Within a change group deletion *i* shares a
    /// screen row with addition *i*; the shorter side is padded with blank
    /// themed cells so the columns stay aligned. Context lines appear on both
    /// sides. An area too narrow for two gutters, two content columns, and
    /// the separator falls back to [`Unified`](DiffLayout::Unified).
    Split,
}

/// A read-only unified-diff view: parses its source once at render time and
/// draws the supported subset into the area, width-aware and deterministic.
///
/// The source is a [`Cow<str>`](std::borrow::Cow) (a literal borrows, a
/// `String` is owned). Parsing produces owned display lines, so the rendered
/// spans are independent of the source lifetime. An optional framing
/// [`Block`], a base [`Style`] that also fills the content area, a vertical
/// scroll offset, a [`DiffLayout`] (unified or side-by-side), and a
/// [`DiffTheme`] are the only knobs — everything else is derived from the
/// patch.
///
/// # Example
///
/// ```
/// use rstui_code::Diff;
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Block;
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
    scroll: usize,
    col: usize,
    theme: DiffTheme,
    layout: DiffLayout,
    syntax: bool,
    tree_sitter: bool,
    language: Language,
    tab_width: usize,
    min_number_width: usize,
}

impl<'a> Diff<'a> {
    /// A diff view of `source` with the default theme, no block, no scroll.
    pub fn new(source: impl Into<Cow<'a, str>>) -> Self {
        Self {
            source: source.into(),
            block: None,
            style: Style::new(),
            scroll: 0,
            col: 0,
            theme: DiffTheme::default(),
            layout: DiffLayout::default(),
            syntax: false,
            tree_sitter: false,
            language: Language::Unknown,
            tab_width: 4,
            min_number_width: 0,
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
    ///
    /// `offset` is a [`usize`] (not the historical `u16`): a generated patch
    /// can exceed 65 535 rows, and the caller clamps with [`row_count`] —
    /// see its docs for the scroll-clamp recipe (gap K of the
    /// `code-editor-and-diff-deep-dive`). An `offset` past the last row simply
    /// renders an empty area, never panics.
    ///
    /// [`row_count`]: Diff::row_count
    #[must_use]
    pub fn scroll(mut self, offset: usize) -> Self {
        self.scroll = offset;
        self
    }

    /// Sets the first body **content** column drawn — the horizontal scroll
    /// position so an over-wide line can be panned with `←`/`→` instead of
    /// being hard-clipped at the right edge (gap B of the
    /// `code-editor-and-diff-deep-dive`).
    ///
    /// Only the *code content* scrolls: the line-number / sign gutter (and a
    /// full-width file / hunk / metadata / binary / `\ No newline` row) is
    /// **never** shifted, so the gutter stays put while the code slides under
    /// it — exactly the behaviour a reader expects from a horizontal pan.
    /// Tabs are expanded (see [`tab_width`]) *before* this offset is applied,
    /// so a column is a rendered cell, not a source byte. `0` (the default)
    /// is byte-identical to the historical render.
    ///
    /// [`tab_width`]: Diff::tab_width
    #[must_use]
    pub fn col(mut self, off: usize) -> Self {
        self.col = off;
        self
    }

    /// Selects the [`Language`] the syntax overlay lexes when
    /// [`syntax`](Diff::syntax) is on.
    ///
    /// The default is [`Language::Unknown`] — the language-blind common-core
    /// mode that is **byte-identical** to the historical built-in tinter, so
    /// `.syntax(true)` on its own renders exactly as it always did. Pick a
    /// concrete language (the app resolves it from the file path via
    /// [`Language::from_path`] over the `git` `Cmd` seam — the widget stays
    /// pure) for that language's own keyword set and string / comment
    /// delimiters. A diff row is a single, non-contiguous line, so multi-line
    /// constructs are *not* carried between rows (each row lexes from a fresh
    /// [`LexState`]); an editor, whose lines are contiguous, threads the
    /// state instead.
    ///
    /// ```
    /// use rstui_code::{Diff, Language};
    /// use rstui_core::{Buffer, Rect, Widget};
    ///
    /// // `fn` is a Rust keyword; the overlay tints it under the add colour.
    /// let mut buf = Buffer::empty(Rect::new(0, 0, 24, 2));
    /// Diff::new("@@ -0,0 +1 @@\n+fn main() {}")
    ///     .syntax(true)
    ///     .language(Language::Rust)
    ///     .render(buf.area(), &mut buf);
    /// ```
    #[must_use]
    pub fn language(mut self, lang: Language) -> Self {
        self.language = lang;
        self
    }

    /// Sets how many cells a literal tab (`\t`) in body content expands to:
    /// it advances to the next multiple of `w` columns (a real tab stop, not
    /// a fixed run), so source indentation no longer collapses to a single
    /// cell (gap D of the `code-editor-and-diff-deep-dive`). The default is
    /// **4**. Header rows (file / hunk / metadata / binary / `\ No newline`)
    /// are unaffected. A `w` of `0` is treated as `1` so a tab still occupies
    /// at least one cell (no panic, no zero-width column).
    ///
    /// Expansion happens before the horizontal [`col`](Diff::col) slice and
    /// the width clip, so columns stay correct under a horizontal pan.
    #[must_use]
    pub fn tab_width(mut self, w: usize) -> Self {
        self.tab_width = w;
        self
    }

    /// Replaces the [`DiffTheme`].
    #[must_use]
    pub fn theme(mut self, theme: DiffTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Selects the body [`DiffLayout`] (unified or side-by-side). The default
    /// is [`DiffLayout::Unified`].
    #[must_use]
    pub fn layout(mut self, layout: DiffLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Shorthand for [`layout(DiffLayout::Split)`](Diff::layout): renders the
    /// old side and the new side in two columns instead of one.
    #[must_use]
    pub fn side_by_side(self) -> Self {
        self.layout(DiffLayout::Split)
    }

    /// Toggles generic, language-agnostic syntax highlighting of the body
    /// code. **Off by default** (so a patch reads exactly as `git diff`
    /// without it, and existing snapshots are unaffected).
    ///
    /// When on, a small deterministic tokenizer tints, per character, string
    /// literals (`"…"`/`'…'`/`` `…` ``), numbers, `//`/`#`/`--` line
    /// comments, `/* … */` block comments, and a curated common-keyword set
    /// ([`syntax_keyword`](DiffTheme::syntax_keyword) and friends). The
    /// highlight is patched *under* the add/del row background and the
    /// intra-line word emphasis, so a changed word still wins visually. It is
    /// independent of [`DiffLayout`] (both layouts honour it).
    #[must_use]
    pub fn syntax(mut self, on: bool) -> Self {
        self.syntax = on;
        self
    }

    /// Opts the body code into **Tier-1** tree-sitter syntax colour — a real
    /// grammar parse of the reconstructed file text instead of the
    /// dependency-free, language-blind four-bucket Tier-0 lexer. **Off by
    /// default.**
    ///
    /// Tier-0 ([`syntax`](Diff::syntax)) is the always-present floor; Tier-1
    /// is the accuracy upgrade, exactly as in the code editor (ADR 0022 /
    /// **[ADR 0024](https://github.com/andymac4182/rstui/blob/main/docs/adr/0024-code-widget-crate-and-treesitter-exemption.md)**:
    /// `rstui-code` deps `tree-sitter` first-class, the dependency-free floor
    /// stays in `rstui-core`/`rstui-widgets`). When on, the source is parsed
    /// per file via [`Changeset`] + [`TsLanguage::from_path`] and each body
    /// line is coloured from the real parse tree's captures into the *richer*
    /// semantic classes — function / type / constant / variable / operator /
    /// punctuation / attribute / namespace
    /// ([`syntax_function`](DiffTheme::syntax_function) and friends) — not
    /// just string / number / comment / keyword.
    ///
    /// Like every overlay this is layered *under* the add/del row background
    /// and the intra-line word emphasis, so a changed word still wins, and it
    /// honours both [`DiffLayout`]s. It is **total and falls back per line**:
    /// a file whose extension no grammar matches, an unparseable region, a
    /// length mismatch, a binary/garbage patch — each such line transparently
    /// uses the Tier-0 overlay (or nothing, if [`syntax`](Diff::syntax) is
    /// off) and never panics. With `tree_sitter(false)` (the default) the
    /// render is byte-identical to the historical Tier-0 path.
    ///
    /// ```
    /// use rstui_code::Diff;
    /// use rstui_core::{Buffer, Rect, Widget};
    ///
    /// // The `+++ b/x.rs` header lets `Changeset` pick the Rust grammar.
    /// let patch = "\
    /// --- a/x.rs
    /// +++ b/x.rs
    /// @@ -0,0 +1 @@
    /// +fn main() {}
    /// ";
    /// let mut buf = Buffer::empty(Rect::new(0, 0, 32, 4));
    /// Diff::new(patch)
    ///     .syntax(true)
    ///     .tree_sitter(true)
    ///     .render(buf.area(), &mut buf);
    /// ```
    #[must_use]
    pub fn tree_sitter(mut self, on: bool) -> Self {
        self.tree_sitter = on;
        self
    }

    /// Floors the line-number column to at least `w` digits wide (default
    /// `0` = exactly the digit count of the largest line number, the
    /// historical render — **byte-identical**).
    ///
    /// Parity with [`LineNumberGutter::min_number_width`](crate::LineNumberGutter::min_number_width):
    /// an app that shows a code editor *and* this diff (e.g. `git-review`)
    /// can set the same floor on both so the gutter's left edge does not
    /// shift when switching panes. Both number columns of a side-by-side /
    /// the old+new columns of a unified diff use the floor.
    #[must_use]
    pub fn min_number_width(mut self, w: u16) -> Self {
        self.min_number_width = w as usize;
        self
    }

    /// Parses the source and lays it out to display rows for a content area
    /// `width` columns wide, honouring the active [`DiffLayout`]. Public so a
    /// host can measure a patch (its row count) for scroll math or a
    /// surrounding scrollbar without re-rendering.
    ///
    /// `width` of zero yields no rows. In [`DiffLayout::Split`] a width too
    /// narrow for two columns degrades to the unified layout (see the type
    /// docs), so the row count reflects whichever layout was actually used.
    #[must_use]
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        // The public contract is "every row" (a host measures `.len()` for
        // scroll math), so never cap here.
        self.laid_out(width, usize::MAX)
    }

    /// The number of composed rows at content `width` — the cheap accessor a
    /// host uses to **clamp [`scroll`]** instead of abusing
    /// `lines(width).len()` (which re-parses *and* re-allocates every line on
    /// every keypress; gap K of the `code-editor-and-diff-deep-dive`).
    ///
    /// The reducer that owns the scroll offset clamps it to
    /// `row_count(w).saturating_sub(viewport_h)` so content cannot scroll off
    /// into a blank pane (the deep-dive Part 2 scroll-clamp recipe). It runs
    /// the layout once (so it is the true count under the active
    /// [`DiffLayout`], including the narrow-area split→unified degrade) but
    /// allocates no rendered cells beyond the row vector — materially cheaper
    /// than `lines`, and the count is exactly `self.lines(width).len()`.
    /// `width` of `0` is `0`.
    ///
    /// [`scroll`]: Diff::scroll
    #[must_use]
    pub fn row_count(&self, width: u16) -> usize {
        // One layout pass, full (uncapped) count — the documented "every row"
        // total, identical to `lines(width).len()` but without keeping the
        // composed `Line`s around.
        self.laid_out(width, usize::MAX).len()
    }

    /// The composed rows, building at most `row_cap` of them (DIFF-1).
    /// `lines` passes `usize::MAX` (the documented full count); `render`
    /// passes `scroll + height` so off-screen rows skip the heavy layout —
    /// `out[..row_cap]` is a byte-identical prefix of the uncapped result
    /// (the gutter/marks pre-scans stay over all rows), so the visible
    /// window `render` paints is unchanged.
    fn laid_out(&self, width: u16, row_cap: usize) -> Vec<Line<'static>> {
        if width == 0 {
            return Vec::new();
        }
        let rows = parse_rows(self.source.as_ref());
        let width = width as usize;
        // Tier-1 (ADR 0024): only when opted in. The map is built once per
        // layout pass (the same `parse → layout` cadence the rest of the
        // widget uses) and is purely additive — with `tree_sitter == false`
        // it is `None` and every code path below is exactly the historical
        // Tier-0 one (gate-enforced byte-identical).
        let tier1 = if self.tree_sitter {
            Some(build_tier1_map(self.source.as_ref(), &self.theme))
        } else {
            None
        };
        let opts = RenderOpts {
            theme: &self.theme,
            syntax: self.syntax,
            language: self.language,
            col: self.col,
            // A 0 tab width would make a tab a zero-width column; clamp to 1
            // so the render path stays total.
            tab_width: self.tab_width.max(1),
            min_number_width: self.min_number_width,
            tier1: tier1.as_ref(),
        };
        match self.layout {
            DiffLayout::Unified => layout_rows(&rows, width, &opts, row_cap),
            DiffLayout::Split => layout_rows_split(&rows, width, &opts, row_cap),
        }
    }
}

// ---------------------------------------------------------------------------
// Tier-1 (tree-sitter) syntax overlay (ADR 0022 / ADR 0024)
// ---------------------------------------------------------------------------

/// One file's precomputed Tier-1 overlays: `new_no -> line styles` and
/// `old_no -> line styles`. Each `Vec<Style>` is one [`Style`] per
/// **character** of that reconstructed line's *content* (sign stripped, no
/// trailing `'\n'`) — exactly `content.chars().count()` long, so it patches
/// straight onto a [`DiffRow::Body`]'s `content` the same way the Tier-0
/// `line_overlay` slice does.
type FileTier1 = (HashMap<u32, Vec<Style>>, HashMap<u32, Vec<Style>>);

/// Per-file Tier-1 overlay map, keyed by the file's
/// [`DiffFile::path`](crate::DiffFile) (the same cleaned path a
/// [`DiffRow::File`] carries). Absent path / side / line ⇒ the caller falls
/// back to Tier-0.
type Tier1Map = HashMap<String, FileTier1>;

/// Precomputes the per-file Tier-1 (tree-sitter) overlay map for `source`.
///
/// For each file in the [`Changeset`] whose path resolves to a
/// [`TsLanguage`], the new-side text is reconstructed from the file's context
/// and addition body lines (each tagged with its running *new* line number,
/// seeded from every hunk's `new_start`); the old-side text from the context
/// and deletion lines (running *old* line number, seeded from `old_start`).
/// Each side is parsed once by an [`Analyzer`]; the flattened
/// newline-inclusive overlay is split back at the `'\n'` slots we placed and
/// each line's slice is keyed by its source line number.
///
/// **Total.** Any file whose extension matches no enabled grammar is skipped
/// (Tier-0 fallback); a parse shortfall just leaves entries absent; a
/// garbage / binary / empty patch yields an empty (or partial) map. Never
/// panics.
fn build_tier1_map(source: &str, theme: &DiffTheme) -> Tier1Map {
    let styles = syntax_styles(theme);
    let mut map: Tier1Map = HashMap::new();
    for file in Changeset::parse(source).files {
        let Some(lang) = TsLanguage::from_path(&file.path) else {
            continue;
        };
        // Reconstruct each side: the line text in source order plus the
        // line-number each text line carries (so we can key the overlay).
        let new_side = reconstruct_side(&file, Side::Right);
        let old_side = reconstruct_side(&file, Side::Left);

        let new_map = highlight_side(lang, &new_side, &styles);
        let old_map = highlight_side(lang, &old_side, &styles);
        if new_map.is_empty() && old_map.is_empty() {
            continue;
        }
        // Key by every label a `DiffRow::File` could carry for this file so
        // the layout's `current_path` (the rendered label) resolves: the raw
        // `DiffFile::path` (matches a plain modified file, old == new) *and*
        // the `file_label`-rendered form (added / deleted / renamed). A path
        // that still misses both transparently falls back to Tier-0.
        for key in file_label_keys(&file) {
            map.entry(key)
                .or_insert_with(|| (new_map.clone(), old_map.clone()));
        }
    }
    map
}

/// Every `DiffRow::File`-label string this file could be shown under, so the
/// layout's running `current_path` (which is exactly that rendered label)
/// resolves the Tier-1 entry. Mirrors [`file_label`]: a plain modified file
/// is its bare path; an added / deleted / renamed / copied file gets the
/// suffixed / `old → new` form too. Over-keying is harmless — the per-line
/// length guard in [`resolve_overlay`] still gates every actual use.
fn file_label_keys(file: &crate::changeset::DiffFile) -> Vec<String> {
    use crate::changeset::FileStatus;
    let mut keys = vec![file.path.clone()];
    let label = match file.status {
        FileStatus::Added => format!("{} (added)", file.path),
        FileStatus::Deleted => format!("{} (deleted)", file.path),
        FileStatus::Renamed | FileStatus::Copied => {
            if let Some(old) = &file.old_path {
                format!("{old} → {}", file.path)
            } else {
                file.path.clone()
            }
        }
        FileStatus::Modified | FileStatus::Binary => file.path.clone(),
    };
    if label != file.path {
        keys.push(label);
    }
    keys
}

/// The reconstructed text of one side of a file's patch: the per-line content
/// (sign stripped, `\r` already normalised by [`Changeset`]) and the source
/// line number that line occupies on that side. New side = context +
/// additions numbered from each hunk's `new_start`; old side = context +
/// deletions numbered from `old_start`.
fn reconstruct_side(file: &crate::changeset::DiffFile, side: Side) -> Vec<(u32, String)> {
    let patch_lines: Vec<&str> = file.patch().lines().collect();
    let mut out: Vec<(u32, String)> = Vec::new();
    for hunk in &file.hunks {
        // Per ADR/changeset contract `patch_lines` is 0-based end-exclusive
        // into `file.patch().lines()` and the first line is the `@@` header.
        let mut no = match side {
            Side::Right => hunk.new_start,
            Side::Left => hunk.old_start,
        };
        let range = hunk.patch_lines.clone();
        for &line in patch_lines
            .get(range.start..range.end.min(patch_lines.len()))
            .unwrap_or(&[])
            .iter()
            .skip(1)
        // skip the `@@ … @@` header line itself
        {
            // Combined `@@@` merge hunks are out of Tier-1 scope (their
            // multi-column signs make a single new/old reconstruction
            // ambiguous); treat only the ordinary single-sign body, and on
            // anything unexpected just stop this hunk (Tier-0 still covers
            // those rows).
            match line.chars().next() {
                Some(' ') => {
                    out.push((no, line[1..].to_owned()));
                    no = no.saturating_add(1);
                }
                Some('+') if matches!(side, Side::Right) => {
                    out.push((no, line[1..].to_owned()));
                    no = no.saturating_add(1);
                }
                Some('-') if matches!(side, Side::Left) => {
                    out.push((no, line[1..].to_owned()));
                    no = no.saturating_add(1);
                }
                // The other side's sign: it does not occupy a line on *this*
                // side, so the running number does not advance.
                Some('+') | Some('-') => {}
                // `\ No newline…`, an empty string, or anything unexpected:
                // not a numbered body line on this side.
                _ => {}
            }
        }
    }
    out
}

/// Parses `side`'s reconstructed text once and splits the flattened,
/// newline-inclusive overlay back into a `line_no -> Vec<Style>` map (one
/// [`Style`] per content char, the `'\n'` slots dropped). Empty input ⇒ an
/// empty map (Tier-0 fallback). Total.
fn highlight_side(
    lang: TsLanguage,
    side: &[(u32, String)],
    styles: &SyntaxStyles,
) -> HashMap<u32, Vec<Style>> {
    let mut out: HashMap<u32, Vec<Style>> = HashMap::new();
    if side.is_empty() {
        return out;
    }
    // Rows joined by `'\n'` — exactly the shape `Analyzer::set_source`
    // documents; the overlay then has one slot per char including each `'\n'`.
    let text = side
        .iter()
        .map(|(_, s)| s.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut a = Analyzer::new(lang);
    a.set_source(&text);
    let ov = a.highlight(styles);
    // The overlay is newline-inclusive and exactly `text.chars().count()`
    // long; walk it line-by-line, consuming each line's char count then the
    // single `'\n'` slot that follows (except after the last line). If the
    // length is ever short (it never is — `highlight` is total and
    // length-exact) we simply stop, leaving later lines absent (Tier-0).
    let mut pos = 0usize;
    for (idx, (no, content)) in side.iter().enumerate() {
        let n = content.chars().count();
        let Some(slice) = ov.get(pos..pos + n) else {
            break;
        };
        out.insert(*no, slice.to_vec());
        pos += n;
        if idx + 1 < side.len() {
            pos += 1; // the '\n' joiner slot
        }
    }
    out
}

/// The content-rendering knobs threaded from a [`Diff`] through the layout
/// layers to [`content_spans`] / [`side_spans`]. Bundled (rather than five
/// more positional parameters) so the cascade — row → syntax under →
/// word-mark on top — stays readable as gaps B/D/G/K are wired in.
#[derive(Clone, Copy)]
struct RenderOpts<'t> {
    /// The active [`DiffTheme`] (row, gutter, word-mark and `syntax_*`
    /// styles).
    theme: &'t DiffTheme,
    /// Generic syntax highlighting is on (gap G).
    syntax: bool,
    /// The [`Language`] the syntax overlay lexes (gap G); default
    /// [`Language::Unknown`] is byte-identical to the historical tinter.
    language: Language,
    /// First body content column drawn — the horizontal scroll (gap B);
    /// `0` is byte-identical to the historical render.
    col: usize,
    /// Cells a literal tab expands to, advancing to the next multiple (gap
    /// D); already clamped to `>= 1`.
    tab_width: usize,
    /// Minimum line-number column width (`Diff::min_number_width`); `0` =
    /// exactly the digit count (byte-identical historical render).
    min_number_width: usize,
    /// The precomputed per-file Tier-1 (tree-sitter) overlay map, when
    /// [`Diff::tree_sitter`] is on; `None` ⇒ Tier-0 only. Resolved by the
    /// layout callers (which track the current file path + each row's side /
    /// line number); a miss falls back to Tier-0.
    tier1: Option<&'t Tier1Map>,
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
        if let Some(b) = &self.block {
            b.render_ref(area, buf);
        }
        if inner.is_empty() {
            return;
        }
        buf.set_style(inner, self.style);

        // DIFF-1: only the `[scroll, scroll + height)` window is painted, so
        // build at most that many rows instead of the whole patch. The
        // produced prefix is byte-identical to the uncapped layout, so the
        // `skip(scroll).take(height)` below selects the exact same cells.
        // `scroll` is a `usize` (gap K) — a patch may exceed `u16::MAX` rows;
        // `skip` past the end just paints nothing.
        let cap = self.scroll.saturating_add(inner.height as usize);
        let rows = self.laid_out(inner.width, cap);
        for (i, mut line) in rows
            .into_iter()
            .skip(self.scroll)
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
    /// A `git` metadata line that is not a path header — `rename`/`copy`,
    /// `old`/`new mode`, `similarity index`, `index`, `new file mode`,
    /// `deleted file mode`. Rendered as a themed row, never dropped.
    Meta { text: String },
    /// A binary-patch notice: a `Binary files … differ` line or the head of
    /// a `GIT binary patch` block (its payload bytes are not displayable).
    Binary { text: String },
    /// A hunk header with its starting line numbers and optional section.
    /// `sign_cols` is 1 for an ordinary `@@` hunk and the parent count (≥ 2)
    /// for a combined `@@@ … @@@` merge hunk, sizing the body sign gutter.
    Hunk {
        old_start: u32,
        new_start: u32,
        section: String,
        sign_cols: usize,
    },
    /// A body line. `old_no`/`new_no` are the 1-based numbers that apply to
    /// this row (a deletion has no new number, an addition no old number).
    /// `signs` is the raw leading sign column(s) — one char for an ordinary
    /// hunk, `sign_cols` chars for a combined hunk — shown verbatim in the
    /// gutter so a 3-way conflict's per-parent `+`/`-` is visible.
    Body {
        kind: ChangeKind,
        old_no: Option<u32>,
        new_no: Option<u32>,
        signs: String,
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
/// hunks. Line-oriented, single pass. `git` metadata (`index`, `rename`,
/// `mode`, …) and binary-patch notices become their own themed rows rather
/// than being dropped; an unrecognised line inside a hunk renders as context
/// so no content is ever silently lost. Combined (`@@@ … @@@`) merge hunks
/// are recognised and their N-column body signs preserved verbatim.
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
    // 1 for an ordinary `@@` hunk, ≥ 2 for a combined `@@@` merge hunk.
    let mut sign_cols = 1usize;
    let mut pending_minus: Option<&str> = None; // last `--- ` awaiting its `+++`
    // A `GIT binary patch` line is followed by base85 payload chunks; skip
    // them (they are bytes, not displayable text) until the next header.
    let mut in_binary_payload = false;

    for line in lines {
        if line.starts_with("diff --git ")
            || line.starts_with("diff --cc ")
            || line.starts_with("diff --combined ")
        {
            // `diff --git a/x b/x` (or the combined-merge `diff --cc`/
            // `diff --combined`) — start a file; the `--- `/`+++ ` pair that
            // follows refines the path, so just reset hunk/binary state.
            in_hunk = false;
            in_binary_payload = false;
            pending_minus = None;
            continue;
        }

        if let Some(path) = line.strip_prefix("--- ") {
            pending_minus = Some(path);
            in_hunk = false;
            in_binary_payload = false;
            continue;
        }

        if let Some(path) = line.strip_prefix("+++ ") {
            let shown = file_label(pending_minus.unwrap_or("/dev/null"), path);
            out.push(DiffRow::File { path: shown });
            pending_minus = None;
            in_hunk = false;
            in_binary_payload = false;
            continue;
        }

        if let Some(h) = parse_hunk_header(line) {
            old_no = h.old_start;
            new_no = h.new_start;
            in_hunk = true;
            in_binary_payload = false;
            sign_cols = h.sign_cols;
            out.push(DiffRow::Hunk {
                old_start: h.old_start,
                new_start: h.new_start,
                section: h.section,
                sign_cols: h.sign_cols,
            });
            continue;
        }

        // Binary-patch notices, in either of git's two forms. The
        // `GIT binary patch` header is followed by base85 payload lines that
        // are bytes, not text — note we are in that payload and skip them
        // until the next header resets the state.
        if !in_hunk {
            if let Some(text) = binary_notice(line) {
                in_binary_payload = line == "GIT binary patch";
                out.push(DiffRow::Binary { text });
                continue;
            }
            if in_binary_payload {
                // base85 payload chunk of a `GIT binary patch` — skip.
                continue;
            }
            if let Some(text) = git_metadata(line) {
                out.push(DiffRow::Meta { text });
                continue;
            }
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
            let (kind, signs, content) = classify_body(line, sign_cols);
            // Ordinary hunk: old advances on context/deletion, new on
            // context/addition — the classic two-counter walk. Combined
            // hunk: only the merge-result line number is meaningful, so the
            // single number column tracks `new_no`, advancing for every line
            // that survives into the result (anything not a pure deletion).
            let (row_old, row_new) = if sign_cols == 1 {
                match kind {
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
                }
            } else if kind == ChangeKind::Deletion {
                (None, None)
            } else {
                let n = new_no;
                new_no += 1;
                (None, Some(n))
            };
            out.push(DiffRow::Body {
                kind,
                old_no: row_old,
                new_no: row_new,
                signs,
                content: content.to_owned(),
            });
            continue;
        }

        // Outside any hunk and not a header we recognise: best-effort drop.
        // (Every git metadata / binary form we know is handled above.)
    }

    out
}

/// Classifies one body line of a hunk into its [`ChangeKind`], the raw sign
/// column(s) to show in the gutter, and the content after them.
///
/// `sign_cols` is 1 for an ordinary hunk and the parent count for a combined
/// `@@@` merge hunk. For a combined line the kind is the *dominant* one — any
/// `+` column makes it an addition, otherwise any `-` makes it a deletion,
/// else context — so the row colour matches its net effect while the per-
/// parent signs stay visible verbatim in the gutter.
fn classify_body(line: &str, sign_cols: usize) -> (ChangeKind, String, &str) {
    if sign_cols <= 1 {
        return match line.chars().next() {
            Some('+') => (ChangeKind::Addition, "+".to_owned(), &line[1..]),
            Some('-') => (ChangeKind::Deletion, "-".to_owned(), &line[1..]),
            Some(' ') => (ChangeKind::Context, " ".to_owned(), &line[1..]),
            // An empty line inside a hunk is an empty context line.
            None => (ChangeKind::Context, " ".to_owned(), ""),
            // Anything else inside a hunk is treated as context so the text
            // is preserved rather than dropped.
            Some(_) => (ChangeKind::Context, " ".to_owned(), line),
        };
    }

    // Combined hunk: the first `sign_cols` *characters* are the per-parent
    // sign columns (each a `+`/`-`/` ` in well-formed input). Split on a char
    // boundary — a malformed line whose lead is a multi-byte char must not
    // panic — and pad a short line with spaces so the gutter stays aligned.
    let split = line
        .char_indices()
        .nth(sign_cols)
        .map_or(line.len(), |(byte, _)| byte);
    let raw = &line[..split];
    let taken = raw.chars().count();
    let mut signs = String::with_capacity(sign_cols);
    signs.push_str(raw);
    for _ in taken..sign_cols {
        signs.push(' ');
    }
    let content = &line[split..];
    let kind = if signs.contains('+') {
        ChangeKind::Addition
    } else if signs.contains('-') {
        ChangeKind::Deletion
    } else {
        ChangeKind::Context
    };
    (kind, signs, content)
}

/// Recognises a `git` metadata line (outside any hunk) that should render as
/// its own themed row rather than be dropped, returning the text to show.
///
/// Covers `old mode`/`new mode`, `deleted file mode`/`new file mode`,
/// `copy from`/`copy to`, `rename from`/`rename to`, `similarity index N%`,
/// `dissimilarity index N%`, and `index <oid>..<oid>[ <mode>]`.
fn git_metadata(line: &str) -> Option<String> {
    const PREFIXES: &[&str] = &[
        "old mode ",
        "new mode ",
        "deleted file mode ",
        "new file mode ",
        "copy from ",
        "copy to ",
        "rename from ",
        "rename to ",
        "rename old ",
        "rename new ",
        "similarity index ",
        "dissimilarity index ",
        "index ",
    ];
    if PREFIXES.iter().any(|p| line.starts_with(p)) {
        Some(line.to_owned())
    } else {
        None
    }
}

/// Recognises a binary-patch notice, returning a clear, fixed display label
/// (the raw form is terse and, for `GIT binary patch`, followed by
/// undisplayable base85 bytes the caller skips).
///
/// Handles the textual `Binary files a/x and b/y differ` (and the older
/// `Binary files differ`) line and the `GIT binary patch` block header.
fn binary_notice(line: &str) -> Option<String> {
    if line == "GIT binary patch" {
        return Some("(binary file changed)".to_owned());
    }
    let body = line.strip_prefix("Binary files ")?;
    let inner = body.strip_suffix(" differ").unwrap_or(body);
    if inner.is_empty() {
        Some("(binary file changed)".to_owned())
    } else {
        Some(format!("(binary) {inner}"))
    }
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

/// A parsed `@@ … @@` (or combined `@@@ … @@@`) hunk header.
struct HunkHeader {
    old_start: u32,
    new_start: u32,
    section: String,
    /// 1 for an ordinary hunk; the parent count (≥ 2) for a combined merge
    /// hunk, which is also the body-line sign-column width.
    sign_cols: usize,
}

/// Parses an ordinary `@@ -<l>[,<s>] +<l>[,<s>] @@[ section]` or a combined
/// merge `@@@ -<l>[,<s>] -<l>[,<s>] +<l>[,<s>] @@@[ section]` header. The
/// fence width (count of leading `@`) is `parents + 1`; a combined header
/// carries one `-` range per parent and a single `+` range. Omitted counts
/// default to 1; the section label is the free text after the closing fence.
/// Returns `None` if the shape does not match (so a malformed header renders
/// best-effort as context, never panicking).
fn parse_hunk_header(line: &str) -> Option<HunkHeader> {
    // Leading `@` run width: 2 for `@@`, 3 for `@@@`, … Each extra `@` past
    // the first two adds a parent (a combined diff). One `@` is not a hunk.
    let fence = line.bytes().take_while(|&b| b == b'@').count();
    if fence < 2 {
        return None;
    }
    let fence_str: &str = &line[..fence];
    let rest = line[fence..].strip_prefix(' ')?;
    let close_pat = format!(" {fence_str}");
    let close = rest.find(&close_pat)?;
    let ranges = &rest[..close];
    let section = rest[close + close_pat.len()..].trim().to_owned();

    // `parents` minus ranges then exactly one plus range.
    let parents = fence - 1;
    let mut parts = ranges.split(' ');
    let mut old_start = None;
    for _ in 0..parents {
        let minus = parts.next()?.strip_prefix('-')?;
        let (start, _count) = parse_range(minus)?;
        old_start.get_or_insert(start);
    }
    let plus = parts.next()?.strip_prefix('+')?;
    let (new_start, _new_count) = parse_range(plus)?;
    if parts.next().is_some() {
        return None;
    }
    Some(HunkHeader {
        old_start: old_start.unwrap_or(new_start),
        new_start,
        section,
        sign_cols: parents,
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
/// groups for the intra-line word diff, then renders every row. `opts`
/// carries the syntax/language/horizontal-scroll/tab-width knobs.
fn layout_rows(
    rows: &[DiffRow],
    width: usize,
    opts: &RenderOpts<'_>,
    row_cap: usize,
) -> Vec<Line<'static>> {
    let num_w = number_width(rows).max(opts.min_number_width);
    // The widest body sign gutter: 1 for ordinary hunks, the parent count for
    // a combined merge hunk. Sized once so every body row's content aligns.
    let sign_w = sign_width(rows);

    // Which body rows are part of a change group, paired for intra-line marks:
    // index → the changed-char mask for that row (empty = no per-word marks).
    let marks = intra_line_marks(rows);

    // DIFF-1 (the landed Paragraph PG-1 pattern): the gutter/marks pre-scans
    // above stay over *all* rows — so `num_w`/`sign_w` and every produced
    // row are byte-identical to the uncapped layout (no scroll-jitter) —
    // but `render` only ever paints `[scroll, scroll + height)`, so it
    // passes `row_cap = scroll + height` and the heavy per-row
    // wrap+syntax+alloc stops once that many output rows exist. The public
    // `Diff::lines` (a host measures the true total for scroll math) passes
    // `usize::MAX`, so its result is unchanged. `render_row(idx, …)` depends
    // only on the full-row pre-scans, so `out[..cap]` is an exact prefix.
    let mut out = Vec::with_capacity(rows.len().min(row_cap));
    // The current file path — the most recent `DiffRow::File` — selects which
    // file's Tier-1 overlay a body row reads (Tier-0 path ignores it).
    let mut current_path: Option<&str> = None;
    for (idx, row) in rows.iter().enumerate() {
        if out.len() >= row_cap {
            break;
        }
        if let DiffRow::File { path } = row {
            current_path = Some(path.as_str());
        }
        out.push(render_row(
            idx,
            row,
            num_w,
            sign_w,
            &marks,
            width,
            current_path,
            opts,
        ));
    }
    out
}

/// The line-number column width: digits of the widest old/new number across
/// all body rows, at least 1 so a no-number side still reserves a column.
fn number_width(rows: &[DiffRow]) -> usize {
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
    digits(max_no).max(1)
}

/// The body sign-column width: the widest `signs` string across all body
/// rows (1 for an ordinary hunk, the parent count for a combined merge
/// hunk), at least 1 so the sign always has a column.
fn sign_width(rows: &[DiffRow]) -> usize {
    rows.iter()
        .filter_map(|r| match r {
            DiffRow::Body { signs, .. } => Some(signs.chars().count()),
            _ => None,
        })
        .max()
        .unwrap_or(1)
        .max(1)
}

/// The decimal digit count of `n` (at least 1, for `0`).
fn digits(n: u32) -> usize {
    if n == 0 { 1 } else { (n.ilog10() + 1) as usize }
}

/// Renders one parsed row into a display [`Line`], padding its content with
/// trailing spaces so a row background spans the full `width`. `sign_w` is
/// the sign-gutter width (1, or the parent count for a combined hunk); `opts`
/// carries the syntax / language / horizontal-scroll / tab-width knobs.
/// `current_path` is the most recent [`DiffRow::File`] path, used to look up
/// this file's Tier-1 overlay (`None` ⇒ Tier-0 only).
// `current_path` varies *per row* within one layout pass (it changes as the
// walk crosses a file header), so — unlike the once-built `RenderOpts` knobs
// — it cannot be folded into that struct; it is a genuine per-call input.
// Same `#[allow]` precedent as `split_line` below.
#[allow(clippy::too_many_arguments)]
fn render_row(
    idx: usize,
    row: &DiffRow,
    num_w: usize,
    sign_w: usize,
    marks: &[Option<Vec<bool>>],
    width: usize,
    current_path: Option<&str>,
    opts: &RenderOpts<'_>,
) -> Line<'static> {
    let theme = opts.theme;
    match row {
        DiffRow::File { path } => full_width_line(&format!("─── {path} "), width, theme.file),
        DiffRow::Meta { text } => full_width_line(text, width, theme.meta),
        DiffRow::Binary { text } => full_width_line(text, width, theme.binary),
        DiffRow::Hunk {
            old_start,
            new_start,
            section,
            sign_cols,
        } => full_width_line(
            &hunk_head(*old_start, *new_start, section, *sign_cols),
            width,
            theme.hunk,
        ),
        DiffRow::NoNewline { text } => full_width_line(text, width, theme.context),
        DiffRow::Body {
            kind,
            old_no,
            new_no,
            signs,
            content,
        } => {
            let (row_style, word_style) = body_styles(*kind, theme);
            // The gutter is built and measured exactly as before — it never
            // scrolls horizontally (gap B): only the body content does.
            let gutter = format!(
                "{old:>w$} {new:>w$} {sign:<sw$} ",
                old = num_str(*old_no),
                new = num_str(*new_no),
                sign = signs,
                w = num_w,
                sw = sign_w,
            );
            let gutter_w = gutter.chars().count();
            let body_w = width.saturating_sub(gutter_w);

            let mut spans = vec![Span::styled(gutter, theme.gutter.patch(row_style))];
            let mask = marks.get(idx).and_then(Option::as_ref).map(Vec::as_slice);
            // Tier-1 side/line: a context or addition row is on the *new*
            // side keyed by its new number; a deletion is on the *old* side
            // keyed by its old number. `resolve_overlay` applies the
            // Tier-1 → Tier-0 → none precedence.
            let (side, lineno) = match kind {
                ChangeKind::Deletion => (Side::Left, *old_no),
                ChangeKind::Context | ChangeKind::Addition => (Side::Right, *new_no),
            };
            let overlay = resolve_overlay(opts, current_path, side, lineno, content);
            spans.extend(content_spans(
                content, body_w, mask, row_style, word_style, &overlay, opts,
            ));
            Line::from(spans).style(row_style)
        }
    }
}

/// The hunk header text: `@@ -<old> +<new> @@[ section]` for an ordinary
/// hunk, widened to the matching `@@@ … @@@` fence for a combined merge hunk
/// so its origin reads at a glance.
fn hunk_head(old_start: u32, new_start: u32, section: &str, sign_cols: usize) -> String {
    let fence = "@".repeat(sign_cols + 1);
    let mut head = format!("{fence} -{old_start} +{new_start} {fence}");
    if !section.is_empty() {
        head.push(' ');
        head.push_str(section);
    }
    head
}

/// The (row, intra-line word) style pair for a body line of `kind`.
fn body_styles(kind: ChangeKind, theme: &DiffTheme) -> (Style, Style) {
    match kind {
        ChangeKind::Addition => (theme.addition, theme.word_added),
        ChangeKind::Deletion => (theme.deletion, theme.word_deleted),
        ChangeKind::Context => (theme.context, theme.context),
    }
}

/// Resolves the per-char syntax overlay for one body line, applying the
/// **Tier-1 → Tier-0 → none** precedence (ADR 0024):
///
/// 1. **Tier-1**: if [`Diff::tree_sitter`] is on (`opts.tier1` is `Some`),
///    `current_path` is known, and the precomputed map has an entry for that
///    file's `side` at `lineno` *whose length exactly matches the line's
///    `content.chars().count()`* — use that real tree-sitter slice. The
///    length guard means any reconstruction skew (a hunk Tier-1 could not
///    align) silently falls through rather than mis-painting.
/// 2. **Tier-0**: else, if [`Diff::syntax`] is on, the dependency-free,
///    language-blind (or `language`-specific) [`crate::syntax`] overlay,
///    lexed from a fresh [`LexState`] (a diff row is one non-contiguous
///    line). With `tree_sitter == false` this is the *only* branch reachable
///    — byte-identical to the historical render (gate-enforced).
/// 3. **Neither**: an empty overlay (no syntax colour).
///
/// `side` selects which precomputed map a body line reads: a context or
/// addition line is on the *new* side keyed by `new_no`; a deletion is on the
/// *old* side keyed by `old_no`.
fn resolve_overlay(
    opts: &RenderOpts<'_>,
    current_path: Option<&str>,
    side: Side,
    lineno: Option<u32>,
    content: &str,
) -> Vec<Style> {
    // 1. Tier-1: an exact-length hit in the precomputed per-file map.
    if let (Some(map), Some(path), Some(no)) = (opts.tier1, current_path, lineno) {
        if let Some((new_map, old_map)) = map.get(path) {
            let per_side = match side {
                Side::Right => new_map,
                Side::Left => old_map,
            };
            if let Some(styles) = per_side.get(&no) {
                if styles.len() == content.chars().count() {
                    return styles.clone();
                }
            }
        }
    }
    // 2. Tier-0: the shared dependency-free overlay (the historical path).
    if opts.syntax {
        let styles = syntax_styles(opts.theme);
        return syntax::line_overlay(content, opts.language, &styles, LexState::default()).0;
    }
    // 3. Neither: no syntax colour.
    Vec::new()
}

/// The styled spans for one body line's content, horizontally scrolled by
/// `opts.col`, clipped to `body_w` cells, and padded with trailing spaces so
/// the row background reads as a block.
///
/// Each char's style is the three-layer cascade: the row add/del/context
/// style, then the resolved syntax `overlay` under it (Tier-1 tree-sitter or
/// Tier-0 — see [`resolve_overlay`], whose precedence the caller already
/// applied), then (where the intra-line `mask` is set) the changed-word
/// emphasis on top — so a changed word always wins over a syntax tint, which
/// wins over the plain row. A run is emitted whenever any of those three
/// layers changes.
///
/// The cascade is computed per **original** char (the syntax overlay and the
/// word `mask` are both original-char-indexed, exactly as before), then a
/// literal tab is expanded to the next [`tab_width`](Diff::tab_width) stop
/// (gap D) and the resulting rendered cells are windowed to
/// `[col, col + body_w)` (gap B) — a style-preserving slice, never a clip of
/// the composed line. With `col == 0`, the default `tab_width`, and content
/// free of literal tabs (the overwhelming majority of diff fixtures) every
/// cell is byte-identical to the historical render.
fn content_spans(
    content: &str,
    body_w: usize,
    mask: Option<&[bool]>,
    row_style: Style,
    word_style: Style,
    overlay: &[Style],
    opts: &RenderOpts<'_>,
) -> Vec<Span<'static>> {
    // The syntax overlay (Tier-1 or Tier-0) is resolved by the caller — it
    // knows the row's side / line number / current file path — and handed in
    // already original-char-indexed (one `Style` per `content` char), the
    // exact contract the `crate::syntax` slice had.
    let chars: Vec<char> = content.chars().collect();

    // 2. The per-original-char cascade, then tab expansion (gap D), into a
    //    flat list of rendered cells. A tab's expansion cells inherit that
    //    char's cascaded style so a tinted / changed tab stays tinted /
    //    changed across its whole width. `col` tracks the *rendered* column
    //    so the tab stop lands on a true multiple of `tab_width`.
    let mut cells: Vec<(char, Style)> = Vec::with_capacity(chars.len());
    let mut col = 0usize;
    for (i, &ch) in chars.iter().enumerate() {
        let syn = overlay.get(i).copied().unwrap_or_else(Style::new);
        let marked = mask.is_some_and(|m| m.get(i).copied().unwrap_or(false));
        let mut style = row_style.patch(syn);
        if marked {
            style = style.patch(word_style);
        }
        if ch == '\t' {
            // Advance to the next multiple of `tab_width` (already clamped to
            // >= 1); always at least one cell.
            let stop = col + opts.tab_width - (col % opts.tab_width);
            for _ in col..stop {
                cells.push((' ', style));
            }
            col = stop;
        } else {
            cells.push((ch, style));
            col += 1;
        }
    }

    // 3. Window the rendered cells to `[col_off, col_off + body_w)` — the
    //    horizontal scroll (gap B), a style-preserving slice (not a clip of
    //    the composed line, so per-cell styles survive the pan). Then
    //    coalesce equal-style runs and pad to the full body.
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut run_style = row_style;
    let mut drawn = 0usize;
    for &(ch, style) in cells.iter().skip(opts.col).take(body_w) {
        if !run.is_empty() && style != run_style {
            spans.push(Span::styled(std::mem::take(&mut run), run_style));
        }
        run.push(ch);
        run_style = style;
        drawn += 1;
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, run_style));
    }
    // Pad to the full body so the row background reads as a block.
    if drawn < body_w {
        spans.push(Span::styled(" ".repeat(body_w - drawn), row_style));
    }
    spans
}

/// Maps a [`DiffTheme`]'s twelve `syntax_*` style fields into the shared
/// [`SyntaxStyles`] both the Tier-0 [`crate::syntax`] overlay and the Tier-1
/// [`crate::treesitter`] analyzer consume, so `Diff` keeps owning the colours
/// (and `rstui-theme` themes code for free) while the lexer / parser stay
/// theme-agnostic.
///
/// The four legacy buckets (comment / string / number / keyword) map exactly
/// as before — the only ones Tier-0's language-blind lexer ever emits — so a
/// Tier-0 render is **byte-identical** to the historical one. The richer
/// eight (function / type / constant / variable / operator / punctuation /
/// attribute / namespace) are only ever applied by the Tier-1 tree-sitter
/// parse ([`Diff::tree_sitter`]); under Tier-0 they are simply never looked
/// up.
fn syntax_styles(theme: &DiffTheme) -> SyntaxStyles {
    SyntaxStyles {
        comment: theme.syntax_comment,
        string: theme.syntax_string,
        number: theme.syntax_number,
        keyword: theme.syntax_keyword,
        function: theme.syntax_function,
        type_: theme.syntax_type,
        constant: theme.syntax_constant,
        variable: theme.syntax_variable,
        operator: theme.syntax_operator,
        punctuation: theme.syntax_punctuation,
        attribute: theme.syntax_attribute,
        namespace: theme.syntax_namespace,
    }
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
// Split (side-by-side) layout
// ---------------------------------------------------------------------------

/// The glyph drawn between the two columns in [`DiffLayout::Split`].
const SPLIT_SEP: char = '│';

/// Which of the two split columns a [`SideCell`] is being drawn into. A
/// context line carries both an old and a new number; this picks the one the
/// column shows (old on the left, new on the right). For a deletion/addition
/// only one number exists, so the side is irrelevant to the gutter value.
#[derive(Clone, Copy)]
enum Side {
    /// The left column: old file, deletions, the old line number.
    Left,
    /// The right column: new file, additions, the new line number.
    Right,
}

/// One column of a split row: a fixed-width slot holding either a body line
/// (its number, content, intra-line word marks) or — for the short side of a
/// paired change — nothing.
struct SideCell<'r> {
    /// Which column this slot is; selects a context line's old vs new number.
    side: Side,
    /// The line shown in this column, or `None` for an empty (padding) slot.
    row: Option<&'r DiffRow>,
    /// The intra-line changed-char mask for `row`, when it is a paired change.
    mask: Option<&'r [bool]>,
    /// The current file's path (the most recent [`DiffRow::File`] label),
    /// used to look up this file's Tier-1 overlay; `None` ⇒ Tier-0 only.
    current_path: Option<&'r str>,
}

/// Lays the parsed rows out side by side for a content area `width` wide:
/// old/deletions left, new/additions right, each with its own gutter, a `│`
/// between them. File/hunk/metadata/binary/`\ No newline` rows span the full
/// width. An area too narrow to seat both gutters, a content column each, and
/// the separator degrades to [`layout_rows`] (the unified layout) rather than
/// producing an unreadable sliver — so the caller never has to special-case
/// tiny areas. `syntax` toggles the generic highlight overlay.
fn layout_rows_split(
    rows: &[DiffRow],
    width: usize,
    opts: &RenderOpts<'_>,
    row_cap: usize,
) -> Vec<Line<'static>> {
    let num_w = number_width(rows).max(opts.min_number_width);
    let sign_w = sign_width(rows);

    // Per side: `<num_w> <sign_w> ` gutter + at least one content column. Two
    // of those plus the 1-col separator is the minimum legible split.
    let gutter_w = num_w + sign_w + 2;
    let min_side = gutter_w + 1;
    if width < min_side * 2 + 1 {
        return layout_rows(rows, width, opts, row_cap);
    }
    let left_w = (width - 1) / 2;
    let right_w = width - 1 - left_w;

    let marks = intra_line_marks(rows);
    // DIFF-1: same byte-identical-prefix cap as `layout_rows` — break before
    // the next change group once `render`'s `[scroll, scroll + height)`
    // window is covered. A group may push several rows, so the produced set
    // can overshoot `row_cap` slightly; that is fine (still an exact prefix,
    // and `render` clips with `take`). `Diff::lines` passes `usize::MAX`.
    let mut out = Vec::with_capacity(rows.len().min(row_cap));
    let mut i = 0;
    // The current file path — the most recent `DiffRow::File` — selects which
    // file's Tier-1 overlay a body row reads (Tier-0 path ignores it).
    let mut cur_path: Option<&str> = None;
    while i < rows.len() {
        if out.len() >= row_cap {
            break;
        }
        if let DiffRow::File { path } = &rows[i] {
            cur_path = Some(path.as_str());
        }
        match &rows[i] {
            // Headers, metadata, binary, and the no-newline marker read
            // across both columns.
            DiffRow::File { .. }
            | DiffRow::Meta { .. }
            | DiffRow::Binary { .. }
            | DiffRow::Hunk { .. }
            | DiffRow::NoNewline { .. } => {
                out.push(full_width_row(&rows[i], width, opts.theme));
                i += 1;
            }
            DiffRow::Body {
                kind: ChangeKind::Context,
                ..
            } => {
                // Context: the same source row on both sides, the left column
                // showing its old number, the right its new number.
                out.push(split_line(
                    &SideCell {
                        side: Side::Left,
                        row: Some(&rows[i]),
                        mask: None,
                        current_path: cur_path,
                    },
                    &SideCell {
                        side: Side::Right,
                        row: Some(&rows[i]),
                        mask: None,
                        current_path: cur_path,
                    },
                    num_w,
                    sign_w,
                    left_w,
                    right_w,
                    opts,
                ));
                i += 1;
            }
            DiffRow::Body { .. } => {
                // A change group: consecutive deletions, then additions.
                // Deletion k pairs with addition k on one screen row; the
                // shorter side is padded with empty slots.
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
                for k in 0..dels.max(adds) {
                    let left = if k < dels {
                        let di = del_start + k;
                        SideCell {
                            side: Side::Left,
                            row: Some(&rows[di]),
                            mask: marks[di].as_deref(),
                            current_path: cur_path,
                        }
                    } else {
                        SideCell {
                            side: Side::Left,
                            row: None,
                            mask: None,
                            current_path: cur_path,
                        }
                    };
                    let right = if k < adds {
                        let ai = add_start + k;
                        SideCell {
                            side: Side::Right,
                            row: Some(&rows[ai]),
                            mask: marks[ai].as_deref(),
                            current_path: cur_path,
                        }
                    } else {
                        SideCell {
                            side: Side::Right,
                            row: None,
                            mask: None,
                            current_path: cur_path,
                        }
                    };
                    out.push(split_line(
                        &left, &right, num_w, sign_w, left_w, right_w, opts,
                    ));
                }

                // A row that began neither a deletion nor an addition (only
                // possible if the grammar grew a new body kind) still moves.
                if i == del_start {
                    out.push(split_line(
                        &SideCell {
                            side: Side::Left,
                            row: Some(&rows[i]),
                            mask: None,
                            current_path: cur_path,
                        },
                        &SideCell {
                            side: Side::Right,
                            row: None,
                            mask: None,
                            current_path: cur_path,
                        },
                        num_w,
                        sign_w,
                        left_w,
                        right_w,
                        opts,
                    ));
                    i += 1;
                }
            }
        }
    }
    out
}

/// A full-width header [`Line`] for split mode, reusing the unified renderer
/// so file/meta/binary/hunk/no-newline rows are styled identically in both
/// layouts.
fn full_width_row(row: &DiffRow, width: usize, theme: &DiffTheme) -> Line<'static> {
    match row {
        DiffRow::File { path } => full_width_line(&format!("─── {path} "), width, theme.file),
        DiffRow::Meta { text } => full_width_line(text, width, theme.meta),
        DiffRow::Binary { text } => full_width_line(text, width, theme.binary),
        DiffRow::Hunk {
            old_start,
            new_start,
            section,
            sign_cols,
        } => full_width_line(
            &hunk_head(*old_start, *new_start, section, *sign_cols),
            width,
            theme.hunk,
        ),
        DiffRow::NoNewline { text } => full_width_line(text, width, theme.context),
        // Body rows never reach here (the split walker handles them).
        DiffRow::Body { content, .. } => full_width_line(content, width, theme.context),
    }
}

/// Composes one split screen row: the left column, the `│` separator, the
/// right column. The line's base style is the separator/blank style so the
/// gap between the two columns and any empty padding inherit the widget base
/// (and the framing block fill) rather than a diff color.
#[allow(clippy::too_many_arguments)]
fn split_line(
    left: &SideCell<'_>,
    right: &SideCell<'_>,
    num_w: usize,
    sign_w: usize,
    left_w: usize,
    right_w: usize,
    opts: &RenderOpts<'_>,
) -> Line<'static> {
    let mut spans = side_spans(left, num_w, sign_w, left_w, opts);
    spans.push(Span::styled(SPLIT_SEP.to_string(), Style::new()));
    spans.extend(side_spans(right, num_w, sign_w, right_w, opts));
    Line::from(spans)
}

/// The spans for one column, exactly `side_w` cells wide: a
/// `<num> <signs> ` gutter then the content with its syntax/word styling,
/// padded so the column's background spans the full slot. An empty slot (the
/// short side of a paired change) is `side_w` blank cells with the
/// inherit-everything style, so it reads as themed empty space, not a
/// colored line. `sign_w` is the (combined-aware) sign-column width. The
/// gutter never scrolls horizontally; only the content honours `opts.col`.
fn side_spans(
    cell: &SideCell<'_>,
    num_w: usize,
    sign_w: usize,
    side_w: usize,
    opts: &RenderOpts<'_>,
) -> Vec<Span<'static>> {
    let Some(DiffRow::Body {
        kind,
        old_no,
        new_no,
        signs,
        content,
    }) = cell.row
    else {
        // Empty padding slot: blank, themed by the cascade only.
        return vec![Span::styled(" ".repeat(side_w), Style::new())];
    };

    let (row_style, word_style) = body_styles(*kind, opts.theme);
    // A deletion has only an old number, an addition only a new one; a
    // context line has both, so the left column shows old, the right new.
    // `shown_no` is also exactly the line number the Tier-1 overlay for this
    // column's `side` is keyed by.
    let shown_no = match cell.side {
        Side::Left => *old_no,
        Side::Right => *new_no,
    };

    let gutter_w = num_w + sign_w + 2;
    let body_w = side_w.saturating_sub(gutter_w);

    let mut spans: Vec<Span<'static>> = Vec::new();
    let gutter = format!(
        "{n:>num_w$} {sign:<sw$} ",
        n = num_str(shown_no),
        sign = signs,
        sw = sign_w,
    );
    spans.push(Span::styled(gutter, opts.theme.gutter.patch(row_style)));
    // The split column already knows its `side`; resolve Tier-1 → Tier-0 →
    // none for *this* column's line number (the same precedence the unified
    // renderer applies).
    let overlay = resolve_overlay(opts, cell.current_path, cell.side, shown_no, content);
    spans.extend(content_spans(
        content, body_w, cell.mask, row_style, word_style, &overlay, opts,
    ));
    spans
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
                // Nested `if` (not an `if let … && …` let-chain): let-chains
                // only stabilized in Rust 1.88, but the workspace MSRV is 1.85
                // (ADR 0003); the chain form was an msrv-gate regression.
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
    fn the_row_cap_is_an_exact_prefix_of_the_full_layout() {
        // DIFF-1 gate (the PG-2/CM-3 exactness discipline). `Diff::render`
        // is `laid_out(w, scroll + height).skip(scroll).take(height)`; the
        // pre-DIFF-1 render was `lines(w).skip(scroll).take(height)`. So if
        // `laid_out(w, cap)` is an *exact prefix* of the uncapped `lines(w)`
        // for every cap, the painted window is provably byte-identical for
        // every scroll (its `[scroll, scroll+height) ⊆ [0, cap)`). Pinned
        // across both layouts, narrow/wide widths, and caps from 1 to past
        // the end, on a patch far longer than any viewport.
        let mut patch = String::from("@@ -1,30 +1,30 @@ fn big\n");
        for i in 0..30 {
            patch.push_str(&format!(
                " ctx {i} with some words\n-old value {i}\n+new value {i}\n"
            ));
        }
        for split in [false, true] {
            for w in [12u16, 30, 80] {
                let base = || {
                    let d = Diff::new(patch.as_str()).syntax(true);
                    if split { d.side_by_side() } else { d }
                };
                let full = base().lines(w); // uncapped: the authoritative rows
                assert_eq!(
                    base().laid_out(w, usize::MAX),
                    full,
                    "lines() must stay the uncapped full layout (split={split} w={w})"
                );
                for cap in [0usize, 1, 4, 13, 47, full.len(), full.len() + 50] {
                    let capped = base().laid_out(w, cap);
                    assert!(
                        capped.len() >= cap.min(full.len()),
                        "cap={cap}: produced {} rows, need ≥{} (split={split} w={w})",
                        capped.len(),
                        cap.min(full.len())
                    );
                    assert_eq!(
                        capped,
                        full[..capped.len()],
                        "laid_out({cap}) diverged from the full-layout prefix (split={split} w={w})"
                    );
                }
            }
        }
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
                sign_cols: 1,
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
                sign_cols: 1,
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
    fn git_index_metadata_line_renders_as_its_own_themed_row() {
        let patch = "\
diff --git a/f.rs b/f.rs
index e69de29..4b825dc 100644
--- a/f.rs
+++ b/f.rs
@@ -1 +1 @@
-a
+b";
        let rows = parse_rows(patch);
        // The `index …` line is now a Meta row, never folded into Body text.
        assert!(!rows.iter().any(|r| matches!(
            r,
            DiffRow::Body { content, .. } if content.contains("index")
        )));
        assert_eq!(
            rows[0],
            DiffRow::Meta {
                text: "index e69de29..4b825dc 100644".to_owned(),
            }
        );
        assert!(matches!(rows[1], DiffRow::File { .. }));
        // And it is drawn (themed `meta`), not dropped: the row text appears.
        let out = lines(Diff::new(patch), 34, 4);
        assert!(
            out.lines()
                .next()
                .unwrap()
                .starts_with("index e69de29..4b825dc 100644")
        );
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

    // -----------------------------------------------------------------------
    // Side-by-side (split) layout
    // -----------------------------------------------------------------------

    #[test]
    fn split_basic_add_context_delete_snapshot() {
        // 24 wide → a 1-col gutter each (num_w 1, gutter `n S `), the `│`
        // separator, content padded so each column's background is a block.
        // left_w = (24-1)/2 = 11, right_w = 24-1-11 = 12.
        let patch = "@@ -1,2 +1,2 @@\n ctx\n-old\n+new";
        let out = lines(Diff::new(patch).side_by_side(), 24, 3);
        assert_eq!(
            out,
            "@@ -1 +1 @@             \n\
             1   ctx    │1   ctx     \n\
             2 - old    │2 + new     \n"
        );
    }

    #[test]
    fn split_pads_the_short_side_with_empty_cells() {
        // 2 deletions, 1 addition: row 0 pairs del0/add0, row 1 is del1 on
        // the left with an empty (blank) right column — the columns stay
        // aligned. 20 wide → left_w 9, right_w 10, gutter 4 each.
        let patch = "@@ -1,2 +1 @@\n-aaa\n-bbb\n+ccc";
        let out = lines(Diff::new(patch).side_by_side(), 20, 3);
        assert_eq!(
            out,
            "@@ -1 +1 @@         \n\
             1 - aaa  │1 + ccc   \n\
             2 - bbb  │          \n"
        );
        // The padded right column of the unequal row is all blanks.
        let right_of_row2: String = out.lines().nth(2).unwrap().chars().skip(10).collect();
        assert!(right_of_row2.chars().all(|c| c == ' '));
    }

    #[test]
    fn split_shows_context_on_both_sides_with_side_specific_numbers() {
        // A leading deletion makes the old/new numbering diverge: the context
        // line "ctx" then carries old=2 on the left, new=1 on the right —
        // proving context is echoed to both columns, each with its own
        // gutter number.
        let patch = "@@ -1,3 +1,2 @@\n-x\n ctx\n yyy";
        let out = lines(Diff::new(patch).side_by_side(), 22, 4);
        let ctx_row = out.lines().nth(2).unwrap();
        let (left, right) = ctx_row.split_once('│').unwrap();
        assert!(left.contains("ctx"));
        assert!(right.contains("ctx"));
        // Left gutter shows the old number (2), right the new number (1).
        assert!(left.trim_start().starts_with('2'));
        assert!(right.trim_start().starts_with('1'));
    }

    #[test]
    fn split_preserves_intra_line_word_highlight_on_a_paired_change() {
        let patch = "@@ -1 +1 @@\n-hello world\n+hello there";
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 2));
        Diff::new(patch).side_by_side().render(buf.area(), &mut buf);
        // 40 wide → left_w 19, right_w 20; the right column begins at x=20,
        // its gutter is `1 + ` (cols 20..24), content from col 24:
        // "hello"=24..28, ' '=29, "there"=30..34. Only "there" changed.
        let unchanged = buf.get(Position::new(26, 1)).unwrap(); // 'l' of hello
        assert_eq!(unchanged.symbol, 'l');
        assert_ne!(unchanged.bg, Color::Green);
        let changed = buf.get(Position::new(31, 1)).unwrap(); // 'h' of there
        assert_eq!(changed.symbol, 'h');
        assert_eq!(changed.bg, Color::Green);
        // The deletion's changed word is likewise marked on the left column.
        // Left gutter `1 - ` (cols 0..4), content from col 4: "world"=10..14.
        let del_changed = buf.get(Position::new(10, 1)).unwrap(); // 'w' of world
        assert_eq!(del_changed.symbol, 'w');
        assert_eq!(del_changed.bg, Color::Red);
    }

    #[test]
    fn split_too_narrow_degrades_to_unified_without_panicking() {
        let patch = "@@ -1,2 +1,2 @@\n ctx\n-old\n+new";
        // num_w 1 ⇒ min split width is 2*(1+3+1)+1 = 11. Below that the
        // split layout falls back to unified, so the rows match exactly.
        assert_eq!(
            Diff::new(patch).side_by_side().lines(10),
            Diff::new(patch).lines(10),
        );
        // And a pathologically narrow render must not panic.
        let _ = lines(Diff::new(patch).side_by_side(), 1, 4);
        let _ = lines(Diff::new(patch).side_by_side(), 3, 4);
        assert!(Diff::new(patch).side_by_side().lines(0).is_empty());
    }

    #[test]
    fn split_frames_content_in_the_block_inner_area() {
        // The block draws the border; the split content renders into the
        // 14-wide inner area, its own `│` separator sitting between the two
        // columns (distinct from the block's border `│`).
        let out = lines(
            Diff::new("@@ -1 +1 @@\n-a\n+b")
                .side_by_side()
                .block(Block::bordered()),
            16,
            4,
        );
        assert_eq!(
            out,
            "┌──────────────┐\n\
             │@@ -1 +1 @@   │\n\
             │1 - a │1 + b  │\n\
             └──────────────┘\n"
        );
    }

    #[test]
    fn unified_layout_output_is_byte_for_byte_unchanged() {
        // Regression guard: the default (unified) layout must render exactly
        // as before the split-mode addition. A file header, a hunk with a
        // section, context, an intra-line edit, and the no-newline marker.
        let patch = "\
--- a/m.rs
+++ b/m.rs
@@ -1,3 +1,3 @@ fn run()
 keep
-let a = 1;
+let a = 2;
\\ No newline at end of file";
        let out = lines(Diff::new(patch), 28, 6);
        // Built with `concat!` (not a `\`-continued literal): the addition
        // row's gutter has two leading spaces (`  2 + `, the absent old
        // number's slot), which a line-continuation would silently eat.
        assert_eq!(
            out,
            concat!(
                "─── m.rs                    \n",
                "@@ -1 +1 @@ fn run()        \n",
                "1 1   keep                  \n",
                "2   - let a = 1;            \n",
                "  2 + let a = 2;            \n",
                "\\ No newline at end of file \n",
            )
        );
        // The explicit-layout setter is equivalent to the default.
        assert_eq!(
            Diff::new(patch).layout(DiffLayout::Unified).lines(28),
            Diff::new(patch).lines(28),
        );
    }

    // -----------------------------------------------------------------------
    // Generic syntax highlight
    // -----------------------------------------------------------------------

    #[test]
    fn syntax_is_off_by_default_so_existing_output_is_unchanged() {
        // The same patch with and without the default `.syntax(false)` must
        // be byte-identical (no overlay, the documented default).
        let patch = "@@ -1 +1 @@\n-let n = 1; // c\n+let n = 2; // c";
        assert_eq!(
            Diff::new(patch).lines(40),
            Diff::new(patch).syntax(false).lines(40),
        );
        // And turning it on does not change the *glyphs* (only styling).
        assert_eq!(
            lines(Diff::new(patch), 40, 3),
            lines(Diff::new(patch).syntax(true), 40, 3),
        );
    }

    /// The overlay now comes from the shared `crate::syntax` module
    /// (gap G); `Diff` only maps its `DiffTheme.syntax_*` fields into
    /// [`SyntaxStyles`] via [`syntax_styles`] and lexes a row from a fresh
    /// [`LexState`] under [`Language::Unknown`]. These two tests pin that
    /// mapping *and* the Unknown classification through `Diff`'s own theme,
    /// exactly as the removed private `syntax_overlay` did — proving the
    /// delegation is behaviour-preserving on the default path.
    fn diff_overlay(line: &str, theme: &DiffTheme) -> Vec<Style> {
        syntax::line_overlay(
            line,
            Language::Unknown,
            &syntax_styles(theme),
            LexState::default(),
        )
        .0
    }

    #[test]
    fn syntax_overlay_classifies_keyword_number_string_comment() {
        let theme = DiffTheme::default();
        let ov = diff_overlay("let x = \"hi\"; // tail", &theme);
        let at = |s: &str, off: usize| ov[s.chars().count() - 1 + off];
        // `let` → keyword (chars 0..3).
        assert_eq!(ov[0], theme.syntax_keyword);
        assert_eq!(ov[2], theme.syntax_keyword);
        // `x` is a plain identifier → no overlay.
        assert_eq!(ov[4], Style::new());
        // The `"hi"` literal (incl. both quotes) → string.
        let q = "let x = ".chars().count();
        assert_eq!(ov[q], theme.syntax_string); // opening "
        assert_eq!(ov[q + 3], theme.syntax_string); // closing "
        // The `// tail` run → comment, to end of line.
        let c = "let x = \"hi\"; ".chars().count();
        assert_eq!(ov[c], theme.syntax_comment);
        assert_eq!(at("let x = \"hi\"; // tail", 0), theme.syntax_comment);
    }

    #[test]
    fn syntax_overlay_handles_numbers_hash_and_dash_comments_and_block() {
        let t = DiffTheme::default();
        // Hex / float / underscore numbers.
        let ov = diff_overlay("0xFF + 3.14 + 1_000", &t);
        assert_eq!(ov[0], t.syntax_number); // 0
        assert_eq!(ov[3], t.syntax_number); // F (last of 0xFF)
        assert_eq!(ov[7], t.syntax_number); // 3 of 3.14
        assert_eq!(ov[9], t.syntax_number); // 1 of 1_000-ish
        // A `#` line comment (shell/python) to end of line.
        let ov = diff_overlay("x # note", &t);
        assert_eq!(ov[2], t.syntax_comment);
        assert_eq!(ov[ov.len() - 1], t.syntax_comment);
        // A `--` line comment (SQL/Lua/Haskell).
        let ov = diff_overlay("v -- sql note", &t);
        assert_eq!(ov[2], t.syntax_comment);
        // A single-line `/* … */` block comment, code after it un-styled.
        let ov = diff_overlay("a /* mid */ b", &t);
        let s = "a ".chars().count();
        let e = "a /* mid */".chars().count();
        assert_eq!(ov[s], t.syntax_comment);
        assert_eq!(ov[e - 1], t.syntax_comment);
        assert_eq!(ov[e], Style::new()); // the trailing ` b` is plain
    }

    #[test]
    fn syntax_highlight_is_layered_under_the_add_background_and_word_mark() {
        // `+let v = 2;` — `let` is a keyword. The addition row fg is green;
        // the keyword overlay re-tints the fg, but the row is unchanged. On
        // a *paired* change the changed word's emphasis must still win.
        let patch = "@@ -1 +1 @@\n-let v = 1;\n+let v = 2;";
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 3));
        Diff::new(patch).syntax(true).render(buf.area(), &mut buf);
        // Gutter `  1 + ` is 6 cols; content begins at col 6: `let`=6..9.
        let kw = buf.get(Position::new(6, 2)).unwrap(); // 'l' of let
        assert_eq!(kw.symbol, 'l');
        // Keyword fg (blue) overlaid on the addition row.
        assert_eq!(kw.fg, Color::Blue);
        // The changed digit `2` (col 14) keeps the changed-word background —
        // the word mark out-ranks the syntax tint.
        let changed = buf.get(Position::new(14, 2)).unwrap();
        assert_eq!(changed.symbol, '2');
        assert_eq!(changed.bg, Color::Green);
    }

    #[test]
    fn syntax_highlight_works_in_the_split_layout_too() {
        let patch = "@@ -1 +1 @@\n-fn a() {}\n+fn b() {}";
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 2));
        Diff::new(patch)
            .side_by_side()
            .syntax(true)
            .render(buf.area(), &mut buf);
        // Left column gutter `1 - ` (4 cols); `fn` keyword at cols 4..6.
        let kw = buf.get(Position::new(4, 1)).unwrap();
        assert_eq!(kw.symbol, 'f');
        assert_eq!(kw.fg, Color::Blue);
    }

    // -----------------------------------------------------------------------
    // CE-1: language delegation, horizontal scroll, usize scroll,
    // cheap row_count, tab expansion (gaps G/B/K/D)
    // -----------------------------------------------------------------------

    /// Gap G: with an explicit [`Language`] the overlay uses *that* language's
    /// keyword set, not the language-blind common core. `crate` is a Rust
    /// keyword but is **not** in the Unknown common-core set, so it is tinted
    /// under `.language(Rust)` and plain under the default `Unknown` — proof
    /// the delegation actually threads the language through.
    #[test]
    fn language_selects_a_language_specific_keyword_set() {
        let patch = "@@ -0,0 +1 @@\n+use crate::x;";
        // 24 wide, addition gutter `  1 + ` = 6 cols; content from col 6:
        // `use`=6..9, ` `=9, `crate`=10..15.
        let render = |d: Diff<'_>| {
            let mut buf = Buffer::empty(Rect::new(0, 0, 24, 2));
            d.render(buf.area(), &mut buf);
            buf
        };
        // Default Unknown: `use` is in the common core (tinted blue) but
        // `crate` is NOT, so `crate`'s 'c' keeps the plain addition fg.
        let unknown = render(Diff::new(patch).syntax(true));
        assert_eq!(unknown.get(Position::new(6, 1)).unwrap().symbol, 'u');
        assert_eq!(unknown.get(Position::new(6, 1)).unwrap().fg, Color::Blue);
        let c_unknown = unknown.get(Position::new(10, 1)).unwrap();
        assert_eq!(c_unknown.symbol, 'c');
        assert_ne!(c_unknown.fg, Color::Blue);
        // Rust: `crate` IS a Rust keyword, so now it is tinted blue.
        let rust = render(Diff::new(patch).syntax(true).language(Language::Rust));
        let c_rust = rust.get(Position::new(10, 1)).unwrap();
        assert_eq!(c_rust.symbol, 'c');
        assert_eq!(c_rust.fg, Color::Blue);
    }

    /// Gap G: the default language is [`Language::Unknown`], so `.syntax(true)`
    /// with no `.language(..)` is byte-identical to a patch rendered before
    /// the delegation — every glyph *and* style. Pinned against an explicit
    /// `Language::Unknown` (same path) across a representative code patch.
    #[test]
    fn default_language_is_unknown_and_byte_identical() {
        let patch = "@@ -1 +1 @@\n-let n = 0xFF; // c\n+let n = 1_0; /* b */ x";
        let implicit = Diff::new(patch).syntax(true).lines(48);
        let explicit = Diff::new(patch)
            .syntax(true)
            .language(Language::Unknown)
            .lines(48);
        assert_eq!(implicit, explicit);
    }

    /// Gap B: the horizontal `col` offset slides the **content** left while
    /// the line-number / sign gutter stays fixed, and the per-span styles of
    /// the windowed content survive the pan (it is a style-preserving slice,
    /// not a clip of the composed line).
    #[test]
    fn col_scrolls_content_but_not_the_gutter_and_preserves_styles() {
        // `+let value = 1;` — addition gutter `  1 + ` is 6 cols; content
        // begins at col 6. Without scroll: `let`=6..9 (keyword, blue).
        let patch = "@@ -0,0 +1 @@\n+let value = 1;";
        let mut a = Buffer::empty(Rect::new(0, 0, 24, 2));
        Diff::new(patch).syntax(true).render(a.area(), &mut a);
        // Scroll content right by 4: the first 4 content chars (`let `) are
        // skipped, so `value` now starts at the content origin (col 6). The
        // gutter (`  1 + `) is byte-for-byte the same — it never scrolls.
        let mut b = Buffer::empty(Rect::new(0, 0, 24, 2));
        Diff::new(patch)
            .syntax(true)
            .col(4)
            .render(b.area(), &mut b);
        let gutter_a: String = (0..6)
            .map(|x| a.get(Position::new(x, 1)).unwrap().symbol)
            .collect();
        let gutter_b: String = (0..6)
            .map(|x| b.get(Position::new(x, 1)).unwrap().symbol)
            .collect();
        assert_eq!(gutter_a, "  1 + ");
        assert_eq!(
            gutter_b, gutter_a,
            "the gutter must not scroll horizontally"
        );
        // Content at col 6 is now `value`'s 'v' (was 'l' of `let`).
        assert_eq!(a.get(Position::new(6, 1)).unwrap().symbol, 'l');
        let v = b.get(Position::new(6, 1)).unwrap();
        assert_eq!(v.symbol, 'v');
        // `value` is a plain identifier → plain addition fg, NOT the keyword
        // blue: the windowed slice kept each cell's own (cascaded) style.
        assert_ne!(v.fg, Color::Blue);
        // And the `1` literal further along is still tinted as a number after
        // the pan (style preserved, not flattened). `1;` original content
        // index: `let value = ` is 12 chars, `1` at 12 → after col(4) it is
        // rendered column 12-4+6 = 14.
        let num = b.get(Position::new(14, 1)).unwrap();
        assert_eq!(num.symbol, '1');
        assert_eq!(num.fg, Color::Magenta);
    }

    /// Gap B: `col(0)` (the default) is byte-identical to the historical
    /// render — the horizontal-scroll seam is inert until used.
    #[test]
    fn col_zero_is_byte_identical() {
        let patch = "@@ -1,2 +1,2 @@\n ctx line here\n-old value\n+new value";
        for split in [false, true] {
            let base = || {
                let d = Diff::new(patch).syntax(true);
                if split { d.side_by_side() } else { d }
            };
            assert_eq!(base().col(0).lines(40), base().lines(40), "split={split}");
        }
    }

    /// Gap K: `scroll` is a `usize`, so an offset past `u16::MAX` is honoured
    /// (it simply scrolls a huge generated patch off the top) instead of the
    /// historical `u16` saturating at 65 535. No panic.
    #[test]
    fn usize_scroll_past_u16_max_works() {
        // A patch with > u16::MAX body rows (plus a hunk header).
        let n = u16::MAX as usize + 10;
        let mut patch = String::from("@@ -1,1 +1,1 @@\n");
        for i in 0..n {
            patch.push_str(&format!(" line {i}\n"));
        }
        let d = Diff::new(patch.as_str());
        let total = d.row_count(20);
        assert_eq!(total, n + 1, "hunk header + {n} context rows");
        // Scroll to exactly the last row (index total-1, which is > u16::MAX):
        // the single visible row is the very last context line. Wide enough
        // that the (6-digit) gutter plus `line 65544` is not clipped.
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 1));
        Diff::new(patch.as_str())
            .scroll(total - 1)
            .render(buf.area(), &mut buf);
        let row: String = (0..40)
            .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
            .collect();
        assert!(
            row.contains(&format!("line {}", n - 1)),
            "last row should be the final context line, got {row:?}"
        );
        // Scrolling past the very end paints an empty (themed) area, no panic.
        let mut empty = Buffer::empty(Rect::new(0, 0, 20, 1));
        Diff::new(patch.as_str())
            .scroll(total + 1000)
            .render(empty.area(), &mut empty);
        assert!(empty.cells().iter().all(|c| c.symbol == ' '));
    }

    /// Gap K: `row_count(w)` is exactly `lines(w).len()` for every layout and
    /// width (including the narrow-area split→unified degrade and `w == 0`),
    /// so a caller can clamp scroll off the cheap accessor without re-parsing
    /// per keypress.
    #[test]
    fn row_count_matches_lines_len_across_layouts_and_widths() {
        let patch = "\
diff --git a/m.rs b/m.rs
--- a/m.rs
+++ b/m.rs
@@ -1,3 +1,3 @@ fn run()
 keep
-let a = 1;
+let a = 2;
@@ -9,2 +9,3 @@
 tail
+added";
        for split in [false, true] {
            for w in [0u16, 3, 8, 11, 30, 80] {
                let d = || {
                    let x = Diff::new(patch).syntax(true);
                    if split { x.side_by_side() } else { x }
                };
                assert_eq!(
                    d().row_count(w),
                    d().lines(w).len(),
                    "row_count must equal lines().len() (split={split} w={w})"
                );
            }
        }
    }

    /// Gap D: a literal tab expands to the next multiple of `tab_width`
    /// columns (a real tab stop, not a fixed run), and the default is 4.
    #[test]
    fn tab_expands_to_the_next_column_stop() {
        // `+\tx` then `+ab\tc` — gutter `  1 + ` = 6 cols, content at col 6.
        // Default tab_width 4: a leading `\t` at content col 0 advances to
        // col 4, so `x` lands at content col 4 (screen col 10). For `ab\tc`
        // the tab at content col 2 advances to col 4 (a 2-cell tab), so `c`
        // is at content col 4 (screen col 10) too.
        let patch = "@@ -0,0 +1,2 @@\n+\tx\n+ab\tc";
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 3));
        Diff::new(patch).render(buf.area(), &mut buf);
        // Row 1 `\tx`: cols 6..10 are the expanded tab (spaces), 'x' at 10.
        for x in 6..10 {
            assert_eq!(buf.get(Position::new(x, 1)).unwrap().symbol, ' ');
        }
        assert_eq!(buf.get(Position::new(10, 1)).unwrap().symbol, 'x');
        // Row 2 `ab\tc`: 'a'=6,'b'=7, tab fills 8..10, 'c' at 10 (next stop).
        assert_eq!(buf.get(Position::new(6, 2)).unwrap().symbol, 'a');
        assert_eq!(buf.get(Position::new(7, 2)).unwrap().symbol, 'b');
        assert_eq!(buf.get(Position::new(8, 2)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(9, 2)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(10, 2)).unwrap().symbol, 'c');
        // A custom width: tab_width 2 puts `x` of `\tx` at content col 2
        // (screen col 8).
        let mut b2 = Buffer::empty(Rect::new(0, 0, 16, 3));
        Diff::new(patch).tab_width(2).render(b2.area(), &mut b2);
        assert_eq!(b2.get(Position::new(8, 1)).unwrap().symbol, 'x');
        // `tab_width(0)` is clamped to 1 (a tab is at least one cell) and
        // never panics: `\tx` → one space then `x` at content col 1 (col 7).
        let mut b0 = Buffer::empty(Rect::new(0, 0, 16, 3));
        Diff::new(patch).tab_width(0).render(b0.area(), &mut b0);
        assert_eq!(b0.get(Position::new(6, 1)).unwrap().symbol, ' ');
        assert_eq!(b0.get(Position::new(7, 1)).unwrap().symbol, 'x');
    }

    /// Gap D: tab expansion happens *before* the horizontal `col` slice, so
    /// columns stay correct under a pan, and a fixture with **no** tab is
    /// unaffected by the (default 4) `tab_width` — the byte-identical
    /// guarantee for the overwhelming majority of diff fixtures.
    #[test]
    fn tab_width_default_does_not_touch_tab_free_content() {
        let patch = "@@ -1 +1 @@\n-let a = 1;\n+let a = 2;";
        // No literal tab anywhere → default tab_width 4 changes nothing vs an
        // explicit tab_width(1) (a tab would be 1 cell — moot here).
        assert_eq!(
            Diff::new(patch).syntax(true).lines(40),
            Diff::new(patch).syntax(true).tab_width(1).lines(40),
        );
    }

    // -----------------------------------------------------------------------
    // Combined (merge, `@@@`) diffs
    // -----------------------------------------------------------------------

    #[test]
    fn combined_hunk_header_parses_with_two_sign_columns() {
        let rows = parse_rows("@@@ -1,2 -1,2 +1,3 @@@ fn merge()");
        assert_eq!(
            rows[0],
            DiffRow::Hunk {
                old_start: 1,
                new_start: 1,
                section: "fn merge()".to_owned(),
                sign_cols: 2,
            }
        );
    }

    #[test]
    fn combined_diff_three_way_conflict_snapshot() {
        // A real conflict-style combined hunk (2 parents → 2 sign columns).
        // ` -` = removed in parent 2, `- ` = removed in parent 1, `++` =
        // added relative to both parents, `  ` = common context. The body
        // sign gutter is two cells wide, and only result lines (anything not
        // a pure deletion) carry a new-file number.
        let patch = "\
diff --cc merged.rs
index 1111111,2222222..3333333
--- a/merged.rs
+++ b/merged.rs
@@@ -1,2 -1,2 +1,3 @@@
  fn keep() {}
- let a = 1;
 -let a = 2;
++let a = 3;";
        let out = lines(Diff::new(patch), 32, 7);
        // Gutter is `{old:>1} {new:>1} {sign:<2} ` (7 cells): the conflict
        // deletions carry neither number (they are not in the merge result),
        // the context and the `++` addition do.
        assert_eq!(
            out,
            concat!(
                "index 1111111,2222222..3333333  \n",
                "─── merged.rs                   \n",
                "@@@ -1 +1 @@@                   \n",
                "  1    fn keep() {}             \n",
                "    -  let a = 1;               \n",
                "     - let a = 2;               \n",
                "  2 ++ let a = 3;               \n",
            )
        );
    }

    #[test]
    fn combined_diff_body_signs_and_kinds_are_two_wide() {
        let patch = "\
@@@ -1,2 -1,2 +1,3 @@@
  ctx
- a
 -b
++c";
        let rows = parse_rows(patch);
        // [Hunk, ctx, "- a", " -b", "++c"].
        assert!(matches!(rows[0], DiffRow::Hunk { sign_cols: 2, .. }));
        let signs: Vec<(&str, ChangeKind)> = rows[1..]
            .iter()
            .filter_map(|r| match r {
                DiffRow::Body { signs, kind, .. } => Some((signs.as_str(), *kind)),
                _ => None,
            })
            .collect();
        assert_eq!(
            signs,
            vec![
                ("  ", ChangeKind::Context),
                ("- ", ChangeKind::Deletion),
                (" -", ChangeKind::Deletion),
                ("++", ChangeKind::Addition),
            ]
        );
        // A 2-wide sign gutter: ctx new-no 1, the `++` addition new-no 2;
        // the two single-parent deletions have no result-line number.
        let out = lines(Diff::new(patch), 16, 5);
        assert_eq!(
            out,
            concat!(
                "@@@ -1 +1 @@@   \n",
                "  1    ctx      \n",
                "    -  a        \n",
                "     - b        \n",
                "  2 ++ c        \n",
            )
        );
    }

    // -----------------------------------------------------------------------
    // git metadata rows (rendered, not dropped)
    // -----------------------------------------------------------------------

    #[test]
    fn git_rename_copy_mode_similarity_rows_are_parsed_and_rendered() {
        let patch = "\
diff --git a/old.rs b/new.rs
old mode 100644
new mode 100755
similarity index 86%
rename from old.rs
rename to new.rs
index 1234567..89abcde 100755
@@ -1 +1 @@
-a
+b";
        let rows = parse_rows(patch);
        let metas: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                DiffRow::Meta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            metas,
            vec![
                "old mode 100644",
                "new mode 100755",
                "similarity index 86%",
                "rename from old.rs",
                "rename to new.rs",
                "index 1234567..89abcde 100755",
            ]
        );
        // They render (none silently dropped); the first row is the rename.
        let out = lines(Diff::new(patch), 24, 9);
        assert!(out.lines().next().unwrap().starts_with("old mode 100644"));
    }

    #[test]
    fn new_and_deleted_file_mode_metadata_rows_render() {
        let added = parse_rows(
            "diff --git a/n.rs b/n.rs\nnew file mode 100644\n--- /dev/null\n+++ b/n.rs\n@@ -0,0 +1 @@\n+x",
        );
        assert_eq!(
            added[0],
            DiffRow::Meta {
                text: "new file mode 100644".to_owned(),
            }
        );
        let gone = parse_rows(
            "diff --git a/g.rs b/g.rs\ndeleted file mode 100644\n--- a/g.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-x",
        );
        assert_eq!(
            gone[0],
            DiffRow::Meta {
                text: "deleted file mode 100644".to_owned(),
            }
        );
    }

    // -----------------------------------------------------------------------
    // Binary patches
    // -----------------------------------------------------------------------

    #[test]
    fn textual_binary_files_differ_line_renders_a_binary_row() {
        let rows = parse_rows(
            "diff --git a/logo.png b/logo.png\nBinary files a/logo.png and b/logo.png differ",
        );
        assert_eq!(
            rows[0],
            DiffRow::Binary {
                text: "(binary) a/logo.png and b/logo.png".to_owned(),
            }
        );
        let out = lines(
            Diff::new("diff --git a/x.bin b/x.bin\nBinary files a/x.bin and b/x.bin differ"),
            40,
            1,
        );
        assert_eq!(out, "(binary) a/x.bin and b/x.bin            \n");
    }

    #[test]
    fn git_binary_patch_block_renders_one_row_and_skips_the_payload() {
        let patch = "\
diff --git a/blob b/blob
index 0000000..1111111 100644
GIT binary patch
literal 8
McmZQ$U|?V8000R80RR91
literal 0
HcmV?d00001
";
        let rows = parse_rows(patch);
        // The `index` line is Meta, the `GIT binary patch` is one Binary row,
        // and the base85 payload lines are skipped entirely (never Body).
        assert_eq!(
            rows,
            vec![
                DiffRow::Meta {
                    text: "index 0000000..1111111 100644".to_owned(),
                },
                DiffRow::Binary {
                    text: "(binary file changed)".to_owned(),
                },
            ]
        );
        let out = lines(Diff::new(patch), 26, 2);
        assert_eq!(
            out,
            concat!(
                "index 0000000..1111111 100\n",
                "(binary file changed)     \n",
            )
        );
    }

    #[test]
    fn malformed_combined_header_does_not_panic_and_is_not_a_hunk() {
        // Missing the closing `@@@`: best-effort, never a Hunk, no panic.
        let rows = parse_rows("@@@ -1 -1 +1\n++x");
        assert!(!rows.iter().any(|r| matches!(r, DiffRow::Hunk { .. })));
        let _ = lines(Diff::new("@@@ -1 -1 +1\n++x"), 8, 2);
    }

    #[test]
    fn combined_body_line_with_multibyte_lead_does_not_panic() {
        // A malformed combined body line whose first chars are multi-byte
        // (not the expected ASCII signs) must split on a char boundary, not
        // panic. `sign_cols` 2, the line leads with a 3-byte glyph.
        let patch = "@@@ -1 -1 +1,2 @@@\n€ rest of line";
        let rows = parse_rows(patch);
        // The lead two chars become the (verbatim) sign string, the rest is
        // content — and rendering it is panic-free.
        assert!(matches!(rows.get(1), Some(DiffRow::Body { .. })));
        let _ = lines(Diff::new(patch), 20, 2);
    }

    // -----------------------------------------------------------------------
    // Tier-1 (tree-sitter) syntax colour (ADR 0022 / ADR 0024)
    // -----------------------------------------------------------------------

    /// Renders `widget` into a `width`×`height` buffer and returns the `fg`
    /// colour of the first cell on row `y` whose glyph starts the substring
    /// `needle` (the rest of `needle` must follow contiguously). `None` if
    /// not found — so a test fails loudly rather than reading the wrong cell.
    fn fg_of<W: Widget>(widget: W, width: u16, height: u16, y: u16, needle: &str) -> Option<Color> {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        let want: Vec<char> = needle.chars().collect();
        'cols: for x in 0..width {
            for (k, &wc) in want.iter().enumerate() {
                let cx = x as usize + k;
                if cx >= width as usize {
                    continue 'cols;
                }
                if buf.get(Position::new(cx as u16, y)).unwrap().symbol != wc {
                    continue 'cols;
                }
            }
            return Some(buf.get(Position::new(x, y)).unwrap().fg);
        }
        None
    }

    /// The headline payoff: a real tree-sitter parse colours the diff with
    /// the *richer* semantic classes, not the four-bucket Tier-0 lexer. A
    /// **multi-hunk** Rust unified patch (the `+++ b/lib.rs` header lets the
    /// internal [`Changeset`] pick the Rust grammar — no `.language(..)`
    /// needed); an added line's *function name* takes `syntax_function`
    /// (Cyan) and an added line's *type name* takes `syntax_type` (Yellow),
    /// both distinct from the `syntax_keyword` (Blue) the `fn`/`struct`
    /// tokens get. The four-bucket Tier-0 lexer cannot tell any of these
    /// apart (every identifier is plain), so this proves the upgrade.
    #[cfg(feature = "rust")]
    #[test]
    fn tree_sitter_colours_added_function_and_type_names_in_a_multi_hunk_rust_patch() {
        // Two hunks; the new-side reconstruction is
        // `struct Foo {}\nstruct Bar {}\nfn keep() {}\nfn other() {}\n\
        //  fn do_thing() -> Foo { Foo {} }`, so `Bar` (added, new line 2),
        // and `do_thing`/`Foo` (added, new line 12) are real parse-tree
        // type / function captures.
        let patch = "\
--- a/lib.rs
+++ b/lib.rs
@@ -1,2 +1,3 @@
 struct Foo {}
+struct Bar {}
 fn keep() {}
@@ -10,1 +11,2 @@
 fn other() {}
+fn do_thing() -> Foo { Foo {} }
";
        let theme = DiffTheme::default();
        let d = || Diff::new(patch).syntax(true).tree_sitter(true);
        // Row layout: File(0), Hunk(1), ` struct Foo`(2), `+struct Bar`(3),
        // ` fn keep`(4), Hunk(5), ` fn other`(6), `+fn do_thing`(7).
        let w = 48;
        let h = 8;
        // The added type name `Bar` (row 3) → the `syntax_type` colour.
        assert_eq!(
            fg_of(d(), w, h, 3, "Bar"),
            Some(theme.syntax_type.fg.unwrap()),
            "added type name must take the tree-sitter type class (Yellow)"
        );
        // The added function name `do_thing` (row 7) → `syntax_function`.
        assert_eq!(
            fg_of(d(), w, h, 7, "do_thing"),
            Some(theme.syntax_function.fg.unwrap()),
            "added fn name must take the tree-sitter function class (Cyan)"
        );
        // The return type `Foo` on the same added line → `syntax_type`.
        assert_eq!(
            fg_of(d(), w, h, 7, "Foo"),
            Some(theme.syntax_type.fg.unwrap()),
            "added return type must take the tree-sitter type class"
        );
        // The `fn` keyword on that added line → `syntax_keyword` (Blue) —
        // a class *distinct* from function/type, which the 4-bucket lexer
        // could not produce for the names.
        assert_eq!(
            fg_of(d(), w, h, 7, "fn "),
            Some(theme.syntax_keyword.fg.unwrap()),
            "the `fn` token stays keyword"
        );
        // Sanity: the three semantic classes are genuinely different colours
        // (so the asserts above are not vacuously equal).
        assert_ne!(theme.syntax_function.fg, theme.syntax_keyword.fg);
        assert_ne!(theme.syntax_type.fg, theme.syntax_keyword.fg);
        assert_ne!(theme.syntax_function.fg, theme.syntax_type.fg);
    }

    /// Tier-1 is opt-in: the *default* (`tree_sitter(false)`) render — even
    /// on a Rust patch — is **byte-identical** to the Tier-0 path (the
    /// gate). Pinned across both layouts: glyphs *and* every per-cell style.
    #[test]
    fn tree_sitter_off_is_byte_identical_to_tier0() {
        let patch = "\
--- a/lib.rs
+++ b/lib.rs
@@ -1,2 +1,2 @@
 struct Foo {}
-fn old() {}
+fn new() -> Foo {}
";
        for split in [false, true] {
            let t0 = || {
                let d = Diff::new(patch).syntax(true);
                if split { d.side_by_side() } else { d }
            };
            let with_default = || {
                // Default = tree_sitter(false); also pin the explicit form.
                let d = Diff::new(patch).syntax(true).tree_sitter(false);
                if split { d.side_by_side() } else { d }
            };
            assert_eq!(
                t0().lines(60),
                with_default().lines(60),
                "tree_sitter(false) must be byte-identical to Tier-0 (split={split})"
            );
            // And the rendered cells (style included), not just the glyphs.
            let mut a = Buffer::empty(Rect::new(0, 0, 60, 6));
            let mut b = Buffer::empty(Rect::new(0, 0, 60, 6));
            t0().render(a.area(), &mut a);
            with_default().render(b.area(), &mut b);
            assert_eq!(a, b, "Tier-0 cells must be identical (split={split})");
        }
    }

    /// An unknown extension (no grammar) with `tree_sitter(true)` transparently
    /// falls back to the Tier-0 overlay — byte-identical to the Tier-0 render,
    /// glyphs and styles. The fallback is *per file/line*, so a mixed patch
    /// still Tier-1-colours the files it can.
    #[test]
    fn unknown_extension_with_tree_sitter_falls_back_to_tier0() {
        // `.weird` matches no shipped grammar ⇒ every line is Tier-0.
        let patch = "\
--- a/notes.weird
+++ b/notes.weird
@@ -1 +1 @@
-let n = 1; // c
+let n = 2; // c
";
        let t0 = Diff::new(patch).syntax(true);
        let t1 = Diff::new(patch).syntax(true).tree_sitter(true);
        assert_eq!(t0.lines(40), t1.lines(40));
        let mut a = Buffer::empty(Rect::new(0, 0, 40, 4));
        let mut b = Buffer::empty(Rect::new(0, 0, 40, 4));
        t0.render(a.area(), &mut a);
        t1.render(b.area(), &mut b);
        assert_eq!(a, b, "unknown-language file must render exactly as Tier-0");
    }

    /// Totality: a garbage / binary / empty patch with `tree_sitter(true)`
    /// (in either layout) must never panic — the precompute, reconstruction,
    /// and per-line length guard are all total.
    #[test]
    fn tree_sitter_is_total_on_garbage_binary_and_empty_patches() {
        let cases = [
            "",
            "not a diff at all\njust text\n",
            // A Rust-named binary patch (no textual hunks).
            "diff --git a/x.rs b/x.rs\nBinary files a/x.rs and b/x.rs differ\n",
            // A truncated hunk header + half a line, Rust-named.
            "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1\n+fn ",
            // A `@@@` combined merge hunk (out of Tier-1 scope → Tier-0).
            "--- a/m.rs\n+++ b/m.rs\n@@@ -1,1 -1,1 +1,1 @@@\n++ fn z() {}\n",
            // Multibyte content under a Rust extension.
            "--- a/u.rs\n+++ b/u.rs\n@@ -0,0 +1 @@\n+let s = \"€λ→\"; // 𝕊\n",
        ];
        for p in cases {
            for split in [false, true] {
                let d = Diff::new(p).syntax(true).tree_sitter(true);
                let d = if split { d.side_by_side() } else { d };
                let _ = d.row_count(40);
                let _ = lines(d, 40, 10);
            }
        }
    }

    /// The new `DiffTheme` Tier-1 fields have sensible non-empty defaults and
    /// the legacy four are unchanged (the Tier-0 byte-identity contract is a
    /// function of these defaults staying put).
    #[test]
    fn diff_theme_tier1_defaults_are_present_and_legacy_unchanged() {
        let t = DiffTheme::default();
        // Legacy four — unchanged.
        assert_eq!(t.syntax_string, Style::new().fg(Color::Green));
        assert_eq!(t.syntax_number, Style::new().fg(Color::Magenta));
        assert_eq!(
            t.syntax_comment,
            Style::new()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC)
        );
        assert_eq!(
            t.syntax_keyword,
            Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD)
        );
        // New eight — non-empty (except `variable`, deliberately the row fg).
        assert_eq!(t.syntax_function.fg, Some(Color::Cyan));
        assert_eq!(t.syntax_type.fg, Some(Color::Yellow));
        assert_eq!(t.syntax_constant.fg, Some(Color::Magenta));
        assert_eq!(t.syntax_variable, Style::new());
        assert_eq!(t.syntax_operator.fg, Some(Color::DarkGray));
        assert_eq!(t.syntax_punctuation.fg, Some(Color::DarkGray));
        assert_eq!(t.syntax_attribute.fg, Some(Color::Blue));
        assert_eq!(t.syntax_namespace.fg, Some(Color::Cyan));
        // `syntax_styles` now threads ALL twelve into `SyntaxStyles`.
        let ss = syntax_styles(&t);
        assert_eq!(ss.function, t.syntax_function);
        assert_eq!(ss.type_, t.syntax_type);
        assert_eq!(ss.namespace, t.syntax_namespace);
        assert_eq!(ss.keyword, t.syntax_keyword); // legacy still mapped
    }

    #[test]
    fn min_number_width_floors_the_gutter_and_default_is_byte_identical() {
        let patch = "--- a/x\n+++ b/x\n@@ -1,1 +1,1 @@\n-a\n+b\n";
        let (w, h) = (40, 6);
        let bare = lines(Diff::new(patch).syntax(true), w, h);
        let zero = lines(Diff::new(patch).syntax(true).min_number_width(0), w, h);
        let floored = lines(Diff::new(patch).syntax(true).min_number_width(6), w, h);
        assert_eq!(
            bare, zero,
            "min_number_width(0) must be the byte-identical historical render"
        );
        assert_ne!(
            bare, floored,
            "a positive min_number_width must widen the number column"
        );
        // The invariant a >=6-wide right-aligned old+new gutter guarantees:
        // a body row's single-digit line number is padded far right, so
        // column 0 is never a digit (header/meta rows — `─…`, `@@…` — are
        // full-width and exempt). This is the parity the editor's
        // `LineNumberGutter` `min_number_width` gives, now on `Diff` too.
        for row in floored.lines() {
            let c0 = row.chars().next().unwrap_or(' ');
            assert!(
                !c0.is_ascii_digit(),
                "wide gutter ⇒ no row starts with a digit in col 0: {row:?}"
            );
        }
    }
}
