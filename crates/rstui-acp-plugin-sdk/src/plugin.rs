//! The plugin-author surface: write handlers, the SDK does the rest.
//!
//! [`serve`] is the ergonomic closure form (source-compatible with the
//! original protocol); [`Plugin`] + [`serve_plugin`] is the structured form.
//! Both own the JSON-RPC loop: handshake (`initialize` → ack), event
//! dispatch, and action framing over a [`Transport`].

use crate::jsonrpc::Kind;
use crate::proto::{
    HostEvent, PluginAction, initialize_ack, message_to_host_event, plugin_action_to_message,
};
use crate::transport::{StdioTransport, Transport};

/// Runs a plugin over `transport` until end-of-stream or `Shutdown`.
///
/// The handler is called once per [`HostEvent`]; anything it passes to the
/// `emit` callback is sent back to the host as a JSON-RPC notification. The
/// `initialize` request is answered automatically before its [`HostEvent::Init`]
/// is delivered.
pub fn serve_over<T: Transport, F>(mut transport: T, mut handler: F)
where
    F: FnMut(HostEvent, &mut dyn FnMut(PluginAction)),
{
    while let Ok(Some(msg)) = transport.recv() {
        if msg.kind() == Kind::Response {
            continue; // not addressed to a plugin
        }
        // Answer the JSON-RPC handshake before dispatching it.
        if msg.kind() == Kind::Request && msg.method.as_deref() == Some("initialize") {
            if let Some(id) = msg.id.clone() {
                let _ = transport.send(&crate::jsonrpc::Message::response(id, initialize_ack()));
            }
        }
        let Some(event) = message_to_host_event(&msg) else {
            continue;
        };
        let stop = matches!(event, HostEvent::Shutdown);

        let mut outbox: Vec<PluginAction> = Vec::new();
        {
            let mut emit = |a: PluginAction| outbox.push(a);
            handler(event, &mut emit);
        }
        for action in &outbox {
            if transport.send(&plugin_action_to_message(action)).is_err() {
                return;
            }
        }
        if stop {
            return;
        }
    }
}

/// Runs a plugin over the default stdio transport (the common case).
pub fn serve<F>(handler: F)
where
    F: FnMut(HostEvent, &mut dyn FnMut(PluginAction)),
{
    serve_over(StdioTransport::new(), handler);
}

/// Runs a plugin as a WebSocket server: binds `addr`, accepts one client,
/// and dispatches over a [`WsTransport`](crate::ws::WsTransport) — the same
/// JSON-RPC, a different transport (the goal's "stdio or websockets").
///
/// # Errors
///
/// Bind/accept/handshake failures.
pub fn serve_ws<F>(addr: impl std::net::ToSocketAddrs, handler: F) -> std::io::Result<()>
where
    F: FnMut(HostEvent, &mut dyn FnMut(PluginAction)),
{
    let transport = crate::ws::WsTransport::accept(addr)?;
    serve_over(transport, handler);
    Ok(())
}

/// [`serve_ws`] for a structured [`Plugin`].
///
/// # Errors
///
/// Bind/accept/handshake failures.
pub fn serve_plugin_ws<P: Plugin>(
    addr: impl std::net::ToSocketAddrs,
    mut plugin: P,
) -> std::io::Result<()> {
    let transport = crate::ws::WsTransport::accept(addr)?;
    serve_over(transport, move |event, emit| {
        let mut host = Host { emit };
        dispatch(&mut plugin, event, &mut host);
    });
    Ok(())
}

/// An ergonomic emit handle passed to [`Plugin`] callbacks.
pub struct Host<'a> {
    emit: &'a mut dyn FnMut(PluginAction),
}

