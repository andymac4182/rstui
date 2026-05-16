//! The real `std::process`-backed [`ProcessRunner`]:
//! `env_clear` + allowlist, piped stdio, cooperative-then-forced shutdown
//! (ADR 0007 §2/§6/§7).
//!
//! This is the production sibling of the in-memory
//! [`FakeProcessRunner`](crate::process::FakeProcessRunner): same
//! [`ProcessRunner`] /
//! [`PluginProcess`] contract, but talking to a
//! genuine child process through [`std::process::Command`]. Nothing here is
//! `unsafe` — it is pure safe `std`, the workspace's hard constraint (ADR 0007
//! drivers 4, §7).
//!
//! ## `env_clear` + allowlist is the §2 enforcement point
//!
//! [`StdProcessRunner::spawn`] calls [`Command::env_clear`] **and then**
//! [`Command::envs`] with exactly the pairs the host put in
//! [`PluginSpawnSpec::env`]. That single ordered pair of calls *is* ADR 0007
//! §2's `env_clear()`-then-re-add-only-the-allowlist mechanism (secure-exec's
//! `filterEnv`): the child sees **only** the declared keys and **nothing
//! ambient** — not the operator's `HOME`, `PATH`, `AWS_*`, terminal, or any
//! other inherited variable. The host is responsible for having resolved the
//! manifest's `env` capability grants into that allowlist before it builds the
//! spec; this runner just enforces "those keys and no others" at the OS
//! boundary. Both `env_clear` and `envs` are ordinary safe `std`.
//!
//! ## Cooperative-then-forced shutdown, and the deliberate absence of SIGTERM
//!
//! Shutdown is the two-phase model of ADR 0007 §6, expressed with the only
//! tools safe `std` exposes:
//!
//! 1. **Cooperative** — [`StdPluginProcess::request_shutdown`] *drops the
//!    child's stdin pipe*. The plugin's read loop then observes EOF on its
//!    stdin and is expected to finish and exit on its own. That is the entire
//!    cooperative signal: closing the pipe, nothing more.
//! 2. **Forced** — [`StdPluginProcess::kill`] calls [`Child::kill`], which on
//!    Unix is `SIGKILL`. `SIGKILL` is the *only* hard stop reachable without
//!    `unsafe` libc, so it is the only forced primitive this runner has.
//!
//! There is **deliberately no graceful `SIGTERM`, no grace-signalling, and no
//! finer escalation** between those two phases. Sending `SIGTERM` (or any
//! specific signal) to a `std::process::Child` requires `libc::kill`, which is
//! `unsafe` and the workspace *forbids* `unsafe` crate-wide (ADR 0007 §7, and
//! the "Made hard / accepted costs" consequence). The grace period itself is
//! **not implemented here**: the host orchestrates it by calling
//! `request_shutdown`, then polling [`StdPluginProcess::try_wait`] against its
//! injected [`Clock`](crate::clock::Clock) for a bounded window, and only
//! calling [`kill`](StdPluginProcess::kill) if that window elapses. This
//! runner provides the close-stdin and the `SIGKILL`; the *timing* between
//! them is the host's, measured by a fake-able clock, never a real sleep here.
//!
//! ## This runner performs NO permission checks
//!
//! [`StdProcessRunner`] is the **dumb spawner**. It does not look at a
//! manifest, consult a [`PermissionPolicy`](crate::permission::PermissionPolicy),
//! canonicalise a path, or decide whether the plugin is allowed to run.
//! Whatever [`PluginSpawnSpec`] it is handed, it spawns verbatim. All
//! capability mediation — the deny-by-default authority model of ADR 0007
//! §1/§2/§3 — lives in the host *around* this seam, never inside it. Keeping
//! the spawner authority-free is what makes the security boundary auditable in
//! one place.
//!
//! [`Command::env_clear`]: std::process::Command::env_clear
//! [`Command::envs`]: std::process::Command::envs
//! [`Child::kill`]: std::process::Child::kill

