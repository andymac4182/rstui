//! The plugin extension wire protocol: newline-delimited JSON over the
//! plugin's stdio.
//!
//! This is the chat-app *extension* vocabulary (footer segments, slash
//! commands, an ask-user overlay, status keys) that the security-focused
//! `rstui-plugin-host` hooks (`SessionStart` / `BeforeCapability` /
//! `SessionEnd`) deliberately do not model. It keeps that crate's
//! [ADR 0007](https://github.com/andymac4182/rstui/blob/main/docs/adr/0007-plugin-host-and-secure-execution.md)
//! posture — **separate process, deny-by-default** (a plugin runs only when
//! the operator passes `--plugin <cmd>`), strictly typed, and **fail-closed**
//! (an unparyseable line ends that plugin, never "skip and continue") — and
//! adds the UI surface, mirroring the pi extension model
//! (`ctx.ui.setStatus`, command registration, `ctx.ui.custom`).

use serde::{Deserialize, Serialize};

/// Current extension API version. A plugin may refuse a mismatch.
pub const API_VERSION: &str = "1";

/// Host → plugin: lifecycle + chat events the plugin reacts to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostEvent {
    /// First message after spawn; negotiates the API version.
    Init {
        /// [`API_VERSION`].
        api_version: String,
        /// The host program name.
        client: String,
        /// Absolute session working directory.
        cwd: String,
    },
    /// A session with an agent began.
    SessionStart {
        /// Registry id / launch command of the connected agent.
        agent: String,
    },
    /// The user submitted a prompt.
    UserPrompt {
        /// The prompt text.
        text: String,
    },
    /// The agent finished a turn.
    TurnEnded {
        /// ACP stop reason, debug-rendered.
        stop_reason: String,
    },
    /// The user invoked a slash command this plugin registered.
    Command {
        /// Command name without the leading slash.
        name: String,
        /// Everything after the command name (may be empty).
        args: String,
    },
    /// The host's answer to a prior [`PluginAction::AskUser`].
    AskResponse {
        /// Correlation id from the request.
        id: u64,
        /// Selected option labels (empty for a pure freeform answer).
        selections: Vec<String>,
        /// Freeform text (empty when only options were chosen).
        text: String,
        /// `true` if the user dismissed the overlay without answering.
        cancelled: bool,
    },
    /// Periodic nudge to refresh footer/status (driven by the UI tick).
    Refresh,
    /// The client is exiting; the plugin should flush and stop.
    Shutdown,
}

/// Plugin → host: UI contributions and requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginAction {
    /// Register a `/name` slash command (shown in help + the palette).
    RegisterCommand {
        /// Command name without the leading slash.
        name: String,
        /// One-line help text.
        description: String,
    },
    /// Set a named status value (the pi `ctx.ui.setStatus` analogue).
    SetStatus {
        /// Status key.
        key: String,
        /// Status value (empty string clears it).
        value: String,
    },
    /// Replace this plugin's footer segments (powerline-style).
    Footer {
        /// Left-to-right segments.
        segments: Vec<FooterSegment>,
    },
    /// Ask the user a structured question via a host overlay (the pi
    /// `pi-ask-user` analogue). The answer returns as
    /// [`HostEvent::AskResponse`].
    AskUser {
        /// Correlation id echoed back in the response.
        id: u64,
        /// The prompt.
        question: String,
        /// Optional context shown above the choices.
        #[serde(default)]
        context: String,
        /// Selectable options (may be empty for a pure freeform ask).
        #[serde(default)]
        options: Vec<String>,
        /// Allow a typed freeform answer in addition to / instead of options.
        #[serde(default)]
        allow_freeform: bool,
    },
    /// Contribute a named panel to the TUI sidebar (the opencode slot
    /// analogue: arbitrary plugin-owned content). Sending it again with the
    /// same `title` replaces that panel; an empty `body` removes it.
    Panel {
        /// Panel heading.
        title: String,
        /// Panel lines (rendered verbatim, wrapped).
        body: Vec<String>,
    },
    /// Post a transient note / toast (the pi side-note analogue).
    Note {
        /// Note body.
        text: String,
    },
    /// Append a diagnostic line to the client's plugin log.
    Log {
        /// Log line.
        text: String,
    },
}

/// One powerline footer cell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FooterSegment {
    /// Segment text (callers include their own padding/glyphs).
    pub text: String,
    /// Foreground colour name (`red`, `green`, `yellow`, `blue`, `magenta`,
    /// `cyan`, `white`, `gray`, `black`) or `None` for the default.
    #[serde(default)]
    pub fg: Option<String>,
    /// Background colour name (same palette) or `None`.
    #[serde(default)]
    pub bg: Option<String>,
}

/// Serializes `event` as one protocol line (newline included).
#[must_use]
pub fn encode_event(event: &HostEvent) -> String {
    let mut line = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_owned());
    line.push('\n');
    line
}

/// Serializes `action` as one protocol line (newline included).
#[must_use]
pub fn encode_action(action: &PluginAction) -> String {
    let mut line = serde_json::to_string(action).unwrap_or_else(|_| "{}".to_owned());
    line.push('\n');
    line
}

/// Parses one protocol line into a [`PluginAction`].
///
/// # Errors
///
/// Returns the `serde_json` message; the host treats any parse error as
/// fail-closed (it stops reading that plugin).
pub fn decode_action(line: &str) -> Result<PluginAction, String> {
    serde_json::from_str(line).map_err(|e| e.to_string())
}

/// Parses one protocol line into a [`HostEvent`] (plugin side).
///
/// # Errors
///
/// Returns the `serde_json` message.
pub fn decode_event(line: &str) -> Result<HostEvent, String> {
    serde_json::from_str(line).map_err(|e| e.to_string())
}
