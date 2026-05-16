//! [`Calendar`] — a single-month day grid, the date-picker / agenda surface a
//! scheduling TUI pins in a pane.
//!
//! # Dependency-free on purpose: the widget does no date math
//!
//! [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)
//! §4 gates any widget that pulls a transitive dependency behind a Cargo
//! feature. A calendar that computed weekdays would need `chrono`/`time`;
//! `Calendar` instead takes the date facts as **caller-owned inputs** — the
//! `year`, the `month`, the `day_count` of that month, and the weekday index
//! of day 1 — and does **no date arithmetic at all** beyond pure grid layout
//! (where the first cell falls, how the days wrap into weeks). The reducer (or
//! a caller-chosen date crate) supplies the numbers; the widget only places
//! them. So `Calendar` adds no dependency, needs no feature gate, and stays a
//! deterministically headless-testable pure projection exactly like
//! [`List`](crate::List).
//!
//! Weekday indices follow the C `tm_wday` convention — **`0` = Sunday … `6` =
//! Saturday** — used for both [`first_weekday`](Calendar::first_weekday) and
//! the weekday index of day 1. The widget only *rotates a static label table*
//! by that index (layout, not date math). A localized month/weekday label set
//! and multi-month/range views are deliberately deferred additive follow-ups,
//! not smuggled into this slice.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, a `month` outside `1..=12`, a `day_count` over 31, weekday indices
//! outside `0..=6`, and a `selected`/`today` day outside the month are all
//! clamped/ignored — never a panic.

use rstui_core::{Buffer, Position, Rect, Style, Widget};

use crate::block::Block;

/// Full month names, indexed `month - 1`. A static label table is *not* date
/// math (the widget computes no dates — see the [module docs](self)).
const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Two-letter weekday headers, indexed by the `0 = Sunday … 6 = Saturday`
/// convention; rotated by [`first_weekday`](Calendar::first_weekday).
const WEEKDAYS: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

/// Columns per day cell: two digits plus a one-column gutter so adjacent
/// highlighted days never visually merge.
const CELL_W: u16 = 3;

/// A one-month day grid: a header, a weekday row, and up to six week rows, as a
/// pure projection of caller-supplied date facts.
///
/// `Calendar` does **no date math** — it is handed the `year`, `month`,
/// `day_count`, and the weekday index of day 1 (see the [module docs](self))
/// and only lays them out. [`selected`](Self::selected) and
/// [`today`](Self::today) are caller-owned day numbers the widget highlights
/// (selection patched **last**, so it wins when a day is both); an optional
/// [`Block`] frames the grid.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Calendar;
///
/// // May 2026: 31 days, the 1st is a Friday (weekday index 5).
/// let mut buf = Buffer::empty(Rect::new(0, 0, 21, 8));
/// Calendar::new(2026, 5, 31, 5)
///     .selected(Some(17))
///     .render(buf.area(), &mut buf);
///
/// // The header is the month name and year, centred over the 21-wide grid.
/// assert_eq!(buf.get(Position::new(6, 0)).unwrap().symbol, 'M'); // "May 2026"
/// // The 1st (Friday) lands in column 5 of the first week row.
/// assert_eq!(buf.get(Position::new(5 * 3 + 1, 2)).unwrap().symbol, '1');
/// ```
#[derive(Debug, Clone)]
pub struct Calendar<'a> {
    year: i32,
    month: u32,
    day_count: u32,
    weekday_of_first: u32,
    first_weekday: u32,
    selected: Option<u32>,
    today: Option<u32>,
    block: Option<Block<'a>>,
    style: Style,
    header_style: Style,
    weekday_style: Style,
    selected_style: Style,
    today_style: Style,
}

impl<'a> Calendar<'a> {
    /// A calendar for `month` of `year` with `day_count` days, where day 1
    /// falls on weekday `weekday_of_first` (`0 = Sunday … 6 = Saturday`).
    ///
    /// Weeks start on Sunday by default; change that with
    /// [`first_weekday`](Self::first_weekday). Out-of-range inputs are clamped
    /// at render time (see the [module docs](self)).
    pub fn new(year: i32, month: u32, day_count: u32, weekday_of_first: u32) -> Self {
        Self {
            year,
            month,
            day_count,
            weekday_of_first,
            first_weekday: 0,
            selected: None,
            today: None,
            block: None,
            style: Style::default(),
            header_style: Style::default(),
            weekday_style: Style::default(),
            selected_style: Style::default(),
            today_style: Style::default(),
        }
    }

    /// Sets the weekday the week starts on (`0 = Sunday … 6 = Saturday`),
    /// rotating the columns. Reduced mod 7.
    #[must_use]
    pub fn first_weekday(mut self, first_weekday: u32) -> Self {
        self.first_weekday = first_weekday;
        self
    }

