//! Readline / emacs-style line editing for the chat composer.
//!
//! The composer is a plain [`TextArea`] (the multi-line text model from
//! `rstui-core`); it knows how to insert, delete, and move a `(row, col)`
//! cursor, but nothing about *words*, a *kill ring*, *transposition*, or
//! *undo*. [`ReadlineState`] is the small companion the [`ChatApp`] owns
//! beside the composer that adds exactly those: the readline word/kill/yank/
//! transpose/case/undo vocabulary every shell user already has in their
//! fingers (`Ctrl+A`, `Ctrl+W`, `Alt+Backspace`, `Ctrl+Y`, …).
//!
//! [`ChatApp`]: crate::app::ChatApp
//!
//! Every operation here is a method on [`ReadlineState`] taking `&mut
//! TextArea`; `app::chat_key` only ever *maps a key to one of them*, so the
//! composer's text keys stay raw exactly as ADR 0015 mandates for the deeply
//! contextual surfaces. Keeping the logic here — not inline in the giant
//! `chat_key` match — also makes it **unit-testable headlessly**: a test
//! builds a `TextArea` + a `ReadlineState` and drives the operations
//! directly, no `ChatApp`, no terminal.
//!
//! Like [`TextArea`] itself every operation is **total**: it only ever calls
//! `TextArea`'s own total mutators ([`delete_span`](TextArea::delete_span) /
//! [`insert_str`](TextArea::insert_str) / [`set_cursor`](TextArea::set_cursor),
//! which clamp every index) over positions derived from the live document,
//! so no key sequence — over ASCII, embedded newlines, or multi-byte UTF-8 —
//! can panic or strand the cursor.
//!
//! Three readline subtleties are handled by the small amount of state here:
//!
//! - **The kill ring** persists across lines (a `Ctrl+W` here, a `Ctrl+Y`
//!   three prompts later). Consecutive kills *coalesce* into one ring entry —
//!   forward kills append, backward kills prepend — so `Ctrl+W Ctrl+W` then
//!   `Ctrl+Y` yanks both words. `Alt+Y` (yank-pop) rotates through the ring.
//! - **Undo** ([`Ctrl+_`](ReadlineState::undo)) is a bounded snapshot stack.
//!   A *run* of typing collapses into one undo step (you don't undo a
//!   sentence one letter at a time); a kill, a yank, a transpose each get
//!   their own. It is reset per *line* — recalling history or submitting
//!   starts a fresh undo history, the readline rule.
//! - The **last command** is remembered so those two — kill coalescing and
//!   yank-pop — and the typing-run undo coalescing know what just happened.

use rstui_core::TextArea;

/// A `(row, col)` document position, matching [`TextArea::cursor`].
type Pos = (usize, usize);

/// Most recent kills retained in the ring (`Ctrl+Y` / `Alt+Y`). Generous for
/// a chat composer; bounded so a long session cannot grow it without limit.
const KILL_RING_MAX: usize = 32;

/// Most recent composer snapshots retained for undo (`Ctrl+_`). One per
/// edit *group* (a typing run is one group), so this spans far more than 64
/// keystrokes; bounded so the stack cannot grow without limit.
const UNDO_MAX: usize = 64;

/// What the previous composer command was — the minimum state readline needs
/// to *coalesce* consecutive same-kind edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LastCmd {
    /// No command yet, or the line was just reset (history recall / submit).
    #[default]
    None,
    /// A character / paste / newline insertion — a run of these is one undo
    /// group.
    Insert,
    /// A `Backspace` / `Delete` — a run of these is one undo group.
    Delete,
    /// A kill (`Ctrl+W/K/U`, `Alt+D`, …) — the next kill appends/prepends to
    /// the same ring entry instead of pushing a new one.
    Kill,
    /// A yank (`Ctrl+Y`) or yank-pop (`Alt+Y`) — `Alt+Y` only rotates the
    /// ring when it *immediately* follows one of these.
    Yank,
    /// Anything else (a cursor move, undo, …) — breaks every coalescing run.
    Other,
}

/// A captured composer state for [`undo`](ReadlineState::undo): the whole
/// text plus the cursor. The composer is small (one chat prompt), so a flat
/// `String` snapshot is cheaper than any structural delta.
type Snapshot = (String, Pos);

