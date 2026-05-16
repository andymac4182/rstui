//! `cargo xtask publish-check` — the release-packaging gate.
//!
//! "The crates are *structurally* publishable" (the `release` drift guards
//! prove the version pins) is weaker than "`cargo` can actually package
//! them". This runs `cargo package` over every publishable crate **as one
//! set**, so cargo resolves the intra-workspace `path` + `version` deps
//! among the set (packaging a single member alone fails — its `rstui-*`
//! dep is not yet on crates.io). It catches the packaging-breakage class
//! *before* a release tag: a crates.io-required field gone missing
//! (`description`/`license`/`repository`), a file wrongly in/excluded, a
//! manifest that will not upload.
//!
//! Scope is deliberately `--no-verify`: the *build* of the packaged crates
//! is already `cargo xtask ci`'s job, and a verify build cannot resolve the
//! still-unpublished workspace deps anyway. So this gate is exactly
//! "does it package", not "does it build" — non-overlapping with the fast
//! loop, which is why it is a separate leg, not a `ci` gate. The real
//! end-to-end publish is still a deliberate, topologically-ordered act at
//! release time; this is the pre-tag smoke test for it.

use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

use crate::ci::cargo_bin;

/// The `[package] name` of `manifest`, if it parses (same minimal scan as
/// the naming guard uses — xtask stays dependency-free, ADR 0003 §7).
fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_package = t == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = t.strip_prefix("name") {
                if let Some(rest) = rest.trim_start().strip_prefix('=') {
                    return Some(rest.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    None
}

/// `true` unless the manifest opts out with `publish = false`.
fn is_publishable(manifest: &str) -> bool {
    !manifest
        .lines()
        .any(|l| l.trim().replace(' ', "").starts_with("publish=false"))
}

/// The names of every publishable crate under `<root>/crates`, sorted for
/// deterministic output.
fn publishable_crates(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(root.join("crates")) {
        for entry in entries.flatten() {
            let manifest = entry.path().join("Cargo.toml");
            if let Ok(text) = fs::read_to_string(&manifest) {
                if is_publishable(&text) {
                    if let Some(name) = package_name(&text) {
                        names.push(name);
                    }
                }
            }
        }
    }
    names.sort();
    names
}

/// Package every publishable crate as one set from `root`. `cargo package`
/// (not `publish --dry-run`) so it works offline and in CI without touching
/// crates.io; the set lets the members satisfy each other's path+version
/// deps.
pub(crate) fn run(root: &Path) -> ExitCode {
    let crates = publishable_crates(root);
    if crates.is_empty() {
        eprintln!("xtask publish-check: no publishable crates found under crates/");
        return ExitCode::FAILURE;
    }
    println!(
        "━━━ xtask publish-check — `cargo package` (no-verify) for {} publishable \
         crate(s): {} ━━━",
        crates.len(),
        crates.join(", ")
    );
    println!(
        "  (packaging only — building the packaged crates is `cargo xtask ci`'s \
         job; a real release publishes these in dependency order.)\n"
    );

    let cargo = cargo_bin();
    let mut cmd = Command::new(&cargo);
    cmd.current_dir(root).args(["package", "--no-verify"]);
    for name in &crates {
        cmd.args(["-p", name]);
    }
    match cmd.status() {
        Ok(s) if s.success() => {
            println!("\n✓ xtask publish-check: every publishable crate packages cleanly.");
            ExitCode::SUCCESS
        }
        Ok(_) => {
            eprintln!(
                "\n✗ xtask publish-check: a crate failed to package — fix the \
                 manifest/files reported above before a release tag."
            );
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("xtask publish-check: could not launch `{cargo}`: {err}");
            ExitCode::FAILURE
        }
    }
}
