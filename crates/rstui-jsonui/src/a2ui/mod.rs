//! Google **A2UI** v0.10 — the surface protocol, the 18-component basic
//! catalog, JSON-Pointer data binding, the action return channel, and
//! capability negotiation, projected onto one
//! [`UiNode`].
//!
//! # The protocol, as caller-owned state
//!
//! A2UI is a server→client *stream*: an agent emits
//! [`A2uiServerMessage`]s (`createSurface`, `updateComponents`,
//! `updateDataModel`, `deleteSurface`, plus `callFunction` /
//! `actionResponse`) that incrementally build and mutate a **surface** —
//! a flat component adjacency list bound to a per-surface JSON data
//! model. This module models the surface as one caller-owned value,
//! [`A2uiSurface`], in the rstui pure-projection idiom (ADR 0012):
//!
//! - the reducer feeds messages in with [`A2uiSurface::apply`]
//!   (or the [`parse`](A2uiServerMessage::parse)/
//!   [`parse_stream`](A2uiServerMessage::parse_stream) JSONL helpers);
//! - `view` calls [`A2uiSurface::project`] **every frame** to get a
//!   fresh [`UiNode`] — there is no retained widget
//!   tree, the document plus the [`DataModel`]
//!   *is* the state;
//! - interaction is resolved, not dispatched: a click maps via the
//!   [`HitMap`](crate::tree::HitMap) to a
//!   [`NodeId`], which
//!   [`A2uiSurface::action_for`] turns into an
//!   [`A2uiClientAction`] the reducer sends
//!   back as the `client_to_server.json` envelope (ADR 0012 §P1).
//!
//! Nothing renders until an `id == "root"` component arrives — messages
//! that reference an absent surface or a not-yet-defined `root` are
//! buffered into state and simply project to a placeholder until the
//! tree is complete (A2UI's progressive-rendering contract).
//!
//! # Totality
//!
//! Every layer here is panic-free for *any* input — truncated, streamed,
//! out-of-order, or hostile JSON degrades to a visible placeholder or an
//! ignored message, never a panic or a blanked screen. See
//! [`envelope`] (parsing), [`catalog`] (the 18 components +
//! `ChildList`), [`binding`] (the `Dynamic*`/`formatString` resolver +
//! the 14 functions), and [`actions`] (the return channel).

pub mod actions;
pub mod binding;
pub mod catalog;
pub mod envelope;

use serde_json::Value;

use crate::tree::{NodeId, UiNode};
use crate::value::DataModel;

pub use actions::A2uiClientAction;
pub use catalog::{Component, ComponentMap, InteractionState, SelectionState};
pub use envelope::A2uiServerMessage;

#[doc(inline)]
pub use crate::capability::{A2UI_CATALOG_ID, A2UI_VERSION, client_capabilities};

/// The `a2uiClientCapabilities` object this terminal client attaches to
/// its ACP/A2A transport metadata so the agent only emits components
/// from the catalog it can render. A thin, intent-named alias for
/// [`crate::capability::client_capabilities`] (the single source of
/// truth — this does not duplicate the descriptor).
#[must_use]
pub fn a2ui_client_capabilities() -> Value {
    client_capabilities()
}

/// One A2UI surface: its identity, the component adjacency list, the
/// bound data model, and the reducer-owned selection/focus state.
///
/// This is the caller-owned value the rstui app holds (ADR 0012). The
/// reducer mutates it via [`apply`](Self::apply) (and, for two-way
/// inputs / tab switches, by writing the [`DataModel`](Self::model_mut) /
/// [`selection`](Self::selection_mut) directly); `view` re-derives the
/// UI from it every frame with [`project`](Self::project).
#[derive(Debug, Clone, Default)]
pub struct A2uiSurface {
    surface_id: Option<String>,
    catalog_id: Option<String>,
    theme: Option<Value>,
    send_data_model: bool,
    components: ComponentMap,
    model: DataModel,
    selection: SelectionState,
    interaction: InteractionState,
    /// Set once a `deleteSurface` for this surface arrives; the surface
    /// then projects to nothing until a new `createSurface`.
    deleted: bool,
}

