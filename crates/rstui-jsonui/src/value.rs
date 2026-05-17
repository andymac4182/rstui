//! The RFC-6901 JSON-Pointer data store both formats bind against.
//!
//! A2UI addresses its per-surface data model by JSON Pointer with
//! relative-scope resolution (a `ChildList` template instance scopes
//! relative paths to its array index); json-render addresses its state by
//! JSON Pointer too. This module is the shared, **total** store: every
//! operation is panic-free and creates intermediate containers on write
//! exactly as both reference engines do (a numeric next segment makes an
//! array, otherwise an object). The caller owns the [`DataModel`] (ADR
//! 0012) and mutates it only in the reducer.

use serde_json::Value;

/// One parsed JSON-Pointer reference token with `~1`→`/` and `~0`→`~`
/// already unescaped.
fn unescape_token(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

/// Splits a JSON Pointer into its already-unescaped reference tokens.
/// `""` and `"/"` are the document root (no tokens). A pointer that does
/// not start with `/` is treated leniently as a single token (the A2UI
/// relative-path case; absolute resolution is the caller's concern via
/// [`resolve_scope`]).
#[must_use]
pub fn parse_pointer(pointer: &str) -> Vec<String> {
    if pointer.is_empty() || pointer == "/" {
        return Vec::new();
    }
    if let Some(rest) = pointer.strip_prefix('/') {
        return rest.split('/').map(unescape_token).collect();
    }
    pointer.split('/').map(unescape_token).collect()
}

/// Resolves a possibly-relative path against a scope path to an absolute
/// JSON Pointer (the A2UI rule: a leading `/` is absolute; `""`/`.` is the
/// scope itself; anything else is `scope/relative`).
#[must_use]
pub fn resolve_scope(scope: &str, path: &str) -> String {
    if path.starts_with('/') {
        return path.to_owned();
    }
    if path.is_empty() || path == "." {
        return scope.to_owned();
    }
    if scope.is_empty() || scope == "/" {
        format!("/{path}")
    } else {
        format!("{scope}/{path}")
    }
}

/// A caller-owned JSON document addressed by JSON Pointer.
///
/// Cloning is cheap-ish (a `serde_json::Value` deep clone) and expected:
/// the reducer mutates one `DataModel`; `view` re-projects from it every
/// frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DataModel {
    root: Value,
}

impl DataModel {
    /// An empty object data model.
    #[must_use]
    pub fn new() -> Self {
        Self {
            root: Value::Object(serde_json::Map::new()),
        }
    }

    /// Wraps an existing JSON value as the document root (used to seed
    /// from A2UI `updateDataModel` / json-render `spec.state`).
    #[must_use]
    pub fn from_root(root: Value) -> Self {
        Self { root }
    }

    /// The whole document (for `sendDataModel` / debug).
    #[must_use]
    pub fn root(&self) -> &Value {
        &self.root
    }

    /// Reads the value at an absolute JSON Pointer, or `None` if any
    /// segment is missing. Never panics.
    #[must_use]
    pub fn get(&self, pointer: &str) -> Option<&Value> {
        let mut current = &self.root;
        for token in parse_pointer(pointer) {
            current = match current {
                Value::Object(map) => map.get(&token)?,
                Value::Array(items) => items.get(token.parse::<usize>().ok()?)?,
                _ => return None,
            };
        }
        Some(current)
    }

    /// Reads the value at a path resolved against `scope` (the A2UI
    /// relative-binding case).
    #[must_use]
    pub fn get_scoped(&self, scope: &str, path: &str) -> Option<&Value> {
        self.get(&resolve_scope(scope, path))
    }

    /// Upserts `value` at an absolute JSON Pointer, creating intermediate
    /// containers (numeric next segment ⇒ array, else object) exactly as
    /// the A2UI/json-render reference engines do. A pointer of `""`/`"/"`
    /// replaces the whole document. Total — a descent into a primitive
    /// is a no-op rather than a panic.
    pub fn set(&mut self, pointer: &str, value: Value) {
        let tokens = parse_pointer(pointer);
        if tokens.is_empty() {
            self.root = value;
            return;
        }
        Self::set_in(&mut self.root, &tokens, value);
    }

