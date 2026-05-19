//! The message vocabulary crossing the app ↔ driver seam.

use std::sync::{Arc, Mutex};

use super::richui::RichUiPayload;

/// One streamed datum from the agent, folded into the transcript by the
/// reducer.
#[derive(Debug, Clone)]
pub enum AcpEvent {
    /// `initialize` succeeded; carries the agent's self-reported info.
    Connected(String),
    /// A session was created (`session/new`) — its id, so the client can
    /// remember it for `/resume`.
    SessionStarted(String),
    /// The agent rejected `session/new` and advertises auth methods: the
    /// user must sign in (ACP `authenticate`) before a session can start.
    AuthRequired(Vec<AuthOption>),
    /// A human-readable connection/lifecycle status line.
    Status(String),
    /// A chunk of assistant message text (appended to the open turn).
    AgentText(String),
    /// A chunk of agent "thinking" (rendered dim, separate from the answer).
    Thought(String),
    /// The agent sent a declarative UI document (A2UI / json-render)
    /// instead of prose; the reducer folds it into the transcript as a
    /// rendered rich entry (ADR 0017).
    RichUi(RichUiPayload),
    /// A new tool call the agent initiated (ACP `tool_call`).
    ToolCall(ToolCallInfo),
    /// A progress/result update to an existing tool call
    /// (ACP `tool_call_update`); only changed fields are present.
    ToolCallUpdate(ToolCallPatch),
    /// The agent's execution plan (ACP `plan`) — the full todo list, which
    /// the client replaces wholesale on each update (per the ACP contract).
    Plan(Vec<TodoEntry>),
    /// The agent's advertised slash commands (`available_commands_update`):
    /// `(name, description)` pairs, surfaced in the autocomplete + help.
    AvailableCommands(Vec<(String, String)>),
    /// The agent's selectable models (from `NewSessionResponse.models`):
    /// the current model id and the catalogue, surfaced by `/model`.
    Models {
        /// The currently-active model id.
        current: String,
        /// The models the agent offers.
        available: Vec<ModelOption>,
    },
    /// The agent confirmed a `session/set_model`; the new current model id.
    ModelSelected(String),
    /// The agent's session modes (from `NewSessionResponse.modes`): the
    /// current mode id and the catalogue, surfaced by `/mode`. This is how
    /// Codex's plan/approval modes reach a generic ACP client.
    Modes {
        /// The currently-active mode id.
        current: String,
        /// The modes the agent offers.
        available: Vec<ModeOption>,
    },
    /// The session mode changed (ACP `current_mode_update`, or our own
    /// `session/set_mode` ack): the new current mode id.
    ModeChanged(String),
    /// A context-window usage update (ACP `usage_update`): tokens currently
    /// in context and the total window size, surfaced in `/status`.
    Usage {
        /// Tokens currently in the context window.
        used: u64,
        /// Total context-window size in tokens (`0` if unknown).
        size: u64,
    },
    /// The current turn finished; carries the ACP stop reason, debug-rendered.
    TurnEnded(String),
    /// The agent wants authorization. Single-flight: the driver waits for the
    /// matching [`DriverCmd::Permission`] before answering the agent.
    Permission {
        /// Correlation id minted by the driver.
        id: u64,
        /// What the agent is asking to do (tool title / summary).
        title: String,
        /// The choices the agent offered.
        options: Vec<PermissionOption>,
    },
    /// A line the agent wrote to its stderr (diagnostics, shown in the log).
    Stderr(String),
    /// A transport/protocol error; the session is no longer usable.
    Error(String),
    /// The agent process exited / the connection closed.
    Disconnected(String),
}

/// The ACP tool category, used to pick an icon/label (`ToolKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// Reading files or data.
    Read,
    /// Modifying files/content.
    Edit,
    /// Removing files/data.
    Delete,
    /// Moving or renaming.
    Move,
    /// Searching for information.
    Search,
    /// Running commands/code.
    Execute,
    /// Internal reasoning/planning.
    Think,
    /// Retrieving external data.
    Fetch,
    /// Switching the session mode.
    SwitchMode,
    /// Anything else (default).
    Other,
}

impl ToolKind {
    /// Parses the ACP `kind` string.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "read" => Self::Read,
            "edit" => Self::Edit,
            "delete" => Self::Delete,
            "move" => Self::Move,
            "search" => Self::Search,
            "execute" => Self::Execute,
            "think" => Self::Think,
            "fetch" => Self::Fetch,
            "switch_mode" => Self::SwitchMode,
            _ => Self::Other,
        }
    }

    /// A short human label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
            Self::Delete => "delete",
            Self::Move => "move",
            Self::Search => "search",
            Self::Execute => "execute",
            Self::Think => "think",
            Self::Fetch => "fetch",
            Self::SwitchMode => "mode",
            Self::Other => "tool",
        }
    }
}

/// Execution status of a tool call (ACP `ToolCallStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    /// Not started (input streaming / awaiting approval).
    Pending,
    /// Currently running.
    InProgress,
    /// Finished successfully.
    Completed,
    /// Failed with an error.
    Failed,
}

impl ToolStatus {
    /// Parses the ACP `status` string.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "in_progress" => Self::InProgress,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// One renderable piece of a tool call's output.
#[derive(Debug, Clone)]
pub enum ToolBody {
    /// Plain text / content block.
    Text(String),
    /// A file modification rendered as a unified diff.
    Diff {
        /// Affected path.
        path: String,
        /// Pre-rendered unified-ish diff text.
        text: String,
    },
}

/// A new tool call (ACP `tool_call`).
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    /// Stable id within the session.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Category (icon/label).
    pub kind: ToolKind,
    /// Execution status.
    pub status: ToolStatus,
    /// Compact `[k=v, …]` summary of `rawInput`.
    pub input: String,
    /// Output content / diffs.
    pub body: Vec<ToolBody>,
}

