//! The spawn-and-pipe seam: the single point where "real OS process" and
//! "in-memory fake" diverge (ADR 0007 §5).
//!
//! ## The seam
//!
//! [`ProcessRunner`] is the trait that decides which world we are in.
//! Calling code — the future host runtime — holds a `&dyn ProcessRunner` and
//! never sees whether it is talking to a real child process or a scripted
//! [`FakePluginProcess`]. The real `std::process` implementation is a later
//! slice and belongs in a separate source file; this module contains only the
//! **trait definitions and the deterministic in-memory fakes**, mirroring the
//! way `rstui-core`'s `TestBackend` / `TestEventSource` are the canonical
//! fakes for their seams.
//!
//! ## Cooperative-then-forced shutdown (ADR 0007 §6)
//!
//! Shutdown is always **two-phase** and the calling code must respect the
//! sequence:
//!
//! 1. [`PluginProcess::request_shutdown`] — cooperative: signals intent to the
//!    plugin (e.g. closes the stdin pipe so the plugin's read loop ends
//!    naturally, or sends a protocol `Shutdown` frame before this is called).
//!    The plugin is expected to exit on its own within a bounded grace period
//!    (measured by a `Clock` in the future host runtime, not here).
//! 2. [`PluginProcess::kill`] — forced: if the grace period elapses or the
//!    plugin misbehaves, kill it unconditionally.
//!
//! [`PluginProcess::wait`] blocks until the process exits (cooperative or
//! forced). [`PluginProcess::try_wait`] is a non-blocking poll for the outer
//! loop that drives the grace period without sleeping.
//!
//! ## Deterministic testing with no OS process, socket, or clock
//!
//! The fakes make every interesting scenario a plain unit test:
//!
//! - **Plugin crash**: script a non-zero / `None` exit code.
//! - **Plugin hangs**: `try_wait` returns `None` until `kill` is called.
//! - **Denied IO**: replace the runner with one that returns
//!   `Err(io::Error::new(io::ErrorKind::PermissionDenied, "…"))`.
//! - **Frame exchange**: write bytes via `stdin()`, read them back from
//!   [`FakePluginProcess::written_to_stdin`]; script what the plugin will
//!   "say" on `stdout()`.
//!
//! No real process, no socket, no TTY, no wall clock — the same guarantee
//! `TestBackend` / `TestEventSource` give to rendering and input.
//!
//! ## Example
//!
//! ```
//! use std::io::{Read, Write};
//! use std::path::PathBuf;
//! use rstui_plugin_host::process::{
//!     ExitOutcome, FakeProcessRunner, FakePluginProcess, PluginProcess,
//!     PluginSpawnSpec, ProcessRunner,
//! };
//!
//! // Script what the fake plugin will emit on stdout.
//! let scripted_stdout = b"hello from plugin\n".to_vec();
//! let runner = FakeProcessRunner::new(
//!     FakePluginProcess::new(
//!         scripted_stdout,
//!         Vec::new(),
//!         ExitOutcome { code: Some(0), success: true },
//!     ),
//! );
//!
//! // Spawn (no real process created).
//! let spec = PluginSpawnSpec {
//!     program: PathBuf::from("/usr/local/bin/my-plugin"),
//!     args: vec!["--mode".into(), "json".into()],
//!     cwd: PathBuf::from("/tmp/plugin-workdir"),
//!     env: vec![("PLUGIN_SECRET".into(), "s3cr3t".into())],
//! };
//! let mut process = runner.spawn(&spec).unwrap();
//!
//! // Write a request frame (raw bytes — framing is the protocol layer's job).
//! process.stdin().write_all(b"request bytes").unwrap();
//!
//! // Read the scripted response.
//! let mut buf = String::new();
//! process.stdout().read_to_string(&mut buf).unwrap();
//! assert_eq!(buf, "hello from plugin\n");
//!
//! // Cooperative shutdown, then confirm the outcome.
//! process.request_shutdown().unwrap();
//! let outcome = process.wait().unwrap();
//! assert!(outcome.success);
//! assert_eq!(outcome.code, Some(0));
//!
//! // The runner recorded exactly the spec that was passed.
//! let spawned = runner.spawned();
//! assert_eq!(spawned.len(), 1);
//! assert_eq!(spawned[0].program, PathBuf::from("/usr/local/bin/my-plugin"));
//! assert_eq!(spawned[0].args, vec!["--mode", "json"]);
//! assert_eq!(spawned[0].env, vec![("PLUGIN_SECRET".into(), "s3cr3t".into())]);
//! ```

