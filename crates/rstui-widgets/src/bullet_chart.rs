//! [`BulletChart`] — Stephen Few's bullet graph, the compact KPI strip a
//! dashboard stacks ("revenue vs. plan", "SLA vs. target", "p99 vs. budget"):
//! a measure bar over shaded qualitative range bands with a single target
//! tick.
//!
//! # A pure projection, like every other widget
//!
//! `BulletChart` owns no state. It is a list of caller-built [`Bullet`]s (a
//! label [`Line`], a `value`, a `target`, and ascending qualitative
//! thresholds) and an optional ceiling; the reducer decides what those numbers
//! are and the widget only projects them. That keeps it deterministically
//! headless-testable and composes with the Elm `view(&self)` model exactly like
//! [`BarChart`](crate::BarChart) and [`Gauge`](crate::Gauge).
//!
//! # Sub-cell precision, reusing the [`BarChart`](crate::BarChart) idea
//!
//! A bullet *is* a bar plus context, so the measure bar reuses the
//! [`BarChart`](crate::BarChart)/[`Gauge`](crate::Gauge) eighth-block ramp:
//! the bar's end rarely lands on a whole cell, so the boundary cell is the
//! eighth-block glyph nearest the true fraction (the horizontal ramp `▏…█`
//! horizontally, the vertical ramp `▁…█` vertically), not rounded to a whole
//! cell. The qualitative range bands behind it are drawn with progressively
//! lighter shade glyphs (`░▒▓`), and the target is one contrasting tick
//! (`│`/`─`). Each glyph is a single Unicode scalar, so it maps 1:1 onto a
//! [`Cell`](rstui_core::Buffer) with no grapheme machinery — the same reasoning
//! [`Block`] borders and the gauge ramp use.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no bullets, empty ranges, a value or target above the ceiling
//! (clamped), and an area too narrow/short for the label or bar are all safe
//! clips/no-ops — never a panic. An optional framing [`Block`] follows the
//! container-widget convention; per-bullet value text and comparative markers
//! are deliberately deferred additive follow-ups, not smuggled into this slice.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The eight bottom-aligned block elements for a **vertical** measure bar,
/// `1/8` … `8/8`.
const VERTICAL_EIGHTHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The eight left-aligned block elements for a **horizontal** measure bar,
/// `1/8` … `8/8` (the same ramp [`Gauge`](crate::Gauge) fills its bar with).
const HORIZONTAL_EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// The qualitative-band shade ramp, lightest first. A band darker than the
/// last entry reuses the darkest glyph (`▓`); the bands get *lighter* outward
/// per Few's "good/satisfactory/poor get progressively lighter" convention.
const SHADES: [char; 3] = ['▓', '▒', '░'];

/// Which way a [`BulletChart`]'s bullets — and each measure bar — grow.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BulletChartDirection {
    /// Bullets are rows stacked top to bottom; each measure bar grows
    /// **rightward** with its label in a reserved left column (the default).
    #[default]
    Horizontal,
    /// Bullets are columns placed left to right; each measure bar grows
    /// **upward** with its label on the bottom row.
    Vertical,
}

/// One KPI of a [`BulletChart`]: a label [`Line`], the measured `value`, a
/// comparative `target`, and the ascending qualitative-band thresholds.
///
/// Build the label from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`); style it through the
/// [`Line`] it wraps. `ranges` are *upper* thresholds in ascending order
/// (e.g. `[60, 80, 100]` → a poor band to 60, satisfactory to 80, good to
/// 100); an empty `ranges` is a bare bar with no qualitative context.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Bullet<'a> {
    /// The KPI label, drawn beside (horizontal) or beneath (vertical) the bar.
    label: Line<'a>,
    /// The measured value the bar's length encodes.
    value: u64,
    /// The comparative target the tick marks.
    target: u64,
    /// Ascending qualitative-band upper thresholds (empty → no bands).
    ranges: Vec<u64>,
}

