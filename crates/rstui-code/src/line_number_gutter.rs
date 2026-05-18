//! [`LineNumberGutter`] — a pure **layout** widget that draws a right-aligned
//! numeric gutter (plus an optional sign column) and hands the remaining
//! [`Rect`] back for the caller to render content into.
//!
//! # The `Block::inner` pattern, specialized to a line-number rail
//!
//! [`Diff`](crate::Diff) bakes a line-number gutter into its own rendering;
//! that is the right call *there* (it owns the patch model). But a code pane,
//! a focused [`Editor`](crate::Editor), or a log view wants the **same** rail
//! in front of content it renders itself. `LineNumberGutter` is that rail
//! extracted as a standalone, composable widget: it owns *no* application
//! state, draws only the numbers/signs, and exposes
//! [`inner`](LineNumberGutter::inner) — the exact
//! [`Block::inner`](rstui_widgets::Block::inner) composition seam. The caller renders
//! the gutter, then renders code/diff/editor into `gutter.inner(area)`, the
//! same two-step every container widget uses:
//!
//! ```text
//! ┌ area ─────────────────────────┐
//! │ 12 │ fn main() {              │  ← gutter (numbers) │ inner (content)
//! │ 13 │     let x = 1;           │
//! └───────────────────────────────┘
//! ```
//!
//! # Caller-owned, a pure projection, total
//!
//! The first line number, the row count, and the optional per-row sign glyph
//! and per-row [`Style`] are all caller-owned inputs the widget only reads —
//! the reducer decides what they are (e.g. signs from a diff, a `>` on the
//! caret row), exactly the [`List`](rstui_widgets::List)/[`Editor`](crate::Editor)
//! discipline. It is **total**: line numbers up to [`u64::MAX`] saturate
//! rather than overflow, an area too narrow for the gutter clips (and
//! [`inner`](LineNumberGutter::inner) collapses to an empty rect), zero rows
//! draws nothing — never a panic.

use rstui_core::{Buffer, Position, Rect, Style, Widget};
use rstui_widgets::Block;

/// A right-aligned line-number gutter with an optional sign column and an
/// optional framing [`Block`], exposing the content [`Rect`] via
/// [`inner`](Self::inner).
///
/// Numbers run `first ..= first + rows - 1`, one per row from the top of the
/// (block-)inner area. The gutter is `number_width + sign_width + 1` columns
/// wide (a one-column separator before the content); the rest of the area is
/// the caller's via [`inner`](Self::inner).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_code::LineNumberGutter;
///
/// let gutter = LineNumberGutter::new(8, 3); // lines 8, 9, 10
/// let area = Rect::new(0, 0, 10, 3);
///
/// // `inner` is a pure accessor (no render yet) — the `Block::inner` seam.
/// // Width: digits("10") = 2, no sign, +1 separator → gutter is 3 wide.
/// let content = gutter.inner(area);
/// assert_eq!(content, Rect::new(3, 0, 7, 3));
///
/// let mut buf = Buffer::empty(area);
/// gutter.render(area, &mut buf);
/// // " 8" is right-aligned in the 2-wide number column.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '8');
/// assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, '1'); // "10"
/// assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, '0');
/// ```
#[derive(Debug, Clone)]
pub struct LineNumberGutter<'a> {
    first: u64,
    rows: usize,
    min_number_width: u16,
    signs: &'a [char],
    row_styles: &'a [Style],
    block: Option<Block<'a>>,
    style: Style,
}

impl<'a> LineNumberGutter<'a> {
    /// A gutter numbering `rows` rows starting at line `first`: no sign
    /// column, no per-row styling, no block, unstyled.
    #[must_use]
    pub fn new(first: u64, rows: usize) -> Self {
        Self {
            first,
            rows,
            min_number_width: 0,
            signs: &[],
            row_styles: &[],
            block: None,
            style: Style::new(),
        }
    }

    /// Frames the gutter+content in `block`; everything renders into
    /// [`block.inner`](rstui_widgets::Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]. It also fills the whole gutter (numbers, sign
    /// column, and the separator) so a background reads as one rail.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the per-row sign glyphs (caller-owned), indexed by **screen row**
    /// (`signs[0]` is the top row). Providing a non-empty slice adds a
    /// one-column sign column after the numbers; a row past the slice's end
    /// gets a blank. The classic use is a diff `+`/`-`/` ` or a `>` on the
    /// caret row.
    #[must_use]
    pub fn signs(mut self, signs: &'a [char]) -> Self {
        self.signs = signs;
        self
    }

    /// Sets the per-row gutter [`Style`] overrides (caller-owned), indexed by
    /// screen row and patched over [`style`](Self::style) for that row's
    /// number and sign cells (e.g. an added line green, the caret row bold).
    /// A row past the slice's end keeps the base style.
    #[must_use]
    pub fn row_styles(mut self, row_styles: &'a [Style]) -> Self {
        self.row_styles = row_styles;
        self
    }

