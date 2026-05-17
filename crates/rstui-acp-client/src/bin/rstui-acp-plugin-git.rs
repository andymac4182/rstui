//! Reference plugin: git branch / dirty state + a changed-files panel.
//!
//! The single most-wanted coding-session affordance (oh-my-pi
//! `git-checkpoint`, opencode's "Modified Files" sidebar): a footer segment
//! `⎇ branch ±N`, a `git` status key, and a `/git` panel listing the
//! `git status --porcelain` changes. Shells out to `git` only; degrades
//! cleanly outside a repo or when `git` is absent.

use std::path::PathBuf;
use std::process::Command;

use rstui_acp_client::plugin::protocol::{FooterSegment, HostEvent, PluginAction};
use rstui_acp_client::plugin::serve;

fn git(cwd: &PathBuf, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim_end().to_owned())
}

struct Snapshot {
    branch: String,
    changes: Vec<String>,
}

fn snapshot(cwd: &PathBuf) -> Option<Snapshot> {
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let porcelain = git(cwd, &["status", "--porcelain"]).unwrap_or_default();
    let changes: Vec<String> = porcelain
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_owned())
        .collect();
    Some(Snapshot { branch, changes })
}

fn emit_state(cwd: &PathBuf, emit: &mut dyn FnMut(PluginAction)) {
    match snapshot(cwd) {
        Some(s) => {
            let n = s.changes.len();
            let (label, bg) = if n == 0 {
                (format!("⎇ {}", s.branch), "green")
            } else {
                (format!("⎇ {} ±{n}", s.branch), "yellow")
            };
            emit(PluginAction::Footer {
                segments: vec![FooterSegment {
                    text: label,
                    fg: Some("black".to_owned()),
                    bg: Some(bg.to_owned()),
                }],
            });
            emit(PluginAction::SetStatus {
                key: "git".to_owned(),
                value: format!("{} ({n} changed)", s.branch),
            });
        }
        None => {
            emit(PluginAction::Footer {
                segments: vec![FooterSegment {
                    text: "⎇ —".to_owned(),
                    fg: Some("white".to_owned()),
                    bg: Some("gray".to_owned()),
                }],
            });
            emit(PluginAction::SetStatus {
                key: "git".to_owned(),
                value: "not a git repo".to_owned(),
            });
        }
    }
}

fn main() {
    let mut cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    serve(move |event, emit| match event {
        HostEvent::Init { cwd: dir, .. } => {
            cwd = PathBuf::from(dir);
            emit(PluginAction::RegisterCommand {
                name: "git".to_owned(),
                description: "Show git branch & changed files".to_owned(),
            });
            emit_state(&cwd, emit);
        }
        HostEvent::SessionStart { .. } | HostEvent::TurnEnded { .. } | HostEvent::Refresh => {
            emit_state(&cwd, emit);
        }
        HostEvent::Command { name, .. } if name == "git" => {
            let body = match snapshot(&cwd) {
                Some(s) if s.changes.is_empty() => {
                    vec![format!("on {}", s.branch), "working tree clean".to_owned()]
                }
                Some(s) => {
                    let mut b = vec![format!("on {} — {} changed:", s.branch, s.changes.len())];
                    b.extend(s.changes.into_iter().take(40));
                    b
                }
                None => vec!["not a git repository".to_owned()],
            };
            emit(PluginAction::Panel {
                title: "Git".to_owned(),
                body,
            });
        }
        _ => {}
    });
}
