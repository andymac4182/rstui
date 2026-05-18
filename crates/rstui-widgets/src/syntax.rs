//! Tier-0, dependency-free, language-aware **lexical syntax overlay**.
//!
//! This is the shared engine behind both [`Diff`](crate::diff::Diff) and
//! [`Editor`](crate::editor::Editor): a single left-to-right character
//! classifier that paints a per-character [`Style`]
//! overlay for one line of source — comments, strings, numbers and a
//! curated common-keyword core. It owns the canonical [`Language`] enum
//! (other widgets, e.g. an outline, are reconciled to reuse it).
//!
//! It is generalised — *not rewritten* — from the language-blind scanner
//! that previously lived in `diff.rs`. The contract that makes that
//! generalisation safe:
//!
//! - **Byte-identical default path.** [`Language::Unknown`] reproduces the
//!   old `diff.rs` `syntax_overlay`/`paint` algorithm *verbatim*: the same
//!   curated keyword set, the same priority order (line comment → block
//!   comment → string/char → number → word), the same single-line scope. It
//!   **ignores** `state_in` and never carries state, so a diff row (whose
//!   lines are not contiguous) keeps producing exactly the bytes it did
//!   before `diff.rs` is later swapped to call this module. Diff snapshot
//!   tests stay byte-identical.
//! - **Language awareness only when asked.** When `lang != Unknown` the
//!   comment leaders, block-comment / string delimiters and keyword set are
//!   the language's own, and multi-line constructs — C-family `/* … */`
//!   block comments and Python triple-quoted strings — carry across
//!   *contiguous* lines through [`LexState`]. This is the deep-dive's
//!   "comments/strings must never silently span lines" fix: for an editor
//!   document the caller threads the returned state into the next line.
//! - **Theme-agnostic.** The four token [`Style`]s come
//!   from the caller via [`SyntaxStyles`] (`Diff` passes its
//!   `DiffTheme.syntax_*`; `Editor` passes styles derived from
//!   `rstui-theme`). This module never names a colour.
//! - **Total.** [`line_overlay`] accepts any input under any language and
//!   never panics; the returned overlay length is always
//!   `content.chars().count()`.

use rstui_core::Style;

// ---------------------------------------------------------------------------
// Language
// ---------------------------------------------------------------------------

/// The languages the overlay can lex. [`Unknown`](Language::Unknown) is the
/// language-blind common-core mode that is byte-identical to the original
/// `diff.rs` scanner; every other variant adds that language's own comment /
/// string delimiters and keyword set (and, for the C-family and Python,
/// multi-line constructs carried through [`LexState`]).
///
/// This is the **canonical** definition shared across widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    /// Language-blind common core. Byte-identical to the legacy scanner;
    /// ignores [`LexState`].
    #[default]
    Unknown,
    /// Rust: `//` + `/* */`, `"`/raw `r"…"`/`r#"…"#`, `'…'`.
    Rust,
    /// Python: `#`, triple-quoted `"""…"""` / `'''…'''` (multi-line).
    Python,
    /// Go: `//` + `/* */`, `"…"` and raw `` `…` ``.
    Go,
    /// JavaScript: `//` + `/* */`, `"`/`'`/template `` `…` ``.
    JavaScript,
    /// TypeScript: same lexical surface as [`JavaScript`](Language::JavaScript).
    TypeScript,
    /// C / C++: `//` + `/* */`, `"…"` / `'…'`.
    C,
    /// POSIX shell: `#` to end of line, `"…"` / `'…'`.
    Shell,
    /// Markdown: no code tokens (prose); strings/numbers are *not* tinted.
    Markdown,
    /// JSON: strings and numbers only — no comments, no keywords.
    Json,
}

impl Language {
    /// Picks a language from a file path by its extension, case-insensitive.
    /// Unrecognised (or extension-less) paths map to
    /// [`Unknown`](Language::Unknown), which is the safe byte-identical
    /// common-core mode.
    #[must_use]
    pub fn from_path(path: &str) -> Self {
        // Last path component, then text after its final '.'. Pure string
        // work — no `std::path` so the rule is identical on every OS.
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let Some(dot) = name.rfind('.') else {
            return Language::Unknown;
        };
        let ext = name[dot + 1..].to_ascii_lowercase();
        match ext.as_str() {
            "rs" => Language::Rust,
            "py" | "pyi" | "pyw" => Language::Python,
            "go" => Language::Go,
            "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
            "ts" | "mts" | "cts" | "tsx" => Language::TypeScript,
            "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Language::C,
            "sh" | "bash" | "zsh" | "ksh" => Language::Shell,
            "md" | "markdown" | "mdown" | "mkd" => Language::Markdown,
            "json" | "jsonc" | "json5" => Language::Json,
            _ => Language::Unknown,
        }
    }
}

// ---------------------------------------------------------------------------
// Styles & carried state
// ---------------------------------------------------------------------------

/// The four token styles the overlay applies, supplied by the caller from
/// its own theme so this module stays theme-agnostic.
#[derive(Debug, Clone, Copy, Default)]
pub struct SyntaxStyles {
    /// Line and block comments.
    pub comment: Style,
    /// String / char / template literals.
    pub string: Style,
    /// Numeric literals.
    pub number: Style,
    /// A word in the active keyword set.
    pub keyword: Style,
}

/// Which raw-quote flavour an open string is, so escape handling on the
/// continuation lines matches the language. Only ever set for the
/// language-aware paths; [`Language::Unknown`] never spans lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum StringKind {
    /// Escapable single-char-delimited string (`"`, `'`, or non-raw
    /// backtick): a `\` escapes the next char.
    #[default]
    Escapable,
    /// Raw — no escape processing. Rust `r"…"`, Go / JS-template `` `…` ``.
    Raw,
    /// Python triple-quoted (`"""` / `'''`): closes only on a matching
    /// triple of the delimiter.
    Triple,
}

