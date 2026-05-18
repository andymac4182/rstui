# ADR 0017: AI-app widgets and declarative agent-driven UI rendering

- **Status:** Accepted
- **Date:** 2026-05-18
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

Agents increasingly drive the UI. Two ecosystems matter for a terminal
client:

1. **Agent-authored declarative UI.** An agent emits a JSON document and
   the client renders it. Two live formats: Google **A2UI** (a versioned
   protocol — surface/component/data-model messages, JSON-Pointer data
   binding, a negotiated component catalog, an action return channel) and
   Vercel **json-render** (a flat element map streamed as RFC-6902 patches,
   `$`-prefixed prop expressions, directives, a component registry). Both
   exist to let a model build a UI without the client hard-coding it.
2. **AI-app chrome.** Independent of who authored the layout, a good AI
   client needs a *vocabulary*: a streaming-markdown view that survives a
   half-arrived token, a tool-call card with the full call state machine, a
   reasoning disclosure, a prompt composer, a token meter, citation chips,
   and so on. Vercel **streamdown** and **ai-elements** are the reference
   for that vocabulary, over the AI SDK v6 `UIMessage`/parts model.

rstui already has the substrate: an immediate-mode, pure-projection widget
model with no retained tree (ADR 0012), a hand-written `Markdown` and a
`Mermaid` module (ADR 0002 §4), a `ScrollState` sticky-bottom primitive
(ADR 0012 §P0), and an ACP chat client whose reducer is `await`-free
(ADR 0011). What is missing is (a) the AI-app widget vocabulary and (b) an
engine that turns an agent's JSON UI into rstui draw calls — and the ACP
client telling the agent it can render it.

The forces already locked in, which this decision fits rather than
relitigates:

- **Immediate-mode, pure `view(&self)`** — a widget is handed a `Buffer`
  and may not mutate; all state is caller-owned; the reducer is the sole
  mutation point (ADR 0012, `docs/composition.md`).
- **`rstui-core` is dependency-free; concrete widgets live in
  `rstui-widgets`** (ADR 0002). A new, heavier capability with its own
  dependencies (serde_json) belongs in its own crate, not core.
- Totality (panic-free on any input, including hostile/truncated agent
  JSON) and the `cargo xtask ci` gates (fmt, lint-names, clippy `-D`,
  rustdoc `-D`, test) apply to every slice.

## Decision

Add **two additive crates** and a **thin, additive ACP-client
integration**. No existing crate's public API changes.

### 1. `crates/rstui-ai` — the AI-app widget set + shared model

- `model` — the AI SDK v6 message model (`UiMessage`, `UiPart`,
  `ToolUiPart`, `ToolState`, `Role`, `TokenUsage`, `ChatStatus`),
  serde-deserializable from the wire shape ai-elements consumes. This is
  the single core type ~15 components are a projection of.
- `stream_markdown` — a Rust port of streamdown's `remend` incomplete-
  markdown repair (the fixed-priority handler pipeline) + block
  segmentation + a per-block render cache, projected through the existing
  `rstui_widgets::Markdown`/`Mermaid`. Streaming markdown is a *new
  behavior over an existing parser*, exactly the ADR 0002 §4 precedent.
- `diagram` — the diagram DSL an AI tool *outputs*: a pure projection that
  unwraps a fenced ```` ```mermaid ````/```` ```structurizr ```` block (or
  `Diagram::extract`s the first one from a chat turn) and delegates to the
  deterministic, total `rstui_widgets::Mermaid`/`Structurizr` — the same
  "new behavior over an existing parser" precedent as `stream_markdown`.
  Its contract is *advertised* to the agent by the
  `rstui-jsonui::capability` `diagram` descriptor (§2), so a model answers
  *with* a diagram instead of describing one in prose.
