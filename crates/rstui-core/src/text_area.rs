//! Multi-line editable text as caller-owned model state.
//!
//! [`TextArea`] is the **multi-line dual of [`TextEdit`](crate::text_edit::TextEdit)**:
//! the same pure-value, total, caller-owned editing model, but a *document*
//! of logical lines plus a `(row, col)` cursor instead of a single string
//! plus a character index. Like [`TextEdit`](crate::TextEdit) it lives as a *field in the
//! application's model*, is mutated only by `update`, and is read by the pure
//! `view`; the `Editor` widget (in `rstui-widgets`) is a pure projection of
//! one — it draws [`lines`](TextArea::lines) and a caret at
//! [`cursor`](TextArea::cursor) and never edits anything itself, exactly as
//! `Input` projects a caller-owned [`TextEdit`](crate::TextEdit). Per
//! [ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
//! this is *forced* by rstui's pure-`view` / immediate-mode design: a widget
//! is handed only a [`Buffer`](crate::buffer::Buffer) at render time, so it
//! can neither own the text being typed nor mutate it on a keystroke. The
//! reducer owns the edit.
//!
//! [`TextEdit`](crate::TextEdit) records the decision verbatim: multi-line editing is *"a
//! separate model, not a flag on this one"*. This is that separate model. A
//! newline is a structural split here (it creates a row), not a literal
//! `'\n'` character stored in a line — every `String` in
//! [`lines`](TextArea::lines) is one logical line with no embedded newline.
//!
//! Like [`text_edit`](crate::text_edit), this module is **optional**: an app
//! may keep its own `Vec<String>` and a `(usize, usize)` cursor and never name
//! a type from here. `TextArea` exists only to turn the 2D
//! cursor/UTF-8-boundary/line-join bookkeeping every such app re-derives — and
//! routinely gets wrong — into one reusable, panic-free primitive:
//!
//! - The cursor is a `(row, col)` pair where `col` is a **character index**
//!   into `lines[row]`, never a byte offset, matching rstui's single-`char`
//!   cell model. The widget maps `col` straight to a column and `row` to a
//!   screen row; the byte math stays internal.
//! - Vertical motion keeps a **sticky goal column**: moving up/down through a
//!   short line and back out restores the column you started from, the
//!   behaviour every real editor has. It is purely internal — no public
//!   surface, no caller bookkeeping.
//! - Every method is **total** — no input, including a paste of arbitrary
//!   UTF-8 with embedded newlines or an out-of-range
//!   [`set_cursor`](TextArea::set_cursor), can panic or leave the cursor
//!   mid-codepoint, off the document, or on a missing row (the iter-25 "a pure
//!   projection must be total" rule, the same guarantee
//!   [`TextEdit`](crate::TextEdit) gives single-line editing).
//!
//! This is **app/widget** state and is unrelated to terminal-window focus
//! (`Event::FocusGained` / `Event::FocusLost`); the reducer decides when an
//! `Editor` is focused (via [`focus`](crate::focus)) and routes keystrokes to
//! its `TextArea` accordingly.
//!
//! # Example
//!
//! ```
//! use rstui_core::text_area::TextArea;
//!
//! // The app stores one per multi-line field in its model. Pre-filling
//! // splits on '\n' and puts the cursor at the end, ready to append.
//! let mut doc = TextArea::from_value("first\nsecond");
//! assert_eq!(doc.row_count(), 2);
//! assert_eq!(doc.cursor(), (1, 6));
//!
//! // `update` maps key messages to edits. Totality means no sequence panics.
//! doc.insert_char('!'); // "second!" on row 1
//! doc.insert_newline(); // splits at the cursor -> a new empty row 2
//! doc.insert_str("third"); // typed into the new row
//! assert_eq!(doc.to_string(), "first\nsecond!\nthird");
//! assert_eq!(doc.cursor(), (2, 5));
//!
//! // Joining lines: Backspace at column 0 merges with the previous row.
//! doc.move_doc_start();
//! doc.move_down(); // row 1, sticky column kept
//! doc.move_home(); // (1, 0)
//! assert!(doc.delete_backward()); // row 1 joins onto row 0
//! assert_eq!(doc.line(0), Some("firstsecond!"));
//!
//! // Out-of-range cursor placement clamps both axes instead of panicking.
//! doc.set_cursor(999, 999);
//! assert_eq!(doc.cursor(), (1, 5));
//! ```

use std::fmt;

