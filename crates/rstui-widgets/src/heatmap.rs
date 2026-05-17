//! [`Heatmap`] — a 2-D value grid mapped to an intensity ramp, the
//! observability primitive for "density over two axes" (request latency
//! buckets over time, per-service error rate, a contribution calendar).
//!
//! # A pure projection, like every other widget
//!
//! `Heatmap` owns no state. It is a borrowed caller-owned flat row-major
//! `&[f64]` plus a `cols` count, an optional value range, and a [`Style`]; the
//! reducer decides *what* the grid is (the matrix it recomputes in `update`)
//! and the widget only projects "the values right now" onto an intensity ramp.
//! That keeps it deterministically headless-testable and composes with the Elm
//! `view(&self)` model exactly like [`List`](crate::List) and
//! [`Gauge`](crate::Gauge).
//!
//! # A flat slice, not a slice-of-slices
//!
//! The caller passes one `&[f64]` and a column count; row `r`, column `c` is
//! `values[r * cols + c]` and the row count is `ceil(len / cols)`. A flat
//! slice sidesteps the lifetime pain of `&[&[f64]]` and is fully total: a
//! short final row is padded with empty cells, never an index panic.
//!
//! # The intensity ramp, one axis over from [`Gauge`](crate::Gauge)
//!
//! [`Gauge`](crate::Gauge) maps a fraction onto the eighth-block ramp on the
//! *horizontal* axis. A heatmap maps each cell's value, scaled against
//! [`min`](Heatmap::min)/[`max`](Heatmap::max), onto an intensity: either the
//! five-step shade ramp ` ░▒▓█` (each a single Unicode scalar, mapping 1:1
//! onto a [`Cell`](rstui_core::Buffer) with no grapheme machinery — the same
//! reasoning [`Block`] borders and the gauge ramp use) or, with
//! [`glyph_ramp(false)`](Heatmap::glyph_ramp), a per-channel background-color
//! lerp from [`low_color`](Heatmap::low_color) to
//! [`high_color`](Heatmap::high_color) drawn on a blank cell (the Grafana
//! colour-block reading).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, an empty grid, a `cols` of zero, a degenerate `min == max` range
//! (every cell maps to the lowest intensity), more rows/columns than the area
//! holds (clipped), a short final row (padded empty), and a non-`Rgb`
//! [`low_color`](Heatmap::low_color)/[`high_color`](Heatmap::high_color) (used
//! directly at each end of the lerp) are all safe clips/no-ops — never a
//! panic. An optional framing [`Block`] follows the container-widget
//! convention.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Position, Rect, Widget};
//! use rstui_widgets::Heatmap;
//!
//! let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
//! let values = [0.0_f64, 8.0];
//! Heatmap::new(&values, 2).max(Some(8.0)).render(buf.area(), &mut buf);
//!
//! // The lowest intensity is the blank cell; the highest is the full block.
//! assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
//! assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '█');
//! ```

use rstui_core::{Buffer, Color, Position, Rect, Style, Widget};

use crate::block::Block;

/// The five intensity shades, lowest to highest.
///
/// `SHADES[0]` is the blank track (a value at the floor of the range) and
/// `SHADES[4]` is the full block; the three between scale the density.
const SHADES: [char; 5] = [' ', '░', '▒', '▓', '█'];

/// A 2-D value grid mapped to an intensity ramp, with an optional framing
/// [`Block`].
///
/// The caller owns a flat row-major `&[f64]` plus a `cols` count; cell
/// `(row, col)` is `values[row * cols + col]` and the row count is
/// `ceil(len / cols)`. Each value is scaled against
/// [`min`](Self::min)/[`max`](Self::max) (auto from the data when unset) and
/// drawn as one [`cell_width`](Self::cell_width)-wide block: either a shade
/// glyph from ` ░▒▓█` ([`glyph_ramp(true)`](Self::glyph_ramp), the default) or
/// a background-colour lerp between [`low_color`](Self::low_color) and
/// [`high_color`](Self::high_color). Styling is a base [`Style`] (filling the
/// content area) with a [`label_style`](Self::label_style) for the optional
/// [`row_labels`](Self::row_labels)/[`col_labels`](Self::col_labels) gutters.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Heatmap;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
/// // 2×2 grid, row-major: top row [0, 4], bottom row [8, 2].
/// let values = [0.0_f64, 4.0, 8.0, 2.0];
/// Heatmap::new(&values, 2).max(Some(8.0)).render(buf.area(), &mut buf);
///
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' '); // 0/8
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '█'); // 8/8
/// ```
#[derive(Debug, Clone)]
pub struct Heatmap<'a> {
    values: &'a [f64],
    cols: usize,
    min: Option<f64>,
    max: Option<f64>,
    glyph_ramp: bool,
    low_color: Color,
    high_color: Color,
    cell_width: u16,
    row_labels: Option<&'a [&'a str]>,
    col_labels: Option<&'a [&'a str]>,
    block: Option<Block<'a>>,
    style: Style,
    label_style: Style,
}

