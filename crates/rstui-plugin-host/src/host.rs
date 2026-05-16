//! The `PluginHost`: spawn a plugin process, run the Initialize handshake,
//! and **mediate every capability call through the policy before any
//! effect** (ADR 0007 §3/§5/§6).
//!
//! This is where the crate's other modules compose into the security
//! boundary. For one plugin run the host:
//!
//! 1. gates the manifest's `api_version` against its own
//!    ([`HostError::ApiVersionMismatch`] — incompatible plugins do not
//!    spawn, mirroring opencode's `engines.opencode` refusal);
//! 2. builds a [`PluginSpawnSpec`] whose env is **only** the manifest's
//!    declared `env` keys resolved from the host environment (the
//!    `env_clear`-then-allowlist model, ADR 0007 §2) and spawns it through
//!    the injected [`ProcessRunner`];
//! 3. performs the [`Initialize`](MessageType::Initialize) →
//!    [`Ready`](MessageType::Ready) handshake;
//! 4. runs the **mediation loop**: every
//!    [`CapabilityCall`](MessageType::CapabilityCall) frame is decoded,
//!    its path *canonicalised by the host* (ADR 0007 §3 — a relative
//!    filesystem path is resolved against the plugin's cwd and
//!    [`normalize_lexical`]-d, so `..` cannot smuggle past a grant), then
//!    run through the [`PermissionPolicy`]. **Only on
//!    [`Decision::Allow`] is [`HostEffects`] invoked**; a
//!    [`Decision::Deny`] returns a [`CapabilityResponse::Denied`] and the
//!    effect is never reached. Either way the host writes a
//!    [`CapabilityResponse`](MessageType::CapabilityResponse) frame echoing
//!    the call's correlation id.
//!
//! Every nondeterministic edge is injected ([`ProcessRunner`],
//! [`PermissionPolicy`], [`HostEffects`], [`Clock`]), so a denied
//! capability, a malformed frame, a misdirected frame, a deterministic
//! timeout, and a plugin crash are each an ordinary unit test with no real
//! process, socket, or wall clock — the `rstui-runtime` `Harness` standard
//! applied to security. The host observes a run as a [`PluginRunReport`]:
//! the ordered [`MediationRecord`]s (request, decision, response), the
//! plugin's log lines, and its [`ExitOutcome`] — a decoupled integration
//! surface a runtime can map to messages without this crate knowing
//! anything about widgets or the event loop.

use std::fmt;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use crate::capability::{CapabilityRequest, normalize_lexical};
use crate::clock::Clock;
use crate::effects::HostEffects;
use crate::hook::{HookKind, HookOutcome};
use crate::manifest::PluginManifest;
use crate::message::{
    CapabilityResponse, MessageError, decode_hook_result, decode_request, encode_hook_dispatch,
    encode_request, encode_response,
};
use crate::permission::{Decision, PermissionPolicy};
use crate::process::{ExitOutcome, PluginProcess, PluginSpawnSpec, ProcessRunner};
use crate::protocol::{Frame, MessageType, ProtocolError, read_frame, write_frame};

/// A plugin's identity, taken from its manifest `name`.
///
/// Carried by every [`HostError`] and [`PluginRunReport`] so a failure or
/// an audit record is always attributable to the originating plugin (ADR
/// 0007 §6: per-plugin error attribution).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginId(String);

impl PluginId {
    /// The plugin name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the host did with one plugin capability call: the
/// (host-canonicalised) request, the policy [`Decision`], and the
/// [`CapabilityResponse`] sent back.
///
/// The ordered list of these on a [`PluginRunReport`] is the auditable
/// record of exactly what authority a plugin exercised — and, by the
/// absence of a record, what it was refused before any effect ran.
///
/// `decision` is always the *policy's* ruling. A `BeforeCapability` hook
/// (ADR 0007 §6) can turn an otherwise-permitted call into a denial:
/// in that case `decision` is [`Decision::Allow`] (the policy permitted
/// it) but `response` is [`CapabilityResponse::Denied`] with a
/// `vetoed by before_capability hook:` reason — and the effect did not
/// run. A hook can only narrow this way, never the reverse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediationRecord {
    /// The request *after* host canonicalisation (filesystem paths are
    /// absolute and lexically normalised — what the policy actually judged).
    pub request: CapabilityRequest,
    /// The policy's ruling.
    pub decision: Decision,
    /// The response the host returned to the plugin.
    pub response: CapabilityResponse,
}

/// Everything the host observed during one plugin run — the decoupled
/// integration surface (ADR 0007 §6: host knows nothing about widgets or
/// the runtime; a caller maps this to messages).
#[derive(Debug)]
pub struct PluginRunReport {
    /// Which plugin this run was.
    pub plugin: PluginId,
    /// Every mediated capability call, in the order the plugin made them.
    pub mediated: Vec<MediationRecord>,
    /// Plugin diagnostic log lines (from [`Log`](MessageType::Log) frames),
    /// decoded lossily from UTF-8.
    pub logs: Vec<String>,
    /// How the plugin process exited.
    pub exit: ExitOutcome,
}

/// A plugin run that could not complete, always attributed to a
/// [`PluginId`].
#[derive(Debug)]
pub enum HostError {
    /// The manifest targets a host-protocol version this host does not
    /// implement; the plugin was never spawned.
    ApiVersionMismatch {
        /// The plugin that was refused.
        plugin: PluginId,
        /// The `api_version` this host implements.
        expected: String,
        /// The `api_version` the manifest declared.
        found: String,
    },
    /// The process could not be spawned.
    Spawn {
        /// The plugin that failed to spawn.
        plugin: PluginId,
        /// The underlying spawn error.
        source: std::io::Error,
    },
    /// The Initialize→Ready handshake did not complete as required.
    Handshake {
        /// The plugin whose handshake failed.
        plugin: PluginId,
        /// What went wrong.
        detail: String,
    },
    /// A framing/codec error on the wire; the connection was terminated
    /// fail-closed (ADR 0007 §4) and the process killed.
    Protocol {
        /// The plugin whose stream was malformed.
        plugin: PluginId,
        /// The framing error.
        source: ProtocolError,
    },
    /// A capability-call payload could not be decoded; terminated
    /// fail-closed and the process killed.
    MalformedCall {
        /// The plugin that sent the bad payload.
        plugin: PluginId,
        /// The payload decode error.
        source: MessageError,
    },
    /// The plugin sent a frame whose type is not valid plugin→host at this
    /// point in the protocol; terminated fail-closed and the process
    /// killed.
    MisdirectedFrame {
        /// The plugin that sent it.
        plugin: PluginId,
        /// The offending message type.
        got: MessageType,
    },
    /// The plugin exceeded its time budget; it was asked to shut down and
    /// then killed (ADR 0007 §6 cooperative-then-forced).
    TimedOut {
        /// The plugin that ran out of time.
        plugin: PluginId,
        /// The budget that was exceeded.
        budget: Duration,
    },
}

