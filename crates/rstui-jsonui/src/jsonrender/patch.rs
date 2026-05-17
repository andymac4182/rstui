//! The json-render **patch stream** — RFC-6902 ops over the spec, the
//! [`SpecStreamCompiler`] (chunked JSONL), and the [`MixedStreamParser`]
//! (prose interleaved with a ```` ```spec ```` fence). Ported from
//! `packages/core/src/types.ts` (`parseSpecStreamLine`,
//! `applySpecStreamPatch`, `createSpecStreamCompiler`,
//! `createMixedStreamParser`).
//!
//! # Why a relaxed RFC-6902
//!
//! An agent streams the spec as one JSON-Patch op per line. The lines
//! arrive split across network chunks, sometimes truncated, sometimes
//! brace-mangled. So:
//!
//! - `replace` is **relaxed**: it sets even when the target is absent
//!   (upstream comment — "for streaming tolerance we set regardless"),
//!   because the spec is still being built.
//! - The compiler keeps the **last incomplete line** buffered, dedupes
//!   processed lines, skips un-parseable lines, and on a parse failure
//!   retries after stripping up to **eight** trailing `}`/`]`
//!   (LLM brace-recovery). Lines not starting `{`/`[` are commentary.
//! - The mixed parser routes everything inside a ```` ```spec ```` fence
//!   to patches and everything else to text.
//!
//! All of it is **total** — a bad line is dropped, never a panic — which
//! is exactly the progressive-rendering contract.

use serde_json::Value;

use super::spec::{Spec, spec_from_value, spec_to_value};
use crate::value::DataModel;

/// One RFC-6902 operation. `value` is required for `add`/`replace`/
/// `test`; `from` for `move`/`copy`. Parsed leniently.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonPatch {
    /// The operation name (`add`/`remove`/`replace`/`move`/`copy`/`test`).
    pub op: String,
    /// The target JSON Pointer.
    pub path: String,
    /// The operand (for `add`/`replace`/`test`).
    pub value: Option<Value>,
    /// The source pointer (for `move`/`copy`).
    pub from: Option<String>,
}

/// Parses one stream line into a [`JsonPatch`], or `None` when the line
/// is empty, not a JSON object/array, or lacks `op`/`path`. Mirrors
/// `parseSpecStreamLine`, plus the brace-recovery retry: a parse failure
/// strips up to eight trailing `}`/`]` and tries again (the LLM often
/// over-closes a truncated object).
#[must_use]
pub fn parse_patch_line(line: &str) -> Option<JsonPatch> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Commentary (prose) — only `{`/`[` lines are candidate JSON.
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    let parsed = serde_json::from_str::<Value>(trimmed)
        .ok()
        .or_else(|| recover_overclosed(trimmed))?;
    patch_from_value(&parsed)
}

/// LLM brace-recovery: strip trailing `}`/`]` (and surrounding
/// whitespace) one at a time, up to eight times, retrying the JSON parse
/// after each strip.
fn recover_overclosed(trimmed: &str) -> Option<Value> {
    let mut candidate = trimmed.to_owned();
    for _ in 0..8 {
        let stripped = candidate.trim_end();
        if !(stripped.ends_with('}') || stripped.ends_with(']')) {
            return None;
        }
        candidate = stripped[..stripped.len() - 1].to_owned();
        if let Ok(value) = serde_json::from_str::<Value>(candidate.trim_end()) {
            return Some(value);
        }
    }
    None
}

