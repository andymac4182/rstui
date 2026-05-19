//! [`WeekView`] — a multi-day time grid: an hour ruler, a day-column header
//! row, an all-day band, and timed events tiled side-by-side; the week surface
//! a scheduling TUI pins next to a [`Calendar`](crate::Calendar) month grid.
//!
//! # A pure projection, like every other widget
//!
//! `WeekView` owns no state. It projects a caller-owned
//! `&[`[`CalendarEvent`]`]` exactly as [`DayView`](crate::DayView) does, only
//! across `N` day columns instead of one. The reducer (or a date crate of the
//! caller's choosing) decides which integer day each event falls on and what
//! `start_day` column 0 represents — the widget never interprets the unit, the
//! [`Gantt`](crate::Gantt) and [`Calendar`](crate::Calendar) axis discipline —
//! and the only behaviour here is grid layout: which column a day maps to,
//! where a minute lands in the hour grid, and the overlap tiling delegated to
//! [`event::pack_day`], the one algorithm every calendar view shares.
//!
//! # Dependency-free on purpose: the widget does no date math
//!
//! [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)
//! §4 gates any widget that pulls a transitive dependency behind a Cargo
//! feature. A week view that computed weekday names or wall-clock spans would
//! need `chrono`/`time`; `WeekView` instead takes a caller-axis `start_day`
//! `i64`, per-column header text the caller already formatted, and the shared
//! [`CalendarEvent`] integer model. The hour ruler is
//! [`time_label`](crate::event::time_label) — pure clock arithmetic on a
//! caller integer, the same justified-arithmetic line
//! [`Calendar`](crate::Calendar)'s `{day:>2}` sits on — so it pulls in no
//! dependency and needs no feature gate, staying a deterministically
//! headless-testable pure projection like [`List`](crate::List).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, a `day_count` outside `1..=14` (clamped), an `hours` window with
//! `end <= start` or `end > 24` (clamped), a `now` outside the window
//! (skipped), an event on a day outside the visible columns (skipped), an
//! out-of-range `selected`/`today`, and an area too small for the ruler or
//! header are all clamped/clipped — never a panic.

use rstui_core::{Buffer, Color, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::event::{self, CalendarEvent};

/// Columns reserved for the left hour ruler: `"HH:00"` is five glyphs plus a
/// one-column gutter so a tinted event block never abuts the ruler text.
const RULER_W: u16 = 6;

/// The grid snaps a clicked / dragged minute to this granularity (quarter
/// hour), the slot resolution a scheduling app creates and moves events on.
const SNAP_MIN: u16 = 15;

/// A multi-day time grid: an hour ruler down the left, a day-column header
/// row, an all-day band, then the hour grid with timed events placed by
/// minute and tiled side-by-side when they overlap, as a pure projection of a
/// caller-owned `&[`[`CalendarEvent`]`]`.
///
/// `WeekView` does **no date math** — column 0 is the caller-axis
/// [`start_day`](Self::new) and an event maps to column `event.day() -
/// start_day` (see the [module docs](self)). [`day_labels`](Self::day_labels)
/// is per-column header text the caller already formatted (a missing entry is
/// blank); [`today`](Self::today) accents one column's header;
/// [`selected_event`](Self::selected_event) is the caller-owned id of the
/// highlighted block; an optional [`Block`] frames the grid.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{CalendarEvent, WeekView};
///
/// // A Mon–Fri week (axis days 0..5); a 10:00–11:00 meeting on Tuesday.
/// let labels = ["Mon", "Tue", "Wed", "Thu", "Fri"];
/// let events = [CalendarEvent::new(1, "Sync")
///     .with_day(1)
///     .with_span(10 * 60, 11 * 60)];
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 41, 12));
/// WeekView::new(0, 5)
///     .day_labels(&labels)
///     .events(&events)
///     .hours(8, 18)
///     .render(buf.area(), &mut buf);
///
/// // Column 0's header begins just right of the 6-wide ruler.
/// assert_eq!(buf.get(Position::new(6, 0)).unwrap().symbol, 'M');
/// ```
#[derive(Debug, Clone)]
pub struct WeekView<'a> {
    start_day: i64,
    day_count: u16,
    events: &'a [CalendarEvent<'a>],
    day_labels: &'a [&'a str],
    today: Option<i64>,
    start_h: u16,
    end_h: u16,
    now: Option<u16>,
    selected_event: Option<u64>,
    block: Option<Block<'a>>,
    style: Style,
    grid_style: Style,
    ruler_style: Style,
    header_style: Style,
    all_day_style: Style,
    now_style: Style,
    selected_style: Style,
}

