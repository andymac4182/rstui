//! [`Editor`] — a multi-line text-entry panel, the multi-line sibling of
//! [`Input`](rstui_widgets::Input).
//!
//! # A pure projection of a caller-owned [`TextArea`] + `focused`
//!
//! [`Input`](rstui_widgets::Input) projects a caller-owned single-line
//! [`TextEdit`](rstui_core::TextEdit); `Editor` projects a caller-owned
//! **multi-line** [`TextArea`] (the `rstui-core`
//! document model: a `Vec<String>` of logical lines plus a `(row, col)`
//! character-indexed cursor) plus a `focused: bool`. The widget borrows the
//! [`TextArea`] — [`Editor::new`] takes `&TextArea` — and only ever *reads*
//! [`lines`](rstui_core::TextArea::lines) and
//! [`cursor`](rstui_core::TextArea::cursor); the reducer owns the edit and
//! mutates it in `update` (insert on a `Char`, `insert_newline` on `Enter`,
//! `delete_backward` on `Backspace`, the `move_*` family on the arrows). The
//! widget never edits anything at render time, so it composes with the Elm
//! `view(&self)` model exactly like every other rstui widget.
//!
//! # Caller-owned 2D scroll — not derived
//!
//! [`Input`](rstui_widgets::Input) derives its horizontal scroll as a pure function
//! of the cursor and width (no caller state). That keeps the caret on screen
//! with zero bookkeeping, but it cannot do the nicer "only scroll when the
//! caret leaves the view" UX, and it does not generalize cleanly to two axes.
//! `Editor` instead takes a caller-owned 2D [`scroll`](Editor::scroll)
//! `(row_offset, col_offset)` — the same caller-owned-offset model
//! [`List`](rstui_widgets::List)/[`Table`](rstui_widgets::Table) use, the option `Input`'s
//! docs name as the deferred-better one. This is the slice where it is
//! appropriate: a document needs scrolling on both axes, and the app/reducer
//! owns that state ([ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
//! §1 — scroll is plain model state the pure `view` reads, never widget- or
//! runtime-mutated). If the cursor is scrolled out of the visible window the
//! widget draws **no caret**; keeping it in view (a `scroll_into_view` seam on
//! [`TextArea`]) is the caller's job and a deliberately
//! deferred additive — as are selection and undo. None are smuggled into this
//! slice (the same scoping discipline [`List`](rstui_widgets::List) records).
//!
//! # The cursor is *rendered*, not the terminal's — on purpose
//!
//! A [`Widget`] is handed only a [`Buffer`] at render time, never the
//! [`Frame`](rstui_core::Frame), so it physically *cannot* call
//! `Frame::set_cursor_position`. `Editor` therefore draws its **own** caret,
//! generalizing [`Input`](rstui_widgets::Input)'s to 2D: when
//! [`focused`](Editor::focused) the cell under the model cursor is stamped
//! with [`cursor_style`](Editor::cursor_style) (default
//! [`Modifier::REVERSED`](rstui_core::Modifier::REVERSED), so a focused
//! editor shows a visible block caret with zero configuration — the caret is
//! a text field's defining affordance, the one justified exception to the
//! "styles default empty" rule). This is the only TTY-free
//! snapshot-testable choice: the rendered caret shows in a
//! [`TestBackend`](rstui_core::TestBackend) frame, the terminal hardware
//! cursor does not.
//!
//! # A container: an optional framing [`Block`]
//!
//! Unlike the single-row leaf [`Input`](rstui_widgets::Input), an `Editor` is a
//! panel: it takes an optional framing [`Block`] and renders
//! the document into the block's [`inner`](rstui_widgets::Block::inner) area, exactly
//! like [`List`](rstui_widgets::List)/[`Table`](rstui_widgets::Table).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) rule (a pure projection must be *total*):
//! an empty area, a one-cell inner, a document far larger than the panel, a
//! multi-byte line, a scroll past the end, and an empty document are all safe
//! clips/no-ops — never a panic. [`TextArea`] already
//! guarantees a valid `(row, col)` cursor on a real char boundary, so the
//! cell math cannot escape the panel.

use std::borrow::Cow;

use rstui_core::{Buffer, DocSelection, Modifier, Position, Rect, Style, TextArea, Widget};
use rstui_widgets::Block;
use rstui_widgets::extmark::{self, Extmark};

/// One rendered terminal cell of a logical line: the glyph to draw and the
/// **character index** in that logical line it stands for.
///
/// A literal `'\t'` expands to several `ExpandedCell`s — one per padding
/// space — that all carry the *same* `col` (the tab character's char index),
/// so every overlay keyed by document position (syntax / extmark / selection /
/// caret) lines up with the expanded columns automatically.
#[derive(Clone, Copy)]
struct ExpandedCell {
    glyph: char,
    col: usize,
}

/// Expands one logical line into the terminal cells it renders to, replacing
/// each `'\t'` with spaces up to the next multiple of `tab_width` (a
/// `tab_width` of `0` is treated as `1` so it is total). Every emitted cell
/// records the source character index, so the inverse map and the overlays
/// stay consistent with the expanded columns.
fn expand_line(line: &str, tab_width: usize) -> Vec<ExpandedCell> {
    let tw = tab_width.max(1);
    let mut cells = Vec::with_capacity(line.len());
    for (col, ch) in line.chars().enumerate() {
        if ch == '\t' {
            let pad = tw - (cells.len() % tw);
            for _ in 0..pad {
                cells.push(ExpandedCell { glyph: ' ', col });
            }
        } else {
            cells.push(ExpandedCell { glyph: ch, col });
        }
    }
    cells
}

/// One **visual row** of the laid-out document — the unit both
/// [`Editor::render`] and [`Editor::cell_to_doc`] consume so they are exact
/// inverses (and the caret, derived from the same layout, agrees with both).
///
/// With [`wrap`](Editor::wrap) off there is exactly one `VRow` per logical
/// line carrying *all* its expanded cells (render then clips by the
/// horizontal scroll and inner width); with `wrap` on a logical line is split
/// into consecutive `VRow`s of at most the inner width.
struct VRow {
    /// The logical-line index this visual row belongs to.
    doc_row: usize,
    /// The expanded-cell index (within the logical line's full expansion) of
    /// this visual row's first cell — `0` for the unwrapped path and the
    /// first wrapped chunk, `k * width` for the `k`-th wrapped chunk. The
    /// caret's expanded index is located relative to this.
    start_expanded: usize,
    /// The expanded cells on this visual row.
    cells: Vec<ExpandedCell>,
}

/// A multi-line text-entry panel rendered as a pure projection of a
/// caller-owned [`TextArea`], a
/// [`focused`](Self::focused) `bool`, and a caller-owned 2D
/// [`scroll`](Self::scroll) offset.
///
/// The base [`style`](Self::style) fills the whole inner panel (so a
/// background reads as one block); when [`focused`](Self::focused),
/// [`focus_style`](Self::focus_style) is patched **last** over it — the same
/// highlight-wins-last fill [`List`](rstui_widgets::List)/[`Input`](rstui_widgets::Input) use
/// — and the cell under the cursor additionally gets
/// [`cursor_style`](Self::cursor_style). When the document is empty an
/// optional [`placeholder`](Self::placeholder) hint is shown on the first
/// row, styled with [`placeholder_style`](Self::placeholder_style).
///
/// Several **caller-owned, read-only** per-cell overlays compose on top of
/// the base in a fixed cascade — each is plain model state the reducer owns
/// and re-derives on every edit; the widget never owns or mutates any of
/// them, it only *projects* them (the same discipline `extmarks` and
/// `scroll` already obey,
/// [ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
/// §1):
///
/// **base → focus → [syntax](Self::syntax) → [extmark](Self::extmarks) →
/// [selection](Self::selection) → caret**
///
/// - [`syntax`](Self::syntax) — a per-character [`Style`] patch indexed by
///   the **flattened** document char index (rows joined by `'\n'`, *exactly*
///   the [`extmarks`](Self::extmarks) index space, so the two compose),
///   beneath both extmarks and the selection. The reducer builds it with
///   [`rstui_code::syntax::line_overlay`](crate::syntax::line_overlay)
///   threading [`LexState`](crate::syntax::LexState) line to line; the widget
///   just reads it. Empty (the default) is today's behaviour byte for byte.
/// - [`extmarks`](Self::extmarks) (the @-mention / pasted-file "pill" model)
///   patch their [`Style`] over the cells in their character range — the same
///   **flattened** char index, so a pill may span a line break — *above*
///   syntax and *below* the selection (see the [`Extmark`] docs).
/// - [`selection`](Self::selection) — a caller-owned logical
///   [`DocSelection`]: for every rendered cell the widget asks
///   [`contains`](rstui_core::DocSelection::contains)`((doc_row, doc_col))`
///   and, if so, patches [`selection_style`](Self::selection_style) *above*
///   syntax and extmarks but *below* the caret. Charwise / linewise /
///   blockwise are honoured by `DocSelection::contains`.
///
/// A literal `'\t'` expands to spaces up to the next multiple of
/// [`tab_width`](Self::tab_width) (default `4`), and when
/// [`wrap`](Self::wrap) is set each logical line soft-wraps at the inner
/// width into several visual rows (default off = clip, today's behaviour byte
/// for byte). Both keep the rendered caret and
/// [`cell_to_doc`](Self::cell_to_doc) consistent with the expanded /
/// wrapped columns.
///
/// # Example
///
/// ```
/// use rstui_code::Editor;
/// use rstui_core::{Buffer, Position, Rect, TextArea, Widget};
///
/// // `doc` is plain caller-owned model state the widget only reads.
/// let mut doc = TextArea::from_value("line one\nline two");
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 2));
/// Editor::new(&doc).focused(true).render(buf.area(), &mut buf);
///
/// // The document renders top-left, one logical line per row.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'l');
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'l');
///
/// // Editing happens in the reducer, on the model — never in the widget.
/// doc.move_doc_start();
/// doc.insert_char('!');
/// assert_eq!(doc.line(0), Some("!line one"));
/// ```
#[derive(Debug, Clone)]
pub struct Editor<'a> {
    model: &'a TextArea,
    focused: bool,
    scroll: (usize, usize),
    extmarks: &'a [Extmark],
    syntax: &'a [Style],
    selection: Option<&'a DocSelection>,
    selection_style: Style,
    tab_width: usize,
    wrap: bool,
    block: Option<Block<'a>>,
    style: Style,
    focus_style: Style,
    cursor_style: Style,
    placeholder: Cow<'a, str>,
    placeholder_style: Style,
}