/// A multi-line editable document plus a character-indexed `(row, col)`
/// cursor.
///
/// `TextArea` is a **pure value type** designed to live as a field in the
/// application's model (it derives [`Clone`]/[`PartialEq`] and implements
/// [`Default`] as one empty line, so it drops into a `#[derive(Default)]`
/// model). It owns *no* terminal, runtime, or widget state: `update` mutates
/// it in response to key/paste messages the app maps, and the pure `view`
/// only reads [`lines`](Self::lines) / [`cursor`](Self::cursor) to project it
/// through an `Editor` widget. The framework never touches it.
///
/// Invariants every mutator upholds (the multi-line generalization of
/// [`TextEdit`](crate::text_edit::TextEdit)'s `0 <= cursor <= len`):
///
/// - [`lines`](Self::lines) is **always non-empty** — an empty document is
///   one empty line, not zero lines.
/// - `row` is in `0..row_count()`.
/// - `col` is in `0..=lines[row].chars().count()` (one past the last
///   character means "append here").
///
/// Every method is **total**: arbitrary input — a multi-byte paste with
/// embedded newlines, a backspace at the document start, an out-of-range
/// [`set_cursor`](Self::set_cursor) — is well-defined and never panics or
/// strands the cursor inside a UTF-8 codepoint or off the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextArea {
    /// One logical line per `String`, never empty, no embedded `'\n'`.
    lines: Vec<String>,
    /// Cached `lines[i].chars().count()` for every row, kept exact by every
    /// mutator so [`line_char_len`](Self::line_char_len) is O(1) instead of
    /// an O(line) UTF-8 re-scan — the per-keystroke (`move_*`) and
    /// per-visible-row (projecting `Editor`) hot path (CM-3, the 2-D
    /// analogue of [`TextEdit`](crate::text_edit::TextEdit)'s `char_len`,
    /// CM-2). It is a pure function of `lines` (`line_lens.len() ==
    /// lines.len()` and `line_lens[i] == lines[i].chars().count()`), so the
    /// derived `PartialEq`/`Eq` stay correct (equal `lines` ⇒ equal cache)
    /// and the totality proptest gate-enforces the invariant after every op.
    line_lens: Vec<usize>,
    /// Row index in `0..lines.len()`.
    row: usize,
    /// Character index in `0..=lines[row].chars().count()`.
    col: usize,
    /// The sticky target column for vertical motion: set on the first
    /// up/down/page move, reused (clamped per row) by subsequent ones, and
    /// cleared by any horizontal move or edit. Purely internal — there is no
    /// public accessor and it is excluded from neither [`Clone`] nor
    /// [`PartialEq`] only because it is observable through cursor behaviour.
    goal_col: Option<usize>,
}

impl Default for TextArea {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            line_lens: vec![0],
            row: 0,
            col: 0,
            goal_col: None,
        }
    }
}

impl TextArea {
    /// An empty document — one empty line, cursor at the origin.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A document pre-filled with `value`, split into rows on `'\n'`, with
    /// the cursor placed **after** the last character of the last row.
    ///
    /// Cursor-at-end matches the usual expectation when a field opens onto an
    /// existing value (ready to append); call [`move_doc_start`](Self::move_doc_start)
    /// or [`set_cursor`](Self::set_cursor) in `update` for a different start.
    #[must_use]
    pub fn from_value(value: impl Into<String>) -> Self {
        let mut area = Self::new();
        area.set_value(value);
        area
    }

    /// The document as logical lines (never empty; no embedded `'\n'`).
    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The text of row `row`, or `None` if `row` is out of range.
    #[must_use]
    pub fn line(&self, row: usize) -> Option<&str> {
        self.lines.get(row).map(String::as_str)
    }

