//! End-to-end text-input tests: every navigation/edit key and modifier an
//! input field uses, driven through the full `on_event` → `update` → `view`
//! pipeline of a real [`App`] — once granularly through the headless
//! [`Harness`] (asserting both the caller-owned [`TextEdit`]/[`TextArea`]
//! model *and* the rendered caret cell and focus colour), and once end to end
//! through the production [`run`] loop over a scripted source.
//!
//! The `text_edit`/`text_area` unit tests prove the *models* in isolation.
//! These prove the whole chain — a keystroke becomes an `Event`, routes by
//! `FocusRing`, mutates the model, and the [`Input`]/[`Editor`] widgets
//! project the result with the right caret position, REVERSED caret cell, and
//! focus fill. That is the "Home/End/arrows/Delete and Ctrl/Alt/Shift all
//! work on inputs" guarantee, asserted through the same loop production runs.

use std::cell::RefCell;
use std::convert::Infallible;
use std::rc::Rc;

use rstui_core::focus::{FocusId, FocusRing};
use rstui_core::{
    Backend, Cell, Color, Event, KeyCode, KeyEvent, KeyModifiers, Modifier, Position, Rect, Size,
    Style, TestBackend, TestEventSource, TextArea, TextEdit,
};
use rstui_runtime::{App, Cmd, Frame, Harness, run};
use rstui_widgets::{Editor, Input};

// --- input helpers ---------------------------------------------------------

/// A bare character press, the way the runtime delivers typed input.
fn ch(c: char) -> Event {
    Event::from(KeyEvent::char(c))
}

/// A bare non-character key press (Home, End, arrows, …).
fn code(code: KeyCode) -> Event {
    Event::from(KeyEvent::from_code(code))
}

/// A key press carrying modifiers (Ctrl+A, Alt+Backspace, Shift+End, …) —
/// the path the modifier matrix proves crossterm delivers.
fn modified(code: KeyCode, mods: KeyModifiers) -> Event {
    Event::from(KeyEvent::new(code, mods))
}

// --- a retained backend so a test can read the final `run` frame ----------

/// Shares its in-memory surface so a test can assert the last frame after
/// [`run`] has consumed and dropped the terminal that owned it (the exact
/// technique `runtime_e2e.rs` uses; an integration crate cannot reach `run`'s
/// private terminal otherwise).
#[derive(Clone)]
struct RetainedBackend(Rc<RefCell<TestBackend>>);

impl RetainedBackend {
    fn new(width: u16, height: u16) -> Self {
        Self(Rc::new(RefCell::new(TestBackend::new(width, height))))
    }
    fn handle(&self) -> Rc<RefCell<TestBackend>> {
        Rc::clone(&self.0)
    }
}

impl Backend for RetainedBackend {
    type Error = Infallible;
    fn draw<'a, I>(&mut self, cells: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = (Position, &'a Cell)>,
    {
        self.0.borrow_mut().draw(cells)
    }
    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().hide_cursor()
    }
    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().show_cursor()
    }
    fn cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.0.borrow_mut().cursor_position()
    }
    fn set_cursor_position(&mut self, p: Position) -> Result<(), Self::Error> {
        self.0.borrow_mut().set_cursor_position(p)
    }
    fn clear(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().clear()
    }
    fn size(&self) -> Result<Size, Self::Error> {
        self.0.borrow().size()
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().flush()
    }
}

// --- the fixture app -------------------------------------------------------

/// The focusable single-line field.
const NAME: FocusId = FocusId::new(0);
/// The focusable multi-line field.
const NOTES: FocusId = FocusId::new(1);

/// A real two-field form: a single-line [`TextEdit`] rendered by [`Input`] and
/// a multi-line [`TextArea`] rendered by [`Editor`], aimed by a [`FocusRing`].
/// It routes the *entire* navigation/edit key set — including Ctrl/Alt/Shift
/// shortcuts — into the focused model, exactly as a production form would.
struct Form {
    name: TextEdit,
    notes: TextArea,
    ring: FocusRing,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            name: TextEdit::new(),
            notes: TextArea::new(),
            ring: FocusRing::with_ids([NAME, NOTES]),
        }
    }
}

