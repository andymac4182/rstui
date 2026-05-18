//! Caller-owned *logical* text selection over a document's `(row, col)`
//! character positions.
//!
//! [`DocSelection`] is the **logical dual of
//! [`Selection`](crate::selection::Selection)**: where `Selection` is a
//! row-major span over the *rendered content buffer's* CELL coordinates (what
//! the user dragged the mouse over, read back by
//! [`selected_text`](crate::selection::selected_text) for clipboard copy),
//! `DocSelection` is a span over a *document's logical character positions* —
//! exactly the `(row, col)` pairs a [`TextArea`](crate::text_area::TextArea)
//! cursor lives in (`row` a logical-line index, `col` a **character** index
//! into that line, never a byte offset). `DocSelection` is to a
//! [`TextArea`](crate::text_area::TextArea) document what
//! [`Selection`](crate::selection::Selection) is to the rendered
//! [`Buffer`](crate::buffer::Buffer): the same pure-value, totally-defined,
//! caller-owned bookkeeping, one logical level up.
//!
//! Like [`Selection`](crate::selection::Selection),
//! [`ScrollState`](crate::scroll::ScrollState),
//! [`TextArea`](crate::text_area::TextArea) and
//! [`FocusRing`](crate::focus::FocusRing), it is a pure value type that lives
//! as a *field in the application's model*, mutated only by `update`, and read
//! by the pure `view`. The reducer drives it directly: a **Shift+motion key**
//! (or a text-area mouse-down) [`start`](DocSelection::start)s it at the
//! caret, each subsequent Shift+motion / mouse-drag
//! [`extend`](DocSelection::extend)s it to the new caret, and a **plain
//! (un-shifted) move** [`clear`](DocSelection::clear)s it — the universal
//! editor selection gesture. An `Editor` widget is a **pure projection** of
//! one: at render time it asks [`contains`](DocSelection::contains) per
//! document character and applies a highlight
//! [`Style`](crate::style::Style); it never owns or mutates the selection.
//! The "copy"/"cut" command reads the spanned characters back out of the
//! caller's own [`TextArea`](crate::text_area::TextArea) via
//! [`range`](DocSelection::range). Per
//! [ADR 0012](https://github.com/andymac4182/rstui/blob/main/docs/adr/0012-widget-composition-and-layout-model.md)
//! §P1 this split is *forced* by rstui's pure-`view` / immediate-mode design:
//! a widget is handed only a [`Buffer`](crate::buffer::Buffer) at render time
//! with no retained tree, so it can neither own the selection anchor nor
//! mutate it on a key event — the reducer owns it, exactly as it owns the
//! scroll, the focus, and the edited document
//! ([ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)).
//!
//! Like [`selection`](crate::selection) and
//! [`text_area`](crate::text_area), this module is **optional**: an app may
//! keep a bare `Option<((usize, usize), (usize, usize))>` of its own and never
//! name a type from here. `DocSelection` exists only to turn the
//! anchor/active normalisation, the three selection *kinds*, and the
//! per-character membership test every editor re-derives — and routinely gets
//! wrong at the edges (an unordered anchor/active pair, an off-by-one at the
//! caret cell, a reversed block-selection rectangle, an
//! [`extend`](DocSelection::extend) with no [`start`](DocSelection::start)) —
//! into one reusable, panic-free primitive:
//!
//! - Coordinates are **logical document positions** `(row, col)` — the same
//!   pair a [`TextArea`](crate::text_area::TextArea) cursor uses, `col` a
//!   character index into the logical line, never pixels or byte offsets. The
//!   app feeds it caret positions it already computes; the selection stays
//!   purely in document space and is reconciled against the real document only
//!   when the caller reads the characters back.
//! - The selection has a [`SelKind`]: **`Char`** (a row-major character
//!   stream, the `Selection` semantics one level up — the usual Shift+arrow
//!   selection), **`Line`** (whole logical rows, the linewise / Shift+Up-Down
//!   "line" selection, columns not meaningful), or **`Block`** (a rectangular
//!   `(row, col)` region between the two corners — the column-block / "box"
//!   selection). Anchor and active are kept together — both set by
//!   [`start`](DocSelection::start), `active` moved by
//!   [`extend`](DocSelection::extend), both dropped by
//!   [`clear`](DocSelection::clear) — so the selection is either fully present
//!   or fully absent and can never be half-formed, exactly as
//!   [`Selection`](crate::selection::Selection)'s `span` is.
//! - Every method is **total** — no input, including an
//!   [`extend`](DocSelection::extend) before a [`start`](DocSelection::start),
//!   a reversed (active-before-anchor) drag, or `usize::MAX` coordinates, can
//!   panic; [`range`](DocSelection::range) always returns
//!   `start <= end` in row-major order (the iter-25 "a pure projection must be
//!   total" rule, the same guarantee
//!   [`Selection`](crate::selection::Selection),
//!   [`ScrollState`](crate::scroll::ScrollState) and
//!   [`FocusRing`](crate::focus::FocusRing) give).
//!
//! This is **logical/document** selection (what the caret swept over in a
//! [`TextArea`](crate::text_area::TextArea)) and is the dual of — not a
//! replacement for — render-space [`Selection`](crate::selection::Selection)
//! (what the mouse dragged over the rendered buffer for a plain copy).
//!
//! # Example
//!
//! ```
//! use rstui_core::doc_selection::{DocSelection, SelKind};
//!
//! // Three logical lines the caller's `TextArea` holds.
//! //   row 0: "hello world"
//! //   row 1: "second line"
//! //   row 2: "third  here"
//!
//! // The app stores a `DocSelection` in its model. On Shift+Right it calls
//! // `start` at the caret then `extend` as the caret keeps moving; a plain
//! // arrow press calls `clear`.
//! let mut sel = DocSelection::new();
//! assert!(sel.is_empty());
//! assert_eq!(sel.kind(), SelKind::Char); // Char when empty
//!
//! sel.start((0, 6), SelKind::Char); // the 'w' of "world"
//! sel.extend((1, 5));               // through "secon" on the next row
//! assert!(!sel.is_empty());
//!
//! // `range()` normalises to (start, end) in row-major order whichever way
//! // the caret swept.
//! assert_eq!(sel.range(), Some(((0, 6), (1, 5))));
//!
//! // An `Editor` projects the selection by asking, per character, `contains`.
//! // Char = row-major stream, end EXCLUSIVE at the caret cell:
//! assert!(sel.contains((0, 8)));  // start row, after the anchor
//! assert!(sel.contains((1, 0)));  // end row, before the active caret
//! assert!(!sel.contains((1, 5))); // the caret cell itself is excluded
//! assert!(!sel.contains((0, 2))); // before the anchor on row 0
//!
//! // A linewise selection ignores columns — whole rows are covered.
//! sel.start((0, 99), SelKind::Line);
//! sel.extend((2, 0));
//! assert!(sel.contains((1, 0)));   // any column on a covered row
//! assert!(sel.contains((2, 999))); // even far past the line's end
//! assert!(!sel.contains((3, 0)));  // a row outside the range
//!
//! // A blockwise selection is the (row, col) rectangle of the two corners.
//! sel.start((0, 2), SelKind::Block);
//! sel.extend((2, 5));
//! assert!(sel.contains((1, 3)));   // inside the column band on a spanned row
//! assert!(!sel.contains((1, 9)));  // same row, but right of the band
//! assert!(sel.contains((0, 2)));   // the anchor corner is included
//!
//! // Every input is total: clearing empties it and an empty selection's
//! // `range()` is `None` and `contains` is always `false`, never a panic.
//! sel.clear();
//! assert!(sel.is_empty());
//! assert_eq!(sel.range(), None);
//! assert!(!sel.contains((0, 0)));
//! ```

