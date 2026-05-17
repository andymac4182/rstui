//! The AI SDK message model — the single core type the chat widgets and
//! the declarative agent-UI engine are a projection of.
//!
//! # Why a hand-written, total parser
//!
//! The wire shape is the Vercel AI SDK v6 `UIMessage`: `{ id, role,
//! parts: [...] }` where each part is a discriminated union *with a
//! prefix discriminator* — `tool-<name>` and `data-<name>` are open
//! families, not closed enums, so a derived `#[serde(tag = "type")]`
//! cannot express it. More importantly, this message arrives **streamed**:
//! a part may be half-built, a `type` may be one a newer agent invented.
//! A renderer that errored on that would blank the transcript mid-turn.
//!
//! So [`UiPart`]/[`UiMessage`] parse from a [`serde_json::Value`] with a
//! *total* classifier ([`UiPart::from_value`]): every unknown or
//! malformed shape degrades to [`UiPart::Unknown`] (carrying the raw
//! value so a debug view can still show it) rather than failing. This is
//! the same progressive-rendering contract A2UI/json-render rely on, and
//! the totality rule every rstui widget already obeys. A [`serde`]
//! [`Deserialize`] shim delegates to the same classifier so
//! `serde_json::from_str::<UiMessage>(…)` also works.

use serde::de::{Deserialize, Deserializer};
use serde_json::Value;

/// Who authored a [`UiMessage`] (the AI SDK `role`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    /// A system / developer instruction turn.
    System,
    /// The end user.
    #[default]
    User,
    /// The agent's reply.
    Assistant,
}

impl Role {
    /// Parses the wire string; an unknown role degrades to
    /// [`Role::User`] (a renderer must still place the turn).
    #[must_use]
    pub fn from_wire(value: &str) -> Self {
        match value {
            "system" => Self::System,
            "assistant" => Self::Assistant,
            _ => Self::User,
        }
    }

    /// The wire string for this role.
    #[must_use]
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

/// Whether a text/reasoning part is still arriving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamState {
    /// The agent is still appending to this part.
    Streaming,
    /// The part is complete.
    #[default]
    Done,
}

impl StreamState {
    /// Parses the AI SDK `state` field (`"streaming"` → streaming, any
    /// other / absent → done).
    #[must_use]
    pub fn from_wire(value: Option<&str>) -> Self {
        match value {
            Some("streaming") => Self::Streaming,
            _ => Self::Done,
        }
    }
}

/// The lifecycle of a tool call (the ai-elements `ToolUIPart["state"]`
/// enum, verbatim — this is the authoritative seven-state contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolState {
    /// Arguments are still streaming in (`"input-streaming"`).
    #[default]
    InputStreaming,
    /// Arguments are complete, the tool is running (`"input-available"`).
    InputAvailable,
    /// The tool is paused for human approval (`"approval-requested"`).
    ApprovalRequested,
    /// The human answered the approval (`"approval-responded"`).
    ApprovalResponded,
    /// The tool finished with a result (`"output-available"`).
    OutputAvailable,
    /// The call was denied by the human (`"output-denied"`).
    OutputDenied,
    /// The tool errored (`"output-error"`).
    OutputError,
}

impl ToolState {
    /// Parses the wire string; an unknown state degrades to
    /// [`ToolState::InputStreaming`] (the safe "not done yet" default).
    #[must_use]
    pub fn from_wire(value: &str) -> Self {
        match value {
            "input-available" => Self::InputAvailable,
            "approval-requested" => Self::ApprovalRequested,
            "approval-responded" => Self::ApprovalResponded,
            "output-available" => Self::OutputAvailable,
            "output-denied" => Self::OutputDenied,
            "output-error" => Self::OutputError,
            _ => Self::InputStreaming,
        }
    }

    /// The short human label ai-elements shows in the status badge.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::InputStreaming => "Pending",
            Self::InputAvailable => "Running",
            Self::ApprovalRequested => "Awaiting Approval",
            Self::ApprovalResponded => "Responded",
            Self::OutputAvailable => "Completed",
            Self::OutputDenied => "Denied",
            Self::OutputError => "Error",
        }
    }

    /// `true` once the call has reached a final state (completed, denied,
    /// or errored) — a card may stop its spinner.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::OutputAvailable | Self::OutputDenied | Self::OutputError
        )
    }
}

