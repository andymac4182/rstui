//! Caller-owned find-in-document query over logical lines.
//!
//! [`Query`] is the search-side sibling of
//! [`Selection`](crate::selection::Selection) /
//! [`TextEdit`](crate::text_edit::TextEdit) /
//! [`FocusRing`](crate::focus::FocusRing): a pure value type that lives as a
//! *field in the application's model*, mutated only by `update` (the user
//! types into the `/` prompt, picks a case mode) and read by the pure `view`
//! (which highlights [`find_all`](Query::find_all)'s spans and scrolls the
//! cursor to [`next_from`](Query::next_from)). It owns no buffer of its own —
//! it is handed a borrowed `&[String]` (exactly
//! [`TextArea::lines`](crate::text_area::TextArea::lines)) at call time and
//! returns **character-indexed** [`Match`]es. Per
//! [ADR 0012](https://github.com/andymac4182/rstui/blob/main/docs/adr/0012-widget-composition-and-layout-model.md)
//! §P1 this is *forced* by rstui's pure-`view` / immediate-mode design: a
//! widget is handed only a [`Buffer`](crate::buffer::Buffer) at render time
//! with no retained tree, so it can neither own the pattern the user is typing
//! nor re-run the search on a keystroke. The reducer owns the query, exactly
//! as it owns the selection, the focus, and the edited text — it drives the
//! editor's `/ n N` and the diff viewer's find-in-hunks.
//!
//! Like [`focus`](crate::focus) and [`selection`](crate::selection), this
//! module is **optional**: an app may keep a bare pattern `String` of its own
//! and `str::find` by hand. `Query` exists only to turn the search bookkeeping
//! every such app re-derives — and routinely gets wrong at the edges (a
//! byte-offset leaking into a `(row, col)` cursor that counts *characters*,
//! an off-by-one in non-overlapping advance, `n` not wrapping at the last
//! match, smart-case mis-deciding on a multi-byte uppercase) — into one
//! reusable, panic-free primitive:
//!
//! - A match's `start`/`end` are **character indices** into the matched row
//!   (`end` exclusive), never byte offsets, matching rstui's single-`char`
//!   cell model and the `(row, col)` cursor a
//!   [`TextArea`](crate::text_area::TextArea) uses everywhere. The byte math
//!   for multi-byte UTF-8 (`é`, `日`, `😀`) stays internal; a caller can feed
//!   a returned `start`/`end` straight back as a column.
//! - Matching is **literal substring** only — the dependency-free floor.
//!   `rstui-core` carries
//!   [zero third-party dependencies](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md),
//!   so there is no `regex` crate here; a real regular-expression engine is an
//!   explicitly **deferred, future feature-gated** capability under that ADR,
//!   not something this module approximates. Case folding for
//!   [`Case::Insensitive`] / [`Case::Smart`] uses [`char::to_lowercase`] (full
//!   Unicode, *not* an ASCII shortcut).
//! - Every method is **total** — an empty pattern, a pattern longer than any
//!   line, multi-byte content, an out-of-range or reversed `from`,
//!   `usize::MAX` coordinates: all well-defined, never a panic and never an
//!   index that splits a codepoint (the iter-25 "a pure projection must be
//!   total" rule, the same guarantee
//!   [`Selection`](crate::selection::Selection) and
//!   [`TextEdit`](crate::text_edit::TextEdit) give). An empty pattern matches
//!   *nothing* (so a freshly-cleared `/` prompt highlights nothing rather than
//!   every gap).
//!
//! This is **app/widget** search state (what the user typed into `/`) and is
//! unrelated to a [`TextEdit`](crate::text_edit::TextEdit) cursor, a
//! [`Selection`](crate::selection::Selection), or terminal scrollback.
//!
//! # Example
//!
//! ```
//! use rstui_core::search::{Case, Match, Query};
//!
//! // The document as logical lines — exactly `TextArea::lines()`.
//! let lines: Vec<String> = ["foo bar foo", "Foo é foo", "nothing"]
//!     .iter()
//!     .map(|s| s.to_string())
//!     .collect();
//!
//! // The app stores one `Query` in its model; `update` rebuilds it as the
//! // user types into the `/` prompt. `new` defaults to smart case.
//! let q = Query::new("foo");
//! assert_eq!(q.pattern(), "foo");
//! assert!(!q.is_empty());
//!
//! // Smart case: an all-lowercase pattern is case-*insensitive*, so the
//! // capitalised "Foo" on row 1 matches too. Matches are non-overlapping and
//! // in document order; columns are CHARACTER indices, `end` exclusive.
//! let all = q.find_all(&lines);
//! assert_eq!(all[0], Match { row: 0, start: 0, end: 3 });
//! assert_eq!(all[1], Match { row: 0, start: 8, end: 11 });
//! assert_eq!(all[2], Match { row: 1, start: 0, end: 3 }); // "Foo"
//! // Row 1 is "Foo é foo": "é" is one char, so the trailing "foo" is at
//! // char index 6..9 even though it is further along in *bytes*.
//! assert_eq!(all[3], Match { row: 1, start: 6, end: 9 });
//!
//! // `next_from` / `prev_from` are the `n` / `N` primitives. From just after
//! // the first hit, the next match is the second one on row 0.
//! assert_eq!(
//!     q.next_from(&lines, (0, 1), false),
//!     Some(Match { row: 0, start: 8, end: 11 })
//! );
//! // Past the last match with `wrap = true`, `n` returns to the top.
//! assert_eq!(
//!     q.next_from(&lines, (2, 0), true),
//!     Some(Match { row: 0, start: 0, end: 3 })
//! );
//! // `N` from the document start with no earlier match wraps to the last.
//! assert_eq!(
//!     q.prev_from(&lines, (0, 0), true),
//!     Some(Match { row: 1, start: 6, end: 9 })
//! );
//!
//! // A mixed-case pattern flips smart case to *sensitive*: now only the
//! // capitalised "Foo" on row 1 matches.
//! let q = Query::new("Foo");
//! assert_eq!(q.find_all(&lines), vec![Match { row: 1, start: 0, end: 3 }]);
//!
//! // Every input is total: an empty pattern matches nothing, never a panic.
//! let q = Query::new("");
//! assert!(q.is_empty());
//! assert!(q.find_all(&lines).is_empty());
//! assert_eq!(q.next_from(&lines, (0, 0), true), None);
//! ```

