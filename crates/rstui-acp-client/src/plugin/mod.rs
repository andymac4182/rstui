//! The deny-by-default plugin extension layer.
//!
//! [`protocol`] is the newline-JSON wire vocabulary; [`host`] is the in-app
//! side (spawn, broadcast, merge); reference plugins live in `src/bin/` and
//! use [`host::serve`]. See [`protocol`] for how this complements — rather
//! than replaces — `rstui-plugin-host`'s ADR 0007 security hooks.

pub mod host;
pub mod protocol;

pub use host::{PluginEvent, PluginHost, serve};
pub use protocol::{API_VERSION, FooterSegment, HostEvent, PluginAction};
