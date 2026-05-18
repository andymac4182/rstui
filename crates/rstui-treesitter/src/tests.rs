//! Per-language correctness + totality tests for the Tier-1 analyzer.
//!
//! Every test is `cfg`-gated to its language feature so the suite tracks
//! exactly the enabled grammars (all on by default). The properties
//! asserted, per the ADR 0022 contract:
//!
//! - `highlight()` length is **always** `src.chars().count()` (the
//!   flattened, newline-inclusive drop-in layout `Editor::syntax` expects);
//! - representative keyword / string / comment / number spans land in the
//!   right [`SyntaxStyles`] bucket (proving a *real* parse colours them);
//! - `outline()` finds the expected symbols with the right
//!   [`SymbolKind`](rstui_widgets::SymbolKind) / `line` / nesting;
//! - [`TsLanguage::from_path`] resolves by extension;
//! - a fixed-seed-LCG fuzz over garbage source for **every** enabled
//!   language never panics and the overlay-length invariant always holds
//!   (the LCG shape is copied from `rstui-core`'s `text_area.rs` tests —
//!   no `rand`/`proptest` dependency).

#![allow(unused_imports)]

use super::*;
use rstui_core::{Color, Style};
use rstui_widgets::SymbolKind;
use rstui_widgets::syntax::SyntaxStyles;

/// Distinct, easily-identified bucket styles so a test can read back which
/// classifier painted each char (the same trick the Tier-0 `syntax.rs`
/// tests use).
fn styles() -> SyntaxStyles {
    SyntaxStyles {
        comment: Style::new().fg(Color::Green),
        string: Style::new().fg(Color::Yellow),
        number: Style::new().fg(Color::Magenta),
        keyword: Style::new().fg(Color::Blue),
    }
}

/// How many overlay slots carry exactly bucket style `s`.
fn count(ov: &[Style], s: Style) -> usize {
    ov.iter().filter(|x| **x == s).count()
}

/// Build an analyzer, set the source, return `(overlay, outline)`.
fn analyze(lang: TsLanguage, src: &str) -> (Vec<Style>, rstui_widgets::Outline) {
    let mut a = Analyzer::new(lang);
    a.set_source(src);
    (a.highlight(&styles()), a.outline())
}

/// Assert the headline drop-in invariant: one `Style` slot per source char,
/// newlines included (so it is byte-compatible with `Editor::syntax`).
fn assert_overlay_len(ov: &[Style], src: &str) {
    assert_eq!(
        ov.len(),
        src.chars().count(),
        "overlay length must equal the flattened char count (newlines included)"
    );
}

fn has(o: &rstui_widgets::Outline, name: &str, kind: SymbolKind) -> bool {
    o.0.iter().any(|s| s.name == name && s.kind == kind)
}

/// Every `TsLanguage` variant that the enabled features provide. The
/// per-feature conditional pushes cannot be a `vec![]` literal, so the
/// `vec_init_then_push` lint is allowed at the *function* level (a
/// statement-level allow does not cover the lint's whole-block span on this
/// clippy version).
#[allow(clippy::vec_init_then_push, unused_mut)]
fn enabled_langs() -> Vec<TsLanguage> {
    let mut v = Vec::with_capacity(8);
    #[cfg(feature = "rust")]
    v.push(TsLanguage::Rust);
    #[cfg(feature = "python")]
    v.push(TsLanguage::Python);
    #[cfg(feature = "javascript")]
    v.push(TsLanguage::JavaScript);
    #[cfg(feature = "typescript")]
    v.push(TsLanguage::TypeScript);
    #[cfg(feature = "go")]
    v.push(TsLanguage::Go);
    #[cfg(feature = "c")]
    v.push(TsLanguage::C);
    #[cfg(feature = "json")]
    v.push(TsLanguage::Json);
    #[cfg(feature = "markdown")]
    v.push(TsLanguage::Markdown);
    v
}

// ---------------------------------------------------------------------------
// from_path
// ---------------------------------------------------------------------------

