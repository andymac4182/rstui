//! The deny-by-default plugin extension layer.
//!
//! The wire vocabulary + transports + the plugin author SDK now live in the
//! standalone [`rstui_acp_plugin_sdk`] crate (JSON-RPC 2.0, ACP/MCP-style,
//! transport-agnostic). This module is the *in-app* side: [`host`] spawns
//! plugin processes, speaks JSON-RPC to them, and merges their UI actions
//! into the reducer. The SDK names are re-exported here (and as
//! [`protocol`]) so the app, the reference plugins, and the tests use one
//! shared definition.

pub mod host;

/// The plugin wire vocabulary (compat path: `crate::plugin::protocol::*`).
pub use rstui_acp_plugin_sdk::proto as protocol;
pub use rstui_acp_plugin_sdk::{
    API_VERSION, FooterSegment, HostEvent, PluginAction, serve, serve_auto,
};

pub use host::{PluginEvent, PluginHost};