/// How a [`Query`] folds case when comparing its pattern to the document.
///
/// [`Smart`](Case::Smart) is the editor default (what [`Query::new`]
/// selects): it behaves as [`Insensitive`](Case::Insensitive) *unless* the
/// pattern contains an uppercase character, in which case it becomes
/// [`Sensitive`](Case::Sensitive) — the familiar Vim `smartcase` rule, so a
/// quick lowercase search is forgiving while deliberately typing a capital
/// narrows it. "Uppercase" is decided with [`char::is_uppercase`] over the
/// whole pattern (full Unicode, not just ASCII `A..Z`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// Exact match: bytes must agree after no folding at all.
    Sensitive,
    /// Fold both sides with [`char::to_lowercase`] before comparing (full
    /// Unicode case folding, not an ASCII shortcut).
    Insensitive,
    /// [`Insensitive`](Case::Insensitive) unless the pattern contains an
    /// uppercase character, then [`Sensitive`](Case::Sensitive) (Vim
    /// `smartcase`).
    Smart,
}

/// One non-overlapping occurrence of the pattern, located by **character**
/// index.
///
/// `row` is the index into the `&[String]` that was searched. `start` and
/// `end` are character indices into `lines[row]` with `end` **exclusive**, so
/// the matched text is the `start..end` character slice and the invariant
/// `start < end <= lines[row].chars().count()` always holds (a non-empty
/// pattern can never produce an empty or out-of-range span). These are the
/// same units a [`TextArea`](crate::text_area::TextArea) `(row, col)` cursor
/// uses, so a caller can move the cursor to `(m.row, m.start)` or select
/// `m.start..m.end` directly — no byte conversion, even across multi-byte
/// UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    /// Index of the matched line in the searched slice.
    pub row: usize,
    /// First character index of the match (inclusive).
    pub start: usize,
    /// One past the last character index of the match (exclusive).
    pub end: usize,
}