/// Intents the form maps input to. Cursor moves and edits are distinct so the
/// reducer stays the single mutation site.
enum Msg {
    FocusNext,
    FocusPrev,
    Insert(char),
    Newline,
    Backspace,
    DeleteForward,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Clear,
    Paste(String),
}

impl Form {
    /// The Input row (single-line field) — y = 0.
    const NAME_ROW: u16 = 0;

    fn name_focused(&self) -> bool {
        self.ring.is_focused(NAME)
    }
}

impl App for Form {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        if let Event::Paste(text) = &event {
            return Some(Msg::Paste(text.clone()));
        }
        let key = event.as_key_press()?;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        Some(match key.code {
            KeyCode::Tab => Msg::FocusNext,
            KeyCode::BackTab => Msg::FocusPrev,
            // Ctrl shortcuts (readline-style) — proves a modifier-bearing key
            // routes to the input, not just bare keys.
            KeyCode::Char('a') if ctrl => Msg::Home,
            KeyCode::Char('e') if ctrl => Msg::End,
            KeyCode::Char('u') if ctrl => Msg::Clear,
            KeyCode::Char(c) => Msg::Insert(c),
            KeyCode::Enter => Msg::Newline,
            KeyCode::Backspace => Msg::Backspace,
            KeyCode::Delete => Msg::DeleteForward,
            KeyCode::Left => Msg::Left,
            KeyCode::Right => Msg::Right,
            KeyCode::Up => Msg::Up,
            KeyCode::Down => Msg::Down,
            KeyCode::Home => Msg::Home,
            KeyCode::End => Msg::End,
            KeyCode::PageUp => Msg::PageUp,
            KeyCode::PageDown => Msg::PageDown,
            _ => return None,
        })
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        let on_name = self.name_focused();
        match message {
            Msg::FocusNext => {
                self.ring.focus_next();
            }
            Msg::FocusPrev => {
                self.ring.focus_prev();
            }
            Msg::Insert(c) if on_name => self.name.insert_char(c),
            Msg::Insert(c) => self.notes.insert_char(c),
            Msg::Newline if on_name => {
                self.ring.focus_next();
            }
            Msg::Newline => self.notes.insert_newline(),
            Msg::Backspace if on_name => {
                self.name.delete_backward();
            }
            Msg::Backspace => {
                self.notes.delete_backward();
            }
            Msg::DeleteForward if on_name => {
                self.name.delete_forward();
            }
            Msg::DeleteForward => {
                self.notes.delete_forward();
            }
            Msg::Left if on_name => {
                self.name.move_left();
            }
            Msg::Left => {
                self.notes.move_left();
            }
            Msg::Right if on_name => {
                self.name.move_right();
            }
            Msg::Right => {
                self.notes.move_right();
            }
            Msg::Up if !on_name => {
                self.notes.move_up();
            }
            Msg::Down if !on_name => {
                self.notes.move_down();
            }
            Msg::Up | Msg::Down => {}
            Msg::Home if on_name => self.name.move_home(),
            Msg::Home => self.notes.move_home(),
            Msg::End if on_name => self.name.move_end(),
            Msg::End => self.notes.move_end(),
            Msg::PageUp if !on_name => {
                self.notes.move_page_up(3);
            }
            Msg::PageDown if !on_name => {
                self.notes.move_page_down(3);
            }
            Msg::PageUp | Msg::PageDown => {}
            Msg::Clear if on_name => self.name.clear(),
            Msg::Clear => self.notes.clear(),
            Msg::Paste(t) if on_name => self.name.insert_str(&t),
            Msg::Paste(t) => self.notes.insert_str(&t),
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        // Row 0: the single-line Input with a distinct focus fill so the
        // colour is assertable; default cursor_style is REVERSED.
        let name_rect = Rect::new(0, Self::NAME_ROW, area.width, 1);
        frame.render_widget(
            Input::new(&self.name)
                .focused(self.name_focused())
                .focus_style(Style::new().bg(Color::Blue)),
            name_rect,
        );
        // Rows 2..height-1: the multi-line Editor.
        let notes_h = area.height.saturating_sub(3);
        if notes_h > 0 {
            frame.render_widget(
                Editor::new(&self.notes)
                    .focused(!self.name_focused())
                    .focus_style(Style::new().bg(Color::Green)),
                Rect::new(0, 2, area.width, notes_h),
            );
        }
        // Last row: a plain-text status line so the model is also assertable
        // via the glyph-only snapshot.
        let (nr, ncx) = self.notes.cursor();
        let status = format!(
            "N='{}'@{} M={}:{} f={}",
            self.name.value(),
            self.name.cursor(),
            nr,
            ncx,
            if self.name_focused() { "name" } else { "notes" },
        );
        frame.buffer_mut().set_str(
            Position::new(0, area.height.saturating_sub(1)),
            &status,
            Style::new(),
        );
    }
}

