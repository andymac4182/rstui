//! Caller-owned text selection over content-buffer coordinates.
//!
//! [`Selection`] is the selection-side sibling of
//! [`ScrollState`](crate::scroll::ScrollState) /
//! [`TextEdit`](crate::text_edit::TextEdit) /
//! [`FocusRing`](crate::focus::FocusRing): a pure value type that lives as a
//! *field in the application's model*, mutated only by `update` (a mouse-down
//! starts it, a drag extends it, a click elsewhere clears it), and read by
//! the pure `view`. A transcript/code widget is a **pure projection** of one
//! — at render time it asks [`contains`](Selection::contains) per cell and
//! applies a highlight [`Style`](crate::style::Style); it never owns or
//! mutates the selection. The "copy" command reads the text back through
//! [`selected_text`]. Per
//! [ADR 0012](https://github.com/andymac4182/rstui/blob/main/docs/adr/0012-widget-composition-and-layout-model.md)
//! §P1 this is *forced* by rstui's pure-`view` / immediate-mode design: a
//! widget is handed only a [`Buffer`] at render time with no retained tree,
//! so it can neither own the drag anchor nor mutate it on a mouse event. The
//! reducer owns the selection, exactly as it owns the scroll, the focus, and
//! the edited text. Drag-select → copy is the deep-dive's last transcript
//! near-blocker and is architecturally novel for immediate-mode — hence its
//! own carefully-designed slice (ADR 0012 §P1).
//!
//! Like [`focus`](crate::focus) and [`scroll`](crate::scroll), this module is
//! **optional**: an app may keep a bare pair of coordinates of its own and
//! never name a type from here. `Selection` exists only to turn the
//! row-major span / clamp / extraction bookkeeping every terminal-style
//! selection re-derives — and routinely gets wrong at the edges (an
//! unordered anchor/active pair, an off-by-one at the last cell, an
//! out-of-buffer drag, trailing blanks copied verbatim) — into one reusable,
//! panic-free primitive:
//!
//! - Coordinates are **content-buffer cell positions** ([`Position`]), the
//!   same absolute coordinates [`Buffer::get`] uses — never pixels or byte
//!   offsets. The app maps a [`MouseEvent`](crate::event::MouseEvent) to a
//!   content [`Position`] (against the `Rect`s it already laid out, the same
//!   way it maps a click to a [`FocusId`](crate::focus::FocusId)); selection
//!   stays purely in cell space.
//! - The span is a **row-major terminal stream** selection, not a rectangle:
//!   from the start point to the end point in reading order — the rest of
//!   the start row, every full intermediate row, then the head of the end
//!   row — exactly the iTerm/opencode drag-select behavior, and the only one
//!   that copies wrapped prose correctly. Both endpoints are **inclusive**
//!   (the cell under the cursor is selected, as in every terminal).
//! - Every method is **total** — no input, including an [`extend`] before a
//!   [`start`], a drag far outside the buffer, or `u16::MAX` coordinates, can
//!   panic; [`ordered`] always returns top-left ≤ bottom-right and
//!   [`selected_text`] is bounded by the buffer it reads (the iter-25 "a pure
//!   projection must be total" rule, the same guarantee
//!   [`ScrollState`](crate::scroll::ScrollState) and
//!   [`FocusRing`](crate::focus::FocusRing) give).
//!
//! This is **app/widget** selection (what the user dragged to copy) and is
//! unrelated to a [`TextEdit`](crate::text_edit::TextEdit) cursor or terminal
//! scrollback.
//!
//! [`start`]: Selection::start
//! [`extend`]: Selection::extend
//! [`ordered`]: Selection::ordered
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Position, Rect, Selection, Style};
//! use rstui_core::selection::selected_text;
//!
//! // Two lines of content the pure `view` rendered into a buffer.
//! let mut buf = Buffer::empty(Rect::new(0, 0, 11, 2));
//! buf.set_str(Position::new(0, 0), "hello world", Style::new());
//! buf.set_str(Position::new(0, 1), "second line", Style::new());
//!
//! // The app stores a `Selection` in its model. `update` maps mouse-down to
//! // `start` and each drag to `extend`, passing content coordinates.
//! let mut sel = Selection::new();
//! assert!(sel.is_empty());
//! sel.start(Position::new(6, 0)); // the 'w' of "world"
//! sel.extend(Position::new(5, 1)); // through "second" on the next row
//! assert!(!sel.is_empty());
//!
//! // `ordered()` normalises to (top-left, bottom-right) in row-major order,
//! // whichever direction the user dragged.
//! assert_eq!(
//!     sel.ordered(),
//!     Some((Position::new(6, 0), Position::new(5, 1)))
//! );
//!
//! // A widget projects the selection by asking, per cell, `contains`:
//! assert!(sel.contains(Position::new(8, 0))); // first row, after the anchor
//! assert!(sel.contains(Position::new(0, 1))); // last row, before the active
//! assert!(!sel.contains(Position::new(2, 0))); // before the start on row 0
//!
//! // Terminal stream semantics: anchor → end-of-row, full middle rows,
//! // start-of-row → active, trailing blanks trimmed, rows joined by '\n'.
//! assert_eq!(selected_text(&buf, &sel), "world\nsecond");
//!
//! // Every input is total: clearing empties it; extracting an empty
//! // selection is the empty string, never a panic.
//! sel.clear();
//! assert!(sel.is_empty());
//! assert_eq!(selected_text(&buf, &sel), "");
//! ```