/// What a [`DocSelection`] selects: a character stream, whole lines, or a
/// rectangle.
///
/// This is the document-selection mode the editor is in — the logical-level
/// counterpart of the deliberately-deferred "block selection is a separate
/// mode, not a flag" note on
/// [`Selection`](crate::selection::Selection). Here all three modes *are*
/// first-class because a code editor genuinely needs linewise (Shift+Up/Down
/// "select line") and blockwise (column / "box" edit) selection alongside the
/// ordinary charwise one; the kind is fixed by
/// [`start`](DocSelection::start) and is invariant for the life of one drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelKind {
    /// A **row-major character stream** (the usual Shift+arrow selection):
    /// from the anchor through reading order to the caret, identical in shape
    /// to [`Selection`](crate::selection::Selection) one logical level up.
    Char,
    /// **Whole logical rows** (the linewise / Shift+Up-Down "select line"
    /// selection): every row in the anchor↔active row range is fully selected
    /// and the columns are not meaningful.
    Line,
    /// A **rectangular `(row, col)` region** (the column-block / "box"
    /// selection): the axis-aligned rectangle whose opposite corners are the
    /// anchor and the active position.
    Block,
}

/// A logical text selection over a document's `(row, col)` character
/// positions, caller-owned.
///
/// `DocSelection` is a **pure value type** designed to live as a field in the
/// application's model (it derives [`Default`] so it drops into a
/// `#[derive(Default)]` model as an empty selection — `new() == default()`,
/// like [`Selection`](crate::selection::Selection) and
/// [`TextArea`](crate::text_area::TextArea), since "nothing selected" is the
/// only sensible inert state). It owns *no* terminal, runtime, or widget
/// state: `update` mutates it in response to the Shift+motion / mouse messages
/// the app maps, and the pure `view` only reads
/// [`contains`](Self::contains) / [`range`](Self::range) to project it. The
/// framework never touches it.
///
/// The selection is stored as an `anchor`/`active` pair plus a [`SelKind`] —
/// the document position where selecting began, the position it currently
/// reaches, and which of the three selection modes is in force. The pair is
/// kept together (both set by [`start`](Self::start), `active` moved by
/// [`extend`](Self::extend), both cleared by [`clear`](Self::clear)), so the
/// selection is either fully present or fully absent and can never be
/// half-formed — exactly as [`Selection`](crate::selection::Selection)'s
/// `span` is. [`range`](Self::range) normalises the pair to row-major
/// `(start, end)` so the projection never has to care which way the caret
/// swept.
///
/// The selected region depends on the [`SelKind`]:
/// - [`Char`](SelKind::Char): a **row-major character stream** — after
///   ordering, a position is selected iff it is at or after `start` and
///   strictly before `end` in reading order (the start position **inclusive**,
///   the caret cell at `end` **exclusive**; see [`contains`](Self::contains)
///   for the rationale and how this relates to
///   [`Selection`](crate::selection::Selection)'s inclusive choice).
/// - [`Line`](SelKind::Line): every logical **row** between the anchor and
///   active rows (inclusive), columns irrelevant.
/// - [`Block`](SelKind::Block): the axis-aligned **rectangle** between the two
///   corners, both the row range and the column range inclusive.
///
/// Every method is **total**: arbitrary input — an [`extend`](Self::extend)
/// with no [`start`](Self::start), a reversed (active-before-anchor) drag,
/// `usize::MAX` positions — is well-defined and never panics, and
/// [`range`](Self::range) always yields `start <= end` in row-major
/// `(row, col)` order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocSelection {
    /// `Some((anchor, active))` once selecting has begun: `anchor` is the
    /// document position [`start`](Self::start) was called with, `active` the
    /// position the latest [`extend`](Self::extend) reached. `None` is the
    /// empty selection. The pair is never partially set, which is what keeps
    /// every accessor total.
    span: Option<(Anchored, Anchored)>,
    /// The selection mode fixed by the most recent [`start`](Self::start).
    /// Retained even while [`span`](Self::span) is `None` so a stale read
    /// before any selection has a defined answer ([`Char`](SelKind::Char), the
    /// inert default).
    kind: SelKind,
}

