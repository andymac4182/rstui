//! [`DatePicker`] — a closed date field that drops an **opaque**,
//! field-anchored month panel (the existing [`Calendar`]) when the
//! caller-owned `open` flag is set: the date-entry control a form uses.
//!
//! # A pure projection that reuses [`Calendar`] and the [`Select`](crate::Select) idiom
//!
//! `DatePicker` owns no state. Like [`Calendar`] it does **no date math** —
//! the `year`, `month`, `day_count`, and the weekday of day 1 are caller-owned
//! inputs (computed by the reducer or a date crate of the caller's choosing,
//! never `chrono`/`time`); so are [`open`](DatePicker::open),
//! [`selected`](DatePicker::selected) (the chosen day number), and
//! [`today`](DatePicker::today). Dropping the panel and committing a day are
//! the reducer's job in `update`; the widget only ever reads — the
//! read-only-state rule [`Calendar`]/[`List`](crate::List) establish.
//!
//! It is **self-contained**: it does not depend on any popover widget. It
//! borrows the [`Select`](crate::Select) dropdown's two techniques exactly —
//! the panel is **anchored** to the field (directly below it, flipping above
//! when the screen runs out), and it is **opaque**
//! ([`clear_region`](rstui_core::Buffer::clear_region)d before drawing so the
//! form behind it cannot bleed through, the [`Modal`](crate::Modal) opacity
//! affordance) — then **reuses [`Calendar`] wholesale** for the panel body
//! (its grid layout, highlighting, and totality inherited rather than
//! re-implemented) and [`Block`] for the panel's optional frame. It is
//! deliberately *not* a [`Modal`](crate::Modal): anchored, not centred, and
//! sized to a month grid.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*): a
//! closed field with no/out-of-range selection (the placeholder), an empty
//! area, a one-cell field, a panel that fits neither below nor above (clamped
//! to the larger gap), and [`Calendar`]'s own out-of-range clamping are all
//! safe clips/no-ops — never a panic. [`panel`](DatePicker::panel) is an empty
//! rect whenever the panel is not drawn.

use std::borrow::Cow;

use rstui_core::{Buffer, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::calendar::Calendar;

/// The right-aligned glyph marking the closed field as a date dropdown.
const DISCLOSURE: char = '▾';

/// The calendar grid's natural width: a 7-day week of 3-column cells.
const GRID_W: u16 = 21;

/// The calendar's natural height: header + weekday row + up to six week rows.
const GRID_H: u16 = 8;

/// A closed date field that drops an opaque [`Calendar`] panel when
/// [`open`](Self::open).
///
/// Closed it is one row — the [`selected`](Self::selected) day formatted as
/// `YYYY-MM-DD` (or the [`placeholder`](Self::placeholder) when there is none)
/// plus a right-aligned disclosure marker, the same closed-field shape
/// [`Select`](crate::Select) uses. [`open`](Self::open) additionally drops the
/// opaque month panel anchored to the field.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::DatePicker;
///
/// // May 2026: 31 days, the 1st is a Friday (weekday index 5). All caller-
/// // owned — the widget does no date math (no `chrono`).
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
/// DatePicker::new(2026, 5, 31, 5)
///     .selected(Some(17))
///     .render(buf.area(), &mut buf);
///
/// // Closed: the selected day formatted, with a right-aligned disclosure.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '2'); // 2026-…
/// assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, '-');
/// assert_eq!(buf.get(Position::new(11, 0)).unwrap().symbol, '▾');
/// ```
#[derive(Debug, Clone)]
pub struct DatePicker<'a> {
    year: i32,
    month: u32,
    day_count: u32,
    weekday_of_first: u32,
    first_weekday: u32,
    open: bool,
    focused: bool,
    selected: Option<u32>,
    today: Option<u32>,
    placeholder: Cow<'a, str>,
    block: Option<Block<'a>>,
    style: Style,
    focus_style: Style,
    header_style: Style,
    weekday_style: Style,
    selected_style: Style,
    today_style: Style,
}