impl<'a> WeekView<'a> {
    /// A week grid whose column 0 is the caller-axis day `start_day`, with
    /// `day_count` day columns.
    ///
    /// `day_count` is clamped to `1..=14` at render (a `0` becomes `1`, a
    /// fortnight is the cap); the default scheduling week passes `7`. Every
    /// other input is clamped/clipped at render too (see the
    /// [module docs](self)).
    pub fn new(start_day: i64, day_count: u16) -> Self {
        Self {
            start_day,
            day_count,
            events: &[],
            day_labels: &[],
            today: None,
            start_h: 0,
            end_h: 24,
            now: None,
            selected_event: None,
            block: None,
            style: Style::default(),
            grid_style: Style::default(),
            ruler_style: Style::default(),
            header_style: Style::default(),
            all_day_style: Style::default(),
            now_style: Style::default(),
            selected_style: Style::default(),
        }
    }

    /// Sets the caller-owned events the view projects. An event on a day
    /// outside the visible columns is simply skipped.
    #[must_use]
    pub fn events(mut self, events: &'a [CalendarEvent<'a>]) -> Self {
        self.events = events;
        self
    }

    /// Sets the per-column header text, caller-formatted (the widget does **no**
    /// date math). Entry `c` heads column `c`; a missing entry renders blank.
    #[must_use]
    pub fn day_labels(mut self, day_labels: &'a [&'a str]) -> Self {
        self.day_labels = day_labels;
        self
    }

    /// Sets the caller-axis day to accent in the header row, or `None`. A day
    /// outside the visible columns is ignored.
    #[must_use]
    pub fn today(mut self, today: Option<i64>) -> Self {
        self.today = today;
        self
    }

    /// Sets the visible hour window `[start_h, end_h)` (default `0..24`).
    /// Clamped so `end_h` is `> start_h` and `<= 24` — never an empty or
    /// inverted window.
    #[must_use]
    pub fn hours(mut self, start_h: u16, end_h: u16) -> Self {
        self.start_h = start_h.min(23);
        self.end_h = end_h.min(24).max(self.start_h + 1);
        self
    }

    /// Sets the minute-of-day a "now" rule is drawn across the grid at, or
    /// `None`. Drawn only when it falls inside the visible
    /// [`hours`](Self::hours) window.
    #[must_use]
    pub fn now(mut self, now: Option<u16>) -> Self {
        self.now = now;
        self
    }

    /// Sets the caller-owned id of the highlighted event, or `None`. An id
    /// matching no visible event simply highlights nothing.
    #[must_use]
    pub fn selected_event(mut self, selected_event: Option<u64>) -> Self {
        self.selected_event = selected_event;
        self
    }

    /// Frames the grid in `block`; the view renders into
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

    /// Sets the [`Style`] of the hour-grid background, over the base.
    #[must_use]
    pub fn grid_style(mut self, style: Style) -> Self {
        self.grid_style = style;
        self
    }

    /// Sets the [`Style`] of the left hour-ruler column, over the base.
    #[must_use]
    pub fn ruler_style(mut self, style: Style) -> Self {
        self.ruler_style = style;
        self
    }

    /// Sets the [`Style`] of the day-label header row, over the base.
    #[must_use]
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Sets the [`Style`] of the all-day band, over the base.
    #[must_use]
    pub fn all_day_style(mut self, style: Style) -> Self {
        self.all_day_style = style;
        self
    }

    /// Sets the [`Style`] of the "now" rule, over the base.
    #[must_use]
    pub fn now_style(mut self, style: Style) -> Self {
        self.now_style = style;
        self
    }

    /// Sets the [`Style`] patched over the selected event's block.
    #[must_use]
    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    // --- Layout, shared by `render` and the hit-test accessors so they can
    // --- never disagree about where a row or column falls.

    /// The clamped day-column count (`1..=14`).
    fn clamped_count(&self) -> u16 {
        self.day_count.clamp(1, 14)
    }

    /// The clamped visible window as `(start_min, end_min)` minute-of-day, with
    /// `end > start` guaranteed.
    fn window(&self) -> (u16, u16) {
        let s = self.start_h.min(23);
        let e = self.end_h.min(24).max(s + 1);
        (s * 60, e * 60)
    }

    /// The inner content rect (inside the optional [`Block`]).
    fn inner(&self, area: Rect) -> Rect {
        match &self.block {
            Some(b) => b.inner(area),
            None => area,
        }
    }

    /// The hour-grid rect: the area below the header + all-day band and right
    /// of the ruler. The app confines a timed-event drag to this.
    ///
    /// Empty (zero height/width) when the area is too small for the header,
    /// the all-day band, and at least one grid row — total, never a panic.
    #[must_use]
    pub fn body(&self, area: Rect) -> Rect {
        let inner = self.inner(area);
        if inner.is_empty() {
            return Rect::new(inner.x, inner.y, 0, 0);
        }
        let grid_top = inner.top().saturating_add(2); // header + all-day band
        let grid_x = inner.left().saturating_add(RULER_W);
        if grid_top >= inner.bottom() || grid_x >= inner.right() {
            return Rect::new(
                grid_x.min(inner.right()),
                grid_top.min(inner.bottom()),
                0,
                0,
            );
        }
        Rect::new(
            grid_x,
            grid_top,
            inner.right() - grid_x,
            inner.bottom() - grid_top,
        )
    }

    /// The all-day band rect: the single row between the header and the hour
    /// grid the app confines an all-day drag to. Empty when there is no room
    /// for it.
    #[must_use]
    pub fn all_day_band(&self, area: Rect) -> Rect {
        let inner = self.inner(area);
        if inner.is_empty() {
            return Rect::new(inner.x, inner.y, 0, 0);
        }
        let band_y = inner.top().saturating_add(1);
        let grid_x = inner.left().saturating_add(RULER_W);
        if band_y >= inner.bottom() || grid_x >= inner.right() {
            return Rect::new(grid_x.min(inner.right()), band_y.min(inner.bottom()), 0, 0);
        }
        Rect::new(grid_x, band_y, inner.right() - grid_x, 1)
    }

    /// The caller-axis day and 15-minute-snapped minute-of-day under `pos`, or
    /// `None` when `pos` is outside the hour grid — the seam a scheduling app
    /// click-creates or drag-moves an event on.
    ///
    /// The minute is snapped to a 15-minute slot and clamped to the visible
    /// window; the day is `start_day + column`. Mirrors `render`'s geometry
    /// exactly.
    #[must_use]
    pub fn slot_at(&self, area: Rect, pos: Position) -> Option<(i64, u16)> {
        let body = self.body(area);
        if body.is_empty() || !body.contains(pos) {
            return None;
        }
        let count = self.clamped_count();
        let (win_lo, win_hi) = self.window();
        let win_min = win_hi - win_lo;
        let grid_rows = body.height;

        let col = ((u32::from(pos.x - body.left()) * u32::from(count)) / u32::from(body.width))
            .min(u32::from(count) - 1) as i64;
        let row = u32::from(pos.y - body.top());
        let raw = win_lo as u32 + (row * u32::from(win_min)) / u32::from(grid_rows);
        let snapped =
            ((raw / u32::from(SNAP_MIN)) * u32::from(SNAP_MIN)).min(u32::from(win_hi)) as u16;
        Some((self.start_day + col, snapped))
    }

    /// The caller-owned id of the timed event whose block is under `pos`, or
    /// `None`. Mirrors `render`'s placement (column → tiled lane → minute
    /// span) exactly, so a click resolves to the same block the user sees.
    #[must_use]
    pub fn event_at(&self, area: Rect, pos: Position) -> Option<u64> {
        let body = self.body(area);
        if body.is_empty() || !body.contains(pos) {
            return None;
        }
        let count = self.clamped_count();
        let (win_lo, win_hi) = self.window();
        let win_min = win_hi - win_lo;
        let grid_rows = u32::from(body.height);
        let col_w = body.width / count;
        if col_w == 0 {
            return None;
        }

        let rel_col = (pos.x - body.left()) / col_w;
        if rel_col >= count {
            return None;
        }
        let col_day = self.start_day + i64::from(rel_col);
        let col_x0 = body.left() + rel_col * col_w;

        // Re-run the exact per-column tiling `render` used.
        let day_events: Vec<&CalendarEvent> = self
            .events
            .iter()
            .filter(|e| e.covers_day(col_day) && !e.all_day())
            .collect();
        let laid = event::pack_day(&day_events);
        // Last-drawn wins (matches `render`'s draw order): scan in reverse.
        for l in laid.iter().rev() {
            let s = l.start_min.clamp(win_lo, win_hi);
            let e = l.end_min.clamp(win_lo, win_hi);
            if e <= s {
                continue;
            }
            let y0 = body.top() as u32 + ((u32::from(s - win_lo) * grid_rows) / u32::from(win_min));
            let y1 = (body.top() as u32
                + ((u32::from(e - win_lo) * grid_rows) / u32::from(win_min)))
            .max(y0 + 1);
            let lanes = l.columns.max(1);
            let lane_w = (col_w / lanes).max(1);
            let lx0 = col_x0 + (l.column.min(lanes - 1)) * lane_w;
            let lx1 = if l.column + 1 >= lanes {
                col_x0 + col_w
            } else {
                lx0 + lane_w
            };
            let py = u32::from(pos.y);
            if py >= y0 && py < y1 && pos.x >= lx0 && pos.x < lx1 {
                return Some(l.id);
            }
        }
        None
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

impl Widget for WeekView<'_> {
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
            b.render_ref(area, buf);
        }
        if inner.is_empty() {
            return;
        }

        // Base fills the content area so a background covers the whole pane.
        buf.set_style(inner, self.style);

        // Clamp every caller input — a pure projection is total.
        let count = self.clamped_count();
        let (win_lo, win_hi) = self.window();
        let win_min = win_hi - win_lo;

        let left = inner.left();
        let right = inner.right();
        let bottom = inner.bottom();
        let top = inner.top();

        // The ruler reserves the left columns; the day columns share the rest.
        let grid_x0 = left.saturating_add(RULER_W);
        if grid_x0 >= right {
            // No room for a single day column past the ruler — still draw the
            // ruler text so the pane reads as a (clipped) week, never a panic.
            self.draw_ruler(buf, inner, win_lo, win_hi);
            return;
        }
        let cols_w = right - grid_x0;
        let col_w = cols_w / count;
        if col_w == 0 {
            self.draw_ruler(buf, inner, win_lo, win_hi);
            return;
        }

        // Row 0: the day-column header. The base carries every column; the
        // `today` column alone is patched with `header_style` so it reads as
        // an accent band, exactly the `Calendar` base-then-accent discipline.
        let base = self.style;
        for c in 0..count {
            let cx = grid_x0 + c * col_w;
            let day = self.start_day + i64::from(c);
            let label = self.day_labels.get(c as usize).copied().unwrap_or("");
            let cw = col_w.min(right.saturating_sub(cx));
            let hs = if self.today == Some(day) {
                let accent = base.patch(self.header_style);
                // Fill the whole header cell so the active column reads as a
                // band even where the label is short.
                for dx in 0..cw {
                    buf.set_cell(Position::new(cx + dx, top), ' ', accent);
                }
                accent
            } else {
                base
            };
            put(buf, label, hs, cx, top, (cx + col_w).min(right));
        }

        // The left ruler ("HH:00" per visible hour) — drawn after the header
        // so its own style owns the whole column.
        self.draw_ruler(buf, inner, win_lo, win_hi);

        // Row 1: the all-day band. All-day events plus any multi-day timed
        // event are stacked on every column they cover.
        let band_y = top.saturating_add(1);
        if band_y < bottom {
            let band_base = self.style.patch(self.all_day_style);
            for c in 0..count {
                let cx = grid_x0 + c * col_w;
                let cw = (col_w).min(right.saturating_sub(cx));
                for dx in 0..cw {
                    buf.set_cell(Position::new(cx + dx, band_y), ' ', band_base);
                }
            }
            for c in 0..count {
                let day = self.start_day + i64::from(c);
                let cx = grid_x0 + c * col_w;
                let cw = (col_w).min(right.saturating_sub(cx));
                if cw == 0 {
                    continue;
                }
                if let Some(ev) = self
                    .events
                    .iter()
                    .find(|e| e.covers_day(day) && (e.all_day() || e.multi_day()))
                {
                    let mut bs = band_base;
                    if ev.color() != Color::Reset {
                        bs = bs.bg(ev.color()).fg(event::readable_fg(ev.color()));
                    }
                    if self.selected_event == Some(ev.id()) {
                        bs = bs.patch(self.selected_style);
                    }
                    for dx in 0..cw {
                        buf.set_cell(Position::new(cx + dx, band_y), ' ', bs);
                    }
                    put(buf, ev.title(), bs, cx, band_y, cx + cw);
                }
            }
        }

        // Rows 2..: the hour grid. Fill it with `grid_style`, then place each
        // column's timed events.
        let grid_top = top.saturating_add(2);
        if grid_top >= bottom {
            return;
        }
        let grid_rows = bottom - grid_top;
        let grid_glyph = self.style.patch(self.grid_style);
        for y in grid_top..bottom {
            for x in grid_x0..right {
                buf.set_cell(Position::new(x, y), ' ', grid_glyph);
            }
        }

        for c in 0..count {
            let col_day = self.start_day + i64::from(c);
            let col_x0 = grid_x0 + c * col_w;
            let day_events: Vec<&CalendarEvent> = self
                .events
                .iter()
                .filter(|e| e.covers_day(col_day) && !e.all_day())
                .collect();
            let laid = event::pack_day(&day_events);
            for l in &laid {
                let s = l.start_min.clamp(win_lo, win_hi);
                let e = l.end_min.clamp(win_lo, win_hi);
                if e <= s {
                    continue; // wholly outside the window or zero-length
                }
                // Map the minute span linearly across the grid rows.
                let y0 = grid_top
                    + ((u32::from(s - win_lo) * u32::from(grid_rows)) / u32::from(win_min)) as u16;
                let y1 = (grid_top
                    + ((u32::from(e - win_lo) * u32::from(grid_rows)) / u32::from(win_min)) as u16)
                    .max(y0 + 1); // a sliver is at least one row
                let y1 = y1.min(bottom);

                // The lane subdivides the day column; the last lane runs to the
                // column edge so rounding never leaves a one-cell seam.
                let lanes = l.columns.max(1);
                let lane_w = (col_w / lanes).max(1);
                let lx0 = col_x0 + l.column.min(lanes - 1) * lane_w;
                let lx1 = if l.column + 1 >= lanes {
                    col_x0 + col_w
                } else {
                    lx0 + lane_w
                };
                let lx1 = lx1.min(right);

                // The event tint, with the selected style patched last.
                let ev = self.events.iter().find(|x| x.id() == l.id);
                let mut bs = grid_glyph;
                if let Some(ev) = ev {
                    if ev.color() != Color::Reset {
                        bs = bs.bg(ev.color()).fg(event::readable_fg(ev.color()));
                    }
                }
                if self.selected_event == Some(l.id) {
                    bs = bs.patch(self.selected_style);
                }

                for y in y0..y1 {
                    for x in lx0..lx1 {
                        buf.set_cell(Position::new(x, y), ' ', bs);
                    }
                }
                // Title on the first row, "HH:MM" on the next if there's room.
                if let Some(ev) = ev {
                    if y0 < bottom {
                        put(buf, ev.title(), bs, lx0, y0, lx1);
                    }
                    if y1 > y0 + 1 && y0 + 1 < bottom {
                        put(buf, &event::time_label(l.start_min), bs, lx0, y0 + 1, lx1);
                    }
                }
            }
        }

        // The "now" rule, drawn last across the whole grid width so it reads
        // as one horizontal line. Skipped when outside the visible window.
        if let Some(n) = self.now {
            if n >= win_lo && n < win_hi {
                let ny = grid_top
                    + ((u32::from(n - win_lo) * u32::from(grid_rows)) / u32::from(win_min)) as u16;
                if ny < bottom {
                    let marker = self.style.patch(self.now_style);
                    for x in grid_x0..right {
                        buf.set_cell(Position::new(x, ny), '─', marker);
                    }
                }
            }
        }
    }
}