use std::io::{self, Cursor, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// Everything the host needs to launch a plugin process.
///
/// A `PluginSpawnSpec` is constructed by the host after the manifest has been
/// parsed, the capability grants resolved, and the env allowlist filtered. It
/// is a **pure value** — no OS interaction — so it is cheap to clone and
/// straightforward to assert on in tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSpawnSpec {
    /// The executable to run.
    pub program: PathBuf,
    /// Arguments passed to the executable.
    pub args: Vec<String>,
    /// The working directory the process starts in.
    pub cwd: PathBuf,
    /// The **fully resolved env allowlist** the host built from the manifest's
    /// `env` capability grants. The real `std::process` runner will call
    /// [`Command::env_clear`] and then set exactly these pairs — no ambient
    /// environment leaks through (ADR 0007 §2). The spec carries no ambient
    /// env itself; callers must not rely on any inherited variable.
    ///
    /// [`Command::env_clear`]: std::process::Command::env_clear
    pub env: Vec<(String, String)>,
}

/// The terminal result of a plugin process.
///
/// `code` is `None` when the process was killed before it could produce an
/// exit status (the OS did not give one back). `success` mirrors
/// [`ExitStatus::success`](std::process::ExitStatus::success) — `true` only
/// when the process exited with code `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitOutcome {
    /// The numeric exit code, or `None` if the process was killed without
    /// producing one.
    pub code: Option<i32>,
    /// Whether the process exited cleanly (`code == Some(0)`).
    pub success: bool,
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Spawns a plugin process from a [`PluginSpawnSpec`].
///
/// The trait is `Send + Sync` so a single shared runner can be called from any
/// thread. The real implementation wraps [`std::process::Command`];
/// [`FakeProcessRunner`] returns pre-configured [`FakePluginProcess`]es.
pub trait ProcessRunner: Send + Sync {
    /// Spawns the process described by `spec` and returns a handle to it.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the process could not be spawned (permission
    /// denied, executable not found, etc.).
    fn spawn(&self, spec: &PluginSpawnSpec) -> io::Result<Box<dyn PluginProcess>>;
}

/// A handle to a running (or just-exited) plugin process.
///
/// The three byte streams map to the child's standard file descriptors:
/// - `stdin` — host sends frame bytes to the plugin.
/// - `stdout` — plugin sends frame bytes to the host.
/// - `stderr` — plugin diagnostic text; the protocol layer never writes frames
///   here (ADR 0007 §4 "Logging Contract": untrusted code must not grow host
///   memory unboundedly through its output).
///
/// Shutdown follows the **cooperative-then-forced** contract (ADR 0007 §6):
/// call [`request_shutdown`](Self::request_shutdown) first, wait a bounded
/// grace period via [`try_wait`](Self::try_wait), then call
/// [`kill`](Self::kill) if the grace period elapses.
pub trait PluginProcess: Send {
    /// A writer that delivers bytes to the plugin's stdin.
    ///
    /// The protocol layer uses this to send length-prefixed frames.
    fn stdin(&mut self) -> &mut dyn Write;

    /// A reader that receives bytes from the plugin's stdout.
    ///
    /// The protocol layer reads length-prefixed frames from here.
    fn stdout(&mut self) -> &mut dyn Read;

    /// Takes ownership of the plugin's stderr stream, if not already taken.
    ///
    /// Returns `Some` on the first call and `None` on every subsequent call —
    /// the stream is consumed once (the host spawns a log-draining task that
    /// owns it). Diagnostic text only; frames never appear on stderr.
    fn take_stderr(&mut self) -> Option<Box<dyn Read + Send>>;

