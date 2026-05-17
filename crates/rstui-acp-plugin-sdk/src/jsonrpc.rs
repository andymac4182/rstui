//! Minimal JSON-RPC 2.0 envelope — the same wire shape ACP and MCP use.
//!
//! One [`Message`] models requests, responses, and notifications by the
//! JSON-RPC presence rules (`id`+`method` = request, `id`+`result`/`error` =
//! response, `method` without `id` = notification). It is transport-agnostic;
//! [`crate::transport`] frames it (newline-delimited over stdio or a
//! websocket text frame).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcError {
    /// Numeric error code.
    pub code: i64,
    /// Human-readable message.
    pub message: String,
    /// Optional structured detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// A single JSON-RPC 2.0 message (request | response | notification).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Correlation id (present on requests + responses, absent on
    /// notifications).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    /// Method name (present on requests + notifications).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Call parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Successful response payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error response payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// How a [`Message`] is classified by the JSON-RPC presence rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `id` + `method`: expects a response.
    Request,
    /// `method`, no `id`: fire-and-forget.
    Notification,
    /// `id` + `result`/`error`: a reply to a request.
    Response,
}

impl Message {
    /// A request: `id` + `method` (+ optional `params`).
    #[must_use]
    pub fn request(id: Value, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id: Some(id),
            method: Some(method.into()),
            params,
            result: None,
            error: None,
        }
    }

    /// A notification: `method` (+ optional `params`), no `id`.
    #[must_use]
    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id: None,
            method: Some(method.into()),
            params,
            result: None,
            error: None,
        }
    }

    /// A successful response carrying `result`.
    #[must_use]
    pub fn response(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id: Some(id),
            method: None,
            params: None,
            result: Some(result),
            error: None,
        }
    }

    /// An error response.
    #[must_use]
    pub fn error_response(id: Value, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id: Some(id),
            method: None,
            params: None,
            result: None,
            error: Some(error),
        }
    }

    /// Classifies this message.
    #[must_use]
    pub fn kind(&self) -> Kind {
        if self.method.is_some() {
            if self.id.is_some() {
                Kind::Request
            } else {
                Kind::Notification
            }
        } else {
            Kind::Response
        }
    }

    /// Serializes to a single newline-terminated JSON-RPC line.
    #[must_use]
    pub fn encode_line(&self) -> String {
        let mut s = serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"jsonrpc":"2.0","method":"$invalid"}"#.to_owned());
        s.push('\n');
        s
    }

    /// Parses one JSON-RPC line.
    ///
    /// # Errors
    ///
    /// Returns the `serde_json` message on malformed input.
    pub fn decode_line(line: &str) -> Result<Self, String> {
        serde_json::from_str(line).map_err(|e| e.to_string())
    }
}
