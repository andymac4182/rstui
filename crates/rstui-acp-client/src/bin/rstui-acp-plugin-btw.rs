//! Reference plugin: `/btw` side-notes (the `pi-btw` analogue).
//!
//! `pi-btw` opens a parallel side conversation kept *out of the agent's
//! context*. This port keeps that core idea on the extension protocol: `/btw
//! <note>` records a timestamped side note the host shows (and counts in a
//! status key) without it ever entering the agent prompt stream.

use std::time::{SystemTime, UNIX_EPOCH};

use rstui_acp_client::plugin::protocol::{HostEvent, PluginAction};
use rstui_acp_client::plugin::serve;

fn stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let s = secs % 86_400;
    format!("{:02}:{:02}", s / 3600, (s % 3600) / 60)
}

fn main() {
    let mut notes: Vec<String> = Vec::new();
    serve(move |event, emit| match event {
        HostEvent::Init { .. } => {
            emit(PluginAction::RegisterCommand {
                name: "btw".to_owned(),
                description: "record a side note, kept out of the agent's context".to_owned(),
            });
            emit(PluginAction::Log {
                text: "btw side-channel ready (/btw <note>)".to_owned(),
            });
        }
        HostEvent::Command { name, args } if name == "btw" => {
            let note = args.trim();
            if note.is_empty() {
                emit(PluginAction::Note {
                    text: "usage: /btw <something to remember>".to_owned(),
                });
                return;
            }
            notes.push(format!("[{}] {note}", stamp()));
            emit(PluginAction::Note {
                text: format!("noted privately: {note}"),
            });
            emit(PluginAction::SetStatus {
                key: "btw".to_owned(),
                value: format!("{} note(s)", notes.len()),
            });
            emit(PluginAction::Log {
                text: format!("btw[{}] {note}", notes.len()),
            });
        }
        HostEvent::Shutdown if !notes.is_empty() => {
            emit(PluginAction::Log {
                text: format!("btw session notes:\n{}", notes.join("\n")),
            });
        }
        _ => {}
    });
}
