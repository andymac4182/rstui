# Widget entry template

Every widget on a `docs/widgets/<family>.md` page uses **exactly** this block,
in this order. Copy it verbatim and fill from the widget's module `//!` doc and
`impl` blocks (never from memory). Keep entries in the same order the family
page already uses; separate entries with a `---` rule.

```markdown
## WidgetName

![WidgetName demo](media/<example_stem>.gif)

One sentence: what it is and the one thing it is for. Mirror the
`crates/rstui-widgets/src/lib.rs` one-liner; do not embellish.

- **Companion types:** `TypeA`, `TypeB` (enum variants in parens) — omit the
  whole line if there are none.
- **State model:** one of —
  *pure projection of caller-owned `x`/`focused`/…* (name the exact state it
  reads, and link the core model if it borrows one, e.g.
  `[`TextEdit`](../core-reference.md#textedit)`);
  *pure layout* (computes child `Rect`s, renders no app data);
  *owns nothing* (purely decorative, caller-configured).

```rust
WidgetName::new(<args>)            // or ::default() — the real constructor
.builder_a(T) .builder_b(T)         // exact signatures from the impl block
.accessor(area: Rect) -> Rect       // include Rect accessors for containers
```

**Demo:** `cargo run -p rstui-widgets --example <example_stem>`
```

## Rules

- The GIF path is always `media/<example_stem>.gif` where `<example_stem>` is
  the file stem of the matching `crates/rstui-widgets/examples/<stem>.rs`
  (usually `<snake_name>_demo`). `cargo xtask record widgets` generates it.
- **State model is the most important line** — it is what the whole framework
  is about. Get the caller-owned state names exactly right; if the widget
  borrows a core model (`TextEdit`, `TextArea`, `ScrollState`, `Selection`,
  `FocusId`) link it into `docs/core-reference.md`.
- Signatures are *condensed but exact*: real method names, real argument
  types (`impl Into<Line>`, `Option<usize>`, `Constraint`, …). Drop `&mut
  self`/`-> Self`; keep `Rect`-accessor return types.
- Companion enums list their variants in parentheses:
  `BorderType (Plain/Rounded/Double/Thick)`.
- Cross-link reused widgets (e.g. Menu/Select/Sidebar "reuses
  `[List`](core-set.md#list)`").
- Update **both** `docs/widgets/README.md` tables when adding/renaming/
  removing: the family table and the alphabetical index (stay alphabetical).
