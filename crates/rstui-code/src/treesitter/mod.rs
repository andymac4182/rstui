//! ADR 0022 **Tier-1** — a feature-gated tree-sitter back end that is a
//! *better producer of the SAME shapes* the dependency-free **Tier-0**
//! ([`crate::syntax`] + [`crate::outline`]) already feeds. It is a
//! **drop-in, never a new widget**: one real parse yields *both* the
//! per-character syntax overlay [`Editor`](crate::Editor)/[`Diff`](crate::Diff)
//! already read ([`Editor::syntax`](crate::Editor)) *and* the
//! [`Outline`] the symbol panel already projects.
//!
//! # First-class in `rstui-code` (ADR 0024)
//!
//! tree-sitter grammars carry generated C. ADR 0024 supersedes ADR 0023:
//! rather than a workspace-excluded leaf, this back end is folded **into the
//! `rstui-code` crate** and depended on first-class. The dependency-free
//! floor still lives in `rstui-core`/`rstui-widgets`, which stay
//! tree-sitter-free; only `rstui-code` (and its consumers) pull tree-sitter,
//! so the dependency direction is still one-way. The five CI gates *do* now
//! compile this back end on the `rstui-code` leg (accepted by ADR 0024).
//! Tier-0 is always present; Tier-1 is the accuracy upgrade, default-on per
//! language behind a Cargo feature each.
//!
//! # One parse → two outputs (ADR 0022 driver 1)
//!
//! [`Analyzer`] holds a tree-sitter `Parser` and the last `Tree`.
//! [`Analyzer::set_source`] (re)parses; [`Analyzer::highlight`] runs that
//! tree through the grammar's `highlights.scm` (capture → one of the four
//! [`SyntaxStyles`] theme buckets) and
//! [`Analyzer::outline`] runs the *same* tree through its `tags.scm` —
//! never a second engine.
//!
//! # Caller-owned (ADR 0012)
//!
//! Exactly like every other overlay in the crate, the app owns the
//! [`Analyzer`] in its model, calls [`set_source`](Analyzer::set_source)
//! when the document changes, and reads
//! [`highlight`](Analyzer::highlight) / [`outline`](Analyzer::outline) in
//! the *pure* `view`. The widget never sees this crate; it only ever reads a
//! `&[Style]` / an [`Outline`] the reducer handed it.
//!
//! # Drop-in overlay layout
//!
//! [`highlight`](Analyzer::highlight) returns a **flattened** per-character
//! `Vec<Style>` over the document as *rows joined by `'\n'`* — one `Style`
//! slot per source char, *including one slot per newline* — i.e. exactly
//! `rstui-git-review`'s `rebuild_edit_overlays` layout / length. It is a
//! true drop-in for `Editor::new(&doc).syntax(&analyzer.highlight(&styles))`.
//!
//! # Totality / no-panic
//!
//! Every entry point is total: any source under any enabled language, an
//! empty document, half-written code or a megabyte of garbage never panics;
//! `highlight()`'s length is always `src.chars().count()`; `outline()`
//! always returns a well-formed [`Outline`]. v1 **full-reparses** on every
//! [`set_source`](Analyzer::set_source) (correct over micro-incremental);
//! the `Tree` is retained so a future slice can switch to
//! `Tree::edit` + incremental re-parse without an API change.
//!
//! # Example
//!
//! ```
//! # #[cfg(feature = "rust")] {
//! use rstui_code::{Analyzer, TsLanguage};
//! use rstui_code::syntax::SyntaxStyles;
//! use rstui_core::{Color, Style};
//!
//! let styles = SyntaxStyles {
//!     keyword: Style::new().fg(Color::Blue),
//!     string: Style::new().fg(Color::Green),
//!     number: Style::new().fg(Color::Magenta),
//!     comment: Style::new().fg(Color::DarkGray),
//!     // The richer Tier-1-only semantic classes default to no colour;
//!     // fill the ones you want and `..Default::default()` the rest.
//!     ..Default::default()
//! };
//! let mut a = Analyzer::new(TsLanguage::Rust);
//! a.set_source("fn main() {\n    let n = 42; // hi\n}\n");
//!
//! let overlay = a.highlight(&styles);          // drop-in for Editor::syntax
//! assert_eq!(overlay.len(), "fn main() {\n    let n = 42; // hi\n}\n".chars().count());
//!
//! let o = a.outline();                          // rstui_code::Outline
//! assert!(o.0.iter().any(|s| s.name == "main"));
//! # }
//! ```

mod highlight;
mod lang;
mod symbols;

use crate::Outline;
use crate::syntax::SyntaxStyles;
use rstui_core::Style;
use tree_sitter::{Parser, Query, Tree};