impl HostError {
    /// The plugin this error is attributed to.
    #[must_use]
    pub fn plugin(&self) -> &PluginId {
        match self {
            Self::ApiVersionMismatch { plugin, .. }
            | Self::Spawn { plugin, .. }
            | Self::Handshake { plugin, .. }
            | Self::Protocol { plugin, .. }
            | Self::MalformedCall { plugin, .. }
            | Self::MisdirectedFrame { plugin, .. }
            | Self::TimedOut { plugin, .. } => plugin,
        }
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiVersionMismatch {
                plugin,
                expected,
                found,
            } => write!(
                f,
                "plugin `{plugin}`: api_version `{found}` is incompatible with host `{expected}`"
            ),
            Self::Spawn { plugin, source } => {
                write!(f, "plugin `{plugin}`: could not spawn: {source}")
            }
            Self::Handshake { plugin, detail } => {
                write!(f, "plugin `{plugin}`: handshake failed: {detail}")
            }
            Self::Protocol { plugin, source } => {
                write!(f, "plugin `{plugin}`: protocol error: {source}")
            }
            Self::MalformedCall { plugin, source } => {
                write!(f, "plugin `{plugin}`: malformed capability call: {source}")
            }
            Self::MisdirectedFrame { plugin, got } => {
                write!(f, "plugin `{plugin}`: misdirected frame {got:?}")
            }
            Self::TimedOut { plugin, budget } => {
                write!(f, "plugin `{plugin}`: timed out after {budget:?}")
            }
        }
    }
}

