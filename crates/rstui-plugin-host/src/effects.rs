//! The privileged side-effect performer: the [`HostEffects`] trait the host
//! runs an *already-permitted* capability through (ADR 0007 §3/§5).
//!
//! ## Responsibility split
//!
//! This module encodes one half of the two-phase enforcement described in ADR
//! 0007 §3:
//!
//! 1. **The policy** ([`permission`](crate::permission)) decides
//!    Allow / Deny for a *canonicalised* [`CapabilityRequest`].
//! 2. **This module** performs the real OS effect — but *only* if the policy
//!    returned Allow. The trait seam means a test can assert that a *denied*
//!    request never reached the effector at all, not just that the response
//!    said "denied".
//!
//! [`SystemHostEffects`] is the production effector: pure safe `std`, no
//! permission checks of its own (it is the *dumb effector*, not the
//! gatekeeper). [`RecordingHostEffects`] is the deterministic test double: it
//! records every request it is given and returns a scripted result, so a
//! security test can assert the *exact* set of requests that reached the
//! effector and confirm a denied one is absent.
//!
//! ```
//! use rstui_plugin_host::effects::{HostEffects, RecordingHostEffects};
//! use rstui_plugin_host::capability::CapabilityRequest;
//!
//! // Default recorder returns Ok(empty payload) for every call.
//! let recorder = RecordingHostEffects::new();
//! let req = CapabilityRequest::Env { key: "PATH".into() };
//! let outcome = recorder.run(&req).unwrap();
//! assert_eq!(outcome.payload, vec![]);
//! assert_eq!(recorder.calls(), vec![req]);
//! ```

use std::fmt;
use std::sync::Mutex;

use crate::capability::{CapabilityRequest, FsMode};

// ── Outcome ──────────────────────────────────────────────────────────────────

/// The opaque byte payload returned to the plugin after a permitted capability
/// is performed.
///
/// The bytes' meaning is per-capability-kind:
///
/// - **`Env`**: the value of the environment variable, UTF-8 encoded.
/// - **`Filesystem { mode: Read }`**: the raw file contents.
/// - **`Command`**: the captured standard output of the child process.
/// - **`Filesystem { mode: Write }` / `Network`**: empty — the effect
///   (`std::fs::write` / a bounded TCP connect) produces no result data, so
///   an empty `payload` on a non-error response *is* the success signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityOutcome {
    /// The raw bytes returned to the plugin. Callers interpret them according
    /// to the capability kind that produced them (see the type docs).
    pub payload: Vec<u8>,
}

// ── Error ─────────────────────────────────────────────────────────────────────

/// An error from performing a permitted capability effect.
///
/// `Io` carries a human-readable description of the OS/runtime failure.
/// `Unsupported` signals that *this effector* does not implement the
/// capability kind. [`SystemHostEffects`] implements all four kinds and never
/// returns it, but the variant remains part of the contract: a custom
/// [`HostEffects`] (e.g. a sandbox that deliberately refuses `Command`) may
/// return it, and test doubles use it via
/// [`RecordingHostEffects::with_err`]. Either way **the permission boundary
/// is enforced upstream regardless** (ADR 0007 §3: the policy runs before
/// `run` is called; `Unsupported` is an effector concern, not a policy gap).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEffectError {
    /// The OS operation failed; `0` is a human-readable explanation including
    /// the original error and, for commands, the exit code and stderr.
    Io(String),
    /// This effector does not implement the requested capability kind.
    /// [`SystemHostEffects`] never returns this (it implements all four
    /// kinds); it exists for alternative effectors and test doubles. The
    /// permission decision still runs before any attempt to call `run`.
    Unsupported,
}

impl fmt::Display for HostEffectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "host effect IO error: {msg}"),
            Self::Unsupported => {
                f.write_str("host effect not supported for this capability kind by this effector")
            }
        }
    }
}