impl A2uiSurface {
    /// An empty surface (no `createSurface` seen yet).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The surface id, once a `createSurface` has been applied.
    #[must_use]
    pub fn surface_id(&self) -> Option<&str> {
        self.surface_id.as_deref()
    }

    /// The catalog id the agent targeted, once known.
    #[must_use]
    pub fn catalog_id(&self) -> Option<&str> {
        self.catalog_id.as_deref()
    }

    /// The initial `theme` parameters from `createSurface`, if any
    /// (kept verbatim for the host to interpret).
    #[must_use]
    pub fn theme(&self) -> Option<&Value> {
        self.theme.as_ref()
    }

    /// Whether the agent asked for the full data model to be echoed on
    /// every client→server message (`createSurface.sendDataModel`).
    #[must_use]
    pub fn send_data_model(&self) -> bool {
        self.send_data_model
    }

    /// The bound data model (for `sendDataModel` echo / inspection).
    #[must_use]
    pub fn model(&self) -> &DataModel {
        &self.model
    }

    /// Mutable data model — the reducer writes a two-way input's value
    /// back here (the pointer comes from an
    /// [`A2uiClientAction::SetData`]).
    pub fn model_mut(&mut self) -> &mut DataModel {
        &mut self.model
    }

    /// The reducer-owned tab/modal selection state.
    #[must_use]
    pub fn selection(&self) -> &SelectionState {
        &self.selection
    }

    /// Mutable selection state — the reducer switches the active tab /
    /// toggles a modal here in response to a resolved hit.
    pub fn selection_mut(&mut self) -> &mut SelectionState {
        &mut self.selection
    }

    /// Mutable focus state — the reducer sets the focused interactive
    /// component id (Tab/click focus) here.
    pub fn interaction_mut(&mut self) -> &mut InteractionState {
        &mut self.interaction
    }

    /// `true` once a `root` component exists, i.e. there is something to
    /// render (the progressive-rendering gate).
    #[must_use]
    pub fn is_ready(&self) -> bool {
        !self.deleted && self.components.contains_key("root")
    }

    /// Applies one server→client message, mutating the surface. A
    /// message for a different surface id, or any
    /// [`Unknown`](A2uiServerMessage::Unknown), is ignored (totality /
    /// progressive rendering). Re-applying `updateComponents` upserts by
    /// id; a different `component` type for an id recreates it.
    pub fn apply(&mut self, message: &A2uiServerMessage) {
        match message {
            A2uiServerMessage::CreateSurface {
                surface_id,
                catalog_id,
                theme,
                send_data_model,
            } => {
                // A fresh createSurface for this id resets the surface
                // (the spec forbids re-create without delete, but a
                // total client simply takes the latest as authoritative).
                self.surface_id = Some(surface_id.clone());
                self.catalog_id = Some(catalog_id.clone());
                self.theme = theme.clone();
                self.send_data_model = *send_data_model;
                self.components.clear();
                self.model = DataModel::new();
                self.selection = SelectionState::default();
                self.interaction = InteractionState::default();
                self.deleted = false;
            }
            A2uiServerMessage::UpdateComponents {
                surface_id,
                components,
            } => {
                if !self.addresses(surface_id) {
                    return;
                }
                for entry in components {
                    let Some(id) = entry.get("id").and_then(Value::as_str) else {
                        continue;
                    };
                    let incoming = Component::from_entry(entry);
                    match self.components.get_mut(id) {
                        // Same type ⇒ update props in place; different
                        // (non-empty) type ⇒ recreate.
                        Some(existing)
                            if incoming.kind.is_empty() || existing.kind == incoming.kind =>
                        {
                            existing.properties = incoming.properties;
                        }
                        _ => {
                            self.components.insert(id.to_owned(), incoming);
                        }
                    }
                }
            }
            A2uiServerMessage::UpdateDataModel {
                surface_id,
                path,
                value,
            } => {
                if !self.addresses(surface_id) {
                    return;
                }
                match value {
                    Some(value) => self.model.set(path, value.clone()),
                    None => self.model.remove(path),
                }
            }
            A2uiServerMessage::DeleteSurface { surface_id } => {
                if self.addresses(surface_id) {
                    self.deleted = true;
                    self.components.clear();
                }
            }
            // The terminal client exposes no remote-callable functions
            // and consumes responses via the data model's responsePath;
            // these are intentionally inert here (parsed for
            // completeness — see the envelope module docs).
            A2uiServerMessage::CallFunction { .. }
            | A2uiServerMessage::ActionResponse { .. }
            | A2uiServerMessage::Unknown => {}
        }
    }