    /// The cursor as `(row, col)` — `col` is a **character index** in
    /// `0..=lines[row].chars().count()`, the cell the projecting widget draws
    /// the caret at (before any scroll the widget applies for itself).
    #[must_use]
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// The number of rows (always at least one).
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.lines.len()
    }

    /// Whether the document is empty (exactly one empty line).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Replaces the whole document, splitting `value` into rows on `'\n'` and
    /// moving the cursor to the **end** (consistent with
    /// [`from_value`](Self::from_value)).
    pub fn set_value(&mut self, value: impl Into<String>) {
        let value = value.into();
        self.lines = value.split('\n').map(String::from).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.line_lens = self.lines.iter().map(|l| l.chars().count()).collect();
        self.row = self.lines.len() - 1;
        self.col = self.line_char_len(self.row);
        self.goal_col = None;
    }

    /// Empties the document back to one empty line and returns the cursor to
    /// the origin.
    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.line_lens = vec![0];
        self.row = 0;
        self.col = 0;
        self.goal_col = None;
    }

    /// Moves the cursor to `(row, col)`, clamping **both** axes: `row` into
    /// `0..row_count()` and `col` into `0..=lines[row].chars().count()`.
    ///
    /// Clamping (rather than panicking on an out-of-range index) is the
    /// totality rule applied to cursor placement: an app mapping a mouse
    /// click `(x, y)` to a cursor can pass any value safely.
    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.goal_col = None;
        self.row = row.min(self.lines.len() - 1);
        self.col = col.min(self.line_char_len(self.row));
    }

    /// Inserts `c` at the cursor and advances past it. A `'\n'` splits the
    /// current line at the column, moving the cursor to the start of the new
    /// row (the dual of [`TextEdit::insert_char`](crate::text_edit::TextEdit::insert_char)).
    pub fn insert_char(&mut self, c: char) {
        self.goal_col = None;
        if c == '\n' {
            self.split_line_at_cursor();
        } else {
            let at = self.byte_at(self.row, self.col);
            self.lines[self.row].insert(at, c);
            self.line_lens[self.row] += 1;
            self.col += 1;
        }
    }

    /// Inserts `s` at the cursor, advancing past all of it.
    ///
    /// This is the paste path: `s` may be arbitrary UTF-8, and any embedded
    /// `'\n'` splits it into new rows (the multi-line generalization of
    /// [`TextEdit::insert_str`](crate::text_edit::TextEdit::insert_str), which
    /// keeps newlines verbatim because it is single-line by convention). The
    /// cursor lands just after the last inserted character.
    pub fn insert_str(&mut self, s: &str) {
        self.goal_col = None;
        if s.is_empty() {
            return;
        }

        // The contiguous row range this paste rewrites — `line_lens` for
        // exactly `[start ..= self.row]` is recomputed from the final
        // strings at the end (CM-3). Paste is the cold path, so a precise
        // recount there is correct-by-construction and not worth O(1) delta
        // arithmetic across this method's several `lines` rewrites.
        let start = self.row;

        let mut segments = s.split('\n');
        // `split` always yields at least one segment, so this never panics.
        let first = segments.next().unwrap_or("");
        let rest: Vec<&str> = segments.collect();

        // Detach the part of the current line after the cursor; it is
        // re-attached to the *last* inserted row so the tail follows the
        // paste exactly as a real editor's does.
        let at = self.byte_at(self.row, self.col);
        let tail = self.lines[self.row].split_off(at);
        self.lines[self.row].push_str(first);

        if rest.is_empty() {
            // No newline: a single-line insert. The cursor sits between the
            // pasted text and the re-attached tail. Count the row directly
            // (not via the cache, which `line_lens` resync below has not yet
            // updated) — identical to the original `line_char_len` here.
            self.col = self.lines[self.row].chars().count();
            self.lines[self.row].push_str(&tail);
        } else {
            let last_idx = rest.len() - 1;
            for (i, segment) in rest.into_iter().enumerate() {
                let mut line = String::from(segment);
                if i == last_idx {
                    self.col = line.chars().count();
                    line.push_str(&tail);
                }
                self.row += 1;
                self.lines.insert(self.row, line);
            }
        }

        // CM-3: the single `line_lens` entry at `start` is replaced by the
        // exact char counts of every row this paste produced
        // (`[start ..= self.row]`), keeping the cache a perfect mirror of
        // `lines` regardless of how many rows the split created.
        let new: Vec<usize> = self.lines[start..=self.row]
            .iter()
            .map(|l| l.chars().count())
            .collect();
        self.line_lens.splice(start..start + 1, new);
    }

    /// Splits the current line at the cursor, opening a new row — exactly
    /// `insert_char('\n')`.
    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Deletes the character before the cursor (Backspace). At column 0 this
    /// joins the current line onto the end of the previous one, with the
    /// cursor at the join. Returns whether anything changed (`false` only at
    /// the document start `(0, 0)`).
    pub fn delete_backward(&mut self) -> bool {
        self.goal_col = None;
        if self.col > 0 {
            let end = self.byte_at(self.row, self.col);
            let start = self.byte_at(self.row, self.col - 1);
            self.lines[self.row].replace_range(start..end, "");
            self.line_lens[self.row] -= 1;
            self.col -= 1;
            true
        } else if self.row > 0 {
            let current = self.lines.remove(self.row);
            // Mirror the row removal in the cache; the prev row's cached
            // len is still exact, so the `line_char_len` read below (the
            // pre-join cursor column) is unchanged from the old behaviour.
            let removed = self.line_lens.remove(self.row);
            self.row -= 1;
            self.col = self.line_char_len(self.row);
            self.lines[self.row].push_str(&current);
            self.line_lens[self.row] += removed;
            true
        } else {
            false
        }
    }

    /// Deletes the character at the cursor (Delete), leaving the cursor in
    /// place. At the end of a line this joins the next line onto this one.
    /// Returns whether anything changed (`false` only at the very end of the
    /// last row).
    pub fn delete_forward(&mut self) -> bool {
        self.goal_col = None;
        if self.col < self.line_char_len(self.row) {
            let start = self.byte_at(self.row, self.col);
            let end = self.byte_at(self.row, self.col + 1);
            self.lines[self.row].replace_range(start..end, "");
            self.line_lens[self.row] -= 1;
            true
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            let removed = self.line_lens.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
            self.line_lens[self.row] += removed;
            true
        } else {
            false
        }
    }

    /// Moves the cursor one character left, wrapping to the end of the
    /// previous line at column 0; returns whether it moved (`false` only at
    /// the document start).
    pub fn move_left(&mut self) -> bool {
        self.goal_col = None;
        if self.col > 0 {
            self.col -= 1;
            true
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.line_char_len(self.row);
            true
        } else {
            false
        }
    }

    /// Moves the cursor one character right, wrapping to the start of the
    /// next line at end of line; returns whether it moved (`false` only at
    /// the document end).
    pub fn move_right(&mut self) -> bool {
        self.goal_col = None;
        if self.col < self.line_char_len(self.row) {
            self.col += 1;
            true
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
            true
        } else {
            false
        }
    }

    /// Moves the cursor up one row, clamping the column to the target row's
    /// length but remembering the original column (the sticky goal column, so
    /// passing through a short line and back out restores it). Returns
    /// whether it moved (`false` only on the first row).
    pub fn move_up(&mut self) -> bool {
        if self.row == 0 {
            return false;
        }
        self.move_to_row(self.row - 1);
        true
    }

    /// Moves the cursor down one row, with the same sticky-column behaviour
    /// as [`move_up`](Self::move_up). Returns whether it moved (`false` only
    /// on the last row).
    pub fn move_down(&mut self) -> bool {
        if self.row + 1 >= self.lines.len() {
            return false;
        }
        self.move_to_row(self.row + 1);
        true
    }

    /// Moves the cursor to the start of the current line (column 0).
    pub fn move_home(&mut self) {
        self.goal_col = None;
        self.col = 0;
    }

    /// Moves the cursor to the end of the current line.
    pub fn move_end(&mut self) {
        self.goal_col = None;
        self.col = self.line_char_len(self.row);
    }

    /// Moves the cursor to the very start of the document `(0, 0)`.
    pub fn move_doc_start(&mut self) {
        self.goal_col = None;
        self.row = 0;
        self.col = 0;
    }

    /// Moves the cursor to the very end of the document (end of the last
    /// row).
    pub fn move_doc_end(&mut self) {
        self.goal_col = None;
        self.row = self.lines.len() - 1;
        self.col = self.line_char_len(self.row);
    }

    /// Moves the cursor up `rows` rows (clamped at the top), with the same
    /// sticky-column behaviour as [`move_up`](Self::move_up). Returns whether
    /// the row changed.
    pub fn move_page_up(&mut self, rows: usize) -> bool {
        let target = self.row.saturating_sub(rows);
        if target == self.row {
            return false;
        }
        self.move_to_row(target);
        true
    }

    /// Moves the cursor down `rows` rows (clamped at the bottom), with the
    /// same sticky-column behaviour as [`move_up`](Self::move_up). Returns
    /// whether the row changed.
    pub fn move_page_down(&mut self, rows: usize) -> bool {
        let last = self.lines.len() - 1;
        let target = self.row.saturating_add(rows).min(last);
        if target == self.row {
            return false;
        }
        self.move_to_row(target);
        true
    }

    /// The new `(row_off, col_off)` scroll offset that keeps the cursor
    /// visible inside an inner text `viewport` of `(width, height)` cells,
    /// moving the minimum amount and keeping `margin` cells of context around
    /// the caret when possible (the vim `scrolloff`/`sidescrolloff` idea).
    ///
    /// **Pure**: it reads the cursor and document and mutates nothing — the
    /// reducer stores the result, because [ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
    /// §1 makes scroll caller-owned model state the pure `view` reads. This
    /// is the `scroll_into_view` seam [`Editor`](../../rstui_widgets/struct.Editor.html)
    /// deliberately deferred to the caller: an editor calls this in `update`
    /// after every motion/edit and on resize, then feeds the result to
    /// `Editor::scroll`, so the caret is never scrolled off-screen and the
    /// view never shows blank space past the end.
    ///
    /// **Total**: a zero-width or zero-height axis keeps that axis's offset
    /// unchanged (nothing can be shown, so nothing moves); the result is
    /// always within `0..row_count()` / the caret's line length, and the
    /// caret is always inside the returned window for any non-zero viewport.
    #[must_use]
    pub fn scroll_into_view(
        &self,
        scroll: (usize, usize),
        viewport: (u16, u16),
        margin: u16,
    ) -> (usize, usize) {
        let (cur_row, cur_col) = (self.row, self.col);
        let (w, h) = (viewport.0 as usize, viewport.1 as usize);
        let m = margin as usize;
        // Rows are bounded by the document; columns by the caret's line
        // length plus one (the append position must be reachable).
        let row_off = Self::scroll_axis(cur_row, scroll.0, h, m, self.lines.len());
        let col_off = Self::scroll_axis(
            cur_col,
            scroll.1,
            w,
            m,
            self.line_char_len(cur_row).saturating_add(1),
        );
        (row_off, col_off)
    }

    /// The text between `a` and `b` (the pair is normalised, so order does
    /// not matter), rows joined by `'\n'` exactly as `to_string()`.
    /// Both positions are clamped into the document first, so any input is
    /// **total**. This is what a "copy/yank the selection" command reads
    /// back (the logical-selection dual of [`selected_text`](crate::selected_text)).
    #[must_use]
    pub fn span_text(&self, a: (usize, usize), b: (usize, usize)) -> String {
        let (s, e) = self.normalised_span(a, b);
        if s.0 == e.0 {
            let line = &self.lines[s.0];
            let from = self.byte_at(s.0, s.1);
            let to = self.byte_at(s.0, e.1);
            return line[from..to].to_owned();
        }
        let mut out = self.lines[s.0][self.byte_at(s.0, s.1)..].to_owned();
        for line in &self.lines[s.0 + 1..e.0] {
            out.push('\n');
            out.push_str(line);
        }
        out.push('\n');
        out.push_str(&self.lines[e.0][..self.byte_at(e.0, e.1)]);
        out
    }

    /// Deletes the characters between `a` and `b` (normalised), joining the
    /// head of the first line to the tail of the last and leaving the cursor
    /// at the (normalised) start. Returns whether anything was removed
    /// (`false` only for an empty span). Clamps both endpoints, so it is
    /// **total** — this is the primitive vim `d`/`c`/visual-delete and
    /// "type over the selection" are built from.
    pub fn delete_span(&mut self, a: (usize, usize), b: (usize, usize)) -> bool {
        let (s, e) = self.normalised_span(a, b);
        if s == e {
            return false;
        }
        let head_end = self.byte_at(s.0, s.1);
        let tail: String = self.lines[e.0][self.byte_at(e.0, e.1)..].to_owned();
        self.lines[s.0].truncate(head_end);
        self.lines[s.0].push_str(&tail);
        // Drop the fully/partly consumed rows after the first.
        if e.0 > s.0 {
            self.lines.drain(s.0 + 1..=e.0);
        }
        // The span touched exactly rows `s.0..=old e.0`; that whole stretch
        // is now the single row `s.0`. Resync the cache with one splice so
        // the CM-3 invariant the totality proptest enforces still holds.
        let new_len = self.lines[s.0].chars().count();
        self.line_lens
            .splice(s.0..=e.0.min(self.line_lens.len() - 1), [new_len]);
        self.row = s.0;
        self.col = s.1;
        self.goal_col = None;
        true
    }

    /// Replaces the span between `a` and `b` with `s` (which may contain
    /// `'\n'`), leaving the cursor just after the inserted text. Clamps both
    /// endpoints, so it is **total**. Equivalent to [`delete_span`](Self::delete_span)
    /// then [`insert_str`](Self::insert_str) — the "replace the selection"
    /// primitive.
    pub fn replace_span(&mut self, a: (usize, usize), b: (usize, usize), s: &str) {
        self.delete_span(a, b);
        self.insert_str(s);
    }

    /// Moves the cursor to `target_row`, clamping the column to that row's
    /// length while preserving the sticky goal column so a sequence of
    /// vertical moves aims at the same column throughout.
    fn move_to_row(&mut self, target_row: usize) {
        let goal = self.goal_col.unwrap_or(self.col);
        self.row = target_row;
        self.col = goal.min(self.line_char_len(target_row));
        self.goal_col = Some(goal);
    }

    /// Splits the current line at the cursor, inserting the tail as a new row
    /// and moving the cursor to its start.
    fn split_line_at_cursor(&mut self) {
        let at = self.byte_at(self.row, self.col);
        let tail = self.lines[self.row].split_off(at);
        // The line keeps its prefix (full − tail) and the tail becomes the
        // next row: a split of the cached length, no recount needed.
        let tail_len = tail.chars().count();
        self.line_lens[self.row] -= tail_len;
        self.row += 1;
        self.lines.insert(self.row, tail);
        self.line_lens.insert(self.row, tail_len);
        self.col = 0;
    }

    /// One axis of [`scroll_into_view`](Self::scroll_into_view): the minimal
    /// offset that keeps `cur` inside `[off, off + view)` with `margin` cells
    /// of context, never scrolling past `count` items (so no blank tail), yet
    /// always leaving `cur` visible. Every step is saturating, so any
    /// `(cur, off, view, margin, count)` — including a zero `view` — is total.
    fn scroll_axis(cur: usize, off: usize, view: usize, margin: usize, count: usize) -> usize {
        if view == 0 {
            return off; // nothing can be shown on this axis — do not move it.
        }
        // A margin wider than half the window is meaningless; clamp it so the
        // "too high" and "too low" bands cannot cross on a tiny viewport.
        let m = margin.min((view - 1) / 2);
        let mut off = off;
        if cur < off + m {
            off = cur.saturating_sub(m); // caret entered the top margin band
        } else if cur + m >= off + view {
            off = (cur + m + 1).saturating_sub(view); // …the bottom band
        }
        // Do not scroll past the end into blank space (the git-review bug
        // this whole seam fixes): the last screenful is the deepest offset.
        off = off.min(count.saturating_sub(view));
        // …but the no-blank clamp must never hide the caret (content shorter
        // than the viewport, or the caret near the very end).
        if cur < off {
            off = cur;
        } else if cur >= off + view {
            off = cur + 1 - view;
        }
        off
    }

    /// Clamps `a` and `b` into the document and returns them as
    /// `(start, end)` in row-major order — the shared front of every span
    /// operation, keeping each one total regardless of caller input.
    fn normalised_span(
        &self,
        a: (usize, usize),
        b: (usize, usize),
    ) -> ((usize, usize), (usize, usize)) {
        let clamp = |(r, c): (usize, usize)| {
            let r = r.min(self.lines.len() - 1);
            (r, c.min(self.line_char_len(r)))
        };
        let (a, b) = (clamp(a), clamp(b));
        if a <= b { (a, b) } else { (b, a) }
    }

    /// The character length of row `row` (the largest valid column there).
    /// `row` is always in range when called (the cursor invariant).
    ///
    /// O(1): reads the [`line_lens`](Self::line_lens) cache every mutator
    /// keeps exact, not an O(line) UTF-8 re-scan (CM-3). This is the
    /// per-keystroke (`move_*`/`set_cursor`) and per-visible-row
    /// (projecting `Editor`) hot path.
    fn line_char_len(&self, row: usize) -> usize {
        self.line_lens[row]
    }

    /// The byte offset of character index `char_idx` within `lines[row]`, or
    /// that line's `len()` for any index at or past its end.
    ///
    /// Always a valid UTF-8 boundary (it is either a `char_indices` boundary
    /// or the line length), which is what keeps every `String::insert` /
    /// `replace_range` / `split_off` above total: no caller-reachable index
    /// can land mid-codepoint. `row` is always in range when called.
    fn byte_at(&self, row: usize, char_idx: usize) -> usize {
        let line = &self.lines[row];
        line.char_indices()
            .nth(char_idx)
            .map_or(line.len(), |(byte, _)| byte)
    }
}