/// The readline editing companion of the composer's [`TextArea`].
///
/// Holds the kill ring, the undo stack, and the last-command bookkeeping;
/// every readline composer key resolves to one method here. Created empty
/// ([`Default`]) and owned for the whole session by [`ChatApp`](crate::app::ChatApp).
#[derive(Debug, Default)]
pub(crate) struct ReadlineState {
    /// The kill ring, oldest first; `Ctrl+Y` yanks the last (newest).
    kill_ring: Vec<String>,
    /// Index into [`kill_ring`](Self::kill_ring) that `Alt+Y` (yank-pop)
    /// currently points at; reset to the newest entry by every fresh kill or
    /// `Ctrl+Y`, then rotated backward by each `Alt+Y`.
    ring_pos: usize,
    /// The document span the most recent yank occupies, so `Alt+Y` can
    /// replace it in place. `None` until the first yank of a sequence.
    yank_span: Option<(Pos, Pos)>,
    /// The undo snapshot stack — one entry per edit group, oldest first.
    undo: Vec<Snapshot>,
    /// What the previous composer command was (for coalescing).
    last: LastCmd,
}

impl ReadlineState {
    /// A fresh, empty editing companion.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // -- coalescing / bookkeeping ------------------------------------------

    /// Pushes a composer snapshot onto the undo stack, skipping a no-op
    /// (identical to the top) and capping the stack at [`UNDO_MAX`].
    fn snapshot(&mut self, ta: &TextArea) {
        let snap: Snapshot = (ta.to_string(), ta.cursor());
        if self.undo.last() == Some(&snap) {
            return;
        }
        self.undo.push(snap);
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
    }

    /// Snapshots before an insertion unless the previous command was also an
    /// insertion — so a *run* of typing is one undo group.
    fn before_insert(&mut self, ta: &TextArea) {
        if self.last != LastCmd::Insert {
            self.snapshot(ta);
        }
    }

    /// Snapshots before a `Backspace`/`Delete` unless the previous command
    /// was also one — so a *run* of deletion is one undo group.
    fn before_delete(&mut self, ta: &TextArea) {
        if self.last != LastCmd::Delete {
            self.snapshot(ta);
        }
    }

    /// Starts a fresh editing context: clears the undo history and yank
    /// state. Called when the composer's *line* is replaced wholesale —
    /// recalling a history entry or submitting — because readline scopes
    /// undo to the line being edited. The **kill ring is kept** (it is
    /// global and outlives any one prompt).
    pub fn reset_line(&mut self) {
        self.undo.clear();
        self.yank_span = None;
        self.last = LastCmd::None;
    }

    /// Breaks every coalescing run without otherwise touching state — used by
    /// keys that are neither a move nor an edit (e.g. redraw) so the next
    /// kill / keystroke starts a fresh group.
    pub fn break_sequence(&mut self) {
        self.last = LastCmd::Other;
    }

    // -- insertion / deletion (undo-tracked wrappers) ----------------------

    /// Inserts one character at the cursor (plain typing).
    pub fn insert_char(&mut self, ta: &mut TextArea, c: char) {
        self.before_insert(ta);
        ta.insert_char(c);
        self.last = LastCmd::Insert;
    }

    /// Splits the line at the cursor (`Shift+Enter`).
    pub fn insert_newline(&mut self, ta: &mut TextArea) {
        self.before_insert(ta);
        ta.insert_newline();
        self.last = LastCmd::Insert;
    }

    /// Inserts a (possibly multi-line) string at the cursor (paste). Always
    /// its own undo group — a paste is a distinct action from the typing
    /// around it.
    pub fn insert_str(&mut self, ta: &mut TextArea, s: &str) {
        if s.is_empty() {
            return;
        }
        self.snapshot(ta);
        ta.insert_str(s);
        self.last = LastCmd::Insert;
    }

    /// Deletes the character before the cursor (`Backspace` / `Ctrl+H`).
    pub fn delete_backward(&mut self, ta: &mut TextArea) {
        self.before_delete(ta);
        ta.delete_backward();
        self.last = LastCmd::Delete;
    }

    /// Deletes the character at the cursor (`Delete` / `Ctrl+D`).
    pub fn delete_forward(&mut self, ta: &mut TextArea) {
        self.before_delete(ta);
        ta.delete_forward();
        self.last = LastCmd::Delete;
    }

    // -- cursor movement ---------------------------------------------------

    /// Moves the cursor one character left (`Ctrl+B`).
    pub fn move_left(&mut self, ta: &mut TextArea) {
        ta.move_left();
        self.last = LastCmd::Other;
    }

    /// Moves the cursor one character right (`Ctrl+F`).
    pub fn move_right(&mut self, ta: &mut TextArea) {
        ta.move_right();
        self.last = LastCmd::Other;
    }

    /// Moves the cursor up one row.
    pub fn move_up(&mut self, ta: &mut TextArea) {
        ta.move_up();
        self.last = LastCmd::Other;
    }

