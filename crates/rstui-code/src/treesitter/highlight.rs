//! Mapping a tree-sitter highlight query's capture names onto the
//! [`SyntaxStyles`] theme buckets, and
//! flattening the result into the exact per-character [`Style`] overlay the
//! Tier-0 floor already feeds [`Editor::syntax`](crate::Editor).
//!
//! # Capture → bucket map
//!
//! tree-sitter highlight captures are dotted names (`keyword.control`,
//! `string.special`, …). Tier-1's job is *accuracy* — a real parse tree can
//! tell a function name from a type from a plain variable, which the Tier-0
//! left-to-right scanner cannot. Every capture is folded onto one of the
//! semantic buckets below, or dropped (left as `Style::new()`, i.e. no
//! colour, exactly like Tier-0's sentinel):
//!
//! | capture name (prefix-matched) | bucket |
//! |---|---|
//! | `keyword`, `keyword.*`, `conditional`, `repeat`, `include`, `storageclass`, `type.qualifier` | **keyword** |
//! | `string`, `string.*`, `character`, `char` | **string** |
//! | `number`, `float`, `constant.numeric`, `constant.numeric.*`, `boolean`, `constant.builtin`, `constant.builtin.*` | **number** |
//! | `comment`, `comment.*` | **comment** |
//! | `function`, `function.*`, `method`, `constructor`, `constructor.*` | **function** |
//! | `type`, `type.builtin`, `type.definition`, `class` (but **not** `type.qualifier`, already keyword) | **type_** |
//! | `constant`, `constant.macro` (but **not** `constant.numeric`/`constant.builtin`, already number), `label` | **constant** |
//! | `variable`, `variable.*`, `property`, `property.*`, `field`, `parameter` | **variable** |
//! | `attribute`, `attribute.*`, `annotation`, `decorator`, `decorator.*` | **attribute** |
//! | `namespace`, `namespace.*`, `module` | **namespace** |
//! | `operator` | **operator** |
//! | `punctuation`, `punctuation.*` | **punctuation** |
//! | `escape` (bare string-escape; `string.escape`/`string.special` already string) | **string** |
//! | `delimiter` (bare; `punctuation.delimiter` already punctuation) | **punctuation** |
//! | `text.title`, `markup.heading*` (markdown headings) | **type_** |
//! | `text.literal`, `markup.raw*`, `text.uri`, `markup.link*` | **string** |
//! | `text.reference` (link/reference label) | **function** |
//! | everything else | none (`Style::new()`) |
//!
//! Every capture every shipped grammar's bundled `highlights.scm` can emit
//! is covered by the rows above **or** is on the audit allowlist
//! (`@embedded` injection regions, `@none`, spell/conceal hints). The
//! `every_grammar_highlight_capture_is_classified_or_allowlisted` test
//! compiles each grammar's real query and fails CI if any capture is
//! neither — the gate-enforced "we handle them all" invariant (a grammar
//! bump that adds a capture turns it red).
//!
//! The match is "exact name, or name starts with `prefix.`", so
//! `keyword.control.return` → keyword and `string.special.key` → string.
//! **Order is load-bearing**: the keyword / string / number / comment blocks
//! are tested *first* so the narrower legacy precedence still wins —
//! `constant.numeric` / `constant.builtin` resolve to **number** and
//! `type.qualifier` to **keyword** before the new bare `constant` / `type`
//! rules can see them.
//!
//! ## Why `constant.builtin` joins the number bucket
//!
//! ADR 0022's table maps `boolean` → **number**. Several pinned grammars
//! have **no `@number` capture** and instead tag numeric literals with
//! `@constant.builtin`: `tree-sitter-rust`'s `highlights.scm` captures
//! `integer_literal`, `float_literal` *and* `boolean_literal` *all* as
//! `@constant.builtin`. Elsewhere `@constant.builtin` is the
//! boolean-class literal anyway (Python `True`/`False`/`None`, JS
//! `null`/`undefined`) — exactly the `boolean → number` case the table
//! already names. So folding `constant.builtin` into the number bucket is
//! the *faithful* reading of ADR 0022's own rule (not a widening): it is
//! what makes a Rust `42` actually take the theme's number colour, which is
//! the whole point of Tier-1 over the heuristic floor. Python/JS/C/Go *do*
//! emit `@number`, handled by the plain `number` rule.
//!
//! # Flattened layout — a drop-in for `Editor::syntax`
//!
//! `rstui-git-review`'s `rebuild_edit_overlays` builds the overlay as: for
//! each logical row, the row's per-char styles, then **one** extra
//! `Style::new()` slot for the `'\n'` joining it to the next row (no
//! trailing newline slot after the last row). The document tree-sitter
//! parses is the rows joined by `'\n'` — i.e.
//! [`TextArea::to_string()`](rstui_core::TextArea). So one `Style` slot per
//! source byte-position *including* each `'\n'` is exactly
//! `src.chars().count()` slots, and a capture's UTF-8 byte range maps onto
//! char slots by walking `char_indices()`. The produced `Vec<Style>` is
//! therefore length-identical and index-identical to Tier-0's, a true
//! drop-in for `.syntax(&overlay)`.

