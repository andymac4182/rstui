//! [`EventCard`] — the single-event detail view a calendar app drops inside a
//! [`Modal`](crate::Modal) (or a popover) when you click an event.
//!
//! # A pure projection of one [`CalendarEvent`]
//!
//! Like every calendar-family widget, `EventCard` is a *pure projection* of a
//! caller-owned [`CalendarEvent`]: a colour-swatched
//! bold title, a date·time line, an optional location line, a divider rule,
//! then the event's wrapped description. It owns no application state, reads
//! nothing back, and mutates nothing at render time — it borrows one event and
//! lays its facts out, exactly as [`DescriptionList`](crate::DescriptionList)
//! projects caller-built rows.
//!
//! # The description wrap is *reused*, never re-implemented
//!
//! A real event description is a paragraph that must wrap inside the card.
//! Rather than grow a second wrap algorithm, the description is rendered
//! through a private [`Paragraph`] with soft
//! [`Wrap`] — so word wrapping and right-edge clipping are
//! *inherited*, the same reuse [`DescriptionList`](crate::DescriptionList) and
//! [`Toast`](crate::Toast) make.
//!
//! # Dependency-free: the card does no date math
//!
//! The card never computes a weekday or formats a date — the day string is a
//! *caller-supplied label* ([`day_label`](EventCard::day_label); a multi-day
//! event's whole "19–21 May" string is the caller's to format), exactly the
//! [no-date-math](crate::event) stance the model takes. The only clock work is
//! turning the event's caller-owned minute counts into `HH:MM` via
//! [`time_label`] — clock arithmetic on an integer,
//! not calendar math, so no dependency is pulled in.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! or short area clips progressively (only as many rows as fit), every missing
//! optional field (location, description) is simply omitted with no blank row
//! left behind, and a degenerate area is a safe no-op — never a panic. It does
//! not centre or clear; pair it with [`Modal`](crate::Modal) at the call site,
//! the same render-then-fill-`inner` contract [`Block`] uses.

use std::borrow::Cow;

use rstui_core::{Buffer, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::event::{CalendarEvent, time_label};
use crate::paragraph::{Paragraph, Wrap};

/// The soft-wrap mode the description uses (keep the author's indentation,
/// like [`DescriptionList`](crate::DescriptionList)'s value column).
const DESC_WRAP: Wrap = Wrap { trim: false };

/// The detail view of one [`CalendarEvent`]: the body a
/// calendar app puts inside a [`Modal`](crate::Modal) on "click an event".
///
/// It draws, top-down, skipping any row whose source field is empty:
///
/// * `● Title` — a swatch tinted the event's [`color`](crate::CalendarEvent::color)
///   then the title in [`title_style`](Self::title_style) (bold by default).
/// * `<day_label> · HH:MM–HH:MM`, or `<day_label> · All day` for an all-day
///   event (times via [`time_label`]).
/// * `📍 <location>` — only when [`location`](crate::CalendarEvent::location)
///   is non-empty.
/// * a divider rule.
/// * the [`description`](crate::CalendarEvent::description), soft-wrapped
///   through a private [`Paragraph`] — only when non-empty.
///
/// An optional framing [`Block`] draws a border/title; the content then lays
/// out in [`Block::inner`]. It does **not** centre or clear — pair it with
/// [`Modal`](crate::Modal) at the call site.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Color, Position, Rect, Widget};
/// use rstui_widgets::{CalendarEvent, EventCard};
///
/// let e = CalendarEvent::new(1, "Standup")
///     .with_day(12)
///     .with_span(9 * 60, 9 * 60 + 30)
///     .with_color(Color::Cyan)
///     .with_location("Room 4");
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 24, 4));
/// EventCard::new(&e).day_label("Mon 12 May").render(buf.area(), &mut buf);
///
/// // Row 0: the colour swatch then the bold title.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '●');
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Cyan);
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'S'); // "Standup"
/// // Row 1: the caller's date label · the clock-formatted span.
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'M'); // "Mon …"
/// ```
#[derive(Debug, Clone)]
pub struct EventCard<'a> {
    event: &'a CalendarEvent<'a>,
    day_label: Cow<'a, str>,
    block: Option<Block<'a>>,
    style: Style,
    title_style: Style,
    time_style: Style,
    location_style: Style,
    divider_style: Style,
}