    /// Moves the cursor down one row.
    pub fn move_down(&mut self, ta: &mut TextArea) {
        ta.move_down();
        self.last = LastCmd::Other;
    }

    /// Moves the cursor to the start of the line (`Ctrl+A`).
    pub fn move_home(&mut self, ta: &mut TextArea) {
        ta.move_home();
        self.last = LastCmd::Other;
    }

    /// Moves the cursor to the end of the line (`Ctrl+E`).
    pub fn move_end(&mut self, ta: &mut TextArea) {
        ta.move_end();
        self.last = LastCmd::Other;
    }

    /// Moves the cursor to the very start of the composer (`Alt+<`).
    pub fn move_doc_start(&mut self, ta: &mut TextArea) {
        ta.move_doc_start();
        self.last = LastCmd::Other;
    }

    /// Moves the cursor to the very end of the composer (`Alt+>`).
    pub fn move_doc_end(&mut self, ta: &mut TextArea) {
        ta.move_doc_end();
        self.last = LastCmd::Other;
    }

    /// Moves the cursor forward to the end of the next word (`Alt+F` /
    /// `Ctrl+Right`). A word is a run of alphanumeric characters; motion
    /// crosses line boundaries, treating the newline as a separator.
    pub fn word_right(&mut self, ta: &mut TextArea) {
        let (r, c) = word_forward(ta, ta.cursor(), is_wordchar);
        ta.set_cursor(r, c);
        self.last = LastCmd::Other;
    }

    /// Moves the cursor backward to the start of the previous word (`Alt+B`
    /// / `Ctrl+Left`).
    pub fn word_left(&mut self, ta: &mut TextArea) {
        let (r, c) = word_backward(ta, ta.cursor(), is_wordchar);
        ta.set_cursor(r, c);
        self.last = LastCmd::Other;
    }

    // -- kill ring ---------------------------------------------------------

    /// Pushes killed `text` onto the ring. Consecutive kills coalesce into
    /// the newest entry: a forward kill appends, a backward kill prepends
    /// (`to_front`), so the recovered text reads in document order. Resets
    /// the yank-pop pointer to the newest entry.
    fn push_kill(&mut self, text: String, to_front: bool) {
        if self.last == LastCmd::Kill && !self.kill_ring.is_empty() {
            let cur = self.kill_ring.last_mut().expect("non-empty checked");
            if to_front {
                cur.insert_str(0, &text);
            } else {
                cur.push_str(&text);
            }
        } else {
            self.kill_ring.push(text);
            if self.kill_ring.len() > KILL_RING_MAX {
                self.kill_ring.remove(0);
            }
        }
        self.ring_pos = self.kill_ring.len() - 1;
    }

    /// Kills the document span `a..b`: records it on the ring and deletes it.
    /// A no-op (no snapshot, no ring entry) for an empty span. `to_front`
    /// selects append vs prepend coalescing — see [`push_kill`](Self::push_kill).
    fn kill_span(&mut self, ta: &mut TextArea, a: Pos, b: Pos, to_front: bool) -> bool {
        let text = ta.span_text(a, b);
        if text.is_empty() {
            return false;
        }
        self.snapshot(ta);
        self.push_kill(text, to_front);
        ta.delete_span(a, b);
        self.last = LastCmd::Kill;
        true
    }

    /// Kills from the cursor to the end of the line (`Ctrl+K`). At the end of
    /// a non-last line it kills the newline instead, joining the next line.
    pub fn kill_line(&mut self, ta: &mut TextArea) -> bool {
        let (r, c) = ta.cursor();
        let len = line_len(ta, r);
        if c < len {
            self.kill_span(ta, (r, c), (r, len), false)
        } else if r + 1 < ta.row_count() {
            self.snapshot(ta);
            self.push_kill("\n".to_owned(), false);
            ta.delete_forward();
            self.last = LastCmd::Kill;
            true
        } else {
            false
        }
    }

    /// Kills from the start of the line to the cursor (`Ctrl+U`,
    /// unix-line-discard). At column 0 of a non-first line it kills the
    /// preceding newline instead, joining onto the previous line.
    pub fn kill_line_backward(&mut self, ta: &mut TextArea) -> bool {
        let (r, c) = ta.cursor();
        if c > 0 {
            self.kill_span(ta, (r, 0), (r, c), true)
        } else if r > 0 {
            self.snapshot(ta);
            self.push_kill("\n".to_owned(), true);
            ta.delete_backward();
            self.last = LastCmd::Kill;
            true
        } else {
            false
        }
    }

