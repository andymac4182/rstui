//! A2UI `Dynamic*` value resolution — the literal / `{path}` /
//! `FunctionCall` dispatch, the 14 basic-catalog functions, and the
//! `formatString` `${…}` interpolation mini-grammar.
//!
//! # What a bindable prop is
//!
//! A2UI's `common_types.json` makes nearly every component property a
//! `DynamicString`/`DynamicNumber`/`DynamicBoolean`: a JSON literal, a
//! `{"path": "/json/pointer"}` data binding, or a `{"call": "...",
//! "args": {...}}` function call. [`resolve`] is the single, **total**
//! evaluator both the catalog projection and the action layer call: it
//! resolves a value once against the caller-owned
//! [`DataModel`] at a given relative scope (the
//! `ChildList` template instance case), exactly the
//! `DataContext.resolveDynamicValue` semantics of the reference engine,
//! but synchronous and panic-free — there is no retained reactive graph
//! (ADR 0012), the document is re-projected every frame.
//!
//! # The function table
//!
//! [`resolve`] dispatches the 14 functions of `basic_catalog.json`
//! (`required`, `regex`, `length`, `numeric`, `email`, `formatString`,
//! `formatNumber`, `formatCurrency`, `formatDate`, `pluralize`,
//! `openUrl`, `and`, `or`, `not`). They mirror `basic_functions.ts`:
//! `regex` is a contained literal-substring/anchor matcher (no regex
//! crate — an unsupported pattern degrades to "matches", never a panic);
//! `formatDate` is best-effort identity (no `chrono`); `openUrl` returns
//! void and is surfaced as an action by the [`actions`](super::actions)
//! layer. An unknown function name resolves to JSON `null` — the
//! progressive-rendering contract, not an error.
//!
//! # The `formatString` grammar
//!
//! [`format_string`] ports `expression_parser.ts`: `${…}` blocks holding
//! a JSON-Pointer path, a named-argument function call
//! (`${formatDate(value:${/d}, format:'MM-dd')}`), or a quoted-string /
//! number / `true`/`false`/`null` literal; brace-balanced and
//! quote-aware extraction; `\${` escapes a literal `${`; nesting is
//! depth-capped (10, the reference's `MAX_DEPTH`) so a hostile document
//! cannot exhaust the stack. Every parse failure degrades to the
//! best-effort partial string rather than panicking.

use serde_json::{Map, Value};

use crate::value::DataModel;

/// The maximum `${…}`/argument nesting depth, matching the reference
/// `ExpressionParser.MAX_DEPTH`. Beyond it, interpolation stops and the
/// raw text is kept (totality over a hostile, deeply-nested document).
pub const MAX_EXPRESSION_DEPTH: usize = 10;

/// Resolves a `Dynamic*` value (literal / `{path}` / `FunctionCall`)
/// against `model` at relative `scope`, once. Total: an unresolvable
/// path or unknown function yields [`Value::Null`] rather than panicking
/// (the A2UI progressive-rendering contract).
///
/// `scope` is the absolute JSON Pointer a `ChildList` template instance
/// is scoped to (`""`/`/` for the surface root); a relative `{path}` is
/// resolved against it via
/// [`get_scoped`](crate::value::DataModel::get_scoped).
#[must_use]
pub fn resolve(node: &Value, model: &DataModel, scope: &str) -> Value {
    resolve_at(node, model, scope, 0)
}

fn resolve_at(node: &Value, model: &DataModel, scope: &str, depth: usize) -> Value {
    if depth > MAX_EXPRESSION_DEPTH {
        return Value::Null;
    }
    match node {
        // Literals (string/number/bool/array/null) pass through; an
        // array is a literal per the schema's DynamicValue.oneOf.
        Value::Object(fields) => {
            if let Some(Value::String(pointer)) = fields.get("path") {
                if fields.len() == 1 {
                    return model
                        .get_scoped(scope, pointer)
                        .cloned()
                        .unwrap_or(Value::Null);
                }
            }
            if let Some(Value::String(name)) = fields.get("call") {
                return call_function(name, fields, model, scope, depth);
            }
            // A literal object argument (FunctionCall.args allows one).
            node.clone()
        }
        other => other.clone(),
    }
}