use crate::buffer::Buffer;
use crate::geometry::Position;

/// A text selection over content-buffer coordinates, caller-owned.
///
/// `Selection` is a **pure value type** designed to live as a field in the
/// application's model (it derives [`Default`] so it drops into a
/// `#[derive(Default)]` model as an empty selection — `new() == default()`,
/// like [`TextEdit`](crate::text_edit::TextEdit), since "nothing selected"
/// is the only sensible inert state). It owns *no* terminal, runtime, or
/// widget state: `update` mutates it in response to the mouse messages the
/// app maps, and the pure `view` only reads
/// [`contains`](Self::contains) / [`ordered`](Self::ordered) to project it.
/// The framework never touches it.
///
/// The selection is stored as an `anchor`/`active` pair — the cell where the
/// drag began and the cell it currently reaches. They are kept together
/// (both set by [`start`](Self::start), `active` moved by
/// [`extend`](Self::extend), both cleared by [`clear`](Self::clear)), so the
/// selection is either fully present or fully absent and can never be
/// half-formed. [`ordered`](Self::ordered) normalises the pair to
/// (top-left, bottom-right) so the projection never has to care which way
/// the user dragged.
///
/// The selected region is a **row-major terminal stream**, not a rectangle:
/// after ordering to `(start, end)`, a cell is selected iff it is at or
/// after `start` and at or before `end` in reading order — so the start row
/// is selected from `start.x` to its end, every row strictly between is
/// selected in full, and the end row is selected from its start to `end.x`.
/// Both endpoints are inclusive. This is the iTerm/opencode behavior and the
/// only span that copies wrapped text faithfully (a rectangular "block"
/// selection is a deliberately deferred separate mode, not a flag on this
/// one).
///
/// Every method is **total**: arbitrary input — an [`extend`](Self::extend)
/// with no [`start`](Self::start), coordinates far outside any buffer,
/// `u16::MAX` positions — is well-defined and never panics, and
/// [`ordered`](Self::ordered) always yields `start <= end` in row-major
/// `(y, x)` order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// `Some((anchor, active))` once a drag has begun: `anchor` is the cell
    /// [`start`](Self::start) was called with, `active` the cell the latest
    /// [`extend`](Self::extend) reached. `None` is the empty selection. The
    /// pair is never partially set, which is what keeps every accessor total.
    span: Option<(Position, Position)>,
}