use crate::syntax::SyntaxStyles;
use rstui_core::Style;
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator, Tree};

/// Which of the [`SyntaxStyles`] buckets a capture name maps to, or
/// `None` for an uncoloured capture.
#[derive(Clone, Copy)]
enum Bucket {
    Keyword,
    String,
    Number,
    Comment,
    Function,
    Type,
    Constant,
    Variable,
    Operator,
    Punctuation,
    Attribute,
    Namespace,
}

/// `true` if `name` is exactly `key`, or is `key` followed by a `.` (a more
/// specific sub-capture like `keyword.control`).
fn matches(name: &str, key: &str) -> bool {
    name == key
        || (name.len() > key.len() && name.starts_with(key) && name.as_bytes()[key.len()] == b'.')
}

/// Whether `name` is classified into a [`SyntaxStyles`] bucket (i.e. it
/// receives a token colour). The capture-coverage audit test asserts every
/// capture every shipped grammar can emit is either classified here or on
/// its documented allowlist — the gate-enforced "we handle them all"
/// invariant. A `bool` (not the private [`Bucket`]) so the audit can call
/// it without leaking module-private types. Test-only (the audit is the
/// sole caller), so it does not exist in the non-test build.
#[cfg(test)]
pub(crate) fn is_capture_classified(name: &str) -> bool {
    bucket_for(name).is_some()
}

/// The bucket for a tree-sitter capture name per the documented table.
/// Order matters: `constant.numeric` must be tested before a bare
/// `constant` would (a bare `constant` is intentionally *not* a number).
fn bucket_for(name: &str) -> Option<Bucket> {
    // keyword family
    if matches(name, "keyword")
        || name == "conditional"
        || name == "repeat"
        || name == "include"
        || name == "storageclass"
        || matches(name, "type.qualifier")
    {
        return Some(Bucket::Keyword);
    }
    // string family
    if matches(name, "string") || name == "character" || name == "char" {
        return Some(Bucket::String);
    }
    // number family. `constant.builtin` is included because several pinned
    // grammars (notably rust) have no `@number` and tag numeric *and*
    // boolean literals as `@constant.builtin`; ADR 0022 already maps
    // `boolean` → number, so this is the faithful reading (see module docs).
    if name == "number"
        || name == "float"
        || matches(name, "constant.numeric")
        || name == "boolean"
        || matches(name, "constant.builtin")
    {
        return Some(Bucket::Number);
    }
    // comment family
    if matches(name, "comment") {
        return Some(Bucket::Comment);
    }
    // --- New semantic classes. These come AFTER the four blocks above so
    // their narrower precedence still wins: `constant.numeric` /
    // `constant.builtin` already returned Number and `type.qualifier`
    // already returned Keyword, so they never reach the bare `constant` /
    // `type` rules here. ---
    // function family (`function` / `function.*`, `method`, and exact
    // `constructor` or any `constructor.*` — `matches` covers both).
    if matches(name, "function") || name == "method" || matches(name, "constructor") {
        return Some(Bucket::Function);
    }
    // type family (bare / `type.builtin` / `type.definition`;
    // `type.qualifier` already returned Keyword above).
    if matches(name, "type") || name == "class" {
        return Some(Bucket::Type);
    }
    // constant family (bare / `constant.macro`; numeric / builtin already
    // returned Number above). `matches` covers exact `constant` and any
    // `constant.*` not already routed to Number by the block above.
    if matches(name, "constant") || name == "label" {
        return Some(Bucket::Constant);
    }
    // variable family (identifiers, parameters, fields / properties)
    if matches(name, "variable")
        || matches(name, "property")
        || name == "field"
        || name == "parameter"
    {
        return Some(Bucket::Variable);
    }
    // attribute family (attributes / decorators / annotations)
    if matches(name, "attribute") || name == "annotation" || matches(name, "decorator") {
        return Some(Bucket::Attribute);
    }
    // namespace family (modules / namespaces)
    if matches(name, "namespace") || name == "module" {
        return Some(Bucket::Namespace);
    }
    if name == "operator" {
        return Some(Bucket::Operator);
    }
    if matches(name, "punctuation") {
        return Some(Bucket::Punctuation);
    }
    // A string-escape sequence (`\n`, `\"`, `\u{…}`) — emitted as a bare
    // `@escape` by rust/python/go/json (the `@string.escape` /
    // `@string.special` forms are already routed to String by the string
    // block above). Colour it as part of the string it lives in.
    if name == "escape" {
        return Some(Bucket::String);
    }
    // A bare `@delimiter` (some C-family grammars) is punctuation
    // (`@punctuation.delimiter` is already handled by the block above).
    if name == "delimiter" {
        return Some(Bucket::Punctuation);
    }
    // Markdown / markup prose (the `tree-sitter-md` highlight set):
    // a heading pops like a type, inline code / a URI reads like a string,
    // a link/reference label like a callable accent. (`@punctuation.*`,
    // `@string.escape` and `@none` from that grammar are already handled /
    // allowlisted.)
    if matches(name, "text.title") || matches(name, "markup.heading") {
        return Some(Bucket::Type);
    }
    if matches(name, "text.literal")
        || matches(name, "markup.raw")
        || matches(name, "text.uri")
        || matches(name, "markup.link")
    {
        return Some(Bucket::String);
    }
    if matches(name, "text.reference") {
        return Some(Bucket::Function);
    }
    None
}