impl fmt::Display for TextArea {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            f.write_str(line)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_default_are_one_empty_line_cursor_at_origin() {
        assert_eq!(TextArea::new(), TextArea::default());
        let ta = TextArea::new();
        assert_eq!(ta.lines(), &[String::new()]);
        assert_eq!(ta.line(0), Some(""));
        assert_eq!(ta.line(1), None);
        assert_eq!(ta.cursor(), (0, 0));
        assert_eq!(ta.row_count(), 1);
        assert!(ta.is_empty());
        assert_eq!(ta.to_string(), "");
    }

    #[test]
    fn from_value_splits_on_newlines_and_puts_cursor_at_end() {
        let ta = TextArea::from_value("line1\nlíne2\n日本");
        assert_eq!(ta.row_count(), 3);
        assert_eq!(ta.line(0), Some("line1"));
        assert_eq!(ta.line(1), Some("líne2"));
        assert_eq!(ta.line(2), Some("日本"));
        // Cursor after the last char of the last row (char count, not bytes).
        assert_eq!(ta.cursor(), (2, 2));
        assert!(!ta.is_empty());
        assert_eq!(ta.to_string(), "line1\nlíne2\n日本");

        // Trailing and consecutive newlines produce empty rows.
        let ta = TextArea::from_value("a\n\n\nb");
        assert_eq!(ta.row_count(), 4);
        assert_eq!(ta.line(1), Some(""));
        assert_eq!(ta.cursor(), (3, 1));
    }

