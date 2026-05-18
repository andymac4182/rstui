# ADR 0020: Keymap contexts (modes & text input)

- **Status:** Accepted
- **Date:** 2026-05-18
- **Deciders:** rstui maintainers
- **Amends:** [ADR 0015](0015-keymap-architecture.md) — lifts its
  *"resolution is shell-level only / per-widget keymaps are not
  modelled"* consequence. Everything else in 0015 stands.

## Context

ADR 0015 deliberately kept the keymap engine *shell-level only*: a chord
maps to an `Action` with no notion of focus or mode. Context-sensitivity
— `q` quits the shell but must *type* into a focused field; arrows move a
cursor, not the sidebar — was pushed entirely onto the consuming app's
reducer, which had to **hand-order guards** so every text/overlay context
`return`s before `Keymaps::dispatch`, plus hand-write a
"bare printable char while a field is focused ⇒ not a command" predicate
(`!ctrl && Char(_)`).

Building the three real apps proved this is the single sharpest edge of
the whole keymap system. The ordering is invisible, untyped, and silently
wrong when broken (get it wrong and `q` quits mid-word); the predicate
was copy-pasted; "do I even need this?" was a recurring question. The
mapping was already one line (`dispatch`); the *context* was the hard
part, and it was unmanaged.

## Decision drivers

- **Hard to get wrong** for someone implementing keybindings — the
  motivating goal ("make this simpler for people implementing our
  library").
- **Pure data, headless-testable, `rstui-core`-only** — the 0015
  invariants that make the engine good must survive.
- **Strictly additive** — three apps and the published crate already
  depend on the current behaviour; existing code must be byte-identical
  unless it opts in.
- **Familiar** — match a model implementers already know.

## Options considered

1. **Status quo + docs.** Document the guard ordering harder. Rejected:
   does not remove the footgun; the failure mode stays silent.
2. **Text-aware `dispatch` only.** A `dispatch_in_text(ev, now, typing)`
   that auto-`Fall`s bare printables. Small and real, but a point fix:
   handles only "field vs not", not modes/panes, and still leaves the
   general ordering problem.
3. **Per-binding predicate closures** (`.when(|app| …)`). Maximally
   flexible but breaks the pure-data / serialisable / headless model and
   is *more* ceremony per binding. Rejected.
4. **Named contexts (chosen).** A binding may be scoped to a context;
   `Keymaps` holds an active context stack the app sets on focus/mode
   change; a `Capture::Text` context makes a focused field swallow raw
   keys automatically. The distilled common core of Vim modes, VS Code
   `when` clauses, and Textual per-widget focus.

## Decision

Add a **pure-data context layer** to `rstui-keymap`:

- `Keymap::bind_in(context, action, specs)` / `bound_in(…)` — a binding
  scoped to a `&'static str` context. `bind`/`bound` remain **global**
  (eligible everywhere), unchanged.
- `enum Capture { Layer, Text }`. **Layer** (default): a normal
  mode/pane — its binds *shadow* globals, but a key it does not bind
  falls back to the global map. **Text**: a focused input / Vim-insert —
  *only* that context's explicit binds resolve; every other key is
  `Dispatch::Fall`, i.e. raw to the widget.
- `Keymaps`: an active **context stack** (`set_context` for the flat
  single-mode case, `push_context`/`pop_context` for nesting a modal over
  an editor) plus `register_context(name, Capture)`. `dispatch`'s
  signature is unchanged — context is engine state set out-of-band,
  exactly like the active keymap or the user overrides.
- `resolve` filters the effective binds by eligibility on **every** call
  (global ∪ active-stack contexts; context binds ordered first so they
  shadow globals; a `Text` top keeps only its explicit binds). An empty
  stack ⇒ globals only ⇒ **byte-identical to ADR 0015** — the shipped
  maps are all-global and their order is preserved.

The reducer no longer hand-orders guards or hand-writes the printable
predicate: it flips one value on focus/mode change and lets `dispatch`
return `Fall` for raw input. A context-scoped binding simply *does not
exist* outside its context, so `q` *cannot* fire Quit while the editor
context is active even if the wiring forgot a guard — the footgun is
removed *by construction*.

## Evidence

- **Vim** — modes (normal/insert/visual): the active mode selects the
  binding set; insert mode is the canonical "keys are text" case →
  `Capture::Text`.
- **VS Code** — `when` clauses (`editorTextFocus`, `terminalFocus`, …):
  bindings are inert unless their context predicate holds. Our
  `&'static str` context is the discrete, pure-data reduction of a
  `when` key (no expression language — YAGNI for a TUI).
- **Textual** — bindings resolve along the *focused* widget's ancestor
  chain; a focused `Input` consumes its keys. Our context **stack** is
  that focus chain made explicit and caller-owned (no retained widget
  tree — ADR 0012).

All three converge on "the active scope selects/filters the bindings,
and a text scope is just a scope that captures." We ship that core; the
app still owns *what* is focused (irreducible — only it knows), but
expresses it as one typed value, not fragile ordering.

## Consequences

**Easy now**

- Text inputs & modes are declarative: `bind_in` + one `set_context` on
  focus change. No guard cascade, no `!ctrl && Char(_)` predicate. The
  wrong-ordering footgun is gone by construction.
- `KeymapView`/help can show bindings per context (the active mode), a
  natural future extension of the reverse-lookup.
- Strictly additive: the published crate + every app that sets no
  context is unchanged; adoption is opt-in and incremental.

**Hard / deliberately deferred**

- The app must still tell the keymap when focus/mode changes — this is
  irreducible (only the app knows focus). The win is *typed value vs
  silent ordering*, not zero responsibility.
- Overrides / `keys_for` reverse-lookup remain context-agnostic for now
  (an override applies to an action regardless of context). Per-context
  remap/display is a future extension, not required by the model.
- `rstui-acp-client`'s bespoke modal/permission/ask/completion cascade is
  flow-control, not "keybindings"; it stays on the back-compat path
  (sets no context). Migrating it to nested contexts is a follow-up, not
  a prerequisite — the model is proven in `git-review` (modes) and the
  kitchen sink (the text-input case that motivated this).