/// A caller-owned find-in-document query: a literal pattern plus a [`Case`]
/// mode.
///
/// `Query` is a **pure value type** designed to live as a field in the
/// application's model (it derives [`Default`] so it drops into a
/// `#[derive(Default)]` model as an empty, matches-nothing query —
/// `Query::default()` has an empty pattern and [`Case::Smart`], the only
/// sensible inert state, the [`Selection`](crate::selection::Selection)
/// convention). It owns *no* document, terminal, or widget state: `update`
/// rebuilds it from the `/` prompt's text, and the pure `view` only calls
/// [`find_all`](Self::find_all) / [`next_from`](Self::next_from) /
/// [`prev_from`](Self::prev_from) against the borrowed lines it is rendering.
/// The framework never touches it.
///
/// Matching is **literal substring** only — the dependency-free floor
/// (`rstui-core` has no `regex` crate; a real engine is a deferred
/// feature-gated future per
/// [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)).
/// [`find_all`](Self::find_all) returns **non-overlapping** matches in
/// document order; [`next_from`](Self::next_from) /
/// [`prev_from`](Self::prev_from) are the `n` / `N` primitives over that same
/// stream.
///
/// Every method is **total**: arbitrary input — an empty pattern (matches
/// nothing), a pattern longer than every line, multi-byte UTF-8 content, a
/// reversed or far-out-of-range `from`, `usize::MAX` coordinates — is
/// well-defined and never panics, and every returned [`Match`] obeys
/// `start < end <= lines[row].chars().count()` with the matched character
/// slice equal to the pattern under the active folding.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// The literal pattern the user typed. Empty ⇒ matches nothing (so a
    /// cleared `/` prompt highlights nothing, not every inter-character gap).
    pattern: String,
    /// How [`Case`] folding is applied; defaults (via `Default`) to
    /// [`Case::Smart`] because `Case`'s own derived `Default` would be its
    /// first variant — this field is set explicitly in every constructor so
    /// the derived `Default` for `Query` is still the smart-case empty query.
    case: Case,
}

impl Default for Case {
    /// [`Case::Smart`] — the editor default, so a `#[derive(Default)]`
    /// [`Query`] is the smart-case empty query (not exact-match).
    fn default() -> Self {
        Self::Smart
    }
}

