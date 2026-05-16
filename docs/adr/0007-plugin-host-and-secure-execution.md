# ADR 0007: Plugin host and secure execution

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

The README commits rstui to "a permissioned plugin host built on
process isolation" as a planned boundary, alongside "more widgets and
examples". No plugin code exists yet, so this is the moment to fix the
expensive-to-reverse axis — *how untrusted plugin code is allowed to
touch the host* — before any concrete plugin surface is written, per
the iter-10/19/21 decision-vs-mechanical-split precedent ADR 0004
followed.

A plugin system's whole point is to run code the framework author did
not write. The question this ADR settles is **not** "how do plugins
extend a TUI" (extension points are a later, cheap-to-revise slice) but
"what is the security boundary, and is it real and testable". rstui's
existing architecture forces the shape of the answer; these constraints
are not relitigated here:

- **`rstui-core` and `rstui-runtime` are dependency-free.** `core`'s
  module doc states it "intentionally has no dependencies"; `runtime`
  depends only on `core`. The only third-party dependency in the
  workspace is crossterm, deliberately quarantined in `rstui-crossterm`
  (ADR 0001). A plugin host that pulls in a WASM runtime, an async
  executor, a serde stack, or a TOML parser would be the single
  largest dependency expansion in the project's history and would
  contradict the stated workspace discipline ("Crates are introduced
  only when there is enough real API surface to justify the boundary").
- **`unsafe_code = "forbid"` is workspace-wide** (`Cargo.toml`
  `[workspace.lints.rust]`, ADR 0003 §1). The reference security
  implementation, `rivet-dev/secure-exec`, relies on `unsafe` libc for
  its strongest in-process controls — `close_inherited_fds()` walking
  `/proc/self/fd`, `set_cloexec()` via `fcntl`, `seccomp`/`rlimit`
  syscall gating. **rstui cannot replicate any of that.** The boundary
  must hold using only safe `std` and the OS process model.
- **Determinism is the framework's defining property.** `rstui-runtime`
  is built so "the *same* `App`/`Cmd` code runs under the headless
  `Harness` ... with no TTY, threads, or clock"; effects are values
  (`Cmd`) the runtime performs, never the app. A plugin host that can
  only be tested by spawning real OS processes against a real clock
  would be the one untestable island in a framework whose entire thesis
  is deterministic, terminal-free testability.
- **The vague-generic-name ban is enforced** (`docs/conventions/
  naming.md`, ADR 0003 §7): no `manager`/`helper`/`util`/`common`/…
  buckets. The host's types must each name a responsibility.

The reference triad was studied directly from source (`npx opensrc`):
`rivet-dev/secure-exec` is the security model; `earendil-works/pi` and
`anomalyco/opencode` are the capability/ergonomics models. Their
concrete designs are cited as Evidence below rather than asserted.

## Decision drivers

1. **The security boundary must be real, not aspirational.** A plugin
   must not be able to read a file, open a socket, run a command, or
   read an env var the operator did not explicitly grant — and that
   must be true by construction, not by the plugin's good behaviour.
2. **The boundary must be deterministically testable** the way the rest
   of rstui is: a denied capability, a malformed frame, a timeout, and
   a plugin crash must each be assertable in a unit test with no real
   process, socket, or wall-clock — the `Harness` standard.
3. **Permissions must be explicit and reviewable.** What a plugin may
   do is declared in its manifest and decided by a host policy an
   operator can read, not discovered at runtime by what it attempts.
4. **No dependency expansion, no `unsafe`.** The host lives within the
   workspace's existing discipline or it does not ship.
5. **Decoupled from widgets and the event loop.** Per the ownership
   split, plugin internals must not couple to a concrete widget or to
   `rstui-runtime`; integration is a thin, later seam.
6. **Honest about what safe Rust cannot enforce.** Where the boundary
   is weaker than secure-exec's because `forbid(unsafe)` forbids the
   mechanism, that gap is documented as a named residual threat tier,
   not hidden.

## Options considered

### A. In-process dynamic plugins (the pi / opencode model)

Both references load plugin code in-process via dynamic `import`
(pi: `jiti.import`; opencode: `await import(entry)`), handing the
plugin a fully-capable host object. pi's own docs state plainly:
*"Extensions run with your full system permissions and can execute
arbitrary code. Only install from sources you trust."*

Rejected. This is a *trust-the-author* extension model, not a
*permissioned* one. It has **no** security boundary — an in-process
plugin shares the host's address space and ambient authority by
definition. The README's word is "permissioned ... process isolation";
driver 1 is unmeetable here. (Their *ergonomics* — manifest shape,
hook reduction, lifecycle — are still worth copying; see the Decision.)

### B. WASM / language-VM sandbox

A WebAssembly runtime (wasmtime, wasmi) gives in-process memory
isolation with a capability-import boundary.

Rejected on drivers 4 and 2. Every production WASM runtime is a large
dependency tree containing `unsafe` (JIT, linear-memory mapping) — it
violates the dependency-free discipline and cannot even compile cleanly
under a workspace that *forbids* `unsafe` in its own crates without
quarantining it the way crossterm is, for far more code. It also makes
deterministic testing harder, not easier (the sandbox is now an opaque
third-party engine, not a seam rstui controls). secure-exec itself
chose a **separate OS process**, not in-process VM isolation, as its
primary boundary for essentially these reasons.

### C. Separate OS process, host-mediated, deny-by-default (chosen)

The plugin is a child process. It has **no** ambient authority over the
host: every privileged action is a request sent over a pipe to the
host, which checks it against a policy and performs it (or refuses) on
the plugin's behalf. This is exactly secure-exec's architecture
(`Runtime` → `Bridge` → `SystemDriver`, "user code ... can only reach
host capabilities through the bridge"), reduced to what safe `std`
expresses.

Chosen. The OS process boundary is the one strong isolation primitive
available under `forbid(unsafe)` (separate memory, separate crash
domain, kernel-enforced). Every nondeterministic edge (process spawn,
the byte pipe, the clock, the host effect) becomes a trait with a real
`std` impl and an in-memory fake — the same pattern `rstui-runtime`
already uses for `Backend`/`EventSource` (`TestBackend`/
`TestEventSource`) and `Cmd`, so the security properties are
`Harness`-grade testable. secure-exec proves this exact factoring is
viable and `unsafe`-free at the seam (`FrameSender`/`ResponseReceiver`
traits with production + stub impls; `CommandExecutor`/`SpawnedProcess`
as the fakeable spawn seam).

## Decision

rstui ships a **`rstui-plugin-host` crate**: dependency-free, no
`unsafe`, plugins run as separate OS processes the host fully mediates,
deny-by-default.

### 1. The capability set is a closed enum

Exactly four capabilities, mirroring secure-exec's `SystemDriver`
(`filesystem`, `network`, `commandExecutor`, `env`):

```
Capability ∈ { Filesystem, Network, Command, Env }
```

Closed on purpose: a plugin cannot invent a new kind of authority, and
the policy code that must reason about every capability is finite and
auditable. A capability the manifest does not declare is **absent**,
and an absent capability is denied — never defaulted to allow (this is
secure-exec's `createFsStub`/`ENOSYS` behaviour: missing capability ⇒
hard refusal, not silent permit).

### 2. Capabilities are declared in a manifest, granted by a policy

A plugin ships a **manifest** declaring its identity (`name`,
`version`), the host-protocol version it targets (`api_version`,
semver-gated like opencode's `engines.opencode`: incompatible ⇒
refuse to load, do not best-effort), the executable to spawn (`entry`),
and the *scoped* capabilities it requests:

- filesystem: explicit `{ mode, path }` grants (no ambient root);
- network: explicit `{ host, port }` grants;
- command: explicit `{ program, args-prefix }` allowlist;
- env: an explicit key allowlist (the host does `env_clear()` then
  re-adds only these — secure-exec's `filterEnv`).

The manifest is the *request*. A host-side **`PermissionPolicy`**
(a trait) is the *grant*: it returns `Allow` or `Deny { reason }` for a
typed `CapabilityRequest`. The shipped `ManifestPolicy` grants exactly
what the manifest declared and nothing else; an operator can wrap or
replace it. The manifest format is a **deliberately minimal, strict,
documented line grammar** parsed by a hand-written parser that
**rejects any unknown key** (fail-closed, like secure-exec closing the
connection on any unrecognised frame) — not a TOML/serde dependency
(driver 4). Its grammar is specified in the crate docs so the parser
and the format cannot drift.

### 3. Enforcement is in the host wrapper, never trusted to the plugin

The policy check runs **before** the host effect, and the request is
**canonicalised first**: a filesystem path is normalised (resolve
`.`/`..`, reject escape outside the granted root) by the host before
`check()` sees it, so a plugin cannot defeat a path grant with `..`
(secure-exec's `normalizeFsPath`, done in the enforcement wrapper, not
trusted to the caller). The actual side effect is performed through a
**`HostEffects`** trait — real `std::fs`/`std::process`/etc. in
production, a recording fake in tests — so a test asserts not only
"the response said denied" but "the host effect was never invoked".

### 4. The host↔plugin protocol is a hand-rolled length-prefixed frame

Communication is over the child's stdin/stdout pipes (no socket: the
`mkdtemp(0700)` + CLOEXEC dance secure-exec uses to secure a Unix
socket *requires the `unsafe` libc rstui forbids*, so a pipe — which
`std` already gives CLOEXEC by default and which has no filesystem
rendezvous to secure — is the safe-Rust-honest choice). The frame, copied
structurally from secure-exec's envelope (`ipc_binary.rs`):

```
[4B length: u32 big-endian, excludes these 4 bytes]
[1B message type]                 (distinct host→plugin / plugin→host ranges)
[16B correlation id]              (routes a response to its request)
[payload bytes]
```

A single `MAX_FRAME_SIZE` const is enforced on both encode and decode;
**any framing or decode error terminates that plugin connection — no
skip-and-continue** (secure-exec's explicit rule). The payload uses a
minimal, explicit, self-describing byte encoding for the small fixed
message set (`Initialize`, `HookDispatch`, `CapabilityCall`,
`CapabilityResponse`, `HookResult`, `Shutdown`) — not serde (driver 4).
The plugin's diagnostic logging goes to its **stderr**, streamed via a
callback, never buffered unbounded into host memory (secure-exec's
"Logging Contract": untrusted code must not be able to grow host memory
through its output).

### 5. Every nondeterministic edge is an injected trait

`ProcessRunner` (spawn), `PluginProcess` (the pipe + kill/wait),
`HostEffects` (the privileged side effects), `PermissionPolicy` (the
grant decision), and a `Clock` (deadlines). Production impls use
`std`; test impls are in-memory and scripted. This makes a denied
capability, a malformed frame, a deterministic timeout (advance a
`FakeClock`, not sleep), and a plugin crash (host stays up) all unit-
testable with no real process/socket/TTY/clock — the `Harness`
standard, achieved the same way `rstui-runtime` achieves it.

### 6. Lifecycle and error policy follow the ergonomics references

A failing plugin is isolated, not fatal: a load/init failure rolls back
*that* plugin and the host continues (opencode: "failure isolated to
that plugin"); errors are attributed to the originating `PluginId`
(pi's source-aware scopes / opencode's per-plugin trace span);
shutdown is cooperative-then-forced (send `Shutdown`, wait a
`Clock`-bounded grace, then kill — secure-exec's grace-then-SIGKILL,
expressed with `std` only). Hook *reduction* semantics (which hook can
veto, which chains) are deferred to the extension-point slice; this ADR
fixes only the *security* boundary they will run inside.

### 7. The `forbid(unsafe)` residual-threat tier is documented, not hidden

Because rstui forbids `unsafe`, the host **cannot** seccomp-gate
syscalls, set `rlimit`/cgroup CPU/memory/FD caps, or `CLOEXEC`-scrub
inherited descriptors the way secure-exec does. The OS process boundary
still gives separate memory and a separate crash domain; the capability
mediation still gives deny-by-default authority. But a *granted*
capability can still be abused within its grant (secure-exec names this
exact tier: an authorized network grant is "an open proxy"), and a
plugin can still burn CPU/memory inside its own process. rstui's stance,
stated outright in the crate docs and mirroring secure-exec's
`security-model.mdx` "User/host-layer" framing: **resource-limit and
syscall hardening is an operator-deployment responsibility** (run
plugins under an OS sandbox / cgroup / container), and *authorized-
capability abuse* is a documented residual risk the manifest review is
the mitigation for. This is a deliberate, recorded consequence of
driver 4, not an oversight.

## Evidence

Concrete facts from the references, read from source, that this
decision rests on:

- **secure-exec — separate process is the primary boundary.**
  `docs/architecture.mdx`: "User code runs inside the sandbox and can
  only reach host capabilities through the bridge ... The system driver
  wraps each capability in a permission check before executing it on
  the host." Untrusted code is a *separate process* (a V8 sidecar),
  never in-process — validating Option C over A/B.
- **secure-exec — closed four-capability set, deny-if-absent.**
  `SystemDriver` exposes exactly `filesystem`, `network`,
  `commandExecutor`, `env`; a missing capability is replaced by a
  deny-all stub that throws `ENOSYS` (`createFsStub` et al.),
  `checkPermission` *throws* when no checker is present — "deny if
  unspecified", never "allow". Directly adopted as Decision §1/§2.
- **secure-exec — enforcement canonicalises before checking.**
  `normalizeFsPath` collapses `//` and resolves `.`/`..` without
  escaping root *in the enforcement wrapper*, not trusted to the
  caller; the wrapper checks then delegates, denial throwing before any
  host op runs. Adopted verbatim as Decision §3.
- **secure-exec — hand-rolled length-prefixed framing, fail-closed.**
  `ipc_binary.rs`: `[4B total_len u32 BE][1B msg_type][…][payload]`,
  distinct host/sidecar type-code ranges, `MAX_FRAME_SIZE = 64MB`,
  "any framing/deserialization error closes the connection immediately
  — no skip-and-continue", length prefix written last. The envelope is
  hand-rolled binary, *not* a serde library. Adopted as Decision §4,
  proving driver 4 is compatible with a real protocol.
- **secure-exec — the fakeable-seam pattern is the same one rstui
  already uses.** `host_call.rs` factors IO behind `FrameSender`/
  `ChannelFrameSender`/`WriterFrameSender` and `ResponseReceiver`/
  `ReaderResponseReceiver` traits with production + stub impls, plus
  `BridgeCallContext::stub()` / byte-buffer constructors for tests;
  `CommandExecutor`/`SpawnedProcess` is the trivially fakeable spawn
  seam. This is `TestBackend`/`TestEventSource` by another name —
  Decision §5.
- **secure-exec — the residual tier is named in its own docs.**
  `security-model.mdx` separates Runtime/Driver/User layers and states
  process isolation does *not* mitigate authorized-capability abuse
  ("if you grant network, the sandbox is an open proxy") or host-side
  processing of sandbox output; the attack-vector catalog is candid
  about unmitigated CPU/memory/output-amplification gaps. Decision §7
  adopts this honesty rather than implying safe Rust closes them.
- **secure-exec — what `forbid(unsafe)` costs us, precisely.**
  Its strongest in-process controls — `close_inherited_fds()` over
  `/proc/self/fd`, `set_cloexec()` via `fcntl(FD_CLOEXEC)`, the
  `mkdtemp` `DirBuilder::mode(0o700)` socket rendezvous, `rlimit`/
  CPU-time caps — are all `unsafe` libc. Enumerated here so Decision §4
  (pipe over socket) and §7 (operator-deployment hardening) read as
  forced moves, not preferences.
- **pi — ambient authority is why rstui requires explicit
  declaration.** pi has *no manifest and no declared capabilities*;
  extensions get full system permission (`jiti.import`, docs:
  "full system permissions ... only install from sources you trust").
  rstui inverts this: the manifest must declare scoped capabilities and
  the default is deny — the precise gap Option C closes.
- **pi — lifecycle/error ergonomics worth copying.** `ExtensionAPI`
  is an event/registry surface (`pi.on(type, …)`, `registerTool/
  Command/Shortcut`); error policy is explicit `errorMode:
  "continue" | "throw"` with `onError`, coding-agent default
  `continue` (one bad extension does not crash the host); hot reload is
  `hooks.clear()` → reload. Informs Decision §6 (failure isolation,
  per-plugin attribution); reduction semantics deferred.
- **opencode — declared-and-approved, last-match-wins, default-ask.**
  Plugins are declared in config (`plugin: [...]`), gated by a semver
  `engines.opencode` (incompatible ⇒ skipped+warned), identified by a
  unique `id`, toggled by `plugin_enabled`. Its permission `evaluate()`
  is `findLast(rule matches)` over user-ordered rules, **default action
  `ask`**, wildcard match on permission *and* an argument pattern;
  plugins can override via the `permission.ask` hook. Adopted as
  Decision §2's policy shape (manifest = request, host policy = grant,
  semver gate, deny/ask default) and as the reason the policy is a
  trait an operator composes.
- **opencode — failure isolated, reverse-order awaited cleanup.**
  `specs/tui-plugins.md`: external plugins activate sequentially; an
  init failure rolls back *that plugin's* tracked registrations and
  loading continues; disposal runs registrations in reverse order,
  awaited, with a per-plugin time budget. Directly shapes Decision §6.

## Consequences

**Made easy:**

- Security properties become ordinary unit tests: "denied request never
  reached the host effect", "unknown manifest key refused", "malformed
  frame closed the connection", "timeout fired at the simulated
  deadline", "plugin crash left the host running" — each a `Harness`-
  style deterministic test, no process/socket/clock.
- Manifests are operator-reviewable plain text; the grant is one
  auditable policy object, not scattered runtime checks.
- Zero new dependencies, zero `unsafe`: the host ships within the same
  discipline as the rest of the workspace and the existing CI gates
  (fmt/clippy/doc/test/lint-names) cover it unchanged.
- The host is decoupled: it knows nothing about widgets or
  `rstui-runtime`; a later, thin seam can surface plugin events as
  `Cmd`/messages without either side coupling.

**Made hard / accepted costs:**

- The frame codec and manifest parser are rstui's to write, test, and
  maintain — the price of driver 4. They are small, fully specified,
  and exhaustively tested; the alternative (a serde/TOML/WASM
  dependency tree) was judged the larger long-term cost.
- No in-process resource or syscall limits (CPU, memory, FD, seccomp)
  and no graceful POSIX-signal escalation finer than cooperative-
  `Shutdown`-then-`kill` — all blocked by `forbid(unsafe)`. Mitigation
  is operator deployment (sandbox/cgroup/container) and manifest
  review, documented as a named residual threat tier (Decision §7), not
  silently absent.
- The pipe transport has no per-connection authentication token
  (secure-exec's constant-time-compared token guards a *socket* with a
  filesystem rendezvous; a private stdin/stdout pair between parent and
  its own child has no such rendezvous to guard). Recorded so it is a
  decision, not a gap.

**Deferred (explicitly out of scope for this ADR):**

- Plugin *extension points* and hook *reduction* semantics (which hook
  vetoes, which chains, the event vocabulary) — a separate, cheap-to-
  revise slice that runs *inside* this boundary.
- WASM/native dynamic loading, plugin hot-reload, an interactive
  permission-prompt UI, network-grant proxying details, and a
  marketplace/distribution story — none are blocked by this decision;
  all build on it.
