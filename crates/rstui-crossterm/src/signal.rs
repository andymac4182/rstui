//! Terminal restore on a termination *signal* (`kill`, window close).
//!
//! [`TerminalGuard`](crate::TerminalGuard)'s [`Drop`] restores the terminal on
//! every path Rust runs destructors for — normal scope exit *and* panic
//! unwinding. A termination **signal** is neither: when the process receives
//! `SIGTERM`/`SIGHUP`/`SIGINT`/`SIGQUIT` and lets it run its default
//! disposition, the process is torn down *without* unwinding, so no destructor
//! runs and the guard never restores. A `kill <pid>`, a closed terminal window
//! (`SIGHUP`), or `Ctrl-C` while *not* in raw mode therefore leaves the user's
//! shell exactly as wedged as the unguarded case ADR 0001 set out to make
//! impossible: no echo, no line editing, stuck on the alternate screen with
//! mouse reporting on.
//!
//! [`install_signal_restore_hook`] closes that last gap. On the first of those
//! four signals it runs the **same** [`restore_terminal`]
//! the guard's [`Drop`] and the panic hook use, then exits with the
//! conventional `128 + signum` status so the parent shell still observes a
//! signal-shaped exit.
//!
//! # Why a dedicated thread, not a signal handler
//!
//! Almost nothing is safe to do in an async-signal handler — not allocating,
//! not locking, not the `disable_raw_mode`/escape-sequence writes
//! [`restore_terminal`] performs. Doing the
//! restore in a handler would be undefined behavior. `signal-hook`'s
//! [`Signals`] iterator solves this the correct
//! way: its real (tiny, async-signal-safe) handler only nudges a self-pipe, and
//! the signal is *delivered* to ordinary safe code blocked on that pipe in a
//! dedicated thread. The restore therefore runs in a normal thread context
//! where allocation, locking and I/O are all sound, and rstui needs **no**
//! `unsafe` (the workspace `forbid`s `unsafe_code`).
//!
//! # Raw mode changes which signals actually arrive here
//!
//! A full-screen rstui app runs in raw mode, where `ISIG` is off: `Ctrl-C`
//! arrives as a [`KeyCode::Char('c')`](rstui_core::event::KeyCode) key event,
//! **not** as `SIGINT` (and `Ctrl-\` likewise is a key, not `SIGQUIT`). So in
//! the case this stream primarily targets, the signals that actually reach this
//! hook are `SIGTERM` (`kill`) and `SIGHUP` (terminal window closed / parent
//! hangup). `SIGINT`/`SIGQUIT` are still registered because they *are* delivered
//! when the app opted out of raw mode (an inline tool) or before/after the
//! guard toggles it, and restoring an already-restored terminal is a harmless
//! no-op — the same "over-restoring is safe" rationale
//! [`restore_terminal`] documents. Handling all
//! four is therefore correct regardless of mode.
//!
//! # Where this sits in the testing layers
//!
//! Signal delivery is process-global and OS-mediated: installing a real handler
//! and raising a signal would tear down (or perturb) the *whole* test binary,
//! so it cannot be asserted deterministically — it is the L4c PTY/process-
//! global surface ADR 0001 anticipates, exactly like the panic hook's
//! non-deterministic seam documented in [`shell`](crate::shell). The
//! deterministic core is split out and unit-tested with no signals at all:
//! `signal_exit_status` (the pure `signum -> 128 + signum` mapping) and the
//! [`Once`]-based idempotency of the installer. The restore *content* is not
//! re-proven here — it is single-sourced through
//! [`restore_terminal`] and asserted
//! byte-for-byte in memory by the [`shell`](crate::shell) tests, so the
//! on-signal restore provably cannot drift from the normal or on-panic one.

use std::sync::Once;
use std::thread;

use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook::iterator::Signals;

use crate::shell::restore_terminal;

/// The four termination signals whose default disposition ends the process
/// *without* unwinding, so [`TerminalGuard`](crate::TerminalGuard)'s [`Drop`]
/// cannot restore the terminal:
///
/// - `SIGTERM` — the polite `kill <pid>` / service-manager stop.
/// - `SIGHUP` — controlling terminal closed (window shut, SSH dropped, parent
///   hangup); the canonical "shell left wedged" trigger.
/// - `SIGINT` — `Ctrl-C` *when not in raw mode* (in raw mode it is a key, not
///   this signal — see the module docs).
/// - `SIGQUIT` — `Ctrl-\` *when not in raw mode*; same raw-mode caveat.
const RESTORE_ON: [i32; 4] = [SIGTERM, SIGHUP, SIGINT, SIGQUIT];

/// The conventional process exit status for "terminated by signal `signum`":
/// `128 + signum` (the value a shell reports in `$?`, e.g. `143` for
/// `SIGTERM`).
///
/// After restoring the terminal we cannot re-raise the signal to keep its
/// *default* disposition (the handler is installed for the lifetime of the
/// `Signals` instance), so we mirror the shell's own convention for an
/// externally-terminated process instead. Pure and total, so it is unit-tested
/// directly — the one fully deterministic piece of this otherwise process-
/// global, OS-mediated surface.
fn signal_exit_status(signum: i32) -> i32 {
    128 + signum
}