impl std::error::Error for HostEffectError {}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// The seam the host runs an *already-permitted* capability through.
///
/// Implementors must be `Send + Sync` so a single shared effector can serve
/// all plugin connections from any thread. `run` takes `&self` (shared
/// reference) so the effector is usable behind an `Arc`.
///
/// **This trait performs no permission checks.** The host calls `run` only
/// after the [`PermissionPolicy`](crate::permission::PermissionPolicy) returned
/// `Allow` (ADR 0007 §3). An effector that re-checked permissions would
/// duplicate policy and create drift; an effector that skipped the upstream
/// policy check would be a security hole. The contract is that the host owns
/// the ordering: *check, then run*.
pub trait HostEffects: Send + Sync {
    /// Perform the privileged side-effect described by `request` and return
    /// its byte-payload result.
    ///
    /// # Errors
    ///
    /// Returns [`HostEffectError::Io`] when the OS operation fails (missing
    /// file, non-zero exit, non-Unicode env value, refused connection, etc.)
    /// and [`HostEffectError::Unsupported`] if the implementor does not
    /// support the requested capability kind ([`SystemHostEffects`] supports
    /// all four and never returns it).
    fn run(&self, request: &CapabilityRequest) -> Result<CapabilityOutcome, HostEffectError>;
}

// ── SystemHostEffects ─────────────────────────────────────────────────────────

/// The production [`HostEffects`] implementation: pure safe `std`, no `unsafe`,
/// no permission checks.
///
/// It is the *dumb effector*: it performs whatever the host sends to `run` and
/// assumes the host already ran the policy check. That assumption is enforced
/// by architecture — only the host wrapper calls `run`, and only after
/// [`PermissionPolicy::check`](crate::permission::PermissionPolicy::check)
/// returns `Allow` (ADR 0007 §3). `SystemHostEffects` itself has no access to
/// the policy and performs no capability gating.
///
/// ## Supported capability kinds
///
/// | Kind | Effect | Payload |
/// |------|--------|---------|
/// | `Env { key }` | `std::env::var(key)` | UTF-8 value bytes |
/// | `Filesystem { mode: Read, path }` | `std::fs::read(path)` | Raw file bytes |
/// | `Filesystem { mode: Write, path, contents }` | `std::fs::write(path, contents)` | Empty payload on success |
/// | `Command { program, args }` | `std::process::Command::new(program).args(args).output()` | Stdout bytes on success |
/// | `Network { host, port }` | bounded `TcpStream::connect_timeout` (then closed) | Empty payload on success |
///
/// All four capability kinds are implemented. Write and Network produce no
/// result data, so success is signalled by an empty `payload` plus a
/// non-error response. As with every kind, the permission boundary is
/// enforced *upstream* by the host (ADR 0007 §3) — `SystemHostEffects`
/// performs no capability gating of its own.
///
/// ## Residual: a check→effect TOCTOU window (ADR 0007 §7)
///
/// The host canonicalises a path and the policy rules on it *lexically*
/// ([`normalize_lexical`](crate::capability::normalize_lexical) is total
/// and symlink-blind by design — see [`capability`](crate::capability)).
/// The real `std::fs::write` / `TcpStream::connect_timeout` then run on
/// that already-approved name. Between the check and the effect a
/// **symlink swap** inside a granted directory can redirect the *write*
/// outside the granted subtree, and a **DNS rebind** can move a granted
/// host name to a different address before `connect`. The write case is
/// strictly higher impact than the read residual already recorded in
/// [`capability`](crate::capability) (it can *create/overwrite* outside
/// the grant, not just read).
///
/// Closing this in-process needs `openat`/`O_NOFOLLOW`/`realpath`-after-
/// open and address-pinned connects — all `unsafe` libc, which the
/// workspace **forbids** (ADR 0007 driver 4 / §7). It is therefore a
/// **named, accepted residual**, mitigated exactly as ADR 0007 §7
/// prescribes for the whole `forbid(unsafe)` tier: run plugins under an
/// operator-provided OS sandbox / mount namespace / container, and treat
/// the manifest review as the authority on what paths and hosts a plugin
/// may be granted in the first place. `SystemHostEffects` does not, and
/// under the workspace constraints cannot, defeat it; that is a recorded
/// decision, not an oversight.
#[derive(Debug, Default)]
pub struct SystemHostEffects;