    /// Parses and applies one server→client JSON message string.
    pub fn apply_str(&mut self, source: &str) {
        self.apply(&A2uiServerMessage::parse_str(source));
    }

    /// Parses and applies a JSONL / concatenated-JSON stream, in order,
    /// skipping any value that is not a well-formed v0.10 message.
    pub fn apply_stream(&mut self, source: &str) {
        for message in A2uiServerMessage::parse_stream(source) {
            self.apply(&message);
        }
    }

    /// Projects the surface to a fresh [`UiNode`]
    /// for this frame. Before a `root` component exists (or after a
    /// `deleteSurface`) this is an empty placeholder — the
    /// progressive-rendering gate, never a panic.
    #[must_use]
    pub fn project(&self) -> UiNode {
        if !self.is_ready() {
            return UiNode::Placeholder(String::new());
        }
        catalog::project(
            &self.components,
            &self.model,
            &self.selection,
            &self.interaction,
        )
    }

    /// Resolves a hit [`NodeId`] (from the
    /// [`HitMap`](crate::tree::HitMap) after a click) to the
    /// [`A2uiClientAction`] the reducer should
    /// act on:
    ///
    /// - a `Button`/`Modal`-trigger with an `action` → its resolved
    ///   server event or local `openUrl`;
    /// - a `TextField`/`CheckBox`/`ChoicePicker` option bound via
    ///   `{path}` → a [`SetData`](actions::A2uiClientAction::SetData)
    ///   carrying the write-back pointer and the toggled value;
    ///
    /// `None` if the id is unknown or carries no actionable binding
    /// (totality).
    #[must_use]
    pub fn action_for(&self, node_id: &str) -> Option<A2uiClientAction> {
        // A ChoicePicker option id is `"<pickerId>/<index>"`.
        if let Some((picker_id, index)) = node_id.rsplit_once('/') {
            if let Some(component) = self.components.get(picker_id) {
                if component.kind == "ChoicePicker" {
                    return self.choice_toggle(component, index);
                }
            }
        }
        let component = self.components.get(node_id)?;
        match component.kind.as_str() {
            "Button" => {
                A2uiClientAction::from_action(component.prop("action")?, node_id, &self.model, "")
            }
            "CheckBox" => {
                let pointer = bound_pointer(component.prop("value")?)?;
                let now = self
                    .model
                    .get(&pointer)
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Some(A2uiClientAction::SetData {
                    pointer,
                    value: Value::Bool(!now),
                })
            }
            _ => None,
        }
    }

    fn choice_toggle(&self, component: &Component, index: &str) -> Option<A2uiClientAction> {
        let options = component.prop("options")?.as_array()?;
        let option = options.get(index.parse::<usize>().ok()?)?;
        let value = option.get("value").and_then(Value::as_str)?.to_owned();
        let pointer = bound_pointer(component.prop("value")?)?;
        let mutually_exclusive =
            component.prop("variant").and_then(Value::as_str) != Some("multipleSelection");
        let mut current: Vec<String> = match self.model.get(&pointer) {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            Some(Value::String(text)) => vec![text.clone()],
            _ => Vec::new(),
        };
        if let Some(position) = current.iter().position(|chosen| chosen == &value) {
            current.remove(position);
        } else if mutually_exclusive {
            current = vec![value];
        } else {
            current.push(value);
        }
        Some(A2uiClientAction::SetData {
            pointer,
            value: Value::Array(current.into_iter().map(Value::String).collect()),
        })
    }