fn patch_from_value(value: &Value) -> Option<JsonPatch> {
    let object = value.as_object()?;
    let op = object.get("op")?.as_str()?.to_owned();
    let path = object.get("path")?.as_str()?.to_owned();
    Some(JsonPatch {
        op,
        path,
        value: object.get("value").cloned(),
        from: object
            .get("from")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// Applies one RFC-6902 op to a [`DataModel`]-wrapped JSON document
/// (the spec-as-JSON, or any state doc). `replace` is **relaxed** (set
/// even if absent). `test` is a no-op on mismatch here — a failed `test`
/// must not abort a streamed render, so it is treated as a soft assertion
/// (the upstream throws; totality forbids that mid-stream).
pub fn apply_patch(document: &mut DataModel, patch: &JsonPatch) {
    match patch.op.as_str() {
        "add" | "replace" => {
            if let Some(value) = &patch.value {
                document.set(&patch.path, value.clone());
            }
        }
        "remove" => document.remove(&patch.path),
        "move" => {
            let Some(source) = &patch.from else { return };
            let moved = document.get(source).cloned();
            document.remove(source);
            if let Some(moved) = moved {
                document.set(&patch.path, moved);
            }
        }
        "copy" => {
            let Some(source) = &patch.from else { return };
            if let Some(copied) = document.get(source).cloned() {
                document.set(&patch.path, copied);
            }
        }
        // `test`: soft no-op (mismatch must not blank a streamed UI).
        "test" => {}
        _ => {}
    }
}

/// A streaming spec compiler: feed it text chunks, it buffers, splits on
/// `\n`, keeps the last incomplete line, dedupes processed lines, applies
/// each parseable patch to the spec-as-JSON, and re-derives the [`Spec`].
/// Mirrors `createSpecStreamCompiler`.
#[derive(Debug)]
pub struct SpecStreamCompiler {
    document: DataModel,
    buffer: String,
    processed: std::collections::HashSet<String>,
    applied: Vec<JsonPatch>,
}

impl Default for SpecStreamCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl SpecStreamCompiler {
    /// A compiler starting from an empty spec (`{ root:"", elements:{} }`).
    #[must_use]
    pub fn new() -> Self {
        Self::with_initial(&Spec::new())
    }

    /// A compiler seeded from an existing spec (so patches refine it).
    #[must_use]
    pub fn with_initial(initial: &Spec) -> Self {
        Self {
            document: DataModel::from_root(spec_to_value(initial)),
            buffer: String::new(),
            processed: std::collections::HashSet::new(),
            applied: Vec::new(),
        }
    }

    /// Pushes a chunk; returns the patches newly applied by this call
    /// (so a caller can re-project only when something changed — the
    /// upstream `{ result, newPatches }`).
    pub fn push(&mut self, chunk: &str) -> Vec<JsonPatch> {
        self.buffer.push_str(chunk);
        let mut new_patches = Vec::new();

        // Split into complete lines; keep the trailing incomplete one.
        let mut lines: Vec<&str> = self.buffer.split('\n').collect();
        let remainder = lines.pop().unwrap_or("").to_owned();

        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || self.processed.contains(trimmed) {
                continue;
            }
            self.processed.insert(trimmed.to_owned());
            if let Some(patch) = parse_patch_line(trimmed) {
                apply_patch(&mut self.document, &patch);
                self.applied.push(patch.clone());
                new_patches.push(patch);
            }
        }
        self.buffer = remainder;
        new_patches
    }

    /// Flushes any buffered incomplete line (call at end-of-stream — the
    /// upstream `getResult` drains the buffer once).
    pub fn finish(&mut self) -> Vec<JsonPatch> {
        let mut new_patches = Vec::new();
        let trimmed = self.buffer.trim().to_owned();
        if !trimmed.is_empty() && !self.processed.contains(&trimmed) {
            self.processed.insert(trimmed.clone());
            if let Some(patch) = parse_patch_line(&trimmed) {
                apply_patch(&mut self.document, &patch);
                self.applied.push(patch.clone());
                new_patches.push(patch);
            }
        }
        self.buffer.clear();
        new_patches
    }

    /// The current spec, re-derived from the patched JSON document.
    #[must_use]
    pub fn spec(&self) -> Spec {
        spec_from_value(self.document.root())
    }

    /// The raw spec-as-JSON document (debug / `sendDataModel`).
    #[must_use]
    pub fn document(&self) -> &Value {
        self.document.root()
    }

    /// Every patch applied so far, in order.
    #[must_use]
    pub fn patches(&self) -> &[JsonPatch] {
        &self.applied
    }
}

/// One classified item from a mixed prose+spec stream.
#[derive(Debug, Clone, PartialEq)]
pub enum MixedItem {
    /// A line of conversational text (outside the fence, not a patch).
    Text(String),
    /// A patch op (inside the ```` ```spec ```` fence, or a JSON line
    /// outside it — the upstream heuristic mode).
    Patch(JsonPatch),
}

/// A stateful parser for an LLM reply that interleaves prose with a
/// ```` ```spec ```` fenced JSONL patch block. Mirrors
/// `createMixedStreamParser`: inside the fence every line is a patch
/// candidate; outside, a JSON line is a patch and anything else is text.
#[derive(Debug, Default)]
pub struct MixedStreamParser {
    buffer: String,
    in_spec_fence: bool,
}

impl MixedStreamParser {
    /// A fresh parser (outside any fence).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes a chunk; returns the items recognised from the complete
    /// lines it contained (the last incomplete line stays buffered).
    pub fn push(&mut self, chunk: &str) -> Vec<MixedItem> {
        self.buffer.push_str(chunk);
        let mut items = Vec::new();
        let mut lines: Vec<String> = self.buffer.split('\n').map(str::to_owned).collect();
        let remainder = lines.pop().unwrap_or_default();
        for line in lines {
            self.process_line(&line, &mut items);
        }
        self.buffer = remainder;
        items
    }

    /// Flushes the final buffered line at end-of-stream.
    pub fn finish(&mut self) -> Vec<MixedItem> {
        let mut items = Vec::new();
        if !self.buffer.trim().is_empty() {
            let line = std::mem::take(&mut self.buffer);
            self.process_line(&line, &mut items);
        }
        self.buffer.clear();
        items
    }

    fn process_line(&mut self, line: &str, items: &mut Vec<MixedItem>) {
        let trimmed = line.trim();

        // Fence transitions are swallowed (not emitted).
        if !self.in_spec_fence && trimmed.starts_with("```spec") {
            self.in_spec_fence = true;
            return;
        }
        if self.in_spec_fence && trimmed == "```" {
            self.in_spec_fence = false;
            return;
        }
        if trimmed.is_empty() {
            return;
        }

        if self.in_spec_fence {
            if let Some(patch) = parse_patch_line(trimmed) {
                items.push(MixedItem::Patch(patch));
            }
            // A non-patch line inside the fence is silently dropped.
            return;
        }

        // Outside the fence: heuristic mode.
        match parse_patch_line(trimmed) {
            Some(patch) => items.push(MixedItem::Patch(patch)),
            None => items.push(MixedItem::Text(line.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_line_skips_prose_and_recovers_overclosed() {
        assert!(parse_patch_line("Here is your UI:").is_none());
        assert!(parse_patch_line("").is_none());
        let patch = parse_patch_line(r#"{"op":"add","path":"/root","value":"main"}"#).unwrap();
        assert_eq!(patch.op, "add");
        assert_eq!(patch.path, "/root");
        assert_eq!(patch.value, Some(json!("main")));
        // Over-closed by the LLM: three extra braces, recovered.
        let recovered = parse_patch_line(r#"{"op":"replace","path":"/x","value":1}}}"#).unwrap();
        assert_eq!(recovered.op, "replace");
        assert_eq!(recovered.value, Some(json!(1)));
    }

    #[test]
    fn compiler_builds_spec_across_chunks_and_keeps_partial_line() {
        let mut compiler = SpecStreamCompiler::new();
        // Stream split mid-line; the partial tail must survive the chunk.
        let applied = compiler.push(
            "{\"op\":\"add\",\"path\":\"/root\",\"value\":\"a\"}\n{\"op\":\"add\",\"path\":\"/elem",
        );
        assert_eq!(applied.len(), 1);
        let more =
            compiler.push("ents/a\",\"value\":{\"type\":\"Text\",\"props\":{\"text\":\"hi\"}}}\n");
        assert_eq!(more.len(), 1);
        let spec = compiler.spec();
        assert_eq!(spec.root, "a");
        assert_eq!(spec.element("a").unwrap().type_name, "Text");
        // A duplicate line is deduped (idempotent stream).
        let dup = compiler.push("{\"op\":\"add\",\"path\":\"/root\",\"value\":\"a\"}\n");
        assert!(dup.is_empty());
    }

    #[test]
    fn apply_patch_move_copy_remove_and_relaxed_replace() {
        let mut document = DataModel::from_root(json!({ "a": 1 }));
        // Relaxed replace: target absent, still set.
        apply_patch(
            &mut document,
            &JsonPatch {
                op: "replace".to_owned(),
                path: "/b".to_owned(),
                value: Some(json!(2)),
                from: None,
            },
        );
        assert_eq!(document.get("/b"), Some(&json!(2)));
        apply_patch(
            &mut document,
            &JsonPatch {
                op: "copy".to_owned(),
                path: "/c".to_owned(),
                value: None,
                from: Some("/a".to_owned()),
            },
        );
        assert_eq!(document.get("/c"), Some(&json!(1)));
        apply_patch(
            &mut document,
            &JsonPatch {
                op: "move".to_owned(),
                path: "/d".to_owned(),
                value: None,
                from: Some("/c".to_owned()),
            },
        );
        assert_eq!(document.get("/d"), Some(&json!(1)));
        assert!(document.get("/c").is_none());
        // A failing `test` is a soft no-op, not a panic/blank.
        apply_patch(
            &mut document,
            &JsonPatch {
                op: "test".to_owned(),
                path: "/a".to_owned(),
                value: Some(json!("wrong")),
                from: None,
            },
        );
        assert_eq!(document.get("/a"), Some(&json!(1)));
    }

    #[test]
    fn mixed_stream_routes_fence_to_patches_and_rest_to_text() {
        let mut parser = MixedStreamParser::new();
        let mut items = parser.push("Here is a dashboard:\n```spec\n");
        items.extend(parser.push("{\"op\":\"add\",\"path\":\"/root\",\"value\":\"r\"}\n"));
        items.extend(parser.push("```\nDone!\n"));
        items.extend(parser.finish());
        assert_eq!(
            items,
            vec![
                MixedItem::Text("Here is a dashboard:".to_owned()),
                MixedItem::Patch(JsonPatch {
                    op: "add".to_owned(),
                    path: "/root".to_owned(),
                    value: Some(json!("r")),
                    from: None,
                }),
                MixedItem::Text("Done!".to_owned()),
            ]
        );
    }

    #[test]
    fn totality_garbage_stream_never_panics() {
        let mut compiler = SpecStreamCompiler::new();
        compiler.push("not json\n{ broken\n][\n{\"op\":\"add\"}\n\u{0}\u{1}\n");
        compiler.push("{\"op\":\"add\",\"path\":\"/root\",\"value\":\"ok\"}\n");
        compiler.finish();
        assert_eq!(compiler.spec().root, "ok");
    }
}