/// A single tool call within an assistant turn (the ai-elements
/// `ToolUIPart`/`DynamicToolUIPart`, unified).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolUiPart {
    /// The tool name, already stripped of the `tool-` discriminator
    /// prefix (or the `dynamic-tool` `toolName`).
    pub tool_name: String,
    /// Stable id correlating the call with its result.
    pub tool_call_id: String,
    /// Where the call is in its lifecycle.
    pub state: ToolState,
    /// The arguments object, if present (rendered as pretty JSON).
    pub input: Option<Value>,
    /// The result, if present (object → JSON, string → text).
    pub output: Option<Value>,
    /// The error text when [`state`](Self::state) is
    /// [`ToolState::OutputError`].
    pub error_text: Option<String>,
}

/// One part of a [`UiMessage`] — the AI SDK discriminated union.
///
/// Parsing is **total**: any shape that does not match a known variant
/// becomes [`UiPart::Unknown`] (see the [module docs](self)).
#[derive(Debug, Clone, PartialEq)]
pub enum UiPart {
    /// Assistant/user prose (markdown). `state` distinguishes a
    /// still-streaming tail from a settled block.
    Text {
        /// The (possibly partial) markdown text.
        text: String,
        /// Whether more text is still arriving.
        state: StreamState,
    },
    /// The agent's chain-of-thought / "thinking" (rendered distinctly).
    Reasoning {
        /// The (possibly partial) reasoning text.
        text: String,
        /// Whether more reasoning is still arriving.
        state: StreamState,
    },
    /// A cited web source (`source-url`).
    SourceUrl {
        /// The agent's id for the source.
        source_id: String,
        /// The source URL.
        url: String,
        /// An optional display title.
        title: Option<String>,
    },
    /// A cited document source (`source-document`).
    SourceDocument {
        /// The agent's id for the source.
        source_id: String,
        /// The document MIME type.
        media_type: String,
        /// The document title.
        title: String,
        /// An optional file name.
        filename: Option<String>,
    },
    /// An attached / produced file (`file`).
    File {
        /// The file MIME type.
        media_type: String,
        /// An optional file name.
        filename: Option<String>,
        /// The file URL (often a `data:` URL).
        url: String,
    },
    /// A tool call (`tool-<name>` or `dynamic-tool`).
    Tool(ToolUiPart),
    /// A step boundary marker (`step-start`); carries no content.
    StepStart,
    /// An app-defined data part (`data-<name>`).
    Data {
        /// The data channel name (the part of `data-<name>` after the
        /// prefix).
        name: String,
        /// The opaque payload.
        value: Value,
    },
    /// Any unrecognised or malformed part — kept (not dropped) so a debug
    /// view can still surface it and so totality holds.
    Unknown(Value),
}

impl UiPart {
    /// Classifies one JSON part value. **Never panics, never errors**:
    /// an unknown `type`, a missing field, or a non-object input all map
    /// to [`UiPart::Unknown`].
    #[must_use]
    pub fn from_value(part: &Value) -> Self {
        let Some(kind) = part.get("type").and_then(Value::as_str) else {
            return Self::Unknown(part.clone());
        };
        let str_field = |key: &str| part.get(key).and_then(Value::as_str).map(ToOwned::to_owned);
        let state = || StreamState::from_wire(part.get("state").and_then(Value::as_str));

        if let Some(tool_name) = kind.strip_prefix("tool-") {
            return Self::Tool(Self::tool_from_value(tool_name.to_owned(), part));
        }
        if kind == "dynamic-tool" {
            let name = str_field("toolName").unwrap_or_default();
            return Self::Tool(Self::tool_from_value(name, part));
        }
        if let Some(channel) = kind.strip_prefix("data-") {
            return Self::Data {
                name: channel.to_owned(),
                value: part.get("data").cloned().unwrap_or(Value::Null),
            };
        }

        match kind {
            "text" => Self::Text {
                text: str_field("text").unwrap_or_default(),
                state: state(),
            },
            "reasoning" => Self::Reasoning {
                text: str_field("text").unwrap_or_default(),
                state: state(),
            },
            "source-url" => Self::SourceUrl {
                source_id: str_field("sourceId").unwrap_or_default(),
                url: str_field("url").unwrap_or_default(),
                title: str_field("title"),
            },
            "source-document" => Self::SourceDocument {
                source_id: str_field("sourceId").unwrap_or_default(),
                media_type: str_field("mediaType").unwrap_or_default(),
                title: str_field("title").unwrap_or_default(),
                filename: str_field("filename"),
            },
            "file" => Self::File {
                media_type: str_field("mediaType").unwrap_or_default(),
                filename: str_field("filename"),
                url: str_field("url").unwrap_or_default(),
            },
            "step-start" => Self::StepStart,
            _ => Self::Unknown(part.clone()),
        }
    }

