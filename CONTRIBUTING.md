# Contributing to rstui

rstui is built by short, individually-green slices. The whole workflow is
designed so one command tells you whether a change is sound, and so the
same thing runs locally and in CI.

## The loop

```sh
cargo xtask ci      # fmt, lint-names, clippy, doc, test — fail-fast
```

Make one coherent change with its tests and docs, run `cargo xtask ci`
until green, commit. A green run locally is a green CI `check` job — the
gate set is asserted to stay in lock-step. Never commit with a red gate;
never weaken a gate to pass a slice — strengthen the slice. Full detail:
[`docs/development.md`](docs/development.md).

## Before a release (reproduce all of CI)

```sh
cargo xtask ci --full
```

The five gates **plus** the separate CI legs: `publish-check` (every
publishable crate `cargo package`s as a set), and `cargo-deny` /
`cargo-machete` when installed (skipped with an install hint otherwise —
CI enforces them regardless). The MSRV leg is printed as a reproduce
command (it needs the pinned toolchain). On-demand equivalents for each
leg are tabled in [`docs/development.md`](docs/development.md).

## Conventions and decisions

- Mechanically-enforced rules new code must follow:
  [`docs/conventions/`](docs/conventions/) — notably the vague-generic-name
  ban (`gate 2`, `cargo xtask lint-names`).
- Why the project is built the way it is, and decisions expensive to
  reverse: [`docs/adr/`](docs/adr/).
- Performance work is the deliberately separate slow loop:
  [`docs/benchmarking.md`](docs/benchmarking.md) (`cargo xtask bench`).

## Landing a change

Single-stream contributors open a PR; CI is the gate. The repo is also
developed by parallel agent streams that merge to a shared `main` — that
serialized, never-push-`main`-red protocol, and the
`cargo xtask merge-check` preflight, are in
[`docs/merging.md`](docs/merging.md). Either way the rule is the same:
**`main` is never red.**

## Recommended repository settings

CI runs on every push to `main` and every pull request: the `check` job
plus the `msrv`, `unused-deps`, `supply-chain`, and `package` legs. For a
production posture, protect `main` with a branch-protection rule that
makes those jobs **required status checks** and requires branches be up
to date before merge — so detection (CI) becomes prevention (a red
change cannot land), closing the one gap the in-repo tooling cannot
enforce on its own.