impl<'a> EventCard<'a> {
    /// A card for `event` with an empty day label, no frame, and styles that
    /// are empty except for a bold title (the conventional detail-view accent).
    pub fn new(event: &'a CalendarEvent<'a>) -> Self {
        Self {
            event,
            day_label: Cow::Borrowed(""),
            block: None,
            style: Style::new(),
            // The title is the one always-on accent (a detail card's heading),
            // the justified exception to styles-default-empty — exactly the
            // reasoning Input's always-drawn caret uses.
            title_style: Style::new().add_modifier(rstui_core::Modifier::BOLD),
            time_style: Style::new(),
            location_style: Style::new(),
            divider_style: Style::new(),
        }
    }

    /// Sets the caller-formatted date string shown on the date·time line (the
    /// card does **no date math** — a multi-day event's whole `"19–21 May"`
    /// string is the caller's to format and pass here).
    #[must_use]
    pub fn day_label(mut self, label: impl Into<Cow<'a, str>>) -> Self {
        self.day_label = label.into();
        self
    }

    /// Frames the card in `block`; the content lays out in
    /// [`Block::inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a background
    /// covers the whole card (the [`List`](crate::List) idiom).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] for the title (default: bold), beneath the base.
    #[must_use]
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// Sets the [`Style`] for the date·time line, beneath the base.
    #[must_use]
    pub fn time_style(mut self, style: Style) -> Self {
        self.time_style = style;
        self
    }

    /// Sets the [`Style`] for the location line, beneath the base.
    #[must_use]
    pub fn location_style(mut self, style: Style) -> Self {
        self.location_style = style;
        self
    }

    /// Sets the [`Style`] for the divider rule, beneath the base.
    #[must_use]
    pub fn divider_style(mut self, style: Style) -> Self {
        self.divider_style = style;
        self
    }

    /// The content area: [`area`](Widget::render) minus the framing
    /// [`block`](Self::block), or `area` itself when there is no block — the
    /// same rule [`Modal::inner`](crate::Modal::inner) uses.
    fn content(&self, area: Rect) -> Rect {
        match &self.block {
            Some(block) => block.inner(area),
            None => area,
        }
    }

    /// The date·time line: `<day_label> · HH:MM–HH:MM`, or
    /// `<day_label> · All day` for an all-day event. An empty day label drops
    /// the separator so an all-day event with no label is just `"All day"`.
    fn time_line(&self) -> String {
        let when = if self.event.all_day() {
            "All day".to_string()
        } else {
            format!(
                "{}–{}",
                time_label(self.event.start_min()),
                time_label(self.event.end_min())
            )
        };
        if self.day_label.is_empty() {
            when
        } else {
            format!("{} · {}", self.day_label, when)
        }
    }
}