impl<'a> Bullet<'a> {
    /// A bullet measuring `value` against `target` over the qualitative
    /// `ranges` (ascending upper thresholds), labelled `label`.
    pub fn new(value: u64, target: u64, ranges: Vec<u64>, label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            value,
            target,
            ranges,
        }
    }
}

/// A stack (or row) of bullet graphs with sub-cell measure bars, shaded
/// qualitative bands, target ticks, and an optional framing [`Block`].
///
/// Each [`Bullet`] is a [`max`](Self::max)-scaled (the largest of every value,
/// target and threshold when unset) track: the qualitative bands fill the
/// track back-to-front with the progressively lighter `░▒▓` ramp, the measure
/// bar is drawn over them with full blocks plus one fractional eighth-block
/// boundary cell, and the target is one contrasting tick glyph. Styling is a
/// base [`Style`] (filling the area) with a [`bar_style`](Self::bar_style) for
/// the measure glyphs, a [`target_style`](Self::target_style) for the tick, and
/// a [`label_style`](Self::label_style) beneath each label's own
/// [`Line`]/[`Span`](rstui_core::Span) styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Bullet, BulletChart};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
/// BulletChart::new([Bullet::new(8, 6, vec![5, 10], "x")])
///     .max(Some(10))
///     .render(buf.area(), &mut buf);
///
/// // A 1-char label column, then the shaded track with the measure bar and a
/// // target tick over it — never a panic, the totality rule.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'x');
/// ```
#[derive(Debug, Default, Clone)]
pub struct BulletChart<'a> {
    bullets: Vec<Bullet<'a>>,
    direction: BulletChartDirection,
    max: Option<u64>,
    block: Option<Block<'a>>,
    style: Style,
    bar_style: Style,
    target_style: Style,
    label_style: Style,
}

