//! The operator-reviewable plugin manifest and its strict, fail-closed
//! parser (ADR 0007 §2).
//!
//! A plugin ships a manifest: a line-oriented UTF-8 text file that declares
//! the plugin's identity, the host-protocol version it targets, the executable
//! to spawn, and the *scoped* capabilities it requests. The manifest is the
//! *request*; a host-side [`permission::PermissionPolicy`](crate::permission)
//! is the *grant* (default deny).
//!
//! The parser is **strict and fail-closed**: an unknown key is an error, not a
//! warning, and there is no serde/TOML dependency (ADR 0007 §2 driver 4). Its
//! grammar is specified below so the parser and the format cannot drift.
//!
//! # Manifest grammar
//!
//! Lines are UTF-8. Leading and trailing ASCII whitespace is trimmed before
//! interpretation. A blank line, or a line whose first non-whitespace character
//! is `#`, is ignored (comment). Any other line is one of:
//!
//! - A **section header**: `[section]` — switches the active scope.
//!   Allowed sections: `filesystem`, `network`, `command`, `env`.
//!   Any other name is an error. A malformed header (e.g. `[noclose`) is an
//!   error.
//! - A **key/value pair**: `key = "value"` — the key is an ASCII identifier
//!   (`[A-Za-z0-9_-]+`), then optional spaces, `=`, optional spaces, then a
//!   double-quoted value. The value is the bytes between the outer `"` delimiters.
//!   **Embedded `"` characters are not supported** (no escape sequences); a value
//!   containing `"` is a parse error. Control characters (including `\n`, `\r`,
//!   `\t`, and all `U+0000`–`U+001F`/`U+007F`) inside the value are rejected.
//! - Any non-blank, non-comment, non-section, non-key/value line is a
//!   **malformed line** — an error.
//!
//! ## Top-level keys (before any `[section]` header)
//!
//! Exactly these keys are required, each allowed at most once:
//!
//! | Key | Type | Notes |
//! |-----|------|-------|
//! | `name` | string | Non-empty plugin name. |
//! | `version` | string | Plugin version string (stored verbatim). |
//! | `api_version` | string | Host-protocol version (stored verbatim; not validated here). |
//! | `entry` | string | Path to the executable to spawn. Non-empty. |
//!
//! An unknown top-level key is an error. A duplicate top-level key is an
//! error. Any required key absent after parsing the entire file is an error.
//!
//! ## Section `[filesystem]`
//!
//! Keys: `read` or `write` (any other key is an error). The value is a path.
//!
//! Produces [`CapabilityGrant::Filesystem`] with `mode` set to
//! [`FsMode::Read`] or [`FsMode::Write`] and `root` set to the lexically
//! normalised path (via [`crate::capability::normalize_lexical`]). A path that, after
//! normalisation, still starts with `..` is rejected (an escaping root has no
//! sensible meaning in a manifest). Repeatable.
//!
//! ## Section `[network]`
//!
//! Key: `allow` only (any other key is an error). Value is `host:port`.
//! `host` must be non-empty and contain no `:`. `port` must parse as a `u16`
//! in the range `1..=65535` (port 0 is rejected). Produces
//! [`CapabilityGrant::Network`]. Repeatable.
//!
//! ## Section `[command]`
//!
//! Key: `allow` only (any other key is an error). Value is whitespace-split
//! into tokens: the first token is `program` (non-empty), remaining tokens are
//! `args_prefix` (may be empty: any args are then allowed). Produces
//! [`CapabilityGrant::Command`]. Repeatable.
//!
//! ## Section `[env]`
//!
//! Key: `allow` only (any other key is an error). Value is an environment
//! variable name: non-empty, must not contain `=` or ASCII whitespace.
//! Produces [`CapabilityGrant::Env`]. Repeatable.
//!
//! ## Section `[hooks]`
//!
//! Key: `subscribe` only (any other key is an error). Value is a hook
//! name — exactly one of `session_start`, `before_capability`,
//! `session_end` (the [`HookKind::wire_name`](crate::hook::HookKind::wire_name)s);
//! an unknown name is a hard error (fail-closed). Repeatable; duplicates
//! are de-duplicated, not errors. The host dispatches only subscribed
//! hooks, and a hook can only *narrow* authority, never widen it
//! (ADR 0007 §6 — see [`crate::hook`]).
//!
//! # Example
//!
//! ```
//! use rstui_plugin_host::manifest::PluginManifest;
//! use rstui_plugin_host::capability::{CapabilityGrant, FsMode};
//! use std::path::Path;
//!
//! let src = r#"
//! name = "my-plugin"
//! version = "1.2.3"
//! api_version = "0.1.0"
//! entry = "/usr/lib/my-plugin/bin"
//!
//! [filesystem]
//! read = "/data/input"
//! write = "/data/output"
//!
//! [network]
//! allow = "api.example.com:443"
//!
//! [command]
//! allow = "git log --oneline"
//!
//! [env]
//! allow = "HOME"
//! "#;
//!
//! let manifest = PluginManifest::parse(src).unwrap();
//! assert_eq!(manifest.name, "my-plugin");
//! assert_eq!(manifest.version, "1.2.3");
//! assert_eq!(manifest.api_version, "0.1.0");
//! assert_eq!(manifest.entry.as_os_str(), "/usr/lib/my-plugin/bin");
//! assert_eq!(manifest.grants.len(), 5);
//! assert_eq!(
//!     manifest.grants[0],
//!     CapabilityGrant::Filesystem {
//!         mode: FsMode::Read,
//!         root: Path::new("/data/input").to_path_buf(),
//!     }
//! );
//! assert_eq!(
//!     manifest.grants[1],
//!     CapabilityGrant::Filesystem {
//!         mode: FsMode::Write,
//!         root: Path::new("/data/output").to_path_buf(),
//!     }
//! );
//! assert_eq!(
//!     manifest.grants[2],
//!     CapabilityGrant::Network {
//!         host: "api.example.com".to_string(),
//!         port: 443,
//!     }
//! );
//! assert_eq!(
//!     manifest.grants[3],
//!     CapabilityGrant::Command {
//!         program: "git".to_string(),
//!         args_prefix: vec!["log".to_string(), "--oneline".to_string()],
//!     }
//! );
//! assert_eq!(
//!     manifest.grants[4],
//!     CapabilityGrant::Env { key: "HOME".to_string() }
//! );
//! ```

