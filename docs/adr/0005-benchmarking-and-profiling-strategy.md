# ADR 0005: Benchmarking and profiling strategy

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

rstui needs a way to answer "did this change make a hot path slower?"
without that question turning into folklore. `docs/development.md`
already made a promise about exactly this: the fast inner loop
(`cargo xtask ci`) explicitly excludes benchmarking, with the sentence
"Heavier, non-gating work (benchmarks, profiling) is deliberately *not*
in this loop so the loop stays fast — see the relevant ADR/docs when
that infrastructure lands." **This ADR is that referenced record**, and
the benchmark infrastructure is the thing that has now landed.

This decision is load-bearing for rstui specifically, for the same
reason ADR 0003 is: rstui is an **agent-driven codebase** built by short
parallel slices. Whatever benchmark workflow is chosen here is the one
every future machine-generated performance-sensitive slice will reach
for, on the machines the maintainers actually develop on. A workflow
that is heavy, non-deterministic, or unrunnable on the primary dev
machine would either be silently bypassed or would slow the loop every
stream pays into — both are failure modes that compound across the whole
project. So the shape of the harness matters as much as having one.

Constraints already locked in by earlier slices, which this decision
must fit rather than relitigate:

- **Tooling is deliberately dependency-free.** ADR 0003 §7 establishes,
  as a ratified principle, that the quality tooling (`xtask`) is
  dependency-free *by design* — "the gate that guards the project must
  not itself become a dependency-update risk." `docs/development.md`
  restates this ("`xtask` is dependency-free by design (ADR 0003 §7)").
  A benchmark runner is quality tooling of exactly the same kind: its
  job is to measure the project, and a measurement tool that drags a
  large transitive dependency tree into the build is itself a
  supply-chain and dependency-churn surface. The ADR 0003 §7 logic
  applies to it directly.
- **`rstui-core` is dependency-free pure terminal logic.** Per ADR 0001
  the only external dependency in the workspace is `crossterm`, isolated
  in `rstui-crossterm`. `rstui-core` — where the hot paths being
  measured live (`Buffer`, `Layout`, `Style`, …) — has **no external
  dependencies at all**. A runner that depends only on `rstui-core`
  builds nothing but rstui code plus the standard library.
- **The fast loop is a contract, not a convention.** `cargo xtask ci`
  runs exactly CI's gate set, and a unit test
  (`step_set_matches_ci_gates` in `crates/xtask/src/ci.rs`) fails if the
  local loop and CI silently diverge. The project treats "the loop stays
  fast" as a structural invariant the agent streams depend on, not a
  soft aspiration — anything added to that loop is paid by every stream
  before every commit.
- **Byte-for-byte reproducibility is a project value.** The wider rstui
  workflow (and the naming/`xtask` tooling) leans on deterministic,
  reproducible output. A tool whose output reshuffles run to run works
  against that value and is harder for an agent to diff.
- **The maintainers develop on macOS (darwin), Apple Silicon.** The
  environment this work is done in is macOS. Any workflow that "iterate
  on it locally" depends on must run there, not only in a Linux CI
  container.

## Decision drivers

- **The fast loop must stay fast.** The single most important property
  of the inner loop is that it is fast and runs before every commit on
  every stream; adding benchmarking to it would tax every slice for a
  signal only hot-path slices need.
- **Dependency-minimalism for tooling** — ADR 0003 §7 is a ratified
  principle, not a one-off; a measurement tool is tooling and is judged
  by it. The objective's standing "do not add dependencies
  speculatively" posture points the same way.
- **Runs on the primary development machine** — a benchmark workflow
  that cannot run on macOS is, for this project, equivalent to not
  having one, because the people and agents who would run it work there.
- **Reproducibility of shape** — the output must be comparable run to
  run and machine to machine well enough to spot a real regression, and
  diffable by an agent.
- **Measures the public contract, not internals** — rstui is built by
  parallel streams; a benchmark crate that reaches into another crate's
  private internals would couple to implementation details another
  stream owns and break across slices. It should consume the published
  surface, exactly as a downstream application does.
- **Signal quality proportionate to the question** — the question at
  this stage is "did a hot path regress by an order of magnitude / did
  CPU or memory behavior change?", not "defend a sub-10% optimization
  with confidence intervals." The mechanism should match that question
  and no more — the same evidence-shaped posture ADR 0003 took.
- **One command, agent-friendly output** — consistent with the
  one-command `xtask` loop, and producing a plain textual table an agent
  can read and diff.

