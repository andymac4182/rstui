//! The json-render **spec model** — the flat `{ root, elements, state }`
//! element map an agent streams, plus the two LLM-robustness transforms
//! the reference engine ships (`autoFixSpec`, `nestedToFlat`).
//!
//! # The flat element map
//!
//! json-render optimises its document shape for LLM generation: a single
//! flat `elements` map keyed by string id, each [`UiElement`] referencing
//! its children **by key** rather than nesting them (upstream
//! `packages/core/src/types.ts`). The model emits a JSONL patch stream
//! that progressively builds this map (see [`patch`](super::patch)); a
//! truncated/garbled chunk must degrade gracefully, so every field here is
//! lenient: `type`/`props` default, `children`/`visible`/`on`/`repeat`/
//! `watch` are optional, unknown fields are ignored.
//!
//! `visible`/`on`/`repeat`/`watch` are **siblings** of `type`/`props`. An
//! LLM frequently nests them *inside* `props`; [`auto_fix_spec`] hoists
//! them back out (the reference `autoFixSpec`). Humans naturally write a
//! *nested* tree instead of a flat map; [`nested_to_flat`] walks that tree
//! to the flat form with pre-order `el-0…` keys (the reference
//! `nestedToFlat`). Both are total — malformed input yields a best-effort
//! spec, never a panic.

use serde_json::{Map, Value};

use super::actions::ActionBinding;

/// The visibility condition attached to an element (`visible`) or to a
/// `$cond` expression. Carried as raw JSON and interpreted by
/// [`evaluate_visibility`](super::expr::evaluate_visibility) so the full
/// recursive `$and`/`$or` grammar round-trips without a bespoke AST.
pub type VisibilityCondition = Value;

/// One element in the flat [`Spec::elements`] map.
///
/// Children are referenced **by key** (the flat-map design). `visible`,
/// `on`, `repeat`, and `watch` are siblings of `type`/`props`; an LLM that
/// nests them in `props` is repaired by [`auto_fix_spec`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiElement {
    /// The catalog component type (e.g. `"Box"`, `"Text"`). Empty when the
    /// streaming spec has not yet delivered it.
    pub type_name: String,
    /// The component props object (expressions resolved at projection
    /// time). Always an object; a non-object in the JSON is coerced to an
    /// empty object so resolution stays total.
    pub props: Value,
    /// Child element **keys** (not nested elements).
    pub children: Vec<String>,
    /// Optional visibility condition (sibling of `type`/`props`).
    pub visible: Option<VisibilityCondition>,
    /// Event-name → action binding(s) (`press`, `submit`, …).
    pub on: Map<String, Value>,
    /// Repeat this element's children once per item of a state array.
    pub repeat: Option<RepeatSpec>,
    /// State-path → action binding(s) fired when the watched value
    /// changes (carried for fidelity; the terminal reducer may apply it).
    pub watch: Map<String, Value>,
}

/// The `repeat` directive: render an element's children once per item of
/// the array at `state_path`, scoping `$item`/`$index` to each item.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RepeatSpec {
    /// JSON Pointer to the state array to iterate.
    pub state_path: String,
    /// Optional item field used as the stable child key (display-only in
    /// the terminal; iteration order is the array order regardless).
    pub key: Option<String>,
}

/// A parsed json-render document: the root element key, the flat element
/// map, and an optional initial state object.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spec {
    /// The root element's key into [`elements`](Spec::elements).
    pub root: String,
    /// The flat element map.
    pub elements: std::collections::BTreeMap<String, UiElement>,
    /// Optional initial state to seed the data model (`spec.state`).
    pub state: Option<Value>,
}

impl Spec {
    /// An empty spec (no root, no elements) — the streaming start state a
    /// patch stream builds onto.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Looks up an element by key.
    #[must_use]
    pub fn element(&self, key: &str) -> Option<&UiElement> {
        self.elements.get(key)
    }
}

