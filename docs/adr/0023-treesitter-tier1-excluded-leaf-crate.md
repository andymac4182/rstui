# ADR 0023: tree-sitter Tier-1 as a workspace-excluded opt-in leaf crate

- **Status:** Accepted
- **Date:** 2026-05-19
- **Deciders:** rstui maintainers
- **Refines:** [ADR 0022](0022-syntax-colour-and-symbol-engine.md) (the
  decision stands; only the *mechanism* of its Tier-1 clause is refined)

## Context

[ADR 0022](0022-syntax-colour-and-symbol-engine.md) decided the syntax/symbol
engine: a dependency-free language-aware lexer is the always-on Tier-0 floor;
**tree-sitter is Tier-1 — optional, per-language feature-gated, one parse →
highlight *and* symbols, and crucially "default builds never compile it and
**the five CI gates do not depend on it**" (ADR 0018 opt-in-leaf precedent).**
ADR 0022's *mechanism* sentence said this lives "as an opt-in adapter on
`rstui-widgets`" behind a `tree-sitter` cargo feature.

Implementing it surfaced a hard incompatibility between that mechanism and
ADR 0022's own invariant. The `cargo xtask ci` gates run (verbatim,
`crates/xtask/src/ci.rs`):

- `clippy --all-targets --all-features -- -D warnings`
- `doc --no-deps --all-features --workspace`
- `test --all-features`

and the workspace is `members = ["crates/*"]` (no `default-members`, no
`exclude`). A cargo **feature** on a workspace member is enabled by
`--all-features`, and `--workspace` selects every member. So a `tree-sitter`
feature on `rstui-widgets` (or an optional dep behind a feature on any gated
member, e.g. `rstui-git-review`) **is compiled and linked by every one of the
five gates** — pulling the `tree-sitter` C runtime plus a `cc`-built grammar
per language into `cargo xtask ci`. That is precisely the cost ADR 0022 and
ADR 0018/0005 forbid ("opt-in, not a gate"; bench/heavy work never gates).
ADR 0022's mechanism clause is therefore internally inconsistent with its own
"the five CI gates do not depend on it" invariant **as the gate is actually
defined**. The decision is sound; the mechanism needs refining.

`rstui-devtools` (the ADR 0018 leaf) does *not* solve this by analogy: it is a
*lightweight* gated member that opts out of the workspace `unsafe_code`
lint — it is still compiled by the gate, which is fine because a counting
allocator is tiny. A multi-language tree-sitter stack is the opposite: heavy
C builds we explicitly do not want in the inner loop.

## Decision drivers

1. **Honour ADR 0022's invariant literally**: the five CI gates must not
   compile tree-sitter or any grammar (drivers 2 & the opt-in-leaf posture of
   ADR 0022/0018/0005).
2. **Preserve every other part of ADR 0022 unchanged**: one parse →
   highlight + symbols, per-language gating, capture→`rstui-theme` colour,
   Tier-0 always present and the default, `unsafe_code` containment.
3. **Still trivially usable**: an app (or example) opts in with one path
   dependency / one build command; no fork, no vendoring ceremony.
4. **Minimal blast radius / no relitigation** of ADR 0022's engine choice.

## Options considered

- **A. `tree-sitter` feature on `rstui-widgets` (ADR 0022 literal).**
  Rejected: `--all-features [--workspace]` pulls it into all five gates —
  violates ADR 0022's own invariant. No feature arrangement escapes
  `--all-features` while remaining a feature of a workspace member.
- **B. Optional dep behind a feature on `rstui-git-review`/an app.** Same
  defect transitively — `--all-features` on that gated member compiles it.
- **C. A new crate under `crates/` (a normal member).** Still a member ⇒
  `--workspace` gates it. Rejected.
- **D. A new crate under `crates/` added to `[workspace] exclude`.** Not a
  member ⇒ `--workspace`/`--all-features` skip it entirely; still builds via
  `cargo build -p rstui-treesitter` and is consumable as a path dep by an
  app/example that itself sits outside the gated set. Honours the invariant
  exactly, preserves everything else. **Chosen.**

