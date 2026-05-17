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
    // Opt-in (ADR 0016): a `--shm` token in the launch command routes
    // this plugin over a shared-memory channel instead of stdio. The
    // stdio path below is left byte-for-byte unchanged — shm is per-
    // plugin, never the default, zero regression for existing plugins.
    if command.split_whitespace().any(|t| t == "--shm") {
        return spawn_plugin_shm(name, command, cwd, ev_tx);
    }
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

/// Shared-memory variant of [`spawn_plugin`] (ADR 0016). The host owns
/// the segment (creator); the plugin attaches via the `--shm <path>` the
/// SDK's `serve_auto` understands. A dedicated OS thread drives the
/// synchronous [`ShmChannel`](rstui_acp_plugin_sdk::ShmChannel): it polls
/// hot (busy, sub-µs pickup) for a short window after any activity and
/// falls to a 1 ms poll when fully idle (≈0 % CPU). The transport itself
/// is sub-µs; this only bounds the idle→active edge. Rust plugins only.
fn spawn_plugin_shm(
    name: String,
    command: String,
    cwd: std::path::PathBuf,
    ev_tx: std::sync::mpsc::Sender<PluginEvent>,
) -> tokio::sync::mpsc::UnboundedSender<HostEvent> {
    use rstui_acp_plugin_sdk::ShmChannel;
    use tokio::sync::mpsc::error::TryRecvError;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HostEvent>();
    std::thread::spawn(move || {
        let mut parts = command.split_whitespace();
        let Some(program) = parts.next() else {
            return;
        };
        // Drop any operator-supplied `--shm`/value; the host picks the path.
        let mut args: Vec<String> = Vec::new();
        let mut it = parts.peekable();
        while let Some(t) = it.next() {
            if t == "--shm" {
                if it.peek().is_some_and(|n| !n.starts_with('-')) {
                    it.next();
                }
                continue;
            }
            args.push(t.to_owned());
        }
        let sanitized = name.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        let path =
            std::env::temp_dir().join(format!("rstui-plug-{}-{sanitized}.shm", std::process::id()));
        let path_s = path.to_string_lossy().into_owned();
        let emit_log = |text: String| {
            let _ = ev_tx.send(PluginEvent {
                plugin: name.clone(),
                action: PluginAction::Log { text },
            });
        };

        let mut chan = match ShmChannel::create(&path_s) {
            Ok(c) => c,
            Err(err) => {
                emit_log(format!("shm create failed for `{name}`: {err}"));
                return;
            }
        };
        let mut child = match std::process::Command::new(program)
            .args(&args)
            .arg("--shm")
            .arg(&path_s)
            .current_dir(&cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(err) => {
                emit_log(format!("failed to spawn shm plugin `{command}`: {err}"));
                return;
            }
        };

        let stay_hot = std::time::Duration::from_millis(4);
        let mut hot_until = std::time::Instant::now();
        let mut shutting = false;
        loop {
            let mut did = false;
            loop {
                match rx.try_recv() {
                    Ok(ev) => {
                        did = true;
                        let is_shutdown = matches!(ev, HostEvent::Shutdown);
                        if chan.send(encode_event(&ev).as_bytes()).is_err() {
                            shutting = true;
                        }
                        if is_shutdown {
                            shutting = true;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        shutting = true;
                        break;
                    }
                }
            }
            loop {
                match chan.try_recv() {
                    Ok(Some(bytes)) => {
                        did = true;
                        match std::str::from_utf8(&bytes)
                            .ok()
                            .and_then(|s| Message::decode_line(s).ok())
                        {
                            Some(msg) => {
                                if let Some(action) = message_to_plugin_action(&msg)
                                    && ev_tx
                                        .send(PluginEvent {
                                            plugin: name.clone(),
                                            action,
                                        })
                                        .is_err()
                                {
                                    shutting = true;
                                }
                            }
                            // Fail-closed (ADR 0007): malformed ends influence.
                            None => shutting = true,
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            if chan.is_closed() || shutting {
                break;
            }
            if did {
                hot_until = std::time::Instant::now() + stay_hot;
            } else if std::time::Instant::now() < hot_until {
                std::hint::spin_loop();
            } else {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        drop(chan); // unmaps + unlinks the segment and semaphores
    });
    tx
}
