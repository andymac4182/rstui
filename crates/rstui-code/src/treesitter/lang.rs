//! Per-language tree-sitter grammar adapters, each `cfg`-gated behind its
//! crate feature.
//!
//! Grammar-crate APIs vary by published version, so every binding is checked
//! against the *actual* exported items rather than assumed:
//!
//! - All eight grammars expose `pub const LANGUAGE: LanguageFn` **except**
//!   `tree-sitter-typescript`, which exposes `LANGUAGE_TYPESCRIPT` /
//!   `LANGUAGE_TSX`. We use `LANGUAGE_TYPESCRIPT` and consume every
//!   `LanguageFn` through [`tree_sitter::Language::from`].
//! - The bundled highlight query const is `HIGHLIGHTS_QUERY` for
//!   rust/python/typescript/go/json and `HIGHLIGHT_QUERY` for
//!   javascript/c; `tree-sitter-md` ships `HIGHLIGHT_QUERY_BLOCK` (the block
//!   grammar is the one whose tree spans the document).
//! - The symbol query const is `TAGS_QUERY` for rust/python/javascript/
//!   typescript/go/c. **`tree-sitter-json` and `tree-sitter-md` ship no
//!   tags query**, so those languages have *no* outline (an empty
//!   [`Outline`](crate::Outline)) — handled robustly by returning
//!   `None` for their tags query.
//! - TypeScript's own `HIGHLIGHTS_QUERY` is *incremental over* the ECMAScript
//!   grammar (it only adds the TS-specific captures). So the TypeScript
//!   highlight query is the JavaScript `HIGHLIGHT_QUERY` **concatenated**
//!   with TypeScript's `HIGHLIGHTS_QUERY`, giving comments / strings /
//!   numbers full coverage. (Requires both the `typescript` and
//!   `javascript` features; both are on by default.)
//!
//! Every accessor is total: it returns owned `&'static str` query text and a
//! [`tree_sitter::Language`]; nothing here can panic.

#![allow(unused_imports)] // some imports are only used under certain features

use tree_sitter::Language;

/// The grammar [`Language`], its bundled highlight query, and its optional
/// tags query for one [`TsLanguage`](crate::TsLanguage) variant.
pub(crate) struct LangData {
    /// The compiled grammar.
    pub language: Language,
    /// The bundled `highlights.scm` (capture names → the four buckets).
    pub highlights_query: &'static str,
    /// The bundled `tags.scm`, or `None` for a grammar that ships none
    /// (JSON, Markdown) — those produce an empty outline.
    pub tags_query: Option<&'static str>,
}

// --- Rust -----------------------------------------------------------------

/// `tree-sitter-rust`: `LANGUAGE`, `HIGHLIGHTS_QUERY`, `TAGS_QUERY`.
#[cfg(feature = "rust")]
pub(crate) fn rust() -> LangData {
    LangData {
        language: Language::from(tree_sitter_rust::LANGUAGE),
        highlights_query: tree_sitter_rust::HIGHLIGHTS_QUERY,
        tags_query: Some(tree_sitter_rust::TAGS_QUERY),
    }
}

// --- Python ---------------------------------------------------------------

/// `tree-sitter-python`: `LANGUAGE`, `HIGHLIGHTS_QUERY`, `TAGS_QUERY`.
#[cfg(feature = "python")]
pub(crate) fn python() -> LangData {
    LangData {
        language: Language::from(tree_sitter_python::LANGUAGE),
        highlights_query: tree_sitter_python::HIGHLIGHTS_QUERY,
        tags_query: Some(tree_sitter_python::TAGS_QUERY),
    }
}

// --- JavaScript -----------------------------------------------------------

/// `tree-sitter-javascript`: `LANGUAGE`, `HIGHLIGHT_QUERY` (singular),
/// `TAGS_QUERY`.
#[cfg(feature = "javascript")]
pub(crate) fn javascript() -> LangData {
    LangData {
        language: Language::from(tree_sitter_javascript::LANGUAGE),
        highlights_query: tree_sitter_javascript::HIGHLIGHT_QUERY,
        tags_query: Some(tree_sitter_javascript::TAGS_QUERY),
    }
}

// --- TypeScript -----------------------------------------------------------

