//! Reference plugin: a live session stopwatch + turn/prompt counters.
//!
//! A pi-`statusline`-style companion to the powerline footer: it tracks how
//! long the session has been running and how much you've exchanged, surfaced
//! as a live footer segment, a status key, and a `/session` summary panel.
//! Offline, std-only.

use std::time::{Duration, SystemTime};

use rstui_acp_client::plugin::protocol::{FooterSegment, HostEvent, PluginAction};
use rstui_acp_client::plugin::serve_auto;

struct State {
    start: SystemTime,
    prompts: u32,
    turns: u32,
}

impl State {
    fn elapsed(&self) -> Duration {
        self.start.elapsed().unwrap_or_default()
    }
}

fn mmss(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

fn footer(st: &State) -> PluginAction {
    PluginAction::Footer {
        segments: vec![
            FooterSegment {
                text: format!("⏱ {}", mmss(st.elapsed())),
                fg: Some("black".to_owned()),
                bg: Some("magenta".to_owned()),
            },
            FooterSegment {
                text: format!("✦ {}t {}p", st.turns, st.prompts),
                fg: Some("white".to_owned()),
                bg: Some("blue".to_owned()),
            },
        ],
    }
}

fn status(st: &State) -> PluginAction {
    PluginAction::SetStatus {
        key: "session".to_owned(),
        value: format!(
            "{} · {} turns · {} prompts",
            mmss(st.elapsed()),
            st.turns,
            st.prompts
        ),
    }
}

fn main() {
    let mut st = State {
        start: SystemTime::now(),
        prompts: 0,
        turns: 0,
    };
    let mut modal_id: u64 = 0;
    serve_auto(move |event, emit| match event {
        HostEvent::Init { .. } => {
            emit(PluginAction::RegisterCommand {
                name: "session".to_owned(),
                description: "Show this session's stopwatch & counters".to_owned(),
            });
            emit(footer(&st));
        }
        HostEvent::SessionStart { .. } => {
            st = State {
                start: SystemTime::now(),
                prompts: 0,
                turns: 0,
            };
            emit(footer(&st));
            emit(status(&st));
        }
        HostEvent::UserPrompt { .. } => {
            st.prompts += 1;
            emit(footer(&st));
            emit(status(&st));
        }
        HostEvent::TurnEnded { .. } => {
            st.turns += 1;
            emit(footer(&st));
            emit(status(&st));
        }
        HostEvent::Refresh => emit(footer(&st)),
        HostEvent::Command { name, .. } if name == "session" => {
            modal_id += 1;
            emit(PluginAction::Modal {
                id: modal_id,
                title: "Session".to_owned(),
                body: vec![
                    format!("elapsed   {}", mmss(st.elapsed())),
                    format!("turns     {}", st.turns),
                    format!("prompts   {}", st.prompts),
                ],
                buttons: vec!["Reset".to_owned(), "Close".to_owned()],
            });
        }
        HostEvent::ModalResponse {
            button, cancelled, ..
        } if !cancelled && button == "Reset" => {
            st = State {
                start: SystemTime::now(),
                prompts: 0,
                turns: 0,
            };
            emit(footer(&st));
            emit(status(&st));
            emit(PluginAction::Note {
                text: "session counters reset".to_owned(),
            });
        }
        _ => {}
    });
}
