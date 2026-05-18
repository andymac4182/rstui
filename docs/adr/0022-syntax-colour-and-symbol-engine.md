# ADR 0022: Syntax-colour & symbol engine (dependency-free floor + optional tree-sitter tier)

- **Status:** Accepted
- **Date:** 2026-05-18
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

[`code-editor-and-diff-deep-dive.md`](../code-editor-and-diff-deep-dive.md)
established that `Editor` has **no** syntax colour at all and `Diff`'s is a
language-blind lexical floor (`diff.rs` `syntax_overlay`: one global keyword
set, no language detection, comments/strings never span lines), and that the
user requires real colouring **plus** a symbol/outline side panel for both
the editor and the files in a diff. Two capabilities, one decision: what
engine produces the token colours and the symbol tree.

The forces this must fit (it relitigates none of them):

- **`rstui-core`/`-widgets`/`-runtime` are dependency-free and
  `unsafe_code = "forbid"` workspace-wide (ADR 0001/0002/0003).** ADR 0002
  §Decision already anticipates *exactly this*: "code highlighting …
  per-language feature-gated (each a separate heavy dep)" and "markdown/code
  … will eventually need optional feature modules … into a dependency-free
  core." This ADR specifies that anticipated module; it does not widen the
  boundary.
- **Pure projection, no retained tree (ADR 0012).** Whatever the engine
  produces is *caller-owned model data* the widget reads per cell — the
  `extmark`/`Selection` seam — never interior widget state.
- **`cargo xtask ci` has five gates and bench is never one (ADR 0005/0018).**
  An optional heavy engine must not enter the default build or the gates; the
  opt-in leaf posture of ADR 0018 (`rstui-devtools`) is the precedent.
- **The requirement is two outputs, not one.** Colour *and* a symbol
  outline (deep-dive Part 5). The engine choice must be judged on whether one
  parse yields both, because a highlighter that does not also give structure
  forces a *second* engine for symbols.

## Decision drivers

1. **Two outputs from one parse** — highlight tokens *and* a definition/scope
   tree, or we pay for two engines.
2. **Core purity / default-build footprint** — the default build stays
   dependency-free; any real engine is opt-in and per-language gated
   (ADR 0002).
3. **Incrementality** — an editor re-highlights on every keystroke; whole-file
   re-tokenisation per key is the wrong cost curve.
4. **Offline, deterministic floor** — colour must degrade to a zero-dep,
   panic-free, snapshot-testable path with no network and no C toolchain.