/// Lexer state carried across **contiguous** lines of one document.
///
/// For an editor the caller threads the [`LexState`] returned by
/// [`line_overlay`] into the next line, so a `/* … */` block comment or a
/// Python `"""…"""` string that opens on one line keeps colouring the lines
/// below it until it closes.
///
/// For a diff row (lines are *not* contiguous) the caller passes
/// [`LexState::default`] every line and ignores the returned state — and
/// the [`Language::Unknown`] path ignores it unconditionally, so that path
/// is byte-identical to the original single-line scanner.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LexState {
    /// Inside an unclosed C-family `/* … */` block comment.
    in_block_comment: bool,
    /// Inside an unclosed multi-line string: the delimiter char and how it
    /// escapes / closes. `None` when not in a string.
    in_string: Option<(char, StringKind)>,
}

// ---------------------------------------------------------------------------
// Per-language lexical rules
// ---------------------------------------------------------------------------

/// The curated, language-agnostic common keyword core used by
/// [`Language::Unknown`]. **Identical, in the same sort order, to the legacy
/// `diff.rs` `KEYWORDS`** so the binary search and the tinting decision are
/// byte-for-byte the same. Sorted for `binary_search` and easy auditing.
const COMMON_KEYWORDS: &[&str] = &[
    "abstract",
    "and",
    "as",
    "async",
    "await",
    "begin",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "data",
    "def",
    "default",
    "defer",
    "del",
    "do",
    "double",
    "elif",
    "else",
    "end",
    "enum",
    "except",
    "export",
    "extends",
    "extern",
    "false",
    "final",
    "finally",
    "float",
    "fn",
    "for",
    "from",
    "func",
    "function",
    "go",
    "goto",
    "if",
    "impl",
    "implements",
    "import",
    "in",
    "instanceof",
    "int",
    "interface",
    "is",
    "lambda",
    "let",
    "long",
    "loop",
    "match",
    "mod",
    "module",
    "move",
    "mut",
    "namespace",
    "new",
    "nil",
    "none",
    "not",
    "null",
    "object",
    "or",
    "package",
    "pass",
    "private",
    "protected",
    "pub",
    "public",
    "raise",
    "ref",
    "return",
    "select",
    "self",
    "short",
    "signed",
    "sizeof",
    "static",
    "str",
    "struct",
    "super",
    "switch",
    "template",
    "then",
    "this",
    "throw",
    "throws",
    "trait",
    "true",
    "try",
    "type",
    "typedef",
    "typeof",
    "union",
    "unsafe",
    "unsigned",
    "use",
    "using",
    "var",
    "void",
    "where",
    "while",
    "with",
    "yield",
];

// Per-language keyword sets. Each MUST stay sorted (asserted in tests) so
// the lookup can binary-search.

/// Rust keywords (incl. the contextual / reserved ones a reader expects to
/// see tinted).
const RUST_KEYWORDS: &[&str] = &[
    "Self", "as", "async", "await", "bool", "break", "char", "const", "continue", "crate", "dyn",
    "else", "enum", "extern", "f32", "f64", "false", "fn", "for", "i128", "i16", "i32", "i64",
    "i8", "if", "impl", "in", "isize", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "static", "str", "struct", "super", "trait", "true", "type", "u128", "u16",
    "u32", "u64", "u8", "union", "unsafe", "use", "usize", "where", "while",
];

/// Python keywords (PEP 3107 set plus the common soft keywords / builtins a
/// reader expects tinted).
const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "match", "nonlocal", "not", "or", "pass", "raise", "return", "try",
    "while", "with", "yield",
];

/// Go keywords plus the predeclared type names a reader expects tinted.
const GO_KEYWORDS: &[&str] = &[
    "any",
    "bool",
    "break",
    "byte",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "error",
    "fallthrough",
    "false",
    "float32",
    "float64",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "int",
    "int16",
    "int32",
    "int64",
    "int8",
    "interface",
    "map",
    "nil",
    "package",
    "range",
    "return",
    "rune",
    "select",
    "string",
    "struct",
    "switch",
    "true",
    "type",
    "uint",
    "uint16",
    "uint32",
    "uint64",
    "uint8",
    "uintptr",
    "var",
];

/// JavaScript / TypeScript keywords (the ES set plus the TS-only ones; the
/// union is fine — the highlight is a reading aid, not a type checker).
const JS_KEYWORDS: &[&str] = &[
    "abstract",
    "any",
    "as",
    "async",
    "await",
    "boolean",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "namespace",
    "never",
    "new",
    "null",
    "number",
    "object",
    "of",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "set",
    "static",
    "string",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "unknown",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// C / C++ keywords plus the common stdint type names a reader expects
/// tinted.
const C_KEYWORDS: &[&str] = &[
    "auto",
    "bool",
    "break",
    "case",
    "char",
    "class",
    "const",
    "constexpr",
    "continue",
    "default",
    "delete",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "false",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "namespace",
    "new",
    "nullptr",
    "operator",
    "private",
    "protected",
    "public",
    "register",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "template",
    "this",
    "throw",
    "true",
    "typedef",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
];

/// POSIX-shell keywords / builtins a reader expects tinted.
const SHELL_KEYWORDS: &[&str] = &[
    "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function", "if", "in",
    "local", "return", "select", "then", "until", "while",
];

/// The keyword set that applies under `lang`. Markdown and JSON have none.
fn keywords_for(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Unknown => COMMON_KEYWORDS,
        Language::Rust => RUST_KEYWORDS,
        Language::Python => PYTHON_KEYWORDS,
        Language::Go => GO_KEYWORDS,
        Language::JavaScript | Language::TypeScript => JS_KEYWORDS,
        Language::C => C_KEYWORDS,
        Language::Shell => SHELL_KEYWORDS,
        Language::Markdown | Language::Json => &[],
    }
}