#[test]
fn from_path_resolves_by_extension_case_insensitively() {
    #[cfg(feature = "rust")]
    assert_eq!(TsLanguage::from_path("src/Main.RS"), Some(TsLanguage::Rust));
    #[cfg(feature = "python")]
    assert_eq!(TsLanguage::from_path("a/b/x.py"), Some(TsLanguage::Python));
    #[cfg(feature = "javascript")]
    assert_eq!(
        TsLanguage::from_path("app.JSX"),
        Some(TsLanguage::JavaScript)
    );
    #[cfg(feature = "typescript")]
    assert_eq!(TsLanguage::from_path("c.tsx"), Some(TsLanguage::TypeScript));
    #[cfg(feature = "go")]
    assert_eq!(TsLanguage::from_path("m.go"), Some(TsLanguage::Go));
    #[cfg(feature = "c")]
    assert_eq!(TsLanguage::from_path("k.hpp"), Some(TsLanguage::C));
    #[cfg(feature = "json")]
    assert_eq!(TsLanguage::from_path("d.JSON"), Some(TsLanguage::Json));
    #[cfg(feature = "markdown")]
    assert_eq!(
        TsLanguage::from_path("README.md"),
        Some(TsLanguage::Markdown)
    );
    // Unrecognised / extension-less / dotfile / empty ⇒ None (fall back to
    // Tier-0).
    assert_eq!(TsLanguage::from_path("Makefile"), None);
    assert_eq!(TsLanguage::from_path("noext"), None);
    assert_eq!(TsLanguage::from_path(".gitignore"), None);
    assert_eq!(TsLanguage::from_path(""), None);
    assert_eq!(TsLanguage::from_path("a.unknownext"), None);
}

// ---------------------------------------------------------------------------
// Per-language: highlight buckets + outline symbols
// ---------------------------------------------------------------------------

#[cfg(feature = "rust")]
#[test]
fn rust_highlight_buckets_and_outline() {
    let src = "\
// a line comment
pub mod parser {
    pub struct Lexer {
        pos: usize,
    }
    pub enum Tok { A, B }
    impl Lexer {
        pub fn next(&mut self) -> u32 {
            let s = \"hello\";
            42
        }
    }
}
fn main() {}
";
    let (ov, o) = analyze(TsLanguage::Rust, src);
    assert_overlay_len(&ov, src);
    let st = styles();
    // A real parse colours each bucket.
    assert!(count(&ov, st.comment) >= 15, "line comment coloured");
    assert!(count(&ov, st.keyword) >= 6, "fn/let/pub/struct… coloured");
    assert!(count(&ov, st.string) >= 5, "\"hello\" coloured");
    assert!(count(&ov, st.number) >= 2, "42 coloured");

    // Outline: Tier-0-equivalent shapes incl. the Rust-impl quirk → `Impl`.
    assert!(has(&o, "parser", SymbolKind::Module));
    assert!(has(&o, "Lexer", SymbolKind::Struct));
    assert!(has(&o, "Tok", SymbolKind::Enum));
    assert!(has(&o, "Lexer", SymbolKind::Impl));
    assert!(has(&o, "main", SymbolKind::Function));
    // `next` is a method inside the `impl` → nested depth >= 1.
    let next = o.0.iter().find(|s| s.name == "next").expect("next found");
    assert_eq!(next.kind, SymbolKind::Method);
    assert!(next.depth >= 1, "method nested inside impl");
    // Pre-order, non-decreasing line; line<=end_line; valid rows.
    let lines = src.split('\n').count();
    let mut prev = 0;
    for s in &o.0 {
        assert!(s.line >= prev);
        prev = s.line;
        assert!(s.line <= s.end_line && s.end_line < lines);
    }
}

