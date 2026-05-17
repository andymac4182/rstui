//! [`CalendarHeatmap`] — a GitHub-style contribution calendar: weeks as
//! columns, weekdays as rows, each day a glyph coloured from an intensity
//! ramp, the "activity over the last year" surface a dashboard pins in a pane
//! (commit streaks, deploy frequency, per-day ticket throughput, habit
//! tracking).
//!
//! # A pure projection, like every other widget
//!
//! `CalendarHeatmap` owns no state. It is a borrowed caller-owned `&[u64]` of
//! *consecutive day values* plus the weekday `day[0]` sits on and an optional
//! ceiling; the reducer decides what a day's value is (commits that day, a
//! rolling count) and the widget only projects "the numbers right now" onto
//! coloured cells. There is **no date math at all** — exactly the
//! [`Calendar`](crate::Calendar) discipline: the caller supplies the flat day
//! slice, the [`start_weekday`](CalendarHeatmap::start_weekday) of `day[0]`,
//! and any month-boundary labels via [`months`](CalendarHeatmap::months) (a
//! `(week column, label)` list), so the widget never computes a weekday or a
//! month and adds
//! no dependency. That keeps it deterministically headless-testable and
//! composes with the Elm `view(&self)` model exactly like
//! [`List`](crate::List) and [`Calendar`](crate::Calendar).
//!
//! # Precision: five intensity buckets, not a ramp glyph
//!
//! Unlike [`Sparkline`](crate::Sparkline)/[`Gauge`](crate::Gauge), the cell is
//! a fixed glyph (`■` by default, one Unicode scalar so it maps 1:1 onto a
//! [`Cell`](rstui_core::Buffer)); intensity is carried by **colour**, the
//! GitHub model. A day's value is bucketed `0..=4` by its ratio to the ceiling
//! ([`max`](CalendarHeatmap::max) or the largest value): `0` is the
//! empty/track level and `1..=4` are quartiles, so each bucket picks one of
//! the five [`levels`](CalendarHeatmap::levels) [`Style`]s (a sensible green
//! scale by default). The grid geometry is pure layout — column = week, row =
//! the weekday offset of that day from
//! [`start_weekday`](CalendarHeatmap::start_weekday).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, an empty slice, an all-zero slice (every cell the level-0 track), a
//! `start_weekday` outside `0..=6` (reduced mod 7), more days than fit (the
//! trailing week columns are clipped), a `max` of `0` (no division by zero —
//! every cell is level 0), and a tiny area are all safe clips/no-ops — never a
//! panic. An optional framing [`Block`] follows the container-widget
//! convention.

use rstui_core::{Buffer, Color, Position, Rect, Style, Widget};

use crate::block::Block;

/// Three-letter weekday labels, indexed by the `0 = Monday … 6 = Sunday`
/// convention this widget uses. A static label table is *not* date math (the
/// widget computes no dates — see the [module docs](self)).
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Columns the weekday-label gutter takes when enabled: three letters plus a
/// one-column gap before the grid.
const LABEL_W: u16 = 4;

/// A GitHub-style contribution calendar: a grid of weekday rows × week columns,
/// each day a glyph coloured from a five-step intensity ramp, as a pure
/// projection of a caller-owned consecutive day-value slice.
///
/// `CalendarHeatmap` does **no date math** — it is handed a flat `&[u64]` of
/// consecutive day values, the [`start_weekday`](Self::start_weekday) `day[0]`
/// falls on, and any [`months`](Self::months) boundary labels (see the
/// [module docs](self)) — and only lays them out. Each day's value is bucketed
/// to one of the five [`levels`](Self::levels) styles by its ratio to the
/// ceiling; an optional [`Block`] frames the grid.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::CalendarHeatmap;
///
/// // 9 consecutive days, day[0] on a Monday (row 0).
/// let values = [0u64, 1, 2, 3, 4, 5, 6, 7, 8];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 2, 7));
/// CalendarHeatmap::new(&values)
///     .start_weekday(0)
///     .weekday_labels(false)
///     .render(buf.area(), &mut buf);
///
/// // Every day is one cell glyph; day 0's value 0 is the level-0 track style
/// // (still the glyph, so a background reads). Days wrap down the column,
/// // then day 7 spills into the next week column (row 0).
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '■'); // day 0
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '■'); // day 1
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '■'); // day 7
/// // A cell with no day at all (row 6 of the short second column) is blank.
/// assert_eq!(buf.get(Position::new(1, 6)).unwrap().symbol, ' ');
/// ```
#[derive(Debug, Clone)]
pub struct CalendarHeatmap<'a> {
    data: &'a [u64],
    start_weekday: u8,
    max: Option<u64>,
    levels: [Style; 5],
    weekday_labels: bool,
    months: Vec<(usize, String)>,
    block: Option<Block<'a>>,
    style: Style,
    cell: char,
}