/// A source language Tier-1 can parse. **Only the variants whose crate
/// feature is enabled exist** — each variant is `cfg`-gated to its feature,
/// so a build that disables a language also drops the variant (the public
/// surface is exactly the enabled languages). All eight are on by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsLanguage {
    /// Rust (`.rs`) — `tree-sitter-rust`.
    #[cfg(feature = "rust")]
    Rust,
    /// Python (`.py`, `.pyi`, `.pyw`) — `tree-sitter-python`.
    #[cfg(feature = "python")]
    Python,
    /// JavaScript (`.js`, `.mjs`, `.cjs`, `.jsx`) — `tree-sitter-javascript`.
    #[cfg(feature = "javascript")]
    JavaScript,
    /// TypeScript (`.ts`, `.tsx`, `.mts`, `.cts`) — `tree-sitter-typescript`.
    #[cfg(feature = "typescript")]
    TypeScript,
    /// Go (`.go`) — `tree-sitter-go`.
    #[cfg(feature = "go")]
    Go,
    /// C / C-family (`.c`, `.h`, `.cc`, …) — `tree-sitter-c`.
    #[cfg(feature = "c")]
    C,
    /// JSON (`.json`, `.jsonc`, `.json5`) — `tree-sitter-json`. No
    /// `tags.scm` ⇒ an empty [`outline`](Analyzer::outline).
    #[cfg(feature = "json")]
    Json,
    /// Markdown (`.md`, `.markdown`, …) — `tree-sitter-md`. No `tags.scm`
    /// ⇒ an empty [`outline`](Analyzer::outline).
    #[cfg(feature = "markdown")]
    Markdown,
}

impl TsLanguage {
    /// Picks a language from a file path by extension, case-insensitive
    /// (the same extension rule as
    /// [`crate::syntax::Language::from_path`]). `None` for an
    /// unrecognised, extension-less, or feature-disabled language — so a
    /// caller can fall back to Tier-0.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "rust")] {
    /// use rstui_code::TsLanguage;
    /// assert_eq!(TsLanguage::from_path("src/Main.RS"), Some(TsLanguage::Rust));
    /// assert_eq!(TsLanguage::from_path("Makefile"), None);
    /// # }
    /// ```
    #[must_use]
    pub fn from_path(path: &str) -> Option<Self> {
        // Last path component, then text after its final '.', lowercased —
        // pure string work, identical on every OS (mirrors Tier-0).
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let dot = name.rfind('.')?;
        if dot == 0 {
            return None; // a dotfile like `.gitignore` has no extension
        }
        let ext = name[dot + 1..].to_ascii_lowercase();
        match ext.as_str() {
            #[cfg(feature = "rust")]
            "rs" => Some(TsLanguage::Rust),
            #[cfg(feature = "python")]
            "py" | "pyi" | "pyw" => Some(TsLanguage::Python),
            #[cfg(feature = "javascript")]
            "js" | "mjs" | "cjs" | "jsx" => Some(TsLanguage::JavaScript),
            #[cfg(feature = "typescript")]
            "ts" | "mts" | "cts" | "tsx" => Some(TsLanguage::TypeScript),
            #[cfg(feature = "go")]
            "go" => Some(TsLanguage::Go),
            #[cfg(feature = "c")]
            "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Some(TsLanguage::C),
            #[cfg(feature = "json")]
            "json" | "jsonc" | "json5" => Some(TsLanguage::Json),
            #[cfg(feature = "markdown")]
            "md" | "markdown" | "mdown" | "mkd" => Some(TsLanguage::Markdown),
            _ => None,
        }
    }

    /// This language's grammar, highlight query text and optional tags
    /// query text (from the `cfg`-gated [`lang`] adapter).
    fn data(self) -> lang::LangData {
        match self {
            #[cfg(feature = "rust")]
            TsLanguage::Rust => lang::rust(),
            #[cfg(feature = "python")]
            TsLanguage::Python => lang::python(),
            #[cfg(feature = "javascript")]
            TsLanguage::JavaScript => lang::javascript(),
            #[cfg(feature = "typescript")]
            TsLanguage::TypeScript => lang::typescript(),
            #[cfg(feature = "go")]
            TsLanguage::Go => lang::go(),
            #[cfg(feature = "c")]
            TsLanguage::C => lang::c(),
            #[cfg(feature = "json")]
            TsLanguage::Json => lang::json(),
            #[cfg(feature = "markdown")]
            TsLanguage::Markdown => lang::markdown(),
        }
    }
}