/// Resolves a `Dynamic*` value to its display string with the same
/// coercion both reference engines use (string verbatim; number/bool
/// stringified; null/absent ⇒ `""`; array/object ⇒ compact JSON).
#[must_use]
pub fn resolve_text(node: &Value, model: &DataModel, scope: &str) -> String {
    coerce_text(&resolve(node, model, scope))
}

/// Resolves a `Dynamic*` value to a boolean with JS-`!!`-style
/// truthiness (the reference `and`/`or`/`not`/check semantics): `false`,
/// `0`, `""`, `null`, `[]`, `{}` are falsey; everything else is truthy.
#[must_use]
pub fn resolve_bool(node: &Value, model: &DataModel, scope: &str) -> bool {
    truthy(&resolve(node, model, scope))
}

/// Resolves a `Dynamic*` value to an `f64`, or `None` if it is not a
/// number (or a numeric string — the reference coerces with `Number()`).
#[must_use]
pub fn resolve_number(node: &Value, model: &DataModel, scope: &str) -> Option<f64> {
    as_number(&resolve(node, model, scope))
}

/// JS-`!!`-style truthiness over a resolved [`Value`].
#[must_use]
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|float| float != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

/// Display-text coercion shared with [`DataModel::get_text`](crate::value::DataModel::get_text).
#[must_use]
pub fn coerce_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        Value::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// Invokes one basic-catalog function by name, resolving each argument
/// against `model`/`scope` first (the reference `resolveDynamicValue`
/// recursion). An unknown name ⇒ [`Value::Null`] (totality).
fn call_function(
    name: &str,
    call: &Map<String, Value>,
    model: &DataModel,
    scope: &str,
    depth: usize,
) -> Value {
    let empty = Map::new();
    let raw_args = match call.get("args") {
        Some(Value::Object(args)) => args,
        _ => &empty,
    };
    let argument = |key: &str| -> Value {
        raw_args
            .get(key)
            .map(|value| resolve_at(value, model, scope, depth + 1))
            .unwrap_or(Value::Null)
    };
    match name {
        "required" => {
            let value = argument("value");
            Value::Bool(match &value {
                Value::Null => false,
                Value::String(text) => !text.is_empty(),
                Value::Array(items) => !items.is_empty(),
                _ => true,
            })
        }
        "regex" => {
            let text = coerce_text(&argument("value"));
            let pattern = coerce_text(&argument("pattern"));
            Value::Bool(regex_like_match(&pattern, &text))
        }
        "length" => {
            let length = match argument("value") {
                Value::String(text) => text.chars().count() as i64,
                Value::Array(items) => items.len() as i64,
                _ => 0,
            };
            let min = as_number(&argument("min"));
            let max = as_number(&argument("max"));
            let ok = min.is_none_or(|low| length as f64 >= low)
                && max.is_none_or(|high| length as f64 <= high);
            Value::Bool(ok)
        }
        "numeric" => {
            let Some(value) = as_number(&argument("value")) else {
                return Value::Bool(false);
            };
            let min = as_number(&argument("min"));
            let max = as_number(&argument("max"));
            let ok = min.is_none_or(|low| value >= low) && max.is_none_or(|high| value <= high);
            Value::Bool(ok)
        }
        "email" => Value::Bool(looks_like_email(&coerce_text(&argument("value")))),
        "formatString" => {
            let template = coerce_text(&argument("value"));
            Value::String(format_string(&template, model, scope, depth + 1))
        }
        "formatNumber" => {
            let Some(value) = as_number(&argument("value")) else {
                return Value::String(String::new());
            };
            let decimals = as_number(&argument("decimals")).map(|float| float as usize);
            let grouping = match argument("grouping") {
                Value::Bool(flag) => flag,
                Value::Null => true,
                other => truthy(&other),
            };
            Value::String(format_decimal(value, decimals, grouping))
        }
        "formatCurrency" => {
            let Some(value) = as_number(&argument("value")) else {
                return Value::String(String::new());
            };
            let currency = coerce_text(&argument("currency"));
            let decimals = as_number(&argument("decimals"))
                .map(|float| float as usize)
                .unwrap_or(2);
            let grouping = match argument("grouping") {
                Value::Bool(flag) => flag,
                Value::Null => true,
                other => truthy(&other),
            };
            let body = format_decimal(value, Some(decimals), grouping);
            Value::String(if currency.is_empty() {
                body
            } else {
                format!("{currency} {body}")
            })
        }
        "formatDate" => {
            // Best-effort identity (no chrono): the value is already an
            // ISO-8601 string in practice; pass it through verbatim.
            Value::String(coerce_text(&argument("value")))
        }
        "pluralize" => {
            let count = as_number(&argument("value")).unwrap_or(0.0);
            // English CLDR: 1 ⇒ "one", everything else ⇒ "other"
            // (the reference's Intl.PluralRules default-locale rule).
            let category = if (count - 1.0).abs() < f64::EPSILON {
                "one"
            } else if count == 0.0 && raw_args.contains_key("zero") {
                "zero"
            } else {
                "other"
            };
            let chosen = argument(category);
            let text = if matches!(chosen, Value::Null) {
                coerce_text(&argument("other"))
            } else {
                coerce_text(&chosen)
            };
            Value::String(text)
        }
        "openUrl" => Value::Null, // void; surfaced as an action elsewhere.
        "and" => Value::Bool(bool_list(raw_args, model, scope, depth).iter().all(|&b| b)),
        "or" => Value::Bool(bool_list(raw_args, model, scope, depth).iter().any(|&b| b)),
        "not" => Value::Bool(!truthy(&argument("value"))),
        _ => Value::Null,
    }
}