impl Host<'_> {
    /// Set / clear (empty value) a status key.
    pub fn set_status(&mut self, key: impl Into<String>, value: impl Into<String>) {
        (self.emit)(PluginAction::SetStatus {
            key: key.into(),
            value: value.into(),
        });
    }
    /// Replace this plugin's footer segments.
    pub fn footer(&mut self, segments: Vec<crate::proto::FooterSegment>) {
        (self.emit)(PluginAction::Footer { segments });
    }
    /// Replace / clear (empty body) this plugin's sidebar panel.
    pub fn panel(&mut self, title: impl Into<String>, body: Vec<String>) {
        (self.emit)(PluginAction::Panel {
            title: title.into(),
            body,
        });
    }
    /// Post a toast.
    pub fn note(&mut self, text: impl Into<String>) {
        (self.emit)(PluginAction::Note { text: text.into() });
    }
    /// Append to the plugin log.
    pub fn log(&mut self, text: impl Into<String>) {
        (self.emit)(PluginAction::Log { text: text.into() });
    }
    /// Register a slash command.
    pub fn register_command(&mut self, name: impl Into<String>, description: impl Into<String>) {
        (self.emit)(PluginAction::RegisterCommand {
            name: name.into(),
            description: description.into(),
        });
    }
    /// Bind a key chord (e.g. `"ctrl+g"`) to one of this plugin's commands.
    pub fn register_keybinding(
        &mut self,
        keys: impl Into<String>,
        command: impl Into<String>,
        description: impl Into<String>,
    ) {
        (self.emit)(PluginAction::RegisterKeybinding {
            keys: keys.into(),
            command: command.into(),
            description: description.into(),
        });
    }
    /// Show a modal dialog; the choice returns via `on_modal_response`.
    pub fn modal(
        &mut self,
        id: u64,
        title: impl Into<String>,
        body: Vec<String>,
        buttons: Vec<String>,
    ) {
        (self.emit)(PluginAction::Modal {
            id,
            title: title.into(),
            body,
            buttons,
        });
    }
    /// Emit a raw action (escape hatch, e.g. `AskUser`).
    pub fn emit(&mut self, action: PluginAction) {
        (self.emit)(action);
    }
}

/// The structured plugin interface. Every method defaults to a no-op, so a
/// plugin overrides only what it needs.
#[allow(unused_variables)]
pub trait Plugin {
    /// Handshake — return any startup actions (e.g. register commands).
    fn initialize(&mut self, client: &str, cwd: &str, host: &mut Host<'_>) {}
    /// A session with an agent began.
    fn on_session_start(&mut self, agent: &str, host: &mut Host<'_>) {}
    /// The user submitted a prompt.
    fn on_prompt(&mut self, text: &str, host: &mut Host<'_>) {}
    /// The agent finished a turn.
    fn on_turn_ended(&mut self, stop_reason: &str, host: &mut Host<'_>) {}
    /// A registered slash command was invoked.
    fn on_command(&mut self, name: &str, args: &str, host: &mut Host<'_>) {}
    /// The user answered a prior `Modal`.
    fn on_modal_response(&mut self, id: u64, button: &str, cancelled: bool, host: &mut Host<'_>) {}
    /// The user answered a prior `AskUser`.
    fn on_ask_response(
        &mut self,
        id: u64,
        selections: &[String],
        text: &str,
        cancelled: bool,
        host: &mut Host<'_>,
    ) {
    }
    /// Periodic refresh tick.
    fn on_tick(&mut self, host: &mut Host<'_>) {}
    /// The client is shutting down.
    fn on_shutdown(&mut self, host: &mut Host<'_>) {}
}

/// Runs a [`Plugin`] over the default stdio transport.
pub fn serve_plugin<P: Plugin>(mut plugin: P) {
    serve(move |event, emit| {
        let mut host = Host { emit };
        dispatch(&mut plugin, event, &mut host);
    });
}

/// Routes one [`HostEvent`] to the matching [`Plugin`] callback. Shared by
/// every `serve_plugin*` entry so the transports stay interchangeable.
fn dispatch<P: Plugin>(plugin: &mut P, event: HostEvent, host: &mut Host<'_>) {
    match event {
        HostEvent::Init { client, cwd, .. } => plugin.initialize(&client, &cwd, host),
        HostEvent::SessionStart { agent } => plugin.on_session_start(&agent, host),
        HostEvent::UserPrompt { text } => plugin.on_prompt(&text, host),
        HostEvent::TurnEnded { stop_reason } => plugin.on_turn_ended(&stop_reason, host),
        HostEvent::Command { name, args } => plugin.on_command(&name, &args, host),
        HostEvent::ModalResponse {
            id,
            button,
            cancelled,
        } => plugin.on_modal_response(id, &button, cancelled, host),
        HostEvent::AskResponse {
            id,
            selections,
            text,
            cancelled,
        } => plugin.on_ask_response(id, &selections, &text, cancelled, host),
        HostEvent::Refresh => plugin.on_tick(host),
        HostEvent::Shutdown => plugin.on_shutdown(host),
    }
}
