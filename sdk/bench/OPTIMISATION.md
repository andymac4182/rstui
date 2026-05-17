# Plugin SDK — overhead analysis & optimisation

What actually costs time in a plugin round-trip, what we changed to cut
it, the transport menu, and an evidence-based answer to "should we use a
binary format / QUIC?". Numbers come from `bench.mjs` (→ `RESULTS.md`)
and `serde-micro.mjs`; reproduce both with the commands at the bottom.

> Single machine, single run, loopback — **indicative, not a benchmark
> suite**. p95 and throughput swing run-to-run (JIT warm-up, scheduler).
> Read the conclusions, not the third significant figure.

## TL;DR / recommendations

1. **Keep JSON-RPC 2.0 as the canonical wire.** It is the *same* wire as
   ACP and MCP; every host (Rust/Node/Bun) and every transport already
   speaks it byte-for-byte. That uniformity is worth more than the
   microseconds a binary codec would save (see §4 — for control messages
   JSON ser/de is ~2–5 % of a round-trip).
2. **Local plugins: Unix-domain socket or stdio; add `--lp` for binary
   framing.** No port, no TCP/IP stack. `serve_auto` / `bridge()` pick it
   from `--uds`/`--lp` (or `RSTUI_PLUGIN_UDS`/`RSTUI_PLUGIN_LP`).
3. **WebSocket only when a plugin must be remote or long-lived.** It is
   ~2.2× slower than the fastest local transport here (RFC 6455 framing +
   a loopback TCP hop) — that cost only buys you something off-box.
4. **QUIC / protobuf / Cap'n Proto: not now, with conditions (§4–§5).**
   They optimise problems this workload doesn't have (WAN loss/mobility;
   multi-MB structured payloads) at the cost of the shared-wire property.

## 1. Where the time goes

`serde-micro.mjs` (Node 24, 2 M iterations, JIT warm):

| message | bytes | JSON encode+decode |
|---|--:|--:|
| `command/invoke` event | 98 | ~0.70 µs |
| `ui/note` action | 122 | ~0.63 µs |
| `ui/panel` 200-line blob | 18 577 | ~23.5 µs |

A whole control-message round-trip therefore spends **~1.3 µs in JSON**
(event in + action out). The measured transport round-trip in
`RESULTS.md` is **23–90 µs**. So JSON ser/de is a **single-digit
percentage** of a round-trip; the rest is process scheduling, the pipe/
socket syscalls, and (for ws) RFC 6455 + a TCP hop. **The serializer is
not the bottleneck for the messages TUI plugins actually send.** The
blob row shows the *only* regime where a zero-copy codec would win —
multi-KB structured payloads, which the plugin vocabulary doesn't carry.

## 2. Hot-path optimisations applied

All three target the dominant cost (per-message work on the hot loop),
not the serializer:

- **Object fast-path across the TS core boundary.** The string-based
  `makeBridgeCore` previously `JSON.stringify`'d an action, immediately
  `JSON.parse`'d it inside `emit`, then `stringify`'d again — and on
  inbound, `stringify`'d params just so `definePlugin` could `parse`
  them. That is **2 redundant JSON conversions per round-trip**. Added
  `emitObj`/`nextObj` so the queue carries the *object*; `definePlugin`
  uses them. The legacy string `emit`/`next`/`feed` is retained, so the
  injected V8-host contract (and `smoke.mjs`) is byte-for-byte unchanged.
- **Length-prefixed decoder: O(n²) → O(n).** The naïve decoder did
  `Buffer.concat([buf, chunk])` on *every* chunk, re-copying the backlog
  under pipelining. Replaced with an offset cursor that advances through
  whole frames in place and, in the steady state (each chunk carries
  complete frames), simply *adopts* the next chunk — zero copy. Rust
  `stdio-lp` throughput moved ~156 k → ~208 k msg/s across re-runs.
- **Rust `LpTransport` buffer reuse.** `recv` no longer `vec![0u8; n]`
  per frame and `send` no longer allocates a temp `Vec` (serialises
  straight into a reused buffer via `serde_json::to_writer`). A steady
  stream now allocates once, not per message.

Net: the JSON-RPC round-trip now does **one decode + one encode** on the
TS side (down from up to three of each) and **zero per-message heap
allocation** on the Rust lp side.

## 3. The transport menu (and when each wins)

From `RESULTS.md` (mean throughput across hosts, this run):
`stdio` › `stdio-lp` ≈ `uds` › `uds-lp` › `ws` (ws ≈ ½ the fastest).

