//! [`MonthView`] — a full month grid where every day cell carries its event
//! chips, multi-day and all-day events stretch as continuous spanning bars
//! across the day columns they cover, and a "+N more" footer absorbs the
//! overflow: the events-bearing month surface a scheduling TUI pins in a pane,
//! the richer sibling of the date-only [`Calendar`](crate::Calendar).
//!
//! # A pure projection, like every other widget
//!
//! `MonthView` owns no state. The grid skeleton is caller-owned date facts —
//! the `year`, the `month`, the `day_count` of that month, and the weekday
//! index of day-of-month 1, exactly the [`Calendar`](crate::Calendar) inputs —
//! and the contents are a borrowed caller-owned `&[CalendarEvent]`, projected
//! the way [`Markdown`](crate::Markdown) projects a borrowed `&[Link]`. The
//! [`selected`](MonthView::selected)/[`today`](MonthView::today) days are
//! caller-owned day-of-month numbers the widget only highlights (selection
//! patched **last**, so it wins when a day is both).
//!
//! # Dependency-free on purpose: the widget does no date math
//!
//! [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)
//! §4 gates any widget that pulls a transitive dependency behind a Cargo
//! feature. A month grid that computed weekdays would need `chrono`/`time`;
//! `MonthView` instead takes the date facts as **caller-owned inputs** and does
//! **no date arithmetic at all** beyond pure grid layout. The cell of
//! day-of-month `dom` sits on the caller's integer day axis at
//! [`first_day`](MonthView::first_day)`+ (dom - 1)` — the same opaque `i64`
//! axis [`CalendarEvent`] and [`Gantt`](crate::Gantt) use, never interpreted as
//! a date — and an event shows in that cell exactly when
//! [`CalendarEvent::covers_day`] is true for the cell's axis day. Day numbers
//! are placed (it rotates the *static* month/weekday label tables it shares
//! with [`Calendar`](crate::Calendar) by the caller's weekday index — layout,
//! not date math), never computed. So `MonthView` adds no dependency, needs no
//! feature gate, and stays a deterministically headless-testable pure
//! projection exactly like [`List`](crate::List). Weekday indices follow the C
//! `tm_wday` convention — **`0` = Sunday … `6` = Saturday**.
//!
//! # Derived geometry is itself a projection
//!
//! [`day_at`](MonthView::day_at) and [`event_at`](MonthView::event_at) are pure
//! functions of the area plus the same caller-owned config the render reads, so
//! a click maps back through the *identical* cell/chip arithmetic the paint
//! used — hit-testing and painting cannot drift because they are the one
//! geometry, read twice (the [`Calendar`](crate::Calendar) discipline extended
//! to the events surface).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, a `month` outside `1..=12`, a `day_count` over 31, weekday indices
//! outside `0..=6`, a `selected`/`today` day outside the month, an area too
//! narrow or short for the grid, and out-of-range or back-to-front events are
//! all clamped/clipped/ignored — never a panic.

use std::borrow::Cow;

use rstui_core::{Buffer, Color, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::event::CalendarEvent;

/// Full month names, indexed `month - 1`. A static label table is *not* date
/// math (the widget computes no dates — see the [module docs](self)); shared
/// verbatim with [`Calendar`](crate::Calendar).
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
/// convention; rotated by [`first_weekday`](MonthView::first_weekday).
const WEEKDAYS: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

/// A full month grid: a centred header, a rotated weekday row, and up to six
/// week rows of day cells, every cell carrying its event chips, with multi-day
/// and all-day events drawn as continuous spanning bars across the day columns
/// they cover and a "+N more" footer for the overflow — a pure projection of
/// caller-supplied date facts and a borrowed `&[CalendarEvent]`.
///
/// `MonthView` does **no date math** — it is handed the `year`, `month`,
/// `day_count`, and the weekday index of day 1 (see the [module docs](self))
/// and only lays them out; the cell of day-of-month `dom` is the caller-axis
/// day [`first_day`](Self::first_day)`+ (dom - 1)`, and an event shows when
/// [`CalendarEvent::covers_day`] holds for it. [`selected`](Self::selected) and
/// [`today`](Self::today) are caller-owned day-of-month numbers the widget
/// highlights (selection patched **last**, so it wins when a day is both); an
/// optional [`Block`] frames the grid.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{CalendarEvent, MonthView};
///
/// // May 2026: 31 days, the 1st is a Friday (weekday index 5). One event on
/// // the 1st on the caller's day axis (day-of-month 1 == axis day 1 here).
/// let events = [CalendarEvent::new(1, "Launch").with_day(1).with_all_day(true)];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 35, 12));
/// MonthView::new(2026, 5, 31, 5)
///     .events(&events)
///     .selected(Some(1))
///     .render(buf.area(), &mut buf);
///
/// // The header is the month name and year, centred over the 35-wide grid.
/// assert_eq!(buf.get(Position::new(13, 0)).unwrap().symbol, 'M'); // "May 2026"
/// // Day-of-month 1 (Friday) is reported by the hit-test for column 5's cell.
/// let cell_w = 35 / 7;
/// let x = 5 * cell_w + 1;
/// assert_eq!(buf.get(Position::new(x, 2)).unwrap().symbol, '1');
/// ```
#[derive(Debug, Clone)]
pub struct MonthView<'a> {
    year: i32,
    month: u32,
    day_count: u32,
    weekday_of_first: u32,
    first_weekday: u32,
    first_day: i64,
    events: &'a [CalendarEvent<'a>],
    selected: Option<u32>,
    today: Option<u32>,
    max_chips: Option<u16>,
    block: Option<Block<'a>>,
    style: Style,
    header_style: Style,
    weekday_style: Style,
    selected_style: Style,
    today_style: Style,
    grid_style: Style,
}