    #[test]
    fn insert_char_newline_splits_the_current_line_at_the_column() {
        let mut ta = TextArea::from_value("abcd");
        ta.set_cursor(0, 2); // between 'b' and 'c'
        ta.insert_char('\n');
        assert_eq!(ta.row_count(), 2);
        assert_eq!(ta.line(0), Some("ab"));
        assert_eq!(ta.line(1), Some("cd"));
        assert_eq!(ta.cursor(), (1, 0));

        // Plain insert does not split and advances the column.
        ta.insert_char('Z');
        assert_eq!(ta.line(1), Some("Zcd"));
        assert_eq!(ta.cursor(), (1, 1));
    }

    #[test]
    fn insert_str_paste_with_embedded_newlines_creates_rows() {
        let mut ta = TextArea::from_value("XYZ");
        ta.set_cursor(0, 1); // between 'X' and 'Y'
        ta.insert_str("ab\ncd\ne");
        assert_eq!(ta.row_count(), 3);
        assert_eq!(ta.line(0), Some("Xab"));
        assert_eq!(ta.line(1), Some("cd"));
        assert_eq!(ta.line(2), Some("eYZ"));
        // Cursor just after the last inserted char, before the carried tail.
        assert_eq!(ta.cursor(), (2, 1));

        // A newline-free paste is a single-line insert (TextEdit parity).
        let mut ta = TextArea::from_value("ab");
        ta.move_home();
        ta.insert_str("日本");
        assert_eq!(ta.row_count(), 1);
        assert_eq!(ta.line(0), Some("日本ab"));
        assert_eq!(ta.cursor(), (0, 2));
    }

