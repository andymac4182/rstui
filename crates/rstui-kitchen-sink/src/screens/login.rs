//! A sign-in experience: a centred card ([`Align`] + [`Block`]) with an
//! [`Avatar`] mark, an [`Input`], a [`MaskedInput`], a "remember me"
//! [`Switch`], a [`Button`], and a live-validation [`Alert`]. `↑/↓` move
//! between fields, `Enter` submits. Try `ada` / `rust`.

use rstui_core::{Alignment, Constraint, KeyCode, Layout, Line, Position, Rect, Style, TextEdit};
use rstui_runtime::Frame;
use rstui_widgets::{
    Alert, AlertLevel, Align, Avatar, Block, BorderType, Button, Input, MaskedInput, Switch,
    VerticalAlignment,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// Validation / submit result.
#[derive(Debug, Clone)]
enum Status {
    Idle,
    Error(String),
    Success(String),
}

/// The four focus stops on the form.
const STOPS: usize = 4;

/// The sign-in form's caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    user: TextEdit,
    pass: TextEdit,
    remember: bool,
    focus: usize,
    status: Status,
}

impl State {
    /// An empty form, the username focused.
    pub(crate) fn new() -> Self {
        Self {
            user: TextEdit::new(),
            pass: TextEdit::new(),
            remember: true,
            focus: 0,
            status: Status::Idle,
        }
    }

    fn submit(&mut self) -> ScreenOutcome {
        if self.user.value().is_empty() || self.pass.value().is_empty() {
            self.status = Status::Error("Enter both a username and a password.".into());
            return ScreenOutcome::consumed();
        }
        if self.user.value() == "ada" && self.pass.value() == "rust" {
            self.status = Status::Success(format!("Welcome back, {}.", self.user.value()));
            return ScreenOutcome::with_toast(crate::screens::ToastLevel::Success, "Signed in");
        }
        self.status = Status::Error("Invalid credentials — try ada / rust.".into());
        ScreenOutcome::with_toast(crate::screens::ToastLevel::Error, "Sign-in failed")
    }

    fn field(&mut self) -> Option<&mut TextEdit> {
        match self.focus {
            0 => Some(&mut self.user),
            1 => Some(&mut self.pass),
            _ => None,
        }
    }

    /// `↑/↓` move focus, typing edits the text fields, `Space` toggles
    /// remember, `Enter` submits.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Up => self.focus = (self.focus + STOPS - 1) % STOPS,
            KeyCode::Down => self.focus = (self.focus + 1) % STOPS,
            KeyCode::Enter => {
                return if self.focus == 3 || self.focus == 1 {
                    self.submit()
                } else {
                    self.focus = (self.focus + 1) % STOPS;
                    ScreenOutcome::consumed()
                };
            }
            KeyCode::Char(' ') if self.focus == 2 => self.remember = !self.remember,
            KeyCode::Char(' ') if self.focus == 3 => return self.submit(),
            KeyCode::Left => {
                if let Some(f) = self.field() {
                    f.move_left();
                }
            }
            KeyCode::Right => {
                if let Some(f) = self.field() {
                    f.move_right();
                }
            }
            KeyCode::Backspace => {
                if let Some(f) = self.field() {
                    f.delete_backward();
                }
            }
            KeyCode::Char(c) => {
                if let Some(f) = self.field() {
                    f.insert_char(c);
                }
            }
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Pasted text drops into the focused text field.
    pub(crate) fn on_paste(&mut self, text: &str) {
        let one = text.replace('\n', " ");
        if let Some(f) = self.field() {
            f.insert_str(&one);
        }
    }

