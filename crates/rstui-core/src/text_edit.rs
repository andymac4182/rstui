//! Single-line editable text as caller-owned model state.
//!
//! [`TextEdit`] is the editing-side dual of [`FocusRing`](crate::focus::FocusRing):
//! a pure value type that lives as a *field in the application's model*,
//! mutated only by `update`, and read by the pure `view`. The `Input`
//! widget (in `rstui-widgets`) is a pure projection of one — it draws
//! [`value`](TextEdit::value) and a cursor at [`cursor`](TextEdit::cursor)
//! and never edits anything itself, exactly as `Checkbox`/`Radio` project a
//! caller-owned `bool`. Per
//! [ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
//! this is *forced* by rstui's pure-`view` / immediate-mode design: a widget
//! is handed only a [`Buffer`](crate::buffer::Buffer) at render time, so it
//! can neither own the text being typed nor mutate it on a keystroke. The
//! reducer owns the edit.
//!
//! Like [`focus`](crate::focus), this module is **optional**: an app may keep
//! a `String` and a cursor `usize` of its own and never name a type from
//! here. `TextEdit` exists only to turn the cursor/UTF-8-boundary
//! bookkeeping every such app re-derives — and routinely gets wrong, because
//! Rust strings are byte-indexed while a terminal renders *characters* — into
//! one reusable, panic-free primitive:
//!
//! - The cursor is a **character index** in `0..=len()`, never a byte
//!   offset, matching rstui's single-`char` cell model (text width is a
//!   `char` count everywhere). The widget maps that index straight to a
//!   column; the byte math stays internal.
//! - Every method is **total** — no input, including a paste of arbitrary
//!   UTF-8 or an out-of-range [`set_cursor`](TextEdit::set_cursor), can
//!   panic or leave the cursor mid-codepoint (the iter-25 "a pure projection
//!   must be total" rule, the same guarantee [`FocusRing`](crate::focus::FocusRing)
//!   gives focus).
//!
//! It is single-line on purpose: a newline is just another inserted
//! character here, and multi-line editing (a future `TextArea`) is a
//! separate model, not a flag on this one.
//!
//! This is **app/widget** state and is unrelated to terminal-window focus
//! (`Event::FocusGained` / `Event::FocusLost`); the reducer decides when an
//! `Input` is focused (via [`focus`](crate::focus)) and routes keystrokes to
//! its `TextEdit` accordingly.
//!
//! # Example
//!
//! ```
//! use rstui_core::text_edit::TextEdit;
//!
//! // The app stores one per text field in its model. Pre-filling puts the
//! // cursor at the end, ready to append (the usual "edit this value" case).
//! let mut name = TextEdit::from_value("Ada");
//! assert_eq!(name.value(), "Ada");
//! assert_eq!(name.cursor(), 3);
//!
//! // `update` maps key messages to edits. Totality means no sequence panics.
//! name.insert_char('!'); // "Ada!" cursor 4
//! name.move_home(); // cursor 0
//! name.delete_forward(); // "da!" cursor 0
//! assert_eq!(name.value(), "da!");
//!
//! // Paste is just an insert of arbitrary UTF-8 — char boundaries are safe.
//! name.move_end();
//! name.insert_str(" 日本"); // multi-byte, cursor counts characters
//! assert_eq!(name.value(), "da! 日本");
//! assert_eq!(name.cursor(), 6);
//!
//! // Backspace over a multi-byte char removes exactly one character.
//! assert!(name.delete_backward());
//! assert_eq!(name.value(), "da! 日");
//!
//! // Out-of-range cursor placement clamps instead of panicking.
//! name.set_cursor(999);
//! assert_eq!(name.cursor(), name.len());
//! ```

/// A single-line editable string plus a character-indexed cursor.
///
/// `TextEdit` is a **pure value type** designed to live as a field in the
/// application's model (it derives [`Default`] so it drops into a
/// `#[derive(Default)]` model as an empty field). It owns *no* terminal,
/// runtime, or widget state: `update` mutates it in response to key/paste
/// messages the app maps, and the pure `view` only reads
/// [`value`](Self::value) / [`cursor`](Self::cursor) to project it through
/// an `Input` widget. The framework never touches it.
///
/// The cursor is a **character index** with the invariant `0 <= cursor <=
/// len()` (one past the last character means "append here"). Every mutator
/// upholds it, and every method is **total**: arbitrary input — a multi-byte
/// paste, a backspace at the start, an out-of-range
/// [`set_cursor`](Self::set_cursor) — is well-defined and never panics or
/// strands the cursor inside a UTF-8 codepoint.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextEdit {
    value: String,
    /// Character index in `0..=value.chars().count()`.
    cursor: usize,
}

