//! A flagship **Calendar** experience: a real scheduling app composed from the
//! whole `rstui_widgets` calendar family. A [`DateNavigator`] toolbar on the
//! top row drives a Day / Week / Month / Year / Agenda body
//! ([`DayView`]/[`WeekView`]/[`MonthView`]/[`YearView`]/[`AgendaView`]); an
//! event opens an [`EventCard`] detail modal; `n` / `e` open the
//! [`EventEditor`] dialog with real [`Input`]/[`Switch`]/[`TimePicker`]/
//! [`DatePicker`]/[`Select`]/[`Editor`](rstui_code::Editor) controls wired to a
//! caller-owned model.
//!
//! **Keyboard** — no modal: `d/w/m/y/a` switch view, `←/→` (or `[`/`]`) page
//! the period, `t` jumps to today, `↑/↓` move the selection through the
//! period's events (or the day ±7 in Month), `n` creates / `e` edits an event,
//! `Enter` opens the selected event's detail, `x`/`Delete` deletes it. The
//! detail modal: `Esc`/`Enter` close, `e` edit, `x` delete. The editor modal:
//! `Tab`/`↑↓` move the [`FocusRing`], type into the focused text field,
//! `Space` toggles all-day / cycles the calendar, `←/→` nudge the focused time
//! / date / category, `Enter` on Save commits, `Esc` cancels.
//!
//! **Mouse** — the board drag seam: pressing an event in the active view
//! claims the gesture ([`on_press`](State::on_press) → drag ghost →
//! [`on_release`](State::on_release) maps the drop back to a day/slot and
//! reschedules the event, preserving its duration). A plain click on the
//! toolbar hits a [`NavTarget`]; a click on an event opens its detail; a click
//! on empty grid selects the day (Week/Day: seeds a new event there).
//!
//! Everything is **deterministic**: a fixed seed of ~12 May-2026 events, a
//! fixed "now" of 11:30 and "today" of the 14th, no wall clock and no RNG, so
//! the kitchen-sink Harness tests stay reproducible. The widgets do no date
//! math; this screen only does day-of-month arithmetic against a static
//! month/weekday name table.

use rstui_code::Editor;
use rstui_core::{
    Constraint, FocusId, FocusRing, KeyCode, Layout, Line, Position, Rect, Style, TextArea,
    TextEdit, stylize::Stylize,
};
use rstui_runtime::Frame;
use rstui_widgets::{
    AgendaView, Block, BorderType, CalendarEvent, DateNavigator, DatePicker, DayView, EventCard,
    EventEditor, EventEditorField, Input, MonthView, NavTarget, Paragraph, Select, Switch,
    TimePicker, ToastLevel, WeekView, Wrap, YearView,
    event::{MINUTES_PER_DAY, time_label},
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// Static month names (1-indexed via `- 1`). The screen — not the widgets —
/// may build period labels from a table; this is the only "date math".
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Static weekday names, `0 = Sunday … 6 = Saturday` (the C `tm_wday` order the
/// calendar widgets use). The 1st of May 2026 is a Friday (index 5).
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// The Week header's per-column day labels.
const WEEK_DAY_LABELS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// The four event categories, their fixed colour drawn from the theme.
const CATEGORIES: [&str; 4] = ["Work", "Personal", "Health", "Travel"];

/// Days in the modelled month (May 2026 has 31).
const DAY_COUNT: u32 = 31;

/// Weekday index of day-of-month 1 (May 1 2026 is a Friday).
const WEEKDAY_OF_FIRST: u32 = 5;

/// 2026's `(day_count, weekday_of_first)` per month, January…December
/// (`0` = Sunday). Caller-owned date facts the screen hands `YearView` so it
/// can draw all twelve mini-months — the widget itself does **no** date math
/// (without these it correctly renders blank). May is `(31, 5)`, consistent
/// with [`DAY_COUNT`]/[`WEEKDAY_OF_FIRST`].
const MONTHS_2026: [(u32, u32); 12] = [
    (31, 4), // Jan — Thu
    (28, 0), // Feb — Sun
    (31, 0), // Mar — Sun
    (30, 3), // Apr — Wed
    (31, 5), // May — Fri
    (30, 1), // Jun — Mon
    (31, 3), // Jul — Wed
    (31, 6), // Aug — Sat
    (30, 2), // Sep — Tue
    (31, 4), // Oct — Thu
    (30, 0), // Nov — Sun
    (31, 2), // Dec — Tue
];

/// The fixed "today" day-of-month (deterministic — no wall clock).
const TODAY: u32 = 14;

/// The fixed "now" minute-of-day for the now-line (11:30, deterministic).
const NOW_MIN: u16 = 11 * 60 + 30;

/// View-mode indices — these match [`DateNavigator`]'s default segment order
/// (`Day/Week/Month/Year/Agenda`).
const DAY: usize = 0;
const WEEK: usize = 1;
const MONTH: usize = 2;
const YEAR: usize = 3;
const AGENDA: usize = 4;

/// Editor focus-ring ids, in tab order. Time ids are skipped while all-day.
const F_TITLE: FocusId = FocusId::new(0);
const F_ALLDAY: FocusId = FocusId::new(1);
const F_START_DATE: FocusId = FocusId::new(2);
const F_START_TIME: FocusId = FocusId::new(3);
const F_END_DATE: FocusId = FocusId::new(4);
const F_END_TIME: FocusId = FocusId::new(5);
const F_LOCATION: FocusId = FocusId::new(6);
const F_CALENDAR: FocusId = FocusId::new(7);
const F_DESC: FocusId = FocusId::new(8);
const F_SAVE: FocusId = FocusId::new(9);
const F_CANCEL: FocusId = FocusId::new(10);

/// The full editor ring order (also the click hit-test order).
const EDITOR_ORDER: [FocusId; 11] = [
    F_TITLE,
    F_ALLDAY,
    F_START_DATE,
    F_START_TIME,
    F_END_DATE,
    F_END_TIME,
    F_LOCATION,
    F_CALENDAR,
    F_DESC,
    F_SAVE,
    F_CANCEL,
];

/// The category accent colour, resolved from the live theme.
fn category_color(theme: &Theme, cat: usize) -> rstui_core::Color {
    match cat {
        0 => theme.accent,     // Work
        1 => theme.accent_alt, // Personal
        2 => theme.ok,         // Health
        _ => theme.warn,       // Travel
    }
}

/// One seed event's plain data — kept separate from [`CalendarEvent`] so the
/// screen can rebuild the themed `&[CalendarEvent]` every frame (colours track
/// the live theme) without storing borrowed widget types in state.
#[derive(Debug, Clone)]
struct Ev {
    id: u64,
    title: String,
    day: u32,
    end_day: u32,
    start_min: u16,
    end_min: u16,
    all_day: bool,
    cat: usize,
    location: String,
    description: String,
}

impl Ev {
    /// Project this model row into a themed, owned [`CalendarEvent`].
    fn to_event(&self, theme: &Theme) -> CalendarEvent<'static> {
        let mut e = CalendarEvent::new(self.id, self.title.clone())
            .with_day(i64::from(self.day))
            .with_end_day(i64::from(self.end_day))
            .with_all_day(self.all_day)
            .with_color(category_color(theme, self.cat))
            .with_location(self.location.clone())
            .with_description(self.description.clone());
        if !self.all_day {
            e = e.with_span(self.start_min, self.end_min);
        }
        e
    }
}

/// The open modal, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modal {
    /// Nothing open — the calendar body has the keyboard.
    None,
    /// The [`EventCard`] detail for this event id.
    Detail(u64),
    /// The [`EventEditor`] dialog (create or edit).
    Editor,
}

