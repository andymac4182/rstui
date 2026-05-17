# Plugin runtime & transport profile

Same plugin (`fortune`) under each host × transport, so the numbers
isolate **runtime + JSON-RPC transport overhead**, not plugin logic.

- **startup** — spawn → `initialize` ack (cold process + handshake), median of 15.
- **latency** — single in-flight `command/invoke` → `ui/note` round-trip; p50/p95 of 500 (after 300 warm-up).
- **throughput** — 3000 `command/invoke` pipelined, messages/sec.

Environment: Darwin 25.3.0 · arm64 · Apple M1 Pro · node v24.15.0 · bun 1.3.5 · rustc 1.95.0 (59807616e 2026-04-14).
Single machine, single run, loopback websockets — **indicative**, not a benchmark suite.
Reproduce: `cargo build --release -p rstui-acp-client --bin rstui-acp-plugin-fortune && node sdk/bench/bench.mjs`.

| host | transport | startup (ms) | latency p50 (µs) | latency p95 (µs) | throughput (msg/s) |
|------|-----------|-------------:|-----------------:|-----------------:|-------------------:|
| rust | stdio | 12.0 | 62 | 941 | 66,481 |
| rust | ws | 61.7 | 227 | 1,794 | 24,483 |
| node | stdio | 57.9 | 124 | 2,324 | 53,232 |
| node | ws | 118.6 | 298 | 7,675 | 12,944 |
| bun | stdio | 81.6 | 163 | 7,927 | 50,612 |
| bun | ws | 79.6 | 274 | 4,180 | 9,248 |

## Reading it (derived from the numbers above)

- **Fastest cold start:** rust/stdio (12.0 ms).
- **Lowest round-trip latency (p50):** rust/stdio (62 µs).
- **Highest throughput:** rust/stdio (66,481 msg/s).
- **stdio beats websocket on throughput** by roughly rust 2.7×, node 4.1×, bun 5.5×
  (websocket adds RFC 6455 framing + a loopback TCP hop). Use stdio for
  local plugins; websocket when the plugin must be remote/long-lived.
- Numbers vary run-to-run (JIT warm-up, scheduler) — treat as orders of
  magnitude, not exact. Rust avoids VM warm-up so its cold start and tail
  latency are the most consistent.
- Every host speaks the **identical JSON-RPC 2.0 wire** — the runtime
  choice is purely performance/operational, never a protocol difference.