impl Default for Heatmap<'_> {
    fn default() -> Self {
        Self {
            values: &[],
            cols: 0,
            min: None,
            max: None,
            // The shade ramp is the default: it needs no colour support and
            // reads in any terminal (the Sparkline glyph reasoning).
            glyph_ramp: true,
            low_color: Color::Black,
            high_color: Color::Red,
            // One-cell blocks: the sensible default that never widens a grid
            // beyond its column count (BarChart's bar_width reasoning).
            cell_width: 1,
            row_labels: None,
            col_labels: None,
            block: None,
            style: Style::default(),
            label_style: Style::default(),
        }
    }
}

impl<'a> Heatmap<'a> {
    /// A heatmap projecting `values` row-major over `cols` columns (row count
    /// `ceil(len / cols)`), auto-scaled to the data, glyph-ramped, unstyled.
    ///
    /// A `cols` of `0` renders nothing (the totality rule); a short final row
    /// is padded with empty cells.
    #[must_use]
    pub fn new(values: &'a [f64], cols: usize) -> Self {
        Self {
            values,
            cols,
            ..Self::default()
        }
    }

    /// Sets the value mapped to the **full** intensity, or `None` to auto-scale
    /// to the largest value.
    ///
    /// A value above the ceiling is clamped to the full intensity (never a
    /// panic — the [`Gauge`](crate::Gauge) totality rule); a degenerate
    /// `min == max` range maps every cell to the lowest intensity.
    #[must_use]
    pub fn max(mut self, max: Option<f64>) -> Self {
        self.max = max;
        self
    }

    /// Sets the value mapped to the **empty** intensity, or `None` to
    /// auto-scale to the smallest value.
    ///
    /// A value below the floor is clamped to the lowest intensity.
    #[must_use]
    pub fn min(mut self, min: Option<f64>) -> Self {
        self.min = min;
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a background
    /// covers the whole pane.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets whether intensity maps to the five-step shade ramp ` ░▒▓█`
    /// (`true`, the default) or to a background-colour lerp between
    /// [`low_color`](Self::low_color) and [`high_color`](Self::high_color)
    /// (`false`).
    #[must_use]
    pub fn glyph_ramp(mut self, glyph_ramp: bool) -> Self {
        self.glyph_ramp = glyph_ramp;
        self
    }

    /// Sets the colour at the **empty** end of the lerp (default
    /// [`Color::Black`]); only used when
    /// [`glyph_ramp(false)`](Self::glyph_ramp).
    #[must_use]
    pub fn low_color(mut self, low_color: Color) -> Self {
        self.low_color = low_color;
        self
    }

    /// Sets the colour at the **full** end of the lerp (default
    /// [`Color::Red`]); only used when
    /// [`glyph_ramp(false)`](Self::glyph_ramp).
    #[must_use]
    pub fn high_color(mut self, high_color: Color) -> Self {
        self.high_color = high_color;
        self
    }

    /// Sets how many columns wide each grid cell is drawn (default `1`), so a
    /// heatmap reads as blocks. Clamped to at least `1` and clipped to the
    /// area at render time.
    #[must_use]
    pub fn cell_width(mut self, cell_width: u16) -> Self {
        self.cell_width = cell_width;
        self
    }

    /// Sets the per-row labels drawn in a reserved left gutter (sized to the
    /// longest, clipped to half the width); the gutter is omitted when unset.
    ///
    /// Label `r` annotates grid row `r`; extra labels are ignored and missing
    /// ones leave a blank gutter cell.
    #[must_use]
    pub fn row_labels(mut self, row_labels: &'a [&'a str]) -> Self {
        self.row_labels = Some(row_labels);
        self
    }

