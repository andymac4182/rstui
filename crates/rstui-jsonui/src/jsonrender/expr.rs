//! The json-render **expression engine** — the twelve-step
//! `resolvePropValue`, `evaluateVisibility`, and `resolveBindings`,
//! ported in order from `packages/core/src/props.ts` and
//! `visibility.ts`.
//!
//! # The twelve steps, in order
//!
//! Every prop value an agent emits is run through [`resolve_prop_value`],
//! which dispatches in the exact upstream order so behaviour matches the
//! reference renderer: `null`→as-is; `{$state}`; `{$item}`/`{$index}`
//! (repeat scope); `{$bindState}`/`{$bindItem}` (value + write-back path
//! captured by [`resolve_bindings`]); `{$cond,$then,$else}`;
//! `{$computed}` (host fn map); `{$template}` (`${tok}` interpolation);
//! array→map; object→custom-directive lookup else recurse; primitive
//! passthrough.
//!
//! [`evaluate_visibility`] interprets the recursive `visible` grammar
//! (bool / single condition / implicit-AND array / `{$and}` / `{$or}`);
//! a `SingleCondition` is a `{$state|$item|$index}` source plus at most
//! one of `eq/neq/gt/gte/lt/lte` (`gt`-family require both numeric) with
//! an optional `not`.
//!
//! Everything is **total** — a missing path resolves to JSON `null`, a
//! malformed condition is `false`, never a panic (the LLM-streaming
//! contract).

use serde_json::{Map, Number, Value};

use super::directives::DirectiveRegistry;
use crate::value::DataModel;

/// A host-supplied `$computed` function: receives the resolved argument
/// map, returns a value. Stored by name in [`ResolveScope::functions`].
pub type ComputedFn = std::sync::Arc<dyn Fn(&Map<String, Value>) -> Value + Send + Sync>;

/// The repeat scope active while projecting a `repeat` element's
/// children: the current item, its array index, and the absolute state
/// path to it (`statePath/index`) used to resolve `$bindItem` write-back.
#[derive(Debug, Clone, Default)]
pub struct RepeatScope {
    /// The current array item value.
    pub item: Value,
    /// The current zero-based array index.
    pub index: usize,
    /// Absolute state pointer to this item (`<statePath>/<index>`).
    pub base_path: String,
}

/// Everything [`resolve_prop_value`] / [`evaluate_visibility`] need: the
/// live data model, the optional repeat scope, the `$computed` function
/// map, and the custom-directive registry. Borrowed, never owned, so the
/// reducer keeps sole ownership of the [`DataModel`] (ADR 0012).
pub struct ResolveScope<'model> {
    /// The caller-owned data model expressions read from.
    pub model: &'model DataModel,
    /// The active repeat scope, if inside a `repeat`.
    pub repeat: Option<&'model RepeatScope>,
    /// Named host functions for `{$computed}`.
    pub functions: &'model std::collections::BTreeMap<String, ComputedFn>,
    /// Custom directive registry (the 8 built-ins live here).
    pub directives: &'model DirectiveRegistry,
}

impl<'model> ResolveScope<'model> {
    /// A scope with no repeat, no host functions, and only the built-in
    /// directive registry — the common top-level case.
    #[must_use]
    pub fn new(
        model: &'model DataModel,
        functions: &'model std::collections::BTreeMap<String, ComputedFn>,
        directives: &'model DirectiveRegistry,
    ) -> Self {
        Self {
            model,
            repeat: None,
            functions,
            directives,
        }
    }

    /// The same scope re-bound to a repeat item (used per child while
    /// projecting a `repeat`).
    #[must_use]
    pub fn with_repeat(&self, repeat: &'model RepeatScope) -> Self {
        Self {
            model: self.model,
            repeat: Some(repeat),
            functions: self.functions,
            directives: self.directives,
        }
    }
}

