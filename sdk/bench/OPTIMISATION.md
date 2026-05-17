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

## 1a. The real stdio RTT (Rust↔Rust) — and getting under 10 µs

`RESULTS.md` measures a Rust plugin against a **Node** harness, so its
latency carries libuv + V8 + Promise/microtask overhead *on the measuring
side*. That is not the production path. The real consumer is the Rust
client driving an SDK plugin over pipes. `examples/rtt.rs` removes Node:
a tight Rust host loop times the round-trip against the real SDK plugin.

Apple M1 Pro, release, single in-flight, 50 000 iterations:

| framing | min | **p50** | p95 |
|---|--:|--:|--:|
| stdio (newline) | 4.6 µs | **6.5 µs** | 18 µs |
| stdio-lp | 4.4 µs | **5.3 µs** | 16 µs |

So **typical stdio RTT is already < 10 µs** (5.3 µs with `--lp`); the
22 µs in `RESULTS.md` was ~75 % Node-harness overhead, not transport.
What is left in the ~5 µs is irreducible at the OS level: two pipe
traversals + **two process wake-ups** + one JSON encode/decode each way.
`min` ≈ 4.5 µs is the wake-up+syscall floor on this machine.

What we changed to get here, and the reasoning per layer:

- **One `write()` syscall per message.** `LpTransport::send` now
  serialises after a 4-byte length placeholder and emits the whole frame
  in a single `write_all` + flush (was length-then-body, two buffered
  writes). Syscall count — not JSON — is what a local RTT is made of, so
  removing one write per direction is a real ~p50 win, and it is why
  `stdio-lp` now beats newline (5.3 vs 6.5 µs): exact-length reads + a
  single write, no newline scan.
- **Nothing else in the transport is on the critical path.** The read
  side is already one `read()` into a `BufReader`; JSON is ~1.3 µs (§1).

**p95 is the open item, and it is not a transport bug.** ~16 µs p95 is
the OS scheduler waking a *sleeping* process — inherent to a two-process
pipe model. No serializer or framing change removes a context switch.
The only ways to push p95 (and p99) under 10 µs:

1. **Spin/poll mode** — don't sleep in `read()`; set the fd non-blocking
   and busy-poll. RTT collapses to ~2–4 µs *including the tail* (no
   wake-up). Cost: one core pinned at 100 % per spinning end, and
   non-blocking stdin needs `fcntl`/`O_NONBLOCK` → `libc` + an `unsafe`
   FFI call. This repo is `unsafe_code = forbid` and zero-dep-by-default,
   so it is a **policy decision**, gated behind an opt-in
   `--spin`/`RSTUI_PLUGIN_SPIN` for the rare latency-critical plugin —
   never the default (a TUI host with many plugins must not burn N cores).
2. **Shared-memory ring + *scoped* spin** — measured p50 **125 ns**,
   p99 **625 ns**, flat tail through p99.9 (~40× lower p50 than stdio).
   futex/eventfd does *not* achieve this — a parked-then-woken peer
   still pays a scheduler wakeup; only busy-spin removes it. Spin is
   scoped to each request→response exchange + a short stay-hot window,
   then both ends **park** — measured ~0 % CPU between exchanges, so it
   does **not** burn a core at realistic cadence. Designed, measured,
   recorded in [ADR 0016](../../docs/adr/0016-shared-memory-plugin-transport.md):
   opt-in, **Rust-plugin-only** (Node can't mmap without a native addon),
   `unsafe` isolated to one audited crate, no new dependency. **Shipped**
   (ADR 0016 phases 1–4): `rstui-acp-shm` + SDK `serve_shm`/`serve_auto
   --shm` + client opt-in (a `--shm` token in a plugin's launch command;
   stdio stays the default, byte-for-byte unchanged). End-to-end through
   the full SDK JSON-RPC stack it measured **p50 ≈ 1.3 µs / p95 ≈ 3.3 µs**
   (vs stdio `--lp` ≈ 10 / 70 µs same run) — p95 < 10 µs *including* serde.

Recommendation: **default stdio with `--lp` already meets "< 10 µs"
for p50/typical** — ship that. Treat sub-10 µs *p95* as opt-in spin
mode, implemented only if a concrete plugin needs it and the
`unsafe`/dependency budget is explicitly opened for that path.

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
- **Rust `LpTransport` buffer reuse + single-write send.** `recv` no
  longer `vec![0u8; n]` per frame; `send` serialises straight into a
  reused buffer *after a 4-byte length placeholder* and writes the whole
  frame in **one `write_all` + flush** (was two buffered writes). A
  steady stream allocates once, not per message, and pays one `write()`
  syscall per direction — the §1a p50 win.

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
node sdk/bench/bench.mjs         # → sdk/bench/RESULTS.md (vs Node harness)
node sdk/bench/serde-micro.mjs   # JSON ser/de cost vs round-trip
cargo run --release --example rtt -p rstui-acp-plugin-sdk  # §1a true Rust↔Rust RTT
```