    /// Click a field to focus it, the remember row to toggle, or the
    /// button to submit. Geometry mirrors [`view`] (same Align + Block).
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let panel = Align::new()
            .horizontal(Alignment::Center)
            .vertical(VerticalAlignment::Center)
            .width(Constraint::Length(46))
            .height(Constraint::Length(16))
            .inner(content);
        let inner = Block::bordered()
            .border_type(BorderType::Double)
            .inner(panel);
        let [_logo, _g0, user_r, pass_r, rem_r, btn_r, _msg] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(inner);
        if user_r.contains(pos) {
            self.focus = 0;
            return ScreenOutcome::consumed();
        }
        if pass_r.contains(pos) {
            self.focus = 1;
            return ScreenOutcome::consumed();
        }
        if rem_r.contains(pos) {
            self.focus = 2;
            self.remember = !self.remember;
            return ScreenOutcome::consumed();
        }
        if btn_r.contains(pos) {
            self.focus = 3;
            return self.submit();
        }
        ScreenOutcome::ignored()
    }

    /// Draw the centred sign-in card.
    pub(crate) fn view(&self, theme: &Theme, _tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let card = Align::new()
            .horizontal(rstui_core::Alignment::Center)
            .vertical(VerticalAlignment::Center)
            .width(Constraint::Length(46))
            .height(Constraint::Length(16));
        let panel = card.inner(area);
        let block = Block::bordered()
            .border_type(BorderType::Double)
            .title(Line::from(" Welcome to rstui ").style(theme.heading()))
            .border_style(theme.border_focused())
            .style(theme.body());
        let inner = block.inner(panel);
        frame.render_widget(block, panel);

        let [logo, _g0, user_r, pass_r, rem_r, btn_r, msg_r] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(inner);

        frame.render_widget(
            Avatar::new("RS").style(Style::new().fg(theme.base).bg(theme.accent)),
            Rect::new(logo.x + (logo.width.saturating_sub(5)) / 2, logo.y, 5, 3),
        );

        let focus_style = theme.focus_field();
        let lbl = |w: u16| Constraint::Length(w);
        let mk = |r: Rect| Layout::horizontal([lbl(10), Constraint::Fill(1)]).areas(r);

        let [lu, fu] = mk(user_r);
        frame.render_widget(Line::from("User").style(theme.caption()), lu);
        frame.render_widget(
            Input::new(&self.user)
                .focused(self.focus == 0)
                .placeholder("ada")
                .style(theme.body())
                .focus_style(focus_style)
                .placeholder_style(theme.caption()),
            fu,
        );
        let [lp, fp] = mk(pass_r);
        frame.render_widget(Line::from("Password").style(theme.caption()), lp);
        frame.render_widget(
            MaskedInput::new(&self.pass)
                .focused(self.focus == 1)
                .placeholder("rust")
                .style(theme.body())
                .focus_style(focus_style)
                .placeholder_style(theme.caption()),
            fp,
        );
        let [lr, fr] = mk(rem_r);
        frame.render_widget(Line::from("Remember").style(theme.caption()), lr);
        frame.render_widget(
            Switch::new()
                .on(self.remember)
                .focused(self.focus == 2)
                .on_label("yes")
                .off_label("no")
                .style(theme.body())
                .focus_style(focus_style),
            fr,
        );
        frame.render_widget(
            Button::new("  Sign in  ")
                .focused(self.focus == 3)
                .style(Style::new().fg(theme.text).bg(theme.surface))
                .focus_style(Style::new().fg(theme.base).bg(theme.accent)),
            Rect::new(btn_r.x + 10, btn_r.y, 13, 1),
        );

        match &self.status {
            Status::Idle => frame.render_widget(
                Line::from("↑↓ move · Enter submit · try ada / rust").style(theme.caption()),
                msg_r,
            ),
            Status::Error(m) => frame.render_widget(
                Alert::new(AlertLevel::Error, "Sign-in failed")
                    .body(m.clone())
                    .style(theme.body())
                    .error_style(
                        Style::new()
                            .fg(theme.err)
                            .add_modifier(rstui_core::Modifier::BOLD),
                    ),
                msg_r,
            ),
            Status::Success(m) => frame.render_widget(
                Alert::new(AlertLevel::Success, "Signed in")
                    .body(m.clone())
                    .style(theme.body())
                    .success_style(
                        Style::new()
                            .fg(theme.ok)
                            .add_modifier(rstui_core::Modifier::BOLD),
                    ),
                msg_r,
            ),
        }
    }
}