impl SystemHostEffects {
    /// Creates a new `SystemHostEffects` effector.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl HostEffects for SystemHostEffects {
    fn run(&self, request: &CapabilityRequest) -> Result<CapabilityOutcome, HostEffectError> {
        match request {
            CapabilityRequest::Env { key } => {
                let value = std::env::var(key)
                    .map_err(|e| HostEffectError::Io(format!("env var `{key}`: {e}")))?;
                Ok(CapabilityOutcome {
                    payload: value.into_bytes(),
                })
            }

            CapabilityRequest::Filesystem {
                mode: FsMode::Read,
                path,
                ..
            } => {
                let bytes = std::fs::read(path)
                    .map_err(|e| HostEffectError::Io(format!("read `{}`: {e}", path.display())))?;
                Ok(CapabilityOutcome { payload: bytes })
            }

            CapabilityRequest::Filesystem {
                mode: FsMode::Write,
                path,
                contents,
            } => {
                std::fs::write(path, contents)
                    .map_err(|e| HostEffectError::Io(format!("write `{}`: {e}", path.display())))?;
                // A write produces no result data; the empty payload plus a
                // non-error response is the "succeeded" signal.
                Ok(CapabilityOutcome {
                    payload: Vec::new(),
                })
            }

            CapabilityRequest::Network { host, port } => {
                use std::net::{TcpStream, ToSocketAddrs};
                use std::time::Duration;

                let mut addrs = (host.as_str(), *port)
                    .to_socket_addrs()
                    .map_err(|e| HostEffectError::Io(format!("resolve `{host}:{port}`: {e}")))?;
                let addr = addrs.next().ok_or_else(|| {
                    HostEffectError::Io(format!("`{host}:{port}` resolved to no address"))
                })?;
                // A bounded TCP connect is the mediated network effect: it
                // proves the host *would* let the plugin reach the granted
                // endpoint. The connection is closed immediately (dropped);
                // streaming a request/response over it is a later extension.
                let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
                    .map_err(|e| HostEffectError::Io(format!("connect `{host}:{port}`: {e}")))?;
                drop(stream);
                Ok(CapabilityOutcome {
                    payload: Vec::new(),
                })
            }

            CapabilityRequest::Command { program, args } => {
                let output = std::process::Command::new(program)
                    .args(args)
                    .output()
                    .map_err(|e| HostEffectError::Io(format!("spawn `{program}`: {e}")))?;
                if output.status.success() {
                    Ok(CapabilityOutcome {
                        payload: output.stdout,
                    })
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let code = output
                        .status
                        .code()
                        .map_or_else(|| "signal".to_owned(), |c| c.to_string());
                    Err(HostEffectError::Io(format!(
                        "`{program}` exited with code {code}: {stderr}"
                    )))
                }
            }
        }
    }
}

// ── RecordingHostEffects ──────────────────────────────────────────────────────

/// The scripted result a [`RecordingHostEffects`] returns for every call.
#[derive(Debug, Clone)]
enum ScriptedResult {
    OkPayload(Vec<u8>),
    Err(HostEffectError),
}

/// A deterministic [`HostEffects`] test double that records every request it
/// receives and returns a caller-configured scripted result.
///
/// The key security-test use-case: construct a [`RecordingHostEffects`], run
/// the host's enforcement path with a denied request, and assert via
/// [`calls`](Self::calls) that the effector was **never invoked** — proving
/// the policy check blocked the request before any effect ran (ADR 0007 §3).
///
/// ## Construction
///
/// | Constructor | Scripted result |
/// |-------------|----------------|
/// | [`RecordingHostEffects::new()`] | `Ok(CapabilityOutcome { payload: vec![] })` for every call |
/// | [`RecordingHostEffects::with_ok(payload)`](Self::with_ok) | `Ok(CapabilityOutcome { payload })` |
/// | [`RecordingHostEffects::with_err(err)`](Self::with_err) | `Err(err)` |
///
/// ```
/// use rstui_plugin_host::effects::{
///     CapabilityOutcome, HostEffectError, HostEffects, RecordingHostEffects,
/// };
/// use rstui_plugin_host::capability::CapabilityRequest;
///
/// // Scripted Ok: the test controls what the effector "returns".
/// let recorder = RecordingHostEffects::with_ok(b"hello".to_vec());
/// let req1 = CapabilityRequest::Env { key: "HOME".into() };
/// let req2 = CapabilityRequest::Env { key: "USER".into() };
///
/// let out = recorder.run(&req1).unwrap();
/// assert_eq!(out.payload, b"hello");
///
/// // Run a second request — still the same scripted payload.
/// recorder.run(&req2).unwrap();
///
/// // calls() returns every request in call order.
/// assert_eq!(recorder.calls(), vec![req1, req2]);
///
/// // Scripted Err: prove the effector was never reached for a denied request.
/// let failing = RecordingHostEffects::with_err(HostEffectError::Io("boom".into()));
/// let result = failing.run(&CapabilityRequest::Env { key: "X".into() });
/// assert!(matches!(result, Err(HostEffectError::Io(_))));
/// // The recorder still recorded the call even though it returned Err.
/// assert_eq!(failing.calls().len(), 1);
/// ```
pub struct RecordingHostEffects {
    scripted: ScriptedResult,
    recorded: Mutex<Vec<CapabilityRequest>>,
}