impl Selection {
    /// The empty selection (nothing selected).
    ///
    /// Identical to [`Selection::default`]: unlike
    /// [`ScrollState`](crate::scroll::ScrollState), there is no useful
    /// non-default starting state for a selection, so `new()` and `default()`
    /// agree (the [`TextEdit`](crate::text_edit::TextEdit) convention).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether there is no selection.
    ///
    /// `true` for a fresh [`new`](Self::new) / after [`clear`](Self::clear).
    /// A [`start`](Self::start)ed but not-yet-[`extend`](Self::extend)ed
    /// selection is **not** empty — it is a one-cell selection (the cell the
    /// drag began on); the app decides whether a bare click should
    /// [`start`](Self::start) a selection or [`clear`](Self::clear) it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.span.is_none()
    }

    /// Begins a selection at `pos` (sets both anchor and active to it).
    ///
    /// This is the mouse-down path: `pos` is the content-buffer cell the app
    /// resolved the press to. Any prior selection is replaced. `pos` is
    /// stored as-is (no clamping needed — it is reconciled against the buffer
    /// only at extraction time, exactly as a raw scroll offset is reconciled
    /// at render), so any coordinate is safe.
    pub fn start(&mut self, pos: Position) {
        self.span = Some((pos, pos));
    }

    /// Extends an in-progress selection so it now reaches `pos` (moves the
    /// active end, keeping the anchor fixed).
    ///
    /// This is the drag path. If there is no selection yet (no
    /// [`start`](Self::start) since the last [`clear`](Self::clear)) this is a
    /// deliberate, total no-op — `extend` only ever extends an existing
    /// selection; the app must `start` on the mouse-down first. `pos` is
    /// stored raw (reconciled against the buffer at extraction), so any
    /// coordinate is safe.
    pub fn extend(&mut self, pos: Position) {
        if let Some((anchor, _)) = self.span {
            self.span = Some((anchor, pos));
        }
    }

    /// Clears the selection back to empty (e.g. a click outside it, Esc, or
    /// the content changing under it).
    pub fn clear(&mut self) {
        self.span = None;
    }

    /// The selection normalised to `(top_left, bottom_right)` in row-major
    /// order, or `None` when empty.
    ///
    /// "Row-major order" means the first element is the endpoint with the
    /// smaller `(y, x)` and the second the larger, regardless of which way
    /// the user dragged (up-left or down-right). The post-condition
    /// `start.y < end.y || (start.y == end.y && start.x <= end.x)` always
    /// holds — the single definition [`contains`](Self::contains) and
    /// [`selected_text`] both build on, so the span semantics cannot drift.
    #[must_use]
    pub fn ordered(&self) -> Option<(Position, Position)> {
        let (a, b) = self.span?;
        if (a.y, a.x) <= (b.y, b.x) {
            Some((a, b))
        } else {
            Some((b, a))
        }
    }

    /// Whether the cell at `pos` is inside the selection, using **row-major
    /// terminal stream** semantics — the per-cell projection a widget reads
    /// at render to decide whether to apply a highlight style.
    ///
    /// With the selection ordered to `(start, end)`:
    /// - empty selection → always `false`;
    /// - rows above `start.y` or below `end.y` → `false`;
    /// - single-row selection (`start.y == end.y`) → `start.x <= pos.x <=
    ///   end.x` (both inclusive);
    /// - the start row of a multi-row selection → `pos.x >= start.x` (the
    ///   anchor to the end of the row);
    /// - the end row of a multi-row selection → `pos.x <= end.x` (the start
    ///   of the row to the active cell);
    /// - any row strictly between → `true` (the whole row).
    ///
    /// This is a pure description of the stream region in content
    /// coordinates and is intentionally **unbounded to the right** on a
    /// spanned row (it has no notion of a buffer width); a projecting widget
    /// clips naturally because it only ever consults its own cells. The
    /// widget never mutates anything — it reads this predicate, exactly as a
    /// `ScrollView` reads [`ScrollState::offset`](crate::scroll::ScrollState::offset).
    #[must_use]
    pub fn contains(&self, pos: Position) -> bool {
        self.span().is_some_and(|s| s.contains(pos))
    }

    /// The selection as a frame-scoped [`SelectionSpan`] projector, or
    /// `None` when empty (CM-1). A widget computes this **once** per frame
    /// and tests every cell against the returned value, instead of paying
    /// `ordered()`'s `Option` destructure + tuple compare *per cell* that
    /// [`contains`](Self::contains) — now a thin shim over it — would. The
    /// shared definition keeps the stream semantics from drifting.
    #[must_use]
    pub fn span(&self) -> Option<SelectionSpan> {
        self.ordered()
            .map(|(start, end)| SelectionSpan { start, end })
    }
}

