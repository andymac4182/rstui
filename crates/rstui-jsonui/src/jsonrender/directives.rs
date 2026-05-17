//! The eight json-render **directives** and the host-extensible
//! [`DirectiveRegistry`], ported from `packages/directives/src/*` and the
//! `defineDirective`/`findDirective` registry in
//! `packages/core/src/directives.ts`.
//!
//! # The eight built-ins
//!
//! `$format` (date/currency/number/percent), `$math` (the eleven
//! arithmetic ops, `/0`→0, `NaN`→0), `$concat`, `$count`, `$truncate`,
//! `$pluralize`, `$join`, `$t` (i18n with `{{param}}`). Browsers give the
//! reference `$format` `Intl`/`Date`; a terminal has neither, so the date
//! and number/currency/percent formatting here is a faithful **best
//! effort** (ISO-ish date, grouped thousands, `$`-prefixed currency,
//! `×100%`) — deterministic and dependency-free, which is also what the
//! totality rule wants.
//!
//! # The registry
//!
//! [`DirectiveRegistry`] mirrors `defineDirective` + `findDirective`: a
//! name must start with `$` and must not collide with the eight built-in
//! prop-expression keys (`$state`/`$item`/`$index`/`$bindState`/
//! `$bindItem`/`$cond`/`$computed`/`$template`). An object that carries
//! **two** directive keys at once is ambiguous; lookup returns `Err(())`
//! and the resolver degrades it to a placeholder string rather than
//! panicking (the LLM-streaming contract — never abort the render).

use serde_json::{Number, Value};

use super::expr::{ResolveScope, coerce_to_string, resolve_prop_value};

/// A custom directive: the `$`-prefixed trigger key and a resolver that
/// receives the raw directive object and the live scope (so it can
/// recurse via [`resolve_prop_value`]). Mirrors the upstream
/// `DirectiveDefinition`.
pub struct Directive {
    /// The trigger key (e.g. `"$format"`); must start with `$`.
    pub name: &'static str,
    /// One-line description for an agent prompt (upstream `description`).
    pub description: &'static str,
    /// Resolver: `(raw_directive_object, scope) -> resolved value`.
    pub resolve: fn(&Value, &ResolveScope<'_>) -> Value,
}

impl std::fmt::Debug for Directive {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Directive")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Why a directive operation did not succeed. Recoverable — the resolver
/// turns either variant into a degraded placeholder rather than aborting
/// the render (the upstream `defineDirective`/`findDirective` throws are
/// demoted to this so streaming stays total).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveError {
    /// A registration name that is not `$`-prefixed or collides with a
    /// built-in prop-expression key (the `defineDirective` guard).
    InvalidName,
    /// An object carries two directive keys at once (the `findDirective`
    /// "ambiguous directive" case).
    Ambiguous,
}

impl std::fmt::Display for DirectiveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName => formatter.write_str(
                "directive name must start with '$' and not collide with a built-in key",
            ),
            Self::Ambiguous => {
                formatter.write_str("object carries multiple directive keys (ambiguous)")
            }
        }
    }
}

impl std::error::Error for DirectiveError {}

/// Keys handled by built-in prop resolution — a custom directive must not
/// shadow these (upstream `BUILT_IN_KEYS`).
pub const BUILT_IN_PROP_KEYS: [&str; 8] = [
    "$state",
    "$item",
    "$index",
    "$bindState",
    "$bindItem",
    "$cond",
    "$computed",
    "$template",
];

/// A name → [`Directive`] registry. Seed it with the eight built-ins via
/// [`with_builtins`](DirectiveRegistry::with_builtins) and extend it with
/// [`register`](DirectiveRegistry::register) (the upstream
/// `createDirectiveRegistry` + `defineDirective` guard rolled together).
#[derive(Debug, Default)]
pub struct DirectiveRegistry {
    entries: std::collections::BTreeMap<&'static str, Directive>,
}