// --- helpers to read back the rendered caret/colour -----------------------

/// The cell at `(x, y)` of the harness's current frame.
fn cell_at(h: &Harness<Form>, x: u16, y: u16) -> Cell {
    h.backend()
        .buffer()
        .get(Position::new(x, y))
        .cloned()
        .expect("cell in bounds")
}

// --- 1. single-line Home / End / arrows -----------------------------------

#[test]
fn home_end_and_arrows_move_the_single_line_caret() {
    let mut h = Harness::new(Form::default(), 40, 8);
    for c in "hello".chars() {
        h.handle(ch(c));
    }
    assert_eq!(h.app().name.value(), "hello");
    assert_eq!(h.app().name.cursor(), 5);

    h.handle(code(KeyCode::Home));
    assert_eq!(h.app().name.cursor(), 0, "Home -> column 0");
    // The REVERSED caret is rendered at column 0 over the focused Input.
    assert!(
        cell_at(&h, 0, Form::NAME_ROW)
            .modifier
            .contains(Modifier::REVERSED),
        "caret cell is REVERSED at Home"
    );

    h.handle(code(KeyCode::End));
    assert_eq!(h.app().name.cursor(), 5, "End -> past last char");
    assert!(
        cell_at(&h, 5, Form::NAME_ROW)
            .modifier
            .contains(Modifier::REVERSED),
        "caret follows to the end cell"
    );

    h.handle(code(KeyCode::Left));
    h.handle(code(KeyCode::Left));
    assert_eq!(h.app().name.cursor(), 3, "two Lefts");
    h.handle(code(KeyCode::Right));
    assert_eq!(h.app().name.cursor(), 4, "Right");
    assert!(
        cell_at(&h, 4, Form::NAME_ROW)
            .modifier
            .contains(Modifier::REVERSED),
        "caret tracks arrow motion"
    );
}

// --- 2. Delete / Backspace edit at the caret ------------------------------

#[test]
fn delete_and_backspace_edit_at_the_single_line_caret() {
    let mut h = Harness::new(Form::default(), 40, 8);
    for c in "abcdef".chars() {
        h.handle(ch(c));
    }
    h.handle(code(KeyCode::Home));
    h.handle(code(KeyCode::Right));
    h.handle(code(KeyCode::Right)); // cursor at index 2 (before 'c')

    h.handle(code(KeyCode::Delete)); // removes 'c'
    assert_eq!(h.app().name.value(), "abdef");
    assert_eq!(h.app().name.cursor(), 2, "Delete keeps the cursor");

    h.handle(code(KeyCode::Backspace)); // removes 'b'
    assert_eq!(h.app().name.value(), "adef");
    assert_eq!(h.app().name.cursor(), 1, "Backspace moves the cursor left");

    // Backspace at column 0 is a no-op (totality).
    h.handle(code(KeyCode::Home));
    h.handle(code(KeyCode::Backspace));
    assert_eq!(h.app().name.value(), "adef");
    assert_eq!(h.app().name.cursor(), 0);
}

