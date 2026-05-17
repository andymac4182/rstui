//! `rstui-ai` — the AI-application widget set for the rstui TUI framework.
//!
//! This crate is to *AI apps* what `rstui-widgets` is to general TUIs: a
//! vocabulary of pure-projection widgets (ADR 0012,
//! [`docs/composition.md`](https://github.com/andymac4182/rstui/blob/main/docs/composition.md))
//! for building an agent chat client — and a shared message model the
//! widgets and the declarative-UI engine ([`rstui-jsonui`]) both speak.
//! It depends only on the dependency-free core and the concrete widget
//! set (ADR 0002), plus `serde_json` for the JSON-shaped AI message model
//! (ADR 0017).
//!
//! Every widget here follows the exact discipline `rstui-widgets`
//! follows — a pure projection of caller-owned state, drawn through the
//! public `Buffer` contract, headless snapshot-tested — so this crate
//! doubles as the worked reference for building AI chrome on rstui.
//!
//! - [`model`]: the AI SDK message model — [`UiMessage`](model::UiMessage),
//!   the [`UiPart`](model::UiPart) discriminated union, the
//!   [`ToolUiPart`](model::ToolUiPart) call state machine,
//!   [`Role`](model::Role), [`TokenUsage`](model::TokenUsage),
//!   [`ChatStatus`](model::ChatStatus). The single core type the chat
//!   widgets and the agent-driven UI are a projection of; total,
//!   serde-deserializable from the wire shape, tolerant of partial /
//!   unknown parts (a streamed message is always renderable).
//! - [`stream_markdown`]: a streaming-markdown view — incomplete-markdown
//!   *repair* (the streamdown `remend` fixed-priority handler pipeline) +
//!   block segmentation + a per-block render cache, projected through
//!   `rstui_widgets::Markdown`/`Mermaid`. Streaming is a new behavior over
//!   the existing parser (the ADR 0002 §4 precedent), strictly
//!   linear-time (no backtracking) because it runs on every token.
//!
//! The remaining modules are the ai-elements vocabulary, one widget per
//! module: the disclosure family ([`reasoning`], [`tool`], [`task`],
//! [`plan`], [`chain_of_thought`]), the transcript ([`conversation`],
//! [`message`]), the composer ([`prompt_input`]), and the supporting
//! cards/chips ([`sources`], [`inline_citation`], [`shimmer`],
//! [`snippet`], [`artifact`], [`agent_card`], [`confirmation`],
//! [`terminal_view`], [`stack_trace`], [`context_meter`],
//! [`model_selector`], [`checkpoint`], [`commit`], [`test_results`],
//! [`package_info`], [`env_vars`], [`file_tree`], [`schema_view`],
//! [`web_console`], [`suggestion`]). Each is a pure projection; user
//! interaction surfaces as pure hit-test accessors and reducer-consumed
//! intents, never callbacks (ADR 0012 §P1).
//!
//! [`rstui-jsonui`]: https://github.com/andymac4182/rstui/tree/main/crates/rstui-jsonui

pub mod model;
pub mod stream_markdown;

pub mod agent_card;
pub mod artifact;
pub mod chain_of_thought;
pub mod checkpoint;
pub mod commit;
pub mod confirmation;
pub mod context_meter;
pub mod conversation;
pub mod env_vars;
pub mod file_tree;
pub mod inline_citation;
pub mod message;
pub mod model_selector;
pub mod package_info;
pub mod plan;
pub mod prompt_input;
pub mod reasoning;
pub mod schema_view;
pub mod shimmer;
pub mod snippet;
pub mod sources;
pub mod stack_trace;
pub mod suggestion;
pub mod task;
pub mod terminal_view;
pub mod test_results;
pub mod tool;
pub mod web_console;
