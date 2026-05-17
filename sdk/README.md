# rstui-acp-client plugin SDKs

Plugins are **separate processes** that speak **JSON-RPC 2.0** (the same wire
ACP and MCP use) to the client over a transport — **stdio, Unix-domain
socket, or WebSocket**, with optional **length-prefixed binary framing**
(`--lp`, a u32-BE length + JSON bytes — no newline scan) on the stdio/uds
paths (same `Message`, different framing). The WebSocket server is
dependency-free (hand-rolled RFC 6455: inline SHA-1/base64 handshake,
masked/unmasked text frames) so the strict workspace dependency/`cargo deny`
budget is untouched. A Rust plugin can pick its transport explicitly
(`serve` = stdio, `serve_stdio_lp`, `serve_unix(path, lp, …)`,
`serve_ws(addr, …)`) or just call `serve_auto(…)` and let `--uds`/`--ws`/
`--lp` (or `RSTUI_PLUGIN_UDS`/`_WS`/`_LP`) choose; the TS SDK's `bridge()`
mirrors the same selection and precedence. See `sdk/bench/OPTIMISATION.md`
for the overhead analysis and the QUIC / protobuf / Cap'n Proto evaluation.
Two SDKs, one wire:

| SDK | Language | Role |
|---|---|---|
| `rstui-acp-plugin-sdk` (`crates/`) | Rust | Owns the **whole** comms stack (framing, handshake, dispatch). A Rust plugin only writes handlers. |
| `@rstui-acp/plugin-sdk` (`sdk/ts`) | TypeScript | Marshals handlers ↔ the transport. `bridge()` auto-selects the injected **V8-host** bridge, else a standalone stdio / uds / ws transport (with optional `--lp` framing) — same selection/precedence as the Rust `serve_auto`. |

## Wire (JSON-RPC 2.0)

- Host → plugin: `initialize` (request → ack), then notifications
  `session/start`, `session/prompt`, `session/turnEnded`, `command/invoke`,
  `modal/response`, `askUser/response`, `tick`, `shutdown`.
- Plugin → host (notifications): `commands/register`,
  `ui/registerKeybinding`, `ui/setStatus`, `ui/footer`, `ui/panel`,
  `ui/note`, `ui/log`, `ui/modal`, `ui/askUser`.
- `params` carries the typed payload (`{"type":"set_status",…}`), so the
  Rust and TS SDKs are byte-compatible.

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