impl Query {
    /// A query for the literal `pattern`, using [`Case::Smart`].
    ///
    /// Smart case is the editor default (a lowercase pattern is forgiving, a
    /// pattern with a capital is precise); call [`case`](Self::case) to pin a
    /// fixed mode. An empty `pattern` is allowed and yields a query that
    /// [`is_empty`](Self::is_empty) and matches nothing.
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            case: Case::Smart,
        }
    }

    /// Returns this query with its [`Case`] mode set to `case` (builder).
    ///
    /// Consumes and returns `self` so it chains off [`new`](Self::new):
    /// `Query::new("foo").case(Case::Sensitive)`.
    #[must_use]
    pub fn case(mut self, case: Case) -> Self {
        self.case = case;
        self
    }

    /// The literal pattern being searched for.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Whether this query matches nothing — i.e. the pattern is empty.
    ///
    /// An empty pattern is deliberately treated as "no query" rather than
    /// "matches every position", so a freshly-cleared `/` prompt highlights
    /// nothing. Every search method short-circuits to no result when this is
    /// `true`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pattern.is_empty()
    }

    /// All **non-overlapping** matches across `lines`, in document order.
    ///
    /// Rows are scanned top to bottom; within a row the search runs
    /// left-to-right and, after each match, resumes from the character
    /// immediately **after** that match (so `"aa"` over `"aaaa"` yields two
    /// matches at char `0..2` and `2..4`, never an overlapping one at `1..3`).
    /// Columns in every [`Match`] are character indices (`end` exclusive),
    /// correct across multi-byte UTF-8.
    ///
    /// Total: an empty pattern returns `Vec::new()`, a pattern longer than a
    /// line simply finds nothing on it, and no input panics.
    #[must_use]
    pub fn find_all(&self, lines: &[String]) -> Vec<Match> {
        let mut out = Vec::new();
        if self.is_empty() {
            return out;
        }
        for (row, line) in lines.iter().enumerate() {
            self.matches_in_row(row, line, &mut out);
        }
        out
    }

    /// The first match at or after `from` (a `(row, col)` character position),
    /// in document order — the `n` primitive.
    ///
    /// A match on row `from.0` counts only if its `start` is `>= from.1`
    /// (character index); rows after `from.0` are searched in full. If nothing
    /// is found at or after `from` and `wrap` is `true`, the search continues
    /// from the document start and returns the first match overall (so `n`
    /// past the last hit cycles back to the first); with `wrap = false` it
    /// returns `None`.
    ///
    /// Total: an empty pattern, an out-of-range `from` (a `row` past the end,
    /// a `col` past the line, `usize::MAX` either component), or no match at
    /// all all yield `None` (or the wrapped first match) without panicking.
    #[must_use]
    pub fn next_from(&self, lines: &[String], from: (usize, usize), wrap: bool) -> Option<Match> {
        if self.is_empty() {
            return None;
        }
        let (from_row, from_col) = from;
        let mut row_buf = Vec::new();
        for (row, line) in lines.iter().enumerate().skip(from_row) {
            row_buf.clear();
            self.matches_in_row(row, line, &mut row_buf);
            // On the start row only matches at or after the cursor column
            // count; later rows are taken whole.
            let first = row_buf
                .iter()
                .find(|m| row > from_row || m.start >= from_col);
            if let Some(&m) = first {
                return Some(m);
            }
        }
        if wrap {
            // `n` past the end cycles to the very first match in the document.
            self.find_all(lines).into_iter().next()
        } else {
            None
        }
    }

    /// The last match **strictly before** `from` (a `(row, col)` character
    /// position), in document order — the `N` primitive.
    ///
    /// A match on row `from.0` counts only if its `start` is `< from.1`
    /// (character index, strict — a match exactly at the cursor is *not*
    /// "before" it); rows before `from.0` are searched in full and the
    /// last match overall before `from` is returned. If nothing is found
    /// before `from` and `wrap` is `true`, the search wraps and returns the
    /// **last** match in the whole document (so `N` at the top cycles to the
    /// end); with `wrap = false` it returns `None`.
    ///
    /// Total: an empty pattern, an out-of-range `from`, `usize::MAX`
    /// components, or no match at all all yield `None` (or the wrapped last
    /// match) without panicking.
    #[must_use]
    pub fn prev_from(&self, lines: &[String], from: (usize, usize), wrap: bool) -> Option<Match> {
        if self.is_empty() {
            return None;
        }
        let (from_row, from_col) = from;
        let mut row_buf = Vec::new();
        // Walk rows from `from_row` downward. Clamp the start row to the last
        // real index so a far-out-of-range `from_row` still considers every
        // row (it is then entirely "before" the cursor).
        let start_row = from_row.min(lines.len().saturating_sub(1));
        for row in (0..=start_row).rev() {
            let Some(line) = lines.get(row) else {
                continue;
            };
            row_buf.clear();
            self.matches_in_row(row, line, &mut row_buf);
            // Earlier rows contribute their last match; the cursor's own row
            // contributes its last match strictly left of the cursor column.
            let last = row_buf
                .iter()
                .rev()
                .find(|m| row < from_row || m.start < from_col);
            if let Some(&m) = last {
                return Some(m);
            }
        }
        if wrap {
            // `N` before the first match cycles to the last in the document.
            self.find_all(lines).into_iter().next_back()
        } else {
            None
        }
    }

    /// Appends every non-overlapping match of the pattern within a single
    /// `line` (numbered `row`) to `out`, in left-to-right order.
    ///
    /// This is the one place the literal-substring search and the
    /// byte→character index conversion live, so [`find_all`](Self::find_all),
    /// [`next_from`](Self::next_from) and [`prev_from`](Self::prev_from) share
    /// exactly the same notion of "a match" (the shared-definition discipline
    /// [`Selection`](crate::selection::Selection) uses for its span). The
    /// caller has already short-circuited the empty-pattern case.
    fn matches_in_row(&self, row: usize, line: &str, out: &mut Vec<Match>) {
        debug_assert!(!self.pattern.is_empty());

        // Decide the effective folding once per line, then compare per
        // character so multi-byte UTF-8 never desynchronises a byte cursor
        // from a char cursor. `char_count` is the pattern's *character*
        // length, which is also the match width (literal match ⇒ the matched
        // slice has exactly the pattern's characters under any folding).
        let insensitive = match self.case {
            Case::Sensitive => false,
            Case::Insensitive => true,
            // Smart: insensitive unless the pattern has an uppercase char
            // (full Unicode, not just ASCII A..Z).
            Case::Smart => !self.pattern.chars().any(char::is_uppercase),
        };

        let pat: Vec<char> = if insensitive {
            fold(&self.pattern)
        } else {
            self.pattern.chars().collect()
        };
        let plen = pat.len();
        debug_assert!(plen > 0);

        let hay: Vec<char> = if insensitive {
            fold(line)
        } else {
            line.chars().collect()
        };
        if hay.len() < plen {
            return; // pattern longer than the line ⇒ no match (total)
        }

        // Naive left-to-right scan; on a hit, jump past the whole match so
        // results are non-overlapping (the `"aa"` over `"aaaa"` rule).
        let last_start = hay.len() - plen;
        let mut i = 0;
        while i <= last_start {
            if hay[i..i + plen] == pat[..] {
                out.push(Match {
                    row,
                    start: i,
                    end: i + plen,
                });
                i += plen;
            } else {
                i += 1;
            }
        }
    }
}

