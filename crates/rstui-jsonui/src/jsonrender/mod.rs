//! Vercel **json-render** — the flat `{ root, elements, state }` element
//! map, the twelve-step `$`-expression resolver, the eight directives,
//! the RFC-6902 patch-stream compiler, the twenty-six standard
//! components, and the host-extensible directive registry, all compiling
//! to the one [`UiNode`] projection target.
//!
//! # The format
//!
//! json-render optimises its document for LLM generation: a single flat
//! [`elements`](spec::Spec::elements) map keyed by id, each element
//! referencing children **by key**. An agent streams the document as a
//! JSONL [`JsonPatch`] stream (`{op,path,value?}` per line), built up by
//! the [`SpecStreamCompiler`]; a chat reply that interleaves prose with a
//! ```` ```spec ```` fence is split by the [`MixedStreamParser`]. Every
//! prop value is run through the twelve-step
//! [`resolve_prop_value`] (`$state`/`$item`/`$index`/`$bindState`/
//! `$bindItem`/`$cond`/`$computed`/`$template` + directives), `visible`
//! through [`evaluate_visibility`].
//!
//! # Pure projection, no retained tree (ADR 0012)
//!
//! The caller owns a [`JsonRenderDoc`]: the parsed [`Spec`] plus a
//! [`DataModel`]. [`JsonRenderDoc::view`]
//! re-projects a fresh [`UiNode`] every frame;
//! [`JsonRenderDoc::ingest`] feeds a stream chunk; interaction is a
//! [`HitMap`](crate::tree::HitMap) accessor turned into a
//! `Vec<`[`ResolvedAction`]`>` by [`JsonRenderDoc::on`], which the
//! **reducer** applies via [`apply_action`] (the widget never mutates —
//! pure projection, never a callback, ADR 0012 §P1).
//!
//! # Totality
//!
//! Parsing and projection are panic-free for **any** input — this is the
//! LLM-streaming use case, so truncated/garbled/over-braced JSON degrades
//! gracefully (skip the bad line, render what landed, `Placeholder` an
//! unknown component), never a panic or a blanked screen.

pub mod actions;
pub mod components;
pub mod directives;
pub mod expr;
pub mod patch;
pub mod spec;

use serde_json::{Value, json};

pub use actions::{ActionBinding, ActionEffect, ResolvedAction, apply_action, resolve_action};
pub use components::{Loading, project};
pub use directives::{
    BUILT_IN_PROP_KEYS, Directive, DirectiveError, DirectiveRegistry, builtin_directives,
};
pub use expr::{
    ComputedFn, RepeatScope, ResolveScope, coerce_to_string, evaluate_visibility,
    resolve_action_param, resolve_bindings, resolve_element_props, resolve_prop_value,
};
pub use patch::{
    JsonPatch, MixedItem, MixedStreamParser, SpecStreamCompiler, apply_patch, parse_patch_line,
};
pub use spec::{
    RepeatSpec, Spec, UiElement, VisibilityCondition, auto_fix_spec, bindings_for_event,
    nested_to_flat, spec_from_value, spec_to_value,
};

use crate::capability::JSON_RENDER_CATALOG_ID;
use crate::tree::{NodeId, UiNode};
use crate::value::DataModel;

/// A caller-owned json-render document: the parsed [`Spec`], the live
/// [`DataModel`], the `$computed` function map, and the directive
/// registry. The reducer mutates this; [`view`](JsonRenderDoc::view)
/// re-projects from it every frame (ADR 0012 — no retained tree).
pub struct JsonRenderDoc {
    spec: Spec,
    model: DataModel,
    functions: std::collections::BTreeMap<String, ComputedFn>,
    directives: DirectiveRegistry,
    stream: SpecStreamCompiler,
    next_id: u64,
    loading: bool,
}

impl std::fmt::Debug for JsonRenderDoc {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JsonRenderDoc")
            .field("spec", &self.spec)
            .field("model", &self.model)
            .field("loading", &self.loading)
            .finish_non_exhaustive()
    }
}