/// The concrete [`Style`] for a bucket, from the caller's theme.
fn style_for(bucket: Bucket, styles: &SyntaxStyles) -> Style {
    match bucket {
        Bucket::Keyword => styles.keyword,
        Bucket::String => styles.string,
        Bucket::Number => styles.number,
        Bucket::Comment => styles.comment,
        Bucket::Function => styles.function,
        Bucket::Type => styles.type_,
        Bucket::Constant => styles.constant,
        Bucket::Variable => styles.variable,
        Bucket::Operator => styles.operator,
        Bucket::Punctuation => styles.punctuation,
        Bucket::Attribute => styles.attribute,
        Bucket::Namespace => styles.namespace,
    }
}

/// Builds the flattened per-character [`Style`] overlay for `src` (the
/// document tree-sitter parsed — rows joined by `'\n'`) from `tree` using
/// `query`'s highlight captures, painting each captured byte range with its
/// bucket's themed [`Style`].
///
/// The returned vector always has length `src.chars().count()` (one slot
/// per source char *including* each `'\n'`), so it is a drop-in for
/// [`Editor::syntax`](crate::Editor). Total: any tree / query /
/// source, never panics.
///
/// Later captures overwrite earlier ones on overlapping ranges — tree-sitter
/// emits more-specific patterns after the general ones, so the more specific
/// classification wins (the conventional highlight precedence).
pub(crate) fn highlight(
    src: &str,
    tree: &Tree,
    query: &Query,
    styles: &SyntaxStyles,
) -> Vec<Style> {
    // One Style slot per char, indexed by char position.
    let char_count = src.chars().count();
    let mut overlay = vec![Style::new(); char_count];
    if char_count == 0 {
        return overlay;
    }

    // A byte-offset → char-index map so a capture's UTF-8 byte range can be
    // painted onto char slots. `byte_to_char[b]` is the char index covering
    // byte `b` (a multi-byte char's continuation bytes resolve to that
    // char's index; `src.len()` maps to `char_count`). Built directly per
    // byte so every entry is meaningful (no sentinel) and monotonic.
    let mut byte_to_char = vec![char_count; src.len() + 1];
    for (ci, (b, ch)) in src.char_indices().enumerate() {
        for slot in byte_to_char.iter_mut().take(b + ch.len_utf8()).skip(b) {
            *slot = ci;
        }
    }

    // Map capture index → bucket once (the query's capture-name list is
    // stable for the query's lifetime).
    let names = query.capture_names();
    let buckets: Vec<Option<Bucket>> = names.iter().map(|n| bucket_for(n)).collect();

    let mut cursor = QueryCursor::new();
    let src_bytes = src.as_bytes();
    let mut it = cursor.captures(query, tree.root_node(), src_bytes);
    while let Some((m, _)) = it.next() {
        for cap in m.captures {
            let Some(Some(bucket)) = buckets.get(cap.index as usize).copied() else {
                continue; // out of range or an uncoloured capture
            };
            let node: Node = cap.node;
            let sb = node.start_byte().min(src.len());
            let eb = node.end_byte().min(src.len());
            let s = byte_to_char[sb];
            let e = byte_to_char[eb];
            let st = style_for(bucket, styles);
            for slot in overlay.iter_mut().take(e).skip(s) {
                *slot = st;
            }
        }
    }
    overlay
}