    #[test]
    fn delete_backward_at_column_zero_joins_with_the_previous_line() {
        let mut ta = TextArea::from_value("ab\ncd");
        ta.set_cursor(1, 0);
        assert!(ta.delete_backward()); // row 1 joins onto row 0
        assert_eq!(ta.row_count(), 1);
        assert_eq!(ta.line(0), Some("abcd"));
        assert_eq!(ta.cursor(), (0, 2)); // at the join

        ta.move_doc_start();
        assert!(!ta.delete_backward()); // nothing before (0, 0)

        // Within a line it removes exactly one preceding character.
        ta.set_cursor(0, 2);
        assert!(ta.delete_backward());
        assert_eq!(ta.line(0), Some("acd"));
        assert_eq!(ta.cursor(), (0, 1));
    }

    #[test]
    fn delete_forward_at_line_end_joins_the_next_line() {
        let mut ta = TextArea::from_value("ab\ncd");
        ta.set_cursor(0, 2); // end of row 0
        assert!(ta.delete_forward()); // row 1 joins onto row 0
        assert_eq!(ta.row_count(), 1);
        assert_eq!(ta.line(0), Some("abcd"));
        assert_eq!(ta.cursor(), (0, 2)); // cursor stays put

        ta.move_doc_end();
        assert!(!ta.delete_forward()); // nothing at the very end

        // Within a line it removes exactly the character at the cursor.
        ta.set_cursor(0, 1);
        assert!(ta.delete_forward());
        assert_eq!(ta.line(0), Some("acd"));
        assert_eq!(ta.cursor(), (0, 1));
    }

    #[test]
    fn vertical_motion_clamps_column_and_sticky_goal_column_restores_it() {
        let mut ta = TextArea::from_value("aaaaaa\nbb\ncccccc");
        ta.set_cursor(0, 5); // column 5 on the long first row

        // Down through the short middle row clamps the column to 2…
        assert!(ta.move_down());
        assert_eq!(ta.cursor(), (1, 2));
        // …but the sticky goal restores column 5 on the long third row.
        assert!(ta.move_down());
        assert_eq!(ta.cursor(), (2, 5));
        // And back up the other way.
        assert!(ta.move_up());
        assert_eq!(ta.cursor(), (1, 2));
        assert!(ta.move_up());
        assert_eq!(ta.cursor(), (0, 5));

        // A horizontal move resets the goal: the column is no longer sticky.
        assert!(!ta.move_up()); // already on the first row
        ta.move_left(); // (0, 4), goal cleared
        assert!(ta.move_down());
        assert_eq!(ta.cursor(), (1, 2));
        assert!(ta.move_down());
        assert_eq!(ta.cursor(), (2, 4)); // new goal is 4, not the old 5
    }

    #[test]
    fn move_doc_start_end_and_page_up_down_clamp_totally() {
        let mut ta = TextArea::from_value("a\nbb\nccc\ndddd");
        ta.move_doc_start();
        assert_eq!(ta.cursor(), (0, 0));
        ta.move_doc_end();
        assert_eq!(ta.cursor(), (3, 4));

        // Page up past the top clamps to row 0, page down past the bottom to
        // the last row; both keep the cursor a valid index.
        ta.set_cursor(3, 4);
        assert!(ta.move_page_up(999));
        assert_eq!(ta.cursor().0, 0);
        assert!(!ta.move_page_up(999)); // already at the top
        assert!(ta.move_page_down(999));
        assert_eq!(ta.cursor().0, 3);
        assert!(!ta.move_page_down(999)); // already at the bottom
        assert!(!ta.move_page_up(0)); // a zero page is a no-op

        // A page move is sticky-column too.
        ta.set_cursor(3, 4);
        assert!(ta.move_page_up(2)); // -> row 1 ("bb"), col clamped to 2
        assert_eq!(ta.cursor(), (1, 2));
        assert!(ta.move_page_down(2)); // -> row 3, goal restores col 4
        assert_eq!(ta.cursor(), (3, 4));
    }