impl Default for JsonRenderDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonRenderDoc {
    /// An empty document (no spec, empty state) — the streaming start
    /// state. Feed it with [`ingest`](JsonRenderDoc::ingest).
    #[must_use]
    pub fn new() -> Self {
        Self {
            spec: Spec::new(),
            model: DataModel::new(),
            functions: std::collections::BTreeMap::new(),
            directives: DirectiveRegistry::with_builtins(),
            stream: SpecStreamCompiler::new(),
            next_id: 0,
            loading: true,
        }
    }

    /// A document seeded from a complete [`Spec`] (its `state` becomes the
    /// initial data model). `loading` is cleared — a full spec is not
    /// mid-stream, so missing children render as visible placeholders.
    #[must_use]
    pub fn from_spec(spec: Spec) -> Self {
        let model = spec
            .state
            .clone()
            .map_or_else(DataModel::new, DataModel::from_root);
        Self {
            stream: SpecStreamCompiler::with_initial(&spec),
            spec,
            model,
            functions: std::collections::BTreeMap::new(),
            directives: DirectiveRegistry::with_builtins(),
            next_id: 0,
            loading: false,
        }
    }

    /// Parses and seeds from a flat-spec JSON value (the
    /// `{ type:"flat" }` stream part). Total — a malformed value yields
    /// an empty doc.
    #[must_use]
    pub fn from_flat_value(value: &Value) -> Self {
        Self::from_spec(spec_from_value(value))
    }

    /// Parses and seeds from a **nested** spec JSON value (the
    /// `{ type:"nested" }` stream part) via [`nested_to_flat`].
    #[must_use]
    pub fn from_nested_value(value: &Value) -> Self {
        Self::from_spec(nested_to_flat(value))
    }

    /// Registers a host `$computed` function (overwrites a same-named
    /// one). Builder-style for setup ergonomics.
    #[must_use]
    pub fn with_function(mut self, name: impl Into<String>, function: ComputedFn) -> Self {
        self.functions.insert(name.into(), function);
        self
    }

    /// Registers a custom [`Directive`]. A bad/colliding name is ignored
    /// (the `defineDirective` guard — degrade, don't panic).
    pub fn register_directive(&mut self, directive: Directive) {
        let _ = self.directives.register(directive);
    }

    /// The parsed spec (debug / inspection).
    #[must_use]
    pub fn spec(&self) -> &Spec {
        &self.spec
    }

    /// The live data model (debug / `sendDataModel`).
    #[must_use]
    pub fn model(&self) -> &DataModel {
        &self.model
    }

    /// Mutable access to the data model **for the reducer only** — this
    /// is where `$bindState`/`$bindItem` write-backs land
    /// (`model_mut().set(pointer, value)`), keeping the widget a pure
    /// projection (ADR 0012 §P1).
    pub fn model_mut(&mut self) -> &mut DataModel {
        &mut self.model
    }

    /// Whether the spec is still streaming (missing children render
    /// blank, not as `[Missing]` placeholders, while `true`).
    #[must_use]
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Marks the stream complete (flushes the last buffered line). Call
    /// at end-of-response so missing children become visible placeholders
    /// rather than silently blank.
    pub fn finish_stream(&mut self) {
        self.stream.finish();
        self.spec = self.stream.spec();
        self.sync_state_from_spec();
        self.loading = false;
    }

    /// Feeds a raw stream chunk (chunked JSONL patch text); re-derives the
    /// spec. Returns the patches applied by this chunk (so a caller can
    /// re-`view` only when something changed). Total.
    pub fn ingest(&mut self, chunk: &str) -> Vec<JsonPatch> {
        let applied = self.stream.push(chunk);
        if !applied.is_empty() {
            self.spec = self.stream.spec();
            self.sync_state_from_spec();
        }
        applied
    }

    /// Applies one already-parsed [`JsonPatch`] to the spec (the
    /// `{ type:"patch" }` stream part path).
    pub fn apply_spec_patch(&mut self, patch: &JsonPatch) {
        let mut document = DataModel::from_root(spec_to_value(&self.spec));
        apply_patch(&mut document, patch);
        self.spec = spec_from_value(document.root());
        self.sync_state_from_spec();
    }

    /// Seeds the data model from `spec.state` the first time it appears
    /// (the model is the live store after that — patches mutate the spec
    /// shape, the reducer mutates state).
    fn sync_state_from_spec(&mut self) {
        if self
            .model
            .root()
            .as_object()
            .is_none_or(serde_json::Map::is_empty)
        {
            if let Some(state) = &self.spec.state {
                self.model = DataModel::from_root(state.clone());
            }
        }
    }