    /// Sets the per-column labels drawn in a reserved bottom row (each clipped
    /// to its cell's width); the row is omitted when unset.
    ///
    /// Label `c` annotates grid column `c`; extra labels are ignored and
    /// missing ones leave a blank cell.
    #[must_use]
    pub fn col_labels(mut self, col_labels: &'a [&'a str]) -> Self {
        self.col_labels = Some(col_labels);
        self
    }

    /// Sets the base [`Style`] for the gutter labels, over the base.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Frames the heatmap in `block`; the grid renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }
}

/// Maps `t` (already clamped to `0.0..=1.0`) to one of the five
/// `SHADES` indices (`0` = blank track, `4` = full block).
fn shade_index(t: f64) -> usize {
    // Nearest of the five steps: round, then clamp so NaN/inf-safe inputs
    // (already pre-clamped) can never index out of bounds.
    let scaled = t * 4.0 + 0.5;
    if scaled <= 0.0 {
        0
    } else if scaled >= 4.0 {
        4
    } else {
        scaled as usize
    }
}

/// One channel of a linear lerp between `a` and `b` at fraction `t`
/// (`0.0..=1.0`), rounded to the nearest byte.
fn lerp_channel(a: u8, b: u8, t: f64) -> u8 {
    let lo = f64::from(a);
    let hi = f64::from(b);
    let v = lo + (hi - lo) * t + 0.5;
    if v <= 0.0 {
        0
    } else if v >= 255.0 {
        255
    } else {
        v as u8
    }
}

/// The lerped colour for fraction `t` (`0.0..=1.0`) between `low` and `high`.
///
/// A per-channel `Rgb` lerp when both ends are `Rgb`; otherwise total by
/// falling back to `low` for `t < 0.5` and `high` for `t >= 0.5` (no panic on
/// a named/indexed colour).
fn lerp_color(low: Color, high: Color, t: f64) -> Color {
    match (low, high) {
        (Color::Rgb(lr, lg, lb), Color::Rgb(hr, hg, hb)) => Color::Rgb(
            lerp_channel(lr, hr, t),
            lerp_channel(lg, hg, t),
            lerp_channel(lb, hb, t),
        ),
        _ => {
            if t < 0.5 {
                low
            } else {
                high
            }
        }
    }
}

/// Stamps `text` left-to-right from `x0` on row `y`, clipped at `right`, with
/// `style`.
fn stamp_str(buf: &mut Buffer, text: &str, style: Style, x0: u16, y: u16, right: u16) {
    let mut x = x0;
    for ch in text.chars() {
        if x >= right {
            break;
        }
        buf.set_cell(Position::new(x, y), ch, style);
        x = x.saturating_add(1);
    }
}

impl Widget for Heatmap<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Heatmap {
            values,
            cols,
            min,
            max,
            glyph_ramp,
            low_color,
            high_color,
            cell_width,
            row_labels,
            col_labels,
            block,
            style,
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
        if cols == 0 || values.is_empty() {
            return;
        }

        // Row count: ceil(len / cols); the final row may be short and is then
        // padded with empty cells.
        let rows = values.len().div_ceil(cols);

        // The range: the caller's, or the data extremes. A degenerate range
        // (min == max, all-equal data, or a single value) maps every cell to
        // the lowest intensity rather than dividing by zero.
        let lo = min.unwrap_or_else(|| {
            values
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .fold(f64::INFINITY, f64::min)
        });
        let hi = max.unwrap_or_else(|| {
            values
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .fold(f64::NEG_INFINITY, f64::max)
        });
        let span = hi - lo;
        // A non-positive or non-finite span (min == max, all-equal data, a
        // single value, or no finite data) maps every cell to the floor
        // rather than dividing by zero.
        let usable_span = span.is_finite() && span > 0.0;

        let cell_w = cell_width.max(1);
        let label_glyph = style.patch(label_style);

