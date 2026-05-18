//! Turning a tree-sitter `tags.scm` query's `@definition.*` / `@name`
//! captures into the exact [`Outline`] /
//! [`Symbol`] /
//! [`SymbolKind`] shapes the landed Tier-0 already
//! produces — so this is a drop-in better producer, never a new type.
//!
//! # How one parse feeds this *and* the highlight overlay
//!
//! [`Analyzer`](crate::Analyzer) parses the source **once** into a single
//! tree-sitter `Tree`. The highlight overlay is that tree run through the
//! grammar's `highlights.scm`; the outline is the *same* tree run through
//! its `tags.scm`. Two queries, one parse — ADR 0022 driver 1 ("two outputs
//! from one parse, or we pay for two engines").
//!
//! # tag → [`SymbolKind`] map
//!
//! Standard `tags.scm` queries capture a *container* node with
//! `@definition.<kind>` and a descendant identifier with `@name`. The
//! `<kind>` suffix is the primary signal; where a grammar overloads one
//! suffix for several constructs (rust tags `struct`/`enum`/`union`/type
//! alias all as `@definition.class`; go/c use `@definition.type` for
//! struct/interface/enum/alias) the captured node's grammar `kind()`
//! refines it, so the emitted [`SymbolKind`] is accurate:
//!
//! | `@definition.<suffix>` | base kind | node-kind refinement |
//! |---|---|---|
//! | `function` | `Function` (→ `Method` when nested in a type) | — |
//! | `method` | `Method` | — |
//! | `class` | `Class` | rust `struct_item`→`Struct`, `enum_item`→`Enum`, `union_item`→`Struct`, `type_item`→`Other`; C `struct_specifier`/`union_specifier`→`Struct` |
//! | `struct` | `Struct` | — |
//! | `enum` | `Enum` | — |
//! | `interface` / `trait` | `Trait` | — |
//! | `module` / `namespace` | `Module` | — |
//! | `constant` | `Constant` | — |
//! | `type` | (refined) | go: child `struct_type`→`Struct`, `interface_type`→`Trait`; C `enum_specifier`→`Enum`, `type_definition`→`Other`; else `Other` |
//! | anything else | `Other` | — |
//!
//! `@reference.*` and `@doc` captures are **not** definitions and are
//! skipped — **except** `@reference.implementation` on a Rust `impl_item`:
//! tree-sitter's `rust/tags.scm` tags an `impl` block as
//! `@reference.implementation` (a known upstream convention quirk — it is
//! not literally a `@definition`), yet Tier-0's
//! [`Outline`] emits it as a
//! [`SymbolKind::Impl`](rstui_widgets::SymbolKind) (name = the type
//! implemented; `impl Trait for T` → `T`). To be a faithful drop-in
//! *producer of the same shapes*, that one capture is lifted to an `Impl`
//! symbol — which also gives the `impl`'s methods a containing type so the
//! structural `depth` / Function→Method rule is correct. `line` is the
//! definition node's 0-based start row, `end_line` its 0-based end row.
//!
//! # `depth` and the Function→Method promotion — by containment
//!
//! `depth` is computed **structurally from the emitted symbols themselves**,
//! not from a per-grammar node-kind guess: a symbol's `depth` is the number
//! of *other emitted symbols whose `[start_byte, end_byte)` strictly
//! contains it*. This is grammar-agnostic and exactly the
//! [`Outline`] nesting contract (`0` = top level; a
//! `fn` inside an `impl` is `1`), and it sidesteps the trap that a grammar's
//! `@definition` node is often an inner node (`function_declarator` inside
//! `function_definition`, `variable_declarator` inside
//! `lexical_declaration`) — those same-construct wrappers are not separate
//! symbols, so they never inflate `depth`. A `Function` whose nearest
//! containing symbol is a type (`Class`/`Struct`/`Enum`/`Trait`/`Impl`)
//! becomes a `Method`, matching Tier-0's nested-method rule.
//!
//! Total: any tree / query / source yields a well-formed [`Outline`]
//! (symbols sorted by `line`, `line <= end_line`, both valid rows); a
//! grammar with no `tags.scm` (JSON, Markdown) yields an empty outline.

use rstui_widgets::{Outline, Symbol, SymbolKind};
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator, Tree};