    fn set_in(target: &mut Value, tokens: &[String], value: Value) {
        let Some((head, rest)) = tokens.split_first() else {
            *target = value;
            return;
        };
        let child_is_index = rest
            .first()
            .is_none_or(|next| next.parse::<usize>().is_ok());
        if rest.is_empty() {
            match target {
                Value::Array(items) => {
                    if let Ok(index) = head.parse::<usize>() {
                        if index < items.len() {
                            items[index] = value;
                        } else if index == items.len() {
                            items.push(value);
                        }
                    }
                }
                _ => {
                    if !target.is_object() {
                        *target = Value::Object(serde_json::Map::new());
                    }
                    if let Value::Object(map) = target {
                        map.insert(head.clone(), value);
                    }
                }
            }
            return;
        }
        // Descend, creating the right container for the next segment.
        if let Ok(index) = head.parse::<usize>() {
            if !target.is_array() {
                *target = Value::Array(Vec::new());
            }
            if let Value::Array(items) = target {
                while items.len() <= index {
                    items.push(Value::Null);
                }
                Self::set_in(&mut items[index], rest, value);
            }
        } else {
            if !target.is_object() {
                *target = Value::Object(serde_json::Map::new());
            }
            if let Value::Object(map) = target {
                let slot = map.entry(head.clone()).or_insert_with(|| {
                    if child_is_index {
                        Value::Array(Vec::new())
                    } else {
                        Value::Object(serde_json::Map::new())
                    }
                });
                Self::set_in(slot, rest, value);
            }
        }
    }

    /// Deletes the key/element at an absolute JSON Pointer (the A2UI
    /// "value omitted ⇒ delete" semantics). Missing path ⇒ no-op.
    pub fn remove(&mut self, pointer: &str) {
        let tokens = parse_pointer(pointer);
        let Some((last, parents)) = tokens.split_last() else {
            self.root = Value::Object(serde_json::Map::new());
            return;
        };
        let mut current = &mut self.root;
        for token in parents {
            current = match current {
                Value::Object(map) => match map.get_mut(token) {
                    Some(child) => child,
                    None => return,
                },
                Value::Array(items) => match token
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| items.get_mut(index))
                {
                    Some(child) => child,
                    None => return,
                },
                _ => return,
            };
        }
        match current {
            Value::Object(map) => {
                map.remove(last);
            }
            Value::Array(items) => {
                if let Ok(index) = last.parse::<usize>() {
                    if index < items.len() {
                        items.remove(index);
                    }
                }
            }
            _ => {}
        }
    }

    /// Reads a bound value as display text the way both engines coerce it
    /// (string verbatim; number/bool stringified; null/absent ⇒ `""`;
    /// object/array ⇒ compact JSON).
    #[must_use]
    pub fn get_text(&self, pointer: &str) -> String {
        match self.get(pointer) {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(text)) => text.clone(),
            Some(Value::Bool(flag)) => flag.to_string(),
            Some(Value::Number(number)) => number.to_string(),
            Some(other) => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pointer_parse_and_scope() {
        assert_eq!(parse_pointer("/a/b"), vec!["a", "b"]);
        assert!(parse_pointer("/").is_empty());
        assert_eq!(parse_pointer("/x~1y/~0z"), vec!["x/y", "~z"]);
        assert_eq!(resolve_scope("/users/0", "name"), "/users/0/name");
        assert_eq!(resolve_scope("/users/0", "/abs"), "/abs");
        assert_eq!(resolve_scope("/s", "."), "/s");
    }

    #[test]
    fn set_creates_containers_and_get_reads_back() {
        let mut model = DataModel::new();
        model.set("/user/name", json!("Ada"));
        model.set("/user/tags/0", json!("a"));
        model.set("/user/tags/1", json!("b"));
        assert_eq!(model.get("/user/name"), Some(&json!("Ada")));
        assert_eq!(model.get("/user/tags/1"), Some(&json!("b")));
        assert!(model.get("/user/tags/9").is_none());
        assert_eq!(model.get_text("/user/name"), "Ada");
        assert_eq!(model.get_text("/missing"), "");
        assert_eq!(model.get_scoped("/user", "name"), Some(&json!("Ada")));
    }

    #[test]
    fn remove_and_root_replace_are_total() {
        let mut model = DataModel::from_root(json!({ "a": { "b": 1 }, "list": [10, 20] }));
        model.remove("/a/b");
        assert!(model.get("/a/b").is_none());
        model.remove("/list/0");
        assert_eq!(model.get("/list/0"), Some(&json!(20)));
        model.remove("/does/not/exist"); // no-op, no panic
        model.set("", json!("replaced"));
        assert_eq!(model.root(), &json!("replaced"));
        model.set("/x", json!(1)); // descend into primitive root → recreates
        assert_eq!(model.get("/x"), Some(&json!(1)));
    }
}
