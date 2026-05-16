# ADR 0003: Lint and code-quality policy

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

A user steering question has been open since iteration 18 and was
deliberately deferred through iterations 19–20. It asks, precisely:

> Evaluate whether rstui should enforce the strictest practical
> clippy/rustc lint setup now while the codebase is still young.
> Consider workspace-level lint policy for `clippy::pedantic`,
> `clippy::nursery`, missing docs, rustdoc warnings, unsafe forbids,
> unused/dead-code hygiene, and deny-by-default CI checks where
> practical. Balance strictness against false positives and
> API-design churn, but prefer setting a high maintainability bar
> early so future generated code stays clean, documented, and easy to
> review. **Record the recommended lint policy and any phased rollout
> plan before applying broad lint changes.**

The user requires the recommendation and the phased plan recorded
**before any broad lint change**. The orchestrator-owned `notes.md`
cannot be written to, so — per the precedent set by ADR 0001 and
ADR 0002, and the explicitly validated finding that a
pure-documentation ADR is the correct highest-leverage move for a
deferred steering item whose decision is expensive to reverse — this
ADR is that record. Splitting the decision from its mechanical
consequence also directly satisfies the "record … before applying
broad lint changes" instruction, keeping each lint change a separate
reviewable slice.

This decision is unusually load-bearing for rstui specifically: it is
an **agent-driven codebase**. The lint bar set here is the bar every
future machine-generated slice is held to, so getting the policy and
its rollout right early compounds across the whole project.

Constraints already locked in by earlier slices, which this decision
must fit rather than relitigate:

- The workspace is **dependency-light and pure terminal logic**:
  four crates (`rstui-core`, `rstui-widgets`, `rstui-runtime`,
  `rstui-crossterm`), the only external dependency being `crossterm`
  isolated in `rstui-crossterm` (ADR 0001). There is **no FFI, no
  GPU, no `unsafe`** anywhere today.
- Every crate already declares, at `lib.rs` top:
  `#![forbid(unsafe_code)]` and `#![warn(missing_docs)]` (4 + 4
  identical attributes), and every crate's `Cargo.toml` already has
  `[lints]` `workspace = true`.
