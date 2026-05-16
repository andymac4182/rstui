//! [`Gauge`] — a horizontal progress bar, the basis for download/task
//! progress, capacity meters, and loading indicators.
//!
//! # The first sub-cell-precision widget
//!
//! Every widget before this one stamped whole glyphs into whole cells.
//! `Gauge` is the first to render at *sub-cell* resolution: the boundary
//! between the filled bar and the empty track lands partway through a column,
//! so that one column is drawn with a partial left-block glyph
//! (`▏▎▍▌▋▊▉█`, eight eighths) instead of being rounded to a whole cell. A
//! width-`w` bar therefore has `8 · w` distinguishable positions, not `w`.
//!
//! This is a clean fit for rstui's single-`char` [`Cell`](rstui_core::Buffer)
//! model for exactly the reason borders were (see `Block`): every block
//! element is a single Unicode scalar, so the ramp is a `[char; 8]` table and
//! needs no `&str`/grapheme machinery. ratatui gates this behind a
//! `use_unicode` flag for legacy terminals that lack the glyphs; rstui always
//! renders at full precision and treats "this terminal can't draw `▌`" as a
//! future *backend-capability* concern, not a per-widget toggle — the same
//! "defer the rare case to the right layer, don't stub the API" stance the
//! core text model took on graphemes.
//!
//! # Clamp, don't panic — a pure-projection divergence
//!
//! ratatui's `Gauge::ratio`/`percent` **panic** on an out-of-range value.
//! rstui treats a gauge as a pure projection of a caller-owned progress
//! number, exactly as [`List`](crate::List) treats `selected`: an
//! out-of-range value renders something sensible (a full or empty bar), it
//! never aborts the program. A ratio computed as `1.0000001` from float math,
//! or a transient negative, must not take down a whole TUI — so the setters
//! clamp to `0.0..=1.0` (and map `NaN` to `0.0`) and the stored value is
//! always valid. That keeps the widget total and headless-testable, and is
//! the deliberate, documented divergence from ratatui here.

use rstui_core::{Buffer, Color, Position, Rect, Span, Style, Widget};

use crate::block::Block;

/// The eight left-aligned block elements, `1/8` … `8/8` filled.
///
/// `EIGHTHS[n - 1]` is the glyph for `n` eighths; `EIGHTHS[7]` is the full
/// block, the same glyph the whole-cell filled run uses. Zero eighths draws
/// nothing (the cell stays the track).
const EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// A horizontal progress bar with an optional centered label.
///
/// The bar fills `ratio` of the content width left to right. The filled run is
/// drawn with full blocks; the single boundary column is drawn with the
/// partial `EIGHTHS` glyph nearest the true sub-cell fraction (so a 37%
/// gauge looks like 37%, not "rounded to whole columns"). The unfilled track
/// is the remaining width.
///
/// The label (defaulting to the rounded percentage, e.g. `42%`) is centered
/// horizontally and vertically. Where it crosses the filled bar its
/// foreground/background are swapped against [`gauge_style`](Self::gauge_style)
/// so the text stays readable over the solid bar; over the track it keeps the
/// gauge style. A caller-supplied [`label`](Self::label)'s own
/// [`Style`] is patched last, so it can override either.
///
/// Styling: the base [`style`](Self::style) fills the whole widget area
/// (frame and background, like every other widget); [`gauge_style`](Self::gauge_style)
/// then covers the inner bar+track, its `fg` painting the bar glyphs and its
/// `bg` the track behind them. The [`ratio`](Self::ratio) and label are
/// ordinary caller-owned values the widget only reads — never mutated at
/// render time — so `Gauge` composes with the Elm `view(&self)` model just
/// like [`List`](crate::List) and [`Tabs`](crate::Tabs).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Gauge;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
/// Gauge::default().ratio(0.5).render(buf.area(), &mut buf);
///
/// // 50% of 10 columns = 5 full blocks (cols 0..5), with the centred "50%"
/// // label overlaying cols 3..6 — so it reads as `███50%    `.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '█');
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, '█');
/// assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, '5');
/// assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, '%');
/// assert_eq!(buf.get(Position::new(9, 0)).unwrap().symbol, ' ');
/// ```
#[derive(Debug, Default, Clone)]
pub struct Gauge<'a> {
    block: Option<Block<'a>>,
    ratio: f64,
    label: Option<Span<'a>>,
    style: Style,
    gauge_style: Style,
}