impl<'a> MonthView<'a> {
    /// A month grid for `month` of `year` with `day_count` days, where
    /// day-of-month 1 falls on weekday `weekday_of_first` (`0 = Sunday … 6 =
    /// Saturday`).
    ///
    /// Weeks start on Sunday by default; change that with
    /// [`first_weekday`](Self::first_weekday). The events slice is empty until
    /// [`events`](Self::events) is set, and day-of-month 1 maps to caller-axis
    /// day `1` until [`first_day`](Self::first_day) is set. Out-of-range inputs
    /// are clamped at render time (see the [module docs](self)).
    pub fn new(year: i32, month: u32, day_count: u32, weekday_of_first: u32) -> Self {
        Self {
            year,
            month,
            day_count,
            weekday_of_first,
            first_weekday: 0,
            first_day: 1,
            events: &[],
            selected: None,
            today: None,
            max_chips: None,
            block: None,
            style: Style::default(),
            header_style: Style::default(),
            weekday_style: Style::default(),
            selected_style: Style::default(),
            today_style: Style::default(),
            grid_style: Style::default(),
        }
    }

    /// Sets the borrowed, caller-owned events the grid projects. An event shows
    /// in a day cell when [`CalendarEvent::covers_day`] holds for that cell's
    /// caller-axis day (see [`first_day`](Self::first_day)).
    #[must_use]
    pub fn events(mut self, events: &'a [CalendarEvent<'a>]) -> Self {
        self.events = events;
        self
    }

    /// Sets the caller-axis day of **day-of-month 1**; cell `dom` is then the
    /// axis day `first_day + (dom - 1)`. Defaults to `1`. The widget never
    /// interprets the unit — it is the same opaque integer day axis
    /// [`CalendarEvent::day`] uses (see the [module docs](self)).
    #[must_use]
    pub fn first_day(mut self, first_day: i64) -> Self {
        self.first_day = first_day;
        self
    }

    /// Sets the weekday the week starts on (`0 = Sunday … 6 = Saturday`),
    /// rotating the columns. Reduced mod 7.
    #[must_use]
    pub fn first_weekday(mut self, first_weekday: u32) -> Self {
        self.first_weekday = first_weekday;
        self
    }

    /// Sets the highlighted (selected) day-of-month, or `None`. A day outside
    /// the month is ignored. Patched **last**, so it wins over
    /// [`today`](Self::today).
    #[must_use]
    pub fn selected(mut self, selected: Option<u32>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the "today" day-of-month to accent, or `None`. A day outside the
    /// month is ignored.
    #[must_use]
    pub fn today(mut self, today: Option<u32>) -> Self {
        self.today = today;
        self
    }

    /// Sets the number of event chips a cell shows before collapsing the rest
    /// into a "+N more" footer line. `None` (the default) derives it from the
    /// cell height (every row below the day number, less one for the footer).
    #[must_use]
    pub fn max_chips(mut self, max_chips: u16) -> Self {
        self.max_chips = Some(max_chips);
        self
    }

    /// Frames the grid in `block`; the month renders into
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

    /// Sets the [`Style`] for the grid rules and the day-number gutter, over
    /// the base.
    #[must_use]
    pub fn grid_style(mut self, style: Style) -> Self {
        self.grid_style = style;
        self
    }

    /// The clamped `(month, day_count, first_weekday, col_of_first)` — the
    /// single source of truth render and the hit-tests all derive from, so the
    /// geometry can never disagree (see the [module docs](self)).
    fn skeleton(&self) -> (u32, u32, u32, u16) {
        let month = self.month.clamp(1, 12);
        let day_count = self.day_count.min(31);
        let first_weekday = self.first_weekday % 7;
        let col_of_first = ((self.weekday_of_first % 7 + 7 - first_weekday) % 7) as u16;
        (month, day_count, first_weekday, col_of_first)
    }

    /// The grid's inner area, accounting for an optional [`Block`]. Render and
    /// the hit-tests share this so the cell maths line up exactly.
    fn grid_area(&self, area: Rect) -> Rect {
        match &self.block {
            Some(b) => b.inner(area),
            None => area,
        }
    }

    /// The `(week, col)` of day-of-month `dom` (`1..=day_count`) on the rotated
    /// grid — the same wrap render walks.
    fn cell_of(dom: u32, col_of_first: u16) -> (u16, u16) {
        let idx = (dom - 1) as u16 + col_of_first;
        (idx / 7, idx % 7)
    }

    /// The day-of-month under `pos`, or `None` when `pos` is outside the month
    /// grid (header, weekday row, a padding cell, or off-pane). A pure function
    /// of the area and the caller-owned config — the render's geometry read
    /// back (see the [module docs](self)).
    #[must_use]
    pub fn day_at(&self, area: Rect, pos: Position) -> Option<u32> {
        if area.is_empty() {
            return None;
        }
        let inner = self.grid_area(area);
        if inner.is_empty() {
            return None;
        }
        let (_, day_count, _, col_of_first) = self.skeleton();
        let geo = Geometry::new(inner)?;

        if pos.x < inner.left() || pos.x >= inner.left() + geo.grid_w {
            return None;
        }
        if pos.y < geo.grid_top || pos.y >= geo.grid_top + geo.grid_h {
            return None;
        }
        let col = (pos.x - inner.left()) / geo.cell_w;
        let week = (pos.y - geo.grid_top) / geo.cell_h;
        if col >= 7 {
            return None;
        }
        let idx = week as i64 * 7 + col as i64 - col_of_first as i64;
        let dom = idx + 1;
        if dom >= 1 && dom <= day_count as i64 {
            Some(dom as u32)
        } else {
            None
        }
    }

    /// The [`CalendarEvent::id`] of the chip or spanning bar under `pos`, or
    /// `None`. Resolves through the *same* per-cell row layout the render
    /// stamps — a click and the paint share one geometry, so they cannot
    /// disagree (see the [module docs](self)).
    #[must_use]
    pub fn event_at(&self, area: Rect, pos: Position) -> Option<u64> {
        let dom = self.day_at(area, pos)?;
        let inner = self.grid_area(area);
        let (_, _, _, col_of_first) = self.skeleton();
        let geo = Geometry::new(inner)?;
        let (week, col) = Self::cell_of(dom, col_of_first);

        let cell_x = inner.left() + col * geo.cell_w;
        let cell_y = geo.grid_top + week * geo.cell_h;
        // Row 0 of a cell is the day number; chips start one row below.
        let rel = pos.y.checked_sub(cell_y)?;
        if rel == 0 {
            return None;
        }
        let chip_row = usize::from(rel - 1);
        let axis_day = self.first_day + i64::from(dom) - 1;
        let (rows, _hidden) = self.cell_chips(axis_day, geo.cell_h);

        if chip_row < rows.len() {
            // A chip occupies its whole row inside the cell's text columns.
            if pos.x >= cell_x && pos.x < cell_x + geo.cell_w {
                return Some(rows[chip_row].id());
            }
        }
        None
    }

    /// The events covering caller-axis `axis_day`, in stable input order — the
    /// order render lays chips out in.
    fn chips_for(&self, axis_day: i64) -> Vec<&CalendarEvent<'a>> {
        self.events
            .iter()
            .filter(|e| e.covers_day(axis_day))
            .collect()
    }

    /// The single source of truth for a day cell's stacked rows: the events
    /// shown as rows below the day number, **spanning/all-day events first**
    /// (the rows the per-week bars occupy) then single-day chips, truncated to
    /// the row budget, plus the count hidden behind the "+N more" footer.
    ///
    /// Render and [`event_at`](Self::event_at) both resolve through this so a
    /// click maps to exactly the chip the paint drew (see the
    /// [module docs](self)).
    fn cell_chips(&self, axis_day: i64, cell_h: u16) -> (Vec<&CalendarEvent<'a>>, usize) {
        let day_events = self.chips_for(axis_day);
        let (spanning, single): (Vec<_>, Vec<_>) = day_events
            .into_iter()
            .partition(|e| e.multi_day() || e.all_day());

        let max_chips = self.max_chips_for(cell_h);
        // Rows below the day number available for chips + an optional footer.
        let body_rows = cell_h.saturating_sub(1);
        let total = spanning.len() + single.len();
        let chip_cap = max_chips.min(body_rows) as usize;
        // A footer is needed when not every event fits; it then costs one row.
        let overflow = total > chip_cap;
        let chip_rows = if overflow {
            usize::from(body_rows.saturating_sub(1).min(max_chips))
        } else {
            chip_cap
        };

        let rows: Vec<&CalendarEvent<'a>> =
            spanning.into_iter().chain(single).take(chip_rows).collect();
        let hidden = total.saturating_sub(rows.len());
        (rows, hidden)
    }

    /// Chips per cell before the "+N more" footer: the caller's
    /// [`max_chips`](Self::max_chips), or every row below the day number less
    /// the footer row, at least one.
    fn max_chips_for(&self, cell_h: u16) -> u16 {
        match self.max_chips {
            Some(n) => n.max(1),
            // Rows: [day number][chip…][+N more]. Reserve 1 for the number and
            // 1 for the footer; the rest are chips (at least one).
            None => cell_h.saturating_sub(2).max(1),
        }
    }
}

