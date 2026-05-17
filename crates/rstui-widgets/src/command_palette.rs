//! [`CommandPalette`] — the canonical fuzzy-command UI: a query row above a
//! filtered result list, in a centred **opaque** panel. The worked example of
//! third-party **composition**.
//!
//! # A pure projection — and the widget does **not** filter
//!
//! Like every rstui widget `CommandPalette` is a **pure projection**. It
//! borrows a caller-owned [`TextEdit`] (the query, the
//! exact model [`Input`] projects) and a caller-owned slice of
//! result [`Line`]s, plus a caller-owned [`highlight`](CommandPalette::highlight)
//! / [`offset`](CommandPalette::offset) / [`focused`](CommandPalette::focused).
//! It renders precisely those results in order and reads nothing else: the
//! **fuzzy matching that turns the query into `results` is the reducer's job**,
//! recomputed in `update` whenever the query changes, never inside the pure
//! `view` — the same "no algorithm smuggled into the view" discipline
//! [`Toast`](crate::Toast) applies to expiry. Moving the highlight and
//! activating the chosen command are likewise the reducer's, exactly the
//! read-only-state rule [`List`]/[`Select`](crate::Select)
//! establish.
//!
//! # The worked example of third-party **composition**
//!
//! Where [`Select`](crate::Select) is the worked example of *reusing one
//! widget wholesale*, `CommandPalette` is the worked example of **composing
//! several**. It owns no glyph-stamping of its own; it is assembled from the
//! public widget set exactly as a third-party crate would assemble it:
//!
//! - it sizes and centres an **opaque** panel with the
//!   [`Modal`](crate::Modal) sizing/placement math (a
//!   [`width`](CommandPalette::width)/[`height`](CommandPalette::height)
//!   [`Constraint`] each, centred, odd remainder toward the start), and
//!   [`clear_region`](rstui_core::Buffer::clear_region)s it opaque for the same
//!   reason `Modal` does (`modal.rs:29-38`: a [`Style`] is a patch and cannot
//!   reset a cell);
//! - it frames the panel with an optional [`Block`];
//! - it renders the query through [`Input`] (borrowing the same
//!   `TextEdit`), behind a [`prompt`](CommandPalette::prompt) glyph run;
//! - it renders the results through [`List`] (inheriting its
//!   scroll, highlight bar, and totality).
//!
//! It is deliberately **its own type, not a [`Modal`](crate::Modal)**: a
//! palette is not a focus-scope trap and has a fixed two-region internal
//! layout, so — the [`Select`](crate::Select)-not-`Modal` precedent — it
//! borrows `Modal`'s math without inheriting modal behaviour.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! overlay, a panel that resolves to zero, an inner too short for even the
//! query row, no results, an out-of-range highlight, and a prompt wider than
//! the panel are all safe clips/no-ops — never a panic. A score-match
//! highlight overlay on each result row is a deliberately deferred additive,
//! not smuggled into this slice.

use std::borrow::Cow;

use rstui_core::{Buffer, Constraint, Line, Position, Rect, Style, TextEdit, Widget};

use crate::block::Block;
use crate::input::Input;
use crate::list::List;

/// The default [`prompt`](CommandPalette::prompt) drawn before the query.
const PROMPT: &str = "> ";

