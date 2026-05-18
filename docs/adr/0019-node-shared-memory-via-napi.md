# ADR 0019: Node/Bun shared memory via a napi-rs addon — evaluated, built, measured

- **Status:** Accepted (proof built + measured; **not productionised** — see Decision)
- **Date:** 2026-05-18
- **Relates to:** [ADR 0016](0016-shared-memory-plugin-transport.md) (reaffirms its "Rust-only" consequence with evidence), [ADR 0007](0007-plugin-host-and-secure-execution.md)

## Context

[ADR 0016](0016-shared-memory-plugin-transport.md) shipped shared memory
as a Rust-plugin-only transport (flat sub-µs RTT) and scoped it Rust-only
because "Node/Bun cannot `mmap` shared memory … without a native addon,
which would break the dependency-free TS posture". A maintainer asked
whether [napi-rs](https://napi.rs/) could close that gap and give Node
plugins the shm fast path, and chose to **build** the optional addon.

Per this project's evidence-first norm (cf. the QUIC / protobuf
evaluation in `sdk/bench/OPTIMISATION.md`), "build it" meant: build a
real proof, **measure it**, and let the number decide productisation.

## Decision drivers

- Does shm actually lower a *Node* plugin's RTT, or is the bottleneck
  the Node runtime, not the IPC?
- Cost to the TS SDK's dependency-free / no-native-build posture.
- Whether the realized win justifies per-platform prebuilt-binary
  packaging + CI.

## What was built

`crates/rstui-acp-shm-native/` — a napi-rs `cdylib` wrapping
[`rstui_acp_shm::ShmChannel`], a **full workspace member** so it gets the
same gates as everything else (`xtask ci` / `cargo-deny` / `machete` /
MSRV 1.85). Like `rstui-acp-shm` it does not inherit
`[lints] workspace = true` — the `#[napi]` macros expand to
`unsafe extern "C"` bindings, so it is a sanctioned `unsafe` boundary
with its own `[lints]` (every other workspace lint replicated); its
hand-written code has no `unsafe`, and the audited mmap/atomic/semaphore
`unsafe` stays in `rstui-acp-shm` (one-way path dep). The napi tree is
**MSRV-1.85-clean** (verified) and adds exactly one new licence — **ISC**
(`libloading`), reviewed and added to `deny.toml` (permissive,
no-copyleft, MIT-equivalent). The JS glue (`sdk/shm-native/index.mjs`
probed loader + `echo.mjs`) and a Rust host RTT harness
(`examples/node_rtt.rs`) round it out.

The API is **synchronous/non-blocking** (`open`/`send`/`tryRecv`/
`isClosed`); the JS side drives an adaptive poll on the event loop (no
background thread, no `ThreadsafeFunction`, no `Send` on the non-`Send`
channel — sidestepping that entire class of bug for a proof).

## Evidence

Node host = the addon; Rust host creates the segment; M1 Pro, 30 000
iters, 128-byte payload, single in-flight:

| path | min | p50 | p90 | p99 |
| --- | --: | --: | --: | --: |
| **Node-over-shm (this addon)** | **0.92 µs** | **14.96 µs** | 31 µs | 80 µs |
| Node-over-stdio (RESULTS.md class) | ~4 µs | ~10–22 µs | — | — |
| Rust-over-shm (ADR 0016) | 0.04 µs | **0.33 µs** | 0.42 µs | 1.33 µs |

`min 0.92 µs` proves the native path **works and is sub-µs** when a poll
happens to coincide. But **p50 ≈ 15 µs ≈ Node-stdio**, ~45× the Rust-shm
p50. The bottleneck is the **Node event-loop tick**, not the ring: every
message delivered into V8 goes through an event-loop-scheduled callback —
this is true for a `ThreadsafeFunction` too (a TSFN call also schedules
on the loop), so a heavier thread/TSFN design is **not expected to change
the conclusion**. The Node runtime, not the IPC, is the floor.

## Decision

**Build the proof, keep it as a documented escape hatch, do *not*
productionise it.** The measured result shows shared memory delivers **no
meaningful p50 win for Node plugins** — it is ≈ Node-stdio because the
event loop dominates. Therefore:

- ADR 0016's "shm is the Rust-plugin fast path; Node/Bun plugins use
  stdio/uds" stands — now backed by measurement, not just by the
  dependency-free-posture argument.
- The `crates/rstui-acp-shm-native/` addon is retained as a **gated,
  reproducible proof and an opt-in escape hatch** (a future Node plugin
  that *does* want it builds it locally; `RSTUI_SHM_NATIVE` points the
  `sdk/shm-native/index.mjs` loader at the built `.node`).
- **No per-platform prebuilt-binary packaging, no TS-SDK `bridge()`
  integration, no CI matrix** — that cost buys ≈ zero Node latency and
  would break the TS SDK's dependency-free posture for nothing. The
  earlier phase-5c/5d plan is **cancelled by this evidence**.

## Consequences

- **Easy / preserved:** the TS SDK stays dependency-free and addon-free;
  the conclusion is now empirical and reproducible (`cargo run --release
  --example node_rtt -p rstui-acp-shm-native`, after building the
  `.node`).
- **Accepted:** Node/Bun plugins do not get a shm latency win — and that
  is a runtime limit, not a missing feature. A Rust plugin remains the
  answer when a flat sub-µs tail genuinely matters.
- **Deferred / out of scope:** a thread+`ThreadsafeFunction` design is
  *not* pursued — the event-loop-delivery floor makes a materially
  different number unlikely; revisit only with a concrete benchmark
  showing otherwise.
- **Reversible:** productisation can still happen later if a real
  workload justifies it; this ADR is the record of why it was not, with
  the number that decided it.

[`rstui_acp_shm::ShmChannel`]: ../../crates/rstui-acp-shm/src/lib.rs