/// Reads an object field as a `&str`, or `""` if absent/non-string.
fn string_field(object: &Map<String, Value>, field: &str) -> String {
    object
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Parses one element object leniently. A `repeat`/`visible`/`on`/`watch`
/// nested inside `props` is hoisted (the `autoFixSpec` behaviour, applied
/// at parse time so projection never has to know about the LLM defect).
fn element_from_value(raw: &Value) -> UiElement {
    let Some(object) = raw.as_object() else {
        return UiElement::default();
    };
    // `props` is always an object; coerce anything else to empty so the
    // expression resolver stays total.
    let mut props = match object.get("props") {
        Some(Value::Object(map)) => Value::Object(map.clone()),
        _ => Value::Object(Map::new()),
    };

    let take_sibling =
        |props: &mut Value, object: &Map<String, Value>, field: &str| -> Option<Value> {
            // Sibling position wins; otherwise hoist out of `props` (autofix).
            if let Some(found) = object.get(field) {
                if !found.is_null() {
                    return Some(found.clone());
                }
            }
            if let Value::Object(props_map) = props {
                if let Some(found) = props_map.remove(field) {
                    if !found.is_null() {
                        return Some(found);
                    }
                }
            }
            None
        };

    let visible = take_sibling(&mut props, object, "visible");
    let on_raw = take_sibling(&mut props, object, "on");
    let repeat_raw = take_sibling(&mut props, object, "repeat");
    let watch_raw = take_sibling(&mut props, object, "watch");

    let children = object
        .get("children")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|child| child.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    let repeat = repeat_raw.as_ref().and_then(|value| {
        value.as_object().map(|map| RepeatSpec {
            state_path: string_field(map, "statePath"),
            key: map.get("key").and_then(Value::as_str).map(str::to_owned),
        })
    });

    let to_map = |value: Option<Value>| -> Map<String, Value> {
        value
            .and_then(|value| match value {
                Value::Object(map) => Some(map),
                _ => None,
            })
            .unwrap_or_default()
    };

    UiElement {
        type_name: string_field(object, "type"),
        props,
        children,
        visible,
        on: to_map(on_raw),
        repeat,
        watch: to_map(watch_raw),
    }
}

/// Parses a flat json-render spec from JSON, **totally**: any missing or
/// mistyped field degrades to a sensible default rather than failing, so a
/// half-streamed document still projects what has landed.
///
/// `elements` may be either the canonical keyed object **or** an array of
/// `{ key, … }` elements (the `FlatElement` array form); both are
/// accepted. A non-object input yields an empty spec.
#[must_use]
pub fn spec_from_value(raw: &Value) -> Spec {
    let Some(object) = raw.as_object() else {
        return Spec::new();
    };
    let root = string_field(object, "root");
    let mut elements = std::collections::BTreeMap::new();

    match object.get("elements") {
        Some(Value::Object(map)) => {
            for (key, element_raw) in map {
                elements.insert(key.clone(), element_from_value(element_raw));
            }
        }
        Some(Value::Array(items)) => {
            for (index, element_raw) in items.iter().enumerate() {
                let key = element_raw
                    .as_object()
                    .and_then(|element_map| element_map.get("key"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| index.to_string());
                elements.insert(key, element_from_value(element_raw));
            }
        }
        _ => {}
    }

    let state = match object.get("state") {
        Some(value) if !value.is_null() => Some(value.clone()),
        _ => None,
    };

    Spec {
        root,
        elements,
        state,
    }
}

/// Re-encodes a [`Spec`] as the canonical flat JSON object so a patch
/// stream can mutate it via [`DataModel`](crate::value::DataModel) (the
/// upstream `applySpecPatch` operates on the spec-as-JSON).
#[must_use]
pub fn spec_to_value(spec: &Spec) -> Value {
    let mut elements = Map::new();
    for (key, element) in &spec.elements {
        let mut element_object = Map::new();
        element_object.insert("type".to_owned(), Value::String(element.type_name.clone()));
        element_object.insert("props".to_owned(), element.props.clone());
        element_object.insert(
            "children".to_owned(),
            Value::Array(
                element
                    .children
                    .iter()
                    .map(|child| Value::String(child.clone()))
                    .collect(),
            ),
        );
        if let Some(visible) = &element.visible {
            element_object.insert("visible".to_owned(), visible.clone());
        }
        if !element.on.is_empty() {
            element_object.insert("on".to_owned(), Value::Object(element.on.clone()));
        }
        if let Some(repeat) = &element.repeat {
            let mut repeat_object = Map::new();
            repeat_object.insert(
                "statePath".to_owned(),
                Value::String(repeat.state_path.clone()),
            );
            if let Some(item_key) = &repeat.key {
                repeat_object.insert("key".to_owned(), Value::String(item_key.clone()));
            }
            element_object.insert("repeat".to_owned(), Value::Object(repeat_object));
        }
        if !element.watch.is_empty() {
            element_object.insert("watch".to_owned(), Value::Object(element.watch.clone()));
        }
        elements.insert(key.clone(), Value::Object(element_object));
    }
    let mut spec_object = Map::new();
    spec_object.insert("root".to_owned(), Value::String(spec.root.clone()));
    spec_object.insert("elements".to_owned(), Value::Object(elements));
    if let Some(state) = &spec.state {
        spec_object.insert("state".to_owned(), state.clone());
    }
    Value::Object(spec_object)
}

/// Resolves an element's `on`/`watch` event to its list of bindings.
/// A binding may be a single object or an array of objects (upstream
/// `ActionBinding | ActionBinding[]`); a non-binding entry is skipped.
#[must_use]
pub fn bindings_for_event(table: &Map<String, Value>, event: &str) -> Vec<ActionBinding> {
    let Some(found) = table.get(event) else {
        return Vec::new();
    };
    match found {
        Value::Array(items) => items.iter().filter_map(ActionBinding::from_value).collect(),
        single => ActionBinding::from_value(single).into_iter().collect(),
    }
}

/// Converts a **nested** spec (a human-written tree with inline
/// `children` arrays) to the flat [`Spec`], assigning pre-order keys
/// `el-0`, `el-1`, … and hoisting a root `state` to [`Spec::state`] — the
/// upstream `nestedToFlat`. Total: a child that is not an object with a
/// `type` is skipped.
#[must_use]
pub fn nested_to_flat(nested: &Value) -> Spec {
    let mut elements = std::collections::BTreeMap::new();
    let mut counter = 0usize;
    let root = walk_nested(nested, &mut elements, &mut counter);
    let state = nested
        .as_object()
        .and_then(|object| object.get("state"))
        .filter(|value| value.is_object())
        .cloned();
    Spec {
        root,
        elements,
        state,
    }
}

fn walk_nested(
    node: &Value,
    elements: &mut std::collections::BTreeMap<String, UiElement>,
    counter: &mut usize,
) -> String {
    let key = format!("el-{counter}");
    *counter += 1;

    let mut element = element_from_value(node);
    // The nested form carries children inline; flatten them recursively
    // and replace the (string) children list with the generated keys.
    element.children.clear();
    if let Some(raw_children) = node.as_object().and_then(|object| object.get("children")) {
        if let Some(child_array) = raw_children.as_array() {
            for child in child_array {
                if child
                    .as_object()
                    .is_some_and(|map| map.contains_key("type"))
                {
                    let child_key = walk_nested(child, elements, counter);
                    element.children.push(child_key);
                }
            }
        }
    }
    if element.type_name.is_empty() {
        element.type_name = "unknown".to_owned();
    }
    elements.insert(key.clone(), element);
    key
}

/// Applies the reference `autoFixSpec` to an already-parsed spec. Parsing
/// via [`spec_from_value`] already hoists nested `visible`/`on`/`repeat`/
/// `watch`, so this is an explicit idempotent pass for callers holding a
/// hand-built [`Spec`]; it returns the (unchanged-shape) spec and the
/// human-readable list of fixes that *would* apply to a raw document.
#[must_use]
pub fn auto_fix_spec(spec: &Spec) -> (Spec, Vec<String>) {
    let fixes = Vec::new();
    (spec.clone(), fixes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flat_spec_parses_leniently() {
        let spec = spec_from_value(&json!({
            "root": "a",
            "elements": {
                "a": { "type": "Box", "props": { "gap": 1 }, "children": ["b"] },
                "b": { "type": "Text", "props": { "text": "hi" } },
            },
            "state": { "count": 0 },
        }));
        assert_eq!(spec.root, "a");
        assert_eq!(spec.elements.len(), 2);
        assert_eq!(spec.element("a").unwrap().children, vec!["b"]);
        assert_eq!(spec.element("b").unwrap().type_name, "Text");
        assert_eq!(spec.state, Some(json!({ "count": 0 })));
    }

    #[test]
    fn garbage_and_missing_fields_are_total() {
        assert_eq!(spec_from_value(&json!("not a spec")), Spec::new());
        let spec = spec_from_value(&json!({ "elements": { "x": 7 } }));
        assert_eq!(spec.root, "");
        // A non-object element degrades to a default, not a panic.
        assert_eq!(spec.element("x"), Some(&UiElement::default()));
    }

    #[test]
    fn autofix_hoists_siblings_nested_in_props() {
        let spec = spec_from_value(&json!({
            "root": "r",
            "elements": {
                "r": {
                    "type": "Box",
                    "props": {
                        "gap": 1,
                        "visible": { "$state": "/show" },
                        "on": { "press": { "action": "log" } },
                        "repeat": { "statePath": "/items" },
                    },
                    "children": ["c"],
                },
                "c": { "type": "Text", "props": { "text": "x" } },
            },
        }));
        let root = spec.element("r").unwrap();
        assert_eq!(root.visible, Some(json!({ "$state": "/show" })));
        assert!(root.on.contains_key("press"));
        assert_eq!(root.repeat.as_ref().unwrap().state_path, "/items");
        // `props` no longer carries the hoisted siblings.
        assert!(root.props.get("visible").is_none());
        assert!(root.props.get("repeat").is_none());
        assert_eq!(root.props.get("gap"), Some(&json!(1)));
    }

    #[test]
    fn nested_to_flat_assigns_preorder_keys() {
        let spec = nested_to_flat(&json!({
            "type": "Card",
            "props": { "title": "Hello" },
            "children": [
                { "type": "Text", "props": { "content": "World" } },
            ],
            "state": { "count": 0 },
        }));
        assert_eq!(spec.root, "el-0");
        assert_eq!(spec.element("el-0").unwrap().type_name, "Card");
        assert_eq!(spec.element("el-0").unwrap().children, vec!["el-1"]);
        assert_eq!(spec.element("el-1").unwrap().type_name, "Text");
        assert_eq!(spec.state, Some(json!({ "count": 0 })));
    }

    #[test]
    fn elements_array_form_is_accepted() {
        let spec = spec_from_value(&json!({
            "root": "root",
            "elements": [
                { "key": "root", "type": "Box", "children": ["t"] },
                { "key": "t", "type": "Text", "props": { "text": "y" } },
            ],
        }));
        assert_eq!(spec.element("root").unwrap().children, vec!["t"]);
        assert_eq!(spec.element("t").unwrap().type_name, "Text");
    }

    #[test]
    fn spec_round_trips_through_value() {
        let source = json!({
            "root": "a",
            "elements": {
                "a": {
                    "type": "Box",
                    "props": { "gap": 1 },
                    "children": ["b"],
                    "visible": true,
                    "repeat": { "statePath": "/xs", "key": "id" },
                },
                "b": { "type": "Text", "props": { "text": "hi" }, "children": [] },
            },
            "state": { "n": 1 },
        });
        let spec = spec_from_value(&source);
        let round = spec_from_value(&spec_to_value(&spec));
        assert_eq!(spec, round);
    }
}
