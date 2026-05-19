# Agent-driven UI: A2UI + json-render

An agent emits a JSON UI document; rstui renders it in the terminal. Two
live formats are supported, both projected through the rstui widget set,
and the ACP client tells the agent it can render them. This is the *what*;
[ADR 0017](adr/0017-ai-app-widgets-and-declarative-agent-ui.md) is the
*why*.

## The two crates

| Crate | Responsibility |
|-------|----------------|
| [`rstui-ai`](#rstui-ai-the-ai-app-widget-set) | The AI-app widget vocabulary: the AI-SDK message model, a streaming-markdown view, and the ai-elements widget set (tool cards, reasoning, conversation, prompt composer, …) |
| [`rstui-jsonui`](#rstui-jsonui-the-declarative-engine) | The declarative engine: parses **A2UI** and **json-render** documents and projects them to one `UiNode` tree rendered through `rstui-widgets`/`rstui-ai` |

Both are pure additive crates (`publish = false` while the API settles)
and obey every framework invariant — immediate-mode, no retained tree,
total on hostile/truncated input (ADR 0012/0017).

## `rstui-jsonui` — the declarative engine

One projection target, no retained tree. Each format parses to its own
document, then **projects** to a single `tree::UiNode` re-walked every
frame against a caller-owned `value::DataModel` — an agent UI is just
more caller-owned state in the existing pure-projection model. User
interaction surfaces as a pure `tree::HitMap` accessor, never a callback.

| Module | What it is |
|--------|------------|
| `value` | RFC-6901 JSON-Pointer data store (`get`/`set`/`remove`, relative-scope) both formats bind against |
| `tree` | `UiNode` (the projection target) + the `render` walker + the `HitMap` hit accessor |
| `a2ui` | Google **A2UI v0.10**: the six-message envelope, the 18-component basic catalog, the `Dynamic*`/`formatString` binding resolver, `ChildList` templating, the action return channel |
| `jsonrender` | Vercel **json-render**: the flat `{root,elements,state}` map, the twelve-step `$`-expression resolver, the eight directives, the RFC-6902 patch-stream compiler, the standard component set |
| `capability` | The descriptors the client sends an agent so it targets exactly what this terminal renders (below) |

### Sending the catalog to the agent

Both formats require the client to *send the catalog*, not merely a
name:

- **A2UI** — `capability::client_capabilities()` advertises the canonical
  basic-catalog id **and ships the full self-contained inline catalog**
  (every component's JSON Schema + the 14 catalog functions + the theme,
  cross-file `$ref`s localized) in
  `a2uiClientCapabilities.v0.10.inlineCatalogs`. The schema is the
  vendored canonical upstream (`crates/rstui-jsonui/assets/a2ui/`).
- **json-render** — the catalog is generated from one declarative
  `declare_json_render_catalog!` table (`capability::json_render_catalog()`
  + `json_render_prompt()`); a CI drift-guard test locks the macro table
  to the renderer's actual coverage so they cannot diverge.

## `rstui-ai` — the AI-app widget set

To AI apps what `rstui-widgets` is to general TUIs:

- `model` — the AI-SDK `UiMessage`/`UiPart`/`ToolUiPart` model (total,
  serde-tolerant of partial/unknown streamed parts).
- `stream_markdown` — a streaming-markdown view: a port of streamdown's
  `remend` incomplete-markdown repair + block cache over the existing
  `rstui_widgets::Markdown`/`Mermaid` (linear-time, runs every token).
- The ai-elements vocabulary, one widget per module — the `Tool`
  keystone (the seven-state call card), `Reasoning`, `Conversation`,
  `Message`, `PromptInput`, plus the supporting cards/chips — all pure
  projections with hit-test accessors and reducer-consumed intents.

## In the ACP client

`rstui-acp-client` advertises the catalog and renders agent-sent UI:

- The ACP `initialize` client-capabilities `_meta` carries
  `a2uiClientCapabilities` (with the inline catalog), the json-render
  catalog + authoring prompt, and the Mermaid/Structurizr diagram DSL —
  so the agent knows it may reply with rich UI, and exactly which
  components, props and events are available.
- `acp::richui` conservatively detects a self-contained A2UI envelope /
  json-render spec / fenced block in an agent message (total — a partial
  streamed chunk stays prose), folds it into the transcript as a
  `Role::RichUi` entry, and the view re-projects it through
  `rstui-jsonui` every frame.
- The **`/render`** slash command makes this work with **any** agent,
  not just one that reads the `initialize` `_meta`: it sends the
  json-render authoring instructions + the component catalog (the same
  `rstui_jsonui::capability::json_render_prompt()` the client
  advertises) as a normal prompt turn, so a generic LLM agent learns
  the format from the conversation. `/render <request>` also appends a
  concrete task (e.g. `/render a sales dashboard`) — the agent replies
  with a ` ```json-render ` document the client renders live.

See [ACP client](acp-client.md) for the full client.

## Try it: the kitchen-sink Agent UI screens

The kitchen sink has an **Agent UI** rail section with two screens —
**A2UI** and **json-render**. The agent's document is an editable
[`Editor`](widgets/core-set.md) code buffer on the left; its **live**
`rstui-jsonui` projection is on the right. Type to edit the JSON and the
right pane re-renders every frame; `PgUp`/`PgDn` switch the worked
examples (edits persist per example).

```sh
cargo run -p rstui-kitchen-sink          # then ':a2ui' or ':json-render'
```

It is headless-tested end to end in
`crates/rstui-kitchen-sink/tests/experiences.rs` (the split renders, the
data-binding resolves, a render-only glyph proves the right pane is a
live projection not a source echo, and editing the buffer re-projects to
the engine's placeholder) and gated by the real-binary
`vhs/e2e/agent-ui.{tape,expect}` (`cargo xtask record e2e --check`).