/// A centred, opaque fuzzy-command panel composed of a [`prompt`] + query
/// [`Input`] row above a [`List`] of results.
///
/// A **pure projection** of a borrowed caller-owned
/// [`TextEdit`] and result slice (see the
/// [module docs](self)): the widget renders the results exactly as given — the
/// fuzzy filtering, highlight movement, and command activation are all the
/// reducer's job.
///
/// [`prompt`]: Self::prompt
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Line, Rect, TextEdit, Widget};
/// use rstui_widgets::CommandPalette;
///
/// // `query` and `results` are plain caller-owned model state the widget only
/// // reads — the reducer recomputes `results` from the query and activates
/// // the highlighted command; the widget never filters.
/// let query = TextEdit::from_value("op");
/// let results = [Line::raw("Open File"), Line::raw("Open Recent")];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
/// CommandPalette::new(&query, &results)
///     .highlight(0)
///     .render(buf.area(), &mut buf);
///
/// // Pure geometry: the centred panel rect (default 60% × 50%), independent
/// // of the buffer — exposed for click-to-row mapping.
/// let palette = CommandPalette::new(&query, &results);
/// assert_eq!(palette.area(Rect::new(0, 0, 20, 8)), Rect::new(4, 2, 12, 4));
/// ```
#[derive(Debug, Clone)]
pub struct CommandPalette<'a> {
    query: &'a TextEdit,
    results: &'a [Line<'a>],
    highlight: usize,
    offset: usize,
    focused: bool,
    block: Option<Block<'a>>,
    width: Constraint,
    height: Constraint,
    prompt: Cow<'a, str>,
    style: Style,
    highlight_style: Style,
    backdrop_style: Style,
}

impl<'a> CommandPalette<'a> {
    /// A palette projecting `query` and `results`, focused-off, the first
    /// result highlighted, sized at 60% × 50% of the overlay and centred,
    /// unframed, with the default `"> "` prompt and no backdrop scrim.
    #[must_use]
    pub fn new(query: &'a TextEdit, results: &'a [Line<'a>]) -> Self {
        Self {
            query,
            results,
            highlight: 0,
            offset: 0,
            focused: false,
            block: None,
            width: Constraint::Percentage(60),
            height: Constraint::Percentage(50),
            prompt: Cow::Borrowed(PROMPT),
            style: Style::new(),
            highlight_style: Style::new(),
            backdrop_style: Style::new(),
        }
    }

    /// Sets which result row the highlight bar is on — caller-owned state the
    /// widget only reads. Out of range simply paints no bar (inherited from
    /// [`List`]).
    #[must_use]
    pub fn highlight(mut self, highlight: usize) -> Self {
        self.highlight = highlight;
        self
    }

    /// Sets the result list's scroll offset, exactly
    /// [`List::offset`](crate::List::offset).
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Sets whether the query row shows the focused [`Input`]
    /// caret — caller-owned state the widget only reads.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Frames the panel in `block`; the query/results render into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the panel width within the overlay (default
    /// [`Percentage(60)`](Constraint::Percentage)). Resolved with
    /// [`Constraint::apply`], so it never exceeds the overlay.
    #[must_use]
    pub fn width(mut self, width: Constraint) -> Self {
        self.width = width;
        self
    }

    /// Sets the panel height within the overlay (default
    /// [`Percentage(50)`](Constraint::Percentage)). Resolved with
    /// [`Constraint::apply`], so it never exceeds the overlay.
    #[must_use]
    pub fn height(mut self, height: Constraint) -> Self {
        self.height = height;
        self
    }

    /// Sets the glyph run drawn before the query (default `"> "`).
    #[must_use]
    pub fn prompt(mut self, prompt: impl Into<Cow<'a, str>>) -> Self {
        self.prompt = prompt.into();
        self
    }

    /// Sets the base [`Style`] of the query row and result list.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched **last** over the highlighted result row,
    /// forwarded straight to the internal [`List`].
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Sets the scrim [`Style`] patched over the *whole* overlay behind the
    /// panel. Defaults empty — opt-in, exactly like
    /// [`Modal::backdrop_style`](crate::Modal::backdrop_style).
    #[must_use]
    pub fn backdrop_style(mut self, style: Style) -> Self {
        self.backdrop_style = style;
        self
    }

    /// The centred panel rect for a given `overlay` area — a pure function of
    /// `overlay` and the size constraints, the [`Modal::area`](crate::Modal)
    /// math (resolved against the overlay, centred, odd remainder toward the
    /// start). Exposed so an app can map a click in the panel to a row.
    #[must_use]
    pub fn area(&self, overlay: Rect) -> Rect {
        let w = self.width.apply(overlay.width);
        let h = self.height.apply(overlay.height);
        let x = overlay
            .x
            .saturating_add(overlay.width.saturating_sub(w) / 2);
        let y = overlay
            .y
            .saturating_add(overlay.height.saturating_sub(h) / 2);
        Rect::new(x, y, w, h)
    }

    /// The content rect inside the panel: [`area`](Self::area) minus the
    /// optional [`block`](Self::block) frame (the whole panel when unframed),
    /// exactly [`Modal::inner`](crate::Modal).
    #[must_use]
    pub fn inner(&self, overlay: Rect) -> Rect {
        let panel = self.area(overlay);
        match &self.block {
            Some(block) => block.inner(panel),
            None => panel,
        }
    }
}

impl Widget for CommandPalette<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // The scrim over the whole overlay (a patch; default-empty is a
        // no-op), exactly as `Modal` dims its background.
        buf.set_style(area, self.backdrop_style);

