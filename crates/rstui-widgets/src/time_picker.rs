//! [`TimePicker`] — a closed `HH:MM` field that drops an **opaque**,
//! field-anchored list of times at a fixed step when the caller-owned `open`
//! flag is set: the time sibling of [`DatePicker`](crate::DatePicker).
//!
//! # A pure projection that reuses [`List`] and the [`Select`](crate::Select) idiom
//!
//! `TimePicker` owns no state. Like [`DatePicker`](crate::DatePicker) it does
//! **no date math** — the selected `minute` of the day is a caller-owned `u16`
//! (computed by the reducer or a clock crate of the caller's choosing, never
//! `chrono`/`time`); so are [`open`](TimePicker::open),
//! [`highlight`](TimePicker::highlight) (the keyboard row while the list is
//! open) and [`offset`](TimePicker::offset) (the list scroll). Dropping the
//! list and committing a time are the reducer's job in `update`; the widget
//! only ever reads — the read-only-state rule
//! [`List`]/[`Select`](crate::Select) establish. Formatting a
//! minute count as `HH:MM` ([`time_label`]) is clock
//! arithmetic on a caller integer, not calendar math, so it pulls in no
//! dependency (the same line [`CalendarEvent`](crate::CalendarEvent) sits on).
//!
//! It is **self-contained**: it does not depend on any popover widget. It
//! borrows the [`Select`](crate::Select) dropdown's two techniques exactly —
//! the panel is **anchored** to the field (directly below it, flipping above
//! when the screen runs out), and it is **opaque**
//! ([`clear_region`](rstui_core::Buffer::clear_region)d before drawing so the
//! form behind it cannot bleed through, the [`Modal`](crate::Modal) opacity
//! affordance) — then **reuses [`List`] wholesale** for the panel
//! body (its scrolling, highlight bar, and totality inherited rather than
//! re-implemented) and [`Block`] for the panel's optional frame. It is
//! deliberately *not* a [`Modal`](crate::Modal): anchored, not centred, and
//! sized to its time rows, exactly like [`DatePicker`](crate::DatePicker) and
//! [`Select`](crate::Select).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*): a
//! closed field whose `minute` is outside the
//! [`range`](TimePicker::range) (the placeholder), an empty area, a one-cell
//! field, an inverted range (an empty list), a panel that fits neither below
//! nor above (clamped to the larger gap), and [`List`]'s own
//! out-of-range clamping are all safe clips/no-ops — never a panic.
//! [`panel`](TimePicker::panel) is an empty rect whenever the panel is not
//! drawn, and [`minute_at`](TimePicker::minute_at) is `None` off the rows.

use std::borrow::Cow;

use rstui_core::{Buffer, Position, Rect, Style, Widget};

use crate::block::Block;
use crate::event::{MINUTES_PER_DAY, time_label};
use crate::list::List;

/// The right-aligned glyph marking the closed field as a time dropdown.
const DISCLOSURE: char = '▾';

/// A sensible default cap on the open list's visible rows: tall enough for a
/// real time menu, short enough to stay anchored to the field on a normal
/// screen (the [`Select`](crate::Select) default), beyond which the
/// caller-owned [`offset`](TimePicker::offset) scrolls.
const VISIBLE_ROWS: u16 = 8;