## Decision

Realise ADR 0022's Tier-1 as a **new crate `crates/rstui-treesitter` listed
in `[workspace] exclude`** (the workspace stays `members = ["crates/*"]`;
`exclude` wins for that path). Consequently:

- The five `cargo xtask ci` gates never see it (`--workspace`/`--all-features`
  skip an excluded path) — ADR 0022's invariant now holds *as the gate is
  actually defined*. Post-push CI likewise (it runs the same gates).
- It depends on `rstui-core` (`Style`) and `rstui-widgets` (re-uses the
  **landed** `Outline`/`Symbol`/`SymbolKind` and `syntax::SyntaxStyles`
  types, so its output is a drop-in for the existing `Editor::syntax(&[Style])`
  overlay and the existing outline panel — Tier-1 is a *better producer* of
  the *same* shapes, never a new widget), plus `tree-sitter` and one
  `tree-sitter-<lang>` grammar crate per **per-language cargo feature**
  (a default-on core set: rust/python/js/ts/go/c/json/markdown/…).
- One parse drives **both** outputs (ADR 0022 driver 1): a `highlights.scm`
  query → the per-char `Vec<Style>` overlay (capture name →
  `rstui_widgets::syntax::SyntaxStyles`/theme colour, never a bundled foreign
  theme); a `tags.scm` query → `rstui_widgets::Outline`. A caller-owned cache
  holds the `tree-sitter::Tree` for incremental re-parse (ADR 0012:
  caller-owned model, pure-projected widget).
- Because it is gate-excluded, it is **validated out-of-gate**, the same
  posture `rstui-bench` has (ADR 0005): its own `cargo test/clippy/doc -p
  rstui-treesitter` must be green, run explicitly, not by `cargo xtask ci`.
- The "looks like a proper code editor with real colours" runnable
  deliverable ships **inside this excluded crate** (a `code_editor` example
  composing the rstui `Editor` + this adapter + crossterm), so the showcase
  is gate-safe too. Tier-agnostic editor polish (e.g. current-line
  highlight) stays in the gated `rstui-widgets` so Tier-0 benefits equally.

`unsafe_code`: the `tree-sitter` crate exposes a **safe** Rust API (`Parser`,
`Tree`, `Query`, `QueryCursor`); grammar crates expose a safe `fn language()`.
This crate writes **no `unsafe`** and keeps `unsafe_code = "forbid"`. The C
FFI lives inside the upstream `tree-sitter` crate, exactly the containment
ADR 0022 intended (and stricter than ADR 0018's devtools, which needed
scoped `unsafe`).

## Consequences

**Makes easy.** ADR 0022 ships in full with its no-gate invariant actually
true. Default builds and every CI gate are byte-for-byte unaffected (zero new
deps, the floor is unchanged). An app opts in with one path dep + feature
flags; the example is a runnable proper code editor with accurate colours and
the landed symbol/scroll/selection/search/undo feature set.

**Makes hard / accepted.** A gate-excluded crate is not protected by
`cargo xtask ci`; its correctness is enforced by an explicit, documented
per-crate gate (the `rstui-bench` precedent) and must be run before landing
changes to it. Enabling many grammar features grows *opt-in* build time only.
A consumer must add the crate as a path dep + pick language features — a
deliberate, one-line opt-in, never transitive.

## See also

- [ADR 0022](0022-syntax-colour-and-symbol-engine.md) — the engine decision
  this refines the Tier-1 mechanism of (decision unchanged).
- [ADR 0018](0018-devtools-and-perf-tooling.md) /
  [ADR 0005](0005-benchmarking-and-profiling-strategy.md) — the opt-in-leaf
  / validated-out-of-gate precedents this follows.
- [ADR 0002](0002-widget-crate-boundary.md) — the dependency-free-core /
  feature-gated-heavy-dep boundary.
- [`code-editor-and-diff-deep-dive.md`](../code-editor-and-diff-deep-dive.md)
  — Part 7 `CE-4` (this crate) and the full code-editor feature set.
