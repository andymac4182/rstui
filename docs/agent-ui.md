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
- At turn end `acp::richui::segments` splits the **assembled** agent
  message into ordered pieces — markdown prose interleaved with **every**
  embedded fenced block — so the message renders like markdown *and* each
  ` ```json-render ` / ` ```a2ui ` / ` ```mermaid ` / ` ```structurizr ` /
  ` ```canvas ` block becomes an inline `Role::RichUi` entry (re-projected
  every frame through `rstui-jsonui` or the same `rstui-widgets` diagram
  widgets the kitchen-sink Rich Text screen uses). Per-chunk detection
  cannot see a streamed, prose-wrapped block (each chunk is an incomplete
  fragment) — splitting the assembled message at turn end is what makes a
  real agent's reply actually render. Total: a reply with no block stays
  ordinary markdown.
- Rendered blocks are **interactive**, not display-only. A mouse click
  on a rendered control is mapped back to its node by re-deriving the
  exact geometry the renderer drew (`acp::richui::click` /
  `ui::rich_hit` — pure, no stored layout). Each interactive
  `Role::RichUi` entry owns a **stateful** `acp::richui::RichDoc` (a
  live `A2uiSurface` / `JsonRenderDoc`, keyed by `Entry::rich`): a
  click `act`s on that owned doc, so a two-way control — a toggled
  checkbox, a switched tab, a json-render `setState` — **mutates the
  owned model and persists** across the every-frame re-projection,
  while a server `event` / `openUrl` round-trips to the agent (a new
  prompt turn) or opens the URL. The doc map is bounded by the live
  transcript (`cap_transcript` evicts dropped ids), so this stays
  within the pure-projection model — the owned state *is* the
  caller-owned state the projection reads, never a retained UI tree.
- An interactive doc **opens in a live pane on the right**, next to the
  chat (the shared pure `ui::body_split` — the pane takes priority over
  the read-only sidebar; it needs a wide enough terminal to keep a
  usable chat column). `Tab` moves keyboard focus from the composer
  into the pane; `Tab`/`⇧Tab`/`↑`/`↓` walk the focus ring (the
  `HitMap::entries()` draw order), a printable char / `Backspace` edits
  the focused text field with **spec-correct two-way write-back** (A2UI
  `text_binding` → the bound `{path}`; json-render's projected field id
  *is* the `$bindState` pointer), `Enter`/`Space` activates, `Esc`
  returns to the composer. A mouse click in the pane does the same. So
  a **form with a submit button** gathers the typed values and sends
  the exact spec envelope: A2UI `{version,action:{name,surfaceId,
  sourceComponentId,timestamp,context}}` with `context` resolved from
  the now-updated model; json-render a host `{action,params}` with
  params resolved from state. The loop **closes**: an agent follow-up
  for the same surface (an A2UI `updateDataModel`/`updateComponents`/
  `actionResponse` with no `createSurface`) folds into the open live
  doc (`RichDoc::merge_followup`) so the pane updates in place rather
  than stacking a duplicate.
- **Every form element is supported and submits**, in *both* formats,
  and each is advertised in the catalog/prompt the agent receives:
  - **json-render**: `Button` (`on.press` → a builtin runs locally,
    else a host `{action,params}` round-trips to the agent),
    `TextInput`, `Checkbox`/`Switch`/`Toggle` (two-way `$bindState`
    bool), `Slider`/`Range` (a `[−] value [+]` stepper, two-way
    `$bindState`, clamped `min..=max`), `Select`/`MultiSelect`,
    `ConfirmInput`, `Tabs`. (`Button`/`Checkbox`/`Slider` were
    advertised-but-unimplemented → a dead `[unsupported]`; now real.)
  - **A2UI**: `Button` (its `action.event` → the spec
    `{version,action:{…}}` envelope), `TextField`, `CheckBox`,
    `ChoicePicker`, `Slider` (was a read-only Gauge; now the same
    interactive two-way stepper), `DateTimeInput`, `Tabs`.
  Inputs write to the data model; a `Button`'s event/host-action is
  the submit, with the bound values resolved into its
  `context`/`params` — so a full form round-trips to the agent.
- **Charts/graphs are first-class** in both formats. `BarChart`,
  `LineChart`, `AreaChart`, `PieChart`, `Sparkline`, `ScatterPlot`,
  `Histogram`, `StackedBarChart`, `Heatmap` project to a real themed
  `UiNode::Chart` (backed 1:1 by the `rstui-widgets` chart suite) via
  the shared format-agnostic `rstui_jsonui::chart::build_chart`, so an
  A2UI surface and a json-render spec draw identical graphs from
  `data:[{label,value}]` or `series:[{name,color?,points:[[x,y]]}]`.
- **Colours are theme tokens.** A component/series `"color"` is a
  semantic token (`accent`/`success`/`warning`/`danger`/`info`/`muted`),
  a chart series (`chart1`…`chart5`, auto-cycled), `bullish`/`bearish`,
  or a raw `#rrggbb`/named fallback — resolved against the active theme
  via `rstui_jsonui::color` (`Palette`/`parse_token`). The ACP client
  maps its live theme (the dedicated `chart_*` tokens) into the doc and
  re-skins it on a theme change; `rstui-jsonui` itself stays
  theme-system-agnostic (the dep-free `Palette::ANSI` default).
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