/// A closed time field that drops an opaque [`List`] of times at
/// a step when [`open`](Self::open).
///
/// Closed it is one row — the `minute` formatted as `HH:MM`
/// (or the [`placeholder`](Self::placeholder) when it is outside the
/// [`range`](Self::range)) plus a right-aligned disclosure marker, the same
/// closed-field shape [`Select`](crate::Select)/[`DatePicker`](crate::DatePicker)
/// use. [`open`](Self::open) additionally drops the opaque time list anchored
/// to the field.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::TimePicker;
///
/// // 09:30 as a minute-of-day (caller-owned `u16`) — the widget does no
/// // date math (no `chrono`).
/// let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
/// TimePicker::new(9 * 60 + 30).render(buf.area(), &mut buf);
///
/// // Closed: the time formatted, with a right-aligned disclosure marker.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '0'); // 09:30
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, ':');
/// assert_eq!(buf.get(Position::new(7, 0)).unwrap().symbol, '▾');
/// ```
#[derive(Debug, Clone)]
pub struct TimePicker<'a> {
    minute: u16,
    open: bool,
    focused: bool,
    step_min: u16,
    range_start: u16,
    range_end: u16,
    highlight: usize,
    offset: usize,
    placeholder: Cow<'a, str>,
    block: Option<Block<'a>>,
    style: Style,
    focus_style: Style,
    selected_style: Style,
}

impl<'a> TimePicker<'a> {
    /// A closed time picker whose selected minute-of-day is the caller-owned
    /// `minute`. Closed (not dropped), a `30`-minute step, the full
    /// `0..`[`MINUTES_PER_DAY`] range, no
    /// placeholder.
    pub fn new(minute: u16) -> Self {
        Self {
            minute,
            open: false,
            focused: false,
            step_min: 30,
            range_start: 0,
            range_end: MINUTES_PER_DAY,
            highlight: 0,
            offset: 0,
            placeholder: Cow::Borrowed(""),
            block: None,
            style: Style::new(),
            focus_style: Style::new(),
            selected_style: Style::new(),
        }
    }

    /// Sets whether the time list is dropped — caller-owned state the widget
    /// only reads (toggle it in `update`, typically on `Enter`/`Esc`).
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

    /// Sets the minute step the dropped list increments by (default `30`).
    /// Clamped to at least `1` so the list is always finite.
    #[must_use]
    pub fn step_min(mut self, step_min: u16) -> Self {
        self.step_min = step_min.max(1);
        self
    }

    /// Sets the inclusive minute-of-day window the list offers (default
    /// `0..=`[`MINUTES_PER_DAY`]). The dropped
    /// list is `start..=end` taken every [`step_min`](Self::step_min); a
    /// `minute` outside it shows the
    /// [`placeholder`](Self::placeholder) (unset). An `end` before `start`
    /// yields an empty list (total, no panic).
    #[must_use]
    pub fn range(mut self, start_min: u16, end_min: u16) -> Self {
        self.range_start = start_min.min(MINUTES_PER_DAY);
        self.range_end = end_min.min(MINUTES_PER_DAY);
        self
    }

    /// Sets the hint shown in the closed field when `minute`
    /// is outside the [`range`](Self::range) (i.e. unset).
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<Cow<'a, str>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets which row the open list highlights (the keyboard row while open),
    /// forwarded straight to the internal [`List`]. Committed
    /// into `minute` by the reducer on `Enter` — never here.
    /// Out of range simply paints no bar (inherited from
    /// [`List`]).
    #[must_use]
    pub fn highlight(mut self, highlight: usize) -> Self {
        self.highlight = highlight;
        self
    }