/// Whether `word` is a keyword under `lang` (binary search; the set is
/// sorted). `Unknown` is exactly the legacy `is_keyword` decision.
fn is_keyword(word: &str, lang: Language) -> bool {
    keywords_for(lang).binary_search(&word).is_ok()
}

// ---------------------------------------------------------------------------
// The overlay
// ---------------------------------------------------------------------------

/// Sets the syntax overlay for char positions `start..end` (clamped to the
/// slice) to `style`. The legacy `diff.rs` `paint`, verbatim.
fn paint(overlay: &mut [Style], start: usize, end: usize, style: Style) {
    for slot in overlay.iter_mut().take(end).skip(start) {
        *slot = style;
    }
}

/// Builds the per-character syntax overlay for one line `content` under
/// `lang`, beginning in lexer state `state_in`, and returns
/// `(overlay, state_out)` where `state_out` is the lexer state *after* this
/// line — thread it into the next line for a contiguous document.
///
/// `overlay[i]` is the [`Style`] patch for char *i*, an
/// empty `Style` where nothing applies (same sentinel the legacy scanner
/// used). The returned vector always has length `content.chars().count()`.
///
/// [`Language::Unknown`] ignores `state_in`, never carries state, and
/// reproduces the original `diff.rs` single-line algorithm byte-for-byte
/// (line comment `//`/`--`/lone `#` → `/* */` block → `"`/`'`/`` ` ``
/// string → number → keyword). Every other language uses its own comment /
/// string delimiters and keyword set, and carries C-family block comments
/// and Python triple-quoted strings across contiguous lines via `state_in`.
///
/// Total: any input, any language, never panics.
#[must_use]
pub fn line_overlay(
    content: &str,
    lang: Language,
    styles: &SyntaxStyles,
    state_in: LexState,
) -> (Vec<Style>, LexState) {
    if lang == Language::Unknown {
        // The byte-identical legacy path. `state_in` is deliberately
        // ignored and no state is ever carried out.
        return (unknown_overlay(content, styles), LexState::default());
    }
    language_overlay(content, lang, styles, state_in)
}

/// The original `diff.rs` `syntax_overlay`, transcribed verbatim (only the
/// caller-supplied [`SyntaxStyles`] replaces the inline `DiffTheme` fields).
/// Single line, no carried state — a diff row is one line.
fn unknown_overlay(content: &str, styles: &SyntaxStyles) -> Vec<Style> {
    let chars: Vec<char> = content.chars().collect();
    let mut overlay = vec![Style::new(); chars.len()];
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];

        // Line comment: `//`, `--`, or `#` to end of line.
        if (c == '/' && chars.get(i + 1) == Some(&'/'))
            || (c == '-' && chars.get(i + 1) == Some(&'-'))
            || c == '#'
        {
            paint(&mut overlay, i, chars.len(), styles.comment);
            break;
        }

        // Block comment `/* … */` (single line; a diff row never spans lines).
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            let mut j = i + 2;
            while j < chars.len() {
                if chars[j] == '*' && chars.get(j + 1) == Some(&'/') {
                    j += 2;
                    break;
                }
                j += 1;
            }
            paint(&mut overlay, i, j, styles.comment);
            i = j;
            continue;
        }

        // String / char literal. `"` and `` ` `` honour a `\` escape; `'`
        // does too (covers escaped char literals without misreading a lone
        // apostrophe — an unterminated quote just colours to end of line).
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            let mut j = i + 1;
            while j < chars.len() {
                if chars[j] == '\\' {
                    j += 2;
                    continue;
                }
                if chars[j] == quote {
                    j += 1;
                    break;
                }
                j += 1;
            }
            let end = j.min(chars.len());
            paint(&mut overlay, i, end, styles.string);
            i = end;
            continue;
        }

        // Numeric literal: a digit (optionally a `0x`/`0o`/`0b` radix), then
        // the run of digits / hex letters / `_` / `.`, plus an `e±` exponent.
        if c.is_ascii_digit() {
            let mut j = i + 1;
            if c == '0' && matches!(chars.get(j), Some('x' | 'X' | 'o' | 'O' | 'b' | 'B')) {
                j += 1;
            }
            while j < chars.len() {
                let d = chars[j];
                // A hex/decimal digit, `_` separator, or a `.` that is
                // followed by another digit (so a method call like `1.foo`
                // does not absorb the dot) extends the literal.
                let extends = d.is_ascii_alphanumeric()
                    || d == '_'
                    || (d == '.' && chars.get(j + 1).is_some_and(char::is_ascii_digit));
                if !extends {
                    break;
                }
                j += 1;
            }
            paint(&mut overlay, i, j, styles.number);
            i = j;
            continue;
        }

        // Word run: a keyword gets the keyword style; any other identifier is
        // left to the row/word styling underneath.
        if c.is_alphanumeric() || c == '_' {
            let start = i;
            let mut j = i;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let word: String = chars[start..j].iter().collect();
            if is_keyword(&word, Language::Unknown) {
                paint(&mut overlay, start, j, styles.keyword);
            }
            i = j;
            continue;
        }

        i += 1;
    }
    overlay
}

/// Per-language lexical config: line-comment leaders, whether `/* */` block
/// comments apply, the string delimiters, whether a raw-string prefix (`r`)
/// applies (Rust), whether Python triple strings apply, and whether word
/// runs are matched at all (Markdown/JSON have no keywords).
struct LangSpec {
    /// Leaders that start a line comment (e.g. `["//"]`, `["#"]`). Each is
    /// 1–2 chars.
    line_comments: &'static [&'static str],
    /// `/* … */` block comments apply (carried via [`LexState`]).
    block_comments: bool,
    /// Quote chars that open a string. `'` is included only where it really
    /// delimits strings/chars in that language.
    string_delims: &'static [char],
    /// A backtick string is *raw* (no `\` escape): Go / JS template.
    backtick_raw: bool,
    /// Rust raw-string prefix `r"…"` / `r#"…"#` applies.
    rust_raw: bool,
    /// Python triple-quoted strings `"""` / `'''` apply (multi-line).
    python_triple: bool,
    /// Match word runs against the keyword set at all.
    has_keywords: bool,
    /// Recognise numeric literals.
    has_numbers: bool,
}