    /// The write-back pointer for a `TextField`/`DateTimeInput` id, if
    /// its `value` is a `{path}` binding — the reducer
    /// [`DataModel::set`](crate::value::DataModel::set)s the new text
    /// here when the field is edited (the widget never mutates).
    #[must_use]
    pub fn text_binding(&self, node_id: &str) -> Option<String> {
        let component = self.components.get(node_id)?;
        if matches!(component.kind.as_str(), "TextField" | "DateTimeInput") {
            return bound_pointer(component.prop("value")?);
        }
        None
    }

    fn addresses(&self, surface_id: &str) -> bool {
        self.surface_id.as_deref() == Some(surface_id)
    }

    /// The currently-resolved interactive ids and their write-back
    /// pointers, for a reducer that wants to wire focus/Tab traversal
    /// without re-deriving the tree (a thin accessor over the adjacency
    /// list — pure, no allocation of a retained tree).
    #[must_use]
    pub fn components(&self) -> &ComponentMap {
        &self.components
    }
}

/// The absolute pointer a `{path}` data binding addresses (`None` for a
/// literal or function-call prop — those are read-only, not write-back).
fn bound_pointer(value: &Value) -> Option<String> {
    let fields = value.as_object()?;
    if fields.len() == 1 {
        if let Some(Value::String(pointer)) = fields.get("path") {
            return Some(pointer.clone());
        }
    }
    None
}