impl TextEdit {
    /// An empty field with the cursor at the start.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A field pre-filled with `value`, cursor placed **after** the last
    /// character.
    ///
    /// Cursor-at-end matches the usual expectation when a form opens onto an
    /// existing value (ready to append); call [`move_home`](Self::move_home)
    /// or [`set_cursor`](Self::set_cursor) in `update` if a different start
    /// is wanted.
    #[must_use]
    pub fn from_value(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    /// The current text.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The cursor as a **character index** in `0..=len()` — the column the
    /// projecting widget draws the caret at (before any horizontal scroll
    /// the widget applies for itself).
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The length in **characters** (not bytes), i.e. the largest valid
    /// cursor index.
    #[must_use]
    pub fn len(&self) -> usize {
        self.value.chars().count()
    }

    /// Whether the text is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Replaces the whole text, moving the cursor to the **end** (consistent
    /// with [`from_value`](Self::from_value)).
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.cursor = self.len();
    }

    /// Empties the text and returns the cursor to the start.
    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }

    /// Moves the cursor to character index `char_index`, clamped to
    /// `0..=len()`.
    ///
    /// Clamping (rather than panicking on an out-of-range index) is the
    /// totality rule applied to cursor placement: an app mapping a mouse
    /// click column to an index can pass any value safely.
    pub fn set_cursor(&mut self, char_index: usize) {
        self.cursor = char_index.min(self.len());
    }

    /// Moves the cursor one character left; returns whether it moved
    /// (`false` if already at the start).
    pub fn move_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        true
    }

    /// Moves the cursor one character right; returns whether it moved
    /// (`false` if already at the end).
    pub fn move_right(&mut self) -> bool {
        if self.cursor >= self.len() {
            return false;
        }
        self.cursor += 1;
        true
    }

    /// Moves the cursor to the start.
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    /// Moves the cursor to just past the last character.
    pub fn move_end(&mut self) {
        self.cursor = self.len();
    }

    /// Inserts `c` at the cursor and advances the cursor past it.
    pub fn insert_char(&mut self, c: char) {
        let at = self.byte_at(self.cursor);
        self.value.insert(at, c);
        self.cursor += 1;
    }

    /// Inserts `s` at the cursor and advances the cursor past all of it.
    ///
    /// This is the paste path: `s` may be arbitrary UTF-8 (including
    /// newlines, which are kept verbatim — `TextEdit` is single-line by
    /// convention, not by filtering) and the cursor advances by its
    /// **character** count.
    pub fn insert_str(&mut self, s: &str) {
        let at = self.byte_at(self.cursor);
        self.value.insert_str(at, s);
        self.cursor += s.chars().count();
    }

    /// Deletes the character before the cursor (Backspace) and moves the
    /// cursor back over it; returns whether anything was deleted (`false` at
    /// the start).
    pub fn delete_backward(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let end = self.byte_at(self.cursor);
        let start = self.byte_at(self.cursor - 1);
        self.value.replace_range(start..end, "");
        self.cursor -= 1;
        true
    }

    /// Deletes the character at the cursor (Delete), leaving the cursor in
    /// place; returns whether anything was deleted (`false` at the end).
    pub fn delete_forward(&mut self) -> bool {
        if self.cursor >= self.len() {
            return false;
        }
        let start = self.byte_at(self.cursor);
        let end = self.byte_at(self.cursor + 1);
        self.value.replace_range(start..end, "");
        true
    }

    /// The byte offset of character index `char_idx`, or `value.len()` for
    /// any index at or past the end.
    ///
    /// Always a valid UTF-8 boundary (it is either a `char_indices` boundary
    /// or the string length), which is what keeps every `String::insert` /
    /// `replace_range` above total: no caller-reachable index can land
    /// mid-codepoint.
    fn byte_at(&self, char_idx: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_idx)
            .map_or(self.value.len(), |(byte, _)| byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_default_are_empty_with_cursor_at_start() {
        assert_eq!(TextEdit::new(), TextEdit::default());
        let te = TextEdit::new();
        assert_eq!(te.value(), "");
        assert_eq!(te.cursor(), 0);
        assert_eq!(te.len(), 0);
        assert!(te.is_empty());
    }

    #[test]
    fn from_value_places_the_cursor_at_the_end() {
        let te = TextEdit::from_value("hello");
        assert_eq!(te.value(), "hello");
        assert_eq!(te.cursor(), 5);
        assert_eq!(te.len(), 5);
        assert!(!te.is_empty());
    }

    #[test]
    fn insert_char_inserts_at_the_cursor_and_advances_it() {
        let mut te = TextEdit::new();
        te.insert_char('a');
        te.insert_char('c');
        assert_eq!(te.value(), "ac");
        assert_eq!(te.cursor(), 2);

        // Insert in the middle.
        te.move_left();
        te.insert_char('b');
        assert_eq!(te.value(), "abc");
        assert_eq!(te.cursor(), 2);
    }

    #[test]
    fn insert_str_is_the_paste_path_and_counts_characters() {
        let mut te = TextEdit::from_value("ab");
        te.move_home();
        te.insert_str("XY");
        assert_eq!(te.value(), "XYab");
        assert_eq!(te.cursor(), 2);

        // Arbitrary UTF-8, including a newline kept verbatim.
        te.move_end();
        te.insert_str(" 日本\n");
        assert_eq!(te.value(), "XYab 日本\n");
        assert_eq!(te.cursor(), 8);
    }

    #[test]
    fn cursor_movement_is_total_and_clamps() {
        let mut te = TextEdit::from_value("hi");
        assert_eq!(te.cursor(), 2);
        assert!(te.move_left());
        assert!(te.move_left());
        assert!(!te.move_left()); // already at the start
        assert_eq!(te.cursor(), 0);
        assert!(te.move_right());
        assert!(te.move_right());
        assert!(!te.move_right()); // already at the end
        assert_eq!(te.cursor(), 2);

        te.move_home();
        assert_eq!(te.cursor(), 0);
        te.move_end();
        assert_eq!(te.cursor(), 2);

        te.set_cursor(1);
        assert_eq!(te.cursor(), 1);
        te.set_cursor(999); // out of range -> clamp, no panic
        assert_eq!(te.cursor(), 2);
    }

    #[test]
    fn delete_backward_and_forward_report_whether_they_changed_anything() {
        let mut te = TextEdit::from_value("abc");
        te.move_home();
        assert!(!te.delete_backward()); // nothing before the start
        assert!(te.delete_forward()); // removes 'a'
        assert_eq!(te.value(), "bc");
        assert_eq!(te.cursor(), 0);

        te.move_end();
        assert!(!te.delete_forward()); // nothing at the end
        assert!(te.delete_backward()); // removes 'c'
        assert_eq!(te.value(), "b");
        assert_eq!(te.cursor(), 1);
    }

    #[test]
    fn editing_around_multibyte_characters_stays_on_char_boundaries() {
        // "é" and "日" are multi-byte; cursor math must stay in characters.
        let mut te = TextEdit::from_value("é日");
        assert_eq!(te.len(), 2);
        assert_eq!(te.cursor(), 2);

        te.move_left(); // between é and 日
        assert_eq!(te.cursor(), 1);
        te.insert_char('x');
        assert_eq!(te.value(), "éx日");
        assert_eq!(te.cursor(), 2);

        // Backspace removes exactly the 'x', not a stray byte.
        assert!(te.delete_backward());
        assert_eq!(te.value(), "é日");
        assert_eq!(te.cursor(), 1);

        // Delete-forward removes the whole "日" codepoint.
        assert!(te.delete_forward());
        assert_eq!(te.value(), "é");
        assert_eq!(te.cursor(), 1);
    }

    #[test]
    fn set_value_replaces_text_and_moves_cursor_to_the_end() {
        let mut te = TextEdit::from_value("old");
        te.move_home();
        te.set_value("longer");
        assert_eq!(te.value(), "longer");
        assert_eq!(te.cursor(), 6);

        te.clear();
        assert_eq!(te, TextEdit::new());
        assert!(te.is_empty());
        assert_eq!(te.cursor(), 0);
    }

    /// The totality property (the iter-25 rule, mirroring
    /// [`FocusRing`](crate::focus::FocusRing)'s): any sequence of any
    /// operation — over plain ASCII *and* multi-byte UTF-8 seeds — never
    /// panics and always leaves the cursor a valid character index whose
    /// byte offset is a real UTF-8 boundary.
    #[test]
    fn any_sequence_of_operations_is_total_and_keeps_a_valid_cursor() {
        // Fixed-seed LCG keeps the run deterministic with no rand dep
        // (rstui-core is dependency-free) — the same technique focus.rs uses.
        let mut state: u64 = 0x0bad_f00d_dead_beef;
        let mut rng = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };

        let seeds = ["", "abc", "héllo wörld", "日本語テキスト", "a😀b"];
        let inserts = ['z', 'é', '日', '\n', '😀'];

        for seed in seeds {
            let mut te = TextEdit::from_value(seed);
            for _ in 0..3_000 {
                match rng() % 11 {
                    0 => te.insert_char(inserts[(rng() % 5) as usize]),
                    1 => te.insert_str("ab日"),
                    2 => {
                        te.delete_backward();
                    }
                    3 => {
                        te.delete_forward();
                    }
                    4 => {
                        te.move_left();
                    }
                    5 => {
                        te.move_right();
                    }
                    6 => te.move_home(),
                    7 => te.move_end(),
                    8 => te.set_cursor((rng() % 9) as usize),
                    9 => te.clear(),
                    _ => te.set_value("reset 値"),
                }

                // Invariant 1: cursor is a valid character index.
                assert!(te.cursor() <= te.len(), "cursor escaped 0..=len");
                // Invariant 2: that index maps to a real UTF-8 boundary, so
                // the next edit cannot split a codepoint.
                let byte = te.byte_at(te.cursor());
                assert!(
                    te.value().is_char_boundary(byte),
                    "cursor landed mid-codepoint"
                );
            }
            // Reaching here for every seed proves no operation panicked.
        }
    }
}
