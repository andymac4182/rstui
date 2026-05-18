//! `rstui-acp-plugin-sdk` — the Rust SDK for writing `rstui-acp-client`
//! plugins.
//!
//! Since ADR 0021 this is a **thin ACP layer over the app-agnostic
//! [`rstui_plugin_core`]**: that crate owns the JSON-RPC 2.0 envelope,
//! every transport, and the `Protocol`-generic serve loop; this crate
//! adds only the ACP vocabulary and ergonomics:
//!
//! - [`jsonrpc`]/[`transport`]/[`ws`] — re-exported from core (so paths
//!   like `rstui_acp_plugin_sdk::jsonrpc::Message` are unchanged).
//! - [`proto`] — the ACP plugin vocabulary ([`HostEvent`]/[`PluginAction`])
//!   mapped onto stable JSON-RPC methods.
//! - [`plugin`] — [`serve`] (closure) / [`Plugin`] + [`serve_plugin`]
//!   (structured), and [`AcpProtocol`]: the ACP binding of the core loop.
//!
//! Another application reuses the framework directly: depend on
//! `rstui-plugin-core`, define your own `Event`/`Action` + one
//! [`Protocol`], and call its `serve_auto`/… exactly as this crate does.
//!
//! A minimal ACP plugin:
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

pub mod plugin;
pub mod proto;

// The framework layer, re-exported so existing paths/imports are
// unchanged (`rstui_acp_plugin_sdk::jsonrpc::…`, `::Message`, the
// transports, `ShmChannel`) — ADR 0021 moved these to the core crate.
pub use rstui_plugin_core::{
    IoTransport, Kind, LpTransport, Message, Protocol, RpcError, ShmChannel, ShmTransport,
    StdioTransport, Transport, WsTransport, jsonrpc, transport, ws,
};

pub use plugin::{
    AcpProtocol, Host, Plugin, serve, serve_auto, serve_over, serve_plugin, serve_plugin_shm,
    serve_plugin_ws, serve_shm, serve_stdio_lp, serve_unix, serve_ws,
};
pub use proto::{
    API_VERSION, FooterSegment, HostEvent, PluginAction, decode_action, decode_event,
    encode_action, encode_event,
};
