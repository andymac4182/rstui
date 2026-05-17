# Plugin system

`rstui-plugin-host` runs plugins as **separate OS processes the host fully
mediates, deny-by-default**. It is dependency-free, contains no `unsafe`, and
every security property is a deterministic unit test (in-memory fakes — no
real processes, sockets or wall clock). The recorded design is
[ADR 0007](adr/0007-plugin-host-and-secure-execution.md).

> The whole model in one sentence: **a plugin can only ever do what its
> operator-reviewed manifest grants, the host canonicalises every request
> before checking it, a hook can only ever *narrow* that authority, and any
> protocol error terminates the connection.**

## The layers

```
 manifest (operator-reviewed text)
        │  parsed fail-closed →
        ▼
 CapabilityGrant[]  ──→  PermissionPolicy (deny-by-default)
                                  ▲
 plugin process ──CapabilityCall──┤ host canonicalises path, THEN checks policy
   (env_clear'd,                  │ on Allow: optional before_capability hook (veto-only)
    allowlisted env)              │ on Allow & not vetoed: HostEffects runs the effect
        ▲           ◀─CapabilityResponse─ (echoes correlation id)
        └────── length-prefixed, fail-closed frame protocol ──────┘
```

## 1. The closed capability model

Exactly four authority kinds. There is no fifth, and no wildcard.

```rust
enum Capability { Filesystem, Network, Command, Env }
enum FsMode { Read, Write }          // Write does NOT imply Read

enum CapabilityGrant {                       // what the manifest declares
    Filesystem { mode: FsMode, root: PathBuf },        // root is lexically normalized
    Network    { host: String, port: u16 },            // exact, no wildcards
    Command    { program: String, args_prefix: Vec<String> },  // empty prefix = any args
    Env        { key: String },
}
enum CapabilityRequest {                      // what the plugin asks for
    Filesystem { mode, path, contents },       // host makes path absolute + normalize_lexical
    Network    { host, port },
    Command    { program, args },
    Env        { key },
}
```

Path safety is **lexical and filesystem-free** — `normalize_lexical` drops
`.`, cancels `..` against prior segments, and `is_within(root, candidate)` is
a component-wise (not string-prefix) check on the normalized paths. So
`data/../../etc/passwd` is rejected *before* any effect, with no TOCTOU
window.

## 2. The manifest

A strict, hand-written parser (no serde/TOML dependency). Required top-level
keys: `name`, `version`, `api_version`, `entry`. Optional repeatable
sections produce grants.

```ini
name = "my-plugin"
version = "1.2.3"
api_version = "0.1.0"               # semver requirement on the host protocol
entry = "/usr/lib/my-plugin/bin"

[filesystem]
read  = "/data/input"
write = "/data/output"

[network]
allow = "api.example.com:443"

[command]
allow = "git log --oneline"          # program + arg prefix

[env]
allow = "HOME"
allow = "PATH"

[hooks]
subscribe = "before_capability"
```

```rust
PluginManifest::parse(text: &str) -> Result<PluginManifest, ManifestError>
struct PluginManifest { name, version, api_version, entry,
                        grants: Vec<CapabilityGrant>, hooks: Vec<HookKind> }
```

**Fail-closed grammar.** Unknown key, unknown section, missing required field,
duplicate top-level key, empty required value, a filesystem path that escapes
after normalization, embedded quotes/control chars, an unknown hook name —
each is a hard **error**, never a warning. There is nothing a malformed
manifest can do except fail to load.

## 3. The permission policy

```rust
trait PermissionPolicy: Send + Sync {
    fn check(&self, request: &CapabilityRequest) -> Decision;   // request is pre-canonicalised
}
enum Decision { Allow, Deny { reason: String } }

struct ManifestPolicy { /* grants */ }
ManifestPolicy::from_manifest(&PluginManifest)
```

Matching: filesystem mode must match exactly (`Read` ≠ `Write`) and the path
must be `is_within` a granted root; network host+port exact; command program
exact and request args must start with `args_prefix`; env key exact. **A
request matching no grant is always denied with a reason** — deny-by-default
is an invariant, not a default.

`RecordingPolicy<P>` wraps any policy and logs every `(request, decision)` so
a test can prove a denied request *never reached the effector*.

## 4. The frame protocol

Length-prefixed binary framing, fail-closed, **no skip-and-continue**:

```
[ length: u32 BE ][ type: u8 ][ correlation-id: 16B ][ payload ]
```

- Length excludes itself and is checked against a 16 MiB cap **before**
  allocation (no OOM DoS).
- Type byte is direction-coded: `0x01–0x7F` host→plugin, `0x80–0xFF`
  plugin→host. Unknown codes are rejected.
- Correlation id is echoed unchanged by the responder.