/// The editor dialog's caller-owned model (the widget owns no state).
#[derive(Debug)]
struct EditorState {
    /// `None` ⇒ creating a new event; `Some(id)` ⇒ editing that event.
    edit_id: Option<u64>,
    title: TextEdit,
    all_day: bool,
    start_day: u32,
    end_day: u32,
    start_min: u16,
    end_min: u16,
    location: TextEdit,
    category: usize,
    description: TextArea,
    focus: FocusRing,
}

impl EditorState {
    /// A blank editor seeded to `day`, 09:00–10:00, Work, focus on the title.
    fn new(day: u32) -> Self {
        Self {
            edit_id: None,
            title: TextEdit::new(),
            all_day: false,
            start_day: day,
            end_day: day,
            start_min: 9 * 60,
            end_min: 10 * 60,
            location: TextEdit::new(),
            category: 0,
            description: TextArea::new(),
            focus: FocusRing::with_ids(EDITOR_ORDER),
        }
    }

    /// An editor prefilled from an existing model event.
    fn from_event(ev: &Ev) -> Self {
        let mut s = Self::new(ev.day);
        s.edit_id = Some(ev.id);
        s.title = TextEdit::from_value(ev.title.clone());
        s.all_day = ev.all_day;
        s.start_day = ev.day;
        s.end_day = ev.end_day;
        s.start_min = ev.start_min;
        s.end_min = ev.end_min;
        s.location = TextEdit::from_value(ev.location.clone());
        s.category = ev.cat;
        s.description = TextArea::from_value(ev.description.clone());
        s
    }

    /// The focused single-line field, when focus is on one.
    fn focused_text(&mut self) -> Option<&mut TextEdit> {
        match self.focus.focused()? {
            id if id == F_TITLE => Some(&mut self.title),
            id if id == F_LOCATION => Some(&mut self.location),
            _ => None,
        }
    }

    /// Advance the ring, skipping the two time fields while all-day (their
    /// rects are zero, so they must not be focusable either).
    fn focus_step(&mut self, forward: bool) {
        for _ in 0..EDITOR_ORDER.len() {
            if forward {
                self.focus.focus_next();
            } else {
                self.focus.focus_prev();
            }
            let f = self.focus.focused();
            if self.all_day && (f == Some(F_START_TIME) || f == Some(F_END_TIME)) {
                continue;
            }
            break;
        }
    }
}

/// An in-flight event drag: the event id and the live pointer position (the
/// Kanban-board drag seam, reused).
#[derive(Debug, Clone, Copy)]
struct Drag {
    id: u64,
    at: Position,
}

/// The calendar's caller-owned state — a plain model the pure `view` reads.
#[derive(Debug)]
pub(crate) struct State {
    events: Vec<Ev>,
    next_id: u64,
    view_mode: usize,
    selected_day: u32,
    selected_event: Option<u64>,
    agenda_off: usize,
    modal: Modal,
    editor: EditorState,
    drag: Option<Drag>,
}

impl State {
    /// A seeded calendar: ~12 events across May 2026, the 14th selected.
    pub(crate) fn new() -> Self {
        let events = vec![
            Ev {
                id: 1,
                title: "Team standup".into(),
                day: 14,
                end_day: 14,
                start_min: 9 * 60,
                end_min: 9 * 60 + 30,
                all_day: false,
                cat: 0,
                location: "Zoom".into(),
                description: "Daily sync — blockers & plan for the day.".into(),
            },
            Ev {
                id: 2,
                title: "Design review".into(),
                day: 14,
                end_day: 14,
                start_min: 9 * 60 + 15,
                end_min: 10 * 60 + 30,
                all_day: false,
                cat: 0,
                location: "Room 4".into(),
                description: "Overlaps standup — shows pack_day tiling.".into(),
            },
            Ev {
                id: 3,
                title: "1:1 with Sam".into(),
                day: 14,
                end_day: 14,
                start_min: 13 * 60,
                end_min: 13 * 60 + 45,
                all_day: false,
                cat: 1,
                location: "Cafe".into(),
                description: "Career & growth check-in.".into(),
            },
            Ev {
                id: 4,
                title: "Gym".into(),
                day: 14,
                end_day: 14,
                start_min: 18 * 60,
                end_min: 19 * 60,
                all_day: false,
                cat: 2,
                location: "Downtown gym".into(),
                description: "Leg day.".into(),
            },
            Ev {
                id: 5,
                title: "Conference".into(),
                day: 19,
                end_day: 21,
                start_min: 0,
                end_min: 0,
                all_day: true,
                cat: 3,
                location: "Convention Centre".into(),
                description: "Three-day multi-day all-day event.".into(),
            },
            Ev {
                id: 6,
                title: "Public holiday".into(),
                day: 4,
                end_day: 4,
                start_min: 0,
                end_min: 0,
                all_day: true,
                cat: 1,
                location: String::new(),
                description: "Office closed.".into(),
            },
            Ev {
                id: 7,
                title: "Sprint planning".into(),
                day: 11,
                end_day: 11,
                start_min: 10 * 60,
                end_min: 11 * 60 + 30,
                all_day: false,
                cat: 0,
                location: "Room 2".into(),
                description: "Plan the next sprint.".into(),
            },
            Ev {
                id: 8,
                title: "Dentist".into(),
                day: 7,
                end_day: 7,
                start_min: 8 * 60 + 30,
                end_min: 9 * 60 + 15,
                all_day: false,
                cat: 2,
                location: "Dental clinic".into(),
                description: "Routine check-up.".into(),
            },
            Ev {
                id: 9,
                title: "Flight to NYC".into(),
                day: 18,
                end_day: 18,
                start_min: 6 * 60 + 45,
                end_min: 10 * 60,
                all_day: false,
                cat: 3,
                location: "Gate B12".into(),
                description: "Travel for the conference.".into(),
            },
            Ev {
                id: 10,
                title: "Lunch with Alex".into(),
                day: 14,
                end_day: 14,
                start_min: 12 * 60,
                end_min: 13 * 60,
                all_day: false,
                cat: 1,
                location: "Bistro".into(),
                description: "Catch-up over lunch.".into(),
            },
            Ev {
                id: 11,
                title: "Quarterly review".into(),
                day: 28,
                end_day: 28,
                start_min: 14 * 60,
                end_min: 15 * 60 + 30,
                all_day: false,
                cat: 0,
                location: "Boardroom".into(),
                description: "Q2 numbers & roadmap.".into(),
            },
            Ev {
                id: 12,
                title: "Yoga class".into(),
                day: 22,
                end_day: 22,
                start_min: 7 * 60,
                end_min: 8 * 60,
                all_day: false,
                cat: 2,
                location: "Studio".into(),
                description: "Morning flow.".into(),
            },
        ];
        Self {
            events,
            next_id: 13,
            view_mode: MONTH,
            selected_day: TODAY,
            selected_event: Some(1),
            agenda_off: 0,
            modal: Modal::None,
            editor: EditorState::new(TODAY),
            drag: None,
        }
    }