    /// Signals cooperative shutdown intent to the plugin (ADR 0007 §6, phase 1).
    ///
    /// A real implementation typically closes the stdin pipe so the plugin's
    /// read loop sees EOF and exits naturally. The host must then poll
    /// [`try_wait`](Self::try_wait) within a bounded grace period before
    /// escalating to [`kill`](Self::kill).
    ///
    /// Calling `request_shutdown` more than once is allowed and has no
    /// additional effect.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the shutdown signal could not be delivered.
    fn request_shutdown(&mut self) -> io::Result<()>;

    /// Forces the process to terminate immediately (ADR 0007 §6, phase 2).
    ///
    /// Used when the cooperative grace period elapses or when an error demands
    /// immediate teardown. After `kill`, [`wait`](Self::wait) will return
    /// promptly with a killed outcome.
    ///
    /// Calling `kill` more than once is allowed and has no additional effect.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the kill signal could not be delivered.
    fn kill(&mut self) -> io::Result<()>;

    /// Blocks until the process exits and returns its [`ExitOutcome`].
    ///
    /// Should only be called after [`request_shutdown`](Self::request_shutdown)
    /// or [`kill`](Self::kill). On a [`FakePluginProcess`] this returns the
    /// scripted outcome immediately.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the wait could not be performed.
    fn wait(&mut self) -> io::Result<ExitOutcome>;

    /// Non-blocking poll: returns `Some(outcome)` if the process has already
    /// exited, `None` if it is still running.
    ///
    /// Returns `None` on a [`FakePluginProcess`] while in the "still running"
    /// state, and `Some` once [`request_shutdown`](Self::request_shutdown) or
    /// [`kill`](Self::kill) has been called, or if the process was scripted as
    /// already-exited.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the poll could not be performed.
    fn try_wait(&mut self) -> io::Result<Option<ExitOutcome>>;
}

// ---------------------------------------------------------------------------
// FakeProcessRunner
// ---------------------------------------------------------------------------

struct FakeRunnerInner {
    /// Specs handed to `spawn`, in call order.
    spawned: Vec<PluginSpawnSpec>,
    /// Pre-configured processes to hand out, front-to-back (FIFO).
    queue: Vec<FakePluginProcess>,
}

/// A scripted [`ProcessRunner`] that records every [`PluginSpawnSpec`] it
/// receives and yields pre-configured [`FakePluginProcess`]es.
///
/// Use this in tests to:
///
/// - Assert that the host passed the correct program, args, cwd, and env
///   allowlist to `spawn`.
/// - Control exactly what the "plugin" emits on stdout and stderr, and what
///   exit outcome it produces.
///
/// The runner is `Send + Sync` (uses a `Mutex` internally) so it can be
/// shared across threads if the host runtime eventually calls `spawn` from a
/// worker.
///
/// # Panics
///
/// `spawn` panics if the pre-configured process queue is exhausted (all
/// scripted processes have already been handed out). Add more processes via
/// [`push_process`](Self::push_process) before the next `spawn` call.
pub struct FakeProcessRunner {
    inner: Arc<Mutex<FakeRunnerInner>>,
}

impl FakeProcessRunner {
    /// Creates a runner pre-loaded with a single process.
    ///
    /// For multiple spawns use [`new_empty`](Self::new_empty) plus
    /// [`push_process`](Self::push_process).
    #[must_use]
    pub fn new(process: FakePluginProcess) -> Self {
        let runner = Self::new_empty();
        runner.push_process(process);
        runner
    }

    /// Creates a runner with an empty process queue.
    ///
    /// Call [`push_process`](Self::push_process) to enqueue processes before
    /// `spawn` is called.
    #[must_use]
    pub fn new_empty() -> Self {
        Self {
            inner: Arc::new(Mutex::new(FakeRunnerInner {
                spawned: Vec::new(),
                queue: Vec::new(),
            })),
        }
    }

    /// Appends `process` to the tail of the queue.
    ///
    /// Processes are consumed FIFO: the first `push_process` call supplies the
    /// first `spawn`, and so on.
    pub fn push_process(&self, process: FakePluginProcess) {
        self.inner
            .lock()
            .expect("lock poisoned")
            .queue
            .push(process);
    }