        let panel = self.area(area);
        if panel.is_empty() {
            return;
        }

        // Opaque panel (the `Modal` `clear_region` affordance — see the
        // module docs), then the optional frame.
        buf.clear_region(panel);
        let inner = match &self.block {
            Some(b) => b.inner(panel),
            None => panel,
        };
        if let Some(block) = self.block {
            block.render(panel, buf);
        }
        if inner.is_empty() {
            return;
        }

        // Region 1 — the query row: the prompt glyphs, then the borrowed
        // `TextEdit` composed straight through `Input`.
        let query_row = Rect::new(inner.x, inner.y, inner.width, 1);
        let mut x = query_row.left();
        for ch in self.prompt.chars() {
            if x >= query_row.right() {
                break;
            }
            buf.set_cell(Position::new(x, query_row.y), ch, self.style);
            x = x.saturating_add(1);
        }
        let input_rect = Rect::new(x, query_row.y, query_row.right().saturating_sub(x), 1);
        Input::new(self.query)
            .focused(self.focused)
            .style(self.style)
            .render(input_rect, buf);

        // Region 2 — the results, composed through `List` (its scroll,
        // highlight bar, and totality inherited, never re-implemented).
        if inner.height < 2 {
            return;
        }
        let results_rect = Rect::new(
            inner.x,
            inner.y.saturating_add(1),
            inner.width,
            inner.height.saturating_sub(1),
        );
        // CP-1: feed `List` only the window it will show. It renders
        // `results[offset, offset + results_rect.height)` as a pure
        // projection of `(results, selected, offset)` (no `len()`-derived
        // state and no block here, so its inner is exactly `results_rect`),
        // so the windowed slice + zero offset + rebased highlight is
        // byte-identical (the offset/highlight snapshot tests gate-enforce
        // it) while a huge match set clones only the ~visible rows.
        let h = results_rect.height as usize;
        let start = self.offset.min(self.results.len());
        let end = self.offset.saturating_add(h).min(self.results.len());
        List::new(self.results[start..end].iter().cloned())
            .selected(self.highlight.checked_sub(start))
            .offset(0)
            .style(self.style)
            .highlight_style(self.highlight_style)
            .render(results_rect, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier};

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

    /// Fills `buf` with a styled `.` background so a clear is observable.
    fn background(buf: &mut Buffer) {
        let style = Style::new().fg(Color::Red).bg(Color::Blue);
        for p in buf.area().positions() {
            buf.set_cell(p, '.', style);
        }
    }

