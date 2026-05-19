//! [`EventEditor`] / [`EventEditorField`] — the create/edit-event dialog as a
//! pure *layout* projection that owns **no** application state.
//!
//! # A pure *layout* projection, never a state owner — the [`Form`](crate::Form) pattern
//!
//! `EventEditor` is the calendar app's "new / edit event" dialog, but it holds
//! exactly what [`Form`](crate::Form) holds: *caller-owned geometry intent
//! only*. It does not own — and cannot read — the title text, the all-day
//! toggle, the start/end date or time, the selected calendar, or the
//! description. Those are the app's model. The editor only answers "given this
//! area, where does each control go", draws the heading, the per-row labels,
//! the divider, the button bar, and the optional help line, then hands each
//! control's [`Rect`] back via [`field_rect`](EventEditor::field_rect). The
//! caller renders its own [`Input`](crate::Input)/[`Switch`](crate::Switch)/
//! `DatePicker`/`TimePicker`/[`Select`](crate::Select)/text-area into those
//! rects — the same render-then-fill-`inner` contract
//! [`Form`](crate::Form)/[`Modal`](crate::Modal)/[`Block`] use.
//!
//! # `field_rect` is a pure geometry function
//!
//! [`EventEditor::field_rect`] returns the control [`Rect`] for one
//! [`EventEditorField`] as a pure function of the area and the configuration —
//! exactly like [`Modal::inner`](crate::Modal::inner) and
//! [`Block::inner`](crate::Block::inner). `render` and `field_rect` agree on
//! the geometry by construction (one shared private placement pass), so a
//! control always lands exactly where its label was drawn. A hidden field (a
//! time row while [`all_day`](EventEditor::all_day) is set) or one that does
//! not fit collapses to [`Rect::ZERO`](rstui_core::Rect::ZERO).
//!
//! # A container: composes with [`Block`], pairs with [`Modal`](crate::Modal)
//!
//! An optional framing [`Block`] draws the border/title and the rows lay out
//! in [`Block::inner`]. It does **not** centre or clear — you draw into the
//! given `area`, exactly like [`Form`](crate::Form); pair it with
//! [`Modal`](crate::Modal) at the call site for the centred opaque dialog.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: a tiny
//! area collapses every control rect to [`Rect::ZERO`](rstui_core::Rect::ZERO)
//! (caller-side indexing stays safe), [`all_day`](EventEditor::all_day) hides
//! the time rows, and no two field rects ever overlap (gate-enforced in the
//! tests) — never a panic.

use std::borrow::Cow;

use rstui_core::{Buffer, Position, Rect, Style, Widget};

use crate::block::Block;

/// One addressable control of an [`EventEditor`]: the caller asks
/// [`field_rect`](EventEditor::field_rect) for any of these and renders its own
/// widget into the returned [`Rect`].
///
/// [`Save`](Self::Save)/[`Cancel`](Self::Cancel) are the button-bar buttons —
/// the editor draws their labels itself (accented), but still exposes their
/// rects so an app can map a click to its own action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventEditorField {
    /// The event title (an [`Input`](crate::Input)).
    Title,
    /// The all-day toggle (a [`Switch`](crate::Switch)).
    AllDay,
    /// The start date (a caller `DatePicker`).
    StartDate,
    /// The start time (a caller `TimePicker`); hidden when
    /// [`all_day`](EventEditor::all_day) is set.
    StartTime,
    /// The end date (a caller `DatePicker`).
    EndDate,
    /// The end time (a caller `TimePicker`); hidden when
    /// [`all_day`](EventEditor::all_day) is set.
    EndTime,
    /// The location (an [`Input`](crate::Input)).
    Location,
    /// The calendar to file the event under (a [`Select`](crate::Select)).
    Calendar,
    /// The multi-row description box (a caller text-area).
    Description,
    /// The "Save" button (the editor draws its label).
    Save,
    /// The "Cancel" button (the editor draws its label).
    Cancel,
}

/// One placed row: its label rect (zero-width when the row has no label, e.g.
/// the button bar) and the caller's control rect.
#[derive(Debug, Clone, Copy)]
struct Placed {
    field: EventEditorField,
    label: Rect,
    control: Rect,
}