Honest reading of the noise: for **tiny** control messages on one box
the framing barely matters next to process + syscall cost — newline
stdio is hard to beat because V8's rope strings make line assembly almost
free. Length-prefix's real payoffs are (a) **exact reads / no newline
scan** → steadier tail latency and binary-safe payloads, and (b) a
cleaner contract for non-text data. UDS's payoff is **no TCP/IP stack and
a nameable socket** the host can own per plugin. WebSocket's cost only
buys you **off-box / long-lived** plugins.

| transport | use it when | cost |
|---|---|---|
| `stdio` (newline) | default; co-located, text JSON | none beyond a pipe |
| `stdio-lp` | co-located; want binary-safe framing / steady tail | tiny |
| `uds` | host manages plugins as sockets, co-located | ~connect setup |
| `uds-lp` | as `uds`, binary-safe framing | ~connect setup |
| `ws` | plugin is remote or must outlive the client | RFC 6455 + TCP |

## 4. Binary serialization (protobuf / Cap'n Proto) — evaluated, declined

**Claim under test:** "a binary format would reduce overhead."
**Evidence (§1):** JSON ser/de is ~1.3 µs of a 23–90 µs control-message
round-trip — **2–5 %**. protobuf/Cap'n Proto would shrink *that* slice
(and not to zero — they still walk fields), leaving ≥95 % of the
round-trip untouched. Net expected win on the real workload: **noise.**

**What it would cost:**

- **Breaks the shared wire.** JSON-RPC 2.0 is *intentionally* the same
  envelope as ACP and MCP. Every host and transport here interoperates
  with zero adapters precisely because the bytes are identical. A schema
  codec splits that into "JSON-RPC for ACP/MCP, protobuf for plugins" —
  the exact fragmentation this architecture set out to avoid.
- **Schema toolchain + dependency.** `.proto`/`.capnp` files, codegen in
  the build, a runtime lib in *every* SDK (Rust, TS) and the V8 host.
  The project is `unsafe_code = forbid`, zero-dep-by-default, and the V8
  host is dependency-light on purpose.
- **Cap'n Proto's headline feature (zero-copy access of large structured
  data) needs large structured data.** Plugin messages are <200 bytes.
  The blob row is the only place it'd matter, and the protocol doesn't
  send blobs.

**Reconsider if** a future plugin class streams multi-MB structured
payloads at high rate (e.g. shipping full file trees / large diffs every
frame). Then add a *content* encoding for that payload type only, behind
the existing JSON-RPC envelope (e.g. a base64/binary `params` blob with a
codec tag) — keep the envelope shared; don't replace the wire.

## 5. QUIC / native sockets — UDS done; QUIC evaluated, deferred

"Native sockets / Unix sockets" — **done**: UDS is implemented both sides
(`serve_unix` / `makeUdsBridge`) and benchmarked above. It is the
lowest-overhead *local* socket: no TCP/IP, no port.

**QUIC** (e.g. `quinn`) is the wrong layer for *local* plugin IPC:

- Its wins — connection migration, multiplexed independent streams,
  loss recovery, 0-RTT over the open internet — address **lossy,
  high-latency WAN** paths. Loopback/UDS has **zero loss and ~0 RTT**;
  none of those wins apply.
- Its costs are pure overhead here: a **TLS 1.3 handshake** on every
  connect, **userspace UDP** with congestion control, and a heavyweight
  async dependency. Against a UDS `connect()` that is essentially free,
  QUIC would *raise* the cold-start and per-connection cost the goal
  asked us to reduce.
- The one scenario QUIC fits — a **remote** plugin over the public
  internet with multiplexed streams — is already served (functionally)
  by the WebSocket transport, which also traverses proxies/firewalls
  that bare QUIC/UDP often can't.

**Reconsider if** plugins must run remotely at scale over unreliable
networks with many concurrent independent streams per plugin — then QUIC
becomes a *better remote* transport than WebSocket, added alongside it
(same JSON-RPC payload, transport-selected exactly like `ws`/`uds`
today). It is not a substitute for the local UDS/stdio path.

## Reproduce

```
cargo build --release -p rstui-acp-client --bin rstui-acp-plugin-fortune
node sdk/bench/verify.mjs        # correctness: 3 hosts × 5 transports
node sdk/bench/bench.mjs         # → sdk/bench/RESULTS.md
node sdk/bench/serde-micro.mjs   # JSON ser/de cost vs round-trip
```