impl<'a> Gauge<'a> {
    /// Frames the gauge in `block`; the bar renders into [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the fill from a ratio (`0.75` is three-quarters full).
    ///
    /// The value is **clamped** to `0.0..=1.0` and `NaN` becomes `0.0`; this
    /// never panics (the deliberate divergence from ratatui — see the [module
    /// docs](self)).
    #[must_use]
    pub fn ratio(mut self, ratio: f64) -> Self {
        self.ratio = if ratio.is_nan() {
            0.0
        } else {
            ratio.clamp(0.0, 1.0)
        };
        self
    }

    /// Sets the fill from a whole percentage (`0..=100`).
    ///
    /// Values above `100` are clamped, not rejected (see [`ratio`](Self::ratio)).
    #[must_use]
    pub fn percent(mut self, percent: u16) -> Self {
        self.ratio = f64::from(percent.min(100)) / 100.0;
        self
    }

    /// Sets the centered label. Defaults to the rounded percentage (`42%`).
    ///
    /// An empty label (`""`) suppresses it, leaving a bare bar.
    #[must_use]
    pub fn label(mut self, label: impl Into<Span<'a>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the base [`Style`], filling the whole widget area (frame and
    /// background) beneath the bar — the [`block`](Self::block) included.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the bar [`Style`]: `fg` paints the filled glyphs, `bg` the track
    /// behind them. Unset colors inherit the base [`style`](Self::style).
    #[must_use]
    pub fn gauge_style(mut self, style: Style) -> Self {
        self.gauge_style = style;
        self
    }
}

impl Widget for Gauge<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Gauge {
            block,
            ratio,
            label,
            style,
            gauge_style,
        } = self;

        // Base style covers the whole area (frame + background), like every
        // widget; the block (if any) frames the bar and reserves the inner.
        buf.set_style(area, style);
        let inner = match &block {
            Some(b) => b.inner(area),
            None => area,
        };
        if let Some(b) = block {
            b.render(area, buf);
        }
        if inner.is_empty() {
            return;
        }

        // The track: gauge_style over the whole inner area, so the unfilled
        // part reads as gauge_style.bg with the bar glyphs layered on top.
        buf.set_style(inner, gauge_style);

        // Whole filled columns, plus the one boundary column drawn with the
        // partial glyph nearest the true sub-cell fraction.
        let filled = f64::from(inner.width) * ratio;
        let full = filled.floor() as u16;
        let eighths = ((filled - f64::from(full)) * 8.0).round() as u16;
        let left = inner.left();
        let right = inner.right();

        for y in inner.top()..inner.bottom() {
            for i in 0..full {
                buf.set_cell(Position::new(left + i, y), '█', gauge_style);
            }
            if eighths > 0 && full < inner.width {
                let glyph = EIGHTHS[(eighths - 1) as usize];
                buf.set_cell(Position::new(left + full, y), glyph, gauge_style);
            }
        }

        // The label: the rounded percentage unless the caller set one, centred
        // both ways. Where it crosses the filled run its fg/bg are swapped so
        // the text stays readable over the solid bar; over the track it keeps
        // the gauge style. A caller label's own style is patched last.
        let label = label.unwrap_or_else(|| Span::raw(format!("{}%", (ratio * 100.0).round())));
        let label_width = inner.width.min(label.width() as u16);
        if label_width == 0 {
            return;
        }
        let label_col = left + (inner.width - label_width) / 2;
        let label_row = inner.top() + inner.height / 2;
        let fill_end = left + full;
        let swapped = Style::new()
            .fg(gauge_style.bg.unwrap_or(Color::Reset))
            .bg(gauge_style.fg.unwrap_or(Color::Reset));