impl std::error::Error for HostError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } => Some(source),
            Self::Protocol { source, .. } => Some(source),
            Self::MalformedCall { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The permissioned plugin host (ADR 0007). Holds the four injected seams
/// plus the host-protocol version it implements; [`run_plugin`] drives one
/// plugin end to end.
///
/// The seams are `Arc`-shared so a test keeps its own handle to a
/// recording policy / recording effects / fake clock and asserts on them
/// after the run — the `Harness` pattern.
///
/// [`run_plugin`]: PluginHost::run_plugin
pub struct PluginHost {
    runner: Arc<dyn ProcessRunner>,
    policy: Arc<dyn PermissionPolicy>,
    effects: Arc<dyn HostEffects>,
    clock: Arc<dyn Clock>,
    host_api_version: String,
}

impl PluginHost {
    /// Build a host from the injected seams and the concrete host-protocol
    /// version it implements (e.g. `"1.0.0"`). Each manifest's
    /// `api_version` is a [`VersionReq`](crate::version::VersionReq) the
    /// host's version must satisfy (ADR 0007 §2, opencode-style); an
    /// unparseable or unsatisfied requirement refuses the spawn,
    /// fail-closed.
    #[must_use]
    pub fn new(
        runner: Arc<dyn ProcessRunner>,
        policy: Arc<dyn PermissionPolicy>,
        effects: Arc<dyn HostEffects>,
        clock: Arc<dyn Clock>,
        host_api_version: impl Into<String>,
    ) -> Self {
        Self {
            runner,
            policy,
            effects,
            clock,
            host_api_version: host_api_version.into(),
        }
    }

    /// Spawn `manifest`'s plugin with working directory `cwd`, mediate it
    /// until it ends, and return what was observed — or a [`HostError`]
    /// attributed to the plugin.
    ///
    /// `timeout` bounds the whole run and is enforced *both* between
    /// frames and while the host is blocked waiting for one — a plugin
    /// that opens a read and stalls is killed at the deadline, not left to
    /// hang the host (ADR 0007 §6). `cwd` is the base a *relative*
    /// filesystem request path is resolved against before the policy sees
    /// it (ADR 0007 §3).
    ///
    /// # Errors
    ///
    /// Returns [`HostError`] on api-version mismatch, spawn failure, a
    /// failed handshake, a framing/payload decode error or misdirected
    /// frame (each terminating the connection fail-closed and killing the
    /// process), or a timeout.
    pub fn run_plugin(
        &self,
        manifest: &PluginManifest,
        cwd: &Path,
        timeout: Duration,
    ) -> Result<PluginRunReport, HostError> {
        let plugin = PluginId(manifest.name.clone());

        // Fail-closed: an unsatisfied OR unparseable requirement refuses the
        // spawn (ADR 0007 §2 — a requirement the host cannot understand is
        // never optimistically allowed).
        if crate::version::is_compatible(&self.host_api_version, &manifest.api_version) != Ok(true)
        {
            return Err(HostError::ApiVersionMismatch {
                plugin,
                expected: self.host_api_version.clone(),
                found: manifest.api_version.clone(),
            });
        }

        let spec = PluginSpawnSpec {
            program: manifest.entry.clone(),
            args: Vec::new(),
            cwd: cwd.to_path_buf(),
            env: resolved_env(manifest),
        };
        let mut process = self
            .runner
            .spawn(&spec)
            .map_err(|source| HostError::Spawn {
                plugin: plugin.clone(),
                source,
            })?;

        let deadline = RunDeadline {
            started: self.clock.elapsed(),
            timeout,
        };

        // Move stdout onto a dedicated reader thread so a plugin that
        // stalls *mid-frame* is still bounded by the deadline, not just one
        // that stalls between frames (ADR 0007 §6). Safe `std` cannot put a
        // read timeout on a pipe without the `unsafe` libc the workspace
        // forbids; a reader thread feeding a channel the host drains with
        // `recv_timeout` is the mechanism.
        let Some(stdout) = process.take_stdout() else {
            let _ = process.kill();
            return Err(HostError::Handshake {
                plugin,
                detail: "process exposed no stdout pipe".to_string(),
            });
        };
        let channel = FrameChannel::spawn(stdout);

        if let Err(err) = self.handshake(plugin.clone(), process.as_mut(), &channel, deadline) {
            let _ = process.request_shutdown();
            let _ = process.kill();
            return Err(err);
        }

        // SessionStart (ADR 0007 §6): an Observe hook — a one-way
        // notification the host does not await a reply for (the reply is
        // ignored *by definition*, so requiring one would only add a
        // deadlock class). A write failure here is a genuine pipe fault,
        // handled fail-closed like any other.
        if manifest.hooks.contains(&HookKind::SessionStart) {
            if let Err(err) =
                self.notify_observe_hook(&plugin, process.as_mut(), HookKind::SessionStart)
            {
                let _ = process.kill();
                return Err(err);
            }
        }

        // `channel` stays owned here; it is dropped the instant `run_plugin`
        // returns (right after `mediate`), which ends the reader thread (its
        // `send` fails once the receiver is gone, and a killed process's
        // closed pipe unblocks a blocked `read`).
        self.mediate(plugin, cwd, deadline, process, &channel, &manifest.hooks)
    }

    /// Write `Initialize`, then expect `Ready` within the deadline. Any
    /// deviation — wrong frame, closed stream, or timeout — is fatal.
    fn handshake(
        &self,
        plugin: PluginId,
        process: &mut dyn PluginProcess,
        channel: &FrameChannel,
        deadline: RunDeadline,
    ) -> Result<(), HostError> {
        let init = Frame::new(
            MessageType::Initialize,
            correlation_id(0),
            self.host_api_version.as_bytes().to_vec(),
        );
        write_frame(&mut process.stdin(), &init).map_err(|source| HostError::Protocol {
            plugin: plugin.clone(),
            source,
        })?;

        match channel.recv(self.clock.as_ref(), deadline) {
            Recv::Frame(frame) => match frame.message_type {
                MessageType::Ready => Ok(()),
                other => Err(HostError::Handshake {
                    plugin,
                    detail: format!("expected Ready, got {other:?}"),
                }),
            },
            Recv::Closed => Err(HostError::Handshake {
                plugin,
                detail: "stream closed before Ready".to_string(),
            }),
            Recv::TimedOut => Err(HostError::TimedOut {
                plugin,
                budget: deadline.timeout,
            }),
        }
    }

    /// Dispatch an **Observe** hook (`SessionStart`/`SessionEnd`): a one-way
    /// notification. The host does *not* await a reply — an Observe reply is
    /// ignored by definition (ADR 0007 §6 / [`crate::hook`]), so requiring
    /// one would add nothing but a deadlock class. A write fault is still a
    /// real protocol failure surfaced to the caller (callers on the
    /// best-effort end-of-run path ignore it).
    fn notify_observe_hook(
        &self,
        plugin: &PluginId,
        process: &mut dyn PluginProcess,
        kind: HookKind,
    ) -> Result<(), HostError> {
        debug_assert!(matches!(
            kind.reduction(),
            crate::hook::HookReduction::Observe
        ));
        let frame = Frame::new(
            MessageType::HookDispatch,
            correlation_id(0),
            encode_hook_dispatch(kind, &[]),
        );
        write_frame(&mut process.stdin(), &frame).map_err(|source| HostError::Protocol {
            plugin: plugin.clone(),
            source,
        })
    }

    /// Dispatch a **VetoChain** hook (`BeforeCapability`) and await its
    /// [`HookOutcome`] within the deadline.
    ///
    /// Strict request/reply: the very next frame *must* be the matching
    /// [`HookResult`](MessageType::HookResult) (the SDK guarantees a plugin
    /// services a dispatch before sending anything else). A wrong type, a
    /// mismatched correlation id, an undecodable payload, a closed stream,
    /// or a timeout is fail-closed — the process is killed and the error
    /// surfaced (ADR 0007 §4/§6). A veto can only *narrow* (the caller only
    /// reaches here on the policy-Allow path).
    fn dispatch_veto_hook(
        &self,
        plugin: &PluginId,
        process: &mut dyn PluginProcess,
        channel: &FrameChannel,
        input: &[u8],
        id: u64,
        deadline: RunDeadline,
    ) -> Result<HookOutcome, HostError> {
        let corr = correlation_id(id);
        let frame = Frame::new(
            MessageType::HookDispatch,
            corr,
            encode_hook_dispatch(HookKind::BeforeCapability, input),
        );
        if let Err(source) = write_frame(&mut process.stdin(), &frame) {
            let _ = process.kill();
            return Err(HostError::Protocol {
                plugin: plugin.clone(),
                source,
            });
        }
        match channel.recv(self.clock.as_ref(), deadline) {
            Recv::Frame(reply)
                if reply.message_type == MessageType::HookResult
                    && reply.correlation_id == corr =>
            {
                match decode_hook_result(&reply.payload) {
                    Ok(outcome) => Ok(outcome),
                    Err(source) => {
                        let _ = process.kill();
                        Err(HostError::MalformedCall {
                            plugin: plugin.clone(),
                            source,
                        })
                    }
                }
            }
            Recv::Frame(reply) => {
                let _ = process.kill();
                Err(HostError::MisdirectedFrame {
                    plugin: plugin.clone(),
                    got: reply.message_type,
                })
            }
            Recv::Closed => {
                let _ = process.kill();
                Err(HostError::Handshake {
                    plugin: plugin.clone(),
                    detail: "stream closed awaiting before_capability HookResult".to_string(),
                })
            }
            Recv::TimedOut => {
                let _ = process.request_shutdown();
                let _ = process.kill();
                Err(HostError::TimedOut {
                    plugin: plugin.clone(),
                    budget: deadline.timeout,
                })
            }
        }
    }

    /// The mediation loop. The run is abandoned once the [`RunDeadline`]
    /// elapses — enforced *both* between frames and while blocked waiting
    /// for the next one (a stalled read cannot outlive the budget).
    fn mediate(
        &self,
        plugin: PluginId,
        cwd: &Path,
        deadline: RunDeadline,
        mut process: Box<dyn PluginProcess>,
        channel: &FrameChannel,
        hooks: &[HookKind],
    ) -> Result<PluginRunReport, HostError> {
        let mut mediated = Vec::new();
        let mut logs = Vec::new();
        let before_capability = hooks.contains(&HookKind::BeforeCapability);
        let mut hook_id: u64 = 1;

        loop {
            let frame = match channel.recv(self.clock.as_ref(), deadline) {
                Recv::TimedOut => {
                    // Cooperative-then-forced (ADR 0007 §6). Dropping
                    // `channel` after we return ends the reader thread:
                    // its `send` fails once the receiver is gone, and once
                    // the killed process closes the pipe its `read`
                    // unblocks — so the thread exits, not leaks.
                    let _ = process.request_shutdown();
                    let _ = process.kill();
                    return Err(HostError::TimedOut {
                        plugin,
                        budget: deadline.timeout,
                    });
                }
                Recv::Closed => {
                    // The plugin's stdout ended: the conversation is over
                    // (standard pipe semantics — EOF means the peer closed).
                    // Stopping here *is* fail-closed (ADR 0007 §4): the host
                    // never skips-and-continues past unreadable input, and a
                    // corrupt *frame* was already rejected per-frame
                    // (MalformedCall / MisdirectedFrame) before this point.
                    // Finalise cooperatively-then-forced (ADR 0007 §6).
                    // SessionEnd is Observe + best-effort: the plugin may
                    // already be gone, so a failed write is expected and
                    // ignored — a run that is ending cannot be vetoed.
                    if hooks.contains(&HookKind::SessionEnd) {
                        let _ = self.notify_observe_hook(
                            &plugin,
                            process.as_mut(),
                            HookKind::SessionEnd,
                        );
                    }
                    let _ = process.request_shutdown();
                    let exit = process.wait().unwrap_or(ExitOutcome {
                        code: None,
                        success: false,
                    });
                    return Ok(PluginRunReport {
                        plugin,
                        mediated,
                        logs,
                        exit,
                    });
                }
                Recv::Frame(frame) => frame,
            };

            match frame.message_type {
                MessageType::CapabilityCall => {
                    let request = match decode_request(&frame.payload) {
                        Ok(request) => request,
                        Err(source) => {
                            let _ = process.kill();
                            return Err(HostError::MalformedCall { plugin, source });
                        }
                    };
                    let canonical = canonicalize(cwd, request);
                    let decision = self.policy.check(&canonical);
                    let response = match &decision {
                        Decision::Deny { reason } => CapabilityResponse::Denied {
                            reason: reason.clone(),
                        },
                        Decision::Allow => {
                            // Defense in depth (ADR 0007 §6): a
                            // `BeforeCapability` hook may VETO an
                            // *already-policy-permitted* call. It can only
                            // narrow — control is on the Allow arm; a hook
                            // is never consulted on the Deny path, so it can
                            // never widen a denial into an allow.
                            let outcome = if before_capability {
                                let id = hook_id;
                                hook_id += 1;
                                // On any protocol fault the helper has
                                // already killed the process; `?` surfaces it.
                                self.dispatch_veto_hook(
                                    &plugin,
                                    process.as_mut(),
                                    channel,
                                    &encode_request(&canonical),
                                    id,
                                    deadline,
                                )?
                            } else {
                                HookOutcome::Continue
                            };
                            match outcome {
                                HookOutcome::Veto { reason } => CapabilityResponse::Denied {
                                    reason: format!("vetoed by before_capability hook: {reason}"),
                                },
                                HookOutcome::Continue => match self.effects.run(&canonical) {
                                    Ok(outcome) => CapabilityResponse::Ok {
                                        payload: outcome.payload,
                                    },
                                    Err(error) => CapabilityResponse::Failed {
                                        error: error.to_string(),
                                    },
                                },
                            }
                        }
                    };
                    let reply = Frame::new(
                        MessageType::CapabilityResponse,
                        frame.correlation_id,
                        encode_response(&response),
                    );
                    if let Err(source) = write_frame(&mut process.stdin(), &reply) {
                        let _ = process.kill();
                        return Err(HostError::Protocol { plugin, source });
                    }
                    mediated.push(MediationRecord {
                        request: canonical,
                        decision,
                        response,
                    });
                }
                MessageType::Log => {
                    logs.push(String::from_utf8_lossy(&frame.payload).into_owned());
                }
                // After the handshake, only CapabilityCall and Log are valid
                // plugin→host frames in this slice; anything else (a second
                // Ready, an un-dispatched HookResult, or a host-range type
                // echoed back) is a protocol violation — fail-closed.
                other => {
                    let _ = process.kill();
                    return Err(HostError::MisdirectedFrame { plugin, got: other });
                }
            }
        }
    }
}

/// Resolve the manifest's declared `env` keys from the host environment —
/// the entire environment the child will see (`env_clear` then exactly
/// these, ADR 0007 §2). A declared key absent from the host environment is
/// simply not passed (the plugin gets no ambient leak either way).
fn resolved_env(manifest: &PluginManifest) -> Vec<(String, String)> {
    manifest
        .grants
        .iter()
        .filter_map(|grant| match grant {
            crate::capability::CapabilityGrant::Env { key } => {
                std::env::var(key).ok().map(|value| (key.clone(), value))
            }
            _ => None,
        })
        .collect()
}

/// Host-side canonicalisation (ADR 0007 §3): a *relative* filesystem
/// request path is resolved against the plugin's `cwd`, then every
/// filesystem path is [`normalize_lexical`]-d so the policy judges an
/// absolute, `..`-free path. Non-filesystem requests pass through
/// unchanged.
fn canonicalize(cwd: &Path, request: CapabilityRequest) -> CapabilityRequest {
    match request {
        CapabilityRequest::Filesystem {
            mode,
            path,
            contents,
        } => {
            let absolute = if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            };
            CapabilityRequest::Filesystem {
                mode,
                path: normalize_lexical(&absolute),
                contents,
            }
        }
        other => other,
    }
}