/// TypeScript's highlight query, ECMAScript base + TS-specific additions.
///
/// `tree-sitter-typescript`'s own `HIGHLIGHTS_QUERY` only declares the
/// TS-only captures (`; inherits: ecma` is a *tooling* directive, not part
/// of the raw query string), so on its own it would not colour comments /
/// strings / numbers. Concatenating the JavaScript `HIGHLIGHT_QUERY` ahead
/// of it gives the TypeScript grammar (which is ECMAScript-compatible) full
/// coverage. Built once and leaked to obtain a `&'static str` — there is
/// exactly one TypeScript grammar per process so this never accumulates.
#[cfg(all(feature = "typescript", feature = "javascript"))]
fn typescript_highlights() -> &'static str {
    use std::sync::OnceLock;
    static Q: OnceLock<String> = OnceLock::new();
    Q.get_or_init(|| {
        format!(
            "{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY
        )
    })
    .as_str()
}

/// Without the `javascript` feature, fall back to TypeScript's own
/// (TS-only) highlight query. Still total — it simply colours less.
#[cfg(all(feature = "typescript", not(feature = "javascript")))]
fn typescript_highlights() -> &'static str {
    tree_sitter_typescript::HIGHLIGHTS_QUERY
}

/// TypeScript's tags query, ECMAScript base + TS-specific additions — same
/// reasoning as [`typescript_highlights`]. `tree-sitter-typescript`'s
/// `TAGS_QUERY` only matches the TS-only constructs (`function_signature`,
/// `method_signature`, `interface_declaration`, `module`,
/// `abstract_class_declaration`); a concrete `class_declaration` /
/// `function_declaration` / arrow-`const` is an ECMAScript node, so without
/// the JavaScript `TAGS_QUERY` ahead of it those would yield no symbol.
/// Composed once and leaked to a `&'static str` (one TypeScript grammar per
/// process).
#[cfg(all(feature = "typescript", feature = "javascript"))]
fn typescript_tags() -> Option<&'static str> {
    use std::sync::OnceLock;
    static Q: OnceLock<String> = OnceLock::new();
    Some(
        Q.get_or_init(|| {
            format!(
                "{}\n{}",
                tree_sitter_javascript::TAGS_QUERY,
                tree_sitter_typescript::TAGS_QUERY
            )
        })
        .as_str(),
    )
}

/// Without the `javascript` feature, TypeScript's own (TS-only) tags query.
#[cfg(all(feature = "typescript", not(feature = "javascript")))]
fn typescript_tags() -> Option<&'static str> {
    Some(tree_sitter_typescript::TAGS_QUERY)
}

/// `tree-sitter-typescript`: `LANGUAGE_TYPESCRIPT` (note the suffix —
/// the crate has no plain `LANGUAGE`), the composed highlight query, and
/// `TAGS_QUERY`.
#[cfg(feature = "typescript")]
pub(crate) fn typescript() -> LangData {
    LangData {
        language: Language::from(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
        highlights_query: typescript_highlights(),
        tags_query: typescript_tags(),
    }
}

// --- Go -------------------------------------------------------------------

/// `tree-sitter-go`: `LANGUAGE`, `HIGHLIGHTS_QUERY`, `TAGS_QUERY`.
#[cfg(feature = "go")]
pub(crate) fn go() -> LangData {
    LangData {
        language: Language::from(tree_sitter_go::LANGUAGE),
        highlights_query: tree_sitter_go::HIGHLIGHTS_QUERY,
        tags_query: Some(tree_sitter_go::TAGS_QUERY),
    }
}

// --- C --------------------------------------------------------------------

/// `tree-sitter-c`: `LANGUAGE`, `HIGHLIGHT_QUERY` (singular), `TAGS_QUERY`.
#[cfg(feature = "c")]
pub(crate) fn c() -> LangData {
    LangData {
        language: Language::from(tree_sitter_c::LANGUAGE),
        highlights_query: tree_sitter_c::HIGHLIGHT_QUERY,
        tags_query: Some(tree_sitter_c::TAGS_QUERY),
    }
}

// --- JSON -----------------------------------------------------------------

/// `tree-sitter-json`: `LANGUAGE`, `HIGHLIGHTS_QUERY`. It ships **no**
/// tags query, so JSON has no outline.
#[cfg(feature = "json")]
pub(crate) fn json() -> LangData {
    LangData {
        language: Language::from(tree_sitter_json::LANGUAGE),
        highlights_query: tree_sitter_json::HIGHLIGHTS_QUERY,
        tags_query: None,
    }
}

// --- Markdown -------------------------------------------------------------

/// `tree-sitter-md`: the *block* grammar (`LANGUAGE`, whose tree spans the
/// whole document) and its `HIGHLIGHT_QUERY_BLOCK`. It ships **no** tags
/// query, so Markdown has no outline.
#[cfg(feature = "markdown")]
pub(crate) fn markdown() -> LangData {
    LangData {
        language: Language::from(tree_sitter_md::LANGUAGE),
        highlights_query: tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
        tags_query: None,
    }
}