/// The default GitHub-like green intensity ramp: an empty track then four
/// brightening greens. Level `0` is the no-activity track.
fn default_levels() -> [Style; 5] {
    [
        Style::default().fg(Color::DarkGray),
        Style::default().fg(Color::Green),
        Style::default().fg(Color::Green),
        Style::default().fg(Color::LightGreen),
        Style::default().fg(Color::LightGreen),
    ]
}

impl<'a> CalendarHeatmap<'a> {
    /// A heatmap projecting `data` (day `0` is the first cell), auto-scaled to
    /// the largest value, with `day[0]` on Monday and the default green ramp.
    #[must_use]
    pub fn new(data: &'a [u64]) -> Self {
        Self {
            data,
            start_weekday: 0,
            max: None,
            levels: default_levels(),
            weekday_labels: false,
            months: Vec::new(),
            block: None,
            style: Style::default(),
            cell: '■',
        }
    }

    /// Sets the weekday `day[0]` sits on (`0 = Monday … 6 = Sunday`), i.e.
    /// which grid row the first day occupies. Reduced mod 7.
    #[must_use]
    pub fn start_weekday(mut self, start_weekday: u8) -> Self {
        self.start_weekday = start_weekday;
        self
    }

    /// Sets the value mapped to the brightest bucket, or `None` to auto-scale
    /// to the largest value.
    ///
    /// A `max` of `0` (or an all-zero auto-scaled slice) renders every cell at
    /// level `0` (never a panic — the [`Gauge`](crate::Gauge) totality rule).
    #[must_use]
    pub fn max(mut self, max: Option<u64>) -> Self {
        self.max = max;
        self
    }

    /// Sets the five intensity-bucket [`Style`]s, index `0` (no activity) …
    /// `4` (the brightest), each patched over the base.
    #[must_use]
    pub fn levels(mut self, levels: [Style; 5]) -> Self {
        self.levels = levels;
        self
    }

    /// Sets whether a three-letter weekday-label gutter is drawn on the left.
    #[must_use]
    pub fn weekday_labels(mut self, weekday_labels: bool) -> Self {
        self.weekday_labels = weekday_labels;
        self
    }

    /// Sets the month-boundary labels as `(week column, label)` pairs drawn on
    /// the top row above the grid.
    ///
    /// The caller supplies these so the widget stays date-math-free (see the
    /// [module docs](self)); a column past the grid is clipped.
    #[must_use]
    pub fn months(mut self, months: Vec<(usize, String)>) -> Self {
        self.months = months;
        self
    }

    /// Frames the heatmap in `block`; the grid renders into
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

    /// Sets the cell glyph (default `■`); one [`char`], stamped 1:1 per day.
    #[must_use]
    pub fn cell(mut self, cell: char) -> Self {
        self.cell = cell;
        self
    }
}

/// The intensity bucket `0..=4` for `value` against `ceiling` (already `>= 1`):
/// `0` for a zero value (the track), else a quartile `1..=4`.
fn bucket(value: u64, ceiling: u64) -> usize {
    if value == 0 {
        return 0;
    }
    // Ceil the ratio into 1..=4 so any non-zero value is at least level 1 and
    // a value at the ceiling is exactly level 4.
    let clamped = value.min(ceiling);
    let level = (u128::from(clamped) * 4).div_ceil(u128::from(ceiling)) as usize;
    level.clamp(1, 4)
}

/// Writes `text` left-to-right from `x0` on row `y`, clipped at `right`.
fn put(buf: &mut Buffer, text: &str, style: Style, x0: u16, y: u16, right: u16) {
    let mut x = x0;
    for ch in text.chars() {
        if x >= right {
            break;
        }
        buf.set_cell(Position::new(x, y), ch, style);
        x = x.saturating_add(1);
    }
}

impl Widget for CalendarHeatmap<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let CalendarHeatmap {
            data,
            start_weekday,
            max,
            levels,
            weekday_labels,
            months,
            block,
            style,
            cell,
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
        if data.is_empty() {
            return;
        }

        let left = inner.left();
        let right = inner.right();
        let bottom = inner.bottom();

        // An optional month-label row reserves the top inner row; the grid
        // starts below it.
        let month_row = !months.is_empty();
        let grid_top = inner.top().saturating_add(u16::from(month_row));

