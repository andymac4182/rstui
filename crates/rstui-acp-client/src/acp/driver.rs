//! The driver task: spawn the agent, run the `sacp` client, bridge to channels.
//!
//! The structure mirrors `sacp`'s own `yolo_one_shot_client` example (the
//! verified, compiling client shape) but is long-lived and multi-turn: the
//! `connect_with` closure runs a command loop instead of one prompt, and
//! `session/update` notifications are streamed to the reducer rather than
//! printed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sacp::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, TextContent,
};
use sacp::{Agent, Client, ConnectionTo};
use tokio::io::AsyncBufReadExt;
use tokio::sync::oneshot;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use super::events::{AcpEvent, DriverCmd, DriverHandle, PermissionChoice, PermissionOption};

type PermMap = Arc<Mutex<HashMap<u64, oneshot::Sender<PermissionChoice>>>>;

/// Spawns the driver for `command` (a `prog arg arg…` string) with `cwd` as
/// the session working directory, returning the reducer-side handle.
///
/// Spawning the agent, initializing, and creating the session all happen on
/// the returned task; their progress and any failure arrive as
/// [`AcpEvent`]s, so a missing `npx`/agent never panics — it surfaces in the
/// UI as a disconnect the user can recover from by picking another agent.
#[must_use]
pub fn spawn_driver(command: String, cwd: PathBuf) -> DriverHandle {
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DriverCmd>();
    let (ev_tx, ev_rx) = std::sync::mpsc::channel::<AcpEvent>();
    let handle = DriverHandle {
        cmd_tx,
        events: Arc::new(Mutex::new(ev_rx)),
    };

    // Spawn only when a tokio runtime is present. The production path always
    // has one (`run_async`); a headless `Harness` test does not — there the
    // driver is inert (an immediate disconnect) instead of panicking, so the
    // rest of the reducer (composer, autocomplete, screens) stays testable.
    match tokio::runtime::Handle::try_current() {
        Ok(rt) => {
            rt.spawn(async move {
                if let Err(err) = run(command, cwd, cmd_rx, ev_tx.clone()).await {
                    let _ = ev_tx.send(AcpEvent::Error(err));
                }
                let _ = ev_tx.send(AcpEvent::Disconnected("agent connection closed".to_owned()));
            });
        }
        Err(_) => {
            let _ = ev_tx.send(AcpEvent::Disconnected(
                "no async runtime (headless)".to_owned(),
            ));
        }
    }

    handle
}

async fn run(
    command: String,
    cwd: PathBuf,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<DriverCmd>,
    ev_tx: std::sync::mpsc::Sender<AcpEvent>,
) -> Result<(), String> {
    let _ = ev_tx.send(AcpEvent::Status(format!("spawning `{command}`…")));

    let mut parts = command.split_whitespace();
    let program = parts.next().ok_or("empty agent command")?;
    let args: Vec<&str> = parts.collect();

    let mut child = tokio::process::Command::new(program)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn `{program}`: {e}"))?;

    let child_stdin = child.stdin.take().ok_or("agent stdin unavailable")?;
    let child_stdout = child.stdout.take().ok_or("agent stdout unavailable")?;
    if let Some(stderr) = child.stderr.take() {
        let tx = ev_tx.clone();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(AcpEvent::Stderr(line));
            }
        });
    }

    let transport = sacp::ByteStreams::new(child_stdin.compat_write(), child_stdout.compat());

    let perm_map: PermMap = Arc::new(Mutex::new(HashMap::new()));
    let perm_ids = Arc::new(AtomicU64::new(1));

    let notif_tx = ev_tx.clone();
    let perm_tx = ev_tx.clone();
    let perm_map_handler = perm_map.clone();
    let perm_ids_handler = perm_ids.clone();
    let loop_tx = ev_tx.clone();

    let result = Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                for event in summarize_update(&notification) {
                    let _ = notif_tx.send(event);
                }
                Ok(())
            },
            sacp::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _connection| {
                let id = perm_ids_handler.fetch_add(1, Ordering::Relaxed);
                let (title, options) = describe_permission(&request);
                let (tx, rx) = oneshot::channel::<PermissionChoice>();
                if let Ok(mut map) = perm_map_handler.lock() {
                    map.insert(id, tx);
                }
                let _ = perm_tx.send(AcpEvent::Permission { id, title, options });
                let choice = rx.await.unwrap_or(PermissionChoice::Cancelled);
                let outcome = match choice {
                    PermissionChoice::Selected(option_id) => RequestPermissionOutcome::Selected(
                        SelectedPermissionOutcome::new(option_id),
                    ),
                    PermissionChoice::Cancelled => RequestPermissionOutcome::Cancelled,
                };
                responder.respond(RequestPermissionResponse::new(outcome))
            },
            sacp::on_receive_request!(),
        )
        .connect_with(transport, |connection: ConnectionTo<Agent>| async move {
            let init = connection
                .send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                .block_task()
                .await?;
            let _ = loop_tx.send(AcpEvent::Connected(format!("{:?}", init.agent_info)));

            let new_session = connection
                .send_request(NewSessionRequest::new(cwd.clone()))
                .block_task()
                .await?;
            let session_id = new_session.session_id;
            let _ = loop_tx.send(AcpEvent::Status("session ready".to_owned()));

            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    DriverCmd::Prompt(text) => {
                        let conn = connection.clone();
                        let sid = session_id.clone();
                        let done_tx = loop_tx.clone();
                        tokio::spawn(async move {
                            let request = PromptRequest::new(
                                sid,
                                vec![ContentBlock::Text(TextContent::new(text))],
                            );
                            match conn.send_request(request).block_task().await {
                                Ok(resp) => {
                                    let _ = done_tx.send(AcpEvent::TurnEnded(format!(
                                        "{:?}",
                                        resp.stop_reason
                                    )));
                                }
                                Err(err) => {
                                    let _ = done_tx.send(AcpEvent::Error(err.to_string()));
                                }
                            }
                        });
                    }
                    DriverCmd::Cancel => {
                        let _ = connection.send_notification(
                            sacp::schema::CancelNotification::new(session_id.clone()),
                        );
                    }
                    DriverCmd::Permission { id, choice } => {
                        if let Ok(mut map) = perm_map.lock() {
                            if let Some(tx) = map.remove(&id) {
                                let _ = tx.send(choice);
                            }
                        }
                    }
                    DriverCmd::Shutdown => break,
                }
            }
            Ok(())
        })
        .await;

    let _ = child.kill().await;
    result.map_err(|e: sacp::Error| e.to_string())
}

