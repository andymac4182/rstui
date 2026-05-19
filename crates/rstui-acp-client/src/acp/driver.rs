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
    AuthMethodId, AuthenticateRequest, ClientCapabilities, ContentBlock, ContentChunk,
    InitializeRequest, LoadSessionRequest, NewSessionRequest, PromptRequest, ProtocolVersion,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionId, SessionModeId, SessionNotification, SessionUpdate,
    SetSessionModeRequest, TextContent,
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
            // ADR 0017: advertise (in the ACP client capabilities `_meta`)
            // that this terminal client can render A2UI / json-render, so
            // an agent may reply with a declarative UI document. A2UI
            // negotiates via this metadata (`a2uiClientCapabilities`).
            let init = connection
                .send_request(
                    InitializeRequest::new(ProtocolVersion::LATEST).client_capabilities(
                        ClientCapabilities::new().meta(super::richui::render_capability_meta()),
                    ),
                )
                .block_task()
                .await?;
            let _ = loop_tx.send(AcpEvent::Connected(format!("{:?}", init.agent_info)));

            // Create the session, running the ACP `authenticate` handshake
            // first if the agent rejects us and advertises auth methods
            // (Codex sign-in). Agents that auth out-of-band (env/API key)
            // simply succeed on the first try and never hit this.
            let new_session = 'session: loop {
                match connection
                    .send_request(NewSessionRequest::new(cwd.clone()))
                    .block_task()
                    .await
                {
                    Ok(s) => break 'session s,
                    Err(e) => {
                        if init.auth_methods.is_empty() {
                            return Err(e);
                        }
                        let methods: Vec<super::events::AuthOption> = init
                            .auth_methods
                            .iter()
                            .map(|m| super::events::AuthOption {
                                id: m.id().0.to_string(),
                                name: m.name().to_owned(),
                                description: m.description().unwrap_or_default().to_owned(),
                            })
                            .collect();
                        let _ = loop_tx.send(AcpEvent::AuthRequired(methods));
                        // Wait for the user's choice; ignore unrelated
                        // commands until authenticated (no session yet).
                        loop {
                            match cmd_rx.recv().await {
                                Some(DriverCmd::Authenticate(id)) => {
                                    match connection
                                        .send_request(AuthenticateRequest::new(AuthMethodId::new(
                                            id,
                                        )))
                                        .block_task()
                                        .await
                                    {
                                        Ok(_) => break, // retry session/new
                                        Err(ae) => {
                                            let _ = loop_tx.send(AcpEvent::Error(ae.to_string()));
                                            let methods: Vec<super::events::AuthOption> = init
                                                .auth_methods
                                                .iter()
                                                .map(|m| super::events::AuthOption {
                                                    id: m.id().0.to_string(),
                                                    name: m.name().to_owned(),
                                                    description: m
                                                        .description()
                                                        .unwrap_or_default()
                                                        .to_owned(),
                                                })
                                                .collect();
                                            let _ = loop_tx.send(AcpEvent::AuthRequired(methods));
                                        }
                                    }
                                }
                                Some(DriverCmd::Shutdown) | None => return Ok(()),
                                _ => {}
                            }
                        }
                    }
                }
            };
            let session_id = new_session.session_id.clone();
            let model_state = new_session.models.clone();
            let mode_state = new_session.modes.clone();
            let _ = loop_tx.send(AcpEvent::Status("session ready".to_owned()));
            // Remember this session id so `/resume` can ask the agent to
            // `session/load` it on a later run.
            let _ = loop_tx.send(AcpEvent::SessionStarted(session_id.0.to_string()));
            // Surface the agent's session modes (if any) so `/mode` can
            // offer them — how Codex's plan/approval modes reach the client.
            if let Some(ms) = mode_state {
                let _ = loop_tx.send(AcpEvent::Modes {
                    current: ms.current_mode_id.0.to_string(),
                    available: ms
                        .available_modes
                        .iter()
                        .map(|m| super::events::ModeOption {
                            id: m.id.0.to_string(),
                            name: m.name.clone(),
                            description: m.description.clone().unwrap_or_default(),
                        })
                        .collect(),
                });
            }
            // Surface the agent's model catalogue (if any) so `/model` can
            // offer it; the ids round-trip back via `session/set_model`.
            if let Some(ms) = model_state {
                let _ = loop_tx.send(AcpEvent::Models {
                    current: ms.current_model_id.0.to_string(),
                    available: ms
                        .available_models
                        .iter()
                        .map(|m| super::events::ModelOption {
                            id: m.model_id.0.to_string(),
                            name: m.name.clone(),
                            description: m.description.clone().unwrap_or_default(),
                        })
                        .collect(),
                });
            }

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
                    DriverCmd::SetModel(model_id) => {
                        // sacp 11 ships no typed `session/set_model` request
                        // (only set_mode/load/…), so send it as a raw
                        // `UntypedMessage` — the wire shape is the stable ACP
                        // contract, the same JSON-first robustness this
                        // module already relies on for notifications.
                        match sacp::UntypedMessage::new(
                            "session/set_model",
                            serde_json::json!({
                                "sessionId": session_id,
                                "modelId": model_id,
                            }),
                        ) {
                            Ok(req) => match connection.send_request(req).block_task().await {
                                Ok(_) => {
                                    let _ = loop_tx.send(AcpEvent::ModelSelected(model_id));
                                }
                                Err(err) => {
                                    let _ = loop_tx.send(AcpEvent::Error(err.to_string()));
                                }
                            },
                            Err(err) => {
                                let _ = loop_tx.send(AcpEvent::Error(err.to_string()));
                            }
                        }
                    }
                    DriverCmd::SetMode(mode_id) => {
                        // sacp 11 *does* type `session/set_mode`.
                        let req = SetSessionModeRequest::new(
                            session_id.clone(),
                            SessionModeId::new(mode_id.clone()),
                        );
                        match connection.send_request(req).block_task().await {
                            Ok(_) => {
                                let _ = loop_tx.send(AcpEvent::ModeChanged(mode_id));
                            }
                            Err(err) => {
                                let _ = loop_tx.send(AcpEvent::Error(err.to_string()));
                            }
                        }
                    }
                    DriverCmd::LoadSession(sid) => {
                        // sacp 11 *does* type `session/load`. The agent
                        // replays the prior conversation as session/update
                        // notifications, which flow into the transcript
                        // through the existing notification path.
                        let req = LoadSessionRequest::new(SessionId::new(sid.clone()), cwd.clone());
                        match connection.send_request(req).block_task().await {
                            Ok(_) => {
                                let _ = loop_tx
                                    .send(AcpEvent::Status(format!("resumed session {sid}")));
                            }
                            Err(err) => {
                                let _ = loop_tx.send(AcpEvent::Error(err.to_string()));
                            }
                        }
                    }
                    DriverCmd::Permission { id, choice } => {
                        if let Ok(mut map) = perm_map.lock() {
                            if let Some(tx) = map.remove(&id) {
                                let _ = tx.send(choice);
                            }
                        }
                    }
                    // Auth is handled before the session exists; once it
                    // does, a stray Authenticate is a no-op.
                    DriverCmd::Authenticate(_) => {}
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
    // DRV-2: read the typed `sacp` request directly instead of round-
    // tripping the whole thing through `serde_json::to_value` every
    // permission prompt. Byte-identical to the JSON walk it replaces,
    // because the JSON keys the old code relied on *are* these typed
    // fields:
    //  * `ToolCallUpdate.fields` is `#[serde(flatten)]`, so JSON
    //    `toolCall.title` IS `tool_call.fields.title` and
    //    `toolCall.rawInput` IS `tool_call.fields.raw_input`. The old
    //    code ran the picked value through `value_to_text`; for a present
    //    title that value is a JSON string and `value_to_text` of a
    //    string is the string itself, so a present title maps through
    //    unchanged, and the rawInput fallback still goes through
    //    `value_to_text` on the *same* `serde_json::Value`.
    //  * `RequestPermissionRequest` has no top-level `title`, so the old
    //    `.or(value["title"])` arm was always `None` for real data — the
    //    default still covers the title-absent case.
    //  * `PermissionOptionId(pub Arc<str>)` is a transparent newtype:
    //    `serde_json` flattens it to its inner string, exactly what
    //    `opt["optionId"].as_str()` yielded; `optionId` and `name` are
    //    always present (non-`Option`, camelCase), so the old
    //    `.or(opt["option_id"])` / `.or(opt["label"])` / `filter_map`
    //    arms were dead for real `sacp` data.
    // Schema-resilience (the old comment's concern) is preserved: the
    // only still-untyped value — `rawInput`, arbitrary JSON — is still
    // resolved by `value_to_text`, never by a fixed struct shape.
    let fields = &request.tool_call.fields;
    let title = fields
        .title
        .clone()
        .or_else(|| fields.raw_input.as_ref().and_then(value_to_text))
        .unwrap_or_else(|| "The agent is requesting permission".to_owned());

    let options = request
        .options
        .iter()
        .map(|o| PermissionOption {
            option_id: o.option_id.0.as_ref().to_owned(),
            label: o.name.clone(),
        })
        .collect();

    (title, options)
}