/// The base [`SymbolKind`] for a definition-bearing capture name, before
/// node-kind refinement. `None` for a capture that is *not* a definition
/// (`@doc`, `@name`, `@reference.*` other than the Rust-impl quirk).
///
/// `@reference.implementation` is accepted because tree-sitter's
/// `rust/tags.scm` tags an `impl` block with it (not `@definition.*`) and
/// Tier-0 emits that as [`SymbolKind::Impl`] — see the module docs.
fn base_kind(capture_name: &str) -> Option<SymbolKind> {
    if capture_name == "reference.implementation" {
        return Some(SymbolKind::Impl);
    }
    let suffix = capture_name.strip_prefix("definition.")?;
    Some(match suffix {
        "function" => SymbolKind::Function,
        "method" => SymbolKind::Method,
        "class" => SymbolKind::Class,
        "struct" => SymbolKind::Struct,
        "enum" => SymbolKind::Enum,
        "interface" | "trait" => SymbolKind::Trait,
        "module" | "namespace" => SymbolKind::Module,
        "constant" => SymbolKind::Constant,
        // `type` is intentionally ambiguous → resolve via the node kind.
        "type" => SymbolKind::Other,
        _ => SymbolKind::Other,
    })
}

/// Refine the base kind using the captured definition node's grammar
/// `kind()` — the part that makes rust's overloaded `@definition.class` and
/// go/c's `@definition.type` produce accurate `Struct`/`Enum`/`Trait`.
fn refine_kind(base: SymbolKind, node: &Node, capture_name: &str) -> SymbolKind {
    let nk = node.kind();
    // rust: struct/enum/union/type-alias are all tagged `@definition.class`;
    // C: struct_specifier / union_specifier also `@definition.class`.
    if capture_name == "definition.class" {
        return match nk {
            "struct_item" | "struct_specifier" | "union_item" | "union_specifier" => {
                SymbolKind::Struct
            }
            "enum_item" => SymbolKind::Enum,
            "type_item" => SymbolKind::Other,
            // JS/TS/python genuine classes.
            _ => SymbolKind::Class,
        };
    }
    // go/c: `@definition.type` covers type_spec / enum / typedef.
    if capture_name == "definition.type" {
        if nk == "enum_specifier" {
            return SymbolKind::Enum;
        }
        if nk == "type_definition" {
            return SymbolKind::Other;
        }
        // Walk children for a go `type X struct {…}` / `interface {…}`.
        let mut walk = node.walk();
        for ch in node.children(&mut walk) {
            match ch.kind() {
                "struct_type" => return SymbolKind::Struct,
                "interface_type" => return SymbolKind::Trait,
                _ => {}
            }
        }
        return SymbolKind::Other;
    }
    base
}

/// A symbol kind that *introduces a type* — a `Function` whose nearest
/// containing symbol is one of these is a `Method` (Tier-0's rule).
fn is_type_kind(k: SymbolKind) -> bool {
    matches!(
        k,
        SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Trait
            | SymbolKind::Impl
    )
}

/// One emitted definition before `depth`/Method are resolved: its byte
/// range (for the structural containment pass) plus the final-`Symbol`
/// fields gathered from the captures.
struct Raw {
    name: String,
    kind: SymbolKind,
    line: usize,
    end_line: usize,
    start_byte: usize,
    end_byte: usize,
}

/// `true` if `outer` strictly contains `inner` by byte range (not the same
/// node — a proper enclosure).
fn strictly_contains(outer: &Raw, inner: &Raw) -> bool {
    outer.start_byte <= inner.start_byte
        && inner.end_byte <= outer.end_byte
        && (outer.start_byte, outer.end_byte) != (inner.start_byte, inner.end_byte)
}