    #[test]
    fn set_cursor_clamps_both_row_and_column() {
        let mut ta = TextArea::from_value("ab\ncde");
        ta.set_cursor(0, 1);
        assert_eq!(ta.cursor(), (0, 1));
        ta.set_cursor(99, 99); // both out of range -> clamp, no panic
        assert_eq!(ta.cursor(), (1, 3));
        ta.set_cursor(1, 99); // row ok, column past end -> clamp column
        assert_eq!(ta.cursor(), (1, 3));
        ta.set_cursor(99, 0); // row past end -> clamp row, column kept
        assert_eq!(ta.cursor(), (1, 0));
    }

    #[test]
    fn editing_around_multibyte_characters_stays_on_char_boundaries() {
        // "é" and "日" are multi-byte; cursor math must stay in characters.
        let mut ta = TextArea::from_value("é日\nx😀y");
        assert_eq!(ta.cursor(), (1, 3));

        ta.set_cursor(0, 1); // between "é" and "日"
        ta.insert_char('z');
        assert_eq!(ta.line(0), Some("éz日"));
        assert_eq!(ta.cursor(), (0, 2));

        // Backspace removes exactly the 'z', not a stray byte.
        assert!(ta.delete_backward());
        assert_eq!(ta.line(0), Some("é日"));
        assert_eq!(ta.cursor(), (0, 1));

        // Splitting a line between multi-byte chars keeps both rows valid.
        ta.set_cursor(1, 1); // after "x", before the emoji
        ta.insert_char('\n');
        assert_eq!(ta.line(1), Some("x"));
        assert_eq!(ta.line(2), Some("😀y"));
        assert_eq!(ta.cursor(), (2, 0));

        // Delete-forward removes the whole emoji codepoint.
        assert!(ta.delete_forward());
        assert_eq!(ta.line(2), Some("y"));
        assert_eq!(ta.cursor(), (2, 0));
    }

    #[test]
    fn set_value_replaces_the_document_and_clear_resets_it() {
        let mut ta = TextArea::from_value("old");
        ta.move_doc_start();
        ta.set_value("new\ndoc");
        assert_eq!(ta.row_count(), 2);
        assert_eq!(ta.cursor(), (1, 3));

        ta.clear();
        assert_eq!(ta, TextArea::new());
        assert!(ta.is_empty());
        assert_eq!(ta.cursor(), (0, 0));
    }

    /// The totality property (the iter-25 rule, mirroring
    /// [`TextEdit`](crate::text_edit::TextEdit)'s): any sequence of any
    /// operation — over plain ASCII, embedded newlines, *and* multi-byte
    /// UTF-8 seeds — never panics and always leaves a valid `(row, col)`
    /// cursor whose byte offset on its row is a real UTF-8 boundary.
    #[test]
    fn any_sequence_of_operations_is_total_and_keeps_a_valid_cursor() {
        // Fixed-seed LCG keeps the run deterministic with no rand dep
        // (rstui-core is dependency-free) — the same technique text_edit.rs
        // and focus.rs use.
        let mut state: u64 = 0x0bad_f00d_dead_beef;
        let mut rng = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };

        let seeds = ["", "abc", "line1\nlíne2\n日本\n", "a\n\n\nb", "x😀\ny"];
        let inserts = ['z', 'é', '日', '\n', '😀'];