use std::fmt;
use std::path::{Path, PathBuf};

use crate::capability::{CapabilityGrant, FsMode, normalize_lexical};
use crate::hook::HookKind;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// An operator-reviewable declaration of a plugin's identity and the scoped
/// capabilities it requests from the host.
///
/// Produced by [`PluginManifest::parse`]; the host spawns [`entry`] and grants
/// exactly the capabilities listed in [`grants`] (subject to the host
/// [`PermissionPolicy`](crate::permission)).
///
/// [`entry`]: PluginManifest::entry
/// [`grants`]: PluginManifest::grants
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    /// The plugin's display name.
    pub name: String,
    /// The plugin's version string (stored verbatim).
    pub version: String,
    /// The host-protocol version the plugin targets (stored verbatim; the host
    /// semver-gates this before spawning).
    pub api_version: String,
    /// Path to the executable the host will spawn.
    pub entry: PathBuf,
    /// The scoped capabilities the plugin requests, in declaration order.
    pub grants: Vec<CapabilityGrant>,
    /// The hook points the plugin subscribes to (`[hooks]` section), in
    /// declaration order, de-duplicated. The host only dispatches a hook a
    /// plugin actually subscribed to (no round-trip a plugin will not
    /// answer), and a hook can only ever *narrow* authority (ADR 0007 §6 —
    /// see [`crate::hook`]).
    pub hooks: Vec<HookKind>,
}

impl PluginManifest {
    /// Parse a manifest from `text` and return a [`PluginManifest`] on
    /// success, or a [`ManifestError`] describing the first detected problem.
    ///
    /// The parser is **fail-closed**: any unrecognised key, malformed line,
    /// structural error (missing required field, duplicate field, bad section)
    /// or invalid capability value causes an error rather than a silent
    /// ignore or best-effort skip.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when `text` violates the manifest grammar.
    /// The error carries a 1-based line number wherever the problem is
    /// attributable to a specific line.
    ///
    /// # Example
    ///
    /// ```
    /// use rstui_plugin_host::manifest::PluginManifest;
    /// use rstui_plugin_host::capability::{CapabilityGrant, FsMode};
    ///
    /// let src = concat!(
    ///     "name = \"demo\"\n",
    ///     "version = \"0.1.0\"\n",
    ///     "api_version = \"1.0.0\"\n",
    ///     "entry = \"/usr/bin/demo-plugin\"\n",
    ///     "\n",
    ///     "[env]\n",
    ///     "allow = \"PATH\"\n",
    /// );
    ///
    /// let manifest = PluginManifest::parse(src).unwrap();
    /// assert_eq!(manifest.name, "demo");
    /// assert_eq!(
    ///     manifest.grants,
    ///     vec![CapabilityGrant::Env { key: "PATH".to_string() }]
    /// );
    /// ```
    pub fn parse(text: &str) -> Result<PluginManifest, ManifestError> {
        Parser::new(text).parse()
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// A precise, fail-closed parse error from [`PluginManifest::parse`].
///
/// Each variant carries the 1-based line number where the problem occurs,
/// except for [`ManifestError::MissingField`] which is detected after the
/// entire file has been processed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// A section header is missing its closing `]` (e.g. `[filesystem`).
    MalformedSection {
        /// 1-based line number.
        line: usize,
    },
    /// A section name is not one of the four allowed names.
    UnknownSection {
        /// 1-based line number.
        line: usize,
        /// The unrecognised section name.
        name: String,
    },
    /// A key/value line does not conform to `key = "value"` syntax.
    MalformedLine {
        /// 1-based line number.
        line: usize,
    },
    /// An unknown key appeared in the top-level scope.
    UnknownTopLevelKey {
        /// 1-based line number.
        line: usize,
        /// The unrecognised key.
        key: String,
    },
    /// A required top-level key appeared more than once.
    DuplicateTopLevelKey {
        /// 1-based line number of the second (duplicate) occurrence.
        line: usize,
        /// The duplicated key name.
        key: String,
    },
    /// A required top-level field was not found anywhere in the manifest.
    MissingField {
        /// The missing field name (`name`, `version`, `api_version`, or
        /// `entry`).
        field: String,
    },
    /// An unknown key appeared inside a section.
    UnknownSectionKey {
        /// 1-based line number.
        line: usize,
        /// The section the key appeared in.
        section: String,
        /// The unrecognised key.
        key: String,
    },
    /// A field value was empty where a non-empty value is required.
    EmptyValue {
        /// 1-based line number.
        line: usize,
        /// The key whose value was empty.
        key: String,
    },
    /// A quoted value contained a `"` or a control character (no escaping is
    /// supported; the value between the outer `"` delimiters must be
    /// printable non-`"` UTF-8).
    InvalidValueContent {
        /// 1-based line number.
        line: usize,
    },
    /// A `[filesystem]` path still starts with `..` after lexical
    /// normalisation, meaning it would escape any anchored root.
    EscapingFilesystemPath {
        /// 1-based line number.
        line: usize,
        /// The (post-normalisation) offending path.
        path: PathBuf,
    },
    /// A `[network]` `allow` value is not a valid `host:port` pair.
    InvalidNetworkEndpoint {
        /// 1-based line number.
        line: usize,
        /// The raw value that failed to parse.
        raw: String,
        /// Human-readable reason for the failure.
        reason: String,
    },
    /// A `[env]` `allow` value is not a valid environment variable name
    /// (empty, or contains `=` or ASCII whitespace).
    InvalidEnvName {
        /// 1-based line number.
        line: usize,
        /// The invalid name.
        raw: String,
    },
    /// A `[hooks]` `subscribe` value is not a known hook name.
    UnknownHook {
        /// 1-based line number.
        line: usize,
        /// The unrecognised hook name.
        name: String,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedSection { line } => {
                write!(f, "line {line}: malformed section header (missing `]`)")
            }
            Self::UnknownSection { line, name } => {
                write!(
                    f,
                    "line {line}: unknown section `{name}` (allowed: filesystem, network, command, env)"
                )
            }
            Self::MalformedLine { line } => {
                write!(
                    f,
                    "line {line}: malformed line (expected `key = \"value\"` or a section header or a comment)"
                )
            }
            Self::UnknownTopLevelKey { line, key } => {
                write!(
                    f,
                    "line {line}: unknown top-level key `{key}` (allowed: name, version, api_version, entry)"
                )
            }
            Self::DuplicateTopLevelKey { line, key } => {
                write!(f, "line {line}: duplicate top-level key `{key}`")
            }
            Self::MissingField { field } => {
                write!(f, "missing required field `{field}`")
            }
            Self::UnknownSectionKey { line, section, key } => {
                write!(
                    f,
                    "line {line}: unknown key `{key}` in section `[{section}]`"
                )
            }
            Self::EmptyValue { line, key } => {
                write!(f, "line {line}: empty value for key `{key}`")
            }
            Self::InvalidValueContent { line } => {
                write!(
                    f,
                    "line {line}: value contains a `\"` or a control character (no escape sequences are supported)"
                )
            }
            Self::EscapingFilesystemPath { line, path } => {
                write!(
                    f,
                    "line {line}: filesystem path `{}` escapes its root after normalisation (starts with `..`)",
                    path.display()
                )
            }
            Self::InvalidNetworkEndpoint { line, raw, reason } => {
                write!(f, "line {line}: invalid network endpoint `{raw}`: {reason}")
            }
            Self::InvalidEnvName { line, raw } => {
                write!(
                    f,
                    "line {line}: invalid env var name `{raw}` (must be non-empty, no `=` or whitespace)"
                )
            }
            Self::UnknownHook { line, name } => {
                write!(
                    f,
                    "line {line}: unknown hook `{name}` (expected one of: session_start, before_capability, session_end)"
                )
            }
        }
    }
}