## Options considered

**Option A — `criterion` benches per crate (`cargo bench`).**
`criterion` is the de-facto Rust micro-benchmark framework and is
statistically rigorous: Tukey outlier rejection, bootstrapped confidence
intervals, and a persisted regression history across runs. The directly
comparable Rust TUI, `ratatui`, benchmarks with `criterion` under a
dedicated `benches/` setup, so this is the well-trodden ecosystem path.
Rejected **as the default**: `criterion` pulls a large transitive
dependency tree (rayon, plotters, regex, and more) into the build. That
directly conflicts with the project's repeatedly-stated
dependency-minimalism for tooling — ADR 0003 §7 makes `xtask`
deliberately dependency-free *precisely* so a quality tool cannot become
a dependency-update / supply-chain risk, and a benchmark runner is a
quality tool of the same kind, so the same logic applies. Its
statistical output is also non-deterministic run to run, working against
the project's byte-for-byte reproducibility value. It is **kept as a
documented escape hatch** rather than discarded (see Decision and
Consequences) — the dependency-free default is the rule, not an
absolute.

**Option B — `iai-callgrind` (instruction-count benchmarking under
valgrind/callgrind).** Attractive on paper precisely where `criterion`
is weak: instruction counts are deterministic and CI-stable, sidestepping
wall-clock noise entirely, which fits the reproducibility value well.
Rejected: `iai-callgrind` requires valgrind, and **valgrind has no
working macOS port** (Apple Silicon especially). The maintainers develop
on macOS; a benchmark workflow that cannot run on the primary
development machine is a non-starter for a project whose whole premise is
that agents and humans iterate on it *locally*. It could be revisited
later as an **optional Linux-only CI lane** (its determinism is genuinely
valuable there) — but only under a new ADR, and never as the local
default the maintainers run.

**Option C — a dependency-free, deterministic in-repo harness wrapped
by `cargo xtask bench` (chosen).** A small std-only binary crate
(`rstui-bench`) depending only on the already-dependency-free
`rstui-core`, driven by one `xtask` subcommand. Zero new dependencies;
behaves identically on macOS and Linux; reproducible in the *shape* of
its result; one command; emits a fixed-width textual table an agent can
diff; and lives outside `xtask ci` so the fast loop stays fast. It has
less statistical sophistication than `criterion` — no outlier rejection,
no confidence intervals, no regression database — and that is accepted,
because the goal at this stage is spotting order-of-magnitude
regressions in hot paths and making CPU/memory behavior easy to inspect,
not publishing rigorous micro-benchmark statistics. This is the same
"minimal mechanism that catches the real defect class, escalate only on
demonstrated need" posture ADR 0003 took for lints.

## Decision

1. **rstui gets a benchmark feedback loop via a dependency-free,
   deterministic, in-repo runner.** The `rstui-bench` crate is a
   std-only binary whose only dependency is the already-dependency-free
   `rstui-core` (`crates/rstui-bench/Cargo.toml`). It is driven by
   `cargo xtask bench`, which always builds and runs it in **release**
   (a debug build reports meaningless numbers) pinned to the same
   toolchain that launched `xtask` (`crates/xtask/src/bench.rs`, sharing
   `ci::cargo_bin`). This extends — it does not relitigate — ADR 0003 §7:
   the dependency-free-tooling principle now explicitly covers the
   benchmark runner, and the crate's `Cargo.toml` says so in a comment.

2. **It measures hot paths through `rstui-core`'s PUBLIC API only.** The
   runner is a consumer of the published surface, exactly like a
   downstream application, and never reaches into another stream's
   internals. The scenarios are the hot paths named in the brief:
   `buffer/diff` in four shapes (idle/identical, sparse, full, resized —
   four because their cost profiles differ sharply), `buffer/fill`,
   `buffer/set_str` throughput, `buffer/clear_region` (the
   opaque-overlay reclaim a modal/popup runs), and `layout/split/nested`
   (a realistic nested app layout). Per-scenario setup
   (buffer/layout allocation) is done *outside* the timed closure;
   only the operation is measured (`crates/rstui-bench/src/scenarios.rs`).
   Frame size is a fixed 160×48 so numbers are comparable run to run.