    #[test]
    fn area_is_centred_within_the_overlay() {
        let q = TextEdit::new();
        let r: [Line<'_>; 0] = [];
        let p = CommandPalette::new(&q, &r)
            .width(Constraint::Length(10))
            .height(Constraint::Length(4));
        assert_eq!(p.area(Rect::new(0, 0, 20, 10)), Rect::new(5, 3, 10, 4));
    }

    #[test]
    fn inner_subtracts_the_block_frame() {
        let q = TextEdit::new();
        let r: [Line<'_>; 0] = [];
        let p = CommandPalette::new(&q, &r)
            .width(Constraint::Length(10))
            .height(Constraint::Length(4));
        assert_eq!(
            p.inner(Rect::new(0, 0, 20, 10)),
            p.area(Rect::new(0, 0, 20, 10))
        );
        let framed = p.clone().block(Block::bordered());
        assert_eq!(framed.inner(Rect::new(0, 0, 20, 10)), Rect::new(6, 4, 8, 2));
    }

    #[test]
    fn the_prompt_precedes_the_query_and_results_follow_below() {
        let q = TextEdit::from_value("ab");
        let r = [Line::raw("First"), Line::raw("Second")];
        // Full-overlay panel: prompt "> " then "ab" on row 0, results below.
        assert_eq!(
            lines(
                CommandPalette::new(&q, &r)
                    .width(Constraint::Percentage(100))
                    .height(Constraint::Percentage(100)),
                7,
                3,
            ),
            "> ab   \nFirst  \nSecond \n"
        );
    }

    #[test]
    fn a_custom_prompt_is_drawn() {
        let q = TextEdit::from_value("x");
        let r: [Line<'_>; 0] = [];
        assert_eq!(
            lines(
                CommandPalette::new(&q, &r)
                    .prompt(": ")
                    .width(Constraint::Percentage(100))
                    .height(Constraint::Percentage(100)),
                5,
                1,
            ),
            ": x  \n"
        );
    }

    #[test]
    fn the_focused_query_draws_an_input_caret() {
        let q = TextEdit::from_value("hi"); // cursor at end (2)
        let r: [Line<'_>; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        CommandPalette::new(&q, &r)
            .focused(true)
            .width(Constraint::Percentage(100))
            .height(Constraint::Percentage(100))
            .render(buf.area(), &mut buf);
        // "> hi" then the reversed caret in the blank after "hi" (col 4).
        let caret = buf.get(Position::new(4, 0)).unwrap();
        assert_eq!(caret.symbol, ' ');
        assert!(caret.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn the_highlighted_result_gets_the_bar() {
        let q = TextEdit::new();
        let r = [Line::raw("a"), Line::raw("b")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        CommandPalette::new(&q, &r)
            .highlight(1)
            .width(Constraint::Percentage(100))
            .height(Constraint::Percentage(100))
            .highlight_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        // Results start on row 1; result index 1 ("b") is on row 2.
        for x in 0..4 {
            assert_eq!(buf.get(Position::new(x, 2)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn results_scroll_with_the_offset() {
        let q = TextEdit::new();
        let r = [Line::raw("r0"), Line::raw("r1"), Line::raw("r2")];
        assert_eq!(
            lines(
                CommandPalette::new(&q, &r)
                    .offset(1)
                    .width(Constraint::Percentage(100))
                    .height(Constraint::Percentage(100)),
                2,
                3,
            ),
            // Width 2: the default "> " prompt fills the query row exactly
            // (no room for the Input), then the offset results follow.
            "> \nr1\nr2\n"
        );
    }

    #[test]
    fn a_scrolled_highlight_bars_the_correct_windowed_row() {
        // CP-1 gate: offset > 0 *and* the highlight inside the scrolled
        // window — the combo the windowing's `selected − start` rebase must
        // get right (the other cases are covered by the scroll/out-of-range
        // tests above). r0..r5, offset 2 ⇒ results show r2,r3,r4…;
        // highlight 3 ⇒ r3, the 2nd visible result. Row 0 is the query, so
        // results begin on row 1 and r3 lands on row 2.
        let q = TextEdit::new();
        let r = [
            Line::raw("r0"),
            Line::raw("r1"),
            Line::raw("r2"),
            Line::raw("r3"),
            Line::raw("r4"),
            Line::raw("r5"),
        ];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 4));
        CommandPalette::new(&q, &r)
            .offset(2)
            .highlight(3)
            .width(Constraint::Percentage(100))
            .height(Constraint::Percentage(100))
            .highlight_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'r');
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, '2'); // r2, row 1
        for x in 0..4 {
            // r3 (highlight) is the 2nd visible result → row 2, full bar.
            assert_eq!(buf.get(Position::new(x, 2)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn the_panel_is_opaque_and_centred_over_a_background() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 6));
        background(&mut buf);
        let q = TextEdit::new();
        let r: [Line<'_>; 0] = [];
        CommandPalette::new(&q, &r)
            .width(Constraint::Length(4))
            .height(Constraint::Length(2))
            .render(buf.area(), &mut buf);
        // Centred 4x2 panel at (2,2): cleared opaque, no '.' bleeds through.
        let inside = buf.get(Position::new(3, 2)).unwrap();
        assert_eq!(inside.symbol, ' ');
        assert_eq!(inside.bg, Color::Reset);
        // Outside the panel the background survives.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '.');
    }

    #[test]
    fn the_backdrop_scrim_patches_the_overlay_but_keeps_its_glyphs() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 6));
        background(&mut buf);
        let q = TextEdit::new();
        let r: [Line<'_>; 0] = [];
        CommandPalette::new(&q, &r)
            .width(Constraint::Length(2))
            .height(Constraint::Length(2))
            .backdrop_style(Style::new().bg(Color::Black))
            .render(buf.area(), &mut buf);
        let scrim = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(scrim.symbol, '.');
        assert_eq!(scrim.bg, Color::Black);
    }

    #[test]
    fn a_block_frames_the_panel() {
        let q = TextEdit::from_value("z");
        let r = [Line::raw("hit")];
        assert_eq!(
            lines(
                CommandPalette::new(&q, &r)
                    .block(Block::bordered())
                    .width(Constraint::Percentage(100))
                    .height(Constraint::Percentage(100)),
                7,
                4,
            ),
            "┌─────┐\n│> z  │\n│hit  │\n└─────┘\n"
        );
    }

    #[test]
    fn no_results_still_renders_the_query_row() {
        let q = TextEdit::from_value("q");
        let r: [Line<'_>; 0] = [];
        assert_eq!(
            lines(
                CommandPalette::new(&q, &r)
                    .width(Constraint::Percentage(100))
                    .height(Constraint::Percentage(100)),
                5,
                2,
            ),
            "> q  \n     \n"
        );
    }

    #[test]
    fn an_inner_too_short_for_results_draws_only_the_query() {
        let q = TextEdit::from_value("q");
        let r = [Line::raw("nope")];
        // One-row inner: only the query row fits; no panic.
        assert_eq!(
            lines(
                CommandPalette::new(&q, &r)
                    .width(Constraint::Percentage(100))
                    .height(Constraint::Length(1)),
                5,
                3,
            ),
            "     \n> q  \n     \n"
        );
    }

    #[test]
    fn a_zero_panel_is_only_the_scrim() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        background(&mut buf);
        let q = TextEdit::new();
        let r: [Line<'_>; 0] = [];
        CommandPalette::new(&q, &r)
            .width(Constraint::Length(0))
            .backdrop_style(Style::new().bg(Color::Black))
            .render(buf.area(), &mut buf);
        for p in buf.area().positions() {
            let c = buf.get(p).unwrap();
            assert_eq!(c.symbol, '.');
            assert_eq!(c.bg, Color::Black);
        }
    }

    #[test]
    fn zero_overlay_area_is_a_no_op() {
        let q = TextEdit::from_value("hello");
        let r = [Line::raw("a")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        CommandPalette::new(&q, &r).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
