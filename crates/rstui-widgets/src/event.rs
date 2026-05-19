//! [`CalendarEvent`] — the caller-owned event model the calendar widget
//! family ([`MonthView`](crate::MonthView), [`WeekView`](crate::WeekView),
//! [`DayView`](crate::DayView), [`AgendaView`](crate::AgendaView),
//! [`YearView`](crate::YearView), [`EventCard`](crate::EventCard)) all
//! project — plus the one genuinely shared algorithm, [`pack_day`], that
//! tiles overlapping timed events into side-by-side columns.
//!
//! # A shared model, not a widget — the [`Link`](crate::Link) precedent
//!
//! Every calendar view is a *pure projection* of a caller-owned
//! `&[CalendarEvent]`, exactly as [`Markdown`](crate::Markdown) projects a
//! caller-owned `&[Link]`. Six widgets would otherwise each invent their own
//! event struct; instead this module owns it once, the views borrow it, and
//! the only behaviour here is the overlap-packing layout maths they all need
//! identically. The module is therefore a *model* (like [`link`](crate::link)),
//! not a [`Widget`](rstui_core::Widget): there is nothing to render on its own.
//! It uses the `with_*` builder + bare-getter shape (a model, not a widget
//! config — chained builders never collide with the getters its many
//! consumers read).
//!
//! # Dependency-free on purpose: the model does no date math
//!
//! [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)
//! §4 gates any widget that pulls a transitive dependency behind a Cargo
//! feature. A calendar that computed weekdays or parsed timestamps would need
//! `chrono`/`time`; `CalendarEvent` instead carries the date facts as
//! **caller-owned integers** — a [`day`](CalendarEvent::day)/
//! [`end_day`](CalendarEvent::end_day) on a *caller-chosen* integer day axis
//! (days since the caller's epoch, day-of-month, a column index — the model
//! never interprets the unit, exactly the [`Gantt`](crate::Gantt) axis
//! discipline) and a [`start_min`](CalendarEvent::start_min)/
//! [`end_min`](CalendarEvent::end_min) minute-of-day. The reducer (or a date
//! crate of the caller's choosing) supplies the numbers; this module only
//! orders, offsets, and packs them. Formatting a minute count as `HH:MM`
//! ([`time_label`]) is *clock arithmetic on a caller integer*, not calendar
//! math — the same justified-arithmetic line [`Calendar`](crate::Calendar)'s
//! `{day:>2}` sits on — so it pulls in no dependency and needs no feature
//! gate.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule, everything here is *total*: an
//! [`end_day`](CalendarEvent::end_day) before [`day`](CalendarEvent::day)
//! clamps to a single day, an [`end_min`](CalendarEvent::end_min) before
//! [`start_min`](CalendarEvent::start_min) clamps to a zero-length span,
//! minutes past midnight clamp to [`MINUTES_PER_DAY`], and [`pack_day`] of an
//! empty slice is an empty `Vec` — never a panic.

use std::borrow::Cow;

use rstui_core::Color;

/// Minutes in a day. [`start_min`](CalendarEvent::start_min) /
/// [`end_min`](CalendarEvent::end_min) are clamped to `0..=MINUTES_PER_DAY`.
pub const MINUTES_PER_DAY: u16 = 24 * 60;