    /// Builds a [`ResolveScope`] borrowing this doc's model, functions,
    /// and directives — the top-level (no repeat) scope.
    fn scope(&self) -> ResolveScope<'_> {
        ResolveScope::new(&self.model, &self.functions, &self.directives)
    }

    /// Projects the current spec+state to a fresh [`UiNode`] (call every
    /// frame; the rstui pure-projection model — no retained tree).
    #[must_use]
    pub fn view(&self) -> UiNode {
        project(&self.spec, &self.scope(), Loading(self.loading))
    }

    /// Resolves a hit-tested node id + event name to the resolved actions
    /// the **reducer** should apply (params resolved against a live
    /// snapshot). Empty when the node has no binding for that event.
    ///
    /// The id may be a sub-event form the projection emits
    /// (`<key>#confirm`, `<key>#deny`, `<key>#select:<value>`); those map
    /// to the element's `confirm`/`deny`/`change` bindings respectively.
    #[must_use]
    pub fn on(&self, node_id: &NodeId, event: &str) -> Vec<ResolvedAction> {
        let (element_key, derived_event) = split_sub_event(node_id, event);
        let Some(element) = self.spec.element(element_key) else {
            return Vec::new();
        };
        let scope = self.scope();
        bindings_for_event(&element.on, derived_event)
            .iter()
            .map(|binding| resolve_action(binding, &scope))
            .collect()
    }

    /// Applies one resolved action to the data model (the reducer's
    /// single mutation point). Returns the host-observable
    /// [`ActionEffect`] (a `log` message, an `exit` request, or an
    /// `Unhandled` action a host map should service).
    pub fn apply(&mut self, action: &ResolvedAction) -> ActionEffect {
        apply_action(action, &mut self.model, &mut self.next_id)
    }

    /// Convenience: resolve **and** apply every action bound to
    /// `(node_id, event)`, returning their effects. The typical reducer
    /// path for a click/submit (still no callback — the caller invokes
    /// this from its own `update`).
    pub fn dispatch(&mut self, node_id: &NodeId, event: &str) -> Vec<ActionEffect> {
        let resolved = self.on(node_id, event);
        resolved.iter().map(|action| self.apply(action)).collect()
    }

    /// Writes a `$bindState`/`$bindItem` value back through the resolved
    /// pointer (a projected [`UiNode::TextField`]/[`UiNode::Checkbox`]
    /// carries that pointer as its `id`). The reducer calls this on an
    /// edit/toggle — the widget never does (pure projection).
    pub fn write_binding(&mut self, pointer: &str, value: Value) {
        self.model.set(pointer, value);
    }
}

/// Splits a projected sub-event id (`<key>#confirm`) into the element key
/// and the upstream event name it maps to. A `select:<value>` form keeps
/// the value out of band (the caller reads it from the id) and maps to
/// `change`.
fn split_sub_event<'id>(node_id: &'id str, event: &'id str) -> (&'id str, &'id str) {
    if let Some((key, suffix)) = node_id.split_once('#') {
        let mapped = if suffix == "confirm" {
            "confirm"
        } else if suffix == "deny" {
            "deny"
        } else if suffix.starts_with("select:") {
            "change"
        } else {
            event
        };
        return (key, mapped);
    }
    (node_id, event)
}

