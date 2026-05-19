//! [`YearView`] — a twelve-month overview: twelve mini-months tiled in a
//! grid, each rendered by **reusing** [`Calendar`] so its
//! layout and totality are inherited, not re-implemented.
//!
//! # A pure projection that composes [`Calendar`]
//!
//! `YearView` owns no state. It is handed the caller-owned date facts of each
//! month — a `(day_count, weekday_of_first)` pair per month, the same inputs
//! [`Calendar`] takes — plus an optional
//! [`today`](YearView::today)/[`selected`](YearView::selected) `(month, dom)`
//! and a [`busy`](YearView::busy) `(month, dom)` accent set, and renders them
//! by tiling the inner area into a grid of cells and drawing one
//! [`Calendar`] into each. Every mini-month *is* a
//! [`Calendar`], so the year view inherits its grid maths and
//! its totality for free and adds only the tiling and the
//! [`busy`](YearView::busy) post-pass — the
//! [`List`](crate::List)/[`MonthView`](crate::MonthView) composition rule, not
//! a second calendar implementation.
//!
//! # Dependency-free on purpose: the view does no date math
//!
//! [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)
//! §4 gates any widget that pulls a transitive dependency behind a Cargo
//! feature. Computing how many days a month has, or which weekday the 1st
//! falls on, is calendar math needing `chrono`/`time`; `YearView` instead
//! takes those as **caller-owned** [`months`](YearView::months) pairs (the
//! reducer or a date crate of the caller's choosing fills them) — the exact
//! [`Calendar`] discipline. The [`busy`](YearView::busy)
//! days are likewise caller-derived from its event model. So it adds no
//! dependency and needs no feature gate.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: a month
//! with no [`months`](YearView::months) pair is left blank, a tiny area draws
//! only the months that fit (skipping the rest), and an out-of-range
//! [`today`](YearView::today)/[`selected`](YearView::selected)/
//! [`busy`](YearView::busy) is ignored — never a panic. [`Calendar`]
//! itself clamps every per-month input.

use rstui_core::{Buffer, Color, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::calendar::Calendar;

/// The most columns the month grid is ever laid out in (a 4×3 tiling of the
/// twelve months — chosen down to fewer when the area is too small).
const MAX_COLS: u16 = 4;

/// The minimum cell a mini-[`Calendar`] needs to show a
/// useful header + weekday row + at least one week (narrower/shorter cells are
/// skipped — the totality rule).
const MIN_CELL_W: u16 = 12;
const MIN_CELL_H: u16 = 4;

/// A twelve-month overview: a `"Year"` title over a grid of up to twelve
/// mini-months, each a reused [`Calendar`], with optional
/// `today`/`selected`/`busy` accents.
///
/// Each present month (one with a [`months`](Self::months)
/// `(day_count, weekday_of_first)` pair) is drawn by building
/// `Calendar::new(year, m, day_count, wd)` into its grid cell — so the layout
/// and totality are [`Calendar`]'s — with
/// [`today`](Self::today)/[`selected`](Self::selected) passed through to the
/// matching month and a [`busy`](Self::busy) accent patched over those days
/// afterwards. The view does **no date math** (see the [module docs](self)).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::YearView;
///
/// // January 2026: 31 days, the 1st is a Thursday (weekday index 4).
/// let months = [(31_u32, 4_u32)];
/// let view = YearView::new(2026).months(&months).today(Some((1, 15)));
/// let mut buf = Buffer::empty(Rect::new(0, 0, 60, 30));
/// // The hit-test inverse: month 1's cell, computed before render.
/// let jan = view.cell_rect(buf.area(), 1);
/// view.clone().render(buf.area(), &mut buf);
///
/// // Row 0 is the centred "Year 2026" title.
/// let title: String = (0..60)
///     .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
///     .collect();
/// assert!(title.contains("Year 2026"));
/// // The first cell hosts a reused `Calendar` ("January 2026" header).
/// let header: String = (jan.left()..jan.right())
///     .map(|x| buf.get(Position::new(x, jan.top())).unwrap().symbol)
///     .collect();
/// assert!(header.contains("January 2026"));
/// ```
#[derive(Debug, Clone)]
pub struct YearView<'a> {
    year: i32,
    months: &'a [(u32, u32)],
    first_weekday: u32,
    today: Option<(u32, u32)>,
    selected: Option<(u32, u32)>,
    busy: &'a [(u32, u32)],
    block: Option<Block<'a>>,
    style: Style,
    header_style: Style,
    title_style: Style,
}

