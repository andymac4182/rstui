//! Editable, focusable controls: a [`Form`] of [`Input`] + [`MaskedInput`],
//! a multi-line [`Editor`], a [`Checkbox`], a [`Radio`] group, a [`Switch`],
//! a [`Slider`], and a [`Button`] — all wired to a caller-owned
//! [`FocusRing`], exactly the ADR 0004 pattern.

use rstui_code::Editor; // ADR 0024: Editor moved to rstui-code
use rstui_core::{
    Constraint, FocusId, FocusRing, KeyCode, Layout, Line, Position, Rect, Style, TextArea,
    TextEdit, stylize::Stylize,
};
use rstui_runtime::Frame;
use rstui_widgets::{
    Block, BorderType, Button, Checkbox, Divider, Form, FormField, Input, MaskedInput, Paragraph,
    Radio, Slider, Switch, ToastLevel, Wrap,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

const NAME: FocusId = FocusId::new(0);
const SECRET: FocusId = FocusId::new(1);
const NOTES: FocusId = FocusId::new(2);
const SUBSCRIBE: FocusId = FocusId::new(3);
const PLAN: FocusId = FocusId::new(4);
const TOGGLE: FocusId = FocusId::new(5);
const VOLUME: FocusId = FocusId::new(6);
const SUBMIT: FocusId = FocusId::new(7);

/// The ring order, also the click hit-test order.
const ORDER: [FocusId; 8] = [NAME, SECRET, NOTES, SUBSCRIBE, PLAN, TOGGLE, VOLUME, SUBMIT];

/// The three subscription plans the radio group offers.
const PLANS: [&str; 3] = ["Free", "Pro", "Team"];

/// Every field's value plus which one the keyboard drives.
#[derive(Debug)]
pub(crate) struct State {
    name: TextEdit,
    secret: TextEdit,
    notes: TextArea,
    subscribe: bool,
    plan: usize,
    toggle: bool,
    volume: f64,
    focus: FocusRing,
}

impl State {
    /// An empty form, the name field focused.
    pub(crate) fn new() -> Self {
        Self {
            name: TextEdit::new(),
            secret: TextEdit::new(),
            notes: TextArea::from_value("multi-line notes —\ntype, Enter adds a line"),
            subscribe: true,
            plan: 1,
            toggle: false,
            volume: 65.0,
            focus: FocusRing::with_ids(ORDER),
        }
    }

    /// The focused single-line editor, if the focus is on one.
    fn focused_text(&mut self) -> Option<&mut TextEdit> {
        match self.focus.focused()? {
            id if id == NAME => Some(&mut self.name),
            id if id == SECRET => Some(&mut self.secret),
            _ => None,
        }
    }

    /// Route a key to the focused control.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        let focused = self.focus.focused().unwrap_or(NAME);
        match code {
            KeyCode::Up => {
                self.focus.focus_prev();
            }
            KeyCode::Down => {
                self.focus.focus_next();
            }
            KeyCode::Enter => {
                if focused == SUBMIT {
                    return ScreenOutcome::with_toast(
                        ToastLevel::Success,
                        format!(
                            "Submitted: {} · {} · vol {}",
                            if self.name.value().is_empty() {
                                "(no name)"
                            } else {
                                self.name.value()
                            },
                            PLANS[self.plan],
                            self.volume as i32
                        ),
                    );
                } else if focused == NOTES {
                    self.notes.insert_char('\n');
                } else {
                    self.focus.focus_next();
                }
            }
            KeyCode::Char(' ') => match focused {
                id if id == SUBSCRIBE => self.subscribe = !self.subscribe,
                id if id == TOGGLE => self.toggle = !self.toggle,
                id if id == PLAN => self.plan = (self.plan + 1) % PLANS.len(),
                id if id == NOTES => self.notes.insert_char(' '),
                id if id == SUBMIT => {
                    return ScreenOutcome::with_toast(ToastLevel::Info, "Use Enter to submit");
                }
                _ => {
                    if let Some(t) = self.focused_text() {
                        t.insert_char(' ');
                    }
                }
            },
            KeyCode::Left => match focused {
                id if id == VOLUME => self.volume = (self.volume - 5.0).max(0.0),
                id if id == PLAN => self.plan = self.plan.saturating_sub(1),
                _ => return ScreenOutcome::ignored(),
            },
            KeyCode::Right => match focused {
                id if id == VOLUME => self.volume = (self.volume + 5.0).min(100.0),
                id if id == PLAN => self.plan = (self.plan + 1).min(PLANS.len() - 1),
                _ => return ScreenOutcome::ignored(),
            },
            KeyCode::Backspace => {
                if focused == NOTES {
                    self.notes.delete_backward();
                } else if let Some(t) = self.focused_text() {
                    t.delete_backward();
                }
            }
            KeyCode::Char(c) => {
                if focused == NOTES {
                    self.notes.insert_char(c);
                } else if let Some(t) = self.focused_text() {
                    t.insert_char(c);
                }
            }
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// A click selects the field whose row it lands in.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        for (i, rect) in Self::field_rows(content).into_iter().enumerate() {
            if rect.contains(pos) {
                self.focus.focus(ORDER[i]);
                // A click on a toggle also flips it.
                match ORDER[i] {
                    id if id == SUBSCRIBE => self.subscribe = !self.subscribe,
                    id if id == TOGGLE => self.toggle = !self.toggle,
                    id if id == PLAN => self.plan = (self.plan + 1) % PLANS.len(),
                    _ => {}
                }
                return ScreenOutcome::consumed();
            }
        }
        ScreenOutcome::ignored()
    }

    /// A paste lands in the focused single-line editor.
    pub(crate) fn on_paste(&mut self, text: &str) {
        if let Some(t) = self.focused_text() {
            t.insert_str(text);
        }
    }

    /// Cut `sel` from the focused field (the multi-line notes or a
    /// single-line input).
    pub(crate) fn cut(&mut self, sel: &str) -> bool {
        if self.focus.focused() == Some(NOTES) {
            return crate::screens::cut_area(&mut self.notes, sel);
        }
        match self.focused_text() {
            Some(t) => crate::screens::cut_field(t, sel),
            None => false,
        }
    }

    /// The eight stacked field rects, the geometry the renderer and the
    /// click hit-test share.
    fn field_rows(area: Rect) -> [Rect; 8] {
        let [form, _div, notes, toggles, slider, button, _foot] = Layout::vertical([
            Constraint::Length(2), // Form: name + secret
            Constraint::Length(1), // divider
            Constraint::Length(4), // editor
            Constraint::Length(1), // checkbox / radio / switch
            Constraint::Length(1), // slider
            Constraint::Length(1), // button
            Constraint::Fill(1),   // read-out
        ])
        .areas(area);
        let form_rows = Layout::vertical([Constraint::Length(1); 2]).split(form);
        let [cb, rad, sw] = Layout::horizontal([
            Constraint::Length(16),
            Constraint::Fill(1),
            Constraint::Length(14),
        ])
        .areas(toggles);
        [
            form_rows[0],
            form_rows[1],
            notes,
            cb,
            rad,
            sw,
            slider,
            button,
        ]
    }

    /// Draw the form screen.
    pub(crate) fn view(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let rows = Self::field_rows(area);
        let focus_style = theme.focus_field();
        let is = |id: FocusId| self.focus.is_focused(id);

        // Name + Secret rendered through a real Form (labels + control rects).
        let form = Form::new()
            .field(FormField::new("Name", 1))
            .field(FormField::new("Secret", 1))
            .label_width(8)
            .style(theme.body())
            .label_style(theme.caption());
        let form_area = Rect::new(area.x, area.y, area.width, 2);
        let controls = form.layout(form_area);
        frame.render_widget(form, form_area);
        if let Some(r) = controls.first() {
            frame.render_widget(
                Input::new(&self.name)
                    .focused(is(NAME))
                    .placeholder("ada lovelace")
                    .style(theme.body())
                    .focus_style(focus_style)
                    .placeholder_style(theme.caption()),
                *r,
            );
        }
        if let Some(r) = controls.get(1) {
            frame.render_widget(
                MaskedInput::new(&self.secret)
                    .focused(is(SECRET))
                    .placeholder("hunter2")
                    .style(theme.body())
                    .focus_style(focus_style)
                    .placeholder_style(theme.caption()),
                *r,
            );
        }

        frame.render_widget(
            Divider::new()
                .label(Line::from(" notes ").style(theme.caption()))
                .style(theme.border()),
            Rect::new(area.x, area.y + 2, area.width, 1),
        );

        frame.render_widget(
            Editor::new(&self.notes)
                .focused(is(NOTES))
                .style(theme.body())
                .focus_style(theme.border_focused())
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .border_style(if is(NOTES) {
                            theme.border_focused()
                        } else {
                            theme.border()
                        }),
                ),
            rows[2],
        );

        frame.render_widget(
            Checkbox::new("Subscribe")
                .checked(self.subscribe)
                .focused(is(SUBSCRIBE))
                .style(theme.body())
                .focus_style(focus_style),
            rows[3],
        );
        let plan_cols = Layout::horizontal([Constraint::Fill(1); 3]).split(rows[4]);
        for (i, name) in PLANS.iter().enumerate() {
            frame.render_widget(
                Radio::new(*name)
                    .selected(self.plan == i)
                    .focused(is(PLAN) && self.plan == i)
                    .style(theme.body())
                    .focus_style(focus_style),
                plan_cols[i],
            );
        }
        frame.render_widget(
            Switch::new()
                .on(self.toggle)
                .focused(is(TOGGLE))
                .on_label("on")
                .off_label("off")
                .style(theme.body())
                .focus_style(focus_style),
            rows[5],
        );

        frame.render_widget(
            Slider::new()
                .range(0.0, 100.0)
                .value(self.volume)
                .label(Line::from("Volume").style(theme.caption()))
                .value_label(Line::from(format!("{}", self.volume as i32)).style(theme.body()))
                .focused(is(VOLUME))
                .style(theme.body())
                .thumb_style(Style::new().fg(theme.accent))
                .focus_style(focus_style),
            rows[6],
        );

        frame.render_widget(
            Button::new("  Submit  ")
                .focused(is(SUBMIT))
                .style(Style::new().fg(theme.text).bg(theme.surface))
                .focus_style(Style::new().fg(theme.base).bg(theme.accent)),
            Rect::new(rows[7].x, rows[7].y, 12, 1),
        );
        frame.render_widget(
            Line::from("↑↓ move · Space toggle · ←→ adjust · Enter submit".fg(theme.dim)),
            Rect::new(rows[7].x + 14, rows[7].y, area.width.saturating_sub(14), 1),
        );

        let readout = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(4),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .split(area)[6];
        frame.render_widget(
            Paragraph::new(format!(
                "live model → name={:?} secret_len={} subscribe={} plan={} switch={} volume={}",
                self.name.value(),
                self.secret.value().len(),
                self.subscribe,
                PLANS[self.plan],
                self.toggle,
                self.volume as i32,
            ))
            .style(theme.caption())
            .wrap(Wrap { trim: true }),
            readout,
        );
    }
}