/// A document position: `(row, col)` where `row` is a logical-line index and
/// `col` a **character** index into that line — the same pair a
/// [`TextArea`](crate::text_area::TextArea) cursor uses.
type Anchored = (usize, usize);

impl Default for SelKind {
    /// [`Char`](SelKind::Char) — the ordinary character-stream selection, the
    /// only sensible inert mode (so `DocSelection::default()` is a fully
    /// inert, `Char`-kind, empty selection).
    fn default() -> Self {
        SelKind::Char
    }
}

impl DocSelection {
    /// The empty selection (nothing selected), of kind
    /// [`Char`](SelKind::Char).
    ///
    /// Identical to [`DocSelection::default`]: like
    /// [`Selection`](crate::selection::Selection) and
    /// [`TextArea`](crate::text_area::TextArea) there is no useful non-default
    /// starting state for a selection, so `new()` and `default()` agree.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether there is no active selection.
    ///
    /// `true` for a fresh [`new`](Self::new) / after [`clear`](Self::clear). A
    /// [`start`](Self::start)ed but not-yet-[`extend`](Self::extend)ed
    /// selection is **not** empty for [`Line`](SelKind::Line) /
    /// [`Block`](SelKind::Block) (it is a one-row / one-cell selection); a
    /// zero-width [`Char`](SelKind::Char) selection — `active == anchor`,
    /// nothing between an inclusive start and an exclusive end — is
    /// non-`is_empty` (selecting *has begun*) but [`contains`](Self::contains)
    /// nothing yet, exactly mirroring how
    /// [`Selection`](crate::selection::Selection) treats a bare
    /// [`start`](Self::start). The app decides whether a given gesture should
    /// [`start`](Self::start) or [`clear`](Self::clear).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.span.is_none()
    }

    /// The selection mode. [`Char`](SelKind::Char) when empty (the inert
    /// default — see [`SelKind::default`]); otherwise whatever the most recent
    /// [`start`](Self::start) fixed.
    #[must_use]
    pub fn kind(&self) -> SelKind {
        self.kind
    }

    /// Begins a selection of mode `kind` at document position `at` (sets both
    /// anchor and active to it).
    ///
    /// This is the Shift+motion-start / text-area mouse-down path: `at` is the
    /// caret's `(row, col)` document position. Any prior selection (and its
    /// kind) is replaced. `at` is stored as-is (no clamping — the selection is
    /// reconciled against the real document only when the caller reads the
    /// characters back, exactly as a raw scroll offset is reconciled at
    /// render), so any coordinate is safe.
    pub fn start(&mut self, at: (usize, usize), kind: SelKind) {
        self.span = Some((at, at));
        self.kind = kind;
    }

    /// Extends an in-progress selection so its active end now reaches `to`
    /// (keeps the anchor and the kind fixed).
    ///
    /// This is the Shift+motion-continue / mouse-drag path. If there is no
    /// selection yet (no [`start`](Self::start) since the last
    /// [`clear`](Self::clear)) this is a deliberate, total **no-op** — `extend`
    /// only ever extends an existing selection; the app must `start` on the
    /// first Shift+motion. `to` is stored raw (reconciled against the document
    /// when read back), so any coordinate is safe.
    pub fn extend(&mut self, to: (usize, usize)) {
        if let Some((anchor, _)) = self.span {
            self.span = Some((anchor, to));
        }
    }

    /// Clears the selection back to empty (e.g. a plain un-shifted arrow key,
    /// Esc, or the document changing under it).
    ///
    /// The [`kind`](Self::kind) reverts to the inert
    /// [`Char`](SelKind::Char) default; the next [`start`](Self::start)
    /// re-establishes one.
    pub fn clear(&mut self) {
        self.span = None;
        self.kind = SelKind::default();
    }

    /// The anchor — the document position selecting began at, or `None` when
    /// empty.
    #[must_use]
    pub fn anchor(&self) -> Option<(usize, usize)> {
        self.span.map(|(a, _)| a)
    }

    /// The active end — the document position the latest
    /// [`extend`](Self::extend) reached (equal to the anchor right after
    /// [`start`](Self::start)), or `None` when empty.
    #[must_use]
    pub fn active(&self) -> Option<(usize, usize)> {
        self.span.map(|(_, b)| b)
    }

    /// The selection normalised to `(start, end)` in **row-major order**
    /// (`start <= end`), or `None` when empty.
    ///
    /// "Row-major order" means the first element is the endpoint with the
    /// smaller `(row, col)` and the second the larger, regardless of which way
    /// the caret swept (up-left or down-right). The post-condition
    /// `start.row < end.row || (start.row == end.row && start.col <= end.col)`
    /// always holds — the single definition [`contains`](Self::contains)
    /// builds on, so the span semantics cannot drift. For
    /// [`Line`](SelKind::Line) the columns are **not meaningful** (the whole
    /// rows from `start.0` to `end.0` are selected); they are still ordered so
    /// the row bounds are usable directly. For [`Block`](SelKind::Block) the
    /// returned pair is the row-major *ordering of the anchor/active pair*, not
    /// the rectangle's min/max corner — use [`contains`](Self::contains) for
    /// rectangle membership (which normalises both axes independently).
    #[must_use]
    pub fn range(&self) -> Option<((usize, usize), (usize, usize))> {
        let (a, b) = self.span?;
        if a <= b { Some((a, b)) } else { Some((b, a)) }
    }

    /// Whether document position `(row, col)` is inside the selection — the
    /// per-character projection an `Editor` widget reads at render to decide
    /// whether to apply a highlight style. Always `false` when empty.
    ///
    /// The region depends on [`kind`](Self::kind), with the anchor/active pair
    /// ordered to `(start, end)`:
    ///
    /// - [`Char`](SelKind::Char) — a **row-major character stream**: selected
    ///   iff at or after `start` and **strictly before** `end` in reading
    ///   order. Concretely, with `(start, end)` row-major: rows outside
    ///   `start.row..=end.row` are out; on a single-row selection
    ///   `start.col <= col < end.col`; on the start row of a multi-row
    ///   selection `col >= start.col`; on the end row `col < end.col`; a row
    ///   strictly between is fully in.
    ///
    ///   **Inclusivity choice (stated, per the spec):** the start is
    ///   **inclusive** and the end (the caret cell) is **EXCLUSIVE**. This is
    ///   the *opposite* end-cell choice to
    ///   [`Selection`](crate::selection::Selection), which is inclusive at
    ///   both ends — and deliberately so. `Selection` models a *mouse drag
    ///   over rendered cells*, where the cell physically under the cursor is
    ///   itself part of what was swept (every terminal highlights it).
    ///   `DocSelection` models a *caret-driven logical selection*, where
    ///   `active` is the **caret position** — the gap *before* a character,
    ///   not a character — so the character the caret sits on is the *next,
    ///   not-yet-selected* one (Shift+Right from before `a` to after `a`
    ///   selects exactly `a`: `[col, col+1)`). Making `end` exclusive is what
    ///   makes a zero-width caret selection (`active == anchor`) correctly
    ///   contain nothing, and matches every editor's Shift+arrow behaviour.
    ///   The shared row-major skeleton is identical to `Selection`'s; only the
    ///   end-cell boundary differs, by design.
    ///
    /// - [`Line`](SelKind::Line) — **whole rows**: selected iff
    ///   `start.row <= row <= end.row`; `col` is ignored entirely (any column
    ///   on a covered row, however large, is in).
    ///
    /// - [`Block`](SelKind::Block) — the **rectangle** of the two corners:
    ///   selected iff `row` is within the anchor/active row range *and* `col`
    ///   is within their column range, each range normalised independently
    ///   (both bounds inclusive), so a reversed or column-crossed drag still
    ///   yields the natural axis-aligned box.
    ///
    /// This is a pure description in document coordinates and is intentionally
    /// **unbounded** (it has no notion of a document size — an off-document
    /// position simply tests the predicate); a projecting widget clips
    /// naturally because it only ever consults positions that exist. The
    /// widget never mutates anything — it reads this predicate, exactly as a
    /// `ScrollView` reads
    /// [`ScrollState::offset`](crate::scroll::ScrollState::offset).
    #[must_use]
    pub fn contains(&self, pos: (usize, usize)) -> bool {
        let Some((a, b)) = self.span else {
            return false;
        };
        let (row, col) = pos;
        match self.kind {
            SelKind::Line => {
                let lo = a.0.min(b.0);
                let hi = a.0.max(b.0);
                row >= lo && row <= hi
            }
            SelKind::Block => {
                let (r_lo, r_hi) = (a.0.min(b.0), a.0.max(b.0));
                let (c_lo, c_hi) = (a.1.min(b.1), a.1.max(b.1));
                row >= r_lo && row <= r_hi && col >= c_lo && col <= c_hi
            }
            SelKind::Char => {
                // Row-major order, then start-inclusive / end-exclusive.
                let (start, end) = if a <= b { (a, b) } else { (b, a) };
                if row < start.0 || row > end.0 {
                    false
                } else if start.0 == end.0 {
                    col >= start.1 && col < end.1
                } else if row == start.0 {
                    col >= start.1
                } else if row == end.0 {
                    col < end.1
                } else {
                    true
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_default_are_the_empty_char_selection() {
        assert_eq!(DocSelection::new(), DocSelection::default());
        let s = DocSelection::new();
        assert!(s.is_empty());
        assert_eq!(s.kind(), SelKind::Char);
        assert_eq!(s.anchor(), None);
        assert_eq!(s.active(), None);
        assert_eq!(s.range(), None);
        assert!(!s.contains((0, 0)));
    }

    #[test]
    fn start_sets_anchor_active_and_kind() {
        let mut s = DocSelection::new();
        s.start((3, 2), SelKind::Block);
        assert!(!s.is_empty());
        assert_eq!(s.kind(), SelKind::Block);
        assert_eq!(s.anchor(), Some((3, 2)));
        assert_eq!(s.active(), Some((3, 2)));
        assert_eq!(s.range(), Some(((3, 2), (3, 2))));
        // A one-cell block contains exactly its corner.
        assert!(s.contains((3, 2)));
        assert!(!s.contains((3, 3)));
        assert!(!s.contains((4, 2)));
    }

    #[test]
    fn extend_moves_active_keeps_anchor_and_kind() {
        let mut s = DocSelection::new();
        s.start((1, 4), SelKind::Char);
        s.extend((5, 9));
        assert_eq!(s.anchor(), Some((1, 4)));
        assert_eq!(s.active(), Some((5, 9)));
        assert_eq!(s.kind(), SelKind::Char);
        // Re-extending only moves the active end.
        s.extend((2, 0));
        assert_eq!(s.anchor(), Some((1, 4)));
        assert_eq!(s.active(), Some((2, 0)));
    }

    #[test]
    fn extend_before_start_is_a_total_no_op() {
        let mut s = DocSelection::new();
        s.extend((9, 9)); // no start yet
        assert!(s.is_empty());
        assert_eq!(s.range(), None);
        assert_eq!(s.anchor(), None);
        assert_eq!(s.active(), None);
    }

    #[test]
    fn clear_empties_and_resets_kind_to_char() {
        let mut s = DocSelection::new();
        s.start((2, 2), SelKind::Line);
        s.extend((4, 0));
        assert!(!s.is_empty());
        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.kind(), SelKind::Char);
        assert_eq!(s.range(), None);
        assert!(!s.contains((3, 0)));
    }

    #[test]
    fn range_normalises_when_active_is_before_anchor() {
        // Caret swept up-and-left: anchor is after active.
        let mut s = DocSelection::new();
        s.start((4, 7), SelKind::Char);
        s.extend((1, 2));
        assert_eq!(s.range(), Some(((1, 2), (4, 7))));
        // Same-row backward sweep orders by column.
        s.start((3, 8), SelKind::Char);
        s.extend((3, 3));
        assert_eq!(s.range(), Some(((3, 3), (3, 8))));
        // Already forward: unchanged.
        s.start((0, 0), SelKind::Char);
        s.extend((9, 9));
        assert_eq!(s.range(), Some(((0, 0), (9, 9))));
    }

    #[test]
    fn contains_char_is_a_row_major_stream_end_exclusive() {
        // anchor (0,6) → active (2,3): start row from col 6, full middle row,
        // end row up to but EXCLUDING col 3 (the caret cell).
        let mut s = DocSelection::new();
        s.start((0, 6), SelKind::Char);
        s.extend((2, 3));

        // Start row: from the anchor (inclusive) to the unbounded end.
        assert!(!s.contains((0, 5)));
        assert!(s.contains((0, 6)));
        assert!(s.contains((0, 999)));
        // A full intermediate row, including columns left of the anchor.
        assert!(s.contains((1, 0)));
        assert!(s.contains((1, 9999)));
        // End row: from the start of the row up to (not incl.) the caret.
        assert!(s.contains((2, 0)));
        assert!(s.contains((2, 2)));
        assert!(!s.contains((2, 3))); // caret cell excluded
        assert!(!s.contains((2, 4)));
        // Outside the row span entirely.
        assert!(!s.contains((3, 6)));
    }

    #[test]
    fn contains_char_single_row_is_a_half_open_column_range() {
        let mut s = DocSelection::new();
        s.start((5, 2), SelKind::Char);
        s.extend((5, 6));
        // [2, 6): cols 2..=5 in, col 6 (the caret) out.
        for c in 2..=5 {
            assert!(s.contains((5, c)), "col {c} should be in");
        }
        assert!(!s.contains((5, 1)));
        assert!(!s.contains((5, 6)));
        assert!(!s.contains((5, 7)));
        assert!(!s.contains((4, 3)));
        // A zero-width caret selection (active == anchor) contains nothing.
        s.start((5, 4), SelKind::Char);
        assert!(!s.is_empty());
        assert!(!s.contains((5, 4)));
    }

    #[test]
    fn contains_char_normalises_a_backward_drag() {
        // Same positions as the forward case but swept the other way: the
        // membership region is identical.
        let mut s = DocSelection::new();
        s.start((2, 3), SelKind::Char);
        s.extend((0, 6));
        assert!(s.contains((0, 6)));
        assert!(s.contains((1, 0)));
        assert!(s.contains((2, 0)));
        assert!(s.contains((2, 2)));
        assert!(!s.contains((2, 3)));
        assert!(!s.contains((0, 5)));
    }

    #[test]
    fn contains_line_is_whole_rows_ignoring_columns() {
        let mut s = DocSelection::new();
        s.start((1, 50), SelKind::Line);
        s.extend((3, 0));
        for r in 1..=3 {
            // Any column, however large, on a covered row is in.
            assert!(s.contains((r, 0)), "row {r} col 0");
            assert!(s.contains((r, 99_999)), "row {r} col huge");
        }
        assert!(!s.contains((0, 0)));
        assert!(!s.contains((4, 0)));
        // Backward (active row before anchor row) is the same row range.
        s.start((3, 0), SelKind::Line);
        s.extend((1, 0));
        assert!(s.contains((2, 12345)));
        assert!(!s.contains((4, 0)));
    }

    #[test]
    fn contains_block_is_a_rectangle_with_a_real_column_band() {
        // Corners (1,2) and (3,5): rows 1..=3, cols 2..=5. Cells on those
        // rows but OUTSIDE the column band are excluded — the rectangle, not
        // a row-major stream.
        let mut s = DocSelection::new();
        s.start((1, 2), SelKind::Block);
        s.extend((3, 5));
        for r in 1..=3 {
            for c in 2..=5 {
                assert!(s.contains((r, c)), "({r},{c}) inside the box");
            }
            assert!(!s.contains((r, 1)), "({r},1) left of the band");
            assert!(!s.contains((r, 6)), "({r},6) right of the band");
        }
        assert!(!s.contains((0, 3))); // row above the box
        assert!(!s.contains((4, 3))); // row below the box
        // Reversed / column-crossed drag yields the same axis-aligned box.
        s.start((3, 5), SelKind::Block);
        s.extend((1, 2));
        assert!(s.contains((2, 3)));
        assert!(!s.contains((2, 6)));
        s.start((1, 5), SelKind::Block); // anchor col > active col
        s.extend((3, 2));
        assert!(s.contains((2, 4)));
        assert!(!s.contains((2, 1)));
        assert!(!s.contains((2, 6)));
    }

    #[test]
    fn empty_selection_contains_nothing_and_range_is_none() {
        let s = DocSelection::new();
        assert_eq!(s.range(), None);
        assert!(!s.contains((0, 0)));
        assert!(!s.contains((usize::MAX, usize::MAX)));
        // After clearing a real selection, likewise.
        let mut s = DocSelection::new();
        s.start((2, 2), SelKind::Block);
        s.extend((9, 9));
        s.clear();
        assert_eq!(s.range(), None);
        assert!(!s.contains((5, 5)));
    }

    /// The totality property (the iter-25 rule, mirroring
    /// [`Selection`](crate::selection::Selection)'s,
    /// [`TextArea`](crate::text_area::TextArea)'s and
    /// [`FocusRing`](crate::focus::FocusRing)'s): any sequence of
    /// `start`/`extend`/`clear` of any [`SelKind`] over random — including
    /// `usize::MAX` and degenerate — coordinates never panics;
    /// [`DocSelection::range`] always yields `start <= end` in row-major
    /// order; and the spec's anchor-membership guarantee holds exactly as the
    /// documented inclusivity dictates: a non-empty [`SelKind::Block`] always
    /// [`contains`](DocSelection::contains) its anchor (a rectangle corner is
    /// inclusive on both axes), and a non-empty [`SelKind::Char`] selection
    /// contains its anchor **whenever the anchor is the row-major start** —
    /// i.e. for the ordinary forward Shift-sweep — because the start is
    /// inclusive while the caret end is exclusive (a zero-width
    /// `active == anchor` Char selection deliberately contains nothing, the
    /// stated boundary choice).
    #[test]
    fn any_sequence_of_operations_is_total_and_normalised() {
        // Fixed-seed LCG keeps the run deterministic with no rand dep
        // (rstui-core is dependency-free) — the same seed/technique
        // text_area.rs's totality proptest uses.
        let mut state: u64 = 0x0bad_f00d_dead_beef;
        let mut rng = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };

        // Coordinates spanning the degenerate corners and large values,
        // including usize::MAX (the document-space analogue of selection.rs's
        // u16::MAX cell probe).
        let pick = |r: u64| -> usize {
            match r % 5 {
                0 => 0,
                1 => 1,
                2 => (r >> 8) as usize % 40,
                3 => usize::MAX,
                _ => (r >> 16) as usize % 4096,
            }
        };
        let kind_of = |r: u64| match r % 3 {
            0 => SelKind::Char,
            1 => SelKind::Line,
            _ => SelKind::Block,
        };

        let mut s = DocSelection::new();
        for _ in 0..30_000 {
            let pos = (pick(rng()), pick(rng()));
            match rng() % 4 {
                0 => s.start(pos, kind_of(rng())),
                1 => s.extend(pos),
                2 => s.clear(),
                _ => s = DocSelection::new(),
            }

            // Invariant 1: range() is always start <= end in row-major
            // (row, col) order, and accessor agreement holds.
            if let Some((start, end)) = s.range() {
                assert!(start <= end, "range() escaped start <= end");
                assert!(!s.is_empty());
                assert!(s.anchor().is_some() && s.active().is_some());
            } else {
                assert!(s.is_empty());
                assert_eq!(s.anchor(), None);
                assert_eq!(s.active(), None);
                assert!(!s.contains(pos));
            }

            // Invariant 2: anchor membership, exactly as the documented
            // inclusivity dictates.
            if let Some(anchor) = s.anchor() {
                let active = s.active().expect("non-empty");
                match s.kind() {
                    // Rectangle corner: always inclusive on both axes.
                    SelKind::Block => {
                        assert!(
                            s.contains(anchor),
                            "non-empty Block selection lost its anchor corner {anchor:?}"
                        );
                        assert!(
                            s.contains(active),
                            "non-empty Block selection lost its active corner {active:?}"
                        );
                    }
                    // Whole rows: the anchor's and active's rows are covered
                    // regardless of column.
                    SelKind::Line => {
                        assert!(
                            s.contains(anchor),
                            "non-empty Line selection lost its anchor row"
                        );
                        assert!(
                            s.contains(active),
                            "non-empty Line selection lost its active row"
                        );
                    }
                    // Start inclusive, caret end EXCLUSIVE (the stated
                    // choice): the row-major *start* is contained whenever the
                    // selection has any width (start != end); the anchor is
                    // that start exactly on a forward sweep, and a zero-width
                    // `active == anchor` selection contains nothing.
                    SelKind::Char => {
                        let (start, end) = s.range().expect("non-empty");
                        if start != end {
                            assert!(
                                s.contains(start),
                                "non-degenerate Char selection lost its ordered start {start:?}"
                            );
                            // The anchor is contained iff it IS that start.
                            assert_eq!(
                                s.contains(anchor),
                                anchor == start,
                                "Char anchor membership disagreed with start-inclusive/end-exclusive rule \
                                 (anchor {anchor:?}, start {start:?}, end {end:?})"
                            );
                            // The caret end is never in (exclusive).
                            assert!(
                                !s.contains(end),
                                "Char caret end {end:?} must be excluded"
                            );
                        } else {
                            // Zero-width caret selection: contains nothing.
                            assert!(
                                !s.contains(anchor),
                                "zero-width Char selection wrongly contained {anchor:?}"
                            );
                        }
                    }
                }
            }

            // Invariant 3: contains never panics for arbitrary probes,
            // including the extreme corners.
            let _ = s.contains((pick(rng()), pick(rng())));
            let _ = s.contains((0, 0));
            let _ = s.contains((usize::MAX, usize::MAX));
            let _ = s.contains((usize::MAX, 0));
            let _ = s.contains((0, usize::MAX));
        }
        // Reaching here proves no operation panicked for any input.
    }
}
