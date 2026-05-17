//! The A2UI client→server action return channel.
//!
//! # Interaction is not a callback
//!
//! Per ADR 0012 §P1 a click is resolved, not dispatched: the renderer
//! records every interactive node's [`Rect`](rstui_core::Rect) into a
//! [`HitMap`](crate::tree::HitMap); the reducer maps the hit
//! [`NodeId`](crate::tree::NodeId) back to intent with
//! [`A2uiSurface::action_for`](super::A2uiSurface::action_for), which
//! returns one of these [`A2uiClientAction`]s. The reducer then turns it
//! into the wire message — the widget never performs I/O.
//!
//! # The two action shapes
//!
//! `common_types.json`'s `Action` is either a **server event**
//! (`{event:{name, context, …}}`) or a **local function call**
//! (`{functionCall:{call:"openUrl", args}}`):
//!
//! - A server event becomes [`A2uiClientAction::Event`]. Its `context`
//!   data-bindings are resolved against the surface's local
//!   [`DataModel`] at build time, then
//!   [`to_client_json`](A2uiClientAction::to_client_json) wraps it as
//!   the `client_to_server.json` `{"version":"v0.10","action":{name,
//!   surfaceId, sourceComponentId, timestamp, context}}` envelope. The
//!   `timestamp` is a caller-supplied ISO-8601 string so the builder
//!   stays pure and unit-testable (no clock dependency).
//! - A local `openUrl` becomes [`A2uiClientAction::OpenUrl`] — the
//!   reducer opens it; nothing goes to the server (`returnType: void`).
//!
//! A two-way input (`TextField`/`CheckBox`/etc.) produces
//! [`A2uiClientAction::SetData`]: the resolved write-back pointer and the
//! toggled/edited value the reducer should
//! [`DataModel::set`](crate::value::DataModel::set) — the widget stays a
//! pure projection.

use serde_json::{Map, Value};

use crate::capability::A2UI_VERSION;
use crate::value::DataModel;

use super::binding::resolve;

/// The reducer-consumed intent a resolved interaction produces.
#[derive(Debug, Clone, PartialEq)]
pub enum A2uiClientAction {
    /// A server-side event to send. Build the wire message with
    /// [`to_client_json`](Self::to_client_json).
    Event {
        /// The event name (`action.event.name`).
        name: String,
        /// The component that triggered it.
        source_component_id: String,
        /// The already-resolved context object.
        context: Value,
        /// The agent expects an `actionResponse`.
        want_response: bool,
        /// Optional pointer to store that response in the local model.
        response_path: Option<String>,
    },
    /// A local `openUrl(url)` — open it; nothing goes to the server.
    OpenUrl(String),
    /// A two-way input write-back: `DataModel::set(pointer, value)`.
    SetData {
        /// The absolute JSON Pointer to write.
        pointer: String,
        /// The value to store.
        value: Value,
    },
}