    /// Sets the highlighted (selected) day, or `None`. A day outside the month
    /// is ignored. Patched **last**, so it wins over [`today`](Self::today).
    #[must_use]
    pub fn selected(mut self, selected: Option<u32>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the "today" day to accent, or `None`. A day outside the month is
    /// ignored.
    #[must_use]
    pub fn today(mut self, today: Option<u32>) -> Self {
        self.today = today;
        self
    }

    /// Frames the calendar in `block`; the grid renders into
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

    /// Sets the [`Style`] for the month/year header row, over the base.
    #[must_use]
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Sets the [`Style`] for the weekday-label row, over the base.
    #[must_use]
    pub fn weekday_style(mut self, style: Style) -> Self {
        self.weekday_style = style;
        self
    }

    /// Sets the [`Style`] patched over the selected day's cell.
    #[must_use]
    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Sets the [`Style`] patched over the "today" cell.
    #[must_use]
    pub fn today_style(mut self, style: Style) -> Self {
        self.today_style = style;
        self
    }
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

impl Widget for Calendar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Calendar {
            year,
            month,
            day_count,
            weekday_of_first,
            first_weekday,
            selected,
            today,
            block,
            style,
            header_style,
            weekday_style,
            selected_style,
            today_style,
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

        // Clamp every caller input — a pure projection is total.
        let month = month.clamp(1, 12);
        let day_count = day_count.min(31);
        let first_weekday = first_weekday % 7;
        let col_of_first = ((weekday_of_first % 7 + 7 - first_weekday) % 7) as u16;

        let left = inner.left();
        let right = inner.right();
        let bottom = inner.bottom();
        let grid_w = CELL_W * 7;
        let span = grid_w.min(inner.width);

        // Row 0: "<Month> <year>", centred over the grid span.
        let header = format!("{} {}", MONTH_NAMES[(month - 1) as usize], year);
        let hw = (header.chars().count() as u16).min(span);
        put(
            buf,
            &header,
            style.patch(header_style),
            left + (span - hw) / 2,
            inner.top(),
            right,
        );

        // Row 1: weekday labels, rotated so column 0 is `first_weekday`.
        let wd_y = inner.top().saturating_add(1);
        if wd_y < bottom {
            for c in 0..7u16 {
                let label = WEEKDAYS[((first_weekday + u32::from(c)) % 7) as usize];
                put(
                    buf,
                    label,
                    style.patch(weekday_style),
                    left + c * CELL_W,
                    wd_y,
                    right,
                );
            }
        }

        // Rows 2..: the day grid. `col` advances per day and wraps every 7.
        let grid_top = inner.top().saturating_add(2);
        let mut col = col_of_first;
        let mut week = 0u16;
        for day in 1..=day_count {
            let y = grid_top.saturating_add(week);
            if y >= bottom {
                break;
            }
            let cell_x = left + col * CELL_W;
            // The two-digit cell (the gutter column stays the base fill).
            let mut cell_style = style;
            if today == Some(day) {
                cell_style = cell_style.patch(today_style);
            }
            if selected == Some(day) {
                cell_style = cell_style.patch(selected_style);
            }
            for dx in 0..2u16 {
                let x = cell_x.saturating_add(dx);
                if x < right {
                    buf.set_cell(Position::new(x, y), ' ', cell_style);
                }
            }
            // Right-aligned within the two digit columns.
            let text = format!("{day:>2}");
            put(buf, &text, cell_style, cell_x, y, right);

            col += 1;
            if col == 7 {
                col = 0;
                week += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Color;

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
    fn header_is_the_month_name_and_year_centred() {
        // "May 2026" is 8 wide; grid span is 21 → centred at col (21-8)/2 = 6.
        let cal = Calendar::new(2026, 5, 31, 5);
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 1));
        cal.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(6, 0)).unwrap().symbol, 'M');
        assert_eq!(buf.get(Position::new(13, 0)).unwrap().symbol, '6');
    }

    #[test]
    fn weekday_row_starts_on_sunday_by_default() {
        let cal = Calendar::new(2026, 5, 31, 5);
        let out = lines(cal, 21, 2);
        let row1: String = out.lines().nth(1).unwrap().to_string();
        assert_eq!(row1, "Su Mo Tu We Th Fr Sa ");
    }

    #[test]
    fn first_weekday_rotates_the_columns() {
        // Week starting Monday (index 1): Mo first, Su last.
        let cal = Calendar::new(2026, 5, 31, 5).first_weekday(1);
        let out = lines(cal, 21, 2);
        let row1: String = out.lines().nth(1).unwrap().to_string();
        assert_eq!(row1, "Mo Tu We Th Fr Sa Su ");
    }

