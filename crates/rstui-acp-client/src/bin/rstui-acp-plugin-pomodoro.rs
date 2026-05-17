//! Reference plugin: a Pomodoro focus timer.
//!
//! `/pomodoro [minutes]` starts a countdown shown live in the footer; it
//! fires a toast when the interval elapses. `/pomodoro stop` cancels it.
//! Drives entirely off the host's periodic `Refresh` + wall clock — offline,
//! std-only. A small, visually-interesting bit of session ritual.

use std::time::{Duration, SystemTime};

use rstui_acp_client::plugin::protocol::{FooterSegment, HostEvent, PluginAction};
use rstui_acp_client::plugin::serve_auto;

struct Timer {
    end: SystemTime,
    minutes: u64,
}

fn mmss(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn clear(emit: &mut dyn FnMut(PluginAction)) {
    emit(PluginAction::Footer {
        segments: Vec::new(),
    });
    emit(PluginAction::SetStatus {
        key: "pomodoro".to_owned(),
        value: String::new(),
    });
}

fn main() {
    let mut timer: Option<Timer> = None;
    serve_auto(move |event, emit| match event {
        HostEvent::Init { .. } => {
            emit(PluginAction::RegisterCommand {
                name: "pomodoro".to_owned(),
                description: "Focus timer: /pomodoro [minutes] | stop".to_owned(),
            });
        }
        HostEvent::Command { name, args } if name == "pomodoro" => {
            let a = args.trim();
            if a == "stop" || a == "cancel" {
                timer = None;
                clear(emit);
                emit(PluginAction::Note {
                    text: "pomodoro cancelled".to_owned(),
                });
            } else {
                let minutes = a.parse::<u64>().ok().filter(|m| *m > 0).unwrap_or(25);
                timer = Some(Timer {
                    end: SystemTime::now() + Duration::from_secs(minutes * 60),
                    minutes,
                });
                emit(PluginAction::Note {
                    text: format!("🍅 pomodoro started — {minutes} min, stay focused"),
                });
            }
        }
        HostEvent::Refresh => {
            if let Some(t) = &timer {
                match t.end.duration_since(SystemTime::now()) {
                    Ok(remaining) if !remaining.is_zero() => {
                        let low = remaining.as_secs() < 60;
                        emit(PluginAction::Footer {
                            segments: vec![FooterSegment {
                                text: format!("🍅 {}", mmss(remaining)),
                                fg: Some("black".to_owned()),
                                bg: Some(if low { "red" } else { "green" }.to_owned()),
                            }],
                        });
                        emit(PluginAction::SetStatus {
                            key: "pomodoro".to_owned(),
                            value: format!("{} left", mmss(remaining)),
                        });
                    }
                    _ => {
                        let m = t.minutes;
                        timer = None;
                        clear(emit);
                        emit(PluginAction::Note {
                            text: format!("🍅 pomodoro done after {m} min — take a break!"),
                        });
                    }
                }
            }
        }
        HostEvent::Shutdown if timer.is_some() => clear(emit),
        _ => {}
    });
}
