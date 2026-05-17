//! End-to-end protocol tests for the bundled reference plugin binaries.
//!
//! Each plugin is a separate process. These spawn the *real* compiled
//! binaries (via Cargo's `CARGO_BIN_EXE_*`), drive the full host-event
//! sequence over stdin, and assert the plugin replies with well-formed
//! [`PluginAction`] lines — exercising the exact newline-JSON wire the
//! in-app host speaks, with no TTY and no agent.

use std::io::Write;
use std::process::{Command, Stdio};

use rstui_acp_client::plugin::protocol::{HostEvent, PluginAction, decode_action, encode_event};

/// Spawns `exe`, feeds it the standard lifecycle (optionally invoking one
/// slash command), and returns every [`PluginAction`] it emitted.
fn drive(exe: &str, command: Option<(&str, &str)>) -> Vec<PluginAction> {
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn reference plugin");

    let mut events = vec![
        HostEvent::Init {
            api_version: "1".to_owned(),
            client: "test".to_owned(),
            cwd: env!("CARGO_MANIFEST_DIR").to_owned(),
        },
        HostEvent::SessionStart {
            agent: "npx -y @zed-industries/claude-code-acp@latest".to_owned(),
        },
        HostEvent::UserPrompt {
            text: "hello agent".to_owned(),
        },
        HostEvent::Refresh,
        HostEvent::TurnEnded {
            stop_reason: "EndTurn".to_owned(),
        },
    ];
    if let Some((name, args)) = command {
        events.push(HostEvent::Command {
            name: name.to_owned(),
            args: args.to_owned(),
        });
    }
    events.push(HostEvent::Refresh);
    events.push(HostEvent::Shutdown);

    {
        let mut stdin = child.stdin.take().expect("plugin stdin");
        for ev in &events {
            stdin
                .write_all(encode_event(ev).as_bytes())
                .expect("write host event");
        }
        // Dropping stdin closes it: the serve loop also exits on EOF.
    }

    let out = child
        .wait_with_output()
        .expect("plugin should exit (Shutdown / stdin EOF)");
    assert!(out.status.success(), "{exe} exited with {:?}", out.status);
    // Plugin stdout is a JSON-RPC stream: action notifications *and* the
    // `initialize` response. Keep the actions; skip responses/non-actions.
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| decode_action(l).ok())
        .collect()
}

fn has_footer(a: &[PluginAction]) -> bool {
    a.iter().any(|x| matches!(x, PluginAction::Footer { .. }))
}
fn registers(a: &[PluginAction], cmd: &str) -> bool {
    a.iter()
        .any(|x| matches!(x, PluginAction::RegisterCommand { name, .. } if name == cmd))
}
fn any_note(a: &[PluginAction]) -> bool {
    a.iter().any(|x| matches!(x, PluginAction::Note { .. }))
}
fn any_panel(a: &[PluginAction]) -> bool {
    a.iter().any(|x| matches!(x, PluginAction::Panel { .. }))
}

#[test]
fn powerline_emits_a_footer() {
    let a = drive(env!("CARGO_BIN_EXE_rstui-acp-plugin-powerline"), None);
    assert!(has_footer(&a), "powerline must contribute footer segments");
}

#[test]
fn btw_registers_and_records_a_note_panel() {
    let a = drive(
        env!("CARGO_BIN_EXE_rstui-acp-plugin-btw"),
        Some(("btw", "remember the milk")),
    );
    assert!(registers(&a, "btw"));
    assert!(any_note(&a) && any_panel(&a), "btw notes + live panel");
}

#[test]
fn ask_user_registers_and_opens_an_ask() {
    let a = drive(
        env!("CARGO_BIN_EXE_rstui-acp-plugin-ask-user"),
        Some(("ask", "ship it?")),
    );
    assert!(registers(&a, "ask"));
    assert!(
        a.iter().any(|x| matches!(x, PluginAction::AskUser { .. })),
        "ask-user must raise an AskUser overlay"
    );
}

#[test]
fn session_tracks_and_summarizes() {
    let a = drive(
        env!("CARGO_BIN_EXE_rstui-acp-plugin-session"),
        Some(("session", "")),
    );
    assert!(registers(&a, "session"));
    assert!(has_footer(&a), "live stopwatch footer");
    assert!(
        a.iter().any(|x| matches!(x, PluginAction::Modal { .. })),
        "/session opens a summary modal with Reset/Close buttons"
    );
}

#[test]
fn fortune_binds_a_key_chord_to_its_command() {
    let a = drive(env!("CARGO_BIN_EXE_rstui-acp-plugin-fortune"), None);
    assert!(
        a.iter().any(|x| matches!(
            x,
            PluginAction::RegisterKeybinding { command, .. } if command == "fortune"
        )),
        "fortune binds a chord to /fortune"
    );
}

#[test]
fn git_reports_branch_or_degrades() {
    // Runs inside this repo, so a branch is expected; either way it must
    // emit a footer + status and a /git panel without crashing.
    let a = drive(
        env!("CARGO_BIN_EXE_rstui-acp-plugin-git"),
        Some(("git", "")),
    );
    assert!(registers(&a, "git"));
    assert!(has_footer(&a));
    assert!(any_panel(&a));
}

#[test]
fn history_records_prompts() {
    let a = drive(
        env!("CARGO_BIN_EXE_rstui-acp-plugin-history"),
        Some(("history", "")),
    );
    assert!(registers(&a, "history"));
    assert!(any_panel(&a), "history surfaces a prompt panel");
}

#[test]
fn pomodoro_starts_and_ticks() {
    let a = drive(
        env!("CARGO_BIN_EXE_rstui-acp-plugin-pomodoro"),
        Some(("pomodoro", "1")),
    );
    assert!(registers(&a, "pomodoro"));
    assert!(
        any_note(&a) && has_footer(&a),
        "pomodoro: start toast + countdown footer on the next Refresh"
    );
}

#[test]
fn fortune_draws_on_turn_end_and_command() {
    let a = drive(
        env!("CARGO_BIN_EXE_rstui-acp-plugin-fortune"),
        Some(("fortune", "")),
    );
    assert!(registers(&a, "fortune"));
    assert!(any_note(&a) && any_panel(&a), "fortune toast + panel");
}