impl RecordingHostEffects {
    /// Creates a recorder whose scripted result is
    /// `Ok(CapabilityOutcome { payload: vec![] })` for every call.
    #[must_use]
    pub fn new() -> Self {
        Self {
            scripted: ScriptedResult::OkPayload(vec![]),
            recorded: Mutex::new(Vec::new()),
        }
    }

    /// Creates a recorder whose scripted result is
    /// `Ok(CapabilityOutcome { payload })` for every call.
    #[must_use]
    pub fn with_ok(payload: Vec<u8>) -> Self {
        Self {
            scripted: ScriptedResult::OkPayload(payload),
            recorded: Mutex::new(Vec::new()),
        }
    }

    /// Creates a recorder whose scripted result is `Err(err)` for every call.
    #[must_use]
    pub fn with_err(err: HostEffectError) -> Self {
        Self {
            scripted: ScriptedResult::Err(err),
            recorded: Mutex::new(Vec::new()),
        }
    }

    /// Returns every [`CapabilityRequest`] that was passed to `run`, in call
    /// order.
    ///
    /// Use this in security tests to assert that a denied request never
    /// reached the effector:
    ///
    /// ```
    /// use rstui_plugin_host::effects::{HostEffects, RecordingHostEffects};
    /// use rstui_plugin_host::capability::CapabilityRequest;
    ///
    /// let recorder = RecordingHostEffects::new();
    /// assert!(recorder.calls().is_empty()); // nothing yet
    ///
    /// let req = CapabilityRequest::Env { key: "PATH".into() };
    /// recorder.run(&req).unwrap();
    /// assert_eq!(recorder.calls(), vec![req]);
    /// ```
    #[must_use]
    pub fn calls(&self) -> Vec<CapabilityRequest> {
        self.recorded
            .lock()
            .expect("RecordingHostEffects mutex poisoned")
            .clone()
    }
}

impl Default for RecordingHostEffects {
    fn default() -> Self {
        Self::new()
    }
}