    /// Sets the open list's scroll offset (the index of its first visible
    /// row), exactly [`List::offset`](crate::List::offset).
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Frames the open time list in `block` (forwarded to the embedded
    /// [`List`]); does not frame the closed field (a leaf).
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

    /// Sets the [`Style`] the open list patches over the highlighted row,
    /// forwarded straight to the internal [`List`].
    #[must_use]
    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Whether `minute` falls inside the configured inclusive range.
    fn in_range(&self) -> bool {
        self.range_start <= self.range_end
            && self.minute >= self.range_start
            && self.minute <= self.range_end
    }

    /// The list of step times `start..=end`, as `HH:MM` labels. Empty when
    /// the range is inverted (total).
    fn times(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.range_start > self.range_end {
            return out;
        }
        let step = self.step_min.max(1) as u32;
        let mut m = self.range_start as u32;
        let end = self.range_end as u32;
        while m <= end {
            out.push(time_label(m as u16));
            m += step;
        }
        out
    }

    /// How many rows the panel shows: the times, capped at
    /// [`VISIBLE_ROWS`].
    fn visible_rows(&self) -> u16 {
        (self.times().len() as u16).min(VISIBLE_ROWS)
    }

    /// A [`Block`]'s constant vertical frame overhead (borders + padding).
    ///
    /// [`Block::inner`] is pure arithmetic, so a tall probe measures the
    /// overhead exactly, letting the panel be sized so the inner [`List`]
    /// shows `visible_rows` content rows.
    fn block_vertical_frame(block: Option<&Block<'_>>) -> u16 {
        block.map_or(0, |b| {
            let probe = Rect::new(0, 0, 1, u16::MAX);
            probe.height.saturating_sub(b.inner(probe).height)
        })
    }

    /// The rect the open time list occupies for a closed-field `area`, or an
    /// empty rect when the panel is not drawn (closed, empty area, or an
    /// empty list).
    ///
    /// A pure function of `area` and the configuration giving the panel's
    /// natural placement: anchored directly **below** the field, `area.width`
    /// wide, and tall enough for `min(times, 8)` rows plus any
    /// [`block`](Self::block) frame. [`render`](Widget::render) mirrors this
    /// but, knowing the live buffer, flips the panel **above** the field (or
    /// clamps it) when the space below is short — the same
    /// derived-geometry-is-a-projection reasoning
    /// [`Select::panel`](crate::Select::panel) /
    /// [`DatePicker::panel`](crate::DatePicker::panel) use. Exposed so an app
    /// can map a click in the panel back to a time (see
    /// [`minute_at`](Self::minute_at)).
    #[must_use]
    pub fn panel(&self, area: Rect) -> Rect {
        if !self.open || area.is_empty() {
            return Rect::ZERO;
        }
        let visible = self.visible_rows();
        if visible == 0 {
            return Rect::ZERO;
        }
        let height = visible.saturating_add(Self::block_vertical_frame(self.block.as_ref()));
        Rect::new(area.x, area.bottom(), area.width, height)
    }

    /// The minute-of-day of the time row at cell `pos` for a closed-field
    /// `area`, or `None` when the panel is not drawn / the pointer is off a
    /// row.
    ///
    /// The pure inverse of the open list's layout (its natural below-the-field
    /// placement, accounting for the framing [`block`](Self::block) and the
    /// caller-owned scroll [`offset`](Self::offset)) — clicking a time you see
    /// picks that minute, so an app maps a click to a reducer action instead
    /// of every caller re-deriving the step arithmetic. Mirrors
    /// [`List::row_at`](crate::List::row_at) over the same panel rect.
    #[must_use]
    pub fn minute_at(&self, area: Rect, pos: Position) -> Option<u16> {
        let panel = self.panel(area);
        if panel.is_empty() {
            return None;
        }
        let row = self.list(self.times()).row_at(panel, pos)?;
        let step = self.step_min.max(1) as u32;
        let m = self.range_start as u32 + row as u32 * step;
        (m <= self.range_end as u32).then_some(m as u16)
    }

    /// The configured internal [`List`] over the owned `times` (the panel
    /// body). The labels are owned by the list (so it borrows nothing of
    /// `self`); only the optional [`block`](Self::block) is cloned.
    fn list(&self, times: Vec<String>) -> List<'a> {
        let mut list = List::new(times)
            .selected(Some(self.highlight))
            .offset(self.offset)
            .highlight_style(self.selected_style);
        if let Some(block) = self.block.clone() {
            list = list.block(block);
        }
        list
    }
}