| Code | Host→plugin | | Code | Plugin→host |
|------|-------------|-|------|-------------|
| 0x01 | Initialize | | 0x81 | Ready |
| 0x02 | HookDispatch | | 0x82 | CapabilityCall |
| 0x03 | CapabilityResponse | | 0x83 | HookResult |
| 0x04 | Shutdown | | 0x84 | Log |

```rust
read_frame<R: Read>(&mut R) -> Result<Frame, ProtocolError>
write_frame<W: Write>(&mut W, &Frame) -> Result<(), ProtocolError>
enum ProtocolError { Io, FrameTooLarge, FrameTooSmall, UnknownMessageType, Truncated }
```

Any framing error **terminates the connection**. There is no recovery path by
design — a desynced or hostile stream cannot be "resumed".

## 5. The host mediation loop

```rust
struct PluginHost { runner, policy, effects, clock, host_api_version }
PluginHost::new(runner, policy, effects, clock, host_api_version)
host.run_plugin(&manifest, cwd: &Path, timeout: Duration)
    -> Result<PluginRunReport, HostError>
```

Pipeline:

1. **API-version gate** (fail-closed): the host's `api_version` must satisfy
   the manifest's `VersionReq`, else spawn is refused.
2. **Spawn** with `env_clear()` then re-add **only** the manifest's `[env]`
   grants. The child inherits *zero* ambient environment. Working dir is `cwd`.
3. **Initialize → Ready** handshake.
4. `SessionStart` hook if subscribed (observe-only, reply ignored).
5. **For every `CapabilityCall`:** canonicalise (relative path → absolute →
   `normalize_lexical`) → `policy.check(...)` → on `Deny` return
   `Denied { reason }`, **the effect never runs** → on `Allow`, if
   `before_capability` is subscribed dispatch it; a `Veto` converts the
   `Allow` into `Denied` → otherwise `HostEffects::run(request)` and return
   the result. Every step is recorded in a `MediationRecord`.
6. `SessionEnd` hook if subscribed (observe-only).

Timeouts are global *and* mid-frame bounded (a stalled plugin can't hang the
host between bytes). Shutdown is cooperative (close stdin → EOF) then forced
(`kill` after a grace window measured by the injected `Clock`).

`StdProcessRunner` is the production runner; the enforcement point is literally
`Command::new(program).env_clear().envs(allowlist)`.

## 6. Hooks: the narrow-only invariant

```rust
enum HookKind { SessionStart, BeforeCapability, SessionEnd }
enum HookOutcome { Continue, Veto { reason: String } }
```

- `SessionStart` / `SessionEnd` are **Observe** — replies are ignored by
  definition, so they can never influence control flow.
- `BeforeCapability` is dispatched **only on the policy-Allow path**. The
  plugin's hook may return `Veto` to deny an already-permitted call (defense
  in depth). The host **never** dispatches it on a `Deny`, so a hook can
  never turn `Deny` into `Allow`.

> This is the security cornerstone: **authority only ever shrinks.** A
> manifest is the ceiling; policy and hooks can only lower it.

## 7. Writing a plugin (the SDK)

The plugin side is `plugin_sdk::PluginConnection` — it owns all framing,
correlation ids and hook servicing. A plugin author writes typed requests:

```rust
use rstui_plugin_host::capability::CapabilityRequest;
use rstui_plugin_host::plugin_sdk::PluginConnection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut host = PluginConnection::connect(std::io::stdin().lock(),
                                             std::io::stdout().lock())?;

    // Optional: reject an incompatible host
    if !host.host_api_version().starts_with("0.") { return Err("bad host".into()); }

    // Optional: narrow our own authority further
    host.set_hook_handler(|_kind, _input| HookOutcome::Continue);

    match host.request(&CapabilityRequest::Env { key: "PATH".into() })? {
        CapabilityResponse::Ok { payload }   => { /* use it */ }
        CapabilityResponse::Denied { reason } => eprintln!("denied: {reason}"),
        CapabilityResponse::Failed { error }  => eprintln!("failed: {error}"),
    }
    host.log("done")?;
    Ok(())
}
```

```rust
PluginConnection::connect(reader, writer) -> Result<Self, SdkError>
.set_hook_handler(impl FnMut(HookKind, &[u8]) -> HookOutcome)
.host_api_version() -> &str
.request(&CapabilityRequest) -> Result<CapabilityResponse, SdkError>
.log(&str) -> Result<(), SdkError>
```

## 8. End to end

The `permissioned_plugin` example wires it all up with in-memory fakes:
parse a manifest, script a fake plugin that makes four capability calls (an
allowed read, a path-escaping read, an allowed env, a denied env), run the
host, and assert that **exactly the two granted calls reached the effector**.

```sh
cargo run -p rstui-plugin-host --example permissioned_plugin
```

Because every seam (`ProcessRunner`, `PermissionPolicy`, `HostEffects`,
`Clock`) is fakeable, that same flow is a deterministic `#[test]` — the
security properties are CI-enforced, not aspirational.

See it used inside a real TUI in the [ACP client](acp-client.md).