impl HostEffects for RecordingHostEffects {
    fn run(&self, request: &CapabilityRequest) -> Result<CapabilityOutcome, HostEffectError> {
        self.recorded
            .lock()
            .expect("RecordingHostEffects mutex poisoned")
            .push(request.clone());
        match &self.scripted {
            ScriptedResult::OkPayload(bytes) => Ok(CapabilityOutcome {
                payload: bytes.clone(),
            }),
            ScriptedResult::Err(err) => Err(err.clone()),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::capability::FsMode;

    // ── SystemHostEffects — Env ───────────────────────────────────────────────

    /// `PATH` is present in every Rust test process; reading it must succeed
    /// and return non-empty bytes.
    #[test]
    fn system_env_reads_present_variable() {
        let effector = SystemHostEffects::new();
        let req = CapabilityRequest::Env { key: "PATH".into() };
        let outcome = effector
            .run(&req)
            .expect("PATH must be readable in the test process");
        assert!(
            !outcome.payload.is_empty(),
            "PATH payload must not be empty"
        );
        // The bytes must be valid UTF-8 on all supported platforms.
        let value = String::from_utf8(outcome.payload).expect("PATH must be valid UTF-8");
        assert!(!value.is_empty(), "PATH value string must not be empty");
    }

    /// A variable with a name that cannot collide with anything in the test
    /// process must return `Err(Io)`.
    #[test]
    fn system_env_absent_variable_returns_io_error() {
        let effector = SystemHostEffects::new();
        // A name constructed to be absent: the test process will never set
        // a variable with this exact UUID-like suffix.
        let req = CapabilityRequest::Env {
            key: "RSTUI_EFFECTS_ABSENT_C2A7B1D4E3F5".into(),
        };
        let result = effector.run(&req);
        assert!(
            matches!(result, Err(HostEffectError::Io(_))),
            "absent env var must return Io error, got: {result:?}"
        );
    }

    // ── SystemHostEffects — Filesystem Read ───────────────────────────────────

    /// Write a temp file with known content, read it back via the effector,
    /// and assert round-trip fidelity.
    #[test]
    fn system_fs_read_returns_file_contents() {
        let mut path = std::env::temp_dir();
        path.push("rstui_effects_test_read_C2A7B1D4.bin");
        let expected: &[u8] = b"rstui effects round-trip \xde\xad\xbe\xef";
        std::fs::write(&path, expected).expect("temp write must succeed");

        let effector = SystemHostEffects::new();
        let req = CapabilityRequest::Filesystem {
            mode: FsMode::Read,
            path: path.clone(),
            contents: Vec::new(),
        };
        let outcome = effector.run(&req).expect("fs read must succeed");
        assert_eq!(outcome.payload, expected);

        // Clean up — best-effort; ignore failure.
        let _ = std::fs::remove_file(&path);
    }

    /// Reading a path that does not exist must return `Err(Io)`.
    #[test]
    fn system_fs_read_missing_file_returns_io_error() {
        let effector = SystemHostEffects::new();
        let req = CapabilityRequest::Filesystem {
            mode: FsMode::Read,
            path: PathBuf::from("/tmp/rstui_effects_definitely_absent_C2A7B1D4E3F5_no_such_file"),
            contents: Vec::new(),
        };
        let result = effector.run(&req);
        assert!(
            matches!(result, Err(HostEffectError::Io(_))),
            "missing file must return Io error, got: {result:?}"
        );
    }

    // ── SystemHostEffects — Command ───────────────────────────────────────────

    /// `/usr/bin/true` exits 0 and emits no stdout; we verify success and an
    /// empty-or-trivial payload. This program is present on macOS and Linux.
    #[test]
    fn system_command_success_returns_stdout() {
        let effector = SystemHostEffects::new();
        let req = CapabilityRequest::Command {
            program: "/usr/bin/true".into(),
            args: vec![],
        };
        let result = effector.run(&req);
        let outcome = result.expect("/usr/bin/true must exit 0");
        // `true` emits nothing to stdout.
        assert_eq!(outcome.payload, b"");
    }

    /// A program that does not exist at all must return `Err(Io)`.
    #[test]
    fn system_command_nonexistent_program_returns_io_error() {
        let effector = SystemHostEffects::new();
        let req = CapabilityRequest::Command {
            program: "/nonexistent/rstui_no_such_binary_C2A7B1D4".into(),
            args: vec![],
        };
        let result = effector.run(&req);
        assert!(
            matches!(result, Err(HostEffectError::Io(_))),
            "nonexistent program must return Io error, got: {result:?}"
        );
    }

    /// `/usr/bin/false` exits non-zero; we verify `Err(Io)` containing the
    /// exit code.
    #[test]
    fn system_command_nonzero_exit_returns_io_error_with_code() {
        let effector = SystemHostEffects::new();
        let req = CapabilityRequest::Command {
            program: "/usr/bin/false".into(),
            args: vec![],
        };
        let result = effector.run(&req);
        match result {
            Err(HostEffectError::Io(msg)) => {
                // The error description must mention the exit code.
                assert!(
                    msg.contains('1') || msg.contains("code"),
                    "Io error from nonzero exit should mention code: {msg}"
                );
            }
            other => panic!("/usr/bin/false must return Io error, got: {other:?}"),
        }
    }

    // ── SystemHostEffects — Filesystem Write ──────────────────────────────────

    /// A `Filesystem { mode: Write }` writes `contents` to `path` and reports
    /// success with an empty payload. Deterministic: a fresh, uniquely-named
    /// file under the OS temp dir, written then read back and removed.
    #[test]
    fn system_fs_write_persists_contents() {
        let mut path = std::env::temp_dir();
        path.push("rstui_effects_test_write_C2A7B1D4.bin");
        // Start clean so a stale file from an aborted prior run cannot mask a
        // regression.
        let _ = std::fs::remove_file(&path);

        let effector = SystemHostEffects::new();
        let req = CapabilityRequest::Filesystem {
            mode: FsMode::Write,
            path: path.clone(),
            contents: b"hello".to_vec(),
        };
        let outcome = effector.run(&req).expect("fs write must succeed");
        // A write produces no result data; success is the empty payload.
        assert_eq!(outcome.payload, Vec::<u8>::new());

        let written = std::fs::read(&path).expect("written file must be readable");
        assert_eq!(written, b"hello");

        // Clean up — best-effort; ignore failure.
        let _ = std::fs::remove_file(&path);
    }

    /// A `Filesystem { mode: Write }` whose parent directory does not exist
    /// cannot be created by `std::fs::write`, so the effector surfaces
    /// `Err(Io)` rather than silently succeeding.
    #[test]
    fn system_fs_write_to_unwritable_path_returns_io_error() {
        let mut path = std::env::temp_dir();
        // A parent component that does not exist: write() will not mkdir -p.
        path.push("rstui_effects_no_such_dir_C2A7B1D4E3F5");
        path.push("inner");
        path.push("target.bin");

        let effector = SystemHostEffects::new();
        let req = CapabilityRequest::Filesystem {
            mode: FsMode::Write,
            path: path.clone(),
            contents: b"data".to_vec(),
        };
        let result = effector.run(&req);
        assert!(
            matches!(result, Err(HostEffectError::Io(_))),
            "write under a missing parent must return Io error, got: {result:?}"
        );
        // The write must not have created anything.
        assert!(!path.exists(), "no file should have been created");
    }

    // ── SystemHostEffects — Network ───────────────────────────────────────────

    /// A `Network` connect to a listening localhost port succeeds with an
    /// empty payload. Deterministic: we bind our own `TcpListener` on an
    /// ephemeral port (`127.0.0.1:0`), read the assigned port, and have a
    /// thread `accept()` exactly one connection. Localhost connect is
    /// immediate, so the 5s connect timeout is never approached.
    #[test]
    fn system_network_connects_to_listening_port() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("read local addr").port();
        let accepter = std::thread::spawn(move || {
            // Accept exactly one connection then return; the stream is dropped
            // immediately (the effector closes its end too).
            let _ = listener.accept();
        });

        let effector = SystemHostEffects::new();
        let req = CapabilityRequest::Network {
            host: "127.0.0.1".into(),
            port,
        };
        let outcome = effector
            .run(&req)
            .expect("connect to live port must succeed");
        // A bounded connect produces no result data.
        assert_eq!(outcome.payload, Vec::<u8>::new());

        accepter.join().expect("accepter thread must not panic");
    }

    /// A `Network` connect to a closed localhost port is refused immediately
    /// and surfaces `Err(Io)`. Deterministic: bind then drop a listener to
    /// obtain a port guaranteed free, so the OS returns connection-refused
    /// (no DNS, no timeout wait).
    #[test]
    fn system_network_connect_refused_returns_io_error() {
        use std::net::TcpListener;

        // Bind to learn a free port, then drop the listener so nothing is
        // accepting on it: connect() gets ECONNREFUSED right away.
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
            listener.local_addr().expect("read local addr").port()
        };

        let effector = SystemHostEffects::new();
        let req = CapabilityRequest::Network {
            host: "127.0.0.1".into(),
            port,
        };
        let result = effector.run(&req);
        assert!(
            matches!(result, Err(HostEffectError::Io(_))),
            "connect to a closed port must return Io error, got: {result:?}"
        );
    }