/// A single calendar event: a caller-owned id and title plus an inclusive
/// `[day, end_day]` span on a caller-chosen integer day axis and a
/// `[start_min, end_min]` minute-of-day span, an [`all_day`](Self::all_day)
/// flag, an accent [`Color`], and optional `location`/`description` text for
/// the [`EventCard`](crate::EventCard) detail view.
///
/// The widget does **no date math** — `day`/`end_day` are whatever integer
/// day axis the caller's model uses (see the [module docs](self)). Build with
/// [`new`](Self::new) then the chained `with_*` setters; every input is
/// clamped at read time so a view can never panic on a malformed event.
///
/// # Example
///
/// ```
/// use rstui_core::Color;
/// use rstui_widgets::CalendarEvent;
///
/// // "Standup", day 12 of the caller's axis, 09:00–09:30.
/// let e = CalendarEvent::new(1, "Standup")
///     .with_day(12)
///     .with_span(9 * 60, 9 * 60 + 30)
///     .with_color(Color::Cyan);
///
/// assert_eq!(e.id(), 1);
/// assert_eq!(e.day(), 12);
/// assert_eq!(e.end_day(), 12); // single-day unless `with_end_day` is set
/// assert_eq!(e.duration_min(), 30);
/// assert!(!e.all_day());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEvent<'a> {
    id: u64,
    title: Cow<'a, str>,
    day: i64,
    end_day: i64,
    start_min: u16,
    end_min: u16,
    all_day: bool,
    color: Color,
    location: Cow<'a, str>,
    description: Cow<'a, str>,
}

impl<'a> CalendarEvent<'a> {
    /// A new event with caller-owned `id` and `title`, on day `0` of the
    /// caller's axis, spanning `00:00`–`00:00` (set a real span with
    /// [`with_span`](Self::with_span) or [`with_all_day`](Self::with_all_day)).
    pub fn new(id: u64, title: impl Into<Cow<'a, str>>) -> Self {
        Self {
            id,
            title: title.into(),
            day: 0,
            end_day: 0,
            start_min: 0,
            end_min: 0,
            all_day: false,
            color: Color::Reset,
            location: Cow::Borrowed(""),
            description: Cow::Borrowed(""),
        }
    }

    /// Sets the start day on the caller's integer day axis. Also moves
    /// [`end_day`](Self::end_day) to match when it would otherwise fall
    /// before the new start (an event is at least one day).
    #[must_use]
    pub fn with_day(mut self, day: i64) -> Self {
        self.day = day;
        if self.end_day < day {
            self.end_day = day;
        }
        self
    }

    /// Sets the inclusive end day (for a multi-day event). Clamped to be no
    /// earlier than [`day`](Self::day).
    #[must_use]
    pub fn with_end_day(mut self, end_day: i64) -> Self {
        self.end_day = end_day.max(self.day);
        self
    }

    /// Sets the minute-of-day span `[start, end]`. Both are clamped to
    /// `0..=`[`MINUTES_PER_DAY`]; an `end` before `start` collapses to a
    /// zero-length span at `start` (read back via [`end_min`](Self::end_min)).
    #[must_use]
    pub fn with_span(mut self, start: u16, end: u16) -> Self {
        self.start_min = start.min(MINUTES_PER_DAY);
        self.end_min = end.min(MINUTES_PER_DAY).max(self.start_min);
        self
    }

    /// Marks the event as all-day (drawn in a view's all-day band, not the
    /// time grid). The minute span is then ignored by the views.
    #[must_use]
    pub fn with_all_day(mut self, all_day: bool) -> Self {
        self.all_day = all_day;
        self
    }

    /// Sets the accent [`Color`] a view tints the event block with.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Sets the optional location shown by [`EventCard`](crate::EventCard).
    #[must_use]
    pub fn with_location(mut self, location: impl Into<Cow<'a, str>>) -> Self {
        self.location = location.into();
        self
    }

    /// Sets the optional long description shown by
    /// [`EventCard`](crate::EventCard).
    #[must_use]
    pub fn with_description(mut self, description: impl Into<Cow<'a, str>>) -> Self {
        self.description = description.into();
        self
    }

    /// The caller-owned event id.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The event title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The start day on the caller's integer day axis.
    #[must_use]
    pub fn day(&self) -> i64 {
        self.day
    }

    /// The inclusive end day (`== `[`day`](Self::day) for a single-day event).
    #[must_use]
    pub fn end_day(&self) -> i64 {
        self.end_day.max(self.day)
    }