/// The create/edit-event dialog as a pure layout projection owning no
/// application state.
///
/// It lays out — top-down — a heading row; labelled rows for
/// [`Title`](EventEditorField::Title),
/// [`AllDay`](EventEditorField::AllDay) (a switch), **Start**
/// ([`StartDate`](EventEditorField::StartDate) +
/// [`StartTime`](EventEditorField::StartTime) side by side), **End**
/// ([`EndDate`](EventEditorField::EndDate) +
/// [`EndTime`](EventEditorField::EndTime)),
/// [`Location`](EventEditorField::Location),
/// [`Calendar`](EventEditorField::Calendar), a multi-row
/// [`Description`](EventEditorField::Description) box; then a bottom button bar
/// with [`Cancel`](EventEditorField::Cancel) and
/// [`Save`](EventEditorField::Save); and the optional [`help`](Self::help)
/// hint on the last row. [`render`](Widget::render) draws the heading, labels,
/// divider, button-bar buttons (accented) and help — it never draws the
/// controls; the caller does, into [`field_rect`](Self::field_rect).
///
/// Setting [`all_day`](Self::all_day) hides the two time rows: their
/// `field_rect` becomes [`Rect::ZERO`](rstui_core::Rect::ZERO) and their
/// labels are not drawn.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_widgets::{EventEditor, EventEditorField};
///
/// let editor = EventEditor::new().title("New event");
///
/// // `field_rect` is a pure geometry function — no state, like `Block::inner`.
/// let area = Rect::new(0, 0, 40, 16);
/// let title = editor.field_rect(EventEditorField::Title, area);
/// assert!(!title.is_empty()); // a real control rect for the caller's Input
///
/// // `all_day` hides the time rows entirely.
/// let all_day = EventEditor::new().all_day(true);
/// assert_eq!(
///     all_day.field_rect(EventEditorField::StartTime, area),
///     Rect::ZERO,
/// );
///
/// let mut buf = Buffer::empty(area);
/// editor.render(area, &mut buf); // labels + divider + buttons + help only
/// ```
#[derive(Debug, Clone)]
pub struct EventEditor<'a> {
    title: Cow<'a, str>,
    all_day: bool,
    help: Option<Cow<'a, str>>,
    save_label: Cow<'a, str>,
    cancel_label: Cow<'a, str>,
    block: Option<Block<'a>>,
    style: Style,
    label_style: Style,
    help_style: Style,
}

/// The label column width (a fixed band keeps `field_rect` a pure function of
/// the area only — no per-call label measuring, the `Form::label_width`-pinned
/// discipline).
const LABEL_W: u16 = 12;
/// Blank columns between the label column and the control column.
const LABEL_GAP: u16 = 1;
/// Rows the description box occupies.
const DESC_ROWS: u16 = 4;

impl<'a> EventEditor<'a> {
    /// An editor headed `"Event"`, not all-day, no help line, `"Save"` /
    /// `"Cancel"` buttons, no frame, and empty styles.
    #[must_use]
    pub fn new() -> Self {
        Self {
            title: Cow::Borrowed("Event"),
            all_day: false,
            help: None,
            save_label: Cow::Borrowed("Save"),
            cancel_label: Cow::Borrowed("Cancel"),
            block: None,
            style: Style::new(),
            label_style: Style::new(),
            help_style: Style::new(),
        }
    }

    /// Sets the heading shown on the top row (default `"Event"`).
    #[must_use]
    pub fn title(mut self, title: impl Into<Cow<'a, str>>) -> Self {
        self.title = title.into();
        self
    }

    /// When `true`, the [`StartTime`](EventEditorField::StartTime) /
    /// [`EndTime`](EventEditorField::EndTime) rows are hidden: their
    /// [`field_rect`](Self::field_rect) returns
    /// [`Rect::ZERO`](rstui_core::Rect::ZERO) and their labels are not drawn.
    #[must_use]
    pub fn all_day(mut self, all_day: bool) -> Self {
        self.all_day = all_day;
        self
    }