/// The fixed cell grid of a [`MonthView`]'s inner area. Computed once and read
/// by both the render and the hit-tests so they cannot drift (see the
/// [`MonthView`] module docs).
#[derive(Debug, Clone, Copy)]
struct Geometry {
    /// Top row of the first week of day cells (below header + weekday rows).
    grid_top: u16,
    /// Columns per day cell (`>= 1`).
    cell_w: u16,
    /// Rows per day cell (`>= 1`).
    cell_h: u16,
    /// Total grid width in columns (`7 * cell_w`, clipped to the inner width).
    grid_w: u16,
    /// Total grid height in rows (`weeks * cell_h`, clipped to fit).
    grid_h: u16,
    /// Number of week rows that fit (`1..=6`).
    weeks: u16,
}

impl Geometry {
    /// The grid layout for `inner`, or `None` when no day cell can fit (the
    /// header and weekday row alone, or a zero area — the totality rule).
    fn new(inner: Rect) -> Option<Self> {
        if inner.is_empty() {
            return None;
        }
        let cell_w = (inner.width / 7).max(1);
        if cell_w * 7 == 0 || inner.width < 7 {
            return None;
        }
        // Row 0 header, row 1 weekday labels, the rest is the day grid.
        let grid_top = inner.top().saturating_add(2);
        if grid_top >= inner.bottom() {
            return None;
        }
        let body_rows = inner.bottom() - grid_top;
        // Up to six week rows; a cell is at least one row tall.
        let cell_h = (body_rows / 6).max(1);
        let weeks = (body_rows / cell_h).clamp(1, 6);
        Some(Self {
            grid_top,
            cell_w,
            cell_h,
            grid_w: cell_w * 7,
            grid_h: weeks * cell_h,
            weeks,
        })
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

/// Tints `style` with `color` as a foreground unless the event left it
/// `Reset`, in which case the base style stands (so an uncoloured event reads
/// in the cell's own colours, exactly like a default [`Span`](rstui_core::Span)).
fn tint(style: Style, color: Color) -> Style {
    if color == Color::Reset {
        style
    } else {
        style.fg(color)
    }
}

impl Widget for MonthView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // The block (if any) frames the content and reserves the inner area;
        // `render_ref` is the clone-free path (a `Block` owns a title `Line`).
        let inner = self.grid_area(area);
        if let Some(b) = &self.block {
            b.render_ref(area, buf);
        }
        if inner.is_empty() {
            return;
        }

        // Base fills the content area so a background covers the whole pane.
        buf.set_style(inner, self.style);

        let (month, day_count, first_weekday, col_of_first) = self.skeleton();
        let left = inner.left();
        let right = inner.right();
        let bottom = inner.bottom();

        let Some(geo) = Geometry::new(inner) else {
            // No day cell fits; still draw the header/weekday labels that do.
            self.render_header(buf, month, first_weekday, inner);
            return;
        };

        self.render_header(buf, month, first_weekday, inner);

        let base = self.style;
        let grid = base.patch(self.grid_style);
        let sel = base.patch(self.selected_style);
        let tod = base.patch(self.today_style);

        // Light vertical rules between the seven columns of every week row,
        // drawn first so cell content (numbers, chips, bars) overlays them.
        for week in 0..geo.weeks {
            let cell_y = geo.grid_top + week * geo.cell_h;
            for col in 1..7u16 {
                let rx = left + col * geo.cell_w;
                if rx >= right {
                    break;
                }
                for r in 0..geo.cell_h {
                    let ry = cell_y + r;
                    if ry >= bottom {
                        break;
                    }
                    buf.set_cell(Position::new(rx, ry), '│', grid);
                }
            }
        }

        // Each day cell drawn from the one source of truth, `cell_chips`: the
        // day number (today accented, selection patched last so it wins), then
        // every visible event on its own row — a multi-day/all-day event as a
        // full-cell-width bar segment (contiguous covered cells abut into one
        // continuous bar), a single-day event as a tinted chip — then the
        // "+N more" footer on the cell's last body row.
        for dom in 1..=day_count {
            let (week, col) = MonthView::cell_of(dom, col_of_first);
            if week >= geo.weeks {
                break;
            }
            let cell_x = left + col * geo.cell_w;
            let cell_y = geo.grid_top + week * geo.cell_h;
            if cell_y >= bottom {
                break;
            }
            let cell_right = (cell_x + geo.cell_w).min(right);

            // Day-number gutter: a two-glyph right-aligned number, accented
            // for today, then selection patched last so it wins.
            let mut num_style = grid;
            if self.today == Some(dom) {
                num_style = num_style.patch(tod);
            }
            if self.selected == Some(dom) {
                num_style = num_style.patch(sel);
            }
            // Written directly (no per-day `format!`): `dom` is 1..=31, the
            // tens glyph is a space when single-digit — the `Calendar` rule.
            let hi = if dom >= 10 {
                (b'0' + (dom / 10) as u8) as char
            } else {
                ' '
            };
            let lo = (b'0' + (dom % 10) as u8) as char;
            if cell_x < right {
                buf.set_cell(Position::new(cell_x, cell_y), hi, num_style);
            }
            let lo_x = cell_x.saturating_add(1);
            if lo_x < right {
                buf.set_cell(Position::new(lo_x, cell_y), lo, num_style);
            }
            // If selected/today, wash the rest of the number row too so the
            // accent reads as the whole header strip of the cell.
            if self.selected == Some(dom) || self.today == Some(dom) {
                for dx in 2..geo.cell_w {
                    let x = cell_x.saturating_add(dx);
                    if x >= right {
                        break;
                    }
                    buf.set_cell(Position::new(x, cell_y), ' ', num_style);
                }
            }

            // The visible rows for this cell (spanning-first, then single,
            // truncated to the row budget) and the hidden overflow count —
            // the identical resolution `event_at` reads.
            let axis_day = self.first_day + i64::from(dom) - 1;
            let (rows, hidden) = self.cell_chips(axis_day, geo.cell_h);

            let chip_top = cell_y.saturating_add(1);
            for (r, e) in rows.iter().enumerate() {
                let y = chip_top.saturating_add(r as u16);
                if y >= bottom || y >= cell_y + geo.cell_h {
                    break;
                }
                if e.multi_day() || e.all_day() {
                    // A bar segment: a solid colour fill with a contrast
                    // label (so a spanning event reads as an actual bar, not
                    // faint tinted text), then the title; contiguous covered
                    // cells abut into one continuous bar. `Reset` keeps the
                    // cell's own colours.
                    let st = if e.color() == Color::Reset {
                        base
                    } else {
                        base.bg(e.color()).fg(crate::event::readable_fg(e.color()))
                    };
                    for x in cell_x..cell_right {
                        buf.set_cell(Position::new(x, y), ' ', st);
                    }
                    put(buf, e.title(), st, cell_x, y, cell_right);
                } else {
                    // A chip: a leading 12h time marker then the title.
                    let st = tint(grid, e.color());
                    let label: Cow<str> = if e.all_day() {
                        Cow::Borrowed(e.title())
                    } else {
                        Cow::Owned(format!(
                            "{} {}",
                            crate::event::time_label_12h(e.start_min()),
                            e.title()
                        ))
                    };
                    put(buf, &label, st, cell_x, y, cell_right);
                }
            }

            // "+N more" footer when the cell could not show every event. It
            // sits on the cell's last body row, which `cell_chips` left free.
            if hidden > 0 {
                let foot_y = cell_y + geo.cell_h.saturating_sub(1);
                if foot_y < bottom && foot_y > cell_y {
                    put(buf, &format!("+{hidden}"), grid, cell_x, foot_y, cell_right);
                }
            }
        }
    }
}