    // --- model helpers ----------------------------------------------------

    /// The model index of `id`, if present.
    fn index_of(&self, id: u64) -> Option<usize> {
        self.events.iter().position(|e| e.id == id)
    }

    /// The events the active period shows, in display order — the list the
    /// `↑/↓` selection cycles. Day: that day; Week: the 7-day window around
    /// `selected_day`; Month/Year: every event; Agenda: chronological.
    fn period_event_ids(&self) -> Vec<u64> {
        let day = i64::from(self.selected_day);
        let mut ids: Vec<u64> = match self.view_mode {
            DAY => self
                .events
                .iter()
                .filter(|e| e.to_owned_covers(day))
                .map(|e| e.id)
                .collect(),
            WEEK => {
                let (lo, hi) = self.week_window();
                self.events
                    .iter()
                    .filter(|e| {
                        let s = i64::from(e.day);
                        let en = i64::from(e.end_day);
                        s <= i64::from(hi) && en >= i64::from(lo)
                    })
                    .map(|e| e.id)
                    .collect()
            }
            _ => self.events.iter().map(|e| e.id).collect(),
        };
        // Stable chronological order (day then start), deterministic.
        ids.sort_by_key(|id| {
            self.index_of(*id)
                .map(|i| {
                    let e = &self.events[i];
                    (e.day, if e.all_day { 0 } else { e.start_min }, e.id)
                })
                .unwrap_or((0, 0, *id))
        });
        ids
    }

    /// The inclusive `[lo, hi]` day-of-month window of the week containing
    /// `selected_day` (weeks start Sunday; clamped to the month).
    fn week_window(&self) -> (u32, u32) {
        // Weekday of `selected_day`: (WEEKDAY_OF_FIRST + (dom - 1)) % 7.
        let wd = (WEEKDAY_OF_FIRST + (self.selected_day - 1)) % 7;
        let lo = self.selected_day.saturating_sub(wd).max(1);
        let hi = (lo + 6).min(DAY_COUNT);
        (lo, hi)
    }

    /// Move the selection to the previous / next event in the active period.
    fn cycle_event(&mut self, forward: bool) {
        let ids = self.period_event_ids();
        if ids.is_empty() {
            self.selected_event = None;
            return;
        }
        let cur = self
            .selected_event
            .and_then(|id| ids.iter().position(|&i| i == id));
        let next = match cur {
            Some(i) if forward => (i + 1) % ids.len(),
            Some(i) => (i + ids.len() - 1) % ids.len(),
            None => 0,
        };
        self.selected_event = Some(ids[next]);
        // Follow the selection's day so it stays on-screen in Day/Week.
        if let Some(i) = self.index_of(ids[next]) {
            self.selected_day = self.events[i].day.clamp(1, DAY_COUNT);
        }
    }

    /// Delete the selected event (if any); returns its title for the toast.
    fn delete_selected(&mut self) -> Option<String> {
        let id = self.selected_event?;
        let i = self.index_of(id)?;
        let title = self.events[i].title.clone();
        self.events.remove(i);
        self.selected_event = None;
        Some(title)
    }

    /// Commit the editor: write a new event or replace the edited one.
    fn commit_editor(&mut self) -> String {
        let ed = &self.editor;
        let title = if ed.title.value().trim().is_empty() {
            "(untitled)".to_string()
        } else {
            ed.title.value().to_string()
        };
        let start_day = ed.start_day.clamp(1, DAY_COUNT);
        let end_day = ed.end_day.clamp(start_day, DAY_COUNT);
        let (start_min, end_min) = if ed.all_day {
            (0, 0)
        } else {
            let s = ed.start_min.min(MINUTES_PER_DAY);
            (s, ed.end_min.min(MINUTES_PER_DAY).max(s))
        };
        let row = Ev {
            id: ed.edit_id.unwrap_or(self.next_id),
            title: title.clone(),
            day: start_day,
            end_day,
            start_min,
            end_min,
            all_day: ed.all_day,
            cat: ed.category.min(CATEGORIES.len() - 1),
            location: ed.location.value().to_string(),
            description: ed.description.lines().join("\n"),
        };
        match ed.edit_id.and_then(|id| self.index_of(id)) {
            Some(i) => {
                self.events[i] = row;
            }
            None => {
                self.events.push(row);
                self.selected_event = Some(self.next_id);
                self.next_id += 1;
            }
        }
        if let Some(id) = self.editor.edit_id {
            self.selected_event = Some(id);
        }
        self.selected_day = start_day;
        title
    }

    // --- keyboard ---------------------------------------------------------

