//! The ACP plugin-author surface: write handlers, the SDK does the rest.
//!
//! This is a **thin layer over [`rstui_plugin_core`]** (ADR 0021): the
//! JSON-RPC loop and every transport live in the app-agnostic core; here
//! we only bind the ACP vocabulary ([`HostEvent`]/[`PluginAction`]) via
//! [`AcpProtocol`] and keep the ergonomic [`serve`]/[`Plugin`]/[`Host`]
//! surface. Every signature is unchanged, so existing plugins and the
//! client are source-compatible.

use rstui_plugin_core::{Message, Protocol, Transport};

use crate::proto::{
    HostEvent, PluginAction, initialize_ack, message_to_host_event, plugin_action_to_message,
};

/// The ACP vocabulary as a [`rstui_plugin_core::Protocol`]: the one place
/// the generic loop is told how to read [`HostEvent`]s and write
/// [`PluginAction`]s. Other applications write their own equivalent.
#[derive(Clone, Copy, Default)]
pub struct AcpProtocol;

impl Protocol for AcpProtocol {
    type Event = HostEvent;
    type Action = PluginAction;

    fn initialize_ack(&self) -> Option<serde_json::Value> {
        Some(initialize_ack())
    }
    fn decode_event(&self, msg: &Message) -> Option<HostEvent> {
        message_to_host_event(msg)
    }
    fn encode_action(&self, action: &PluginAction) -> Message {
        plugin_action_to_message(action)
    }
    fn is_shutdown(&self, event: &HostEvent) -> bool {
        matches!(event, HostEvent::Shutdown)
    }
}

/// Runs a plugin over `transport` until end-of-stream or `Shutdown`.
///
/// The handler is called once per [`HostEvent`]; anything it passes to the
/// `emit` callback is sent back as a JSON-RPC notification. The
/// `initialize` request is answered automatically.
pub fn serve_over<T: Transport, F>(transport: T, handler: F)
where
    F: FnMut(HostEvent, &mut dyn FnMut(PluginAction)),
{
    rstui_plugin_core::serve_over(transport, AcpProtocol, handler);
}

/// Runs a plugin over the default stdio transport (the common case).
pub fn serve<F>(handler: F)
where
    F: FnMut(HostEvent, &mut dyn FnMut(PluginAction)),
{
    rstui_plugin_core::serve(AcpProtocol, handler);
}

/// Runs a plugin over stdio with **length-prefixed** framing (binary
/// `u32` length + JSON bytes) — no newline scan, exact reads.
pub fn serve_stdio_lp<F>(handler: F)
where
    F: FnMut(HostEvent, &mut dyn FnMut(PluginAction)),
{
    rstui_plugin_core::serve_stdio_lp(AcpProtocol, handler);
}

/// Runs a plugin as a **Unix-domain-socket** server. `lp` selects
/// length-prefixed framing; otherwise newline JSON. One client, then exit.
///
/// # Errors
///
/// Bind/accept failures (or never, on the non-Unix stdio fallback).
pub fn serve_unix<F>(path: &str, lp: bool, handler: F) -> std::io::Result<()>
where
    F: FnMut(HostEvent, &mut dyn FnMut(PluginAction)),
{
    rstui_plugin_core::serve_unix(path, lp, AcpProtocol, handler)
}

/// The transport-selecting entry the reference plugins use:
/// `--shm`/`--uds`/`--ws` (or `RSTUI_PLUGIN_SHM`/`_UDS`/`_WS`) pick the
/// transport, `--lp`/`RSTUI_PLUGIN_LP` the framing; otherwise stdio.
pub fn serve_auto<F>(handler: F)
where
    F: FnMut(HostEvent, &mut dyn FnMut(PluginAction)),
{
    rstui_plugin_core::serve_auto(AcpProtocol, handler);
}

/// Runs a plugin over a **shared-memory** channel (ADR 0016).
///
/// # Errors
///
/// Segment attach (`mmap` / semaphore) failure.
pub fn serve_shm<F>(path: &str, handler: F) -> std::io::Result<()>
where
    F: FnMut(HostEvent, &mut dyn FnMut(PluginAction)),
{
    rstui_plugin_core::serve_shm(path, AcpProtocol, handler)
}

/// [`serve_shm`] for a structured [`Plugin`].
///
/// # Errors
///
/// Segment attach failure.
pub fn serve_plugin_shm<P: Plugin>(path: &str, mut plugin: P) -> std::io::Result<()> {
    serve_shm(path, move |event, emit| {
        let mut host = Host { emit };
        dispatch(&mut plugin, event, &mut host);
    })
}

/// Runs a plugin as a WebSocket server: binds `addr`, accepts one client.
///
/// # Errors
///
/// Bind/accept/handshake failures.
pub fn serve_ws<F>(addr: impl std::net::ToSocketAddrs, handler: F) -> std::io::Result<()>
where
    F: FnMut(HostEvent, &mut dyn FnMut(PluginAction)),
{
    rstui_plugin_core::serve_ws(addr, AcpProtocol, handler)
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
    serve_ws(addr, move |event, emit| {
        let mut host = Host { emit };
        dispatch(&mut plugin, event, &mut host);
    })
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