    /// The number of day columns the event spans, always `>= 1`.
    #[must_use]
    pub fn span_days(&self) -> u32 {
        (self.end_day.max(self.day) - self.day + 1) as u32
    }

    /// The start minute-of-day (clamped to `0..=`[`MINUTES_PER_DAY`]).
    #[must_use]
    pub fn start_min(&self) -> u16 {
        self.start_min.min(MINUTES_PER_DAY)
    }

    /// The end minute-of-day, never before [`start_min`](Self::start_min).
    #[must_use]
    pub fn end_min(&self) -> u16 {
        self.end_min.min(MINUTES_PER_DAY).max(self.start_min())
    }

    /// The timed duration in minutes (`0` for a zero-length or all-day event).
    #[must_use]
    pub fn duration_min(&self) -> u16 {
        self.end_min().saturating_sub(self.start_min())
    }

    /// Whether the event is all-day.
    #[must_use]
    pub fn all_day(&self) -> bool {
        self.all_day
    }

    /// Whether the event spans more than one day.
    #[must_use]
    pub fn multi_day(&self) -> bool {
        self.end_day.max(self.day) > self.day
    }

    /// The accent [`Color`].
    #[must_use]
    pub fn color(&self) -> Color {
        self.color
    }

    /// The optional location (`""` when unset).
    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }

    /// The optional description (`""` when unset).
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Whether this event covers the caller-axis day `d` (inclusive of
    /// [`day`](Self::day)..=[`end_day`](Self::end_day)).
    #[must_use]
    pub fn covers_day(&self, d: i64) -> bool {
        d >= self.day && d <= self.end_day.max(self.day)
    }

    /// Whether this event's timed span overlaps `other`'s on a shared day.
    /// All-day events never overlap (they live in a separate band); touching
    /// endpoints (`a.end == b.start`) do **not** overlap, so back-to-back
    /// meetings tile into one column, not two.
    #[must_use]
    pub fn overlaps(&self, other: &CalendarEvent<'_>) -> bool {
        if self.all_day || other.all_day {
            return false;
        }
        self.start_min() < other.end_min() && other.start_min() < self.end_min()
    }
}

/// Where [`pack_day`] placed one timed event: its column and the total
/// column count of the overlap *cluster* it belongs to, so a view draws it at
/// `column / columns` of the day's width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventLayout {
    /// The packed event's [`CalendarEvent::id`].
    pub id: u64,
    /// This event's 0-based column within its overlap cluster.
    pub column: u16,
    /// The cluster's total column count (`>= 1`); every event in one
    /// mutually-overlapping run shares this so they tile to equal widths.
    pub columns: u16,
    /// The event's clamped start minute-of-day (echoed for the view's
    /// convenience — it need not re-clamp).
    pub start_min: u16,
    /// The event's clamped end minute-of-day.
    pub end_min: u16,
}

