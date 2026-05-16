# The rstui development loop

This is the inner loop. rstui is built by short, validated slices — by
humans and by agents — so the single most important thing is that
*validation is one command, runs the same locally as in CI, and tells you
exactly what failed*.

## One command

```sh
cargo xtask ci
```

That runs every project gate, fail-fast, in this order:

| # | Gate | Catches | Underlying command |
|---|------|---------|--------------------|
| 1 | `fmt` | Unformatted code | `cargo fmt --all --check` |
| 2 | `lint-names` | Banned vague generic names ([naming](conventions/naming.md)) | in-process workspace scan |
| 3 | `clippy` | Clippy lints, denied | `cargo clippy --all-targets --all-features -- -D warnings` |
| 4 | `doc` | Broken intra-doc links / missing docs | `cargo doc --no-deps --all-features --workspace` with `RUSTDOCFLAGS=-D warnings` |
| 5 | `test` | Failing unit / integration / doc tests | `cargo test --all-features` |

The gate set is exactly what [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)
enforces — a green `cargo xtask ci` locally means a green CI run. The only
ordering difference is that the instant in-process naming scan runs *before*
the slow clippy build, so a naming mistake fails in milliseconds. A
`xtask ci` test asserts this gate set stays in lock-step with CI; changing
one without the other fails that test.

On failure you get a single banner naming the gate and the defect class it
catches, and later gates are skipped. Fix it, re-run `cargo xtask ci`.

## When you only changed naming

```sh
cargo xtask lint-names
```

Instant, dependency-free, no build. This is gate 2 on its own — useful when
renaming and you want the fast signal before a full run.

## The slice loop

1. Make one coherent change with its tests and docs.
2. `cargo xtask ci` until green.
3. Commit the slice.
4. Rebase on the latest `main`, re-run `cargo xtask ci`, merge, push.

Keep slices coherent and individually green. Never commit with a red gate;
never weaken a gate to make a slice pass — strengthen the slice.

## Why a single command

`cargo xtask` pins every gate to the toolchain that launched it (via the
`$CARGO` cargo sets for subcommands), so "works on my machine" and "works in
CI" cannot diverge by toolchain. `xtask` is dependency-free by design
(ADR 0003 §7): the gate that guards the project must not itself become a
dependency-update risk. Heavier, non-gating work (benchmarks, profiling) is
deliberately *not* in this loop so the loop stays fast — see the relevant
ADR/docs when that infrastructure lands.

## Benchmarks and profiling (the slow loop)

Benchmarks and profiling are deliberately *outside* `cargo xtask ci` so the
fast loop stays fast (ADR 0005). The one command is:

```sh
cargo xtask bench
```

That builds and runs the `rstui-core` hot-path scenarios in release. The full
workflow — scenario list, env tuning, adding a scenario, and CPU + memory
profiling on macOS and Linux — is in [`docs/benchmarking.md`](benchmarking.md).
Fast loop (the gate, every commit) vs slow loop (benchmarks, only when you
touch a hot path): keep them separate and run each at the right time.

## Conventions and decisions

- Mechanically-enforced rules new code must follow:
  [`docs/conventions/`](conventions/).
- Why the project is built the way it is: [`docs/adr/`](adr/). ADR 0003 is
  the lint and code-quality policy this loop implements.