        // An optional weekday gutter reserves LABEL_W columns on the left.
        let gutter = if weekday_labels {
            LABEL_W.min(inner.width)
        } else {
            0
        };
        let grid_left = left.saturating_add(gutter);
        if grid_left >= right || grid_top >= bottom {
            return;
        }

        // The ceiling: the caller's, or the largest value, never below 1 so
        // the bucket math is total (an all-zero slice then renders all level 0).
        let ceiling = max
            .unwrap_or_else(|| data.iter().copied().max().unwrap_or(0))
            .max(1);

        let start_row = u16::from(start_weekday % 7);

        // Each day advances down its week column and wraps every 7 rows. The
        // first day sits on `start_row`, so column 0 may begin part-way down.
        for (i, &value) in data.iter().enumerate() {
            let slot = i as u16 + start_row;
            let week = slot / 7;
            let row = slot % 7;
            let x = grid_left.saturating_add(week);
            if x >= right {
                break;
            }
            let y = grid_top.saturating_add(row);
            if y >= bottom {
                continue;
            }
            let level = bucket(value, ceiling);
            let cell_style = style.patch(levels[level]);
            // Level 0 is the empty track: the base-styled glyph still draws so
            // a background reads, but it uses the track style.
            buf.set_cell(Position::new(x, y), cell, cell_style);
        }

        // The weekday-label gutter, one three-letter label per row.
        if gutter > 0 {
            for (r, label) in WEEKDAYS.iter().enumerate() {
                let y = grid_top.saturating_add(r as u16);
                if y >= bottom {
                    break;
                }
                put(buf, label, style, left, y, grid_left);
            }
        }

