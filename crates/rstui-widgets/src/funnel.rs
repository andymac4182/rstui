//! [`Funnel`] — a conversion funnel: vertically stacked, horizontally centred
//! bands whose width shrinks stage by stage, the dashboard primitive for "how
//! many made it through each step" (visitors → sign-ups → trials → paid, or
//! leads → qualified → demo → closed-won). Each band's width is proportional
//! to its value against the **top** stage, and each row carries the stage
//! label, its value, and (optionally) its conversion percentage of the first
//! stage so a drop-off jumps out.
//!
//! # A pure projection, like every other widget
//!
//! `Funnel` owns no state. It is a list of caller-built [`FunnelStage`]s (a
//! label [`Line`] plus a `u64` value); the reducer decides what the stages are
//! (it derives the counts from the model) and the widget only projects them.
//! That keeps it deterministically headless-testable and composes with the Elm
//! `view(&self)` model exactly like [`List`](crate::List) and
//! [`BarChart`](crate::BarChart).
//!
//! # Sub-cell precision, reusing the [`Gauge`](crate::Gauge) idea
//!
//! A band's edge rarely lands on a whole cell, so — exactly like
//! [`BarChart`](crate::BarChart) and [`Gauge`](crate::Gauge) — the boundary
//! cell is drawn with the *horizontal* eighth-block glyph (`▏…█`, the same
//! ramp the gauge fills its bar with) nearest the true fraction, not rounded
//! to a whole cell. Each glyph is one Unicode scalar, so it maps 1:1 onto a
//! [`Cell`](rstui_core::Buffer) with no grapheme machinery — the same
//! reasoning the gauge ramp and [`Block`] borders use. The ramp fills a cell
//! from the left, so each band is centred to a whole-cell left margin with its
//! one fractional boundary cell on the right edge (the
//! [`BarChart`](crate::BarChart) leading-edge precedent).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no stages, an all-zero series (no division by zero — every band
//! degenerates to width zero), a single stage, and an area too narrow/short
//! for the bands or text are all safe clips/no-ops — never a panic. An
//! optional framing [`Block`] follows the container-widget convention; a
//! reversed/horizontal funnel is a deliberately deferred additive follow-up,
//! not smuggled into this slice.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The eight left-aligned block elements, `1/8` … `8/8` wide (the same ramp
/// [`Gauge`](crate::Gauge) fills its bar with).
const HORIZONTAL_EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// One stage of a [`Funnel`]: a label [`Line`] and its `u64` value.
///
/// Build the label from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`); style it through the
/// [`Line`] it wraps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FunnelStage<'a> {
    label: Line<'a>,
    value: u64,
}

impl<'a> FunnelStage<'a> {
    /// A funnel stage of magnitude `value` labelled `label` (anything
    /// convertible to a [`Line`]).
    pub fn new(value: u64, label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            value,
        }
    }
}

/// A conversion funnel: vertically stacked, horizontally centred bands with
/// sub-cell precision and an optional framing [`Block`].
///
/// Stages are drawn top to bottom, one row each (the rows are split evenly
/// when there is height to spare). Each band's width is its value scaled
/// against the **first** stage's value (the widest band, full content width);
/// a band is centred with one fractional eighth-block boundary cell on its
/// right edge. Each row overlays the stage label and its value, plus — when
/// [`percent`](Self::percent) is set (the default) — its conversion
/// percentage of the first stage. Styling is a base [`Style`] (filling the
/// area) with a [`bar_style`](Self::bar_style) for the band glyphs and a
/// [`label_style`](Self::label_style) for the overlaid text, beneath each
/// label's own [`Line`]/[`Span`](rstui_core::Span) styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Funnel, FunnelStage};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 2));
/// Funnel::new([FunnelStage::new(8, ""), FunnelStage::new(4, "")])
///     .percent(false)
///     .render(buf.area(), &mut buf);
///
/// // The top stage spans the full width; the second is half as wide and
/// // centred, so its row starts with a blank margin.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '█');
/// assert_eq!(buf.get(Position::new(7, 0)).unwrap().symbol, '█');
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, ' ');
/// assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, '█');
/// ```
#[derive(Debug, Clone)]
pub struct Funnel<'a> {
    stages: Vec<FunnelStage<'a>>,
    percent: bool,
    block: Option<Block<'a>>,
    style: Style,
    bar_style: Style,
    label_style: Style,
}