    #[test]
    fn day_one_lands_in_its_weekday_column() {
        // 1st is Friday (5) with a Sunday-start week → column 5.
        let cal = Calendar::new(2026, 5, 31, 5);
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 3));
        cal.render(buf.area(), &mut buf);
        // Column 5 cell starts at x = 5*3 = 15; "  1" → '1' at x = 16.
        assert_eq!(buf.get(Position::new(16, 2)).unwrap().symbol, '1');
        // Column 0..4 of the first week row are blank (no day there).
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, ' ');
    }

    #[test]
    fn days_wrap_into_week_rows() {
        // Sun-start, 1st on Friday: row0 has 1,2; row1 starts at 3 (Sunday).
        let cal = Calendar::new(2026, 5, 31, 5);
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 8));
        cal.render(buf.area(), &mut buf);
        // Day 2 is column 6 (Saturday) of week row 0 (grid row 2).
        assert_eq!(buf.get(Position::new(6 * 3 + 1, 2)).unwrap().symbol, '2');
        // Day 3 wraps to column 0 of week row 1 (grid row 3).
        assert_eq!(buf.get(Position::new(1, 3)).unwrap().symbol, '3');
    }

    #[test]
    fn selected_day_takes_the_selected_style() {
        let cal = Calendar::new(2026, 5, 31, 5)
            .selected(Some(1))
            .selected_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 3));
        cal.render(buf.area(), &mut buf);
        // The "1" cell (col 5) digit columns carry the selected bg.
        assert_eq!(buf.get(Position::new(15, 2)).unwrap().bg, Color::Blue);
        assert_eq!(buf.get(Position::new(16, 2)).unwrap().bg, Color::Blue);
        // The gutter column stays the base fill.
        assert_eq!(buf.get(Position::new(17, 2)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn selected_wins_over_today_when_a_day_is_both() {
        let cal = Calendar::new(2026, 5, 31, 5)
            .selected(Some(1))
            .today(Some(1))
            .selected_style(Style::new().bg(Color::Blue))
            .today_style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 3));
        cal.render(buf.area(), &mut buf);
        // Selected is patched last → blue, not red.
        assert_eq!(buf.get(Position::new(16, 2)).unwrap().bg, Color::Blue);
    }

    #[test]
    fn today_accents_only_its_own_cell() {
        let cal = Calendar::new(2026, 5, 31, 5)
            .today(Some(2))
            .today_style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 3));
        cal.render(buf.area(), &mut buf);
        // Day 2 is column 6 → cell x = 18.
        assert_eq!(buf.get(Position::new(19, 2)).unwrap().bg, Color::Red);
        // Day 1 (column 5) is untouched.
        assert_eq!(buf.get(Position::new(16, 2)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn an_out_of_range_month_is_clamped() {
        // Month 99 clamps to 12 (December); no panic, no out-of-bounds index.
        let cal = Calendar::new(2026, 99, 31, 0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 1));
        cal.render(buf.area(), &mut buf);
        // "December 2026" (13 wide) centres at col (21-13)/2 = 4.
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, 'D');
    }

    #[test]
    fn an_out_of_range_selected_day_simply_does_not_highlight() {
        let cal = Calendar::new(2026, 5, 31, 5)
            .selected(Some(99))
            .selected_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 8));
        cal.render(buf.area(), &mut buf);
        for cell in buf.cells() {
            assert_ne!(cell.bg, Color::Blue);
        }
    }

    #[test]
    fn a_block_frames_the_calendar_in_the_inner_area() {
        // 23×4 bordered → inner Rect(1,1,21,2): the header on the first inner
        // row, the weekday labels on the second.
        let cal = Calendar::new(2026, 5, 0, 0).block(Block::bordered());
        let mut buf = Buffer::empty(Rect::new(0, 0, 23, 4));
        cal.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(22, 0)).unwrap().symbol, '┐');
        assert_eq!(buf.get(Position::new(22, 3)).unwrap().symbol, '┘');
        // "May 2026" centred in the 21-wide inner starting at x=1 → 1+6 = 7.
        assert_eq!(buf.get(Position::new(7, 1)).unwrap().symbol, 'M');
        // Weekday row inside the frame.
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, 'S');
    }

    #[test]
    fn a_narrow_area_clips_the_grid() {
        // Width 5 clips after the second cell; no panic.
        let cal = Calendar::new(2026, 5, 31, 5);
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 4));
        cal.render(buf.area(), &mut buf);
        // Weekday row clipped: "Su M" then the column-1 cell starts past 5.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'S');
        assert_eq!(buf.get(Position::new(3, 1)).unwrap().symbol, 'M');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 8));
        Calendar::new(2026, 5, 31, 5).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