impl Widget for EventCard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // The frame (if any) reserves the content area; the base fill covers
        // the whole content so a background reads as one card.
        let inner = self.content(area);
        if let Some(block) = &self.block {
            block.clone().render(area, buf);
        }
        if inner.is_empty() {
            return;
        }
        buf.set_style(inner, self.style);

        let title_base = self.style.patch(self.title_style);
        let time_base = self.style.patch(self.time_style);
        let loc_base = self.style.patch(self.location_style);
        let div_base = self.style.patch(self.divider_style);

        let left = inner.left();
        let right = inner.right();
        let bottom = inner.bottom();
        let mut y = inner.top();

        // Row 1: "● Title" — the swatch tinted the event colour, then the
        // title. The swatch takes its own fg; the title its own base.
        if y < bottom {
            let swatch = self.style.fg(self.event.color());
            buf.set_cell(Position::new(left, y), '●', swatch);
            // One blank gutter cell (base-styled), then the title clipped at
            // the right edge.
            if left.saturating_add(1) < right {
                buf.set_cell(Position::new(left.saturating_add(1), y), ' ', self.style);
            }
            let title_x = left.saturating_add(2);
            if title_x < right {
                buf.set_str(Position::new(title_x, y), self.event.title(), title_base);
            }
            y = y.saturating_add(1);
        }

        // Row 2: the date·time line.
        if y < bottom {
            buf.set_str(Position::new(left, y), &self.time_line(), time_base);
            y = y.saturating_add(1);
        }

        // Row 3: "📍 <location>" — only when there is a location.
        if y < bottom && !self.event.location().is_empty() {
            let line = format!("📍 {}", self.event.location());
            buf.set_str(Position::new(left, y), &line, loc_base);
            y = y.saturating_add(1);
        }

        // The divider rule, only if a row is left for it (and content follows
        // it — but a divider above the description is the conventional close
        // to the header even when the description is empty, so draw it
        // whenever a row remains).
        if y < bottom {
            let rule: String = "─".repeat(inner.width as usize);
            buf.set_str(Position::new(left, y), &rule, div_base);
            y = y.saturating_add(1);
        }

        // The wrapped description fills the rest, only when non-empty. Reuse
        // Paragraph's soft wrap — never a second wrap algorithm.
        if y < bottom && !self.event.description().is_empty() {
            let rest = Rect::new(left, y, inner.width, bottom.saturating_sub(y));
            Paragraph::new(self.event.description())
                .wrap(DESC_WRAP)
                .style(time_base)
                .render(rest, buf);
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

    fn sample() -> CalendarEvent<'static> {
        CalendarEvent::new(1, "Standup")
            .with_day(12)
            .with_span(9 * 60, 9 * 60 + 30)
            .with_color(Color::Cyan)
            .with_location("Room 4")
            .with_description("Daily sync")
    }

    /// The first `n` *chars* of `row` (byte-slicing splits the multibyte
    /// `●`/`·`/`–`/`📍`/`─` glyphs the card draws).
    fn head(row: &str, n: usize) -> String {
        row.chars().take(n).collect()
    }

    #[test]
    fn rows_are_title_then_datetime_then_location_then_divider_then_desc() {
        let e = sample();
        // 24 wide so nothing clips; 6 tall so every row has room.
        let out = lines(EventCard::new(&e).day_label("Mon 12 May"), 24, 6);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(head(rows[0], 9), "● Standup");
        assert_eq!(head(rows[1], 24), "Mon 12 May · 09:00–09:30");
        assert_eq!(head(rows[2], 8), "📍 Room 4");
        assert_eq!(head(rows[3], 3), "───"); // the divider rule
        assert_eq!(head(rows[4], 10), "Daily sync"); // the wrapped description
    }

    #[test]
    fn the_swatch_takes_the_event_colour_and_the_title_is_bold() {
        let e = sample();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 2));
        EventCard::new(&e).render(buf.area(), &mut buf);
        let dot = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(dot.symbol, '●');
        assert_eq!(dot.fg, Color::Cyan); // tinted the event colour
        let t = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(t.symbol, 'S');
        assert!(t.modifier.contains(Modifier::BOLD)); // default title accent
    }

    #[test]
    fn an_all_day_event_shows_all_day_not_a_clock_span() {
        let e = CalendarEvent::new(2, "Holiday").with_all_day(true);
        let out = lines(EventCard::new(&e).day_label("Fri 1 May"), 24, 2);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(head(rows[1], 19), "Fri 1 May · All day");
    }

    #[test]
    fn an_empty_day_label_drops_the_separator() {
        let e = CalendarEvent::new(2, "Holiday").with_all_day(true);
        let out = lines(EventCard::new(&e), 12, 2);
        let rows: Vec<&str> = out.lines().collect();
        // No label ⇒ just "All day", no leading " · ".
        assert_eq!(head(rows[1], 7), "All day");
    }

    #[test]
    fn a_caller_supplied_multi_day_label_is_used_verbatim() {
        // The card does NO date math — the whole range string is the caller's.
        let e = CalendarEvent::new(3, "Conf")
            .with_day(19)
            .with_end_day(21)
            .with_all_day(true);
        let out = lines(EventCard::new(&e).day_label("19–21 May"), 24, 2);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(head(rows[1], 17), "19–21 May · All d");
    }

    #[test]
    fn a_missing_location_omits_the_row_with_no_blank_gap() {
        let e = CalendarEvent::new(4, "Solo")
            .with_span(600, 660)
            .with_description("No room booked");
        let out = lines(EventCard::new(&e).day_label("D"), 20, 5);
        let rows: Vec<&str> = out.lines().collect();
        // Row 2 is the divider (location skipped, NOT left blank), row 3 desc.
        assert_eq!(head(rows[0], 4), "● So");
        assert_eq!(head(rows[1], 3), "D ·");
        assert_eq!(head(rows[2], 3), "───");
        assert_eq!(head(rows[3], 3), "No ");
    }

    #[test]
    fn a_missing_description_omits_the_paragraph() {
        let e = CalendarEvent::new(5, "Bare")
            .with_span(600, 660)
            .with_location("Hall");
        let out = lines(EventCard::new(&e).day_label("D"), 20, 6);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(head(rows[3], 3), "───"); // divider drawn
        assert_eq!(rows[4], "                    "); // no description rows
    }

    #[test]
    fn the_description_soft_wraps_through_the_reused_paragraph() {
        let e = CalendarEvent::new(6, "T")
            .with_span(600, 660)
            .with_description("the quick brown");
        // Width 6: description wraps at word boundaries (Paragraph's wrap).
        let out = lines(EventCard::new(&e), 6, 7);
        let rows: Vec<&str> = out.lines().collect();
        // row0 title, row1 time, row2 divider (no location), then wrapped desc.
        assert_eq!(&rows[3][..3], "the");
        assert_eq!(&rows[4][..5], "quick");
        assert_eq!(&rows[5][..5], "brown");
    }

    #[test]
    fn a_short_area_clips_progressively_and_never_panics() {
        let e = sample();
        // Only one row: just the title, everything below clipped away.
        let out = lines(EventCard::new(&e).day_label("Mon"), 12, 1);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(head(rows[0], 9), "● Standup");
    }

    #[test]
    fn a_block_frames_the_card_in_the_inner_area() {
        let e = CalendarEvent::new(7, "Hi").with_span(540, 600);
        let out = lines(
            EventCard::new(&e)
                .day_label("D")
                .block(Block::bordered().title("Event")),
            14,
            5,
        );
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(head(rows[0], 7), "┌Event─");
        // Content starts inside the border at (1,1): the swatch then title.
        assert_eq!(rows[1].chars().nth(1).unwrap(), '●');
        assert_eq!(rows[1].chars().nth(3).unwrap(), 'H');
        assert_eq!(rows[4].chars().next().unwrap(), '└');
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_content() {
        let e = sample();
        // inner() collapses to empty; the block still renders, content does
        // not, and nothing panics.
        let out = lines(EventCard::new(&e).block(Block::bordered()), 2, 2);
        assert_eq!(out, "┌┐\n└┘\n");
    }

    #[test]
    fn the_base_style_fills_the_whole_content_area() {
        let e = CalendarEvent::new(8, "X").with_span(600, 660);
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        EventCard::new(&e)
            .style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for y in 0..3 {
            for x in 0..6 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Blue);
            }
        }
    }

    #[test]
    fn styles_cascade_base_then_each_part_style() {
        let e = sample();
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 2));
        EventCard::new(&e)
            .day_label("D")
            .style(Style::new().bg(Color::Blue))
            .time_style(Style::new().fg(Color::Green))
            .render(buf.area(), &mut buf);
        // The date·time row: base bg cascades, time_style fg on top.
        let c = buf.get(Position::new(0, 1)).unwrap();
        assert_eq!(c.symbol, 'D');
        assert_eq!(c.bg, Color::Blue); // base fill
        assert_eq!(c.fg, Color::Green); // time_style
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let e = sample();
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        EventCard::new(&e).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn a_zero_minute_span_is_a_valid_point_in_time() {
        // end clamps to start in the model; the card just formats both.
        let e = CalendarEvent::new(9, "Ping").with_span(600, 600);
        let out = lines(EventCard::new(&e).day_label("D"), 24, 2);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(head(rows[1], 15), "D · 10:00–10:00");
    }
}