fn bool_list(
    raw_args: &Map<String, Value>,
    model: &DataModel,
    scope: &str,
    depth: usize,
) -> Vec<bool> {
    match raw_args.get("values") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| truthy(&resolve_at(item, model, scope, depth + 1)))
            .collect(),
        _ => Vec::new(),
    }
}

/// A contained stand-in for the reference `new RegExp(pattern).test()`:
/// honours `^`/`$` anchors and treats the rest as a literal needle (no
/// regex engine in this crate). An empty pattern always matches; this is
/// deliberately permissive so a real regex never blanks the screen — the
/// `Button`/`TextField` check just stays enabled.
fn regex_like_match(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    let anchored_start = pattern.starts_with('^');
    let anchored_end = pattern.ends_with('$');
    let core = pattern
        .strip_prefix('^')
        .unwrap_or(pattern)
        .strip_suffix('$')
        .unwrap_or_else(|| pattern.strip_prefix('^').unwrap_or(pattern));
    // A core with regex metacharacters can't be checked literally;
    // accept (permissive — never a false "invalid").
    if core.chars().any(|c| "[](){}*+?.|\\".contains(c)) {
        return true;
    }
    match (anchored_start, anchored_end) {
        (true, true) => text == core,
        (true, false) => text.starts_with(core),
        (false, true) => text.ends_with(core),
        (false, false) => text.contains(core),
    }
}

/// The reference engine's deliberately-simple email regex, ported as a
/// structural check (`local@domain.tld`, no spaces, a dotted domain with
/// a ≥2-char alphabetic TLD).
fn looks_like_email(text: &str) -> bool {
    let mut parts = text.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    if local.is_empty()
        || !local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-_%+".contains(c))
    {
        return false;
    }
    let Some((host, tld)) = domain.rsplit_once('.') else {
        return false;
    };
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
}

