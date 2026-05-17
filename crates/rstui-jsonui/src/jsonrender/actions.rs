//! json-render **actions** — the `on:{event: ActionBinding|[…]}` model,
//! the five built-in actions, and the reducer-consumable
//! [`ResolvedAction`] an interaction produces. Ported from
//! `packages/core/src/actions.ts` and the built-in handlers in
//! `packages/ink/src/contexts/actions.tsx`.
//!
//! # Interaction is an intent, never a callback (ADR 0012 §P1)
//!
//! The widget is a pure projection: it never mutates state. A click is
//! resolved by [`HitMap`](crate::tree::HitMap) to a node id; the document
//! turns that into a `Vec<`[`ResolvedAction`]`>` (params resolved against
//! a live snapshot via the expression engine). The **reducer** then
//! applies each one to the caller-owned [`DataModel`]
//! — the built-ins map onto `set`/`remove` exactly as the reference
//! handlers do:
//!
//! - `setState{statePath,value}` → `model.set(statePath, value)`
//! - `pushState{statePath,value,clearStatePath?}` → append to the array
//!   (`$id` in `value` becomes a counter-derived id), optionally clear
//!   another path
//! - `removeState{statePath,index}` → splice the array element
//! - `log` / `exit` → no model mutation (host-observable signals)
//!
//! A host map may override any built-in by name (the upstream "custom
//! handler override" rule); unknown actions are surfaced verbatim for the
//! host to interpret. All resolution is total.

use serde_json::{Map, Value};

use super::expr::{ResolveScope, resolve_action_param};
use crate::value::DataModel;

/// One `on` binding: an action name, optional dynamic params, and the
/// optional confirm/onSuccess/onError carried verbatim for a host that
/// implements them. Mirrors the upstream `ActionBinding`.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionBinding {
    /// The action name (`setState`, a host action, …).
    pub action: String,
    /// Raw (unresolved) parameter expressions.
    pub params: Map<String, Value>,
    /// Optional confirmation descriptor (raw JSON; host-rendered).
    pub confirm: Option<Value>,
    /// Optional success handler (raw JSON; host-interpreted).
    pub on_success: Option<Value>,
    /// Optional error handler (raw JSON; host-interpreted).
    pub on_error: Option<Value>,
}

impl ActionBinding {
    /// Parses one binding from JSON, leniently. `None` only when the
    /// value is not an object with a string `action`.
    #[must_use]
    pub fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let action = object.get("action")?.as_str()?.to_owned();
        Some(Self {
            action,
            params: object
                .get("params")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            confirm: object.get("confirm").cloned(),
            on_success: object.get("onSuccess").cloned(),
            on_error: object.get("onError").cloned(),
        })
    }
}

/// An [`ActionBinding`] with its params resolved against a live state
/// snapshot — what the reducer consumes (the upstream `ResolvedAction`).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAction {
    /// The action name.
    pub action: String,
    /// Params resolved against the data model at emit time.
    pub params: Map<String, Value>,
    /// The confirmation descriptor, if any (still raw).
    pub confirm: Option<Value>,
    /// The success handler, if any (still raw).
    pub on_success: Option<Value>,
    /// The error handler, if any (still raw).
    pub on_error: Option<Value>,
}

/// Resolves a binding's params against the scope (every value through
/// [`resolve_action_param`], so a `{ $item: "f" }` becomes an absolute
/// state path the built-ins can write to).
#[must_use]
pub fn resolve_action(binding: &ActionBinding, scope: &ResolveScope<'_>) -> ResolvedAction {
    let mut params = Map::new();
    for (key, value) in &binding.params {
        params.insert(key.clone(), resolve_action_param(value, scope));
    }
    ResolvedAction {
        action: binding.action.clone(),
        params,
        confirm: binding.confirm.clone(),
        on_success: binding.on_success.clone(),
        on_error: binding.on_error.clone(),
    }
}

