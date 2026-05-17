//! The [ACP registry][reg] loader and the agent-picker model.
//!
//! [reg]: https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json
//!
//! The registry is a JSON catalogue of installable ACP agents. Each agent
//! declares a `distribution` — either an `npx` package or a per-platform
//! prebuilt `binary` — which this module resolves into a concrete shell
//! command the [`acp`](crate::acp) driver can spawn.
//!
//! Fetching shells out to `curl` (the exact tool the project brief uses for
//! the registry) so the client adds **no** HTTP/TLS dependency. Offline or
//! curl-less environments degrade gracefully to a small built-in catalogue
//! plus the always-available manual `--agent <cmd>` path: the TUI is never
//! blocked on the network.

use serde::Deserialize;

/// The canonical registry URL.
pub const REGISTRY_URL: &str =
    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

/// One installable agent, resolved to a launch command for this platform.
#[derive(Debug, Clone)]
pub struct Agent {
    /// Stable registry id (e.g. `"gemini"`).
    pub id: String,
    /// Human display name.
    pub name: String,
    /// One-line description shown in the picker.
    pub description: String,
    /// The resolved shell command to spawn this agent over stdio, or `None`
    /// when no distribution matches the current platform.
    pub command: Option<String>,
}

/// The parsed, platform-resolved registry.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    /// Agents in registry order.
    pub agents: Vec<Agent>,
    /// `true` when these are the built-in fallback entries (network/curl
    /// unavailable), surfaced in the UI so the user knows the list is partial.
    pub offline: bool,
}

// ---- Raw deserialization shapes (a tolerant subset of the schema) ----

#[derive(Deserialize)]
struct RawRegistry {
    #[serde(default)]
    agents: Vec<RawAgent>,
}

#[derive(Deserialize)]
struct RawAgent {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    distribution: Option<RawDistribution>,
}

#[derive(Deserialize)]
struct RawDistribution {
    #[serde(default)]
    npx: Option<RawNpx>,
    #[serde(default)]
    binary: Option<std::collections::BTreeMap<String, RawBinary>>,
}

#[derive(Deserialize)]
struct RawNpx {
    package: String,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Deserialize)]
struct RawBinary {
    cmd: String,
}

/// The `{os}-{arch}` distribution key for the running platform, matching the
/// registry's convention (`darwin-aarch64`, `linux-x86_64`, …).
#[must_use]
pub fn platform_key() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = std::env::consts::ARCH;
    format!("{os}-{arch}")
}

impl Registry {
    /// Parses registry JSON, resolving each agent's distribution to a launch
    /// command for [`platform_key`]. `npx` packages become
    /// `npx -y <package> <args…>`; binaries are surfaced by their `cmd` (the
    /// archive must already be installed on `PATH`).
    ///
    /// # Errors
    ///
    /// Returns the `serde_json` message if the payload is not the expected
    /// registry shape.
    pub fn parse(json: &str) -> Result<Self, String> {
        let raw: RawRegistry = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let key = platform_key();
        let agents = raw
            .agents
            .into_iter()
            .map(|a| {
                let command = a.distribution.and_then(|d| {
                    if let Some(npx) = d.npx {
                        let mut cmd = format!("npx -y {}", npx.package);
                        for arg in npx.args {
                            cmd.push(' ');
                            cmd.push_str(&arg);
                        }
                        Some(cmd)
                    } else {
                        d.binary.and_then(|mut b| b.remove(&key).map(|bin| bin.cmd))
                    }
                });
                let name = if a.name.is_empty() {
                    a.id.clone()
                } else {
                    a.name
                };
                Agent {
                    id: a.id,
                    name,
                    description: a.description,
                    command,
                }
            })
            .collect();
        Ok(Self {
            agents,
            offline: false,
        })
    }

    /// Fetches and parses the live registry via a blocking `curl`.
    ///
    /// Called only inside a `Cmd::perform` closure, which the async runtime
    /// runs on `spawn_blocking` — so a blocking subprocess is correct here and
    /// adds no HTTP/TLS dependency. Any failure — no `curl`, offline, non-zero
    /// exit, bad JSON — collapses to [`Registry::offline_fallback`] so the
    /// picker always has content.
    #[must_use]
    pub fn fetch_blocking() -> Self {
        Self::try_fetch_blocking().unwrap_or_else(|_| Self::offline_fallback())
    }

    fn try_fetch_blocking() -> Result<Self, String> {
        let out = std::process::Command::new("curl")
            .args(["-sSL", "--max-time", "20", REGISTRY_URL])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("curl exited with {}", out.status));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        Self::parse(&text)
    }

    /// A minimal built-in catalogue of well-known agents, used when the live
    /// registry cannot be fetched. The commands mirror the registry's own
    /// `npx` distributions for these agents.
    #[must_use]
    pub fn offline_fallback() -> Self {
        let agent = |id: &str, name: &str, description: &str, command: &str| Agent {
            id: id.to_owned(),
            name: name.to_owned(),
            description: description.to_owned(),
            command: Some(command.to_owned()),
        };
        Self {
            agents: vec![
                agent(
                    "claude-code",
                    "Claude Code",
                    "Anthropic's Claude Code agent over ACP.",
                    "npx -y @zed-industries/claude-code-acp@latest",
                ),
                agent(
                    "codex",
                    "Codex",
                    "OpenAI Codex agent over ACP.",
                    "npx -y @zed-industries/codex-acp@latest",
                ),
                agent(
                    "gemini",
                    "Gemini CLI",
                    "Google's Gemini CLI agent over ACP.",
                    "npx -y @google/gemini-cli@latest --experimental-acp",
                ),
            ],
            offline: true,
        }
    }
}