/// A partial update to an existing tool call (ACP `tool_call_update`):
/// only the fields the agent changed are `Some`.
#[derive(Debug, Clone)]
pub struct ToolCallPatch {
    /// The tool call being updated.
    pub id: String,
    /// New title, if changed.
    pub title: Option<String>,
    /// New kind, if changed.
    pub kind: Option<ToolKind>,
    /// New status, if changed.
    pub status: Option<ToolStatus>,
    /// New input summary, if `rawInput` changed.
    pub input: Option<String>,
    /// New body, if `content` changed (collections are replaced, not merged).
    pub body: Option<Vec<ToolBody>>,
}

/// Execution status of a single ACP plan entry (todo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    /// Not started yet.
    Pending,
    /// Currently being worked on.
    InProgress,
    /// Finished.
    Completed,
}

impl TodoStatus {
    /// Parses the ACP `status` string (`pending`/`in_progress`/`completed`).
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "in_progress" => Self::InProgress,
            "completed" => Self::Completed,
            _ => Self::Pending,
        }
    }
}

/// One entry in the agent's execution plan (an ACP `PlanEntry`).
#[derive(Debug, Clone)]
pub struct TodoEntry {
    /// Human-readable description of the task.
    pub content: String,
    /// Current execution status.
    pub status: TodoStatus,
    /// Relative priority (`high`/`medium`/`low`); empty if unspecified.
    pub priority: String,
}

/// One agent-advertised model (ACP `ModelInfo`), shown in the `/model`
/// picker.
#[derive(Debug, Clone)]
pub struct ModelOption {
    /// Opaque model id, echoed back in `session/set_model`.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Optional one-line description (empty if the agent gave none).
    pub description: String,
}

/// One agent auth method (ACP `AuthMethod`), shown in the sign-in picker.
#[derive(Debug, Clone)]
pub struct AuthOption {
    /// Opaque method id, echoed back in `authenticate`.
    pub id: String,
    /// Human-readable name (e.g. "Sign in with ChatGPT").
    pub name: String,
    /// Optional one-line description (empty if the agent gave none).
    pub description: String,
}

/// One agent-advertised session mode (ACP `SessionMode`), shown in the
/// `/mode` picker.
#[derive(Debug, Clone)]
pub struct ModeOption {
    /// Opaque mode id, echoed back in `session/set_mode`.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Optional one-line description (empty if the agent gave none).
    pub description: String,
}

/// One selectable answer to a [`AcpEvent::Permission`] request.
#[derive(Debug, Clone)]
pub struct PermissionOption {
    /// Opaque id echoed back to the agent when chosen.
    pub option_id: String,
    /// Human label shown in the modal.
    pub label: String,
}

/// The user's answer to a permission request.
#[derive(Debug, Clone)]
pub enum PermissionChoice {
    /// Approve with the given option id.
    Selected(String),
    /// Decline / dismiss.
    Cancelled,
}

/// A command from the reducer to the driver task.
#[derive(Debug, Clone)]
pub enum DriverCmd {
    /// Send a user prompt as a new turn.
    Prompt(String),
    /// Cancel the in-flight turn (best-effort `session/cancel`).
    Cancel,
    /// Switch the session model (`session/set_model`) to this model id.
    SetModel(String),
    /// Switch the session mode (`session/set_mode`) to this mode id.
    SetMode(String),
    /// Resume a prior session (`session/load`) by its id.
    LoadSession(String),
    /// Authenticate with the chosen auth method id (ACP `authenticate`),
    /// then the driver retries `session/new`.
    Authenticate(String),
    /// Answer a pending [`AcpEvent::Permission`].
    Permission {
        /// The id from the request being answered.
        id: u64,
        /// The user's choice.
        choice: PermissionChoice,
    },
    /// Tear down the connection and stop the driver.
    Shutdown,
}

/// The reducer-side handle to a running driver.
///
/// `events` is an `Arc<Mutex<Receiver>>` because `Cmd::perform` takes a
/// `FnOnce` that the runtime may run on any blocking thread; the mutex makes
/// the single consumer movable into each re-armed drain closure.
#[derive(Clone)]
pub struct DriverHandle {
    /// app → driver commands.
    pub cmd_tx: tokio::sync::mpsc::UnboundedSender<DriverCmd>,
    /// driver → app events, drained by a re-armed `Cmd::perform`.
    pub events: Arc<Mutex<std::sync::mpsc::Receiver<AcpEvent>>>,
}

impl DriverHandle {
    /// Blocks until the next driver event (or `None` if the driver is gone).
    ///
    /// Called only from inside a `Cmd::perform` closure, which the async
    /// runtime runs on `spawn_blocking` — so this blocking `recv` is correct
    /// and never stalls the event loop.
    #[must_use]
    pub fn recv_blocking(&self) -> Option<AcpEvent> {
        let rx = self.events.lock().ok()?;
        rx.recv().ok()
    }

    /// Sends a command to the driver, ignoring send errors (a dead driver
    /// simply means the command is dropped; the UI surfaces disconnection
    /// through [`AcpEvent::Disconnected`]).
    pub fn send(&self, cmd: DriverCmd) {
        let _ = self.cmd_tx.send(cmd);
    }
}