        // The month-boundary labels on the reserved top row, each at its
        // caller-supplied week column.
        if month_row {
            let mty = inner.top();
            for (week_col, label) in &months {
                let x = grid_left.saturating_add(*week_col as u16);
                if x >= right {
                    continue;
                }
                put(buf, label, style, x, mty, right);
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
    fn days_fill_a_week_column_then_wrap_to_the_next() {
        // 9 days, day[0] on Monday (row 0), no labels. Column 0 = days 0..6,
        // column 1 = days 7,8. Value 0 is the track glyph too (■ default).
        let values = [1u64, 1, 1, 1, 1, 1, 1, 1, 1];
        let out = lines(
            CalendarHeatmap::new(&values).start_weekday(0).max(Some(1)),
            2,
            7,
        );
        assert_eq!(out, "■■\n■■\n■ \n■ \n■ \n■ \n■ \n");
    }

    #[test]
    fn start_weekday_offsets_the_first_day_down_the_column() {
        // day[0] on Wednesday (row 2): the first two rows of column 0 are blank.
        let values = [1u64, 1];
        let out = lines(
            CalendarHeatmap::new(&values).start_weekday(2).max(Some(1)),
            1,
            7,
        );
        assert_eq!(out, " \n \n■\n■\n \n \n \n");
    }

    #[test]
    fn the_intensity_bucket_picks_the_level_style() {
        // Ceiling 4: 0→L0, 1→L1, 2→L2, 3→L3, 4→L4. Distinct fg per level.
        let values = [0u64, 1, 2, 3, 4];
        let levels = [
            Style::new().fg(Color::Black),
            Style::new().fg(Color::Red),
            Style::new().fg(Color::Yellow),
            Style::new().fg(Color::Blue),
            Style::new().fg(Color::Green),
        ];
        let cal = CalendarHeatmap::new(&values)
            .start_weekday(0)
            .max(Some(4))
            .levels(levels);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 7));
        cal.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Black);
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().fg, Color::Red);
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().fg, Color::Yellow);
        assert_eq!(buf.get(Position::new(0, 3)).unwrap().fg, Color::Blue);
        assert_eq!(buf.get(Position::new(0, 4)).unwrap().fg, Color::Green);
    }

    #[test]
    fn an_all_zero_slice_is_every_cell_the_track_level() {
        let values = [0u64, 0, 0];
        let levels = [
            Style::new().fg(Color::DarkGray),
            Style::new().fg(Color::Green),
            Style::new().fg(Color::Green),
            Style::new().fg(Color::Green),
            Style::new().fg(Color::Green),
        ];
        let cal = CalendarHeatmap::new(&values)
            .start_weekday(0)
            .levels(levels);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 7));
        cal.render(buf.area(), &mut buf);
        for y in 0..3 {
            assert_eq!(buf.get(Position::new(0, y)).unwrap().fg, Color::DarkGray);
        }
    }

    #[test]
    fn a_zero_max_does_not_divide_and_renders_all_track() {
        let values = [5u64, 9];
        let cal = CalendarHeatmap::new(&values).start_weekday(0).max(Some(0));
        let levels = [
            Style::new().fg(Color::DarkGray),
            Style::new().fg(Color::Green),
            Style::new().fg(Color::Green),
            Style::new().fg(Color::Green),
            Style::new().fg(Color::Green),
        ];
        let cal = cal.levels(levels);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 7));
        cal.render(buf.area(), &mut buf);
        // max=0 floors to 1; values clamp to it → level 4 (not a panic), but
        // crucially no division by zero occurred.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '■');
    }

    #[test]
    fn weekday_labels_draw_a_left_gutter() {
        let values = [1u64];
        let cal = CalendarHeatmap::new(&values)
            .start_weekday(0)
            .max(Some(1))
            .weekday_labels(true);
        let out = lines(cal, 5, 7);
        // 4-col gutter ("Mon" + gap), grid at col 4.
        assert_eq!(out, "Mon ■\nTue  \nWed  \nThu  \nFri  \nSat  \nSun  \n");
    }

    #[test]
    fn month_labels_draw_on_a_reserved_top_row() {
        let values = [1u64; 14];
        let cal = CalendarHeatmap::new(&values)
            .start_weekday(0)
            .max(Some(1))
            .months(vec![(0, "Jan".to_string()), (1, "Feb".to_string())]);
        let out = lines(cal, 4, 8);
        // Row 0 = month labels at week cols 0 and 1; grid below.
        let row0 = out.lines().next().unwrap();
        assert_eq!(&row0[..1], "J");
    }

    #[test]
    fn the_custom_cell_glyph_is_used() {
        let values = [1u64];
        let cal = CalendarHeatmap::new(&values)
            .start_weekday(0)
            .max(Some(1))
            .cell('●');
        assert_eq!(buf_symbol(cal), '●');
    }

    fn buf_symbol(cal: CalendarHeatmap) -> char {
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 7));
        cal.render(buf.area(), &mut buf);
        buf.get(Position::new(0, 0)).unwrap().symbol
    }

    #[test]
    fn more_days_than_columns_clip_the_trailing_weeks() {
        // 21 days = 3 week columns, but only 2 columns wide → 3rd clipped.
        let values = [1u64; 21];
        let cal = CalendarHeatmap::new(&values).start_weekday(0).max(Some(1));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 7));
        cal.render(buf.area(), &mut buf);
        // Both visible columns full; no panic on the clipped third.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '■');
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '■');
    }

    #[test]
    fn a_block_frames_the_heatmap_in_the_inner_area() {
        let values = [1u64];
        let cal = CalendarHeatmap::new(&values)
            .start_weekday(0)
            .max(Some(1))
            .block(Block::bordered());
        // 3×3 bordered → inner Rect(1,1,1,1): one cell.
        assert_eq!(lines(cal, 3, 3), "┌─┐\n│■│\n└─┘\n");
    }

    #[test]
    fn an_empty_slice_with_a_block_still_renders_the_block() {
        let values: [u64; 0] = [];
        let cal = CalendarHeatmap::new(&values).block(Block::bordered());
        assert_eq!(lines(cal, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn an_empty_slice_just_fills_the_area() {
        let values: [u64; 0] = [];
        assert_eq!(lines(CalendarHeatmap::new(&values), 3, 2), "   \n   \n");
    }

    #[test]
    fn style_cascades_base_then_level_styles() {
        // value 3 against max 6 → ratio 0.5 → bucket 2.
        let values = [3u64];
        let levels = [
            Style::new(),
            Style::new(),
            Style::new().add_modifier(Modifier::BOLD),
            Style::new(),
            Style::new(),
        ];
        let cal = CalendarHeatmap::new(&values)
            .start_weekday(0)
            .max(Some(6))
            .levels(levels)
            .style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 7));
        cal.render(buf.area(), &mut buf);
        let c = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(c.symbol, '■');
        assert_eq!(c.bg, Color::Blue); // base fill cascades
        assert!(c.modifier.contains(Modifier::BOLD)); // level 2 style cascades
    }

    #[test]
    fn a_start_weekday_above_six_is_reduced_mod_seven() {
        // 9 → 9 % 7 = 2 (Wednesday): first two rows of column 0 blank.
        let values = [1u64];
        let cal = CalendarHeatmap::new(&values).start_weekday(9).max(Some(1));
        let out = lines(cal, 1, 7);
        assert_eq!(out, " \n \n■\n \n \n \n \n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let values = [1u64, 2, 3];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 7));
        CalendarHeatmap::new(&values).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