3. **It is deliberately NOT a `cargo xtask ci` gate.** The fast gate
   loop every agent stream runs before every commit stays fast; the
   benchmark loop is the explicit *slow loop*, reached for only when a
   hot path changes. This fast-loop / slow-loop split is enforced
   **structurally, not by convention**: the unit test
   `bench_is_never_a_ci_gate` in `crates/xtask/src/ci.rs` fails if any
   future slice folds a `bench` step into the gate sequence. The split
   is a contract, exactly as `step_set_matches_ci_gates` makes the gate
   set itself a contract.

4. **Output is `min` / `median` / `mean` per iteration, and `min` is
   the regression signal.** Each measured iteration is timed
   individually, so a single scheduler hiccup inflates one sample, not
   the whole run; the fastest sample (`min`) is therefore the one least
   polluted by OS noise and the most stable number to compare across
   machines and runs. Warmup iterations run untimed first so the
   measured region runs at steady state. This is documented, in the
   runner and here, as a **deterministic timing *aid*, not a
   statistically rigorous benchmark**: there is intentionally no outlier
   rejection, no confidence intervals, and no regression database. It is
   reproducible in the *shape* of the result (which scenario is fast,
   which regressed by an order of magnitude), which is the signal it is
   built to give.

5. **`criterion` is a documented escape hatch, not a forbidden tool.**
   The dependency-free runner is the *default*, the rule for the routine
   question. If statistically rigorous micro-benchmarks become genuinely
   necessary, `criterion` MAY be added — under the strict conditions in
   Consequences and only via a new superseding ADR. The default is the
   rule, not an absolute (the same shape as ADR 0003 §6's "the forbid is
   the default, not an absolute").

## Evidence

Unlike ADR 0003 — which cited facts gathered from freshly-cloned
reference projects — **this slice did not clone any reference project**.
To keep this record honest, the evidence below is framed for what it
actually is: the **well-known Rust ecosystem landscape plus this
repository's own locked-in constraints**, not a fresh reference-repo
audit. No reference-repo line-citations are claimed here, deliberately,
because none were re-verified for this decision.

- **`criterion` is the de-facto Rust micro-benchmark framework, and it
  has a large transitive dependency tree** (rayon, plotters, regex, and
  more). This is widely-known ecosystem fact, not a fresh audit.
  `ratatui` — the directly comparable Rust TUI — benchmarks with
  `criterion` under a dedicated `benches/` setup, which is why
  `criterion` is the obvious ecosystem default and is kept here as the
  documented escape hatch rather than dismissed.
- **`iai-callgrind` depends on valgrind, and valgrind has no working
  macOS port** (Apple Silicon in particular). This is a known platform
  constraint of the valgrind ecosystem, not something measured here, and
  it is the decisive fact against Option B *for the local default*.
- **This repository's own ADR 0003 §7 establishes the
  dependency-free-tooling precedent**, and `docs/development.md` already
  promised this ADR ("benchmarks, profiling … see the relevant ADR/docs
  when that infrastructure lands"). These are this repo's own ratified
  constraints, verifiable in-tree (ADR 0003, `docs/development.md`),
  and they — not an external survey — are what bound this decision.
- **`rstui-core` is itself dependency-free pure terminal logic**
  (ADR 0001: the only external dependency in the workspace is
  `crossterm`, isolated in `rstui-crossterm`; `rstui-core` has none).
  So a runner depending only on `rstui-core` builds nothing but rstui
  code plus the standard library — verifiable from
  `crates/rstui-core/Cargo.toml` and `crates/rstui-bench/Cargo.toml`
  in-tree. The zero-new-dependency claim for Option C is a property of
  this repository, not an estimate.

**Synthesis used to shape the decision.** The ecosystem offers two
mature options with the rigor `criterion` is the standard
(dependency-heavy, non-deterministic output) and instruction-count tools
are deterministic (`iai-callgrind`, but valgrind-bound and therefore
macOS-unrunnable). Neither fits *both* this repo's ratified
dependency-free-tooling principle (ADR 0003 §7) *and* its
macOS-primary, reproducibility-valuing constraints. A small in-repo
harness over `rstui-core`'s already-dependency-free public API satisfies
both at the cost of statistical sophistication that the current
question — order-of-magnitude hot-path regressions, inspectable
CPU/memory behavior — does not require. The escape hatch
(scoped `criterion` under a new ADR) is retained for the day the
question changes.

## Consequences

**Positive**

- The promise `docs/development.md` made — that benchmarks live outside
  the fast loop and there would be a record when the infrastructure
  landed — is now discharged by a concrete, runnable mechanism.
- **Zero new dependencies.** The benchmark runner builds only rstui code
  plus std, so it cannot become a dependency-update or supply-chain
  surface — the ADR 0003 §7 principle extended cleanly to measurement
  tooling instead of being contradicted by it.
- **Identical on macOS and Linux.** It runs on the maintainers' primary
  development machine with no valgrind/Linux precondition, so "iterate
  on it locally" is true for the people and agents who would use it.
- **The fast loop stays fast, structurally.** Benchmarks are out of
  `xtask ci` and a unit test (`bench_is_never_a_ci_gate`) keeps them
  out; the split is a contract, not an accident that a future slice can
  erode silently.
- **Measures the public contract.** Because the runner consumes only
  `rstui-core`'s public API, it does not couple to internals another
  stream owns and does not break when an internal is refactored — it
  breaks (correctly, via the `cargo test` smoke test) only if the public
  surface it uses changes.
- **Agent- and human-friendly.** One command (`cargo xtask bench`),
  with substring filtering and `--list`, producing a fixed-width
  `min`/`median`/`mean` table that is easy to diff across runs.

**Negative / accepted**

- **Less statistical rigor than `criterion`** — no outlier rejection,
  no confidence intervals, no persisted regression history. Accepted and
  in fact *intended*: the current question is order-of-magnitude
  hot-path regression and inspectable CPU/memory behavior, not defending
  a sub-10% optimization. **Escape hatch:** if statistically rigorous
  micro-benchmarks become necessary (e.g. to defend a sub-10%
  optimization), `criterion` MAY be added as a dev-dependency **scoped
  to the `rstui-bench` crate ONLY**, recorded as a **new superseding
  ADR** — the dependency-free default is the rule, not an absolute.
- **Wall-clock numbers are machine-relative.** Absolute timings differ
  across hardware; only the *shape* (relative cost, order-of-magnitude
  change, `min` compared run to run on the same machine) is the trusted
  signal. Accepted: that shape is exactly the signal the tool is built
  to give, and it is documented as such in the runner and here.
- **Benchmarks must be kept building by hand-maintained discipline plus
  the `cargo test` smoke test.** Since they are not a CI gate, a
  one-iteration run of every scenario is exercised by `cargo test`
  (`every_scenario_runs_and_summarizes`) so they cannot silently
  bit-rot, but performance *regressions* themselves are not auto-caught
  in CI — catching them is a deliberate act on a hot-path slice.
  Accepted: auto-gating performance is the precise thing that would slow
  the loop, which this ADR exists to avoid.

**Neutral / deferred**

- **A scoped `criterion` dev-dependency** — not adopted; allowed only
  later, `rstui-bench`-only, under a new superseding ADR, and only on
  demonstrated need (a regression too small for the timing aid to
  resolve).
- **An optional Linux-only `iai-callgrind` CI lane** — not adopted;
  permitted as a future *addition* (its determinism is valuable in CI)
  under a new ADR, never as the local default the maintainers run.
- **Wiring more hot paths** (text wrapping, widget render, the
  event/runtime loop) — deferred until those surfaces stabilize in
  their owning streams, so the benchmark crate does not couple to a
  surface still in flux.
- **Committed baseline numbers** — deferred to `docs/benchmarking.md`
  once hot paths stabilize; baselines pinned before that would be noise.

## Follow-up

This ADR discharges the `docs/development.md` benchmark/profiling
forward-reference and is the reference contract for benchmark-related
slices. Concrete next steps, none bundled with unrelated feature work:

1. **Author `docs/benchmarking.md`** as the full workflow doc the runner
   and this ADR point at: how to run `cargo xtask bench`, how to read
   the `min`/`median`/`mean` table and why `min` is the regression
   signal, and the CPU/memory profiling recipe for macOS and Linux.
2. **Commit baseline numbers to `docs/benchmarking.md`** once the
   measured hot paths have stabilized, so future runs have something
   concrete to diff against.
3. **Wire additional hot paths as their surfaces stabilize** in other
   streams — text wrapping, widget render, and the event/runtime loop —
   each added as its own slice, still through the public API only.
4. **Revisit `criterion` only on demonstrated need** (e.g. defending a
   sub-10% optimization the timing aid cannot resolve), and only as a
   new superseding ADR adding it `rstui-bench`-scoped.
5. **Explicitly not planned** (each would require a new superseding
   ADR): `criterion` as a default or workspace-wide dependency; a
   Linux-only `iai-callgrind` CI lane; folding any `bench` step into
   `cargo xtask ci`.