/// Reads a path **relative to a value** (RFC-6901 within the repeat item),
/// where `""` is the whole value — the upstream `getByPath(item, path)`.
fn get_in_value<'value>(root: &'value Value, pointer: &str) -> Option<&'value Value> {
    if pointer.is_empty() || pointer == "/" {
        return Some(root);
    }
    let mut current = root;
    for token in crate::value::parse_pointer(pointer) {
        current = match current {
            Value::Object(map) => map.get(&token)?,
            Value::Array(items) => items.get(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

/// `String(value)` the way the json-render template/concat code coerces:
/// string verbatim, number/bool stringified, null/absent → `""`,
/// object/array → compact JSON.
#[must_use]
pub fn coerce_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

/// Reads an object's single string field if it is the *only* key — the
/// upstream `isStateExpression`-style guard ("object with exactly the
/// expression key"). Used for `$state`/`$item`/`$bindState`/`$bindItem`.
fn sole_string_field<'value>(value: &'value Value, key: &str) -> Option<&'value str> {
    let map = value.as_object()?;
    map.get(key)?.as_str()
}

fn has_key(value: &Value, key: &str) -> bool {
    value.as_object().is_some_and(|map| map.contains_key(key))
}

/// The twelve-step prop resolver, dispatched in upstream order. Returns
/// JSON `null` for an unresolvable state/item path (mirrors
/// `getByPath` → `undefined` → serialised as absent). Total.
#[must_use]
pub fn resolve_prop_value(value: &Value, scope: &ResolveScope<'_>) -> Value {
    // 1. null / (undefined modelled as null) → as-is.
    if value.is_null() {
        return Value::Null;
    }

    // 2. { $state: ptr } → read from the global data model.
    if let Some(pointer) = sole_string_field(value, "$state") {
        return scope.model.get(pointer).cloned().unwrap_or(Value::Null);
    }

    // 3. { $item: path } → field on the current repeat item.
    if let Some(path) = sole_string_field(value, "$item") {
        return match scope.repeat {
            None => Value::Null,
            Some(repeat) => get_in_value(&repeat.item, path)
                .cloned()
                .unwrap_or(Value::Null),
        };
    }

    // 4. { $index: true } → the current repeat index.
    if has_key(value, "$index") {
        return match scope.repeat {
            None => Value::Null,
            Some(repeat) => Value::Number(Number::from(repeat.index)),
        };
    }

    // 5. { $bindState: ptr } → value at the path (write-back path is
    //    captured separately by `resolve_bindings`).
    if let Some(pointer) = sole_string_field(value, "$bindState") {
        return scope.model.get(pointer).cloned().unwrap_or(Value::Null);
    }

    // 6. { $bindItem: path } → value at <basePath>/<path>.
    if let Some(path) = sole_string_field(value, "$bindItem") {
        return match resolve_bind_item_path(path, scope) {
            None => Value::Null,
            Some(absolute) => scope.model.get(&absolute).cloned().unwrap_or(Value::Null),
        };
    }

    // 7. { $cond, $then, $else } → evaluate condition, pick a branch.
    if has_key(value, "$cond") && has_key(value, "$then") && has_key(value, "$else") {
        let object = value.as_object().expect("has_key implies object");
        let chosen = if evaluate_visibility(object.get("$cond"), scope) {
            object.get("$then")
        } else {
            object.get("$else")
        };
        return chosen
            .map(|branch| resolve_prop_value(branch, scope))
            .unwrap_or(Value::Null);
    }

    // 8. { $computed: name, args? } → call a host function.
    if let Some(name) = sole_computed_name(value) {
        let Some(function) = scope.functions.get(name) else {
            return Value::Null;
        };
        let mut resolved_args = Map::new();
        if let Some(args) = value.as_object().and_then(|map| map.get("args")) {
            if let Some(arg_map) = args.as_object() {
                for (key, arg) in arg_map {
                    resolved_args.insert(key.clone(), resolve_prop_value(arg, scope));
                }
            }
        }
        return function(&resolved_args);
    }

    // 9. { $template: "…${tok}…" } → interpolate.
    if let Some(template) = sole_string_field(value, "$template") {
        return Value::String(interpolate_template(template, scope));
    }

    // 10. array → resolve each element.
    if let Some(items) = value.as_array() {
        return Value::Array(
            items
                .iter()
                .map(|item| resolve_prop_value(item, scope))
                .collect(),
        );
    }

    // 11. object → custom-directive lookup, else recurse every value.
    if let Some(object) = value.as_object() {
        match scope.directives.find(object) {
            Ok(Some(directive)) => return (directive.resolve)(value, scope),
            // Ambiguous co-occurrence is an error → degrade, do not panic.
            Err(_) => return Value::String("[ambiguous directive]".to_owned()),
            Ok(None) => {}
        }
        let mut resolved = Map::new();
        for (key, child) in object {
            resolved.insert(key.clone(), resolve_prop_value(child, scope));
        }
        return Value::Object(resolved);
    }

    // 12. primitive literal → passthrough.
    value.clone()
}

/// `{ $computed: "<name>" }` guard (the name must be a string).
fn sole_computed_name(value: &Value) -> Option<&str> {
    value.as_object()?.get("$computed")?.as_str()
}

/// Resolves a `$bindItem` path to an absolute state pointer using the
/// repeat scope's `base_path`. `""` → the whole item. `None` outside a
/// repeat scope (upstream warns + returns undefined).
fn resolve_bind_item_path(item_path: &str, scope: &ResolveScope<'_>) -> Option<String> {
    let repeat = scope.repeat?;
    if item_path.is_empty() {
        Some(repeat.base_path.clone())
    } else {
        Some(format!("{}/{item_path}", repeat.base_path))
    }
}

/// `$template` interpolation: `${/abs}` resolves against the state model;
/// a bare `${tok}` tries the repeat item first then `/tok` in state;
/// `null` → `""` (upstream regex `\$\{([^}]+)\}`).
fn interpolate_template(template: &str, scope: &ResolveScope<'_>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            // Unclosed `${` — emit the rest verbatim and stop.
            out.push_str("${");
            rest = after;
            break;
        };
        let token = &after[..end];
        out.push_str(&resolve_template_token(token, scope));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn resolve_template_token(token: &str, scope: &ResolveScope<'_>) -> String {
    if let Some(stripped) = token.strip_prefix('/') {
        let _ = stripped;
        return scope
            .model
            .get(token)
            .filter(|value| !value.is_null())
            .map(coerce_to_string)
            .unwrap_or_default();
    }
    if let Some(repeat) = scope.repeat {
        if let Some(from_item) = get_in_value(&repeat.item, token) {
            if !from_item.is_null() {
                return coerce_to_string(from_item);
            }
        }
    }
    scope
        .model
        .get(&format!("/{token}"))
        .filter(|value| !value.is_null())
        .map(coerce_to_string)
        .unwrap_or_default()
}

/// Resolves every prop in an element's props object, returning a new
/// resolved object (the upstream `resolveElementProps`).
#[must_use]
pub fn resolve_element_props(props: &Value, scope: &ResolveScope<'_>) -> Map<String, Value> {
    let mut resolved = Map::new();
    if let Some(object) = props.as_object() {
        for (key, value) in object {
            resolved.insert(key.clone(), resolve_prop_value(value, scope));
        }
    }
    resolved
}

/// Scans raw props for `$bindState`/`$bindItem` and returns prop-name →
/// absolute state pointer (the write-back map the reducer uses for
/// two-way `TextField`/`Checkbox` binding) — the upstream
/// `resolveBindings`. A `$bindItem` outside a repeat scope is dropped.
#[must_use]
pub fn resolve_bindings(
    props: &Value,
    scope: &ResolveScope<'_>,
) -> std::collections::BTreeMap<String, String> {
    let mut bindings = std::collections::BTreeMap::new();
    if let Some(object) = props.as_object() {
        for (key, value) in object {
            if let Some(pointer) = sole_string_field(value, "$bindState") {
                bindings.insert(key.clone(), pointer.to_owned());
            } else if let Some(path) = sole_string_field(value, "$bindItem") {
                if let Some(absolute) = resolve_bind_item_path(path, scope) {
                    bindings.insert(key.clone(), absolute);
                }
            }
        }
    }
    bindings
}

/// Resolves one action-param value. Like [`resolve_prop_value`] but a
/// `{ $item: "field" }` resolves to the **absolute state path** (so it
/// can be handed to `setState`/`pushState`/`removeState`), per the
/// upstream `resolveActionParam`.
#[must_use]
pub fn resolve_action_param(value: &Value, scope: &ResolveScope<'_>) -> Value {
    if let Some(path) = sole_string_field(value, "$item") {
        return match resolve_bind_item_path(path, scope) {
            None => Value::Null,
            Some(absolute) => Value::String(absolute),
        };
    }
    if has_key(value, "$index") {
        return match scope.repeat {
            None => Value::Null,
            Some(repeat) => Value::Number(Number::from(repeat.index)),
        };
    }
    resolve_prop_value(value, scope)
}

// ===========================================================================
// Visibility
// ===========================================================================

/// Resolves a comparison RHS: a `{ $state }` is looked up, anything else
/// is the literal (upstream `resolveComparisonValue`).
fn resolve_comparison_value(value: &Value, scope: &ResolveScope<'_>) -> Value {
    if let Some(pointer) = sole_string_field(value, "$state") {
        return scope.model.get(pointer).cloned().unwrap_or(Value::Null);
    }
    value.clone()
}

/// The LHS of a single condition based on its source key.
fn resolve_condition_value(condition: &Map<String, Value>, scope: &ResolveScope<'_>) -> Value {
    if condition.contains_key("$index") {
        return match scope.repeat {
            None => Value::Null,
            Some(repeat) => Value::Number(Number::from(repeat.index)),
        };
    }
    if let Some(path) = condition.get("$item").and_then(Value::as_str) {
        return match scope.repeat {
            None => Value::Null,
            Some(repeat) => get_in_value(&repeat.item, path)
                .cloned()
                .unwrap_or(Value::Null),
        };
    }
    if let Some(pointer) = condition.get("$state").and_then(Value::as_str) {
        return scope.model.get(pointer).cloned().unwrap_or(Value::Null);
    }
    Value::Null
}

fn as_f64(value: &Value) -> Option<f64> {
    value.as_f64()
}

/// JSON `===` for `eq`/`neq` (structural equality, the upstream `===`
/// over primitives plus deep equality for the rare object/array RHS).
fn json_strict_eq(left: &Value, right: &Value) -> bool {
    left == right
}

/// Evaluates one `SingleCondition` (source + at most one operator + an
/// optional `not`), in upstream operator precedence.
fn evaluate_single(condition: &Map<String, Value>, scope: &ResolveScope<'_>) -> bool {
    let lhs = resolve_condition_value(condition, scope);

    let result = if let Some(expected) = condition.get("eq") {
        json_strict_eq(&lhs, &resolve_comparison_value(expected, scope))
    } else if let Some(expected) = condition.get("neq") {
        !json_strict_eq(&lhs, &resolve_comparison_value(expected, scope))
    } else if let Some(bound) = condition.get("gt") {
        compare_numeric(&lhs, &resolve_comparison_value(bound, scope), |a, b| a > b)
    } else if let Some(bound) = condition.get("gte") {
        compare_numeric(&lhs, &resolve_comparison_value(bound, scope), |a, b| a >= b)
    } else if let Some(bound) = condition.get("lt") {
        compare_numeric(&lhs, &resolve_comparison_value(bound, scope), |a, b| a < b)
    } else if let Some(bound) = condition.get("lte") {
        compare_numeric(&lhs, &resolve_comparison_value(bound, scope), |a, b| a <= b)
    } else {
        is_truthy(&lhs)
    };

    if condition.get("not") == Some(&Value::Bool(true)) {
        !result
    } else {
        result
    }
}

fn compare_numeric(lhs: &Value, rhs: &Value, predicate: impl Fn(f64, f64) -> bool) -> bool {
    match (as_f64(lhs), as_f64(rhs)) {
        (Some(a), Some(b)) => predicate(a, b),
        // gt/gte/lt/lte require BOTH numeric (upstream returns false).
        _ => false,
    }
}

/// JavaScript `Boolean(value)` semantics for the no-operator truthiness
/// case: `false`/`0`/`""`/`null` are falsy, everything else (incl. `{}`,
/// `[]`) is truthy.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|float| float != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Evaluates the recursive `visible` grammar. `None` (absent) ⇒ visible.
/// Total — an unrecognised shape is treated as `false` rather than a
/// panic.
#[must_use]
pub fn evaluate_visibility(condition: Option<&Value>, scope: &ResolveScope<'_>) -> bool {
    let Some(condition) = condition else {
        return true;
    };
    match condition {
        Value::Bool(flag) => *flag,
        Value::Array(items) => items
            .iter()
            .all(|child| evaluate_single_value(child, scope)),
        Value::Object(map) => {
            if let Some(Value::Array(children)) = map.get("$and") {
                return children
                    .iter()
                    .all(|child| evaluate_visibility(Some(child), scope));
            }
            if let Some(Value::Array(children)) = map.get("$or") {
                return children
                    .iter()
                    .any(|child| evaluate_visibility(Some(child), scope));
            }
            evaluate_single(map, scope)
        }
        _ => false,
    }
}

fn evaluate_single_value(condition: &Value, scope: &ResolveScope<'_>) -> bool {
    condition
        .as_object()
        .is_some_and(|map| evaluate_single(map, scope))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrender::directives::DirectiveRegistry;
    use serde_json::json;

    fn empty_fns() -> std::collections::BTreeMap<String, ComputedFn> {
        std::collections::BTreeMap::new()
    }

    #[test]
    fn state_item_index_template_resolution() {
        let model = DataModel::from_root(json!({ "user": { "name": "Ada" }, "n": 3 }));
        let functions = empty_fns();
        let registry = DirectiveRegistry::with_builtins();
        let repeat = RepeatScope {
            item: json!({ "title": "Buy milk" }),
            index: 2,
            base_path: "/todos/2".to_owned(),
        };
        let base = ResolveScope::new(&model, &functions, &registry);
        let scope = base.with_repeat(&repeat);

        assert_eq!(
            resolve_prop_value(&json!({ "$state": "/user/name" }), &scope),
            json!("Ada")
        );
        assert_eq!(
            resolve_prop_value(&json!({ "$item": "title" }), &scope),
            json!("Buy milk")
        );
        assert_eq!(
            resolve_prop_value(&json!({ "$index": true }), &scope),
            json!(2)
        );
        // Absolute template token vs bare token (item then state).
        assert_eq!(
            resolve_prop_value(&json!({ "$template": "#${/n}: ${title}" }), &scope),
            json!("#3: Buy milk")
        );
        // Missing path → "" in a template, null as a bare $state.
        assert_eq!(
            resolve_prop_value(&json!({ "$template": "[${/missing}]" }), &scope),
            json!("[]")
        );
        assert_eq!(
            resolve_prop_value(&json!({ "$state": "/missing" }), &scope),
            Value::Null
        );
    }

    #[test]
    fn cond_then_else_and_bindings() {
        let model = DataModel::from_root(json!({ "flag": true, "form": { "email": "a@b.c" } }));
        let functions = empty_fns();
        let registry = DirectiveRegistry::with_builtins();
        let scope = ResolveScope::new(&model, &functions, &registry);

        assert_eq!(
            resolve_prop_value(
                &json!({ "$cond": { "$state": "/flag" }, "$then": "Y", "$else": "N" }),
                &scope
            ),
            json!("Y")
        );
        let bindings = resolve_bindings(
            &json!({ "value": { "$bindState": "/form/email" }, "label": "Email" }),
            &scope,
        );
        assert_eq!(bindings.get("value"), Some(&"/form/email".to_owned()));
        // The bound value still resolves like $state.
        assert_eq!(
            resolve_prop_value(&json!({ "$bindState": "/form/email" }), &scope),
            json!("a@b.c")
        );
    }

    #[test]
    fn visibility_grammar() {
        let model = DataModel::from_root(json!({ "count": 7, "name": "x" }));
        let functions = empty_fns();
        let registry = DirectiveRegistry::with_builtins();
        let scope = ResolveScope::new(&model, &functions, &registry);

        assert!(evaluate_visibility(Some(&json!(true)), &scope));
        assert!(!evaluate_visibility(Some(&json!(false)), &scope));
        assert!(evaluate_visibility(None, &scope)); // absent ⇒ visible
        assert!(evaluate_visibility(
            Some(&json!({ "$state": "/count", "gt": 5 })),
            &scope
        ));
        assert!(!evaluate_visibility(
            Some(&json!({ "$state": "/count", "gt": 5, "not": true })),
            &scope
        ));
        // gt with a non-numeric LHS ⇒ false (both must be numeric).
        assert!(!evaluate_visibility(
            Some(&json!({ "$state": "/name", "gt": 5 })),
            &scope
        ));
        // Implicit-AND array.
        assert!(evaluate_visibility(
            Some(&json!([{ "$state": "/count", "gte": 7 }, { "$state": "/name", "eq": "x" }])),
            &scope
        ));
        // $or / $and recursion.
        assert!(evaluate_visibility(
            Some(&json!({ "$or": [{ "$state": "/missing" }, { "$state": "/count", "eq": 7 }] })),
            &scope
        ));
    }

    #[test]
    fn computed_function_invoked_with_resolved_args() {
        let model = DataModel::from_root(json!({ "a": 2, "b": 5 }));
        let mut functions = empty_fns();
        functions.insert(
            "sum".to_owned(),
            std::sync::Arc::new(|args: &Map<String, Value>| {
                let a = args.get("x").and_then(Value::as_f64).unwrap_or(0.0);
                let b = args.get("y").and_then(Value::as_f64).unwrap_or(0.0);
                json!(a + b)
            }),
        );
        let registry = DirectiveRegistry::with_builtins();
        let scope = ResolveScope::new(&model, &functions, &registry);
        assert_eq!(
            resolve_prop_value(
                &json!({
                    "$computed": "sum",
                    "args": { "x": { "$state": "/a" }, "y": { "$state": "/b" } }
                }),
                &scope
            ),
            json!(7.0)
        );
        // Unknown function ⇒ null, never a panic.
        assert_eq!(
            resolve_prop_value(&json!({ "$computed": "nope" }), &scope),
            Value::Null
        );
    }
}
