# rstui-acp-client plugin SDKs

Plugins are **separate processes** that speak **JSON-RPC 2.0** (the same wire
ACP and MCP use) to the client over a transport — **stdio, Unix-domain
socket, WebSocket, or shared memory**, with optional **length-prefixed binary
framing** (`--lp`, a u32-BE length + JSON bytes — no newline scan) on the
stdio/uds paths (same `Message`, different framing). The WebSocket server is
dependency-free (hand-rolled RFC 6455: inline SHA-1/base64 handshake,
masked/unmasked text frames) so the strict workspace dependency/`cargo deny`
budget is untouched. **Shared memory** (`--shm <path>`, ADR 0016) is the
lowest-latency local transport — flat sub-µs RTT via an mmap SPSC ring with
scoped-spin/semaphore park — but is **opt-in and Rust-plugin-only** (Node
can't `mmap` addon-free); all its `unsafe` is isolated in `rstui-acp-shm`.
A Rust plugin can pick its transport explicitly (`serve` = stdio,
`serve_stdio_lp`, `serve_unix(path, lp, …)`, `serve_ws(addr, …)`,
`serve_shm(path, …)`) or just call `serve_auto(…)` and let
`--shm`/`--uds`/`--ws`/`--lp` (or `RSTUI_PLUGIN_SHM`/`_UDS`/`_WS`/`_LP`)
choose; the TS SDK's `bridge()` mirrors the same selection and precedence
(minus shm). See `sdk/bench/OPTIMISATION.md` for the overhead analysis and
the QUIC / protobuf / Cap'n Proto evaluation. Two SDKs, one wire:

Since **ADR 0021** the comms stack is an **app-agnostic framework** with a
thin ACP layer on top, so other applications reuse it (see *Reusing the
framework* below):

| Layer | Language | Role |
|---|---|---|
| `rstui-plugin-core` (`crates/`) | Rust | The framework: JSON-RPC envelope, every transport, and a `Protocol`-generic serve loop (`serve_auto`/…). **No app vocabulary.** |
| `rstui-acp-plugin-sdk` (`crates/`) | Rust | Thin ACP layer: `proto` (`HostEvent`/`PluginAction`) + `AcpProtocol` + ergonomic `Plugin`/`Host`. Re-exports core; **API byte-stable**. |
| `@rstui-acp/plugin-sdk/core` (`sdk/ts/core.mjs`) | JS | The framework: transports + `bridge(proto)`, generic over `{ actionMethod, initializeResult, isShutdown }`. **No app vocabulary.** |
| `@rstui-acp/plugin-sdk` (`sdk/ts/index.mjs`) | JS | Thin ACP layer: `ACTION_METHOD` + `definePlugin` host surface, on `core.mjs`. **API unchanged.** |

## Wire (JSON-RPC 2.0)

- Host → plugin: `initialize` (request → ack), then notifications
  `session/start`, `session/prompt`, `session/turnEnded`, `command/invoke`,
  `modal/response`, `askUser/response`, `tick`, `shutdown`.
- Plugin → host (notifications): `commands/register`,
  `ui/registerKeybinding`, `ui/setStatus`, `ui/footer`, `ui/panel`,
  `ui/note`, `ui/log`, `ui/modal`, `ui/askUser`.
- `params` carries the typed payload (`{"type":"set_status",…}`), so the
  Rust and TS SDKs are byte-compatible.

## Reusing the framework in another app (ADR 0021)

The transports + serve loop are **not ACP-specific**. To give *your* app a
plugin system, depend on the core and bring your own vocabulary — no fork:

- **Rust:** depend on `rstui-plugin-core`, define your own `Event`/`Action`
  enums and one `impl Protocol` (`initialize_ack` / `decode_event` /
  `encode_action` / `is_shutdown`), then call `serve_auto(MyProto, |ev,
  emit| { … })`. Every transport (stdio/lp/uds/ws/shm) +
  `--shm/--uds/--ws/--lp` selection comes for free. A compile-tested
  end-to-end example of a non-ACP protocol is the doctest in
  `rstui-plugin-core`'s crate docs.
- **JS/TS:** `import { bridge } from "@rstui-acp/plugin-sdk/core"` (i.e.
  `sdk/ts/core.mjs`), pass `proto = { actionMethod, initializeResult,
  isShutdown }`, and drive `feed`/`nextObj`/`emitObj` with your own loop —
  `definePlugin` (ACP) is just one such loop built on it.

ACP stays the reference consumer; its API is byte-stable. **Naming
caveat:** `rstui-acp-shm` / `rstui-acp-shm-native` and the
`@rstui-acp/plugin-shm-native` npm package are **generic transports** —
the `acp` is historical (ADR 0021), they carry no ACP vocabulary and are
reusable by any app.

## Capabilities (opencode/pi parity)

Slash commands, keyboard shortcuts, modals, ask-user overlays, sidebar
panels, powerline footer segments, status keys, toasts/notes, logs.

## Rust plugin

```rust
use rstui_acp_plugin_sdk::{serve, HostEvent, PluginAction};
fn main() {
    serve(|event, emit| {
        if let HostEvent::Init { .. } = event {
            emit(PluginAction::RegisterCommand { name: "hi".into(), description: "say hi".into() });
        }
    });
}
```
Run: `rstui-acp-client --plugin /path/to/your-rust-plugin`

## TypeScript plugin (V8 host)

```ts
import { definePlugin } from "@rstui-acp/plugin-sdk";
await definePlugin({
  onInit(_i, host) {
    host.registerCommand("hi", "say hi");
    host.registerKeybinding("ctrl+h", "hi", "say hi");
  },
  async onCommand(name, _args, host) {
    if (name === "hi") {
      const b = await host.modal("Hello", ["from a TS plugin"], ["OK"]);
      host.note(`you pressed ${b}`);
    }
  },
});
```

A TS plugin is **just a process** speaking the wire — same as a Rust
plugin. The SDK has a built-in stdio bridge, so **no V8 host is needed**:

```sh
# Plain process (uniform with Rust plugins):
rstui-acp-client --plugin "node ./my-plugin.mjs"
# …or bun ./my-plugin.ts (native TS, no build step)
```

**Recommended hardening — zero dependencies (Node Permission Model):**

```sh
rstui-acp-client --plugin "node --permission --allow-fs-read=. ./my-plugin.mjs"
```

This denies fs-write / child-process / workers / native addons. Network is
not gated by Node yet — run under an OS sandbox (`sandbox-exec`,
container) if network isolation is required (operator concern, ADR 0007).

The optional **V8 host** (`sdk/v8-host`) adds a `--harden` convenience
(re-execs under the Permission Model for you) and an opt-in, **experimental
`--sandbox`** that runs the plugin in a `secure-exec` V8 isolate:

```sh
rstui-acp-client --plugin "node sdk/v8-host/host.mjs --harden ./my-plugin.mjs"
rstui-acp-client --plugin "node sdk/v8-host/host.mjs --sandbox ./my-plugin.mjs"  # experimental
```

**Why this design (not secure-exec by default): see
[`RUNTIME_DECISION.md`](./RUNTIME_DECISION.md).** `--sandbox` is a
work-in-progress (secure-exec is pre-1.0, native, and its bounded `run()`
fights our long-lived loop); the supported path is the process model +
Node permissions.

## Verify

```sh
node sdk/v8-host/test/smoke.mjs   # full JSON-RPC round-trip, no deps/network
```

Drives the handshake, a command, a modal request/response, and shutdown
against the sample `sdk/examples/clock.plugin.mjs`.
