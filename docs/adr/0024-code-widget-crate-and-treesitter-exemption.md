# ADR 0024: A `rstui-code` widget crate with a first-class tree-sitter exemption

- **Status:** Accepted
- **Date:** 2026-05-19
- **Deciders:** rstui maintainers
- **Supersedes:** [ADR 0023](0023-treesitter-tier1-excluded-leaf-crate.md)
- **Amends:** [ADR 0022](0022-syntax-colour-and-symbol-engine.md) (Tier-1
  mechanism only — the engine choice stands), [ADR 0002](0002-widget-crate-boundary.md)
  (a scoped, documented dependency exemption)

## Context

[ADR 0022](0022-syntax-colour-and-symbol-engine.md) chose tree-sitter for
Tier-1 (accurate colour + symbols from one parse); [ADR 0023](0023-treesitter-tier1-excluded-leaf-crate.md)
realised it as a **workspace-`exclude`d, validated-out-of-gate leaf**
(`rstui-treesitter`) so the `--all-features --workspace` CI gates would not
compile it.

That mechanism has a real cost the maintainers have now judged unacceptable:

- **tree-sitter is not fringe — it is the core of a real code editor.** An
  editor without accurate, structural highlighting and a symbol outline is
  not "a proper code editor". Treating its engine as an opt-in leaf that the
  five gates *deliberately do not protect* (the `rstui-bench` posture)
  permanently under-tests the single most load-bearing capability of the
  code-editing surface. ADR 0023's own "Consequences" flagged this; the
  decision here is that the under-protection is the bigger risk.
- **The excluded-leaf showcase is awkward.** The "proper code editor" only
  existed inside an excluded crate's example; the shipped, gated `Editor`
  could never *itself* offer accurate colour without an out-of-gate opt-in.
- **The dependency-free boundary (ADR 0002) was the only real objection,**
  and ADR 0002 §Decision-4 already anticipates "code highlighting …
  per-language … (each a separate heavy dep)". The boundary's intent is that
  the *universally-depended-on* substrate (`rstui-core`) and the *general*
  widget set (`rstui-widgets`) stay lean — **not** that no widget crate may
  ever have a domain dependency.

## Decision drivers

1. **tree-sitter is core to the code editor**, gate-protected like any other
   shipped capability — not an out-of-gate opt-in (the maintainer directive
   this ADR encodes).
2. **`rstui-core` and `rstui-widgets` stay tree-sitter-free**, so the
   universal substrate and the general widget set carry zero new weight.
3. **Only consumers of the code widgets pull tree-sitter** — a crate
   boundary, not a feature flag, so the dependency is transitive-by-use and
   impossible to acquire accidentally.
4. **One coherent code-editor crate**: the `Editor`/`Diff` widgets and their
   engine ship and are tested together.

## Decision

**Add `rstui-code` — a normal, gated workspace member — and move the
code-editing widgets there with tree-sitter as a first-class dependency.**

- **`rstui-code` contains**, moved verbatim out of `rstui-widgets`:
  `editor` (`Editor`), `diff` (`Diff`/`DiffTheme`/`DiffLayout`),
  `syntax`, `outline`, `changeset`, `line_number_gutter`; **plus** the
  former `rstui-treesitter` engine folded in as `rstui_code::treesitter`
  (`Analyzer`/`TsLanguage` — one `tree_sitter::Tree` → highlight overlay +
  `Outline`, the ADR 0022 design, unchanged). `crates/rstui-treesitter` and
  the `[workspace] exclude` are deleted.
- **Dependencies:** `rstui-core`, `rstui-widgets` (for `Block`/`Extmark` —
  `extmark` stays in `rstui-widgets`, also used by `Input`), `tree-sitter`,
  and one `tree-sitter-<lang>` grammar per **per-language cargo feature**
  (default = the core set rust/python/js/ts/go/c/json/markdown). The
  grammars are per-language gated (ADR 0002 §Decision-4) but tree-sitter
  itself is **not** optional and **not** out-of-gate.
- **`rstui-code` is a gated `crates/*` member**, so `cargo xtask ci`'s five
  gates compile and test it — **including tree-sitter**. This is the
  **tree-sitter exemption**: ADR 0002's dependency-free rule is scoped to
  `rstui-core`/`rstui-widgets`/`rstui-runtime`; `rstui-code` is explicitly
  exempt because tree-sitter is its domain core. Accepted cost: CI compiles
  the core grammar set (a one-time `cc` build, cached) on the `rstui-code`
  leg only.
- **No cycle:** `rstui-code → rstui-widgets → rstui-core`. `rstui-widgets`
  and `rstui-core` never depend on `rstui-code` or tree-sitter; an
  application acquires tree-sitter **iff** it depends on `rstui-code`
  (driver 3). `rstui-widgets` examples that demoed the moved widgets move to
  `rstui-code/examples` (a crate may not depend on a crate that depends on
  it).

This **supersedes ADR 0023** (the excluded-leaf mechanism is withdrawn) and
**amends ADR 0022's Tier-1 mechanism**: tree-sitter is now a default,
gate-protected dependency of `rstui-code`, not an optional never-gated leaf.
ADR 0022's *engine choice* (tree-sitter over TextMate/Oniguruma; one parse →
both outputs; capture → `rstui-theme` colour; Tier-0 dependency-free lexer
remains the always-present floor inside `rstui-code`) is **unchanged**.

## Consequences

**Makes easy.** Accurate colour + symbols are a first-class, gate-protected
part of the shipped `Editor`/`Diff` — a genuinely "proper code editor". One
crate to depend on for the whole code-editing surface. `rstui-core`/
`rstui-widgets` are byte-for-byte unchanged in weight (tree-sitter-free); a
chat composer or form using *other* widgets never sees tree-sitter.

**Makes hard / accepted.** The `cargo xtask ci` `rstui-code` leg now
compiles the default grammar set — a bounded, cached one-time `cc` cost on
the gate (the maintainers' explicit trade for gate-protecting the editor's
core). A consumer of `Editor`/`Diff` now also compiles tree-sitter — the
intended, transitive-by-use behaviour, not accidental. The Tier-0 lexer
stays as the zero-dep fallback path *within* `rstui-code` (offline/no-cc
robustness, and the always-present floor ADR 0022 mandates).

## See also

- [ADR 0023](0023-treesitter-tier1-excluded-leaf-crate.md) (superseded) ·
  [ADR 0022](0022-syntax-colour-and-symbol-engine.md) (engine, amended
  mechanism) · [ADR 0002](0002-widget-crate-boundary.md) (boundary, scoped
  exemption) · [ADR 0018](0018-devtools-and-perf-tooling.md) (the
  opt-in-leaf precedent this case is judged *not* to fit).
- [`code-editor-and-diff-deep-dive.md`](../code-editor-and-diff-deep-dive.md)
  — the full code-editor feature set now homed in `rstui-code`.
