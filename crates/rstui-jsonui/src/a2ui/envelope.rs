//! The A2UI server→client message envelope and its **total** parsers.
//!
//! # The six-message surface protocol
//!
//! Every server→client message is `{"version":"v0.10", <oneKey>:{…}}`
//! where `<oneKey>` selects the variant (`server_to_client.json`):
//!
//! - `createSurface` — open a surface (`surfaceId`, `catalogId`,
//!   optional `theme`, `sendDataModel`); nothing renders until a
//!   `root` component arrives.
//! - `updateComponents` — a flat adjacency list of `{id, component, …}`;
//!   exactly one `id == "root"`. Upsert by id; a different `component`
//!   type for an existing id recreates it
//!   ([`Component`](super::catalog::Component) holds the raw props).
//! - `updateDataModel` — write `value` at `path` (default `/`); a
//!   missing `value` deletes; `/` replaces the whole model.
//! - `deleteSurface` — drop the surface.
//! - `callFunction` / `actionResponse` — parsed into a variant for
//!   completeness; a no-op projection is correct (these drive a
//!   server-initiated call / a response, not the tree).
//!
//! # Totality
//!
//! [`A2uiServerMessage::parse`] never fails or panics: a missing key, a
//! non-object body, a wrong/absent `version`, truncated or hostile JSON
//! all yield [`A2uiServerMessage::Unknown`], which
//! [`apply`](super::A2uiSurface::apply) ignores — the screen keeps the
//! last good frame (A2UI's own progressive-rendering contract).
//! [`parse_stream`](A2uiServerMessage::parse_stream) is the JSONL
//! reader: it scans concatenated/
//! newline-delimited JSON values and skips any that do not parse, so a
//! partially-received stream still applies every complete message.

use serde_json::Value;

use crate::capability::A2UI_VERSION;

/// One parsed A2UI server→client message.
///
/// Each variant carries only the fields the terminal client acts on;
/// the original JSON object is not retained (the document is
/// re-projected every frame from [`A2uiSurface`](super::A2uiSurface)
/// state, ADR 0012).
#[derive(Debug, Clone, PartialEq)]
pub enum A2uiServerMessage {
    /// Open a new surface and begin rendering it.
    CreateSurface {
        /// The surface this message addresses.
        surface_id: String,
        /// The component catalog the agent is targeting.
        catalog_id: String,
        /// Optional initial theme parameters (kept verbatim).
        theme: Option<Value>,
        /// Echo the full data model on every client→server message.
        send_data_model: bool,
    },
    /// Upsert a batch of components into a surface's adjacency list.
    UpdateComponents {
        /// The surface this message addresses.
        surface_id: String,
        /// The flat `{id, component, …}` component objects.
        components: Vec<Value>,
    },
    /// Mutate a surface's data model at a JSON-Pointer path.
    UpdateDataModel {
        /// The surface this message addresses.
        surface_id: String,
        /// The target pointer (`/` / absent ⇒ whole model).
        path: String,
        /// The new value, or `None` to delete the key at `path`.
        value: Option<Value>,
    },
    /// Drop a surface and its state.
    DeleteSurface {
        /// The surface to delete.
        surface_id: String,
    },
    /// A server-initiated function call (parsed for completeness; the
    /// terminal client has no remote-callable functions, so applying it
    /// is a no-op).
    CallFunction {
        /// The unique id to echo in the response.
        function_call_id: String,
        /// The raw `callFunction` payload.
        call: Value,
    },
    /// A response to a client-initiated `wantResponse` action (parsed
    /// for completeness; a no-op projection is correct).
    ActionResponse {
        /// The id of the action this responds to.
        action_id: String,
        /// The raw `actionResponse` payload.
        response: Value,
    },
    /// Anything that is not a well-formed v0.10 message — ignored by
    /// [`apply`](super::A2uiSurface::apply) so a truncated/hostile
    /// stream degrades instead of erroring.
    Unknown,
}

