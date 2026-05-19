//! [`DayView`] — a single-day timeline: an hour ruler, an all-day band, and
//! one wide event column with overlap tiling and a now-line — the focused-day
//! surface of a scheduling TUI (the one-column, richer sibling of
//! [`WeekView`](crate::WeekView)).
//!
//! # A pure projection, like every other calendar view
//!
//! `DayView` owns no state. It borrows a caller-owned
//! `&[`[`CalendarEvent`]`]` and projects the events that
//! fall on one caller-axis [`day`](DayView::new) — exactly as
//! [`MonthView`](crate::MonthView) / [`WeekView`](crate::WeekView) project the
//! same slice, and as [`Markdown`](crate::Markdown) projects a caller-owned
//! `&[Link]`. The reducer decides which events exist and what the integer day
//! axis means (days since the caller's epoch, a day-of-month, a column index —
//! the widget never interprets the unit); the widget only orders, tiles, and
//! stamps them. Overlapping timed events are tiled side-by-side by the one
//! genuinely shared algorithm, [`event::pack_day`](crate::event::pack_day),
//! the layout it shares with [`WeekView`](crate::WeekView).
//!
//! # Dependency-free on purpose: the widget does no date math
//!
//! [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)
//! §4 gates any widget that pulls a transitive dependency behind a Cargo
//! feature. A day view that computed a calendar date would need
//! `chrono`/`time`; `DayView` instead takes the day as a **caller-owned
//! integer** and the column header as a **caller-formatted string**
//! ([`day_label`](DayView::day_label)) — it does **no date arithmetic at
//! all**, exactly the [`Calendar`](crate::Calendar) / [`Gantt`](crate::Gantt)
//! axis discipline. Turning a minute-of-day into `HH:00` for the ruler is pure
//! clock arithmetic on a caller integer ([`time_label`]),
//! *not* calendar math (see the [event model docs](crate::event)), so it pulls
//! in no dependency and needs no feature gate, and the widget stays a
//! deterministically headless-testable pure projection like
//! [`List`](crate::List).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no events, an hour window with `end <= start` (clamped to one hour),
//! hours past `24` (clamped), a [`now`](DayView::now) outside the window (the
//! line is simply not drawn), a [`selected_event`](DayView::selected_event)
//! id that matches nothing, and an area too narrow or short for the ruler or
//! the blocks are all safe clamps/clips — never a panic. An optional framing
//! [`Block`] follows the container-widget convention.

use std::borrow::Cow;

use rstui_core::{Buffer, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::event::{CalendarEvent, pack_day, time_label};

/// The hour-ruler gutter: `"HH:00"` (5 columns) plus a one-column separator so
/// the ruler never visually merges with the first event block.
const RULER_W: u16 = 6;

/// The most all-day / multi-day rows the band will ever draw, so a day with a
/// pathological number of holidays can never push the time grid off-screen.
/// Extra all-day events past this are summarised in the last row.
const MAX_ALL_DAY_ROWS: u16 = 3;

/// A single-day timeline as a pure projection of a caller-owned event slice:
/// a header, an all-day band, a left hour ruler with per-hour grid lines, and
/// one wide event column where overlapping timed events tile side-by-side via
/// [`event::pack_day`](crate::event::pack_day), with an optional now-line and
/// an accented selected event.
///
/// `DayView` does **no date math** — it is handed the caller-axis
/// [`day`](Self::new), a caller-formatted [`day_label`](Self::day_label), and
/// the borrowed [`events`](Self::events); it draws the ones that
/// [`cover that day`](crate::CalendarEvent::covers_day). The visible hour
/// window is [`hours`](Self::hours) (default the whole day). Styling is a base
/// [`Style`] (filling the area) with a [`header_style`](Self::header_style),
/// a [`ruler_style`](Self::ruler_style), an
/// [`all_day_style`](Self::all_day_style), a [`grid_style`](Self::grid_style)
/// for the hour lines, a [`now_style`](Self::now_style) for the now-line, and
/// a [`selected_style`](Self::selected_style) accent; each block is tinted by
/// its event's own [`color`](crate::CalendarEvent::color).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{CalendarEvent, DayView};
///
/// // One 09:00–10:00 event on caller-axis day 12, ruler 08:00..=10:00.
/// let events = [CalendarEvent::new(1, "Standup")
///     .with_day(12)
///     .with_span(9 * 60, 10 * 60)];
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 24, 6));
/// DayView::new(12)
///     .events(&events)
///     .day_label("Tue 12")
///     .hours(8, 10)
///     .render(buf.area(), &mut buf);
///
/// // Row 0 is the caller-formatted header.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'T'); // "Tue 12"
/// // The ruler's first hour label, one row below the header.
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '0'); // "08:00"
/// ```
#[derive(Debug, Clone)]
pub struct DayView<'a> {
    day: i64,
    events: &'a [CalendarEvent<'a>],
    day_label: Cow<'a, str>,
    start_h: u16,
    end_h: u16,
    now: Option<u16>,
    selected_event: Option<u64>,
    block: Option<Block<'a>>,
    style: Style,
    ruler_style: Style,
    header_style: Style,
    all_day_style: Style,
    now_style: Style,
    grid_style: Style,
    selected_style: Style,
}

