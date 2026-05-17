# ADR 0016: Shared-memory plugin transport (opt-in, Rust-only, spin)

- **Status:** Accepted (shipped — phases 1–4 landed)
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Relates to:** [ADR 0003](0003-lint-and-code-quality-policy.md)
  (`unsafe_code = forbid`), [ADR 0007](0007-plugin-host-and-secure-execution.md)
  (plugin host), [ADR 0005](0005-benchmarking-and-profiling-strategy.md)

## Context

The plugin SDK speaks JSON-RPC 2.0 over stdio / Unix-domain socket /
WebSocket, with optional length-prefixed framing. Profiling
(`sdk/bench/OPTIMISATION.md` §1a, `examples/rtt.rs`) established the real
Rust↔Rust stdio floor: **p50 5.3 µs (`--lp`), p95 ~16 µs**. p50 already
meets a "< 10 µs" target; p95 does not, and *cannot* over pipes — ~16 µs
p95 is the OS scheduler waking a **sleeping** process, inherent to any
two-process pipe/socket model. No framing or serializer change removes a
context switch.

A maintainer asked to push further and explicitly chose to **explore a
shared-memory transport**. This ADR records the design, the measured
evidence, and the consequences — so the decision to build (or not) the
production transport is made with the costs visible, not re-derived.

## Decision drivers

- **Tail latency, not just median.** The open item is p95/p99, which only
  a wake-up-free path can fix.
- **`unsafe_code = forbid` is workspace policy** (ADR 0003), opted into
  per-crate via `[lints] workspace = true`. Shared memory requires
  `mmap` + atomics over mapped memory — irreducibly `unsafe`.
- **Zero-dep / strict licence budget** (`deny.toml`: Apache-2.0/MIT/
  Unicode-3.0 only). New crates under other licences fail the gate.
- **Two SDKs, one wire.** The TS SDK is a first-class goal; a transport
  Node cannot speak narrows the architecture.
- **Portability.** CI is Linux; local dev/test is macOS. The transport
  must compile and run on both.
- **A TUI host runs many plugins.** A technique that pins a core per
  plugin cannot be the default.

## Options considered

1. **stdio/uds + futex/eventfd doorbell.** Fewer syscalls than a full
   pipe message, but the wake is *still a scheduler wakeup* — p95 stays
   ≈ the pipe tail (~16 µs). Does **not** solve the stated problem.
2. **Shared-memory ring + futex park.** Lower mean (no pipe copy, no
   two write/read syscalls) but, again, a parked-then-woken peer pays a
   scheduler wakeup → tail not flat. Solves mean, not p95.
3. **Shared-memory ring + *scoped* adaptive spin.** Plugin RPC is
   request/response, so the latency-critical wait is *bounded and
   predictable*: the caller spins only in the window between "request
   sent" and "response seen" (µs for an shm-fast plugin); the callee
   spins a short *stay-hot* window after each message, then **parks on
   a named semaphore**. No "spin for the whole turn" — spin is scoped to
   each message exchange. This is the *only* option with a flat sub-µs
   tail on the hot path, and — measured — it does **not** burn a core
   (it parks between exchanges; see Evidence). Cost: `unsafe`; Rust-only;
   a *cold* message arriving after the stay-hot window pays one
   semaphore wake ≈ the stdio tail (~16 µs) — but a cold post-idle
   message is by definition not the latency-critical one.
4. **Do nothing** — ship the landed stdio result (p50 < 10 µs).

## Decision

Offer a shared-memory transport as **option 3**, strictly **opt-in**,
**Rust-plugin-only**, with the `unsafe` **isolated in a dedicated crate**
that does *not* inherit the workspace lints (consistent with ADR 0003's
per-crate opt-in model — the forbid is a default, not an inviolable
global; a scoped, audited, reviewed exception in one clearly-named
low-level crate is the sanctioned escape hatch). It uses **`libc`/`rustix`
already in the dependency graph** (libc 0.2, rustix 1.1 via crossterm —
both deny-allowed) so it adds **no new dependency or licence**. No
futex/eventfd: an `mmap`'d `MAP_SHARED` tmpfile + portable adaptive spin
works on macOS and Linux alike.

It does **not** replace stdio/uds; `serve_auto`/`bridge()` keep their
precedence and stdio stays the default. shm is selected only by an
explicit `--shm <path>` for a plugin the host has deliberately marked
latency-critical. The spin is **scoped to each request→response
exchange** (caller) plus a short post-message stay-hot window (callee),
then both **park** — so a quiescent plugin uses ~0 % CPU.

Implementation was **phased** (below) and is now **shipped** — all four
phases landed on `main`, each its own gated slice, stdio still the
default.

## Evidence