/// Pulls a human title + the offered options out of a permission request,
/// going through JSON so it never depends on the exact `sacp` struct shape
/// (the schema evolves; the JSON keys are the stable ACP contract).
fn describe_permission(request: &RequestPermissionRequest) -> (String, Vec<PermissionOption>) {
    let value = serde_json::to_value(request).unwrap_or(serde_json::Value::Null);
    let title = value
        .get("toolCall")
        .and_then(|tc| tc.get("title").or_else(|| tc.get("rawInput")))
        .and_then(value_to_text)
        .or_else(|| value.get("title").and_then(value_to_text))
        .unwrap_or_else(|| "The agent is requesting permission".to_owned());

    let options = value
        .get("options")
        .and_then(|o| o.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|opt| {
                    let option_id = opt
                        .get("optionId")
                        .or_else(|| opt.get("option_id"))
                        .and_then(|v| v.as_str())?
                        .to_owned();
                    let label = opt
                        .get("name")
                        .or_else(|| opt.get("label"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(&option_id)
                        .to_owned();
                    Some(PermissionOption { option_id, label })
                })
                .collect()
        })
        .unwrap_or_default();

    (title, options)
}

/// Turns one `session/update` notification into transcript events, extracting
/// text from the JSON form so it is robust to schema variant renames.
fn summarize_update(notification: &SessionNotification) -> Vec<AcpEvent> {
    let Ok(value) = serde_json::to_value(&notification.update) else {
        return Vec::new();
    };
    let obj = match value.as_object() {
        Some(o) => o,
        None => return vec![AcpEvent::Status(value.to_string())],
    };
    let kind = obj
        .get("sessionUpdate")
        .and_then(|v| v.as_str())
        .unwrap_or("update");
    let content_text = obj.get("content").and_then(value_to_text);

    match kind {
        "agent_message_chunk" => content_text.map(AcpEvent::AgentText).into_iter().collect(),
        "agent_thought_chunk" => content_text.map(AcpEvent::Thought).into_iter().collect(),
        "user_message_chunk" => Vec::new(),
        "tool_call" | "tool_call_update" => {
            let title = obj
                .get("title")
                .and_then(value_to_text)
                .or(content_text)
                .unwrap_or_else(|| kind.to_owned());
            let status = obj
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| format!(" [{s}]"))
                .unwrap_or_default();
            vec![AcpEvent::ToolCall(format!("{title}{status}"))]
        }
        "available_commands_update" => {
            let cmds = obj
                .get("availableCommands")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| {
                            let name = c.get("name").and_then(|v| v.as_str())?.to_owned();
                            let desc = c
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_owned();
                            Some((name, desc))
                        })
                        .collect()
                })
                .unwrap_or_default();
            vec![AcpEvent::AvailableCommands(cmds)]
        }
        "plan" => {
            let entries = obj
                .get("entries")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|e| {
                            let content = e.get("content").and_then(|c| c.as_str()).unwrap_or("");
                            let st = e.get("status").and_then(|s| s.as_str()).unwrap_or("");
                            format!("• {content} ({st})")
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            vec![AcpEvent::Plan(entries)]
        }
        other => content_text
            .map(AcpEvent::AgentText)
            .into_iter()
            .chain(std::iter::once(AcpEvent::Status(format!(
                "update: {other}"
            ))))
            .collect(),
    }
}

/// Best-effort text extraction from an ACP content value: a string, a
/// `{type:text,text}` block, an array of blocks, or `{text}`.
fn value_to_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(items) => {
            let joined = items
                .iter()
                .filter_map(value_to_text)
                .collect::<Vec<_>>()
                .join("");
            (!joined.is_empty()).then_some(joined)
        }
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(t)) = map.get("text") {
                return Some(t.clone());
            }
            if let Some(content) = map.get("content") {
                return value_to_text(content);
            }
            match map.get("type").and_then(|v| v.as_str()) {
                Some("image") => Some("[image]".to_owned()),
                Some("audio") => Some("[audio]".to_owned()),
                Some("resource" | "resource_link") => Some("[resource]".to_owned()),
                _ => None,
            }
        }
        _ => None,
    }
}
