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
//! - [`model`]: the AI SDK message model — [`UiMessage`],
//!   the [`UiPart`] discriminated union, the
//!   [`ToolUiPart`] call state machine,
//!   [`Role`], [`TokenUsage`],
//!   [`ChatStatus`]. The single core type the chat
//!   widgets and the agent-driven UI are a projection of; total,
//!   serde-deserializable from the wire shape, tolerant of partial /
//!   unknown parts (a streamed message is always renderable).
//! - [`stream_markdown`]: a streaming-markdown view — incomplete-markdown
//!   *repair* (the streamdown `remend` fixed-priority handler pipeline) +
//!   block segmentation + a per-block render cache, projected through
//!   `rstui_widgets::Markdown`/`Mermaid`. Streaming is a new behavior over
//!   the existing parser (the ADR 0002 §4 precedent), strictly
//!   linear-time (no backtracking) because it runs on every token.
//! - [`diagram`]: the diagram DSL an AI tool *outputs* — a pure projection
//!   that unwraps a fenced ```` ```mermaid ````/```` ```structurizr ````
//!   block (or [`Diagram::extract`](diagram::Diagram::extract)s the first
//!   one from a chat turn) and delegates to the deterministic, total
//!   `rstui_widgets::Mermaid`/`Structurizr`. The contract is advertised to
//!   the agent via `rstui_jsonui::capability`.
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
pub mod conversation_cache;
pub mod diagram;
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

// Crate-root re-exports of the public surface, the `rstui-widgets`
// convention (the widget *type* is promoted; helper free functions stay
// module-scoped, reached via `module::helper`).
pub use model::{
    ChatStatus, Role, StreamState, TokenUsage, ToolState, ToolUiPart, UiMessage, UiPart,
};
pub use stream_markdown::{
    LinkMode, RemendHandler, RemendOptions, StreamCache, StreamMarkdown, StreamMarkdownState,
};

pub use agent_card::{AgentCard, AgentDef, AgentTool};
pub use artifact::{Artifact, ArtifactAction, ArtifactIntent};
pub use chain_of_thought::{ChainOfThought, ChainStep, ChainStepStatus};
pub use checkpoint::Checkpoint;
pub use commit::{Commit, CommitFile, CommitInfo, FileStatus};
pub use confirmation::{Confirmation, ConfirmationIntent};
pub use context_meter::ContextMeter;
pub use conversation::Conversation;
pub use conversation_cache::ConversationCache;
pub use env_vars::{EnvVars, EnvVarsIntent};
pub use file_tree::{FileNode, FileTree};
pub use inline_citation::{InlineCitation, InlineCitationCard};
pub use message::{Message, MessageBranch, MessageBranchState};
pub use model_selector::{Model, ModelSelector, ModelSelectorIntent};
pub use package_info::{ChangeType, Package, PackageInfo};
pub use plan::Plan;
pub use prompt_input::{Attachment, PromptInput, PromptInputIntent};
pub use reasoning::Reasoning;
pub use schema_view::{SchemaNode, SchemaView};
pub use shimmer::Shimmer;
pub use snippet::{Snippet, SnippetIntent};
pub use sources::Sources;
pub use stack_trace::{ParsedStackTrace, StackFrame, StackTrace, StackTraceIntent};
pub use suggestion::{SuggestionIntent, Suggestions};
pub use task::{Task, TaskItem};
pub use terminal_view::TerminalView;
pub use test_results::{Summary, TestCase, TestResults, TestStatus, TestSuite};
pub use tool::Tool;
pub use web_console::{ConsoleLevel, ConsoleLog, WebConsole};
