//! [`AgendaView`] — a chronological, day-grouped event list (the
//! "schedule" / list calendar view), a pure projection of a caller-owned
//! `&[CalendarEvent]` plus a [`List`](crate::List)-style scroll offset.
//!
//! # A pure projection, like every other calendar view
//!
//! `AgendaView` owns no state. It is handed a caller-owned
//! `&[CalendarEvent]` — the same shared model
//! [`MonthView`](crate::MonthView)/[`WeekView`](crate::WeekView)/
//! [`DayView`](crate::DayView) project — plus a caller-owned scroll
//! [`offset`](AgendaView::offset) and an optional
//! [`selected`](AgendaView::selected) event id, and only renders them. It
//! groups the events by [`day`](crate::CalendarEvent::day) ascending then
//! [`start_min`](crate::CalendarEvent::start_min), flattens that into a flat
//! list of *rows* (a day-header row, then one row per event), and draws the
//! window `[offset, offset + height)` exactly as [`List`](crate::List)
//! windows its items: the scroll offset is ordinary application state the
//! reducer owns and changes in `update`, never mutated here (rstui's
//! `App::view` takes `&self`). [`row_count`](AgendaView::row_count) hands the
//! reducer the total flattened-row count so it can clamp that scroll.
//!
//! # Dependency-free on purpose: the view does no date math
//!
//! [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)
//! §4 gates any widget that pulls a transitive dependency behind a Cargo
//! feature. An agenda that turned a day index into "Mon 18 May" would need
//! `chrono`/`time`; `AgendaView` instead takes the day→header text as
//! **caller-owned** [`day_labels`](AgendaView::day_labels) (an axis-day → text
//! map the reducer or a date crate of the caller's choosing fills) and falls
//! back to `"Day {n}"` for an unlabelled day — the exact
//! [`Calendar`](crate::Calendar) caller-owned-date-facts discipline. The day
//! axis is an opaque `i64` (days since the caller's epoch, a column index — the
//! view never interprets the unit, the [`Gantt`](crate::Gantt) axis rule), and
//! a time range is formatted only via [`time_label`] — pure clock arithmetic
//! on a caller integer, not calendar math. So it adds
//! no dependency and needs no feature gate.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: no
//! events draws a centred [`empty_text`](AgendaView::empty_text), an
//! [`offset`](AgendaView::offset) past the last row leaves a blank pane, a
//! tiny area clips, and an out-of-range [`selected`](AgendaView::selected) id
//! simply paints no accent — never a panic.

use std::borrow::Cow;

use rstui_core::{Buffer, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::event::{CalendarEvent, time_label};

/// One flattened row of the agenda: either a day header or a single event.
///
/// The render and the [`event_at`](AgendaView::event_at) /
/// [`row_count`](AgendaView::row_count) accessors all walk this one shared
/// projection so the geometry can never desync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    /// A day-header row for the given caller-axis day.
    Header(i64),
    /// An event row: an index into the caller's `&[CalendarEvent]`.
    Event(usize),
}

/// A chronological, day-grouped event list — the "schedule" / list calendar
/// view — as a pure projection of a caller-owned `&[CalendarEvent]` and a
/// [`List`](crate::List)-style scroll [`offset`](Self::offset).
///
/// Events are grouped by [`day`](CalendarEvent::day) ascending then
/// [`start_min`](CalendarEvent::start_min) and flattened to rows: a day-header
/// row (the caller's [`day_labels`](Self::day_labels) text, or `"Day {n}"`)
/// then one row per event — `"HH:MM–HH:MM  ● Title  @loc"`, or `"all day  ●
/// Title"` for an all-day event, with a `→ +Nd` hint appended for a multi-day
/// event. The window `[offset, offset + height)` is drawn into the inner area;
/// the [`selected`](Self::selected) event's row is patched with
/// [`selected_style`](Self::selected_style). An empty event slice draws a
/// centred [`empty_text`](Self::empty_text).
///
/// The day axis is an opaque `i64` and the view does **no date math** (see the
/// [module docs](self)).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{AgendaView, CalendarEvent};
///
/// let events = [
///     CalendarEvent::new(1, "Standup").with_day(0).with_span(9 * 60, 9 * 60 + 30),
/// ];
/// let labels = [(0_i64, "Mon")];
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 30, 2));
/// AgendaView::new(&events)
///     .day_labels(&labels)
///     .render(buf.area(), &mut buf);
///
/// // Row 0 is the day header, row 1 the event ("09:00–09:30  ● Standup").
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'M'); // "Mon"
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '0'); // "09:00…"
/// ```
#[derive(Debug, Clone)]
pub struct AgendaView<'a> {
    events: &'a [CalendarEvent<'a>],
    day_labels: &'a [(i64, &'a str)],
    offset: usize,
    selected: Option<u64>,
    block: Option<Block<'a>>,
    style: Style,
    day_header_style: Style,
    time_style: Style,
    selected_style: Style,
    empty_text: Cow<'a, str>,
}