/// Builds the [`Outline`] for `src` from `tree` using the grammar's
/// `tags_query` (already compiled). `None` ⇒ the grammar ships no tags
/// query (JSON / Markdown) ⇒ an empty outline.
///
/// One raw symbol per `tags.scm` match that has *both* a `@definition.*`
/// capture and a `@name` capture. `depth` and the Function→Method promotion
/// are then resolved by structural containment among those symbols (see the
/// module docs). The result is sorted by `(line, depth)` so it is pre-order
/// and in non-decreasing `line` order — the [`Outline`] contract. Total.
pub(crate) fn outline(src: &str, tree: &Tree, tags_query: Option<&Query>) -> Outline {
    let Some(query) = tags_query else {
        return Outline(Vec::new());
    };
    let total_lines = src.split('\n').count();
    let last = total_lines.saturating_sub(1);
    let src_bytes = src.as_bytes();
    let names = query.capture_names();

    // --- Pass 1: collect raw definitions -------------------------------
    let mut raws: Vec<Raw> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut it = cursor.matches(query, tree.root_node(), src_bytes);
    while let Some(m) = it.next() {
        let mut def: Option<(Node, &str)> = None;
        let mut name_node: Option<Node> = None;
        for cap in m.captures {
            let Some(cn) = names.get(cap.index as usize).copied() else {
                continue;
            };
            if cn == "name" {
                if name_node.is_none() {
                    name_node = Some(cap.node);
                }
            } else if base_kind(cn).is_some() {
                // The Rust-impl quirk only applies to an actual `impl_item`;
                // any other `@reference.*` is not a definition.
                if cn == "reference.implementation" && cap.node.kind() != "impl_item" {
                    continue;
                }
                def = Some((cap.node, cn));
            }
        }
        let (Some((dnode, dname)), Some(nnode)) = (def, name_node) else {
            continue;
        };
        let Some(base) = base_kind(dname) else {
            continue;
        };
        let kind = refine_kind(base, &dnode, dname);

        let line = dnode.start_position().row.min(last);
        let mut end_line = dnode.end_position().row.min(last);
        if end_line < line {
            end_line = line;
        }
        raws.push(Raw {
            name: nnode.utf8_text(src_bytes).unwrap_or("").to_string(),
            kind,
            line,
            end_line,
            start_byte: dnode.start_byte(),
            end_byte: dnode.end_byte(),
        });
    }

    // --- Pass 2: structural depth + Function→Method --------------------
    // `depth(i)` = count of *other* raws strictly containing raw i.
    // `method`  = raw i is a Function whose *innermost* containing raw is a
    // type. Both are O(n²); a source's symbol count is small (it is an
    // outline, not a token stream) so this stays cheap and is exactly the
    // Tier-0 nesting semantics.
    // tree-sitter's `rust/tags.scm` double-captures every method on the
    // *same* `function_item` node — once `@definition.method`, once
    // `@definition.function`. Collapse any such exact-byte-range twin to the
    // single most-specific kind (`Method` beats `Function`) before the depth
    // pass so the outline has one symbol per real definition.
    fn specificity(k: SymbolKind) -> u8 {
        match k {
            SymbolKind::Method => 3,
            SymbolKind::Function => 1,
            _ => 2,
        }
    }
    raws.sort_by(|a, b| {
        a.start_byte
            .cmp(&b.start_byte)
            .then(a.end_byte.cmp(&b.end_byte))
            .then(specificity(b.kind).cmp(&specificity(a.kind)))
    });
    raws.dedup_by(|a, b| {
        // `a` follows `b`; keep `b` (already the more specific by the sort).
        a.start_byte == b.start_byte && a.end_byte == b.end_byte && a.name == b.name
    });

    let mut syms: Vec<Symbol> = Vec::with_capacity(raws.len());
    for (i, r) in raws.iter().enumerate() {
        let mut depth: u16 = 0;
        // The innermost (smallest byte span) container, for the Method rule.
        let mut innermost: Option<&Raw> = None;
        for (j, o) in raws.iter().enumerate() {
            if i == j {
                continue;
            }
            if strictly_contains(o, r) {
                depth = depth.saturating_add(1);
                let tighter = match innermost {
                    None => true,
                    Some(cur) => (o.end_byte - o.start_byte) < (cur.end_byte - cur.start_byte),
                };
                if tighter {
                    innermost = Some(o);
                }
            }
        }
        let mut kind = r.kind;
        if kind == SymbolKind::Function {
            if let Some(parent) = innermost {
                if is_type_kind(parent.kind) {
                    kind = SymbolKind::Method;
                }
            }
        }
        syms.push(Symbol {
            name: r.name.clone(),
            kind,
            line: r.line,
            end_line: r.end_line,
            depth,
        });
    }

    // The `Outline` contract: non-decreasing `line`, parents (shallower
    // `depth`) before children on the same line. Stable-sort to that and
    // drop exact duplicates a grammar's overlapping patterns can produce.
    syms.sort_by(|a, b| a.line.cmp(&b.line).then(a.depth.cmp(&b.depth)));
    syms.dedup_by(|a, b| {
        a.line == b.line && a.end_line == b.end_line && a.kind == b.kind && a.name == b.name
    });
    Outline(syms)
}