/// Turns one `session/update` notification into transcript events, extracting
/// text from the JSON form so it is robust to schema variant renames.
/// Classifies one agent text block: a self-contained A2UI / json-render
/// document becomes an [`AcpEvent::RichUi`] (rendered as a rich
/// transcript entry); anything else is ordinary [`AcpEvent::AgentText`].
/// Detection is conservative and total, so a streamed prose chunk (never
/// a complete JSON doc) is unaffected.
fn agent_text_event(text: String) -> AcpEvent {
    match super::richui::detect(&text) {
        Some(payload) => AcpEvent::RichUi(payload),
        None => AcpEvent::AgentText(text),
    }
}

fn summarize_update(notification: &SessionNotification) -> Vec<AcpEvent> {
    // DRV-1: typed fast-path for the two highest-frequency streamed variants.
    // Agent message/thought chunks arrive token-by-token throughout every
    // turn, so they dominate notification volume; the generic arm below pays
    // a full `serde_json::to_value(&update)` serialize + a `value_to_text`
    // re-walk for each one. Matching the typed enum directly skips both. This
    // is behaviour-identical to the generic arm for a text chunk (it extracts
    // exactly `TextContent.text`) and only fires for `ContentBlock::Text`;
    // any non-text content or any other variant falls through to the proven
    // serde_json path unchanged.
    match &notification.update {
        SessionUpdate::AgentMessageChunk(ContentChunk {
            content: ContentBlock::Text(TextContent { text, .. }),
            ..
        }) => return vec![agent_text_event(text.clone())],
        SessionUpdate::AgentThoughtChunk(ContentChunk {
            content: ContentBlock::Text(TextContent { text, .. }),
            ..
        }) => return vec![AcpEvent::Thought(text.clone())],
        _ => {}
    }

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
        "agent_message_chunk" => content_text.map(agent_text_event).into_iter().collect(),
        "agent_thought_chunk" => content_text.map(AcpEvent::Thought).into_iter().collect(),
        "user_message_chunk" => Vec::new(),
        "tool_call" => {
            let id = obj
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            vec![AcpEvent::ToolCall(super::events::ToolCallInfo {
                id,
                title: obj
                    .get("title")
                    .and_then(value_to_text)
                    .unwrap_or_else(|| "tool".to_owned()),
                kind: super::events::ToolKind::parse(
                    obj.get("kind").and_then(|v| v.as_str()).unwrap_or(""),
                ),
                status: super::events::ToolStatus::parse(
                    obj.get("status").and_then(|v| v.as_str()).unwrap_or(""),
                ),
                input: compact_input(obj.get("rawInput")),
                body: tool_bodies(obj.get("content")),
            })]
        }
        "tool_call_update" => {
            let id = obj
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            vec![AcpEvent::ToolCallUpdate(super::events::ToolCallPatch {
                id,
                title: obj.get("title").and_then(value_to_text),
                kind: obj
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .map(super::events::ToolKind::parse),
                status: obj
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(super::events::ToolStatus::parse),
                input: obj.get("rawInput").map(|v| compact_input(Some(v))),
                body: obj.get("content").map(|c| tool_bodies(Some(c))),
            })]
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
                            let content = e
                                .get("content")
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .to_owned();
                            let status = super::events::TodoStatus::parse(
                                e.get("status").and_then(|s| s.as_str()).unwrap_or(""),
                            );
                            let priority = e
                                .get("priority")
                                .and_then(|p| p.as_str())
                                .unwrap_or("")
                                .to_owned();
                            super::events::TodoEntry {
                                content,
                                status,
                                priority,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            vec![AcpEvent::Plan(entries)]
        }
        "usage_update" => {
            let used = obj.get("used").and_then(serde_json::Value::as_u64);
            let size = obj.get("size").and_then(serde_json::Value::as_u64);
            // Only surface a usage event when the agent actually sent the
            // numbers; a malformed update should not zero the display.
            match (used, size) {
                (Some(used), size) => vec![AcpEvent::Usage {
                    used,
                    size: size.unwrap_or(0),
                }],
                _ => Vec::new(),
            }
        }
        "current_mode_update" => obj
            .get("currentModeId")
            .and_then(|v| v.as_str())
            .map(|id| AcpEvent::ModeChanged(id.to_owned()))
            .into_iter()
            .collect(),
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

/// Compacts a tool's `rawInput` object into `key=value, …` (scalars only,
/// truncated) — the opencode `[k=v]` affordance.
fn compact_input(value: Option<&serde_json::Value>) -> String {
    let Some(serde_json::Value::Object(map)) = value else {
        return String::new();
    };
    let mut parts = Vec::new();
    for (k, v) in map {
        let rendered = match v {
            serde_json::Value::String(s) => {
                let s = s.replace('\n', " ");
                if s.chars().count() > 40 {
                    format!("{}…", s.chars().take(40).collect::<String>())
                } else {
                    s
                }
            }
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => continue,
        };
        parts.push(format!("{k}={rendered}"));
        if parts.len() >= 6 {
            break;
        }
    }
    parts.join(", ")
}

/// Parses an ACP tool-call `content` array into renderable bodies, turning
/// `diff` blocks into a unified-ish text the UI colours per line.
fn tool_bodies(value: Option<&serde_json::Value>) -> Vec<super::events::ToolBody> {
    use super::events::ToolBody;
    let Some(serde_json::Value::Array(items)) = value else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        match item.get("type").and_then(|v| v.as_str()) {
            Some("diff") => {
                let path = item
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let old = item.get("oldText").and_then(|v| v.as_str()).unwrap_or("");
                let new = item.get("newText").and_then(|v| v.as_str()).unwrap_or("");
                out.push(ToolBody::Diff {
                    path,
                    text: render_diff(old, new),
                });
            }
            Some("terminal") => out.push(ToolBody::Text("[terminal]".to_owned())),
            _ => {
                if let Some(t) = value_to_text(item) {
                    out.push(ToolBody::Text(t));
                }
            }
        }
    }
    out
}

/// A minimal, deterministic line-wise unified diff (no LCS — full old block
/// as deletions then the new block as additions; the UI colours `+`/`-`).
fn render_diff(old: &str, new: &str) -> String {
    if old.is_empty() {
        return new
            .lines()
            .map(|l| format!("+{l}"))
            .collect::<Vec<_>>()
            .join("\n");
    }
    if new.is_empty() {
        return old
            .lines()
            .map(|l| format!("-{l}"))
            .collect::<Vec<_>>()
            .join("\n");
    }
    let mut lines: Vec<String> = old.lines().map(|l| format!("-{l}")).collect();
    lines.extend(new.lines().map(|l| format!("+{l}")));
    lines.join("\n")
}

#[cfg(test)]
mod drv2_tests {
    use super::*;
    use sacp::schema::{
        PermissionOption as SacpOption, PermissionOptionId, PermissionOptionKind, SessionId,
        ToolCallId, ToolCallUpdate, ToolCallUpdateFields,
    };

    /// `events::PermissionOption` is not `PartialEq`; compare the two
    /// results by their observable fields.
    fn proj(r: &(String, Vec<PermissionOption>)) -> (String, Vec<(String, String)>) {
        (
            r.0.clone(),
            r.1.iter()
                .map(|o| (o.option_id.clone(), o.label.clone()))
                .collect(),
        )
    }

    /// The exact pre-DRV-2 `serde_json` walk, kept verbatim as the oracle:
    /// the typed `describe_permission` must equal it byte-for-byte.
    fn describe_permission_via_json(
        request: &RequestPermissionRequest,
    ) -> (String, Vec<PermissionOption>) {
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

    fn opt(id: &str, name: &str) -> SacpOption {
        SacpOption::new(
            PermissionOptionId::new(id),
            name,
            PermissionOptionKind::AllowOnce,
        )
    }

    fn req(fields: ToolCallUpdateFields, opts: Vec<SacpOption>) -> RequestPermissionRequest {
        RequestPermissionRequest::new(
            SessionId::new("sess-1"),
            ToolCallUpdate::new(ToolCallId::new("tc-1"), fields),
            opts,
        )
    }

    /// DRV-2 gate (the PG-2/CM-3 exactness discipline): the typed
    /// extraction must be byte-identical to the JSON walk it replaced,
    /// across title-present, rawInput-fallback, neither (→ default),
    /// empty-title (NOT default), rawInput-not-text (→ default), and
    /// zero/many options.
    #[test]
    fn typed_describe_permission_is_byte_identical_to_the_json_walk() {
        let cases = [
            req(
                ToolCallUpdateFields::new().title(Some("Run the test suite".to_owned())),
                vec![opt("allow", "Allow"), opt("deny", "Deny")],
            ),
            req(
                ToolCallUpdateFields::new()
                    .raw_input(Some(serde_json::json!({ "command": "ls -la" }))),
                vec![opt("o1", "Only one")],
            ),
            req(ToolCallUpdateFields::new(), vec![]),
            req(
                ToolCallUpdateFields::new().title(Some(String::new())),
                vec![opt("x", "X")],
            ),
            req(
                ToolCallUpdateFields::new().raw_input(Some(serde_json::json!(42))),
                vec![],
            ),
        ];
        for (i, r) in cases.iter().enumerate() {
            assert_eq!(
                proj(&describe_permission(r)),
                proj(&describe_permission_via_json(r)),
                "case {i}: typed describe_permission diverged from the JSON walk"
            );
        }
    }
}