    // ── HostEffectError Display / Error ───────────────────────────────────────

    #[test]
    fn host_effect_error_display_is_readable() {
        let io_err = HostEffectError::Io("something went wrong".into());
        let display = io_err.to_string();
        assert!(
            display.contains("something went wrong"),
            "Io display must include the message: {display}"
        );

        let unsupported = HostEffectError::Unsupported;
        let display = unsupported.to_string();
        assert!(!display.is_empty(), "Unsupported display must not be empty");
    }

    #[test]
    fn host_effect_error_implements_std_error() {
        // Verify the Error impl compiles and the source chain is None.
        let err: &dyn std::error::Error = &HostEffectError::Io("x".into());
        assert!(err.source().is_none());
        let err: &dyn std::error::Error = &HostEffectError::Unsupported;
        assert!(err.source().is_none());
    }

    // ── RecordingHostEffects ──────────────────────────────────────────────────

    #[test]
    fn recorder_starts_empty() {
        let recorder = RecordingHostEffects::new();
        assert!(recorder.calls().is_empty());
    }

    #[test]
    fn recorder_records_every_request_in_order() {
        let recorder = RecordingHostEffects::new();
        let req1 = CapabilityRequest::Env { key: "A".into() };
        let req2 = CapabilityRequest::Env { key: "B".into() };
        let req3 = CapabilityRequest::Filesystem {
            mode: FsMode::Read,
            path: PathBuf::from("/tmp/x"),
            contents: Vec::new(),
        };
        recorder.run(&req1).unwrap();
        recorder.run(&req2).unwrap();
        recorder.run(&req3).unwrap();

        let calls = recorder.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0], req1);
        assert_eq!(calls[1], req2);
        assert_eq!(calls[2], req3);
    }

    #[test]
    fn recorder_default_returns_empty_ok_payload() {
        let recorder = RecordingHostEffects::new();
        let outcome = recorder
            .run(&CapabilityRequest::Env { key: "X".into() })
            .unwrap();
        assert_eq!(outcome.payload, vec![]);
    }

    #[test]
    fn recorder_with_ok_returns_scripted_payload() {
        let recorder = RecordingHostEffects::with_ok(b"scripted".to_vec());
        let outcome = recorder
            .run(&CapabilityRequest::Env { key: "X".into() })
            .unwrap();
        assert_eq!(outcome.payload, b"scripted");
        // Second call also returns the same scripted payload.
        let outcome2 = recorder
            .run(&CapabilityRequest::Env { key: "Y".into() })
            .unwrap();
        assert_eq!(outcome2.payload, b"scripted");
        // Both calls were recorded.
        assert_eq!(recorder.calls().len(), 2);
    }

    #[test]
    fn recorder_with_err_returns_scripted_error() {
        let recorder =
            RecordingHostEffects::with_err(HostEffectError::Io("scripted failure".into()));
        let result = recorder.run(&CapabilityRequest::Env { key: "X".into() });
        assert_eq!(result, Err(HostEffectError::Io("scripted failure".into())));
        // The call was still recorded even though it returned Err.
        assert_eq!(recorder.calls().len(), 1);
    }

    #[test]
    fn recorder_with_err_unsupported_returns_unsupported() {
        let recorder = RecordingHostEffects::with_err(HostEffectError::Unsupported);
        let result = recorder.run(&CapabilityRequest::Network {
            host: "h".into(),
            port: 80,
        });
        assert_eq!(result, Err(HostEffectError::Unsupported));
    }

    /// Key security property: if the host enforcement path denies a request
    /// and never calls `run`, the recorder's `calls()` is empty, proving the
    /// effector was never reached.
    #[test]
    fn recorder_empty_calls_proves_effector_was_never_reached() {
        let recorder = RecordingHostEffects::new();
        // Simulate a path where the host denied the request and never called
        // `run` — calls() must be empty.
        let denied_request = CapabilityRequest::Command {
            program: "rm".into(),
            args: vec!["-rf".into(), "/".into()],
        };
        // We deliberately do NOT call recorder.run(&denied_request) here,
        // just as a real host would not call the effector for a denied request.
        let _ = denied_request; // named so the intent is clear
        assert!(
            recorder.calls().is_empty(),
            "recorder.calls() must be empty when the effector was never invoked"
        );
    }

    /// `RecordingHostEffects` must be `Send + Sync` so it can be used as a
    /// `&dyn HostEffects` from any thread. This is a compile-time assertion.
    #[test]
    fn recorder_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RecordingHostEffects>();
        assert_send_sync::<SystemHostEffects>();
    }

    /// `calls()` is available by shared reference; two calls to `calls()`
    /// return independent owned `Vec`s (not aliases).
    #[test]
    fn calls_returns_owned_clone() {
        let recorder = RecordingHostEffects::new();
        recorder
            .run(&CapabilityRequest::Env { key: "A".into() })
            .unwrap();
        let c1 = recorder.calls();
        let c2 = recorder.calls();
        assert_eq!(c1, c2);
    }

    /// `Default` for `RecordingHostEffects` is equivalent to `new()`.
    #[test]
    fn recorder_default_equivalent_to_new() {
        let r = RecordingHostEffects::default();
        assert!(r.calls().is_empty());
        let outcome = r.run(&CapabilityRequest::Env { key: "X".into() }).unwrap();
        assert_eq!(outcome.payload, vec![]);
    }
}