- The workspace `[workspace.lints.clippy]` table today is minimal —
  exactly `uninlined_format_args = "warn"` and
  `needless_pass_by_value = "warn"` ("keep the core honest without
  opting into the full pedantic firehose"). There is **no
  `[workspace.lints.rust]` and no `[workspace.lints.rustdoc]`**
  table.
- CI (`.github/workflows/ci.yml`) runs, with `RUSTFLAGS: "-D
  warnings"`: `cargo fmt --all --check`, `cargo clippy --all-targets
  --all-features -- -D warnings`, `cargo test --all-features`. There
  is **no rustdoc/`cargo doc` job**, no MSRV matrix leg, no
  supply-chain gate.
- A crucial nuance: because CI runs clippy with `-D warnings`, **any
  lint set to `warn` in the workspace table is effectively `deny` in
  CI**. "Add it at warn" is not a soft introduction here — it is a
  hard gate the moment it lands. This is what makes a phased rollout
  mandatory rather than cosmetic.
- `rust-version = "1.85"` is declared in `[workspace.package]` but is
  **not CI-gated** (CI builds on `stable`).
- There is **no `clippy.toml`, no `rustfmt.toml`, no `deny.toml`, no
  `xtask`** — formatting is stable-default rustfmt.
- A separate standing steering note (iteration 19) asks for a
  project-specific check that **bans vague generic names** (`utils`,
  `helpers`, `common`, `misc`, …) in modules/files/public items,
  "ideally a small repository script or xtask that fails CI". That is
  a code-quality gate and therefore belongs under this policy's
  umbrella, sequenced here but landing as its own slice.

## Decision drivers

- **Set the maintainability bar early, while churn is cheap** — the
  young-codebase window the steering note explicitly wants to use.
- **False-positive / API-churn cost** — the steering note explicitly
  asks to balance strictness against this; `-D warnings` CI makes the
  cost immediate, not deferred.
- **Per-slice reviewability** — the objective requires "one small,
  commit-ready improvement … reviewable" per iteration. A broad lint
  flip that touches many files must be its *own* slice, not bundled.
- **Evidence over taste** — judged primarily against the
  directly-comparable Rust TUI (`ratatui`), with `gpui-component`
  (Rust breadth), `secure-exec` (Rust, security-sensitive FFI), and
  the cross-language posture of `bubbletea`/`opentui`/`opencode` as
  corroboration.
- **Signal quality** — gates must catch real defects (broken
  intra-doc links, missing docs on new public items) without drowning
  agents in subjective noise that triggers churn-for-churn's-sake.
- **Forward fit** — the policy must still hold once the plugin host
  (a security boundary) and heavier dependencies arrive; supply-chain
  hygiene matters increasingly then.

## Options considered

**Option A — Keep the status quo** (the two-lint workspace table,
per-crate `unsafe`/`docs` attributes, no rustdoc/MSRV/supply-chain
gate). Lowest churn, but it wastes the young-codebase window the
steering note wants to exploit and leaves a real silent-failure gap:
broken intra-doc links and missing docs on *new* public items are
**not caught by CI today** (`#![warn(missing_docs)]` only warns, and
nothing denies rustdoc warnings). Rejected as under-ambitious and
leaving a concrete hole.

**Option B — Maximalist now** (`clippy::pedantic` +
`clippy::nursery` + `clippy::cargo` + `clippy::restriction` all at
deny, `#![deny(missing_docs)]`, MSRV leg, full supply-chain, nightly
`rustfmt.toml`, all in one slice). Rejected: **no reference project
does this**. `clippy::restriction` is explicitly documented by clippy
as a grab-bag of mutually contradictory lints never meant to be
enabled wholesale; *no* surveyed project enables `nursery`, `cargo`,
or `restriction` as groups. Under `-D warnings` CI this would be a
large, unbudgeted, single-slice churn event — the exact opposite of
the steering note's "balance strictness against … API-design churn"
and the objective's reviewable-slice rule.

**Option C — Evidence-shaped tiered policy, phased (chosen).** Adopt
the `ratatui`-proven shape — clippy default groups denied in CI;
`pedantic` opt-in *at warn* with an explicit, commented allow-list;
no nursery/cargo/restriction groups; `unsafe_code = "forbid"` where
the domain permits; `missing_docs` warn + rustdoc `-D warnings` —
and roll it out in phases where each phase is an independently
reviewable slice and the one *broad* change (pedantic) is isolated to
its own slice, exactly as the user instructed.

## Decision

1. **`[workspace.lints.*]` is the single source of truth.** Keep
   every crate's `[lints] workspace = true` (already so). Consolidate
   the eight per-crate `lib.rs` attributes into the workspace table:
   `[workspace.lints.rust]` gets `unsafe_code = "forbid"` and
   `missing_docs = "warn"`, and the four `#![forbid(unsafe_code)]` /
   four `#![warn(missing_docs)]` inner attributes are removed. This
   is behavior-identical (the same lints, same levels, applied
   workspace-wide) and matches `ratatui`'s mechanism. rstui has **no**
   `no_std`/opt-out crate, so the per-crate exception that forces
   `ratatui` to keep `missing_docs` per-crate does not apply here —
   workspace-level is strictly cleaner for rstui.

2. **Clippy default groups stay denied in CI.** `cargo clippy
   --all-targets --all-features -- -D warnings` with `RUSTFLAGS: -D
   warnings` is kept exactly as-is. This (correctness / style /
   complexity / perf / suspicious at deny) is the single rule
   *universally* enforced across every surveyed project and is
   already in place — it is ratified, not changed.

3. **Adopt `clippy::pedantic` at `warn` with an explicit,
   commented allow-list — as its own later slice (Phase 2), not
   now.** The allow-list is seeded from `ratatui`'s
   battle-tested set and pruned to what rstui actually hits. Because
   rstui's CI is `-D warnings`, pedantic-at-warn is *effectively
   enforced*, so this is precisely the "broad lint change" the user
   required be planned before applied: it is scheduled as a dedicated
   slice that triages every finding and fixes-or-justifies each.

4. **Add a rustdoc gate (Phase 1).** Add `[workspace.lints.rustdoc]`
   (`broken_intra_doc_links = "deny"`,
   `private_intra_doc_links = "warn"`,
   `invalid_codeblock_attributes = "deny"`,
   `unescaped_backticks = "warn"`) and a CI step `cargo doc --no-deps
   --all-features --workspace` under `RUSTDOCFLAGS: -D warnings`. This
   closes the **only silent-failure gap** in the current setup
   (broken intra-doc links / missing docs on new public items pass CI
   today). Expected churn ≈ zero: doc discipline is already maintained
   (iteration 20 even fixed a broken intra-doc link by hand precisely
   because nothing caught it).

5. **Do not adopt `clippy::nursery`, `clippy::cargo`, or
   `clippy::restriction` as whole groups — ever, without a new ADR.**
   No surveyed project does. Individual nursery/restriction lints may
   be cherry-picked at `warn` *only* when a concrete recurring defect
   justifies that specific lint (the `ratatui` precedent: ~20
   individually opted-in), each recorded with a one-line rationale
   comment in the workspace table. The group flip is explicitly out
   of scope.

6. **`unsafe_code = "forbid"` workspace-wide is correct for rstui —
   because of rstui's domain, not as a universal.** rstui is pure
   terminal logic with zero FFI. Evidence shows the unsafe policy is
   domain-dependent: `secure-exec`, despite being security-sensitive,
   *cannot* forbid unsafe (pervasive V8 FFI). The workspace forbid is
   rstui's correct default; a *future* leaf crate that genuinely needs
   `unsafe` (an FFI/GPU backend, a plugin host trampoline) may carry a
   scoped, reviewed `#![allow(unsafe_code)]` with written
   justification — the forbid is the default, not an absolute, and any
   exception is itself an ADR-worthy event.

7. **Supply-chain and project-specific gates are adopted in
   principle, each as its own Phase-3 slice.** `cargo-deny`
   (advisories non-blocking initially; licenses/bans/sources gating)
   and `cargo-machete` (unused-dependency gate — directly reinforces
   the objective's "do not add dependencies speculatively"), then the
   iteration-19 vague-generic-naming check as the **first
   rstui-specific lint**, housed alongside the other gates (an `xtask`
   is the idiomatic home; `ratatui` precedent). These grow in value as
   dependencies and the plugin security boundary arrive.

8. **MSRV: keep the declared `rust-version` and add a dedicated CI
   matrix leg pinned to it (Phase 3, low urgency).** `ratatui`'s
   robust model (declared `rust-version` + a build/test leg pinned to
   exactly that version) is the one to copy. Low urgency because a
   dependency-light pure-logic core's only MSRV risk is
   new-language-feature creep, cheaply caught by one pinned leg.

9. **Stay on stable default rustfmt; do not add a nightly
   `rustfmt.toml` now.** `ratatui`/`gpui-component` use nightly-only
   rustfmt options, forcing nightly fmt in CI. rstui's recurring
   "cargo-fmt auto-rewrap" friction is a *managed workflow practice*
   ("`cargo fmt --all` before the `--check` gate"), not a correctness
   gap. Adopting nightly rustfmt + a config is a separate deliberate
   decision, revisited only against a measured churn cost and recorded
   as a new ADR if reversed.

## Evidence

Concrete, citable facts gathered from the reference projects (clones
via `npx opensrc@latest path github:OWNER/REPO`).

**ratatui/ratatui** — the directly-comparable Rust TUI workspace.

- `Cargo.toml` `[workspace.lints.rust]` is exactly
  `unsafe_code = "forbid"` (one lint). No `[workspace.lints.rustdoc]`
  table exists.
- `[workspace.lints.clippy]`:
  `pedantic = { level = "warn", priority = -1 }` plus **nine
  pragmatic per-lint allows** —
  `cast_possible_truncation`, `cast_possible_wrap`,
  `cast_precision_loss`, `cast_sign_loss`, `missing_errors_doc`,
  `missing_panics_doc`, `module_name_repetitions`,
  `must_use_candidate`, `module_inception` — followed by ~20
  hand-picked nursery/restriction lints individually set to `warn`
  (`use_self`, `implicit_clone`, `redundant_type_annotations`,
  `or_fun_call`, `missing_const_for_fn`, `string_slice`, …).
  `clippy::nursery`, `clippy::cargo`, `clippy::restriction` are
  **never enabled as groups**.
- Every member crate uses `[lints] workspace = true` (verified across
  all 35 such `Cargo.toml`); `missing_docs` is enforced **per-crate
  at `#![warn(missing_docs)]`** (not deny) and becomes hard only via
  the docs CI job's `RUSTDOCFLAGS: -Dwarnings`. The single
  module-scoped escalation in the whole tree is `text/line.rs`'s
  `#![deny(missing_docs)] #![warn(clippy::pedantic, clippy::nursery,
  clippy::arithmetic_side_effects)]` — proving even ratatui treats
  group escalation as a rare, narrowly-scoped exception.
- CI: clippy run as `cargo clippy --all-features --all-targets
  --workspace … -- -D warnings` (stable = hard gate, beta
  non-blocking); a separate **docs job** with
  `RUSTDOCFLAGS: -Dwarnings`; nightly `cargo fmt --all --check` +
  `taplo` TOML fmt; an **MSRV leg pinned to `1.88.0`** in the
  build/test matrix; `cargo-deny` (licenses/bans/sources gating,
  advisories non-blocking), `cargo-machete`, `typos` all gating.
  `clippy.toml` is exactly `avoid-breaking-exported-api = false`.

**longbridge/gpui-component** — Rust breadth analog. Clippy-only
policy: no `[workspace.lints.rust]`/`rustdoc` tables; the entire
`clippy::style` group set to `allow`, only `dbg_macro`/`todo`
escalated to `deny`; **no `pedantic`/`nursery`/`cargo`/`restriction`**;
**no `unsafe` constraint anywhere**; **no `missing_docs` policy**; no
declared MSRV; clippy `--deny warnings` gates CI but only on the
macOS leg with default features; no fmt-check, no rustdoc job.

**rivet-dev/secure-exec** — Rust, security-sensitive. **No `[lints]`
table, no `clippy.toml`/`rustfmt.toml`, and crucially no
`forbid(unsafe_code)` despite the security focus** — `unsafe` is
pervasive (~47 sites, V8 FFI), constrained only by local
`#[allow(...)]`. Enforcement is purely CI: `cargo clippy --all-targets
-- -D warnings` + `cargo fmt --all -- --check` as a hard pre-build
gate on a toolchain pinned to `1.85.0`.

**Cross-language posture** — `charmbracelet/bubbletea`: a curated
golangci-lint v2 set (incl. `gosec`) gating CI deny-by-default.
`anomalyco/opentui`: an intentionally near-empty ruleset but CI
enforces `--deny-warnings` + format-check as required checks.
`anomalyco/opencode`: linter/formatter exist but **no CI lint/format
gate at all** (the outlier; enforced only via local husky).

**Synthesis used to shape the decision.** CI clippy `-D warnings` is
the one universally-enforced rule (rstui already has it — Decision
§2). `pedantic` is *only ever* opt-in **at warn** with an explicit
allow-list (Decision §3); `nursery`/`cargo`/`restriction` are *never*
whole-group (Decision §5). `unsafe`-forbid is domain-dependent —
appropriate for pure-logic rstui, impossible for FFI `secure-exec`
(Decision §6). `missing_docs` is realistically *warn + rustdoc
`-Dwarnings`*, not crate-wide `deny` (Decision §1, §4).
`[lints] workspace = true` with central tables is the standard
mechanism (Decision §1). MSRV gating is inconsistent;
`ratatui`'s declared-`rust-version`-plus-pinned-CI-leg is the robust
model (Decision §8). Supply-chain tooling (`cargo-deny`,
`cargo-machete`, typos) correlates with project maturity and is worth
adopting as the project grows (Decision §7).

## Consequences

**Positive**

- The maintainability bar is set in the young-codebase window the
  steering note targets, and is the bar every future
  machine-generated slice is held to.
- Phase 1 closes the **only silent-failure gap**: broken intra-doc
  links and missing docs on new public items currently pass CI; after
  Phase 1 they fail it, at ≈ zero churn cost.
- One single source of truth for lint policy (`[workspace.lints.*]`),
  removing eight duplicated per-crate attributes.
- Scope is evidence-bounded: by *not* adopting
  nursery/cargo/restriction groups, rstui avoids the precise churn
  trap the steering note warned about, with `ratatui` as the proof
  that this is the realistic Rust ceiling.
- The plan is decomposed so every lint change is an independently
  reviewable slice and the one broad change (pedantic) is isolated —
  exactly the user's "record … before applying broad lint changes"
  instruction and the objective's reviewable-slice rule, satisfied
  structurally.

**Negative / accepted**

- Consolidating per-crate `lib.rs` attributes into the workspace
  table changes *where the policy is read* (Cargo.toml, not lib.rs).
  Accepted: it is behavior-identical, matches `ratatui`, and the ADR
  + a workspace-table comment document the location.
- Phase 2 (pedantic) will touch many files at once. Accepted and in
  fact *intended*: it is deliberately its own slice — the "broad lint
  change" the user said must be planned before applied — bounded by
  the small four-crate codebase and reviewed on its own.
- More CI jobs over time (rustdoc, later supply-chain, MSRV). Accepted:
  each is cheap, gates a real defect class, and lands incrementally.

**Neutral / deferred**

- `clippy::nursery` / `clippy::cargo` / `clippy::restriction` as
  whole groups — not planned; reversal requires a new ADR.
- A nightly `rustfmt.toml` — deferred; revisited only against a
  measured formatting-churn cost (Decision §9).
- An MSRV CI leg — Phase 3, low urgency for a dependency-light core.
- `#![deny(missing_docs)]` (vs `warn` + rustdoc `-Dwarnings`) —
  deferred; the rustdoc gate already makes missing docs fail CI, so
  crate-wide `deny` adds friction without additional signal.

## Follow-up

This ADR discharges the iteration-18 lint-policy steering question and
is the reference contract for the lint-related slices. Each phase is
its own iteration; none bundles with unrelated feature work.

1. **Phase 1 (next slice — narrow, behavior-identical + the doc
   gate).** Add `[workspace.lints.rust]` (`unsafe_code = "forbid"`,
   `missing_docs = "warn"`) and `[workspace.lints.rustdoc]`; delete
   the four `#![forbid(unsafe_code)]` and four `#![warn(missing_docs)]`
   inner attributes; add the `cargo doc --no-deps --all-features
   --workspace` CI step under `RUSTDOCFLAGS: -D warnings`; update the
   README build/CI section. Verify zero clippy/test regressions and a
   clean `cargo doc`. This is *not* a broad lint change — it is a
   consolidation plus one new, near-zero-churn gate.
2. **Phase 2 (its own slice — the one broad lint change).** Add
   `clippy::pedantic = { level = "warn", priority = -1 }` with an
   allow-list seeded from `ratatui`'s nine and pruned to rstui's
   actual hits; run clippy locally; triage **every** finding and
   either fix it or add a per-lint `allow` with a one-line rationale
   comment. Reviewed entirely on its own.
3. **Phase 3 (independent slices, ordered by value/cost).**
   `cargo-deny` + `deny.toml` → `cargo-machete` → the iteration-19
   vague-generic-naming check as the first rstui-specific lint
   (`xtask`, housed with the other gates) → the MSRV CI matrix leg.
4. **Explicitly not planned** (would each require a new superseding
   ADR): nursery/cargo/restriction as whole groups; a nightly
   `rustfmt.toml`; crate-wide `#![deny(missing_docs)]`.