impl<'a> BulletChart<'a> {
    /// A horizontal chart of `bullets`, auto-scaled to the largest of every
    /// value/target/threshold, with no frame.
    #[must_use]
    pub fn new<I>(bullets: I) -> Self
    where
        I: IntoIterator<Item = Bullet<'a>>,
    {
        Self {
            bullets: bullets.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets the value mapped to a full-length track, or `None` to auto-scale
    /// to the largest value/target/threshold.
    ///
    /// A value or target above the ceiling is clamped (never a panic — the
    /// [`Gauge`](crate::Gauge) totality rule).
    #[must_use]
    pub fn max(mut self, max: Option<u64>) -> Self {
        self.max = max;
        self
    }

    /// Sets whether bullets stack as rows growing right (horizontal) or sit as
    /// columns growing up (vertical).
    #[must_use]
    pub fn direction(mut self, direction: BulletChartDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Frames the chart in `block`; bullets render into [`block.inner`](Block::inner).
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

    /// Sets the [`Style`] the measure-bar glyphs (and the bands beneath them)
    /// are drawn with, over the base.
    #[must_use]
    pub fn bar_style(mut self, style: Style) -> Self {
        self.bar_style = style;
        self
    }

    /// Sets the [`Style`] the target tick glyph is drawn with, over the base.
    #[must_use]
    pub fn target_style(mut self, style: Style) -> Self {
        self.target_style = style;
        self
    }

    /// Sets the base [`Style`] for labels, beneath each label's own
    /// [`Line`]/[`Span`](rstui_core::Span) styles.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }
}

/// The number of eighths a `value` fills of a `span`-cell axis against
/// `ceiling` (rounded to the nearest eighth; `ceiling` is already `>= 1`).
fn eighths(value: u64, ceiling: u64, span: u16) -> u64 {
    let clamped = u128::from(value.min(ceiling));
    let total = u128::from(span) * 8;
    ((clamped * total + u128::from(ceiling) / 2) / u128::from(ceiling)) as u64
}

/// The whole-cell length a `value` maps to of a `span`-cell axis against
/// `ceiling` (rounded; used for band fills and the target column).
fn cells(value: u64, ceiling: u64, span: u16) -> u16 {
    let clamped = u128::from(value.min(ceiling));
    let total = u128::from(span);
    ((clamped * total + u128::from(ceiling) / 2) / u128::from(ceiling)) as u16
}

/// Stamps `line` left-to-right from `x0` on row `y`, clipped at `right`, with
/// `base` beneath the line→span cascade.
fn stamp_line(buf: &mut Buffer, line: &Line, base: Style, x0: u16, y: u16, right: u16) {
    let line_base = base.patch(line.style);
    let mut x = x0;
    'line: for span in &line.spans {
        let style = line_base.patch(span.style);
        for ch in span.content.chars() {
            if x >= right {
                break 'line;
            }
            buf.set_cell(Position::new(x, y), ch, style);
            x = x.saturating_add(1);
        }
    }
}

/// The shade glyph for band index `i` (`0` darkest/innermost, lighter
/// outward); an index past the ramp reuses the lightest entry.
fn shade(i: usize) -> char {
    SHADES[i.min(SHADES.len() - 1)]
}

impl Widget for BulletChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let BulletChart {
            bullets,
            direction,
            max,
            block,
            style,
            bar_style,
            target_style,
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
        if bullets.is_empty() {
            return;
        }

        // The ceiling: the caller's, or the largest of every value, target and
        // threshold, never below 1 so the scale math is total.
        let ceiling = max
            .or_else(|| {
                bullets
                    .iter()
                    .flat_map(|b| {
                        std::iter::once(b.value)
                            .chain(std::iter::once(b.target))
                            .chain(b.ranges.iter().copied())
                    })
                    .max()
            })
            .unwrap_or(0)
            .max(1);

        let bar_glyph = style.patch(bar_style);
        let tick_glyph = style.patch(target_style);

        match direction {
            BulletChartDirection::Horizontal => {
                // A left label column, at most half the width, sized to the
                // longest label; the track fills the rest.
                let longest = bullets.iter().map(|b| b.label.width()).max().unwrap_or(0) as u16;
                let label_w = longest.min(inner.width / 2);
                let bar_x0 = inner.left().saturating_add(label_w);
                let track_w = inner.width.saturating_sub(label_w);
                let bottom = inner.bottom();
                let track_right = inner.right();

                let rows = bullets.len() as u16;
                let row_h = (inner.height / rows.max(1)).max(1);

                let mut y0 = inner.top();
                for bullet in &bullets {
                    if y0 >= bottom {
                        break;
                    }
                    let group_bottom = y0.saturating_add(row_h).min(bottom);

                    for y in y0..group_bottom {
                        // Qualitative bands, widest (darkest) first so a
                        // lighter outer band overpaints the tail of the
                        // previous fill — Few's progressively-lighter look.
                        for (i, &thr) in bullet.ranges.iter().enumerate().rev() {
                            let w = cells(thr, ceiling, track_w);
                            let g = shade(i);
                            for c in 0..w {
                                let x = bar_x0.saturating_add(c);
                                if x >= track_right {
                                    break;
                                }
                                buf.set_cell(Position::new(x, y), g, bar_glyph);
                            }
                        }
                        // The measure bar over the bands: full blocks plus one
                        // fractional eighth-block boundary cell.
                        let total_e = eighths(bullet.value, ceiling, track_w);
                        let full = (total_e / 8) as u16;
                        let rem = (total_e % 8) as u16;
                        for c in 0..full {
                            let x = bar_x0.saturating_add(c);
                            if x >= track_right {
                                break;
                            }
                            buf.set_cell(Position::new(x, y), '█', bar_glyph);
                        }
                        if rem > 0 && full < track_w {
                            let x = bar_x0.saturating_add(full);
                            if x < track_right {
                                buf.set_cell(
                                    Position::new(x, y),
                                    HORIZONTAL_EIGHTHS[(rem - 1) as usize],
                                    bar_glyph,
                                );
                            }
                        }
                        // The target tick: one contrasting vertical glyph.
                        let tcol = cells(bullet.target, ceiling, track_w);
                        let tx = bar_x0.saturating_add(tcol.min(track_w.saturating_sub(1)));
                        if tx < track_right && track_w > 0 {
                            buf.set_cell(Position::new(tx, y), '│', tick_glyph);
                        }
                    }

                    // Label in the left column on the group's first row.
                    if label_w > 0 {
                        stamp_line(
                            buf,
                            &bullet.label,
                            style.patch(label_style),
                            inner.left(),
                            y0,
                            inner.left().saturating_add(label_w),
                        );
                    }
                    y0 = group_bottom;
                }
            }
            BulletChartDirection::Vertical => {
                // The bottom inner row is the label row (when there is more
                // than one row); the track rises in the rows above it.
                let label_row = inner.height > 1;
                let track_rows = inner.height.saturating_sub(u16::from(label_row));
                let label_y = inner.bottom().saturating_sub(1);
                let right = inner.right();

                let cols = bullets.len() as u16;
                let col_w = (inner.width / cols.max(1)).max(1);

                let mut x0 = inner.left();
                for bullet in &bullets {
                    if x0 >= right {
                        break;
                    }
                    let group_right = x0.saturating_add(col_w).min(right);

                    for x in x0..group_right {
                        // Qualitative bands from the baseline up, widest
                        // (darkest) first so lighter outer bands overpaint.
                        for (i, &thr) in bullet.ranges.iter().enumerate().rev() {
                            let h = cells(thr, ceiling, track_rows);
                            let g = shade(i);
                            for r in 0..h {
                                let y = inner.top().saturating_add(track_rows - 1 - r);
                                buf.set_cell(Position::new(x, y), g, bar_glyph);
                            }
                        }
                        // The measure bar over the bands.
                        let total_e = eighths(bullet.value, ceiling, track_rows);
                        let full = (total_e / 8) as u16;
                        let rem = (total_e % 8) as u16;
                        for r in 0..full {
                            let y = inner.top().saturating_add(track_rows - 1 - r);
                            buf.set_cell(Position::new(x, y), '█', bar_glyph);
                        }
                        if rem > 0 && full < track_rows {
                            let y = inner.top().saturating_add(track_rows - 1 - full);
                            buf.set_cell(
                                Position::new(x, y),
                                VERTICAL_EIGHTHS[(rem - 1) as usize],
                                bar_glyph,
                            );
                        }
                        // The target tick: one contrasting horizontal glyph.
                        let trow = cells(bullet.target, ceiling, track_rows);
                        let ty = inner.top().saturating_add(
                            track_rows.saturating_sub(1 + trow.min(track_rows.saturating_sub(1))),
                        );
                        if track_rows > 0 {
                            buf.set_cell(Position::new(x, ty), '─', tick_glyph);
                        }
                    }

                    // Label centred under the column group, clipped to it.
                    if label_row {
                        let lw = (bullet.label.width() as u16).min(col_w);
                        let lx = x0.saturating_add((col_w - lw) / 2);
                        stamp_line(
                            buf,
                            &bullet.label,
                            style.patch(label_style),
                            lx,
                            label_y,
                            group_right,
                        );
                    }
                    x0 = group_right;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier, Span};

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row (legible for ASCII border
    /// snapshots).
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

    /// Renders `widget` and returns its glyphs as a row-major `char` grid, so
    /// assertions index by *cell* (the bar/band/shade glyphs are multi-byte —
    /// byte slicing would split them).
    fn grid<W: Widget>(widget: W, width: u16, height: u16) -> Vec<Vec<char>> {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buf.get(Position::new(x, y)).unwrap().symbol)
                    .collect()
            })
            .collect()
    }

    /// One row of a [`grid`] as a `String` (UTF-8 safe — collected from
    /// `char`s, never byte-sliced).
    fn row(g: &[Vec<char>], y: usize) -> String {
        g[y].iter().collect()
    }

    #[test]
    fn a_horizontal_bullet_layers_bands_bar_and_target() {
        // ceiling 10, 11 wide → 1 label col + 10 track. ranges [5,10] paint
        // back-to-front (▒ to 10, then ▓ to 5); the measure bar (value 8 → 8
        // full blocks) overpaints them; the target tick (target 6) is drawn
        // *last*, on top of the bar, so it always stays visible (Few's rule).
        let chart = BulletChart::new([Bullet::new(8, 6, vec![5, 10], "x")]).max(Some(10));
        let g = grid(chart, 11, 1);
        assert_eq!(row(&g, 0), "x██████│█▒▒");
    }

    #[test]
    fn the_target_tick_shows_past_the_measure_bar() {
        // value 3, target 8: no label, no bands → a bare 10-wide track. The
        // bar fills cols 0..3; the target tick stands clear at col 8.
        let chart = BulletChart::new([Bullet::new(3, 8, vec![], "")]).max(Some(10));
        let g = grid(chart, 10, 1);
        assert_eq!(row(&g, 0), "███     │ ");
    }

    #[test]
    fn a_fractional_measure_bar_uses_a_sub_cell_glyph() {
        // value 1, ceiling 4, 2-wide track → 0.5 cell → 4 eighths → ▌ at col
        // 0; target 4 (full) → tick clamps to the last col, clear of the
        // boundary glyph so the sub-cell precision is visible.
        let chart = BulletChart::new([Bullet::new(1, 4, vec![], "")]).max(Some(4));
        let g = grid(chart, 2, 1);
        assert_eq!(row(&g, 0), "▌│");
    }

    #[test]
    fn empty_ranges_draw_a_bare_bar_with_a_tick() {
        // 10 wide, no bands: just the measure bar (5 █) and the target tick.
        let chart = BulletChart::new([Bullet::new(5, 7, vec![], "")]).max(Some(10));
        let g = grid(chart, 10, 1);
        assert_eq!(row(&g, 0), "█████  │  ");
    }

    #[test]
    fn value_and_target_above_the_ceiling_clamp() {
        // Both clamp to the ceiling: a full measure bar, and the target tick
        // clamped to the last cell (drawn on top, still visible) — no panic,
        // no overflow.
        let chart = BulletChart::new([Bullet::new(999, 999, vec![], "")]).max(Some(10));
        let g = grid(chart, 10, 1);
        assert_eq!(row(&g, 0), "█████████│");
    }

    #[test]
    fn auto_scale_uses_the_largest_value_target_or_threshold() {
        // No max → ceiling = max(value 4, target 2, threshold 8) = 8, 8-wide
        // track. The single band (▓) fills 0..8, the bar (4 █) overpaints
        // 0..4, the target tick (2) overpaints col 2.
        let chart = BulletChart::new([Bullet::new(4, 2, vec![8], "")]);
        let g = grid(chart, 8, 1);
        assert_eq!(row(&g, 0), "██│█▓▓▓▓");
    }

    #[test]
    fn the_vertical_direction_grows_up_with_a_bottom_label_row() {
        // 3 tall: 2 track rows + 1 label row. ceiling 8, value 8 → both rows
        // full; no ranges; target 4 → 4/8 of 2 rows = 1 → tick row.
        let chart = BulletChart::new([Bullet::new(8, 0, vec![], "v")])
            .direction(BulletChartDirection::Vertical)
            .max(Some(8));
        // value 8 fills both track rows; target 0 → `─` at the baseline row,
        // overpainted by the full bar there. Top row is the bar.
        let out = lines(chart, 1, 3);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows[0], "█");
        assert_eq!(rows[2], "v"); // label row
    }

    #[test]
    fn a_vertical_bullet_shows_the_target_dash() {
        // value 0 so the bar never hides the tick; target 8 (full) of 2 rows
        // → tick on the top track row.
        let chart = BulletChart::new([Bullet::new(0, 8, vec![], "v")])
            .direction(BulletChartDirection::Vertical)
            .max(Some(8));
        let out = lines(chart, 1, 3);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows[0], "─"); // target tick at the top
        assert_eq!(rows[2], "v"); // label
    }

