//! The rstui-acp-client plugin vocabulary, carried as JSON-RPC 2.0.
//!
//! [`HostEvent`] / [`PluginAction`] are the *semantic* payloads (unchanged
//! from the original bespoke protocol, so the app and existing plugins are
//! source-compatible). Each maps to a stable JSON-RPC `method` with the
//! typed payload as `params` — ACP/MCP-style. `initialize` is a real
//! request/response handshake; everything else is a notification.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::jsonrpc::Message;

/// Current extension API version. A plugin may refuse a mismatch.
pub const API_VERSION: &str = "1";

/// Host → plugin: lifecycle + chat events the plugin reacts to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostEvent {
    /// First message after spawn; negotiates the API version. Sent as a
    /// JSON-RPC **request** (`initialize`); the SDK auto-acknowledges.
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
    /// The user dismissed/answered a prior [`PluginAction::Modal`].
    ModalResponse {
        /// Correlation id from the request.
        id: u64,
        /// The chosen button label (empty if cancelled).
        button: String,
        /// `true` if dismissed without choosing a button.
        cancelled: bool,
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
    /// Ask the user a structured question via a host overlay. The answer
    /// returns as [`HostEvent::AskResponse`].
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
    /// Bind a key chord to one of this plugin's registered commands
    /// (opencode keymap-layer analogue). `keys` is a canonical chord like
    /// `"ctrl+g"`, `"alt+s"`, `"f5"` (modifiers in `ctrl+alt+shift+super`
    /// order, lowercase key). The host invokes `command` when it is pressed
    /// and nothing else is consuming input.
    RegisterKeybinding {
        /// Canonical chord string.
        keys: String,
        /// A command name this plugin registered.
        command: String,
        /// One-line help (shown in `/help`).
        description: String,
    },
    /// Show a modal dialog (opencode `Dialog`/`Confirm`/`Select`, pi
    /// `ctx.ui.custom`): a title, body lines, and a row of buttons. The
    /// choice returns as [`HostEvent::ModalResponse`].
    Modal {
        /// Correlation id echoed back in the response.
        id: u64,
        /// Modal title.
        title: String,
        /// Body lines (rendered wrapped).
        #[serde(default)]
        body: Vec<String>,
        /// Button labels, left to right (defaults to `["OK"]` if empty).
        #[serde(default)]
        buttons: Vec<String>,
    },
    /// Contribute a named panel to the TUI sidebar. Re-sending the same
    /// `title` replaces it; an empty `body` removes it.
    Panel {
        /// Panel heading.
        title: String,
        /// Panel lines (rendered verbatim, wrapped).
        body: Vec<String>,
    },
    /// Post a transient note / toast.
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

// ---- JSON-RPC method names (host→plugin) ----

/// The JSON-RPC `method` for a host→plugin event.
#[must_use]
pub fn host_method(event: &HostEvent) -> &'static str {
    match event {
        HostEvent::Init { .. } => "initialize",
        HostEvent::SessionStart { .. } => "session/start",
        HostEvent::UserPrompt { .. } => "session/prompt",
        HostEvent::TurnEnded { .. } => "session/turnEnded",
        HostEvent::Command { .. } => "command/invoke",
        HostEvent::ModalResponse { .. } => "modal/response",
        HostEvent::AskResponse { .. } => "askUser/response",
        HostEvent::Refresh => "tick",
        HostEvent::Shutdown => "shutdown",
    }
}

/// The JSON-RPC `method` for a plugin→host action.
#[must_use]
pub fn plugin_method(action: &PluginAction) -> &'static str {
    match action {
        PluginAction::RegisterCommand { .. } => "commands/register",
        PluginAction::SetStatus { .. } => "ui/setStatus",
        PluginAction::Footer { .. } => "ui/footer",
        PluginAction::AskUser { .. } => "ui/askUser",
        PluginAction::RegisterKeybinding { .. } => "ui/registerKeybinding",
        PluginAction::Modal { .. } => "ui/modal",
        PluginAction::Panel { .. } => "ui/panel",
        PluginAction::Note { .. } => "ui/note",
        PluginAction::Log { .. } => "ui/log",
    }
}

/// Wraps a host→plugin event in a JSON-RPC [`Message`]. `Init` is a request
/// (carrying `id`); every other event is a notification.
#[must_use]
pub fn host_event_to_message(event: &HostEvent, id: u64) -> Message {
    let method = host_method(event);
    let params = serde_json::to_value(event).ok();
    if matches!(event, HostEvent::Init { .. }) {
        Message::request(json!(id), method, params)
    } else {
        Message::notification(method, params)
    }
}