    fn tool_from_value(tool_name: String, part: &Value) -> ToolUiPart {
        ToolUiPart {
            tool_name,
            tool_call_id: part
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            state: ToolState::from_wire(part.get("state").and_then(Value::as_str).unwrap_or("")),
            input: part.get("input").cloned(),
            output: part.get("output").cloned(),
            error_text: part
                .get("errorText")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        }
    }

    /// The plain text this part contributes to a flattened transcript
    /// (empty for non-text parts) — the ai-elements `getMessageText`
    /// helper.
    #[must_use]
    pub fn as_text(&self) -> &str {
        match self {
            Self::Text { text, .. } => text,
            _ => "",
        }
    }
}

/// One chat message: an id, a [`Role`], and the ordered [`UiPart`]s.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UiMessage {
    /// The message id (stable across streamed updates).
    pub id: String,
    /// Who authored it.
    pub role: Role,
    /// The ordered parts.
    pub parts: Vec<UiPart>,
    /// Opaque app metadata, passed through untouched.
    pub metadata: Option<Value>,
}

impl UiMessage {
    /// Builds a message from a JSON value. **Total** — a non-object, a
    /// missing `parts`, or malformed parts all yield a best-effort
    /// message (never an error), so a streamed/garbled message is always
    /// renderable.
    #[must_use]
    pub fn from_value(message: &Value) -> Self {
        let parts = message
            .get("parts")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(UiPart::from_value).collect())
            .unwrap_or_default();
        Self {
            id: message
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            role: Role::from_wire(message.get("role").and_then(Value::as_str).unwrap_or("")),
            parts,
            metadata: message.get("metadata").cloned(),
        }
    }

    /// Parses a message from a JSON string, or `None` if the string is
    /// not valid JSON (a non-object JSON value still parses, by the
    /// totality of [`from_value`](Self::from_value)).
    #[must_use]
    pub fn from_json_str(json: &str) -> Option<Self> {
        serde_json::from_str::<Value>(json)
            .ok()
            .map(|value| Self::from_value(&value))
    }

    /// The concatenated text of every [`UiPart::Text`] part — the
    /// ai-elements `getMessageText`, used for copy/markdown export.
    #[must_use]
    pub fn text(&self) -> String {
        self.parts.iter().map(UiPart::as_text).collect()
    }

    /// The tool-call parts, in order — what a tool-card list projects.
    pub fn tool_parts(&self) -> impl Iterator<Item = &ToolUiPart> {
        self.parts.iter().filter_map(|part| match part {
            UiPart::Tool(tool) => Some(tool),
            _ => None,
        })
    }
}

impl<'de> Deserialize<'de> for UiMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(Self::from_value(&value))
    }
}

/// Token accounting for a turn (the AI SDK `LanguageModelUsage`), what
/// [`context_meter`](crate::context_meter) projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenUsage {
    /// Prompt tokens.
    pub input_tokens: Option<u64>,
    /// Completion tokens.
    pub output_tokens: Option<u64>,
    /// Reasoning tokens (a subset of output for some models).
    pub reasoning_tokens: Option<u64>,
    /// Prompt tokens served from cache.
    pub cached_input_tokens: Option<u64>,
}