/// Deep-resolves `$state` references and the `$id` token inside a
/// `pushState` value (the upstream `deepResolveValue`): a lone
/// `{ $state: ptr }` reads the model; `"$id"` / `{ "$id": … }` becomes a
/// fresh id from `next_id`.
fn deep_resolve_value(value: &Value, model: &DataModel, next_id: &mut u64, depth: usize) -> Value {
    if depth > 10 {
        return value.clone();
    }
    if value == &Value::String("$id".to_owned()) {
        let id = format!("id-{next_id}");
        *next_id += 1;
        return Value::String(id);
    }
    if let Some(object) = value.as_object() {
        if object.len() == 1 {
            if let Some(pointer) = object.get("$state").and_then(Value::as_str) {
                return model.get(pointer).cloned().unwrap_or(Value::Null);
            }
            if object.contains_key("$id") {
                let id = format!("id-{next_id}");
                *next_id += 1;
                return Value::String(id);
            }
        }
        let mut resolved = Map::new();
        for (key, child) in object {
            resolved.insert(
                key.clone(),
                deep_resolve_value(child, model, next_id, depth + 1),
            );
        }
        return Value::Object(resolved);
    }
    if let Some(items) = value.as_array() {
        return Value::Array(
            items
                .iter()
                .map(|item| deep_resolve_value(item, model, next_id, depth + 1))
                .collect(),
        );
    }
    value.clone()
}

/// The outcome of applying one [`ResolvedAction`] in the reducer: the
/// caller may need to react to a non-state signal (`log` text, an `exit`
/// request, or an unhandled action a host map should service).
#[derive(Debug, Clone, PartialEq)]
pub enum ActionEffect {
    /// The action mutated the data model in place (`setState` etc.).
    StateChanged,
    /// A `log` action — the message for the host to surface.
    Log(String),
    /// An `exit` action — the optional exit code.
    Exit(Option<i64>),
    /// Not a built-in: the host action map should handle it.
    Unhandled(ResolvedAction),
}

