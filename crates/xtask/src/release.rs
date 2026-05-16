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
//! These are asserted as `#[test]`s so a desync fails `cargo test` — gate 5
//! of `cargo xtask ci` and CI — rather than being caught by a human reading
//! `cargo publish` output. Parsing is std-only line scanning (xtask is
//! dependency-free by design, ADR 0003 §7): the inputs are this repo's own
//! tightly-shaped manifests/workflow, not arbitrary TOML/YAML.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// `<root>/crates/xtask` → `<root>`, resolved from the compile-time
    /// manifest dir so it is correct from any working directory.
    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("xtask is at <root>/crates/xtask")
            .to_path_buf()
    }

    /// The `value` of `key = "value"` inside the `[workspace.package]` table
    /// of the root `Cargo.toml`.
    fn workspace_package_value(root: &Path, key: &str) -> String {
        let text = fs::read_to_string(root.join("Cargo.toml")).expect("read root Cargo.toml");
        let mut in_table = false;
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with('[') {
                in_table = t == "[workspace.package]";
                continue;
            }
            if in_table {
                if let Some(rest) = t.strip_prefix(key) {
                    let rest = rest.trim_start();
                    if let Some(rest) = rest.strip_prefix('=') {
                        return rest.trim().trim_matches('"').to_string();
                    }
                }
            }
        }
        panic!("`{key}` not found in [workspace.package] of root Cargo.toml");
    }

    /// `(major, minor, patch)` of a `1`, `1.85`, or `1.85.0` version string;
    /// a missing component is `0`. Lets `rust-version = "1.85"` compare equal
    /// to a CI pin of `1.85.0`.
    fn semverish(s: &str) -> (u64, u64, u64) {
        let mut it = s.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
        (
            it.next().unwrap_or(0),
            it.next().unwrap_or(0),
            it.next().unwrap_or(0),
        )
    }

    /// The single pinned `dtolnay/rust-toolchain@<X>` ref in the CI workflow
    /// (the channel refs — `stable`/`beta`/`nightly`/`master` — are not
    /// pins). That ref is the `msrv` job's toolchain.
    fn ci_msrv_pin(root: &Path) -> String {
        let text = fs::read_to_string(root.join(".github/workflows/ci.yml"))
            .expect("read .github/workflows/ci.yml");
        let mut pins = Vec::new();
        for line in text.lines() {
            if let Some((_, after)) = line.split_once("dtolnay/rust-toolchain@") {
                let pin = after.trim();
                if !matches!(pin, "stable" | "beta" | "nightly" | "master") {
                    pins.push(pin.to_string());
                }
            }
        }
        assert_eq!(
            pins.len(),
            1,
            "expected exactly one pinned dtolnay/rust-toolchain@<version> (the \
             msrv job); found {pins:?}"
        );
        pins.into_iter().next().unwrap()
    }

    /// `true` unless the crate manifest opts out with `publish = false`.
    fn is_publishable(manifest_text: &str) -> bool {
        !manifest_text
            .lines()
            .any(|l| l.trim().replace(' ', "").starts_with("publish=false"))
    }

    /// Internal `rstui-*` path dependencies in a manifest's `[dependencies]`
    /// (and `[build-dependencies]`) — *not* `[dev-dependencies]`, which
    /// `cargo publish` strips and which may omit a version. Returns
    /// `(dep_name, version_or_none)`.
    fn internal_path_deps(manifest_text: &str) -> Vec<(String, Option<String>)> {
        let mut out = Vec::new();
        let mut in_runtime_deps = false;
        for line in manifest_text.lines() {
            let t = line.trim();
            if t.starts_with('[') {
                in_runtime_deps = t == "[dependencies]" || t == "[build-dependencies]";
                continue;
            }
            if !in_runtime_deps || t.starts_with('#') {
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

    #[test]
    fn ci_msrv_matches_workspace_rust_version() {
        let root = workspace_root();
        let declared = workspace_package_value(&root, "rust-version");
        let pinned = ci_msrv_pin(&root);
        assert_eq!(
            semverish(&declared),
            semverish(&pinned),
            "CI msrv pin (dtolnay/rust-toolchain@{pinned}) must equal \
             [workspace.package] rust-version ({declared}); bump both together"
        );
    }

    #[test]
    fn publishable_crate_internal_deps_pin_workspace_version() {
        let root = workspace_root();
        let ws_version = workspace_package_value(&root, "version");
        let crates_dir = root.join("crates");
        let mut entries: Vec<PathBuf> = fs::read_dir(&crates_dir)
            .expect("read crates/")
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();

        let mut checked = 0_u32;
        for dir in entries {
            let manifest = dir.join("Cargo.toml");
            let Ok(text) = fs::read_to_string(&manifest) else {
                continue;
            };
            if !is_publishable(&text) {
                continue;
            }
            for (dep, version) in internal_path_deps(&text) {
                let crate_name = dir.file_name().unwrap().to_string_lossy();
                let version = version.unwrap_or_else(|| {
                    panic!(
                        "publishable crate `{crate_name}` dep `{dep}` is a bare \
                         path dep (no `version`) — unpublishable; add \
                         `version = \"{ws_version}\"`"
                    )
                });
                assert_eq!(
                    version, ws_version,
                    "publishable crate `{crate_name}` dep `{dep}` version \
                     ({version}) must equal the workspace version \
                     ({ws_version})"
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "guard found no internal path deps to check — parsing likely broke"
        );
    }
}