impl<'a> DatePicker<'a> {
    /// A closed date picker for `month` of `year` with `day_count` days, where
    /// day 1 falls on weekday `weekday_of_first` (`0 = Sunday … 6 = Saturday`,
    /// the [`Calendar`] convention). Nothing selected, no placeholder.
    pub fn new(year: i32, month: u32, day_count: u32, weekday_of_first: u32) -> Self {
        Self {
            year,
            month,
            day_count,
            weekday_of_first,
            first_weekday: 0,
            open: false,
            focused: false,
            selected: None,
            today: None,
            placeholder: Cow::Borrowed(""),
            block: None,
            style: Style::new(),
            focus_style: Style::new(),
            header_style: Style::new(),
            weekday_style: Style::new(),
            selected_style: Style::new(),
            today_style: Style::new(),
        }
    }

    /// Sets the weekday the panel's weeks start on (`0 = Sunday … 6 =
    /// Saturday`), forwarded to the embedded [`Calendar`].
    #[must_use]
    pub fn first_weekday(mut self, first_weekday: u32) -> Self {
        self.first_weekday = first_weekday;
        self
    }

    /// Sets whether the calendar panel is dropped — caller-owned state the
    /// widget only reads (toggle it in `update`, typically on `Enter`/`Esc`).
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets whether the closed field is focused — caller-owned state the
    /// widget only reads (move it in `update`, typically on `Tab`).
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Sets the committed day number shown in the closed field and highlighted
    /// in the panel, or `None`. A day outside `1..=day_count` falls back to the
    /// placeholder (and is not highlighted, inheriting [`Calendar`]'s clamp).
    #[must_use]
    pub fn selected(mut self, selected: Option<u32>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the "today" day the panel accents, or `None`.
    #[must_use]
    pub fn today(mut self, today: Option<u32>) -> Self {
        self.today = today;
        self
    }

    /// Sets the hint shown in the closed field when nothing is selected.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<Cow<'a, str>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Frames the open calendar panel in `block` (forwarded to the embedded
    /// [`Calendar`]); does not frame the closed field (a leaf).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the closed field's row so a
    /// background reads as one bar.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] applied to the closed field when
    /// [`focused`](Self::focused), patched **last** across the row so the
    /// focus emphasis reads as one bar (the [`Select`](crate::Select) idiom).
    #[must_use]
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = style;
        self
    }

    /// Sets the panel's month/year header [`Style`] (forwarded to
    /// [`Calendar`]).
    #[must_use]
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Sets the panel's weekday-label [`Style`] (forwarded to [`Calendar`]).
    #[must_use]
    pub fn weekday_style(mut self, style: Style) -> Self {
        self.weekday_style = style;
        self
    }

    /// Sets the [`Style`] the panel patches over the selected day (forwarded
    /// to [`Calendar`]).
    #[must_use]
    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Sets the [`Style`] the panel patches over the "today" day (forwarded to
    /// [`Calendar`]).
    #[must_use]
    pub fn today_style(mut self, style: Style) -> Self {
        self.today_style = style;
        self
    }

    /// A [`Block`]'s constant vertical frame overhead (borders + padding).
    fn frame_v(&self) -> u16 {
        self.block.as_ref().map_or(0, |b| {
            let probe = Rect::new(0, 0, 1, u16::MAX);
            probe.height.saturating_sub(b.inner(probe).height)
        })
    }

    /// A [`Block`]'s constant horizontal frame overhead (borders + padding).
    fn frame_h(&self) -> u16 {
        self.block.as_ref().map_or(0, |b| {
            let probe = Rect::new(0, 0, u16::MAX, 1);
            probe.width.saturating_sub(b.inner(probe).width)
        })
    }

    /// The panel's natural size: a month grid plus any [`block`](Self::block)
    /// frame, at least as wide as the field.
    fn desired(&self, field_width: u16) -> (u16, u16) {
        let w = field_width.max(GRID_W.saturating_add(self.frame_h()));
        let h = GRID_H.saturating_add(self.frame_v());
        (w, h)
    }

    /// The rect the open calendar panel occupies for a closed-field `area`, or
    /// an empty rect when the panel is not drawn (closed, or empty area).
    ///
    /// A pure function of `area` and the configuration giving the panel's
    /// natural placement: anchored directly **below** the field.
    /// [`render`](Widget::render) mirrors this but, knowing the live buffer,
    /// flips the panel **above** the field (or clamps it) when the space below
    /// is short — the same derived-geometry-is-a-projection reasoning
    /// [`Select::panel`](crate::Select::panel) uses. Exposed so an app can map
    /// a click in the panel back to a day.
    #[must_use]
    pub fn panel(&self, area: Rect) -> Rect {
        if !self.open || area.is_empty() {
            return Rect::ZERO;
        }
        let (w, h) = self.desired(area.width);
        Rect::new(area.x, area.bottom(), w, h)
    }

    /// The embedded, configured [`Calendar`].
    fn calendar(self) -> Calendar<'a> {
        let mut cal = Calendar::new(self.year, self.month, self.day_count, self.weekday_of_first)
            .first_weekday(self.first_weekday)
            .selected(self.selected)
            .today(self.today)
            .header_style(self.header_style)
            .weekday_style(self.weekday_style)
            .selected_style(self.selected_style)
            .today_style(self.today_style);
        if let Some(block) = self.block {
            cal = cal.block(block);
        }
        cal
    }
}