impl Default for Funnel<'_> {
    fn default() -> Self {
        Self {
            stages: Vec::new(),
            percent: true,
            block: None,
            style: Style::default(),
            bar_style: Style::default(),
            label_style: Style::default(),
        }
    }
}

impl<'a> Funnel<'a> {
    /// A funnel of `stages` (the first stage is the widest band), with the
    /// conversion percentage shown and no frame.
    pub fn new<I>(stages: I) -> Self
    where
        I: IntoIterator<Item = FunnelStage<'a>>,
    {
        Self {
            stages: stages.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets whether each row shows its conversion percentage of the first
    /// stage (default `true`).
    #[must_use]
    pub fn percent(mut self, percent: bool) -> Self {
        self.percent = percent;
        self
    }

    /// Frames the funnel in `block`; bands render into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a background
    /// covers the whole pane.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] the band glyphs are drawn with, over the base.
    #[must_use]
    pub fn bar_style(mut self, style: Style) -> Self {
        self.bar_style = style;
        self
    }

    /// Sets the base [`Style`] for the overlaid label/value text, beneath each
    /// label's own [`Line`]/[`Span`](rstui_core::Span) styles.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }
}

/// The number of eighths a `value` fills of a `cells`-wide axis against
/// `top` (rounded to the nearest eighth; `top` is already `>= 1`). A `value`
/// above `top` is clamped so the result never exceeds `cells * 8`.
fn eighths(value: u64, top: u64, cells: u16) -> u64 {
    let clamped = u128::from(value.min(top));
    let total = u128::from(cells) * 8;
    ((clamped * total + u128::from(top) / 2) / u128::from(top)) as u64
}

/// Stamps `line` left-to-right from `x0` on row `y`, clipped at `right`, with
/// `base` beneath the line→span cascade. A space advances without stamping so
/// the band shows through the gap (the [`Badge`](crate::Badge) "paint only
/// your own glyphs" reasoning) rather than being blanked.
fn stamp_line(buf: &mut Buffer, line: &Line, base: Style, x0: u16, y: u16, right: u16) {
    let line_base = base.patch(line.style);
    let mut x = x0;
    'line: for span in &line.spans {
        let style = line_base.patch(span.style);
        for ch in span.content.chars() {
            if x >= right {
                break 'line;
            }
            if ch != ' ' {
                buf.set_cell(Position::new(x, y), ch, style);
            }
            x = x.saturating_add(1);
        }
    }
}

/// Stamps a plain `text` run left-to-right from `x0` on row `y`, clipped at
/// `right`, in `style`; a space advances without stamping so the band shows
/// through (same reasoning as [`stamp_line`]).
fn stamp_overlay(buf: &mut Buffer, text: &str, style: Style, x0: u16, y: u16, right: u16) {
    let mut x = x0;
    for ch in text.chars() {
        if x >= right {
            break;
        }
        if ch != ' ' {
            buf.set_cell(Position::new(x, y), ch, style);
        }
        x = x.saturating_add(1);
    }
}

impl Widget for Funnel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Funnel {
            stages,
            percent,
            block,
            style,
            bar_style,
            label_style,
        } = self;

        // The block (if any) frames the content and reserves the inner area.
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

        // Base fills the content area so a background covers the whole pane.
        buf.set_style(inner, style);
        if stages.is_empty() {
            return;
        }

        // The first stage is the reference (the widest band). Floored at 1 so
        // an all-zero funnel divides safely and degenerates to zero-width
        // bands (no panic — the Gauge totality rule).
        let top = stages.first().map_or(1, |s| s.value).max(1);
        let bar_glyph = style.patch(bar_style);
        let text_base = style.patch(label_style);

