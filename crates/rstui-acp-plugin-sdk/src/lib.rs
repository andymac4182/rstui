//! `rstui-acp-plugin-sdk` — the Rust SDK for writing `rstui-acp-client`
//! plugins.
//!
//! It owns the whole communications stack so a plugin only writes handlers:
//!
//! - [`jsonrpc`] — a minimal JSON-RPC 2.0 envelope (the ACP/MCP wire).
//! - [`proto`] — the plugin vocabulary ([`HostEvent`]/[`PluginAction`])
//!   mapped onto stable JSON-RPC methods.
//! - [`transport`] — framing: [`StdioTransport`](transport::StdioTransport)
//!   today; a websocket transport is additive (same [`Message`]).
//! - [`plugin`] — [`serve`] (closure) and [`Plugin`]/[`serve_plugin`]
//!   (structured): the JSON-RPC loop, handshake, and dispatch.
//!
//! A minimal plugin:
//!
//! ```no_run
//! use rstui_acp_plugin_sdk::{serve, HostEvent, PluginAction};
//!
//! serve(|event, emit| {
//!     if let HostEvent::Init { .. } = event {
//!         emit(PluginAction::RegisterCommand {
//!             name: "hi".into(),
//!             description: "say hi".into(),
//!         });
//!     }
//! });
//! ```

pub mod jsonrpc;
pub mod plugin;
pub mod proto;
pub mod transport;
pub mod ws;

pub use jsonrpc::{Kind, Message, RpcError};
pub use plugin::{
    Host, Plugin, serve, serve_auto, serve_over, serve_plugin, serve_plugin_ws, serve_ws,
};
pub use proto::{
    API_VERSION, FooterSegment, HostEvent, PluginAction, decode_action, decode_event,
    encode_action, encode_event,
};
pub use ws::WsTransport;