/// Packs the timed events of **one** day into side-by-side columns so
/// overlapping events tile rather than occlude — the layout
/// [`WeekView`](crate::WeekView) and [`DayView`](crate::DayView) share.
///
/// The classic interval-partitioning sweep: events are taken in
/// `(start, end, id)` order (deterministic regardless of input order); each
/// is assigned the lowest-indexed lane free at its start; a *cluster* is a
/// maximal run with no gap (a point where every lane has freed), and every
/// event in a cluster is given that cluster's peak lane count as
/// [`columns`](EventLayout::columns) so they render to equal widths.
///
/// All-day events are skipped (a view draws them in its all-day band). The
/// caller passes the events it has already decided fall on this day (the
/// model does no date math). Returns one [`EventLayout`] per *timed* input,
/// in the sorted order, keyed by [`id`](CalendarEvent::id).
///
/// # Example
///
/// ```
/// use rstui_widgets::{CalendarEvent, event::pack_day};
///
/// let a = CalendarEvent::new(1, "A").with_span(540, 600); // 09:00–10:00
/// let b = CalendarEvent::new(2, "B").with_span(570, 630); // 09:30–10:30 (overlaps A)
/// let c = CalendarEvent::new(3, "C").with_span(660, 720); // 11:00–12:00 (separate)
/// let laid = pack_day(&[&a, &b, &c]);
///
/// assert_eq!(laid[0].id, 1);
/// assert_eq!((laid[0].column, laid[0].columns), (0, 2)); // A | B share 2 cols
/// assert_eq!((laid[1].column, laid[1].columns), (1, 2));
/// assert_eq!((laid[2].column, laid[2].columns), (0, 1)); // C alone
/// ```
#[must_use]
pub fn pack_day(events: &[&CalendarEvent<'_>]) -> Vec<EventLayout> {
    // Timed events only, normalised to clamped (start, end), kept with their
    // original handle so the output is keyed by id.
    let mut timed: Vec<(u16, u16, u64)> = events
        .iter()
        .filter(|e| !e.all_day())
        .map(|e| (e.start_min(), e.end_min(), e.id()))
        .collect();
    // Deterministic order: by start, then end, then id (ties can't reorder).
    timed.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    let mut out: Vec<EventLayout> = Vec::with_capacity(timed.len());
    // `lane_end[i]` = the end minute of the event currently occupying lane i
    // within the open cluster (empty between clusters).
    let mut lane_end: Vec<u16> = Vec::new();
    // The half-open range of `out` indices forming the current cluster, and
    // the peak lane count seen in it (its eventual `columns`).
    let mut cluster_start = 0usize;
    let mut cluster_cols: u16 = 0;
    // The latest end seen in the open cluster: once an event starts at or
    // after it, every lane has freed and a new cluster begins.
    let mut cluster_end: u16 = 0;

    for (s, e, id) in timed {
        if !lane_end.is_empty() && s >= cluster_end {
            // Gap: close the cluster, stamping its peak column count onto
            // every member, then start a fresh one.
            for layout in &mut out[cluster_start..] {
                layout.columns = cluster_cols.max(1);
            }
            lane_end.clear();
            cluster_start = out.len();
            cluster_cols = 0;
        }
        // Lowest lane whose occupant has ended by `s` (touching is free).
        let col = match lane_end.iter().position(|&end| end <= s) {
            Some(i) => {
                lane_end[i] = e;
                i
            }
            None => {
                lane_end.push(e);
                lane_end.len() - 1
            }
        };
        cluster_cols = cluster_cols.max(lane_end.len() as u16);
        cluster_end = cluster_end.max(e);
        out.push(EventLayout {
            id,
            column: col as u16,
            columns: 1, // back-patched when the cluster closes
            start_min: s,
            end_min: e,
        });
    }
    // Close the final open cluster.
    for layout in &mut out[cluster_start..] {
        layout.columns = cluster_cols.max(1);
    }
    out
}

/// Formats a minute-of-day as 24-hour `HH:MM` (e.g. `540` → `"09:00"`).
///
/// Pure clock arithmetic on a caller integer — *not* calendar math (see the
/// [module docs](self)); minutes past midnight clamp to `24:00`.
#[must_use]
pub fn time_label(minute: u16) -> String {
    let m = minute.min(MINUTES_PER_DAY);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// Formats a minute-of-day as compact 12-hour `9am`/`1:05pm` — the label the
/// month/agenda chips use. Pure clock arithmetic; clamps to `24:00` →
/// `12:00am`.
#[must_use]
pub fn time_label_12h(minute: u16) -> String {
    let m = minute.min(MINUTES_PER_DAY) % MINUTES_PER_DAY;
    let (h24, mm) = (m / 60, m % 60);
    let period = if h24 < 12 { "am" } else { "pm" };
    let h12 = match h24 % 12 {
        0 => 12,
        h => h,
    };
    if mm == 0 {
        format!("{h12}{period}")
    } else {
        format!("{h12}:{mm:02}{period}")
    }
}

/// A foreground [`Color`] that stays legible drawn **on** an event-block fill
/// of `bg` — the contrast colour the time-grid/month views give an event's
/// label so a category tint never renders unreadable text.
///
/// A block tinted by an *arbitrary* caller [`color`](CalendarEvent::color)
/// must keep its title legible — the [`Gauge`](crate::Gauge) totality rule
/// applied to colour, not just geometry. Inheriting the panel's text colour
/// makes a near-white label on a light accent fill (or near-black on a dark
/// one) invisible. This picks near-black or near-white by the fill's
/// perceptual luminance (`0.299R + 0.587G + 0.114B`, the standard
/// coefficients). [`Color::Reset`] returns `Reset` so the caller can leave
/// the base style untouched (an uncoloured event reads in the panel's own
/// colours); named/indexed ANSI colours map by conventional brightness.
#[must_use]
pub fn readable_fg(bg: Color) -> Color {
    /// Near-black, for a light fill.
    const DARK: Color = Color::Rgb(20, 22, 26);
    /// Near-white, for a dark fill.
    const LIGHT: Color = Color::Rgb(245, 247, 250);
    /// Luminance (0..=255) above which a fill is "light" → use a dark fg.
    /// `128` keeps a saturated mid-green (≈136) on the dark side of the line
    /// (black text reads better on it) while a dark amber (≈106) stays light.
    const LIGHT_CUTOFF: u32 = 128;

    let bright = |r: u32, g: u32, b: u32| (299 * r + 587 * g + 114 * b) / 1000;
    let pick = |r, g, b| {
        if bright(r, g, b) > LIGHT_CUTOFF {
            DARK
        } else {
            LIGHT
        }
    };
    match bg {
        Color::Reset => Color::Reset,
        Color::Rgb(r, g, b) => pick(u32::from(r), u32::from(g), u32::from(b)),
        // The 6×6×6 colour cube (16..=231): reconstruct the RGB and reuse the
        // luminance test; the 24-step grey ramp (232..=255) is a line.
        Color::Indexed(i @ 16..=231) => {
            let c = u32::from(i - 16);
            let lvl = |v: u32| if v == 0 { 0 } else { 55 + 40 * v };
            pick(lvl(c / 36), lvl((c / 6) % 6), lvl(c % 6))
        }
        Color::Indexed(i @ 232..=255) => {
            if u32::from(i - 232) * 10 + 8 > LIGHT_CUTOFF {
                DARK
            } else {
                LIGHT
            }
        }
        // The conventionally *light* ANSI names take a dark fg; everything
        // else (the dark names, dim indexed `0..=15`, anything else) a light.
        Color::White
        | Color::Gray
        | Color::LightYellow
        | Color::LightGreen
        | Color::LightCyan
        | Color::Yellow => DARK,
        _ => LIGHT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_and_clamps_every_field() {
        let e = CalendarEvent::new(7, "Review")
            .with_day(10)
            .with_end_day(12)
            .with_span(13 * 60, 14 * 60)
            .with_color(Color::Magenta)
            .with_location("Room 4")
            .with_description("Quarterly review");
        assert_eq!(e.id(), 7);
        assert_eq!(e.title(), "Review");
        assert_eq!(e.day(), 10);
        assert_eq!(e.end_day(), 12);
        assert_eq!(e.span_days(), 3);
        assert!(e.multi_day());
        assert_eq!(e.start_min(), 780);
        assert_eq!(e.end_min(), 840);
        assert_eq!(e.duration_min(), 60);
        assert_eq!(e.color(), Color::Magenta);
        assert_eq!(e.location(), "Room 4");
        assert_eq!(e.description(), "Quarterly review");
    }

    #[test]
    fn end_before_start_clamps_to_zero_length_never_panics() {
        let e = CalendarEvent::new(1, "X").with_span(600, 300);
        assert_eq!(e.start_min(), 600);
        assert_eq!(e.end_min(), 600); // clamped up to start
        assert_eq!(e.duration_min(), 0);
    }

    #[test]
    fn end_day_before_day_clamps_to_single_day() {
        let e = CalendarEvent::new(1, "X").with_day(20).with_end_day(5);
        assert_eq!(e.end_day(), 20);
        assert_eq!(e.span_days(), 1);
        assert!(!e.multi_day());
    }

    #[test]
    fn day_setter_drags_a_stale_end_day_forward() {
        // end_day defaults to 0; setting with_day(9) must not leave end_day < day.
        let e = CalendarEvent::new(1, "X").with_day(9);
        assert_eq!(e.end_day(), 9);
        assert_eq!(e.span_days(), 1);
    }

    #[test]
    fn minutes_past_midnight_clamp() {
        let e = CalendarEvent::new(1, "X").with_span(5000, 6000);
        assert_eq!(e.start_min(), MINUTES_PER_DAY);
        assert_eq!(e.end_min(), MINUTES_PER_DAY);
    }

    #[test]
    fn covers_day_is_inclusive_of_the_whole_span() {
        let e = CalendarEvent::new(1, "X").with_day(10).with_end_day(12);
        assert!(!e.covers_day(9));
        assert!(e.covers_day(10));
        assert!(e.covers_day(11));
        assert!(e.covers_day(12));
        assert!(!e.covers_day(13));
    }

    #[test]
    fn overlap_excludes_all_day_and_touching_endpoints() {
        let a = CalendarEvent::new(1, "A").with_span(540, 600);
        let touch = CalendarEvent::new(2, "B").with_span(600, 660);
        let over = CalendarEvent::new(3, "C").with_span(590, 660);
        let allday = CalendarEvent::new(4, "D").with_all_day(true);
        assert!(!a.overlaps(&touch)); // 10:00 meets 10:00 — not an overlap
        assert!(a.overlaps(&over));
        assert!(!a.overlaps(&allday));
        assert!(!allday.overlaps(&over));
    }

    #[test]
    fn pack_day_empty_is_empty() {
        assert!(pack_day(&[]).is_empty());
    }

    #[test]
    fn pack_day_single_event_is_one_full_column() {
        let a = CalendarEvent::new(9, "A").with_span(540, 600);
        let laid = pack_day(&[&a]);
        assert_eq!(laid.len(), 1);
        assert_eq!(laid[0].id, 9);
        assert_eq!((laid[0].column, laid[0].columns), (0, 1));
        assert_eq!((laid[0].start_min, laid[0].end_min), (540, 600));
    }

    #[test]
    fn pack_day_tiles_an_overlapping_pair_then_a_separate_event() {
        // Given out of order to prove the deterministic sort.
        let c = CalendarEvent::new(3, "C").with_span(660, 720);
        let a = CalendarEvent::new(1, "A").with_span(540, 600);
        let b = CalendarEvent::new(2, "B").with_span(570, 630);
        let laid = pack_day(&[&c, &b, &a]);
        // Sorted by start: A(540) B(570) C(660).
        assert_eq!(laid[0].id, 1);
        assert_eq!((laid[0].column, laid[0].columns), (0, 2));
        assert_eq!(laid[1].id, 2);
        assert_eq!((laid[1].column, laid[1].columns), (1, 2));
        // C does not overlap B (B ends 10:30, C starts 11:00) → its own
        // single-column cluster.
        assert_eq!(laid[2].id, 3);
        assert_eq!((laid[2].column, laid[2].columns), (0, 1));
    }

    #[test]
    fn pack_day_reuses_a_freed_lane_within_a_cluster() {
        // A 09–10, B 09:30–11 (overlaps A), C 10–11 (A freed, reuses lane 0,
        // still overlaps B → cluster stays open, 2 columns throughout).
        let a = CalendarEvent::new(1, "A").with_span(540, 600);
        let b = CalendarEvent::new(2, "B").with_span(570, 660);
        let c = CalendarEvent::new(3, "C").with_span(600, 660);
        let laid = pack_day(&[&a, &b, &c]);
        assert_eq!((laid[0].id, laid[0].column, laid[0].columns), (1, 0, 2));
        assert_eq!((laid[1].id, laid[1].column, laid[1].columns), (2, 1, 2));
        // C reuses lane 0 (A ended at 600 ≤ C's start 600).
        assert_eq!((laid[2].id, laid[2].column, laid[2].columns), (3, 0, 2));
    }

    #[test]
    fn pack_day_triple_overlap_is_three_columns() {
        let a = CalendarEvent::new(1, "A").with_span(540, 660);
        let b = CalendarEvent::new(2, "B").with_span(550, 660);
        let c = CalendarEvent::new(3, "C").with_span(560, 660);
        let laid = pack_day(&[&a, &b, &c]);
        for l in &laid {
            assert_eq!(l.columns, 3);
        }
        assert_eq!(laid[0].column, 0);
        assert_eq!(laid[1].column, 1);
        assert_eq!(laid[2].column, 2);
    }

    #[test]
    fn pack_day_skips_all_day_events() {
        let a = CalendarEvent::new(1, "A").with_span(540, 600);
        let holiday = CalendarEvent::new(2, "Holiday").with_all_day(true);
        let laid = pack_day(&[&holiday, &a]);
        assert_eq!(laid.len(), 1);
        assert_eq!(laid[0].id, 1);
    }

    #[test]
    fn time_label_is_24h_clock_arithmetic() {
        assert_eq!(time_label(0), "00:00");
        assert_eq!(time_label(540), "09:00");
        assert_eq!(time_label(13 * 60 + 5), "13:05");
        assert_eq!(time_label(60_000), "24:00"); // clamped, no panic
    }

    #[test]
    fn time_label_12h_is_compact() {
        assert_eq!(time_label_12h(0), "12am");
        assert_eq!(time_label_12h(540), "9am");
        assert_eq!(time_label_12h(570), "9:30am");
        assert_eq!(time_label_12h(720), "12pm");
        assert_eq!(time_label_12h(780), "1pm");
        assert_eq!(time_label_12h(13 * 60 + 5), "1:05pm");
    }

    #[test]
    fn readable_fg_contrasts_the_fill_and_passes_reset_through() {
        let dark = Color::Rgb(20, 22, 26);
        let light = Color::Rgb(245, 247, 250);
        // Reset ⇒ Reset (the caller then leaves the base style untouched).
        assert_eq!(readable_fg(Color::Reset), Color::Reset);
        // Light fills (the kitchen-sink dark-theme category palette) ⇒ dark fg.
        assert_eq!(readable_fg(Color::Rgb(88, 166, 255)), dark); // accent blue
        assert_eq!(readable_fg(Color::Rgb(63, 185, 80)), dark); // ok green
        assert_eq!(readable_fg(Color::Rgb(210, 153, 34)), dark); // warn amber
        assert_eq!(readable_fg(Color::Rgb(188, 140, 255)), dark); // accent_alt
        // Genuinely dark fills ⇒ light fg.
        assert_eq!(readable_fg(Color::Rgb(13, 17, 23)), light); // base
        assert_eq!(readable_fg(Color::Rgb(154, 103, 0)), light); // light-theme amber
        // ANSI names map by conventional brightness; the cube/grey ramp work.
        assert_eq!(readable_fg(Color::White), dark);
        assert_eq!(readable_fg(Color::Black), light);
        assert_eq!(readable_fg(Color::Indexed(231)), dark); // cube white
        assert_eq!(readable_fg(Color::Indexed(16)), light); // cube black
        assert_eq!(readable_fg(Color::Indexed(255)), dark); // grey ramp top
        assert_eq!(readable_fg(Color::Indexed(232)), light); // grey ramp bottom
    }
}
