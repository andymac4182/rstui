# ADR 0019: Node/Bun shared memory via a napi-rs addon — evaluated, built, measured

- **Status:** Accepted (built + measured; **productionised by informed maintainer decision despite ≈0 latency win** — see Decision)
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

The measured result shows shared memory delivers **no meaningful p50 win
for Node plugins** — it is ≈ Node-stdio because the event loop dominates,
not the IPC. That fact is recorded here unambiguously and is *not*
walked back.

**Decision (maintainer, with the measurement explicit): productionise it
anyway — for optionality and transport parity, expressly NOT for
latency.** Presented with the ≈0-µs-win evidence the maintainer chose the
full path: a TS/Node plugin *may* opt into shm exactly like a Rust one,
and it ships as a real per-platform package — uniformity and operator
choice are judged worth the cost even though speed is not the payoff.
Therefore:

- The ≈0 Node latency win stands as the honest characterisation;
  ADR 0016's "shm is the **Rust** fast path" is still true on the
  numbers. shm-for-Node is a **parity/optionality** feature, and every
  doc says so plainly so no one adopts it expecting speed.
- The `crates/rstui-acp-shm-native/` addon is **gated in-workspace**
  (xtask ci / deny / machete / MSRV), and is wired into the TS SDK
  `bridge()` (probe + graceful fallback to uds/stdio when the optional
  addon is absent — the **core TS SDK stays dependency-free**: the addon
  is an `optionalDependency`, never required).
- **Phase 5c** (TS `bridge()` shm + probe/fallback) and **phase 5d**
  (per-platform prebuilt-binary npm package + CI matrix) are **done**,
  not cancelled — the dependency-free posture is preserved by making the
  native package strictly optional and probed, never a hard dep.

## Consequences

- **Easy / preserved:** the **core** TS SDK stays dependency-free — the
  native package is an `optionalDependency`, dynamically probed; absent
  it, `bridge()` falls back to uds/stdio with one log line. The
  ≈0-latency conclusion is empirical and reproducible (`cargo run
  --release --example node_rtt -p rstui-acp-shm-native`).
- **Accepted (eyes open):** Node/Bun plugins get **no shm latency win** —
  a runtime limit, not a bug. shm-for-Node is shipped for
  parity/optionality; every doc says "not faster than stdio for Node" so
  no one adopts it expecting speed. A Rust plugin remains the answer when
  a flat sub-µs tail genuinely matters.
- **Cost taken on:** per-platform prebuilt `.node` packaging + a CI
  cross-build matrix + a new `unsafe`/lint-deviation crate in the gated
  workspace — accepted by maintainer decision for transport uniformity,
  with the no-speed-win caveat recorded.
- **Deferred / out of scope:** a thread+`ThreadsafeFunction` design is
  *not* pursued — the event-loop-delivery floor makes a materially
  different number unlikely; revisit only with a concrete benchmark
  showing otherwise.

[`rstui_acp_shm::ShmChannel`]: ../../crates/rstui-acp-shm/src/lib.rs