fn spec_for(lang: Language) -> LangSpec {
    match lang {
        Language::Unknown => unreachable!("Unknown uses unknown_overlay"),
        Language::Rust => LangSpec {
            line_comments: &["//"],
            block_comments: true,
            string_delims: &['"', '\''],
            backtick_raw: false,
            rust_raw: true,
            python_triple: false,
            has_keywords: true,
            has_numbers: true,
        },
        Language::Python => LangSpec {
            line_comments: &["#"],
            block_comments: false,
            string_delims: &['"', '\''],
            backtick_raw: false,
            rust_raw: false,
            python_triple: true,
            has_keywords: true,
            has_numbers: true,
        },
        Language::Go => LangSpec {
            line_comments: &["//"],
            block_comments: true,
            string_delims: &['"', '`'],
            backtick_raw: true,
            rust_raw: false,
            python_triple: false,
            has_keywords: true,
            has_numbers: true,
        },
        Language::JavaScript | Language::TypeScript => LangSpec {
            line_comments: &["//"],
            block_comments: true,
            string_delims: &['"', '\'', '`'],
            backtick_raw: true,
            rust_raw: false,
            python_triple: false,
            has_keywords: true,
            has_numbers: true,
        },
        Language::C => LangSpec {
            line_comments: &["//"],
            block_comments: true,
            string_delims: &['"', '\''],
            backtick_raw: false,
            rust_raw: false,
            python_triple: false,
            has_keywords: true,
            has_numbers: true,
        },
        Language::Shell => LangSpec {
            line_comments: &["#"],
            block_comments: false,
            string_delims: &['"', '\''],
            backtick_raw: false,
            rust_raw: false,
            python_triple: false,
            has_keywords: true,
            has_numbers: false,
        },
        Language::Markdown => LangSpec {
            line_comments: &[],
            block_comments: false,
            string_delims: &[],
            backtick_raw: false,
            rust_raw: false,
            python_triple: false,
            has_keywords: false,
            has_numbers: false,
        },
        Language::Json => LangSpec {
            line_comments: &[],
            block_comments: false,
            string_delims: &['"'],
            backtick_raw: false,
            rust_raw: false,
            python_triple: false,
            has_keywords: false,
            has_numbers: true,
        },
    }
}

/// Does a line comment start at `chars[i]`? Returns the leader length so the
/// caller can paint from `i`.
fn line_comment_at(chars: &[char], i: usize, spec: &LangSpec) -> bool {
    spec.line_comments.iter().any(|lead| {
        let lead: Vec<char> = lead.chars().collect();
        chars[i..].len() >= lead.len() && chars[i..i + lead.len()] == lead[..]
    })
}