    /// Sets a minimum number-column width. The column is the wider of this and
    /// the digits of the largest line number — so a streaming view can pin a
    /// stable gutter width (no content reflow as numbers cross a power of ten)
    /// the same way [`Diff`](crate::Diff) sizes its gutter once.
    #[must_use]
    pub fn min_number_width(mut self, width: u16) -> Self {
        self.min_number_width = width;
        self
    }

    /// The number-column width: the decimal digits of the largest line number
    /// (`first + rows - 1`, saturating), at least 1 and at least
    /// [`min_number_width`](Self::min_number_width).
    fn number_width(&self) -> usize {
        let last = self
            .first
            .saturating_add(self.rows as u64)
            .saturating_sub(1);
        digits(last).max(1).max(self.min_number_width as usize)
    }

    /// The sign-column width: 1 if any signs were provided, else 0.
    fn sign_width(&self) -> usize {
        usize::from(!self.signs.is_empty())
    }

    /// The total gutter width: numbers + optional sign + a one-column
    /// separator, saturated into a [`u16`] (a pathologically large
    /// `min_number_width` clamps rather than overflows).
    fn gutter_width(&self) -> u16 {
        let cols = self
            .number_width()
            .saturating_add(self.sign_width())
            .saturating_add(1);
        u16::try_from(cols).unwrap_or(u16::MAX)
    }

    /// The content [`Rect`] to the right of the gutter — the
    /// [`Block::inner`](rstui_widgets::Block::inner) composition seam.
    ///
    /// A **pure geometry accessor** (no [`Buffer`], owns no state): render the
    /// gutter into `area`, then render content into `gutter.inner(area)`. The
    /// result is clamped (never wider than `area`, never negative) so a gutter
    /// in a too-narrow region degrades to an empty inner rect, not a panic.
    #[must_use]
    pub fn inner(&self, area: Rect) -> Rect {
        let base = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        let gw = self.gutter_width();
        Rect::new(
            base.x.saturating_add(gw),
            base.y,
            base.width.saturating_sub(gw),
            base.height,
        )
    }
}

impl Widget for LineNumberGutter<'_> {
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

        let num_w = self.number_width();
        let sign_w = self.sign_width();
        let gw = self.gutter_width();

        // Fill the gutter (numbers + sign + separator) so a background reads
        // as one rail — the Editor/List base-fill idiom, clipped to `inner`.
        let fill_w = gw.min(inner.width);
        buf.set_style(
            Rect::new(inner.x, inner.y, fill_w, inner.height),
            self.style,
        );

        let left = inner.left();
        let right = inner.right();
        let top = inner.top();

        // Only the rows that fit are drawn; this also bounds the loop so a
        // huge `rows` cannot wrap the `u16` screen-row math.
        let visible = self.rows.min(inner.height as usize);
        let num_w_u16 = u16::try_from(num_w).unwrap_or(u16::MAX);
        for r in 0..visible {
            let y = top.saturating_add(r as u16);
            let n = self.first.saturating_add(r as u64);
            // Decimal digits of `n`, written from a stack buffer instead of
            // `n.to_string()` — that allocated a String per visible row every
            // frame (GUT-1), scrolling a code pane churned one heap String per
            // line each frame. `u64` is at most 20 digits; `n == 0` → "0".
            let mut digits = [0u8; 20];
            let mut ndig = 0;
            let mut v = n;
            loop {
                digits[ndig] = b'0' + (v % 10) as u8;
                ndig += 1;
                v /= 10;
                if v == 0 {
                    break;
                }
            }
            let row_style = self
                .style
                .patch(self.row_styles.get(r).copied().unwrap_or_else(Style::new));

            // Right-align the number within `num_w` columns starting at left.
            let pad = num_w.saturating_sub(ndig);
            let mut x = left;
            for _ in 0..pad {
                if x >= right {
                    break;
                }
                buf.set_cell(Position::new(x, y), ' ', row_style);
                x = x.saturating_add(1);
            }
            // `digits` holds least-significant first; stamp most-significant
            // first for the same glyph order `to_string()` produced.
            for k in (0..ndig).rev() {
                if x >= right {
                    break;
                }
                buf.set_cell(Position::new(x, y), digits[k] as char, row_style);
                x = x.saturating_add(1);
            }

            // The sign sits in its own column right after the numbers.
            if sign_w == 1 {
                let sx = left.saturating_add(num_w_u16);
                if sx < right {
                    let glyph = self.signs.get(r).copied().unwrap_or(' ');
                    buf.set_cell(Position::new(sx, y), glyph, row_style);
                }
            }
        }
    }
}