/// Recovers a [`HostEvent`] from a JSON-RPC message (the typed payload lives
/// in `params`; `method` is the routing key).
#[must_use]
pub fn message_to_host_event(msg: &Message) -> Option<HostEvent> {
    let params = msg.params.clone()?;
    serde_json::from_value(params).ok()
}

/// Wraps a plugin→host action as a JSON-RPC notification.
#[must_use]
pub fn plugin_action_to_message(action: &PluginAction) -> Message {
    Message::notification(plugin_method(action), serde_json::to_value(action).ok())
}

/// Recovers a [`PluginAction`] from a JSON-RPC message, or `None` if the
/// message is a response/unknown (not an action).
#[must_use]
pub fn message_to_plugin_action(msg: &Message) -> Option<PluginAction> {
    msg.method.as_ref()?; // a response (no method) is not an action
    serde_json::from_value(msg.params.clone()?).ok()
}

// ---- line codecs (compat names; now JSON-RPC framed) ----

/// Serializes a host event as one JSON-RPC line (newline included).
#[must_use]
pub fn encode_event(event: &HostEvent) -> String {
    host_event_to_message(event, 1).encode_line()
}

/// Serializes a plugin action as one JSON-RPC line (newline included).
#[must_use]
pub fn encode_action(action: &PluginAction) -> String {
    plugin_action_to_message(action).encode_line()
}

/// Parses one JSON-RPC line into a [`PluginAction`].
///
/// # Errors
///
/// Errors if the line is not valid JSON-RPC or does not carry an action
/// (e.g. it is the `initialize` response).
pub fn decode_action(line: &str) -> Result<PluginAction, String> {
    let msg = Message::decode_line(line)?;
    message_to_plugin_action(&msg).ok_or_else(|| "not a plugin action".to_owned())
}

/// Parses one JSON-RPC line into a [`HostEvent`].
///
/// # Errors
///
/// Errors if the line is not valid JSON-RPC or does not carry a host event.
pub fn decode_event(line: &str) -> Result<HostEvent, String> {
    let msg = Message::decode_line(line)?;
    message_to_host_event(&msg).ok_or_else(|| "not a host event".to_owned())
}

/// The JSON-RPC `result` the SDK returns for the `initialize` request.
#[must_use]
pub fn initialize_ack() -> Value {
    json!({ "ok": true, "apiVersion": API_VERSION })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrpc::Kind;

    #[test]
    fn init_is_a_jsonrpc_request_others_are_notifications() {
        let init = HostEvent::Init {
            api_version: API_VERSION.to_owned(),
            client: "rstui-acp-client".to_owned(),
            cwd: "/tmp".to_owned(),
        };
        let m = host_event_to_message(&init, 7);
        assert_eq!(m.kind(), Kind::Request);
        assert_eq!(m.method.as_deref(), Some("initialize"));
        assert_eq!(m.id, Some(json!(7)));

        let m2 = host_event_to_message(&HostEvent::Refresh, 7);
        assert_eq!(m2.kind(), Kind::Notification);
        assert_eq!(m2.method.as_deref(), Some("tick"));
    }

    #[test]
    fn host_event_round_trips_through_jsonrpc() {
        let ev = HostEvent::Command {
            name: "git".to_owned(),
            args: "status".to_owned(),
        };
        let line = host_event_to_message(&ev, 1).encode_line();
        let back = decode_event(line.trim()).expect("decodes");
        match back {
            HostEvent::Command { name, args } => {
                assert_eq!((name.as_str(), args.as_str()), ("git", "status"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn plugin_action_round_trips_and_methods_are_stable() {
        let a = PluginAction::SetStatus {
            key: "git".to_owned(),
            value: "main".to_owned(),
        };
        let msg = plugin_action_to_message(&a);
        assert_eq!(msg.method.as_deref(), Some("ui/setStatus"));
        let back = decode_action(msg.encode_line().trim()).expect("action");
        assert!(matches!(back, PluginAction::SetStatus { .. }));
    }

    #[test]
    fn a_response_is_not_decoded_as_an_action() {
        let resp = Message::response(json!(1), initialize_ack());
        assert!(decode_action(resp.encode_line().trim()).is_err());
        assert!(message_to_plugin_action(&resp).is_none());
    }
}