/// One tree-sitter parse → **both** the syntax overlay and the symbol
/// outline (ADR 0022 driver 1). **Caller-owned** (ADR 0012): the app holds
/// it in its model, calls [`set_source`](Self::set_source) on every edit,
/// and reads [`highlight`](Self::highlight) / [`outline`](Self::outline) in
/// the pure `view`.
///
/// Construction compiles the grammar's highlight query (and tags query, if
/// any) once; [`set_source`](Self::set_source) re-parses. If a grammar's
/// bundled query somehow fails to compile against its own grammar (it never
/// does for the pinned versions, but the API is kept total) that query is
/// simply absent: [`highlight`](Self::highlight) returns an all-empty
/// (no-colour) overlay of the correct length and
/// [`outline`](Self::outline) an empty [`Outline`] — degrade, never panic.
pub struct Analyzer {
    parser: Parser,
    /// The last successful parse. `None` before the first
    /// [`set_source`](Self::set_source) (or if the grammar failed to load —
    /// then every output is the safe empty one).
    tree: Option<Tree>,
    /// The exact source last parsed — rows joined by `'\n'`. The overlay is
    /// indexed against *this* string's chars.
    src: String,
    /// The compiled `highlights.scm`. `None` only if it failed to compile.
    highlights: Option<Query>,
    /// The compiled `tags.scm`, or `None` (grammar ships none, or it failed
    /// to compile) ⇒ an empty outline.
    tags: Option<Query>,
}

impl Analyzer {
    /// Builds an analyzer for `lang`, compiling its bundled highlight and
    /// tags queries once. Total — a grammar/query load failure (impossible
    /// for the pinned versions) degrades to empty outputs rather than
    /// panicking.
    #[must_use]
    pub fn new(lang: TsLanguage) -> Self {
        let data = lang.data();
        let mut parser = Parser::new();
        // `set_language` only fails on an ABI mismatch — impossible for the
        // version-matched grammar crates. If it ever did, every parse is
        // `None` and all outputs are the safe empty ones.
        let language_ok = parser.set_language(&data.language).is_ok();
        let highlights = if language_ok {
            Query::new(&data.language, data.highlights_query).ok()
        } else {
            None
        };
        let tags = if language_ok {
            data.tags_query
                .and_then(|q| Query::new(&data.language, q).ok())
        } else {
            None
        };
        Self {
            parser,
            tree: None,
            src: String::new(),
            highlights,
            tags,
        }
    }

    /// (Re)parses `src` and keeps the resulting `Tree`.
    ///
    /// `src` is the document **rows joined by `'\n'`** — exactly
    /// [`TextArea::to_string()`](rstui_core::TextArea) /
    /// `editor.lines().join("\n")`. v1 does a **full re-parse** every call
    /// (correctness over a fiddly incremental edit; the `Tree` is retained
    /// so a later slice can add `Tree::edit` incremental re-parse without
    /// changing this signature). Total: any input; a parse failure simply
    /// clears the tree (outputs degrade to the safe empty ones).
    pub fn set_source(&mut self, src: &str) {
        self.src.clear();
        self.src.push_str(src);
        // Full reparse (no `old_tree`). `parse` returns `None` only if the
        // language was never set — handled by the `None` tree path.
        self.tree = self.parser.parse(src, None);
    }

    /// The flattened per-character syntax overlay for the current source,
    /// painted from the parse tree's `highlights.scm` captures into the
    /// caller's four theme buckets (`styles`).
    ///
    /// **Drop-in for [`Editor::syntax`](crate::Editor)**: the
    /// returned `Vec<Style>` has exactly `current_source.chars().count()`
    /// slots — one per source char *including each `'\n'`* — the identical
    /// layout/length `rstui-git-review`'s `rebuild_edit_overlays` builds, so
    /// `Editor::new(&doc).syntax(&analyzer.highlight(&styles))` just works.
    /// Total: never panics; before [`set_source`](Self::set_source), or on
    /// a parse/query failure, every slot is an empty (no-colour) `Style`.
    #[must_use]
    pub fn highlight(&self, styles: &SyntaxStyles) -> Vec<Style> {
        match (&self.tree, &self.highlights) {
            (Some(tree), Some(q)) => highlight::highlight(&self.src, tree, q, styles),
            // No tree or query: the correct-length all-empty overlay so it
            // is still a valid drop-in.
            _ => vec![Style::new(); self.src.chars().count()],
        }
    }

    /// The symbol [`Outline`] for the current source, from the parse tree's
    /// `tags.scm` definition captures — the **exact**
    /// [`crate::Outline`] shape Tier-0 produces, so it is a drop-in
    /// for the symbol panel. A language whose grammar ships no `tags.scm`
    /// (JSON, Markdown) yields an empty outline. Total: never panics; before
    /// [`set_source`](Self::set_source) it is empty.
    #[must_use]
    pub fn outline(&self) -> Outline {
        match &self.tree {
            Some(tree) => symbols::outline(&self.src, tree, self.tags.as_ref()),
            None => Outline(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests;