impl std::error::Error for ManifestError {}

// ---------------------------------------------------------------------------
// Internal parser
// ---------------------------------------------------------------------------

/// Tracks which top-level required fields have been seen.
struct TopLevel {
    name: Option<String>,
    version: Option<String>,
    api_version: Option<String>,
    entry: Option<String>,
}

impl TopLevel {
    fn new() -> Self {
        Self {
            name: None,
            version: None,
            api_version: None,
            entry: None,
        }
    }
}

/// The active section scope while parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    TopLevel,
    Filesystem,
    Network,
    Command,
    Env,
    Hooks,
}

/// The hand-written, fail-closed line-oriented parser.
struct Parser<'src> {
    text: &'src str,
}

impl<'src> Parser<'src> {
    fn new(text: &'src str) -> Self {
        Self { text }
    }

    fn parse(self) -> Result<PluginManifest, ManifestError> {
        let mut top = TopLevel::new();
        let mut grants: Vec<CapabilityGrant> = Vec::new();
        let mut hooks: Vec<HookKind> = Vec::new();
        let mut section = Section::TopLevel;

        for (line_idx, raw_line) in self.text.lines().enumerate() {
            let line_num = line_idx + 1;
            let trimmed = raw_line.trim_ascii();

            // Blank lines and comments are ignored.
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Section header?
            if trimmed.starts_with('[') {
                section = Self::parse_section_header(trimmed, line_num)?;
                continue;
            }

            // Key/value pair.
            let (key, value) = Self::parse_key_value(trimmed, line_num)?;

            match section {
                Section::TopLevel => {
                    Self::handle_top_level(key, value, line_num, &mut top)?;
                }
                Section::Filesystem => {
                    let grant = Self::handle_filesystem(key, &value, line_num)?;
                    grants.push(grant);
                }
                Section::Network => {
                    let grant = Self::handle_network(key, value, line_num)?;
                    grants.push(grant);
                }
                Section::Command => {
                    let grant = Self::handle_command(key, &value, line_num)?;
                    grants.push(grant);
                }
                Section::Hooks => {
                    let hook = Self::handle_hooks(key, &value, line_num)?;
                    // De-duplicate: subscribing twice is harmless, not an error.
                    if !hooks.contains(&hook) {
                        hooks.push(hook);
                    }
                }
                Section::Env => {
                    let grant = Self::handle_env(key, value, line_num)?;
                    grants.push(grant);
                }
            }
        }

        // Validate that all required top-level fields are present.
        let name = top.name.ok_or_else(|| ManifestError::MissingField {
            field: "name".to_string(),
        })?;
        let version = top.version.ok_or_else(|| ManifestError::MissingField {
            field: "version".to_string(),
        })?;
        let api_version = top.api_version.ok_or_else(|| ManifestError::MissingField {
            field: "api_version".to_string(),
        })?;
        let entry_str = top.entry.ok_or_else(|| ManifestError::MissingField {
            field: "entry".to_string(),
        })?;

        Ok(PluginManifest {
            name,
            version,
            api_version,
            entry: PathBuf::from(entry_str),
            grants,
            hooks,
        })
    }

    /// Handle a `[hooks]` line: `subscribe = "<hook-name>"`. The name must
    /// be a known [`HookKind::wire_name`]; anything else is a hard,
    /// fail-closed error (an unrecognised hook subscription is never
    /// silently dropped — ADR 0007 §2).
    fn handle_hooks(key: &str, value: &str, line: usize) -> Result<HookKind, ManifestError> {
        if key != "subscribe" {
            return Err(ManifestError::UnknownSectionKey {
                line,
                section: "hooks".to_string(),
                key: key.to_string(),
            });
        }
        HookKind::from_wire_name(value).ok_or_else(|| ManifestError::UnknownHook {
            line,
            name: value.to_string(),
        })
    }

    /// Parse a `[section]` line and return the new active section.
    fn parse_section_header(trimmed: &str, line: usize) -> Result<Section, ManifestError> {
        // Must start with `[` (already checked) and end with `]`.
        if !trimmed.ends_with(']') {
            return Err(ManifestError::MalformedSection { line });
        }
        // Slice out the name between `[` and `]`.
        let name = &trimmed[1..trimmed.len() - 1];
        match name {
            "filesystem" => Ok(Section::Filesystem),
            "network" => Ok(Section::Network),
            "command" => Ok(Section::Command),
            "env" => Ok(Section::Env),
            "hooks" => Ok(Section::Hooks),
            other => Err(ManifestError::UnknownSection {
                line,
                name: other.to_string(),
            }),
        }
    }