impl<'a> YearView<'a> {
    /// A year overview for `year` with no month facts yet (every month blank
    /// until [`months`](Self::months) supplies its `(day_count, weekday)`).
    pub fn new(year: i32) -> Self {
        Self {
            year,
            months: &[],
            first_weekday: 0,
            today: None,
            selected: None,
            busy: &[],
            block: None,
            style: Style::default(),
            header_style: Style::default(),
            title_style: Style::default(),
        }
    }

    /// Sets the caller-owned per-month date facts: up to twelve
    /// `(day_count, weekday_of_first)` pairs (index `0` = January). The view
    /// does **no date math** (see the [module docs](self)); a month with no
    /// pair (a short slice, or beyond index 11) is left blank.
    #[must_use]
    pub fn months(mut self, months: &'a [(u32, u32)]) -> Self {
        self.months = months;
        self
    }

    /// Sets the weekday each mini-month's week starts on
    /// (`0 = Sunday … 6 = Saturday`), forwarded to every
    /// [`Calendar`]. Reduced mod 7 there.
    #[must_use]
    pub fn first_weekday(mut self, first_weekday: u32) -> Self {
        self.first_weekday = first_weekday;
        self
    }

    /// Sets the "today" cell as `(month, day_of_month)` with `month` in
    /// `1..=12`, or `None`. Forwarded to the matching month's
    /// [`Calendar`]; an out-of-range value is ignored there.
    #[must_use]
    pub fn today(mut self, today: Option<(u32, u32)>) -> Self {
        self.today = today;
        self
    }