        // Rows are split as evenly as possible across the available height so
        // a tall area spreads the bands out instead of clumping at the top.
        let n = stages.len() as u16;
        let total_h = inner.height;
        let base_h = total_h / n;
        let extra = (total_h % n) as usize;
        let left = inner.left();
        let width = inner.width;
        let mut y = inner.top();
        let bottom = inner.bottom();

        for (i, stage) in stages.iter().enumerate() {
            if y >= bottom {
                break;
            }
            // The first `extra` rows get one spare cell each so the bands
            // stack with no blank seams.
            let row_h = base_h + u16::from(i < extra);
            if row_h == 0 {
                continue;
            }
            let group_bottom = y.saturating_add(row_h).min(bottom);
            // The text overlays the group's middle row so a 1-tall group
            // still reads.
            let band_y = y + row_h / 2;

            let e = eighths(stage.value, top, width);
            let full = (e / 8) as u16;
            let rem = (e % 8) as u16;
            // Total cells the band touches (full run + a fractional cell).
            let touched = full + u16::from(rem > 0 && full < width);
            // Centre the band: the left margin is whole-cell (the ramp fills
            // from the left, so the fractional cell is the band's right edge).
            let margin = (width.saturating_sub(touched)) / 2;
            let band_x0 = left.saturating_add(margin);
            let band_right = left.saturating_add(width);

            // The band fills every row of its group so consecutive stages
            // stack into the funnel silhouette.
            for by in y..group_bottom {
                for c in 0..full {
                    let x = band_x0.saturating_add(c);
                    if x >= band_right {
                        break;
                    }
                    buf.set_cell(Position::new(x, by), '█', bar_glyph);
                }
                if rem > 0 && full < width {
                    let x = band_x0.saturating_add(full);
                    if x < band_right {
                        buf.set_cell(
                            Position::new(x, by),
                            HORIZONTAL_EIGHTHS[(rem - 1) as usize],
                            bar_glyph,
                        );
                    }
                }
            }

            // Overlay the label, then the value (and percentage) after it,
            // centred over the band so the row reads even when the band is
            // narrow. The text layers on top of the band glyphs; space cells
            // are *not* stamped, so the band shows through the gaps instead
            // of being blanked out (the [`Badge`](crate::Badge) "paint only
            // your own glyphs" reasoning).
            let pct = if percent {
                format!(" {}%", (stage.value.saturating_mul(100)) / top)
            } else {
                String::new()
            };
            let value = format!("{}{pct}", stage.value);
            let label_w = stage.label.width() as u16;
            // One separator column between a non-empty label and the value.
            let gap = u16::from(label_w > 0);
            let text_w = label_w + gap + value.chars().count() as u16;
            let tx = left.saturating_add(width.saturating_sub(text_w.min(width)) / 2);
            stamp_line(buf, &stage.label, text_base, tx, band_y, band_right);
            let value_x = tx.saturating_add(label_w).saturating_add(gap);
            stamp_overlay(buf, &value, text_base, value_x, band_y, band_right);

            y = y.saturating_add(row_h);
        }
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

    /// Renders `widget` into a fresh `width`×`height` buffer and hands the
    /// buffer back for per-cell assertions (the value text overlays the
    /// band, so cell checks read more clearly than a clobbered row string).
    fn render_buf<W: Widget>(widget: W, width: u16, height: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        buf
    }

    fn sym(buf: &Buffer, x: u16, y: u16) -> char {
        buf.get(Position::new(x, y)).unwrap().symbol
    }

    /// `true` if `c` is a band glyph (a full or fractional horizontal block).
    fn is_band(c: char) -> bool {
        c == '█' || HORIZONTAL_EIGHTHS.contains(&c)
    }