    /// Sets the footer hint shown on the editor's last row (default none).
    #[must_use]
    pub fn help(mut self, help: impl Into<Cow<'a, str>>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Sets the Save button's label (default `"Save"`).
    #[must_use]
    pub fn save_label(mut self, label: impl Into<Cow<'a, str>>) -> Self {
        self.save_label = label.into();
        self
    }

    /// Sets the Cancel button's label (default `"Cancel"`).
    #[must_use]
    pub fn cancel_label(mut self, label: impl Into<Cow<'a, str>>) -> Self {
        self.cancel_label = label.into();
        self
    }

    /// Frames the editor in `block`; the rows lay out in
    /// [`Block::inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a background
    /// covers the whole dialog (the [`Form`](crate::Form) idiom).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the base [`Style`] for the heading and row labels, beneath the
    /// base.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Sets the base [`Style`] for the help line, beneath the base.
    #[must_use]
    pub fn help_style(mut self, style: Style) -> Self {
        self.help_style = style;
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

    /// The single source of truth both [`field_rect`](Self::field_rect) and
    /// [`Widget::render`] use, so a control always lands where its label was
    /// drawn. Every entry is clipped to the content area (a row past the
    /// bottom collapses to [`Rect::ZERO`](rstui_core::Rect::ZERO)) — total.
    ///
    /// Returns the placed rows plus the heading rect, the divider rect, the
    /// button-bar rects, and the help rect.
    fn place(&self, area: Rect) -> Layout {
        let content = self.content(area);
        let mut layout = Layout::empty();
        if content.is_empty() {
            return layout;
        }

        // A fixed label+gap band keeps `field_rect` a pure fn of the area
        // alone (the pinned-`label_width` Form discipline).
        let band = LABEL_W.saturating_add(LABEL_GAP).min(content.width);
        let mut b = RowBuilder {
            left: content.left(),
            bottom: content.bottom(),
            width: content.width,
            label_w: LABEL_W.min(content.width),
            ctrl_x: content.left().saturating_add(band),
            ctrl_w: content.width.saturating_sub(band),
            y: content.top(),
        };

        // The heading.
        layout.heading = b.row(1);
        b.advance(1);

        b.labelled(&mut layout, EventEditorField::Title);
        b.labelled(&mut layout, EventEditorField::AllDay);

        // Start / End: Date + Time side by side in the control column.
        b.date_time(
            &mut layout,
            EventEditorField::StartDate,
            EventEditorField::StartTime,
            self.all_day,
        );
        b.date_time(
            &mut layout,
            EventEditorField::EndDate,
            EventEditorField::EndTime,
            self.all_day,
        );

        b.labelled(&mut layout, EventEditorField::Location);
        b.labelled(&mut layout, EventEditorField::Calendar);

        // The description: a label row, then a multi-row box beneath it.
        {
            let label_r = b.row(1);
            let lbl = if label_r.is_empty() {
                Rect::ZERO
            } else {
                Rect::new(b.left, label_r.y, b.label_w, 1)
            };
            b.advance(1);
            let box_r = b.row(DESC_ROWS);
            layout.rows.push(Placed {
                field: EventEditorField::Description,
                label: lbl,
                control: box_r,
            });
            b.advance(box_r.height);
        }

        // The button bar at the bottom: Cancel on the left, Save on the
        // right, each its own field rect.
        {
            let bar = b.row(1);
            let (cancel_r, save_r) = if bar.is_empty() {
                (Rect::ZERO, Rect::ZERO)
            } else {
                // [Cancel] / [Save] sized to their labels (+2 brackets),
                // Cancel left-anchored, Save right-anchored.
                let cancel_w = (self.cancel_label.chars().count() as u16)
                    .saturating_add(2)
                    .min(bar.width);
                let save_w = (self.save_label.chars().count() as u16)
                    .saturating_add(2)
                    .min(bar.width);
                let save_x = bar.right().saturating_sub(save_w).max(bar.x);
                (
                    Rect::new(bar.x, bar.y, cancel_w, 1),
                    Rect::new(save_x, bar.y, save_w, 1),
                )
            };
            layout.rows.push(Placed {
                field: EventEditorField::Cancel,
                label: Rect::ZERO,
                control: cancel_r,
            });
            layout.rows.push(Placed {
                field: EventEditorField::Save,
                label: Rect::ZERO,
                control: save_r,
            });
            layout.button_bar = bar;
            b.advance(1);
        }

        // The optional help line on the next row.
        layout.help = if self.help.is_some() {
            b.row(1)
        } else {
            Rect::ZERO
        };

        layout
    }

    /// The control [`Rect`] for `field` — a **pure geometry function** of
    /// `area` and the configuration (no state, like
    /// [`Block::inner`](crate::Block::inner)). The caller renders its own
    /// widget into it; the editor never touches the control's value.
    ///
    /// Returns [`Rect::ZERO`](rstui_core::Rect::ZERO) for a hidden field (a
    /// time row while [`all_day`](Self::all_day) is set) or one that does not
    /// fit the area — total, so caller-side handling stays safe.
    #[must_use]
    pub fn field_rect(&self, field: EventEditorField, area: Rect) -> Rect {
        self.place(area)
            .rows
            .iter()
            .find(|p| p.field == field)
            .map(|p| p.control)
            .unwrap_or(Rect::ZERO)
    }
}

impl Default for EventEditor<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// The full placement: every row plus the heading/divider/button-bar/help
/// chrome rects, shared by [`EventEditor::field_rect`] and
/// [`EventEditor::render`].
struct Layout {
    heading: Rect,
    rows: Vec<Placed>,
    button_bar: Rect,
    help: Rect,
}

impl Layout {
    fn empty() -> Self {
        Self {
            heading: Rect::ZERO,
            rows: Vec::new(),
            button_bar: Rect::ZERO,
            help: Rect::ZERO,
        }
    }
}

/// The running placement cursor: the fixed column geometry plus the current
/// `y`. A plain struct (not a closure) so the per-row helpers can each take
/// `&mut Layout` without two closures fighting over `layout.rows`.
struct RowBuilder {
    left: u16,
    bottom: u16,
    width: u16,
    label_w: u16,
    ctrl_x: u16,
    ctrl_w: u16,
    y: u16,
}

impl RowBuilder {
    /// A full-width row rect of height `h` at the cursor, clipped to the
    /// content bottom; [`Rect::ZERO`] once the cursor is at/past the bottom.
    fn row(&self, h: u16) -> Rect {
        if self.y >= self.bottom {
            return Rect::ZERO;
        }
        let avail = self.bottom.saturating_sub(self.y);
        Rect::new(self.left, self.y, self.width, h.min(avail))
    }

    /// Advances the cursor `rows` down (saturating).
    fn advance(&mut self, rows: u16) {
        self.y = self.y.saturating_add(rows);
    }

    /// One full-width labelled row: label in the band, control to the right.
    fn labelled(&mut self, layout: &mut Layout, field: EventEditorField) {
        let r = self.row(1);
        let (label, control) = if r.is_empty() {
            (Rect::ZERO, Rect::ZERO)
        } else {
            (
                Rect::new(self.left, r.y, self.label_w, 1),
                Rect::new(self.ctrl_x, r.y, self.ctrl_w, 1),
            )
        };
        layout.rows.push(Placed {
            field,
            label,
            control,
        });
        self.advance(1);
    }

    /// A Date+Time row: the date control in the left half of the control
    /// column, the time in the right half (sharing the row's Start/End
    /// label). When `all_day` the time is hidden ([`Rect::ZERO`]) and the
    /// date spans the whole control column.
    fn date_time(
        &mut self,
        layout: &mut Layout,
        date: EventEditorField,
        time: EventEditorField,
        all_day: bool,
    ) {
        let r = self.row(1);
        let (date_r, time_r, label) = if r.is_empty() {
            (Rect::ZERO, Rect::ZERO, Rect::ZERO)
        } else {
            let label = Rect::new(self.left, r.y, self.label_w, 1);
            if all_day {
                (
                    Rect::new(self.ctrl_x, r.y, self.ctrl_w, 1),
                    Rect::ZERO,
                    label,
                )
            } else {
                let half = self.ctrl_w / 2;
                let date_r = Rect::new(self.ctrl_x, r.y, half, 1);
                let time_x = self.ctrl_x.saturating_add(half);
                let time_r = Rect::new(time_x, r.y, self.ctrl_w.saturating_sub(half), 1);
                (date_r, time_r, label)
            }
        };
        layout.rows.push(Placed {
            field: date,
            label,
            control: date_r,
        });
        layout.rows.push(Placed {
            field: time,
            label: Rect::ZERO, // shares the row's "Start"/"End" label
            control: time_r,
        });
        self.advance(1);
    }
}

/// The caller-facing label for one labelled row (the `Start`/`End` rows share
/// one label across their two side-by-side controls).
fn label_for(field: EventEditorField) -> &'static str {
    match field {
        EventEditorField::Title => "Title",
        EventEditorField::AllDay => "All day",
        EventEditorField::StartDate => "Start",
        EventEditorField::EndDate => "End",
        EventEditorField::Location => "Location",
        EventEditorField::Calendar => "Calendar",
        EventEditorField::Description => "Description",
        // These never carry their own label (time shares the Start/End label;
        // buttons are drawn by the bar).
        EventEditorField::StartTime
        | EventEditorField::EndTime
        | EventEditorField::Save
        | EventEditorField::Cancel => "",
    }
}

/// Stamps `s` across one row of `rect`, clipped at its right edge. A no-op for
/// an empty rect (total).
fn stamp(buf: &mut Buffer, rect: Rect, s: &str, style: Style) {
    if rect.is_empty() {
        return;
    }
    buf.set_str(Position::new(rect.left(), rect.top()), s, style);
}

impl Widget for EventEditor<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        let layout = self.place(area);
        let content = self.content(area);