use std::io::{self, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

use crate::process::{ExitOutcome, PluginProcess, PluginSpawnSpec, ProcessRunner};

// ---------------------------------------------------------------------------
// StdProcessRunner
// ---------------------------------------------------------------------------

/// The production [`ProcessRunner`]: spawns a real child via
/// [`std::process::Command`] with the env allowlist enforced and all three
/// standard streams piped.
///
/// It is a zero-sized unit struct — it holds no configuration because it has
/// no policy to hold (see the module doc: this is the authority-free spawner).
/// Construct it with [`StdProcessRunner::new`] or [`Default`]; share one
/// `&dyn ProcessRunner` across threads (`ProcessRunner: Send + Sync`).
///
/// # Example
///
/// ```no_run
/// use std::path::PathBuf;
/// use rstui_plugin_host::process::{PluginSpawnSpec, ProcessRunner};
/// use rstui_plugin_host::std_process::StdProcessRunner;
///
/// let runner = StdProcessRunner::new();
/// let spec = PluginSpawnSpec {
///     program: PathBuf::from("/usr/local/bin/my-plugin"),
///     args: vec!["--mode".into(), "json".into()],
///     cwd: PathBuf::from("/tmp"),
///     // env_clear() is applied first, so the child sees ONLY this key.
///     env: vec![("PLUGIN_TOKEN".into(), "abc123".into())],
/// };
/// // Spawns a real OS process (hence `no_run`).
/// let mut process = runner.spawn(&spec).unwrap();
/// process.request_shutdown().unwrap();
/// let _ = process.wait().unwrap();
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct StdProcessRunner;

impl StdProcessRunner {
    /// Creates a new runner.
    ///
    /// Equivalent to [`StdProcessRunner::default`]; provided so call sites read
    /// `StdProcessRunner::new()` like the rest of the crate's constructors.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl ProcessRunner for StdProcessRunner {
    /// Spawns the process described by `spec`.
    ///
    /// Builds a [`std::process::Command`] that:
    ///
    /// - runs [`spec.program`](PluginSpawnSpec::program) with
    ///   [`spec.args`](PluginSpawnSpec::args) in
    ///   [`spec.cwd`](PluginSpawnSpec::cwd);
    /// - calls [`Command::env_clear`] then [`Command::envs`] with
    ///   [`spec.env`](PluginSpawnSpec::env) — the ADR 0007 §2 enforcement
    ///   point: the child inherits **no** ambient environment, only the
    ///   allowlist;
    /// - pipes stdin, stdout, and stderr so the host owns all three streams.
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] from [`Command::spawn`] if the process could
    /// not be started (executable not found, permission denied, etc.).
    fn spawn(&self, spec: &PluginSpawnSpec) -> io::Result<Box<dyn PluginProcess>> {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(&spec.cwd)
            // ADR 0007 §2: wipe the inherited environment, then re-add ONLY
            // the resolved allowlist. Order matters — env_clear must precede
            // envs or the cleared ambient vars would be re-cleared after the
            // allowlist was set. Nothing ambient survives this pair.
            .env_clear()
            .envs(spec.env.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn()?;

        // `Stdio::piped()` guarantees these are `Some` immediately after a
        // successful spawn; take ownership so the host drives them and so
        // `request_shutdown` can drop stdin to signal EOF.
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        Ok(Box::new(StdPluginProcess {
            child,
            stdin,
            stdout,
            stderr,
        }))
    }
}

// ---------------------------------------------------------------------------
// StdPluginProcess
// ---------------------------------------------------------------------------

/// A handle to a real spawned plugin child process.
///
/// Wraps the [`Child`] plus the three piped stream handles. `stdin` is an
/// [`Option`] because [`request_shutdown`](Self::request_shutdown) *takes and
/// drops it* to deliver the cooperative-shutdown EOF; `stdout`/`stderr` are
/// `Option` only so [`take_stderr`](Self::take_stderr) can move stderr out
/// once.
///
/// Constructed exclusively by [`StdProcessRunner::spawn`]; the host only ever
/// sees it as a `Box<dyn PluginProcess>`.
pub struct StdPluginProcess {
    /// The live child. Owns the OS process; `kill`/`wait`/`try_wait` go
    /// through it.
    child: Child,
    /// The child's stdin pipe. `None` after [`request_shutdown`](Self::request_shutdown)
    /// has dropped it to signal cooperative shutdown via EOF.
    stdin: Option<ChildStdin>,
    /// The child's stdout pipe. `None` after [`take_stdout`](Self::take_stdout)
    /// has moved it onto the host's deadline-bounded reader thread.
    stdout: Option<ChildStdout>,
    /// The child's stderr pipe. `None` after [`take_stderr`](Self::take_stderr)
    /// has moved it to the host's log-draining task.
    stderr: Option<ChildStderr>,
}

impl PluginProcess for StdPluginProcess {
    /// The live [`ChildStdin`] as a `&mut dyn Write`.
    ///
    /// The host writes length-prefixed request frames here.
    ///
    /// # Panics
    ///
    /// Panics if called **after** [`request_shutdown`](Self::request_shutdown)
    /// has run: cooperative shutdown deliberately drops the stdin pipe (that
    /// dropped pipe *is* the EOF the plugin reacts to), so there is no writer
    /// left. Writing to a plugin after asking it to shut down is a host bug,
    /// hence a panic with a clear message rather than a silently swallowed or
    /// surprising `io::Error`. Send all frames before `request_shutdown`.
    fn stdin(&mut self) -> &mut dyn Write {
        self.stdin.as_mut().expect(
            "StdPluginProcess::stdin() called after request_shutdown() closed the stdin pipe; \
             cooperative shutdown drops stdin to signal EOF, so no frames may be written after it",
        )
    }

    /// The live [`ChildStdout`] as a `&mut dyn Read`.
    ///
    /// The host reads length-prefixed response frames here when it uses the
    /// synchronous path; once [`take_stdout`](Self::take_stdout) has moved
    /// the pipe onto a reader thread this must not be called.
    fn stdout(&mut self) -> &mut dyn Read {
        self.stdout
            .as_mut()
            .expect("StdPluginProcess::stdout() called after take_stdout() moved the pipe")
    }

    /// Move the child's [`ChildStdout`] out so the host can run
    /// deadline-bounded reads on a dedicated thread (ADR 0007 §6). `Some`
    /// once, then `None`. After this, [`stdout`](Self::stdout) panics —
    /// the host uses exactly one of the two paths per process.
    fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>> {
        self.stdout
            .take()
            .map(|stdout| Box::new(stdout) as Box<dyn Read + Send>)
    }

    /// Moves the child's stderr out for the host's log-draining task.
    ///
    /// Returns `Some` on the first call, `None` on every call after — the
    /// stream is consumed exactly once. Diagnostic text only; frames never
    /// appear on stderr (ADR 0007 §4).
    fn take_stderr(&mut self) -> Option<Box<dyn Read + Send>> {
        self.stderr
            .take()
            .map(|stderr| Box::new(stderr) as Box<dyn Read + Send>)
    }

    /// Cooperative shutdown (ADR 0007 §6 phase 1): drop the child's stdin pipe.
    ///
    /// Taking `self.stdin` to `None` drops the [`ChildStdin`], which closes
    /// the write end of the pipe. The plugin's read loop then sees EOF on its
    /// stdin and is expected to exit on its own within the host's
    /// `Clock`-bounded grace window (the timing lives in the host, not here —
    /// see the module doc).
    ///
    /// **Idempotent**: a second call finds `stdin` already `None` and is a
    /// no-op returning `Ok(())`. It never escalates to a kill — escalation is
    /// the host's explicit decision via [`kill`](Self::kill).
    ///
    /// # Errors
    ///
    /// Never returns `Err` in this implementation (dropping a pipe is
    /// infallible); the `io::Result` is kept for the trait contract and for
    /// implementations that might signal differently.
    fn request_shutdown(&mut self) -> io::Result<()> {
        // Drop the stdin pipe (if still held). Drop closes the fd → the
        // plugin reads EOF. This is the entire cooperative signal.
        self.stdin = None;
        Ok(())
    }

    /// Forced shutdown (ADR 0007 §6 phase 2): [`Child::kill`].
    ///
    /// On Unix this delivers `SIGKILL` — the only hard stop available without
    /// `unsafe` libc, so the only forced primitive this runner has (ADR 0007
    /// §7; there is intentionally no `SIGTERM` path).
    ///
    /// **Idempotent / already-exited tolerant**: if the process has already
    /// exited, `Child::kill` reports an error (`InvalidInput` on most
    /// platforms — "process already exited"). That is treated as **success**:
    /// the goal of `kill` is "the process is not running", which already
    /// holds, so this returns `Ok(())`. A second `kill` after the process is
    /// gone is therefore also `Ok(())`.
    ///
    /// # Errors
    ///
    /// Propagates an [`io::Error`] from [`Child::kill`] only if it is *not*
    /// the already-exited case (i.e. a genuine failure to deliver the signal
    /// to a still-running process).
    fn kill(&mut self) -> io::Result<()> {
        match self.child.kill() {
            Ok(()) => Ok(()),
            Err(error) => {
                // "Already exited" is the success case for an idempotent kill:
                // the OS refuses to signal a process that is already gone, but
                // the postcondition ("not running") is already satisfied.
                if matches!(self.child.try_wait(), Ok(Some(_))) {
                    Ok(())
                } else {
                    Err(error)
                }
            }
        }
    }

    /// Blocks on [`Child::wait`] and maps the status to an [`ExitOutcome`].
    ///
    /// `code` is [`ExitStatus::code`](std::process::ExitStatus::code) (`None`
    /// when the process was terminated by a signal — e.g. the `SIGKILL` from
    /// [`kill`](Self::kill) — without a normal exit code) and `success` is
    /// [`ExitStatus::success`](std::process::ExitStatus::success).
    ///
    /// Call only after [`request_shutdown`](Self::request_shutdown) or
    /// [`kill`](Self::kill), per the trait contract.
    ///
    /// # Errors
    ///
    /// Propagates the [`io::Error`] from [`Child::wait`].
    fn wait(&mut self) -> io::Result<ExitOutcome> {
        let status = self.child.wait()?;
        Ok(ExitOutcome {
            code: status.code(),
            success: status.success(),
        })
    }

    /// Non-blocking [`Child::try_wait`] mapped to an [`ExitOutcome`].
    ///
    /// `Some(outcome)` if the process has already exited, `None` if it is
    /// still running. The host polls this against its [`Clock`](crate::clock::Clock)
    /// to drive the cooperative grace period without sleeping.
    ///
    /// # Errors
    ///
    /// Propagates the [`io::Error`] from [`Child::try_wait`].
    fn try_wait(&mut self) -> io::Result<Option<ExitOutcome>> {
        Ok(self.child.try_wait()?.map(|status| ExitOutcome {
            code: status.code(),
            success: status.success(),
        }))
    }
}

// ---------------------------------------------------------------------------
// Tests — real OS processes, deterministic, Unix-only
// ---------------------------------------------------------------------------

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Read;
    use std::path::PathBuf;

    /// `/bin/sh` is POSIX-guaranteed and present on this dev/CI target
    /// (darwin/unix). All process tests drive it or `cat` so they need no
    /// repo file and are fully self-contained.
    const SH: &str = "/bin/sh";

    /// A spec running `sh -c <script>` with the given env allowlist and a cwd
    /// that is guaranteed to exist (the system temp dir).
    fn sh_spec(script: &str, env: Vec<(String, String)>) -> PluginSpawnSpec {
        PluginSpawnSpec {
            program: PathBuf::from(SH),
            args: vec!["-c".into(), script.into()],
            cwd: std::env::temp_dir(),
            env,
        }
    }

    fn read_stdout_to_end(process: &mut Box<dyn PluginProcess>) -> Vec<u8> {
        let mut buf = Vec::new();
        process
            .stdout()
            .read_to_end(&mut buf)
            .expect("read stdout to end");
        buf
    }

    // -----------------------------------------------------------------------
    // ADR 0007 §2: env_clear + allowlist — the child sees ONLY declared keys
    // -----------------------------------------------------------------------

    #[test]
    fn env_clear_then_allowlist_is_the_only_environment_the_child_sees() {
        // Print the one allowed var, then HOME with a shell default of UNSET.
        // If env_clear worked, HOME (ambient on every dev machine) is gone, so
        // the parameter expansion yields "UNSET".
        let runner = StdProcessRunner::new();
        let spec = sh_spec(
            r#"printf "%s" "$ALLOWED"; printf "|%s" "${HOME-UNSET}""#,
            vec![("ALLOWED".into(), "yes".into())],
        );

        let mut process = runner.spawn(&spec).expect("spawn sh");
        let out = read_stdout_to_end(&mut process);
        let status = process.wait().expect("wait");

        assert_eq!(
            String::from_utf8_lossy(&out),
            "yes|UNSET",
            "only the ALLOWED allowlist var must be present; ambient HOME must have been cleared",
        );
        assert!(status.success);
    }

    // -----------------------------------------------------------------------
    // cwd is honoured
    // -----------------------------------------------------------------------

    #[test]
    fn current_dir_is_the_spec_cwd() {
        // Canonicalise both sides: on macOS the temp dir is under a symlink
        // (`/var` → `/private/var`), and `pwd` in the child resolves it, so a
        // naive string compare against `temp_dir()` would spuriously fail.
        let temp = std::env::temp_dir();
        let canonical_temp = std::fs::canonicalize(&temp).expect("canonicalize temp dir");

        let runner = StdProcessRunner::new();
        let spec = PluginSpawnSpec {
            program: PathBuf::from(SH),
            args: vec!["-c".into(), "pwd".into()],
            cwd: temp,
            env: Vec::new(),
        };

        let mut process = runner.spawn(&spec).expect("spawn sh");
        let out = read_stdout_to_end(&mut process);
        process.wait().expect("wait");

        let reported = String::from_utf8_lossy(&out);
        let reported_canonical =
            std::fs::canonicalize(reported.trim()).expect("canonicalize child pwd");
        assert_eq!(
            reported_canonical, canonical_temp,
            "child working directory must be the spec cwd",
        );
    }

    // -----------------------------------------------------------------------
    // stdin → stdout piping, with request_shutdown closing stdin (EOF)
    // -----------------------------------------------------------------------

    #[test]
    fn stdin_is_piped_to_stdout_and_request_shutdown_closes_stdin_as_eof() {
        // `cat` echoes stdin to stdout and exits when stdin hits EOF. We
        // write bytes, then request_shutdown() (drops stdin → cat sees EOF),
        // then drain stdout: it must be exactly what we wrote, and the
        // process must exit cleanly.
        let runner = StdProcessRunner::new();
        let spec = PluginSpawnSpec {
            program: PathBuf::from("/bin/cat"),
            args: Vec::new(),
            cwd: std::env::temp_dir(),
            env: Vec::new(),
        };

        let mut process = runner.spawn(&spec).expect("spawn cat");
        process
            .stdin()
            .write_all(b"frame-bytes")
            .expect("write to stdin");

        // Cooperative shutdown: drop stdin so `cat` reads EOF and exits.
        process.request_shutdown().expect("request_shutdown");

        let out = read_stdout_to_end(&mut process);
        assert_eq!(out, b"frame-bytes", "cat must echo exactly what was sent");

        let status = process.wait().expect("wait");
        assert!(status.success, "cat exits 0 after clean EOF");
    }

    #[test]
    fn stdin_panics_after_request_shutdown_closed_it() {
        let runner = StdProcessRunner::new();
        let spec = PluginSpawnSpec {
            program: PathBuf::from("/bin/cat"),
            args: Vec::new(),
            cwd: std::env::temp_dir(),
            env: Vec::new(),
        };
        let mut process = runner.spawn(&spec).expect("spawn cat");

        process.request_shutdown().expect("request_shutdown");

        // Writing after cooperative shutdown is a host bug → documented panic.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = process.stdin();
        }));
        assert!(
            result.is_err(),
            "stdin() must panic once request_shutdown has dropped the pipe",
        );

        // Clean up the child so the test leaves no process behind.
        let _ = process.kill();
        let _ = process.wait();
    }

    // -----------------------------------------------------------------------
    // wait reports the child's exit code
    // -----------------------------------------------------------------------

    #[test]
    fn wait_reports_non_zero_exit_code() {
        let runner = StdProcessRunner::new();
        let spec = sh_spec("exit 7", Vec::new());

        let mut process = runner.spawn(&spec).expect("spawn sh");
        let status = process.wait().expect("wait");

        assert_eq!(status.code, Some(7), "exit code must be propagated");
        assert!(!status.success, "non-zero exit is not success");
    }

    #[test]
    fn try_wait_is_none_while_running_then_some_after_exit() {
        let runner = StdProcessRunner::new();
        // Block on a closed stdin read so the process is reliably alive for
        // the first poll, then make it exit by closing stdin.
        let spec = PluginSpawnSpec {
            program: PathBuf::from("/bin/cat"),
            args: Vec::new(),
            cwd: std::env::temp_dir(),
            env: Vec::new(),
        };

        let mut process = runner.spawn(&spec).expect("spawn cat");
        assert_eq!(
            process.try_wait().expect("try_wait"),
            None,
            "try_wait must be None while cat is still reading stdin",
        );

        process.request_shutdown().expect("request_shutdown");
        let status = process.wait().expect("wait");
        assert!(status.success);

        // After exit, try_wait must report the outcome (not None).
        let polled = process.try_wait().expect("try_wait after exit");
        assert_eq!(
            polled,
            Some(ExitOutcome {
                code: Some(0),
                success: true,
            }),
        );
    }

    // -----------------------------------------------------------------------
    // kill forces termination; second kill after exit is idempotent Ok(())
    // -----------------------------------------------------------------------

    #[test]
    fn kill_terminates_a_hung_process_and_is_idempotent_after_exit() {
        let runner = StdProcessRunner::new();
        // `sleep 30` would never exit on its own within the test.
        let spec = sh_spec("sleep 30", Vec::new());

        let mut process = runner.spawn(&spec).expect("spawn sh");

        // Forced kill (SIGKILL on unix).
        process.kill().expect("first kill");

        let status = process.wait().expect("wait after kill");
        assert!(
            !status.success,
            "a SIGKILLed process must not report success",
        );

        // Second kill, now that the process is gone, must be Ok(()) — the
        // already-exited case is treated as success (idempotent).
        process
            .kill()
            .expect("second kill after exit must be idempotent Ok(())");
    }

    // -----------------------------------------------------------------------
    // request_shutdown is idempotent
    // -----------------------------------------------------------------------

    #[test]
    fn request_shutdown_is_idempotent() {
        let runner = StdProcessRunner::new();
        let spec = PluginSpawnSpec {
            program: PathBuf::from("/bin/cat"),
            args: Vec::new(),
            cwd: std::env::temp_dir(),
            env: Vec::new(),
        };
        let mut process = runner.spawn(&spec).expect("spawn cat");

        process.request_shutdown().expect("first request_shutdown");
        process
            .request_shutdown()
            .expect("second request_shutdown must be a no-op Ok(())");

        let status = process.wait().expect("wait");
        assert!(status.success, "cat still exits cleanly on EOF");
    }

    // -----------------------------------------------------------------------
    // take_stderr yields the stream once, then None
    // -----------------------------------------------------------------------

    #[test]
    fn take_stderr_yields_diagnostic_stream_once_then_none() {
        let runner = StdProcessRunner::new();
        let spec = sh_spec(r#"printf "diag" 1>&2"#, Vec::new());

        let mut process = runner.spawn(&spec).expect("spawn sh");

        let mut stderr = process
            .take_stderr()
            .expect("stderr available on first take");
        let mut buf = String::new();
        stderr.read_to_string(&mut buf).expect("read stderr");
        assert_eq!(buf, "diag");

        assert!(
            process.take_stderr().is_none(),
            "stderr must be consumed exactly once",
        );

        process.wait().expect("wait");
    }

    // -----------------------------------------------------------------------
    // spawn error: non-existent program
    // -----------------------------------------------------------------------

    #[test]
    fn spawn_of_a_nonexistent_program_is_an_error() {
        let runner = StdProcessRunner::new();
        let spec = PluginSpawnSpec {
            program: PathBuf::from("/nonexistent/definitely/not/a/real/binary-xyz"),
            args: Vec::new(),
            cwd: std::env::temp_dir(),
            env: Vec::new(),
        };

        let result = runner.spawn(&spec);
        assert!(
            result.is_err(),
            "spawning a missing executable must return Err",
        );
    }

    // -----------------------------------------------------------------------
    // Trait-object usability (the host only ever sees `dyn` forms)
    // -----------------------------------------------------------------------

    #[test]
    fn usable_through_trait_objects_end_to_end() {
        fn drive(runner: &dyn ProcessRunner, spec: &PluginSpawnSpec) -> ExitOutcome {
            let mut process = runner.spawn(spec).expect("spawn");
            process.stdin().write_all(b"hello").expect("write stdin");
            process.request_shutdown().expect("request_shutdown");
            let mut buf = Vec::new();
            process.stdout().read_to_end(&mut buf).expect("read stdout");
            assert_eq!(buf, b"hello");
            process.wait().expect("wait")
        }

        let runner = StdProcessRunner::new();
        let spec = PluginSpawnSpec {
            program: PathBuf::from("/bin/cat"),
            args: Vec::new(),
            cwd: std::env::temp_dir(),
            env: Vec::new(),
        };
        let outcome = drive(&runner, &spec);
        assert!(outcome.success);
        assert_eq!(outcome.code, Some(0));
    }
}