impl<'a> AgendaView<'a> {
    /// An agenda over the caller-owned `events`, scrolled to the top, nothing
    /// selected.
    ///
    /// The slice is **borrowed**, never collected — the pure-projection seam
    /// (the same one [`List::from_slice`](crate::List::from_slice) uses).
    pub fn new(events: &'a [CalendarEvent<'a>]) -> Self {
        Self {
            events,
            day_labels: &[],
            offset: 0,
            selected: None,
            block: None,
            style: Style::default(),
            day_header_style: Style::default(),
            time_style: Style::default(),
            selected_style: Style::default(),
            empty_text: Cow::Borrowed("No events"),
        }
    }

    /// Sets the caller-owned axis-day → header-text map (e.g.
    /// `(18, "Mon 18 May")`). The view does **no date math**; an unlabelled
    /// day falls back to `"Day {n}"` (see the [module docs](self)). The first
    /// entry matching a day wins.
    #[must_use]
    pub fn day_labels(mut self, day_labels: &'a [(i64, &'a str)]) -> Self {
        self.day_labels = day_labels;
        self
    }

    /// Sets the index of the first visible flattened row (the scroll offset),
    /// the [`List`](crate::List) idiom. Caller-owned, never mutated here; an
    /// offset past the last row simply leaves a blank pane (clamp it with
    /// [`row_count`](Self::row_count)).
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Sets the highlighted event by [`id`](CalendarEvent::id), or `None`. An
    /// id not in the slice simply paints no accent.
    #[must_use]
    pub fn selected(mut self, selected: Option<u64>) -> Self {
        self.selected = selected;
        self
    }

    /// Frames the agenda in `block`; rows render into
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

    /// Sets the [`Style`] for a day-header row, over the base.
    #[must_use]
    pub fn day_header_style(mut self, style: Style) -> Self {
        self.day_header_style = style;
        self
    }

    /// Sets the [`Style`] for the leading `HH:MM–HH:MM` / `all day` time
    /// column of an event row, over the base.
    #[must_use]
    pub fn time_style(mut self, style: Style) -> Self {
        self.time_style = style;
        self
    }

    /// Sets the [`Style`] patched over the [`selected`](Self::selected)
    /// event's row (a full-width bar, patched **last** so it wins).
    #[must_use]
    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Sets the text centred when there are no events (default `"No events"`).
    #[must_use]
    pub fn empty_text(mut self, empty_text: impl Into<Cow<'a, str>>) -> Self {
        self.empty_text = empty_text.into();
        self
    }

    /// The caller's header text for axis-day `day`, or `None` when unlabelled.
    fn label_for(&self, day: i64) -> Option<&'a str> {
        self.day_labels
            .iter()
            .find_map(|&(d, t)| (d == day).then_some(t))
    }

    /// The flattened row list: events grouped by [`day`](CalendarEvent::day)
    /// ascending then [`start_min`](CalendarEvent::start_min), with a
    /// [`Row::Header`] before each day's run. The single shared projection
    /// `render` / [`event_at`](Self::event_at) / [`row_count`](Self::row_count)
    /// all walk, so the geometry can never desync.
    fn rows(&self) -> Vec<Row> {
        // Stable sort of event *indices* by (day, start_min, id): stable so
        // equal keys keep input order, deterministic regardless of input.
        let mut order: Vec<usize> = (0..self.events.len()).collect();
        order.sort_by(|&a, &b| {
            let (ea, eb) = (&self.events[a], &self.events[b]);
            ea.day()
                .cmp(&eb.day())
                .then(ea.start_min().cmp(&eb.start_min()))
                .then(ea.id().cmp(&eb.id()))
        });

        let mut rows = Vec::with_capacity(order.len() + 8);
        let mut last_day: Option<i64> = None;
        for idx in order {
            let day = self.events[idx].day();
            if last_day != Some(day) {
                rows.push(Row::Header(day));
                last_day = Some(day);
            }
            rows.push(Row::Event(idx));
        }
        rows
    }

    /// One event row's display string, e.g. `"09:00–09:30  ● Standup  @Room 4"`
    /// or `"all day  ● Holiday"`, with a `→ +2d` hint for a multi-day event.
    fn event_text(&self, e: &CalendarEvent<'a>) -> String {
        let mut s = if e.all_day() {
            "all day  ".to_string()
        } else {
            // En dash between the two clock labels (clock arithmetic on a
            // caller integer — not date math; see the module docs).
            format!(
                "{}\u{2013}{}  ",
                time_label(e.start_min()),
                time_label(e.end_min())
            )
        };
        s.push('\u{25cf}'); // ● accent bullet
        s.push(' ');
        s.push_str(e.title());
        if !e.location().is_empty() {
            s.push_str("  @");
            s.push_str(e.location());
        }
        if e.multi_day() {
            // `span_days` is >= 1; a multi-day event spans 2+, so this is >= 1.
            s.push_str(&format!("  \u{2192} +{}d", e.span_days().saturating_sub(1)));
        }
        s
    }

    /// The event [`id`](CalendarEvent::id) under cell `pos` for `area`, if a
    /// (non-header) event row is there.
    ///
    /// The pure inverse of the render layout — clicking what you see picks
    /// that event. It accounts for the framing [`block`](Self::block) and the
    /// caller-owned scroll [`offset`](Self::offset) once, here, instead of
    /// every app re-deriving it. `None` over a day-header row, the empty-state
    /// text, or off the populated rows.
    #[must_use]
    pub fn event_at(&self, area: Rect, pos: Position) -> Option<u64> {
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if inner.is_empty() || !inner.contains(pos) {
            return None;
        }
        let rows = self.rows();
        let row_idx = self.offset + usize::from(pos.y - inner.top());
        match rows.get(row_idx) {
            Some(Row::Event(i)) => Some(self.events[*i].id()),
            _ => None,
        }
    }

    /// The total flattened-row count (day headers plus event rows) so the
    /// reducer can clamp the scroll [`offset`](Self::offset). `0` when there
    /// are no events.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows().len()
    }

    /// The one-row, full-inner-width rectangle of the populated list row
    /// under `pos` — the **same-size, aligned** band a drag ghost snaps to so
    /// the user sees which row position the event points at, instead of a
    /// floating box. A pure function of `area`, the framing
    /// [`block`](Self::block) and the caller-owned [`offset`](Self::offset)
    /// (the same inner/row arithmetic [`event_at`](Self::event_at) uses);
    /// empty over the blank pane past the last row, the empty-state text, or
    /// outside the list (never a panic).
    #[must_use]
    pub fn row_rect(&self, area: Rect, pos: Position) -> Rect {
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if inner.is_empty() || !inner.contains(pos) {
            return Rect::ZERO;
        }
        let row_idx = self.offset + usize::from(pos.y - inner.top());
        if row_idx >= self.rows().len() {
            return Rect::ZERO;
        }
        Rect::new(inner.left(), pos.y, inner.width, 1)
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

impl Widget for AgendaView<'_> {
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
        let top = inner.top();
        let bottom = inner.bottom();

        // No events → the empty-state text, centred in the inner area.
        if self.events.is_empty() {
            let text = self.empty_text.as_ref();
            let w = (text.chars().count() as u16).min(inner.width);
            let x = left + (inner.width - w) / 2;
            let y = top + inner.height / 2;
            put(buf, text, self.style, x, y, right);
            return;
        }

        let rows = self.rows();
        let header_base = self.style.patch(self.day_header_style);
        let time_base = self.style.patch(self.time_style);

        for (row, item) in rows
            .iter()
            .enumerate()
            .skip(self.offset)
            .take(inner.height as usize)
        {
            let y = top.saturating_add((row - self.offset) as u16);
            if y >= bottom {
                break;
            }
            match item {
                Row::Header(day) => {
                    // Caller-owned label, or the no-date-math `"Day {n}"`
                    // fallback.
                    match self.label_for(*day) {
                        Some(t) => put(buf, t, header_base, left, y, right),
                        None => put(buf, &format!("Day {day}"), header_base, left, y, right),
                    }
                }
                Row::Event(i) => {
                    let e = &self.events[*i];
                    let is_selected = self.selected == Some(e.id());
                    if is_selected {
                        // Full-width selection bar, patched last so it wins
                        // over the time/base styling, exactly the `List` rule.
                        buf.set_style(Rect::new(left, y, inner.width, 1), self.selected_style);
                    }

                    let text = self.event_text(e);
                    // The leading time/`all day` column carries `time_style`
                    // (under any accent colour the event itself sets); the
                    // selection patch wins last on both segments.
                    let time_len = if e.all_day() {
                        "all day  ".chars().count()
                    } else {
                        // "HH:MM–HH:MM  " — both labels are clamped HH:MM.
                        time_label(e.start_min()).chars().count()
                            + 1
                            + time_label(e.end_min()).chars().count()
                            + 2
                    };

                    let mut time_st = time_base;
                    let mut body_st = self.style;
                    if e.color() != rstui_core::Color::Reset {
                        body_st = body_st.fg(e.color());
                    }
                    if is_selected {
                        time_st = time_st.patch(self.selected_style);
                        body_st = body_st.patch(self.selected_style);
                    }

                    let mut x = left;
                    for (i, ch) in text.chars().enumerate() {
                        if x >= right {
                            break;
                        }
                        let st = if i < time_len { time_st } else { body_st };
                        buf.set_cell(Position::new(x, y), ch, st);
                        x = x.saturating_add(1);
                    }
                }
            }
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

    fn sample() -> Vec<CalendarEvent<'static>> {
        vec![
            CalendarEvent::new(1, "Standup")
                .with_day(0)
                .with_span(9 * 60, 9 * 60 + 30),
            CalendarEvent::new(2, "Lunch")
                .with_day(0)
                .with_span(12 * 60, 13 * 60)
                .with_location("Cafe"),
            CalendarEvent::new(3, "Trip")
                .with_day(1)
                .with_end_day(3)
                .with_all_day(true),
        ]
    }

    #[test]
    fn groups_by_day_with_a_header_then_one_row_per_event() {
        let ev = sample();
        let labels = [(0_i64, "Mon"), (1, "Tue")];
        let out = lines(AgendaView::new(&ev).day_labels(&labels), 40, 6);
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[0].starts_with("Mon"));
        assert!(rows[1].starts_with("09:00\u{2013}09:30  \u{25cf} Standup"));
        assert!(rows[2].starts_with("12:00\u{2013}13:00  \u{25cf} Lunch  @Cafe"));
        assert!(rows[3].starts_with("Tue"));
        // Multi-day all-day event: "all day" prefix + a "→ +2d" hint.
        assert!(rows[4].starts_with("all day  \u{25cf} Trip"));
        assert!(rows[4].contains("\u{2192} +2d"));
    }

    #[test]
    fn events_are_ordered_by_day_then_start_minute_regardless_of_input() {
        // Fed out of order (later first) to prove the deterministic sort.
        let ev = vec![
            CalendarEvent::new(2, "Late")
                .with_day(0)
                .with_span(15 * 60, 16 * 60),
            CalendarEvent::new(1, "Early")
                .with_day(0)
                .with_span(8 * 60, 9 * 60),
        ];
        let out = lines(AgendaView::new(&ev), 30, 3);
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[0].starts_with("Day 0")); // unlabelled fallback
        assert!(rows[1].contains("Early"));
        assert!(rows[2].contains("Late"));
    }

    #[test]
    fn an_unlabelled_day_falls_back_to_day_n() {
        let ev = vec![CalendarEvent::new(1, "X").with_day(42)];
        let out = lines(AgendaView::new(&ev), 12, 2);
        assert_eq!(out.lines().next().unwrap(), "Day 42      ");
    }

    #[test]
    fn offset_scrolls_the_flattened_rows_list_idiom() {
        let ev = sample();
        let labels = [(0_i64, "Mon"), (1, "Tue")];
        // Rows: 0 Mon, 1 Standup, 2 Lunch, 3 Tue, 4 Trip. Offset 3 → Tue first.
        let out = lines(AgendaView::new(&ev).day_labels(&labels).offset(3), 30, 2);
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[0].starts_with("Tue"));
        assert!(rows[1].starts_with("all day  \u{25cf} Trip"));
    }

    #[test]
    fn an_offset_past_the_last_row_leaves_a_blank_pane() {
        let ev = sample();
        let out = lines(AgendaView::new(&ev).offset(999), 10, 2);
        assert_eq!(out, "          \n          \n");
    }

    #[test]
    fn row_count_is_headers_plus_event_rows() {
        let ev = sample();
        // 2 day headers + 3 events = 5.
        assert_eq!(AgendaView::new(&ev).row_count(), 5);
        assert_eq!(AgendaView::new(&[]).row_count(), 0);
    }

    #[test]
    fn no_events_draws_the_centred_empty_text() {
        let out = lines(AgendaView::new(&[]).empty_text("Nothing"), 11, 3);
        // 7-wide "Nothing" centred in 11 → x=2, y=1.
        assert_eq!(out.lines().nth(1).unwrap(), "  Nothing  ");
        assert_eq!(out.lines().next().unwrap(), "           ");
    }

    #[test]
    fn the_default_empty_text_is_no_events() {
        let out = lines(AgendaView::new(&[]), 9, 1);
        assert_eq!(out, "No events\n");
    }

    #[test]
    fn the_selected_event_row_takes_the_selected_style() {
        let ev = sample();
        let v = AgendaView::new(&ev)
            .selected(Some(2)) // "Lunch"
            .selected_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 6));
        v.render(buf.area(), &mut buf);
        // Row 2 is the Lunch row; the whole width is the selection bar.
        for x in 0..30 {
            assert_eq!(buf.get(Position::new(x, 2)).unwrap().bg, Color::Blue);
        }
        // The Standup row (row 1) is untouched.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn an_out_of_range_selected_id_paints_no_accent() {
        let ev = sample();
        let v = AgendaView::new(&ev)
            .selected(Some(999))
            .selected_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 6));
        v.render(buf.area(), &mut buf);
        for cell in buf.cells() {
            assert_ne!(cell.bg, Color::Blue);
        }
    }

    #[test]
    fn event_at_inverts_the_layout_with_offset_and_block() {
        let ev = sample();
        let labels = [(0_i64, "Mon"), (1, "Tue")];
        let v = AgendaView::new(&ev).day_labels(&labels);
        let area = Rect::new(0, 0, 40, 6);
        // Row 0 = header (None), row 1 = Standup (id 1), row 2 = Lunch (id 2).
        assert_eq!(v.event_at(area, Position::new(5, 0)), None);
        assert_eq!(v.event_at(area, Position::new(5, 1)), Some(1));
        assert_eq!(v.event_at(area, Position::new(20, 2)), Some(2));
        assert_eq!(v.event_at(area, Position::new(0, 99)), None); // off-area
        // The offset shifts the mapping: row 0 is now the Tue header.
        let s = AgendaView::new(&ev).day_labels(&labels).offset(3);
        assert_eq!(s.event_at(area, Position::new(0, 0)), None); // Tue header
        assert_eq!(s.event_at(area, Position::new(0, 1)), Some(3)); // Trip
        // A framing block insets the rows by its border.
        let b = AgendaView::new(&ev).block(Block::bordered());
        let ba = Rect::new(0, 0, 40, 8);
        assert_eq!(b.event_at(ba, Position::new(2, 0)), None); // on the border
        assert_eq!(b.event_at(ba, Position::new(2, 2)), Some(1)); // Standup row
    }

    #[test]
    fn a_block_frames_the_agenda_in_the_inner_area() {
        let ev = vec![
            CalendarEvent::new(1, "X")
                .with_day(0)
                .with_span(9 * 60, 10 * 60),
        ];
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 4));
        AgendaView::new(&ev)
            .block(Block::bordered())
            .render(buf.area(), &mut buf);
        // The frame corners.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '\u{250c}'); // ┌
        assert_eq!(buf.get(Position::new(23, 0)).unwrap().symbol, '\u{2510}'); // ┐
        assert_eq!(buf.get(Position::new(0, 3)).unwrap().symbol, '\u{2514}'); // └
        // Inner row 0 (buffer row 1) is the "Day 0" header, inside the border.
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'D');
        // Inner row 1 (buffer row 2) is the event "09:00–10:00  ● X".
        let r2: String = (1..23)
            .map(|x| buf.get(Position::new(x, 2)).unwrap().symbol)
            .collect();
        assert!(r2.starts_with("09:00\u{2013}10:00  \u{25cf} X"));
    }

    #[test]
    fn an_empty_agenda_with_a_block_still_renders_the_block() {
        // Inner is Rect(1,1,3,1); "No events" is clipped to its first 3 cells.
        assert_eq!(
            lines(AgendaView::new(&[]).block(Block::bordered()), 5, 3),
            "\u{250c}\u{2500}\u{2500}\u{2500}\u{2510}\n\u{2502}No \u{2502}\n\u{2514}\u{2500}\u{2500}\u{2500}\u{2518}\n"
        );
    }

    #[test]
    fn a_narrow_area_clips_each_row() {
        let ev = vec![
            CalendarEvent::new(1, "Meeting")
                .with_day(0)
                .with_span(9 * 60, 10 * 60),
        ];
        // Width 5: header "Day 0" exactly fills it; the event row is clipped.
        let out = lines(AgendaView::new(&ev), 5, 2);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows[0], "Day 0");
        assert_eq!(rows[1], "09:00");
    }

    #[test]
    fn the_event_accent_colour_tints_the_title_segment() {
        let ev = vec![
            CalendarEvent::new(1, "X")
                .with_day(0)
                .with_span(9 * 60, 10 * 60)
                .with_color(Color::Magenta),
        ];
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 2));
        AgendaView::new(&ev).render(buf.area(), &mut buf);
        // "09:00–10:00  " is 13 cols (time column); the bullet after it is the
        // body segment and carries the event's accent colour.
        assert_eq!(buf.get(Position::new(13, 1)).unwrap().symbol, '\u{25cf}');
        assert_eq!(buf.get(Position::new(13, 1)).unwrap().fg, Color::Magenta);
        // The time column does not take the accent.
        assert_ne!(buf.get(Position::new(0, 1)).unwrap().fg, Color::Magenta);
    }

    #[test]
    fn the_time_column_and_day_header_take_their_styles() {
        let ev = vec![
            CalendarEvent::new(1, "X")
                .with_day(0)
                .with_span(9 * 60, 10 * 60),
        ];
        let v = AgendaView::new(&ev)
            .day_header_style(Style::new().add_modifier(Modifier::BOLD))
            .time_style(Style::new().fg(Color::Cyan));
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 2));
        v.render(buf.area(), &mut buf);
        // Day-header row 0 is bold.
        assert!(
            buf.get(Position::new(0, 0))
                .unwrap()
                .modifier
                .contains(Modifier::BOLD)
        );
        // The leading time column on row 1 is cyan.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().fg, Color::Cyan);
    }

    #[test]
    fn the_base_style_fills_the_whole_content_area() {
        let ev = vec![CalendarEvent::new(1, "X").with_day(0)];
        let v = AgendaView::new(&ev).style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 4));
        v.render(buf.area(), &mut buf);
        for y in 0..4 {
            for x in 0..6 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let ev = sample();
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 6));
        AgendaView::new(&ev)
            .selected(Some(1))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn row_rect_is_a_one_row_band_at_a_populated_row() {
        let ev = sample();
        let v = AgendaView::new(&ev);
        let area = Rect::new(0, 0, 30, 12);
        let n = v.row_count();
        assert!(n >= 2);
        // A populated row: a full-inner-width, one-row-tall aligned band at
        // the pointer's row (the ghost is the same height as a list row).
        let r = v.row_rect(area, Position::new(5, area.y + 1));
        assert_eq!((r.x, r.y, r.width, r.height), (0, area.y + 1, 30, 1));
        // The blank pane past the last row, and a point outside the list,
        // both collapse to empty (total, never a panic).
        assert!(
            v.row_rect(area, Position::new(5, area.y + n as u16 + 1))
                .is_empty()
        );
        assert!(
            v.row_rect(area, Position::new(5, area.bottom() + 3))
                .is_empty()
        );
    }
}