        for (i, ch) in label.content.chars().take(label_width as usize).enumerate() {
            let x = label_col.saturating_add(i as u16);
            if x >= right {
                break;
            }
            let base = if x < fill_end { swapped } else { gauge_style };
            buf.set_cell(Position::new(x, label_row), ch, base.patch(label.style));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Modifier;

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

    #[test]
    fn default_is_an_empty_track_labelled_zero_percent() {
        // ratio 0 ⇒ no bar; "0%" (width 2) centred in 6 ⇒ col (6-2)/2 = 2.
        assert_eq!(lines(Gauge::default(), 6, 1), "  0%  \n");
    }

    #[test]
    fn a_whole_ratio_fills_whole_columns_with_the_full_block() {
        // 4·0.5 = 2.0 ⇒ two full blocks, no partial. The label is suppressed;
        // the bar is drawn on every row of the inner area.
        assert_eq!(
            lines(Gauge::default().ratio(0.5).label(""), 4, 2),
            "██  \n██  \n"
        );
    }

    #[test]
    fn a_full_ratio_fills_every_column() {
        assert_eq!(
            lines(Gauge::default().ratio(1.0).label(""), 5, 1),
            "█████\n"
        );
    }

    #[test]
    fn a_fractional_ratio_uses_a_sub_cell_block_glyph() {
        // 1·0.5 = 0.5 ⇒ 0 full + round(0.5·8)=4 eighths ⇒ left half block.
        assert_eq!(lines(Gauge::default().ratio(0.5).label(""), 1, 1), "▌\n");
        // 2·0.75 = 1.5 ⇒ 1 full + 4 eighths ⇒ "█▌".
        assert_eq!(lines(Gauge::default().ratio(0.75).label(""), 2, 1), "█▌\n");
    }

    #[test]
    fn the_eighth_ramp_maps_each_fraction_to_its_block() {
        // One column wide, so the whole bar is the single boundary glyph.
        for (ratio, glyph) in [
            (0.125, '▏'), // 1/8
            (0.250, '▎'), // 2/8
            (0.375, '▍'), // 3/8
            (0.500, '▌'), // 4/8
            (0.625, '▋'), // 5/8
            (0.750, '▊'), // 6/8
            (0.875, '▉'), // 7/8
        ] {
            assert_eq!(
                lines(Gauge::default().ratio(ratio).label(""), 1, 1),
                format!("{glyph}\n"),
                "ratio {ratio}"
            );
        }
    }

    #[test]
    fn ratio_is_clamped_and_never_panics() {
        // Above 1, below 0, and NaN all render a sensible bar instead of
        // aborting (the deliberate divergence from ratatui).
        assert_eq!(lines(Gauge::default().ratio(2.0).label(""), 3, 1), "███\n");
        assert_eq!(lines(Gauge::default().ratio(-0.5).label(""), 3, 1), "   \n");
        assert_eq!(
            lines(Gauge::default().ratio(f64::NAN).label(""), 3, 1),
            "   \n"
        );
    }

    #[test]
    fn percent_sets_the_ratio_and_is_clamped() {
        assert_eq!(
            lines(Gauge::default().percent(50).label(""), 4, 1),
            "██  \n"
        );
        assert_eq!(
            lines(Gauge::default().percent(250).label(""), 3, 1),
            "███\n"
        );
    }

    #[test]
    fn the_default_label_is_the_rounded_percentage_over_the_bar() {
        // 10·0.5 = 5 full blocks; "50%" centred at col (10-3)/2 = 3. '5','0'
        // land on filled columns (x<5); '%' lands on the track (x==5).
        assert_eq!(lines(Gauge::default().ratio(0.5), 10, 1), "███50%    \n");
    }

    #[test]
    fn the_label_is_centred_horizontally_and_vertically() {
        // ratio 0 ⇒ bare track; "Hi" centred in 9×3 ⇒ col 3, row 1.
        assert_eq!(
            lines(Gauge::default().ratio(0.0).label("Hi"), 9, 3),
            "         \n   Hi    \n         \n"
        );
    }

    #[test]
    fn the_label_over_the_filled_bar_swaps_colours_for_readability() {
        let g = Gauge::default()
            .ratio(1.0)
            .label("AB")
            .gauge_style(Style::new().fg(Color::Green).bg(Color::Black));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        g.render(buf.area(), &mut buf);

        // A filled, non-label column: bar glyph in gauge fg over gauge bg.
        let bar = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(bar.symbol, '█');
        assert_eq!(bar.fg, Color::Green);
        assert_eq!(bar.bg, Color::Black);

        // The label sits over the filled bar (col (4-2)/2 = 1): swapped so it
        // reads as the background colour on the bar colour.
        let lbl = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(lbl.symbol, 'A');
        assert_eq!(lbl.fg, Color::Black);
        assert_eq!(lbl.bg, Color::Green);
    }

    #[test]
    fn the_label_over_the_track_keeps_the_gauge_style() {
        let g = Gauge::default()
            .ratio(0.0)
            .label("Hi")
            .gauge_style(Style::new().fg(Color::Green).bg(Color::Black));
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        g.render(buf.area(), &mut buf);
        // Over the empty track the label keeps the gauge style (not swapped).
        let h = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(h.symbol, 'H');
        assert_eq!(h.fg, Color::Green);
        assert_eq!(h.bg, Color::Black);
    }

    #[test]
    fn a_caller_label_style_is_patched_last_over_either_side() {
        let g = Gauge::default()
            .ratio(1.0)
            .label(Span::styled("X", Style::new().fg(Color::Red)))
            .gauge_style(Style::new().fg(Color::Green).bg(Color::Black));
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        g.render(buf.area(), &mut buf);
        let c = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(c.symbol, 'X');
        assert_eq!(c.fg, Color::Red); // label fg wins over the swap
        assert_eq!(c.bg, Color::Green); // swap bg (gauge fg) still shows
    }

    #[test]
    fn the_base_style_fills_the_whole_area() {
        let g = Gauge::default()
            .ratio(0.0)
            .label("")
            .style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        g.render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
    }

    #[test]
    fn the_gauge_style_colours_the_bar_and_the_track() {
        let g = Gauge::default()
            .ratio(0.5)
            .label("")
            .gauge_style(Style::new().fg(Color::Green).bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        g.render(buf.area(), &mut buf);
        let bar = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(bar.symbol, '█');
        assert_eq!(bar.fg, Color::Green);
        assert_eq!(bar.bg, Color::Blue);
        // The track keeps gauge_style.bg even with no glyph.
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().bg, Color::Blue);
    }

    #[test]
    fn a_block_frames_the_bar_in_the_inner_area() {
        assert_eq!(
            lines(
                Gauge::default()
                    .ratio(1.0)
                    .label("")
                    .block(Block::bordered()),
                5,
                3
            ),
            "┌───┐\n│███│\n└───┘\n"
        );
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_bar() {
        assert_eq!(
            lines(Gauge::default().ratio(1.0).block(Block::bordered()), 2, 2),
            "┌┐\n└┘\n"
        );
    }

    #[test]
    fn a_label_carries_modifiers_through_the_patch() {
        let g = Gauge::default()
            .ratio(0.0)
            .label(Span::styled("Z", Style::new().add_modifier(Modifier::BOLD)));
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        g.render(buf.area(), &mut buf);
        assert!(
            buf.get(Position::new(0, 0))
                .unwrap()
                .modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Gauge::default()
            .ratio(0.5)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
