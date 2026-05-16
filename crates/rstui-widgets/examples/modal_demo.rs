//! A confirm-dialog over a list, driven by the headless [`Harness`] — the
//! capstone for [ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
//! Follow-up §3: it exercises the whole modal-focus model end to end with no
//! terminal.
//!
//! ADR 0004 §6 splits a modal in two and this demo composes both halves:
//!
//! - **State** — the [`FocusRing`] scope stack (rstui-core, iter 37):
//!   [`push_scope`](FocusRing::push_scope) on open traps focus to the modal's
//!   own ids and *captures* the prior focus; [`pop_scope`](FocusRing::pop_scope)
//!   on close *validate-restores* it; [`in_scope`](FocusRing::in_scope) is the
//!   declarative predicate the reducer gates background input on.
//! - **Visual** — the [`Modal`] widget (rstui-widgets, this slice): the
//!   centred, opaque, [`Block`]-framed dialog `view` renders *only while*
//!   `ring.in_scope()`, with [`Button`]s whose `focused` is read from the
//!   ring.
//!
//! The load-bearing points it proves, all TTY-free over a
//! [`TestBackend`](rstui_core::TestBackend):
//!
//! 1. **Declarative trapping** (ADR 0004 §6): the *same* `j` keystroke moves
//!    the background list when no modal is open and does **nothing** while one
//!    is — because `update` gates the background branch on
//!    `!ring.in_scope()`, not because the runtime sinks input.
//! 2. **Scope-constrained traversal**: `Tab` cycles only `OK`/`Cancel` while
//!    trapped and never escapes to the background list.
//! 3. **Validated capture/restore**: closing the modal returns focus to
//!    exactly the background control that had it.
//!
//! ```text
//! cargo run -p rstui-widgets --example modal_demo
//! ```

use rstui_core::{Color, Constraint, FocusId, FocusRing, KeyCode, KeyEvent, Layout, Style};
use rstui_runtime::{App, Cmd, Event, Frame, Harness};
use rstui_widgets::{Block, Button, List, Modal};

// Stable focus ids the app mints. The background has one focusable (the list);
// the modal owns two (its buttons). Order in a *scope* — not these values — is
// what `Tab` traverses while trapped.
const LIST: FocusId = FocusId::new(0);
const OK: FocusId = FocusId::new(10);
const CANCEL: FocusId = FocusId::new(11);

/// Everything is plain caller-owned model data: the list, which row is
/// selected, the focus ring (whose scope stack *is* the modal state), and the
/// last delete (so the demo can assert the OK path ran).
struct Confirm {
    items: Vec<String>,
    selected: usize,
    focus: FocusRing,
    deleted: Option<String>,
}

impl Default for Confirm {
    fn default() -> Self {
        Self {
            items: vec!["alpha".into(), "beta".into(), "gamma".into()],
            selected: 0,
            // The base ring is just the background list. A modal pushes its
            // own scope on top when it opens.
            focus: FocusRing::with_ids([LIST]),
            deleted: None,
        }
    }
}

enum Msg {
    Down,
    Up,
    Open,
    FocusNext,
    FocusPrev,
    Activate,
    Cancel,
    Quit,
}