    /// Route a key. The screen is text-entry, so it receives every key; it
    /// consumes the ones it acts on.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match self.modal {
            Modal::None => self.on_key_calendar(code),
            Modal::Detail(id) => self.on_key_detail(code, id),
            Modal::Editor => self.on_key_editor(code),
        }
    }

    /// Keys with no modal open: navigation + open dialogs.
    fn on_key_calendar(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Char('d') => self.view_mode = DAY,
            KeyCode::Char('w') => self.view_mode = WEEK,
            KeyCode::Char('m') => self.view_mode = MONTH,
            KeyCode::Char('y') => self.view_mode = YEAR,
            KeyCode::Char('a') => self.view_mode = AGENDA,
            KeyCode::Char('t') => self.selected_day = TODAY,
            KeyCode::Left | KeyCode::Char('[') => self.page(false),
            KeyCode::Right | KeyCode::Char(']') => self.page(true),
            KeyCode::Up => {
                if self.view_mode == MONTH || self.view_mode == YEAR {
                    self.selected_day = self.selected_day.saturating_sub(7).max(1);
                } else {
                    self.cycle_event(false);
                }
            }
            KeyCode::Down => {
                if self.view_mode == MONTH || self.view_mode == YEAR {
                    self.selected_day = (self.selected_day + 7).min(DAY_COUNT);
                } else {
                    self.cycle_event(true);
                }
            }
            KeyCode::Char('n') => {
                self.editor = EditorState::new(self.selected_day);
                self.modal = Modal::Editor;
            }
            KeyCode::Char('e') => {
                if let Some(i) = self.selected_event.and_then(|id| self.index_of(id)) {
                    self.editor = EditorState::from_event(&self.events[i]);
                    self.modal = Modal::Editor;
                } else {
                    return ScreenOutcome::with_toast(ToastLevel::Info, "No event selected");
                }
            }
            KeyCode::Enter => {
                if let Some(id) = self.selected_event {
                    self.modal = Modal::Detail(id);
                } else {
                    return ScreenOutcome::ignored();
                }
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                return match self.delete_selected() {
                    Some(t) => {
                        ScreenOutcome::with_toast(ToastLevel::Warning, format!("Deleted {t}"))
                    }
                    None => ScreenOutcome::with_toast(ToastLevel::Info, "No event selected"),
                };
            }
            _ => return ScreenOutcome::ignored(),
        }
        self.clamp();
        ScreenOutcome::consumed()
    }

    /// Keys while the detail card is open.
    fn on_key_detail(&mut self, code: KeyCode, id: u64) -> ScreenOutcome {
        match code {
            KeyCode::Esc | KeyCode::Enter => self.modal = Modal::None,
            KeyCode::Char('e') => {
                if let Some(i) = self.index_of(id) {
                    self.editor = EditorState::from_event(&self.events[i]);
                    self.modal = Modal::Editor;
                }
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                self.selected_event = Some(id);
                let t = self.delete_selected();
                self.modal = Modal::None;
                return ScreenOutcome::with_toast(
                    ToastLevel::Warning,
                    format!("Deleted {}", t.unwrap_or_default()),
                );
            }
            _ => return ScreenOutcome::consumed(),
        }
        ScreenOutcome::consumed()
    }

    /// Keys while the editor dialog is open.
    fn on_key_editor(&mut self, code: KeyCode) -> ScreenOutcome {
        let focused = self.editor.focus.focused().unwrap_or(F_TITLE);
        match code {
            KeyCode::Esc => self.modal = Modal::None,
            KeyCode::Tab => self.editor.focus_step(true),
            KeyCode::BackTab => self.editor.focus_step(false),
            KeyCode::Down => self.editor.focus_step(true),
            KeyCode::Up => self.editor.focus_step(false),
            KeyCode::Enter => {
                if focused == F_SAVE {
                    let t = self.commit_editor();
                    self.modal = Modal::None;
                    return ScreenOutcome::with_toast(ToastLevel::Success, format!("Saved {t}"));
                } else if focused == F_CANCEL {
                    self.modal = Modal::None;
                } else if focused == F_DESC {
                    self.editor.description.insert_char('\n');
                } else {
                    self.editor.focus_step(true);
                }
            }
            KeyCode::Char(' ') => match focused {
                id if id == F_ALLDAY => self.editor.all_day = !self.editor.all_day,
                id if id == F_CALENDAR => {
                    self.editor.category = (self.editor.category + 1) % CATEGORIES.len();
                }
                id if id == F_SAVE => {
                    let t = self.commit_editor();
                    self.modal = Modal::None;
                    return ScreenOutcome::with_toast(ToastLevel::Success, format!("Saved {t}"));
                }
                id if id == F_CANCEL => self.modal = Modal::None,
                id if id == F_DESC => self.editor.description.insert_char(' '),
                _ => {
                    if let Some(t) = self.editor.focused_text() {
                        t.insert_char(' ');
                    }
                }
            },
            KeyCode::Left => self.editor_adjust(focused, -1),
            KeyCode::Right => self.editor_adjust(focused, 1),
            KeyCode::Backspace => {
                if focused == F_DESC {
                    self.editor.description.delete_backward();
                } else if let Some(t) = self.editor.focused_text() {
                    t.delete_backward();
                }
            }
            KeyCode::Char(c) => {
                if focused == F_DESC {
                    self.editor.description.insert_char(c);
                } else if let Some(t) = self.editor.focused_text() {
                    t.insert_char(c);
                } else {
                    return ScreenOutcome::ignored();
                }
            }
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// `←/→` on a focused editor control: nudge the time ±15m, step the date
    /// ±1 day, or cycle the category.
    fn editor_adjust(&mut self, focused: FocusId, dir: i32) {
        let step = |v: u16, d: i32| -> u16 {
            let n = i32::from(v) + d * 15;
            n.clamp(0, i32::from(MINUTES_PER_DAY)) as u16
        };
        let day_step = |v: u32, d: i32| -> u32 { (v as i32 + d).clamp(1, DAY_COUNT as i32) as u32 };
        match focused {
            id if id == F_START_TIME => {
                self.editor.start_min = step(self.editor.start_min, dir);
                self.editor.end_min = self.editor.end_min.max(self.editor.start_min);
            }
            id if id == F_END_TIME => {
                self.editor.end_min = step(self.editor.end_min, dir).max(self.editor.start_min);
            }
            id if id == F_START_DATE => {
                self.editor.start_day = day_step(self.editor.start_day, dir);
                self.editor.end_day = self.editor.end_day.max(self.editor.start_day);
            }
            id if id == F_END_DATE => {
                self.editor.end_day = day_step(self.editor.end_day, dir).max(self.editor.start_day);
            }
            id if id == F_CALENDAR => {
                let n = CATEGORIES.len() as i32;
                self.editor.category = ((self.editor.category as i32 + dir).rem_euclid(n)) as usize;
            }
            _ => {}
        }
    }

    /// Page the period backward / forward (Day ±1, Week ±7, clamped; the
    /// other modes have no per-period step).
    fn page(&mut self, forward: bool) {
        let delta: i32 = match self.view_mode {
            DAY => 1,
            WEEK => 7,
            _ => 0,
        };
        if delta == 0 {
            return;
        }
        let d = self.selected_day as i32 + if forward { delta } else { -delta };
        self.selected_day = d.clamp(1, DAY_COUNT as i32) as u32;
    }

    /// Keep every index in range — totality.
    fn clamp(&mut self) {
        self.view_mode = self.view_mode.min(AGENDA);
        self.selected_day = self.selected_day.clamp(1, DAY_COUNT);
        if let Some(id) = self.selected_event {
            if self.index_of(id).is_none() {
                self.selected_event = None;
            }
        }
    }

    // --- mouse ------------------------------------------------------------

    /// Pointer pressed: if it lands on an event in the active view, select it
    /// and pick it up (claim the gesture). Otherwise defer to the click path.
    pub(crate) fn on_press(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        if self.modal != Modal::None {
            self.drag = None;
            return ScreenOutcome::ignored();
        }
        let theme = Theme::new(crate::theme::Mode::Dark);
        let evs = self.events_for(&theme);
        let body = Self::body_area(content);
        if let Some(id) = self.event_at(&evs, body, pos) {
            self.selected_event = Some(id);
            self.drag = Some(Drag { id, at: pos });
            return ScreenOutcome::consumed();
        }
        self.drag = None;
        ScreenOutcome::ignored()
    }

    /// Pointer moved while carrying an event — track it for the ghost.
    pub(crate) fn on_pointer_drag(&mut self, pos: Position, _content: Rect) -> ScreenOutcome {
        if let Some(d) = &mut self.drag {
            d.at = pos;
            return ScreenOutcome::consumed();
        }
        ScreenOutcome::ignored()
    }

    /// Pointer released: drop the carried event onto the day/slot under it,
    /// preserving its duration.
    pub(crate) fn on_release(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let Some(d) = self.drag.take() else {
            return ScreenOutcome::ignored();
        };
        let theme = Theme::new(crate::theme::Mode::Dark);
        let evs = self.events_for(&theme);
        let body = Self::body_area(content);
        let Some(i) = self.index_of(d.id) else {
            return ScreenOutcome::consumed();
        };
        let title = self.events[i].title.clone();
        let dur_days = self.events[i].end_day.saturating_sub(self.events[i].day);
        let dur_min = self.events[i]
            .end_min
            .saturating_sub(self.events[i].start_min);

        let moved = match self.view_mode {
            WEEK => {
                let wv = self.week_widget(&evs);
                wv.slot_at(body, pos).map(|(day, min)| {
                    let day = (day.clamp(1, i64::from(DAY_COUNT))) as u32;
                    (day, Some(min))
                })
            }
            DAY => {
                let dv = self.day_widget(&evs);
                dv.minute_at(body, pos)
                    .map(|min| (self.selected_day, Some(min)))
            }
            MONTH => {
                let mv = self.month_widget(&evs);
                mv.day_at(body, pos).map(|dom| (dom, None))
            }
            _ => None,
        };

        if let Some((day, min)) = moved {
            let day = day.clamp(1, DAY_COUNT);
            let e = &mut self.events[i];
            e.day = day;
            e.end_day = (day + dur_days).min(DAY_COUNT);
            if let Some(m) = min {
                if !e.all_day {
                    let s = m.min(MINUTES_PER_DAY);
                    e.start_min = s;
                    e.end_min = (s.saturating_add(dur_min)).min(MINUTES_PER_DAY);
                }
            }
            self.selected_event = Some(d.id);
            self.selected_day = day;
            return ScreenOutcome::with_toast(ToastLevel::Info, format!("Moved {title}"));
        }
        ScreenOutcome::consumed()
    }

    /// A plain click (a release with no drag): the toolbar, then the active
    /// view's event / day hit-test, then the open modal.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let theme = Theme::new(crate::theme::Mode::Dark);
        let evs = self.events_for(&theme);
        let (nav_row, body) = Self::split(content);

        // The editor modal: hit-test its field rects first (it is opaque).
        if self.modal == Modal::Editor {
            return self.editor_click(pos, content);
        }
        if let Modal::Detail(_) = self.modal {
            // Any click closes the detail card (it has no inner controls).
            self.modal = Modal::None;
            return ScreenOutcome::consumed();
        }

        // The toolbar.
        let nav = self.nav_widget();
        if let Some(t) = nav.target_at(nav_row, pos) {
            match t {
                NavTarget::Prev => self.page(false),
                NavTarget::Next => self.page(true),
                NavTarget::Today => self.selected_day = TODAY,
                NavTarget::New => {
                    self.editor = EditorState::new(self.selected_day);
                    self.modal = Modal::Editor;
                }
                NavTarget::Mode(i) => self.view_mode = i.min(AGENDA),
            }
            self.clamp();
            return ScreenOutcome::consumed();
        }

        // An event in the body opens its detail.
        if let Some(id) = self.event_at(&evs, body, pos) {
            self.selected_event = Some(id);
            self.modal = Modal::Detail(id);
            return ScreenOutcome::consumed();
        }

        // Empty grid: select the day; Week/Day also seed a new event there.
        match self.view_mode {
            MONTH => {
                if let Some(dom) = self.month_widget(&evs).day_at(body, pos) {
                    self.selected_day = dom.clamp(1, DAY_COUNT);
                    return ScreenOutcome::consumed();
                }
            }
            YEAR => {
                if let Some(_m) = self.year_widget(&evs).month_at(body, pos) {
                    // One modelled month — selecting it just keeps the day.
                    return ScreenOutcome::consumed();
                }
            }
            WEEK => {
                if let Some((day, min)) = self.week_widget(&evs).slot_at(body, pos) {
                    let day = (day.clamp(1, i64::from(DAY_COUNT))) as u32;
                    self.selected_day = day;
                    let mut ed = EditorState::new(day);
                    ed.start_min = min;
                    ed.end_min = (min.saturating_add(60)).min(MINUTES_PER_DAY);
                    self.editor = ed;
                    self.modal = Modal::Editor;
                    return ScreenOutcome::consumed();
                }
            }
            DAY => {
                if let Some(min) = self.day_widget(&evs).minute_at(body, pos) {
                    let mut ed = EditorState::new(self.selected_day);
                    ed.start_min = min;
                    ed.end_min = (min.saturating_add(60)).min(MINUTES_PER_DAY);
                    self.editor = ed;
                    self.modal = Modal::Editor;
                    return ScreenOutcome::consumed();
                }
            }
            _ => {}
        }
        ScreenOutcome::ignored()
    }

    /// Hit-test the editor dialog's field rects (Save/Cancel act; a field
    /// focuses; the switch toggles; the calendar select cycles).
    fn editor_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let area = Self::modal_inner(content);
        let ed = self.editor_widget();
        for &id in &EDITOR_ORDER {
            let field = focus_to_field(id);
            if self.editor.all_day && (id == F_START_TIME || id == F_END_TIME) {
                continue;
            }
            let r = ed.field_rect(field, area);
            if !r.is_empty() && r.contains(pos) {
                match id {
                    i if i == F_SAVE => {
                        let t = self.commit_editor();
                        self.modal = Modal::None;
                        return ScreenOutcome::with_toast(
                            ToastLevel::Success,
                            format!("Saved {t}"),
                        );
                    }
                    i if i == F_CANCEL => {
                        self.modal = Modal::None;
                        return ScreenOutcome::consumed();
                    }
                    i if i == F_ALLDAY => {
                        self.editor.focus.focus(id);
                        self.editor.all_day = !self.editor.all_day;
                    }
                    i if i == F_CALENDAR => {
                        self.editor.focus.focus(id);
                        self.editor.category = (self.editor.category + 1) % CATEGORIES.len();
                    }
                    _ => {
                        self.editor.focus.focus(id);
                    }
                }
                return ScreenOutcome::consumed();
            }
        }
        // A click outside any field but inside the dialog is swallowed
        // (the modal is opaque); nothing changes.
        ScreenOutcome::consumed()
    }

    /// Wheel scroll: Agenda scrolls; Month/Day step the day; otherwise noop.
    pub(crate) fn on_scroll(&mut self, up: bool) {
        if self.modal != Modal::None {
            return;
        }
        match self.view_mode {
            AGENDA => {
                let theme = Theme::new(crate::theme::Mode::Dark);
                let evs = self.events_for(&theme);
                let rows = self.agenda_widget(&evs).row_count();
                if up {
                    self.agenda_off = self.agenda_off.saturating_sub(1);
                } else {
                    self.agenda_off = (self.agenda_off + 1).min(rows.saturating_sub(1));
                }
            }
            DAY | MONTH | WEEK => {
                if up {
                    self.selected_day = self.selected_day.saturating_sub(1).max(1);
                } else {
                    self.selected_day = (self.selected_day + 1).min(DAY_COUNT);
                }
            }
            _ => {}
        }
    }

    /// A paste lands in the focused editor text field (when the editor is up).
    pub(crate) fn on_paste(&mut self, text: &str) {
        if self.modal != Modal::Editor {
            return;
        }
        if self.editor.focus.focused() == Some(F_DESC) {
            self.editor.description.insert_str(text);
        } else if let Some(t) = self.editor.focused_text() {
            t.insert_str(text);
        }
    }

    // --- geometry shared by render + hit-tests ----------------------------

    /// Toolbar row (row 0) and the body area beneath it — the one split both
    /// `view` and the hit-tests derive from, so they cannot disagree.
    fn split(content: Rect) -> (Rect, Rect) {
        let [nav, body] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(content);
        (nav, body)
    }

    /// The body rect alone.
    fn body_area(content: Rect) -> Rect {
        Self::split(content).1
    }

    /// The editor modal's inner content rect for `content` (the screen area).
    fn modal_inner(content: Rect) -> Rect {
        let m = rstui_widgets::Modal::new()
            .block(Block::bordered())
            .width(Constraint::Percentage(70))
            .height(Constraint::Percentage(80));
        m.inner(content)
    }

    /// The detail modal's inner rect.
    fn detail_inner(content: Rect) -> Rect {
        let m = rstui_widgets::Modal::new()
            .block(Block::bordered())
            .width(Constraint::Percentage(50))
            .height(Constraint::Percentage(50));
        m.inner(content)
    }

    /// The themed events slice the views project (rebuilt per call so colours
    /// track the live theme — the `CalendarEvent` borrows from this `Vec`).
    fn events_for(&self, theme: &Theme) -> Vec<CalendarEvent<'static>> {
        self.events.iter().map(|e| e.to_event(theme)).collect()
    }

    // --- widget constructors (one geometry, render + hit-test share) ------

    fn nav_widget(&self) -> DateNavigator<'static> {
        DateNavigator::new(self.period_label()).mode(self.view_mode)
    }

    fn month_widget<'a>(&self, evs: &'a [CalendarEvent<'a>]) -> MonthView<'a> {
        MonthView::new(2026, 5, DAY_COUNT, WEEKDAY_OF_FIRST)
            .events(evs)
            .first_day(1)
            .selected(Some(self.selected_day))
            .today(Some(TODAY))
    }

    fn week_widget<'a>(&self, evs: &'a [CalendarEvent<'a>]) -> WeekView<'a> {
        let (lo, _hi) = self.week_window();
        WeekView::new(i64::from(lo), 7)
            .events(evs)
            .day_labels(&WEEK_DAY_LABELS)
            .today(Some(i64::from(TODAY)))
            .hours(6, 22)
            .now(Some(NOW_MIN))
            .selected_event(self.selected_event)
    }

    fn day_widget<'a>(&self, evs: &'a [CalendarEvent<'a>]) -> DayView<'a> {
        DayView::new(i64::from(self.selected_day))
            .events(evs)
            .day_label(self.day_label())
            .hours(6, 22)
            .now(if self.selected_day == TODAY {
                Some(NOW_MIN)
            } else {
                None
            })
            .selected_event(self.selected_event)
    }

    fn year_widget<'a>(&self, _evs: &'a [CalendarEvent<'a>]) -> YearView<'a> {
        // Hand it the caller-owned 2026 month facts — without `.months(...)`
        // YearView has nothing to lay out and renders blank.
        YearView::new(2026)
            .months(&MONTHS_2026)
            .first_weekday(0)
            .today(Some((5, TODAY)))
            .selected(Some((5, self.selected_day)))
    }

    fn agenda_widget<'a>(&self, evs: &'a [CalendarEvent<'a>]) -> AgendaView<'a> {
        AgendaView::new(evs)
            .offset(self.agenda_off)
            .selected(self.selected_event)
            .empty_text("No upcoming events")
    }

    fn editor_widget(&self) -> EventEditor<'static> {
        EventEditor::new()
            .title(if self.editor.edit_id.is_some() {
                "Edit event"
            } else {
                "New event"
            })
            .all_day(self.editor.all_day)
            .help("Tab move · Space toggle · ←→ adjust · ⏎ save · Esc cancel")
    }

    /// Find the event under `pos` for the active view.
    fn event_at(&self, evs: &[CalendarEvent<'_>], body: Rect, pos: Position) -> Option<u64> {
        match self.view_mode {
            DAY => self.day_widget(evs).event_at(body, pos),
            WEEK => self.week_widget(evs).event_at(body, pos),
            MONTH => self.month_widget(evs).event_at(body, pos),
            AGENDA => self.agenda_widget(evs).event_at(body, pos),
            _ => None,
        }
    }

    // --- labels (the only "date math": a static-table lookup) -------------

    /// The weekday name of `selected_day`.
    fn weekday_name(&self) -> &'static str {
        let wd = (WEEKDAY_OF_FIRST + (self.selected_day - 1)) % 7;
        WEEKDAYS[wd as usize]
    }

    /// e.g. `"Thu 14 May 2026"`.
    fn day_label(&self) -> String {
        format!(
            "{} {} {} 2026",
            self.weekday_name(),
            self.selected_day,
            MONTHS[4]
        )
    }

    /// The toolbar's centred period label, per view mode.
    fn period_label(&self) -> String {
        match self.view_mode {
            DAY => self.day_label(),
            WEEK => {
                let (lo, hi) = self.week_window();
                format!("{} {}–{} 2026", MONTHS[4], lo, hi)
            }
            YEAR => "2026".to_string(),
            AGENDA => format!("Agenda · {} 2026", MONTHS[4]),
            _ => format!("{} 2026", MONTHS[4]),
        }
    }

    // --- render -----------------------------------------------------------

    /// Draw the calendar. `tick` is unused (deterministic — no animation).
    pub(crate) fn view(&self, theme: &Theme, _tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let evs = self.events_for(theme);
        let (nav_row, body) = Self::split(area);

        // The toolbar.
        frame.render_widget(
            self.nav_widget()
                .style(Style::new().fg(theme.text).bg(theme.raised))
                .label_style(Style::new().fg(theme.accent).bg(theme.raised))
                .button_style(Style::new().fg(theme.dim).bg(theme.raised))
                .selected_style(theme.selection()),
            nav_row,
        );

        // The active view inside a titled, themed block.
        let title = match self.view_mode {
            DAY => " Day ",
            WEEK => " Week ",
            MONTH => " Month ",
            YEAR => " Year ",
            _ => " Agenda ",
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Line::from(title).style(theme.accent_text()))
            .border_style(theme.border())
            .style(theme.body());

        match self.view_mode {
            DAY => frame.render_widget(
                self.day_widget(&evs)
                    .block(block)
                    .style(theme.body())
                    .ruler_style(theme.caption())
                    .header_style(theme.accent_text())
                    .all_day_style(Style::new().fg(theme.warn).bg(theme.surface))
                    .now_style(Style::new().fg(theme.err))
                    .grid_style(theme.border())
                    .selected_style(theme.selection()),
                body,
            ),
            WEEK => frame.render_widget(
                self.week_widget(&evs)
                    .block(block)
                    .style(theme.body())
                    .grid_style(theme.border())
                    .ruler_style(theme.caption())
                    .header_style(theme.accent_text())
                    .all_day_style(Style::new().fg(theme.warn).bg(theme.surface))
                    .now_style(Style::new().fg(theme.err))
                    .selected_style(theme.selection()),
                body,
            ),
            MONTH => frame.render_widget(
                self.month_widget(&evs)
                    .block(block)
                    .style(theme.body())
                    .header_style(theme.accent_text())
                    .weekday_style(theme.caption())
                    .selected_style(theme.selection())
                    .today_style(Style::new().fg(theme.warn).bg(theme.surface).bold())
                    .grid_style(theme.border()),
                body,
            ),
            YEAR => frame.render_widget(
                self.year_widget(&evs)
                    .block(block)
                    .style(theme.body())
                    .header_style(theme.accent_text())
                    .title_style(theme.caption()),
                body,
            ),
            _ => frame.render_widget(
                self.agenda_widget(&evs)
                    .block(block)
                    .style(theme.body())
                    .day_header_style(theme.accent_text())
                    .time_style(theme.caption())
                    .selected_style(theme.selection()),
                body,
            ),
        }

        // The footer hint line over the body's bottom-left (so the screen
        // explains itself even with no modal).
        let hint =
            "d/w/m/y/a view · ←→ page · t today · ↑↓ event · n new · e edit · ⏎ open · x delete";
        frame.render_widget(
            Line::from(hint.fg(theme.dim)),
            Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
        );

        // Modals over the whole screen area.
        match self.modal {
            Modal::None => {}
            Modal::Detail(id) => self.view_detail(theme, frame, area, id),
            Modal::Editor => self.view_editor(theme, frame, area),
        }

        // The drag ghost, last, following the pointer (the board seam). In
        // Day view it is the SAME-SIZE, grid-aligned slot the event will
        // occupy (snapped to 15-min rows, full event-column width) so the
        // user sees exactly where it lands and which hour it aligns with;
        // every other view keeps the compact floating box.
        if let Some(d) = self.drag {
            if let Some(i) = self.index_of(d.id) {
                let label = self.events[i].title.clone();
                let aligned = if self.view_mode == DAY {
                    // Same block inset as the rendered day widget (geometry
                    // depends only on borders/padding, not title/colour).
                    let dv = self
                        .day_widget(&evs)
                        .block(Block::bordered().border_type(BorderType::Rounded));
                    dv.minute_at(body, d.at).and_then(|start| {
                        let dur = evs
                            .iter()
                            .find(|e| e.id() == d.id)
                            .map_or(60, |e| e.duration_min())
                            .max(15);
                        let end = start.saturating_add(dur).min(MINUTES_PER_DAY);
                        let r = dv.slot_rect(body, start, end);
                        (!r.is_empty()).then_some(r)
                    })
                } else {
                    None
                };
                let grect = aligned.unwrap_or_else(|| {
                    let w = (label.chars().count() as u16 + 4).min(28).min(area.width);
                    let h = 3u16.min(area.height);
                    let gx = d.at.x.min(area.right().saturating_sub(w)).max(area.x);
                    let gy = d.at.y.min(area.bottom().saturating_sub(h)).max(area.y);
                    Rect::new(gx, gy, w, h)
                });
                let gblock = Block::bordered()
                    .border_type(BorderType::Thick)
                    .border_style(theme.border_focused())
                    .style(Style::new().bg(theme.raised));
                let gin = gblock.inner(grect);
                frame.render_widget(gblock, grect);
                frame.render_widget(
                    Paragraph::new(Line::from(label.fg(theme.accent).bold()))
                        .wrap(Wrap { trim: true })
                        .style(Style::new().bg(theme.raised)),
                    gin,
                );
            }
        }
    }

    /// The [`EventCard`] detail modal.
    fn view_detail(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect, id: u64) {
        let modal = rstui_widgets::Modal::new()
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(
                        Line::from(" Event · e edit · x delete · Esc close ")
                            .style(theme.accent_text()),
                    )
                    .border_style(theme.border_focused())
                    .style(theme.body()),
            )
            .width(Constraint::Percentage(50))
            .height(Constraint::Percentage(50))
            .style(theme.body())
            .backdrop_style(Style::new().bg(theme.base));
        frame.render_widget(modal, area);
        let inner = Self::detail_inner(area);
        if inner.is_empty() {
            return;
        }
        let Some(i) = self.index_of(id) else {
            return;
        };
        let evs = self.events_for(theme);
        let ev = &evs[i];
        let day_label = format!(
            "{} {} {} 2026",
            {
                let wd = (WEEKDAY_OF_FIRST + (self.events[i].day - 1)) % 7;
                WEEKDAYS[wd as usize]
            },
            self.events[i].day,
            MONTHS[4]
        );
        frame.render_widget(
            EventCard::new(ev)
                .day_label(day_label)
                .style(theme.body())
                .title_style(theme.heading())
                .time_style(theme.accent_text())
                .location_style(theme.caption())
                .divider_style(theme.border()),
            inner,
        );
    }

    /// The [`EventEditor`] dialog with the real controls drawn into its rects.
    fn view_editor(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let modal = rstui_widgets::Modal::new()
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(Line::from(" Event editor ").style(theme.accent_text()))
                    .border_style(theme.border_focused())
                    .style(theme.body()),
            )
            .width(Constraint::Percentage(70))
            .height(Constraint::Percentage(80))
            .style(theme.body())
            .backdrop_style(Style::new().bg(theme.base));
        frame.render_widget(modal, area);
        let inner = Self::modal_inner(area);
        if inner.is_empty() {
            return;
        }

        let editor = self.editor_widget();
        // The editor draws the heading, labels, divider, buttons & help.
        frame.render_widget(
            editor
                .clone()
                .style(theme.body())
                .label_style(theme.caption())
                .help_style(theme.caption()),
            inner,
        );

        let is = |id: FocusId| self.editor.focus.is_focused(id);
        let focus_style = theme.focus_field();

        // Title.
        let r = editor.field_rect(EventEditorField::Title, inner);
        if !r.is_empty() {
            frame.render_widget(
                Input::new(&self.editor.title)
                    .focused(is(F_TITLE))
                    .placeholder("Event title")
                    .style(theme.body())
                    .focus_style(focus_style)
                    .placeholder_style(theme.caption()),
                r,
            );
        }
        // All-day switch.
        let r = editor.field_rect(EventEditorField::AllDay, inner);
        if !r.is_empty() {
            frame.render_widget(
                Switch::new()
                    .on(self.editor.all_day)
                    .focused(is(F_ALLDAY))
                    .on_label("yes")
                    .off_label("no")
                    .style(theme.body())
                    .focus_style(focus_style),
                r,
            );
        }
        // Start date.
        let r = editor.field_rect(EventEditorField::StartDate, inner);
        if !r.is_empty() {
            frame.render_widget(
                DatePicker::new(2026, 5, DAY_COUNT, WEEKDAY_OF_FIRST)
                    .selected(Some(self.editor.start_day))
                    .today(Some(TODAY))
                    .open(false)
                    .focused(is(F_START_DATE))
                    .style(theme.body())
                    .focus_style(focus_style)
                    .selected_style(theme.selection()),
                r,
            );
        }
        // Start time (hidden when all-day — field_rect is then zero).
        let r = editor.field_rect(EventEditorField::StartTime, inner);
        if !r.is_empty() {
            frame.render_widget(
                TimePicker::new(self.editor.start_min)
                    .open(false)
                    .focused(is(F_START_TIME))
                    .step_min(15)
                    .style(theme.body())
                    .focus_style(focus_style)
                    .selected_style(theme.selection()),
                r,
            );
        }
        // End date.
        let r = editor.field_rect(EventEditorField::EndDate, inner);
        if !r.is_empty() {
            frame.render_widget(
                DatePicker::new(2026, 5, DAY_COUNT, WEEKDAY_OF_FIRST)
                    .selected(Some(self.editor.end_day))
                    .today(Some(TODAY))
                    .open(false)
                    .focused(is(F_END_DATE))
                    .style(theme.body())
                    .focus_style(focus_style)
                    .selected_style(theme.selection()),
                r,
            );
        }
        // End time.
        let r = editor.field_rect(EventEditorField::EndTime, inner);
        if !r.is_empty() {
            frame.render_widget(
                TimePicker::new(self.editor.end_min)
                    .open(false)
                    .focused(is(F_END_TIME))
                    .step_min(15)
                    .style(theme.body())
                    .focus_style(focus_style)
                    .selected_style(theme.selection()),
                r,
            );
        }
        // Location.
        let r = editor.field_rect(EventEditorField::Location, inner);
        if !r.is_empty() {
            frame.render_widget(
                Input::new(&self.editor.location)
                    .focused(is(F_LOCATION))
                    .placeholder("Where")
                    .style(theme.body())
                    .focus_style(focus_style)
                    .placeholder_style(theme.caption()),
                r,
            );
        }
        // Calendar / category select.
        let r = editor.field_rect(EventEditorField::Calendar, inner);
        if !r.is_empty() {
            frame.render_widget(
                Select::new(CATEGORIES)
                    .selected(Some(self.editor.category))
                    .open(false)
                    .focused(is(F_CALENDAR))
                    .style(theme.body())
                    .focus_style(focus_style)
                    .highlight_style(theme.selection()),
                r,
            );
        }
        // Description (multi-line editor).
        let r = editor.field_rect(EventEditorField::Description, inner);
        if !r.is_empty() {
            frame.render_widget(
                Editor::new(&self.editor.description)
                    .focused(is(F_DESC))
                    .style(theme.body())
                    .focus_style(theme.border_focused())
                    .block(
                        Block::bordered()
                            .border_type(BorderType::Rounded)
                            .border_style(if is(F_DESC) {
                                theme.border_focused()
                            } else {
                                theme.border()
                            }),
                    ),
                r,
            );
        }
        // A small live read-out under the buttons (Start/End summary).
        let bar = editor.field_rect(EventEditorField::Save, inner);
        if !bar.is_empty() && bar.y + 2 < inner.bottom() {
            let summary = if self.editor.all_day {
                format!(
                    "{} {}–{} (all day) · {}",
                    MONTHS[4],
                    self.editor.start_day,
                    self.editor.end_day,
                    CATEGORIES[self.editor.category.min(CATEGORIES.len() - 1)]
                )
            } else {
                format!(
                    "{} {} {}–{} {} · {}",
                    MONTHS[4],
                    self.editor.start_day,
                    time_label(self.editor.start_min),
                    self.editor.end_day,
                    time_label(self.editor.end_min),
                    CATEGORIES[self.editor.category.min(CATEGORIES.len() - 1)]
                )
            };
            frame.render_widget(
                Paragraph::new(Line::from(summary.fg(theme.dim)))
                    .wrap(Wrap { trim: true })
                    .style(theme.caption()),
                Rect::new(inner.x, bar.y + 1, inner.width, 1),
            );
        }
    }
}

/// Map a focus id to its [`EventEditorField`] (the editor click hit-test).
fn focus_to_field(id: FocusId) -> EventEditorField {
    match id {
        i if i == F_TITLE => EventEditorField::Title,
        i if i == F_ALLDAY => EventEditorField::AllDay,
        i if i == F_START_DATE => EventEditorField::StartDate,
        i if i == F_START_TIME => EventEditorField::StartTime,
        i if i == F_END_DATE => EventEditorField::EndDate,
        i if i == F_END_TIME => EventEditorField::EndTime,
        i if i == F_LOCATION => EventEditorField::Location,
        i if i == F_CALENDAR => EventEditorField::Calendar,
        i if i == F_DESC => EventEditorField::Description,
        i if i == F_SAVE => EventEditorField::Save,
        _ => EventEditorField::Cancel,
    }
}

/// A tiny model-event helper so `period_event_ids` can ask "does this row
/// cover axis day `d`" without building a `CalendarEvent` (the screen owns the
/// day axis = day-of-month).
impl Ev {
    fn to_owned_covers(&self, d: i64) -> bool {
        d >= i64::from(self.day) && d <= i64::from(self.end_day.max(self.day))
    }
}
