//! [`Outline`] — a dependency-free **heuristic symbol scanner** that turns any
//! source text into a flat, pre-order list of [`Symbol`]s with a nesting
//! `depth`, the caller then projects through the existing
//! [`Tree`](crate::Tree) / [`List`](crate::List) widgets.
//!
//! # Model + scanner, not a widget
//!
//! This module is deliberately **only the model and the scanner**. It owns no
//! [`Buffer`](rstui_core::Buffer), implements no
//! [`Widget`](rstui_core::Widget), and renders nothing. An app calls
//! [`Outline::scan`] once (it owns the resulting `Vec<Symbol>` like every other
//! reducer-owned bit of state), then feeds it to a tree/list view — exactly the
//! caller-owns-it, the-widget-only-projects-it discipline the rest of the crate
//! follows (the [`Extmark`](crate::Extmark) precedent). A symbol carries the
//! line range and `depth` a `Tree` node needs and nothing more, so the
//! projection stays the app's choice.
//!
//! # A heuristic *floor*, on purpose
//!
//! This is the **Tier-0** scanner: a dependency-free, language-blind-ish,
//! single-pass *heuristic*. It is the floor, not the ceiling — it recognises
//! the common shapes of mainstream definitions (a `fn`/`def`/`func`, a
//! `struct`/`class`, an ATX heading) by looking at the start of a line, and
//! tracks nesting with brace counting or indentation. It is **not** a parser:
//! it does not build an AST, it does not understand macros, and a construct
//! split across lines in an unusual way may be missed or its `end_line`
//! approximated. The *accurate* tier — a real grammar via a feature-gated
//! tree-sitter back end — is deliberately left to the future; this Tier-0 floor
//! is what works everywhere, today, with zero dependencies.
//!
//! That choice follows the same reasoning as [`Diff`](crate::Diff)'s
//! hand-written generic syntax tokenizer: rstui is dependency-free below the
//! backend (see [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)
//! §4 — a widget that pulls a transitive dependency is feature-gated; an
//! own-crate split is reserved for *heavy, optional, conceptually alien*
//! engines). A line-oriented symbol heuristic is none of those: it is the same
//! kind of small character classifier [`Diff`](crate::Diff)'s syntax mode and
//! [`Markdown`](crate::Markdown)'s parser already use, so it lives here as a
//! plain module with zero new dependencies.
//!
//! # Total — any `&str`, any language, never panics
//!
//! Every entry point is **pure and total** (the "a pure projection must be
//! total" rule): [`Outline::scan`] accepts *any* `&str` for *any* [`Language`]
//! — random bytes, half-written code, a 1 MB blob of one line, an empty string
//! — and always returns a well-formed [`Outline`]. The returned symbols are in
//! non-decreasing `line` order, every `line <= end_line` and both are valid
//! line indices, and `depth` stays small. [`Outline::at_line`] is likewise
//! total: an out-of-range line simply yields `None`.
//!
//! # Example
//!
//! ```
//! use rstui_widgets::outline::{Language, Outline, SymbolKind};
//!
//! let src = "\
//! pub mod parser {
//!     pub struct Lexer;
//!     impl Lexer {
//!         pub fn next(&mut self) {}
//!     }
//! }
//! ";
//! let o = Outline::scan(src, Language::from_path("src/lib.rs"));
//! let names: Vec<_> = o.0.iter().map(|s| (s.name.as_str(), s.kind, s.depth)).collect();
//! assert_eq!(
//!     names,
//!     vec![
//!         ("parser", SymbolKind::Module, 0),
//!         ("Lexer", SymbolKind::Struct, 1),
//!         ("Lexer", SymbolKind::Impl, 1),
//!         ("next", SymbolKind::Method, 2),
//!     ]
//! );
//! // The deepest symbol enclosing the `fn next` line is the method itself.
//! assert_eq!(o.at_line(3).map(|s| s.name.as_str()), Some("next"));
//! ```

/// What a [`Symbol`] is, coarsely. A *reading aid* taxonomy, not a type
/// system: the heuristic buckets a construct by the keyword that introduces it
/// (so a Rust `impl` block and a Go `type … struct` are each one bucket), and
/// anything it recognises as a definition but cannot otherwise classify falls
/// to [`SymbolKind::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// A module / namespace (`mod`, `pub mod`).
    Module,
    /// A struct / record (`struct`, Go `type X struct`).
    Struct,
    /// An enum / sum type (`enum`).
    Enum,
    /// A trait / interface / protocol (`trait`, TS `interface`,
    /// Go `type X interface`).
    Trait,
    /// A Rust `impl` block (name = the type or trait being implemented).
    Impl,
    /// A free function (`fn`, `def`, `func`, `function`, an arrow `const`).
    Function,
    /// A function bound to a type — nested in an `impl`/`trait`/`class` by
    /// brace or indentation depth.
    Method,
    /// A class (`class`).
    Class,
    /// A module-level constant / static (`const`, `static`).
    Constant,
    /// A named field (reserved for callers that post-process; the scanner does
    /// not emit struct fields itself).
    Field,
    /// A Markdown ATX heading (`#`..`######`).
    Heading,
    /// A recognised definition the heuristic cannot bucket more precisely.
    Other,
}

/// One symbol in an [`Outline`]: its `name`, coarse [`kind`](SymbolKind), the
/// 0-based `line` it is declared on, the best-effort `end_line` of its body
/// (the last line the construct spans, `>= line`), and its `depth` (nesting,
/// `0` = top level).
///
/// The fields are public so a caller can cheaply re-bucket or filter the list
/// (the same "the reducer owns it" stance [`Extmark`](crate::Extmark) takes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The declared identifier (best-effort; empty only if none could be
    /// scanned, e.g. an anonymous `impl` target).
    pub name: String,
    /// The coarse classification.
    pub kind: SymbolKind,
    /// 0-based line the symbol is declared on.
    pub line: usize,
    /// 0-based last line the construct spans (`>= line`; best-effort via brace
    /// or indentation depth — equal to `line` when no body is detected).
    pub end_line: usize,
    /// Nesting depth (`0` = top level), incremented inside an enclosing
    /// construct's body.
    pub depth: u16,
}

/// A scanned outline: a flat **pre-order** list of [`Symbol`]s where `depth`
/// encodes the tree (a child immediately follows its parent and has a strictly
/// greater `depth`). Project it through [`Tree`](crate::Tree) /
/// [`List`](crate::List); the app owns this value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outline(pub Vec<Symbol>);