impl TokenUsage {
    /// Reads the four optional counters from a `LanguageModelUsage` JSON
    /// object (totally — missing/!number fields stay `None`).
    #[must_use]
    pub fn from_value(usage: &Value) -> Self {
        let count = |key: &str| usage.get(key).and_then(Value::as_u64);
        Self {
            input_tokens: count("inputTokens"),
            output_tokens: count("outputTokens"),
            reasoning_tokens: count("reasoningTokens"),
            cached_input_tokens: count("cachedInputTokens"),
        }
    }

    /// The total tokens attributable to this turn (input + output, the
    /// counters that are present), for a "used / max" meter.
    #[must_use]
    pub fn total(self) -> u64 {
        self.input_tokens.unwrap_or(0) + self.output_tokens.unwrap_or(0)
    }
}

/// The chat turn lifecycle the composer's submit/stop control projects
/// (the AI SDK `ChatStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChatStatus {
    /// Idle — ready for input; the control is "send".
    #[default]
    Ready,
    /// The prompt was submitted, awaiting the first token; "stop".
    Submitted,
    /// The agent is streaming a reply; "stop".
    Streaming,
    /// The turn ended in an error; the control shows the error affordance.
    Error,
}

impl ChatStatus {
    /// Parses the wire string; unknown degrades to [`ChatStatus::Ready`].
    #[must_use]
    pub fn from_wire(value: &str) -> Self {
        match value {
            "submitted" => Self::Submitted,
            "streaming" => Self::Streaming,
            "error" => Self::Error,
            _ => Self::Ready,
        }
    }

    /// `true` while a turn is in flight (submitted or streaming) — the
    /// composer shows a stop button and disables send.
    #[must_use]
    pub fn is_busy(self) -> bool {
        matches!(self, Self::Submitted | Self::Streaming)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_a_full_assistant_message() {
        let message = UiMessage::from_value(&json!({
            "id": "m1",
            "role": "assistant",
            "parts": [
                { "type": "text", "text": "Hello ", "state": "streaming" },
                { "type": "reasoning", "text": "thinking" },
                { "type": "tool-search", "toolCallId": "t1", "state": "output-available",
                  "input": { "q": "rust" }, "output": "ok" },
                { "type": "dynamic-tool", "toolName": "exec", "toolCallId": "t2",
                  "state": "input-available" },
                { "type": "source-url", "sourceId": "s1", "url": "https://e.com" },
                { "type": "step-start" },
                { "type": "data-custom", "data": { "k": 1 } }
            ]
        }));
        assert_eq!(message.role, Role::Assistant);
        assert_eq!(message.parts.len(), 7);
        assert_eq!(message.text(), "Hello ");
        assert!(matches!(
            message.parts[0],
            UiPart::Text {
                state: StreamState::Streaming,
                ..
            }
        ));
        let tools: Vec<_> = message.tool_parts().collect();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].tool_name, "search");
        assert_eq!(tools[0].state, ToolState::OutputAvailable);
        assert!(tools[0].state.is_terminal());
        assert_eq!(tools[1].tool_name, "exec");
        assert!(matches!(message.parts[5], UiPart::StepStart));
        assert!(matches!(message.parts[6], UiPart::Data { .. }));
    }

    #[test]
    fn totality_unknown_and_garbled_parts_never_panic() {
        let message = UiMessage::from_value(&json!({
            "role": "martian",
            "parts": [ { "type": "future-thing", "x": 1 }, 42, "nope", { "no": "type" } ]
        }));
        assert_eq!(message.role, Role::User); // unknown role → User
        assert_eq!(message.parts.len(), 4);
        assert!(
            message
                .parts
                .iter()
                .all(|p| matches!(p, UiPart::Unknown(_)))
        );
        // Non-object messages are still total.
        assert_eq!(UiMessage::from_value(&json!("x")), UiMessage::default());
        assert!(UiMessage::from_json_str("{ not json").is_none());
    }

    #[test]
    fn serde_shim_and_usage_and_status() {
        let message: UiMessage =
            serde_json::from_str(r#"{"id":"a","role":"user","parts":[]}"#).unwrap();
        assert_eq!(message.id, "a");
        let usage = TokenUsage::from_value(&json!({ "inputTokens": 10, "outputTokens": 5 }));
        assert_eq!(usage.total(), 15);
        assert!(ChatStatus::from_wire("streaming").is_busy());
        assert!(!ChatStatus::from_wire("ready").is_busy());
    }
}