    /// Parse a `key = "value"` line. Returns `(key, value)` with the value
    /// being the raw (unquoted) string content.
    fn parse_key_value(trimmed: &str, line: usize) -> Result<(&str, String), ManifestError> {
        // Find the `=` separator.
        let eq_pos = trimmed
            .find('=')
            .ok_or(ManifestError::MalformedLine { line })?;

        let key_part = trimmed[..eq_pos].trim_ascii();
        let rest = trimmed[eq_pos + 1..].trim_ascii();

        // Key must be non-empty and only ASCII alnum + `_`/`-`.
        if key_part.is_empty()
            || !key_part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ManifestError::MalformedLine { line });
        }

        // Value must be surrounded by exactly one `"` on each side.
        if !rest.starts_with('"') || !rest.ends_with('"') || rest.len() < 2 {
            return Err(ManifestError::MalformedLine { line });
        }

        // For a single `""` (two chars total) the value is empty.
        // For a two-char string `""`, start==end after slicing means empty.
        let inner = if rest.len() == 2 {
            // `""` — empty value
            ""
        } else {
            // Must have a closing `"` that is different from the opening one.
            // We already confirmed starts_with and ends_with `"`.
            // But we need to ensure there's no embedded `"` in the interior.
            &rest[1..rest.len() - 1]
        };

        // Reject embedded `"` or control characters (no escaping supported).
        if inner.contains('"') {
            return Err(ManifestError::InvalidValueContent { line });
        }
        if inner.chars().any(|c| c.is_control()) {
            return Err(ManifestError::InvalidValueContent { line });
        }

        Ok((key_part, inner.to_string()))
    }

    /// Handle a top-level key/value pair, updating `top`.
    fn handle_top_level(
        key: &str,
        value: String,
        line: usize,
        top: &mut TopLevel,
    ) -> Result<(), ManifestError> {
        match key {
            "name" | "version" | "api_version" | "entry" => {}
            other => {
                return Err(ManifestError::UnknownTopLevelKey {
                    line,
                    key: other.to_string(),
                });
            }
        }

        // Check for empty values (none of the top-level fields allow empty).
        if value.is_empty() {
            return Err(ManifestError::EmptyValue {
                line,
                key: key.to_string(),
            });
        }

        match key {
            "name" => {
                if top.name.is_some() {
                    return Err(ManifestError::DuplicateTopLevelKey {
                        line,
                        key: "name".to_string(),
                    });
                }
                top.name = Some(value);
            }
            "version" => {
                if top.version.is_some() {
                    return Err(ManifestError::DuplicateTopLevelKey {
                        line,
                        key: "version".to_string(),
                    });
                }
                top.version = Some(value);
            }
            "api_version" => {
                if top.api_version.is_some() {
                    return Err(ManifestError::DuplicateTopLevelKey {
                        line,
                        key: "api_version".to_string(),
                    });
                }
                top.api_version = Some(value);
            }
            "entry" => {
                if top.entry.is_some() {
                    return Err(ManifestError::DuplicateTopLevelKey {
                        line,
                        key: "entry".to_string(),
                    });
                }
                top.entry = Some(value);
            }
            // Already validated above — unreachable.
            _ => unreachable!(),
        }
        Ok(())
    }

    /// Handle a `[filesystem]` key/value, returning the produced grant.
    fn handle_filesystem(
        key: &str,
        value: &str,
        line: usize,
    ) -> Result<CapabilityGrant, ManifestError> {
        let mode = match key {
            "read" => FsMode::Read,
            "write" => FsMode::Write,
            other => {
                return Err(ManifestError::UnknownSectionKey {
                    line,
                    section: "filesystem".to_string(),
                    key: other.to_string(),
                });
            }
        };

        if value.is_empty() {
            return Err(ManifestError::EmptyValue {
                line,
                key: key.to_string(),
            });
        }

        let root = normalize_lexical(Path::new(value));

        // Reject paths that escape after normalisation.
        if root.starts_with("..") {
            return Err(ManifestError::EscapingFilesystemPath { line, path: root });
        }

        Ok(CapabilityGrant::Filesystem { mode, root })
    }

    /// Handle a `[network]` key/value, returning the produced grant.
    fn handle_network(
        key: &str,
        value: String,
        line: usize,
    ) -> Result<CapabilityGrant, ManifestError> {
        if key != "allow" {
            return Err(ManifestError::UnknownSectionKey {
                line,
                section: "network".to_string(),
                key: key.to_string(),
            });
        }

        if value.is_empty() {
            return Err(ManifestError::EmptyValue {
                line,
                key: key.to_string(),
            });
        }

        // Find the last `:` to split host and port (IPv6 addresses would have
        // colons, but our grammar says host must contain no `:`, so splitting
        // on the *first* `:` is equivalent and correct).
        let colon_pos = value
            .find(':')
            .ok_or_else(|| ManifestError::InvalidNetworkEndpoint {
                line,
                raw: value.clone(),
                reason: "missing `:` separator between host and port".to_string(),
            })?;

        let host = value[..colon_pos].to_string();
        let port_str = &value[colon_pos + 1..];

        if host.is_empty() {
            return Err(ManifestError::InvalidNetworkEndpoint {
                line,
                raw: value,
                reason: "host must be non-empty".to_string(),
            });
        }

        // The grammar says host must contain no `:`.  Because we split on the
        // first `:`, any additional `:` in the remainder is a port parse error.
        // But we also need to check the host portion.
        if host.contains(':') {
            return Err(ManifestError::InvalidNetworkEndpoint {
                line,
                raw: value,
                reason: "host must not contain `:`".to_string(),
            });
        }

        let port: u16 =
            port_str
                .parse::<u16>()
                .map_err(|_| ManifestError::InvalidNetworkEndpoint {
                    line,
                    raw: value.clone(),
                    reason: format!("port `{port_str}` is not a valid u16 (1..=65535)"),
                })?;

        if port == 0 {
            return Err(ManifestError::InvalidNetworkEndpoint {
                line,
                raw: value,
                reason: "port 0 is not allowed".to_string(),
            });
        }

        Ok(CapabilityGrant::Network { host, port })
    }

    /// Handle a `[command]` key/value, returning the produced grant.
    fn handle_command(
        key: &str,
        value: &str,
        line: usize,
    ) -> Result<CapabilityGrant, ManifestError> {
        if key != "allow" {
            return Err(ManifestError::UnknownSectionKey {
                line,
                section: "command".to_string(),
                key: key.to_string(),
            });
        }

        if value.is_empty() {
            return Err(ManifestError::EmptyValue {
                line,
                key: key.to_string(),
            });
        }

        let mut tokens: Vec<String> = value.split_whitespace().map(|s| s.to_string()).collect();

        // First token is the program; the rest are the args prefix.
        // `split_whitespace` on a non-empty string always yields at least one
        // token.
        let program = tokens.remove(0);
        let args_prefix = tokens;

        Ok(CapabilityGrant::Command {
            program,
            args_prefix,
        })
    }

    /// Handle an `[env]` key/value, returning the produced grant.
    fn handle_env(key: &str, value: String, line: usize) -> Result<CapabilityGrant, ManifestError> {
        if key != "allow" {
            return Err(ManifestError::UnknownSectionKey {
                line,
                section: "env".to_string(),
                key: key.to_string(),
            });
        }

        if value.is_empty() {
            return Err(ManifestError::InvalidEnvName { line, raw: value });
        }

        if value.contains('=') || value.chars().any(|c| c.is_ascii_whitespace()) {
            return Err(ManifestError::InvalidEnvName { line, raw: value });
        }

        Ok(CapabilityGrant::Env { key: value })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{CapabilityGrant, FsMode};
    use std::path::Path;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// The four required top-level fields as a ready-to-extend string.
    /// The trailing newline is intentional so callers can concatenate a
    /// section header immediately after.
    const MINIMAL_VALID: &str = concat!(
        "name = \"test-plugin\"\n",
        "version = \"1.0.0\"\n",
        "api_version = \"0.1.0\"\n",
        "entry = \"/usr/bin/test-plugin\"\n",
    );

    /// Return `MINIMAL_VALID` with `extra` appended (runtime concatenation
    /// because `concat!` only accepts literals, not const values).
    fn with_base(extra: &str) -> String {
        format!("{MINIMAL_VALID}{extra}")
    }

    // -----------------------------------------------------------------------
    // Full valid manifest with every grant kind
    // -----------------------------------------------------------------------

    #[test]
    fn full_valid_manifest_parses_all_grant_kinds() {
        let src = concat!(
            "name = \"full-plugin\"\n",
            "version = \"2.3.4\"\n",
            "api_version = \"1.0.0\"\n",
            "entry = \"/opt/full-plugin/bin\"\n",
            "\n",
            "# Grant filesystem access\n",
            "[filesystem]\n",
            "read = \"/data/input\"\n",
            "write = \"/data/output\"\n",
            "\n",
            "[network]\n",
            "allow = \"api.example.com:443\"\n",
            "allow = \"internal.svc:8080\"\n",
            "\n",
            "[command]\n",
            "allow = \"git log --oneline\"\n",
            "allow = \"ls\"\n",
            "\n",
            "[env]\n",
            "allow = \"HOME\"\n",
            "allow = \"PATH\"\n",
        );

        let m = PluginManifest::parse(src).unwrap();
        assert_eq!(m.name, "full-plugin");
        assert_eq!(m.version, "2.3.4");
        assert_eq!(m.api_version, "1.0.0");
        assert_eq!(m.entry, Path::new("/opt/full-plugin/bin"));
        assert_eq!(m.grants.len(), 8);

        assert_eq!(
            m.grants[0],
            CapabilityGrant::Filesystem {
                mode: FsMode::Read,
                root: PathBuf::from("/data/input")
            }
        );
        assert_eq!(
            m.grants[1],
            CapabilityGrant::Filesystem {
                mode: FsMode::Write,
                root: PathBuf::from("/data/output"),
            }
        );
        assert_eq!(
            m.grants[2],
            CapabilityGrant::Network {
                host: "api.example.com".into(),
                port: 443
            }
        );
        assert_eq!(
            m.grants[3],
            CapabilityGrant::Network {
                host: "internal.svc".into(),
                port: 8080
            }
        );
        assert_eq!(
            m.grants[4],
            CapabilityGrant::Command {
                program: "git".into(),
                args_prefix: vec!["log".into(), "--oneline".into()],
            }
        );
        assert_eq!(
            m.grants[5],
            CapabilityGrant::Command {
                program: "ls".into(),
                args_prefix: vec![]
            }
        );
        assert_eq!(m.grants[6], CapabilityGrant::Env { key: "HOME".into() });
        assert_eq!(m.grants[7], CapabilityGrant::Env { key: "PATH".into() });
    }

    // -----------------------------------------------------------------------
    // Missing required fields
    // -----------------------------------------------------------------------

    #[test]
    fn missing_name_is_an_error() {
        let src = concat!(
            "version = \"1.0.0\"\n",
            "api_version = \"0.1.0\"\n",
            "entry = \"/bin/p\"\n",
        );
        assert_eq!(
            PluginManifest::parse(src).unwrap_err(),
            ManifestError::MissingField {
                field: "name".to_string()
            }
        );
    }

    #[test]
    fn missing_version_is_an_error() {
        let src = concat!(
            "name = \"p\"\n",
            "api_version = \"0.1.0\"\n",
            "entry = \"/bin/p\"\n",
        );
        assert_eq!(
            PluginManifest::parse(src).unwrap_err(),
            ManifestError::MissingField {
                field: "version".to_string()
            }
        );
    }

    #[test]
    fn missing_api_version_is_an_error() {
        let src = concat!(
            "name = \"p\"\n",
            "version = \"1.0.0\"\n",
            "entry = \"/bin/p\"\n",
        );
        assert_eq!(
            PluginManifest::parse(src).unwrap_err(),
            ManifestError::MissingField {
                field: "api_version".to_string()
            }
        );
    }

    #[test]
    fn missing_entry_is_an_error() {
        let src = concat!(
            "name = \"p\"\n",
            "version = \"1.0.0\"\n",
            "api_version = \"0.1.0\"\n",
        );
        assert_eq!(
            PluginManifest::parse(src).unwrap_err(),
            ManifestError::MissingField {
                field: "entry".to_string()
            }
        );
    }

    // -----------------------------------------------------------------------
    // Duplicate top-level keys
    // -----------------------------------------------------------------------

    #[test]
    fn duplicate_name_key_is_an_error() {
        let src = concat!(
            "name = \"first\"\n",
            "name = \"second\"\n",
            "version = \"1.0.0\"\n",
            "api_version = \"0.1.0\"\n",
            "entry = \"/bin/p\"\n",
        );
        assert_eq!(
            PluginManifest::parse(src).unwrap_err(),
            ManifestError::DuplicateTopLevelKey {
                line: 2,
                key: "name".to_string()
            }
        );
    }

    #[test]
    fn duplicate_version_key_is_an_error() {
        let src = concat!(
            "name = \"p\"\n",
            "version = \"1.0.0\"\n",
            "version = \"2.0.0\"\n",
            "api_version = \"0.1.0\"\n",
            "entry = \"/bin/p\"\n",
        );
        assert_eq!(
            PluginManifest::parse(src).unwrap_err(),
            ManifestError::DuplicateTopLevelKey {
                line: 3,
                key: "version".to_string()
            }
        );
    }

    #[test]
    fn duplicate_api_version_key_is_an_error() {
        let src = concat!(
            "name = \"p\"\n",
            "version = \"1.0.0\"\n",
            "api_version = \"0.1.0\"\n",
            "api_version = \"0.2.0\"\n",
            "entry = \"/bin/p\"\n",
        );
        assert_eq!(
            PluginManifest::parse(src).unwrap_err(),
            ManifestError::DuplicateTopLevelKey {
                line: 4,
                key: "api_version".to_string()
            }
        );
    }

    #[test]
    fn duplicate_entry_key_is_an_error() {
        let src = concat!(
            "name = \"p\"\n",
            "version = \"1.0.0\"\n",
            "api_version = \"0.1.0\"\n",
            "entry = \"/bin/p\"\n",
            "entry = \"/bin/q\"\n",
        );
        assert_eq!(
            PluginManifest::parse(src).unwrap_err(),
            ManifestError::DuplicateTopLevelKey {
                line: 5,
                key: "entry".to_string()
            }
        );
    }

    // -----------------------------------------------------------------------
    // Unknown keys
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_top_level_key_is_an_error() {
        let src = concat!(
            "name = \"p\"\n",
            "version = \"1.0.0\"\n",
            "api_version = \"0.1.0\"\n",
            "entry = \"/bin/p\"\n",
            "foo = \"bar\"\n",
        );
        assert_eq!(
            PluginManifest::parse(src).unwrap_err(),
            ManifestError::UnknownTopLevelKey {
                line: 5,
                key: "foo".to_string()
            }
        );
    }

    #[test]
    fn unknown_filesystem_key_is_an_error() {
        let src = with_base("[filesystem]\nexec = \"/bin/x\"\n");
        assert_eq!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::UnknownSectionKey {
                line: 6,
                section: "filesystem".to_string(),
                key: "exec".to_string(),
            }
        );
    }

    #[test]
    fn unknown_network_key_is_an_error() {
        let src = with_base("[network]\ndeny = \"example.com:80\"\n");
        assert_eq!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::UnknownSectionKey {
                line: 6,
                section: "network".to_string(),
                key: "deny".to_string(),
            }
        );
    }

    #[test]
    fn unknown_command_key_is_an_error() {
        let src = with_base("[command]\nblock = \"rm\"\n");
        assert_eq!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::UnknownSectionKey {
                line: 6,
                section: "command".to_string(),
                key: "block".to_string(),
            }
        );
    }

    #[test]
    fn unknown_env_key_is_an_error() {
        let src = with_base("[env]\ndeny = \"SECRET\"\n");
        assert_eq!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::UnknownSectionKey {
                line: 6,
                section: "env".to_string(),
                key: "deny".to_string(),
            }
        );
    }

    // -----------------------------------------------------------------------
    // Unknown sections
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_section_is_an_error() {
        let src = with_base("[database]\nallow = \"main\"\n");
        assert_eq!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::UnknownSection {
                line: 5,
                name: "database".to_string()
            }
        );
    }

    // -----------------------------------------------------------------------
    // Malformed lines
    // -----------------------------------------------------------------------

    #[test]
    fn line_with_no_equals_is_malformed() {
        let src = with_base("[env]\nNOEQUALS\n");
        assert_eq!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::MalformedLine { line: 6 }
        );
    }

    #[test]
    fn value_missing_opening_quote_is_malformed() {
        let src = concat!(
            "name = no-quotes\n",
            "version = \"1.0.0\"\n",
            "api_version = \"0.1.0\"\n",
            "entry = \"/bin/p\"\n",
        );
        assert_eq!(
            PluginManifest::parse(src).unwrap_err(),
            ManifestError::MalformedLine { line: 1 }
        );
    }

    #[test]
    fn malformed_section_header_missing_close_bracket_is_an_error() {
        let src = with_base("[filesystem\nread = \"/data\"\n");
        assert_eq!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::MalformedSection { line: 5 }
        );
    }

    // -----------------------------------------------------------------------
    // Value content rules
    // -----------------------------------------------------------------------

    #[test]
    fn value_with_embedded_double_quote_is_rejected() {
        // `\"` in Rust string literals produces one `"` in the raw bytes.
        // The line seen by the parser is: name = "emb"edded"
        let src = concat!(
            "name = \"emb\"edded\"\n",
            "version = \"1.0.0\"\n",
            "api_version = \"0.1.0\"\n",
            "entry = \"/bin/p\"\n",
        );
        assert!(matches!(
            PluginManifest::parse(src).unwrap_err(),
            ManifestError::MalformedLine { line: 1 }
                | ManifestError::InvalidValueContent { line: 1 }
        ));
    }

    /// Same check built with runtime `String` to make the embedded `"` intent
    /// unambiguous regardless of escape-sequence reading.
    #[test]
    fn value_with_literal_embedded_quote_is_rejected() {
        let mut src = String::new();
        src.push_str("name = \"em");
        src.push('"');
        src.push_str("bedded\"\n");
        src.push_str("version = \"1.0.0\"\n");
        src.push_str("api_version = \"0.1.0\"\n");
        src.push_str("entry = \"/bin/p\"\n");
        let err = PluginManifest::parse(&src).unwrap_err();
        assert!(
            matches!(
                err,
                ManifestError::MalformedLine { .. } | ManifestError::InvalidValueContent { .. }
            ),
            "expected a parse error, got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Network endpoint validation
    // -----------------------------------------------------------------------

    #[test]
    fn network_without_colon_separator_is_invalid() {
        let src = with_base("[network]\nallow = \"nocolon\"\n");
        assert!(matches!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::InvalidNetworkEndpoint { line: 6, .. }
        ));
    }

    #[test]
    fn network_empty_host_is_invalid() {
        let src = with_base("[network]\nallow = \":443\"\n");
        assert!(matches!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::InvalidNetworkEndpoint { line: 6, .. }
        ));
    }

    #[test]
    fn network_port_zero_is_invalid() {
        let src = with_base("[network]\nallow = \"host:0\"\n");
        assert!(matches!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::InvalidNetworkEndpoint { line: 6, .. }
        ));
    }

    #[test]
    fn network_port_above_65535_is_invalid() {
        let src = with_base("[network]\nallow = \"host:65536\"\n");
        assert!(matches!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::InvalidNetworkEndpoint { line: 6, .. }
        ));
    }

    #[test]
    fn network_non_numeric_port_is_invalid() {
        let src = with_base("[network]\nallow = \"host:http\"\n");
        assert!(matches!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::InvalidNetworkEndpoint { line: 6, .. }
        ));
    }

    #[test]
    fn network_port_65535_is_valid() {
        let src = with_base("[network]\nallow = \"host:65535\"\n");
        let m = PluginManifest::parse(&src).unwrap();
        assert_eq!(
            m.grants[0],
            CapabilityGrant::Network {
                host: "host".into(),
                port: 65535
            }
        );
    }

    #[test]
    fn network_port_1_is_valid() {
        let src = with_base("[network]\nallow = \"host:1\"\n");
        let m = PluginManifest::parse(&src).unwrap();
        assert_eq!(
            m.grants[0],
            CapabilityGrant::Network {
                host: "host".into(),
                port: 1
            }
        );
    }

    // -----------------------------------------------------------------------
    // Env variable name validation
    // -----------------------------------------------------------------------

    #[test]
    fn empty_env_name_is_invalid() {
        let src = with_base("[env]\nallow = \"\"\n");
        assert!(matches!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::InvalidEnvName { line: 6, .. }
        ));
    }

    #[test]
    fn env_name_with_equals_is_invalid() {
        let src = with_base("[env]\nallow = \"KEY=VALUE\"\n");
        assert_eq!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::InvalidEnvName {
                line: 6,
                raw: "KEY=VALUE".to_string()
            }
        );
    }

    #[test]
    fn env_name_with_whitespace_is_invalid() {
        let src = with_base("[env]\nallow = \"MY VAR\"\n");
        assert_eq!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::InvalidEnvName {
                line: 6,
                raw: "MY VAR".to_string()
            }
        );
    }

    // -----------------------------------------------------------------------
    // Filesystem path escaping
    // -----------------------------------------------------------------------

    #[test]
    fn filesystem_path_that_normalises_to_dotdot_is_rejected() {
        let src = with_base("[filesystem]\nread = \"../outside\"\n");
        assert!(matches!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::EscapingFilesystemPath { line: 6, .. }
        ));
    }

    #[test]
    fn filesystem_path_with_dotdot_that_resolves_inside_is_accepted() {
        // `/data/a/../b` normalises to `/data/b` — not escaping.
        let src = with_base("[filesystem]\nread = \"/data/a/../b\"\n");
        let m = PluginManifest::parse(&src).unwrap();
        assert_eq!(
            m.grants[0],
            CapabilityGrant::Filesystem {
                mode: FsMode::Read,
                root: PathBuf::from("/data/b")
            }
        );
    }

    #[test]
    fn filesystem_relative_escaping_path_is_rejected() {
        // `a/../../b` normalises to `../b`, which starts with `..`.
        let src = with_base("[filesystem]\nread = \"a/../../b\"\n");
        assert!(matches!(
            PluginManifest::parse(&src).unwrap_err(),
            ManifestError::EscapingFilesystemPath { line: 6, .. }
        ));
    }

    // -----------------------------------------------------------------------
    // Blank lines and comments are ignored
    // -----------------------------------------------------------------------

    #[test]
    fn blank_lines_and_comments_are_ignored() {
        let src = concat!(
            "# This is a comment\n",
            "\n",
            "name = \"p\"\n",
            "# another comment\n",
            "\n",
            "version = \"1.0.0\"\n",
            "api_version = \"0.1.0\"\n",
            "entry = \"/bin/p\"\n",
        );
        let m = PluginManifest::parse(src).unwrap();
        assert_eq!(m.name, "p");
        assert_eq!(m.grants, vec![]);
    }

    // -----------------------------------------------------------------------
    // Section ordering is irrelevant
    // -----------------------------------------------------------------------

    #[test]
    fn sections_may_appear_in_any_order() {
        // sections in env → filesystem → network → command order (not the
        // "natural" filesystem-first order); grants accumulate in that order.
        let src = concat!(
            "name = \"order-test\"\n",
            "version = \"1.0.0\"\n",
            "api_version = \"0.1.0\"\n",
            "entry = \"/bin/p\"\n",
            "[env]\n",
            "allow = \"HOME\"\n",
            "[filesystem]\n",
            "read = \"/data\"\n",
            "[network]\n",
            "allow = \"svc:80\"\n",
            "[command]\n",
            "allow = \"ls\"\n",
        );
        let m = PluginManifest::parse(src).unwrap();
        assert_eq!(m.name, "order-test");
        // Grants appear in declaration order.
        assert_eq!(m.grants[0], CapabilityGrant::Env { key: "HOME".into() });
        assert_eq!(
            m.grants[1],
            CapabilityGrant::Filesystem {
                mode: FsMode::Read,
                root: PathBuf::from("/data")
            }
        );
        assert_eq!(
            m.grants[2],
            CapabilityGrant::Network {
                host: "svc".into(),
                port: 80
            }
        );
        assert_eq!(
            m.grants[3],
            CapabilityGrant::Command {
                program: "ls".into(),
                args_prefix: vec![]
            }
        );
    }

    // -----------------------------------------------------------------------
    // Repeated grants accumulate
    // -----------------------------------------------------------------------

    #[test]
    fn repeated_grants_all_accumulate() {
        let src = with_base(concat!(
            "[filesystem]\n",
            "read = \"/a\"\n",
            "read = \"/b\"\n",
            "write = \"/c\"\n",
            "[network]\n",
            "allow = \"h1:80\"\n",
            "allow = \"h2:443\"\n",
            "[command]\n",
            "allow = \"git\"\n",
            "allow = \"ls -la\"\n",
            "[env]\n",
            "allow = \"HOME\"\n",
            "allow = \"PATH\"\n",
            "allow = \"TERM\"\n",
        ));
        let m = PluginManifest::parse(&src).unwrap();
        assert_eq!(m.grants.len(), 10);
    }

    // -----------------------------------------------------------------------
    // Display impl produces operator-readable messages
    // -----------------------------------------------------------------------

    #[test]
    fn display_messages_contain_line_numbers_and_key_names() {
        let e = ManifestError::UnknownTopLevelKey {
            line: 7,
            key: "foo".to_string(),
        };
        let msg = e.to_string();
        assert!(msg.contains("line 7"), "expected line number in: {msg}");
        assert!(msg.contains("foo"), "expected key name in: {msg}");

        let e2 = ManifestError::MissingField {
            field: "entry".to_string(),
        };
        let msg2 = e2.to_string();
        assert!(msg2.contains("entry"), "expected field name in: {msg2}");
        // MissingField has no line number.
        assert!(
            !msg2.contains("line"),
            "should not have a line number: {msg2}"
        );

        let e3 = ManifestError::InvalidNetworkEndpoint {
            line: 12,
            raw: "bad".to_string(),
            reason: "missing `:` separator".to_string(),
        };
        let msg3 = e3.to_string();
        assert!(msg3.contains("line 12"), "expected line number in: {msg3}");
        assert!(msg3.contains("bad"), "expected raw value in: {msg3}");
    }

    #[test]
    fn manifest_error_implements_std_error() {
        // Confirm the trait is satisfied (compile-time check via usage).
        fn accepts_error(_: &dyn std::error::Error) {}
        let e = ManifestError::MissingField {
            field: "name".to_string(),
        };
        accepts_error(&e);
    }

    // -----------------------------------------------------------------------
    // Command args_prefix
    // -----------------------------------------------------------------------

    #[test]
    fn command_with_no_args_has_empty_prefix() {
        let src = with_base("[command]\nallow = \"ls\"\n");
        let m = PluginManifest::parse(&src).unwrap();
        assert_eq!(
            m.grants[0],
            CapabilityGrant::Command {
                program: "ls".into(),
                args_prefix: vec![]
            }
        );
    }

    #[test]
    fn command_with_multiple_args_splits_correctly() {
        let src = with_base("[command]\nallow = \"git log --oneline -n 10\"\n");
        let m = PluginManifest::parse(&src).unwrap();
        assert_eq!(
            m.grants[0],
            CapabilityGrant::Command {
                program: "git".into(),
                args_prefix: vec!["log".into(), "--oneline".into(), "-n".into(), "10".into()],
            }
        );
    }

    // -----------------------------------------------------------------------
    // Filesystem path normalisation
    // -----------------------------------------------------------------------

    #[test]
    fn filesystem_path_is_normalised() {
        // `/data/./sub/../sub` normalises to `/data/sub`.
        let src = with_base("[filesystem]\nread = \"/data/./sub/../sub\"\n");
        let m = PluginManifest::parse(&src).unwrap();
        assert_eq!(
            m.grants[0],
            CapabilityGrant::Filesystem {
                mode: FsMode::Read,
                root: PathBuf::from("/data/sub")
            }
        );
    }

    // -----------------------------------------------------------------------
    // Top-level key after section is treated as an unknown section key
    // -----------------------------------------------------------------------

    #[test]
    fn top_level_key_after_section_is_unknown_section_key() {
        // Once inside `[env]`, `name` is an unknown env key, not a top-level one.
        let src = concat!(
            "version = \"1.0.0\"\n",
            "api_version = \"0.1.0\"\n",
            "entry = \"/bin/p\"\n",
            "[env]\n",
            "name = \"late\"\n",
        );
        assert_eq!(
            PluginManifest::parse(src).unwrap_err(),
            ManifestError::UnknownSectionKey {
                line: 5,
                section: "env".to_string(),
                key: "name".to_string(),
            }
        );
    }

    // -----------------------------------------------------------------------
    // Empty manifest
    // -----------------------------------------------------------------------

    #[test]
    fn empty_manifest_is_missing_all_required_fields() {
        // First missing field reported is `name`.
        assert_eq!(
            PluginManifest::parse("").unwrap_err(),
            ManifestError::MissingField {
                field: "name".to_string()
            }
        );
    }

    // -----------------------------------------------------------------------
    // Trimming of leading/trailing whitespace
    // -----------------------------------------------------------------------

    #[test]
    fn leading_and_trailing_whitespace_on_lines_is_ignored() {
        let src = concat!(
            "   name = \"ws-test\"   \n",
            "  version = \"1.0.0\"\n",
            "api_version = \"0.1.0\"  \n",
            "  entry = \"/bin/p\"  \n",
        );
        let m = PluginManifest::parse(src).unwrap();
        assert_eq!(m.name, "ws-test");
    }

    fn with_hooks(section: &str) -> Result<PluginManifest, ManifestError> {
        PluginManifest::parse(&format!(
            "name = \"p\"\nversion = \"1\"\napi_version = \"1\"\nentry = \"b\"\n{section}"
        ))
    }

    #[test]
    fn hooks_section_parses_subscriptions_in_order_deduped() {
        let m = with_hooks(
            "[hooks]\nsubscribe = \"before_capability\"\nsubscribe = \"session_start\"\nsubscribe = \"before_capability\"\n",
        )
        .unwrap();
        assert_eq!(
            m.hooks,
            vec![HookKind::BeforeCapability, HookKind::SessionStart],
            "declaration order preserved, duplicate de-duplicated"
        );
    }

    #[test]
    fn unknown_hook_name_is_fail_closed() {
        let err = with_hooks("[hooks]\nsubscribe = \"on_everything\"\n").unwrap_err();
        let msg = err.to_string();
        assert!(matches!(&err, ManifestError::UnknownHook { name, .. } if name == "on_everything"));
        assert!(msg.contains("unknown hook `on_everything`"));
    }

    #[test]
    fn unknown_hooks_key_is_an_error() {
        let err = with_hooks("[hooks]\nlisten = \"session_start\"\n").unwrap_err();
        assert!(matches!(
            err,
            ManifestError::UnknownSectionKey { section, key, .. }
                if section == "hooks" && key == "listen"
        ));
    }

    #[test]
    fn no_hooks_section_means_no_subscriptions() {
        let m = with_hooks("[env]\nallow = \"PATH\"\n").unwrap();
        assert!(m.hooks.is_empty());
    }
}