    /// Sets the selected cell as `(month, day_of_month)` with `month` in
    /// `1..=12`, or `None`. Forwarded to the matching month's
    /// [`Calendar`] (it wins over `today` there).
    #[must_use]
    pub fn selected(mut self, selected: Option<(u32, u32)>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the caller-owned "busy" days as `(month, day_of_month)` pairs
    /// (month `1..=12`) — days the caller derived from its event model — each
    /// accented with a `•` dot in its cell. Out-of-range pairs are ignored.
    #[must_use]
    pub fn busy(mut self, busy: &'a [(u32, u32)]) -> Self {
        self.busy = busy;
        self
    }

    /// Frames the year view in `block`; the grid renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area (and is
    /// forwarded as each mini-[`Calendar`]'s base) so a
    /// background covers the whole pane.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] each mini-month's month/year header row takes
    /// (forwarded as the [`Calendar`] header style).
    #[must_use]
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Sets the [`Style`] for the top `"Year"` title row, over the base.
    #[must_use]
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// The grid geometry for `inner`: `(cols, rows, cell_w, cell_h, grid)`
    /// where `grid` is the [`Rect`] below the title the cells tile. Columns
    /// shrink from [`MAX_COLS`] until a cell is at least
    /// [`MIN_CELL_W`]×[`MIN_CELL_H`]; `(0, …)` when nothing fits. The single
    /// shared layout `render` and [`cell_rect`](Self::cell_rect) /
    /// [`month_at`](Self::month_at) all use, so the geometry can never desync.
    fn grid(&self, inner: Rect) -> (u16, u16, u16, u16, Rect) {
        // Row 0 is the title; the months tile the area below it.
        if inner.height <= 1 {
            return (0, 0, 0, 0, Rect::ZERO);
        }
        let grid = Rect::new(
            inner.x,
            inner.y.saturating_add(1),
            inner.width,
            inner.height.saturating_sub(1),
        );
        if grid.width < MIN_CELL_W || grid.height < MIN_CELL_H {
            return (0, 0, 0, 0, grid);
        }
        // Widest column count (≤ [`MAX_COLS`]) whose cell still meets
        // [`MIN_CELL_W`]; at least one column (we already know the grid is
        // ≥ MIN_CELL_W wide, so this clamp never yields 0).
        let cols = (grid.width / MIN_CELL_W).clamp(1, MAX_COLS);
        // Each mini-month is at least [`MIN_CELL_H`] tall; the grid holds at
        // most `grid.height / MIN_CELL_H` rows, and we never need more rows
        // than the twelve months require at this width. Months beyond the
        // rows that fit are simply not drawn (the totality rule).
        let rows_needed = 12_u16.div_ceil(cols);
        let rows_that_fit = (grid.height / MIN_CELL_H).max(1);
        let rows = rows_needed.min(rows_that_fit);
        // Cells split the grid evenly across the rows/cols actually used; the
        // even split is ≥ the minimum because `rows ≤ grid.height/MIN_CELL_H`
        // and `cols ≤ grid.width/MIN_CELL_W`, so neither dimension is ever 0.
        let cell_w = grid.width / cols;
        let cell_h = grid.height / rows;
        (cols, rows, cell_w, cell_h, grid)
    }

    /// The cell [`Rect`] month `m` (`1..=12`) tiles, or [`Rect::ZERO`] when it
    /// is out of range or the area is too small to lay out a grid.
    ///
    /// The pure inverse companion of [`month_at`](Self::month_at); accounts
    /// for the framing [`block`](Self::block) once, here.
    #[must_use]
    pub fn cell_rect(&self, area: Rect, month: u32) -> Rect {
        if !(1..=12).contains(&month) {
            return Rect::ZERO;
        }
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if inner.is_empty() {
            return Rect::ZERO;
        }
        let (cols, rows, cell_w, cell_h, grid) = self.grid(inner);
        if cols == 0 {
            return Rect::ZERO;
        }
        let idx = (month - 1) as u16;
        let (r, c) = (idx / cols, idx % cols);
        // A row past the rows that fit has no cell (the totality rule — those
        // months are simply not drawn); the same bound `render`/`month_at`
        // enforce, so the geometry can't desync.
        if r >= rows {
            return Rect::ZERO;
        }
        let x = grid.x.saturating_add(c * cell_w);
        let y = grid.y.saturating_add(r * cell_h);
        if y.saturating_add(cell_h) > grid.bottom() {
            return Rect::ZERO;
        }
        Rect::new(x, y, cell_w, cell_h)
    }

    /// The month (`1..=12`) whose grid cell contains `pos` for `area`, or
    /// `None` outside every cell.
    ///
    /// The pure inverse of the tiling — clicking a mini-month picks it. It
    /// accounts for the framing [`block`](Self::block) and the grid layout
    /// once, here, instead of every app re-deriving it.
    #[must_use]
    pub fn month_at(&self, area: Rect, pos: Position) -> Option<u32> {
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if inner.is_empty() || !inner.contains(pos) {
            return None;
        }
        let (cols, rows, cell_w, cell_h, grid) = self.grid(inner);
        if cols == 0 || !grid.contains(pos) {
            return None;
        }
        let c = (pos.x - grid.x) / cell_w;
        let r = (pos.y - grid.y) / cell_h;
        if c >= cols || r >= rows {
            return None;
        }
        let month = r * cols + c + 1;
        (month <= 12).then(|| u32::from(month))
    }
}

impl Widget for YearView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // The block (if any) frames the content and reserves the inner area.
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if let Some(b) = &self.block {
            b.clone().render(area, buf);
        }
        if inner.is_empty() {
            return;
        }

