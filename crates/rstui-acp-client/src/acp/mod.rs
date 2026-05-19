//! The ACP transport: a tokio task owning the agent child process and the
//! `sacp` JSON-RPC connection, bridged to the synchronous rstui reducer.
//!
//! # Why a separate task + channels
//!
//! `sacp` is async and streaming; the rstui reducer is synchronous and
//! deterministic (ADR 0011 keeps the reducer `await`-free). The seam between
//! them is two channels:
//!
//! - app → driver: a tokio [`UnboundedSender`](tokio::sync::mpsc::UnboundedSender)
//!   of [`DriverCmd`] (prompts, cancel, permission answers, shutdown).
//! - driver → app: a `std::sync::mpsc` of [`AcpEvent`]. The reducer drains it
//!   with a re-armed `Cmd::perform` (the runtime runs that blocking `recv` on
//!   `spawn_blocking`, so it never stalls the loop) — the idiomatic Elm
//!   "subscription" over this runtime.
//!
//! A prompt is dispatched on its own `tokio::spawn` (the connection is
//! `Clone`) so the command loop stays free to answer the agent's
//! `session/request_permission` *while a turn is still streaming* — without
//! that, the agent would block on a permission the user cannot yet answer.

mod driver;
mod events;
mod richui;
mod wire;

pub use driver::spawn_driver;
pub use events::{
    AcpEvent, AuthOption, DriverCmd, DriverHandle, ModeOption, ModelOption, PermissionChoice,
    PermissionOption, TodoEntry, TodoStatus, ToolBody, ToolCallInfo, ToolCallPatch, ToolKind,
    ToolStatus, WireDir,
};
pub use richui::{
    MessageSegment, RichAction, RichDoc, RichUiFormat, RichUiPayload, click as rich_click,
    render_capability_meta, render_source as render_rich_ui, segments as message_segments,
    split_message as split_rich_ui,
};
