//! A real-terminal text-input fixture for the VHS end-to-end gate.
//!
//! `vhs/e2e/text-input.tape` runs this unmodified binary on an actual
//! terminal (ttyd) and drives it with real keystrokes — Home/End/arrows/
//! Delete/Backspace and the `Ctrl+A`/`Ctrl+E` shortcuts — then asserts the
//! final `STATE val=… cur=… len=…` line in `text-input.expect`. That proves
//! the *whole* pipeline end to end on a real terminal: crossterm key +
//! modifier translation → the `run` loop → the caller-owned [`TextEdit`]
//! model → the rendered frame. The deterministic Harness tests
//! (`rstui-smoke`) cover the model and widget projection; this proves the
//! real terminal delivers the keys those tests assume.
//!
//! Rendered with `rstui-core` only (no widget dependency) so the backend
//! crate stays minimal — the key→model→frame path is what is under test.
//!
//! ```text
//! cargo run -p rstui-crossterm --example text_input_e2e
//! ```
//!
//! `Esc` quits. It needs a TTY, so CI builds it (type-check) but the VHS
//! gate is the thing that runs it.

use std::error::Error;

use rstui_core::{Color, KeyCode, KeyModifiers, Position, Style, TextEdit};
use rstui_crossterm::run_app;
use rstui_runtime::{App, Cmd, Event, Frame};

/// A single-line field driven entirely by real keystrokes.
#[derive(Default)]
struct InputFixture {
    field: TextEdit,
}

/// The intents the fixture maps real terminal input to.
enum Msg {
    Insert(char),
    Backspace,
    DeleteForward,
    Left,
    Right,
    Home,
    End,
    Clear,
    Paste(String),
    Quit,
}

impl App for InputFixture {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        if let Event::Paste(text) = &event {
            return Some(Msg::Paste(text.clone()));
        }
        let key = event.as_key_press()?;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        Some(match key.code {
            KeyCode::Esc => Msg::Quit,
            // Readline-style Ctrl shortcuts: proves a *modified* key reaches
            // the input over a real terminal, not just bare keys.
            KeyCode::Char('a') if ctrl => Msg::Home,
            KeyCode::Char('e') if ctrl => Msg::End,
            KeyCode::Char('u') if ctrl => Msg::Clear,
            KeyCode::Char(c) => Msg::Insert(c),
            KeyCode::Backspace => Msg::Backspace,
            KeyCode::Delete => Msg::DeleteForward,
            KeyCode::Left => Msg::Left,
            KeyCode::Right => Msg::Right,
            KeyCode::Home => Msg::Home,
            KeyCode::End => Msg::End,
            _ => return None,
        })
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Insert(c) => self.field.insert_char(c),
            Msg::Backspace => {
                self.field.delete_backward();
            }
            Msg::DeleteForward => {
                self.field.delete_forward();
            }
            Msg::Left => {
                self.field.move_left();
            }
            Msg::Right => {
                self.field.move_right();
            }
            Msg::Home => self.field.move_home(),
            Msg::End => self.field.move_end(),
            Msg::Clear => self.field.clear(),
            Msg::Paste(t) => self.field.insert_str(&t),
            Msg::Quit => return Cmd::quit(),
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let buf = frame.buffer_mut();
        buf.set_str(
            Position::new(0, 0),
            "rstui text-input e2e — Esc to quit",
            Style::new().fg(Color::Cyan),
        );
        // The field with a visible caret marker '‹' inserted at the cursor.
        let v = self.field.value();
        let cur = self.field.cursor();
        let mut shown = String::new();
        for (i, ch) in v.chars().enumerate() {
            if i == cur {
                shown.push('‹');
            }
            shown.push(ch);
        }
        if cur >= v.chars().count() {
            shown.push('‹');
        }
        buf.set_str(
            Position::new(0, 2),
            &format!("field: {shown}"),
            Style::new().fg(Color::Yellow),
        );
        // The deterministic marker line the .expect file asserts. Kept ASCII
        // and exact so the VHS text capture matches verbatim.
        buf.set_str(
            Position::new(0, 4),
            &format!(
                "STATE val={} cur={} len={}",
                self.field.value(),
                self.field.cursor(),
                self.field.len()
            ),
            Style::new(),
        );
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    run_app(InputFixture::default())?;
    Ok(())
}
