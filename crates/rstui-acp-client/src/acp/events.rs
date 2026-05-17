//! The message vocabulary crossing the app ↔ driver seam.

use std::sync::{Arc, Mutex};

/// One streamed datum from the agent, folded into the transcript by the
/// reducer.
#[derive(Debug, Clone)]
pub enum AcpEvent {
    /// `initialize` succeeded; carries the agent's self-reported info.
    Connected(String),
    /// A human-readable connection/lifecycle status line.
    Status(String),
    /// A chunk of assistant message text (appended to the open turn).
    AgentText(String),
    /// A chunk of agent "thinking" (rendered dim, separate from the answer).
    Thought(String),
    /// A tool call the agent initiated (title / one-line summary).
    ToolCall(String),
    /// A plan/`todo` update line from the agent.
    Plan(String),
    /// The agent's advertised slash commands (`available_commands_update`):
    /// `(name, description)` pairs, surfaced in the autocomplete + help.
    AvailableCommands(Vec<(String, String)>),
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