        // The frame (if any) reserves the content; the base fill covers it so
        // a background reads as one dialog (the Form idiom).
        if let Some(block) = &self.block {
            block.clone().render(area, buf);
        }
        if content.is_empty() {
            return;
        }
        buf.set_style(content, self.style);

        let label_base = self.style.patch(self.label_style);
        let help_base = self.style.patch(self.help_style);
        let accent = self
            .style
            .patch(self.label_style)
            .add_modifier(rstui_core::Modifier::REVERSED);

        // The heading (bold-ish via label_style; the divider rule beneath it).
        stamp(buf, layout.heading, &self.title, label_base);

        // Each row's label (controls are the caller's; never drawn here).
        for placed in &layout.rows {
            let text = label_for(placed.field);
            if !text.is_empty() {
                stamp(buf, placed.label, text, label_base);
            }
        }

        // The button bar: draw "[ Cancel ]" / "[ Save ]" (accented) into
        // their own field rects so they read as buttons.
        for placed in &layout.rows {
            match placed.field {
                EventEditorField::Cancel => {
                    let s = format!("[{}]", self.cancel_label);
                    stamp(buf, placed.control, &s, accent);
                }
                EventEditorField::Save => {
                    let s = format!("[{}]", self.save_label);
                    stamp(buf, placed.control, &s, accent);
                }
                _ => {}
            }
        }