    /// Returns a snapshot of every [`PluginSpawnSpec`] handed to
    /// [`spawn`](ProcessRunner::spawn), in call order.
    ///
    /// This is the primary assertion surface for spawn arguments: assert the
    /// program, args, cwd, and env allowlist were constructed correctly by the
    /// host.
    #[must_use]
    pub fn spawned(&self) -> Vec<PluginSpawnSpec> {
        self.inner.lock().expect("lock poisoned").spawned.clone()
    }
}

impl ProcessRunner for FakeProcessRunner {
    fn spawn(&self, spec: &PluginSpawnSpec) -> io::Result<Box<dyn PluginProcess>> {
        let mut inner = self.inner.lock().expect("lock poisoned");
        inner.spawned.push(spec.clone());
        assert!(
            !inner.queue.is_empty(),
            "FakeProcessRunner: spawn called but the pre-configured process queue is empty",
        );
        let process = inner.queue.remove(0);
        Ok(Box::new(process))
    }
}

// ---------------------------------------------------------------------------
// FakePluginProcess — lifecycle state machine
// ---------------------------------------------------------------------------

/// The lifecycle state of a [`FakePluginProcess`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeLifecycle {
    /// `try_wait` returns `None` — process is "running".
    Running,
    /// `request_shutdown` was called; `try_wait` returns `Some(scripted_outcome)`.
    ShuttingDown,
    /// `kill` was called; `try_wait` returns `Some(killed_outcome)`.
    Killed,
    /// The process was scripted as already-exited; `try_wait` returns
    /// `Some(scripted_outcome)` on the very first call.
    AlreadyExited,
}

/// A concrete writer stored inside [`FakePluginProcess`] that appends to the
/// shared stdin capture buffer.
///
/// Stored by value so `stdin()` can return `&mut dyn Write` with lifetime tied
/// to `&mut self`.
struct StdinCapture {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl Write for StdinCapture {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf
            .lock()
            .expect("lock poisoned")
            .extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// An in-memory [`PluginProcess`] whose behaviour is entirely scripted.
///
/// Build one with [`FakePluginProcess::new`] (or
/// [`FakePluginProcess::already_exited`] for a process that is dead on
/// arrival), hand it to a [`FakeProcessRunner`], and then drive the host code
/// under test.
///
/// After the interaction, call [`written_to_stdin`](Self::written_to_stdin) to
/// inspect every byte the host wrote to the plugin's stdin pipe.
pub struct FakePluginProcess {
    /// The write end of the fake stdin pipe; `stdin()` returns `&mut` this.
    stdin_capture: StdinCapture,
    /// Shared read-back handle for [`written_to_stdin`](Self::written_to_stdin).
    stdin_buf: Arc<Mutex<Vec<u8>>>,
    /// Scripted stdout bytes read via `stdout()`.
    stdout_script: Cursor<Vec<u8>>,
    /// Scripted stderr bytes; taken once via `take_stderr`.
    stderr_script: Option<Vec<u8>>,
    /// Outcome returned by `wait` and (once exited) `try_wait`.
    scripted_outcome: ExitOutcome,
    /// Lifecycle state machine.
    lifecycle: FakeLifecycle,
    /// Outcome used when `kill` is called (code=None, success=false).
    killed_outcome: ExitOutcome,
}

impl FakePluginProcess {
    /// Creates a process that starts in the **Running** state.
    ///
    /// - `stdout_bytes`: the bytes the "plugin" will emit on stdout, read back
    ///   in order through [`PluginProcess::stdout`].
    /// - `stderr_bytes`: bytes available via the first call to
    ///   [`PluginProcess::take_stderr`]; subsequent calls return `None`.
    /// - `outcome`: the [`ExitOutcome`] returned by [`PluginProcess::wait`]
    ///   and (after shutdown or kill) by [`PluginProcess::try_wait`].
    ///
    /// **Scripting a crash**: set `outcome.success = false` and
    /// `outcome.code = Some(non_zero)`.
    ///
    /// **Scripting a killed process** (no exit code): set `outcome.code = None`
    /// and `outcome.success = false`.
    ///
    /// **Scripting a hang until killed**: leave the process in `Running` state
    /// (the default) and let the test call `kill()`.
    #[must_use]
    pub fn new(stdout_bytes: Vec<u8>, stderr_bytes: Vec<u8>, outcome: ExitOutcome) -> Self {
        let stdin_buf = Arc::new(Mutex::new(Vec::new()));
        Self {
            stdin_capture: StdinCapture {
                buf: Arc::clone(&stdin_buf),
            },
            stdin_buf,
            stdout_script: Cursor::new(stdout_bytes),
            stderr_script: Some(stderr_bytes),
            scripted_outcome: outcome,
            lifecycle: FakeLifecycle::Running,
            killed_outcome: ExitOutcome {
                code: None,
                success: false,
            },
        }
    }

