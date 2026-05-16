//! The vague-generic-naming guardrail (iteration-19 steering note,
//! sequenced by ADR 0003 §7 as the first rstui-specific lint).
//!
//! rstui is an agent-driven codebase, so the names future machine-generated
//! slices introduce must keep describing intent. This module scans the
//! workspace for the one defect class clippy/rustdoc cannot see: crate
//! names, source-file/module paths, module declarations, and public-item
//! declarations whose name is a generic intent-hiding bucket
//! (`utils`, `helpers`, `common`, …).
//!
//! Scope is deliberately precise to keep false positives — and therefore
//! churn, which ADR 0003 treats as the cost to minimise — near zero:
//!
//! - Only **declarations** are checked: `mod` names (any visibility, since
//!   a module name hides intent regardless of who sees it) and
//!   fully-`pub` items (the steering note targets *public* APIs;
//!   `pub(crate)`/`pub(super)` internals and `let` bindings are out of
//!   scope), plus crate names and `.rs` path segments. Prose/doc text is
//!   not scanned — words like "the common case" are legitimate English and
//!   gating on them would be noise, not signal.
//! - Matching is **whole word segment**, not substring: `event_source`
//!   splits to `event`/`source`, `uncommon` stays one segment and is not
//!   `common`. This trades catching `miscellaneous` (rare) for never
//!   misfiring on a word that merely contains a banned substring (the
//!   churn trap). The banned set is intentionally tight and trivially
//!   extensible (one const).
//!
//! The convention, the scope rationale, and how to register a documented
//! exception live in `docs/conventions/naming.md`.

use std::fs;
use std::path::{Path, PathBuf};

/// Lowercased word segments banned from rstui identifiers and source
/// paths because they hide intent. The iteration-19 note enumerates
/// `helpers`/`utils`/`common`/`misc`/`stuff`/`shared`; the singular/plural
/// variants and `thing(s)` are the clearest "similarly generic" additions
/// it asks for. Kept tight on purpose (see the module docs).
pub(crate) const BANNED_SEGMENTS: &[&str] = &[
    "helper", "helpers", "util", "utils", "common", "misc", "stuff", "shared", "thing", "things",
];

/// Exact identifiers or workspace-relative paths intentionally kept
/// despite a banned segment. Empty today: the workspace is clean and the
/// `xtask` crate name is the cargo-xtask convention ADR 0003 §7 endorses,
/// which contains no banned segment so it needs no entry. Any future
/// entry must be justified in `docs/conventions/naming.md`.
pub(crate) const ALLOWED_EXCEPTIONS: &[&str] = &[];

/// Cargo-structural path components: conventions, not names that hide
/// intent, so they are skipped when checking a source path.
const STRUCTURAL_DIRS: &[&str] = &["crates", "src", "tests", "examples", "benches", "bin"];

/// Directory names never descended into during the source walk.
const SKIP_DIRS: &[&str] = &[".git", "target", ".gnhf"];

/// What kind of name a violation is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameKind {
    /// A `[package] name` in a `crates/*/Cargo.toml`.
    Crate,
    /// A `.rs` file stem or a non-structural directory component.
    SourcePath,
    /// A `mod` declaration (any visibility).
    Module,
    /// A fully-`pub` item declaration.
    PublicItem,
}

impl NameKind {
    /// A short human-readable label for the report line.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Crate => "crate name",
            Self::SourcePath => "source path",
            Self::Module => "module",
            Self::PublicItem => "public item",
        }
    }
}

/// One naming-convention violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Violation {
    /// Where the violating name lives.
    pub(crate) kind: NameKind,
    /// Workspace-relative `path` or `path:line`.
    pub(crate) location: String,
    /// The offending name as written.
    pub(crate) name: String,
    /// The banned segment it contains.
    pub(crate) banned: &'static str,
}