    #[test]
    fn a_block_frames_the_chart_in_the_inner_area() {
        let chart = BulletChart::new([Bullet::new(8, 8, vec![], "")])
            .max(Some(8))
            .block(Block::bordered());
        // 5×3 → inner 3×3 track: the bar fills it and the target tick (8 →
        // clamped to the last column) sits on top, all inside the border.
        assert_eq!(lines(chart, 5, 3), "┌───┐\n│██││\n└───┘\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_nothing_inside() {
        let chart = BulletChart::new([Bullet::new(8, 4, vec![], "")]).block(Block::bordered());
        assert_eq!(lines(chart, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn no_bullets_with_a_block_still_renders_the_block() {
        let chart = BulletChart::new(Vec::<Bullet>::new()).block(Block::bordered());
        assert_eq!(lines(chart, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn style_cascades_base_then_bar_target_and_label_styles() {
        let bullet = Bullet::new(
            8,
            4,
            vec![],
            Line::from(Span::styled("L", Style::new().fg(Color::Red))),
        );
        let chart = BulletChart::new([bullet])
            .max(Some(8))
            .style(Style::new().bg(Color::Blue))
            .bar_style(Style::new().fg(Color::Green))
            .target_style(Style::new().fg(Color::Yellow))
            .label_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 9, 1));
        chart.render(buf.area(), &mut buf);

        // Label column is 1 wide (longest label "L"); track is 8.
        let l = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(l.symbol, 'L');
        assert_eq!(l.fg, Color::Red); // span fg wins
        assert!(l.modifier.contains(Modifier::BOLD)); // label_style cascades
        assert_eq!(l.bg, Color::Blue); // base fill cascades

        // A measure cell: bar_style fg over the base bg.
        let bar = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(bar.symbol, '█');
        assert_eq!(bar.fg, Color::Green);
        assert_eq!(bar.bg, Color::Blue);

        // The target tick (value 8 covers cols 0..8; target 4 → col 1+4=5,
        // which is under the bar, so check a value that exposes it).
        let chart2 = BulletChart::new([Bullet::new(2, 6, vec![], "")])
            .max(Some(8))
            .target_style(Style::new().fg(Color::Yellow));
        let mut buf2 = Buffer::empty(Rect::new(0, 0, 8, 1));
        chart2.render(buf2.area(), &mut buf2);
        let tick = buf2.get(Position::new(6, 0)).unwrap();
        assert_eq!(tick.symbol, '│');
        assert_eq!(tick.fg, Color::Yellow);
    }

    #[test]
    fn the_base_style_fills_the_whole_content_area() {
        let chart = BulletChart::new([Bullet::new(3, 5, vec![], "")])
            .max(Some(10))
            .style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        chart.render(buf.area(), &mut buf);
        for y in 0..3 {
            for x in 0..8 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
    }

    #[test]
    fn multiple_bullets_stack_as_rows() {
        let chart = BulletChart::new([
            Bullet::new(8, 0, vec![], "a"),
            Bullet::new(4, 0, vec![], "b"),
        ])
        .max(Some(8));
        // 2 rows: 1 label col + 7 track. a = 8/8 → 7 █ (target 0 → tick on
        // col 1, over the bar). b = 4/8 ≈ 3.5 → "███▌".
        let g = grid(chart, 8, 2);
        assert_eq!(row(&g, 0), "a│██████");
        assert_eq!(row(&g, 1), "b│██▌   ");
    }

    #[test]
    fn a_one_cell_track_does_not_panic() {
        // The label clamps to width/2 = 1 cell, leaving a 1-cell track for the
        // bands/bar/tick — exercises the saturating track math.
        let chart = BulletChart::new([Bullet::new(5, 3, vec![2, 4], "label")]);
        let g = grid(chart, 2, 1);
        // Label 'l' then the (overpainted) 1-cell track — rendered, no panic.
        assert_eq!(g[0].len(), 2);
        assert_eq!(g[0][0], 'l');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        BulletChart::new([Bullet::new(5, 3, vec![2], "x")]).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