    /// Kills the alphanumeric word before the cursor (`Alt+Backspace`,
    /// backward-kill-word — stops at punctuation).
    pub fn kill_word_backward(&mut self, ta: &mut TextArea) -> bool {
        let end = ta.cursor();
        let start = word_backward(ta, end, is_wordchar);
        self.kill_span(ta, start, end, true)
    }

    /// Kills the whitespace-delimited word before the cursor (`Ctrl+W`,
    /// unix-word-rubout — punctuation is part of the word).
    pub fn unix_word_rubout(&mut self, ta: &mut TextArea) -> bool {
        let end = ta.cursor();
        let start = word_backward(ta, end, is_nonspace);
        self.kill_span(ta, start, end, true)
    }

    /// Kills the alphanumeric word after the cursor (`Alt+D`, kill-word).
    pub fn kill_word_forward(&mut self, ta: &mut TextArea) -> bool {
        let start = ta.cursor();
        let end = word_forward(ta, start, is_wordchar);
        self.kill_span(ta, start, end, false)
    }

    /// Yanks (inserts) the newest kill-ring entry at the cursor (`Ctrl+Y`).
    /// A no-op when the ring is empty.
    pub fn yank(&mut self, ta: &mut TextArea) -> bool {
        let Some(text) = self.kill_ring.last().cloned() else {
            return false;
        };
        self.ring_pos = self.kill_ring.len() - 1;
        self.snapshot(ta);
        let before = ta.cursor();
        ta.insert_str(&text);
        self.yank_span = Some((before, ta.cursor()));
        self.last = LastCmd::Yank;
        true
    }

    /// Yank-pop (`Alt+Y`): replaces the text the previous yank inserted with
    /// the *next older* kill-ring entry, cycling the ring. Only valid
    /// immediately after a [`yank`](Self::yank) or another `yank_pop`;
    /// otherwise a no-op.
    pub fn yank_pop(&mut self, ta: &mut TextArea) -> bool {
        if self.last != LastCmd::Yank {
            return false;
        }
        let Some((before, after)) = self.yank_span else {
            return false;
        };
        if self.kill_ring.is_empty() {
            return false;
        }
        let len = self.kill_ring.len();
        self.ring_pos = (self.ring_pos + len - 1) % len;
        let text = self.kill_ring[self.ring_pos].clone();
        // The pre-yank snapshot still covers this — one undo removes the
        // whole yank/yank-pop sequence — so do not snapshot again here.
        ta.delete_span(before, after);
        ta.insert_str(&text);
        self.yank_span = Some((before, ta.cursor()));
        self.last = LastCmd::Yank;
        true
    }

    // -- transpose / case --------------------------------------------------

    /// Transposes the two characters around the cursor and steps past them
    /// (`Ctrl+T`). At the end of the line it transposes the last two
    /// characters in place — exactly readline's `transpose-chars`.
    pub fn transpose_chars(&mut self, ta: &mut TextArea) -> bool {
        let (r, c) = ta.cursor();
        let mut chars: Vec<char> = ta.line(r).unwrap_or("").chars().collect();
        let len = chars.len();
        let (i, j, new_col) = if len < 2 || c == 0 {
            return false;
        } else if c >= len {
            (len - 2, len - 1, len)
        } else {
            (c - 1, c, c + 1)
        };
        self.snapshot(ta);
        chars.swap(i, j);
        let rebuilt: String = chars.into_iter().collect();
        ta.replace_span((r, 0), (r, len), &rebuilt);
        ta.set_cursor(r, new_col);
        self.last = LastCmd::Other;
        true
    }

    /// Transposes the word around/before the cursor with the word after it,
    /// leaving the cursor past the second word (`Alt+T`, transpose-words).
    /// A no-op unless two whole words can be identified.
    pub fn transpose_words(&mut self, ta: &mut TextArea) -> bool {
        let pt = ta.cursor();
        let w2_end = word_forward(ta, pt, is_wordchar);
        let w2_start = word_backward(ta, w2_end, is_wordchar);
        let w1_start = word_backward(ta, w2_start, is_wordchar);
        let w1_end = word_forward(ta, w1_start, is_wordchar);
        // Need two distinct, non-empty words with the gap between them.
        if !(w1_start < w1_end && w1_end <= w2_start && w2_start < w2_end) {
            return false;
        }
        let word1 = ta.span_text(w1_start, w1_end);
        let gap = ta.span_text(w1_end, w2_start);
        let word2 = ta.span_text(w2_start, w2_end);
        self.snapshot(ta);
        ta.replace_span(w1_start, w2_end, &format!("{word2}{gap}{word1}"));
        self.last = LastCmd::Other;
        true
    }