        // A left gutter for row labels, at most half the width, sized to the
        // longest label; omitted entirely when no labels are set.
        let gutter_w = match row_labels {
            Some(labels) => {
                let longest = labels.iter().map(|s| s.chars().count()).max().unwrap_or(0);
                (longest as u16).min(inner.width / 2)
            }
            None => 0,
        };
        // A bottom row for column labels; omitted when no labels are set or
        // there is only one row to draw.
        let has_col_row = col_labels.is_some() && inner.height > 1;

        let grid_x0 = inner.left().saturating_add(gutter_w);
        let grid_right = inner.right();
        let grid_top = inner.top();
        let grid_bottom = inner.bottom().saturating_sub(u16::from(has_col_row));

        // Only the rows that fit can ever be drawn, so cap the loop there:
        // this keeps the row index small enough that the flat-slice offset
        // and the `u16` row coordinate can never overflow.
        let drawable_rows = rows.min(usize::from(grid_bottom.saturating_sub(grid_top)));
        for r in 0..drawable_rows {
            let y = grid_top.saturating_add(r as u16);
            if y >= grid_bottom {
                break;
            }

            // The optional row-label gutter on this grid row.
            if gutter_w > 0 {
                if let Some(label) = row_labels.and_then(|l| l.get(r)) {
                    stamp_str(
                        buf,
                        label,
                        label_glyph,
                        inner.left(),
                        y,
                        inner.left().saturating_add(gutter_w),
                    );
                }
            }

            let mut x = grid_x0;
            for c in 0..cols {
                if x >= grid_right {
                    break;
                }
                // Checked offset: an out-of-range or overflowing index is a
                // padded empty cell, never a panic or wrap.
                let value = r
                    .checked_mul(cols)
                    .and_then(|base| base.checked_add(c))
                    .and_then(|idx| values.get(idx))
                    .copied();
                // A short final row (no value) is a padded empty cell; a
                // non-finite value also maps to the floor.
                let t = match value {
                    Some(v) if v.is_finite() && usable_span => ((v - lo) / span).clamp(0.0, 1.0),
                    _ => 0.0,
                };

                let block_right = x.saturating_add(cell_w).min(grid_right);
                if glyph_ramp {
                    let glyph = SHADES[shade_index(t)];
                    for bx in x..block_right {
                        buf.set_cell(Position::new(bx, y), glyph, style);
                    }
                } else {
                    let bg = lerp_color(low_color, high_color, t);
                    let cell_style = style.bg(bg);
                    for bx in x..block_right {
                        buf.set_cell(Position::new(bx, y), ' ', cell_style);
                    }
                }
                x = block_right;
            }
        }