impl<'a> Editor<'a> {
    /// An editor projecting `model`: unfocused, unscrolled, no block, no
    /// placeholder, a default reversed-cell caret and otherwise unstyled.
    #[must_use]
    pub fn new(model: &'a TextArea) -> Self {
        Self {
            model,
            focused: false,
            scroll: (0, 0),
            extmarks: &[],
            syntax: &[],
            selection: None,
            selection_style: Style::new(),
            tab_width: 4,
            wrap: false,
            block: None,
            style: Style::new(),
            focus_style: Style::new(),
            // The caret is a text field's defining affordance: a focused
            // editor with an invisible cursor is broken, so unlike
            // `focus_style` this defaults to a visible reverse-video block.
            cursor_style: Style::new().add_modifier(Modifier::REVERSED),
            placeholder: Cow::Borrowed(""),
            placeholder_style: Style::new(),
        }
    }

    /// Sets whether this editor is focused — caller-owned state the widget
    /// only reads (move it in `update`, typically on `Tab`, e.g. via a
    /// `FocusRing`). When `true` the [`focus_style`](Self::focus_style) fill
    /// and the [`cursor_style`](Self::cursor_style) caret are drawn.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Sets the caller-owned 2D scroll offset `(row_offset, col_offset)`: the
    /// first document row and the first character column drawn at the inner
    /// top-left. Caller-owned state the reducer changes in `update`; the
    /// widget never derives or mutates it (see the [module docs](self)). A
    /// cursor scrolled outside the visible window draws no caret.
    #[must_use]
    pub fn scroll(mut self, scroll: (usize, usize)) -> Self {
        self.scroll = scroll;
        self
    }

    /// Sets the caller-owned [`Extmark`] list — styled (optionally atomic)
    /// character ranges patched over the document (@-mention / pasted-file
    /// pills). The range is a **flattened** char index (rows joined by
    /// `'\n'`), so a pill may cross a line break. The reducer owns the slice
    /// and re-derives it on every edit; the widget only reads it and never
    /// enforces atomicity (that is the reducer's cursor-stepping job — see the
    /// [`Extmark`] docs). Empty, reversed, overlapping, and out-of-range
    /// ranges are all total.
    ///
    /// ```
    /// use rstui_code::Editor;
    /// use rstui_core::{Buffer, Color, Position, Rect, Style, TextArea, Widget};
    /// use rstui_widgets::Extmark;
    ///
    /// let doc = TextArea::from_value("hi @ada\nbye");
    /// // Flattened char index: '@' is char 3 of "hi @ada\nbye".
    /// let marks = [Extmark::pill(3..7, Style::new().bg(Color::Blue))];
    /// let mut buf = Buffer::empty(Rect::new(0, 0, 7, 2));
    /// Editor::new(&doc).extmarks(&marks).render(buf.area(), &mut buf);
    ///
    /// assert_eq!(buf.get(Position::new(3, 0)).unwrap().bg, Color::Blue);
    /// assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Reset);
    /// ```
    #[must_use]
    pub fn extmarks(mut self, extmarks: &'a [Extmark]) -> Self {
        self.extmarks = extmarks;
        self
    }

    /// Sets the caller-owned **syntax-highlight overlay**: a per-character
    /// [`Style`] patch indexed by the **flattened** document char index (rows
    /// joined by `'\n'`, *exactly* the [`extmarks`](Self::extmarks) index
    /// space, so the two compose). `overlay[i]` is patched onto the cell at
    /// flattened char *i*; an empty [`Style`] (or an index past the slice end)
    /// leaves the cell unchanged.
    ///
    /// It cascades **beneath** [`extmarks`](Self::extmarks) and the
    /// [`selection`](Self::selection) (cascade: base → focus → **syntax** →
    /// extmark → selection → caret), mirroring the
    /// [`Diff`](crate::Diff) widget's `syntax-under` layering. This is plain
    /// caller-owned model state the widget only reads: the reducer rebuilds it
    /// on every edit by walking the document line by line with
    /// [`syntax::line_overlay`](crate::syntax::line_overlay), threading the
    /// returned [`LexState`](crate::syntax::LexState) into the next line so
    /// multi-line strings/comments colour correctly, then concatenating the
    /// per-line vectors (inserting one empty slot for each `'\n'`) into one
    /// flattened slice. The widget never owns, derives, or mutates it.
    ///
    /// **Empty (the default) is today's render byte for byte** — exactly like
    /// the other `*_style` defaults-empty rule. Out-of-range, short, and empty
    /// slices are all total (a missing index is simply no patch).
    ///
    /// ```
    /// use rstui_core::{Buffer, Color, Position, Rect, Style, TextArea, Widget};
    /// use rstui_code::Editor;
    ///
    /// let doc = TextArea::from_value("let x");
    /// // The reducer would build this with `syntax::line_overlay`; here a
    /// // hand-rolled overlay paints the `let` keyword (flat chars 0..3) red.
    /// let kw = Style::new().fg(Color::Red);
    /// let overlay = [kw, kw, kw, Style::new(), Style::new()];
    /// let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
    /// Editor::new(&doc).syntax(&overlay).render(buf.area(), &mut buf);
    ///
    /// assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Red); // 'l'
    /// assert_eq!(buf.get(Position::new(4, 0)).unwrap().fg, Color::Reset); // 'x'
    /// ```
    #[must_use]
    pub fn syntax(mut self, overlay: &'a [Style]) -> Self {
        self.syntax = overlay;
        self
    }

    /// Sets the caller-owned **logical selection** to project. For every
    /// rendered cell the widget asks
    /// [`DocSelection::contains`](rstui_core::DocSelection::contains)`((doc_row,
    /// doc_col))` and, when it is inside, patches
    /// [`selection_style`](Self::selection_style) over that cell —
    /// charwise / linewise / blockwise all honoured by `DocSelection`'s own
    /// per-character predicate.
    ///
    /// The selection cascades **above** [`syntax`](Self::syntax) and
    /// [`extmarks`](Self::extmarks) but **below** the caret (cascade: base →
    /// focus → syntax → extmark → **selection** → caret), so a highlighted
    /// span keeps the syntax foreground yet the caret still wins the cell it
    /// sits on (the proven `Selection`/`extmark` per-cell-flag pattern, the
    /// logical dual one level up). It is plain caller-owned model state the
    /// reducer drives (Shift+motion `start`/`extend`, a plain move `clear`,
    /// the mouse via [`cell_to_doc`](Self::cell_to_doc)); the widget only
    /// reads it. No selection set (the default `None`) is no change at all.
    ///
    /// ```
    /// use rstui_core::{Buffer, Color, DocSelection, Modifier, Position, Rect, Style, TextArea, Widget};
    /// use rstui_code::Editor;
    ///
    /// let doc = TextArea::from_value("hello");
    /// // Caller-owned: Shift+Right ×2 from col 1 selected "el" ([1, 3)).
    /// let mut sel = DocSelection::new();
    /// sel.start((0, 1), rstui_core::SelKind::Char);
    /// sel.extend((0, 3));
    /// let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
    /// Editor::new(&doc)
    ///     .selection(&sel)
    ///     .selection_style(Style::new().add_modifier(Modifier::REVERSED))
    ///     .render(buf.area(), &mut buf);
    ///
    /// // 'e' and 'l' are highlighted; 'h' before and the caret cell are not.
    /// assert!(buf.get(Position::new(1, 0)).unwrap().modifier.contains(Modifier::REVERSED));
    /// assert!(buf.get(Position::new(2, 0)).unwrap().modifier.contains(Modifier::REVERSED));
    /// assert!(!buf.get(Position::new(0, 0)).unwrap().modifier.contains(Modifier::REVERSED));
    /// assert!(!buf.get(Position::new(3, 0)).unwrap().modifier.contains(Modifier::REVERSED));
    /// ```
    #[must_use]
    pub fn selection(mut self, sel: &'a DocSelection) -> Self {
        self.selection = Some(sel);
        self
    }