/// A 16-byte correlation id from a counter (first 8 bytes big-endian).
/// Deterministic — the host only originates the Initialize id; capability
/// responses echo the plugin's id verbatim.
fn correlation_id(n: u64) -> [u8; 16] {
    let mut id = [0u8; 16];
    id[..8].copy_from_slice(&n.to_be_bytes());
    id
}

/// What the reader thread hands the host for one read attempt.
enum ReaderItem {
    /// A successfully framed message.
    Frame(Frame),
    /// The stream ended or a framing error occurred — either way the
    /// conversation is over (the host treats both identically: stop,
    /// fail-closed; a corrupt *frame type/payload* is caught later, per
    /// frame).
    Closed,
}

/// The run's time budget: the clock reading at spawn (`started`) and the
/// total `timeout`. Bundled so the deadline travels as one value (and to
/// keep the hook/mediation helpers under the argument-count bar).
#[derive(Debug, Clone, Copy)]
struct RunDeadline {
    started: Duration,
    timeout: Duration,
}

/// The outcome of waiting for the next frame within the remaining budget.
enum Recv {
    /// A frame arrived.
    Frame(Frame),
    /// The plugin's stdout ended (EOF or unreadable).
    Closed,
    /// The deadline elapsed before a frame arrived — including while the
    /// read was *blocked*, which is the whole point of the reader thread.
    TimedOut,
}

/// Owns the plugin's stdout on a dedicated thread so the host can bound
/// every read by the deadline (ADR 0007 §6). The thread loops
/// [`read_frame`] and forwards each result over a bounded channel; the
/// host drains it with [`recv`](FrameChannel::recv), whose wait is capped
/// at the remaining budget. Dropping the `FrameChannel` drops the
/// receiver, so the thread's next `send` fails and it exits (and once a
/// killed process closes the pipe, a blocked `read` unblocks) — it is
/// detached, never joined, so a wedged pipe can never wedge the host.
struct FrameChannel {
    rx: mpsc::Receiver<ReaderItem>,
}