impl Widget for DatePicker<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        let y = area.top();
        let left = area.left();
        let right = area.right();

        // --- The closed field row (always drawn, even when open) ---

        let base = if self.focused {
            self.style.patch(self.focus_style)
        } else {
            self.style
        };
        buf.set_style(Rect::new(left, y, area.width, 1), base);

        // The disclosure marker owns the last column; the text is clipped
        // before it. A one-cell field is just the marker.
        let text_right = right.saturating_sub(1);
        buf.set_cell(Position::new(text_right, y), DISCLOSURE, base);

        // A day in `1..=day_count` formats as the field text; anything else
        // (incl. `None`) falls back to the placeholder.
        let valid = self.selected.filter(|&d| d >= 1 && d <= self.day_count);
        let text = if let Some(d) = valid {
            let (yr, mo) = (self.year, self.month);
            Cow::Owned(format!("{yr}-{mo:02}-{d:02}"))
        } else {
            self.placeholder.clone()
        };
        let mut x = left;
        for ch in text.chars() {
            if x >= text_right {
                break;
            }
            buf.set_cell(Position::new(x, y), ch, base);
            x = x.saturating_add(1);
        }

        // --- The open calendar panel ---

        if !self.open {
            return;
        }
        let (w, h) = self.desired(area.width);
        let screen = buf.area();
        let gap_below = screen.bottom().saturating_sub(area.bottom());
        let gap_above = area.top().saturating_sub(screen.top());
        let panel = if h <= gap_below {
            Rect::new(area.x, area.bottom(), w, h)
        } else if h <= gap_above {
            Rect::new(area.x, area.top().saturating_sub(h), w, h)
        } else if gap_below >= gap_above {
            Rect::new(area.x, area.bottom(), w, gap_below)
        } else {
            Rect::new(area.x, area.top().saturating_sub(gap_above), w, gap_above)
        };
        // Clamp the (grid-wide) panel so it never runs off the buffer.
        let max_w = screen.right().saturating_sub(panel.x);
        let panel = Rect::new(panel.x, panel.y, panel.width.min(max_w), panel.height);
        if panel.is_empty() {
            return;
        }

        // Opaque: take exclusive ownership of the panel cells (the `Modal`
        // affordance — see the module docs), then reuse `Calendar` wholesale.
        buf.clear_region(panel);
        self.calendar().render(panel, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Style};

    /// Fills `buf` with a styled `.` background so a clear is observable.
    fn background(buf: &mut Buffer) {
        let style = Style::new().fg(Color::Red).bg(Color::Blue);
        for p in buf.area().positions() {
            buf.set_cell(p, '.', style);
        }
    }

    #[test]
    fn closed_shows_the_selected_day_formatted_and_a_disclosure_marker() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
        DatePicker::new(2026, 5, 31, 5)
            .selected(Some(17))
            .render(buf.area(), &mut buf);
        // "2026-05-17" then padding then the right-aligned '▾'.
        for (i, ch) in "2026-05-17".chars().enumerate() {
            assert_eq!(buf.get(Position::new(i as u16, 0)).unwrap().symbol, ch);
        }
        assert_eq!(buf.get(Position::new(11, 0)).unwrap().symbol, '▾');
    }

    #[test]
    fn closed_with_no_selection_shows_the_placeholder() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
        DatePicker::new(2026, 5, 31, 5)
            .placeholder("pick a day")
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'p');
        assert_eq!(buf.get(Position::new(11, 0)).unwrap().symbol, '▾');
    }

    #[test]
    fn an_out_of_range_selected_day_falls_back_to_the_placeholder() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
        DatePicker::new(2026, 5, 31, 5)
            .selected(Some(99))
            .placeholder("none")
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'n');
    }

    #[test]
    fn the_disclosure_marker_is_always_the_last_column() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        DatePicker::new(2026, 5, 31, 5)
            .selected(Some(1))
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, '▾');
    }

    #[test]
    fn a_focused_closed_field_is_a_full_width_focus_bar() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
        DatePicker::new(2026, 5, 31, 5)
            .selected(Some(1))
            .focused(true)
            .focus_style(Style::new().bg(Color::Cyan))
            .render(buf.area(), &mut buf);
        for x in 0..12 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Cyan);
        }
    }

    #[test]
    fn open_drops_the_calendar_panel_below_the_field() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 10));
        DatePicker::new(2026, 5, 31, 5)
            .open(true)
            .render(Rect::new(0, 0, 12, 1), &mut buf);
        // The Calendar header "May 2026" is centred over the 21-wide grid on
        // the panel's first row (y = field.bottom() = 1): 'M' at col 6.
        assert_eq!(buf.get(Position::new(6, 1)).unwrap().symbol, 'M');
        // The field row still carries its disclosure marker.
        assert_eq!(buf.get(Position::new(11, 0)).unwrap().symbol, '▾');
    }

    #[test]
    fn the_open_panel_is_opaque_and_the_background_does_not_bleed_through() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 10));
        background(&mut buf);
        DatePicker::new(2026, 5, 31, 5)
            .open(true)
            .render(Rect::new(0, 0, 12, 1), &mut buf);
        // A blank panel cell is cleared (no '.' / red-blue bleed). The weekday
        // gutter column past the labels is a good blank probe.
        let cell = buf.get(Position::new(20, 2)).unwrap();
        assert_eq!(cell.bg, Color::Reset);
        assert_ne!(cell.symbol, '.');
    }

    #[test]
    fn the_panel_flips_above_when_there_is_no_room_below() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 10));
        // Field on the last row: no room below, 9 rows above (>= 8 needed).
        DatePicker::new(2026, 5, 31, 5)
            .open(true)
            .render(Rect::new(0, 9, 12, 1), &mut buf);
        // Panel flipped above: its top is at row 1, header 'M' there.
        assert_eq!(buf.get(Position::new(6, 1)).unwrap().symbol, 'M');
        assert_eq!(buf.get(Position::new(11, 9)).unwrap().symbol, '▾');
    }

    #[test]
    fn the_selected_day_is_highlighted_in_the_open_panel() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 10));
        DatePicker::new(2026, 5, 31, 5)
            .open(true)
            .selected(Some(17))
            .selected_style(Style::new().bg(Color::Cyan))
            .render(Rect::new(0, 0, 12, 1), &mut buf);
        assert!(
            buf.cells().iter().any(|c| c.bg == Color::Cyan),
            "the Calendar painted the selected day's accent"
        );
    }

    #[test]
    fn today_is_accented_in_the_open_panel() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 21, 10));
        DatePicker::new(2026, 5, 31, 5)
            .open(true)
            .today(Some(9))
            .today_style(Style::new().bg(Color::Yellow))
            .render(Rect::new(0, 0, 12, 1), &mut buf);
        assert!(buf.cells().iter().any(|c| c.bg == Color::Yellow));
    }

    #[test]
    fn a_block_frames_the_open_panel() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 25, 12));
        DatePicker::new(2026, 5, 31, 5)
            .open(true)
            .block(Block::bordered())
            .render(Rect::new(0, 0, 12, 1), &mut buf);
        // Panel below the field at y=1, bordered: top-left corner there.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '┌');
    }

    #[test]
    fn the_panel_accessor_is_empty_when_closed_and_natural_when_open() {
        let closed = DatePicker::new(2026, 5, 31, 5);
        assert!(closed.panel(Rect::new(0, 0, 12, 1)).is_empty());

        let open = DatePicker::new(2026, 5, 31, 5).open(true);
        // Natural placement: anchored below, grid-wide (max of field & 21).
        assert_eq!(open.panel(Rect::new(0, 0, 12, 1)), Rect::new(0, 1, 21, 8));
    }

    #[test]
    fn the_field_text_is_clipped_before_the_disclosure_marker() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        DatePicker::new(2026, 5, 31, 5)
            .selected(Some(17))
            .render(buf.area(), &mut buf);
        // Width 6: text region is cols 0..5, the marker is col 5.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '2');
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, '-'); // 2026-…
        assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, '▾');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
        DatePicker::new(2026, 5, 31, 5)
            .open(true)
            .selected(Some(1))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