/// The language-aware scanner. Carries C-family `/* */` block comments and
/// Python triple-quoted strings across contiguous lines via `state`.
fn language_overlay(
    content: &str,
    lang: Language,
    styles: &SyntaxStyles,
    mut state: LexState,
) -> (Vec<Style>, LexState) {
    let spec = spec_for(lang);
    let chars: Vec<char> = content.chars().collect();
    let mut overlay = vec![Style::new(); chars.len()];
    let mut i = 0;

    // --- Continuations from the previous line --------------------------

    // A `/* … */` that opened on an earlier line: paint until its `*/` (or
    // the whole line if it does not close here, keeping the state set).
    if state.in_block_comment {
        let mut j = 0;
        while j < chars.len() {
            if chars[j] == '*' && chars.get(j + 1) == Some(&'/') {
                j += 2;
                state.in_block_comment = false;
                break;
            }
            j += 1;
        }
        paint(&mut overlay, 0, j, styles.comment);
        i = j;
    } else if let Some((delim, kind)) = state.in_string {
        // A multi-line string (Python triple, or — defensively — a carried
        // raw/escapable string) that opened earlier.
        let j = scan_string_body(&chars, 0, delim, kind);
        paint(&mut overlay, 0, j.end, styles.string);
        if j.closed {
            state.in_string = None;
        }
        i = j.end;
    }

    // --- Fresh tokens on this line -------------------------------------

    while i < chars.len() {
        let c = chars[i];

        // Line comment to end of line.
        if line_comment_at(&chars, i, &spec) {
            paint(&mut overlay, i, chars.len(), styles.comment);
            break;
        }

        // Block comment `/* … */` — may not close on this line.
        if spec.block_comments && c == '/' && chars.get(i + 1) == Some(&'*') {
            let mut j = i + 2;
            let mut closed = false;
            while j < chars.len() {
                if chars[j] == '*' && chars.get(j + 1) == Some(&'/') {
                    j += 2;
                    closed = true;
                    break;
                }
                j += 1;
            }
            paint(&mut overlay, i, j, styles.comment);
            if !closed {
                state.in_block_comment = true;
            }
            i = j;
            continue;
        }

        // Rust raw string: `r"…"` or `r#…"…"#…` (any `#` count). The hash
        // run length must match to close; single-line (Rust raw strings can
        // span lines too, but the common case the reader needs is one line —
        // an unterminated one simply colours to end of line, like the
        // legacy scanner).
        if spec.rust_raw
            && c == 'r'
            && matches!(chars.get(i + 1), Some('"' | '#'))
            && !prev_is_ident(&chars, i)
        {
            let mut k = i + 1;
            let mut hashes = 0;
            while chars.get(k) == Some(&'#') {
                hashes += 1;
                k += 1;
            }
            if chars.get(k) == Some(&'"') {
                let mut j = k + 1;
                loop {
                    if j >= chars.len() {
                        break;
                    }
                    if chars[j] == '"' && closing_hashes_match(&chars, j + 1, hashes) {
                        j += 1 + hashes;
                        break;
                    }
                    j += 1;
                }
                let end = j.min(chars.len());
                paint(&mut overlay, i, end, styles.string);
                i = end;
                continue;
            }
        }

        // Python triple-quoted string `"""…"""` / `'''…'''` (multi-line).
        if spec.python_triple && (c == '"' || c == '\'') && is_triple(&chars, i, c) {
            let body = scan_string_body(&chars, i + 3, c, StringKind::Triple);
            paint(&mut overlay, i, body.end, styles.string);
            if !body.closed {
                state.in_string = Some((c, StringKind::Triple));
            }
            i = body.end;
            continue;
        }

        // Ordinary string / char / template literal.
        if spec.string_delims.contains(&c) {
            let kind = if c == '`' && spec.backtick_raw {
                StringKind::Raw
            } else {
                StringKind::Escapable
            };
            let body = scan_string_body(&chars, i + 1, c, kind);
            paint(&mut overlay, i, body.end, styles.string);
            // A raw backtick (Go / JS template) may legally span lines.
            if !body.closed && kind == StringKind::Raw {
                state.in_string = Some((c, kind));
            }
            i = body.end;
            continue;
        }

        // Numeric literal — same shape as the legacy scanner.
        if spec.has_numbers && c.is_ascii_digit() {
            let mut j = i + 1;
            if c == '0' && matches!(chars.get(j), Some('x' | 'X' | 'o' | 'O' | 'b' | 'B')) {
                j += 1;
            }
            while j < chars.len() {
                let d = chars[j];
                let extends = d.is_ascii_alphanumeric()
                    || d == '_'
                    || (d == '.' && chars.get(j + 1).is_some_and(char::is_ascii_digit));
                if !extends {
                    break;
                }
                j += 1;
            }
            paint(&mut overlay, i, j, styles.number);
            i = j;
            continue;
        }

        // Word run → keyword tinting.
        if c.is_alphanumeric() || c == '_' {
            let start = i;
            let mut j = i;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            if spec.has_keywords {
                let word: String = chars[start..j].iter().collect();
                if is_keyword(&word, lang) {
                    paint(&mut overlay, start, j, styles.keyword);
                }
            }
            i = j;
            continue;
        }

        i += 1;
    }

    (overlay, state)
}

/// Outcome of scanning a string body that started at `from`.
struct StringScan {
    /// One past the last char of the literal (the closing delimiter
    /// inclusive when `closed`, else `chars.len()`).
    end: usize,
    /// The closing delimiter was found on this line.
    closed: bool,
}

/// Scans a string body beginning at index `from` (i.e. just after the
/// opening delimiter) for delimiter `delim` of kind `kind`. Stops at the
/// matching close or end of line. [`StringKind::Escapable`] honours a
/// `\`-escape; [`StringKind::Raw`] does not; [`StringKind::Triple`] closes
/// only on three consecutive `delim`.
fn scan_string_body(chars: &[char], from: usize, delim: char, kind: StringKind) -> StringScan {
    let mut j = from;
    while j < chars.len() {
        match kind {
            StringKind::Escapable => {
                if chars[j] == '\\' {
                    j += 2;
                    continue;
                }
                if chars[j] == delim {
                    return StringScan {
                        end: (j + 1).min(chars.len()),
                        closed: true,
                    };
                }
            }
            StringKind::Raw => {
                if chars[j] == delim {
                    return StringScan {
                        end: (j + 1).min(chars.len()),
                        closed: true,
                    };
                }
            }
            StringKind::Triple => {
                if chars[j] == delim
                    && chars.get(j + 1) == Some(&delim)
                    && chars.get(j + 2) == Some(&delim)
                {
                    return StringScan {
                        end: (j + 3).min(chars.len()),
                        closed: true,
                    };
                }
            }
        }
        j += 1;
    }
    StringScan {
        end: chars.len(),
        closed: false,
    }
}

/// Three consecutive `q` at index `i` (a Python triple-quote opener).
fn is_triple(chars: &[char], i: usize, q: char) -> bool {
    chars.get(i) == Some(&q) && chars.get(i + 1) == Some(&q) && chars.get(i + 2) == Some(&q)
}

/// Whether the char before `i` is an identifier char — used so a Rust raw
/// string prefix `r` is only treated as such at a token boundary (not the
/// `r` inside `for`).
fn prev_is_ident(chars: &[char], i: usize) -> bool {
    i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_')
}

