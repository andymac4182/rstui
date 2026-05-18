//! `rstui-plugin-core` — an app-agnostic JSON-RPC 2.0 plugin framework
//! (ADR 0021).
//!
//! It owns the whole communications stack so an application only defines
//! its own vocabulary:
//!
//! - [`jsonrpc`] — a minimal JSON-RPC 2.0 envelope (the ACP/MCP wire).
//! - [`transport`] — framing: stdio, length-prefixed binary, Unix
//!   socket, shared memory ([`ShmTransport`]).
//! - [`ws`] — a dependency-free RFC 6455 WebSocket transport.
//! - [`host`] — the [`Protocol`] trait and a serve loop + every
//!   transport selector, **generic over the protocol**.
//!
//! `rstui-acp-plugin-sdk` is the ACP layer built on this; any other
//! application builds its own the same way — define `Event`/`Action`
//! and one `Protocol`, then reuse every transport and the loop:
//!
//! ```
//! use rstui_plugin_core::{Message, Protocol};
//!
//! enum Ev {
//!     Greet(String),
//!     Bye,
//! }
//! enum Act {
//!     Reply(String),
//! }
//!
//! struct MyApp;
//! impl Protocol for MyApp {
//!     type Event = Ev;
//!     type Action = Act;
//!     fn initialize_ack(&self) -> Option<serde_json::Value> {
//!         Some(serde_json::json!({ "ok": true }))
//!     }
//!     fn decode_event(&self, m: &Message) -> Option<Ev> {
//!         match m.method.as_deref()? {
//!             "greet" => Some(Ev::Greet(
//!                 m.params.as_ref()?.get("who")?.as_str()?.to_owned(),
//!             )),
//!             "bye" => Some(Ev::Bye),
//!             _ => None,
//!         }
//!     }
//!     fn encode_action(&self, a: &Act) -> Message {
//!         let Act::Reply(t) = a;
//!         Message::notification("reply", Some(serde_json::json!({ "text": t })))
//!     }
//!     fn is_shutdown(&self, e: &Ev) -> bool {
//!         matches!(e, Ev::Bye)
//!     }
//! }
//!
//! // Any transport works; `serve_auto(MyApp, |ev, emit| { … })` would
//! // pick stdio/uds/ws/shm from argv/env exactly like the ACP SDK.
//! fn handler(ev: Ev, emit: &mut dyn FnMut(Act)) {
//!     if let Ev::Greet(who) = ev {
//!         emit(Act::Reply(format!("hello, {who}")));
//!     }
//! }
//! let _ = (MyApp, handler as fn(Ev, &mut dyn FnMut(Act)));
//! ```

pub mod host;
pub mod jsonrpc;
pub mod transport;
pub mod ws;

pub use host::{
    Protocol, serve, serve_auto, serve_over, serve_shm, serve_stdio_lp, serve_unix, serve_ws,
};
pub use jsonrpc::{Kind, Message, RpcError};
pub use rstui_acp_shm::ShmChannel;
pub use transport::{IoTransport, LpTransport, ShmTransport, StdioTransport, Transport};
pub use ws::WsTransport;
