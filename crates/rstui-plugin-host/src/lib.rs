//! `rstui-plugin-host` — the permissioned plugin host for the rstui TUI
//! framework.
//!
//! rstui plugins run as **separate OS processes the host fully mediates**,
//! deny-by-default. A plugin has no ambient authority: every privileged
//! action (read a file, open a socket, run a command, read an environment
//! variable) is a request sent to the host, which checks it against a policy
//! and performs it — or refuses — on the plugin's behalf. This is the
//! architecture fixed by
//! [ADR 0007](https://github.com/andymac4182/rstui/blob/main/docs/adr/0007-plugin-host-and-secure-execution.md),
//! reduced to what safe `std` expresses: this crate, like `rstui-core`, has
//! **no dependencies** and contains **no `unsafe`** (the workspace forbids
//! it), so a WASM/seccomp/`rlimit` sandbox is deliberately not the model —
//! the OS process boundary plus host-side capability mediation is.
//!
//! The pieces:
//!
//! - [`capability`]: the closed four-capability authority model
//!   ([`Capability`]) shared by every other module — the scoped grants a
//!   manifest declares ([`CapabilityGrant`]), the typed action a plugin
//!   asks for ([`CapabilityRequest`]), and the *pure, filesystem-free*
//!   lexical path-scope check ([`capability::normalize_lexical`],
//!   [`capability::is_within`]) that defends a filesystem grant against
//!   `..` traversal (ADR 0007 §3).
//! - [`manifest`]: the operator-reviewable plugin manifest
//!   ([`PluginManifest`]) and its strict, hand-written, *fail-closed*
//!   parser — an unknown key is an error, not a warning, and there is no
//!   serde/TOML dependency (ADR 0007 §2).
//! - [`permission`]: the [`permission::PermissionPolicy`] trait (the host's
//!   *grant* decision over a *canonicalised* request), the manifest-derived
//!   default policy, and a recording test double.
//! - [`protocol`]: the hand-rolled length-prefixed, *fail-closed* host↔plugin
//!   frame codec (ADR 0007 §4) — any framing error terminates the
//!   connection, no skip-and-continue.
//! - [`process`]: the [`process::ProcessRunner`]/[`process::PluginProcess`]
//!   spawn-and-pipe seam, with in-memory fakes so the whole boundary is
//!   deterministically testable with no real process, socket, or clock —
//!   the `rstui-runtime` `Harness` standard applied to security.
//! - [`message`]: the hand-rolled, fail-closed codec that gives the opaque
//!   frame payload meaning — a [`CapabilityRequest`] in, a
//!   [`message::CapabilityResponse`] back.
//! - [`effects`]: the [`effects::HostEffects`] performer the host runs an
//!   *already-permitted* request through, with a recording fake so a test
//!   asserts a denied request never reached it.
//! - [`clock`]: the [`clock::Clock`] time seam so plugin timeouts are
//!   deterministically advanceable in tests, never wall-clock.
//! - [`host`]: the [`host::PluginHost`] that composes all of the above —
//!   spawn, handshake, then the mediation loop where every capability call
//!   is host-canonicalised, policy-checked, and only *then* (if allowed)
//!   run through [`effects`], returning an auditable
//!   [`host::PluginRunReport`].
//!
//! Every nondeterministic edge is an injected trait with a `std` impl and a
//! scripted in-memory fake, so a denied capability, a malformed frame, a
//! deterministic timeout, and a plugin crash are each an ordinary unit test.
//!
//! # Example
//!
//! The pure path-scope primitive every filesystem grant is enforced through
//! — no filesystem, no plugin, fully deterministic:
//!
//! ```
//! use std::path::Path;
//! use rstui_plugin_host::capability::{is_within, normalize_lexical};
//!
//! // A `..` that climbs back into the granted root is fine...
//! assert!(is_within(Path::new("/srv/plugin/data"), Path::new("/srv/plugin/data/x/../y")));
//! // ...but one that escapes it is rejected before any host effect runs.
//! assert!(!is_within(Path::new("/srv/plugin/data"), Path::new("/srv/plugin/data/../../etc/passwd")));
//!
//! // Normalisation is lexical (no filesystem access, so it is total and
//! // deterministic): `.` is dropped and `..` cancels the prior segment.
//! assert_eq!(
//!     normalize_lexical(Path::new("/srv/./plugin/data/../data/file")),
//!     Path::new("/srv/plugin/data/file"),
//! );
//! ```

pub mod capability;
pub mod clock;
pub mod effects;
pub mod host;
pub mod manifest;
pub mod message;
pub mod permission;
pub mod process;
pub mod protocol;
pub mod std_process;

pub use capability::{Capability, CapabilityGrant, CapabilityRequest, FsMode};
pub use clock::{Clock, FakeClock, SystemClock};
pub use effects::{
    CapabilityOutcome, HostEffectError, HostEffects, RecordingHostEffects, SystemHostEffects,
};
pub use host::{HostError, MediationRecord, PluginHost, PluginId, PluginRunReport};
pub use manifest::{ManifestError, PluginManifest};
pub use message::{
    CapabilityResponse, MessageError, decode_request, decode_response, encode_request,
    encode_response,
};
pub use permission::{Decision, ManifestPolicy, PermissionPolicy, RecordingPolicy};
pub use process::{
    ExitOutcome, FakePluginProcess, FakeProcessRunner, PluginProcess, PluginSpawnSpec,
    ProcessRunner,
};
pub use protocol::{Frame, MAX_FRAME_SIZE, MessageType, ProtocolError, read_frame, write_frame};
pub use std_process::{StdPluginProcess, StdProcessRunner};
