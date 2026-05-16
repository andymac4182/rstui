//! `rstui-runtime` — the Elm/Bubble Tea–style application runtime for the
//! rstui TUI framework.
//!
//! `rstui-core` is the dependency-free rendering substrate and deliberately
//! "knows nothing about the application event loop". This crate is that event
//! loop, expressed as a contract rather than a thread of control:
//!
//! - [`App`]: the state type you implement, with
//!   [`init`](App::init)/[`on_event`](App::on_event)/[`update`](App::update)/
//!   [`view`](App::view). State changes flow through `update` only.
//! - [`Cmd`]: the side effects an [`update`](App::update) schedules — quit,
//!   feed a follow-up message, perform deferred work — performed by the
//!   runtime, never by the app.
//! - [`Harness`]: a deterministic, terminal-free driver that runs the real
//!   loop over a [`TestBackend`](rstui_core::TestBackend) so whole apps are
//!   unit-testable with no TTY, threads, or clock.
//!
//! Keeping the loop a contract means the *same* `App`/`Cmd` code runs under
//! the headless [`Harness`] today and under a real terminal runtime later with
//! no changes — the harness is the reference semantics for that future driver.
//! [`Event`] and [`Frame`] are re-exported from `rstui-core` so an [`App`]
//! impl needs only this crate in scope.
//!
//! # Example
//!
//! ```
//! use rstui_runtime::{App, Cmd, Event, Frame, Harness};
//! use rstui_core::{KeyCode, Style};
//!
//! struct Hello {
//!     greeted: bool,
//! }
//!
//! enum Msg {
//!     Greet,
//!     Quit,
//! }
//!
//! impl App for Hello {
//!     type Message = Msg;
//!
//!     fn on_event(&self, event: Event) -> Option<Msg> {
//!         match event.as_key_press()?.code {
//!             KeyCode::Enter => Some(Msg::Greet),
//!             KeyCode::Esc => Some(Msg::Quit),
//!             _ => None,
//!         }
//!     }
//!
//!     fn update(&mut self, message: Msg) -> Cmd<Msg> {
//!         match message {
//!             Msg::Greet => {
//!                 self.greeted = true;
//!                 Cmd::none()
//!             }
//!             Msg::Quit => Cmd::quit(),
//!         }
//!     }
//!
//!     fn view(&self, frame: &mut Frame<'_>) {
//!         let text = if self.greeted { "hello rstui" } else { "press enter" };
//!         let pos = frame.area().position();
//!         frame.buffer_mut().set_str(pos, text, Style::new());
//!     }
//! }
//!
//! let mut harness = Harness::new(Hello { greeted: false }, 11, 1);
//! assert_eq!(harness.snapshot(), "press enter\n");
//! harness.handle(Event::from(rstui_core::KeyEvent::from_code(KeyCode::Enter)));
//! assert_eq!(harness.snapshot(), "hello rstui\n");
//! assert!(harness.is_running());
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod app;
pub mod cmd;
pub mod harness;

pub use app::App;
pub use cmd::Cmd;
pub use harness::{DEFAULT_COMMAND_BUDGET, Harness};

// Re-exported so an `App` implementor needs only `rstui_runtime` in scope for
// the trait's own signatures.
pub use rstui_core::{Event, Frame};