5. **Theme coherence** — colours must track the active `rstui-theme`, not a
   bundled foreign theme format (the deep-dive's hunk lesson).
6. **Maintained grammars** — we do not want to hand-maintain language
   definitions.

## Options considered

### A. Dependency-free hand-written lexer only (today's floor, made real)

Extend the `diff.rs` scanner: per-`Language` rulesets, carried end-of-line
state for multi-line strings/comments. Zero deps, fast, deterministic,
snapshot-testable. **But** purely lexical — no parse tree, so **no symbols**;
and heuristic (macros, generics, here-docs mis-tint at the edges).
Necessary, not sufficient.

### B. TextMate grammars + Oniguruma / syntect (hunk's / VS Code's stack)

`syntect` loads Sublime/TextMate `.sublime-syntax` grammars, tokenised by
`onig` (the Oniguruma C library, FFI `unsafe`, a C build) or `fancy-regex`
(pure-Rust, slower). Bundles many grammars and themes.

- Solves **only highlighting**. TextMate grammars are regex token scopes,
  **not** a syntax tree — they yield no definition/scope structure, so the
  **symbol panel still needs a second engine** (tree-sitter or a ctags-class
  tool). We would carry a heavy dependency that solves half the requirement
  at full cost (driver 1, decisively).
- Not incremental: TextMate tokenisation is stateful per line over the whole
  buffer; there is no cheap "re-parse only the edited range" (driver 3).
- Theme-format mismatch: syntect themes are `.tmTheme`; we want
  `rstui-theme` colours (driver 5) — the exact friction hunk has (it pins
  Pierre/Shiki themes and cannot use its own UI theme for tokens).
- `onig` adds a C toolchain + FFI `unsafe` (tolerable only in an opt-in
  leaf, ADR 0018); `fancy-regex` avoids that but is materially slower.

### C. tree-sitter, optional & per-language feature-gated

`tree-sitter` + per-language grammar crates (`tree-sitter-rust`, …). An
incremental GLR parser producing a concrete syntax tree; **`highlights.scm`**
queries give token classes and **`tags.scm`/`locals.scm`** queries give the
definition/symbol tree — **both from the one parse** (driver 1). Incremental
re-parse of just the edited range (driver 3). Grammars are upstream-maintained
and shared across the ecosystem (driver 6). Token *colours* are mapped from
capture names to `rstui-theme` (driver 5). Cost: a real dependency with C
grammar code — so **opt-in, default-off, each grammar its own feature**,
exactly ADR 0002 §Decision-4's prescribed shape and ADR 0018's opt-in-leaf
posture (drivers 2, 4 satisfied by *also* keeping Option A as the floor).

## Decision

**Two tiers. Ship both.**

- **Tier 0 — the always-compiled, dependency-free floor (Option A).** The
  `diff.rs` lexer is extracted into a shared `syntax` module used by **both**
  `Diff` and `Editor`, made language-aware (a `Language` resolved from file
  extension by the app/`Cmd`; the widget stays pure) with a carried
  end-of-line `LexState` so multi-line strings/comments colour correctly. A
  matching dependency-free heuristic `outline` scanner produces the symbol
  list. This is the default path; it needs no features, no network, no C
  toolchain, is panic-free and snapshot-tested. It is **language-aware but
  heuristic** — explicitly a floor, like the diff tinter is a tinting floor.
  *(Landed: `rstui_widgets::syntax` + `rstui_widgets::outline`, this slice.)*

- **Tier 1 — optional, feature-gated tree-sitter (Option C).** Behind a
  default-off `tree-sitter` feature (and one sub-feature per language), a
  single tree-sitter parse drives **both** the highlight overlay (via
  `highlights.scm`, capture → `rstui-theme` colour) **and** the symbol
  outline (via `tags.scm`). One engine, two outputs, incremental. It lives as
  an opt-in adapter on `rstui-widgets` (ADR 0018's leaf precedent); default
  builds never compile it and the five CI gates do not depend on it.

**TextMate grammars + Oniguruma/syntect (Option B) is rejected** as the
engine: it solves only highlighting, so the symbol panel — a first-class
requirement here — would still force tree-sitter; carrying syntect *and*
tree-sitter to get colour+symbols is strictly worse than tree-sitter alone,
which gives both. Its non-incremental model and `.tmTheme` format are
secondary strikes. (Borrowing hunk's *layering* — colour first, then diff
backgrounds, then word-diff emphasis — is unaffected and already how
`diff.rs` `content_spans` works; that lesson stands regardless of engine.)

tree-sitter is **never the default** (ADR 0002 core purity, driver 2); the
floor is **always** present (driver 4). This *specifies* the deep-dive's and
ADR 0002's "feature-gated optional, never default" stance — it does not
overturn it.

## Evidence

- **hunk** (research, deep-dive Part 1): TypeScript on Shiki = TextMate
  grammars + Oniguruma-WASM; **no symbol panel anywhere in its repo** —
  concrete proof Option B's stack does not yield structure for free, and that
  a symbol panel is independent work whatever the highlighter.
- **The ecosystem that needs colour *and* structure converges on
  tree-sitter:** Helix, Zed, Neovim, and the structural-diff tool
  `difftastic` all use tree-sitter precisely because one parse serves
  highlighting *and* navigation/structure; none use TextMate for the
  structural half.
- **ADR 0002 §Decision-4** already names "code highlighting … per-language
  feature-gated (each a separate heavy dep)" and §Context the "markdown/code
  … optional feature modules into a dependency-free core" — Tier 1 is the
  realisation of an already-accepted boundary, not a new one.
- **The floor already exists and is validated:** `diff.rs` `syntax_overlay`
  (lexical scanner) and `content_spans` (the 3-layer colour→diff-bg→
  word-emphasis cascade hunk also uses) — Tier 0 is an extraction +
  generalisation of shipping, tested code.

## Consequences

**Makes easy.** Colour works on day one with zero new dependencies for every
default build, offline, deterministic, snapshot-tested. The symbol panel
(deep-dive Part 5) and accurate highlight come from the *same* opt-in
tree-sitter parse — no second engine, no duplicated language config.
Per-language gating means a build pays only for the grammars it enables.
Colours stay in `rstui-theme`, so every existing theme themes code for free.

**Makes hard / deferred.** The Tier-0 floor is heuristic: exotic syntax
(nested macros, here-docs, regex literals) and deep symbol nesting are
approximate until Tier 1 is enabled — accepted, and the explicit reason Tier 1
exists. Enabling many tree-sitter languages grows *opt-in* build time
(bounded by features, never unconditional). LSP / semantic analysis /
cross-file go-to-definition remain **non-goals** (deep-dive Part 8): the
panel is *structure of the open/changed file*, not a language service.
tree-sitter grammars carry C code: the Tier-1 adapter is an opt-in leaf and
does not relax the workspace `unsafe_code = "forbid"` for core/widgets/runtime
(same containment as ADR 0018's counting allocator).

## See also

- [`code-editor-and-diff-deep-dive.md`](../code-editor-and-diff-deep-dive.md)
  — Parts 3 & 5 (the two-tier colour + symbol design this ADR decides) and
  Part 7 (`CE-4/5/6/7` the slices that implement it).
- [ADR 0002](0002-widget-crate-boundary.md) — the dependency-free-core /
  feature-gated-heavy-dep boundary this ADR instantiates.
- [ADR 0012](0012-widget-composition-and-layout-model.md) — why the engine's
  output is caller-owned model data, not widget state.
- [ADR 0018](0018-devtools-and-perf-tooling.md) — the opt-in-leaf, not-a-gate
  posture Tier 1 follows.