impl FrameChannel {
    fn spawn(reader: Box<dyn Read + Send>) -> Self {
        let (tx, rx) = mpsc::sync_channel::<ReaderItem>(16);
        thread::spawn(move || {
            let mut reader = reader;
            loop {
                let mut borrowed: &mut dyn Read = reader.as_mut();
                match read_frame(&mut borrowed) {
                    Ok(frame) => {
                        if tx.send(ReaderItem::Frame(frame)).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(ReaderItem::Closed);
                        break;
                    }
                }
            }
        });
        Self { rx }
    }

    /// Wait for the next frame, but never longer than the budget left
    /// (`timeout` minus the clock's elapsed-since-`started`). The budget is
    /// measured by the injected [`Clock`] (so the *between-frames* check is
    /// deterministic in tests); the actual block uses the channel's
    /// real-time `recv_timeout`, because a genuinely stalled pipe can only
    /// be escaped in real time — a fake clock cannot unblock a real read.
    fn recv(&self, clock: &dyn Clock, deadline: RunDeadline) -> Recv {
        let elapsed = clock.elapsed().saturating_sub(deadline.started);
        if elapsed >= deadline.timeout {
            return Recv::TimedOut;
        }
        match self.rx.recv_timeout(deadline.timeout - elapsed) {
            Ok(ReaderItem::Frame(frame)) => Recv::Frame(frame),
            Ok(ReaderItem::Closed) => Recv::Closed,
            Err(mpsc::RecvTimeoutError::Timeout) => Recv::TimedOut,
            Err(mpsc::RecvTimeoutError::Disconnected) => Recv::Closed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::FsMode;
    use crate::clock::FakeClock;
    use crate::effects::{HostEffectError, RecordingHostEffects};
    use crate::manifest::PluginManifest;
    use crate::message::{decode_response, encode_hook_result, encode_request};
    use crate::permission::{ManifestPolicy, RecordingPolicy};
    use crate::process::{FakePluginProcess, FakeProcessRunner};
    use crate::protocol::Frame;
    use std::path::PathBuf;
    use std::sync::Mutex;

    const API: &str = "1";

    fn manifest(body: &str) -> PluginManifest {
        PluginManifest::parse(body).expect("manifest parses")
    }

    fn base_manifest(extra: &str) -> PluginManifest {
        manifest(&format!(
            "name = \"demo\"\nversion = \"0.1.0\"\napi_version = \"1\"\nentry = \"bin/demo\"\n{extra}"
        ))
    }

    /// Encode a plugin→host frame sequence into scripted stdout bytes.
    fn script(frames: &[Frame]) -> Vec<u8> {
        let mut out = Vec::new();
        for frame in frames {
            write_frame(&mut out, frame).expect("script frame");
        }
        out
    }

    fn ready() -> Frame {
        Frame::new(MessageType::Ready, [0u8; 16], Vec::new())
    }

    fn call(id: u8, request: &CapabilityRequest) -> Frame {
        let mut cid = [0u8; 16];
        cid[15] = id;
        Frame::new(MessageType::CapabilityCall, cid, encode_request(request))
    }

    fn host(
        runner: Arc<dyn ProcessRunner>,
        policy: Arc<dyn PermissionPolicy>,
        effects: Arc<dyn HostEffects>,
        clock: Arc<dyn Clock>,
    ) -> PluginHost {
        PluginHost::new(runner, policy, effects, clock, API)
    }

    #[test]
    fn allowed_call_reaches_effects_and_returns_ok() {
        let m = base_manifest("[env]\nallow = \"PATH\"\n");
        let req = CapabilityRequest::Env { key: "PATH".into() };
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[ready(), call(1, &req)]),
            Vec::new(),
            ExitOutcome {
                code: Some(0),
                success: true,
            },
        )));
        let policy = Arc::new(RecordingPolicy::new(ManifestPolicy::from_manifest(&m)));
        let effects = Arc::new(RecordingHostEffects::with_ok(b"the-value".to_vec()));
        let clock = Arc::new(FakeClock::new());

        let report = host(runner, policy.clone(), effects.clone(), clock)
            .run_plugin(&m, Path::new("/work"), Duration::from_secs(5))
            .expect("run ok");

        assert_eq!(report.plugin.as_str(), "demo");
        assert_eq!(report.mediated.len(), 1);
        assert_eq!(report.mediated[0].decision, Decision::Allow);
        assert_eq!(
            report.mediated[0].response,
            CapabilityResponse::Ok {
                payload: b"the-value".to_vec()
            }
        );
        // The effect was actually invoked, with exactly the request.
        assert_eq!(effects.calls(), vec![req.clone()]);
        // The policy was consulted for it.
        assert_eq!(policy.records().len(), 1);
    }

    #[test]
    fn denied_call_never_reaches_effects() {
        // Manifest grants nothing => the env read is denied.
        let m = base_manifest("");
        let req = CapabilityRequest::Env {
            key: "SECRET".into(),
        };
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[ready(), call(7, &req)]),
            Vec::new(),
            ExitOutcome {
                code: Some(0),
                success: true,
            },
        )));
        let policy = Arc::new(RecordingPolicy::new(ManifestPolicy::from_manifest(&m)));
        let effects = Arc::new(RecordingHostEffects::new());
        let clock = Arc::new(FakeClock::new());

        let report = host(runner, policy.clone(), effects.clone(), clock)
            .run_plugin(&m, Path::new("/work"), Duration::from_secs(5))
            .expect("run ok");

        assert_eq!(report.mediated.len(), 1);
        match &report.mediated[0].response {
            CapabilityResponse::Denied { reason } => assert!(reason.contains("env")),
            other => panic!("expected Denied, got {other:?}"),
        }
        // The crux of the security model: the host effect was NEVER invoked
        // for a denied request.
        assert!(
            effects.calls().is_empty(),
            "denied request must not reach HostEffects"
        );
        assert_eq!(policy.records().len(), 1, "policy was still consulted");
    }

    #[test]
    fn filesystem_request_is_canonicalised_before_the_policy() {
        // Grant /work/data read; the plugin asks for a relative `..` escape.
        let m = base_manifest("[filesystem]\nread = \"/work/data\"\n");
        let escape = CapabilityRequest::Filesystem {
            mode: FsMode::Read,
            path: PathBuf::from("data/../secret"),
            contents: Vec::new(),
        };
        let inside = CapabilityRequest::Filesystem {
            mode: FsMode::Read,
            path: PathBuf::from("data/notes.txt"),
            contents: Vec::new(),
        };
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[ready(), call(1, &escape), call(2, &inside)]),
            Vec::new(),
            ExitOutcome {
                code: Some(0),
                success: true,
            },
        )));
        let policy = Arc::new(ManifestPolicy::from_manifest(&m));
        let effects = Arc::new(RecordingHostEffects::with_ok(b"ok".to_vec()));
        let clock = Arc::new(FakeClock::new());

        let report = host(runner, policy, effects.clone(), clock)
            .run_plugin(&m, Path::new("/work"), Duration::from_secs(5))
            .expect("run ok");

        // Escape: canonicalised to /work/secret, outside the grant => Denied,
        // effect not reached.
        assert_eq!(
            report.mediated[0].request,
            CapabilityRequest::Filesystem {
                mode: FsMode::Read,
                path: PathBuf::from("/work/secret"),
                contents: Vec::new(),
            }
        );
        assert!(matches!(
            report.mediated[0].response,
            CapabilityResponse::Denied { .. }
        ));
        // In-scope: canonicalised to /work/data/notes.txt => Allowed.
        assert_eq!(
            report.mediated[1].request,
            CapabilityRequest::Filesystem {
                mode: FsMode::Read,
                path: PathBuf::from("/work/data/notes.txt"),
                contents: Vec::new(),
            }
        );
        assert_eq!(report.mediated[1].decision, Decision::Allow);
        assert_eq!(effects.calls().len(), 1, "only the allowed read ran");
    }

    #[test]
    fn effect_failure_becomes_a_failed_response_not_an_error() {
        let m = base_manifest("[env]\nallow = \"PATH\"\n");
        let req = CapabilityRequest::Env { key: "PATH".into() };
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[ready(), call(1, &req)]),
            Vec::new(),
            ExitOutcome {
                code: Some(0),
                success: true,
            },
        )));
        let policy = Arc::new(ManifestPolicy::from_manifest(&m));
        let effects = Arc::new(RecordingHostEffects::with_err(HostEffectError::Io(
            "disk on fire".into(),
        )));
        let report = host(runner, policy, effects, Arc::new(FakeClock::new()))
            .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
            .expect("run ok");
        match &report.mediated[0].response {
            CapabilityResponse::Failed { error } => assert!(error.contains("disk on fire")),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn api_version_mismatch_never_spawns() {
        let m = base_manifest("");
        let mut m = m;
        m.api_version = "2".into();
        let runner = Arc::new(FakeProcessRunner::new_empty());
        let err = host(
            runner.clone(),
            Arc::new(ManifestPolicy::from_manifest(&m)),
            Arc::new(RecordingHostEffects::new()),
            Arc::new(FakeClock::new()),
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_secs(1))
        .expect_err("must reject");
        assert!(matches!(err, HostError::ApiVersionMismatch { .. }));
        assert_eq!(err.plugin().as_str(), "demo");
        assert!(runner.spawned().is_empty(), "must not have spawned");
    }

    #[test]
    fn malformed_capability_payload_is_fail_closed_and_kills() {
        let m = base_manifest("[env]\nallow = \"PATH\"\n");
        let bad = Frame::new(MessageType::CapabilityCall, [0u8; 16], vec![0xff, 0xff]);
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[ready(), bad]),
            Vec::new(),
            ExitOutcome {
                code: None,
                success: false,
            },
        )));
        let err = host(
            runner,
            Arc::new(ManifestPolicy::from_manifest(&m)),
            Arc::new(RecordingHostEffects::new()),
            Arc::new(FakeClock::new()),
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
        .expect_err("malformed payload must fail closed");
        assert!(matches!(err, HostError::MalformedCall { .. }));
    }

    #[test]
    fn misdirected_frame_after_handshake_is_fail_closed() {
        let m = base_manifest("");
        // A second Ready after the handshake is a protocol violation.
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[ready(), ready()]),
            Vec::new(),
            ExitOutcome {
                code: None,
                success: false,
            },
        )));
        let err = host(
            runner,
            Arc::new(ManifestPolicy::from_manifest(&m)),
            Arc::new(RecordingHostEffects::new()),
            Arc::new(FakeClock::new()),
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
        .expect_err("misdirected frame must fail closed");
        assert!(matches!(
            err,
            HostError::MisdirectedFrame {
                got: MessageType::Ready,
                ..
            }
        ));
    }

    #[test]
    fn handshake_without_ready_fails() {
        let m = base_manifest("");
        // Plugin sends a CapabilityCall instead of Ready.
        let req = CapabilityRequest::Env { key: "X".into() };
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[call(1, &req)]),
            Vec::new(),
            ExitOutcome {
                code: None,
                success: false,
            },
        )));
        let err = host(
            runner,
            Arc::new(ManifestPolicy::from_manifest(&m)),
            Arc::new(RecordingHostEffects::new()),
            Arc::new(FakeClock::new()),
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
        .expect_err("no Ready => handshake error");
        assert!(matches!(err, HostError::Handshake { .. }));
    }

    /// A clock that advances a fixed step every time it is read, so the
    /// deadline is crossed deterministically after a bounded number of
    /// loop iterations — no sleeping.
    struct TickingClock {
        step: Duration,
        at: Mutex<Duration>,
    }
    impl Clock for TickingClock {
        fn elapsed(&self) -> Duration {
            let mut at = self.at.lock().unwrap();
            *at += self.step;
            *at
        }
    }

    #[test]
    fn timeout_is_deterministic_and_kills_the_plugin() {
        let m = base_manifest("[env]\nallow = \"PATH\"\n");
        let req = CapabilityRequest::Env { key: "PATH".into() };
        // A long stream of calls; the ticking clock trips the deadline
        // before they are exhausted.
        let frames: Vec<Frame> = std::iter::once(ready())
            .chain((0..50).map(|i| call(i as u8, &req)))
            .collect();
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&frames),
            Vec::new(),
            ExitOutcome {
                code: None,
                success: false,
            },
        )));
        let clock = Arc::new(TickingClock {
            step: Duration::from_millis(40),
            at: Mutex::new(Duration::ZERO),
        });
        let err = host(
            runner,
            Arc::new(ManifestPolicy::from_manifest(&m)),
            Arc::new(RecordingHostEffects::with_ok(Vec::new())),
            clock,
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_millis(100))
        .expect_err("must time out");
        match err {
            HostError::TimedOut { plugin, budget } => {
                assert_eq!(plugin.as_str(), "demo");
                assert_eq!(budget, Duration::from_millis(100));
            }
            other => panic!("expected TimedOut, got {other}"),
        }
    }

    #[test]
    fn timeout_fires_even_when_the_plugin_stalls_mid_read() {
        // The between-frames clock check cannot catch a plugin that opens a
        // read and never produces a byte — the host would block in `read`
        // forever. The reader-thread + recv_timeout seam must still bound
        // it. Here the plugin never even sends Ready (its stdout read
        // blocks); the host must time out and kill it promptly.
        use std::io::{self, Read, Write};
        use std::sync::atomic::{AtomicBool, Ordering};

        struct BlockingReader {
            released: Arc<AtomicBool>,
        }
        impl Read for BlockingReader {
            fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
                while !self.released.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Ok(0) // EOF once the test releases it, so the thread exits
            }
        }

        struct StallingProcess {
            stdout: Option<Box<dyn Read + Send>>,
            sink: Vec<u8>,
            killed: Arc<AtomicBool>,
        }
        impl PluginProcess for StallingProcess {
            fn stdin(&mut self) -> &mut dyn Write {
                &mut self.sink
            }
            fn stdout(&mut self) -> &mut dyn Read {
                unreachable!("host uses take_stdout")
            }
            fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>> {
                self.stdout.take()
            }
            fn take_stderr(&mut self) -> Option<Box<dyn Read + Send>> {
                None
            }
            fn request_shutdown(&mut self) -> io::Result<()> {
                Ok(())
            }
            fn kill(&mut self) -> io::Result<()> {
                self.killed.store(true, Ordering::SeqCst);
                Ok(())
            }
            fn wait(&mut self) -> io::Result<ExitOutcome> {
                Ok(ExitOutcome {
                    code: None,
                    success: false,
                })
            }
            fn try_wait(&mut self) -> io::Result<Option<ExitOutcome>> {
                Ok(None)
            }
        }

        struct StallingRunner {
            released: Arc<AtomicBool>,
            killed: Arc<AtomicBool>,
        }
        impl ProcessRunner for StallingRunner {
            fn spawn(&self, _spec: &PluginSpawnSpec) -> io::Result<Box<dyn PluginProcess>> {
                Ok(Box::new(StallingProcess {
                    stdout: Some(Box::new(BlockingReader {
                        released: Arc::clone(&self.released),
                    })),
                    sink: Vec::new(),
                    killed: Arc::clone(&self.killed),
                }))
            }
        }

        let m = base_manifest("");
        let released = Arc::new(AtomicBool::new(false));
        let killed = Arc::new(AtomicBool::new(false));
        let runner = StallingRunner {
            released: Arc::clone(&released),
            killed: Arc::clone(&killed),
        };

        let started = std::time::Instant::now();
        let err = host(
            Arc::new(runner),
            Arc::new(ManifestPolicy::from_manifest(&m)),
            Arc::new(RecordingHostEffects::new()),
            Arc::new(FakeClock::new()),
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_millis(50))
        .expect_err("a plugin that stalls mid-read must still time out");
        let elapsed = started.elapsed();

        assert!(matches!(err, HostError::TimedOut { .. }), "got {err}");
        assert!(
            killed.load(Ordering::SeqCst),
            "the stalled process must be force-killed"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "must time out at ~50ms, not block forever (took {elapsed:?})"
        );
        // Let the detached reader thread observe EOF and exit cleanly.
        released.store(true, Ordering::SeqCst);
    }

    #[test]
    fn host_survives_a_crashing_plugin_and_can_run_another() {
        let m = base_manifest("");
        // Plugin readies then exits non-zero with no further frames.
        let crashing = FakePluginProcess::new(
            script(&[ready()]),
            b"boom\n".to_vec(),
            ExitOutcome {
                code: Some(101),
                success: false,
            },
        );
        let healthy = FakePluginProcess::new(
            script(&[ready()]),
            Vec::new(),
            ExitOutcome {
                code: Some(0),
                success: true,
            },
        );
        let runner = Arc::new(FakeProcessRunner::new_empty());
        runner.push_process(crashing);
        runner.push_process(healthy);
        let host = host(
            runner,
            Arc::new(ManifestPolicy::from_manifest(&m)),
            Arc::new(RecordingHostEffects::new()),
            Arc::new(FakeClock::new()),
        );

        let first = host
            .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
            .expect("run completes even though plugin crashed");
        assert_eq!(first.exit.code, Some(101));
        assert!(!first.exit.success);

        // The host is unaffected and runs the next plugin fine.
        let second = host
            .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
            .expect("host still usable after a crash");
        assert!(second.exit.success);
    }

    #[test]
    fn spawn_env_is_only_the_declared_allowlist() {
        // PATH almost certainly exists in the test env; UNLIKELY_VAR_XZ does
        // not. Only declared keys that resolve are passed; nothing ambient.
        let m = base_manifest("[env]\nallow = \"PATH\"\nallow = \"RSTUI_DEFINITELY_UNSET_XYZ\"\n");
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[ready()]),
            Vec::new(),
            ExitOutcome {
                code: Some(0),
                success: true,
            },
        )));
        host(
            runner.clone(),
            Arc::new(ManifestPolicy::from_manifest(&m)),
            Arc::new(RecordingHostEffects::new()),
            Arc::new(FakeClock::new()),
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
        .expect("run ok");
        let spec = &runner.spawned()[0];
        assert!(spec.env.iter().all(|(k, _)| k == "PATH"));
        assert!(
            spec.env
                .iter()
                .all(|(k, _)| k != "RSTUI_DEFINITELY_UNSET_XYZ")
        );
        assert_eq!(spec.program, PathBuf::from("bin/demo"));
        assert_eq!(spec.cwd, PathBuf::from("/w"));
    }

    #[test]
    fn host_response_bytes_decode_to_the_expected_capability_response() {
        // End-to-end wire check: capture what the host wrote to the plugin's
        // stdin and decode it back through the protocol + message codecs.
        let m = base_manifest("[env]\nallow = \"PATH\"\n");
        let req = CapabilityRequest::Env { key: "PATH".into() };
        let proc = FakePluginProcess::new(
            script(&[ready(), call(9, &req)]),
            Vec::new(),
            ExitOutcome {
                code: Some(0),
                success: true,
            },
        );
        let stdin_view = proc.stdin_handle();
        let runner = Arc::new(FakeProcessRunner::new(proc));
        host(
            runner,
            Arc::new(ManifestPolicy::from_manifest(&m)),
            Arc::new(RecordingHostEffects::with_ok(b"V".to_vec())),
            Arc::new(FakeClock::new()),
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
        .expect("run ok");

        let written = stdin_view.lock().unwrap().clone();
        let mut cursor = std::io::Cursor::new(written);
        // Frame 1: the host's Initialize.
        let init = read_frame(&mut cursor).expect("init frame");
        assert_eq!(init.message_type, MessageType::Initialize);
        // Frame 2: the CapabilityResponse, echoing the call's correlation id.
        let resp = read_frame(&mut cursor).expect("response frame");
        assert_eq!(resp.message_type, MessageType::CapabilityResponse);
        assert_eq!(resp.correlation_id[15], 9);
        assert_eq!(
            decode_response(&resp.payload).unwrap(),
            CapabilityResponse::Ok {
                payload: b"V".to_vec()
            }
        );
    }

    // ── Hook extension points (ADR 0007 §6) ──────────────────────────────

    /// A scripted plugin→host `HookResult` echoing the host's hook
    /// correlation id. The host's first `BeforeCapability` dispatch uses
    /// `correlation_id(1)` (the hook counter starts at 1), so a test that
    /// scripts a reply to the Nth dispatch passes `n`.
    fn hook_result(corr_n: u64, outcome: &HookOutcome) -> Frame {
        let mut cid = [0u8; 16];
        cid[..8].copy_from_slice(&corr_n.to_be_bytes());
        Frame::new(MessageType::HookResult, cid, encode_hook_result(outcome))
    }

    #[test]
    fn before_capability_hook_vetoes_an_allowed_call_and_the_effect_never_runs() {
        // PATH is granted, so the policy ALLOWS the env read — but the
        // plugin subscribes to before_capability and vetoes it.
        let m =
            base_manifest("[env]\nallow = \"PATH\"\n[hooks]\nsubscribe = \"before_capability\"\n");
        assert!(m.hooks.contains(&HookKind::BeforeCapability));
        let req = CapabilityRequest::Env { key: "PATH".into() };
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[
                ready(),
                call(1, &req),
                hook_result(
                    1,
                    &HookOutcome::Veto {
                        reason: "policy-of-policies says no".into(),
                    },
                ),
            ]),
            Vec::new(),
            ExitOutcome {
                code: Some(0),
                success: true,
            },
        )));
        let effects = Arc::new(RecordingHostEffects::with_ok(b"PATH-VALUE".to_vec()));
        let report = host(
            runner,
            Arc::new(ManifestPolicy::from_manifest(&m)),
            effects.clone(),
            Arc::new(FakeClock::new()),
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
        .expect("run completes");

        // The policy still ALLOWED it — the hook is the second lock.
        assert_eq!(report.mediated[0].decision, Decision::Allow);
        match &report.mediated[0].response {
            CapabilityResponse::Denied { reason } => {
                assert!(reason.contains("vetoed by before_capability hook"));
                assert!(reason.contains("policy-of-policies says no"));
            }
            other => panic!("expected hook-vetoed Denied, got {other:?}"),
        }
        // The crux: a vetoed call NEVER reaches the effector.
        assert!(
            effects.calls().is_empty(),
            "a hook-vetoed call must not reach HostEffects"
        );
    }

    #[test]
    fn before_capability_continue_lets_the_allowed_call_proceed() {
        let m =
            base_manifest("[env]\nallow = \"PATH\"\n[hooks]\nsubscribe = \"before_capability\"\n");
        let req = CapabilityRequest::Env { key: "PATH".into() };
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[
                ready(),
                call(1, &req),
                hook_result(1, &HookOutcome::Continue),
            ]),
            Vec::new(),
            ExitOutcome {
                code: Some(0),
                success: true,
            },
        )));
        let effects = Arc::new(RecordingHostEffects::with_ok(b"V".to_vec()));
        let report = host(
            runner,
            Arc::new(ManifestPolicy::from_manifest(&m)),
            effects.clone(),
            Arc::new(FakeClock::new()),
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
        .expect("run completes");
        assert_eq!(
            report.mediated[0].response,
            CapabilityResponse::Ok {
                payload: b"V".to_vec()
            }
        );
        assert_eq!(effects.calls(), vec![req]);
    }

    #[test]
    fn no_hook_subscription_means_no_dispatch_and_no_extra_frame_needed() {
        // Same allowed call, but the manifest does NOT subscribe to
        // before_capability — the host must not dispatch a hook, so the
        // scripted stdout has no HookResult and the call still succeeds.
        let m = base_manifest("[env]\nallow = \"PATH\"\n");
        assert!(m.hooks.is_empty());
        let req = CapabilityRequest::Env { key: "PATH".into() };
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[ready(), call(1, &req)]),
            Vec::new(),
            ExitOutcome {
                code: Some(0),
                success: true,
            },
        )));
        let effects = Arc::new(RecordingHostEffects::with_ok(b"V".to_vec()));
        let report = host(
            runner,
            Arc::new(ManifestPolicy::from_manifest(&m)),
            effects.clone(),
            Arc::new(FakeClock::new()),
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
        .expect("run completes");
        assert!(report.mediated[0].response.eq(&CapabilityResponse::Ok {
            payload: b"V".to_vec()
        }));
        assert_eq!(effects.calls(), vec![req]);
    }

    #[test]
    fn a_hook_can_never_widen_a_policy_denial() {
        // The policy DENIES (no grant). Even with before_capability
        // subscribed, the host must not dispatch the hook on the deny path
        // — a hook can only narrow. The scripted stdout deliberately has NO
        // HookResult; if the host wrongly dispatched, it would block/err.
        let m = base_manifest("[hooks]\nsubscribe = \"before_capability\"\n");
        let req = CapabilityRequest::Env {
            key: "SECRET".into(),
        };
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[ready(), call(1, &req)]),
            Vec::new(),
            ExitOutcome {
                code: Some(0),
                success: true,
            },
        )));
        let effects = Arc::new(RecordingHostEffects::new());
        let report = host(
            runner,
            Arc::new(ManifestPolicy::from_manifest(&m)),
            effects.clone(),
            Arc::new(FakeClock::new()),
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
        .expect("run completes without a hook round-trip on the deny path");
        assert!(matches!(
            report.mediated[0].response,
            CapabilityResponse::Denied { .. }
        ));
        assert!(effects.calls().is_empty());
    }

    #[test]
    fn malformed_hook_result_is_fail_closed_and_kills() {
        let m =
            base_manifest("[env]\nallow = \"PATH\"\n[hooks]\nsubscribe = \"before_capability\"\n");
        let req = CapabilityRequest::Env { key: "PATH".into() };
        // Reply to the hook with a CapabilityResponse-typed frame instead
        // of HookResult → misdirected → fail-closed.
        let mut cid = [0u8; 16];
        cid[7] = 1;
        let bogus = Frame::new(MessageType::CapabilityResponse, cid, vec![0xff]);
        let runner = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
            script(&[ready(), call(1, &req), bogus]),
            Vec::new(),
            ExitOutcome {
                code: None,
                success: false,
            },
        )));
        let err = host(
            runner,
            Arc::new(ManifestPolicy::from_manifest(&m)),
            Arc::new(RecordingHostEffects::with_ok(Vec::new())),
            Arc::new(FakeClock::new()),
        )
        .run_plugin(&m, Path::new("/w"), Duration::from_secs(5))
        .expect_err("a non-HookResult reply to a hook must fail closed");
        assert!(
            matches!(err, HostError::MisdirectedFrame { .. }),
            "got {err}"
        );
    }
}