/// Resolves a [`HitMap`](crate::tree::HitMap) hit position against a
/// surface in one call: the convenience the reducer uses on a click
/// (`surface.resolve_click(hits.at(pos)?)`).
#[must_use]
pub fn resolve_click(surface: &A2uiSurface, node_id: &NodeId) -> Option<A2uiClientAction> {
    surface.action_for(node_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn boot() -> A2uiSurface {
        let mut surface = A2uiSurface::new();
        surface.apply(&A2uiServerMessage::parse(&json!({
            "version": "v0.10",
            "createSurface": {"surfaceId": "s1", "catalogId": A2UI_CATALOG_ID}
        })));
        surface
    }

    #[test]
    fn buffers_until_root_then_projects() {
        let mut surface = boot();
        assert!(!surface.is_ready());
        assert!(matches!(surface.project(), UiNode::Placeholder(s) if s.is_empty()));

        surface.apply(&A2uiServerMessage::parse(&json!({
            "version": "v0.10",
            "updateComponents": {"surfaceId": "s1", "components": [
                {"id": "root", "component": "Column", "children": ["t"]},
                {"id": "t", "component": "Text", "text": {"path": "/title"}}
            ]}
        })));
        surface.apply(&A2uiServerMessage::parse(&json!({
            "version": "v0.10",
            "updateDataModel": {"surfaceId": "s1", "path": "/title", "value": "Live"}
        })));
        assert!(surface.is_ready());
        assert_eq!(surface.project().to_plain(), "Live");
    }

    #[test]
    fn wrong_surface_and_recreate_and_delete() {
        let mut surface = boot();
        // a message for another surface id is ignored
        surface.apply(&A2uiServerMessage::parse(&json!({
            "version": "v0.10",
            "updateComponents": {"surfaceId": "other", "components": [
                {"id": "root", "component": "Text", "text": "nope"}
            ]}
        })));
        assert!(!surface.is_ready());

        surface.apply(&A2uiServerMessage::parse(&json!({
            "version": "v0.10",
            "updateComponents": {"surfaceId": "s1", "components": [
                {"id": "root", "component": "Text", "text": "first"}
            ]}
        })));
        assert_eq!(surface.project().to_plain(), "first");
        // upsert same id, different type ⇒ recreate
        surface.apply(&A2uiServerMessage::parse(&json!({
            "version": "v0.10",
            "updateComponents": {"surfaceId": "s1", "components": [
                {"id": "root", "component": "Column", "children": []}
            ]}
        })));
        assert!(matches!(surface.project(), UiNode::Column { .. }));
        // delete ⇒ projects to nothing again
        surface.apply(&A2uiServerMessage::parse(&json!({
            "version": "v0.10", "deleteSurface": {"surfaceId": "s1"}
        })));
        assert!(!surface.is_ready());
    }

    #[test]
    fn action_for_resolves_button_checkbox_and_choice() {
        let mut surface = boot();
        surface.apply(&A2uiServerMessage::parse(&json!({
            "version": "v0.10",
            "updateComponents": {"surfaceId": "s1", "components": [
                {"id": "root", "component": "Column",
                 "children": ["go", "agree", "picker"]},
                {"id": "go_l", "component": "Text", "text": "Go"},
                {"id": "go", "component": "Button", "child": "go_l",
                 "action": {"event": {"name": "submit", "context": {"who": {"path": "/who"}}}}},
                {"id": "agree", "component": "CheckBox", "label": "Agree",
                 "value": {"path": "/agreed"}},
                {"id": "picker", "component": "ChoicePicker",
                 "variant": "mutuallyExclusive",
                 "options": [{"label": "A", "value": "a"}, {"label": "B", "value": "b"}],
                 "value": {"path": "/choice"}}
            ]}
        })));
        surface.apply(&A2uiServerMessage::parse(&json!({
            "version": "v0.10",
            "updateDataModel": {"surfaceId": "s1", "path": "/who", "value": "Ada"}
        })));

        // Button → resolved server event
        let action = surface.action_for("go").expect("button action");
        let wire = action
            .to_client_json("s1", "2026-05-18T12:00:00Z")
            .expect("event wire form");
        assert_eq!(wire["action"]["name"], "submit");
        assert_eq!(wire["action"]["context"]["who"], "Ada");

        // CheckBox → SetData toggling /agreed
        assert_eq!(
            surface.action_for("agree"),
            Some(A2uiClientAction::SetData {
                pointer: "/agreed".to_owned(),
                value: json!(true)
            })
        );

        // ChoicePicker option 1 → SetData replacing /choice (exclusive)
        assert_eq!(
            surface.action_for("picker/1"),
            Some(A2uiClientAction::SetData {
                pointer: "/choice".to_owned(),
                value: json!(["b"])
            })
        );

        // unknown id → None
        assert!(surface.action_for("ghost").is_none());
    }

    #[test]
    fn jsonl_stream_drives_a_surface_and_text_binding() {
        let mut surface = A2uiSurface::new();
        let stream = [
            r#"{"version":"v0.10","createSurface":{"surfaceId":"s","catalogId":""#,
            A2UI_CATALOG_ID,
            r#""}}
{"version":"v0.10","updateComponents":{"surfaceId":"s","components":[
  {"id":"root","component":"TextField","label":"Name","value":{"path":"/n"}}]}}
{"version":"v0.10","updateDataModel":{"surfaceId":"s","path":"/n","value":"Bo"}}"#,
        ]
        .concat();
        surface.apply_stream(&stream);
        assert!(surface.is_ready());
        assert_eq!(surface.project().to_plain(), "");
        if let UiNode::TextField { value, label, .. } = surface.project() {
            assert_eq!(value, "Bo");
            assert_eq!(label, "Name");
        } else {
            panic!("expected a TextField");
        }
        // the reducer learns where to write the edited text back
        assert_eq!(surface.text_binding("root"), Some("/n".to_owned()));

        // capability re-export is the single descriptor, not a copy
        assert_eq!(a2ui_client_capabilities(), client_capabilities());
    }
}