    /// Creates a process that is **already exited** before any interaction.
    ///
    /// Both `try_wait` and `wait` return `outcome` immediately; there is no
    /// "running" phase.
    #[must_use]
    pub fn already_exited(outcome: ExitOutcome) -> Self {
        let stdin_buf = Arc::new(Mutex::new(Vec::new()));
        Self {
            stdin_capture: StdinCapture {
                buf: Arc::clone(&stdin_buf),
            },
            stdin_buf,
            stdout_script: Cursor::new(Vec::new()),
            stderr_script: Some(Vec::new()),
            scripted_outcome: outcome,
            lifecycle: FakeLifecycle::AlreadyExited,
            killed_outcome: ExitOutcome {
                code: None,
                success: false,
            },
        }
    }

    /// Returns a copy of every byte the host has written to stdin so far.
    ///
    /// This is the primary assertion surface for the stdin direction: after the
    /// host writes a request frame, assert that `written_to_stdin` contains the
    /// exact expected bytes.
    #[must_use]
    pub fn written_to_stdin(&self) -> Vec<u8> {
        self.stdin_buf.lock().expect("lock poisoned").clone()
    }

    /// The exit outcome implied by the current lifecycle state, or `None` if
    /// the process is still running.
    fn current_outcome(&self) -> Option<ExitOutcome> {
        match self.lifecycle {
            FakeLifecycle::Running => None,
            FakeLifecycle::ShuttingDown => Some(self.scripted_outcome),
            FakeLifecycle::Killed => Some(self.killed_outcome),
            FakeLifecycle::AlreadyExited => Some(self.scripted_outcome),
        }
    }
}

impl PluginProcess for FakePluginProcess {
    fn stdin(&mut self) -> &mut dyn Write {
        &mut self.stdin_capture
    }

    fn stdout(&mut self) -> &mut dyn Read {
        &mut self.stdout_script
    }

    fn take_stderr(&mut self) -> Option<Box<dyn Read + Send>> {
        self.stderr_script.take().map(|bytes| {
            let reader: Box<dyn Read + Send> = Box::new(Cursor::new(bytes));
            reader
        })
    }

    fn request_shutdown(&mut self) -> io::Result<()> {
        if self.lifecycle == FakeLifecycle::Running {
            self.lifecycle = FakeLifecycle::ShuttingDown;
        }
        Ok(())
    }

    fn kill(&mut self) -> io::Result<()> {
        if matches!(
            self.lifecycle,
            FakeLifecycle::Running | FakeLifecycle::ShuttingDown
        ) {
            self.lifecycle = FakeLifecycle::Killed;
        }
        Ok(())
    }