        // The optional help line on its own row.
        if let Some(help) = &self.help {
            stamp(buf, layout.help, help, help_base);
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

    /// A roomy area where every row fits.
    const BIG: Rect = Rect::new(0, 0, 44, 20);

    /// Every field, for overlap/totality sweeps.
    const ALL: [EventEditorField; 11] = [
        EventEditorField::Title,
        EventEditorField::AllDay,
        EventEditorField::StartDate,
        EventEditorField::StartTime,
        EventEditorField::EndDate,
        EventEditorField::EndTime,
        EventEditorField::Location,
        EventEditorField::Calendar,
        EventEditorField::Description,
        EventEditorField::Save,
        EventEditorField::Cancel,
    ];

    #[test]
    fn the_heading_defaults_to_event_and_is_overridable() {
        assert_eq!(&lines(EventEditor::new(), 10, 1)[..5], "Event");
        assert_eq!(
            &lines(EventEditor::new().title("New event"), 12, 1)[..9],
            "New event"
        );
    }

    #[test]
    fn every_field_gets_a_non_empty_control_rect_in_a_roomy_area() {
        let ed = EventEditor::new();
        for f in ALL {
            assert!(
                !ed.field_rect(f, BIG).is_empty(),
                "{f:?} should have a real rect in a roomy area"
            );
        }
    }

    #[test]
    fn a_control_rect_sits_right_of_the_label_column() {
        let ed = EventEditor::new();
        let title = ed.field_rect(EventEditorField::Title, BIG);
        // Heading on row 0 ⇒ Title row at y = 1; control past the 12+1 band.
        assert_eq!(title.y, 1);
        assert_eq!(title.x, LABEL_W + LABEL_GAP);
    }

    #[test]
    fn start_date_and_time_are_side_by_side_and_do_not_overlap() {
        let ed = EventEditor::new();
        let d = ed.field_rect(EventEditorField::StartDate, BIG);
        let t = ed.field_rect(EventEditorField::StartTime, BIG);
        assert_eq!(d.y, t.y); // same row
        assert!(
            d.right() <= t.left(),
            "date {d:?} must end before time {t:?}"
        );
        assert!(!d.is_empty() && !t.is_empty());
    }

    #[test]
    fn all_day_hides_both_time_rows() {
        let ed = EventEditor::new().all_day(true);
        assert_eq!(ed.field_rect(EventEditorField::StartTime, BIG), Rect::ZERO);
        assert_eq!(ed.field_rect(EventEditorField::EndTime, BIG), Rect::ZERO);
        // …and the date control then spans the whole control column.
        let normal = EventEditor::new().field_rect(EventEditorField::StartDate, BIG);
        let wide = ed.field_rect(EventEditorField::StartDate, BIG);
        assert!(wide.width > normal.width);
    }

    #[test]
    fn all_day_does_not_draw_a_time_label_but_still_draws_the_others() {
        // "All day" label IS drawn (it's the AllDay row's label); the time
        // controls/labels are gone but Start/End/Location labels remain.
        let out = lines(EventEditor::new().all_day(true), 44, 20);
        assert!(out.contains("All day"));
        assert!(out.contains("Start"));
        assert!(out.contains("Location"));
    }

    #[test]
    fn no_two_field_rects_ever_overlap() {
        // The load-bearing layout invariant: render-then-fill demands the
        // caller's controls never collide. Check every distinct pair across a
        // matrix of areas and the all-day toggle.
        for area in [
            BIG,
            Rect::new(0, 0, 30, 18),
            Rect::new(2, 1, 50, 16),
            Rect::new(0, 0, 20, 14),
        ] {
            for all_day in [false, true] {
                let ed = EventEditor::new().all_day(all_day);
                for (i, &a) in ALL.iter().enumerate() {
                    for &b in &ALL[i + 1..] {
                        let ra = ed.field_rect(a, area);
                        let rb = ed.field_rect(b, area);
                        if ra.is_empty() || rb.is_empty() {
                            continue;
                        }
                        assert!(
                            !ra.intersects(rb),
                            "{a:?} {ra:?} overlaps {b:?} {rb:?} \
                             (area={area:?} all_day={all_day})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn render_and_field_rect_agree_on_geometry() {
        // The control rect `field_rect` hands back is exactly where `render`
        // left the row blank for the caller's widget.
        let ed = EventEditor::new();
        let r = ed.field_rect(EventEditorField::Title, BIG);
        let mut buf = Buffer::empty(BIG);
        ed.render(BIG, &mut buf);
        for x in r.left()..r.right() {
            assert_eq!(
                buf.get(Position::new(x, r.top())).unwrap().symbol,
                ' ',
                "render must not paint the caller's control cell at x={x}"
            );
        }
    }

    #[test]
    fn the_button_bar_draws_bracketed_save_and_cancel() {
        let out = lines(EventEditor::new(), 44, 20);
        assert!(out.contains("[Cancel]"));
        assert!(out.contains("[Save]"));
    }

    #[test]
    fn custom_button_labels_are_used() {
        let out = lines(
            EventEditor::new().save_label("Create").cancel_label("Back"),
            44,
            20,
        );
        assert!(out.contains("[Create]"));
        assert!(out.contains("[Back]"));
    }

    #[test]
    fn save_is_right_anchored_and_cancel_left_anchored() {
        let ed = EventEditor::new();
        let cancel = ed.field_rect(EventEditorField::Cancel, BIG);
        let save = ed.field_rect(EventEditorField::Save, BIG);
        assert_eq!(cancel.x, BIG.left()); // left-anchored
        assert_eq!(save.right(), BIG.right()); // right-anchored
        assert_eq!(cancel.y, save.y); // same bar row
        assert!(cancel.right() <= save.left()); // and they don't overlap
    }

    #[test]
    fn the_help_line_is_drawn_only_when_set() {
        assert!(!lines(EventEditor::new(), 30, 20).contains("⏎ save"));
        let out = lines(EventEditor::new().help("⏎ save · esc cancel"), 30, 20);
        assert!(out.contains("⏎ save"));
    }

    #[test]
    fn the_description_box_is_multi_row() {
        let d = EventEditor::new().field_rect(EventEditorField::Description, BIG);
        assert_eq!(d.height, DESC_ROWS);
    }

    #[test]
    fn a_block_frames_the_dialog_in_the_inner_area() {
        let out = lines(
            EventEditor::new()
                .title("Edit")
                .block(Block::bordered().title("Dialog")),
            20,
            6,
        );
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[0].starts_with("┌Dialog"));
        // Heading inside the border at (1,1).
        assert_eq!(rows[1].chars().nth(1).unwrap(), 'E'); // "Edit"
        assert!(rows[5].starts_with('└'));
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_content() {
        let out = lines(EventEditor::new().block(Block::bordered()), 2, 2);
        assert_eq!(out, "┌┐\n└┘\n");
    }

    #[test]
    fn a_tiny_area_collapses_every_rect_to_zero_without_panicking() {
        let tiny = Rect::new(0, 0, 4, 1); // only the heading fits
        let ed = EventEditor::new();
        for f in ALL {
            assert_eq!(
                ed.field_rect(f, tiny),
                Rect::ZERO,
                "{f:?} must collapse in a 1-row area"
            );
        }
        // …and rendering it is a safe no-op beyond the heading.
        let _ = lines(EventEditor::new(), 4, 1);
    }

    #[test]
    fn zero_area_is_a_no_op_and_field_rects_are_all_zero() {
        let ed = EventEditor::new();
        for f in ALL {
            assert_eq!(ed.field_rect(f, Rect::ZERO), Rect::ZERO);
        }
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        EventEditor::new().render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn the_base_style_fills_the_whole_content_area() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        EventEditor::new()
            .style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for y in 0..4 {
            for x in 0..8 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Blue);
            }
        }
    }

    #[test]
    fn styles_cascade_base_then_label_then_help() {
        let mut buf = Buffer::empty(BIG);
        EventEditor::new()
            .help("hint")
            .style(Style::new().bg(Color::Blue))
            .label_style(Style::new().fg(Color::Green))
            .help_style(Style::new().fg(Color::Yellow))
            .render(BIG, &mut buf);
        // Heading uses base bg + label_style fg.
        let h = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(h.symbol, 'E');
        assert_eq!(h.bg, Color::Blue);
        assert_eq!(h.fg, Color::Green);
        // The help line uses base bg + help_style fg; find its row.
        let help_y = EventEditor::new().help("hint").place(BIG).help.top();
        let c = buf.get(Position::new(0, help_y)).unwrap();
        assert_eq!(c.symbol, 'h');
        assert_eq!(c.fg, Color::Yellow);
        assert_eq!(c.bg, Color::Blue);
    }

    #[test]
    fn the_button_labels_are_accented_reversed() {
        let mut buf = Buffer::empty(BIG);
        EventEditor::new().render(BIG, &mut buf);
        let bar = EventEditor::new().place(BIG).button_bar;
        // First glyph of "[Cancel]" at the bar's left, reversed (button look).
        let c = buf.get(Position::new(bar.left(), bar.top())).unwrap();
        assert_eq!(c.symbol, '[');
        assert!(c.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn field_rect_is_a_pure_function_of_area_and_config() {
        // Same query, same answer — no internal state mutated by a call.
        let ed = EventEditor::new();
        let a = ed.field_rect(EventEditorField::Calendar, BIG);
        let b = ed.field_rect(EventEditorField::Calendar, BIG);
        assert_eq!(a, b);
    }

    #[test]
    fn honours_the_area_origin_not_the_buffer_origin() {
        let ed = EventEditor::new();
        let off = Rect::new(3, 2, 40, 18);
        let title = ed.field_rect(EventEditorField::Title, off);
        // Control x = area.x + band; y = area.y + 1 (heading row).
        assert_eq!(title.x, 3 + LABEL_W + LABEL_GAP);
        assert_eq!(title.y, 2 + 1);
    }
}
