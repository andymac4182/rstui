//! `rstui-jsonui` — a declarative agent-driven UI engine for rstui.
//!
//! An agent emits a JSON UI document; this crate renders it through the
//! rstui widget set. Two live formats are supported, both compiling to
//! one projection target:
//!
//! - [`a2ui`]: Google **A2UI** v0.10 — the six-message surface protocol,
//!   the 18-component basic catalog, JSON-Pointer data binding with the
//!   `Dynamic*`/`formatString` resolver, `ChildList` templating, the
//!   action return channel, and capability negotiation.
//! - [`jsonrender`]: Vercel **json-render** — the flat
//!   `{root, elements, state}` element map, the twelve-step `$`-expression
//!   prop resolver, the eight directives, the RFC-6902 patch stream
//!   compiler, and the host-extensible component registry.
//!
//! # The one projection target, no retained tree
//!
//! Both formats parse to their own document and then **project** to a
//! single [`tree::UiNode`] — a resolved, concrete, immediate-mode
//! renderable that maps to `rstui-widgets`/`rstui-ai` draw calls. There
//! is no retained widget tree (ADR 0012): the parsed document plus the
//! caller-owned [`value::DataModel`] is re-projected and re-walked every
//! frame, so an agent UI is just more caller-owned state in the existing
//! pure-projection model. User interaction surfaces as a pure
//! [`tree::HitMap`] accessor and a reducer-consumed intent the format turns
//! back into the agent's action/event JSON — never a callback (ADR 0012
//! §P1).
//!
//! - [`value`]: the RFC-6901 JSON-Pointer data store (`get`/`set`/
//!   `remove`, relative-scope resolution) both formats bind against.
//! - [`tree`]: [`UiNode`](tree::UiNode), the projection target, plus the
//!   `render` walker and the `hit` accessor.
//! - [`capability`]: the descriptors each format advertises to an agent
//!   (the A2UI `a2uiClientCapabilities`, the json-render catalog) so the
//!   agent knows what this client can render.
//!
//! Parsing and rendering are **total**: hostile, truncated, or
//! unknown-component JSON degrades to a placeholder (the formats' own
//! progressive-rendering contract), never a panic or a blanked screen.

pub mod capability;
pub mod tree;
pub mod value;

pub mod a2ui;
pub mod jsonrender;