/// Installs the termination-signal terminal-restore hook exactly once for the
/// process; idempotent and chaining-safe, so repeated
/// [`run_app`](crate::run_app)/[`run_app_with`](crate::run_app_with) calls
/// never spawn a second listener.
///
/// Spawns one dedicated, detached thread that blocks on a
/// [`signal_hook::iterator::Signals`] iterator over
/// `SIGTERM`/`SIGHUP`/`SIGINT`/`SIGQUIT`. On the **first** such signal it calls
/// [`restore_terminal`] — the single-sourced
/// restore the guard's [`Drop`] and the panic hook also use — and then
/// [`std::process::exit`]s with `signal_exit_status` so the parent shell
/// still sees a signal-shaped exit code. The [`Once`] guarantees the listener
/// is spawned at most once even if `run_app` is called repeatedly (e.g. in a
/// test harness driving multiple sessions in one process); a second call is a
/// cheap no-op.
///
/// Public so an application with its own entry point (not going through
/// [`run_app`](crate::run_app)) can opt in to the same protection; the
/// `run_app` family installs it for you, mirroring the panic-restore hook.
///
/// If the OS refuses to register the handlers (vanishingly rare — only on a
/// genuinely broken process state) the error is swallowed: failing to *add*
/// signal protection must not itself crash the app, exactly as
/// [`restore_terminal`]'s every step is
/// best-effort.
pub fn install_signal_restore_hook() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        // `Signals::new` is the safe blocking-iterator constructor: its own
        // async-signal handler only writes a self-pipe; the signal is
        // *delivered* to the safe code below, off the handler. A failure here
        // means the process cannot register handlers at all — there is nothing
        // useful to do but continue without the extra protection.
        let Ok(mut signals) = Signals::new(RESTORE_ON) else {
            return;
        };
        thread::Builder::new()
            .name("rstui-signal-restore".to_owned())
            .spawn(move || {
                // `forever()` blocks until a signal arrives; the first one is
                // all we need — we are about to terminate. Running in this
                // ordinary thread (not the handler) is what makes the
                // allocation/locking/I/O inside `restore_terminal` sound.
                if let Some(signum) = signals.forever().next() {
                    restore_terminal();
                    std::process::exit(signal_exit_status(signum));
                }
            })
            // Spawning the listener thread should never fail this early in a
            // process; if it somehow does, drop the extra protection rather
            // than abort — the guard still covers scope exit and panics.
            .ok();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exit status is the shell's `128 + signum` convention, exactly — the
    /// one fully deterministic piece of this process-global surface, so it is
    /// pinned directly. (Signal *delivery* is the L4c seam documented in the
    /// module docs and is not unit-testable without perturbing the whole test
    /// binary.)
    #[test]
    fn signal_exit_status_is_128_plus_signum() {
        assert_eq!(signal_exit_status(SIGTERM), 128 + SIGTERM);
        assert_eq!(signal_exit_status(SIGHUP), 128 + SIGHUP);
        assert_eq!(signal_exit_status(SIGINT), 128 + SIGINT);
        assert_eq!(signal_exit_status(SIGQUIT), 128 + SIGQUIT);
        // SIGTERM is 15 on every platform rstui targets, so the canonical
        // shell-visible code is 143 — a concrete anchor for the formula.
        assert_eq!(signal_exit_status(15), 143);
    }

    /// All four termination signals are registered, and the list is exactly
    /// those four (a regression guard if the set is ever edited): the signals
    /// whose default disposition skips unwinding and thus the guard's `Drop`.
    #[test]
    fn restores_on_exactly_the_four_termination_signals() {
        assert_eq!(RESTORE_ON, [SIGTERM, SIGHUP, SIGINT, SIGQUIT]);
        assert!(RESTORE_ON.contains(&SIGTERM));
        assert!(RESTORE_ON.contains(&SIGHUP));
        assert!(RESTORE_ON.contains(&SIGINT));
        assert!(RESTORE_ON.contains(&SIGQUIT));
    }

    /// Calling the installer more than once must not panic and must not spawn
    /// a second listener thread — the same idempotency guarantee
    /// `install_panic_restore_hook` relies on, so repeated `run_app` calls in
    /// one process are safe. The [`Once`] is what enforces "spawn at most
    /// once"; exercising the double call here proves it does not panic or
    /// double-register. Deliberately the *only* signal-side behavior touched
    /// in tests: it installs real process-global handlers, so it must run at
    /// most once across the whole binary (the `Once` also makes that true) and
    /// no test raises a signal, so the listener thread only ever blocks
    /// harmlessly.
    #[test]
    fn install_is_idempotent_and_panic_free() {
        install_signal_restore_hook();
        install_signal_restore_hook();
    }
}