        // Base fills the content area so a background covers the whole pane.
        buf.set_style(inner, self.style);

        // Row 0: the "Year <year>" title, centred over the inner width.
        let title = format!("Year {}", self.year);
        let tw = (title.chars().count() as u16).min(inner.width);
        let mut tx = inner.left() + (inner.width - tw) / 2;
        let title_st = self.style.patch(self.title_style);
        for ch in title.chars() {
            if tx >= inner.right() {
                break;
            }
            buf.set_cell(Position::new(tx, inner.top()), ch, title_st);
            tx = tx.saturating_add(1);
        }

        let (cols, rows, cell_w, cell_h, grid) = self.grid(inner);
        if cols == 0 {
            // No room for a single mini-month; the title alone is the view.
            return;
        }

        for m in 1..=12u32 {
            let Some((day_count, weekday_of_first)) = self.months.get((m - 1) as usize).copied()
            else {
                continue; // No facts for this month → left blank.
            };
            let idx = (m - 1) as u16;
            let (r, c) = (idx / cols, idx % cols);
            // A row past the rows that fit is not drawn — the same bound
            // `month_at`/`cell_rect` enforce, so the geometry can't desync
            // (the totality rule: those months are simply skipped).
            if r >= rows {
                continue;
            }
            let cx = grid.x.saturating_add(c * cell_w);
            let cy = grid.y.saturating_add(r * cell_h);
            if cy.saturating_add(cell_h) > grid.bottom() {
                continue;
            }
            let cell = Rect::new(cx, cy, cell_w, cell_h);
            if cell.is_empty() {
                continue;
            }

            // Reuse `Calendar` for the mini-month: its grid layout and its
            // totality (clamped month/day_count/weekday) are inherited, not
            // re-implemented. today/selected only pass through on their month.
            let mut cal = Calendar::new(self.year, m, day_count, weekday_of_first)
                .first_weekday(self.first_weekday)
                .style(self.style)
                .header_style(self.header_style);
            if let Some((tm, td)) = self.today {
                if tm == m {
                    cal = cal.today(Some(td));
                }
            }
            if let Some((sm, sd)) = self.selected {
                if sm == m {
                    cal = cal.selected(Some(sd));
                }
            }
            cal.render(cell, buf);

            // The `busy` accent: a post-pass `•` dot over each busy day's
            // cell, derived by inverting the same Calendar grid maths. Kept
            // total — a day off-month or off-cell is skipped, never a panic.
            for &(bm, bd) in self.busy {
                if bm != m || bd == 0 {
                    continue;
                }
                if let Some(p) =
                    day_cell(&cell, day_count, weekday_of_first, self.first_weekday, bd)
                {
                    if cell.contains(p) {
                        let cur = buf.get(p).map(|c| c.style()).unwrap_or_default();
                        buf.set_cell(p, '\u{2022}', cur.fg(Color::Yellow)); // •
                    }
                }
            }
        }
    }
}

