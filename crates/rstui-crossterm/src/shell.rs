//! The full-screen app shell: one call from an [`App`] to a live terminal.
//!
//! [`run_app`] is the ergonomic capstone the lower slices built toward. The
//! `run_app` example used to hand-compose four things —
//! [`CrosstermBackend`] over stdout, a
//! [`TerminalGuard`], a
//! [`CrosstermEventSource`], and the runtime loop — in every `main`. That
//! composition is always the same, so this module owns it once (and drives
//! the off-loop [`rstui_runtime::run_threaded`] so a slow command never
//! freezes a full-screen UI — see [`run_app`]):
//!
//! ```no_run
//! use rstui_crossterm::run_app;
//! # use rstui_runtime::{App, Cmd, Frame};
//! # #[derive(Default)] struct MyApp;
//! # impl App for MyApp {
//! #     type Message = ();
//! #     fn update(&mut self, _: ()) -> Cmd<()> { Cmd::quit() }
//! #     fn view(&self, _: &mut Frame<'_>) {}
//! # }
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     run_app(MyApp::default())?;
//!     Ok(())
//! }
//! ```
//!
//! This is the "ergonomic app run loop" rstui owes a full-screen framework: the
//! whole-terminal lifecycle (alternate screen, raw mode, mouse/paste/focus
//! capture) and panic-safe restore are handled, and the app the harness tests
//! drive headless runs live with no extra code.
//!
//! # Panic policy: the terminal *and* the panic message both survive
//!
//! [`TerminalGuard`]'s [`Drop`] already restores the
//! terminal while unwinding from a panic — that guarantee is proven in memory
//! in the [`lifecycle`](crate::lifecycle) tests. What it cannot do alone is
//! make the panic *message readable*: Rust's default panic hook prints
//! **before** unwinding starts, i.e. while the app is still on the alternate
//! screen in raw mode, so that text is wiped when the guard later leaves the
//! alternate screen.
//!
//! [`run_app`] closes that gap by installing a process-global panic hook that
//! restores the terminal *first*, then chains the previously-installed hook
//! (Rust's default, or a user reporter such as `human-panic` / `color-eyre`).
//! The message therefore lands on the user's restored normal screen. This is
//! the same ordering ratatui's `init()` uses (`restore()` then the prior
//! hook); chaining rather than replacing is what preserves a user's own panic
//! reporter.
//!
//! ## Why this lives behind a small non-deterministic seam
//!
//! Installing a process-global hook, writing to the real stdout, and toggling
//! raw mode are inherently process-wide and TTY-bound (ADR 0001 testing layer
//! L4c) — exactly the surface the rest of this crate already isolates. The
//! *content* of the restore, however, is single-sourced with the guard's
//! teardown via [`queue_leave_sequence`](crate::lifecycle) and asserted
//! byte-for-byte in memory, so the on-panic restore provably cannot drift from
//! the normal one. The hook is installed exactly once per process (a
//! [`Once`]), so repeated [`run_app`] calls never nest restore hooks.
//!
//! # The other restore gap: termination signals
//!
//! [`TerminalGuard`]'s [`Drop`] covers normal scope exit *and* panic
//! unwinding. It cannot cover a **termination signal** (`kill`, a closed
//! terminal window's `SIGHUP`, `Ctrl-C` when not in raw mode): the default
//! disposition ends the process *without* unwinding, so no destructor runs and
//! the terminal is left wedged. [`run_app`] also installs the
//! [`signal`](crate::signal) module's hook for that case — a dedicated
//! listener thread that runs the *same* [`restore_terminal`] then exits — so
//! every way a full-screen rstui process can end now restores the terminal.
//! See [`signal`](crate::signal) for why a thread (not an async-signal
//! handler) and the raw-mode caveat on which signals actually arrive.

use std::io::{self, Write};
use std::sync::Once;

use crossterm::terminal::disable_raw_mode;
use rstui_runtime::{App, RunError, run_threaded};

use crate::backend::CrosstermBackend;
use crate::event_source::CrosstermEventSource;
use crate::lifecycle::{LifecycleOptions, TerminalGuard, queue_leave_sequence};

/// The error [`run_app`] can fail with: a [`RunError`] whose render-backend and
/// input-source halves are both [`io::Error`] (crossterm's error type).
///
/// A named alias because the fully-spelled `RunError<io::Error, io::Error>`
/// otherwise leaks into every `main` signature; it implements
/// [`std::error::Error`] so `?` bubbles it into `Box<dyn Error>`/`anyhow`.
pub type CrosstermRunError = RunError<io::Error, io::Error>;

/// Restores the terminal to its normal state, best-effort.
///
/// Disables raw mode and emits the **full-screen preset's** leave sequence
/// (disable focus/paste/mouse reporting, then leave the alternate screen) to
/// stdout. It deliberately restores the *whole* default preset rather than a
/// subset: the panic hook cannot know which [`LifecycleOptions`] subset an app
/// chose, and disabling a mode that was never enabled is a harmless no-op on
/// every terminal — the same "over-restoring is safe" rationale
/// [`TerminalGuard`] documents for its half-constructed path.
///
/// Public so an app with its own teardown path (or a custom panic reporter)
/// can call it directly; [`run_app`] installs it as the panic hook for you.
/// Every step is best-effort: a failure during restore has nowhere useful to
/// go, and a partially-restored terminal is still better than none.
pub fn restore_terminal() {
    // Raw mode off first: it has the most side effects (input
    // canonicalization, signal generation), so it is the highest-priority
    // teardown step — ratatui documents this exact ordering.
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    emit_leave(&mut stdout);
}