    /// Sets the [`Style`] patched over every cell the
    /// [`selection`](Self::selection) [`contains`](rstui_core::DocSelection::contains).
    ///
    /// Defaults to an **empty** [`Style`] — the same "styles default empty,
    /// the caller opts in" rule [`focus_style`](Self::focus_style) /
    /// [`placeholder_style`](Self::placeholder_style) follow (the caret is the
    /// one documented exception). A typical caller sets
    /// `Style::new().add_modifier(Modifier::REVERSED)` for the usual
    /// reverse-video selection, or a background colour from its theme. With
    /// the default left empty a [`selection`](Self::selection) is set but
    /// visually inert — deliberate, so the projection stays byte-identical
    /// until the caller chooses an emphasis.
    #[must_use]
    pub fn selection_style(mut self, s: Style) -> Self {
        self.selection_style = s;
        self
    }

    /// Sets how many columns a literal `'\t'` expands to (default `4`): a tab
    /// is rendered as spaces up to the **next multiple of `w`** measured from
    /// the start of the rendered line (the universal "elastic tab stop"
    /// behaviour, so source indentation no longer collapses to one cell —
    /// gap **D**).
    ///
    /// The expanded columns are the single source of truth for the geometry:
    /// the rendered caret column and [`cell_to_doc`](Self::cell_to_doc) both
    /// account for the expansion (a click anywhere on an expanded tab maps to
    /// that tab's char index; the caret over a tab sits at the tab's *first*
    /// expanded cell), and [`content_height`](Self::content_height) measures
    /// each line by its **expanded** width. A `w` of `0` is treated as `1`,
    /// so it is **total**; with no tab characters present the render is
    /// byte-identical to before regardless of `w`.
    ///
    /// ```
    /// use rstui_core::{Buffer, Position, Rect, TextArea, Widget};
    /// use rstui_code::Editor;
    ///
    /// let doc = TextArea::from_value("\tx"); // a tab then 'x'
    /// let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
    /// Editor::new(&doc).tab_width(4).render(buf.area(), &mut buf);
    ///
    /// // The tab expands to four spaces; 'x' lands at expanded column 4.
    /// for x in 0..4 {
    ///     assert_eq!(buf.get(Position::new(x, 0)).unwrap().symbol, ' ');
    /// }
    /// assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, 'x');
    /// // A click on any of the four expanded cells maps to the tab (col 0);
    /// // 'x' is char index 1.
    /// let ed = Editor::new(&doc).tab_width(4);
    /// let area = Rect::new(0, 0, 6, 1);
    /// assert_eq!(ed.cell_to_doc(area, Position::new(2, 0)), Some((0, 0)));
    /// assert_eq!(ed.cell_to_doc(area, Position::new(4, 0)), Some((0, 1)));
    /// ```
    #[must_use]
    pub fn tab_width(mut self, w: usize) -> Self {
        self.tab_width = w;
        self
    }

    /// Enables **soft-wrap** projection (default `false` = clip, today's
    /// render byte for byte — gap **C**).
    ///
    /// When `true`, each logical line is broken into consecutive *visual rows*
    /// of the inner width (after [`tab_width`](Self::tab_width) expansion); an
    /// empty logical line is one visual row. The caller-owned vertical
    /// [`scroll`](Self::scroll)`.0` then counts **visual** rows (not logical
    /// lines), the caret maps through the wrap to the visual row/column of its
    /// character, and [`cell_to_doc`](Self::cell_to_doc) inverts the wrap. The
    /// horizontal `scroll.1` is ignored when wrapping (there is nothing to
    /// scroll past — the line is reflowed, not clipped).
    ///
    /// **Semantics, stated precisely:** wrapping is by expanded column, hard
    /// (mid-word) at exactly the inner width — no word boundaries. The caret
    /// is mapped to its character's wrapped cell; a caret one past the end of
    /// a line that itself fills the width sits on the first cell of the *next*
    /// visual row (where the next typed character would land). It is **total**
    /// for every input — a zero-width area, a caret far past the end, an empty
    /// document, a `scroll` past the last visual row — none panic; they clip
    /// to a blank/no caret exactly as the unwrapped path does. Clip stays the
    /// default; wrap is an opt-in projection mode with its own correctness
    /// surface (the deliberate non-goal in the deep-dive's Part 8).
    ///
    /// ```
    /// use rstui_core::{Buffer, Position, Rect, TextArea, Widget};
    /// use rstui_code::Editor;
    ///
    /// // One 6-char logical line wrapped into two visual rows at width 4.
    /// let doc = TextArea::from_value("abcdef");
    /// let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
    /// Editor::new(&doc).wrap(true).render(buf.area(), &mut buf);
    ///
    /// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'a');
    /// assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, 'd');
    /// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'e');
    /// assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'f');
    /// ```
    #[must_use]
    pub fn wrap(mut self, on: bool) -> Self {
        self.wrap = on;
        self
    }

    /// The number of terminal rows the document needs if every logical line
    /// is soft-wrapped at `width` columns — a **pure measurement** of the
    /// borrowed model, owning no state and touching no [`Buffer`], exactly as
    /// [`Block::inner`](rstui_widgets::Block::inner) is a pure geometry accessor.
    ///
    /// This is the composer auto-grow input: a chat/commit-message panel asks
    /// "how tall must I be to show all of this at my current width?" and sizes
    /// the [`Editor`]'s area accordingly (then drives the visible window with
    /// a caller-owned [`scroll`](Self::scroll) /
    /// [`ScrollState`](rstui_core::ScrollState) once it hits its cap). Each
    /// logical line contributes `ceil(expanded / width)` rows where `expanded`
    /// is its width after [`tab_width`](Self::tab_width) tab expansion (with
    /// no tabs that is just the char count, so this is byte-identical to
    /// before); an empty line is one row, so the result is at least `1` (a
    /// [`TextArea`] is never zero lines). With [`wrap`](Self::wrap) off the
    /// [`Editor`] *renders* by clipping columns, not wrapping — this is then
    /// the height a wrapping composer reserves; with `wrap` on it is exactly
    /// the rendered visual-row count. The two were intentionally distinct
    /// seams before `wrap` existed; they now agree.
    ///
    /// **Total**: `width == 0` yields `0` (no column to wrap into), an
    /// enormous document saturates at [`u16::MAX`] — never a panic.
    #[must_use]
    pub fn content_height(&self, width: u16) -> u16 {
        let width = width as usize;
        if width == 0 {
            return 0;
        }
        let rows = self.model.lines().iter().fold(0usize, |acc, line| {
            let cells = expand_line(line, self.tab_width).len();
            acc.saturating_add(if cells == 0 { 1 } else { cells.div_ceil(width) })
        });
        u16::try_from(rows).unwrap_or(u16::MAX)
    }

    /// [`content_height`](Self::content_height) clamped to `min..=max` rows —
    /// the height an auto-growing composer gives the editor: it grows with the
    /// text but never below `min` (a one-line minimum) nor above `max` (after
    /// which the caller scrolls the overflow). A pure accessor; `min`/`max`
    /// passed in either order are normalised, so it is **total**.
    #[must_use]
    pub fn desired_height(&self, width: u16, min: u16, max: u16) -> u16 {
        let lo = min.min(max);
        let hi = min.max(max);
        self.content_height(width).clamp(lo, hi)
    }

