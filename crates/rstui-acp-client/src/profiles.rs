//! **Agent profiles** — named `(command, plugins)` recipes in a tiny
//! hand-editable config file, so a custom ACP setup is one
//! `--profile <name>` away instead of a long `--cmd … --plugin … --plugin …`.
//!
//! Format is a minimal INI (dependency-free — the workspace forbids a TOML
//! crate, ADR 0001/0003 — and parsed here the same way the keymap
//! `id = keys` file and the theme/history/sessions files are):
//!
//! ```ini
//! # ~/.config/rstui/acp-client.agents   (RSTUI_ACP_AGENTS_FILE overrides)
//! [claude]
//! command = npx -y @zed-industries/claude-code-acp@latest
//! plugin  = rstui-acp-plugin-git
//!
//! [mydev]
//! command = ./target/debug/my-acp --stdio
//! plugin  = ./plugins/my-plugin
//! ```
//!
//! `[name]` opens a profile; `command =` is its launch command (last one
//! wins) and `plugin =` adds a plugin (repeatable). `#`/`;` comments and
//! blank lines are ignored, as is anything before the first `[section]`.
//! Parsing is pure (no I/O) so it is unit-tested without touching disk;
//! [`load`] is the thin file wrapper.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// One named recipe: the ACP command plus the plugins to attach with it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentProfile {
    /// The custom local-stdio ACP command (empty ⇒ the profile names no
    /// command and is treated as absent when resolving).
    pub command: String,
    /// Plugin launch commands to attach (in file order).
    pub plugins: Vec<String>,
}

/// Where profiles are read from: `RSTUI_ACP_AGENTS_FILE` wins (the same
/// typo-safe override convention as `RSTUI_ACP_*`); otherwise
/// `$XDG_CONFIG_HOME`/`~/.config` → `rstui/acp-client.agents`. Mirrors
/// [`crate::history::history_config_path`].
#[must_use]
pub fn profiles_config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("RSTUI_ACP_AGENTS_FILE") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".rstui"));
    base.join("rstui").join("acp-client.agents")
}

/// Parses the INI-ish profiles text. Pure and total: malformed lines are
/// skipped, never fatal — a broken profiles file must never stop the
/// client from starting (the picker still opens).
#[must_use]
pub fn parse_profiles(text: &str) -> BTreeMap<String, AgentProfile> {
    let mut out: BTreeMap<String, AgentProfile> = BTreeMap::new();
    let mut current: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let name = name.trim().to_owned();
            if !name.is_empty() {
                out.entry(name.clone()).or_default();
                current = Some(name);
            }
            continue;
        }
        let Some(name) = &current else {
            continue; // a key before any [section] — ignore
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        let entry = out.entry(name.clone()).or_default();
        match key.trim() {
            "command" => entry.command = value.to_owned(),
            "plugin" if !value.is_empty() => entry.plugins.push(value.to_owned()),
            _ => {}
        }
    }
    out
}

/// Loads & parses the profiles file. Never fails: a missing/unreadable file
/// yields an empty map (the picker just opens as before).
#[must_use]
pub fn load() -> BTreeMap<String, AgentProfile> {
    std::fs::read_to_string(profiles_config_path())
        .map(|t| parse_profiles(&t))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_command_and_repeatable_plugins() {
        let txt = "
# a comment
; also a comment
ignored = before any section

[claude]
command = npx -y @zed/claude-code-acp
plugin  = rstui-acp-plugin-git
plugin  = ./local-plugin

[ mydev ]
command = ./acp --stdio
command = ./acp2 --stdio
plugin =
";
        let p = parse_profiles(txt);
        assert_eq!(p.len(), 2);
        let c = &p["claude"];
        assert_eq!(c.command, "npx -y @zed/claude-code-acp");
        assert_eq!(c.plugins, ["rstui-acp-plugin-git", "./local-plugin"]);
        let m = &p["mydev"];
        assert_eq!(m.command, "./acp2 --stdio", "last command wins");
        assert!(m.plugins.is_empty(), "empty plugin value is skipped");
    }

    #[test]
    fn empty_or_malformed_is_inert() {
        assert!(parse_profiles("").is_empty());
        assert!(parse_profiles("no sections here\ncommand = x").is_empty());
        // A section with no command still exists (resolve treats it absent).
        let p = parse_profiles("[only]\nplugin = a\n");
        assert_eq!(p["only"].command, "");
        assert_eq!(p["only"].plugins, ["a"]);
    }
}