    fn wait(&mut self) -> io::Result<ExitOutcome> {
        // If still running, advance to ShuttingDown so we get the scripted
        // outcome (cooperative wait — caller did not pre-kill).
        if self.lifecycle == FakeLifecycle::Running {
            self.lifecycle = FakeLifecycle::ShuttingDown;
        }
        Ok(self.current_outcome().unwrap_or(self.scripted_outcome))
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitOutcome>> {
        Ok(self.current_outcome())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::path::PathBuf;

    fn success_outcome() -> ExitOutcome {
        ExitOutcome {
            code: Some(0),
            success: true,
        }
    }

    fn crash_outcome() -> ExitOutcome {
        ExitOutcome {
            code: Some(1),
            success: false,
        }
    }

    fn killed_outcome() -> ExitOutcome {
        ExitOutcome {
            code: None,
            success: false,
        }
    }

    fn sample_spec() -> PluginSpawnSpec {
        PluginSpawnSpec {
            program: PathBuf::from("/usr/local/bin/my-plugin"),
            args: vec!["--mode".into(), "json".into()],
            cwd: PathBuf::from("/tmp/plugin-workdir"),
            env: vec![
                ("PLUGIN_ID".into(), "42".into()),
                ("PLUGIN_SECRET".into(), "s3cr3t".into()),
            ],
        }
    }

    // -----------------------------------------------------------------------
    // FakeProcessRunner: spawn recording
    // -----------------------------------------------------------------------

    #[test]
    fn runner_records_exact_spawn_spec_fields() {
        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            Vec::new(),
            Vec::new(),
            success_outcome(),
        ));

        let spec = sample_spec();
        let _process = runner.spawn(&spec).unwrap();

        let spawned = runner.spawned();
        assert_eq!(spawned.len(), 1);
        assert_eq!(
            spawned[0].program,
            PathBuf::from("/usr/local/bin/my-plugin")
        );
        assert_eq!(spawned[0].args, vec!["--mode", "json"]);
        assert_eq!(spawned[0].cwd, PathBuf::from("/tmp/plugin-workdir"));
        assert_eq!(
            spawned[0].env,
            vec![
                ("PLUGIN_ID".into(), "42".into()),
                ("PLUGIN_SECRET".into(), "s3cr3t".into()),
            ]
        );
    }

    #[test]
    fn runner_records_multiple_spawns_in_order() {
        let runner = FakeProcessRunner::new_empty();
        runner.push_process(FakePluginProcess::new(
            Vec::new(),
            Vec::new(),
            success_outcome(),
        ));
        runner.push_process(FakePluginProcess::new(
            Vec::new(),
            Vec::new(),
            crash_outcome(),
        ));

        let spec_a = PluginSpawnSpec {
            program: PathBuf::from("/bin/plugin-a"),
            args: Vec::new(),
            cwd: PathBuf::from("/"),
            env: Vec::new(),
        };
        let spec_b = PluginSpawnSpec {
            program: PathBuf::from("/bin/plugin-b"),
            args: Vec::new(),
            cwd: PathBuf::from("/"),
            env: Vec::new(),
        };

        let _pa = runner.spawn(&spec_a).unwrap();
        let _pb = runner.spawn(&spec_b).unwrap();

        let spawned = runner.spawned();
        assert_eq!(spawned.len(), 2);
        assert_eq!(spawned[0].program, PathBuf::from("/bin/plugin-a"));
        assert_eq!(spawned[1].program, PathBuf::from("/bin/plugin-b"));
    }

    // -----------------------------------------------------------------------
    // Stdout: scripted bytes read back correctly
    // -----------------------------------------------------------------------