    /// The whole document laid out as the ordered list of [`VRow`] visual
    /// rows for an inner width of `inner_w`, the single source of truth both
    /// [`render`](Widget::render) and [`cell_to_doc`](Self::cell_to_doc)
    /// project (so they invert each other exactly, and the caret — located
    /// against the same layout — agrees with both). Pure; touches no buffer;
    /// total for any width including `0`.
    fn layout(&self, inner_w: u16) -> Vec<VRow> {
        let w = inner_w as usize;
        let mut rows = Vec::new();
        for (doc_row, line) in self.model.lines().iter().enumerate() {
            let cells = expand_line(line, self.tab_width);
            if !self.wrap || w == 0 {
                // Clip mode (and the degenerate zero-width wrap): one visual
                // row per logical line carrying the full expansion. A
                // zero-width area never draws anyway, so a single empty-ish
                // row keeps both projections total without dividing by zero.
                rows.push(VRow {
                    doc_row,
                    start_expanded: 0,
                    cells,
                });
            } else if cells.is_empty() {
                rows.push(VRow {
                    doc_row,
                    start_expanded: 0,
                    cells: Vec::new(),
                });
            } else {
                let mut start = 0;
                while start < cells.len() {
                    let end = (start + w).min(cells.len());
                    rows.push(VRow {
                        doc_row,
                        start_expanded: start,
                        cells: cells[start..end].to_vec(),
                    });
                    start = end;
                }
            }
        }
        rows
    }

    /// The caret's position against [`layout`](Self::layout): the **global
    /// visual-row index** and the **expanded column within that visual row**
    /// of the model cursor, or `None` if the model is empty (no caret cell).
    ///
    /// The expanded column may equal the visual row's width when the caret is
    /// one past the end of a line that exactly fills the width — it then
    /// renders off the right edge and draws nothing, the same documented
    /// clamp the existing "scrolled out of view → no caret" rule already
    /// applies. Pure and total.
    fn caret_visual(&self, rows: &[VRow]) -> Option<(usize, usize)> {
        if self.model.is_empty() {
            return None;
        }
        let (cur_row, cur_col) = self.model.cursor();
        let line = self.model.line(cur_row).unwrap_or("");
        let expanded = expand_line(line, self.tab_width);
        // The expanded index of the caret: the first expanded cell that maps
        // back to `cur_col` (a tab's first padding cell — "the caret over a
        // tab sits at its first expanded cell"), or the end of the expanded
        // line when the caret is at/after the line's last character.
        let e = expanded
            .iter()
            .position(|c| c.col == cur_col)
            .unwrap_or(expanded.len());
        for (idx, vr) in rows.iter().enumerate() {
            if vr.doc_row != cur_row {
                continue;
            }
            let len = vr.cells.len();
            let last_for_line = rows.get(idx + 1).is_none_or(|n| n.doc_row != cur_row);
            // `e` is on this visual row when it is at/after the row's start
            // and either before the row's end, or this is the line's *last*
            // visual row (so a caret exactly at end-of-line lands here, at
            // column `e - start` — possibly == the row width, the off-edge
            // clamp documented on `wrap`).
            if e >= vr.start_expanded && (last_for_line || e < vr.start_expanded + len) {
                return Some((idx, e - vr.start_expanded));
            }
        }
        None
    }

    /// Maps a buffer cell [`Position`] (e.g. a mouse click) back to the
    /// document `(row, col)` it overlies, or `None` if the cell is outside
    /// the inner text area.
    ///
    /// The pure inverse of the render mapping (it consumes the very same
    /// internal visual-row layout, so the two cannot drift), the same
    /// `Rect`-accessor seam [`content_height`](Self::content_height) is:
    /// hit-testing is the reducer's job
    /// ([ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)),
    /// so an app turns a mouse click into a caret with
    /// [`TextArea::set_cursor`](rstui_core::TextArea::set_cursor) (or a
    /// selection anchor) from this. It accounts for the optional [`Block`],
    /// the caller-owned 2D [`scroll`](Self::scroll),
    /// [`tab_width`](Self::tab_width) expansion (a click anywhere on an
    /// expanded tab maps to that tab's char index) and
    /// [`wrap`](Self::wrap) (the click is resolved through the soft-wrap),
    /// and clamps the result to a valid `(row, col)` in the borrowed model,
    /// so it is **total** — any `area`/`pos`, including a click in the
    /// border, on tab padding, or past the end of a short line, is
    /// well-defined and never panics.
    #[must_use]
    pub fn cell_to_doc(&self, area: Rect, pos: Position) -> Option<(usize, usize)> {
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if inner.is_empty()
            || pos.x < inner.left()
            || pos.x >= inner.right()
            || pos.y < inner.top()
            || pos.y >= inner.bottom()
        {
            return None;
        }
        let (row_off, col_off) = self.scroll;
        let rows = self.layout(inner.width);
        let last_row = self.model.row_count().saturating_sub(1);
        let vidx = row_off + (pos.y - inner.top()) as usize;
        let Some(vr) = rows.get(vidx) else {
            // Clicked below the last visual row: clamp to the document end.
            let max_col = self.model.line(last_row).map_or(0, |l| l.chars().count());
            return Some((last_row, max_col));
        };
        // The cell index within this visual row: the unwrapped path adds the
        // horizontal scroll; the wrapped path is reflowed so there is none.
        let sx = (pos.x - inner.left()) as usize;
        let cell_idx = if self.wrap { sx } else { col_off + sx };
        let row = vr.doc_row;
        let col = match vr.cells.get(cell_idx) {
            Some(c) => c.col,
            // Click past the last cell of the visual row → end of that
            // logical line (the natural "click in the blank → line end").
            None => self.model.line(row).map_or(0, |l| l.chars().count()),
        };
        Some((row, col))
    }

    /// Frames the editor in `block`; the document renders into
    /// [`block.inner`](rstui_widgets::Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]. It also fills the inner panel so a background
    /// covers it edge to edge.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] applied when [`focused`](Self::focused).
    ///
    /// Patched **last** across the inner panel, so the focus emphasis
    /// overrides the base and reads as one block — the same role
    /// [`List`](rstui_widgets::List)'s `highlight_style` plays for selection.
    #[must_use]
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = style;
        self
    }

    /// Sets the [`Style`] of the caret cell when [`focused`](Self::focused)
    /// (default [`Modifier::REVERSED`](rstui_core::Modifier::REVERSED)).
    ///
    /// Patched over the base/focus fill at exactly the cursor cell.
    #[must_use]
    pub fn cursor_style(mut self, style: Style) -> Self {
        self.cursor_style = style;
        self
    }

    /// Sets the hint shown on the first row when the document is empty
    /// (default none).
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<Cow<'a, str>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets the [`Style`] of the [`placeholder`](Self::placeholder) hint,
    /// patched over the base (and the focus fill when focused).
    #[must_use]
    pub fn placeholder_style(mut self, style: Style) -> Self {
        self.placeholder_style = style;
        self
    }
}

