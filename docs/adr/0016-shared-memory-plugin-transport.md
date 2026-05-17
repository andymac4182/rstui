# ADR 0016: Shared-memory plugin transport (opt-in, Rust-only, spin)

- **Status:** Proposed
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
3. **Shared-memory ring + adaptive spin.** Both ends busy-poll an atomic
   doorbell while a turn is in flight (no sleep → no wakeup), backing
   off to yield/sleep when idle (so an idle plugin frees its core).
   This is the *only* option that yields a flat sub-µs tail. Cost: a
   spinning core during active turns; `unsafe`; Rust-only.
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
latency-critical, and the host spins **only while a turn is in flight**.

Implementation is **phased** (below); this ADR is `Proposed` and becomes
`Accepted` on the go decision — the evidence already justifies it.

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
708 ns shows it is a lone blip, not the distribution). The premise is
validated: shared memory + spin is the wake-up-free path, and it crushes
"< 10 µs" including p99.

## Consequences

**Makes easy:** sub-µs, flat-tail RTT for a designated latency-critical
Rust plugin — interactive/streaming plugins that would feel pipe jitter.

**Makes hard / accepted costs:**

- **Rust-only.** Node/Bun cannot `mmap` shared memory or do lock-free
  atomics without a native addon, which would break the dependency-free
  TS posture (ADR 0007 lineage). TS plugins keep stdio/uds — this fast
  path is Rust-plugin-only, and that is documented, not hidden.
- **CPU.** A spinning end is a core at ~100% *while waiting*. Adaptive
  back-off frees an idle plugin's core, but many simultaneously-active
  shm plugins is untenable — hence opt-in, single/few, host spins only
  during an in-flight turn.
- **`unsafe`.** Confined to one audited crate (`rstui-acp-shm`,
  `[lints]` not inheriting the workspace forbid; every `unsafe` block
  carries a `SAFETY:` justification; reviewed as its own slice). The
  SDK and all other crates stay `unsafe`-free.
- **Lifecycle.** A spinning host must detect a dead plugin (peer-liveness
  epoch in the shared header + a spin budget ceiling) and the tmpfile
  must be cleaned up; a real SPSC ring with variable-length framing and
  backpressure replaces the prototype's single ping-pong slot.

**Deferred (phased plan):**

1. ✅ Prototype + measurement (this ADR's evidence).
2. `rstui-acp-shm` crate: `MAP_SHARED` SPSC ring, length-framed,
   peer-liveness, adaptive spin; isolated audited `unsafe`.
3. `impl Transport for ShmTransport` + `serve_shm` + `serve_auto`
   `--shm`/`RSTUI_PLUGIN_SHM`; `examples/rtt.rs` gains a shm row.
4. Docs: SDK README + OPTIMISATION.md; the opt-in/CPU contract stated
   at the call site.

Each phase is its own gated slice; stdio remains the default throughout.
If the go decision is "no", option 4 stands and this ADR is the record
of why shm was evaluated and what it would have cost.