        // The optional column-label row beneath the grid, each label aligned
        // to its cell and clipped to the cell width.
        if has_col_row {
            let label_y = inner.bottom().saturating_sub(1);
            if let Some(labels) = col_labels {
                let mut x = grid_x0;
                for c in 0..cols {
                    if x >= grid_right {
                        break;
                    }
                    let block_right = x.saturating_add(cell_w).min(grid_right);
                    if let Some(label) = labels.get(c) {
                        stamp_str(buf, label, label_glyph, x, label_y, block_right);
                    }
                    x = block_right;
                }
            }
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
    fn the_shade_ramp_maps_each_fraction_to_its_glyph() {
        // Range 0..=8; the five values land on the five ramp steps.
        let values = [0.0_f64, 2.0, 4.0, 6.0, 8.0];
        assert_eq!(
            lines(Heatmap::new(&values, 5).max(Some(8.0)), 5, 1),
            " ░▒▓█\n"
        );
    }

    #[test]
    fn auto_scale_maps_the_data_extremes_to_the_ramp_ends() {
        // No explicit range: min=2 → blank, max=10 → full block.
        let values = [2.0_f64, 6.0, 10.0];
        assert_eq!(lines(Heatmap::new(&values, 3), 3, 1), " ▒█\n");
    }

    #[test]
    fn an_explicit_range_scales_against_min_and_max() {
        let values = [10.0_f64, 20.0, 30.0];
        // min=10 → blank, max=30 → full, 20 → midpoint ▒.
        assert_eq!(
            lines(
                Heatmap::new(&values, 3).min(Some(10.0)).max(Some(30.0)),
                3,
                1
            ),
            " ▒█\n"
        );
    }

    #[test]
    fn a_value_above_the_ceiling_clamps_to_the_full_block() {
        let values = [999.0_f64];
        assert_eq!(
            lines(Heatmap::new(&values, 1).min(Some(0.0)).max(Some(8.0)), 1, 1),
            "█\n"
        );
    }

    #[test]
    fn a_value_below_the_floor_clamps_to_the_blank_track() {
        let values = [-5.0_f64];
        assert_eq!(
            lines(Heatmap::new(&values, 1).min(Some(0.0)).max(Some(8.0)), 1, 1),
            " \n"
        );
    }

    #[test]
    fn a_degenerate_range_maps_every_cell_to_the_lowest_intensity() {
        // All-equal data → span 0 → every cell the blank track, no div-by-zero.
        let values = [5.0_f64, 5.0, 5.0, 5.0];
        assert_eq!(lines(Heatmap::new(&values, 2), 2, 2), "  \n  \n");
    }

    #[test]
    fn the_grid_is_row_major_over_the_column_count() {
        // Top row [0, 8], bottom row [8, 0] against range 0..=8.
        let values = [0.0_f64, 8.0, 8.0, 0.0];
        assert_eq!(
            lines(Heatmap::new(&values, 2).max(Some(8.0)), 2, 2),
            " █\n█ \n"
        );
    }

    #[test]
    fn a_short_final_row_is_padded_with_empty_cells() {
        // 3 values, 2 cols → 2 rows; the second row's missing cell is blank.
        let values = [8.0_f64, 8.0, 8.0];
        assert_eq!(
            lines(Heatmap::new(&values, 2).min(Some(0.0)).max(Some(8.0)), 2, 2),
            "██\n█ \n"
        );
    }

    #[test]
    fn cell_width_widens_each_block() {
        let values = [0.0_f64, 8.0];
        // Each cell two columns wide: blank-blank then block-block.
        assert_eq!(
            lines(Heatmap::new(&values, 2).max(Some(8.0)).cell_width(2), 4, 1),
            "  ██\n"
        );
    }

    #[test]
    fn more_columns_than_the_area_clip_at_the_right_edge() {
        let values = [8.0_f64, 8.0, 8.0, 8.0, 8.0];
        assert_eq!(
            lines(Heatmap::new(&values, 5).min(Some(0.0)).max(Some(8.0)), 3, 1),
            "███\n"
        );
    }

    #[test]
    fn more_rows_than_the_area_clip_at_the_bottom_edge() {
        let values = [8.0_f64, 8.0, 8.0, 8.0];
        // 2×2 grid into a 2×1 area: only the first row is drawn.
        assert_eq!(
            lines(Heatmap::new(&values, 2).min(Some(0.0)).max(Some(8.0)), 2, 1),
            "██\n"
        );
    }

    #[test]
    fn an_empty_grid_just_fills_the_area() {
        let values: [f64; 0] = [];
        assert_eq!(lines(Heatmap::new(&values, 4), 3, 1), "   \n");
    }

    #[test]
    fn a_zero_column_count_renders_nothing() {
        let values = [1.0_f64, 2.0, 3.0];
        assert_eq!(lines(Heatmap::new(&values, 0), 3, 1), "   \n");
    }

    #[test]
    fn row_labels_reserve_a_left_gutter() {
        let values = [0.0_f64, 8.0, 8.0, 0.0];
        let row_labels: [&str; 2] = ["a", "b"];
        assert_eq!(
            lines(
                Heatmap::new(&values, 2)
                    .max(Some(8.0))
                    .row_labels(&row_labels),
                3,
                2
            ),
            "a █\nb█ \n"
        );
    }

    #[test]
    fn col_labels_reserve_a_bottom_row() {
        let values = [0.0_f64, 8.0];
        let col_labels: [&str; 2] = ["x", "y"];
        assert_eq!(
            lines(
                Heatmap::new(&values, 2)
                    .max(Some(8.0))
                    .col_labels(&col_labels),
                2,
                2
            ),
            " █\nxy\n"
        );
    }

    #[test]
    fn no_labels_omit_both_gutters_entirely() {
        let values = [0.0_f64, 8.0, 8.0, 0.0];
        // Without labels the grid uses the whole area, no reserved edges.
        assert_eq!(
            lines(Heatmap::new(&values, 2).max(Some(8.0)), 2, 2),
            " █\n█ \n"
        );
    }

    #[test]
    fn the_colour_lerp_draws_blank_cells_with_a_background() {
        let values = [0.0_f64, 8.0];
        let heat = Heatmap::new(&values, 2)
            .max(Some(8.0))
            .glyph_ramp(false)
            .low_color(Color::Rgb(0, 0, 0))
            .high_color(Color::Rgb(0, 0, 255));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        heat.render(buf.area(), &mut buf);
        // Both cells are blank; the bg lerps from low (0,0,0) to high (0,0,255).
        let lo = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(lo.symbol, ' ');
        assert_eq!(lo.bg, Color::Rgb(0, 0, 0));
        let hi = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(hi.symbol, ' ');
        assert_eq!(hi.bg, Color::Rgb(0, 0, 255));
    }

    #[test]
    fn a_non_rgb_lerp_colour_is_total_and_picks_an_end() {
        // Named colours can't lerp per-channel: low for t<0.5, high otherwise.
        let values = [0.0_f64, 8.0];
        let heat = Heatmap::new(&values, 2)
            .max(Some(8.0))
            .glyph_ramp(false)
            .low_color(Color::Black)
            .high_color(Color::Red);
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        heat.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Black);
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().bg, Color::Red);
    }

    #[test]
    fn a_block_frames_the_grid_in_the_inner_area() {
        let values = [8.0_f64];
        let heat = Heatmap::new(&values, 1)
            .min(Some(0.0))
            .max(Some(8.0))
            .block(Block::bordered());
        assert_eq!(lines(heat, 3, 3), "┌─┐\n│█│\n└─┘\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_grid() {
        let values = [8.0_f64];
        let heat = Heatmap::new(&values, 1).block(Block::bordered());
        assert_eq!(lines(heat, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn an_empty_grid_with_a_block_still_renders_the_block() {
        let values: [f64; 0] = [];
        let heat = Heatmap::new(&values, 2).block(Block::bordered());
        assert_eq!(lines(heat, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn style_cascades_base_then_label_style() {
        let values = [8.0_f64];
        let row_labels: [&str; 1] = ["L"];
        let heat = Heatmap::new(&values, 1)
            .min(Some(0.0))
            .max(Some(8.0))
            .row_labels(&row_labels)
            .style(Style::new().bg(Color::Blue))
            .label_style(Style::new().fg(Color::Green).add_modifier(Modifier::BOLD));
        // gutter_w = min(1, 2/2) = 1; the block sits to its right.
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        heat.render(buf.area(), &mut buf);

        let label = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(label.symbol, 'L');
        assert_eq!(label.fg, Color::Green); // label_style fg
        assert!(label.modifier.contains(Modifier::BOLD)); // label_style cascades
        assert_eq!(label.bg, Color::Blue); // base fill cascades

        let glyph = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(glyph.symbol, '█');
        assert_eq!(glyph.bg, Color::Blue); // base fill cascades to the glyph
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let values = [8.0_f64, 8.0];
        let heat = Heatmap::new(&values, 2).min(Some(0.0)).max(Some(8.0));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        heat.render(Rect::new(2, 3, 2, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 3)).unwrap().symbol, '█');
        assert_eq!(buf.get(Position::new(3, 3)).unwrap().symbol, '█');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let values = [1.0_f64, 2.0, 3.0];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Heatmap::new(&values, 3).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn a_non_finite_value_maps_to_the_floor_without_panicking() {
        // NaN/inf must not panic or poison the mapping: they fall to the
        // floor while the finite cells still scale against the range.
        let values = [f64::NAN, 4.0, f64::INFINITY, 8.0];
        assert_eq!(
            lines(Heatmap::new(&values, 2).min(Some(0.0)).max(Some(8.0)), 2, 2),
            " ▒\n █\n"
        );
    }
}