// --- 3. Ctrl modifier shortcuts route to the input ------------------------

#[test]
fn ctrl_modifier_shortcuts_reach_the_focused_input() {
    let mut h = Harness::new(Form::default(), 40, 8);
    for c in "modifier".chars() {
        h.handle(ch(c));
    }
    assert_eq!(h.app().name.cursor(), 8);

    // Ctrl+A -> Home, Ctrl+E -> End: a modifier-bearing key must route to the
    // input exactly like the bare navigation key.
    h.handle(modified(KeyCode::Char('a'), KeyModifiers::CONTROL));
    assert_eq!(h.app().name.cursor(), 0, "Ctrl+A behaves as Home");
    h.handle(modified(KeyCode::Char('e'), KeyModifiers::CONTROL));
    assert_eq!(h.app().name.cursor(), 8, "Ctrl+E behaves as End");

    // A plain 'a' (no modifier) still types — the modifier is what changes
    // the meaning, proving on_event reads key.modifiers, not just the code.
    h.handle(ch('a'));
    assert_eq!(h.app().name.value(), "modifiera");

    // Ctrl+U clears the field (readline kill).
    h.handle(modified(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert_eq!(h.app().name.value(), "");
    assert!(h.app().name.is_empty());
}

// --- 4. focus fill + REVERSED caret render only on the focused field ------

#[test]
fn focus_colour_and_caret_render_only_on_the_focused_field() {
    let mut h = Harness::new(Form::default(), 30, 8);
    h.handle(ch('h'));
    h.handle(ch('i'));

    // NAME is focused: the whole Input row carries the blue focus fill, and a
    // non-caret cell proves the fill (not just the caret) is applied.
    assert_eq!(
        cell_at(&h, 10, Form::NAME_ROW).bg,
        Color::Blue,
        "focused Input row has the focus fill"
    );
    assert!(
        cell_at(&h, 2, Form::NAME_ROW)
            .modifier
            .contains(Modifier::REVERSED),
        "REVERSED caret after 'hi' (column 2)"
    );

    // Move focus to NOTES: the Input row must lose both the focus fill and the
    // caret (pure projection of the new focus state).
    h.handle(code(KeyCode::Tab));
    assert!(!h.app().name_focused());
    assert_ne!(
        cell_at(&h, 10, Form::NAME_ROW).bg,
        Color::Blue,
        "unfocused Input row drops the focus fill"
    );
    assert!(
        !cell_at(&h, 2, Form::NAME_ROW)
            .modifier
            .contains(Modifier::REVERSED),
        "unfocused Input draws no caret"
    );
    // The Editor row now carries its green focus fill instead.
    assert_eq!(
        cell_at(&h, 5, 2).bg,
        Color::Green,
        "focused Editor row has the focus fill"
    );
}

// --- 5. multi-line Up/Down/Home/End/Page navigation -----------------------

#[test]
fn multiline_editor_vertical_and_page_navigation() {
    let mut h = Harness::new(Form::default(), 40, 12);
    h.handle(code(KeyCode::Tab)); // focus NOTES
    assert!(!h.app().name_focused());

    // Build a few lines: "one\ntwo\nthree".
    for c in "one".chars() {
        h.handle(ch(c));
    }
    h.handle(code(KeyCode::Enter));
    for c in "two".chars() {
        h.handle(ch(c));
    }
    h.handle(code(KeyCode::Enter));
    for c in "three".chars() {
        h.handle(ch(c));
    }
    assert_eq!(h.app().notes.cursor(), (2, 5), "caret at end of line 3");
    assert_eq!(h.app().notes.row_count(), 3);

    h.handle(code(KeyCode::Up));
    assert_eq!(h.app().notes.cursor().0, 1, "Up -> previous row");
    h.handle(code(KeyCode::Home));
    assert_eq!(
        h.app().notes.cursor(),
        (1, 0),
        "Home -> column 0 of the row"
    );
    h.handle(code(KeyCode::End));
    assert_eq!(h.app().notes.cursor(), (1, 3), "End -> end of 'two'");
    h.handle(code(KeyCode::Down));
    assert_eq!(h.app().notes.cursor().0, 2, "Down -> next row");

    // Page motion is clamped and total (3-row page over a 3-line doc).
    h.handle(code(KeyCode::PageUp));
    assert_eq!(
        h.app().notes.cursor().0,
        0,
        "PageUp clamps to the first row"
    );
    h.handle(code(KeyCode::PageDown));
    assert_eq!(
        h.app().notes.cursor().0,
        2,
        "PageDown clamps to the last row"
    );

    // The Editor draws a REVERSED caret somewhere in its region (rows 2..).
    let caret_seen =
        (2..11).any(|y| (0..40).any(|x| cell_at(&h, x, y).modifier.contains(Modifier::REVERSED)));
    assert!(caret_seen, "focused Editor renders a REVERSED caret");
}

// --- 6. paste inserts at the caret ----------------------------------------

#[test]
fn paste_inserts_at_the_caret_in_both_fields() {
    let mut h = Harness::new(Form::default(), 40, 10);
    for c in "ad".chars() {
        h.handle(ch(c));
    }
    h.handle(code(KeyCode::Left)); // cursor between 'a' and 'd'
    h.handle(Event::Paste("BC".to_string()));
    assert_eq!(h.app().name.value(), "aBCd", "paste lands at the caret");
    assert_eq!(h.app().name.cursor(), 3);

    // Multi-line paste into the Editor creates rows.
    h.handle(code(KeyCode::Tab));
    h.handle(Event::Paste("x\ny\nz".to_string()));
    assert_eq!(h.app().notes.row_count(), 3);
    assert_eq!(h.app().notes.cursor(), (2, 1), "caret after the last char");
}

// --- 7. the whole sequence through the real `run` loop --------------------

#[test]
fn full_key_sequence_through_the_production_run_loop() {
    // Drive the *production* loop (not the Harness) over a scripted source:
    // type, navigate with Home/End/arrows, edit with Backspace/Delete, use a
    // Ctrl shortcut, paste, switch fields with Tab — then the source drains
    // (no Cmd::quit) and we assert the final model AND the final frame the
    // loop actually presented.
    let backend = RetainedBackend::new(40, 6);
    let surface = backend.handle();
    let mut input = TestEventSource::with_events([
        ch('h'),
        ch('x'),
        ch('o'), // "hxo"
        code(KeyCode::Left),
        code(KeyCode::Left),   // caret before 'x'
        code(KeyCode::Delete), // -> "ho", caret at 1
        ch('e'),
        ch('l'),
        ch('l'), // "hello" assembled: h e l l o
        code(KeyCode::End),
        modified(KeyCode::Char('a'), KeyModifiers::CONTROL), // Ctrl+A == Home
        code(KeyCode::End),
        Event::Paste("!!".to_string()), // append "!!"
        code(KeyCode::Tab),             // focus NOTES
        ch('n'),
        ch('o'),
        ch('w'),
    ]);

    let app = run(Form::default(), backend, &mut input).unwrap();

    assert_eq!(app.name.value(), "hello!!", "single-line assembled value");
    assert!(!app.name_focused(), "Tab moved focus to the notes field");
    assert_eq!(
        app.notes.to_string(),
        "now",
        "typing routed to the focused notes"
    );
    assert!(input.is_empty(), "the source drained (end-of-input stop)");

    // The status line the production loop last presented reflects the model.
    let frame = format!("{}", surface.borrow());
    assert!(
        frame.contains("N='hello!!'@7"),
        "final presented frame shows the single-line value + caret:\n{frame}"
    );
    assert!(
        frame.contains("f=notes"),
        "final frame shows focus moved to notes:\n{frame}"
    );
}
