//! The in-app plugin host: spawns plugin processes, broadcasts host events,
//! and merges their actions back to the reducer.
//!
//! Mirrors the [`acp`](crate::acp) seam: app → plugins over per-plugin tokio
//! channels (a task owns each child's stdin), plugins → app over one
//! `std::sync::mpsc` drained by a re-armed `Cmd::perform`. Deny-by-default:
//! nothing spawns unless the operator passed `--plugin` (or an adjacent
//! reference plugin is auto-discovered); a malformed line is fail-closed.

use std::path::Path;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use rstui_acp_plugin_sdk::jsonrpc::Message;
use rstui_acp_plugin_sdk::proto::message_to_plugin_action;
use rstui_acp_plugin_sdk::{HostEvent, PluginAction, encode_event};

/// One action from a named plugin, delivered to the reducer.
#[derive(Debug, Clone)]
pub struct PluginEvent {
    /// The plugin that produced it (its launch-command basename).
    pub plugin: String,
    /// The action.
    pub action: PluginAction,
}

struct PluginConn {
    name: String,
    tx: tokio::sync::mpsc::UnboundedSender<HostEvent>,
}

/// The set of running plugins.
#[derive(Clone)]
pub struct PluginHost {
    conns: Arc<Vec<PluginConn>>,
    events: Arc<Mutex<std::sync::mpsc::Receiver<PluginEvent>>>,
    names: Arc<Vec<String>>,
}

impl PluginHost {
    /// Launches every command in `commands` with `cwd` as the working
    /// directory. A command that fails to spawn surfaces a single
    /// [`PluginAction::Log`] rather than aborting the client.
    #[must_use]
    pub fn launch(commands: &[String], cwd: &Path) -> Self {
        let (ev_tx, ev_rx) = std::sync::mpsc::channel::<PluginEvent>();
        let mut conns = Vec::new();
        let mut names = Vec::new();
        for command in commands {
            let name = plugin_name(command);
            let tx = spawn_plugin(
                name.clone(),
                command.clone(),
                cwd.to_path_buf(),
                ev_tx.clone(),
            );
            conns.push(PluginConn {
                name: name.clone(),
                tx,
            });
            names.push(name);
        }
        Self {
            conns: Arc::new(conns),
            events: Arc::new(Mutex::new(ev_rx)),
            names: Arc::new(names),
        }
    }

    /// `true` when no plugins are running (skip the drain subscription).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.conns.is_empty()
    }

    /// The launched plugin names, in order.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Sends `event` to every plugin.
    pub fn broadcast(&self, event: &HostEvent) {
        for conn in self.conns.iter() {
            let _ = conn.tx.send(event.clone());
        }
    }

    /// Sends `event` to a single plugin by name (used to route an
    /// ask-user answer back to its originator).
    pub fn send_to(&self, name: &str, event: &HostEvent) {
        for conn in self.conns.iter() {
            if conn.name == name {
                let _ = conn.tx.send(event.clone());
            }
        }
    }

    /// Blocks until the next plugin action (called only inside a
    /// `Cmd::perform`, which the runtime runs on `spawn_blocking`).
    #[must_use]
    pub fn recv_blocking(&self) -> Option<PluginEvent> {
        let rx = self.events.lock().ok()?;
        rx.recv().ok()
    }
}

/// The launch-command basename, used as the plugin's display name.
fn plugin_name(command: &str) -> String {
    command
        .split_whitespace()
        .next()
        .and_then(|p| Path::new(p).file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| command.to_owned())
}

fn spawn_plugin(
    name: String,
    command: String,
    cwd: std::path::PathBuf,
    ev_tx: std::sync::mpsc::Sender<PluginEvent>,
) -> tokio::sync::mpsc::UnboundedSender<HostEvent> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HostEvent>();
    tokio::spawn(async move {
        let mut parts = command.split_whitespace();
        let Some(program) = parts.next() else {
            return;
        };
        let args: Vec<&str> = parts.collect();
        let spawned = tokio::process::Command::new(program)
            .args(&args)
            .current_dir(&cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();
        let mut child = match spawned {
            Ok(c) => c,
            Err(err) => {
                let _ = ev_tx.send(PluginEvent {
                    plugin: name.clone(),
                    action: PluginAction::Log {
                        text: format!("failed to spawn plugin `{command}`: {err}"),
                    },
                });
                return;
            }
        };

        let mut stdin = match child.stdin.take() {
            Some(s) => s,
            None => return,
        };
        if let Some(stdout) = child.stdout.take() {
            let reader_name = name.clone();
            let reader_tx = ev_tx.clone();
            tokio::spawn(async move {
                let mut lines = tokio::io::BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.trim().is_empty() {
                        continue;
                    }
                    match Message::decode_line(&line) {
                        Ok(msg) => {
                            // A JSON-RPC response (e.g. the `initialize` ack)
                            // or a non-action message is not a UI action —
                            // skip it, do not fail-close.
                            if let Some(action) = message_to_plugin_action(&msg) {
                                if reader_tx
                                    .send(PluginEvent {
                                        plugin: reader_name.clone(),
                                        action,
                                    })
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                        // Fail-closed (ADR 0007): a malformed (non-JSON-RPC)
                        // line ends the plugin's influence.
                        Err(_) => break,
                    }
                }
            });
        }

        while let Some(event) = rx.recv().await {
            let shutdown = matches!(event, HostEvent::Shutdown);
            if stdin
                .write_all(encode_event(&event).as_bytes())
                .await
                .is_err()
            {
                break;
            }
            let _ = stdin.flush().await;
            if shutdown {
                break;
            }
        }
        let _ = child.kill().await;
    });
    tx
}
