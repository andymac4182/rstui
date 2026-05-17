//! [`Extmark`] — a caller-owned, optionally *atomic*, styled character range
//! that [`Editor`](crate::Editor) and [`Input`](crate::Input) project over
//! their text.
//!
//! # The reducer owns it; the widget only projects it
//!
//! An extmark is the **@-mention / pasted-file "pill"** model: a contiguous
//! run of the model's text the app wants drawn with a distinct
//! [`Style`] (a highlighted chip), and — when
//! [`atomic`](Extmark::atomic) — treated by the *reducer's* cursor logic as a
//! single indivisible unit (the caret steps over the whole pill in one
//! `move_left`/`move_right`, Backspace removes it whole). It is exactly the
//! [`TextEdit`](rstui_core::TextEdit)/[`TextArea`](rstui_core::TextArea)
//! discipline taken one step further: the **reducer owns the extmark list and
//! re-derives it on every edit** (insert at column 3 ⇒ every range starting at
//! or after 3 shifts), and the widget — handed only a
//! [`Buffer`](rstui_core::Buffer) at render time — *only reads* it. The widget
//! never mutates a range and never enforces atomicity; it merely paints the
//! styled span. Atomicity is a property the reducer honours when it moves the
//! caret, not something the pure projection can (or should) do.
//!
//! This keeps extmarks on exactly the same side of the line as `focused`,
//! `scroll`, and the text itself: plain model state the pure `view` reads, the
//! single `update` mutates ([ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
//! §1). No retained tree, no widget-owned state.
//!
//! # Character indices, never bytes — and total
//!
//! [`range`](Extmark::range) is a half-open **character-index** range into the
//! model's text — for [`Input`](crate::Input) the char index into
//! [`value`](rstui_core::TextEdit::value); for [`Editor`](crate::Editor) the
//! char index into the flattened document (rows joined by `'\n'`, exactly
//! [`TextArea`](rstui_core::TextArea)'s `to_string()`), so a pill may span a
//! line break. Character indices match rstui's single-`char` cell model the
//! same way [`TextEdit`](rstui_core::TextEdit)'s cursor does; no byte math ever
//! leaks through, so a multi-byte pill is automatically correct.
//!
//! Every shape is **total** (the iter-25 "a pure projection must be total"
//! rule): an empty range (`3..3`), a reversed range (`5..2`), and a range past
//! the end of the text all simply paint nothing — never a panic, never an
//! out-of-bounds. Overlapping extmarks cascade in slice order (a later
//! [`Extmark`] patched over an earlier one), the same last-wins idiom
//! [`List`](crate::List)'s highlight uses.

use rstui_core::Style;

/// A styled span of a text model, addressed by **character index**.
///
/// Construct one directly (the fields are public so a reducer can rebuild the
/// list cheaply every edit) or via [`Extmark::new`] /
/// [`Extmark::pill`]. Pass a `&[Extmark]` to
/// [`Input::extmarks`](crate::Input::extmarks) /
/// [`Editor::extmarks`](crate::Editor::extmarks); at render the widget patches
/// [`style`](Self::style) over every cell whose character index lies in
/// [`range`](Self::range), beneath the focus fill and beneath the cursor caret
/// (cascade: base → focus → **extmark** → caret).
#[derive(Debug, Clone)]
pub struct Extmark {
    /// The half-open character-index range the style covers. Empty, reversed,
    /// or out-of-range values paint nothing (the projection is total).
    pub range: core::ops::Range<usize>,
    /// The [`Style`] patched over the cells in [`range`](Self::range), over
    /// the base/focus fill and under the caret.
    pub style: Style,
    /// Whether the reducer treats this span as one indivisible unit when it
    /// moves the caret or deletes (a non-editable "pill"). The widget carries
    /// the flag but does **not** act on it — enforcing atomic cursor stepping
    /// is the reducer's job, since only `update` owns the caret.
    pub atomic: bool,
}

impl Extmark {
    /// A non-atomic styled span (a plain highlight, freely editable through).
    #[must_use]
    pub fn new(range: core::ops::Range<usize>, style: Style) -> Self {
        Self {
            range,
            style,
            atomic: false,
        }
    }

    /// An **atomic** styled span — the @-mention / pasted-file pill the
    /// reducer steps over and deletes as a whole. Identical projection to
    /// [`new`](Self::new); the flag is the caller's signal to its own cursor
    /// logic (the widget never reads it).
    #[must_use]
    pub fn pill(range: core::ops::Range<usize>, style: Style) -> Self {
        Self {
            range,
            style,
            atomic: true,
        }
    }
}

/// Patches every [`Extmark`] in `marks` whose [`range`](Extmark::range)
/// contains the character index `char_idx` over `base`, in slice order so a
/// later mark wins (the last-wins cascade). Totally defined: an empty,
/// reversed, or out-of-range range simply does not match.
pub(crate) fn patch_at(base: Style, marks: &[Extmark], char_idx: usize) -> Style {
    let mut style = base;
    for mark in marks {
        if mark.range.contains(&char_idx) {
            style = style.patch(mark.style);
        }
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Color;

    #[test]
    fn new_is_not_atomic_and_pill_is() {
        let m = Extmark::new(0..3, Style::new());
        assert!(!m.atomic);
        let p = Extmark::pill(0..3, Style::new());
        assert!(p.atomic);
    }

    #[test]
    fn patch_at_applies_only_inside_the_range() {
        let red = Style::new().fg(Color::Red);
        let marks = [Extmark::new(2..4, red)];
        // Inside the range gets the style; outside keeps the base.
        assert_eq!(patch_at(Style::new(), &marks, 2).fg, Some(Color::Red));
        assert_eq!(patch_at(Style::new(), &marks, 3).fg, Some(Color::Red));
        assert_eq!(patch_at(Style::new(), &marks, 1).fg, None);
        assert_eq!(patch_at(Style::new(), &marks, 4).fg, None);
    }

    #[test]
    // Reversed/empty ranges are exactly what this totality test feeds in.
    #[allow(clippy::reversed_empty_ranges)]
    fn empty_reversed_and_out_of_range_match_nothing_and_never_panic() {
        let red = Style::new().fg(Color::Red);
        for range in [3..3, 5..2, 100..200] {
            let marks = [Extmark::new(range, red)];
            for idx in 0..10 {
                assert_eq!(patch_at(Style::new(), &marks, idx).fg, None);
            }
        }
    }

    #[test]
    fn overlapping_marks_cascade_in_slice_order_last_wins() {
        let marks = [
            Extmark::new(0..6, Style::new().fg(Color::Red)),
            Extmark::new(2..4, Style::new().fg(Color::Blue)),
        ];
        // Only the first mark covers idx 1; both cover idx 3 → the later wins.
        assert_eq!(patch_at(Style::new(), &marks, 1).fg, Some(Color::Red));
        assert_eq!(patch_at(Style::new(), &marks, 3).fg, Some(Color::Blue));
    }
}