    #[test]
    fn each_band_narrows_against_the_first_stage() {
        // top=8 → the first band is full width 8; the second (value 4) is 4
        // cells, centred (margin (8-4)/2 = 2). The value text overlays the
        // band centred, spaces showing the band through.
        let buf = render_buf(
            Funnel::new([FunnelStage::new(8, ""), FunnelStage::new(4, "")]).percent(false),
            8,
            2,
        );
        // Row 0: full band across all eight columns (the '8' overlays x=3).
        for x in 0..8 {
            assert!(is_band(sym(&buf, x, 0)) || sym(&buf, x, 0) == '8');
        }
        // Row 1: a 4-wide band centred in columns 2..6, blank margins.
        assert_eq!(sym(&buf, 0, 1), ' ');
        assert_eq!(sym(&buf, 1, 1), ' ');
        assert!(is_band(sym(&buf, 2, 1)));
        assert!(is_band(sym(&buf, 5, 1)));
        assert_eq!(sym(&buf, 6, 1), ' ');
        assert_eq!(sym(&buf, 7, 1), ' ');
    }

    #[test]
    fn a_fractional_band_uses_a_sub_cell_right_edge() {
        // width 4, top=3. Stage 1 (=top) is full width. Stage 2 value 1 →
        // 1/3 of 4 cells = 1.33 → one full cell + a 3/8 right edge (`▍`),
        // centred (margin (4-2)/2 = 1). The value '1' overlays the full
        // cell, leaving the fractional `▍` cell visible at x=2.
        let buf = render_buf(
            Funnel::new([FunnelStage::new(3, ""), FunnelStage::new(1, "")]).percent(false),
            4,
            2,
        );
        assert_eq!(sym(&buf, 2, 1), '▍');
        assert_eq!(sym(&buf, 0, 1), ' '); // left margin is blank
        assert_eq!(sym(&buf, 3, 1), ' '); // past the fractional edge
    }

    #[test]
    fn an_all_zero_funnel_degenerates_to_no_bands() {
        // The reference floors to 1, but every value is 0 → 0 eighths → no
        // band glyph anywhere; only the '0' value text shows.
        let out = lines(
            Funnel::new([FunnelStage::new(0, ""), FunnelStage::new(0, "")]).percent(false),
            4,
            2,
        );
        assert!(!out.chars().any(is_band), "no band glyph: {out:?}");
        assert!(out.contains('0'));
    }

    #[test]
    fn a_single_stage_spans_the_full_width() {
        // value 42 = top → the band fills the whole width; '42' overlays it.
        let buf = render_buf(Funnel::new([FunnelStage::new(42, "")]).percent(false), 4, 1);
        assert_eq!(sym(&buf, 0, 0), '█');
        assert_eq!(sym(&buf, 3, 0), '█');
        assert_eq!(sym(&buf, 1, 0), '4');
        assert_eq!(sym(&buf, 2, 0), '2');
    }

    #[test]
    fn the_first_stage_is_always_full_even_if_not_the_largest() {
        // The reference is the *first* stage, not the max; a later larger
        // value clamps to the full width (never a panic).
        let buf = render_buf(
            Funnel::new([FunnelStage::new(4, ""), FunnelStage::new(99, "")]).percent(false),
            4,
            2,
        );
        for x in 0..4 {
            assert!(is_band(sym(&buf, x, 0)) || sym(&buf, x, 0).is_ascii_digit());
            assert!(is_band(sym(&buf, x, 1)) || sym(&buf, x, 1).is_ascii_digit());
        }
        // The clamped second stage is still full width (its edges are band).
        assert_eq!(sym(&buf, 0, 1), '█');
        assert_eq!(sym(&buf, 3, 1), '█');
    }