    /// Upper-cases the word from the cursor to the end of the next word
    /// (`Alt+U`), leaving the cursor past it.
    pub fn upcase_word(&mut self, ta: &mut TextArea) -> bool {
        self.case_word(ta, Case::Upper)
    }

    /// Lower-cases the word from the cursor to the end of the next word
    /// (`Alt+L`), leaving the cursor past it.
    pub fn downcase_word(&mut self, ta: &mut TextArea) -> bool {
        self.case_word(ta, Case::Lower)
    }

    /// Capitalizes the word from the cursor to the end of the next word
    /// (`Alt+C`) — first letter upper, the rest lower — cursor past it.
    pub fn capitalize_word(&mut self, ta: &mut TextArea) -> bool {
        self.case_word(ta, Case::Capitalize)
    }

    /// Shared body of [`upcase_word`](Self::upcase_word) /
    /// [`downcase_word`](Self::downcase_word) /
    /// [`capitalize_word`](Self::capitalize_word).
    fn case_word(&mut self, ta: &mut TextArea, mode: Case) -> bool {
        let start = ta.cursor();
        let end = word_forward(ta, start, is_wordchar);
        if start == end {
            return false;
        }
        let text = ta.span_text(start, end);
        let recased = match mode {
            Case::Upper => text.to_uppercase(),
            Case::Lower => text.to_lowercase(),
            Case::Capitalize => capitalize(&text),
        };
        self.snapshot(ta);
        ta.replace_span(start, end, &recased);
        self.last = LastCmd::Other;
        true
    }

    /// Deletes every space and tab around the cursor on the current line
    /// (`Alt+\`, delete-horizontal-space).
    pub fn delete_horizontal_space(&mut self, ta: &mut TextArea) -> bool {
        let (r, c) = ta.cursor();
        let chars: Vec<char> = ta.line(r).unwrap_or("").chars().collect();
        let is_blank = |c: char| c == ' ' || c == '\t';
        let mut s = c;
        while s > 0 && chars.get(s - 1).copied().is_some_and(is_blank) {
            s -= 1;
        }
        let mut e = c;
        while chars.get(e).copied().is_some_and(is_blank) {
            e += 1;
        }
        if s == e {
            return false;
        }
        self.snapshot(ta);
        ta.delete_span((r, s), (r, e));
        self.last = LastCmd::Other;
        true
    }

    // -- undo --------------------------------------------------------------

    /// Reverts the composer to the state before the most recent edit group
    /// (`Ctrl+_`). Returns whether anything was undone.
    pub fn undo(&mut self, ta: &mut TextArea) -> bool {
        let Some((text, (r, c))) = self.undo.pop() else {
            return false;
        };
        ta.set_value(text);
        ta.set_cursor(r, c);
        self.last = LastCmd::Other;
        true
    }

    /// Reverts the composer to the state it had when the current line's
    /// editing began, discarding every undo step (`Alt+R`, revert-line).
    pub fn revert_line(&mut self, ta: &mut TextArea) -> bool {
        let Some((text, (r, c))) = self.undo.first().cloned() else {
            return false;
        };
        self.undo.clear();
        ta.set_value(text);
        ta.set_cursor(r, c);
        self.last = LastCmd::Other;
        true
    }
}

/// Which case [`ReadlineState::case_word`] applies.
#[derive(Debug, Clone, Copy)]
enum Case {
    Upper,
    Lower,
    Capitalize,
}