impl DirectiveRegistry {
    /// An empty registry (no directives at all).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry pre-loaded with the eight standard directives.
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        for directive in builtin_directives() {
            // Built-ins are statically valid; ignore the (impossible) Err.
            let _ = registry.register(directive);
        }
        registry
    }

    /// Registers a custom directive. Returns
    /// [`DirectiveError::InvalidName`] (degrade, don't panic) if the name
    /// does not start with `$` or collides with a built-in
    /// prop-expression key — the `defineDirective` guard.
    ///
    /// # Errors
    ///
    /// [`DirectiveError::InvalidName`] when the name is not `$`-prefixed
    /// or shadows a [`BUILT_IN_PROP_KEYS`] entry.
    pub fn register(&mut self, directive: Directive) -> Result<(), DirectiveError> {
        if !directive.name.starts_with('$') {
            return Err(DirectiveError::InvalidName);
        }
        if BUILT_IN_PROP_KEYS.contains(&directive.name) {
            return Err(DirectiveError::InvalidName);
        }
        self.entries.insert(directive.name, directive);
        Ok(())
    }

    /// The number of registered directives.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry holds no directives.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The directive names, sorted — for a capability/prompt summary.
    #[must_use]
    pub fn names(&self) -> Vec<&'static str> {
        self.entries.keys().copied().collect()
    }

    /// Finds the directive triggered by an object's keys. `Ok(None)` when
    /// none match; [`DirectiveError::Ambiguous`] when **two** directive
    /// keys co-occur (the upstream "ambiguous directive" throw, demoted
    /// to a recoverable error so the renderer degrades instead of
    /// aborting).
    ///
    /// # Errors
    ///
    /// [`DirectiveError::Ambiguous`] when the object contains more than
    /// one registered directive key.
    pub fn find(
        &self,
        object: &serde_json::Map<String, Value>,
    ) -> Result<Option<&Directive>, DirectiveError> {
        let mut found: Option<&Directive> = None;
        for (name, directive) in &self.entries {
            if object.contains_key(*name) {
                if found.is_some() {
                    return Err(DirectiveError::Ambiguous);
                }
                found = Some(directive);
            }
        }
        Ok(found)
    }
}

/// The eight standard directives as a fresh vector (the upstream
/// `standardDirectives` plus `$t` — the i18n directive uses an empty
/// message table here; a host wires real messages via a custom
/// `$t` registration if it needs translation).
#[must_use]
pub fn builtin_directives() -> Vec<Directive> {
    vec![
        Directive {
            name: "$format",
            description: "Best-effort value formatting (date, currency, number, percent).",
            resolve: resolve_format,
        },
        Directive {
            name: "$math",
            description: "Arithmetic (add/subtract/multiply/divide/mod/min/max/round/floor/ceil/abs); /0 and NaN → 0.",
            resolve: resolve_math,
        },
        Directive {
            name: "$concat",
            description: "Concatenate resolved values into one string.",
            resolve: resolve_concat,
        },
        Directive {
            name: "$count",
            description: "Length of an array or string (else 0).",
            resolve: resolve_count,
        },
        Directive {
            name: "$truncate",
            description: "Truncate text to `length` (default 100) with `suffix` (default \"...\").",
            resolve: resolve_truncate,
        },
        Directive {
            name: "$pluralize",
            description: "Pick zero/one/other by count: \"3 items\" / \"1 item\" / \"no items\".",
            resolve: resolve_pluralize,
        },
        Directive {
            name: "$join",
            description: "Join an array with `separator` (default \", \").",
            resolve: resolve_join,
        },
        Directive {
            name: "$t",
            description: "i18n lookup with {{param}} interpolation (empty table ⇒ the key).",
            resolve: resolve_i18n,
        },
    ]
}

