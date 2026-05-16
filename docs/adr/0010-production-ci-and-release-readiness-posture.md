# ADR 0010: Production CI and release-readiness posture

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

ADR 0003 §7/§8 scheduled a phased rollout of quality/supply-chain gates
(cargo-deny, cargo-machete, the iteration-19 naming check, an MSRV CI
leg) and ADR 0005 added the non-gating benchmark loop. Those phases have
now all landed, and several **additional** production mechanisms were
built alongside them that exist only in code and scattered docs, not in
any ADR:

- the **separate-CI-legs** model (legs that are not part of the
  five-gate `cargo xtask ci` fast loop);
- **`cargo-deny` advisories promoted from non-blocking to gating** (the
  "later, deliberate slice" ADR 0003 §7 explicitly deferred);
- **publishability**: the library crates carry `version` on their
  internal `path` deps, and `cargo xtask publish-check` packages them;
- **structural drift guards** (`xtask` `release` tests) for the MSRV pin
  and the internal-dep versions;
- **`cargo xtask ci --full`** and **`merge-check`** as the reproduce-CI
  and pre-merge tools;
- the **two-layer enforcement model** (pre-push prevention by the brief,
  post-push detection by CI as the authority).

Per the ADR README, decisions "expensive to reverse, [that] constrain
later work, or would otherwise have to be re-derived" must be recorded.
This posture qualifies: it is the contract every future release and
every parallel stream depends on, and the rationale (why these are
*separate legs*, why advisories gate *now*, why prevention is
best-effort) is non-obvious and currently un-auditable. This ADR is that
record. It does not relitigate ADR 0003/0005; it ratifies the completed
result and the mechanisms added to reach it.

## Decision drivers

- **Reproducibility** — "what CI runs == what you run locally" (ADR 0003
  §7) must still hold once CI has more than the five-gate job.
- **The fast loop stays fast** — every stream pays `cargo xtask ci`
  before every commit; slow or tool-dependent checks must not tax it.
- **Dependency-minimal tooling** — ADR 0003 §7: a quality tool must not
  itself become a supply-chain/dependency risk; the guards are std-only
  `xtask` code, not new dependencies.
- **Low churn** — ADR 0003's explicit cost to minimise; a gate that
  red-walls every stream on an external event (a fresh CVE) needs a
  reviewed escape hatch, not a blanket weakening.
- **Recordedness** — the project mandates decisions be written down
  before they become folklore (the precedent ADR 0003 set for itself).

## Options considered

**A — Leave the posture implicit (code + scattered docs).** Lowest
effort; rejected: the *why* (separate legs, advisories-now, prevention
is best-effort) is exactly the re-derivation cost ADRs exist to remove,
and a future contributor would reasonably "simplify" one of these
without the rationale.

**B — Fold every check into one gate.** Maximal "one command"; rejected:
MSRV needs a second toolchain, supply-chain/unused-deps need external
plugins, packaging is slow — folding them in taxes the inner loop every
stream runs, the precise failure ADR 0003 §7 anticipated.

**C — Record the layered posture as-built (chosen).** The five-gate fast
loop is the contract enforced identically locally and in CI's `check`
job; the heavier/tool-dependent checks are *separate CI jobs*, runnable
on demand and bundled by `ci --full`; drift is prevented structurally;
the recorded enforcement model is honest about what is prevention vs
detection.

## Decision

1. **The fast loop is exactly five gates** (fmt, lint-names, clippy,
   doc, test), identical via `cargo xtask ci` and CI's `check` job, with
   the `step_set_matches_ci_gates` test keeping them in lock-step. New
   checks do **not** join this set; a test asserts that.

2. **Heavier/tool-dependent checks are separate CI jobs**: `msrv`
   (pinned toolchain), `unused-deps` (cargo-machete), `supply-chain`
   (cargo-deny), `package` (publish-check). Each has a documented
   on-demand local command; `cargo xtask ci --full` runs all runnable
   ones in one command (skipping an absent external plugin with an
   install hint — CI remains authoritative).

3. **cargo-deny advisories are gating** (ADR 0003 §7's deferred
   promotion, now executed). The reviewed escape hatch is a
   rationale-commented `[advisories] ignore` entry in `deny.toml` — an
   in-tree, auditable decision, never silent suppression. Licenses are
   an explicit allow-list; sources are crates.io-only.

4. **The library is publishable and stays so structurally.** Publishable
   crates pin internal `rstui-*` deps with `version`; `publish-check`
   packages the set; `xtask` `release` tests fail `cargo test` if the
   MSRV pin ≠ `[workspace.package] rust-version` or an internal dep
   version ≠ the workspace version. The guard *logic* is pure and
   unit-tested with synthetic inputs, so a half-done bump is provably
   detected, not merely described.

5. **Enforcement is two layers, and the docs say so honestly.**
   Pre-push prevention (`merge-check` + the serialized protocol) is
   carried by the stream brief and therefore best-effort — it reaches a
   running stream only on relaunch; post-push detection (CI on every
   push to `main` and every PR, all jobs) is the authority. A bespoke
   "merged tree is green" check is **not** built: CI re-running the
   gates on the pushed commit already is that check.

6. **Branch protection is the recommended, owner-applied closure.**
   Making the jobs required status checks turns detection into
   prevention; it needs repo-admin and is documented in
   `CONTRIBUTING.md`, not automated from the tree.

## Evidence

In-tree and verifiable, not an external survey: `.github/workflows/ci.yml`
(the `check` job + the four separate legs), `deny.toml` (allow-list +
the `ignore` escape-hatch convention), `crates/xtask/src/{ci,release,
publish_check,merge_check}.rs`, `docs/development.md` (the legs table),
`docs/merging.md` (the enforcement model), `CONTRIBUTING.md`. The
phased plan this completes is ADR 0003 §7/§8; the non-gating loop it
sits beside is ADR 0005. `cargo deny check`, `cargo xtask publish-check`,
and `cargo xtask ci --full` were each verified green on `main` before
this record.

## Consequences

**Positive**

- The production posture is auditable and constrains future "simplify"
  changes with recorded rationale.
- One command (`cargo xtask ci --full`) reproduces CI before a release;
  the everyday loop stays five fast gates.
- Supply-chain, MSRV, unused-deps, and packaging breakage are all caught
  before a tag; version/MSRV desync is structurally impossible to land
  silently.

**Negative / accepted**

- More CI jobs (cost: minutes, parallel) — accepted; each gates a real
  defect class and none taxes the inner loop.
- Pre-push prevention is best-effort until streams relaunch — accepted
  and documented; CI is the guaranteed backstop.

**Neutral / deferred**

- A `typos` leg and a coverage gate are *not* adopted: not in ADR 0003's
  plan, and adding gates beyond it is itself an ADR-worthy decision, not
  a unilateral slice. Recorded here as deliberately out of scope.
- Benchmarks for further hot paths remain deferred per ADR 0005 until
  those surfaces stabilise in their owning streams.

## Follow-up

This ADR closes the ADR 0003 §7/§8 phased rollout: no further
quality/supply-chain *gate* is planned. Future changes to this posture
(a new gate, advisories-ignore entries, demoting any gate) are
ADR-worthy events, recorded before applied — the same discipline
ADR 0003 set for itself.