/// First alphanumeric character upper-cased, every other character
/// lower-cased — readline's `capitalize-word`.
fn capitalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut seen = false;
    for c in s.chars() {
        if !seen && c.is_alphanumeric() {
            seen = true;
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// A word character for `Alt+F`/`Alt+B`/`Alt+D` and friends — readline's
/// default: letters and digits, so punctuation breaks a word.
fn is_wordchar(c: char) -> bool {
    c.is_alphanumeric()
}

/// A "word" character for `Ctrl+W` (unix-word-rubout): anything that is not
/// whitespace, so a path or a flag is killed whole.
fn is_nonspace(c: char) -> bool {
    !c.is_whitespace()
}

/// The character count of row `row`, or 0 if `row` is out of range.
fn line_len(ta: &TextArea, row: usize) -> usize {
    ta.line(row).map_or(0, |l| l.chars().count())
}

/// The character starting at `pos`, the newline `'\n'` at a non-last line's
/// end, or `None` at the very end of the document. This is what makes word
/// motion treat the buffer as one continuous run of characters.
fn char_at(ta: &TextArea, (r, c): Pos) -> Option<char> {
    let line = ta.line(r)?;
    let len = line.chars().count();
    if c < len {
        line.chars().nth(c)
    } else if c == len && r + 1 < ta.row_count() {
        Some('\n')
    } else {
        None
    }
}

/// The position one character after `pos`, wrapping to the next row, or
/// `None` at the end of the document.
fn next_pos(ta: &TextArea, (r, c): Pos) -> Option<Pos> {
    if c < line_len(ta, r) {
        Some((r, c + 1))
    } else if r + 1 < ta.row_count() {
        Some((r + 1, 0))
    } else {
        None
    }
}

/// The position one character before `pos`, wrapping to the previous row, or
/// `None` at the start of the document.
fn prev_pos(ta: &TextArea, (r, c): Pos) -> Option<Pos> {
    if c > 0 {
        Some((r, c - 1))
    } else if r > 0 {
        Some((r - 1, line_len(ta, r - 1)))
    } else {
        None
    }
}

/// The position at the end of the next word at or after `pos`: skip leading
/// separators, then skip the word. Clamped to the document end.
fn word_forward(ta: &TextArea, mut pos: Pos, is_word: fn(char) -> bool) -> Pos {
    while matches!(char_at(ta, pos), Some(c) if !is_word(c)) {
        match next_pos(ta, pos) {
            Some(n) => pos = n,
            None => return pos,
        }
    }
    while matches!(char_at(ta, pos), Some(c) if is_word(c)) {
        match next_pos(ta, pos) {
            Some(n) => pos = n,
            None => return pos,
        }
    }
    pos
}

/// The position at the start of the word at or before `pos`: skip trailing
/// separators backward, then skip the word backward. Clamped to the document
/// start.
fn word_backward(ta: &TextArea, mut pos: Pos, is_word: fn(char) -> bool) -> Pos {
    loop {
        let Some(prev) = prev_pos(ta, pos) else {
            return pos;
        };
        if matches!(char_at(ta, prev), Some(c) if is_word(c)) {
            break;
        }
        pos = prev;
    }
    loop {
        let Some(prev) = prev_pos(ta, pos) else {
            return pos;
        };
        if matches!(char_at(ta, prev), Some(c) if is_word(c)) {
            pos = prev;
        } else {
            return pos;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A composer with `text` and the cursor placed at `(row, col)`.
    fn at(text: &str, row: usize, col: usize) -> TextArea {
        let mut ta = TextArea::from_value(text);
        ta.set_cursor(row, col);
        ta
    }

    #[test]
    fn word_motion_skips_punctuation_and_crosses_lines() {
        let ta = at("foo, bar\nbaz", 0, 0);
        // Forward: end of "foo", then end of "bar", then across the newline
        // to the end of "baz".
        let p = word_forward(&ta, (0, 0), is_wordchar);
        assert_eq!(p, (0, 3));
        let p = word_forward(&ta, p, is_wordchar);
        assert_eq!(p, (0, 8));
        let p = word_forward(&ta, p, is_wordchar);
        assert_eq!(p, (1, 3));
        // Backward from the end is the mirror image.
        let p = word_backward(&ta, (1, 3), is_wordchar);
        assert_eq!(p, (1, 0));
        let p = word_backward(&ta, p, is_wordchar);
        assert_eq!(p, (0, 5));
        let p = word_backward(&ta, p, is_wordchar);
        assert_eq!(p, (0, 0));
    }

    #[test]
    fn kill_word_backward_is_alphanumeric_delimited() {
        let mut rl = ReadlineState::new();
        let mut ta = at("hello world", 0, 11);
        assert!(rl.kill_word_backward(&mut ta));
        assert_eq!(ta.to_string(), "hello ");
        assert_eq!(ta.cursor(), (0, 6));
        assert_eq!(rl.kill_ring.last().map(String::as_str), Some("world"));
    }

    #[test]
    fn unix_word_rubout_is_whitespace_delimited() {
        let mut rl = ReadlineState::new();
        // C-w kills back to whitespace — punctuation stays in the word.
        let mut ta = at("run src/app.rs", 0, 14);
        assert!(rl.unix_word_rubout(&mut ta));
        assert_eq!(ta.to_string(), "run ");
        assert_eq!(rl.kill_ring.last().map(String::as_str), Some("src/app.rs"));
    }

    #[test]
    fn kill_word_forward_kills_the_next_word() {
        let mut rl = ReadlineState::new();
        let mut ta = at("hello world", 0, 0);
        assert!(rl.kill_word_forward(&mut ta));
        assert_eq!(ta.to_string(), " world");
        assert_eq!(ta.cursor(), (0, 0));
    }

    #[test]
    fn kill_line_forward_and_backward() {
        let mut rl = ReadlineState::new();
        let mut ta = at("hello world", 0, 5);
        assert!(rl.kill_line(&mut ta));
        assert_eq!(ta.to_string(), "hello");
        assert_eq!(rl.kill_ring.last().map(String::as_str), Some(" world"));

        let mut rl = ReadlineState::new();
        let mut ta = at("hello world", 0, 6);
        assert!(rl.kill_line_backward(&mut ta));
        assert_eq!(ta.to_string(), "world");
        assert_eq!(ta.cursor(), (0, 0));
    }

    #[test]
    fn kill_line_at_line_end_joins_the_next_line() {
        let mut rl = ReadlineState::new();
        let mut ta = at("one\ntwo", 0, 3);
        assert!(rl.kill_line(&mut ta));
        assert_eq!(ta.to_string(), "onetwo");
    }

    #[test]
    fn consecutive_kills_coalesce_into_one_ring_entry() {
        let mut rl = ReadlineState::new();
        let mut ta = at("alpha beta gamma", 0, 16);
        // Two backward kills prepend, so the recovered text reads in order.
        assert!(rl.kill_word_backward(&mut ta));
        assert!(rl.kill_word_backward(&mut ta));
        assert_eq!(rl.kill_ring.len(), 1);
        assert_eq!(rl.kill_ring.last().map(String::as_str), Some("beta gamma"));
        // And a yank pastes the whole coalesced run back.
        assert!(rl.yank(&mut ta));
        assert_eq!(ta.to_string(), "alpha beta gamma");
    }

    #[test]
    fn yank_inserts_the_newest_kill() {
        let mut rl = ReadlineState::new();
        let mut ta = at("delete me", 0, 9);
        rl.kill_word_backward(&mut ta);
        ta.move_home();
        rl.break_sequence();
        assert!(rl.yank(&mut ta));
        assert_eq!(ta.to_string(), "medelete ");
    }

    #[test]
    fn yank_pop_cycles_through_the_ring() {
        let mut rl = ReadlineState::new();
        let mut ta = at("", 0, 0);
        // Three separate kills (a break between each → three ring entries).
        for word in ["one", "two", "three"] {
            ta.set_value(word);
            ta.move_doc_end();
            rl.break_sequence();
            rl.unix_word_rubout(&mut ta);
        }
        ta.clear();
        rl.break_sequence();
        assert!(rl.yank(&mut ta)); // newest
        assert_eq!(ta.to_string(), "three");
        assert!(rl.yank_pop(&mut ta)); // → "two"
        assert_eq!(ta.to_string(), "two");
        assert!(rl.yank_pop(&mut ta)); // → "one"
        assert_eq!(ta.to_string(), "one");
        assert!(rl.yank_pop(&mut ta)); // wraps → "three"
        assert_eq!(ta.to_string(), "three");
        // Yank-pop only works straight after a yank.
        rl.break_sequence();
        assert!(!rl.yank_pop(&mut ta));
    }

    #[test]
    fn transpose_chars_mid_line_and_at_end() {
        let mut rl = ReadlineState::new();
        let mut ta = at("abcd", 0, 2);
        assert!(rl.transpose_chars(&mut ta)); // swap b<->c, step past
        assert_eq!(ta.to_string(), "acbd");
        assert_eq!(ta.cursor(), (0, 3));

        let mut ta = at("abcd", 0, 4); // at end → swap last two
        assert!(rl.transpose_chars(&mut ta));
        assert_eq!(ta.to_string(), "abdc");

        let mut ta = at("a", 0, 1); // too short → no-op
        assert!(!rl.transpose_chars(&mut ta));
    }

    #[test]
    fn transpose_words_swaps_around_the_cursor() {
        let mut rl = ReadlineState::new();
        let mut ta = at("foo bar", 0, 7);
        assert!(rl.transpose_words(&mut ta));
        assert_eq!(ta.to_string(), "bar foo");
    }

    #[test]
    fn case_operations_change_the_next_word() {
        let mut rl = ReadlineState::new();
        let mut ta = at("hello WORLD mixed", 0, 0);
        assert!(rl.upcase_word(&mut ta));
        assert_eq!(ta.to_string(), "HELLO WORLD mixed");
        assert_eq!(ta.cursor(), (0, 5));
        assert!(rl.downcase_word(&mut ta));
        assert_eq!(ta.to_string(), "HELLO world mixed");
        assert!(rl.capitalize_word(&mut ta));
        assert_eq!(ta.to_string(), "HELLO world Mixed");
    }

    #[test]
    fn delete_horizontal_space_removes_blanks_around_the_cursor() {
        let mut rl = ReadlineState::new();
        let mut ta = at("foo    bar", 0, 5);
        assert!(rl.delete_horizontal_space(&mut ta));
        assert_eq!(ta.to_string(), "foobar");
        // Nothing to delete → no-op.
        assert!(!rl.delete_horizontal_space(&mut ta));
    }

    #[test]
    fn undo_coalesces_a_typing_run_then_steps_back_per_edit() {
        let mut rl = ReadlineState::new();
        let mut ta = TextArea::new();
        for c in "hello".chars() {
            rl.insert_char(&mut ta, c);
        }
        // A whole typing run is one undo group.
        assert!(rl.undo(&mut ta));
        assert_eq!(ta.to_string(), "");
        assert!(!rl.undo(&mut ta));
    }

    #[test]
    fn undo_restores_text_removed_by_a_kill() {
        let mut rl = ReadlineState::new();
        let mut ta = at("keep this", 0, 9);
        rl.kill_word_backward(&mut ta);
        assert_eq!(ta.to_string(), "keep ");
        assert!(rl.undo(&mut ta));
        assert_eq!(ta.to_string(), "keep this");
        assert_eq!(ta.cursor(), (0, 9));
    }

    #[test]
    fn revert_line_drops_every_edit_at_once() {
        let mut rl = ReadlineState::new();
        let mut ta = at("original", 0, 8);
        rl.kill_word_backward(&mut ta);
        for c in "replacement".chars() {
            rl.insert_char(&mut ta, c);
        }
        assert!(rl.revert_line(&mut ta));
        assert_eq!(ta.to_string(), "original");
    }

    #[test]
    fn reset_line_clears_undo_but_keeps_the_kill_ring() {
        let mut rl = ReadlineState::new();
        let mut ta = at("word", 0, 4);
        rl.kill_word_backward(&mut ta);
        rl.reset_line();
        // Undo history is gone with the old line…
        assert!(!rl.undo(&mut ta));
        // …but the kill ring survives across lines.
        ta.clear();
        assert!(rl.yank(&mut ta));
        assert_eq!(ta.to_string(), "word");
    }

    #[test]
    fn operations_are_total_over_a_random_sequence() {
        // A fixed-seed LCG (no rand dep) — the technique text_area.rs uses.
        let mut state: u64 = 0x5eed_1234_5678_9abc;
        let mut rng = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };
        for seed in ["", "a b c", "líne\nдва\n日本 x", "  \t  ", "one-two_three"] {
            let mut rl = ReadlineState::new();
            let mut ta = TextArea::from_value(seed);
            for _ in 0..2_000 {
                let (r, c) = ((rng() % 4) as usize, (rng() % 8) as usize);
                ta.set_cursor(r, c);
                match rng() % 22 {
                    0 => rl.insert_char(&mut ta, '日'),
                    1 => rl.insert_str(&mut ta, "x\ny"),
                    2 => rl.delete_backward(&mut ta),
                    3 => rl.delete_forward(&mut ta),
                    4 => rl.word_left(&mut ta),
                    5 => rl.word_right(&mut ta),
                    6 => {
                        rl.kill_line(&mut ta);
                    }
                    7 => {
                        rl.kill_line_backward(&mut ta);
                    }
                    8 => {
                        rl.kill_word_backward(&mut ta);
                    }
                    9 => {
                        rl.unix_word_rubout(&mut ta);
                    }
                    10 => {
                        rl.kill_word_forward(&mut ta);
                    }
                    11 => {
                        rl.yank(&mut ta);
                    }
                    12 => {
                        rl.yank_pop(&mut ta);
                    }
                    13 => {
                        rl.transpose_chars(&mut ta);
                    }
                    14 => {
                        rl.transpose_words(&mut ta);
                    }
                    15 => {
                        rl.upcase_word(&mut ta);
                    }
                    16 => {
                        rl.downcase_word(&mut ta);
                    }
                    17 => {
                        rl.capitalize_word(&mut ta);
                    }
                    18 => {
                        rl.delete_horizontal_space(&mut ta);
                    }
                    19 => {
                        rl.undo(&mut ta);
                    }
                    20 => {
                        rl.revert_line(&mut ta);
                    }
                    _ => rl.reset_line(),
                }
                // The cursor invariant TextArea guarantees must always hold.
                let (cr, cc) = ta.cursor();
                assert!(cr < ta.row_count());
                assert!(cc <= ta.line(cr).map_or(0, |l| l.chars().count()));
            }
        }
    }
}