/// A frame-scoped, `Copy` projection of a non-empty [`Selection`]: resolve
/// the ordered endpoints once via [`Selection::span`], then test many cells
/// with [`contains`](SelectionSpan::contains) without re-deriving them per
/// cell — the per-cell highlight check a widget runs at render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionSpan {
    start: Position,
    end: Position,
}

impl SelectionSpan {
    /// Row-major-terminal-stream membership — identical semantics to
    /// [`Selection::contains`], with the ordered endpoints already resolved.
    #[must_use]
    pub fn contains(self, pos: Position) -> bool {
        let (start, end) = (self.start, self.end);
        if pos.y < start.y || pos.y > end.y {
            false
        } else if start.y == end.y {
            pos.x >= start.x && pos.x <= end.x
        } else if pos.y == start.y {
            pos.x >= start.x
        } else if pos.y == end.y {
            pos.x <= end.x
        } else {
            true
        }
    }
}

/// Extracts the selected text from `content`, the "copy" path.
///
/// The selection is read in **row-major terminal stream** order (the same
/// semantics [`Selection::contains`] projects): for each buffer row the
/// selection covers, the run of cells from the row's selected start column
/// to its selected end column (inclusive) is read out, trailing spaces
/// (`' '`) on that row are trimmed, and rows are joined with a single `'\n'`
/// (no trailing newline). Trailing-blank trimming matches what a terminal
/// copies — a row padded with blank cells yields no spurious whitespace, and
/// an all-blank covered row contributes an empty line between its neighbours.
///
/// Totality and bounds: the selection is reconciled against
/// [`Buffer::area`] here (the way a raw scroll offset is reconciled at
/// render), so coordinates outside the buffer are clamped and a selection
/// with no overlap — or over an empty buffer, or an empty
/// [`Selection`] — yields `String::new()`. The result is bounded by the
/// buffer it reads (at most one character per covered cell plus the
/// row-joining newlines); no input panics.
#[must_use]
pub fn selected_text(content: &Buffer, sel: &Selection) -> String {
    let area = content.area();
    let Some((start, end)) = sel.ordered() else {
        return String::new();
    };
    if area.is_empty() {
        return String::new();
    }

    // Reconcile the selection against the buffer: only rows/columns that
    // actually exist in `content` can be copied (last valid index = edge-1,
    // since `right`/`bottom` are exclusive).
    let last_col = area.right().saturating_sub(1);
    let last_row = area.bottom().saturating_sub(1);
    let first_row = start.y.max(area.top());
    let final_row = end.y.min(last_row);
    if first_row > final_row {
        return String::new();
    }

    let mut out = String::new();
    let mut row = String::new();
    for y in first_row..=final_row {
        // Per-row column span from the stream semantics, then clipped to the
        // buffer. The bounds are derived from the *true* start/end rows, not
        // the clamped ones, so a selection beginning above the buffer still
        // treats the first visible row as a full intermediate row.
        let row_lo = if y == start.y { start.x } else { area.left() };
        let row_hi = if y == end.y { end.x } else { last_col };
        let col_lo = row_lo.max(area.left());
        let col_hi = row_hi.min(last_col);

        row.clear();
        if col_lo <= col_hi {
            for x in col_lo..=col_hi {
                if let Some(cell) = content.get(Position::new(x, y)) {
                    row.push(cell.symbol);
                }
            }
        }
        if y != first_row {
            out.push('\n');
        }
        out.push_str(row.trim_end_matches(' '));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::style::Style;

    fn buf_of(lines: &[&str]) -> Buffer {
        let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
        let mut b = Buffer::empty(Rect::new(0, 0, width, lines.len() as u16));
        for (y, line) in lines.iter().enumerate() {
            b.set_str(Position::new(0, y as u16), line, Style::new());
        }
        b
    }

    #[test]
    fn new_and_default_are_the_empty_selection() {
        assert_eq!(Selection::new(), Selection::default());
        let s = Selection::new();
        assert!(s.is_empty());
        assert_eq!(s.ordered(), None);
        assert!(!s.contains(Position::ORIGIN));
    }

    #[test]
    fn start_makes_a_one_cell_selection_that_is_not_empty() {
        let mut s = Selection::new();
        s.start(Position::new(3, 2));
        assert!(!s.is_empty());
        assert_eq!(
            s.ordered(),
            Some((Position::new(3, 2), Position::new(3, 2)))
        );
        assert!(s.contains(Position::new(3, 2)));
        assert!(!s.contains(Position::new(4, 2)));
        assert!(!s.contains(Position::new(3, 1)));
    }

    #[test]
    fn extend_before_start_is_a_total_no_op() {
        let mut s = Selection::new();
        s.extend(Position::new(9, 9)); // no start yet
        assert!(s.is_empty());
        assert_eq!(s.ordered(), None);
    }

    #[test]
    fn ordered_normalises_a_backward_drag() {
        // Dragging up-and-left: anchor is below/right of active.
        let mut s = Selection::new();
        s.start(Position::new(7, 4));
        s.extend(Position::new(2, 1));
        assert_eq!(
            s.ordered(),
            Some((Position::new(2, 1), Position::new(7, 4)))
        );
        // Same-row backward drag orders by column.
        s.start(Position::new(8, 3));
        s.extend(Position::new(3, 3));
        assert_eq!(
            s.ordered(),
            Some((Position::new(3, 3), Position::new(8, 3)))
        );
    }

    #[test]
    fn contains_is_terminal_stream_not_a_rectangle() {
        let mut s = Selection::new();
        s.start(Position::new(6, 0));
        s.extend(Position::new(3, 2));

        // Start row: from the anchor to the (unbounded) end of the row.
        assert!(!s.contains(Position::new(5, 0)));
        assert!(s.contains(Position::new(6, 0)));
        assert!(s.contains(Position::new(999, 0)));
        // A full intermediate row, including a column left of the anchor.
        assert!(s.contains(Position::new(0, 1)));
        assert!(s.contains(Position::new(50, 1)));
        // End row: from the start of the row up to and including the active.
        assert!(s.contains(Position::new(0, 2)));
        assert!(s.contains(Position::new(3, 2)));
        assert!(!s.contains(Position::new(4, 2)));
        // Outside the row span entirely.
        assert!(!s.contains(Position::new(6, 3)));
    }

    #[test]
    fn single_row_selection_is_an_inclusive_column_range() {
        let mut s = Selection::new();
        s.start(Position::new(2, 5));
        s.extend(Position::new(6, 5));
        for x in 2..=6 {
            assert!(s.contains(Position::new(x, 5)), "x={x} should be in");
        }
        assert!(!s.contains(Position::new(1, 5)));
        assert!(!s.contains(Position::new(7, 5)));
        assert!(!s.contains(Position::new(4, 4)));
    }

    #[test]
    fn selected_text_single_row_trims_trailing_blanks() {
        let buf = buf_of(&["hello world"]);
        let mut s = Selection::new();
        s.start(Position::new(0, 0));
        s.extend(Position::new(4, 0));
        assert_eq!(selected_text(&buf, &s), "hello");

        // A run that ends in blank cells: the trailing spaces are trimmed.
        let buf = buf_of(&["hi        "]);
        let mut s = Selection::new();
        s.start(Position::new(0, 0));
        s.extend(Position::new(9, 0));
        assert_eq!(selected_text(&buf, &s), "hi");
    }

    #[test]
    fn selected_text_spans_rows_with_stream_semantics() {
        let buf = buf_of(&["hello world", "second line", "third row!!"]);
        let mut s = Selection::new();
        s.start(Position::new(6, 0)); // 'w'
        s.extend(Position::new(4, 2)); // 'd' of "third"
        // start row from 'w' to EOL, the whole middle row, end row to col 4.
        assert_eq!(selected_text(&buf, &s), "world\nsecond line\nthird");
    }

    #[test]
    fn selected_text_is_empty_for_an_empty_or_non_overlapping_selection() {
        let buf = buf_of(&["abc", "def"]);
        let empty = Selection::new();
        assert_eq!(selected_text(&buf, &empty), "");

        // Entirely below the buffer: no overlap, empty string (no panic).
        let mut s = Selection::new();
        s.start(Position::new(0, 50));
        s.extend(Position::new(2, 60));
        assert_eq!(selected_text(&buf, &s), "");

        // Empty buffer is total too.
        let mut s = Selection::new();
        s.start(Position::ORIGIN);
        s.extend(Position::new(10, 10));
        assert_eq!(selected_text(&Buffer::empty(Rect::ZERO), &s), "");
    }

    #[test]
    fn selected_text_clamps_a_drag_that_starts_above_the_buffer() {
        let buf = buf_of(&["alpha", "bravo"]);
        // Anchor is above row 0: the first visible row is a full
        // intermediate row, not the (off-buffer) start row.
        let mut s = Selection::new();
        s.start(Position::new(3, 0)); // start.y == row 0 here
        s.extend(Position::new(2, 9)); // far below the 2-row buffer
        // Row 0 from x=3 to EOL ("ha"), row 1 in full ("bravo").
        assert_eq!(selected_text(&buf, &s), "ha\nbravo");
    }

    #[test]
    fn selected_text_keeps_a_blank_covered_row_as_an_empty_line() {
        let buf = buf_of(&["top", "   ", "end"]);
        let mut s = Selection::new();
        s.start(Position::new(0, 0));
        s.extend(Position::new(2, 2));
        // The all-blank middle row trims to "" but still separates its
        // neighbours with a newline.
        assert_eq!(selected_text(&buf, &s), "top\n\nend");
    }

    /// The totality property (the iter-25 rule, mirroring
    /// [`ScrollState`](crate::scroll::ScrollState)'s and
    /// [`FocusRing`](crate::focus::FocusRing)'s): any sequence of
    /// `start`/`extend`/`clear` over randomly-sized buffers (including the
    /// degenerate empty buffer and `u16::MAX` coordinates) never panics;
    /// [`Selection::ordered`] always yields `start <= end` in row-major
    /// order; [`Selection::contains`] agrees with that ordering at the
    /// endpoints; and [`selected_text`] is bounded by the buffer it reads.
    #[test]
    fn any_sequence_of_operations_is_total_and_bounded() {
        // Fixed-seed LCG keeps the run deterministic with no rand dep
        // (rstui-core is dependency-free) — the technique focus.rs uses.
        let mut state: u64 = 0x5e1e_c714_0f00_d123;
        let mut rng = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };

        let mut s = Selection::new();
        for _ in 0..20_000 {
            // Coordinates spanning the degenerate corners and large values.
            let pick = |r: u64| -> u16 {
                match r % 5 {
                    0 => 0,
                    1 => 1,
                    2 => (r >> 8) as u16 % 40,
                    3 => u16::MAX,
                    _ => (r >> 16) as u16 % 4096,
                }
            };
            let pos = Position::new(pick(rng()), pick(rng()));

            match rng() % 4 {
                0 => s.start(pos),
                1 => s.extend(pos),
                2 => s.clear(),
                _ => s = Selection::new(),
            }

            // Invariant 1: ordered() is always top-left <= bottom-right in
            // row-major (y, x) order, and the endpoints are contained.
            if let Some((a, b)) = s.ordered() {
                assert!(
                    (a.y, a.x) <= (b.y, b.x),
                    "ordered() escaped top-left <= bottom-right"
                );
                assert!(s.contains(a) && s.contains(b));
                assert!(!s.is_empty());
            } else {
                assert!(s.is_empty());
                assert!(!s.contains(pos));
            }

            // Invariant 2: a small random buffer extraction never panics and
            // is bounded by that buffer (≤ one char per covered cell plus
            // the row-joining newlines).
            let w = (rng() % 6) as u16;
            let h = (rng() % 6) as u16;
            let buf = Buffer::empty(Rect::new(0, 0, w, h));
            let text = selected_text(&buf, &s);
            let bound = (w as usize) * (h as usize) + (h as usize);
            assert!(
                text.chars().count() <= bound,
                "selected_text length {} exceeded buffer bound {bound}",
                text.chars().count()
            );
        }
        // Reaching here proves no operation panicked for any input.
    }
}