impl MonthView<'_> {
    /// Draws the centred "Month Year" header row and the rotated weekday-label
    /// row. Always safe to call — clipped to `inner`.
    fn render_header(&self, buf: &mut Buffer, month: u32, first_weekday: u32, inner: Rect) {
        let left = inner.left();
        let right = inner.right();
        let bottom = inner.bottom();
        let cell_w = (inner.width / 7).max(1);
        let span = (cell_w * 7).min(inner.width);

        // Row 0: "<Month> <year>", centred over the grid span.
        let header = format!("{} {}", MONTH_NAMES[(month - 1) as usize], self.year);
        let hw = (header.chars().count() as u16).min(span);
        put(
            buf,
            &header,
            self.style.patch(self.header_style),
            left + (span - hw) / 2,
            inner.top(),
            right,
        );

        // Row 1: weekday labels, one per column, rotated so column 0 is
        // `first_weekday`.
        let wd_y = inner.top().saturating_add(1);
        if wd_y < bottom {
            let wd_style = self.style.patch(self.weekday_style);
            for c in 0..7u16 {
                let label = WEEKDAYS[((first_weekday + u32::from(c)) % 7) as usize];
                put(buf, label, wd_style, left + c * cell_w, wd_y, right);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn ev(id: u64, title: &'static str, day: i64) -> CalendarEvent<'static> {
        CalendarEvent::new(id, title).with_day(day)
    }

    #[test]
    fn header_is_the_month_name_and_year_centred() {
        // "May 2026" is 8 wide; grid span over a 35-wide area is 35 (cell_w=5)
        // → centred at col (35-8)/2 = 13.
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 8));
        MonthView::new(2026, 5, 31, 5).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(13, 0)).unwrap().symbol, 'M');
        assert_eq!(buf.get(Position::new(20, 0)).unwrap().symbol, '6');
    }

    #[test]
    fn weekday_row_starts_on_sunday_by_default() {
        let out = lines(MonthView::new(2026, 5, 31, 5), 35, 8);
        let row1 = out.lines().nth(1).unwrap();
        // cell_w = 5, labels left-aligned per column.
        assert!(row1.starts_with("Su"));
        assert_eq!(&row1[5..7], "Mo");
        assert_eq!(&row1[30..32], "Sa");
    }

    #[test]
    fn first_weekday_rotates_the_columns() {
        let out = lines(MonthView::new(2026, 5, 31, 5).first_weekday(1), 35, 8);
        let row1 = out.lines().nth(1).unwrap();
        assert!(row1.starts_with("Mo"));
        assert_eq!(&row1[30..32], "Su");
    }

    #[test]
    fn day_one_lands_in_its_weekday_column() {
        // 1st is Friday (5), Sunday-start → column 5. cell_w = 5 → x = 25,
        // two-digit number right-aligned so '1' at x = 26, grid row 2.
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 12));
        MonthView::new(2026, 5, 31, 5).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(26, 2)).unwrap().symbol, '1');
        // Columns before it on the first week row carry no day number.
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, ' ');
    }

    #[test]
    fn days_wrap_into_week_rows() {
        // Sun-start, 1st Friday: row0 cols 5,6 hold 1,2; day 3 wraps to col 0
        // of week row 1. With height 12 the cell is 1 row tall (10 body rows
        // / 6 weeks = 1) so week rows are grid rows 2,3,4,...
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 12));
        MonthView::new(2026, 5, 31, 5).render(buf.area(), &mut buf);
        // Day 2 → column 6, week 0 (grid row 2). cell_x = 30, '2' at x = 31.
        assert_eq!(buf.get(Position::new(31, 2)).unwrap().symbol, '2');
        // Day 3 wraps to column 0, week 1 (grid row 3). '3' at x = 1.
        assert_eq!(buf.get(Position::new(1, 3)).unwrap().symbol, '3');
    }

    #[test]
    fn single_day_chip_renders_below_the_day_number() {
        // A timed event on day-of-month 1 (axis day 1) — chip on the row
        // below the number with a 12h time prefix. A 70-wide grid → cell_w 10
        // so the chip text is not clipped.
        let events = [CalendarEvent::new(7, "Sync")
            .with_day(1)
            .with_span(9 * 60, 9 * 60 + 30)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 70, 20));
        MonthView::new(2026, 5, 31, 5)
            .events(&events)
            .render(buf.area(), &mut buf);
        // 18 body rows / 6 = cell_h 3; cell_w = 70/7 = 10; day 1 col 5 →
        // cell_x 50, cell_y 2. Chip on cell_y + 1 = row 3 from cell_x = 50:
        // "9am Sync" (8 chars, fits the 10-wide cell).
        let row: String = (50..60)
            .map(|x| buf.get(Position::new(x, 3)).unwrap().symbol)
            .collect();
        assert!(row.starts_with("9am Sync"), "row was {row:?}");
    }

    #[test]
    fn a_chip_title_is_clipped_to_its_cell_width() {
        // A 35-wide grid → cell_w 5; "9am Sync" cannot fit and is truncated
        // at the cell boundary (the spec's "truncated title").
        let events = [CalendarEvent::new(7, "Sync")
            .with_day(1)
            .with_span(9 * 60, 9 * 60 + 30)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 20));
        MonthView::new(2026, 5, 31, 5)
            .events(&events)
            .render(buf.area(), &mut buf);
        // Day 1 col 5 → cell_x 25, cell_w 5: chip text clipped to "9am S".
        let row: String = (25..30)
            .map(|x| buf.get(Position::new(x, 3)).unwrap().symbol)
            .collect();
        assert_eq!(row, "9am S");
        // The cell boundary column is untouched by the clipped chip.
        assert_eq!(buf.get(Position::new(29, 3)).unwrap().symbol, 'S');
    }

    #[test]
    fn a_chip_is_tinted_with_the_event_color() {
        let events = [CalendarEvent::new(1, "X")
            .with_day(1)
            .with_all_day(true)
            .with_color(Color::Green)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 20));
        MonthView::new(2026, 5, 31, 5)
            .events(&events)
            .render(buf.area(), &mut buf);
        // All-day single-cell event renders as a one-column-wide spanning
        // bar on cell_y+1. Day 1 col 5 → x0 = 25. The bar is a solid colour
        // fill with a contrast label (was faint tinted text on the surface).
        assert_eq!(buf.get(Position::new(26, 3)).unwrap().bg, Color::Green);
        assert_eq!(
            buf.get(Position::new(25, 3)).unwrap().fg,
            crate::event::readable_fg(Color::Green)
        );
    }

    #[test]
    fn a_multi_day_event_is_one_continuous_spanning_bar() {
        // Event on axis days 1..=3 → day-of-month 1 (Fri col5), 2 (Sat col6),
        // 3 (Sun col0 of week 1). Within week 0 it spans cols 5..6 as one bar.
        let events = [CalendarEvent::new(9, "Trip")
            .with_day(1)
            .with_end_day(3)
            .with_color(Color::Blue)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 20));
        MonthView::new(2026, 5, 31, 5)
            .events(&events)
            .render(buf.area(), &mut buf);
        // Week 0 cell_y = 2, bar at cell_y+1 = 3. cols 5..6 → x 25..35.
        // The bar is a solid colour fill, continuous across the column
        // boundary at x = 30 and up to Saturday's right edge (x = 34).
        assert_eq!(buf.get(Position::new(25, 3)).unwrap().bg, Color::Blue);
        assert_eq!(buf.get(Position::new(30, 3)).unwrap().bg, Color::Blue);
        assert_eq!(buf.get(Position::new(34, 3)).unwrap().bg, Color::Blue);
    }

    #[test]
    fn overflow_collapses_into_a_plus_n_more_footer() {
        // Four single-day events on day 1 in a 3-row cell. Rows are
        // [number][body][body]; an overflow footer takes the last body row so
        // only one chip fits → 3 hidden → "+3".
        let events = [ev(1, "A", 1), ev(2, "B", 1), ev(3, "C", 1), ev(4, "D", 1)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 20));
        MonthView::new(2026, 5, 31, 5)
            .events(&events)
            .max_chips(2)
            .render(buf.area(), &mut buf);
        // cell_h = 3 (18/6); day 1 col 5 → cell_x 25, cell_y 2. The footer
        // sits on cell_y + cell_h - 1 = 4.
        let foot: String = (25..30)
            .map(|x| buf.get(Position::new(x, 4)).unwrap().symbol)
            .collect();
        assert_eq!(foot, "+3   ");
        // The footer never collides with a chip row: row 3 (the one chip) is
        // event A, not the footer.
        let area = Rect::new(0, 0, 35, 20);
        let mv = MonthView::new(2026, 5, 31, 5).events(&events).max_chips(2);
        assert_eq!(mv.event_at(area, Position::new(26, 3)), Some(1));
        assert_eq!(mv.event_at(area, Position::new(26, 4)), None); // footer row
    }

    #[test]
    fn selected_day_takes_the_selected_style_patched_last() {
        let mv = MonthView::new(2026, 5, 31, 5)
            .selected(Some(1))
            .today(Some(1))
            .selected_style(Style::new().bg(Color::Blue))
            .today_style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 12));
        mv.render(buf.area(), &mut buf);
        // Day 1 col 5 → cell_x 25, number row grid row 2. Selection patched
        // last → blue, not red.
        assert_eq!(buf.get(Position::new(26, 2)).unwrap().bg, Color::Blue);
    }

    #[test]
    fn today_accents_only_its_own_cell() {
        let mv = MonthView::new(2026, 5, 31, 5)
            .today(Some(2))
            .today_style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 12));
        mv.render(buf.area(), &mut buf);
        // Day 2 col 6 → cell_x 30, number row 2.
        assert_eq!(buf.get(Position::new(31, 2)).unwrap().bg, Color::Red);
        // Day 1 (col 5) untouched.
        assert_eq!(buf.get(Position::new(26, 2)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn day_at_maps_a_pointer_back_to_its_day_of_month() {
        let mv = MonthView::new(2026, 5, 31, 5);
        let area = Rect::new(0, 0, 35, 12);
        // Day 1 is col 5, week 0; cell_y = grid_top = 2, cell_x = 25.
        assert_eq!(mv.day_at(area, Position::new(26, 2)), Some(1));
        // Day 3 is col 0, week 1 (grid row 3).
        assert_eq!(mv.day_at(area, Position::new(1, 3)), Some(3));
        // A padding cell before day 1 on week 0 → None.
        assert_eq!(mv.day_at(area, Position::new(1, 2)), None);
        // The header row → None.
        assert_eq!(mv.day_at(area, Position::new(13, 0)), None);
        // Off-pane → None.
        assert_eq!(mv.day_at(area, Position::new(100, 100)), None);
    }

    #[test]
    fn event_at_resolves_the_chip_under_the_pointer() {
        let events = [CalendarEvent::new(42, "Meet")
            .with_day(1)
            .with_span(10 * 60, 11 * 60)];
        let mv = MonthView::new(2026, 5, 31, 5).events(&events);
        let area = Rect::new(0, 0, 35, 20);
        // cell_h = 3, day 1 col 5 cell_y 2; chip on cell_y + 1 = row 3.
        assert_eq!(mv.event_at(area, Position::new(26, 3)), Some(42));
        // The day-number row of the same cell is not a chip.
        assert_eq!(mv.event_at(area, Position::new(26, 2)), None);
        // An empty cell (day 2) yields no event.
        assert_eq!(mv.event_at(area, Position::new(31, 3)), None);
    }

    #[test]
    fn an_out_of_range_month_is_clamped() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 8));
        MonthView::new(2026, 99, 31, 0).render(buf.area(), &mut buf);
        // "December 2026" (13 wide) centres at col (35-13)/2 = 11.
        assert_eq!(buf.get(Position::new(11, 0)).unwrap().symbol, 'D');
    }

    #[test]
    fn an_out_of_range_selected_day_simply_does_not_highlight() {
        let mv = MonthView::new(2026, 5, 31, 5)
            .selected(Some(99))
            .selected_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 12));
        mv.render(buf.area(), &mut buf);
        for cell in buf.cells() {
            assert_ne!(cell.bg, Color::Blue);
        }
    }

    #[test]
    fn a_block_frames_the_month_in_the_inner_area() {
        let mv = MonthView::new(2026, 5, 0, 0).block(Block::bordered());
        let mut buf = Buffer::empty(Rect::new(0, 0, 37, 6));
        mv.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(36, 0)).unwrap().symbol, '┐');
        assert_eq!(buf.get(Position::new(36, 5)).unwrap().symbol, '┘');
        // Header centred in the 35-wide inner starting at x = 1.
        assert_eq!(buf.get(Position::new(14, 1)).unwrap().symbol, 'M');
        // Weekday row inside the frame.
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, 'S');
    }

    #[test]
    fn a_narrow_area_clips_the_grid_without_panicking() {
        // Width 9 → cell_w = 1, only one glyph per column; no panic.
        let events = [ev(1, "Long title here", 1)];
        let out = lines(MonthView::new(2026, 5, 31, 5).events(&events), 9, 6);
        // Header clipped but present; first weekday glyph at col 0.
        assert!(out.lines().nth(1).unwrap().starts_with('S'));
    }

    #[test]
    fn a_tiny_area_with_no_room_for_a_grid_is_safe() {
        // Two rows: header + weekday only, no day grid. No panic.
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 2));
        MonthView::new(2026, 5, 31, 5).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(13, 0)).unwrap().symbol, 'M');
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'S');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 12));
        let events = [ev(1, "X", 1)];
        MonthView::new(2026, 5, 31, 5)
            .events(&events)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        // The accessors are equally total on a zero area.
        let mv = MonthView::new(2026, 5, 31, 5);
        assert_eq!(mv.day_at(Rect::new(0, 0, 0, 0), Position::new(0, 0)), None);
        assert_eq!(
            mv.event_at(Rect::new(0, 0, 0, 0), Position::new(0, 0)),
            None
        );
    }

    #[test]
    fn first_day_offsets_which_axis_day_each_cell_shows() {
        // first_day = 100 → day-of-month 1 is axis day 100. An event on axis
        // day 101 must surface in day-of-month 2's cell, not day 1's.
        let events = [CalendarEvent::new(5, "Z").with_day(101).with_all_day(true)];
        let mv = MonthView::new(2026, 5, 31, 5)
            .events(&events)
            .first_day(100);
        let area = Rect::new(0, 0, 35, 20);
        // Day 2 col 6 cell_y 2; spanning bar on cell_y+1 = row 3, x in 30..35.
        assert_eq!(mv.event_at(area, Position::new(31, 3)), Some(5));
        // Day 1's cell has no event.
        assert_eq!(mv.event_at(area, Position::new(26, 3)), None);
    }

    #[test]
    fn out_of_range_weekday_indices_are_reduced_mod_seven() {
        // weekday_of_first 12 ≡ 5 (Friday); first_weekday 8 ≡ 1 (Mon-start).
        let mv = MonthView::new(2026, 5, 31, 12).first_weekday(8);
        let out = lines(mv, 35, 12);
        let row1 = out.lines().nth(1).unwrap();
        assert!(row1.starts_with("Mo")); // Monday-start
        // 1st (Fri) with Mon-start → column 4. cell_w 5 → '1' at x = 21.
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 12));
        MonthView::new(2026, 5, 31, 12)
            .first_weekday(8)
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(21, 2)).unwrap().symbol, '1');
    }

    #[test]
    fn an_event_outside_the_month_window_is_not_shown() {
        // Event on axis day 999 (far past the month) shows nowhere; no panic.
        // The title uses a glyph absent from every month/weekday label so a
        // stray render would be unambiguous.
        let events = [CalendarEvent::new(1, "Zzz")
            .with_day(999)
            .with_all_day(true)];
        let mv = MonthView::new(2026, 5, 31, 5).events(&events);
        let mut buf = Buffer::empty(Rect::new(0, 0, 35, 20));
        mv.clone().render(buf.area(), &mut buf);
        for cell in buf.cells() {
            assert_ne!(cell.symbol, 'Z');
        }
        // And the hit-test reports no chip anywhere in the grid body.
        let area = Rect::new(0, 0, 35, 20);
        assert_eq!(mv.event_at(area, Position::new(26, 3)), None);
        assert_eq!(mv.event_at(area, Position::new(1, 3)), None);
    }
}