        for seed in seeds {
            let mut ta = TextArea::from_value(seed);
            for _ in 0..3_000 {
                match rng() % 22 {
                    0 => ta.insert_char(inserts[(rng() % 5) as usize]),
                    1 => ta.insert_str("ab\n日"),
                    2 => ta.insert_newline(),
                    3 => {
                        ta.delete_backward();
                    }
                    4 => {
                        ta.delete_forward();
                    }
                    5 => {
                        ta.move_left();
                    }
                    6 => {
                        ta.move_right();
                    }
                    7 => {
                        ta.move_up();
                    }
                    8 => {
                        ta.move_down();
                    }
                    9 => ta.move_home(),
                    10 => ta.move_end(),
                    11 => ta.move_doc_start(),
                    12 => ta.move_doc_end(),
                    13 => {
                        ta.move_page_up((rng() % 4) as usize);
                    }
                    14 => {
                        ta.move_page_down((rng() % 4) as usize);
                    }
                    15 => ta.set_cursor((rng() % 5) as usize, (rng() % 7) as usize),
                    16 => ta.clear(),
                    17 => {
                        let _ = ta.span_text(
                            ((rng() % 5) as usize, (rng() % 7) as usize),
                            ((rng() % 5) as usize, (rng() % 7) as usize),
                        );
                    }
                    18 => {
                        ta.delete_span(
                            ((rng() % 5) as usize, (rng() % 7) as usize),
                            ((rng() % 5) as usize, (rng() % 7) as usize),
                        );
                    }
                    19 => ta.replace_span(
                        ((rng() % 5) as usize, (rng() % 7) as usize),
                        ((rng() % 5) as usize, (rng() % 7) as usize),
                        "x\n値",
                    ),
                    20 => {
                        let _ = ta.scroll_into_view(
                            ((rng() % 4) as usize, (rng() % 4) as usize),
                            ((rng() % 6) as u16, (rng() % 5) as u16),
                            (rng() % 3) as u16,
                        );
                    }
                    _ => ta.set_value("re\nset 値"),
                }

                // Invariant 1: the document is never empty.
                assert!(!ta.lines().is_empty(), "lines emptied");
                let (r, c) = ta.cursor();
                // Invariant 2: the row is a valid index.
                assert!(r < ta.row_count(), "row escaped 0..row_count");
                // Invariant 3: the column is a valid character index there.
                let line = ta.line(r).expect("row in range");
                assert!(c <= line.chars().count(), "col escaped 0..=len");
                // Invariant 4: that index maps to a real UTF-8 boundary, so
                // the next edit cannot split a codepoint.
                let byte = ta.byte_at(r, c);
                assert!(line.is_char_boundary(byte), "cursor landed mid-codepoint");
                // Invariant 5 (CM-3): the `line_lens` cache is *exactly* the
                // per-row char count after every operation — the contract
                // that makes the O(1) `line_char_len` correct. This is the
                // gate-enforced desync detector (the CM-2 precedent): any
                // mutator that ever leaves it stale fails here, across all
                // 22 ops × 3000 iters × 5 seeds (paste/clear/set/join/split
                // and the span edits included), so a desync can never ship
                // silently.
                assert_eq!(
                    ta.line_lens.len(),
                    ta.lines.len(),
                    "line_lens length desynced from lines"
                );
                for (i, l) in ta.lines.iter().enumerate() {
                    assert_eq!(
                        ta.line_lens[i],
                        l.chars().count(),
                        "line_lens[{i}] desynced from its line's char count"
                    );
                }
                // Invariant 6: `scroll_into_view` is pure and *always* keeps
                // the caret inside the returned window and the offsets within
                // bounds — for every non-zero viewport, any prior offset, any
                // margin. This is the gate-enforced proof of the fix the
                // git-review scroll bug needed (the caret can never be
                // scrolled off-screen, the view never runs past the end).
                for (vw, vh) in [(1u16, 1u16), (3, 2), (7, 5), (40, 20)] {
                    let (ro, co) = ta.scroll_into_view((r + 9, c + 9), (vw, vh), vw.min(vh));
                    let (w, h) = (vw as usize, vh as usize);
                    assert!(
                        ro <= r && r < ro + h,
                        "row caret {r} escaped [{ro},{ro}+{h})"
                    );
                    assert!(
                        co <= c && c < co + w,
                        "col caret {c} escaped [{co},{co}+{w})"
                    );
                    assert!(ro < ta.row_count(), "row_off {ro} past document");
                }
            }
            // Reaching here for every seed proves no operation panicked.
        }
    }

    #[test]
    fn scroll_into_view_follows_the_caret_without_blank_past_the_end() {
        let mut ta = TextArea::from_value("0\n1\n2\n3\n4\n5\n6\n7\n8\n9");
        // Caret deep in the document, a 4-row window, 1 row of margin.
        ta.set_cursor(7, 0);
        let (ro, _) = ta.scroll_into_view((0, 0), (10, 4), 1);
        // Window must contain row 7 and not run past row 9 into blank space:
        // deepest offset is row_count(10) - height(4) = 6.
        assert!(ro <= 7 && 7 < ro + 4);
        assert!(ro <= 6, "scrolled into blank space past the end");

        // Moving back to the top scrolls up so the caret is visible again.
        ta.set_cursor(0, 0);
        assert_eq!(ta.scroll_into_view((ro, 0), (10, 4), 1).0, 0);
    }

    #[test]
    fn scroll_into_view_is_total_and_keeps_short_content_pinned() {
        let mut ta = TextArea::from_value("a\nb");
        ta.set_cursor(1, 1);
        // Content shorter than the viewport: never scroll, stay at the top.
        assert_eq!(ta.scroll_into_view((0, 0), (20, 10), 2), (0, 0));
        // A zero-size axis is a no-op on that axis (nothing can be shown).
        assert_eq!(ta.scroll_into_view((3, 5), (0, 0), 0), (3, 5));
        // A long line scrolls horizontally so the caret column is visible.
        let mut wide = TextArea::from_value(&"x".repeat(200)[..]);
        wide.set_cursor(0, 150);
        let (_, co) = wide.scroll_into_view((0, 0), (20, 1), 3);
        assert!(co <= 150 && 150 < co + 20);
    }

    #[test]
    fn span_text_reads_within_and_across_lines_total() {
        let ta = TextArea::from_value("abc\nédef\nghi");
        // Within one row.
        assert_eq!(ta.span_text((0, 1), (0, 3)), "bc");
        // Across rows, multi-byte safe, order-independent (reversed args).
        assert_eq!(ta.span_text((2, 1), (0, 2)), "c\nédef\ng");
        // Out-of-range endpoints clamp instead of panicking.
        assert_eq!(ta.span_text((0, 0), (99, 99)), "abc\nédef\nghi");
    }

    #[test]
    fn delete_and_replace_span_join_lines_and_place_the_cursor() {
        let mut ta = TextArea::from_value("hello\nbrave\nworld");
        // Delete from (0,2) to (2,2): "he" + "rld".
        assert!(ta.delete_span((0, 2), (2, 2)));
        assert_eq!(ta.row_count(), 1);
        assert_eq!(ta.line(0), Some("herld"));
        assert_eq!(ta.cursor(), (0, 2)); // at the normalised start
        // An empty span removes nothing.
        assert!(!ta.delete_span((0, 3), (0, 3)));

        // Replace places the cursor just after the inserted text and keeps
        // the line_lens cache exact (the CM-3 invariant).
        let mut ta = TextArea::from_value("one two three");
        ta.replace_span((0, 4), (0, 7), "2\n2"); // replace "two"
        assert_eq!(ta.line(0), Some("one 2"));
        assert_eq!(ta.line(1), Some("2 three"));
        assert_eq!(ta.cursor(), (1, 1));
        for (i, l) in ta.lines().iter().enumerate() {
            assert_eq!(ta.line_lens[i], l.chars().count());
        }
    }
}
