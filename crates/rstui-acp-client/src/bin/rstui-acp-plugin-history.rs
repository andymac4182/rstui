//! Reference plugin: an automatic prompt-history panel.
//!
//! Unlike `btw` (manual private notes), this passively records *every* user
//! prompt into a live sidebar panel — a session scrollback you can re-read
//! without scrolling the transcript (the pi `studio`/recall idea). Offline,
//! std-only. `/history` echoes it, `/history clear` wipes it.

use std::time::{SystemTime, UNIX_EPOCH};

use rstui_acp_client::plugin::protocol::{HostEvent, PluginAction};
use rstui_acp_client::plugin::serve;

fn stamp() -> String {
    let s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        % 86_400;
    format!("{:02}:{:02}", s / 3600, (s % 3600) / 60)
}

fn one_line(text: &str, max: usize) -> String {
    let flat = text.replace('\n', " ");
    if flat.chars().count() > max {
        format!("{}…", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

fn panel(hist: &[String]) -> PluginAction {
    PluginAction::Panel {
        title: "Prompt history".to_owned(),
        body: if hist.is_empty() {
            Vec::new()
        } else {
            hist.iter().rev().take(20).cloned().collect()
        },
    }
}

fn main() {
    let mut hist: Vec<String> = Vec::new();
    serve(move |event, emit| match event {
        HostEvent::Init { .. } => {
            emit(PluginAction::RegisterCommand {
                name: "history".to_owned(),
                description: "Recent prompts (\"/history clear\" to wipe)".to_owned(),
            });
        }
        HostEvent::UserPrompt { text } => {
            // Skip slash commands — they aren't conversational prompts.
            if text.starts_with('/') {
                return;
            }
            hist.push(format!("[{}] {}", stamp(), one_line(&text, 60)));
            emit(PluginAction::SetStatus {
                key: "history".to_owned(),
                value: format!("{} prompts", hist.len()),
            });
            emit(panel(&hist));
        }
        HostEvent::Command { name, args } if name == "history" => {
            if args.trim() == "clear" {
                hist.clear();
                emit(PluginAction::SetStatus {
                    key: "history".to_owned(),
                    value: String::new(),
                });
                emit(panel(&hist));
                emit(PluginAction::Note {
                    text: "prompt history cleared".to_owned(),
                });
            } else if hist.is_empty() {
                emit(PluginAction::Note {
                    text: "no prompts yet".to_owned(),
                });
            } else {
                emit(PluginAction::Note {
                    text: format!("{} prompts recorded (see the sidebar)", hist.len()),
                });
                emit(panel(&hist));
            }
        }
        _ => {}
    });
}