impl<'a> DayView<'a> {
    /// A day view focused on caller-axis `day`, with no events, an empty
    /// header, and the whole `00:00..=24:00` hour window.
    ///
    /// `day` is whatever integer day axis the caller's model uses — the widget
    /// never interprets it (see the [module docs](self)). Attach the events
    /// with [`events`](Self::events) and a header with
    /// [`day_label`](Self::day_label).
    pub fn new(day: i64) -> Self {
        Self {
            day,
            events: &[],
            day_label: Cow::Borrowed(""),
            start_h: 0,
            end_h: 24,
            now: None,
            selected_event: None,
            block: None,
            style: Style::default(),
            ruler_style: Style::default(),
            header_style: Style::default(),
            all_day_style: Style::default(),
            now_style: Style::default(),
            grid_style: Style::default(),
            selected_style: Style::default(),
        }
    }

    /// Borrows the caller-owned event slice. Only events that
    /// [`cover`](crate::CalendarEvent::covers_day) this view's day are drawn;
    /// the rest are ignored (the widget does no date math — the caller's model
    /// owns which events exist).
    #[must_use]
    pub fn events(mut self, events: &'a [CalendarEvent<'a>]) -> Self {
        self.events = events;
        self
    }

    /// Sets the caller-formatted header text (e.g. `"Tue 12 May"`). The widget
    /// formats **no** date — this is whatever string the caller's model holds
    /// (see the [module docs](self)).
    #[must_use]
    pub fn day_label(mut self, label: impl Into<Cow<'a, str>>) -> Self {
        self.day_label = label.into();
        self
    }

    /// Sets the visible hour window `[start_h, end_h]` (default `0, 24`). Both
    /// are clamped to `0..=24` and `end_h` is forced above `start_h` (a window
    /// is at least one hour) — never a panic.
    #[must_use]
    pub fn hours(mut self, start_h: u16, end_h: u16) -> Self {
        self.start_h = start_h.min(23);
        self.end_h = end_h.clamp(self.start_h + 1, 24);
        self
    }

    /// Sets the now-line at `Some(minute_of_day)`, or `None`. The line is only
    /// drawn when the minute falls inside the visible
    /// [`hours`](Self::hours) window; otherwise it is silently skipped.
    #[must_use]
    pub fn now(mut self, now: Option<u16>) -> Self {
        self.now = now;
        self
    }

    /// Sets the accented (selected) event by [`id`](crate::CalendarEvent::id),
    /// or `None`. An id matching no drawn event simply accents nothing.
    #[must_use]
    pub fn selected_event(mut self, id: Option<u64>) -> Self {
        self.selected_event = id;
        self
    }

    /// Frames the view in `block`; the timeline renders into
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

    /// Sets the [`Style`] for the hour-ruler gutter, over the base.
    #[must_use]
    pub fn ruler_style(mut self, style: Style) -> Self {
        self.ruler_style = style;
        self
    }

    /// Sets the [`Style`] for the day header row, over the base.
    #[must_use]
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Sets the [`Style`] for the all-day band, over the base (then each chip
    /// is tinted by its own event [`color`](crate::CalendarEvent::color)).
    #[must_use]
    pub fn all_day_style(mut self, style: Style) -> Self {
        self.all_day_style = style;
        self
    }

    /// Sets the [`Style`] for the now-line, over the base.
    #[must_use]
    pub fn now_style(mut self, style: Style) -> Self {
        self.now_style = style;
        self
    }

    /// Sets the [`Style`] for the per-hour grid lines, over the base.
    #[must_use]
    pub fn grid_style(mut self, style: Style) -> Self {
        self.grid_style = style;
        self
    }

    /// Sets the [`Style`] patched over the selected event's block.
    #[must_use]
    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// The clamped visible hour window `[start_h, end_h]` (`end > start`,
    /// both `<= 24`) — the single source of truth shared by `render` and the
    /// accessors so geometry can never desync.
    fn window(&self) -> (u16, u16) {
        let s = self.start_h.min(23);
        let e = self.end_h.clamp(s + 1, 24);
        (s, e)
    }

    /// The number of all-day band rows for the current events, capped at
    /// [`MAX_ALL_DAY_ROWS`] (so the time grid can never be pushed off-screen).
    fn all_day_rows(&self) -> u16 {
        let n = self
            .events
            .iter()
            .filter(|e| (e.all_day() || e.multi_day()) && e.covers_day(self.day))
            .count();
        (n as u16).min(MAX_ALL_DAY_ROWS)
    }

    /// The hour-grid rectangle: the timed-event surface *excluding* the ruler
    /// gutter — the rect [`minute_at`](Self::minute_at) and
    /// [`event_at`](Self::event_at) map a [`Position`] into. Mirrors the
    /// `render` layout exactly; an area too small for the header/ruler/band
    /// yields a zero-area rect.
    #[must_use]
    pub fn body(&self, area: Rect) -> Rect {
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if inner.is_empty() {
            return Rect::ZERO;
        }
        // Row 0 is the header; then the all-day band; the rest is the grid.
        let band = self.all_day_rows();
        let consumed = 1u16.saturating_add(band);
        let grid_top = inner.top().saturating_add(consumed);
        let grid_h = inner.height.saturating_sub(consumed);
        let grid_x = inner.left().saturating_add(RULER_W);
        let grid_w = inner.width.saturating_sub(RULER_W);
        if grid_w == 0 || grid_h == 0 {
            return Rect::ZERO;
        }
        Rect::new(grid_x, grid_top, grid_w, grid_h)
    }

    /// The minute-of-day at `pos` within the hour grid, snapped down to the
    /// nearest 15 minutes (for click-to-create / drag), or `None` if `pos` is
    /// outside [`body`](Self::body). The inverse of the row mapping `render`
    /// uses, so a click lands on the slot it visually points at.
    #[must_use]
    pub fn minute_at(&self, area: Rect, pos: Position) -> Option<u16> {
        let body = self.body(area);
        if body.is_empty() || !body.contains(pos) {
            return None;
        }
        let (s, e) = self.window();
        let win_min = u32::from(e - s) * 60;
        let row = u32::from(pos.y - body.top());
        let rows = u32::from(body.height);
        // Linear row→minute, then snap to a 15-minute slot.
        let minute = u32::from(s) * 60 + row * win_min / rows;
        let snapped = (minute / 15) * 15;
        Some(snapped.min(u32::from(crate::event::MINUTES_PER_DAY)) as u16)
    }

    /// The [`id`](crate::CalendarEvent::id) of the timed event whose block
    /// covers `pos`, or `None`. Mirrors `render`'s tiling exactly (same
    /// [`pack_day`] columns and row mapping), so a
    /// click resolves to the block drawn under the cursor. The all-day band is
    /// not hit-tested (those events live above the grid).
    #[must_use]
    pub fn event_at(&self, area: Rect, pos: Position) -> Option<u64> {
        let body = self.body(area);
        if body.is_empty() || !body.contains(pos) {
            return None;
        }
        let (s, e) = self.window();
        let win_lo = u32::from(s) * 60;
        let win_hi = u32::from(e) * 60;
        let rows = u32::from(body.height);

        let timed: Vec<&CalendarEvent> = self
            .events
            .iter()
            .filter(|ev| ev.covers_day(self.day) && !ev.all_day())
            .collect();
        let laid = pack_day(&timed);
        // Walk in reverse so a later-drawn (higher-column) block, which paints
        // over an earlier one where they share a cell, wins the hit-test —
        // matching what the user sees.
        for layout in laid.iter().rev() {
            let (bx, bw) = column_span(body, layout.column, layout.columns);
            if pos.x < bx || pos.x >= bx.saturating_add(bw) {
                continue;
            }
            let (top, h) = block_rows(
                u32::from(layout.start_min),
                u32::from(layout.end_min),
                win_lo,
                win_hi,
                body.top(),
                rows,
            );
            if pos.y >= top && pos.y < top.saturating_add(h) {
                return Some(layout.id);
            }
        }
        None
    }
}

