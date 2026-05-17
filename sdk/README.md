# rstui-acp-client plugin SDKs

Plugins are **separate processes** that speak **JSON-RPC 2.0** (the same wire
ACP and MCP use) to the client over a transport — **stdio or WebSocket**
(same `Message`, different framing). The WebSocket server is dependency-free
(hand-rolled RFC 6455: inline SHA-1/base64 handshake, masked/unmasked text
frames) so the strict workspace dependency/`cargo deny` budget is untouched.
A Rust plugin chooses its transport with `serve(...)` (stdio) or
`serve_ws(addr, ...)` / `serve_plugin_ws(addr, p)` (WebSocket). Two SDKs,
one wire:

| SDK | Language | Role |
|---|---|---|
| `rstui-acp-plugin-sdk` (`crates/`) | Rust | Owns the **whole** comms stack (framing, handshake, dispatch). A Rust plugin only writes handlers. |
| `@rstui-acp/plugin-sdk` (`sdk/ts`) | TypeScript | Marshals handlers ↔ the **V8 host** bridge. It never touches stdio — the host owns the transport. |

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

Run it via the **V8 host** (`sdk/v8-host`):

```sh
rstui-acp-client --plugin "node sdk/v8-host/host.mjs ./my-plugin.mjs"
```

- With `secure-exec` installed (`npm i secure-exec`), the plugin runs in an
  isolated **V8 sandbox** with **deny-by-default** fs/network — untrusted
  code is contained (ADR 0007 posture, in JS).
- Without it, the host runs the plugin in-process (dev mode, **not**
  sandboxed; a stderr warning says so). Same bridge, same wire — "our own
  host, learning from secure-exec".
- `--sandbox` requires secure-exec (errors if absent).

## Verify

```sh
node sdk/v8-host/test/smoke.mjs   # full JSON-RPC round-trip, no deps/network
```

Drives the handshake, a command, a modal request/response, and shutdown
against the sample `sdk/examples/clock.plugin.mjs`.