/// Exactly `n` `#` at index `from` (the Rust raw-string closing run).
fn closing_hashes_match(chars: &[char], from: usize, n: usize) -> bool {
    (0..n).all(|k| chars.get(from + k) == Some(&'#'))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Color;

    /// Distinct, easily-identified styles so a test can read back exactly
    /// which classifier painted each char.
    fn styles() -> SyntaxStyles {
        SyntaxStyles {
            comment: Style::new().fg(Color::Green),
            string: Style::new().fg(Color::Yellow),
            number: Style::new().fg(Color::Magenta),
            keyword: Style::new().fg(Color::Blue),
        }
    }

    /// The contiguous char index ranges that carry style `s` in `ov`.
    fn spans(ov: &[Style], s: Style) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut run: Option<usize> = None;
        for (i, st) in ov.iter().enumerate() {
            match (run, *st == s) {
                (None, true) => run = Some(i),
                (Some(start), false) => {
                    out.push((start, i));
                    run = None;
                }
                _ => {}
            }
        }
        if let Some(start) = run {
            out.push((start, ov.len()));
        }
        out
    }

    fn ov(line: &str, lang: Language) -> Vec<Style> {
        line_overlay(line, lang, &styles(), LexState::default()).0
    }

    // -- from_path -------------------------------------------------------

    #[test]
    fn from_path_maps_extensions_case_insensitively() {
        assert_eq!(Language::from_path("src/main.rs"), Language::Rust);
        assert_eq!(Language::from_path("a/b/Thing.PY"), Language::Python);
        assert_eq!(Language::from_path("x.go"), Language::Go);
        assert_eq!(Language::from_path("app.JSX"), Language::JavaScript);
        assert_eq!(Language::from_path("comp.tsx"), Language::TypeScript);
        assert_eq!(Language::from_path("k.hpp"), Language::C);
        assert_eq!(Language::from_path("deploy.sh"), Language::Shell);
        assert_eq!(Language::from_path("README.md"), Language::Markdown);
        assert_eq!(Language::from_path("data.JSON"), Language::Json);
        assert_eq!(Language::from_path("C:\\proj\\m.rs"), Language::Rust);
        assert_eq!(Language::from_path("Makefile"), Language::Unknown);
        assert_eq!(Language::from_path("noext"), Language::Unknown);
        assert_eq!(Language::from_path(".gitignore"), Language::Unknown);
        assert_eq!(Language::from_path(""), Language::Unknown);
    }

    // -- byte-identical Unknown ------------------------------------------

    /// The headline guarantee: `Unknown` reproduces the legacy `diff.rs`
    /// algorithm exactly. Hand-computed spans on a representative mixed
    /// line. (Char indices, not bytes.)
    ///
    /// Line: `let x = 0xFF; // c "s\"t" 'a' 1.0 not_kw`
    ///        0123456789...
    #[test]
    fn unknown_reproduces_the_documented_algorithm_on_a_mixed_line() {
        let line = r#"let x = 0xFF; // c "s\"t" 'a' 1.0 not_kw"#;
        let o = ov(line, Language::Unknown);
        let st = styles();
        assert_eq!(o.len(), line.chars().count());

        // `let` keyword at 0..3.
        assert_eq!(spans(&o, st.keyword), vec![(0, 3)]);
        // `0xFF` number at 8..12.
        assert_eq!(spans(&o, st.number), vec![(8, 12)]);
        // Line comment from the first `/` (index 14) to end of line — the
        // comment swallows the rest, so the `"s\"t"` / `'a'` / `1.0` after
        // it are *not* separately classified (matches the legacy scanner:
        // line comment wins and breaks).
        assert_eq!(spans(&o, st.comment), vec![(14, line.chars().count())]);
        // Therefore no string / no other number was painted.
        assert_eq!(spans(&o, st.string), Vec::<(usize, usize)>::new());
    }

    /// Pre-comment string + number + keyword *are* classified when nothing
    /// shadows them — exercises the string-escape and number rules of the
    /// legacy path directly.
    #[test]
    fn unknown_classifies_string_number_keyword_before_any_comment() {
        let line = r#"return "a\"b" 42 + 0b1010_1 foo"#;
        let o = ov(line, Language::Unknown);
        let st = styles();
        // `return` 0..6.
        assert_eq!(spans(&o, st.keyword), vec![(0, 6)]);
        // `"a\"b"` 7..13 (the escaped quote does not close it).
        assert_eq!(spans(&o, st.string), vec![(7, 13)]);
        // `42` 14..16 and `0b1010_1` 19..27.
        assert_eq!(spans(&o, st.number), vec![(14, 16), (19, 27)]);
        // `foo` is not a keyword → nothing.
    }

    #[test]
    fn unknown_block_comment_is_single_line_and_ignores_state() {
        // Even with an "in block comment" state fed in, Unknown ignores it
        // and the returned state is always default.
        let prior = LexState {
            in_block_comment: true,
            in_string: None,
        };
        let (o, out) = line_overlay("code /* c */ x", Language::Unknown, &styles(), prior);
        let st = styles();
        // The leading `code ` was NOT treated as comment (state ignored);
        // only the real `/* c */` (5..12) is.
        assert_eq!(spans(&o, st.comment), vec![(5, 12)]);
        assert_eq!(out, LexState::default());
    }

    /// A lone `#` is a line comment in the legacy path (covers shell/python
    /// inside an Unknown diff row).
    #[test]
    fn unknown_lone_hash_is_a_line_comment() {
        let o = ov("x = 1 # tail", Language::Unknown);
        let st = styles();
        assert_eq!(spans(&o, st.comment), vec![(6, 12)]);
    }

    // -- Rust ------------------------------------------------------------

    #[test]
    fn rust_raw_string_and_line_comment() {
        // `let s = r#"a "b" c"#; // done`
        let line = r##"let s = r#"a "b" c"#; // done"##;
        let o = ov(line, Language::Rust);
        let st = styles();
        let n = line.chars().count();
        // `let` keyword.
        assert_eq!(spans(&o, st.keyword), vec![(0, 3)]);
        // Raw string `r#"a "b" c"#` spans 8..20 — the inner `"b"` does NOT
        // close it (raw, hash-delimited).
        assert_eq!(spans(&o, st.string), vec![(8, 20)]);
        // `// done` line comment to EOL.
        assert_eq!(spans(&o, st.comment), vec![(22, n)]);
    }

    #[test]
    fn rust_plain_string_with_escape_and_char_literal() {
        let line = r#"let c = '\n'; let s = "x\"y";"#;
        let o = ov(line, Language::Rust);
        let st = styles();
        // Two `let` keywords.
        assert_eq!(spans(&o, st.keyword), vec![(0, 3), (14, 17)]);
        // char `'\n'` at 8..12 and string `"x\"y"` at 22..28.
        assert_eq!(spans(&o, st.string), vec![(8, 12), (22, 28)]);
    }

    #[test]
    fn rust_raw_prefix_not_triggered_inside_identifier() {
        // The `r` in `for` must not start a raw string.
        let o = ov(r#"for x in v {}"#, Language::Rust);
        let st = styles();
        // `for` and `in` are Rust keywords; nothing is a string.
        assert_eq!(spans(&o, st.string), Vec::<(usize, usize)>::new());
        assert_eq!(spans(&o, st.keyword), vec![(0, 3), (6, 8)]);
    }

    // -- Python triple string spanning lines -----------------------------

    #[test]
    fn python_triple_string_stays_string_across_three_lines() {
        let st = styles();
        let l1 = r#"x = """first"#;
        let l2 = r#"second "quoted" still"#;
        let l3 = r#"third""" + 1"#;

        let (o1, s1) = line_overlay(l1, Language::Python, &st, LexState::default());
        // Open triple from index 4 to EOL on line 1.
        assert_eq!(spans(&o1, st.string), vec![(4, l1.chars().count())]);
        assert!(s1.in_string.is_some(), "triple string must carry");

        let (o2, s2) = line_overlay(l2, Language::Python, &st, s1);
        // Entire line 2 is still the string (incl. the inner `"quoted"`).
        assert_eq!(spans(&o2, st.string), vec![(0, l2.chars().count())]);
        assert!(s2.in_string.is_some(), "still open after line 2");

        let (o3, s3) = line_overlay(l3, Language::Python, &st, s2);
        // Line 3: string up to and including the closing `"""` (0..8),
        // then the `+ 1` is normal code → `1` is a number.
        assert_eq!(spans(&o3, st.string), vec![(0, 8)]);
        assert_eq!(spans(&o3, st.number), vec![(11, 12)]);
        assert_eq!(s3.in_string, None, "closed on line 3");
        assert_eq!(s3, LexState::default());
    }

    #[test]
    fn python_hash_comment_and_keywords() {
        let o = ov("def f(): return None  # note", Language::Python);
        let st = styles();
        // `def` 0..3, `return` 9..15, `None` 16..20 are Python keywords.
        assert_eq!(spans(&o, st.keyword), vec![(0, 3), (9, 15), (16, 20)]);
        // `# note` comment 22..28.
        assert_eq!(spans(&o, st.comment), vec![(22, 28)]);
    }

    // -- C-family multi-line block comment -------------------------------

    #[test]
    fn c_block_comment_stays_comment_across_lines() {
        let st = styles();
        let l1 = "int a; /* multi";
        let l2 = "still comment 123";
        let l3 = "end */ int b;";

        let (o1, s1) = line_overlay(l1, Language::C, &st, LexState::default());
        // `int` keyword 0..3; block comment from 7 to EOL.
        assert_eq!(spans(&o1, st.keyword), vec![(0, 3)]);
        assert_eq!(spans(&o1, st.comment), vec![(7, l1.chars().count())]);
        assert!(s1.in_block_comment, "block comment must carry");

        let (o2, s2) = line_overlay(l2, Language::C, &st, s1);
        // The whole line is comment — the `123` is NOT a number.
        assert_eq!(spans(&o2, st.comment), vec![(0, l2.chars().count())]);
        assert_eq!(spans(&o2, st.number), Vec::<(usize, usize)>::new());
        assert!(s2.in_block_comment, "still in comment");

        let (o3, s3) = line_overlay(l3, Language::C, &st, s2);
        // `end */` (0..6) closes the comment; then ` int b;` → `int` kw.
        assert_eq!(spans(&o3, st.comment), vec![(0, 6)]);
        assert_eq!(spans(&o3, st.keyword), vec![(7, 10)]);
        assert!(!s3.in_block_comment, "closed");
        assert_eq!(s3, LexState::default());
    }

    // -- keyword sets differ per language --------------------------------

    #[test]
    fn keyword_sets_are_language_specific() {
        let st = styles();
        // `func` is a Go keyword but not Python's.
        assert_eq!(
            spans(&ov("func main()", Language::Go), st.keyword),
            vec![(0, 4)]
        );
        assert_eq!(
            spans(&ov("func main()", Language::Python), st.keyword),
            Vec::<(usize, usize)>::new()
        );
        // `def` is a Python keyword but not Go's.
        assert_eq!(
            spans(&ov("def main()", Language::Python), st.keyword),
            vec![(0, 3)]
        );
        assert_eq!(
            spans(&ov("def main()", Language::Go), st.keyword),
            Vec::<(usize, usize)>::new()
        );
        // `func`/`def` are both in the Unknown common core.
        assert_eq!(
            spans(&ov("func x", Language::Unknown), st.keyword),
            vec![(0, 4)]
        );
        assert_eq!(
            spans(&ov("def x", Language::Unknown), st.keyword),
            vec![(0, 3)]
        );
    }

    #[test]
    fn go_raw_backtick_string_can_span_lines() {
        let st = styles();
        let l1 = "s := `line one";
        let l2 = "line two` + x";
        let (o1, s1) = line_overlay(l1, Language::Go, &st, LexState::default());
        // Backtick raw string from 5 to EOL; carries.
        assert_eq!(spans(&o1, st.string), vec![(5, l1.chars().count())]);
        assert!(s1.in_string.is_some());
        let (o2, s2) = line_overlay(l2, Language::Go, &st, s1);
        // Closes at the backtick (0..9 inclusive of `` ` ``).
        assert_eq!(spans(&o2, st.string), vec![(0, 9)]);
        assert_eq!(s2, LexState::default());
    }

    #[test]
    fn shell_only_double_and_single_quotes_and_hash_comment() {
        let st = styles();
        let o = ov(r#"echo "hi" # rest"#, Language::Shell);
        assert_eq!(spans(&o, st.string), vec![(5, 9)]);
        assert_eq!(spans(&o, st.comment), vec![(10, 16)]);
        // Shell has no numeric literals (a bare `1` is just a word).
        let o2 = ov("x=1", Language::Shell);
        assert_eq!(spans(&o2, st.number), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn json_strings_and_numbers_only_no_keywords() {
        let st = styles();
        let o = ov(r#"{"k": 12.5, "v": true}"#, Language::Json);
        // `"k"` 1..4 and `"v"` 12..15 strings; `12.5` 6..10 number.
        assert_eq!(spans(&o, st.string), vec![(1, 4), (12, 15)]);
        assert_eq!(spans(&o, st.number), vec![(6, 10)]);
        // `true` is NOT tinted as a keyword in JSON.
        assert_eq!(spans(&o, st.keyword), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn markdown_has_no_code_tokens() {
        let o = ov(
            r#"# Heading with `code` and 12 and "q""#,
            Language::Markdown,
        );
        // No comment / string / number / keyword anywhere (prose).
        assert!(o.iter().all(|s| *s == Style::new()));
    }

    // -- determinism / totality (fixed-seed LCG, no rand dep) ------------

    /// Deterministic LCG over random text × every [`Language`], threading
    /// [`LexState`] across a random number of lines: `line_overlay` never
    /// panics and the overlay length always equals the char count. The LCG
    /// shape (seed + constants) is copied from
    /// `rstui_core::text_area`'s `mod tests`.
    #[test]
    fn fuzz_total_and_overlay_len_equals_char_count_all_languages() {
        let mut state: u64 = 0x0bad_f00d_dead_beef;
        let mut rng = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };

        // A char alphabet rich in delimiters / escapes / multi-byte so the
        // scanner's every branch is exercised.
        let alphabet: &[char] = &[
            'a', 'z', '_', '0', '9', '.', ' ', '/', '*', '#', '-', '"', '\'', '`', '\\', 'r', '{',
            '}', 'é', '日', '😀', 'f', 'n', 'x',
        ];
        let langs = [
            Language::Unknown,
            Language::Rust,
            Language::Python,
            Language::Go,
            Language::JavaScript,
            Language::TypeScript,
            Language::C,
            Language::Shell,
            Language::Markdown,
            Language::Json,
        ];
        let st = styles();

        for lang in langs {
            for _ in 0..400 {
                // 1..=6 contiguous lines, threading LexState.
                let line_count = (rng() % 6) as usize + 1;
                let mut carried = LexState::default();
                for _ in 0..line_count {
                    let len = (rng() % 40) as usize;
                    let line: String = (0..len)
                        .map(|_| alphabet[(rng() % alphabet.len() as u64) as usize])
                        .collect();
                    let (overlay, next) = line_overlay(&line, lang, &st, carried);
                    // Totality invariant: length == char count, always.
                    assert_eq!(
                        overlay.len(),
                        line.chars().count(),
                        "overlay len must equal char count (lang={lang:?}, line={line:?})"
                    );
                    // Unknown must never carry state out.
                    if lang == Language::Unknown {
                        assert_eq!(
                            next,
                            LexState::default(),
                            "Unknown must never carry lexer state"
                        );
                    }
                    carried = next;
                }
            }
        }
    }

    /// Every per-language keyword table must be sorted (the binary search
    /// depends on it) and free of duplicates.
    #[test]
    fn keyword_tables_are_sorted_and_unique() {
        for set in [
            COMMON_KEYWORDS,
            RUST_KEYWORDS,
            PYTHON_KEYWORDS,
            GO_KEYWORDS,
            JS_KEYWORDS,
            C_KEYWORDS,
            SHELL_KEYWORDS,
        ] {
            for w in set.windows(2) {
                assert!(w[0] < w[1], "keyword table not strictly sorted at {w:?}");
            }
        }
    }

    /// The Unknown common-core keyword set must remain *exactly* the legacy
    /// `diff.rs` `KEYWORDS` list — the byte-identical contract. Pinned here
    /// (sorted, 110 entries) so any accidental edit fails immediately.
    #[test]
    fn unknown_common_keyword_set_is_the_legacy_diff_set() {
        const LEGACY: &[&str] = &[
            "abstract",
            "and",
            "as",
            "async",
            "await",
            "begin",
            "bool",
            "break",
            "byte",
            "case",
            "catch",
            "char",
            "class",
            "const",
            "continue",
            "data",
            "def",
            "default",
            "defer",
            "del",
            "do",
            "double",
            "elif",
            "else",
            "end",
            "enum",
            "except",
            "export",
            "extends",
            "extern",
            "false",
            "final",
            "finally",
            "float",
            "fn",
            "for",
            "from",
            "func",
            "function",
            "go",
            "goto",
            "if",
            "impl",
            "implements",
            "import",
            "in",
            "instanceof",
            "int",
            "interface",
            "is",
            "lambda",
            "let",
            "long",
            "loop",
            "match",
            "mod",
            "module",
            "move",
            "mut",
            "namespace",
            "new",
            "nil",
            "none",
            "not",
            "null",
            "object",
            "or",
            "package",
            "pass",
            "private",
            "protected",
            "pub",
            "public",
            "raise",
            "ref",
            "return",
            "select",
            "self",
            "short",
            "signed",
            "sizeof",
            "static",
            "str",
            "struct",
            "super",
            "switch",
            "template",
            "then",
            "this",
            "throw",
            "throws",
            "trait",
            "true",
            "try",
            "type",
            "typedef",
            "typeof",
            "union",
            "unsafe",
            "unsigned",
            "use",
            "using",
            "var",
            "void",
            "where",
            "while",
            "with",
            "yield",
        ];
        assert_eq!(COMMON_KEYWORDS, LEGACY);
    }
}