#[cfg(feature = "python")]
#[test]
fn python_highlight_buckets_and_outline() {
    let src = "\
import os
# a comment
class Greeter:
    def __init__(self):
        self.k = 1
    async def greet(self):
        return \"hi\"
def free():
    return 3.14
";
    let (ov, o) = analyze(TsLanguage::Python, src);
    assert_overlay_len(&ov, src);
    let st = styles();
    assert!(count(&ov, st.comment) >= 10, "# comment coloured");
    assert!(count(&ov, st.keyword) >= 5, "class/def/return coloured");
    assert!(count(&ov, st.string) >= 4, "\"hi\" coloured");
    assert!(count(&ov, st.number) >= 2, "1 / 3.14 coloured");

    assert!(has(&o, "Greeter", SymbolKind::Class));
    assert!(has(&o, "free", SymbolKind::Function));
    let init = o.0.iter().find(|s| s.name == "__init__").unwrap();
    assert_eq!(init.kind, SymbolKind::Method);
    assert!(init.depth >= 1, "method nested in class");
    let greet = o.0.iter().find(|s| s.name == "greet").unwrap();
    assert_eq!(greet.kind, SymbolKind::Method);
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_highlight_buckets_and_outline() {
    let src = "\
// header
class Widget {
    render() { return 1; }
}
function plain(a) { return \"s\"; }
const add = (a, b) => a + b;
";
    let (ov, o) = analyze(TsLanguage::JavaScript, src);
    assert_overlay_len(&ov, src);
    let st = styles();
    assert!(count(&ov, st.comment) >= 7, "// comment coloured");
    assert!(count(&ov, st.keyword) >= 4, "class/function/return/const");
    assert!(count(&ov, st.string) >= 3, "\"s\" coloured");
    assert!(count(&ov, st.number) >= 1, "1 coloured");

    assert!(has(&o, "Widget", SymbolKind::Class));
    assert!(has(&o, "plain", SymbolKind::Function));
    assert!(has(&o, "add", SymbolKind::Function));
    let render = o.0.iter().find(|s| s.name == "render").unwrap();
    assert_eq!(render.kind, SymbolKind::Method);
    assert!(render.depth >= 1, "method nested in class");
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_highlight_buckets_and_outline() {
    // The TS highlight + tags queries are composed JS-base + TS-additions,
    // so a concrete class/function AND a TS interface both resolve.
    let src = "\
// c
interface Shape {
    area(): number;
}
class Widget {
    render(): string { return \"x\"; }
}
function plain(a: number) { return a + 1; }
";
    let (ov, o) = analyze(TsLanguage::TypeScript, src);
    assert_overlay_len(&ov, src);
    let st = styles();
    assert!(count(&ov, st.comment) >= 3, "// comment coloured");
    assert!(
        count(&ov, st.keyword) >= 4,
        "interface/class/function/return"
    );
    assert!(count(&ov, st.string) >= 3, "\"x\" coloured");
    assert!(count(&ov, st.number) >= 1, "1 coloured");

    assert!(has(&o, "Shape", SymbolKind::Trait));
    assert!(has(&o, "Widget", SymbolKind::Class));
    assert!(has(&o, "plain", SymbolKind::Function));
    let render = o.0.iter().find(|s| s.name == "render").unwrap();
    assert_eq!(render.kind, SymbolKind::Method);
}

#[cfg(feature = "go")]
#[test]
fn go_highlight_buckets_and_outline() {
    let src = "\
package main
// a comment
type Shape interface {
\tArea() float64
}
type Rect struct {
\tw int
}
func (r Rect) Area() float64 { return 3 }
func main() { s := \"hi\"; _ = s }
";
    let (ov, o) = analyze(TsLanguage::Go, src);
    assert_overlay_len(&ov, src);
    let st = styles();
    assert!(count(&ov, st.comment) >= 10, "// comment coloured");
    assert!(count(&ov, st.keyword) >= 4, "package/type/func/return");
    assert!(count(&ov, st.string) >= 3, "\"hi\" coloured");
    assert!(count(&ov, st.number) >= 1, "3 coloured");

    assert!(has(&o, "Shape", SymbolKind::Trait));
    assert!(has(&o, "Rect", SymbolKind::Struct));
    assert!(has(&o, "Area", SymbolKind::Method));
    assert!(has(&o, "main", SymbolKind::Function));
}

#[cfg(feature = "c")]
#[test]
fn c_highlight_buckets_and_outline() {
    let src = "\
#include <stdio.h>
// a comment
struct Point { int x; };
enum Color { RED };
static int add(int a, int b) { return a + b; }
int main(void) { char *s = \"hi\"; return 0; }
";
    let (ov, o) = analyze(TsLanguage::C, src);
    assert_overlay_len(&ov, src);
    let st = styles();
    assert!(count(&ov, st.comment) >= 10, "// comment coloured");
    assert!(count(&ov, st.keyword) >= 4, "struct/enum/static/int/return");
    assert!(count(&ov, st.string) >= 3, "\"hi\" coloured");
    assert!(count(&ov, st.number) >= 1, "0 coloured");

    assert!(has(&o, "Point", SymbolKind::Struct));
    assert!(has(&o, "Color", SymbolKind::Enum));
    assert!(has(&o, "add", SymbolKind::Function));
    assert!(has(&o, "main", SymbolKind::Function));
}

#[cfg(feature = "json")]
#[test]
fn json_highlight_buckets_and_empty_outline() {
    let src = "{\n  \"k\": 12.5,\n  \"v\": true,\n  \"s\": \"text\"\n}\n";
    let (ov, o) = analyze(TsLanguage::Json, src);
    assert_overlay_len(&ov, src);
    let st = styles();
    assert!(count(&ov, st.string) >= 4, "JSON strings coloured");
    assert!(count(&ov, st.number) >= 3, "12.5 coloured (number bucket)");
    // No comments / keywords in JSON.
    assert_eq!(count(&ov, st.comment), 0);
    // tree-sitter-json ships no tags.scm ⇒ empty outline.
    assert!(o.0.is_empty(), "JSON has no tags query → empty outline");
}

#[cfg(feature = "markdown")]
#[test]
fn markdown_overlay_len_and_empty_outline() {
    let src = "# Title\n\nsome *text* and `code`\n\n## Section\n\nmore\n";
    let (ov, o) = analyze(TsLanguage::Markdown, src);
    // The headline drop-in invariant must hold even for prose.
    assert_overlay_len(&ov, src);
    // tree-sitter-md ships no tags.scm ⇒ empty outline.
    assert!(o.0.is_empty(), "Markdown has no tags query → empty outline");
}

// ---------------------------------------------------------------------------
// set_source can be called repeatedly (caller-owned, re-parse-on-edit)
// ---------------------------------------------------------------------------

#[cfg(feature = "rust")]
#[test]
fn set_source_reparse_tracks_edits() {
    let mut a = Analyzer::new(TsLanguage::Rust);
    a.set_source("fn a() {}\n");
    assert!(a.outline().0.iter().any(|s| s.name == "a"));
    // A later edit re-parses; the outline reflects the new source.
    a.set_source("fn b() {}\nfn c() {}\n");
    let o = a.outline();
    assert!(o.0.iter().any(|s| s.name == "b"));
    assert!(o.0.iter().any(|s| s.name == "c"));
    assert!(!o.0.iter().any(|s| s.name == "a"));
    // Overlay still the right length for the new source.
    let src = "fn b() {}\nfn c() {}\n";
    assert_overlay_len(&a.highlight(&styles()), src);
}

#[cfg(feature = "rust")]
#[test]
fn before_set_source_outputs_are_safe_empty() {
    let a = Analyzer::new(TsLanguage::Rust);
    assert!(a.highlight(&styles()).is_empty());
    assert!(a.outline().0.is_empty());
}

// ---------------------------------------------------------------------------
// Totality — fixed-seed LCG, every enabled language, no rand/proptest
// ---------------------------------------------------------------------------

/// A deterministic LCG fuzz: thousands of random byte/line soups (a
/// code-flavoured alphabet + raw control / multi-byte UTF-8) over **every**
/// enabled language must never panic, and the overlay-length invariant
/// (`highlight().len() == src.chars().count()`) and the outline
/// well-formedness invariants must always hold. The LCG seed/constants are
/// the exact ones `rstui-core`'s `text_area.rs` tests use — zero deps.
#[test]
fn fuzz_total_and_invariants_all_enabled_languages() {
    let mut state: u64 = 0x0bad_f00d_dead_beef;
    let mut rng = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        state
    };

    // Code-flavoured tokens + raw bytes so every parser branch is hit.
    let toks: &[&str] = &[
        "fn ",
        "pub ",
        "mod ",
        "struct ",
        "enum ",
        "impl ",
        "trait ",
        "def ",
        "class ",
        "async ",
        "func ",
        "type ",
        "interface ",
        "const ",
        "static ",
        "let ",
        "var ",
        "function ",
        "return ",
        "package ",
        "import ",
        "name",
        "Foo",
        "x",
        "(",
        ")",
        "{",
        "}",
        "<",
        ">",
        ";",
        "=",
        "=>",
        "//",
        "/*",
        "*/",
        "\"q\"",
        "'c'",
        "`t`",
        "#",
        "##",
        "@deco",
        "#[attr]",
        " for ",
        ":",
        "[",
        "]",
        ".",
        "1",
        "0x1F",
        "3.14",
        "true",
        "null",
        "\\",
        "  ",
        "\t",
    ];

    // Only the enabled languages exist as variants (see [`enabled_langs`]).
    let langs = enabled_langs();
    let st = styles();

    for lang in langs {
        // One analyzer, many re-`set_source` (the caller-owned edit loop).
        let mut a = Analyzer::new(lang);
        for _ in 0..150 {
            let line_count = (rng() % 30) as usize;
            let mut s = String::new();
            for _ in 0..line_count {
                let pieces = (rng() % 8) as usize;
                for _ in 0..pieces {
                    if rng() % 16 == 0 {
                        let cands = ['\u{0}', 'é', '日', '\t', '😀', '\\', '"'];
                        s.push(cands[(rng() % cands.len() as u64) as usize]);
                    } else {
                        s.push_str(toks[(rng() % toks.len() as u64) as usize]);
                    }
                }
                s.push('\n');
            }

            a.set_source(&s); // Invariant: never panics.

            // Drop-in length invariant: exactly one slot per source char,
            // newlines included — what `Editor::syntax` requires.
            let ov = a.highlight(&st);
            assert_eq!(
                ov.len(),
                s.chars().count(),
                "lang {lang:?}: overlay len must equal char count\nsrc={s:?}"
            );

            // Outline well-formedness (the Tier-0 `Outline` contract).
            let o = a.outline();
            let total = s.split('\n').count();
            let mut prev = 0usize;
            for (i, sym) in o.0.iter().enumerate() {
                if i > 0 {
                    assert!(
                        sym.line >= prev,
                        "lang {lang:?}: line order regressed: {} < {prev}",
                        sym.line
                    );
                }
                prev = sym.line;
                assert!(
                    sym.line <= sym.end_line,
                    "lang {lang:?}: line {} > end_line {}",
                    sym.line,
                    sym.end_line
                );
                assert!(
                    sym.end_line < total,
                    "lang {lang:?}: end_line {} >= total {total}",
                    sym.end_line
                );
                assert!(sym.depth < 64, "lang {lang:?}: depth {} insane", sym.depth);
            }
        }

        // Pathological one-shot inputs must also be total.
        for bad in [
            "",
            "\n",
            "   ",
            "\u{0}",
            "{{{{{{",
            "\"unterminated",
            "/*",
            "###",
            "😀😀",
        ] {
            a.set_source(bad);
            let ov = a.highlight(&st);
            assert_eq!(ov.len(), bad.chars().count(), "lang {lang:?}: bad={bad:?}");
            let _ = a.outline(); // must not panic
        }
    }
}