    #[test]
    fn the_label_value_and_percentage_overlay_the_band() {
        // Default percent=true. top=100. Stage A: label "A", value "100",
        // pct " 100%" → text "A 100 100%" centred (text_w=10, tx=1).
        let buf = render_buf(
            Funnel::new([FunnelStage::new(100, "A"), FunnelStage::new(50, "B")]),
            12,
            2,
        );
        assert_eq!(sym(&buf, 1, 0), 'A');
        assert_eq!(sym(&buf, 3, 0), '1');
        assert_eq!(sym(&buf, 4, 0), '0');
        assert_eq!(sym(&buf, 5, 0), '0');
        assert_eq!(sym(&buf, 10, 0), '%');
        // Stage B: "B", "50", " 50%" → "B 50 50%" centred (text_w=8, tx=2).
        assert_eq!(sym(&buf, 2, 1), 'B');
        assert_eq!(sym(&buf, 4, 1), '5');
        assert_eq!(sym(&buf, 5, 1), '0');
        assert_eq!(sym(&buf, 7, 1), '5');
        assert_eq!(sym(&buf, 9, 1), '%');
    }

    #[test]
    fn percent_false_drops_the_percentage() {
        let out = lines(
            Funnel::new([FunnelStage::new(100, "A")]).percent(false),
            12,
            1,
        );
        assert!(out.contains('A'));
        assert!(out.contains('1') && out.contains('0'));
        assert!(!out.contains('%'));
    }

    #[test]
    fn rows_spread_across_a_tall_area() {
        // 2 stages, 4 rows → 2 rows each; each band fills its 2-row group so
        // they stack into the funnel silhouette (the value overlays the
        // group's middle row, x=0).
        let buf = render_buf(
            Funnel::new([FunnelStage::new(8, ""), FunnelStage::new(8, "")]).percent(false),
            2,
            4,
        );
        for y in 0..4 {
            // Column 1 is always band (the value '8' only ever lands at x=0).
            assert_eq!(sym(&buf, 1, y), '█', "row {y} col 1");
        }
        assert_eq!(sym(&buf, 0, 1), '8'); // first band's value, middle row
        assert_eq!(sym(&buf, 0, 3), '8'); // second band's value, middle row
    }

    #[test]
    fn a_block_frames_the_funnel_in_the_inner_area() {
        // 5×3 → inner 3×1; the full band fills it, '8' overlays the centre.
        let f = Funnel::new([FunnelStage::new(8, "")])
            .percent(false)
            .block(Block::bordered());
        assert_eq!(lines(f, 5, 3), "┌───┐\n│█8█│\n└───┘\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_bands() {
        let f = Funnel::new([FunnelStage::new(8, "x")]).block(Block::bordered());
        assert_eq!(lines(f, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn no_stages_with_a_block_still_renders_the_block() {
        let f = Funnel::new(Vec::<FunnelStage>::new()).block(Block::bordered());
        assert_eq!(lines(f, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn style_cascades_base_then_bar_and_label_styles() {
        let f = Funnel::new([FunnelStage::new(8, "L")])
            .percent(false)
            .style(Style::new().bg(Color::Blue))
            .bar_style(Style::new().fg(Color::Green))
            .label_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        f.render(buf.area(), &mut buf);

        // A band cell not under the text keeps the bar fg over the base bg.
        let band = buf.get(Position::new(3, 0)).unwrap();
        assert_eq!(band.symbol, '█');
        assert_eq!(band.fg, Color::Green);
        assert_eq!(band.bg, Color::Blue);

        // The label glyph cascades base + label_style over the band.
        let l = buf
            .cells()
            .iter()
            .find(|c| c.symbol == 'L')
            .expect("the label is drawn");
        assert!(l.modifier.contains(Modifier::BOLD));
        assert_eq!(l.bg, Color::Blue);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Funnel::new([FunnelStage::new(5, "x")]).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn a_tiny_area_clips_without_panicking() {
        // More stages than rows → the tail clips; must not panic.
        let f = Funnel::new([
            FunnelStage::new(9, "a"),
            FunnelStage::new(5, "b"),
            FunnelStage::new(1, "c"),
        ]);
        let _ = lines(f, 1, 1);
        let f = Funnel::new([FunnelStage::new(4, "a")]).block(Block::bordered());
        let _ = lines(f, 1, 1);
    }
}