/// The decimal digit count of `n` (at least 1, for `0`) — mirrors the
/// `Diff` gutter's `digits`, widened to [`u64`] for the standalone widget.
fn digits(n: u64) -> usize {
    if n == 0 { 1 } else { (n.ilog10() + 1) as usize }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Color;

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row.
    fn lines(widget: LineNumberGutter, width: u16, height: u16) -> String {
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
    fn numbers_are_right_aligned_one_per_row() {
        // first 9, 3 rows → 9, 10, 11. num_w = digits(11) = 2, +1 sep = 3.
        assert_eq!(
            lines(LineNumberGutter::new(9, 3), 5, 3),
            " 9   \n10   \n11   \n"
        );
    }

    #[test]
    fn inner_is_the_block_inner_pattern_and_pure() {
        let g = LineNumberGutter::new(1, 5); // 1..=5, num_w = 1, +1 = 2 wide.
        assert_eq!(g.inner(Rect::new(0, 0, 10, 5)), Rect::new(2, 0, 8, 5));
        // Calling it does not draw — a pure geometry accessor.
        assert_eq!(g.inner(Rect::new(4, 2, 20, 3)), Rect::new(6, 2, 18, 3));
    }

    #[test]
    fn a_block_frames_the_gutter_and_inner_subtracts_both() {
        let g = LineNumberGutter::new(1, 1).block(Block::bordered());
        // Block inner of 8×3 is (1,1,6,1); gutter is num_w(1)+1 = 2 → (3,1,4,1).
        assert_eq!(g.inner(Rect::new(0, 0, 8, 3)), Rect::new(3, 1, 4, 1));

        let g = LineNumberGutter::new(1, 1).block(Block::bordered());
        assert_eq!(
            lines(g, 6, 3),
            "┌────┐\n\
             │1   │\n\
             └────┘\n"
        );
    }

    #[test]
    fn a_sign_column_is_added_when_signs_are_given() {
        let signs = ['+', '-', ' '];
        // num_w = 1, sign_w = 1, +1 sep → gutter 3 wide.
        let g = LineNumberGutter::new(1, 3).signs(&signs);
        assert_eq!(g.inner(Rect::new(0, 0, 10, 3)), Rect::new(3, 0, 7, 3));
        assert_eq!(lines(g, 4, 3), "1+  \n2-  \n3   \n");
    }

    #[test]
    fn signs_shorter_than_rows_blank_the_remaining_rows() {
        let signs = ['>']; // only the first row has a sign
        let g = LineNumberGutter::new(1, 3).signs(&signs);
        assert_eq!(lines(g, 4, 3), "1>  \n2   \n3   \n");
    }

    #[test]
    fn per_row_styles_are_patched_over_the_base_for_that_row_only() {
        let row_styles = [
            Style::new().fg(Color::Green),
            Style::new(),
            Style::new().fg(Color::Red),
        ];
        let g = LineNumberGutter::new(1, 3).row_styles(&row_styles);
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 3));
        g.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Green);
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().fg, Color::Reset);
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().fg, Color::Red);
    }

    #[test]
    fn min_number_width_pins_a_stable_gutter_width() {
        // Only 1 digit needed, but a min of 4 keeps the column stable.
        let g = LineNumberGutter::new(1, 2).min_number_width(4);
        assert_eq!(g.inner(Rect::new(0, 0, 12, 2)), Rect::new(5, 0, 7, 2));
        assert_eq!(lines(g, 6, 2), "   1  \n   2  \n");
    }

    #[test]
    fn huge_line_numbers_saturate_and_never_overflow() {
        // first near u64::MAX: digits(u64::MAX) = 20, saturating add/sub.
        let g = LineNumberGutter::new(u64::MAX - 1, 3);
        assert_eq!(g.number_width(), 20);
        let mut buf = Buffer::empty(Rect::new(0, 0, 22, 3));
        g.render(buf.area(), &mut buf); // must not panic
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '1');
    }

    #[test]
    fn a_too_narrow_area_clips_and_inner_collapses() {
        // Gutter wants 3 cols but the area is 2 wide: clip, inner empty.
        let g = LineNumberGutter::new(10, 2);
        assert!(g.inner(Rect::new(0, 0, 2, 2)).is_empty());
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        g.render(buf.area(), &mut buf); // no panic
        // The first two digit cells of "10" still drew, clipped.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '1');
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '0');
    }

    #[test]
    fn zero_rows_draws_nothing_but_still_reserves_the_gutter() {
        let g = LineNumberGutter::new(1, 0);
        // No numbers, but the gutter width is still reserved (min 1 + sep).
        assert_eq!(g.inner(Rect::new(0, 0, 6, 2)), Rect::new(2, 0, 4, 2));
        assert_eq!(lines(LineNumberGutter::new(1, 0), 4, 2), "    \n    \n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        LineNumberGutter::new(1, 5).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn the_base_style_fills_the_whole_gutter_rail() {
        let g = LineNumberGutter::new(1, 2).style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        g.render(buf.area(), &mut buf);
        // Both the digit column and the separator carry the background.
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Blue);
            }
        }
    }

    #[test]
    fn content_renders_into_inner_and_composes_with_the_gutter() {
        let g = LineNumberGutter::new(1, 1);
        let area = Rect::new(0, 0, 8, 1);
        let content = g.inner(area);
        let mut buf = Buffer::empty(area);
        g.render(area, &mut buf);
        "code".render(content, &mut buf);
        // Gutter number then content, side by side, no overlap.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '1');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'c');
        assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, 'e');
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        LineNumberGutter::new(1, 2).render(Rect::new(3, 1, 4, 2), &mut buf);
        assert_eq!(buf.get(Position::new(3, 1)).unwrap().symbol, '1');
        assert_eq!(buf.get(Position::new(3, 2)).unwrap().symbol, '2');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }
}
