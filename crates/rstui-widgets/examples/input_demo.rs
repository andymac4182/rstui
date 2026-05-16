//! A two-field sign-in form driven by the headless [`Harness`], the capstone
//! for [ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
//! Follow-up §2: it exercises the whole focus + text-edit stack end to end
//! with no terminal.
//!
//! It composes four pieces that each landed on their own:
//!
//! - [`TextEdit`] (rstui-core) — one per field, the caller-owned editing model.
//! - [`FocusRing`] (rstui-core) — the caller-owned focus order; `Tab` /
//!   `Shift+Tab` move it.
//! - [`Input`] (rstui-widgets) — the pure projection of a `&TextEdit` +
//!   `focused`, drawing the value and a reversed caret.
//! - [`App`] / [`Harness`] (rstui-runtime) — the Elm loop, scripted so the run
//!   is deterministic.
//!
//! The load-bearing point is **ADR 0004 §4**: the runtime never auto-routes
//! input to "the focused widget". The reducer reads its *own*
//! [`FocusRing::focused`] in `update` and dispatches the keystroke to that
//! field's `TextEdit`. Focus is plain model state; the widget only ever reads
//! it. Running over a [`TestBackend`](rstui_core::TestBackend) keeps it
//! TTY-free, so it doubles as a deterministic snapshot smoke test of the
//! Input + focus layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example input_demo
//! ```

use rstui_core::{
    Color, Constraint, FocusId, FocusRing, KeyCode, KeyEvent, Layout, Style, TextEdit,
};
use rstui_runtime::{App, Cmd, Event, Frame, Harness};
use rstui_widgets::{Block, Input};

// Stable focus ids the app mints, one per focusable field. Order in the ring
// (below) — not these values — is what `Tab` traverses.
const USERNAME: FocusId = FocusId::new(0);
const PASSWORD: FocusId = FocusId::new(1);

/// The form's whole state: two editable fields and which one the keyboard is
/// aimed at. Every bit of it is plain caller-owned model data.
struct SignIn {
    username: TextEdit,
    password: TextEdit,
    focus: FocusRing,
}

impl Default for SignIn {
    fn default() -> Self {
        Self {
            username: TextEdit::new(),
            password: TextEdit::new(),
            // The ring *is* the Tab order; it focuses its first id (USERNAME).
            focus: FocusRing::with_ids([USERNAME, PASSWORD]),
        }
    }
}

impl SignIn {
    /// ADR 0004 §4: the reducer routes input to the focused field by reading
    /// its **own** [`FocusRing`] — there is no runtime auto-routing and the
    /// widget never sees the event.
    fn focused_field(&mut self) -> Option<&mut TextEdit> {
        let id = self.focus.focused()?;
        if id == USERNAME {
            Some(&mut self.username)
        } else if id == PASSWORD {
            Some(&mut self.password)
        } else {
            None
        }
    }
}

enum Msg {
    FocusNext,
    FocusPrev,
    Type(char),
    Backspace,
    Quit,
}

impl App for SignIn {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        let key = event.as_key_press()?;
        match key.code {
            KeyCode::Tab => Some(Msg::FocusNext),
            KeyCode::BackTab => Some(Msg::FocusPrev),
            KeyCode::Backspace => Some(Msg::Backspace),
            KeyCode::Char(c) => Some(Msg::Type(c)),
            KeyCode::Esc | KeyCode::Enter => Some(Msg::Quit),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::FocusNext => {
                self.focus.focus_next();
            }
            Msg::FocusPrev => {
                self.focus.focus_prev();
            }
            Msg::Type(c) => {
                if let Some(field) = self.focused_field() {
                    field.insert_char(c);
                }
            }
            Msg::Backspace => {
                if let Some(field) = self.focused_field() {
                    field.delete_backward();
                }
            }
            Msg::Quit => return Cmd::quit(),
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let outer = Block::bordered().title("Sign in");
        let inner = outer.inner(frame.area());
        frame.render_widget(outer, frame.area());

        let [user_row, pass_row] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

        let focus_style = Style::new().fg(Color::Black).bg(Color::Cyan);
        let placeholder_style = Style::new().fg(Color::DarkGray);

        let [user_label, user_field] =
            Layout::horizontal([Constraint::Length(6), Constraint::Min(0)]).areas(user_row);
        frame.render_widget("User:", user_label);
        frame.render_widget(
            Input::new(&self.username)
                .focused(self.focus.is_focused(USERNAME))
                .placeholder("username")
                .focus_style(focus_style)
                .placeholder_style(placeholder_style),
            user_field,
        );

        let [pass_label, pass_field] =
            Layout::horizontal([Constraint::Length(6), Constraint::Min(0)]).areas(pass_row);
        frame.render_widget("Pass:", pass_label);
        frame.render_widget(
            Input::new(&self.password)
                .focused(self.focus.is_focused(PASSWORD))
                .placeholder("password")
                .focus_style(focus_style)
                .placeholder_style(placeholder_style),
            pass_field,
        );
    }
}

fn main() {
    let mut harness = Harness::new(SignIn::default(), 28, 4);
    println!("start (USERNAME focused, both empty -> placeholders):");
    println!("{}", harness.snapshot());

    // A scripted session. Each key flows on_event -> update -> view; the
    // reducer routes Type/Backspace to whichever field the ring says is
    // focused, so the *same* keys land in different fields after Tab.
    let tab = || Event::from(KeyEvent::from_code(KeyCode::Tab));
    let backtab = || Event::from(KeyEvent::from_code(KeyCode::BackTab));
    let backspace = || Event::from(KeyEvent::from_code(KeyCode::Backspace));
    let typing = |c: char| Event::from(KeyEvent::char(c));

    for c in "ada".chars() {
        harness.handle(typing(c)); // -> username (focused first)
    }
    println!("\nafter typing 'ada' into the focused username field:");
    println!("{}", harness.snapshot());

    harness.handle(tab()); // focus -> PASSWORD
    for c in "sec".chars() {
        harness.handle(typing(c)); // same keys, now -> password
    }
    harness.handle(backspace()); // password: "sec" -> "se"
    println!("\nafter Tab then typing 'sec' + Backspace into password:");
    println!("{}", harness.snapshot());

    harness.handle(backtab()); // focus -> USERNAME again
    harness.handle(typing('!')); // appends to username
    println!("\nafter Shift+Tab back to username and typing '!':");
    println!("{}", harness.snapshot());

    harness.handle(Event::from(KeyEvent::from_code(KeyCode::Esc))); // quit
    harness.handle(typing('x')); // ignored: the app already quit

    // The model is the single source of truth — assert directly on it.
    assert_eq!(harness.app().username.value(), "ada!");
    assert_eq!(harness.app().password.value(), "se");
    assert!(
        harness.app().focus.is_focused(USERNAME),
        "Shift+Tab returned focus to the username field"
    );
    assert!(!harness.is_running(), "Esc quit the app");
    println!(
        "\nfinal: username={:?} password={:?} (asserts passed)",
        "ada!", "se"
    );
}