impl A2uiServerMessage {
    /// Parses one server→client message value. Total: a wrong/missing
    /// `version`, a missing/duplicate body key, or a non-object body all
    /// yield [`Unknown`](Self::Unknown) — never an error or panic.
    #[must_use]
    pub fn parse(message: &Value) -> Self {
        let Some(fields) = message.as_object() else {
            return Self::Unknown;
        };
        // The version must be present and exactly v0.10; a foreign or
        // absent version is a stream we cannot interpret.
        if fields.get("version").and_then(Value::as_str) != Some(A2UI_VERSION) {
            return Self::Unknown;
        }
        if let Some(body) = fields.get("createSurface").and_then(Value::as_object) {
            let (Some(surface_id), Some(catalog_id)) = (
                body.get("surfaceId").and_then(Value::as_str),
                body.get("catalogId").and_then(Value::as_str),
            ) else {
                return Self::Unknown;
            };
            return Self::CreateSurface {
                surface_id: surface_id.to_owned(),
                catalog_id: catalog_id.to_owned(),
                theme: body.get("theme").cloned(),
                send_data_model: body
                    .get("sendDataModel")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            };
        }
        if let Some(body) = fields.get("updateComponents").and_then(Value::as_object) {
            let Some(surface_id) = body.get("surfaceId").and_then(Value::as_str) else {
                return Self::Unknown;
            };
            let components = body
                .get("components")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            return Self::UpdateComponents {
                surface_id: surface_id.to_owned(),
                components,
            };
        }
        if let Some(body) = fields.get("updateDataModel").and_then(Value::as_object) {
            let Some(surface_id) = body.get("surfaceId").and_then(Value::as_str) else {
                return Self::Unknown;
            };
            let path = body
                .get("path")
                .and_then(Value::as_str)
                .filter(|pointer| !pointer.is_empty())
                .unwrap_or("/")
                .to_owned();
            return Self::UpdateDataModel {
                surface_id: surface_id.to_owned(),
                path,
                value: body.get("value").cloned(),
            };
        }
        if let Some(body) = fields.get("deleteSurface").and_then(Value::as_object) {
            let Some(surface_id) = body.get("surfaceId").and_then(Value::as_str) else {
                return Self::Unknown;
            };
            return Self::DeleteSurface {
                surface_id: surface_id.to_owned(),
            };
        }
        if let Some(call) = fields.get("callFunction") {
            return Self::CallFunction {
                function_call_id: fields
                    .get("functionCallId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                call: call.clone(),
            };
        }
        if let Some(response) = fields.get("actionResponse") {
            return Self::ActionResponse {
                action_id: fields
                    .get("actionId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                response: response.clone(),
            };
        }
        Self::Unknown
    }

    /// Parses one server→client message from a JSON string. A parse
    /// failure yields [`Unknown`](Self::Unknown) (totality).
    #[must_use]
    pub fn parse_str(source: &str) -> Self {
        serde_json::from_str::<Value>(source).map_or(Self::Unknown, |value| Self::parse(&value))
    }

    /// Parses a JSONL / concatenated-JSON stream into messages, skipping
    /// any value that is not well-formed v0.10. A truncated trailing
    /// value is dropped; every complete one is returned in order — the
    /// progressive-streaming contract.
    #[must_use]
    pub fn parse_stream(source: &str) -> Vec<Self> {
        let mut messages = Vec::new();
        let mut stream = serde_json::Deserializer::from_str(source).into_iter::<Value>();
        for value in stream.by_ref() {
            let Ok(value) = value else {
                // A malformed/truncated value: stop reading the rest of
                // this chunk (its byte offset is now ambiguous) but keep
                // everything parsed so far.
                break;
            };
            match Self::parse(&value) {
                Self::Unknown => {}
                message => messages.push(message),
            }
        }
        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_each_message_type() {
        assert!(matches!(
            A2uiServerMessage::parse(&json!({
                "version": "v0.10",
                "createSurface": {"surfaceId": "s1", "catalogId": "c", "sendDataModel": true}
            })),
            A2uiServerMessage::CreateSurface {
                send_data_model: true,
                ..
            }
        ));
        assert!(matches!(
            A2uiServerMessage::parse(&json!({
                "version": "v0.10",
                "updateComponents": {"surfaceId": "s1", "components": [{"id": "root"}]}
            })),
            A2uiServerMessage::UpdateComponents { .. }
        ));
        // path default `/`, value omitted ⇒ delete
        let msg = A2uiServerMessage::parse(&json!({
            "version": "v0.10",
            "updateDataModel": {"surfaceId": "s1"}
        }));
        assert_eq!(
            msg,
            A2uiServerMessage::UpdateDataModel {
                surface_id: "s1".to_owned(),
                path: "/".to_owned(),
                value: None,
            }
        );
        assert!(matches!(
            A2uiServerMessage::parse(&json!({
                "version": "v0.10", "deleteSurface": {"surfaceId": "s1"}
            })),
            A2uiServerMessage::DeleteSurface { .. }
        ));
        assert!(matches!(
            A2uiServerMessage::parse(&json!({
                "version": "v0.10", "functionCallId": "f1",
                "callFunction": {"call": "x", "returnType": "string", "callableFrom": "remoteOnly"}
            })),
            A2uiServerMessage::CallFunction { .. }
        ));
        assert!(matches!(
            A2uiServerMessage::parse(&json!({
                "version": "v0.10", "actionId": "a1",
                "actionResponse": {"value": 1}
            })),
            A2uiServerMessage::ActionResponse { .. }
        ));
    }

    #[test]
    fn totality_bad_version_and_garbage() {
        assert_eq!(
            A2uiServerMessage::parse(&json!({"version": "v9", "createSurface": {}})),
            A2uiServerMessage::Unknown
        );
        assert_eq!(
            A2uiServerMessage::parse(&json!("not even an object")),
            A2uiServerMessage::Unknown
        );
        assert_eq!(
            A2uiServerMessage::parse(&json!({"version": "v0.10"})),
            A2uiServerMessage::Unknown
        );
        assert_eq!(
            A2uiServerMessage::parse_str("{ truncated"),
            A2uiServerMessage::Unknown
        );
    }

    #[test]
    fn jsonl_stream_skips_unparseable_keeps_complete() {
        let stream = r#"
            {"version":"v0.10","createSurface":{"surfaceId":"s","catalogId":"c"}}
            {"not":"a message"}
            {"version":"v0.10","deleteSurface":{"surfaceId":"s"}}
        "#;
        let messages = A2uiServerMessage::parse_stream(stream);
        assert_eq!(messages.len(), 2);
        assert!(matches!(
            messages[0],
            A2uiServerMessage::CreateSurface { .. }
        ));
        assert!(matches!(
            messages[1],
            A2uiServerMessage::DeleteSurface { .. }
        ));

        // A truncated trailing object: the first complete message still
        // applies, the partial one is dropped (no panic).
        let partial = r#"{"version":"v0.10","createSurface":{"surfaceId":"s","catalogId":"c"}}
            {"version":"v0.10","createSur"#;
        assert_eq!(A2uiServerMessage::parse_stream(partial).len(), 1);
    }
}