/// Best-effort `Number(value)` like the upstream `toNum`: `null`→0,
/// numeric string parsed, non-numeric → 0 (the reference also coerces
/// non-numeric to 0 with a console warning).
fn to_number(value: &Value) -> f64 {
    match value {
        Value::Null => 0.0,
        Value::Number(number) => number.as_f64().unwrap_or(0.0),
        Value::Bool(true) => 1.0,
        Value::Bool(false) => 0.0,
        Value::String(text) => text.trim().parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// `serde_json` number from an `f64`, normalising integral values to
/// integers (so `2+3` renders `5`, not `5.0`).
fn number_value(float: f64) -> Value {
    if !float.is_finite() {
        return Value::Number(Number::from(0));
    }
    if float.fract() == 0.0 && float.abs() < 9.007_199_254_740_992e15 {
        #[allow(clippy::cast_possible_truncation)]
        return Value::Number(Number::from(float as i64));
    }
    Number::from_f64(float).map_or(Value::Number(Number::from(0)), Value::Number)
}

/// Groups an integer string with thousands separators (a tiny stand-in
/// for `Intl.NumberFormat` grouping; terminals have no `Intl`).
fn group_thousands(integer_digits: &str) -> String {
    let negative = integer_digits.starts_with('-');
    let digits = integer_digits.trim_start_matches('-');
    let mut grouped = String::new();
    for (offset, ch) in digits.chars().enumerate() {
        if offset > 0 && (digits.len() - offset) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    if negative {
        format!("-{grouped}")
    } else {
        grouped
    }
}

fn format_number_grouped(float: f64) -> String {
    if float.fract() == 0.0 {
        #[allow(clippy::cast_possible_truncation)]
        return group_thousands(&format!("{}", float as i64));
    }
    let text = format!("{float}");
    match text.split_once('.') {
        Some((int_part, frac_part)) => {
            format!("{}.{frac_part}", group_thousands(int_part))
        }
        None => group_thousands(&text),
    }
}

fn resolve_format(raw: &Value, scope: &ResolveScope<'_>) -> Value {
    let object = match raw.as_object() {
        Some(map) => map,
        None => return Value::Null,
    };
    let kind = object.get("$format").and_then(Value::as_str).unwrap_or("");
    let resolved = object
        .get("value")
        .map(|value| resolve_prop_value(value, scope))
        .unwrap_or(Value::Null);

    let formatted = match kind {
        "date" => format_date(&resolved, object),
        "currency" => {
            let symbol = currency_symbol(
                object
                    .get("currency")
                    .and_then(Value::as_str)
                    .unwrap_or("USD"),
            );
            format!("{symbol}{}", format_number_grouped(to_number(&resolved)))
        }
        "number" => format_number_grouped(to_number(&resolved)),
        "percent" => format!("{}%", format_number_grouped(to_number(&resolved) * 100.0)),
        _ => coerce_to_string(&resolved),
    };
    Value::String(formatted)
}

fn currency_symbol(code: &str) -> &'static str {
    match code {
        "EUR" => "€",
        "GBP" => "£",
        "JPY" | "CNY" => "¥",
        _ => "$",
    }
}

/// Best-effort date rendering with no `chrono`: a millisecond epoch
/// number becomes a UTC `YYYY-MM-DD`; an ISO-ish string is taken up to
/// its `T`; `style:"relative"` produces `Nd/h/m/s ago|from now`.
fn format_date(value: &Value, object: &serde_json::Map<String, Value>) -> String {
    let relative = object.get("style").and_then(Value::as_str) == Some("relative");
    if relative {
        let now = object
            .get("now")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let then = to_number(value);
        let diff = now - then;
        if diff == 0.0 {
            return "just now".to_owned();
        }
        let suffix = if diff > 0.0 { "ago" } else { "from now" };
        let abs_seconds = (diff.abs() / 1000.0).floor();
        let minutes = (abs_seconds / 60.0).floor();
        let hours = (minutes / 60.0).floor();
        let days = (hours / 24.0).floor();
        if days > 0.0 {
            return format!("{days:.0}d {suffix}");
        }
        if hours > 0.0 {
            return format!("{hours:.0}h {suffix}");
        }
        if minutes > 0.0 {
            return format!("{minutes:.0}m {suffix}");
        }
        return format!("{abs_seconds:.0}s {suffix}");
    }
    match value {
        Value::Number(_) => epoch_millis_to_iso_date(to_number(value)),
        Value::String(text) => text.split('T').next().unwrap_or(text).to_owned(),
        other => coerce_to_string(other),
    }
}

/// Converts a millisecond epoch to a UTC `YYYY-MM-DD` with a self-
/// contained civil-from-days algorithm (Howard Hinnant's), so no date
/// crate is needed and it stays total.
fn epoch_millis_to_iso_date(millis: f64) -> String {
    #[allow(clippy::cast_possible_truncation)]
    let days = (millis / 86_400_000.0).floor() as i64;
    // days since 1970-01-01 → civil date (era-based, proleptic Gregorian).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
}

fn resolve_math(raw: &Value, scope: &ResolveScope<'_>) -> Value {
    let object = match raw.as_object() {
        Some(map) => map,
        None => return number_value(0.0),
    };
    let operation = object.get("$math").and_then(Value::as_str).unwrap_or("");
    let a = to_number(
        &object
            .get("a")
            .map(|value| resolve_prop_value(value, scope))
            .unwrap_or(Value::Null),
    );
    let b = to_number(
        &object
            .get("b")
            .map(|value| resolve_prop_value(value, scope))
            .unwrap_or(Value::Null),
    );
    let result = match operation {
        "add" => a + b,
        "subtract" => a - b,
        "multiply" => a * b,
        "divide" => {
            if b != 0.0 {
                a / b
            } else {
                0.0
            }
        }
        "mod" => {
            if b != 0.0 {
                a % b
            } else {
                0.0
            }
        }
        "min" => a.min(b),
        "max" => a.max(b),
        "round" => a.round(),
        "floor" => a.floor(),
        "ceil" => a.ceil(),
        "abs" => a.abs(),
        _ => a,
    };
    number_value(if result.is_nan() { 0.0 } else { result })
}

fn resolve_concat(raw: &Value, scope: &ResolveScope<'_>) -> Value {
    let parts = raw
        .as_object()
        .and_then(|object| object.get("$concat"))
        .and_then(Value::as_array);
    let Some(parts) = parts else {
        return Value::String(String::new());
    };
    let joined = parts
        .iter()
        .map(|part| {
            let resolved = resolve_prop_value(part, scope);
            if resolved.is_null() {
                String::new()
            } else {
                coerce_to_string(&resolved)
            }
        })
        .collect::<String>();
    Value::String(joined)
}

fn resolve_count(raw: &Value, scope: &ResolveScope<'_>) -> Value {
    let target = raw.as_object().and_then(|object| object.get("$count"));
    let Some(target) = target else {
        return number_value(0.0);
    };
    let resolved = resolve_prop_value(target, scope);
    let count = match &resolved {
        Value::Array(items) => items.len(),
        Value::String(text) => text.chars().count(),
        _ => 0,
    };
    Value::Number(Number::from(count))
}

fn resolve_truncate(raw: &Value, scope: &ResolveScope<'_>) -> Value {
    let object = match raw.as_object() {
        Some(map) => map,
        None => return Value::String(String::new()),
    };
    let resolved = object
        .get("$truncate")
        .map(|value| resolve_prop_value(value, scope))
        .unwrap_or(Value::Null);
    let text = if resolved.is_null() {
        String::new()
    } else {
        coerce_to_string(&resolved)
    };
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let max_length = object
        .get("length")
        .and_then(Value::as_f64)
        .map_or(100, |float| float.max(0.0) as usize);
    let suffix = object
        .get("suffix")
        .and_then(Value::as_str)
        .unwrap_or("...");
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_length {
        Value::String(text)
    } else {
        let head: String = chars.into_iter().take(max_length).collect();
        Value::String(format!("{head}{suffix}"))
    }
}

fn resolve_pluralize(raw: &Value, scope: &ResolveScope<'_>) -> Value {
    let object = match raw.as_object() {
        Some(map) => map,
        None => return Value::String(String::new()),
    };
    let resolved = object
        .get("$pluralize")
        .map(|value| resolve_prop_value(value, scope))
        .unwrap_or(Value::Null);
    let count = to_number(&resolved);
    let zero = object.get("zero").and_then(Value::as_str);
    let one = object.get("one").and_then(Value::as_str).unwrap_or("");
    let other = object.get("other").and_then(Value::as_str).unwrap_or("");
    if count == 0.0 {
        if let Some(zero_text) = zero {
            return Value::String(zero_text.to_owned());
        }
    }
    if count == 1.0 {
        return Value::String(format!("{} {one}", number_value(count).as_string_lossy()));
    }
    Value::String(format!("{} {other}", number_value(count).as_string_lossy()))
}

fn resolve_join(raw: &Value, scope: &ResolveScope<'_>) -> Value {
    let object = match raw.as_object() {
        Some(map) => map,
        None => return Value::String(String::new()),
    };
    let resolved = object
        .get("$join")
        .map(|value| resolve_prop_value(value, scope))
        .unwrap_or(Value::Null);
    let separator = object
        .get("separator")
        .and_then(Value::as_str)
        .unwrap_or(", ");
    match &resolved {
        Value::Array(items) => {
            let joined = items
                .iter()
                .map(|item| {
                    if item.is_null() {
                        String::new()
                    } else {
                        coerce_to_string(item)
                    }
                })
                .collect::<Vec<_>>()
                .join(separator);
            Value::String(joined)
        }
        Value::Null => Value::String(String::new()),
        other => Value::String(coerce_to_string(other)),
    }
}

/// `$t` with an empty built-in message table: returns the key with any
/// `{{param}}` placeholders interpolated from resolved `params` — a host
/// that needs real translation registers its own `$t` (the upstream
/// `createI18nDirective` factory pattern).
fn resolve_i18n(raw: &Value, scope: &ResolveScope<'_>) -> Value {
    let object = match raw.as_object() {
        Some(map) => map,
        None => return Value::String(String::new()),
    };
    let mut template = object
        .get("$t")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if let Some(params) = object.get("params").and_then(Value::as_object) {
        for (key, value) in params {
            let resolved = resolve_prop_value(value, scope);
            let replacement = if resolved.is_null() {
                String::new()
            } else {
                coerce_to_string(&resolved)
            };
            template = template.replace(&format!("{{{{{key}}}}}"), &replacement);
        }
    }
    Value::String(template)
}

/// A tiny helper so `$pluralize` can stringify its (already integral)
/// count without re-deriving the int/float split.
trait AsStringLossy {
    fn as_string_lossy(&self) -> String;
}

impl AsStringLossy for Value {
    fn as_string_lossy(&self) -> String {
        coerce_to_string(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::DataModel;
    use serde_json::json;

    fn scope_with() -> (
        std::collections::BTreeMap<String, super::super::expr::ComputedFn>,
        DirectiveRegistry,
    ) {
        (
            std::collections::BTreeMap::new(),
            DirectiveRegistry::with_builtins(),
        )
    }

    #[test]
    fn registry_rejects_bad_and_colliding_names() {
        let mut registry = DirectiveRegistry::new();
        assert!(
            registry
                .register(Directive {
                    name: "noDollar",
                    description: "",
                    resolve: |_, _| Value::Null,
                })
                .is_err()
        );
        assert!(
            registry
                .register(Directive {
                    name: "$state",
                    description: "",
                    resolve: |_, _| Value::Null,
                })
                .is_err()
        );
        assert!(
            registry
                .register(Directive {
                    name: "$ok",
                    description: "",
                    resolve: |_, _| Value::Null,
                })
                .is_ok()
        );
    }

    #[test]
    fn ambiguous_directive_is_recoverable_error() {
        let registry = DirectiveRegistry::with_builtins();
        let object = json!({ "$concat": ["a"], "$join": [1, 2] });
        assert!(registry.find(object.as_object().unwrap()).is_err());
        // And the resolver degrades it instead of panicking.
        let model = DataModel::new();
        let (functions, directives) = scope_with();
        let scope = ResolveScope::new(&model, &functions, &directives);
        assert_eq!(
            resolve_prop_value(&object, &scope),
            json!("[ambiguous directive]")
        );
    }

    #[test]
    fn math_concat_count_truncate_pluralize_join() {
        let model = DataModel::from_root(json!({ "xs": [1, 2, 3], "name": "Ada" }));
        let (functions, directives) = scope_with();
        let scope = ResolveScope::new(&model, &functions, &directives);

        assert_eq!(
            resolve_prop_value(&json!({ "$math": "add", "a": 2, "b": 3 }), &scope),
            json!(5)
        );
        assert_eq!(
            resolve_prop_value(&json!({ "$math": "divide", "a": 1, "b": 0 }), &scope),
            json!(0)
        );
        assert_eq!(
            resolve_prop_value(
                &json!({ "$concat": ["Hi ", { "$state": "/name" }, "!"] }),
                &scope
            ),
            json!("Hi Ada!")
        );
        assert_eq!(
            resolve_prop_value(&json!({ "$count": { "$state": "/xs" } }), &scope),
            json!(3)
        );
        assert_eq!(
            resolve_prop_value(
                &json!({ "$truncate": "abcdefgh", "length": 3, "suffix": "…" }),
                &scope
            ),
            json!("abc…")
        );
        assert_eq!(
            resolve_prop_value(
                &json!({ "$pluralize": 0, "zero": "no items", "one": "item", "other": "items" }),
                &scope
            ),
            json!("no items")
        );
        assert_eq!(
            resolve_prop_value(
                &json!({ "$pluralize": 1, "one": "item", "other": "items" }),
                &scope
            ),
            json!("1 item")
        );
        assert_eq!(
            resolve_prop_value(
                &json!({ "$join": { "$state": "/xs" }, "separator": "-" }),
                &scope
            ),
            json!("1-2-3")
        );
    }

    #[test]
    fn format_best_effort_without_intl() {
        let model = DataModel::new();
        let (functions, directives) = scope_with();
        let scope = ResolveScope::new(&model, &functions, &directives);

        assert_eq!(
            resolve_prop_value(
                &json!({ "$format": "currency", "value": 1234.5, "currency": "EUR" }),
                &scope
            ),
            json!("€1,234.5")
        );
        assert_eq!(
            resolve_prop_value(&json!({ "$format": "number", "value": 1234567 }), &scope),
            json!("1,234,567")
        );
        assert_eq!(
            resolve_prop_value(&json!({ "$format": "percent", "value": 0.42 }), &scope),
            json!("42%")
        );
        // 2021-01-01T00:00:00Z = 1609459200000 ms.
        assert_eq!(
            resolve_prop_value(
                &json!({ "$format": "date", "value": 1_609_459_200_000_i64 }),
                &scope
            ),
            json!("2021-01-01")
        );
        assert_eq!(
            resolve_prop_value(
                &json!({ "$format": "date", "value": "2024-03-09T12:00:00Z" }),
                &scope
            ),
            json!("2024-03-09")
        );
    }

    #[test]
    fn i18n_interpolates_params_into_key() {
        let model = DataModel::from_root(json!({ "who": "world" }));
        let (functions, directives) = scope_with();
        let scope = ResolveScope::new(&model, &functions, &directives);
        assert_eq!(
            resolve_prop_value(
                &json!({ "$t": "Hello, {{name}}!", "params": { "name": { "$state": "/who" } } }),
                &scope
            ),
            json!("Hello, world!")
        );
    }
}