    #[test]
    fn scripted_stdout_reads_back_exactly() {
        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            b"frame data from plugin".to_vec(),
            Vec::new(),
            success_outcome(),
        ));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        let mut buf = Vec::new();
        process.stdout().read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"frame data from plugin");
    }

    #[test]
    fn scripted_stdout_streams_correctly_across_multiple_reads() {
        let data = b"ABCDEFGH".to_vec();
        let runner =
            FakeProcessRunner::new(FakePluginProcess::new(data, Vec::new(), success_outcome()));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        let mut first = [0u8; 4];
        let n = process.stdout().read(&mut first).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&first, b"ABCD");

        let mut second = [0u8; 4];
        let n = process.stdout().read(&mut second).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&second, b"EFGH");

        // EOF
        let n = process.stdout().read(&mut second).unwrap();
        assert_eq!(n, 0);
    }

    // -----------------------------------------------------------------------
    // Stdin: bytes written by the host are captured
    // -----------------------------------------------------------------------

    #[test]
    fn bytes_written_to_stdin_are_captured_and_inspectable() {
        // FakePluginProcess must be constructed separately to call
        // written_to_stdin later.  We obtain it via a runner with an empty
        // queue + push.
        let stdin_buf = Arc::new(Mutex::new(Vec::new()));
        let process = FakePluginProcess {
            stdin_capture: StdinCapture {
                buf: Arc::clone(&stdin_buf),
            },
            stdin_buf: Arc::clone(&stdin_buf),
            stdout_script: Cursor::new(Vec::new()),
            stderr_script: Some(Vec::new()),
            scripted_outcome: success_outcome(),
            lifecycle: FakeLifecycle::Running,
            killed_outcome: killed_outcome(),
        };
        let reader = Arc::clone(&process.stdin_buf);

        let runner = FakeProcessRunner::new(process);
        let mut handle = runner.spawn(&sample_spec()).unwrap();

        handle.stdin().write_all(b"frame-header").unwrap();
        handle.stdin().write_all(b"-body").unwrap();

        let captured = reader.lock().unwrap().clone();
        assert_eq!(captured, b"frame-header-body");
    }

    #[test]
    fn written_to_stdin_via_public_api() {
        // Build the process first, keep a reference via Arc for inspection,
        // then hand it to the runner.
        let shared_buf = Arc::new(Mutex::new(Vec::new()));
        let process = FakePluginProcess {
            stdin_capture: StdinCapture {
                buf: Arc::clone(&shared_buf),
            },
            stdin_buf: Arc::clone(&shared_buf),
            stdout_script: Cursor::new(Vec::new()),
            stderr_script: Some(Vec::new()),
            scripted_outcome: success_outcome(),
            lifecycle: FakeLifecycle::Running,
            killed_outcome: killed_outcome(),
        };
        let inspection_buf = Arc::clone(&shared_buf);

        let runner = FakeProcessRunner::new(process);
        let mut handle = runner.spawn(&sample_spec()).unwrap();

        handle.stdin().write_all(b"hello plugin").unwrap();

        let captured = inspection_buf.lock().unwrap().clone();
        assert_eq!(captured, b"hello plugin");
    }

    // -----------------------------------------------------------------------
    // Stderr: taken once, then None
    // -----------------------------------------------------------------------

    #[test]
    fn take_stderr_yields_scripted_bytes_once_then_none() {
        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            Vec::new(),
            b"plugin diagnostic log".to_vec(),
            success_outcome(),
        ));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        // First call: Some
        let mut stderr = process
            .take_stderr()
            .expect("expected stderr on first call");
        let mut buf = String::new();
        stderr.read_to_string(&mut buf).unwrap();
        assert_eq!(buf, "plugin diagnostic log");

        // Second call: None
        assert!(
            process.take_stderr().is_none(),
            "second take_stderr should return None"
        );
    }

    // -----------------------------------------------------------------------
    // try_wait: Running → None, after shutdown/kill → Some
    // -----------------------------------------------------------------------

    #[test]
    fn try_wait_returns_none_while_running() {
        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            Vec::new(),
            Vec::new(),
            success_outcome(),
        ));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        assert_eq!(
            process.try_wait().unwrap(),
            None,
            "try_wait must return None while the process is still running"
        );
    }

    #[test]
    fn try_wait_returns_some_after_request_shutdown() {
        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            Vec::new(),
            Vec::new(),
            success_outcome(),
        ));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        process.request_shutdown().unwrap();
        assert_eq!(
            process.try_wait().unwrap(),
            Some(success_outcome()),
            "try_wait should return scripted outcome after cooperative shutdown"
        );
    }

    #[test]
    fn try_wait_returns_some_after_kill() {
        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            Vec::new(),
            Vec::new(),
            success_outcome(),
        ));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        process.kill().unwrap();
        let outcome = process
            .try_wait()
            .unwrap()
            .expect("should be Some after kill");
        assert!(!outcome.success);
        assert_eq!(outcome.code, None, "kill yields code=None");
    }

    // -----------------------------------------------------------------------
    // wait: success path and crash path
    // -----------------------------------------------------------------------

    #[test]
    fn wait_returns_scripted_success_outcome() {
        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            Vec::new(),
            Vec::new(),
            success_outcome(),
        ));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        process.request_shutdown().unwrap();
        let outcome = process.wait().unwrap();
        assert!(outcome.success);
        assert_eq!(outcome.code, Some(0));
    }

    #[test]
    fn wait_returns_crash_outcome_non_zero_code() {
        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            Vec::new(),
            Vec::new(),
            crash_outcome(),
        ));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        process.request_shutdown().unwrap();
        let outcome = process.wait().unwrap();
        assert!(!outcome.success);
        assert_eq!(outcome.code, Some(1));
    }

    #[test]
    fn wait_returns_crash_outcome_no_code() {
        let no_code = ExitOutcome {
            code: None,
            success: false,
        };
        let runner =
            FakeProcessRunner::new(FakePluginProcess::new(Vec::new(), Vec::new(), no_code));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        process.request_shutdown().unwrap();
        let outcome = process.wait().unwrap();
        assert!(!outcome.success);
        assert_eq!(outcome.code, None);
    }

    // -----------------------------------------------------------------------
    // kill before wait
    // -----------------------------------------------------------------------

    #[test]
    fn kill_before_wait_yields_killed_outcome() {
        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            Vec::new(),
            Vec::new(),
            success_outcome(),
        ));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        process.kill().unwrap();
        let outcome = process.wait().unwrap();
        // After kill, the killed_outcome (code=None, success=false) is returned.
        assert!(!outcome.success);
        assert_eq!(outcome.code, None);
    }

    // -----------------------------------------------------------------------
    // already_exited constructor
    // -----------------------------------------------------------------------

    #[test]
    fn already_exited_process_returns_outcome_immediately() {
        let runner = FakeProcessRunner::new(FakePluginProcess::already_exited(success_outcome()));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        // try_wait returns Some without any shutdown/kill call.
        assert_eq!(process.try_wait().unwrap(), Some(success_outcome()));
        assert_eq!(process.wait().unwrap(), success_outcome());
    }

    // -----------------------------------------------------------------------
    // Idempotency: calling request_shutdown / kill multiple times is safe
    // -----------------------------------------------------------------------

    #[test]
    fn request_shutdown_is_idempotent() {
        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            Vec::new(),
            Vec::new(),
            success_outcome(),
        ));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        process.request_shutdown().unwrap();
        process.request_shutdown().unwrap(); // should not panic or error
        assert_eq!(process.wait().unwrap(), success_outcome());
    }

    #[test]
    fn kill_is_idempotent() {
        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            Vec::new(),
            Vec::new(),
            success_outcome(),
        ));
        let mut process = runner.spawn(&sample_spec()).unwrap();

        process.kill().unwrap();
        process.kill().unwrap(); // should not panic or error
    }

    // -----------------------------------------------------------------------
    // Trait object usability
    // -----------------------------------------------------------------------

    #[test]
    fn plugin_process_is_usable_via_trait_object() {
        fn drive(process: &mut dyn PluginProcess) -> ExitOutcome {
            process.stdin().write_all(b"ping").unwrap();
            let mut buf = Vec::new();
            process.stdout().read_to_end(&mut buf).unwrap();
            process.request_shutdown().unwrap();
            process.wait().unwrap()
        }

        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            b"pong".to_vec(),
            Vec::new(),
            success_outcome(),
        ));
        let mut process = runner.spawn(&sample_spec()).unwrap();
        let outcome = drive(process.as_mut());
        assert!(outcome.success);
    }

    #[test]
    fn process_runner_is_usable_via_trait_object() {
        fn spawn_via_dyn(runner: &dyn ProcessRunner, spec: &PluginSpawnSpec) -> ExitOutcome {
            let mut process = runner.spawn(spec).unwrap();
            process.request_shutdown().unwrap();
            process.wait().unwrap()
        }

        let runner = FakeProcessRunner::new(FakePluginProcess::new(
            Vec::new(),
            Vec::new(),
            success_outcome(),
        ));
        let outcome = spawn_via_dyn(&runner, &sample_spec());
        assert!(outcome.success);
    }
}
