//! Reference plugin: `ask_user` structured prompts (the `pi-ask-user`
//! analogue).
//!
//! `pi-ask-user` registers an `ask_user` tool that renders a custom
//! interactive overlay (`ctx.ui.custom`) for single/multi-select + freeform
//! answers. This port registers an `/ask` command that triggers the host's
//! ask-user overlay ([`PluginAction::AskUser`]) and reports the structured
//! [`HostEvent::AskResponse`] back as a note.

use rstui_acp_client::plugin::protocol::{HostEvent, PluginAction};
use rstui_acp_client::plugin::serve;

fn main() {
    let mut next_id: u64 = 1;
    serve(move |event, emit| match event {
        HostEvent::Init { .. } => {
            emit(PluginAction::RegisterCommand {
                name: "ask".to_owned(),
                description: "ask yourself a structured question (ask_user overlay)".to_owned(),
            });
        }
        HostEvent::Command { name, args } if name == "ask" => {
            let question = if args.trim().is_empty() {
                "How should we proceed?".to_owned()
            } else {
                args.trim().to_owned()
            };
            let id = next_id;
            next_id += 1;
            emit(PluginAction::AskUser {
                id,
                question,
                context: "Answer routes back to the ask-user plugin, not the agent.".to_owned(),
                options: vec![
                    "Yes, continue".to_owned(),
                    "No, stop".to_owned(),
                    "Let me explain (freeform)".to_owned(),
                ],
                allow_freeform: true,
            });
        }
        HostEvent::AskResponse {
            selections,
            text,
            cancelled,
            ..
        } => {
            if cancelled {
                emit(PluginAction::Note {
                    text: "ask-user: cancelled".to_owned(),
                });
                return;
            }
            let mut parts: Vec<String> = Vec::new();
            if !selections.is_empty() {
                parts.push(format!("chose: {}", selections.join(", ")));
            }
            if !text.is_empty() {
                parts.push(format!("said: {text}"));
            }
            if parts.is_empty() {
                parts.push("no answer".to_owned());
            }
            emit(PluginAction::Note {
                text: format!("ask-user → {}", parts.join(" · ")),
            });
        }
        _ => {}
    });
}