Throwaway prototype (two processes, one `MAP_SHARED` tmpfile, two
cache-line-separated atomic doorbells, adaptive spin), Apple M1 Pro,
release, 500 000 iterations, 128-byte payload, single in-flight:

| transport | p50 | p95 | p99 | p99.9 | tail |
| --- | --: | --: | --: | --: | --- |
| stdio (`--lp`, ADR §1a) | 5.3 µs | ~16 µs | — | — | scheduler-bound |
| **shm ring + spin** | **0.125 µs** | **0.50 µs** | **0.625 µs** | **0.708 µs** | **flat** |

min 41 ns. ~**40× lower p50** than stdio and a flat tail through p99.9.
A single 1 ms `max` over 500 k iters is one OS preemption of the spin
thread (expected on a non-real-time OS without core pinning; p99.9 =
708 ns shows it is a lone blip, not the distribution).

A second prototype answered "does this burn a core?". With **scoped**
adaptive spin (caller spins only request→response; callee a 200 µs
stay-hot window then parks on a named POSIX semaphore): the hot-path
distribution is **unchanged** (pipelined p50 **0.125 µs**, p99
**0.583 µs**, flat), and between exchanges both processes were observed
at **0.0 % CPU** (`ps`) — parked on the semaphore, not spinning. So the
flat sub-µs tail is preserved for an active exchange while average CPU
is *negligible*: the cost is microseconds of spin **per RPC**, not a
pinned core. The premise is validated and the core-burn objection is
disproved: scoped shm+spin crushes "< 10 µs" including p99 **without**
monopolising a core at realistic plugin cadence.

## Consequences

**Makes easy:** sub-µs, flat-tail RTT for a designated latency-critical
Rust plugin — interactive/streaming plugins that would feel pipe jitter.

**Makes hard / accepted costs:**

- **Rust-only.** Node/Bun cannot `mmap` shared memory or do lock-free
  atomics without a native addon, which would break the dependency-free
  TS posture (ADR 0007 lineage). TS plugins keep stdio/uds — this fast
  path is Rust-plugin-only, and that is documented, not hidden.
- **CPU.** *Scoped* spin means a core is hot only inside a
  request→response window (µs) and a short post-message stay-hot window;
  between exchanges both ends park (measured ~0 % CPU). Average cost is
  `spin_window × request_rate` — a rounding error at any realistic
  plugin event rate, **not** a pinned core. Two residual rules still
  hold: (a) keep it opt-in / single-few, since N tight-looping shm
  plugins would still add up; (b) a *cold* message after the stay-hot
  window pays one semaphore wake ≈ the stdio tail — acceptable because
  cold post-idle messages are not the latency-critical ones.
- **`unsafe`.** Confined to one audited crate (`rstui-acp-shm`,
  `[lints]` not inheriting the workspace forbid; every `unsafe` block
  carries a `SAFETY:` justification; reviewed as its own slice). The
  SDK and all other crates stay `unsafe`-free.
- **Lifecycle.** A spinning host must detect a dead plugin (peer-liveness
  epoch in the shared header + a spin budget ceiling) and the tmpfile
  must be cleaned up; a real SPSC ring with variable-length framing and
  backpressure replaces the prototype's single ping-pong slot.

**Shipped (phased plan — all landed):**

1. ✅ Prototype + measurement (this ADR's evidence).
2. ✅ `rstui-acp-shm` crate: `MAP_SHARED` SPSC ring, length-framed,
   peer-liveness watchdog, scoped adaptive spin → semaphore park;
   isolated audited `unsafe` (the sole sanctioned `unsafe` crate).
3. ✅ `transport::ShmTransport` + `serve_shm`/`serve_plugin_shm` +
   `serve_auto` `--shm`/`RSTUI_PLUGIN_SHM` (precedence shm → uds → ws →
   stdio); `examples/rtt.rs` has a shm row.
4. ✅ Client host: a `--shm` token in a plugin's launch command opts it
   in — the host owns the segment (creator) and drives it on a dedicated
   thread (adaptive: hot during an exchange, 1 ms idle poll); the
   default stdio path is byte-for-byte unchanged. Docs: SDK README +
   OPTIMISATION.md.

**Measured end-to-end through the full SDK JSON-RPC stack** (M1 Pro,
50 k iters, single in-flight): shm **p50 ≈ 1.3 µs, p95 ≈ 3.3 µs** vs
stdio `--lp` p50 ≈ 10 µs / p95 ≈ 70 µs on the same run — ~8× lower p50,
p95 decisively < 10 µs *including* serde. `ShmChannel` adds `try_recv`/
`is_closed` for the asymmetric host driver. Each phase landed as its own
gated slice; stdio remains the default.