impl A2uiClientAction {
    /// Builds an [`A2uiClientAction`] from a component's `action` value,
    /// resolving an event's `context` bindings against `model` at
    /// `scope`. `source_component_id` is the triggering component's id.
    /// Returns `None` if `action` is absent or unrecognised (totality).
    #[must_use]
    pub fn from_action(
        action: &Value,
        source_component_id: &str,
        model: &DataModel,
        scope: &str,
    ) -> Option<Self> {
        let fields = action.as_object()?;
        if let Some(event) = fields.get("event").and_then(Value::as_object) {
            let name = event.get("name").and_then(Value::as_str)?.to_owned();
            let context = match event.get("context").and_then(Value::as_object) {
                Some(raw) => {
                    let mut resolved = Map::with_capacity(raw.len());
                    for (key, value) in raw {
                        resolved.insert(key.clone(), resolve(value, model, scope));
                    }
                    Value::Object(resolved)
                }
                None => Value::Object(Map::new()),
            };
            return Some(Self::Event {
                name,
                source_component_id: source_component_id.to_owned(),
                context,
                want_response: event
                    .get("wantResponse")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                response_path: event
                    .get("responsePath")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
        }
        if let Some(call) = fields.get("functionCall").and_then(Value::as_object) {
            if call.get("call").and_then(Value::as_str) == Some("openUrl") {
                let url = call
                    .get("args")
                    .and_then(Value::as_object)
                    .and_then(|args| args.get("url"))
                    .map(|raw| resolve(raw, model, scope))
                    .as_ref()
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                return Some(Self::OpenUrl(url));
            }
        }
        None
    }

    /// Serialises an [`Event`](Self::Event) into the
    /// `client_to_server.json` envelope. `timestamp` is a caller-supplied
    /// ISO-8601 string (kept a parameter so this is pure/testable).
    /// Non-`Event` variants have no wire form and yield `None`.
    #[must_use]
    pub fn to_client_json(&self, surface_id: &str, timestamp: &str) -> Option<Value> {
        let Self::Event {
            name,
            source_component_id,
            context,
            want_response,
            ..
        } = self
        else {
            return None;
        };
        let mut action = Map::new();
        action.insert("name".to_owned(), Value::String(name.clone()));
        action.insert("surfaceId".to_owned(), Value::String(surface_id.to_owned()));
        action.insert(
            "sourceComponentId".to_owned(),
            Value::String(source_component_id.clone()),
        );
        action.insert("timestamp".to_owned(), Value::String(timestamp.to_owned()));
        action.insert("context".to_owned(), context.clone());
        if *want_response {
            action.insert("wantResponse".to_owned(), Value::Bool(true));
        }
        let mut envelope = Map::new();
        envelope.insert("version".to_owned(), Value::String(A2UI_VERSION.to_owned()));
        envelope.insert("action".to_owned(), Value::Object(action));
        Some(Value::Object(envelope))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn server_event_resolves_context_and_serialises() {
        let model = DataModel::from_root(json!({ "cart": { "id": "c-9" } }));
        let action = json!({
            "event": {
                "name": "checkout",
                "context": { "cartId": {"path": "/cart/id"}, "qty": 2 }
            }
        });
        let resolved =
            A2uiClientAction::from_action(&action, "buy_btn", &model, "").expect("an event");
        let wire = resolved
            .to_client_json("surf-1", "2026-05-18T00:00:00Z")
            .expect("event has a wire form");
        assert_eq!(wire["version"], "v0.10");
        assert_eq!(wire["action"]["name"], "checkout");
        assert_eq!(wire["action"]["surfaceId"], "surf-1");
        assert_eq!(wire["action"]["sourceComponentId"], "buy_btn");
        assert_eq!(wire["action"]["timestamp"], "2026-05-18T00:00:00Z");
        // the {path} binding was resolved against the local model
        assert_eq!(wire["action"]["context"]["cartId"], "c-9");
        assert_eq!(wire["action"]["context"]["qty"], 2);
    }

    #[test]
    fn local_open_url_has_no_wire_form() {
        let model = DataModel::new();
        let action =
            json!({ "functionCall": { "call": "openUrl", "args": { "url": "https://x" } } });
        let resolved = A2uiClientAction::from_action(&action, "link", &model, "").unwrap();
        assert_eq!(resolved, A2uiClientAction::OpenUrl("https://x".to_owned()));
        assert!(resolved.to_client_json("s", "t").is_none());
    }

    #[test]
    fn totality_missing_or_unknown_action() {
        let model = DataModel::new();
        assert!(A2uiClientAction::from_action(&json!({}), "x", &model, "").is_none());
        assert!(A2uiClientAction::from_action(&json!("nope"), "x", &model, "").is_none());
        assert!(
            A2uiClientAction::from_action(
                &json!({"functionCall": {"call": "unknownFn"}}),
                "x",
                &model,
                ""
            )
            .is_none()
        );
    }
}