- One module per AI-app widget (the ai-elements vocabulary), each a pure
  projection of caller-owned state in the ADR 0012 discipline: a
  collapsible/disclosure family (`Reasoning`, `Tool`, `Task`, `Plan`,
  `ChainOfThought`), a chat transcript (`Conversation`, `Message`,
  `MessageBranch`), a composer (`PromptInput`), and the supporting
  cards/chips (`Sources`, `InlineCitation`, `Shimmer`, `Snippet`,
  `Artifact`, `AgentCard`, `Confirmation`, `TerminalView`, `StackTrace`,
  `ContextMeter`, `ModelSelector`, `Checkpoint`, `Commit`, `TestResults`,
  `PackageInfo`, `EnvVars`, `FileTree`, `SchemaView`, `WebConsole`,
  `Suggestion`). Web-only ai-elements (xyflow canvas, Rive persona,
  media-chrome audio, iframe preview) are explicitly out of scope; their
  portable sub-logic (e.g. console-log list, stack parsing) is kept.
- Depends only on `rstui-core` + `rstui-widgets` + `serde`/`serde_json`.

### 2. `crates/rstui-jsonui` — the declarative agent-UI engine

- `tree` — `UiNode`, the **single projection target** both formats
  compile to: a borrowed, immediate-mode renderable that maps to
  `rstui-widgets`/`rstui-ai` draw calls. There is **no retained widget
  tree**; a parsed document plus the caller-owned data model is re-walked
  every frame (ADR 0012). Interaction surfaces as pure hit-test accessors
  and a reducer-consumed event list, never callbacks (ADR 0012 §P1).
- `value` — a JSON-Pointer (RFC 6901) data store with upsert/delete and
  relative-scope resolution, shared by both formats.
- `a2ui` — A2UI v0.10: the six-message envelope, the 18-component basic
  catalog, the `Dynamic*` binding resolver (the 14 catalog functions +
  the `formatString` mini-grammar), `ChildList` static/template
  expansion, two-way input write-back, action→`client` JSON, and the
  capability descriptor.
- `jsonrender` — json-render: the flat `{root,elements,state}` map, the
  twelve-step prop resolver, the eight directives, the RFC-6902 patch
  stream compiler (line-buffered, LLM-brace-tolerant), the 26 standard
  components, and the host-extensible registry.
- `capability` — the descriptors each format advertises to an agent,
  including the **diagram DSL** descriptor (`DIAGRAM_DSL_NOTE` /
  `diagram_capability()`): the contract a model follows to *output a
  diagram* (a fenced ```` ```mermaid ````/```` ```structurizr ```` block,
  rendered by `rstui-ai::diagram`), folded into `render_capability_summary`.
- Depends on `rstui-core` + `rstui-widgets` + `rstui-ai` +
  `serde`/`serde_json`.

### 3. ACP-client integration (additive)

- Advertise renderable capability in the ACP `initialize`/session
  metadata: the A2UI `a2uiClientCapabilities` (`supportedCatalogIds`) and
  a json-render catalog id, so an agent knows it may stream rich UI.
- When a `session/update` content block carries an A2UI or json-render
  payload, parse it (`rstui-jsonui`) and fold it into the transcript as a
  rich entry rendered in `ui.rs`, alongside the existing markdown/tool
  cards. Plain text is unaffected.

## Consequences

- **Positive.** rstui becomes a first-class substrate for building TUI AI
  apps and for *being* the renderer an agent targets. Both crates are
  pure additive new files (conflict-free with concurrent streams); the
  ACP touch is small and additive. The pure-projection model extends to
  agent-driven UI for free — the data model is just more caller-owned
  state.
- **Negative / accepted.** `serde_json` enters the workspace's
  application-layer dependency set (already present transitively via the
  ACP client). The two new crates are `publish = false` until the API
  settles. A2UI/json-render are young specs; we pin v0.10 / current-main
  and treat unknown components as graceful placeholders (the formats'
  own progressive-rendering contract), so spec drift degrades instead of
  breaking. Web-only components are intentionally not ported.
- **Boundary.** No retained tree, no callbacks, totality on hostile
  input, `rstui-core` untouched — every ADR 0012/0002 invariant holds.
  Streaming markdown repair is strictly linear-time (no backtracking
  regex), matching streamdown's own ReDoS posture, because it runs on
  every token over a growing buffer.