/// A compact, agent-readable description of the json-render surface this
/// client renders — the catalog id, the 26 standard components, and the
/// directive names — for injection into a system prompt (json-render has
/// no wire handshake; the host hands the model the catalog).
#[must_use]
pub fn render_capability_summary() -> Value {
    let registry = DirectiveRegistry::with_builtins();
    json!({
        "jsonRender": {
            "catalogId": JSON_RENDER_CATALOG_ID,
            "format": "flat-element-map (root/elements/state) + RFC6902 JSONL patch stream",
            "components": [
                "Box", "Text", "Newline", "Spacer", "Heading", "Divider", "Badge",
                "Spinner", "ProgressBar", "Sparkline", "BarChart", "Table", "List",
                "ListItem", "Card", "KeyValue", "Link", "StatusLine", "Metric",
                "Callout", "Timeline", "TextInput", "Select", "MultiSelect",
                "ConfirmInput", "Tabs", "Markdown",
            ],
            "directives": registry.names(),
            "actions": ["setState", "pushState", "removeState", "log", "exit"],
            "note": "Unknown components and malformed/truncated streaming JSON \
                     degrade to a visible placeholder; never a panic.",
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn streamed_doc_projects_progressively_then_finalises() {
        let mut doc = JsonRenderDoc::new();
        // First chunk: only the root + one child key, child not yet sent.
        doc.ingest("{\"op\":\"add\",\"path\":\"/root\",\"value\":\"r\"}\n");
        doc.ingest(
            "{\"op\":\"add\",\"path\":\"/elements/r\",\"value\":{\"type\":\"Box\",\"children\":[\"t\"]}}\n",
        );
        // While loading, the missing child renders blank (not [Missing]).
        assert!(doc.is_loading());
        assert!(!doc.view().to_plain().contains("Missing"));
        // Deliver the child, then finish.
        doc.ingest(
            "{\"op\":\"add\",\"path\":\"/elements/t\",\"value\":{\"type\":\"Text\",\"props\":{\"text\":\"Hello\"}}}\n",
        );
        doc.finish_stream();
        assert!(!doc.is_loading());
        assert_eq!(doc.view().to_plain(), "Hello");
    }

    #[test]
    fn action_routing_resolves_and_mutates_via_reducer() {
        let spec = spec_from_value(&json!({
            "root": "btn",
            "elements": {
                "btn": {
                    "type": "ConfirmInput",
                    "props": { "message": "Delete?" },
                    "on": {
                        "confirm": { "action": "setState", "params": { "statePath": "/done", "value": true } },
                    },
                },
            },
            "state": { "done": false },
        }));
        let mut doc = JsonRenderDoc::from_spec(spec);
        // The projection emits `btn#confirm` for the Yes button.
        let resolved = doc.on(&"btn#confirm".to_owned(), "press");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].action, "setState");
        let effects = doc.dispatch(&"btn#confirm".to_owned(), "press");
        assert_eq!(effects, vec![ActionEffect::StateChanged]);
        assert_eq!(doc.model().get("/done"), Some(&json!(true)));
    }

    #[test]
    fn two_way_binding_write_back_through_reducer() {
        let spec = spec_from_value(&json!({
            "root": "f",
            "elements": {
                "f": { "type": "TextInput", "props": { "label": "Name", "value": { "$bindState": "/name" } } },
            },
            "state": { "name": "" },
        }));
        let mut doc = JsonRenderDoc::from_spec(spec);
        // The projected TextField's id is the write-back pointer.
        let pointer = match doc.view() {
            UiNode::TextField { id, .. } => id,
            other => panic!("expected TextField, got {other:?}"),
        };
        assert_eq!(pointer, "/name");
        doc.write_binding(&pointer, json!("Ada"));
        // Re-projecting reflects the new state (pure projection).
        match doc.view() {
            UiNode::TextField { value, .. } => assert_eq!(value, "Ada"),
            other => panic!("expected TextField, got {other:?}"),
        }
    }

    #[test]
    fn capability_summary_lists_components_and_directives() {
        let summary = render_capability_summary();
        let components = summary["jsonRender"]["components"].as_array().unwrap();
        assert_eq!(components.len(), 27);
        assert!(components.iter().any(|component| component == "Markdown"));
        let directives = summary["jsonRender"]["directives"].as_array().unwrap();
        assert!(directives.iter().any(|directive| directive == "$format"));
        assert_eq!(directives.len(), 8);
    }

    #[test]
    fn totality_hostile_stream_never_panics() {
        let mut doc = JsonRenderDoc::new();
        doc.ingest("garbage not json\n");
        doc.ingest("{\"op\":\"add\"}\n"); // missing path
        doc.ingest("{\"op\":\"add\",\"path\":\"/root\",\"value\":\"z\"}}}}\n"); // over-braced
        doc.finish_stream();
        // Recovered the over-braced root; renders an empty placeholder
        // because `z` was never delivered — no panic, no blank crash.
        let _ = doc.view();
        assert_eq!(doc.spec().root, "z");
    }
}