/// Applies one resolved action to the data model, the way the reference
/// built-in handlers do, and returns what the host still needs to know.
/// `next_id` is a caller-owned monotonic counter standing in for the
/// reference `crypto.randomUUID()` (deterministic, so headless tests are
/// reproducible).
///
/// The widget never calls this — the **reducer** does (pure projection).
pub fn apply_action(
    action: &ResolvedAction,
    model: &mut DataModel,
    next_id: &mut u64,
) -> ActionEffect {
    match action.action.as_str() {
        "setState" => {
            if let Some(state_path) = action.params.get("statePath").and_then(Value::as_str) {
                let value = action.params.get("value").cloned().unwrap_or(Value::Null);
                model.set(state_path, value);
            }
            ActionEffect::StateChanged
        }
        "pushState" => {
            if let Some(state_path) = action.params.get("statePath").and_then(Value::as_str) {
                let raw_value = action.params.get("value").cloned().unwrap_or(Value::Null);
                let resolved = deep_resolve_value(&raw_value, model, next_id, 0);
                let mut array = match model.get(state_path) {
                    Some(Value::Array(items)) => items.clone(),
                    _ => Vec::new(),
                };
                array.push(resolved);
                model.set(state_path, Value::Array(array));
                if let Some(clear) = action.params.get("clearStatePath").and_then(Value::as_str) {
                    model.set(clear, Value::String(String::new()));
                }
            }
            ActionEffect::StateChanged
        }
        "removeState" => {
            if let Some(state_path) = action.params.get("statePath").and_then(Value::as_str) {
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                let index = action
                    .params
                    .get("index")
                    .and_then(Value::as_f64)
                    .map(|float| float as usize);
                if let (Some(index), Some(Value::Array(items))) = (index, model.get(state_path)) {
                    let mut array = items.clone();
                    if index < array.len() {
                        array.remove(index);
                        model.set(state_path, Value::Array(array));
                    }
                }
            }
            ActionEffect::StateChanged
        }
        "log" => {
            let message = action
                .params
                .get("message")
                .or_else(|| action.params.get("value"))
                .map(super::expr::coerce_to_string)
                .unwrap_or_default();
            ActionEffect::Log(message)
        }
        "exit" => {
            #[allow(clippy::cast_possible_truncation)]
            let code = action.params.get("code").and_then(Value::as_i64);
            ActionEffect::Exit(code)
        }
        _ => ActionEffect::Unhandled(action.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrender::directives::DirectiveRegistry;
    use crate::jsonrender::expr::ComputedFn;
    use serde_json::json;

    fn parse(value: &Value) -> ActionBinding {
        ActionBinding::from_value(value).expect("valid binding")
    }

    #[test]
    fn resolve_binding_params_against_model() {
        let model = DataModel::from_root(json!({ "draft": "hello" }));
        let functions: std::collections::BTreeMap<String, ComputedFn> =
            std::collections::BTreeMap::new();
        let registry = DirectiveRegistry::with_builtins();
        let scope = ResolveScope::new(&model, &functions, &registry);
        let binding = parse(&json!({
            "action": "setState",
            "params": { "statePath": "/saved", "value": { "$state": "/draft" } }
        }));
        let resolved = resolve_action(&binding, &scope);
        assert_eq!(resolved.params.get("value"), Some(&json!("hello")));
    }

    #[test]
    fn builtins_mutate_the_model() {
        let mut model = DataModel::from_root(json!({ "todos": [] }));
        let mut next_id = 0;

        let push = ResolvedAction {
            action: "pushState".to_owned(),
            params: json!({ "statePath": "/todos", "value": { "id": "$id", "text": "a" } })
                .as_object()
                .unwrap()
                .clone(),
            confirm: None,
            on_success: None,
            on_error: None,
        };
        assert_eq!(
            apply_action(&push, &mut model, &mut next_id),
            ActionEffect::StateChanged
        );
        assert_eq!(model.get("/todos/0/text"), Some(&json!("a")));
        assert_eq!(model.get("/todos/0/id"), Some(&json!("id-0")));

        let set = ResolvedAction {
            action: "setState".to_owned(),
            params: json!({ "statePath": "/title", "value": "Tasks" })
                .as_object()
                .unwrap()
                .clone(),
            confirm: None,
            on_success: None,
            on_error: None,
        };
        apply_action(&set, &mut model, &mut next_id);
        assert_eq!(model.get("/title"), Some(&json!("Tasks")));

        let remove = ResolvedAction {
            action: "removeState".to_owned(),
            params: json!({ "statePath": "/todos", "index": 0 })
                .as_object()
                .unwrap()
                .clone(),
            confirm: None,
            on_success: None,
            on_error: None,
        };
        apply_action(&remove, &mut model, &mut next_id);
        assert_eq!(model.get("/todos"), Some(&json!([])));
    }

    #[test]
    fn log_exit_and_unhandled_surface_to_host() {
        let mut model = DataModel::new();
        let mut next_id = 0;
        assert_eq!(
            apply_action(
                &ResolvedAction {
                    action: "log".to_owned(),
                    params: json!({ "message": "hi" }).as_object().unwrap().clone(),
                    confirm: None,
                    on_success: None,
                    on_error: None,
                },
                &mut model,
                &mut next_id,
            ),
            ActionEffect::Log("hi".to_owned())
        );
        assert_eq!(
            apply_action(
                &ResolvedAction {
                    action: "exit".to_owned(),
                    params: json!({ "code": 2 }).as_object().unwrap().clone(),
                    confirm: None,
                    on_success: None,
                    on_error: None,
                },
                &mut model,
                &mut next_id,
            ),
            ActionEffect::Exit(Some(2))
        );
        match apply_action(
            &ResolvedAction {
                action: "customThing".to_owned(),
                params: Map::new(),
                confirm: None,
                on_success: None,
                on_error: None,
            },
            &mut model,
            &mut next_id,
        ) {
            ActionEffect::Unhandled(action) => assert_eq!(action.action, "customThing"),
            other => panic!("expected Unhandled, got {other:?}"),
        }
    }
}
