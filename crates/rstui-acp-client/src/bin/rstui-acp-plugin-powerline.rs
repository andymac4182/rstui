//! Reference plugin: a powerline-style footer (the `pi-powerline-footer`
//! analogue) over this client's extension protocol.
//!
//! It contributes themed footer segments — agent, working directory, a
//! rotating "working vibe", a prompt counter, and a UTC clock — refreshed on
//! the host's periodic [`HostEvent::Refresh`], exactly the
//! `ctx.ui.setStatus`/footer model pi plugins use, but as an ADR 0007
//! separate process speaking newline JSON.

use std::time::{SystemTime, UNIX_EPOCH};

use rstui_acp_client::plugin::protocol::{FooterSegment, HostEvent, PluginAction};
use rstui_acp_client::plugin::serve_auto;

#[derive(Default)]
struct State {
    agent: String,
    cwd: String,
    prompts: u32,
    vibe: usize,
}

const VIBES: [&str; 6] = [
    "engage ⚡",
    "make it so ✦",
    "warp 9 ➤",
    "aye captain ⌁",
    "scanning… ◎",
    "steady ▰",
];

fn clock() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let s = secs % 86_400;
    format!("{:02}:{:02}:{:02} UTC", s / 3600, (s % 3600) / 60, s % 60)
}

fn footer(state: &State) -> PluginAction {
    let dir = state
        .cwd
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("~")
        .to_owned();
    let agent = if state.agent.is_empty() {
        "no agent".to_owned()
    } else {
        agent_label(&state.agent)
    };
    PluginAction::Footer {
        segments: vec![
            seg(format!("⛭ {agent}"), "black", "cyan"),
            seg(format!("⎇ {dir}"), "white", "blue"),
            seg(VIBES[state.vibe % VIBES.len()].to_owned(), "black", "green"),
            seg(format!("✉ {}", state.prompts), "black", "yellow"),
            seg(clock(), "white", "gray"),
        ],
    }
}

fn seg(text: String, fg: &str, bg: &str) -> FooterSegment {
    FooterSegment {
        text,
        fg: Some(fg.to_owned()),
        bg: Some(bg.to_owned()),
    }
}

/// Derives a short agent label from its launch command: the last non-flag
/// token, with any trailing `@version` and package scope/path stripped
/// (`npx -y @zed-industries/claude-code-acp@latest` → `claude-code-acp`).
fn agent_label(command: &str) -> String {
    let token = command
        .split_whitespace()
        .rfind(|t| !t.starts_with('-'))
        .unwrap_or(command);
    let no_version = match token.rfind('@') {
        Some(i) if i > 0 => &token[..i],
        _ => token,
    };
    no_version
        .rsplit('/')
        .next()
        .unwrap_or(no_version)
        .to_owned()
}

fn main() {
    let mut state = State::default();
    serve_auto(move |event, emit| match event {
        HostEvent::Init { cwd, .. } => {
            state.cwd = cwd;
            emit(PluginAction::Log {
                text: "powerline footer online".to_owned(),
            });
            emit(footer(&state));
        }
        HostEvent::SessionStart { agent } => {
            state.agent = agent;
            emit(footer(&state));
        }
        HostEvent::UserPrompt { .. } => {
            state.prompts += 1;
            emit(footer(&state));
        }
        HostEvent::Refresh | HostEvent::TurnEnded { .. } => {
            state.vibe = state.vibe.wrapping_add(1);
            emit(PluginAction::SetStatus {
                key: "vibe".to_owned(),
                value: VIBES[state.vibe % VIBES.len()].to_owned(),
            });
            emit(footer(&state));
        }
        _ => {}
    });
}