/// Writes the full-preset leave escape sequence to `w` and flushes it.
///
/// Split out from [`restore_terminal`] purely so the (deterministic) escape
/// sequence is assertable against an in-memory writer with no TTY, while
/// `restore_terminal` itself binds it to the real stdout. The bytes are
/// produced by the *same* [`queue_leave_sequence`] the guard's [`Drop`] uses,
/// which is what guarantees the on-panic restore matches the normal one.
fn emit_leave<W: Write>(w: &mut W) {
    let _ = queue_leave_sequence(w, LifecycleOptions::default());
    let _ = w.flush();
}

/// Installs the panic-restore hook exactly once for the process.
///
/// Chains rather than replaces: the previously-installed hook (Rust's default,
/// or a user reporter) is captured and invoked *after* the terminal is
/// restored, so the panic message prints onto the user's normal screen and any
/// custom reporter still runs. The [`Once`] makes repeated [`run_app`] calls
/// idempotent — without it, each call would wrap the previous wrapper and
/// restore N times per panic.
fn install_panic_restore_hook() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            previous(info);
        }));
    });
}

/// Runs `app` full-screen on the real terminal with the default lifecycle
/// (alternate screen, raw mode, mouse + bracketed paste + focus reporting) and
/// panic-safe restore, returning the final app state.
///
/// One call replaces the four-seam hand-composition: it installs the
/// [panic-restore hook](self#panic-policy-the-terminal-and-the-panic-message-both-survive)
/// *and* the [termination-signal restore hook](self#the-other-restore-gap-termination-signals)
/// so the terminal is restored no matter how the process ends, then
/// builds a [`CrosstermBackend`] over stdout wrapped
/// in a [`TerminalGuard`], reads input through a
/// [`CrosstermEventSource`], and drives [`rstui_runtime::run_threaded`] with
/// the *same reducer* the headless [`Harness`](rstui_runtime::Harness) tests
/// exercise (see *Off-loop commands by default* below).
///
/// Use [`run_app_with`] to choose a different [`LifecycleOptions`] preset
/// (e.g. no mouse capture, or no alternate screen for an inline tool).
///
/// # Off-loop commands by default
///
/// `run_app` drives [`rstui_runtime::run_threaded`], **not** the inline
/// [`run`](rstui_runtime::run()): a full-screen application must stay responsive,
/// so a slow [`Cmd::perform`](rstui_runtime::Cmd::perform) (a file load, a
/// network call) or a real [`Cmd::tick`](rstui_runtime::Cmd::tick) delay runs
/// on its own thread instead of freezing input and rendering. This is the
/// deliberate decision ADR 0008's follow-up left open — for the *live*
/// full-screen entry point, off-loop is the right default; the headless
/// [`Harness`](rstui_runtime::Harness) and the bare [`run`](rstui_runtime::run())
/// stay inline so tests remain deterministic. The reducer (`update`) the
/// harness drives is *identical* either way (ADR 0008: only effect dispatch
/// differs), so an app is still unchanged between `cargo test` and production —
/// it merely stops blocking the UI on slow work when run live.
///
/// The cost is one bound: `A::Message` must be `Send + 'static` (a command
/// result crosses a thread boundary). Almost every message enum already is;
/// it is the same bound `Cmd::perform`'s closure has always carried.
///
/// # Errors
///
/// Returns [`CrosstermRunError::Backend`] if entering the terminal modes or a
/// later render fails, or [`CrosstermRunError::Input`] if reading the terminal
/// fails. On any return path the [`TerminalGuard`]'s
/// [`Drop`] has already restored the terminal.
pub fn run_app<A: App>(app: A) -> Result<A, CrosstermRunError>
where
    A::Message: Send + 'static,
{
    run_app_with(app, LifecycleOptions::default())
}