/// Fixed-decimal formatting with optional thousands grouping (a small,
/// locale-free stand-in for `Intl.NumberFormat`: `1234.5 → "1,234.50"`).
fn format_decimal(value: f64, decimals: Option<usize>, grouping: bool) -> String {
    let places = decimals.unwrap_or(0);
    let negative = value.is_sign_negative() && value != 0.0;
    let formatted = format!("{:.*}", places, value.abs());
    let (integer, fraction) = match formatted.split_once('.') {
        Some((whole, frac)) => (whole.to_owned(), Some(frac.to_owned())),
        None => (formatted.clone(), None),
    };
    let grouped = if grouping {
        group_thousands(&integer)
    } else {
        integer
    };
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&grouped);
    if let Some(frac) = fraction {
        out.push('.');
        out.push_str(&frac);
    }
    out
}

fn group_thousands(integer: &str) -> String {
    let digits: Vec<char> = integer.chars().collect();
    let mut out = String::new();
    for (index, digit) in digits.iter().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(*digit);
    }
    out
}

// --- formatString `${…}` interpolation ---------------------------------

/// Interpolates a `formatString` template against `model`/`scope`,
/// porting `expression_parser.ts`: `${…}` blocks (path / named-arg
/// function call / quoted-string / number / bool / null), `\${` escapes,
/// brace- and quote-aware extraction, depth-capped nesting. Any malformed
/// expression contributes its best-effort partial rather than panicking.
#[must_use]
pub fn format_string(template: &str, model: &DataModel, scope: &str, depth: usize) -> String {
    if depth > MAX_EXPRESSION_DEPTH || !template.contains("${") {
        return template.to_owned();
    }
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\\' && peek(&chars, index + 1, '$') && peek(&chars, index + 2, '{') {
            out.push_str("${");
            index += 3;
            continue;
        }
        if chars[index] == '$' && peek(&chars, index + 1, '{') {
            index += 2;
            let (content, next) = extract_interpolation(&chars, index);
            index = next;
            let value = evaluate_expression(&content, model, scope, depth + 1);
            out.push_str(&coerce_text(&value));
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn peek(chars: &[char], index: usize, expected: char) -> bool {
    chars.get(index) == Some(&expected)
}

/// Brace-balanced, quote-aware extraction of one `${…}` body (the
/// reference `extractInterpolationContent`). Returns the inner text and
/// the index just past the closing `}`; an unclosed block consumes to
/// end-of-input (total).
fn extract_interpolation(chars: &[char], start: usize) -> (String, usize) {
    let mut depth = 1;
    let mut index = start;
    while index < chars.len() && depth > 0 {
        let current = chars[index];
        index += 1;
        match current {
            '{' => depth += 1,
            '}' => depth -= 1,
            '\'' | '"' => {
                let quote = current;
                while index < chars.len() {
                    let inner = chars[index];
                    index += 1;
                    if inner == '\\' {
                        index += 1;
                    } else if inner == quote {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    let end = if depth == 0 { index - 1 } else { index };
    (chars[start..end].iter().collect(), index)
}

/// Evaluates a single `${…}` expression body: a nested `${…}`, a quoted
/// string / number / `true`/`false`/`null` literal, a function call
/// `name(arg:expr, …)`, or otherwise a JSON-Pointer path.
fn evaluate_expression(expr: &str, model: &DataModel, scope: &str, depth: usize) -> Value {
    if depth > MAX_EXPRESSION_DEPTH {
        return Value::Null;
    }
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Value::String(String::new());
    }
    let chars: Vec<char> = trimmed.chars().collect();
    // Nested interpolation block.
    if chars.first() == Some(&'$') && peek(&chars, 1, '{') {
        let (inner, _) = extract_interpolation(&chars, 2);
        return evaluate_expression(&inner, model, scope, depth + 1);
    }
    // Quoted string literal.
    if let Some(quote @ ('\'' | '"')) = chars.first().copied() {
        return Value::String(parse_string_literal(&chars[1..], quote));
    }
    // Numeric literal.
    if chars[0].is_ascii_digit() {
        let literal: String = chars
            .iter()
            .take_while(|c| c.is_ascii_digit() || **c == '.')
            .collect();
        return literal
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or(Value::Null, Value::Number);
    }
    // Keyword literals.
    match trimmed {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" => return Value::String(String::new()),
        _ => {}
    }
    // Identifier — a function call if followed by `(`, else a path.
    let token: String = chars
        .iter()
        .take_while(|c| {
            c.is_ascii_alphanumeric() || **c == '/' || **c == '.' || **c == '_' || **c == '-'
        })
        .collect();
    let rest: String = chars[token.chars().count()..]
        .iter()
        .collect::<String>()
        .trim_start()
        .to_owned();
    if rest.starts_with('(') {
        let call = parse_function_call(&token, &rest, model, scope, depth);
        return call_function(&token, &call, model, scope, depth);
    }
    if token.is_empty() {
        return Value::String(String::new());
    }
    model
        .get_scoped(scope, &token)
        .cloned()
        .unwrap_or(Value::Null)
}

fn parse_string_literal(body: &[char], quote: char) -> String {
    let mut out = String::new();
    let mut index = 0;
    while index < body.len() {
        let current = body[index];
        index += 1;
        if current == '\\' {
            match body.get(index) {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => out.push(*other),
                None => {}
            }
            index += 1;
        } else if current == quote {
            break;
        } else {
            out.push(current);
        }
    }
    out
}

/// Parses `(name:expr, …)` into a synthetic `{call, args}` object so it
/// can flow through [`call_function`]. Argument expressions are recorded
/// as already-resolved literals (the reference resolves args eagerly).
fn parse_function_call(
    name: &str,
    rest: &str,
    model: &DataModel,
    scope: &str,
    depth: usize,
) -> Map<String, Value> {
    let chars: Vec<char> = rest.chars().collect();
    let mut index = 1; // past '('
    let mut args = Map::new();
    while index < chars.len() && chars[index] != ')' {
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        let arg_name: String = chars[index..]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
            .collect();
        index += arg_name.chars().count();
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if chars.get(index) != Some(&':') {
            break;
        }
        index += 1;
        let (raw, next) = scan_argument(&chars, index);
        index = next;
        if !arg_name.is_empty() {
            args.insert(
                arg_name,
                evaluate_expression(raw.trim(), model, scope, depth + 1),
            );
        }
        while index < chars.len() && (chars[index].is_whitespace() || chars[index] == ',') {
            index += 1;
        }
    }
    let mut call = Map::new();
    call.insert("call".to_owned(), Value::String(name.to_owned()));
    call.insert("args".to_owned(), Value::Object(args));
    call
}

/// Scans one argument value up to the next top-level `,` or `)`,
/// respecting nested `${…}`/parens and quotes.
fn scan_argument(chars: &[char], start: usize) -> (String, usize) {
    let mut index = start;
    let mut paren = 0;
    let mut brace = 0;
    while index < chars.len() {
        let current = chars[index];
        if (current == ',' || current == ')') && paren == 0 && brace == 0 {
            break;
        }
        match current {
            '(' => paren += 1,
            ')' => paren -= 1,
            '{' => brace += 1,
            '}' => brace -= 1,
            '\'' | '"' => {
                let quote = current;
                index += 1;
                while index < chars.len() {
                    let inner = chars[index];
                    index += 1;
                    if inner == '\\' {
                        index += 1;
                    } else if inner == quote {
                        break;
                    }
                }
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    (chars[start..index].iter().collect(), index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn model() -> DataModel {
        DataModel::from_root(json!({
            "user": { "name": "Ada", "age": 36, "active": true },
            "price": 1234.5,
            "items": ["x", "y"],
            "count": 1,
        }))
    }

    #[test]
    fn literal_path_and_function_dispatch() {
        let store = model();
        assert_eq!(resolve(&json!("hi"), &store, ""), json!("hi"));
        assert_eq!(resolve(&json!(42), &store, ""), json!(42));
        assert_eq!(
            resolve(&json!({"path": "/user/name"}), &store, ""),
            json!("Ada")
        );
        // relative scope (a ChildList template instance)
        assert_eq!(
            resolve(&json!({"path": "name"}), &store, "/user"),
            json!("Ada")
        );
        // unknown path / unknown function → null, never a panic
        assert_eq!(resolve(&json!({"path": "/nope"}), &store, ""), Value::Null);
        assert_eq!(
            resolve(&json!({"call": "mystery", "args": {}}), &store, ""),
            Value::Null
        );
    }

    #[test]
    fn catalog_functions() {
        let store = model();
        let call = |v: Value| resolve(&v, &store, "");
        assert_eq!(
            call(json!({"call":"required","args":{"value":{"path":"/user/name"}}})),
            json!(true)
        );
        assert_eq!(
            call(json!({"call":"required","args":{"value":""}})),
            json!(false)
        );
        assert_eq!(
            call(json!({"call":"email","args":{"value":"a@b.com"}})),
            json!(true)
        );
        assert_eq!(
            call(json!({"call":"email","args":{"value":"nope"}})),
            json!(false)
        );
        assert_eq!(
            call(json!({"call":"length","args":{"value":"abcd","min":2,"max":5}})),
            json!(true)
        );
        assert_eq!(
            call(json!({"call":"numeric","args":{"value":{"path":"/user/age"},"min":18}})),
            json!(true)
        );
        assert_eq!(
            call(json!({"call":"not","args":{"value":{"path":"/user/active"}}})),
            json!(false)
        );
        assert_eq!(
            call(json!({"call":"and","args":{"values":[true,{"path":"/user/active"}]}})),
            json!(true)
        );
        assert_eq!(
            call(json!({"call":"or","args":{"values":[false,false]}})),
            json!(false)
        );
        assert_eq!(
            call(
                json!({"call":"formatCurrency","args":{"value":{"path":"/price"},"currency":"USD"}})
            ),
            json!("USD 1,234.50")
        );
        assert_eq!(
            call(json!({"call":"formatNumber","args":{"value":1234.5,"decimals":1}})),
            json!("1,234.5")
        );
        assert_eq!(
            call(
                json!({"call":"pluralize","args":{"value":{"path":"/count"},"one":"item","other":"items"}})
            ),
            json!("item")
        );
        assert_eq!(
            call(json!({"call":"openUrl","args":{"url":"x"}})),
            Value::Null
        );
        // regex stand-in: anchored literal
        assert_eq!(
            call(json!({"call":"regex","args":{"value":"hello","pattern":"^hel"}})),
            json!(true)
        );
    }

    #[test]
    fn format_string_grammar() {
        let store = model();
        assert_eq!(format_string("Hi ${/user/name}!", &store, "", 0), "Hi Ada!");
        assert_eq!(
            format_string("name ${name}", &store, "/user", 0),
            "name Ada"
        );
        // nested function call with a quoted-string arg
        assert_eq!(
            format_string(
                "${formatCurrency(value:${/price}, currency:'USD')}",
                &store,
                "",
                0
            ),
            "USD 1,234.50"
        );
        // escape: \${ stays literal
        assert_eq!(format_string("cost \\${5}", &store, "", 0), "cost ${5}");
        // literal-only template returns verbatim
        assert_eq!(format_string("plain", &store, "", 0), "plain");
        // unclosed brace must not panic
        let _ = format_string("oops ${/user/name", &store, "", 0);
    }

    #[test]
    fn totality_hostile_and_deep() {
        let store = model();
        // deeply nested ${${${…}}} cannot exhaust the stack
        let bomb = "${".repeat(50) + "/x" + &"}".repeat(50);
        let _ = format_string(&bomb, &store, "", 0);
        // garbage function body
        let _ = format_string("${weird(:::)}", &store, "", 0);
        // non-object/string nodes pass through
        assert_eq!(resolve(&json!([1, 2]), &store, ""), json!([1, 2]));
    }
}