/// The source language the heuristic is tuned for. [`Language::Unknown`]
/// scans nothing (an empty [`Outline`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// Rust (`.rs`).
    Rust,
    /// Python (`.py`, `.pyi`).
    Python,
    /// Go (`.go`).
    Go,
    /// JavaScript (`.js`, `.mjs`, `.cjs`, `.jsx`).
    JavaScript,
    /// TypeScript (`.ts`, `.tsx`, `.mts`, `.cts`).
    TypeScript,
    /// C / C-family headers and sources (`.c`, `.h`).
    C,
    /// Markdown (`.md`, `.markdown`).
    Markdown,
    /// Anything unrecognised — scans to an empty [`Outline`].
    Unknown,
}

impl Language {
    /// Resolve by file extension (e.g. `"src/app.rs"` → [`Language::Rust`]).
    /// Case-insensitive on the extension. Anything unrecognised — including a
    /// path with no extension — is [`Language::Unknown`].
    ///
    /// # Example
    ///
    /// ```
    /// use rstui_widgets::outline::Language;
    /// assert_eq!(Language::from_path("src/App.RS"), Language::Rust);
    /// assert_eq!(Language::from_path("README.md"), Language::Markdown);
    /// assert_eq!(Language::from_path("Makefile"), Language::Unknown);
    /// ```
    #[must_use]
    pub fn from_path(path: &str) -> Self {
        // The extension = chars after the last `.`, but only if that `.` is
        // in the final path segment (so `dir.v2/file` is *not* a `.v2/file`
        // extension, and `.gitignore` has no extension — its only `.` is the
        // first char of the segment).
        let seg = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let ext = match seg.rfind('.') {
            // `idx == 0` ⇒ a dotfile like `.gitignore`: no extension.
            Some(idx) if idx > 0 => &seg[idx + 1..],
            _ => return Language::Unknown,
        };
        // Lowercase without allocating beyond a tiny stack buffer would need
        // unsafe-free byte work; `to_ascii_lowercase` on the short ext is
        // simplest and the scan is not hot.
        match ext.to_ascii_lowercase().as_str() {
            "rs" => Language::Rust,
            "py" | "pyi" => Language::Python,
            "go" => Language::Go,
            "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
            "ts" | "tsx" | "mts" | "cts" => Language::TypeScript,
            "c" | "h" => Language::C,
            "md" | "markdown" => Language::Markdown,
            _ => Language::Unknown,
        }
    }
}

impl Outline {
    /// Heuristic single-pass scan of `src` for `lang`.
    ///
    /// `line` is 0-based; `end_line` is the last line the construct spans
    /// (best-effort via brace or indentation depth, never `< line`); `depth`
    /// is nesting (`0` = top level). The result is pre-order and in
    /// non-decreasing `line` order. **Total**: any `src`, any `lang`, never
    /// panics. [`Language::Unknown`] yields an empty outline.
    ///
    /// # Example
    ///
    /// ```
    /// use rstui_widgets::outline::{Language, Outline, SymbolKind};
    /// let o = Outline::scan("# Title\n## Sub\n", Language::Markdown);
    /// assert_eq!(o.0[0].kind, SymbolKind::Heading);
    /// assert_eq!(o.0[0].depth, 0);
    /// assert_eq!(o.0[1].depth, 1);
    /// ```
    #[must_use]
    pub fn scan(src: &str, lang: Language) -> Outline {
        let lines: Vec<&str> = src.split('\n').collect();
        let syms = match lang {
            Language::Rust => scan_braced(&lines, RUST),
            Language::Go => scan_braced(&lines, GO),
            Language::JavaScript | Language::TypeScript => scan_braced(&lines, JS),
            Language::C => scan_braced(&lines, C),
            Language::Python => scan_python(&lines),
            Language::Markdown => scan_markdown(&lines),
            Language::Unknown => Vec::new(),
        };
        Outline(syms)
    }

    /// The deepest [`Symbol`] whose `[line, end_line]` (inclusive) contains
    /// `line` — the "current symbol" for a caret or a diff hunk — or `None` if
    /// `line` is outside every symbol.
    ///
    /// When symbols nest, the one with the greatest `depth` among those that
    /// contain `line` wins (the innermost enclosing construct). Total: any
    /// `line`, including past EOF, is safe.
    #[must_use]
    pub fn at_line(&self, line: usize) -> Option<&Symbol> {
        self.0
            .iter()
            .filter(|s| s.line <= line && line <= s.end_line)
            .max_by_key(|s| (s.depth, s.line))
    }
}

// ---------------------------------------------------------------------------
// Identifier / token scanning — tiny, hand-written, no regex
// ---------------------------------------------------------------------------