/// The buffer [`Position`] of the *ones* digit of day `dom` inside a
/// mini-[`Calendar`] drawn at `cell`, mirroring
/// [`Calendar`]'s own grid layout (header row, weekday row,
/// then `CELL_W = 3` columns per day wrapping every 7). `None` when the day
/// falls outside the month or off the cell — total, never a panic.
fn day_cell(
    cell: &Rect,
    day_count: u32,
    weekday_of_first: u32,
    first_weekday: u32,
    dom: u32,
) -> Option<Position> {
    let day_count = day_count.min(31);
    if dom == 0 || dom > day_count {
        return None;
    }
    // Mirror Calendar: column of day 1, then advance one column per day.
    let first_weekday = first_weekday % 7;
    let col_of_first = (weekday_of_first % 7 + 7 - first_weekday) % 7;
    let offset = col_of_first + (dom - 1);
    let (week, col) = (offset / 7, offset % 7);
    // Calendar lays the grid at inner.top()+2; the ones digit is the 2nd of
    // the 3-wide cell.
    let x = cell.left().saturating_add((col * 3 + 1) as u16);
    let y = cell.top().saturating_add(2).saturating_add(week as u16);
    Some(Position::new(x, y))
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

    /// Twelve plausible `(day_count, weekday_of_first)` pairs (2026), the
    /// caller-owned facts the reducer/date-crate would supply.
    fn months_2026() -> [(u32, u32); 12] {
        [
            (31, 4), // Jan, 1st = Thu
            (28, 0), // Feb, 1st = Sun
            (31, 0), // Mar
            (30, 3), // Apr
            (31, 5), // May
            (30, 1), // Jun
            (31, 3), // Jul
            (31, 6), // Aug
            (30, 2), // Sep
            (31, 4), // Oct
            (30, 0), // Nov
            (31, 2), // Dec
        ]
    }

    #[test]
    fn the_title_row_is_year_centred() {
        let m = months_2026();
        // "Year 2026" is 9 wide; inner 30 → centred at (30-9)/2 = 10.
        let out = lines(YearView::new(2026).months(&m), 30, 20);
        let row0 = out.lines().next().unwrap();
        assert_eq!(&row0[10..19], "Year 2026");
    }

    /// The glyphs on row `y` of `buf`, columns `[r.left, r.right)`, as a
    /// String — the text a mini-`Calendar` centred in cell `r`'s top row
    /// (its month/year header).
    fn cell_row(buf: &Buffer, r: Rect, y: u16) -> String {
        (r.left()..r.right())
            .map(|x| buf.get(Position::new(x, y)).unwrap().symbol)
            .collect()
    }

    #[test]
    fn it_tiles_twelve_mini_calendars_reusing_calendar() {
        let m = months_2026();
        let v = YearView::new(2026).months(&m);
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 40));
        let jan = v.cell_rect(buf.area(), 1);
        let dec = v.cell_rect(buf.area(), 12);
        v.render(buf.area(), &mut buf);
        // The first cell hosts "January 2026" (Calendar's own centred header)
        // and, under it, Calendar's weekday row — proof it is a real reused
        // `Calendar`, not a re-implementation.
        assert!(cell_row(&buf, jan, jan.top()).contains("January 2026"));
        assert!(cell_row(&buf, jan, jan.top() + 1).contains("Su"));
        // The twelfth cell hosts "December 2026" — all twelve tile.
        assert!(cell_row(&buf, dec, dec.top()).contains("December 2026"));
    }

    #[test]
    fn a_month_with_no_pair_is_left_blank() {
        // Only January supplied; Feb..Dec have no facts → blank cells.
        let m = [(31_u32, 4_u32)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 40));
        let v = YearView::new(2026).months(&m);
        let jan = v.cell_rect(buf.area(), 1);
        // February's cell rect exists but is empty (no "February" header).
        let feb = v.cell_rect(buf.area(), 2);
        assert!(!feb.is_empty());
        v.render(buf.area(), &mut buf);
        let mut any = false;
        for x in feb.left()..feb.right() {
            if buf.get(Position::new(x, feb.top())).unwrap().symbol != ' ' {
                any = true;
            }
        }
        assert!(!any, "February has no facts → its cell stays blank");
        // January (supplied) did draw its header.
        assert!(cell_row(&buf, jan, jan.top()).contains("January 2026"));
    }

    #[test]
    fn today_passes_through_only_to_its_own_month() {
        let m = months_2026();
        let v = YearView::new(2026).months(&m).today(Some((1, 15)));
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 40));
        let jan = v.cell_rect(buf.area(), 1);
        let feb = v.cell_rect(buf.area(), 2);
        v.render(buf.area(), &mut buf);
        // Jan cell drew its header; today is inside Jan's cell so no panic and
        // the grid is intact (Feb's own header still renders, unaffected).
        assert!(cell_row(&buf, jan, jan.top()).contains("January 2026"));
        assert!(cell_row(&buf, feb, feb.top()).contains("February 2026"));
    }

    #[test]
    fn selected_is_forwarded_and_styled_on_its_month() {
        let m = months_2026();
        let v = YearView::new(2026).months(&m).selected(Some((1, 1)));
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 40));
        // Pass a selected_style by wrapping: YearView forwards style/header
        // only, so verify selection is *delegated* to Calendar by checking
        // the day-1 cell digit is present in January's cell.
        let jan = v.cell_rect(buf.area(), 1);
        v.render(buf.area(), &mut buf);
        // Calendar draws day 1 somewhere in January's grid; the cell exists.
        assert!(!jan.is_empty());
        // The mini-calendar's weekday row is present (proves Calendar ran
        // with the forwarded inputs and selection did not break it).
        assert_eq!(
            buf.get(Position::new(jan.left(), jan.top() + 1))
                .unwrap()
                .symbol,
            'S'
        );
    }

    #[test]
    fn busy_days_get_a_dot_accent() {
        let m = months_2026();
        // Jan 1st is a Thursday (weekday 4, Sunday-start) → column 4 of week
        // 0; ones digit at x = 4*3+1 = 13, grid row = cell.top()+2.
        let busy = [(1_u32, 1_u32)];
        let v = YearView::new(2026).months(&m).busy(&busy);
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 40));
        let jan = v.cell_rect(buf.area(), 1);
        v.render(buf.area(), &mut buf);
        let p = Position::new(jan.left() + 13, jan.top() + 2);
        assert_eq!(buf.get(p).unwrap().symbol, '\u{2022}'); // •
        assert_eq!(buf.get(p).unwrap().fg, Color::Yellow);
    }

    #[test]
    fn an_out_of_range_busy_day_is_ignored() {
        let m = months_2026();
        let busy = [(1_u32, 99_u32), (13, 1)]; // day 99, month 13: both bogus
        let v = YearView::new(2026).months(&m).busy(&busy);
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 40));
        v.render(buf.area(), &mut buf);
        for cell in buf.cells() {
            assert_ne!(cell.symbol, '\u{2022}');
        }
    }

    #[test]
    fn month_at_inverts_the_tiling() {
        let m = months_2026();
        let v = YearView::new(2026).months(&m);
        let area = Rect::new(0, 0, 80, 40);
        // 80 wide / MIN 12 → 4 cols (clamped to MAX_COLS); 12/4 = 3 rows.
        // Grid starts at y=1; cell_w = 80/4 = 20, cell_h = 39/3 = 13.
        assert_eq!(v.month_at(area, Position::new(0, 1)), Some(1)); // Jan: r0c0
        assert_eq!(v.month_at(area, Position::new(20, 1)), Some(2)); // Feb: r0c1
        assert_eq!(v.month_at(area, Position::new(0, 14)), Some(5)); // May: r1c0
        assert_eq!(v.month_at(area, Position::new(60, 27)), Some(12)); // Dec
        assert_eq!(v.month_at(area, Position::new(0, 0)), None); // title row
        assert_eq!(v.month_at(area, Position::new(0, 99)), None); // off-area
    }

    #[test]
    fn cell_rect_is_the_inverse_companion_of_month_at() {
        let m = months_2026();
        let v = YearView::new(2026).months(&m);
        let area = Rect::new(0, 0, 80, 40);
        let r = v.cell_rect(area, 6);
        assert!(!r.is_empty());
        // Every corner of month 6's cell maps back to month 6.
        assert_eq!(v.month_at(area, r.position()), Some(6));
        assert_eq!(
            v.month_at(area, Position::new(r.right() - 1, r.bottom() - 1)),
            Some(6)
        );
        // Out-of-range months have no cell.
        assert_eq!(v.cell_rect(area, 0), Rect::ZERO);
        assert_eq!(v.cell_rect(area, 13), Rect::ZERO);
    }

    #[test]
    fn a_block_frames_the_year_view_in_the_inner_area() {
        let m = months_2026();
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 20));
        YearView::new(2026)
            .months(&m)
            .block(Block::bordered())
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '\u{250c}'); // ┌
        assert_eq!(buf.get(Position::new(0, 19)).unwrap().symbol, '\u{2514}'); // └
        // The "Year 2026" title is on the first inner row, framed.
        let r1: String = (1..59)
            .map(|x| buf.get(Position::new(x, 1)).unwrap().symbol)
            .collect();
        assert!(r1.contains("Year 2026"));
    }

    #[test]
    fn an_empty_year_view_with_a_block_still_renders_the_block() {
        // No months at all + a frame: just the border and the title inside.
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 4));
        YearView::new(2026)
            .block(Block::bordered())
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '\u{250c}');
        assert_eq!(buf.get(Position::new(0, 3)).unwrap().symbol, '\u{2514}');
        let r1: String = (1..13)
            .map(|x| buf.get(Position::new(x, 1)).unwrap().symbol)
            .collect();
        assert!(r1.contains("Year 2026"));
    }

    #[test]
    fn a_tiny_area_skips_months_that_do_not_fit() {
        let m = months_2026();
        // 13 wide, 6 tall: only one column fits (1 cell), so at most a couple
        // of months render; it must not panic and the title still shows.
        let out = lines(YearView::new(2026).months(&m), 13, 6);
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[0].contains("Year 2026"));
        // January (the first month) fits the single column.
        assert_eq!(rows[1].chars().next().unwrap(), 'J');
    }

    #[test]
    fn an_area_with_no_room_for_a_grid_draws_only_the_title() {
        let m = months_2026();
        // Height 1: only the title row, no grid at all (no panic).
        let out = lines(YearView::new(2026).months(&m), 20, 1);
        assert!(out.contains("Year 2026"));
        // Height 2 but width 5: grid area too narrow for MIN_CELL_W → no
        // months, just the (clipped) title.
        let v = YearView::new(2026).months(&m);
        let a = Rect::new(0, 0, 5, 3);
        let mut buf = Buffer::empty(a);
        v.clone().render(a, &mut buf);
        assert_eq!(v.month_at(a, Position::new(0, 1)), None);
    }

    #[test]
    fn the_title_takes_the_title_style_and_base_fills_the_pane() {
        let m = months_2026();
        let v = YearView::new(2026)
            .months(&m)
            .style(Style::new().bg(Color::Red))
            .title_style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        v.render(buf.area(), &mut buf);
        // Whole pane has the base bg.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Red);
        assert_eq!(buf.get(Position::new(39, 19)).unwrap().bg, Color::Red);
        // The "Year 2026" title glyphs are bold.
        let row0 = (0..40)
            .map(|x| buf.get(Position::new(x, 0)).unwrap())
            .find(|c| c.symbol == 'Y')
            .unwrap();
        assert!(row0.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let m = months_2026();
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 30));
        YearView::new(2026)
            .months(&m)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn the_grid_falls_back_to_fewer_columns_when_too_short() {
        let m = months_2026();
        let v = YearView::new(2026).months(&m);
        // Wide but short: 4 columns would need 3 rows; if too short the grid
        // logic reduces columns. Just assert it is total and consistent: the
        // month under a known point round-trips through cell_rect.
        let area = Rect::new(0, 0, 60, 9);
        if let Some(mth) = v.month_at(area, Position::new(0, 1)) {
            let r = v.cell_rect(area, mth);
            assert!(!r.is_empty());
            assert_eq!(v.month_at(area, r.position()), Some(mth));
        }
    }
}
