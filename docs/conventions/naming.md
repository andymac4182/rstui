# Naming convention: no vague generic names

**Status:** Enforced in CI (`cargo xtask lint-names`) and by
`cargo test` · **Since:** 2026-05-17 · **Authority:**
[ADR 0003](../adr/0003-lint-and-code-quality-policy.md) §7

## Rule

Crate names, source-file and module paths, `mod` declarations, and
public-item declarations must describe their responsibility. Generic,
intent-hiding bucket names are banned:

> `helper` · `helpers` · `util` · `utils` · `common` · `misc` ·
> `stuff` · `shared` · `thing` · `things`

Prefer a name that says what the thing *is* or *does* — the codebase
already models this: `layout`, `event_source`, `backend`, `lifecycle`,
`stylize`, `style_cascade`, `text` (the wrap composer), `from_crossterm`.
If the only honest name for a module is `utils`, that is a signal the
module has no single responsibility — split it, don't name around it.

## Why a custom check

clippy and rustdoc cannot see this defect class. rstui is an
agent-driven codebase: the names every future machine-generated slice
introduces are held to whatever bar is enforced *mechanically*, so the
guardrail is set early (the iteration-19 steering note's explicit ask)
while the codebase is still clean and churn is cheap.

## Scope (deliberately precise)

ADR 0003 treats false-positive churn as the cost to minimise. The check
is scoped tightly so it produces signal, not noise:

| Checked | Not checked |
| --- | --- |
| `crates/*` package names | `let` bindings, fields, fn params |
| `.rs` file stems & non-structural dir components | Cargo-structural dirs (`src`, `tests`, `examples`, `benches`, `bin`) |
| `mod NAME` — **any** visibility (a module name hides intent regardless of who sees it) | `pub(crate)` / `pub(super)` / `pub(in …)` items (internal, not public API) |
| **Fully-`pub`** `fn`/`struct`/`enum`/`trait`/`type`/`union`/`static`/`const` | `pub use` re-exports (introduce no new name) |
| | Prose, doc comments, string literals |

Two deliberate decisions, recorded so they are not re-litigated:

1. **Whole word-segment matching, not substring.** Identifiers are split
   on `_`/`-`/`.` and `camelCase`/`PascalCase` boundaries, then each
   segment is matched exactly. `event_source` → `event`,`source`;
   `uncommon` stays one segment and is *not* `common`. This trades
   catching the rare `miscellaneous` for never misfiring on a word that
   merely contains a banned substring — the precise churn trap the
   policy exists to avoid. `misc`/`utils`/`helpers` (the patterns that
   actually occur) are still caught.
2. **Prose is not scanned.** "Handles the common case" is legitimate
   English; gating on it would be noise. The convention still applies to
   prose by review — this table just bounds the *automated* gate to
   identifiers and paths, exactly the steering note's CI scope ("new
   modules/files/public items").

## Exceptions

If a name must be kept despite a banned segment, add it (exact
identifier or workspace-relative path) to `ALLOWED_EXCEPTIONS` in
`crates/xtask/src/naming.rs` **and** document it here with its
rationale. The bar is "an established, specific domain meaning", per the
steering note.

- **Current exceptions: none.** The workspace is clean.
- The `xtask` crate name (the cargo-xtask convention ADR 0003 §7
  endorses) needs no entry: `xtask` contains no banned segment, so it is
  not flagged in the first place.

## Running it

```sh
cargo xtask lint-names        # the gate; exits non-zero on a violation
cargo run -p xtask -- lint-names   # without the .cargo/config.toml alias
```

Enforced two ways on purpose ("structural, not aspirational"): the
dedicated CI **Naming** step, and a `#[test]` in `xtask` that scans the
workspace — so `cargo test` fails too, independently of CI. Extending
the banned set is a one-line edit to `BANNED_SEGMENTS`.
