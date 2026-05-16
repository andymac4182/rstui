//! Release-readiness drift guards (compiled under `cfg(test)` only).
//!
//! Two facts must stay in lock-step for rstui to be a *publishable*,
//! MSRV-honest library, and "by discipline" is exactly how they silently
//! desynced before:
//!
//! 1. CI's pinned `msrv` toolchain (`.github/workflows/ci.yml`) and the
//!    declared `[workspace.package] rust-version` (root `Cargo.toml`).
//! 2. Every **publishable** crate's internal `rstui-*` path dependency
//!    carries a `version`, equal to the workspace version — a bare `path`
//!    dep makes a crate unpublishable, and a stale pinned version breaks
//!    `cargo publish` after a bump.
//!
//! Asserted as `#[test]`s so a desync fails `cargo test` — gate 5 of
//! `cargo xtask ci` and CI. The *decision* logic ([`msrv_in_sync`],
//! [`dep_version_ok`]) is pure and unit-tested with synthetic inputs, so a
//! half-done version/MSRV bump is provably *detected*, not merely
//! re-described; the live tests then apply that same logic to the real
//! repo. Parsing is std-only line scanning (xtask is dependency-free by
//! design, ADR 0003 §7): the inputs are this repo's own tightly-shaped
//! manifests/workflow, not arbitrary TOML/YAML.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    // ---- pure parsers: text in, value out ------------------------------

    /// The `value` of `key = "value"` inside `[table]` of a manifest's text.
    fn package_value(toml: &str, table: &str, key: &str) -> Option<String> {
        let header = format!("[{table}]");
        let mut in_table = false;
        for line in toml.lines() {
            let t = line.trim();
            if t.starts_with('[') {
                in_table = t == header;
                continue;
            }
            if in_table {
                if let Some(rest) = t.strip_prefix(key) {
                    if let Some(rest) = rest.trim_start().strip_prefix('=') {
                        return Some(rest.trim().trim_matches('"').to_string());
                    }
                }
            }
        }
        None
    }

    /// The single pinned `dtolnay/rust-toolchain@<X>` ref in a workflow's
    /// text (channel refs — `stable`/`beta`/`nightly`/`master` — are not
    /// pins). `Err` if not exactly one: that ambiguity is itself a desync.
    fn msrv_pin(ci_yaml: &str) -> Result<String, String> {
        let mut pins = Vec::new();
        for line in ci_yaml.lines() {
            if let Some((_, after)) = line.split_once("dtolnay/rust-toolchain@") {
                let pin = after.trim();
                if !matches!(pin, "stable" | "beta" | "nightly" | "master") {
                    pins.push(pin.to_string());
                }
            }
        }
        match pins.len() {
            1 => Ok(pins.pop().unwrap()),
            n => Err(format!(
                "expected exactly one pinned dtolnay/rust-toolchain@<version> \
                 (the msrv job); found {n}: {pins:?}"
            )),
        }
    }

    /// `(major, minor, patch)` of `1`, `1.85`, or `1.85.0`; a missing
    /// component is `0`, so `rust-version = "1.85"` compares equal to a CI
    /// pin of `1.85.0`.
    fn semverish(s: &str) -> (u64, u64, u64) {
        let mut it = s.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
        (
            it.next().unwrap_or(0),
            it.next().unwrap_or(0),
            it.next().unwrap_or(0),
        )
    }

    /// `true` unless the manifest opts out with `publish = false`.
    fn is_publishable(manifest: &str) -> bool {
        !manifest
            .lines()
            .any(|l| l.trim().replace(' ', "").starts_with("publish=false"))
    }

    /// Internal `rstui-*` path deps in `[dependencies]` /
    /// `[build-dependencies]` — *not* `[dev-dependencies]`, which
    /// `cargo publish` strips and which may omit a version. Returns
    /// `(dep_name, version_or_none)`.
    fn internal_path_deps(manifest: &str) -> Vec<(String, Option<String>)> {
        let mut out = Vec::new();
        let mut in_publishable_deps = false;
        for line in manifest.lines() {
            let t = line.trim();
            if t.starts_with('[') {
                in_publishable_deps = t == "[dependencies]" || t == "[build-dependencies]";
                continue;
            }
            if !in_publishable_deps || t.starts_with('#') {
                continue;
            }
            let Some((name, value)) = t.split_once('=') else {
                continue;
            };
            let name = name.trim();
            if !name.starts_with("rstui-") || !value.contains("path") {
                continue;
            }
            let version = value.split_once("version").and_then(|(_, after)| {
                after.trim_start().strip_prefix('=').map(|v| {
                    v.trim()
                        .trim_matches(|c| c == '"' || c == ' ' || c == '}')
                        .to_string()
                })
            });
            out.push((name.to_string(), version));
        }
        out
    }

    // ---- pure decisions: the guard logic both live and synthetic --------
    // ---- tests exercise, so the guard cannot be wrong-but-passing ------

    /// The MSRV invariant: the declared `rust-version` and the CI pin name
    /// the same Rust release (patch-insensitive).
    fn msrv_in_sync(rust_version: &str, ci_pin: &str) -> bool {
        semverish(rust_version) == semverish(ci_pin)
    }

    /// The publishability invariant for one internal dep: it must carry a
    /// `version` (a bare path dep is unpublishable) equal to the workspace
    /// version (a stale pin breaks `cargo publish` after a bump).
    fn dep_version_ok(workspace_version: &str, dep_version: Option<&str>) -> Result<(), String> {
        match dep_version {
            None => Err("bare path dependency (no `version`) — unpublishable".into()),
            Some(v) if v == workspace_version => Ok(()),
            Some(v) => Err(format!(
                "pinned `{v}` but the workspace version is `{workspace_version}`"
            )),
        }
    }

    // ---- file wrappers / workspace root --------------------------------

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("xtask is at <root>/crates/xtask")
            .to_path_buf()
    }

    fn read(root: &Path, rel: &str) -> String {
        fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
    }

    // ---- live guards: apply the pure logic to the real repo ------------

    #[test]
    fn ci_msrv_matches_workspace_rust_version() {
        let root = workspace_root();
        let declared = package_value(
            &read(&root, "Cargo.toml"),
            "workspace.package",
            "rust-version",
        )
        .expect("rust-version in [workspace.package]");
        let pinned = msrv_pin(&read(&root, ".github/workflows/ci.yml")).expect("one msrv pin");
        assert!(
            msrv_in_sync(&declared, &pinned),
            "CI msrv pin (dtolnay/rust-toolchain@{pinned}) must name the same \
             release as [workspace.package] rust-version ({declared}); bump both"
        );
    }

    #[test]
    fn publishable_crate_internal_deps_pin_workspace_version() {
        let root = workspace_root();
        let ws = package_value(&read(&root, "Cargo.toml"), "workspace.package", "version")
            .expect("version in [workspace.package]");
        let mut dirs: Vec<PathBuf> = fs::read_dir(root.join("crates"))
            .expect("read crates/")
            .flatten()
            .map(|e| e.path())
            .collect();
        dirs.sort();

        let mut checked = 0_u32;
        for dir in dirs {
            let Ok(manifest) = fs::read_to_string(dir.join("Cargo.toml")) else {
                continue;
            };
            if !is_publishable(&manifest) {
                continue;
            }
            let crate_name = dir.file_name().unwrap().to_string_lossy().into_owned();
            for (dep, version) in internal_path_deps(&manifest) {
                if let Err(why) = dep_version_ok(&ws, version.as_deref()) {
                    panic!("publishable crate `{crate_name}` dep `{dep}`: {why}");
                }
                checked += 1;
            }
        }
        assert!(checked > 0, "no internal path deps checked — parsing broke");
    }

    // ---- synthetic tests: prove the parsers + decisions are correct ----

    #[test]
    fn semverish_is_patch_insensitive_but_release_sensitive() {
        assert_eq!(semverish("1.85"), semverish("1.85.0"));
        assert_eq!(semverish("1"), semverish("1.0.0"));
        assert_ne!(semverish("1.85"), semverish("1.90"));
        assert_ne!(semverish("1.85.0"), semverish("1.85.1"));
    }

    #[test]
    fn msrv_pin_requires_exactly_one_pinned_ref() {
        let one = "uses: dtolnay/rust-toolchain@stable\nuses: dtolnay/rust-toolchain@1.90.0\n";
        assert_eq!(msrv_pin(one).unwrap(), "1.90.0");
        assert!(msrv_pin("uses: dtolnay/rust-toolchain@stable\n").is_err());
        let two = "dtolnay/rust-toolchain@1.85.0\ndtolnay/rust-toolchain@1.90.0\n";
        assert!(msrv_pin(two).is_err());
    }

    #[test]
    fn internal_path_deps_reads_version_skips_devdeps_and_flags_bare() {
        let manifest = "\
[dependencies]\n\
rstui-core = { path = \"../rstui-core\", version = \"0.0.1\" }\n\
rstui-runtime = { path = \"../rstui-runtime\" }\n\
crossterm = \"0.29\"\n\
[dev-dependencies]\n\
rstui-widgets = { path = \"../rstui-widgets\" }\n";
        let deps = internal_path_deps(manifest);
        assert_eq!(
            deps,
            vec![
                ("rstui-core".to_string(), Some("0.0.1".to_string())),
                ("rstui-runtime".to_string(), None),
            ],
            "dev-deps excluded, external skipped, bare path => None"
        );
    }

    #[test]
    fn is_publishable_honours_publish_false_any_spacing() {
        assert!(is_publishable("[package]\nname = \"x\"\n"));
        assert!(!is_publishable("[package]\npublish = false\n"));
        assert!(!is_publishable("[package]\npublish=false\n"));
    }

    #[test]
    fn package_value_reads_the_named_table_only() {
        let toml = "\
[workspace.package]\n\
version = \"0.0.1\"\n\
rust-version = \"1.85\"\n\
[package]\n\
version = \"9.9.9\"\n";
        assert_eq!(
            package_value(toml, "workspace.package", "version").as_deref(),
            Some("0.0.1")
        );
        assert_eq!(
            package_value(toml, "workspace.package", "rust-version").as_deref(),
            Some("1.85")
        );
        assert_eq!(package_value(toml, "workspace.package", "edition"), None);
    }

    /// The headline: a *half-done* `0.0.1 → 0.1.0` bump (or an MSRV that
    /// moved on only one side) is caught by the exact functions the live
    /// guards call — this is what makes the rehearsal a permanent test.
    #[test]
    fn half_done_bump_is_detected() {
        // Workspace bumped, an internal dep left behind:
        assert!(dep_version_ok("0.1.0", Some("0.0.1")).is_err());
        // A dep that was never given a version at all:
        assert!(dep_version_ok("0.1.0", None).is_err());
        // The fully-consistent state passes:
        assert!(dep_version_ok("0.1.0", Some("0.1.0")).is_ok());
        // rust-version bumped but the CI pin not (or vice-versa):
        assert!(!msrv_in_sync("1.90", "1.85.0"));
        assert!(msrv_in_sync("1.85", "1.85.0"));
    }
}
