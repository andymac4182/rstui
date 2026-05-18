# ADR 0021: Reusable plugin-framework core vs. ACP vocabulary layer

- **Status:** Accepted
- **Date:** 2026-05-18
- **Relates to:** [ADR 0007](0007-plugin-host-and-secure-execution.md),
  [ADR 0016](0016-shared-memory-plugin-transport.md),
  [ADR 0019](0019-node-shared-memory-via-napi.md)

## Context

The plugin stack grew up inside `rstui-acp-client`, so the SDK crate
`rstui-acp-plugin-sdk` mixes two very different things:

1. **App-agnostic framework** — JSON-RPC 2.0 envelope, the transport
   trait + every transport (stdio, length-prefixed, Unix socket,
   WebSocket, shared memory), and the serve loop (handshake → decode →
   dispatch → encode) with its `--shm/--uds/--ws/--lp` auto-selection.
2. **ACP-specific vocabulary** — `proto`'s `HostEvent`/`PluginAction`
   enums, their JSON-RPC method names, and the ergonomic `Plugin`/`Host`
   surface, all shaped by the ACP chat-client domain.

Only (2) is ACP. (1) is a generic, well-tested JSON-RPC plugin host that
any TUI/app could reuse — but it is not reusable while the serve loop is
hard-coded to `HostEvent::Shutdown` /
`message_to_host_event` / `plugin_action_to_message` / a fixed
`initialize` ack.

A maintainer asked to **separate the core framework from the ACP parts so
other applications can build plugin systems on it.**

## Decision drivers

- Other apps must define **their own** event/action vocabulary and method
  names without forking the SDK.
- **Zero churn** for what exists: the 8 reference plugins, the client,
  and the TS plugins must compile and behave identically — the ACP API
  stays byte-stable.
- Keep the strict workspace posture (gates, no new licences, the
  per-crate `unsafe` boundary already established).
- Don't rename crates whose names are now historical but load-bearing
  (`rstui-acp-shm`, the `@rstui-acp/plugin-shm-native` npm package): the
  churn (ADRs, CI, published package name) outweighs the naming clarity;
  document them as generic instead.

## Decision

Extract **`rstui-plugin-core`** — the app-agnostic framework — and make
**`rstui-acp-plugin-sdk`** a thin ACP layer on top of it.

`rstui-plugin-core` owns:

- `jsonrpc` (`Message`/`Kind`/`RpcError`), `transport` (the `Transport`
  trait + `IoTransport`/`LpTransport`/`StdioTransport`/`ShmTransport`),
  `ws` (`WsTransport`) — moved verbatim, no behaviour change.
- `host`: a generic **`Protocol`** trait —

  ```text
  trait Protocol {
      type Event; type Action;
      fn initialize_ack(&self) -> Option<serde_json::Value>;
      fn decode_event(&self, &Message) -> Option<Self::Event>;
      fn encode_action(&self, &Self::Action) -> Message;
      fn is_shutdown(&self, &Self::Event) -> bool;
  }
  ```

  and the serve loop + every transport selector
  (`serve`/`serve_stdio_lp`/`serve_unix`/`serve_ws`/`serve_shm`/
  `serve_auto`) **generic over `Protocol`**. The `initialize` handshake
  method name stays in core (a JSON-RPC convention shared by LSP/MCP/ACP);
  only its *ack payload* is app-supplied.

`rstui-acp-plugin-sdk` keeps `proto` (the ACP `HostEvent`/`PluginAction`),
adds a zero-sized `AcpProtocol: Protocol`, and re-exposes the existing
`serve*`/`Plugin`/`Host` with **identical signatures** as thin wrappers
that bind `AcpProtocol`. Its public API is unchanged; downstream is
untouched.

`rstui-acp-shm` / `rstui-acp-shm-native` are **generic transports**; the
`acp` in their names is historical. They are documented as reusable and
not renamed (churn ≫ benefit).

The TS SDK is split the same way: `sdk/ts/core.mjs` (transports +
transport-agnostic bridge core, generic over an action↔method map + ack)
and `sdk/ts/index.mjs` (ACP `definePlugin` + host vocabulary on top), API
stable.

## Consequences

- **Reusable:** a new app adds a crate, defines its `Event`/`Action` +
  one `Protocol` impl, and gets every transport + auto-selection for
  free. A doctest/example in `rstui-plugin-core` proves a non-ACP
  protocol end to end.
- **Stable:** `rstui-acp-plugin-sdk`'s surface is byte-for-byte the same
  (re-exports + same-signature wrappers); the 8 plugins, the client, and
  the tests build and run unchanged — enforced by the gate.
- **Boundary is explicit:** ACP vocabulary cannot leak into core (core
  doesn't depend on the SDK; the dependency is one-way
  core ← acp-sdk ← client).
- **Deferred:** renaming the `*-acp-shm*` crates / npm package to a
  neutral name — recorded here as intentional debt; the docstrings say
  "generic, name historical" so a new app is not misled.