impl WeekView<'_> {
    /// Stamps the left hour ruler: `"HH:00"` on the grid row each visible hour
    /// starts. Split out so `render`'s early "no room for a column" exits can
    /// still draw it (the pane stays a recognisable, clipped week).
    fn draw_ruler(&self, buf: &mut Buffer, inner: Rect, win_lo: u16, win_hi: u16) {
        let top = inner.top();
        let bottom = inner.bottom();
        let left = inner.left();
        let right = inner.right();
        let grid_top = top.saturating_add(2);
        if grid_top >= bottom {
            return;
        }
        let grid_rows = bottom - grid_top;
        let win_min = win_hi - win_lo;
        let ruler_base = self.style.patch(self.ruler_style);
        // Fill the ruler column so its style owns it even between hour labels.
        let ruler_right = (left + RULER_W).min(right);
        for y in top..bottom {
            for x in left..ruler_right {
                buf.set_cell(Position::new(x, y), ' ', ruler_base);
            }
        }
        // One label per whole hour inside the window.
        let mut m = win_lo;
        while m < win_hi {
            let y = grid_top
                + ((u32::from(m - win_lo) * u32::from(grid_rows)) / u32::from(win_min)) as u16;
            if y < bottom {
                put(buf, &event::time_label(m), ruler_base, left, y, ruler_right);
            }
            m = m.saturating_add(60);
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
    fn full_render_is_a_ruler_a_header_a_band_and_the_grid() {
        // The whole surface as one snapshot: ruler column, the two day
        // headers, the all-day band row, then a "Sync" block placed by minute.
        let labels = ["Mo", "Tu"];
        let events = [CalendarEvent::new(1, "Sync")
            .with_day(0)
            .with_span(9 * 60, 10 * 60)];
        let wv = WeekView::new(0, 2)
            .day_labels(&labels)
            .events(&events)
            .hours(9, 11);
        // 14 wide: 6 ruler + 8 over 2 cols → col_w 4. 5 tall: header (row 0),
        // band (row 1), grid rows 2..5 (3 rows for 120 min → 40 min/row).
        // "Sync" (09:00–10:00) fills the first grid row; "10:00" labels the
        // grid row 60 min in (buffer row 3).
        assert_eq!(
            lines(wv, 14, 5),
            "      Mo  Tu  \n              \n09:00 Sync    \n10:00         \n              \n",
        );
    }

    #[test]
    fn header_row_carries_the_per_column_day_labels() {
        let labels = ["Mon", "Tue", "Wed"];
        let wv = WeekView::new(0, 3).day_labels(&labels);
        let mut buf = Buffer::empty(Rect::new(0, 0, 36, 6));
        wv.render(buf.area(), &mut buf);
        // 36 - 6 ruler = 30 over 3 cols → col_w 10. Headers at x = 6, 16, 26.
        assert_eq!(buf.get(Position::new(6, 0)).unwrap().symbol, 'M');
        assert_eq!(buf.get(Position::new(16, 0)).unwrap().symbol, 'T');
        assert_eq!(buf.get(Position::new(26, 0)).unwrap().symbol, 'W');
    }

    #[test]
    fn the_hour_ruler_labels_each_visible_hour() {
        // Window 09:00–11:00, 2 rows per hour: "09:00" at the first grid row,
        // "10:00" two rows below.
        let wv = WeekView::new(0, 1).hours(9, 11);
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 6));
        wv.render(buf.area(), &mut buf);
        // Grid starts at row 2 (header + all-day band), 4 rows for 120 min.
        let row2: String = (0..5)
            .map(|x| buf.get(Position::new(x, 2)).unwrap().symbol)
            .collect();
        assert_eq!(row2, "09:00");
        let row4: String = (0..5)
            .map(|x| buf.get(Position::new(x, 4)).unwrap().symbol)
            .collect();
        assert_eq!(row4, "10:00");
    }

    #[test]
    fn a_timed_event_is_placed_by_minute_in_its_day_column() {
        // One event on column 1 (day 1), 10:00–11:00 in a 09:00–12:00 window.
        let events = [CalendarEvent::new(1, "X")
            .with_day(1)
            .with_span(10 * 60, 11 * 60)
            .with_color(Color::Blue)];
        let wv = WeekView::new(0, 2).events(&events).hours(9, 12);
        let mut buf = Buffer::empty(Rect::new(0, 0, 26, 8));
        wv.render(buf.area(), &mut buf);
        // 26 - 6 = 20 over 2 cols → col_w 10. Column 1 starts at x = 6+10 = 16.
        // Grid: row 2.., 6 rows for 180 min → 2 rows/hr. 10:00 → grid row 2+2.
        let cell = buf.get(Position::new(16, 4)).unwrap();
        assert_eq!(cell.bg, Color::Blue);
        assert_eq!(cell.symbol, 'X'); // title on the block's first row
        // Column 0 same row is just grid background, not the event.
        assert_ne!(buf.get(Position::new(6, 4)).unwrap().bg, Color::Blue);
    }

    #[test]
    fn overlapping_events_tile_into_side_by_side_lanes() {
        // Two overlapping events on the same day → two lanes within the column.
        let events = [
            CalendarEvent::new(1, "A")
                .with_day(0)
                .with_span(9 * 60, 10 * 60)
                .with_color(Color::Red),
            CalendarEvent::new(2, "B")
                .with_day(0)
                .with_span(9 * 60 + 30, 10 * 60 + 30)
                .with_color(Color::Green),
        ];
        let wv = WeekView::new(0, 1).events(&events).hours(9, 11);
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 8));
        wv.render(buf.area(), &mut buf);
        // col_w = 16 - 6 = 10, two lanes → lane_w 5. A in lane 0 (x 6..11),
        // B in lane 1 (x 11..16). At grid row 2 (09:00) A is present, B not.
        assert_eq!(buf.get(Position::new(6, 2)).unwrap().bg, Color::Red);
        assert_ne!(buf.get(Position::new(13, 2)).unwrap().bg, Color::Red);
        // At 09:30 (grid row 3) both are live in their own lane.
        assert_eq!(buf.get(Position::new(6, 3)).unwrap().bg, Color::Red);
        assert_eq!(buf.get(Position::new(13, 3)).unwrap().bg, Color::Green);
    }

    #[test]
    fn all_day_events_render_in_the_all_day_band_not_the_grid() {
        let events = [CalendarEvent::new(1, "Holiday")
            .with_day(0)
            .with_all_day(true)
            .with_color(Color::Yellow)];
        let wv = WeekView::new(0, 2).events(&events);
        let mut buf = Buffer::empty(Rect::new(0, 0, 26, 8));
        wv.render(buf.area(), &mut buf);
        // Band is row 1. Column 0 starts at x = 6.
        let band = buf.get(Position::new(6, 1)).unwrap();
        assert_eq!(band.bg, Color::Yellow);
        assert_eq!(band.symbol, 'H');
        // Nothing tinted yellow anywhere in the grid rows (2..).
        for y in 2..8 {
            for x in 0..26 {
                assert_ne!(buf.get(Position::new(x, y)).unwrap().bg, Color::Yellow);
            }
        }
    }

    #[test]
    fn a_multi_day_event_spans_the_all_day_band_on_every_covered_column() {
        let events = [CalendarEvent::new(1, "Trip")
            .with_day(0)
            .with_end_day(2)
            .with_color(Color::Cyan)];
        let wv = WeekView::new(0, 3).events(&events);
        let mut buf = Buffer::empty(Rect::new(0, 0, 36, 6));
        wv.render(buf.area(), &mut buf);
        // col_w = 10; columns at x = 6, 16, 26 — all three carry the band.
        assert_eq!(buf.get(Position::new(6, 1)).unwrap().bg, Color::Cyan);
        assert_eq!(buf.get(Position::new(16, 1)).unwrap().bg, Color::Cyan);
        assert_eq!(buf.get(Position::new(26, 1)).unwrap().bg, Color::Cyan);
    }

    #[test]
    fn an_event_outside_the_visible_columns_is_skipped() {
        // Event on day 9, but only days 0..3 are visible.
        let events = [CalendarEvent::new(1, "Far")
            .with_day(9)
            .with_span(10 * 60, 11 * 60)
            .with_color(Color::Magenta)];
        let wv = WeekView::new(0, 3).events(&events);
        let mut buf = Buffer::empty(Rect::new(0, 0, 36, 8));
        wv.render(buf.area(), &mut buf);
        for cell in buf.cells() {
            assert_ne!(cell.bg, Color::Magenta);
        }
    }

    #[test]
    fn the_now_rule_is_drawn_only_inside_the_window() {
        // now = 10:00 inside a 09:00–12:00 window → a line across the grid.
        let inside = WeekView::new(0, 2).hours(9, 12).now(Some(10 * 60));
        let mut buf = Buffer::empty(Rect::new(0, 0, 26, 8));
        inside.render(buf.area(), &mut buf);
        // Grid 6 rows for 180 min → 2 rows/hr; 10:00 → grid_top(2)+2 = row 4.
        assert_eq!(buf.get(Position::new(8, 4)).unwrap().symbol, '─');

        // now = 07:00 outside the 09:00–12:00 window → no rule anywhere.
        let outside = WeekView::new(0, 2).hours(9, 12).now(Some(7 * 60));
        let mut buf2 = Buffer::empty(Rect::new(0, 0, 26, 8));
        outside.render(buf2.area(), &mut buf2);
        for cell in buf2.cells() {
            assert_ne!(cell.symbol, '─');
        }
    }

    #[test]
    fn today_accents_only_its_own_header_column() {
        let labels = ["Mon", "Tue", "Wed"];
        let wv = WeekView::new(10, 3)
            .day_labels(&labels)
            .today(Some(11)) // axis day 11 = column 1
            .header_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 36, 6));
        wv.render(buf.area(), &mut buf);
        // Column 1 header band (x = 16..26) is blue; column 0 (x = 6) is not.
        assert_eq!(buf.get(Position::new(16, 0)).unwrap().bg, Color::Blue);
        assert_ne!(buf.get(Position::new(6, 0)).unwrap().bg, Color::Blue);
    }

    #[test]
    fn selected_event_takes_the_selected_style() {
        let events = [CalendarEvent::new(7, "Sel")
            .with_day(0)
            .with_span(9 * 60, 10 * 60)];
        let wv = WeekView::new(0, 1)
            .events(&events)
            .hours(9, 11)
            .selected_event(Some(7))
            .selected_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 8));
        wv.render(buf.area(), &mut buf);
        // The block (grid row 2, x = 6) carries the selected bg.
        assert_eq!(buf.get(Position::new(6, 2)).unwrap().bg, Color::Blue);
        // A different id selected highlights nothing.
        let events2 = [CalendarEvent::new(7, "Sel")
            .with_day(0)
            .with_span(9 * 60, 10 * 60)];
        let wv2 = WeekView::new(0, 1)
            .events(&events2)
            .hours(9, 11)
            .selected_event(Some(99))
            .selected_style(Style::new().bg(Color::Blue));
        let mut buf2 = Buffer::empty(Rect::new(0, 0, 16, 8));
        wv2.render(buf2.area(), &mut buf2);
        for cell in buf2.cells() {
            assert_ne!(cell.bg, Color::Blue);
        }
    }

    #[test]
    fn slot_at_maps_a_click_to_a_day_and_a_snapped_minute() {
        let wv = WeekView::new(100, 3).hours(9, 12);
        let area = Rect::new(0, 0, 36, 8);
        // body: x 6.., y 2.., width 30 (col_w 10), height 6 (180 min → 30/row).
        // Click column 1, two rows down from grid top.
        let (day, min) = wv.slot_at(area, Position::new(16, 4)).unwrap();
        assert_eq!(day, 101); // start_day 100 + column 1
        // row 2 → 2*30 = 60 min past 09:00 = 600; snapped to 15 → 600.
        assert_eq!(min, 9 * 60 + 60);
        // A click in the ruler / header / band is outside the body → None.
        assert_eq!(wv.slot_at(area, Position::new(2, 0)), None);
        assert_eq!(wv.slot_at(area, Position::new(16, 1)), None);
        // The snapped minute is always a multiple of 15.
        let (_, m) = wv.slot_at(area, Position::new(16, 5)).unwrap();
        assert_eq!(m % 15, 0);
    }

    #[test]
    fn event_at_resolves_a_click_to_the_block_under_it() {
        let events = [
            CalendarEvent::new(1, "A")
                .with_day(0)
                .with_span(9 * 60, 10 * 60),
            CalendarEvent::new(2, "B")
                .with_day(1)
                .with_span(10 * 60, 11 * 60),
        ];
        let wv = WeekView::new(0, 2).events(&events).hours(9, 12);
        let area = Rect::new(0, 0, 26, 8);
        // col_w 10. A is column 0, 09:00 → grid row 2. B is column 1, 10:00 →
        // grid row 4 (180 min over 6 rows → 2 rows/hr).
        assert_eq!(wv.event_at(area, Position::new(6, 2)), Some(1));
        assert_eq!(wv.event_at(area, Position::new(16, 4)), Some(2));
        // Empty grid cell → None.
        assert_eq!(wv.event_at(area, Position::new(16, 2)), None);
        // Outside the body entirely → None.
        assert_eq!(wv.event_at(area, Position::new(0, 0)), None);
    }

    #[test]
    fn body_and_all_day_band_rects_match_the_render_geometry() {
        let wv = WeekView::new(0, 5);
        let area = Rect::new(0, 0, 46, 12);
        let body = wv.body(area);
        assert_eq!((body.x, body.y), (6, 2)); // right of ruler, below band
        assert_eq!((body.width, body.height), (40, 10));
        let band = wv.all_day_band(area);
        assert_eq!((band.x, band.y, band.width, band.height), (6, 1, 40, 1));
    }

    #[test]
    fn day_count_is_clamped_to_one_through_fourteen() {
        // 0 clamps up to 1.
        let zero = WeekView::new(0, 0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 6));
        zero.clone().render(buf.area(), &mut buf); // no panic
        assert_eq!(zero.body(Rect::new(0, 0, 20, 6)).width, 14); // one column
        // 99 clamps down to 14.
        let many = WeekView::new(0, 99);
        let mut buf2 = Buffer::empty(Rect::new(0, 0, 80, 6));
        many.clone().render(buf2.area(), &mut buf2); // no panic
        // 80 - 6 = 74 over 14 cols → col_w 5; slot_at sees 14 columns.
        let (d, _) = many
            .slot_at(Rect::new(0, 0, 80, 6), Position::new(75, 2))
            .unwrap();
        assert_eq!(d, 13); // last of the 14 clamped columns
    }

    #[test]
    fn an_inverted_or_empty_hours_window_is_clamped() {
        // end <= start clamps to a one-hour window at start; never a panic and
        // never a divide-by-zero.
        let wv = WeekView::new(0, 1).hours(14, 14);
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 6));
        wv.render(buf.area(), &mut buf);
        // The single visible hour is 14:00.
        let row2: String = (0..5)
            .map(|x| buf.get(Position::new(x, 2)).unwrap().symbol)
            .collect();
        assert_eq!(row2, "14:00");
        // end > 24 clamps to 24.
        let wv2 = WeekView::new(0, 1).hours(23, 99);
        let mut buf2 = Buffer::empty(Rect::new(0, 0, 12, 4));
        wv2.render(buf2.area(), &mut buf2); // no panic
    }

    #[test]
    fn a_block_frames_the_view_in_the_inner_area() {
        let labels = ["Mo"];
        let wv = WeekView::new(0, 1)
            .day_labels(&labels)
            .block(Block::bordered());
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 6));
        wv.clone().render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(19, 5)).unwrap().symbol, '┘');
        // Inner Rect(1,1,18,4): ruler at x=1, header at inner row 1 (y=1),
        // the day column starts at x = 1 + 6 = 7.
        assert_eq!(buf.get(Position::new(7, 1)).unwrap().symbol, 'M');
        // body() is reported relative to the inner area.
        let body = wv.body(Rect::new(0, 0, 20, 6));
        assert_eq!((body.x, body.y), (7, 3));
    }

    #[test]
    fn a_too_narrow_area_clips_to_the_ruler_without_panicking() {
        // Width 4 — narrower than the 6-col ruler, no room for a day column.
        let wv = WeekView::new(0, 7).hours(9, 11);
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 6));
        wv.clone().render(buf.area(), &mut buf); // must not panic
        // The ruler text is still drawn (clipped) so the pane reads as a week.
        let row2: String = (0..4)
            .map(|x| buf.get(Position::new(x, 2)).unwrap().symbol)
            .collect();
        assert_eq!(row2, "09:0"); // "09:00" clipped at width 4
        // Hit-testing a clipped pane is a clean None, not a panic.
        assert_eq!(wv.slot_at(Rect::new(0, 0, 4, 6), Position::new(2, 3)), None);
    }

    #[test]
    fn zero_area_and_no_events_are_no_ops() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 6));
        WeekView::new(0, 7).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        // No events, real area: a clear, ruler + header only, never a panic.
        let mut buf2 = Buffer::empty(Rect::new(0, 0, 26, 8));
        WeekView::new(0, 3).render(buf2.area(), &mut buf2);
        // Hit-testing an empty body rect is None, not a panic.
        let wv = WeekView::new(0, 3);
        assert_eq!(
            wv.event_at(Rect::new(0, 0, 0, 0), Position::new(0, 0)),
            None
        );
        assert_eq!(wv.slot_at(Rect::new(0, 0, 0, 0), Position::new(0, 0)), None);
        assert!(wv.body(Rect::new(0, 0, 0, 0)).is_empty());
        assert!(wv.all_day_band(Rect::new(0, 0, 0, 0)).is_empty());
    }

    #[test]
    fn an_event_wholly_outside_the_window_does_not_draw() {
        // 07:00–08:00 event, window 09:00–12:00 → clamped to zero-length, no
        // block drawn, never a panic.
        let events = [CalendarEvent::new(1, "Early")
            .with_day(0)
            .with_span(7 * 60, 8 * 60)
            .with_color(Color::Red)];
        let wv = WeekView::new(0, 1).events(&events).hours(9, 12);
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 8));
        wv.render(buf.area(), &mut buf);
        for cell in buf.cells() {
            assert_ne!(cell.bg, Color::Red);
        }
    }
}