/// Whether `c` may start or continue an identifier (ASCII-ident plus `_`;
/// non-ASCII letters are accepted too so a Unicode identifier is not split).
fn is_ident(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// Returns the line with a trailing `\r` removed (so a CRLF file scans the
/// same as LF) — the same trailing-`\r` strip [`Diff`](crate::Diff) does.
fn no_cr(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

/// The byte index of the first non-space/tab char (the "indent width" of a
/// line, used by the Python scanner). A blank/whitespace-only line yields its
/// full length.
fn indent_of(line: &str) -> usize {
    line.find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(line.len())
}

/// Whether a line is blank or only whitespace (Python's block scanner ignores
/// these when deciding where a block ends).
fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// The first identifier appearing in `s` at or after byte index `from`,
/// returned with the byte index just past it. `None` if there is none. A tiny
/// hand scan — no regex, no allocation beyond the returned `String`.
fn next_ident(s: &str, from: usize) -> Option<(String, usize)> {
    let bytes = s.char_indices().skip_while(|&(i, _)| i < from);
    let mut start = None;
    for (i, c) in bytes {
        if is_ident(c) {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(st) = start {
            return Some((s[st..i].to_string(), i));
        }
    }
    start.map(|st| (s[st..].to_string(), s.len()))
}

/// Whether `s` (already trimmed of leading space) begins with the word
/// `word` followed by a non-identifier char (so `func` matches `func x` but
/// not `functions`). Returns the byte index just past `word` on a hit.
fn starts_word(s: &str, word: &str) -> Option<usize> {
    let rest = s.strip_prefix(word)?;
    match rest.chars().next() {
        None => Some(word.len()),
        Some(c) if !is_ident(c) => Some(word.len()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Brace-depth scanner (Rust / Go / JS / TS / C)
// ---------------------------------------------------------------------------

/// A keyword → kind rule for the brace-depth scanner. `prefix` is matched as a
/// whole leading word of the (visibility-stripped) trimmed line; `extract`
/// pulls the symbol name from the byte position just past `prefix`.
struct Rule {
    /// The introducing keyword, matched as a whole word at line start.
    prefix: &'static str,
    /// The bucket this keyword maps to (before the nested-method override).
    kind: SymbolKind,
}

/// A language profile for [`scan_braced`]: its definition rules plus the small
/// set of visibility/qualifier words that may precede a definition and are
/// skipped before rule matching.
struct BracedLang {
    /// Definition rules, tried in order; first whole-word match wins.
    rules: &'static [Rule],
    /// Leading qualifier words to strip (e.g. `pub`, `export`, `default`).
    /// `pub(crate)` / `pub(super)` are handled generically.
    qualifiers: &'static [&'static str],
    /// Kinds that open a "type body": a [`SymbolKind::Function`] declared with
    /// `depth` strictly inside one becomes a [`SymbolKind::Method`].
    method_parents: &'static [SymbolKind],
    /// Whether a bare `name(args) {` (no introducing keyword) directly inside
    /// a [`method_parents`](BracedLang::method_parents) scope is a
    /// [`SymbolKind::Method`] — the JS/TS class-method shape. Off for Rust/Go
    /// (their methods always carry `fn`/`func`), keeping those false-positive
    /// free.
    bare_method: bool,
}

const RUST: &BracedLang = &BracedLang {
    rules: &[
        Rule { prefix: "mod", kind: SymbolKind::Module },
        Rule { prefix: "struct", kind: SymbolKind::Struct },
        Rule { prefix: "enum", kind: SymbolKind::Enum },
        Rule { prefix: "trait", kind: SymbolKind::Trait },
        Rule { prefix: "union", kind: SymbolKind::Struct },
        Rule { prefix: "impl", kind: SymbolKind::Impl },
        Rule { prefix: "fn", kind: SymbolKind::Function },
        Rule { prefix: "const", kind: SymbolKind::Constant },
        Rule { prefix: "static", kind: SymbolKind::Constant },
    ],
    // `async`/`unsafe`/`extern` may sit between `pub` and `fn`; `default` for
    // specialization. `pub(crate)`/`pub(…)` handled by the `(` strip below.
    qualifiers: &["pub", "async", "unsafe", "extern", "default", "const"],
    method_parents: &[SymbolKind::Impl, SymbolKind::Trait],
    bare_method: false,
};

const GO: &BracedLang = &BracedLang {
    rules: &[
        // `type X struct` / `type X interface` are special-cased in the name
        // extractor; the generic `type` rule is the fallback (alias → Other).
        Rule { prefix: "func", kind: SymbolKind::Function },
        Rule { prefix: "type", kind: SymbolKind::Other },
        Rule { prefix: "const", kind: SymbolKind::Constant },
        Rule { prefix: "var", kind: SymbolKind::Constant },
    ],
    qualifiers: &[],
    method_parents: &[],
    bare_method: false,
};

const JS: &BracedLang = &BracedLang {
    rules: &[
        Rule { prefix: "class", kind: SymbolKind::Class },
        Rule { prefix: "interface", kind: SymbolKind::Trait },
        Rule { prefix: "function", kind: SymbolKind::Function },
        // `const`/`let`/`var` only become a symbol when they bind a function
        // or arrow (decided in the extractor); otherwise skipped.
        Rule { prefix: "const", kind: SymbolKind::Function },
        Rule { prefix: "let", kind: SymbolKind::Function },
        Rule { prefix: "var", kind: SymbolKind::Function },
    ],
    qualifiers: &["export", "default", "async", "public", "private", "static"],
    method_parents: &[SymbolKind::Class, SymbolKind::Trait],
    bare_method: true,
};

const C: &BracedLang = &BracedLang {
    rules: &[
        Rule { prefix: "struct", kind: SymbolKind::Struct },
        Rule { prefix: "union", kind: SymbolKind::Struct },
        Rule { prefix: "enum", kind: SymbolKind::Enum },
    ],
    qualifiers: &["static", "inline", "extern", "const", "unsigned", "signed"],
    method_parents: &[],
    bare_method: false,
};

/// State carried while sweeping a brace-counted language: which open construct
/// (if any) each brace level belongs to, so a function inside an `impl`/`class`
/// becomes a [`SymbolKind::Method`] and so we know whose `end_line` to close
/// when a `}` returns to that level.
struct OpenScope {
    /// Index into the result `Vec` of the symbol that opened at this depth
    /// (`None` for a brace block with no recognised owner, e.g. a bare `{`).
    sym: Option<usize>,
    /// The construct kind that owns this scope (for the method override).
    kind: SymbolKind,
}

/// Single left-to-right, brace-counted sweep. Recognises a definition by its
/// leading keyword (after stripping `lang.qualifiers` and any `pub(...)`),
/// records it with the current `depth`, and — tracking the scope stack — sets
/// each symbol's `end_line` to the line whose `}` closes its block (or the
/// declaration line itself for a `;`-terminated item with no body).
///
/// Braces inside `//` line comments, `/* … */` block comments and `"`/`'`/
/// `` ` `` string or char literals are ignored, reusing the comment/string
/// skip idea from [`Diff`](crate::Diff)'s syntax tokenizer (kept simple: the
/// scan is line-oriented and a reading aid, not a compiler).
fn scan_braced(lines: &[&str], lang: &BracedLang) -> Vec<Symbol> {
    let mut out: Vec<Symbol> = Vec::new();
    let mut scopes: Vec<OpenScope> = Vec::new();
    // Carries an open `/* … */` across lines.
    let mut in_block_comment = false;

    for (lineno, raw) in lines.iter().enumerate() {
        let line = no_cr(raw);
        let trimmed = line.trim_start();

        // Depth = current open-brace nesting *before* this line's braces.
        let depth_here = scopes.len().min(u16::MAX as usize) as u16;

        // Try to recognise a definition starting this line (only when not
        // mid-block-comment). A keyword-introduced def first; failing that,
        // for languages with `bare_method` (JS/TS), a bare `name(args) {`
        // *directly inside* a class/interface scope is a method.
        let recognised = if in_block_comment {
            None
        } else {
            recognise(trimmed, lang).or_else(|| {
                let in_type_body = scopes
                    .last()
                    .is_some_and(|s| lang.method_parents.contains(&s.kind));
                if lang.bare_method && in_type_body {
                    recognise_bare_method(trimmed)
                } else {
                    None
                }
            })
        };

        if let Some((name, mut kind)) = recognised {
            // `depth_here` decides Function→Method: strictly inside a
            // type-body scope.
            if kind == SymbolKind::Function
                && scopes
                    .last()
                    .is_some_and(|s| lang.method_parents.contains(&s.kind))
            {
                kind = SymbolKind::Method;
            }
            out.push(Symbol {
                name,
                kind,
                line: lineno,
                end_line: lineno, // provisional; closed on matching `}`
                depth: depth_here,
            });
            let idx = out.len() - 1;
            // Walk this line's delimiters; if the construct opens a `{`
            // on this very line it pushes a scope owned by `idx`.
            sweep_line_at(
                lineno,
                line,
                &mut scopes,
                &mut out,
                &mut in_block_comment,
                Some(idx),
            );
            continue;
        }

        // No definition here: still must count this line's braces/comments so
        // depth and the scope stack stay correct.
        sweep_line_at(lineno, line, &mut scopes, &mut out, &mut in_block_comment, None);
    }

    // EOF: anything whose body never closed (no matching `}`, or a `;`-less
    // item that hit end of input) ends on the last line. `end_line` is
    // otherwise already the closing-brace line. Finally clamp so the
    // invariant `line <= end_line < line_count` always holds.
    let last = lines.len().saturating_sub(1);
    for s in &mut out {
        if s.end_line < s.line {
            s.end_line = last;
        }
        s.end_line = s.end_line.clamp(s.line, last);
    }
    out
}

/// Walks one physical line's delimiters in a single pass, maintaining the
/// brace `scopes` stack and closing the `end_line` of any symbol whose `}` is
/// seen on `cur_line`. `opening` is `Some(idx)` when this line *began* a
/// recognised construct, so the first `{` on the line is attributed to that
/// symbol and a `;` reached before any `{` marks it body-less (its `end_line`
/// is `cur_line`). `in_block_comment` persists an open `/* … */` across lines.
/// Braces inside `//` comments, `/* … */` and `"`/`'`/`` ` `` literals are
/// skipped — the comment/string-skip idea from [`Diff`](crate::Diff)'s syntax
/// tokenizer, kept line-oriented and simple.
fn sweep_line_at(
    cur_line: usize,
    line: &str,
    scopes: &mut Vec<OpenScope>,
    out: &mut [Symbol],
    in_block_comment: &mut bool,
    opening: Option<usize>,
) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut pending = opening; // Some(idx) until its `{` or `;`
    while i < chars.len() {
        let c = chars[i];

        if *in_block_comment {
            if c == '*' && chars.get(i + 1) == Some(&'/') {
                *in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        // Line comment: `//` (or `#` — harmless for the brace languages, none
        // use `#` for code) ends the line.
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            break;
        }
        // Block comment open.
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            *in_block_comment = true;
            i += 2;
            continue;
        }
        // String / char literal — skip to its close, honouring `\` escape.
        if c == '"' || c == '\'' || c == '`' {
            let q = c;
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == q {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        if c == '{' {
            // This brace either belongs to the pending recognised symbol or
            // is an anonymous block.
            match pending.take() {
                Some(idx) => scopes.push(OpenScope {
                    sym: Some(idx),
                    kind: out[idx].kind,
                }),
                None => scopes.push(OpenScope { sym: None, kind: SymbolKind::Other }),
            }
        } else if c == '}' {
            if let Some(scope) = scopes.pop() {
                if let Some(idx) = scope.sym {
                    out[idx].end_line = cur_line;
                }
            }
        } else if c == ';' && pending.is_some() {
            // A `;` before any `{` ⇒ body-less item (a `;` `struct`/`const`/
            // a Rust `fn` declaration in a trait): it ends on this line.
            if let Some(idx) = pending.take() {
                out[idx].end_line = cur_line;
            }
        }
        i += 1;
    }
}

/// Recognise a definition at the *start* of `trimmed` (already left-trimmed).
/// Strips `lang.qualifiers` (whole words) and any `pub(...)` / `(...)`
/// qualifier group, then matches the first [`Rule`] whose `prefix` is a whole
/// leading word, and extracts the name. `None` when nothing matches.
fn recognise(trimmed: &str, lang: &BracedLang) -> Option<(String, SymbolKind)> {
    // Skip an attribute / decorator line entirely (`#[...]`, `#![...]`, a TS
    // `@decorator` on its own line) — it precedes the def, it is not one.
    let t0 = trimmed.trim_start();
    if t0.starts_with("#[") || t0.starts_with("#![") || t0.starts_with('@') {
        return None;
    }

    // Whether `w` is one of this language's rule keywords (a whole-word
    // introducer like `fn`, `struct`, `const`).
    let is_rule_kw = |w: &str| lang.rules.iter().any(|ru| ru.prefix == w);
    let is_qual = |w: &str| lang.qualifiers.contains(&w);

    // Strip leading qualifier words and `(...)` groups (e.g. `pub(crate)`).
    // A word that is *both* a qualifier and a rule keyword (`const`/`static`
    // in Rust, `const`/`let`/`var` in JS) is only consumed-as-qualifier when
    // the next word is itself a qualifier or a rule keyword (so `const fn` /
    // `const unsafe fn` strip `const`, but `const MAX = 1;` / `let x = …`
    // keep it for the rule layer — which is what makes it a Constant/binding).
    let mut rest = t0;
    loop {
        let r = rest.trim_start();
        let mut advanced = false;
        for q in lang.qualifiers {
            let Some(after) = starts_word(r, q) else {
                continue;
            };
            if is_rule_kw(q) {
                // Peek the next word; only treat `q` as a qualifier when the
                // construct continues with another qualifier or a keyword.
                let nxt = r[after..].trim_start();
                let next_word: String =
                    nxt.chars().take_while(|&c| is_ident(c)).collect();
                if !(is_qual(&next_word) || is_rule_kw(&next_word)) {
                    break; // `q` introduces a binding — let the rules see it
                }
            }
            rest = &r[after..];
            // An immediately-following `(...)` visibility group (`pub(crate)`).
            let r2 = rest.trim_start();
            if r2.starts_with('(') {
                if let Some(close) = r2.find(')') {
                    rest = &r2[close + 1..];
                }
            }
            advanced = true;
            break;
        }
        if !advanced {
            break;
        }
    }
    let rest = rest.trim_start();

    for rule in lang.rules {
        if let Some(after) = starts_word(rest, rule.prefix) {
            let tail = &rest[after..];
            if let Some((name, kind)) = extract_name(rule, tail, rest) {
                return Some((name, kind));
            }
            // The keyword matched but it is not actually a definition we emit
            // (e.g. a JS `const x = 1;` with no function/arrow): stop — no
            // other rule can match the same leading word.
            return None;
        }
    }
    None
}

/// Recognise a **bare class method** — `name(args) {` / `name(args): T {` /
/// `get name() {` / `async name() {` / `*gen() {` — used only inside a JS/TS
/// class or interface scope (the caller gates this). It is deliberately
/// conservative: the line must, after an optional `get`/`set`/`async`/`static`/
/// `*` prefix, be `identifier` then `(`, and `name` must not be a control-flow
/// keyword (`if`/`for`/`while`/`switch`/`catch`/`return`/`function`/…) so a
/// `if (x) {` body is never mistaken for a method. The TS interface member
/// `area(): number;` also matches (a `;`-terminated body-less method).
fn recognise_bare_method(trimmed: &str) -> Option<(String, SymbolKind)> {
    let t0 = trimmed.trim_start();
    if t0.starts_with("#[") || t0.starts_with('@') || t0.starts_with("//") {
        return None;
    }
    // Strip leading method qualifiers / a generator `*`.
    let mut s = t0;
    loop {
        let r = s.trim_start();
        let stripped = ["get", "set", "async", "static", "public", "private"]
            .iter()
            .find_map(|q| starts_word(r, q).map(|n| &r[n..]))
            .or_else(|| r.strip_prefix('*'));
        match stripped {
            Some(next) => s = next,
            None => break,
        }
    }
    let s = s.trim_start();
    let (name, end) = next_ident(s, 0)?;
    // Reject keywords that can be followed by `(` but are not methods.
    const NOT_METHOD: &[&str] = &[
        "if", "for", "while", "switch", "catch", "return", "function",
        "do", "else", "with", "await", "yield", "new", "delete", "typeof",
        "void", "in", "of", "case", "throw", "constructor",
    ];
    // `constructor` *is* a method in JS; keep it (remove from the reject set
    // by special-casing): only the genuine control words are rejected.
    if NOT_METHOD.contains(&name.as_str()) && name != "constructor" {
        return None;
    }
    // Next non-space must be `(` — i.e. `name(`.
    let after = s[end..].trim_start();
    if !after.starts_with('(') {
        return None;
    }
    // It must look like a definition: a `{` (body) or a `;` (interface
    // member) somewhere after the matching `)`, not e.g. a call `foo(x);`
    // used as a statement — but inside a class/interface body a bare
    // `name(...)` line is overwhelmingly a method, so accept it. The
    // brace/`;` close is handled by the sweeper.
    Some((name, SymbolKind::Method))
}

/// Pull the symbol name from `tail` (the slice just past a matched
/// [`Rule::prefix`]); `whole` is the full qualifier-stripped line, used for
/// the language-specific shapes (Go `type … struct/interface`, Go method
/// receiver, a JS function-valued `const`).
fn extract_name(rule: &Rule, tail: &str, whole: &str) -> Option<(String, SymbolKind)> {
    match rule.kind {
        // Go `func (r *T) Name(...)` → Method "Name"; `func Name(...)` →
        // Function. Detect a leading `(` receiver group.
        SymbolKind::Function if whole.starts_with("func") => {
            let t = tail.trim_start();
            if t.starts_with('(') {
                let close = t.find(')')?;
                let after = &t[close + 1..];
                let (name, _) = next_ident(after, 0)?;
                Some((name, SymbolKind::Method))
            } else {
                let (name, _) = next_ident(t, 0)?;
                Some((name, SymbolKind::Function))
            }
        }
        // Go `type X struct {` / `type X interface {` / `type X = …`.
        SymbolKind::Other if whole.starts_with("type") => {
            let (name, end) = next_ident(tail, 0)?;
            let after = tail[end..].trim_start();
            if after.starts_with("struct") {
                Some((name, SymbolKind::Struct))
            } else if after.starts_with("interface") {
                Some((name, SymbolKind::Trait))
            } else {
                Some((name, SymbolKind::Other))
            }
        }
        // JS/TS `const|let|var name = function|(... )=>|async (...) =>` ⇒ a
        // function. Anything else bound is not emitted.
        SymbolKind::Function
            if whole.starts_with("const")
                || whole.starts_with("let")
                || whole.starts_with("var") =>
        {
            let (name, end) = next_ident(tail, 0)?;
            let rhs = tail[end..].trim_start();
            // Must be `= function` / `= (...) =>` / `= async ...` / `= x =>`.
            let rhs = rhs.strip_prefix('=')?.trim_start();
            let looks_fn = rhs.starts_with("function")
                || rhs.starts_with("async")
                || rhs.starts_with('(')
                || rhs
                    .find("=>")
                    .is_some_and(|a| !rhs[..a].contains([';', '{']));
            if looks_fn {
                Some((name, SymbolKind::Function))
            } else {
                None
            }
        }
        // Rust `impl` — the name is the *type* implemented: for
        // `impl Trait for Type` use `Type`; for `impl<…> Type` use `Type`.
        SymbolKind::Impl => {
            // Drop a generic `<...>` right after `impl`.
            let t = skip_generics(tail.trim_start());
            // Tokens up to `{`, `where`, or end. If there is a ` for `, the
            // implemented type is the token *after* `for`.
            let head = t
                .split('{')
                .next()
                .unwrap_or(t)
                .split(" where ")
                .next()
                .unwrap_or(t);
            let target = if let Some(pos) = head.find(" for ") {
                &head[pos + 5..]
            } else {
                head
            };
            let target = skip_generics(target.trim_start());
            let (name, _) = next_ident(target, 0)?;
            Some((name, SymbolKind::Impl))
        }
        // Default: the next identifier after the keyword is the name.
        kind => {
            let (name, _) = next_ident(tail, 0)?;
            Some((name, kind))
        }
    }
}

/// If `s` begins with a `<` generic group, return the slice just past the
/// balanced `<…>`; otherwise `s` unchanged. Depth-counted so
/// `<A<B>, C>` is skipped whole. Total: an unbalanced `<` returns `s`.
fn skip_generics(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'<') {
        return s;
    }
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return s[i + 1..].trim_start();
                }
            }
            _ => {}
        }
    }
    s
}

// ---------------------------------------------------------------------------
// Python — indentation-based
// ---------------------------------------------------------------------------

/// Indentation-scanned Python. A `class`/`def` opens a block; the block ends
/// at the first later **non-blank** line whose indent is `<=` the def's
/// indent. `depth` is the count of enclosing open defs/classes; a `def`
/// directly inside a `class` is a [`SymbolKind::Method`].
fn scan_python(lines: &[&str]) -> Vec<Symbol> {
    let mut out: Vec<Symbol> = Vec::new();
    // Stack of (indent, result-index, kind) for currently-open blocks.
    let mut stack: Vec<(usize, usize, SymbolKind)> = Vec::new();

    for (lineno, raw) in lines.iter().enumerate() {
        let line = no_cr(raw);
        if is_blank(line) {
            continue;
        }
        let indent = indent_of(line);

        // Close every block this line has dedented out of.
        while let Some(&(open_indent, idx, _)) = stack.last() {
            if indent <= open_indent {
                // The block ended on the previous non-blank line.
                let prev = last_nonblank_before(lines, lineno);
                out[idx].end_line = prev.max(out[idx].line);
                stack.pop();
            } else {
                break;
            }
        }

        let body = line.trim_start();
        // A decorator line (`@app.route(...)`) precedes a def; skip it.
        if body.starts_with('@') {
            continue;
        }

        let (kind, kw_len) = if let Some(n) = starts_word(body, "class") {
            (SymbolKind::Class, n)
        } else if let Some(n) = starts_word(body, "def") {
            (SymbolKind::Function, n)
        } else if let Some(n) = starts_word(body, "async") {
            // `async def name(...)`
            let after = body[n..].trim_start();
            if let Some(dn) = starts_word(after, "def") {
                // name follows `def`
                let off = (body.len() - after.len()) + dn;
                (SymbolKind::Function, off)
            } else {
                continue;
            }
        } else {
            continue;
        };

        let Some((name, _)) = next_ident(body, kw_len) else {
            continue;
        };
        let mut kind = kind;
        if kind == SymbolKind::Function
            && stack.last().is_some_and(|&(_, _, k)| k == SymbolKind::Class)
        {
            kind = SymbolKind::Method;
        }
        let depth = stack.len().min(u16::MAX as usize) as u16;
        out.push(Symbol {
            name,
            kind,
            line: lineno,
            end_line: lineno,
            depth,
        });
        stack.push((indent, out.len() - 1, kind));
    }

    // EOF closes everything still open at the last non-blank line.
    let last = last_nonblank_before(lines, lines.len());
    for &(_, idx, _) in &stack {
        out[idx].end_line = last.max(out[idx].line);
    }
    out
}

/// The index of the last non-blank line strictly before `before` (0-based),
/// or `0` if there is none.
fn last_nonblank_before(lines: &[&str], before: usize) -> usize {
    (0..before.min(lines.len()))
        .rev()
        .find(|&i| !is_blank(no_cr(lines[i])))
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Markdown — ATX headings
// ---------------------------------------------------------------------------

/// ATX-heading scan. `#`..`######` at the very start of a line (after at most
/// three leading spaces, per CommonMark) is a [`SymbolKind::Heading`] with
/// `depth = level - 1`. Its `end_line` is the line before the next heading of
/// equal or shallower level (or EOF). A `#` not followed by a space (or with
/// more than six) is not a heading. Fenced code blocks (```` ``` ````) are
/// skipped so a `#` comment inside a code fence is not mistaken for a heading.
fn scan_markdown(lines: &[&str]) -> Vec<Symbol> {
    let mut out: Vec<Symbol> = Vec::new();
    let mut in_fence = false;
    let mut fence: Option<char> = None;

    for (lineno, raw) in lines.iter().enumerate() {
        let line = no_cr(raw);
        let t = line.trim_start();

        // Toggle a ``` / ~~~ fence (at most 3 leading spaces).
        if (line.len() - t.len()) <= 3
            && (t.starts_with("```") || t.starts_with("~~~"))
        {
            let marker = t.as_bytes()[0] as char;
            match fence {
                None => {
                    in_fence = true;
                    fence = Some(marker);
                }
                Some(m) if m == marker => {
                    in_fence = false;
                    fence = None;
                }
                _ => {}
            }
            continue;
        }
        if in_fence {
            continue;
        }

        // Up to 3 spaces of indent then 1..=6 `#` then a space (or EOL).
        if (line.len() - t.len()) > 3 || !t.starts_with('#') {
            continue;
        }
        let hashes = t.chars().take_while(|&c| c == '#').count();
        if !(1..=6).contains(&hashes) {
            continue;
        }
        let after = &t[hashes..];
        if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
            continue;
        }
        // Heading text: trim a trailing run of `#` and surrounding space.
        let name = after
            .trim()
            .trim_end_matches('#')
            .trim()
            .to_string();
        let level = hashes as u16;
        out.push(Symbol {
            name,
            kind: SymbolKind::Heading,
            line: lineno,
            end_line: lineno, // closed below
            depth: level - 1,
        });
    }

    // end_line of heading i = (line of the next heading whose level <= i's
    // level) - 1, else last line.
    let last = lines.len().saturating_sub(1);
    for i in 0..out.len() {
        let my_level = out[i].depth;
        let mut end = last;
        for nxt in &out[i + 1..] {
            if nxt.depth <= my_level {
                end = nxt.line.saturating_sub(1).max(out[i].line);
                break;
            }
        }
        out[i].end_line = end.max(out[i].line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compact view of a result for the exact-match assertions:
    /// `(name, kind, line, depth)`.
    fn nkld(o: &Outline) -> Vec<(&str, SymbolKind, usize, u16)> {
        o.0.iter()
            .map(|s| (s.name.as_str(), s.kind, s.line, s.depth))
            .collect()
    }

    #[test]
    fn language_from_path_is_case_insensitive_and_extension_only() {
        use Language::*;
        assert_eq!(Language::from_path("src/app.rs"), Rust);
        assert_eq!(Language::from_path("SRC/APP.RS"), Rust);
        assert_eq!(Language::from_path("a/b/c.py"), Python);
        assert_eq!(Language::from_path("main.GO"), Go);
        assert_eq!(Language::from_path("x.tsx"), TypeScript);
        assert_eq!(Language::from_path("x.mjs"), JavaScript);
        assert_eq!(Language::from_path("v.h"), C);
        assert_eq!(Language::from_path("README.markdown"), Markdown);
        assert_eq!(Language::from_path("Makefile"), Unknown);
        assert_eq!(Language::from_path(".gitignore"), Unknown);
        assert_eq!(Language::from_path("dir.v2/file"), Unknown);
        assert_eq!(Language::from_path(""), Unknown);
    }

    #[test]
    fn rust_module_struct_impl_and_nested_methods() {
        let src = "\
//! crate doc
pub mod parser {
    /// the lexer
    pub struct Lexer {
        pos: usize,
    }

    pub enum Tok { A, B }

    impl Lexer {
        pub fn new() -> Self { Self { pos: 0 } }
        fn step(&mut self) {
        }
    }
}

const MAX: usize = 8;

fn main() {}
";
        let o = Outline::scan(src, Language::Rust);
        assert_eq!(
            nkld(&o),
            vec![
                ("parser", SymbolKind::Module, 1, 0),
                ("Lexer", SymbolKind::Struct, 3, 1),
                ("Tok", SymbolKind::Enum, 7, 1),
                ("Lexer", SymbolKind::Impl, 9, 1),
                ("new", SymbolKind::Method, 10, 2),
                ("step", SymbolKind::Method, 11, 2),
                ("MAX", SymbolKind::Constant, 16, 0),
                ("main", SymbolKind::Function, 18, 0),
            ]
        );
        // Deterministic end_lines (brace-closed).
        let by = |n: &str| o.0.iter().find(|s| s.name == n).unwrap();
        assert_eq!((by("parser").line, by("parser").end_line), (1, 14));
        assert_eq!((by("Lexer").end_line), 5); // the struct, first match
        assert_eq!((by("step").line, by("step").end_line), (11, 12));
        assert_eq!((by("main").line, by("main").end_line), (18, 18));
        // body-less items end on their own line.
        assert_eq!((by("Tok").line, by("Tok").end_line), (7, 7));
        assert_eq!((by("MAX").line, by("MAX").end_line), (16, 16));
    }

    #[test]
    fn rust_impl_trait_for_type_uses_the_type_name() {
        let src = "\
impl<T: Clone> std::fmt::Display for Wrapper<T> {
    fn fmt(&self) {}
}
pub(crate) fn helper() {}
";
        let o = Outline::scan(src, Language::Rust);
        assert_eq!(
            nkld(&o),
            vec![
                ("Wrapper", SymbolKind::Impl, 0, 0),
                ("fmt", SymbolKind::Method, 1, 1),
                ("helper", SymbolKind::Function, 3, 0),
            ]
        );
    }

    #[test]
    fn rust_braces_in_strings_and_comments_do_not_nest() {
        let src = "\
fn a() {
    let s = \"} not a close {\";
    // } also not
    let c = '}';
}
fn b() {}
";
        let o = Outline::scan(src, Language::Rust);
        assert_eq!(
            nkld(&o),
            vec![
                ("a", SymbolKind::Function, 0, 0),
                ("b", SymbolKind::Function, 5, 0),
            ]
        );
        assert_eq!(o.0[0].end_line, 4); // closes on the real `}`
    }

    #[test]
    fn python_class_methods_by_indentation() {
        let src = "\
import os


@decorator
class Greeter:
    name = \"x\"

    def __init__(self):
        self.k = 1

    async def greet(self):
        return 1


def free():
    pass
";
        let o = Outline::scan(src, Language::Python);
        assert_eq!(
            nkld(&o),
            vec![
                ("Greeter", SymbolKind::Class, 4, 0),
                ("__init__", SymbolKind::Method, 7, 1),
                ("greet", SymbolKind::Method, 10, 1),
                ("free", SymbolKind::Function, 14, 0),
            ]
        );
        // `Greeter` spans to the last line of `greet` (line 11), not into the
        // dedented `def free`.
        let g = o.0.iter().find(|s| s.name == "Greeter").unwrap();
        assert_eq!(g.end_line, 11);
        let init = o.0.iter().find(|s| s.name == "__init__").unwrap();
        assert_eq!((init.line, init.end_line), (7, 8));
        let free = o.0.iter().find(|s| s.name == "free").unwrap();
        assert_eq!((free.line, free.end_line), (14, 15));
    }

    #[test]
    fn go_funcs_methods_and_types() {
        let src = "\
package main

type Shape interface {
    Area() float64
}

type Rect struct {
    w int
}

func (r Rect) Area() float64 {
    return r.w
}

func main() {
}
";
        let o = Outline::scan(src, Language::Go);
        assert_eq!(
            nkld(&o),
            vec![
                ("Shape", SymbolKind::Trait, 2, 0),
                ("Rect", SymbolKind::Struct, 6, 0),
                ("Area", SymbolKind::Method, 10, 0),
                ("main", SymbolKind::Function, 14, 0),
            ]
        );
        let shape = o.0.iter().find(|s| s.name == "Shape").unwrap();
        assert_eq!((shape.line, shape.end_line), (2, 4));
    }

    #[test]
    fn js_class_function_and_arrow_const() {
        let src = "\
import x from 'y';

export class Widget {
    render() {
    }
}

function plain(a) {
    return a;
}

const add = (a, b) => a + b;
const obj = { not: 1 };
export const fetchIt = async () => {
    return 2;
};
interface Shape {
    area(): number;
}
";
        let o = Outline::scan(src, Language::TypeScript);
        assert_eq!(
            nkld(&o),
            vec![
                ("Widget", SymbolKind::Class, 2, 0),
                ("render", SymbolKind::Method, 3, 1),
                ("plain", SymbolKind::Function, 7, 0),
                ("add", SymbolKind::Function, 11, 0),
                ("fetchIt", SymbolKind::Function, 13, 0),
                ("Shape", SymbolKind::Trait, 16, 0),
                // TS interface member: a bare `area(): number;` inside an
                // `interface` scope is a (body-less) Method, depth 1.
                ("area", SymbolKind::Method, 17, 1),
            ]
        );
        // `const obj = { not: 1 };` is not a function ⇒ no symbol for it.
        assert!(o.0.iter().all(|s| s.name != "obj"));
        // The `;`-terminated interface member ends on its own line.
        let area = o.0.iter().find(|s| s.name == "area").unwrap();
        assert_eq!((area.line, area.end_line), (17, 17));
        // `interface Shape` brace-closes on line 18.
        let shape = o.0.iter().find(|s| s.name == "Shape").unwrap();
        assert_eq!((shape.line, shape.end_line), (16, 18));
    }

    #[test]
    fn c_structs_and_functions() {
        let src = "\
#include <stdio.h>

struct Point {
    int x;
};

static int add(int a, int b) {
    return a + b;
}

int main(void) {
    return 0;
}
";
        let o = Outline::scan(src, Language::C);
        // Brace-only C heuristic: the `struct` is recognised; functions are
        // best-effort (a bare `int add(...) {` has no introducing keyword in
        // the rule set, so it is not emitted — Tier-0 floor, by design).
        let kinds: Vec<_> = o.0.iter().map(|s| (s.name.as_str(), s.kind)).collect();
        assert_eq!(kinds, vec![("Point", SymbolKind::Struct)]);
        assert_eq!((o.0[0].line, o.0[0].end_line), (2, 4));
    }

    #[test]
    fn markdown_heading_tree_and_endlines() {
        let src = "\
# Title

intro

## Section A
text
### Sub A1
more

## Section B

```
# not a heading (fenced)
```

text
";
        let o = Outline::scan(src, Language::Markdown);
        assert_eq!(
            nkld(&o),
            vec![
                ("Title", SymbolKind::Heading, 0, 0),
                ("Section A", SymbolKind::Heading, 4, 1),
                ("Sub A1", SymbolKind::Heading, 6, 2),
                ("Section B", SymbolKind::Heading, 9, 1),
            ]
        );
        let title = &o.0[0];
        assert_eq!(title.end_line, 16); // to EOF (last line index)
        let sa = &o.0[1];
        assert_eq!(sa.end_line, 8); // line before `## Section B`
        let sub = &o.0[2];
        assert_eq!(sub.end_line, 8);
        let sb = &o.0[3];
        assert_eq!(sb.end_line, 16);
    }

    #[test]
    fn markdown_hash_without_space_or_too_deep_is_not_a_heading() {
        let src = "#nospace\n####### seven\n#\n";
        let o = Outline::scan(src, Language::Markdown);
        // `#` alone (line 2) IS a heading (level 1, empty text); the other two
        // are not.
        assert_eq!(nkld(&o), vec![("", SymbolKind::Heading, 2, 0)]);
    }

    #[test]
    fn at_line_returns_the_deepest_enclosing_symbol() {
        let src = "\
mod m {
    fn outer() {
        let x = 1;
    }
}
";
        let o = Outline::scan(src, Language::Rust);
        // Line 2 (`let x = 1;`) is inside both `m` (depth 0) and `outer`
        // (depth 1) — the method/fn wins.
        assert_eq!(o.at_line(2).map(|s| s.name.as_str()), Some("outer"));
        // Line 0 is only inside `m`.
        assert_eq!(o.at_line(0).map(|s| s.name.as_str()), Some("m"));
        // Past EOF / outside everything ⇒ None.
        assert_eq!(o.at_line(999), None);
    }

    #[test]
    fn unknown_language_and_empty_input_yield_empty_outline() {
        assert_eq!(Outline::scan("anything {\n}\n", Language::Unknown), Outline::default());
        assert_eq!(Outline::scan("", Language::Rust), Outline(vec![]));
        assert_eq!(Outline::scan("", Language::Markdown), Outline(vec![]));
        assert_eq!(Outline::scan("", Language::Python), Outline(vec![]));
    }

    #[test]
    fn empty_and_whitespace_inputs_never_panic_all_languages() {
        for lang in [
            Language::Rust,
            Language::Python,
            Language::Go,
            Language::JavaScript,
            Language::TypeScript,
            Language::C,
            Language::Markdown,
            Language::Unknown,
        ] {
            for src in ["", "\n", "   ", "\n\n\n", "\t", "\r\n", "}", "{{{{", "###"] {
                let _ = Outline::scan(src, lang); // must not panic
            }
        }
    }

    /// The totality property (the iter-25 "a pure projection must be total"
    /// rule): a fixed-seed LCG feeds thousands of random byte/line soups —
    /// drawn from a code-flavoured alphabet *and* every language — and the
    /// scan must never panic and must always return a well-formed outline:
    /// symbols in non-decreasing `line` order, every `line <= end_line` with
    /// both valid line indices, and `depth` small (`< 64`). Deterministic with
    /// no `rand` dependency — the same LCG shape `rstui-core`'s
    /// `text_area.rs` tests use.
    #[test]
    fn random_soups_are_total_and_well_formed_for_every_language() {
        // Fixed-seed LCG (the rstui-core text_area.rs constants) keeps the run
        // deterministic with zero deps.
        let mut state: u64 = 0x0bad_f00d_dead_beef;
        let mut rng = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };

        // A code-flavoured alphabet: keywords/punctuation that exercise the
        // brace/indent/heading paths, plus raw bytes for the "garbage" case.
        let toks: &[&str] = &[
            "fn ", "pub ", "mod ", "struct ", "enum ", "impl ", "trait ",
            "def ", "class ", "async ", "func ", "type ", "interface ",
            "const ", "static ", "let ", "var ", "function ", "return ",
            "name", "Foo", "x", "(", ")", "{", "}", "<", ">", ";", "=",
            "=>", "//", "/*", "*/", "\"q\"", "'c'", "`t`", "#", "##",
            "######", "@deco", "#[attr]", " for ", " where ", "  ", "\t",
            "struct {", "interface {", "( r *T ) ", "\\",
        ];
        let langs = [
            Language::Rust,
            Language::Python,
            Language::Go,
            Language::JavaScript,
            Language::TypeScript,
            Language::C,
            Language::Markdown,
            Language::Unknown,
        ];

        for lang in langs {
            for _ in 0..2_000 {
                // Build a random multi-line soup.
                let line_count = (rng() % 40) as usize;
                let mut s = String::new();
                for _ in 0..line_count {
                    let pieces = (rng() % 8) as usize;
                    for _ in 0..pieces {
                        // Mostly tokens; occasionally a raw control/UTF byte.
                        if rng() % 16 == 0 {
                            let cands = ['\u{0}', 'é', '日', '\t', '😀', '\\', '"'];
                            s.push(cands[(rng() % cands.len() as u64) as usize]);
                        } else {
                            s.push_str(toks[(rng() % toks.len() as u64) as usize]);
                        }
                    }
                    s.push('\n');
                }

                let o = Outline::scan(&s, lang); // Invariant: no panic.
                let total_lines = s.split('\n').count();

                let mut prev_line = 0usize;
                for (i, sym) in o.0.iter().enumerate() {
                    // Non-decreasing line order (pre-order, parents first).
                    if i > 0 {
                        assert!(
                            sym.line >= prev_line,
                            "lang {lang:?}: line order regressed at {i}: \
                             {} < {prev_line}",
                            sym.line
                        );
                    }
                    prev_line = sym.line;

                    // Every line/end_line is a valid index and ordered.
                    assert!(
                        sym.line < total_lines,
                        "lang {lang:?}: line {} >= total {total_lines}",
                        sym.line
                    );
                    assert!(
                        sym.line <= sym.end_line,
                        "lang {lang:?}: line {} > end_line {}",
                        sym.line,
                        sym.end_line
                    );
                    assert!(
                        sym.end_line < total_lines,
                        "lang {lang:?}: end_line {} >= total {total_lines}",
                        sym.end_line
                    );

                    // Depth stays sane.
                    assert!(
                        sym.depth < 64,
                        "lang {lang:?}: depth {} unreasonable",
                        sym.depth
                    );

                    // `at_line` is total and self-consistent: a line strictly
                    // inside this symbol always resolves to *some* enclosing
                    // symbol.
                    if sym.line <= sym.end_line {
                        let hit = o.at_line(sym.line);
                        assert!(
                            hit.is_some(),
                            "lang {lang:?}: at_line({}) lost its own symbol",
                            sym.line
                        );
                    }
                }

                // `at_line` past EOF is always None and never panics.
                assert!(o.at_line(total_lines + 5).is_none());
                assert!(o.at_line(usize::MAX).is_none());
            }
        }
    }
}