/// Lowercase-folds `s` to a `Vec<char>` using full-Unicode
/// [`char::to_lowercase`] (one source char may fold to several), so
/// [`Case::Insensitive`] / [`Case::Smart`] comparison is correct for
/// non-ASCII text (`É` → `é`, `İ` → `i̇`) rather than the common ASCII-only
/// shortcut. Folding both pattern and haystack the same way and comparing the
/// resulting `char` slices keeps the match width well-defined.
fn fold(s: &str) -> Vec<char> {
    s.chars().flat_map(char::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the `&[String]` document the API takes (exactly the shape
    /// `TextArea::lines()` hands back).
    fn doc(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn new_and_default_are_the_empty_smart_query() {
        assert_eq!(Query::new(""), Query::default());
        let q = Query::new("");
        assert_eq!(q.pattern(), "");
        assert!(q.is_empty());
        assert_eq!(q.case, Case::Smart);
        // An empty pattern matches nothing (not every gap).
        let lines = doc(&["anything at all", ""]);
        assert!(q.find_all(&lines).is_empty());
        assert_eq!(q.next_from(&lines, (0, 0), true), None);
        assert_eq!(q.prev_from(&lines, (0, 0), true), None);
    }

    #[test]
    fn find_all_returns_multiple_non_overlapping_matches_per_line() {
        let lines = doc(&["ab ab xx ab", "no hits here", "abab"]);
        let q = Query::new("ab").case(Case::Sensitive);
        assert_eq!(
            q.find_all(&lines),
            vec![
                Match {
                    row: 0,
                    start: 0,
                    end: 2
                },
                Match {
                    row: 0,
                    start: 3,
                    end: 5
                },
                Match {
                    row: 0,
                    start: 9,
                    end: 11
                },
                // Adjacent, non-overlapping: 0..2 then 2..4, never 1..3.
                Match {
                    row: 2,
                    start: 0,
                    end: 2
                },
                Match {
                    row: 2,
                    start: 2,
                    end: 4
                },
            ]
        );
    }

    #[test]
    fn find_all_columns_are_char_indices_across_multibyte_utf8() {
        // "é" is 2 bytes, "日" 3, "😀" 4 — columns must stay in *characters*.
        let lines = doc(&["é foo 日 foo", "😀😀foo"]);
        let q = Query::new("foo").case(Case::Sensitive);
        assert_eq!(
            q.find_all(&lines),
            vec![
                // "é foo ..." — 'é'(0) ' '(1) 'f'(2) ⇒ 2..5
                Match {
                    row: 0,
                    start: 2,
                    end: 5
                },
                // "... 日 foo" — through "é foo 日 " is 8 chars ⇒ 8..11
                Match {
                    row: 0,
                    start: 8,
                    end: 11
                },
                // "😀😀foo" — two emoji then "foo" ⇒ 2..5
                Match {
                    row: 1,
                    start: 2,
                    end: 5
                },
            ]
        );
        // The reported char slice really is the pattern.
        for m in q.find_all(&lines) {
            let got: String = lines[m.row]
                .chars()
                .skip(m.start)
                .take(m.end - m.start)
                .collect();
            assert_eq!(got, "foo");
        }
    }

    #[test]
    fn smart_case_is_insensitive_for_a_lowercase_pattern() {
        let lines = doc(&["Foo foo FOO fOo"]);
        // All-lowercase pattern under Smart ⇒ insensitive: every casing hits.
        let q = Query::new("foo"); // default Case::Smart
        assert_eq!(
            q.find_all(&lines),
            vec![
                Match {
                    row: 0,
                    start: 0,
                    end: 3
                },
                Match {
                    row: 0,
                    start: 4,
                    end: 7
                },
                Match {
                    row: 0,
                    start: 8,
                    end: 11
                },
                Match {
                    row: 0,
                    start: 12,
                    end: 15
                },
            ]
        );
    }

    #[test]
    fn smart_case_is_sensitive_when_the_pattern_has_an_uppercase_char() {
        let lines = doc(&["Foo foo FOO fOo"]);
        // Mixed-case pattern under Smart ⇒ sensitive: only exact "Foo".
        let q = Query::new("Foo");
        assert_eq!(
            q.find_all(&lines),
            vec![Match {
                row: 0,
                start: 0,
                end: 3
            }]
        );
        // Non-ASCII uppercase also flips smart case to sensitive.
        let lines = doc(&["éÉéÉ"]);
        let q = Query::new("É");
        assert_eq!(
            q.find_all(&lines),
            vec![
                Match {
                    row: 0,
                    start: 1,
                    end: 2
                },
                Match {
                    row: 0,
                    start: 3,
                    end: 4
                },
            ]
        );
    }

    #[test]
    fn explicit_case_modes_override_smart_heuristic() {
        let lines = doc(&["aAaA"]);
        // Forced sensitive: lowercase pattern no longer matches 'A'.
        let q = Query::new("a").case(Case::Sensitive);
        assert_eq!(
            q.find_all(&lines),
            vec![
                Match {
                    row: 0,
                    start: 0,
                    end: 1
                },
                Match {
                    row: 0,
                    start: 2,
                    end: 3
                },
            ]
        );
        // Forced insensitive: a mixed-case pattern still folds.
        let q = Query::new("A").case(Case::Insensitive);
        assert_eq!(q.find_all(&lines).len(), 4);
    }

    #[test]
    fn insensitive_folding_is_full_unicode_not_ascii_only() {
        // "İ" (U+0130) lowercases to "i̇" (i + combining dot, 2 chars). The
        // common ASCII-only shortcut would mishandle this; full folding makes
        // pattern and haystack agree.
        let lines = doc(&["xÉy"]);
        let q = Query::new("é").case(Case::Insensitive);
        assert_eq!(
            q.find_all(&lines),
            vec![Match {
                row: 0,
                start: 1,
                end: 2
            }]
        );
    }

    #[test]
    fn pattern_longer_than_any_line_finds_nothing() {
        let lines = doc(&["hi", "", "ab"]);
        let q = Query::new("longer than content").case(Case::Sensitive);
        assert!(q.find_all(&lines).is_empty());
        assert_eq!(q.next_from(&lines, (0, 0), true), None);
        assert_eq!(q.prev_from(&lines, (2, 9), true), None);
    }

    #[test]
    fn next_from_with_and_without_wrap() {
        let lines = doc(&["foo a foo", "b foo", "no"]);
        let q = Query::new("foo").case(Case::Sensitive);

        // At/after on the start row is decided by char column.
        assert_eq!(
            q.next_from(&lines, (0, 0), false),
            Some(Match {
                row: 0,
                start: 0,
                end: 3
            })
        );
        assert_eq!(
            q.next_from(&lines, (0, 1), false),
            Some(Match {
                row: 0,
                start: 6,
                end: 9
            })
        );
        // Falls through to a later row.
        assert_eq!(
            q.next_from(&lines, (0, 7), false),
            Some(Match {
                row: 1,
                start: 2,
                end: 5
            })
        );
        // Past the last match: no wrap ⇒ None, wrap ⇒ first match overall.
        assert_eq!(q.next_from(&lines, (1, 3), false), None);
        assert_eq!(
            q.next_from(&lines, (1, 3), true),
            Some(Match {
                row: 0,
                start: 0,
                end: 3
            })
        );
        // Out-of-range `from` is total.
        assert_eq!(q.next_from(&lines, (999, 999), false), None);
        assert_eq!(
            q.next_from(&lines, (usize::MAX, usize::MAX), true),
            Some(Match {
                row: 0,
                start: 0,
                end: 3
            })
        );
    }

    #[test]
    fn prev_from_with_and_without_wrap() {
        let lines = doc(&["foo a foo", "b foo", "no"]);
        let q = Query::new("foo").case(Case::Sensitive);

        // Strictly before: a match exactly at the cursor does not count.
        assert_eq!(q.prev_from(&lines, (0, 0), false), None);
        assert_eq!(
            q.prev_from(&lines, (0, 1), false),
            Some(Match {
                row: 0,
                start: 0,
                end: 3
            })
        );
        assert_eq!(
            q.prev_from(&lines, (0, 6), false),
            Some(Match {
                row: 0,
                start: 0,
                end: 3
            })
        );
        // Looks back into earlier rows; takes the last match before `from`.
        assert_eq!(
            q.prev_from(&lines, (2, 0), false),
            Some(Match {
                row: 1,
                start: 2,
                end: 5
            })
        );
        // Nothing before the first match: no wrap ⇒ None, wrap ⇒ last overall.
        assert_eq!(q.prev_from(&lines, (0, 0), false), None);
        assert_eq!(
            q.prev_from(&lines, (0, 0), true),
            Some(Match {
                row: 1,
                start: 2,
                end: 5
            })
        );
        // Out-of-range `from` is total (clamped to the last real row).
        assert_eq!(
            q.prev_from(&lines, (999, 999), false),
            Some(Match {
                row: 1,
                start: 2,
                end: 5
            })
        );
    }

    #[test]
    fn next_and_prev_round_trip_through_all_matches() {
        let lines = doc(&["a x a", "y a", "a a"]);
        let q = Query::new("a").case(Case::Sensitive);
        let all = q.find_all(&lines);
        assert_eq!(all.len(), 5);

        // `n` from the very start, repeatedly, visits every match in order
        // then wraps to the first.
        let mut cur = (0, 0);
        let mut visited = Vec::new();
        for _ in 0..all.len() {
            let m = q.next_from(&lines, cur, true).unwrap();
            visited.push(m);
            cur = (m.row, m.start + 1);
        }
        assert_eq!(visited, all);
        // One more `n` wraps back to the first.
        assert_eq!(q.next_from(&lines, cur, true), Some(all[0]));

        // `N` from the end walks the matches in reverse.
        let mut cur = (lines.len(), 0);
        let mut back = Vec::new();
        for _ in 0..all.len() {
            let m = q.prev_from(&lines, cur, true).unwrap();
            back.push(m);
            cur = (m.row, m.start);
        }
        back.reverse();
        assert_eq!(back, all);
    }

    #[test]
    fn empty_document_and_empty_lines_are_total() {
        let q = Query::new("x").case(Case::Sensitive);
        let empty: Vec<String> = Vec::new();
        assert!(q.find_all(&empty).is_empty());
        assert_eq!(q.next_from(&empty, (0, 0), true), None);
        assert_eq!(q.prev_from(&empty, (0, 0), true), None);

        let blanks = doc(&["", "", ""]);
        assert!(q.find_all(&blanks).is_empty());
        assert_eq!(q.next_from(&blanks, (5, 5), true), None);
    }

    /// The totality property (the iter-25 rule, mirroring
    /// [`Selection`](crate::selection::Selection)'s and
    /// [`TextEdit`](crate::text_edit::TextEdit)'s): over randomly-sized line
    /// sets with multi-byte content and random patterns / cases /
    /// `from`-positions (including `usize::MAX`), no method panics; every
    /// returned [`Match`] satisfies `start < end <= lines[row].chars().count()`
    /// and its character slice equals the pattern under the active folding;
    /// and `find_all`'s matches are ordered and non-overlapping.
    #[test]
    fn any_query_over_any_document_is_total_and_well_formed() {
        // Fixed-seed LCG keeps the run deterministic with no rand dep
        // (rstui-core is dependency-free) — the same technique text_area.rs,
        // text_edit.rs and focus.rs use.
        let mut state: u64 = 0x5ea2_c4ed_5eed_1234;
        let mut rng = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };

        // A small alphabet that mixes case and multi-byte codepoints so
        // folding, char-vs-byte indexing and smart case all get exercised.
        let alphabet = ['a', 'A', 'b', 'é', 'É', '日', '😀', ' '];
        let cases = [Case::Sensitive, Case::Insensitive, Case::Smart];

        for _ in 0..2_000 {
            // Random document: up to 6 lines, each up to 10 chars.
            let nlines = (rng() % 7) as usize;
            let mut lines: Vec<String> = Vec::with_capacity(nlines);
            for _ in 0..nlines {
                let len = (rng() % 11) as usize;
                let mut s = String::new();
                for _ in 0..len {
                    s.push(alphabet[(rng() % alphabet.len() as u64) as usize]);
                }
                lines.push(s);
            }

            // Random pattern: sometimes empty, else up to 4 chars.
            let plen = (rng() % 5) as usize;
            let mut pat = String::new();
            for _ in 0..plen {
                pat.push(alphabet[(rng() % alphabet.len() as u64) as usize]);
            }
            let case = cases[(rng() % 3) as usize];
            let q = Query::new(pat.clone()).case(case);

            // Whether folding is in effect for this query (mirrors the
            // internal decision) so we can check the matched slice.
            let insensitive = match case {
                Case::Sensitive => false,
                Case::Insensitive => true,
                Case::Smart => !pat.chars().any(char::is_uppercase),
            };
            let folded_pat = if insensitive {
                fold(&pat)
            } else {
                pat.chars().collect()
            };

            let all = q.find_all(&lines);

            // Per-row ordering + non-overlap, and the well-formed invariant.
            let mut prev: Option<&Match> = None;
            for m in &all {
                let char_count = lines[m.row].chars().count();
                assert!(
                    m.start < m.end && m.end <= char_count,
                    "Match {m:?} violated start < end <= chars().count() ({char_count})"
                );
                // The matched char slice equals the pattern under folding.
                let slice: Vec<char> = lines[m.row]
                    .chars()
                    .skip(m.start)
                    .take(m.end - m.start)
                    .collect();
                let slice_cmp: Vec<char> = if insensitive {
                    slice.iter().flat_map(|c| c.to_lowercase()).collect()
                } else {
                    slice
                };
                assert_eq!(
                    slice_cmp, folded_pat,
                    "matched slice did not equal the pattern under folding"
                );
                if let Some(p) = prev {
                    if p.row == m.row {
                        assert!(
                            m.start >= p.end,
                            "overlapping/out-of-order matches {p:?} then {m:?}"
                        );
                    } else {
                        assert!(p.row < m.row, "rows out of order: {p:?} then {m:?}");
                    }
                }
                prev = Some(m);
            }
            // An empty pattern must yield nothing.
            if q.is_empty() {
                assert!(all.is_empty());
            }

            // Random, possibly wild, `from` positions: next/prev never panic
            // and (when found) return a well-formed, in-document match.
            for _ in 0..4 {
                let from = match rng() % 4 {
                    0 => (0, 0),
                    1 => ((rng() % 8) as usize, (rng() % 12) as usize),
                    2 => (usize::MAX, usize::MAX),
                    _ => ((rng() % 3) as usize, usize::MAX),
                };
                let wrap = rng() % 2 == 0;
                for m in [
                    q.next_from(&lines, from, wrap),
                    q.prev_from(&lines, from, wrap),
                ]
                .into_iter()
                .flatten()
                {
                    assert!(m.row < lines.len(), "match row {} out of document", m.row);
                    let cc = lines[m.row].chars().count();
                    assert!(
                        m.start < m.end && m.end <= cc,
                        "next/prev returned malformed {m:?} (chars {cc})"
                    );
                }
            }
        }
        // Reaching here proves no input panicked.
    }
}
