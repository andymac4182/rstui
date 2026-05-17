//! Reference plugin: a developer-fortune toast on every turn.
//!
//! The pi `pirate`/flavour-text idea: after each agent turn it surfaces a
//! rotating one-liner as a toast (and a `Fortune` panel), and `/fortune`
//! draws one on demand. Deterministic rotation — offline, std-only.

use rstui_acp_client::plugin::protocol::{HostEvent, PluginAction};
use rstui_acp_client::plugin::serve;

const FORTUNES: &[&str] = &[
    "Make it work, make it right, make it fast.",
    "Weeks of coding can save you hours of planning.",
    "There are two hard things: cache invalidation and naming.",
    "The best code is the code you didn't have to write.",
    "Premature optimization is the root of all evil.",
    "Programs must be written for people to read.",
    "Simplicity is prerequisite for reliability.",
    "First, solve the problem. Then, write the code.",
    "Deleted code is debugged code.",
    "A good agent reads the diff before it trusts the patch.",
    "Talk is cheap. Show me the tests.",
    "If it hurts, do it more often — automate the pain away.",
];

fn main() {
    let mut idx: usize = 0;
    serve(move |event, emit| match event {
        HostEvent::Init { .. } => {
            emit(PluginAction::RegisterCommand {
                name: "fortune".to_owned(),
                description: "Draw a developer fortune".to_owned(),
            });
            // Bind Ctrl+Y to draw a fortune without typing the command.
            emit(PluginAction::RegisterKeybinding {
                keys: "ctrl+y".to_owned(),
                command: "fortune".to_owned(),
                description: "Draw a fortune".to_owned(),
            });
        }
        HostEvent::TurnEnded { .. } => {
            let f = FORTUNES[idx % FORTUNES.len()];
            idx += 1;
            emit(PluginAction::Note {
                text: format!("🥠 {f}"),
            });
            emit(PluginAction::Panel {
                title: "Fortune".to_owned(),
                body: vec![f.to_owned()],
            });
        }
        HostEvent::Command { name, .. } if name == "fortune" => {
            let f = FORTUNES[idx % FORTUNES.len()];
            idx += 1;
            emit(PluginAction::Note {
                text: format!("🥠 {f}"),
            });
            emit(PluginAction::Panel {
                title: "Fortune".to_owned(),
                body: vec![f.to_owned()],
            });
        }
        _ => {}
    });
}