/// The `[x, width)` of cluster column `column` of `columns` across the grid
/// `body`, every column equal width with the remainder spread over the leftmost
/// columns so the lanes always tile the full width with no gap.
fn column_span(body: Rect, column: u16, columns: u16) -> (u16, u16) {
    let cols = columns.max(1);
    let col = column.min(cols - 1);
    let w = body.width / cols;
    let extra = body.width % cols;
    // The first `extra` columns are one cell wider; offset is the running sum.
    let x = u32::from(col) * u32::from(w) + u32::from(col.min(extra));
    let width = w + u16::from(col < extra);
    (body.left().saturating_add(x as u16), width)
}

/// The `[top, height)` of a timed `[start_min, end_min]` span clipped to the
/// visible window `[win_lo, win_hi)` minutes, mapped linearly across `rows`
/// rows from `grid_top`. Height is at least `1` so a short meeting is never
/// invisible; an entirely off-window span yields height `0`.
fn block_rows(
    start_min: u32,
    end_min: u32,
    win_lo: u32,
    win_hi: u32,
    grid_top: u16,
    rows: u32,
) -> (u16, u16) {
    let win = win_hi - win_lo; // window always >= 60 minutes
    let s = start_min.clamp(win_lo, win_hi);
    let e = end_min.clamp(win_lo, win_hi).max(s);
    if e <= win_lo || s >= win_hi {
        return (grid_top, 0);
    }
    let y0 = (s - win_lo) * rows / win;
    let y1 = (e - win_lo) * rows / win;
    let top = grid_top.saturating_add(y0 as u16);
    let height = (y1.saturating_sub(y0).max(1) as u16).min(rows.saturating_sub(y0) as u16);
    (top, height.max(1))
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

impl Widget for DayView<'_> {
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

        let left = inner.left();
        let right = inner.right();
        let bottom = inner.bottom();
        let (s_h, e_h) = self.window();

        // Row 0: the caller-formatted header.
        put(
            buf,
            &self.day_label,
            self.style.patch(self.header_style),
            left,
            inner.top(),
            right,
        );

        // The all-day band: events that are all-day OR multi-day and cover
        // this day, one per row, capped at MAX_ALL_DAY_ROWS so the grid is
        // never pushed away. The cap row, when overflowing, gets a "+N" tail.
        let band_rows = self.all_day_rows();
        let all_day: Vec<&CalendarEvent> = self
            .events
            .iter()
            .filter(|e| (e.all_day() || e.multi_day()) && e.covers_day(self.day))
            .collect();
        let band_base = self.style.patch(self.all_day_style);
        for row in 0..band_rows {
            let y = inner.top().saturating_add(1).saturating_add(row);
            if y >= bottom {
                break;
            }
            // The last band row absorbs every remaining overflow event.
            let is_last = row + 1 == band_rows;
            let overflow = all_day.len() as u16 > MAX_ALL_DAY_ROWS;
            if is_last && overflow {
                let hidden = all_day.len() as u16 - (MAX_ALL_DAY_ROWS - 1);
                let label = format!("+{hidden} more all-day");
                put(buf, &label, band_base, left, y, right);
                continue;
            }
            let ev = all_day[row as usize];
            let chip = band_base.patch(Style::new().fg(ev.color()));
            // Whole-row tint so the chip reads as a band, then the title.
            buf.set_style(Rect::new(left, y, inner.width, 1), chip);
            put(buf, ev.title(), chip, left, y, right);
        }

        // The hour grid: everything below the header + band.
        let consumed = 1u16.saturating_add(band_rows);
        let grid_top = inner.top().saturating_add(consumed);
        let grid_h = inner.height.saturating_sub(consumed);
        let grid_x = left.saturating_add(RULER_W);
        let grid_w = inner.width.saturating_sub(RULER_W);
        if grid_h == 0 || grid_w == 0 {
            return;
        }
        let body = Rect::new(grid_x, grid_top, grid_w, grid_h);
        let win_lo = u32::from(s_h) * 60;
        let win_hi = u32::from(e_h) * 60;
        let win = win_hi - win_lo;
        let rows = u32::from(grid_h);

        // Per-hour ruler labels + grid lines. The label sits on the row the
        // hour boundary maps to; the faint grid line fills the rest of that
        // row across the event column (drawn first, so blocks paint over it).
        let ruler_glyph = self.style.patch(self.ruler_style);
        let grid_glyph = self.style.patch(self.grid_style);
        for h in s_h..=e_h {
            let minute = u32::from(h) * 60;
            // The half-open grid maps the bottom boundary (`== e_h`) to
            // exactly `rows`; pin its label to the last row so the closing
            // hour ("HH:00") is still visible (the standard calendar gutter).
            let yo = ((minute - win_lo) * rows / win).min(rows - 1);
            let y = grid_top.saturating_add(yo as u16);
            // "HH:00" in the ruler gutter.
            put(
                buf,
                &time_label(minute as u16),
                ruler_glyph,
                left,
                y,
                grid_x,
            );
            // The faint hour grid line across the event column (drawn before
            // the blocks so they paint over it).
            for x in grid_x..right {
                buf.set_cell(Position::new(x, y), '·', grid_glyph);
            }
        }

        // Timed events: those covering this day that are not all-day. Tile
        // them across the single wide column by pack_day's (column, columns).
        let timed: Vec<&CalendarEvent> = self
            .events
            .iter()
            .filter(|e| e.covers_day(self.day) && !e.all_day())
            .collect();
        let laid = pack_day(&timed);
        for layout in &laid {
            let ev = match timed.iter().find(|e| e.id() == layout.id) {
                Some(e) => *e,
                None => continue,
            };
            let (bx, bw) = column_span(body, layout.column, layout.columns);
            if bw == 0 {
                continue;
            }
            let (top, h) = block_rows(
                u32::from(layout.start_min),
                u32::from(layout.end_min),
                win_lo,
                win_hi,
                grid_top,
                rows,
            );
            if h == 0 {
                continue;
            }
            let mut fill = self.style.patch(Style::new().bg(ev.color()));
            if self.selected_event == Some(ev.id()) {
                fill = fill.patch(self.selected_style);
            }
            let block_rect = Rect::new(bx, top, bw, h).intersection(body);
            if block_rect.is_empty() {
                continue;
            }
            buf.set_style(block_rect, fill);
            // Title on the first row; a "HH:MM–HH:MM" + location subtitle when
            // the block is at least 3 rows tall (the richer-than-WeekView
            // affordance), each clipped to the block's right edge.
            let bx_right = bx.saturating_add(bw);
            put(buf, ev.title(), fill, bx, top, bx_right);
            if h >= 3 {
                let span = format!(
                    "{}–{}",
                    time_label(ev.start_min()),
                    time_label(ev.end_min())
                );
                put(buf, &span, fill, bx, top.saturating_add(1), bx_right);
                if !ev.location().is_empty() {
                    put(
                        buf,
                        ev.location(),
                        fill,
                        bx,
                        top.saturating_add(2),
                        bx_right,
                    );
                }
            }
        }

        // The now-line, drawn last over the whole event column so it reads as
        // one rule. Only when the minute is inside the visible window.
        if let Some(n) = self.now {
            let nm = u32::from(n);
            if nm >= win_lo && nm < win_hi {
                let yo = (nm - win_lo) * rows / win;
                if (yo as u16) < grid_h {
                    let y = grid_top.saturating_add(yo as u16);
                    let marker = self.style.patch(self.now_style);
                    for x in grid_x..right {
                        buf.set_cell(Position::new(x, y), '▔', marker);
                    }
                    // A one-cell caret in the ruler's separator column points
                    // at the now-row without clobbering an "HH:00" label.
                    if grid_x > left {
                        buf.set_cell(Position::new(grid_x.saturating_sub(1), y), '►', marker);
                    }
                }
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

    fn ev(
        id: u64,
        title: &'static str,
        day: i64,
        sh: u16,
        sm: u16,
        eh: u16,
        em: u16,
    ) -> CalendarEvent<'static> {
        CalendarEvent::new(id, title)
            .with_day(day)
            .with_span(sh * 60 + sm, eh * 60 + em)
    }

    #[test]
    fn header_is_the_caller_formatted_label_on_row_zero() {
        let dv = DayView::new(12).day_label("Tue 12 May");
        let out = lines(dv, 24, 4);
        let row0 = out.lines().next().unwrap();
        assert!(row0.starts_with("Tue 12 May"), "got {row0:?}");
    }

    #[test]
    fn ruler_shows_hour_labels_for_the_window() {
        // Window 08..=10 → labels 08:00, 09:00, 10:00. No band (no events).
        let dv = DayView::new(0).hours(8, 10);
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 7));
        dv.render(buf.area(), &mut buf);
        // Grid starts at row 1 (row 0 = header). 08:00 boundary maps to row 1.
        let mut r1 = String::new();
        for x in 0..5 {
            r1.push(buf.get(Position::new(x, 1)).unwrap().symbol);
        }
        assert_eq!(r1, "08:00");
        // The bottom boundary 10:00 maps to the last grid row.
        let last = 1 + 6 - 1; // grid_h = 6, grid_top = 1
        let mut rl = String::new();
        for x in 0..5 {
            rl.push(buf.get(Position::new(x, last)).unwrap().symbol);
        }
        assert_eq!(rl, "10:00");
    }

    #[test]
    fn a_timed_event_tints_its_block_in_its_own_color() {
        let events = [ev(1, "Standup", 5, 9, 0, 10, 0).with_color(Color::Cyan)];
        let dv = DayView::new(5).events(&events).hours(8, 12);
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 9));
        dv.render(buf.area(), &mut buf);
        // Grid: row 1.., x >= RULER_W. 09:00 in an 08..=12 window over 8 rows
        // → row offset (60*8)/(240) = 2 → grid row 1+2 = 3. Block bg = Cyan.
        let cell = buf.get(Position::new(RULER_W, 3)).unwrap();
        assert_eq!(cell.bg, Color::Cyan);
        // The title is stamped on the block's first row.
        assert_eq!(cell.symbol, 'S');
    }

    #[test]
    fn overlapping_events_tile_into_side_by_side_columns() {
        // A 09:00–10:00, B 09:30–10:30 overlap → two columns of equal-ish
        // width across the event area.
        let events = [
            ev(1, "A", 0, 9, 0, 10, 0).with_color(Color::Red),
            ev(2, "B", 0, 9, 30, 10, 30).with_color(Color::Blue),
        ];
        let dv = DayView::new(0).events(&events).hours(9, 11);
        let mut buf = Buffer::empty(Rect::new(0, 0, 26, 9));
        dv.render(buf.area(), &mut buf);
        // body x = RULER_W=6, width = 26-6 = 20 → two 10-wide columns.
        // 09:00 maps to grid row 1 (top). Column 0 = A (Red) at x=6.
        assert_eq!(buf.get(Position::new(6, 1)).unwrap().bg, Color::Red);
        // Column 1 = B (Blue) at x = 6 + 10 = 16, first B row.
        // 09:30 in 09..=11 over 8 rows → (30*8)/120 = 2 → grid row 1+2 = 3.
        assert_eq!(buf.get(Position::new(16, 3)).unwrap().bg, Color::Blue);
        // The two lanes tile the full width: x=15 (last of col 0) is still A.
        assert_eq!(buf.get(Position::new(15, 1)).unwrap().bg, Color::Red);
    }

    #[test]
    fn an_all_day_event_draws_in_the_band_not_the_grid() {
        let events = [CalendarEvent::new(1, "Holiday")
            .with_day(3)
            .with_all_day(true)
            .with_color(Color::Green)];
        let dv = DayView::new(3).events(&events).hours(8, 10);
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
        dv.render(buf.area(), &mut buf);
        // Band is row 1 (after the header). Whole row tinted Green + title.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'H');
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().fg, Color::Green);
        // The grid (and its ruler) now starts at row 2.
        let mut r2 = String::new();
        for x in 0..5 {
            r2.push(buf.get(Position::new(x, 2)).unwrap().symbol);
        }
        assert_eq!(r2, "08:00");
    }

    #[test]
    fn a_multi_day_event_covering_this_day_goes_in_the_band() {
        // Spans day 4..6; viewing day 5 → in the band even though it is not
        // flagged all-day.
        let events = [CalendarEvent::new(1, "Conf")
            .with_day(4)
            .with_end_day(6)
            .with_span(9 * 60, 17 * 60)
            .with_color(Color::Magenta)];
        let dv = DayView::new(5).events(&events).hours(8, 10);
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
        dv.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'C');
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().fg, Color::Magenta);
    }

    #[test]
    fn events_not_covering_the_day_are_ignored() {
        let events = [ev(1, "Elsewhere", 99, 9, 0, 10, 0).with_color(Color::Red)];
        let dv = DayView::new(5).events(&events).hours(8, 12);
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 9));
        dv.render(buf.area(), &mut buf);
        // No Red anywhere — the event is on another day.
        assert!(buf.cells().iter().all(|c| c.bg != Color::Red));
    }

    #[test]
    fn a_tall_block_shows_a_time_and_location_subtitle() {
        let events = [ev(1, "Workshop", 0, 9, 0, 13, 0)
            .with_color(Color::Blue)
            .with_location("Room 4")];
        // 09..=13 over many rows → the 4-hour block is well over 3 tall.
        let dv = DayView::new(0).events(&events).hours(9, 13);
        let mut buf = Buffer::empty(Rect::new(0, 0, 28, 14));
        dv.render(buf.area(), &mut buf);
        // Row 1 = title. Row 2 = "09:00–13:00". Row 3 = "Room 4".
        let read = |y: u16| {
            let mut s = String::new();
            for x in RULER_W..28 {
                s.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            s.trim_end().to_string()
        };
        assert!(read(1).starts_with("Workshop"));
        assert!(read(2).starts_with("09:00–13:00"), "got {:?}", read(2));
        assert!(read(3).starts_with("Room 4"), "got {:?}", read(3));
    }

    #[test]
    fn a_short_block_is_still_at_least_one_row_tall() {
        // A 5-minute event in a 12-hour window over a short grid still draws.
        let events = [ev(1, "Quick", 0, 9, 0, 9, 5).with_color(Color::Yellow)];
        let dv = DayView::new(0).events(&events).hours(0, 24);
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 9));
        dv.render(buf.area(), &mut buf);
        // Some cell in the event column carries the Yellow bg.
        assert!(buf.cells().iter().any(|c| c.bg == Color::Yellow));
    }

    #[test]
    fn the_now_line_draws_only_inside_the_window() {
        let events: [CalendarEvent; 0] = [];
        // now = 10:00 inside 08..=12 → a rule of '▔' across the event column.
        let dv = DayView::new(0)
            .events(&events)
            .hours(8, 12)
            .now(Some(10 * 60))
            .now_style(Style::new().fg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 9));
        dv.render(buf.area(), &mut buf);
        let has_now = (RULER_W..24).any(|x| {
            (1..9).any(|y| {
                let c = buf.get(Position::new(x, y)).unwrap();
                c.symbol == '▔' && c.fg == Color::Red
            })
        });
        assert!(has_now, "expected a now-line in the window");
    }

    #[test]
    fn a_now_outside_the_window_is_not_drawn() {
        let events: [CalendarEvent; 0] = [];
        let dv = DayView::new(0)
            .events(&events)
            .hours(8, 12)
            .now(Some(2 * 60)) // 02:00, before the 08:00 window
            .now_style(Style::new().fg(Color::Red));
        let out = lines(dv, 24, 9);
        assert!(!out.contains('▔'), "now-line should not be drawn");
    }

    #[test]
    fn the_selected_event_takes_the_selected_style() {
        let events = [
            ev(1, "A", 0, 9, 0, 10, 0).with_color(Color::Blue),
            ev(2, "B", 0, 11, 0, 12, 0).with_color(Color::Blue),
        ];
        let dv = DayView::new(0)
            .events(&events)
            .hours(8, 13)
            .selected_event(Some(2))
            .selected_style(Style::new().bg(Color::Yellow));
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 12));
        dv.render(buf.area(), &mut buf);
        // Some cell carries the selected Yellow bg (event 2); event 1 keeps
        // its own Blue bg somewhere.
        assert!(buf.cells().iter().any(|c| c.bg == Color::Yellow));
        assert!(buf.cells().iter().any(|c| c.bg == Color::Blue));
    }

    #[test]
    fn minute_at_snaps_to_a_fifteen_minute_slot() {
        let dv = DayView::new(0).hours(8, 12); // 4h window
        let area = Rect::new(0, 0, 24, 9);
        let body = dv.body(area);
        assert!(!body.is_empty());
        // Top of the grid = 08:00 exactly.
        assert_eq!(
            dv.minute_at(area, Position::new(body.left(), body.top())),
            Some(8 * 60)
        );
        // A click below the grid is None.
        assert_eq!(
            dv.minute_at(area, Position::new(body.left(), body.bottom())),
            None
        );
        // A click in the ruler gutter (x < RULER_W) is None (outside body).
        assert_eq!(dv.minute_at(area, Position::new(0, body.top())), None);
        // Every returned minute is a multiple of 15.
        for y in body.top()..body.bottom() {
            if let Some(m) = dv.minute_at(area, Position::new(body.left(), y)) {
                assert_eq!(m % 15, 0, "minute {m} not snapped at row {y}");
            }
        }
    }

    #[test]
    fn event_at_resolves_the_block_under_the_cursor() {
        let events = [ev(1, "A", 0, 9, 0, 10, 0), ev(2, "B", 0, 11, 0, 12, 0)];
        let dv = DayView::new(0).events(&events).hours(9, 13);
        let area = Rect::new(0, 0, 24, 12);
        let body = dv.body(area);
        // 09:00 maps to the top grid row → event 1.
        assert_eq!(
            dv.event_at(area, Position::new(body.left(), body.top())),
            Some(1)
        );
        // 11:00–12:00 is event 2. 11:00 in 09..=13 over body.height rows:
        // offset = 120 * h / 240 = h/2 → that row resolves to event 2.
        let half = body.top() + body.height / 2;
        assert_eq!(dv.event_at(area, Position::new(body.left(), half)), Some(2));
        // 10:30 is empty space between the two blocks → None (but inside the
        // grid, so it is a real "no event here", not an out-of-bounds miss).
        // Row for 10:30: offset = 90 * h / 240.
        let gap = body.top() + (90 * u32::from(body.height) / 240) as u16;
        assert_eq!(dv.event_at(area, Position::new(body.left(), gap)), None);
        // Anything outside the body (the header / ruler corner) → None.
        assert_eq!(dv.event_at(area, Position::new(0, 0)), None);
    }

    #[test]
    fn hours_clamps_a_reversed_or_out_of_range_window() {
        // end <= start and hours past 24 must clamp, never panic.
        let dv = DayView::new(0).hours(30, 2);
        let (s, e) = dv.window();
        assert!(s <= 23 && e <= 24 && e > s, "window ({s},{e}) invalid");
        // Render with the degenerate window: must not panic.
        let _ = lines(dv, 20, 6);
    }

    #[test]
    fn a_block_frames_the_view_in_the_inner_area() {
        let events = [ev(1, "X", 0, 9, 0, 10, 0)];
        let dv = DayView::new(0)
            .events(&events)
            .hours(9, 11)
            .block(Block::bordered().title("Today"));
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
        // `body` is queried before `render` consumes the widget (it is a
        // pure function of the same layout inputs).
        let body = dv.body(Rect::new(0, 0, 20, 8));
        dv.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(19, 7)).unwrap().symbol, '┘');
        // The grid lives inside the frame; body is offset by the border.
        assert!(body.left() >= 1 && body.top() >= 1);
        assert!(!body.is_empty());
    }

    #[test]
    fn no_events_renders_a_clean_ruler_and_grid_only() {
        let dv = DayView::new(0).hours(8, 10);
        let out = lines(dv, 20, 6);
        // No panic, has the ruler labels, no event glyphs.
        assert!(out.contains("08:00"));
        assert!(out.contains("10:00"));
    }

    #[test]
    fn a_tiny_area_clips_without_panicking() {
        let events = [ev(1, "Standup", 0, 9, 0, 10, 0).with_color(Color::Cyan)];
        // Degenerate areas (1×1, 3×2, ruler-only width, single row): each
        // must render without panicking and keep the accessors total — a
        // zero-area body returns `None`/empty for every query.
        for (w, h) in [(1, 1), (3, 2), (RULER_W - 1, 4), (8, 1), (RULER_W + 1, 2)] {
            let area = Rect::new(0, 0, w, h);
            let dv = DayView::new(0).events(&events).hours(8, 12);
            let body = dv.body(area);
            let mut buf = Buffer::empty(area);
            dv.render(buf.area(), &mut buf);

            let q = DayView::new(0).events(&events).hours(8, 12);
            if body.is_empty() {
                assert_eq!(q.minute_at(area, Position::new(0, 0)), None);
                assert_eq!(q.event_at(area, Position::new(0, 0)), None);
            } else {
                // The body's own origin is the window start (08:00).
                assert_eq!(
                    q.minute_at(area, body.position()),
                    Some(8 * 60),
                    "body {body:?} for area {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let events = [ev(1, "X", 0, 9, 0, 10, 0)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
        DayView::new(0)
            .events(&events)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn the_all_day_band_caps_at_three_rows_with_an_overflow_tail() {
        // Five all-day events; band caps at MAX_ALL_DAY_ROWS and the last row
        // becomes a "+N more all-day" summary so the grid stays on-screen.
        let events: Vec<CalendarEvent> = (0..5)
            .map(|i| CalendarEvent::new(i, "All").with_day(0).with_all_day(true))
            .collect();
        let dv = DayView::new(0).events(&events).hours(8, 10);
        assert_eq!(dv.all_day_rows(), MAX_ALL_DAY_ROWS);
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 10));
        dv.render(buf.area(), &mut buf);
        // The 3rd band row (grid_top-1) shows the overflow summary.
        let band_last = 1 + MAX_ALL_DAY_ROWS - 1; // header row + 2
        let mut row = String::new();
        for x in 0..10 {
            row.push(buf.get(Position::new(x, band_last)).unwrap().symbol);
        }
        assert!(row.starts_with("+3 more"), "got {row:?}");
    }
}