/// Like [`run_app`] but with a caller-chosen [`LifecycleOptions`] preset.
///
/// The panic-restore hook still restores the *full* preset regardless of
/// `options` (over-restoring is harmless; see [`restore_terminal`]), so an app
/// that opts out of, say, the alternate screen still cannot leave the terminal
/// wedged on a panic. Like [`run_app`] it drives the off-loop
/// [`rstui_runtime::run_threaded`] loop.
///
/// # Errors
///
/// Identical to [`run_app`].
pub fn run_app_with<A: App>(app: A, options: LifecycleOptions) -> Result<A, CrosstermRunError>
where
    A::Message: Send + 'static,
{
    install_panic_restore_hook();
    // Closes the last restore gap the guard's `Drop` cannot: a termination
    // signal (`kill`, closed window) ends the process without unwinding, so no
    // destructor runs. Installed here, beside the panic hook, because both are
    // process-global "restore before the normal path can" seams; idempotent, so
    // repeated `run_app` calls never stack listeners.
    crate::signal::install_signal_restore_hook();

    // Detect the real terminal's colour fidelity so a truecolor theme
    // degrades to 256/16/none instead of emitting escapes it cannot parse.
    let backend = CrosstermBackend::new(io::stdout())
        .with_color_level(CrosstermBackend::<io::Stdout>::detect_color_level());
    // One panic-safe ownership chain: Terminal -> TerminalGuard ->
    // CrosstermBackend -> Stdout. The guard enters the modes here and restores
    // them when `run` drops the terminal, on success or panic.
    let guard = TerminalGuard::with_options(backend, options).map_err(RunError::Backend)?;
    let mut events = CrosstermEventSource::new();

    // Off-loop commands: a slow load/timer must not freeze a full-screen UI.
    // Same reducer as the headless harness (ADR 0008) — only effect dispatch
    // differs — so the app is unchanged between `cargo test` and production.
    run_threaded(app, guard, &mut events)
}

/// Like [`run_app`], but drives the **async event loop**
/// ([`rstui_runtime::run_async`], ADR 0011) over a crossterm
/// [`EventStream`](crossterm::event::EventStream). Available only with the
/// `async` cargo feature; **must be awaited inside a tokio runtime**
/// (e.g. `#[tokio::main]`).
///
/// This is the lowest-latency full-screen entry point: input, command
/// results, and ticks are `tokio::select!`ed, so a resize, a scroll, or a
/// finished background command repaints *immediately* with none of the sync
/// loops' poll-interval, and the process is genuinely idle (zero wakeups)
/// when nothing is happening. The reducer is the *same* one the headless
/// `Harness` drives (ADR 0011) — only IO/effect multiplexing is async — so an
/// app is unchanged between `cargo test` and production.
///
/// The panic- and signal-restore hooks are installed exactly as in
/// [`run_app`]; the terminal is restored however the process ends.
///
/// # Errors
///
/// Identical to [`run_app`].
#[cfg(feature = "async")]
pub async fn run_app_async<A: App>(app: A) -> Result<A, CrosstermRunError>
where
    A::Message: Send + 'static,
{
    run_app_async_with(app, LifecycleOptions::default()).await
}

/// Like [`run_app_async`] but with a caller-chosen [`LifecycleOptions`]
/// preset (see [`run_app_with`]). Available only with the `async` cargo
/// feature; **must be awaited inside a tokio runtime**.
///
/// # Errors
///
/// Identical to [`run_app`].
#[cfg(feature = "async")]
pub async fn run_app_async_with<A: App>(
    app: A,
    options: LifecycleOptions,
) -> Result<A, CrosstermRunError>
where
    A::Message: Send + 'static,
{
    install_panic_restore_hook();
    crate::signal::install_signal_restore_hook();

    // Detect the real terminal's colour fidelity so a truecolor theme
    // degrades to 256/16/none instead of emitting escapes it cannot parse.
    let backend = CrosstermBackend::new(io::stdout())
        .with_color_level(CrosstermBackend::<io::Stdout>::detect_color_level());
    let guard = TerminalGuard::with_options(backend, options).map_err(RunError::Backend)?;
    let mut events = crate::event_source_async::CrosstermAsyncEventSource::new();

    // The `tokio::select!` loop: input/results/ticks, no poll interval. Same
    // reducer as the sync paths and the headless `Harness`.
    rstui_runtime::run_async(app, guard, &mut events).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `emit_leave` must produce *exactly* the guard's documented teardown
    /// escape sequence for the full preset — the property that makes the
    /// on-panic restore provably equal to the normal one. Asserted in memory
    /// with no TTY (the raw-mode toggle is the only PTY-bound part of
    /// `restore_terminal`, and is excluded here by construction).
    #[test]
    fn emit_leave_matches_the_full_preset_teardown_sequence() {
        use crossterm::event::{
            DisableBracketedPaste, DisableFocusChange, DisableMouseCapture,
            PopKeyboardEnhancementFlags,
        };
        use crossterm::queue;
        use crossterm::terminal::LeaveAlternateScreen;

        let mut got = Vec::new();
        emit_leave(&mut got);

        let mut expected = Vec::new();
        queue!(
            expected,
            // The full-screen preset now also negotiates the Kitty keyboard
            // protocol, so its single-sourced leave sequence pops it first
            // (release/repeat/disambiguation — see `lifecycle`).
            PopKeyboardEnhancementFlags,
            DisableFocusChange,
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
        )
        .unwrap();

        assert_eq!(got, expected);
    }

    /// `restore_terminal` is idempotent and side-effect-safe to call more than
    /// once (a panic mid-restore, a manual call plus the hook, repeated guards
    /// all converge). It writes a few bytes to the captured test stdout and
    /// must not panic; it deliberately installs no global hook, so it cannot
    /// contaminate other tests in this binary.
    #[test]
    fn restore_terminal_is_idempotent_and_panic_free() {
        restore_terminal();
        restore_terminal();
    }
}