impl Widget for TimePicker<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        let y = area.top();
        let left = area.left();
        let right = area.right();

        // --- The closed field row (always drawn, even when open) ---

        // The base, with the focus emphasis patched in when focused. Filling
        // the whole row makes a focused field read as one contiguous bar —
        // the Select/DatePicker focus-bar idiom.
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

        // A minute inside the range formats as the field text; anything else
        // falls back to the placeholder.
        let text: Cow<'_, str> = if self.in_range() {
            Cow::Owned(time_label(self.minute))
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

        // --- The open time list ---

        if !self.open {
            return;
        }
        let times = self.times();
        let visible = (times.len() as u16).min(VISIBLE_ROWS);
        if visible == 0 {
            // An open picker with no times is a no-op panel — total.
            return;
        }
        let frame_v = Self::block_vertical_frame(self.block.as_ref());
        let desired = visible.saturating_add(frame_v);

        // Anchor to the field: prefer directly below; flip above when the
        // space below is short; clamp to the larger gap when it fits neither.
        // Both gaps zero ⇒ an empty panel (no-op). `buf.area()` is the screen.
        let screen = buf.area();
        let gap_below = screen.bottom().saturating_sub(area.bottom());
        let gap_above = area.top().saturating_sub(screen.top());
        let panel = if desired <= gap_below {
            Rect::new(area.x, area.bottom(), area.width, desired)
        } else if desired <= gap_above {
            Rect::new(
                area.x,
                area.top().saturating_sub(desired),
                area.width,
                desired,
            )
        } else if gap_below >= gap_above {
            Rect::new(area.x, area.bottom(), area.width, gap_below)
        } else {
            Rect::new(
                area.x,
                area.top().saturating_sub(gap_above),
                area.width,
                gap_above,
            )
        };
        if panel.is_empty() {
            return;
        }

        // Opaque: take exclusive ownership of the panel cells so background
        // content cannot bleed through (the Modal opacity technique — see the
        // module docs for why this is NOT a Modal), then reuse `List`
        // wholesale so its scrolling, highlight bar, and totality are
        // inherited rather than re-implemented.
        buf.clear_region(panel);
        self.list(times).render(panel, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Style};

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

    /// Fills `buf` with a styled `.` background so a clear is observable.
    fn background(buf: &mut Buffer) {
        let style = Style::new().fg(Color::Red).bg(Color::Blue);
        for p in buf.area().positions() {
            buf.set_cell(p, '.', style);
        }
    }

    #[test]
    fn closed_shows_the_selected_time_formatted_and_a_disclosure_marker() {
        // 09:30 then padding then the right-aligned '▾'.
        assert_eq!(lines(TimePicker::new(9 * 60 + 30), 8, 1), "09:30  ▾\n");
    }

    #[test]
    fn closed_with_an_out_of_range_minute_shows_the_placeholder() {
        // 06:00 is outside an 08:00..17:00 working-hours range → placeholder.
        let tp = TimePicker::new(6 * 60)
            .range(8 * 60, 17 * 60)
            .placeholder("pick");
        assert_eq!(lines(tp, 8, 1), "pick   ▾\n");
    }

    #[test]
    fn an_in_range_minute_formats_as_the_field_text() {
        let tp = TimePicker::new(13 * 60 + 5).range(8 * 60, 17 * 60);
        assert_eq!(lines(tp, 8, 1), "13:05  ▾\n");
    }

    #[test]
    fn the_disclosure_marker_is_always_the_last_column() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        TimePicker::new(0).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, '▾');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '0'); // 00:00
    }

    #[test]
    fn a_focused_closed_field_is_a_full_width_focus_bar() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        TimePicker::new(9 * 60)
            .focused(true)
            .focus_style(Style::new().bg(Color::Cyan))
            .render(buf.area(), &mut buf);
        for x in 0..10 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Cyan);
        }
    }

    #[test]
    fn open_drops_the_time_list_below_the_field() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 5));
        // 00:00 step 30, first three list rows are 00:00, 00:30, 01:00.
        TimePicker::new(0)
            .open(true)
            .render(Rect::new(0, 0, 8, 1), &mut buf);
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '0'); // 00:00
        assert_eq!(buf.get(Position::new(3, 2)).unwrap().symbol, '3'); // 00:30
        assert_eq!(buf.get(Position::new(1, 3)).unwrap().symbol, '1'); // 01:00
        // The field row still carries its disclosure marker.
        assert_eq!(buf.get(Position::new(7, 0)).unwrap().symbol, '▾');
    }

    #[test]
    fn step_min_controls_the_listed_times_and_clamps_to_one() {
        // Step 60 over 08:00..10:00 → exactly 08:00, 09:00, 10:00.
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        TimePicker::new(8 * 60)
            .range(8 * 60, 10 * 60)
            .step_min(60)
            .open(true)
            .render(Rect::new(0, 0, 8, 1), &mut buf);
        assert_eq!(lines_at(&buf, 0, 1), "08:00   ");
        assert_eq!(lines_at(&buf, 0, 2), "09:00   ");
        assert_eq!(lines_at(&buf, 0, 3), "10:00   ");
        // A zero step is clamped to 1 (no infinite list / panic).
        let tp = TimePicker::new(0).step_min(0);
        assert_eq!(tp.step_min, 1);
    }

    /// The row `y`'s glyphs as a `String` (a single-row helper).
    fn lines_at(buf: &Buffer, x0: u16, y: u16) -> String {
        let w = buf.area().width;
        (x0..w)
            .map(|x| buf.get(Position::new(x, y)).unwrap().symbol)
            .collect()
    }

    #[test]
    fn the_panel_flips_above_when_there_is_no_room_below() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 6));
        // Field on the last row: no room below; the panel flips above it.
        TimePicker::new(0)
            .open(true)
            .render(Rect::new(0, 5, 8, 1), &mut buf);
        // 8 rows fit at the top (rows 0..5 for the visible window cap of 8,
        // clamped to the 5 rows above the field): first list row is 00:00.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '0');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, ':');
        // The field row itself still carries its disclosure marker.
        assert_eq!(buf.get(Position::new(7, 5)).unwrap().symbol, '▾');
    }

    #[test]
    fn the_open_panel_is_opaque_and_the_background_does_not_bleed_through() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 5));
        background(&mut buf);
        TimePicker::new(0)
            .open(true)
            .range(0, 60) // 3 rows: 00:00, 00:30, 01:00
            .render(Rect::new(0, 0, 8, 1), &mut buf);
        // A blank panel cell (past the 5-char "HH:MM") is cleared: no '.' /
        // red-blue bleed.
        let cell = buf.get(Position::new(7, 1)).unwrap();
        assert_eq!(cell.bg, Color::Reset);
        assert_ne!(cell.symbol, '.');
        // Below the (3-row) panel the background is untouched.
        assert_eq!(buf.get(Position::new(0, 4)).unwrap().symbol, '.');
    }

    #[test]
    fn highlight_row_gets_the_bar_in_the_open_list() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        TimePicker::new(0)
            .open(true)
            .range(0, 90) // 4 rows at step 30
            .highlight(2) // the 01:00 row
            .selected_style(Style::new().bg(Color::Blue))
            .render(Rect::new(0, 0, 8, 1), &mut buf);
        // Row index 2 is panel row y=3 (field row 0, list starts y=1).
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 3)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn offset_scrolls_the_open_list() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        TimePicker::new(0)
            .open(true)
            .range(0, 120) // 00:00 00:30 01:00 01:30 02:00
            .offset(2) // first visible row is 01:00
            .render(Rect::new(0, 0, 8, 1), &mut buf);
        assert_eq!(lines_at(&buf, 0, 1), "01:00   ");
    }

    #[test]
    fn a_block_frames_the_open_list() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 6));
        TimePicker::new(0)
            .open(true)
            .range(0, 30) // 2 rows
            .block(Block::bordered())
            .render(Rect::new(0, 0, 10, 1), &mut buf);
        // Panel below the field at y=1, bordered: top-left corner there.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, '0'); // 00:00 inside
        assert_eq!(buf.get(Position::new(0, 4)).unwrap().symbol, '└');
    }

    #[test]
    fn an_inverted_range_is_a_total_no_op_panel() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        background(&mut buf);
        TimePicker::new(0)
            .open(true)
            .range(10 * 60, 8 * 60) // end before start ⇒ empty list
            .render(Rect::new(0, 0, 8, 1), &mut buf);
        // No times ⇒ no panel: the background below the field is intact.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '.');
        assert!(
            TimePicker::new(0)
                .open(true)
                .range(600, 480)
                .panel(Rect::new(0, 0, 8, 1))
                .is_empty()
        );
    }

    #[test]
    fn the_panel_accessor_is_empty_when_closed_and_natural_when_open() {
        let closed = TimePicker::new(0);
        assert!(closed.panel(Rect::new(0, 0, 8, 1)).is_empty());

        // Open with a 3-row list: natural placement anchored below.
        let open = TimePicker::new(0).open(true).range(0, 60);
        assert_eq!(open.panel(Rect::new(0, 0, 8, 1)), Rect::new(0, 1, 8, 3));

        // The cap bounds a very long list at VISIBLE_ROWS rows.
        let long = TimePicker::new(0).open(true); // full day, 48 step-30 rows
        assert_eq!(long.panel(Rect::new(0, 0, 8, 1)).height, VISIBLE_ROWS);
    }

    #[test]
    fn minute_at_inverts_the_step_layout_with_offset_and_block() {
        // Step 30 over 08:00..10:00: rows 480, 510, 540, 570, 600.
        let tp = TimePicker::new(8 * 60).open(true).range(8 * 60, 10 * 60);
        let area = Rect::new(0, 0, 8, 1);
        // Panel is rows y=1.. (field row 0). Row 0 → 08:00, row 2 → 09:00.
        assert_eq!(tp.minute_at(area, Position::new(3, 1)), Some(8 * 60));
        assert_eq!(tp.minute_at(area, Position::new(0, 3)), Some(9 * 60));
        assert_eq!(tp.minute_at(area, Position::new(0, 0)), None); // the field row
        assert_eq!(tp.minute_at(area, Position::new(0, 99)), None); // off-area

        // The scroll offset shifts the mapping.
        let s = tp.clone().offset(2); // first visible row is 09:00
        assert_eq!(s.minute_at(area, Position::new(0, 1)), Some(9 * 60));

        // A closed picker has no panel ⇒ always None.
        assert_eq!(
            TimePicker::new(0).minute_at(area, Position::new(0, 1)),
            None
        );
    }

    #[test]
    fn the_field_text_is_clipped_before_the_disclosure_marker() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        TimePicker::new(13 * 60 + 45).render(buf.area(), &mut buf);
        // Width 4: text region is cols 0..3, the marker is col 3.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '1'); // 13:45
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, ':');
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, '▾');
    }

    #[test]
    fn the_range_boundary_minute_formats_clamped_and_an_absurd_one_is_unset() {
        // The top of the full range (MINUTES_PER_DAY) is in range and the
        // label clamps to 24:00 (clock arithmetic, no panic).
        let top = TimePicker::new(MINUTES_PER_DAY).range(0, MINUTES_PER_DAY);
        assert_eq!(lines(top, 8, 1), "24:00  ▾\n");
        // A minute well past the range is simply *unset* (placeholder) — the
        // total, non-panicking fallback, exactly like an out-of-range select.
        let absurd = TimePicker::new(60_000)
            .range(0, MINUTES_PER_DAY)
            .placeholder("--:--");
        assert_eq!(lines(absurd, 8, 1), "--:--  ▾\n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        TimePicker::new(9 * 60)
            .open(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