impl App for Confirm {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        let key = event.as_key_press()?;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => Some(Msg::Down),
            KeyCode::Char('k') | KeyCode::Up => Some(Msg::Up),
            KeyCode::Char('o') => Some(Msg::Open),
            KeyCode::Tab => Some(Msg::FocusNext),
            KeyCode::BackTab => Some(Msg::FocusPrev),
            KeyCode::Enter => Some(Msg::Activate),
            KeyCode::Esc => Some(Msg::Cancel),
            KeyCode::Char('q') => Some(Msg::Quit),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            // ADR 0004 §6 declarative trapping: background navigation is
            // *predicated on the modal stack being empty*. The runtime still
            // delivers the event; the reducer simply ignores it while trapped.
            Msg::Down => {
                if !self.focus.in_scope() && self.selected + 1 < self.items.len() {
                    self.selected += 1;
                }
            }
            Msg::Up => {
                if !self.focus.in_scope() {
                    self.selected = self.selected.saturating_sub(1);
                }
            }
            Msg::Open => {
                if !self.focus.in_scope() && !self.items.is_empty() {
                    // Trap focus to the dialog's buttons; LIST is captured for
                    // validate-restore on close.
                    self.focus.push_scope([OK, CANCEL]);
                }
            }
            // Scope-constrained by FocusRing: while trapped this cycles only
            // OK/Cancel; on the single-focusable background it is a harmless
            // self-cycle.
            Msg::FocusNext => {
                self.focus.focus_next();
            }
            Msg::FocusPrev => {
                self.focus.focus_prev();
            }
            Msg::Activate => {
                if self.focus.in_scope() {
                    if self.focus.is_focused(OK) {
                        let removed = self.items.remove(self.selected);
                        self.selected = self.selected.min(self.items.len().saturating_sub(1));
                        self.deleted = Some(removed);
                    }
                    // Both OK and Cancel close the modal; pop validate-restores
                    // focus to the captured background control (LIST).
                    self.focus.pop_scope();
                }
            }
            Msg::Cancel => {
                if self.focus.in_scope() {
                    self.focus.pop_scope();
                } else {
                    return Cmd::quit();
                }
            }
            Msg::Quit => return Cmd::quit(),
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        // The background is always drawn; the modal floats over it.
        let list = List::new(self.items.iter().map(String::as_str))
            .block(Block::bordered().title("Items  (o: delete · j/k: move · q: quit)"))
            .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan))
            .highlight_symbol("> ")
            .selected(Some(self.selected));
        frame.render_widget(list, frame.area());

        // ADR 0004 §6: the widget never reads focus — the *app* decides a
        // modal is open (its scope stack is non-empty) and renders it.
        if self.focus.in_scope() {
            let prompt = self
                .items
                .get(self.selected)
                .map_or_else(|| "Delete?".to_string(), |it| format!("Delete \"{it}\"?"));

            let modal = Modal::new()
                .width(Constraint::Length(34))
                .height(Constraint::Length(7))
                .backdrop_style(Style::new().fg(Color::DarkGray))
                .block(Block::bordered().title("Confirm"));
            let inner = modal.inner(frame.area());
            frame.render_widget(modal, frame.area());

            let [prompt_row, _, button_row] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Fill(1),
                Constraint::Length(1),
            ])
            .areas(inner);
            frame.render_widget(prompt.as_str(), prompt_row);

            let [ok_col, cancel_col] =
                Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(button_row);
            let focus_style = Style::new().fg(Color::Black).bg(Color::Cyan);
            frame.render_widget(
                Button::new("OK")
                    .focused(self.focus.is_focused(OK))
                    .focus_style(focus_style),
                ok_col,
            );
            frame.render_widget(
                Button::new("Cancel")
                    .focused(self.focus.is_focused(CANCEL))
                    .focus_style(focus_style),
                cancel_col,
            );
        }
    }
}

fn main() {
    let mut harness = Harness::new(Confirm::default(), 44, 9);
    println!("start (background list, nothing trapped):");
    println!("{}", harness.snapshot());

    let key = |c: KeyCode| Event::from(KeyEvent::from_code(c));

    // 1. Background navigation works while no modal is open.
    harness.handle(key(KeyCode::Char('j'))); // alpha -> beta
    harness.handle(key(KeyCode::Char('j'))); // beta  -> gamma
    assert_eq!(
        harness.app().selected,
        2,
        "j moves the list when not trapped"
    );

    // 2. Open the modal: focus is captured (LIST) and trapped to OK/Cancel.
    harness.handle(key(KeyCode::Char('o')));
    assert!(harness.app().focus.in_scope(), "the modal scope is active");
    assert!(
        harness.app().focus.is_focused(OK),
        "the modal focuses OK first"
    );
    println!("\nmodal open over the list (focus trapped to OK/Cancel):");
    println!("{}", harness.snapshot());

    // 3. The SAME `j` keystroke now does nothing — declarative trapping, not a
    //    runtime sink. The selection stays put while the modal is up.
    harness.handle(key(KeyCode::Char('j')));
    assert_eq!(
        harness.app().selected,
        2,
        "j is gated on !in_scope while the modal is open"
    );

    // 4. Tab is scope-constrained: it cycles only OK <-> Cancel.
    harness.handle(key(KeyCode::Tab));
    assert!(harness.app().focus.is_focused(CANCEL), "Tab -> Cancel");
    harness.handle(key(KeyCode::Tab));
    assert!(
        harness.app().focus.is_focused(OK),
        "Tab wraps within the scope, never to the background list"
    );

    // 5. Cancel (Esc) closes the modal and validate-restores focus to LIST.
    harness.handle(key(KeyCode::Esc));
    assert!(!harness.app().focus.in_scope(), "Esc popped the scope");
    assert!(
        harness.app().focus.is_focused(LIST),
        "focus restored to the captured background control"
    );
    assert_eq!(harness.app().items.len(), 3, "Cancel deleted nothing");
    println!("\nafter Esc — modal closed, focus back on the list:");
    println!("{}", harness.snapshot());

    // 6. Re-open and confirm with OK: the selected item ("gamma") is deleted
    //    and focus is restored to the list again.
    harness.handle(key(KeyCode::Char('o')));
    harness.handle(key(KeyCode::Enter)); // OK is focused first
    assert_eq!(harness.app().deleted.as_deref(), Some("gamma"));
    assert_eq!(harness.app().items, ["alpha", "beta"]);
    assert!(!harness.app().focus.in_scope() && harness.app().focus.is_focused(LIST));
    println!("\nafter re-open + Enter on OK — \"gamma\" deleted:");
    println!("{}", harness.snapshot());

    harness.handle(key(KeyCode::Char('q'))); // quit (not trapped -> exits)
    harness.handle(key(KeyCode::Char('j'))); // ignored: the app already quit
    assert!(!harness.is_running(), "q quit the app");
    println!("\nfinal state asserted; all checks passed.");
}
