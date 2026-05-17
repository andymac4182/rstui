# Plugin runtime & transport profile

Same plugin (`fortune`) under each host × transport, so the numbers
isolate **runtime + JSON-RPC transport overhead**, not plugin logic.

- **startup** — spawn → `initialize` ack (cold process + handshake), median of 15.
- **latency** — single in-flight `command/invoke` → `ui/note` round-trip; p50/p95 of 500 (after 300 warm-up).
- **throughput** — 3000 `command/invoke` pipelined, messages/sec.
- **transports** — `stdio` (newline JSON), `stdio-lp` (length-prefixed
  binary framing — u32 BE length + JSON bytes, no newline scan), `uds`
  (Unix-domain socket, newline), `uds-lp` (UDS + length-prefixed), `ws`
  (RFC 6455 over loopback TCP). All five carry the identical JSON-RPC 2.0
  payload — only the framing/socket differs. See OPTIMISATION.md.

Environment: Darwin 25.3.0 · arm64 · Apple M1 Pro · node v24.15.0 · bun 1.3.5 · rustc 1.95.0 (59807616e 2026-04-14).
Single machine, single run, loopback websockets — **indicative**, not a benchmark suite.
Reproduce: `cargo build --release -p rstui-acp-client --bin rstui-acp-plugin-fortune && node sdk/bench/bench.mjs`.

| host | transport | startup (ms) | latency p50 (µs) | latency p95 (µs) | throughput (msg/s) |
|------|-----------|-------------:|-----------------:|-----------------:|-------------------:|
| rust | stdio | 6.7 | 23 | 60 | 244,945 |
| rust | stdio-lp | 4.9 | 29 | 448 | 207,750 |
| rust | uds | 29.7 | 23 | 255 | 149,310 |
| rust | uds-lp | 29.7 | 24 | 71 | 126,431 |
| rust | ws | 55.3 | 86 | 212 | 107,420 |
| node | stdio | 36.4 | 33 | 119 | 160,849 |
| node | stdio-lp | 34.2 | 29 | 79 | 172,617 |
| node | uds | 55.0 | 35 | 98 | 192,125 |
| node | uds-lp | 55.1 | 37 | 82 | 157,729 |
| node | ws | 56.6 | 86 | 183 | 116,471 |
| bun | stdio | 28.3 | 44 | 349 | 223,693 |
| bun | stdio-lp | 26.0 | 43 | 166 | 153,408 |
| bun | uds | 30.4 | 30 | 193 | 187,237 |
| bun | uds-lp | 30.8 | 32 | 101 | 173,379 |
| bun | ws | 57.7 | 90 | 203 | 68,217 |

## Reading it (derived from the numbers above)

- **Fastest cold start:** rust/stdio-lp (4.9 ms).
- **Lowest round-trip latency (p50):** rust/stdio (23 µs).
- **Highest throughput:** rust/stdio (244,945 msg/s).
- **Transports by mean throughput** (msg/s, across hosts): `stdio` 209,829 › `stdio-lp` 177,925 › `uds` 176,224 › `uds-lp` 152,513 › `ws` 97,369 — the fastest local framing is ~2.2× websocket.
  websocket carries RFC 6455 framing + a loopback TCP hop; length-prefixed
  framing removes the newline scan/concat; a Unix-domain socket removes the
  TCP/IP stack. Use the lowest-overhead local transport for co-located
  plugins; websocket only when a plugin must be remote/long-lived.
- Numbers vary run-to-run (JIT warm-up, scheduler) — treat as orders of
  magnitude, not exact. Rust avoids VM warm-up so its cold start and tail
  latency are the most consistent.
- Every host speaks the **identical JSON-RPC 2.0 wire** — the runtime and
  transport choice is purely performance/operational, never a protocol
  difference.