/// Split `ident` into lowercased word segments, breaking on `_`/`-`/`.`/
/// space and on a `camelCase`/`PascalCase` boundary (a lower/digit char
/// immediately followed by an uppercase one).
fn segments(ident: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev: Option<char> = None;
    for ch in ident.chars() {
        if matches!(ch, '_' | '-' | '.' | ' ') {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev = None;
            continue;
        }
        let camel_boundary = ch.is_ascii_uppercase()
            && matches!(prev, Some(p) if p.is_ascii_lowercase() || p.is_ascii_digit());
        if camel_boundary && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(ch.to_ascii_lowercase());
        prev = Some(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The banned segment in `name`, if any. Honours [`ALLOWED_EXCEPTIONS`]
/// (exact whole-name match).
fn offending(name: &str) -> Option<&'static str> {
    if ALLOWED_EXCEPTIONS.contains(&name) {
        return None;
    }
    for seg in segments(name) {
        if let Some(banned) = BANNED_SEGMENTS.iter().copied().find(|&b| b == seg.as_str()) {
            return Some(banned);
        }
    }
    None
}

/// Trim a `tok` down to its leading Rust identifier (handles `r#raw`,
/// `Name<T>`, `Name(`, `Name;`, `Name:` …). `None` if empty.
fn clean_ident(tok: &str) -> Option<String> {
    let tok = tok.strip_prefix("r#").unwrap_or(tok);
    let end = tok
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(tok.len());
    let id = &tok[..end];
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// Strip a leading visibility. Returns whether it was *fully* `pub`
/// (not `pub(crate)`/`pub(super)`/`pub(in …)`) and the remaining text.
fn strip_visibility(t: &str) -> (bool, &str) {
    let Some(rest) = t.strip_prefix("pub") else {
        return (false, t);
    };
    if let Some(after) = rest.strip_prefix('(') {
        // Restricted visibility — not public API. Skip past the `)`.
        return after
            .find(')')
            .map_or((false, ""), |c| (false, after[c + 1..].trim_start()));
    }
    if rest.starts_with(char::is_whitespace) {
        return (true, rest.trim_start());
    }
    // `pub` was just a prefix of a longer identifier (e.g. `pub_count`).
    (false, t)
}

/// If `line` declares a `mod` (any visibility) or a fully-`pub` item,
/// the `(kind, identifier)` it introduces.
fn classify_line(line: &str) -> Option<(NameKind, String)> {
    let t = line.trim_start();
    if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
        return None;
    }
    let (is_fully_pub, rest) = strip_visibility(t);
    let mut tokens = rest.split_whitespace();
    let first = tokens.next()?;

    if first == "mod" {
        return Some((NameKind::Module, clean_ident(tokens.next()?)?));
    }
    if !is_fully_pub {
        return None;
    }

    // Skip item modifiers to reach the item keyword. `const` is both a
    // modifier (`const fn`) and an item kind (`const NAME`), so it needs a
    // one-token lookahead.
    let mut kw = first;
    loop {
        match kw {
            "async" | "unsafe" | "default" => kw = tokens.next()?,
            "extern" => {
                kw = tokens.next()?;
                if kw.starts_with('"') {
                    kw = tokens.next()?;
                }
            }
            "const" => {
                let next = tokens.next()?;
                if next == "fn" {
                    kw = "fn";
                    break;
                }
                return Some((NameKind::PublicItem, clean_ident(next)?));
            }
            _ => break,
        }
    }
    match kw {
        "fn" | "struct" | "enum" | "trait" | "type" | "union" | "static" => {
            Some((NameKind::PublicItem, clean_ident(tokens.next()?)?))
        }
        _ => None,
    }
}

/// Workspace-relative, forward-slash form of `p` for report lines.
fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The checkable name tokens of a source path: non-structural directory
/// components and the file stem, relative to `root`.
fn path_names(root: &Path, file: &Path) -> Vec<String> {
    let Ok(relp) = file.strip_prefix(root) else {
        return Vec::new();
    };
    let comps: Vec<_> = relp.components().collect();
    let mut names = Vec::new();
    for (idx, c) in comps.iter().enumerate() {
        let std::path::Component::Normal(os) = c else {
            continue;
        };
        let s = os.to_string_lossy();
        if idx + 1 == comps.len() {
            if let Some(stem) = Path::new(s.as_ref()).file_stem() {
                names.push(stem.to_string_lossy().into_owned());
            }
        } else if !STRUCTURAL_DIRS.contains(&s.as_ref()) {
            names.push(s.into_owned());
        }
    }
    names
}

/// Recursively collect `.rs` files under `dir`, skipping VCS/build/run
/// directories and any dotfile directory.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The `[package] name` declared in `manifest`, if it parses.
fn crate_name(manifest: &Path) -> Option<String> {
    let text = fs::read_to_string(manifest).ok()?;
    let mut in_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package {
            if let Some(value) = line.strip_prefix("name") {
                if let Some(eq) = value.find('=') {
                    let v = value[eq + 1..].trim().trim_matches('"');
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Scan the workspace rooted at `root` for vague-generic-name violations.
pub(crate) fn scan(root: &Path) -> Vec<Violation> {
    let mut violations = Vec::new();

    // Crate names, in a stable order.
    if let Ok(entries) = fs::read_dir(root.join("crates")) {
        let mut crate_dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        crate_dirs.sort();
        for dir in crate_dirs {
            let manifest = dir.join("Cargo.toml");
            if let Some(name) = crate_name(&manifest) {
                if let Some(banned) = offending(&name) {
                    violations.push(Violation {
                        kind: NameKind::Crate,
                        location: rel(root, &manifest),
                        name,
                        banned,
                    });
                }
            }
        }
    }

    // Source paths + `mod`/`pub` declarations.
    let mut files = Vec::new();
    rust_files(root, &mut files);
    files.sort();
    let mut seen_paths: Vec<String> = Vec::new();
    for file in &files {
        for name in path_names(root, file) {
            if seen_paths.contains(&name) {
                continue;
            }
            if let Some(banned) = offending(&name) {
                violations.push(Violation {
                    kind: NameKind::SourcePath,
                    location: rel(root, file),
                    name: name.clone(),
                    banned,
                });
            }
            seen_paths.push(name);
        }
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if let Some((kind, name)) = classify_line(line) {
                if let Some(banned) = offending(&name) {
                    violations.push(Violation {
                        kind,
                        location: format!("{}:{}", rel(root, file), i + 1),
                        name,
                        banned,
                    });
                }
            }
        }
    }
    violations
}

/// Scan the workspace at `root`, print the human-readable report (the
/// `OK` line or the violation list with the convention pointer), and return
/// whether it is clean.
///
/// The single source of truth for the naming report's wording, shared by the
/// `lint-names` task and the `ci` task's in-process naming gate so the two
/// can never drift apart.
pub(crate) fn check_and_report(root: &Path) -> bool {
    let violations = scan(root);
    if violations.is_empty() {
        println!(
            "xtask lint-names: OK — no banned vague generic names in crate \
             names, source paths, modules, or public items."
        );
        return true;
    }
    eprintln!(
        "xtask lint-names: {} banned vague generic name(s) found.\n\
         The convention and how to register a documented exception: \
         docs/conventions/naming.md\n",
        violations.len()
    );
    for v in &violations {
        eprintln!(
            "  {} [{}] `{}` contains banned generic segment `{}`",
            v.location,
            v.kind.label(),
            v.name,
            v.banned
        );
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn segments_splits_snake_camel_and_separators() {
        assert_eq!(segments("event_source"), ["event", "source"]);
        assert_eq!(segments("EventSource"), ["event", "source"]);
        assert_eq!(segments("set_str"), ["set", "str"]);
        assert_eq!(segments("RcShared"), ["rc", "shared"]);
        assert_eq!(segments("rstui-core"), ["rstui", "core"]);
        assert_eq!(segments("with_helpers"), ["with", "helpers"]);
        // Whole-segment, not substring: these must stay single segments.
        assert_eq!(segments("uncommon"), ["uncommon"]);
        assert_eq!(segments("commonly"), ["commonly"]);
        assert_eq!(segments("something"), ["something"]);
    }

    #[test]
    fn offending_matches_whole_segments_only() {
        assert_eq!(offending("utils"), Some("utils"));
        assert_eq!(offending("Helpers"), Some("helpers"));
        assert_eq!(offending("SharedState"), Some("shared"));
        assert_eq!(offending("common_mod"), Some("common"));
        assert_eq!(offending("misc"), Some("misc"));
        // Not banned: precise names and substring near-misses.
        assert_eq!(offending("event_source"), None);
        assert_eq!(offending("set_str"), None);
        assert_eq!(offending("uncommon"), None);
        assert_eq!(offending("commonly"), None);
        assert_eq!(offending("rstui-core"), None);
        assert_eq!(offending("xtask"), None);
    }

    #[test]
    fn classify_line_detects_modules_any_visibility() {
        assert_eq!(
            classify_line("mod common {"),
            Some((NameKind::Module, "common".to_string()))
        );
        assert_eq!(
            classify_line("    pub mod widget;"),
            Some((NameKind::Module, "widget".to_string()))
        );
        assert_eq!(
            classify_line("#[cfg(test)] mod tests {"),
            None,
            "the attribute prefix is not stripped, so this is intentionally not classified"
        );
    }

    #[test]
    fn classify_line_detects_public_items_only() {
        assert_eq!(
            classify_line("pub fn helpers() {}"),
            Some((NameKind::PublicItem, "helpers".to_string()))
        );
        assert_eq!(
            classify_line("    pub struct SharedState;"),
            Some((NameKind::PublicItem, "SharedState".to_string()))
        );
        assert_eq!(
            classify_line("pub async fn shared_thing() {}"),
            Some((NameKind::PublicItem, "shared_thing".to_string()))
        );
        assert_eq!(
            classify_line("pub const MAX_UTILS: u8 = 1;"),
            Some((NameKind::PublicItem, "MAX_UTILS".to_string()))
        );
        assert_eq!(
            classify_line("pub const fn util_count() -> u8 { 0 }"),
            Some((NameKind::PublicItem, "util_count".to_string()))
        );
        assert_eq!(
            classify_line("pub fn Name<'a>(x: &'a str) {}"),
            Some((NameKind::PublicItem, "Name".to_string()))
        );
    }

    #[test]
    fn classify_line_skips_non_public_re_exports_and_comments() {
        assert_eq!(classify_line("pub(crate) fn shared_state() {}"), None);
        assert_eq!(classify_line("pub(super) struct Helpers;"), None);
        assert_eq!(classify_line("pub use crate::utils::Thing;"), None);
        assert_eq!(classify_line("fn private_helpers() {}"), None);
        assert_eq!(classify_line("// pub fn utils() {}"), None);
        assert_eq!(classify_line("/// see `pub fn helpers`"), None);
        assert_eq!(classify_line("let pub_count = 1;"), None);
    }

    fn fixture_root() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("rstui-xtask-naming-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    #[test]
    fn scan_flags_crate_path_module_and_item_but_not_internals() {
        let root = fixture_root();
        write(
            &root,
            "crates/good-crate/Cargo.toml",
            "[package]\nname = \"good-crate\"\n",
        );
        write(
            &root,
            "crates/bad-utils/Cargo.toml",
            "[package]\nname = \"bad-utils\"\n",
        );
        write(
            &root,
            "crates/good-crate/src/event_source.rs",
            "pub fn parse() {}\nmod inner;\n",
        );
        write(&root, "crates/good-crate/src/helpers.rs", "fn x() {}\n");
        write(
            &root,
            "crates/good-crate/src/decl.rs",
            "pub fn shared_state() {}\nmod common {}\npub(crate) fn util_x() {}\n// pub fn utils()\npub use a::b;\n",
        );

        let mut got: Vec<(NameKind, String, &str)> = scan(&root)
            .into_iter()
            .map(|v| (v.kind, v.name, v.banned))
            .collect();
        got.sort_by(|a, b| (a.0 as u8, &a.1).cmp(&(b.0 as u8, &b.1)));

        assert_eq!(
            got,
            vec![
                (NameKind::Crate, "bad-utils".to_string(), "utils"),
                (NameKind::SourcePath, "helpers".to_string(), "helpers"),
                (NameKind::Module, "common".to_string(), "common"),
                (NameKind::PublicItem, "shared_state".to_string(), "shared"),
            ],
            "pub(crate)/comment/re-export/private must not appear; \
             crate+path+module+pub item must"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// Structural enforcement (rstui's "structural, not aspirational"
    /// philosophy): `cargo test` itself fails if any future slice
    /// introduces a banned name, independently of the CI xtask step.
    #[test]
    fn workspace_is_free_of_banned_names() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("xtask is at <root>/crates/xtask");
        let violations = scan(root);
        assert!(
            violations.is_empty(),
            "banned vague generic names found (see docs/conventions/naming.md):\n{violations:#?}"
        );
    }
}
