//! Multi-line editable text as caller-owned model state.
//!
//! [`TextArea`] is the **multi-line dual of [`TextEdit`](crate::text_edit::TextEdit)**:
//! the same pure-value, total, caller-owned editing model, but a *document*
//! of logical lines plus a `(row, col)` cursor instead of a single string
//! plus a character index. Like [`TextEdit`] it lives as a *field in the
//! application's model*, is mutated only by `update`, and is read by the pure
//! `view`; the `Editor` widget (in `rstui-widgets`) is a pure projection of
//! one — it draws [`lines`](TextArea::lines) and a caret at
//! [`cursor`](TextArea::cursor) and never edits anything itself, exactly as
//! `Input` projects a caller-owned [`TextEdit`]. Per
//! [ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
//! this is *forced* by rstui's pure-`view` / immediate-mode design: a widget
//! is handed only a [`Buffer`](crate::buffer::Buffer) at render time, so it
//! can neither own the text being typed nor mutate it on a keystroke. The
//! reducer owns the edit.
//!
//! [`TextEdit`] records the decision verbatim: multi-line editing is *"a
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
//!   [`TextEdit`] gives single-line editing).
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
        self.row = self.lines.len() - 1;
        self.col = self.line_char_len(self.row);
        self.goal_col = None;
    }

    /// Empties the document back to one empty line and returns the cursor to
    /// the origin.
    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
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
            // pasted text and the re-attached tail.
            self.col = self.line_char_len(self.row);
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
            self.col -= 1;
            true
        } else if self.row > 0 {
            let current = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.line_char_len(self.row);
            self.lines[self.row].push_str(&current);
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
            true
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
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
        self.row += 1;
        self.lines.insert(self.row, tail);
        self.col = 0;
    }

    /// The character length of row `row` (the largest valid column there).
    /// `row` is always in range when called (the cursor invariant).
    fn line_char_len(&self, row: usize) -> usize {
        self.lines[row].chars().count()
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
                match rng() % 18 {
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
            }
            // Reaching here for every seed proves no operation panicked.
        }
    }
}