impl Widget for Editor<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let model = self.model;
        let focused = self.focused;
        let scroll = self.scroll;
        let extmarks = self.extmarks;
        let syntax = self.syntax;
        let selection = self.selection;
        let selection_style = self.selection_style;
        let wrap = self.wrap;
        let style = self.style;
        let focus_style = self.focus_style;
        let cursor_style = self.cursor_style;
        let placeholder_style = self.placeholder_style;

        // The block (if any) frames the content and reserves the inner area.
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if let Some(b) = &self.block {
            b.clone().render(area, buf);
        }
        if inner.is_empty() {
            return;
        }

        // The base, with the focus emphasis patched in when focused. Filling
        // the whole inner panel makes a focused editor read as one block —
        // List's selection-fill idiom, here keyed by `focused`.
        let base = if focused {
            style.patch(focus_style)
        } else {
            style
        };
        buf.set_style(inner, base);

        let left = inner.left();
        let right = inner.right();
        let top = inner.top();

        // Empty document: show the placeholder on the first inner row (never
        // scrolled — there is nothing to scroll). When focused, the caret
        // sits at the inner origin over the placeholder's first glyph (a
        // reversed blank if there is no placeholder), the same "caret
        // reverses the glyph under it" rule the document path uses.
        if model.is_empty() {
            let placeholder = self.placeholder.as_ref();
            let ph_style = base.patch(placeholder_style);
            let mut x = left;
            for ch in placeholder.chars() {
                if x >= right {
                    break;
                }
                buf.set_cell(Position::new(x, top), ch, ph_style);
                x = x.saturating_add(1);
            }
            if focused {
                let glyph = placeholder.chars().next().unwrap_or(' ');
                buf.set_cell(Position::new(left, top), glyph, base.patch(cursor_style));
            }
            return;
        }

        // The shared visual-row layout (tab-expanded, and soft-wrapped when
        // `wrap`). `cell_to_doc` and the caret consume the *same* layout, so
        // the projection and its inverse cannot drift. With `wrap` off this
        // is one `VRow` per logical line carrying every expanded cell.
        let rows = self.layout(inner.width);

        // `flat` is the character index into the flattened document (rows
        // joined by '\n', exactly `TextArea::to_string()`), which is what an
        // extmark range *and* the `syntax` overlay are addressed in. The
        // per-line prefix sum is built once.
        //
        // EDIT-1 (kept): that prefix sum and the per-cell `patch_at` /
        // `syntax` lookup exist only for the flat-indexed overlays. With
        // neither extmarks nor a syntax overlay it is pure per-frame waste,
        // so skip the whole thing — the common composer/editor path.
        let marked = !extmarks.is_empty();
        let syntaxed = !syntax.is_empty();
        let flat_indexed = marked || syntaxed;
        let line_start_flat: Vec<usize> = if flat_indexed {
            let mut acc = 0usize;
            let mut starts = Vec::with_capacity(model.row_count());
            for line in model.lines() {
                starts.push(acc);
                acc = acc.saturating_add(line.chars().count() + 1);
            }
            starts
        } else {
            Vec::new()
        };

        // The full per-cell cascade, given a visual row's `doc_row` and a
        // cell's logical char index `col`:
        //   base/focus → syntax[flat] → extmark(flat) → selection(row,col)
        // (the caret is patched last, after this, on its own cell).
        let cell_style = |doc_row: usize, col: usize| -> Style {
            let mut s = base;
            if flat_indexed {
                let flat = line_start_flat
                    .get(doc_row)
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(col);
                if syntaxed {
                    if let Some(sy) = syntax.get(flat) {
                        s = s.patch(*sy);
                    }
                }
                if marked {
                    s = extmark::patch_at(s, extmarks, flat);
                }
            }
            if let Some(sel) = selection {
                if sel.contains((doc_row, col)) {
                    s = s.patch(selection_style);
                }
            }
            s
        };

        let bottom = inner.bottom();
        let (row_off, col_off) = scroll;

        // Stamp the visible window: visual rows [row_off, row_off + height).
        // With `wrap` off a cell's column is `col_off + screen_x` (horizontal
        // scroll); with `wrap` on the line is reflowed so `screen_x` *is* the
        // in-row index. A row or column past the document is blank base fill.
        for screen_row in 0..inner.height {
            let vidx = row_off + screen_row as usize;
            let Some(vr) = rows.get(vidx) else {
                break;
            };
            let y = top.saturating_add(screen_row);
            let mut x = left;
            for (i, ec) in vr.cells.iter().enumerate() {
                // Unwrapped: skip the horizontally-scrolled-past prefix.
                if !wrap && i < col_off {
                    continue;
                }
                if x >= right {
                    break;
                }
                buf.set_cell(
                    Position::new(x, y),
                    ec.glyph,
                    cell_style(vr.doc_row, ec.col),
                );
                x = x.saturating_add(1);
            }
        }

        // The caret: locate it against the very same layout and patch
        // `cursor_style` last over the cascaded cell. If it is scrolled out
        // of the visible window (or off the right edge — the documented
        // end-of-full-wrapped-line clamp) draw nothing; keeping it in view
        // is the caller's `scroll_into_view` job (see the module docs).
        if focused {
            if let Some((vidx, ecol)) = self.caret_visual(&rows) {
                // Horizontal scroll only applies on the unwrapped path.
                let col_in_view = if wrap {
                    Some(ecol)
                } else {
                    ecol.checked_sub(col_off)
                };
                let row_in_view = vidx.checked_sub(row_off);
                if let (Some(cx), Some(cy)) = (col_in_view, row_in_view) {
                    let sx = left as usize + cx;
                    let sy = top as usize + cy;
                    if sx < right as usize && sy < bottom as usize {
                        let vr = &rows[vidx];
                        // The glyph under the caret is that cell's, or a
                        // blank when the caret is past the row's last cell
                        // (end of line) — exactly the prior behaviour.
                        let (glyph, under) = match vr.cells.get(ecol) {
                            Some(ec) => (ec.glyph, cell_style(vr.doc_row, ec.col)),
                            None => {
                                let col = model.line(vr.doc_row).map_or(0, |l| l.chars().count());
                                (' ', cell_style(vr.doc_row, col))
                            }
                        };
                        buf.set_cell(
                            Position::new(sx as u16, sy as u16),
                            glyph,
                            under.patch(cursor_style),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Color;

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
    fn renders_visible_lines_and_fills_the_inner_panel() {
        let model = TextArea::from_value("abc\nde\nf");
        assert_eq!(lines(Editor::new(&model), 4, 4), "abc \nde  \nf   \n    \n");
    }

    #[test]
    fn row_and_col_offset_scroll_both_axes() {
        let model = TextArea::from_value("row0\nrow1\nrow2\nrow3");
        // Skip the first row and the first two columns.
        let scrolled = Editor::new(&model).scroll((1, 2));
        assert_eq!(lines(scrolled, 3, 3), "w1 \nw2 \nw3 \n");
    }

    #[test]
    fn focused_draws_a_reversed_caret_at_the_2d_cursor_cell() {
        let mut model = TextArea::from_value("abc\ndef");
        model.set_cursor(1, 1); // over 'e' on row 1
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        Editor::new(&model)
            .focused(true)
            .render(buf.area(), &mut buf);

        let caret = buf.get(Position::new(1, 1)).unwrap();
        assert_eq!(caret.symbol, 'e');
        assert!(caret.modifier.contains(Modifier::REVERSED));
        // A neighbouring cell is not reversed.
        assert!(
            !buf.get(Position::new(0, 0))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn caret_scrolled_out_of_view_draws_nothing() {
        let mut model = TextArea::from_value("abc\ndef\nghi");
        model.set_cursor(0, 0); // top-left of the document…
        // …but the view is scrolled down two rows, so it is off-screen.
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Editor::new(&model)
            .focused(true)
            .scroll((2, 0))
            .render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..4 {
                assert!(
                    !buf.get(Position::new(x, y))
                        .unwrap()
                        .modifier
                        .contains(Modifier::REVERSED)
                );
            }
        }
    }

    #[test]
    fn caret_sits_on_the_blank_past_the_end_of_a_line() {
        let model = TextArea::from_value("ab\ncd"); // cursor at (1, 2)
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        Editor::new(&model)
            .focused(true)
            .render(buf.area(), &mut buf);
        let caret = buf.get(Position::new(2, 1)).unwrap();
        assert_eq!(caret.symbol, ' ');
        assert!(caret.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn an_unfocused_editor_draws_no_caret() {
        let model = TextArea::from_value("ab\ncd");
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        Editor::new(&model).render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..5 {
                assert!(
                    !buf.get(Position::new(x, y))
                        .unwrap()
                        .modifier
                        .contains(Modifier::REVERSED)
                );
            }
        }
    }

    #[test]
    fn focus_style_fills_the_panel() {
        let model = TextArea::from_value("hi");
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Editor::new(&model)
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..4 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Blue);
            }
        }
    }

    #[test]
    fn placeholder_shows_only_when_the_model_is_empty() {
        let empty = TextArea::new();
        assert_eq!(
            lines(Editor::new(&empty).placeholder("type…"), 6, 2),
            "type… \n      \n"
        );

        let typed = TextArea::from_value("hi");
        assert_eq!(
            lines(Editor::new(&typed).placeholder("type…"), 6, 2),
            "hi    \n      \n"
        );
    }

    #[test]
    fn a_focused_empty_editor_shows_the_caret_at_the_inner_origin() {
        let empty = TextArea::new();
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        Editor::new(&empty)
            .placeholder("hint")
            .focused(true)
            .render(buf.area(), &mut buf);
        let caret = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(caret.symbol, 'h');
        assert!(caret.modifier.contains(Modifier::REVERSED));
        assert!(
            !buf.get(Position::new(1, 0))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn block_frames_the_editor_in_the_inner_area() {
        let model = TextArea::from_value("hi");
        assert_eq!(
            lines(Editor::new(&model).block(Block::bordered()), 4, 3),
            "┌──┐\n│hi│\n└──┘\n"
        );
    }

    #[test]
    fn a_one_cell_inner_is_total() {
        let mut model = TextArea::from_value("abc\ndef");
        model.set_cursor(0, 0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        Editor::new(&model)
            .focused(true)
            .render(buf.area(), &mut buf);
        let only = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(only.symbol, 'a');
        assert!(only.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn scroll_past_the_document_is_blank_not_a_panic() {
        let model = TextArea::from_value("a\nb");
        // Both offsets far past the end: every cell is the blank base fill.
        assert_eq!(
            lines(Editor::new(&model).scroll((99, 99)), 3, 2),
            "   \n   \n"
        );
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let model = TextArea::from_value("hello\nworld");
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        Editor::new(&model)
            .focused(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let model = TextArea::from_value("Hi\nYo");
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 5));
        Editor::new(&model).render(Rect::new(2, 1, 4, 2), &mut buf);
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, 'H');
        assert_eq!(buf.get(Position::new(3, 1)).unwrap().symbol, 'i');
        assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, 'Y');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn content_height_wraps_each_logical_line_at_width() {
        // 3 chars at width 4 -> 1 row; 9 chars at width 4 -> ceil(9/4)=3 rows;
        // an empty logical line -> 1 row. Total 1 + 3 + 1 = 5.
        let model = TextArea::from_value("abc\n123456789\n");
        assert_eq!(model.row_count(), 3);
        assert_eq!(Editor::new(&model).content_height(4), 5);
        // Wider than every line -> one row per logical line.
        assert_eq!(Editor::new(&model).content_height(80), 3);
        // An empty document is one line, so at least one row.
        assert_eq!(Editor::new(&TextArea::new()).content_height(10), 1);
    }

    #[test]
    fn content_height_is_total_at_zero_width_and_huge_input() {
        let model = TextArea::from_value("hello world");
        assert_eq!(Editor::new(&model).content_height(0), 0); // no panic
        // A single very long line saturates at u16::MAX, not an overflow.
        let huge = TextArea::from_value("x".repeat(300_000));
        assert_eq!(Editor::new(&huge).content_height(1), u16::MAX);
    }

    #[test]
    fn desired_height_clamps_into_the_composer_range() {
        let model = TextArea::from_value("one\ntwo\nthree\nfour");
        // 4 rows at a wide width, clamped to a 2..=10 grow range -> 4.
        assert_eq!(Editor::new(&model).desired_height(40, 2, 10), 4);
        // The same content capped at max 3 -> the caller scrolls the rest.
        assert_eq!(Editor::new(&model).desired_height(40, 1, 3), 3);
        // A short document still gets the minimum height.
        assert_eq!(
            Editor::new(&TextArea::from_value("hi")).desired_height(40, 5, 9),
            5
        );
        // min/max passed reversed are normalised (total).
        assert_eq!(Editor::new(&model).desired_height(40, 10, 2), 4);
    }

    #[test]
    fn a_multibyte_line_maps_each_char_to_one_column() {
        // "é" and "日" are multi-byte; the cursor is a char index so it maps
        // straight to a column with no byte math leaking through.
        let mut model = TextArea::from_value("é日x\nynext");
        model.set_cursor(0, 1); // over "日"
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        Editor::new(&model)
            .focused(true)
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'é');
        let caret = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(caret.symbol, '日');
        assert!(caret.modifier.contains(Modifier::REVERSED));
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'x');
    }

    fn bg(buf: &Buffer, x: u16, y: u16) -> Color {
        buf.get(Position::new(x, y)).unwrap().bg
    }

    #[test]
    fn an_extmark_patches_a_single_line_char_range() {
        let model = TextArea::from_value("hi @ada");
        let marks = [Extmark::pill(3..7, Style::new().bg(Color::Blue))];
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        Editor::new(&model)
            .extmarks(&marks)
            .render(buf.area(), &mut buf);
        for x in 0..3 {
            assert_eq!(bg(&buf, x, 0), Color::Reset);
        }
        for x in 3..7 {
            assert_eq!(bg(&buf, x, 0), Color::Blue);
        }
    }

    #[test]
    fn an_extmark_spans_a_newline_in_the_flattened_index() {
        // "ab\ncd": flat indices a=0 b=1 '\n'=2 c=3 d=4. A pill 1..4 covers
        // 'b' (row 0, col 1) and 'c' (row 1, col 0) — across the line break.
        let model = TextArea::from_value("ab\ncd");
        let marks = [Extmark::new(1..4, Style::new().bg(Color::Red))];
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        Editor::new(&model)
            .extmarks(&marks)
            .render(buf.area(), &mut buf);
        assert_eq!(bg(&buf, 0, 0), Color::Reset); // 'a'
        assert_eq!(bg(&buf, 1, 0), Color::Red); // 'b'
        assert_eq!(bg(&buf, 0, 1), Color::Red); // 'c'
        assert_eq!(bg(&buf, 1, 1), Color::Reset); // 'd'
    }

    #[test]
    fn multiple_extmarks_each_apply() {
        let model = TextArea::from_value("abcd\nefgh");
        let marks = [
            Extmark::new(0..2, Style::new().bg(Color::Red)),
            // flat: e=5 f=6 g=7 → 6..8 covers 'f','g' on row 1.
            Extmark::new(6..8, Style::new().bg(Color::Green)),
        ];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Editor::new(&model)
            .extmarks(&marks)
            .render(buf.area(), &mut buf);
        assert_eq!(bg(&buf, 0, 0), Color::Red);
        assert_eq!(bg(&buf, 1, 0), Color::Red);
        assert_eq!(bg(&buf, 1, 1), Color::Green); // 'f'
        assert_eq!(bg(&buf, 2, 1), Color::Green); // 'g'
        assert_eq!(bg(&buf, 0, 1), Color::Reset); // 'e'
    }

    #[test]
    fn overlapping_extmarks_cascade_last_wins() {
        let model = TextArea::from_value("abcdef");
        let marks = [
            Extmark::new(0..6, Style::new().bg(Color::Red)),
            Extmark::new(2..4, Style::new().bg(Color::Blue)),
        ];
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Editor::new(&model)
            .extmarks(&marks)
            .render(buf.area(), &mut buf);
        assert_eq!(bg(&buf, 1, 0), Color::Red);
        assert_eq!(bg(&buf, 2, 0), Color::Blue); // later mark wins
        assert_eq!(bg(&buf, 4, 0), Color::Red);
    }

    #[test]
    fn an_out_of_range_extmark_is_a_total_no_op() {
        let model = TextArea::from_value("abc\ndef");
        let marks = [Extmark::new(100..200, Style::new().bg(Color::Red))];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Editor::new(&model)
            .extmarks(&marks)
            .render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..4 {
                assert_eq!(bg(&buf, x, y), Color::Reset);
            }
        }
    }

    #[test]
    // Reversed/empty ranges are exactly what this totality test feeds in.
    #[allow(clippy::reversed_empty_ranges)]
    fn empty_and_reversed_ranges_paint_nothing() {
        let model = TextArea::from_value("abcdef");
        let marks = [
            Extmark::new(3..3, Style::new().bg(Color::Red)),
            Extmark::new(5..2, Style::new().bg(Color::Green)),
        ];
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Editor::new(&model)
            .extmarks(&marks)
            .render(buf.area(), &mut buf);
        for x in 0..6 {
            assert_eq!(bg(&buf, x, 0), Color::Reset);
        }
    }

    #[test]
    fn the_caret_wins_over_an_extmark_under_it() {
        let mut model = TextArea::from_value("abc\ndef");
        model.set_cursor(1, 1); // 'e', flat index 5
        let marks = [Extmark::new(0..9, Style::new().bg(Color::Blue))];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Editor::new(&model)
            .focused(true)
            .extmarks(&marks)
            .render(buf.area(), &mut buf);
        let caret = buf.get(Position::new(1, 1)).unwrap();
        assert_eq!(caret.symbol, 'e');
        assert_eq!(caret.bg, Color::Blue); // extmark cascades under…
        assert!(caret.modifier.contains(Modifier::REVERSED)); // …the caret
    }

    #[test]
    fn an_extmark_maps_through_2d_scroll() {
        // Skip the first row and the first two columns; the pill is addressed
        // in flat document indices regardless of the viewport.
        let model = TextArea::from_value("row0\nrow1\nrow2");
        // flat: r=0..3 '\n'=4 r=5 o=6 w=7 1=8 → "row1" is 5..9.
        let marks = [Extmark::pill(5..9, Style::new().bg(Color::Blue))];
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        Editor::new(&model)
            .scroll((1, 2))
            .extmarks(&marks)
            .render(buf.area(), &mut buf);
        // Row 1 ("row1") is the first visible row; cols 2.. → "w1".
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'w');
        assert_eq!(bg(&buf, 0, 0), Color::Blue); // 'w' (flat 7)
        assert_eq!(bg(&buf, 1, 0), Color::Blue); // '1' (flat 8)
    }

    #[test]
    fn an_extmark_over_a_multibyte_line_is_char_indexed() {
        let model = TextArea::from_value("é日x\nynext");
        let marks = [Extmark::new(1..2, Style::new().bg(Color::Red))];
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        Editor::new(&model)
            .extmarks(&marks)
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '日');
        assert_eq!(bg(&buf, 0, 0), Color::Reset); // 'é'
        assert_eq!(bg(&buf, 1, 0), Color::Red); // '日'
        assert_eq!(bg(&buf, 2, 0), Color::Reset); // 'x'
    }

    #[test]
    fn zero_area_with_extmarks_is_a_no_op() {
        let model = TextArea::from_value("hello\nworld");
        let marks = [Extmark::new(0..11, Style::new().bg(Color::Red))];
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        Editor::new(&model)
            .extmarks(&marks)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.bg == Color::Reset));
    }

    #[test]
    fn an_empty_model_with_extmarks_leaves_the_placeholder_untinted() {
        let model = TextArea::new();
        let marks = [Extmark::new(0..5, Style::new().bg(Color::Red))];
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        Editor::new(&model)
            .placeholder("type…")
            .extmarks(&marks)
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 't');
        for y in 0..2 {
            for x in 0..6 {
                assert_eq!(bg(&buf, x, y), Color::Reset);
            }
        }
    }

    #[test]
    fn extmarks_project_independently_of_focus_and_compose_with_the_block() {
        let model = TextArea::from_value("hi");
        let marks = [Extmark::new(0..2, Style::new().bg(Color::Red))];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        // Unfocused + framed: the pill still renders inside the block's inner.
        Editor::new(&model)
            .block(Block::bordered())
            .extmarks(&marks)
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'h');
        assert_eq!(bg(&buf, 1, 1), Color::Red);
        assert_eq!(bg(&buf, 2, 1), Color::Red);
    }

    // --- gap G: the syntax overlay --------------------------------------

    fn fg(buf: &Buffer, x: u16, y: u16) -> Color {
        buf.get(Position::new(x, y)).unwrap().fg
    }

    #[test]
    fn syntax_overlay_patches_by_flattened_char_index() {
        // "ab\ncd": flat a=0 b=1 '\n'=2 c=3 d=4. Colour 'b' (flat 1) and
        // 'c' (flat 3); the '\n' slot (flat 2) is unused.
        let model = TextArea::from_value("ab\ncd");
        let red = Style::new().fg(Color::Red);
        let none = Style::new();
        let overlay = [none, red, none, red, none];
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        Editor::new(&model)
            .syntax(&overlay)
            .render(buf.area(), &mut buf);
        assert_eq!(fg(&buf, 0, 0), Color::Reset); // 'a'
        assert_eq!(fg(&buf, 1, 0), Color::Red); // 'b'
        assert_eq!(fg(&buf, 0, 1), Color::Red); // 'c'  (flat 3)
        assert_eq!(fg(&buf, 1, 1), Color::Reset); // 'd'
    }

    #[test]
    fn syntax_cascades_under_an_extmark_and_under_the_selection() {
        // Same cell carries syntax (fg red) + an extmark (bg blue): both show
        // because the extmark patches *over* the syntax (cascade order).
        let model = TextArea::from_value("abcdef");
        let red = Style::new().fg(Color::Red);
        let overlay = [red, red, red, red, red, red];
        let marks = [Extmark::new(0..6, Style::new().bg(Color::Blue))];
        let mut sel = DocSelection::new();
        sel.start((0, 2), rstui_core::SelKind::Char);
        sel.extend((0, 4)); // selects cols 2,3
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Editor::new(&model)
            .syntax(&overlay)
            .extmarks(&marks)
            .selection(&sel)
            .selection_style(Style::new().add_modifier(Modifier::REVERSED))
            .render(buf.area(), &mut buf);
        // Col 0: syntax fg + extmark bg, no selection.
        let c0 = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(c0.fg, Color::Red);
        assert_eq!(c0.bg, Color::Blue);
        assert!(!c0.modifier.contains(Modifier::REVERSED));
        // Col 2: syntax fg + extmark bg + selection on top (all three).
        let c2 = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(c2.fg, Color::Red);
        assert_eq!(c2.bg, Color::Blue);
        assert!(c2.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn an_empty_syntax_overlay_is_byte_identical() {
        let model = TextArea::from_value("abc\nde\nf");
        let empty: [Style; 0] = [];
        assert_eq!(
            lines(Editor::new(&model).syntax(&empty), 4, 4),
            "abc \nde  \nf   \n    \n"
        );
        // A too-short overlay is total: the uncovered tail is just unstyled.
        let one = [Style::new().fg(Color::Red)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Editor::new(&model)
            .syntax(&one)
            .render(buf.area(), &mut buf);
        assert_eq!(fg(&buf, 0, 0), Color::Red);
        assert_eq!(fg(&buf, 1, 0), Color::Reset);
    }

    // --- gap F: the logical selection projection ------------------------

    #[test]
    fn selection_highlights_a_charwise_span_end_exclusive() {
        let model = TextArea::from_value("hello\nworld");
        let mut sel = DocSelection::new();
        sel.start((0, 1), rstui_core::SelKind::Char);
        sel.extend((1, 2)); // "ello\nwo" — end col 2 on row 1 EXCLUSIVE
        let rev = Style::new().add_modifier(Modifier::REVERSED);
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        Editor::new(&model)
            .selection(&sel)
            .selection_style(rev)
            .render(buf.area(), &mut buf);
        let on = |x, y| {
            buf.get(Position::new(x, y))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        };
        assert!(!on(0, 0)); // 'h' before the anchor
        assert!(on(1, 0)); // 'e' (anchor, inclusive)
        assert!(on(4, 0)); // 'o' end of row 0
        assert!(on(0, 1)); // 'w' start of row 1
        assert!(on(1, 1)); // 'o' (col 1)
        assert!(!on(2, 1)); // 'r' (col 2 — the caret cell, excluded)
    }

    #[test]
    fn selection_linewise_covers_whole_rows_and_blockwise_is_a_rectangle() {
        let model = TextArea::from_value("aaaa\nbbbb\ncccc");
        let rev = Style::new().add_modifier(Modifier::REVERSED);
        // Linewise: rows 0..=1 fully, every column.
        let mut line_sel = DocSelection::new();
        line_sel.start((0, 99), rstui_core::SelKind::Line);
        line_sel.extend((1, 0));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        Editor::new(&model)
            .selection(&line_sel)
            .selection_style(rev)
            .render(buf.area(), &mut buf);
        let on = |b: &Buffer, x, y| {
            b.get(Position::new(x, y))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        };
        for x in 0..4 {
            assert!(on(&buf, x, 0));
            assert!(on(&buf, x, 1));
            assert!(!on(&buf, x, 2)); // row 2 outside
        }
        // Blockwise: the (row,col) rectangle cols 1..=2 over rows 0..=2.
        let mut blk = DocSelection::new();
        blk.start((0, 1), rstui_core::SelKind::Block);
        blk.extend((2, 2));
        let mut buf2 = Buffer::empty(Rect::new(0, 0, 4, 3));
        Editor::new(&model)
            .selection(&blk)
            .selection_style(rev)
            .render(buf2.area(), &mut buf2);
        for y in 0..3 {
            assert!(!on(&buf2, 0, y)); // left of the band
            assert!(on(&buf2, 1, y)); // in the band
            assert!(on(&buf2, 2, y));
            assert!(!on(&buf2, 3, y)); // right of the band
        }
    }

    #[test]
    fn no_selection_set_is_unchanged_and_the_caret_still_wins() {
        let model = TextArea::from_value("hi\nyo");
        // No `.selection(..)` at all: byte-identical.
        assert_eq!(lines(Editor::new(&model), 3, 2), "hi \nyo \n");
        // The selection cascades *under* the caret: a selected caret cell is
        // still the caret (reverse from the caret, plus the selection bg).
        let mut sel = DocSelection::new();
        sel.start((0, 0), rstui_core::SelKind::Char);
        sel.extend((0, 2)); // selects cols 0,1
        let mut model2 = TextArea::from_value("hi\nyo");
        model2.set_cursor(0, 0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        Editor::new(&model2)
            .focused(true)
            .selection(&sel)
            .selection_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        let caret = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(caret.symbol, 'h');
        assert_eq!(caret.bg, Color::Blue); // selection cascades under…
        assert!(caret.modifier.contains(Modifier::REVERSED)); // …the caret
    }

    // --- gap D: tab-width expansion -------------------------------------

    #[test]
    fn a_tab_expands_to_the_next_tab_stop_and_default_is_four() {
        // "\tx": tab → 4 spaces (default), 'x' at column 4.
        let model = TextArea::from_value("\tx");
        assert_eq!(lines(Editor::new(&model), 6, 1), "    x \n");
        // tab_width 2 → 2 spaces.
        assert_eq!(lines(Editor::new(&model).tab_width(2), 6, 1), "  x   \n");
        // "ab\tc": two chars then a tab → pad to the next multiple of 4
        // (cols 2,3), so 'c' lands at column 4.
        let m2 = TextArea::from_value("ab\tc");
        assert_eq!(lines(Editor::new(&m2), 6, 1), "ab  c \n");
        // A tab_width of 0 is treated as 1 (total, no divide-by-zero).
        let m3 = TextArea::from_value("\tx");
        assert_eq!(lines(Editor::new(&m3).tab_width(0), 4, 1), " x  \n");
    }

    #[test]
    fn the_caret_over_a_tab_sits_at_its_first_expanded_cell() {
        let mut model = TextArea::from_value("\tx"); // tab is char 0
        model.set_cursor(0, 0); // caret on the tab
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Editor::new(&model)
            .focused(true)
            .render(buf.area(), &mut buf);
        // The caret is the FIRST expanded cell (column 0), a reversed space.
        let c0 = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(c0.symbol, ' ');
        assert!(c0.modifier.contains(Modifier::REVERSED));
        // The other three pad cells are not the caret.
        for x in 1..4 {
            assert!(
                !buf.get(Position::new(x, 0))
                    .unwrap()
                    .modifier
                    .contains(Modifier::REVERSED)
            );
        }
        // Caret on 'x' (char 1) sits at expanded column 4.
        model.set_cursor(0, 1);
        let mut buf2 = Buffer::empty(Rect::new(0, 0, 6, 1));
        Editor::new(&model)
            .focused(true)
            .render(buf2.area(), &mut buf2);
        let cx = buf2.get(Position::new(4, 0)).unwrap();
        assert_eq!(cx.symbol, 'x');
        assert!(cx.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn cell_to_doc_stays_consistent_with_expanded_tab_columns() {
        let model = TextArea::from_value("\tx\nab\tc");
        let ed = Editor::new(&model).tab_width(4);
        let area = Rect::new(0, 0, 8, 2);
        // Row 0: a click on any of the four tab cells → char 0; 'x' is char 1.
        for x in 0..4 {
            assert_eq!(ed.cell_to_doc(area, Position::new(x, 0)), Some((0, 0)));
        }
        assert_eq!(ed.cell_to_doc(area, Position::new(4, 0)), Some((0, 1)));
        // Click far past the end of row 0 → end of that line (char 2).
        assert_eq!(ed.cell_to_doc(area, Position::new(7, 0)), Some((0, 2)));
        // Row 1 "ab\tc": cols 0,1 = a,b; cols 2,3 = the tab (char 2);
        // col 4 = 'c' (char 3).
        assert_eq!(ed.cell_to_doc(area, Position::new(0, 1)), Some((1, 0)));
        assert_eq!(ed.cell_to_doc(area, Position::new(2, 1)), Some((1, 2)));
        assert_eq!(ed.cell_to_doc(area, Position::new(3, 1)), Some((1, 2)));
        assert_eq!(ed.cell_to_doc(area, Position::new(4, 1)), Some((1, 3)));
    }

    #[test]
    fn content_height_measures_the_expanded_tab_width() {
        // "\t\t" with tab_width 4 expands to 8 cells → ceil(8/4) = 2 rows.
        let model = TextArea::from_value("\t\t");
        assert_eq!(Editor::new(&model).tab_width(4).content_height(4), 2);
        // No tabs: byte-identical to the pre-tab measurement.
        let plain = TextArea::from_value("abc\n123456789\n");
        assert_eq!(Editor::new(&plain).content_height(4), 5);
    }

    // --- gap C: soft-wrap projection ------------------------------------

    #[test]
    fn wrap_reflows_a_long_line_into_visual_rows() {
        // 6-char line wrapped at width 4 → rows "abcd" / "ef".
        let model = TextArea::from_value("abcdef");
        assert_eq!(
            lines(Editor::new(&model).wrap(true), 4, 3),
            "abcd\nef  \n    \n"
        );
        // Off (the default) is byte-identical: the line is clipped at width.
        assert_eq!(lines(Editor::new(&model), 4, 3), "abcd\n    \n    \n");
    }

    #[test]
    fn wrap_counts_visual_rows_for_the_vertical_scroll() {
        // "abcdef" wraps to 3 rows at width 2: "ab","cd","ef". A vertical
        // scroll of 1 visual row starts at "cd".
        let model = TextArea::from_value("abcdef\nZZ");
        assert_eq!(
            lines(Editor::new(&model).wrap(true).scroll((1, 0)), 2, 3),
            "cd\nef\nZZ\n"
        );
    }

    #[test]
    fn the_caret_follows_through_the_wrap() {
        // "abcdef" at width 4 wraps to "abcd"/"ef". The caret on 'f' (char 5)
        // is on the SECOND visual row, column 1.
        let mut model = TextArea::from_value("abcdef");
        model.set_cursor(0, 5);
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Editor::new(&model)
            .wrap(true)
            .focused(true)
            .render(buf.area(), &mut buf);
        let caret = buf.get(Position::new(1, 1)).unwrap();
        assert_eq!(caret.symbol, 'f');
        assert!(caret.modifier.contains(Modifier::REVERSED));
        // Caret on 'a' (char 0) is the first cell of the first visual row.
        model.set_cursor(0, 0);
        let mut b2 = Buffer::empty(Rect::new(0, 0, 4, 2));
        Editor::new(&model)
            .wrap(true)
            .focused(true)
            .render(b2.area(), &mut b2);
        assert!(
            b2.get(Position::new(0, 0))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn cell_to_doc_inverts_the_wrap() {
        let model = TextArea::from_value("abcdef\nZZ");
        let ed = Editor::new(&model).wrap(true);
        let area = Rect::new(0, 0, 4, 4);
        // Visual rows: 0="abcd", 1="ef", 2="ZZ".
        assert_eq!(ed.cell_to_doc(area, Position::new(0, 0)), Some((0, 0))); // 'a'
        assert_eq!(ed.cell_to_doc(area, Position::new(3, 0)), Some((0, 3))); // 'd'
        assert_eq!(ed.cell_to_doc(area, Position::new(0, 1)), Some((0, 4))); // 'e'
        assert_eq!(ed.cell_to_doc(area, Position::new(1, 1)), Some((0, 5))); // 'f'
        assert_eq!(ed.cell_to_doc(area, Position::new(0, 2)), Some((1, 0))); // 'Z'
        // Click past the end of a wrapped visual row → that line's end.
        assert_eq!(ed.cell_to_doc(area, Position::new(3, 1)), Some((0, 6)));
    }

    #[test]
    fn wrap_composes_with_tab_expansion_and_an_extmark() {
        // "\tab" with tab_width 4 expands to 6 cells; wrapped at width 4 →
        // row 0 = "    " (the tab), row 1 = "ab".
        let model = TextArea::from_value("\tab");
        // flat: tab=0, a=1, b=2. Pill over 'a' (flat 1).
        let marks = [Extmark::new(1..2, Style::new().bg(Color::Red))];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Editor::new(&model)
            .wrap(true)
            .tab_width(4)
            .extmarks(&marks)
            .render(buf.area(), &mut buf);
        // Row 0 is the four tab pad cells.
        for x in 0..4 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().symbol, ' ');
        }
        // Row 1: 'a' (extmark red bg), 'b' (plain).
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'a');
        assert_eq!(bg(&buf, 0, 1), Color::Red);
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'b');
        assert_eq!(bg(&buf, 1, 1), Color::Reset);
    }

    #[test]
    fn wrap_is_total_for_degenerate_inputs() {
        // A zero-width area: no panic, nothing drawn.
        let model = TextArea::from_value("abcdef");
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        Editor::new(&model)
            .wrap(true)
            .focused(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        // An empty document with wrap + a caret: the placeholder path, total.
        let empty = TextArea::new();
        let mut b2 = Buffer::empty(Rect::new(0, 0, 4, 2));
        Editor::new(&empty)
            .wrap(true)
            .focused(true)
            .placeholder("hi")
            .render(b2.area(), &mut b2);
        assert_eq!(b2.get(Position::new(0, 0)).unwrap().symbol, 'h');
        assert!(
            b2.get(Position::new(0, 0))
                .unwrap()
                .modifier
                .contains(Modifier::REVERSED)
        );
        // The caret far past the end of a wrapped line never panics. "abcd"
        // exactly fills width 4; a caret one past the end (char 4) is the
        // documented off-edge clamp — drawn or not, but always total.
        let mut m3 = TextArea::from_value("abcd");
        m3.set_cursor(0, 4);
        let mut b3 = Buffer::empty(Rect::new(0, 0, 4, 3));
        Editor::new(&m3)
            .wrap(true)
            .focused(true)
            .render(b3.area(), &mut b3);
        assert_eq!(b3.get(Position::new(0, 0)).unwrap().symbol, 'a');
        // A scroll past every visual row is blank, not a panic.
        assert_eq!(
            lines(Editor::new(&model).wrap(true).scroll((99, 0)), 4, 2),
            "    \n    \n"
        );
    }

    #[test]
    fn cell_to_doc_clamps_a_click_below_the_last_visual_row() {
        // Unwrapped: a click past the last logical line clamps to its end.
        let model = TextArea::from_value("ab\ncde");
        let ed = Editor::new(&model);
        let area = Rect::new(0, 0, 6, 5);
        assert_eq!(ed.cell_to_doc(area, Position::new(0, 4)), Some((1, 3)));
        // Wrapped likewise: below the last visual row → document end.
        let wed = Editor::new(&model).wrap(true);
        assert_eq!(wed.cell_to_doc(area, Position::new(0, 4)), Some((1, 3)));
    }
}
