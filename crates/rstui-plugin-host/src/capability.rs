//! The closed four-capability authority model (ADR 0007 §1) and the pure,
//! filesystem-free path-scope check (§3) every filesystem grant is enforced
//! through.
//!
//! This module is the shared backbone: [`manifest`](crate::manifest) parses
//! declarations into [`CapabilityGrant`]s, [`permission`](crate::permission)
//! decides a [`CapabilityRequest`] against them, and the host performs the
//! allowed effect. The capability set is **closed on purpose** — a plugin
//! cannot invent a new kind of authority, so the code that must reason about
//! every capability is finite and auditable, mirroring secure-exec's
//! four-member `SystemDriver` (`filesystem`, `network`, `commandExecutor`,
//! `env`).
//!
//! ## Why the path check is lexical
//!
//! [`normalize_lexical`] and [`is_within`] resolve `.`/`..` **without
//! touching the filesystem**. That is deliberate and matches secure-exec's
//! `normalizeFsPath` (also lexical): it is *total* (no IO, no errors, no
//! TOCTOU window between the check and the effect) and therefore
//! deterministically testable — the property the whole crate is built for.
//! The cost, recorded in ADR 0007 §7 as a named residual, is that a
//! *symlink* inside a granted root that points outside it is not detected
//! lexically; defeating that needs real filesystem resolution and is an
//! operator-deployment concern (run the plugin under an OS sandbox), not
//! something safe Rust enforces here.

use std::fmt;
use std::path::{Component, Path, PathBuf};

/// The closed set of authority kinds a plugin process can be granted
/// (ADR 0007 §1). Closed: a manifest cannot request, and a policy cannot
/// grant, anything outside these four.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Reading or writing a scoped filesystem subtree.
    Filesystem,
    /// Connecting to a scoped network endpoint.
    Network,
    /// Running a scoped external command.
    Command,
    /// Reading a named environment variable.
    Env,
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Filesystem => "filesystem",
            Self::Network => "network",
            Self::Command => "command",
            Self::Env => "env",
        })
    }
}

/// Whether a filesystem grant or request is for reading or writing.
///
/// Modes are matched **exactly** (a `Write` grant does not imply `Read`):
/// least privilege is explicit, so a plugin that needs both declares both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FsMode {
    /// Read access.
    Read,
    /// Write access.
    Write,
}

impl fmt::Display for FsMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Read => "read",
            Self::Write => "write",
        })
    }
}

/// A single scoped capability a [`PluginManifest`](crate::manifest::PluginManifest)
/// declares. The host [`PermissionPolicy`](crate::permission::PermissionPolicy)
/// matches an incoming [`CapabilityRequest`] against the manifest's grants;
/// anything unmatched is denied (ADR 0007 §2: the manifest is the *request*,
/// the policy is the *grant*, default deny).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityGrant {
    /// A filesystem subtree the plugin may access in `mode`. `root` is
    /// stored already [`normalize_lexical`]-d so the policy compares
    /// canonical prefixes.
    Filesystem {
        /// Read vs write.
        mode: FsMode,
        /// The lexically-normalised subtree root.
        root: PathBuf,
    },
    /// A network endpoint (`host`, `port`) the plugin may connect to.
    Network {
        /// Hostname or IP literal, exactly as declared.
        host: String,
        /// TCP port.
        port: u16,
    },
    /// A program the plugin may run, gated by an argument-*prefix*
    /// allowlist: a request's args must start with `args_prefix`.
    Command {
        /// The program name/path, matched exactly.
        program: String,
        /// Required leading arguments (may be empty: any args allowed).
        args_prefix: Vec<String>,
    },
    /// An environment variable name the plugin may read.
    Env {
        /// The variable name, matched exactly.
        key: String,
    },
}

impl CapabilityGrant {
    /// Which [`Capability`] kind this grant is for.
    #[must_use]
    pub fn capability(&self) -> Capability {
        match self {
            Self::Filesystem { .. } => Capability::Filesystem,
            Self::Network { .. } => Capability::Network,
            Self::Command { .. } => Capability::Command,
            Self::Env { .. } => Capability::Env,
        }
    }
}

/// A privileged action a plugin asks the host to perform on its behalf.
///
/// The host **canonicalises this before the policy sees it** (ADR 0007 §3):
/// a [`CapabilityRequest::Filesystem`] `path` is made absolute and
/// [`normalize_lexical`]-d by the host, so a plugin cannot defeat a grant
/// with `..`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityRequest {
    /// Access `path` in `mode`.
    Filesystem {
        /// Read vs write.
        mode: FsMode,
        /// The target path (host-canonicalised before policy check).
        path: PathBuf,
    },
    /// Connect to `host:port`.
    Network {
        /// Target hostname or IP literal.
        host: String,
        /// Target TCP port.
        port: u16,
    },
    /// Run `program` with `args`.
    Command {
        /// Program name/path.
        program: String,
        /// Full argument vector.
        args: Vec<String>,
    },
    /// Read environment variable `key`.
    Env {
        /// The variable name.
        key: String,
    },
}

impl CapabilityRequest {
    /// Which [`Capability`] kind this request needs.
    #[must_use]
    pub fn capability(&self) -> Capability {
        match self {
            Self::Filesystem { .. } => Capability::Filesystem,
            Self::Network { .. } => Capability::Network,
            Self::Command { .. } => Capability::Command,
            Self::Env { .. } => Capability::Env,
        }
    }
}

/// Lexically normalise `path`: drop `.`, and resolve `..` by cancelling the
/// preceding *normal* segment — **without any filesystem access**.
///
/// This is total and deterministic (ADR 0007 §3, secure-exec's
/// `normalizeFsPath`). A `..` that cannot cancel a real segment (because the
/// path is relative and has run out of segments to pop) is **preserved** as a
/// leading `..`, so an escaping path stays detectably escaping rather than
/// silently collapsing. For an absolute path, `..` at the root is dropped
/// (you cannot climb above `/`).
///
/// ```
/// use std::path::Path;
/// use rstui_plugin_host::capability::normalize_lexical;
///
/// assert_eq!(normalize_lexical(Path::new("a/./b/../c")), Path::new("a/c"));
/// assert_eq!(normalize_lexical(Path::new("/a/../../b")), Path::new("/b"));
/// assert_eq!(normalize_lexical(Path::new("../escapes")), Path::new("../escapes"));
/// ```
#[must_use]
pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {
                    // Cannot climb above an absolute root: drop the `..`.
                }
                _ => out.push(Component::ParentDir),
            },
            other => out.push(other),
        }
    }
    if out.is_empty() {
        return PathBuf::from(".");
    }
    out.iter().collect()
}

/// Whether `candidate` lies within the granted `root` subtree, judged
/// **lexically** (both sides [`normalize_lexical`]-d first).
///
/// `true` only if the normalised candidate is `root` itself or a descendant
/// of it. Because normalisation resolves `..` *before* the prefix test, a
/// path that climbs out of `root` (`root/../sibling`) is rejected — the
/// traversal cannot smuggle past the check. A normalised candidate that
/// still begins with `..` (it escaped a relative base) is never within any
/// root.
///
/// ```
/// use std::path::Path;
/// use rstui_plugin_host::capability::is_within;
///
/// assert!(is_within(Path::new("/g/data"), Path::new("/g/data/sub/f")));
/// assert!(is_within(Path::new("/g/data"), Path::new("/g/data")));
/// assert!(!is_within(Path::new("/g/data"), Path::new("/g/data/../secret")));
/// assert!(!is_within(Path::new("/g/data"), Path::new("/g/database"))); // not a path-prefix
/// ```
#[must_use]
pub fn is_within(root: &Path, candidate: &Path) -> bool {
    let root = normalize_lexical(root);
    let candidate = normalize_lexical(candidate);
    // A normalised path that still starts with `..` escaped a relative base
    // and is not contained by anything.
    if candidate.components().next() == Some(Component::ParentDir) {
        return false;
    }
    // `Path::starts_with` is component-wise, so "/g/database" does not start
    // with "/g/data" — exactly the prefix semantics a subtree grant needs.
    candidate.starts_with(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_of_request_and_grant_agree() {
        let g = CapabilityGrant::Network {
            host: "h".into(),
            port: 443,
        };
        let r = CapabilityRequest::Network {
            host: "h".into(),
            port: 443,
        };
        assert_eq!(g.capability(), Capability::Network);
        assert_eq!(r.capability(), Capability::Network);
        assert_eq!(
            CapabilityRequest::Env { key: "X".into() }.capability(),
            Capability::Env
        );
        assert_eq!(
            CapabilityRequest::Command {
                program: "git".into(),
                args: vec![]
            }
            .capability(),
            Capability::Command
        );
        assert_eq!(
            CapabilityRequest::Filesystem {
                mode: FsMode::Read,
                path: "/x".into()
            }
            .capability(),
            Capability::Filesystem
        );
    }

    #[test]
    fn display_is_stable_for_reports() {
        assert_eq!(Capability::Filesystem.to_string(), "filesystem");
        assert_eq!(Capability::Network.to_string(), "network");
        assert_eq!(Capability::Command.to_string(), "command");
        assert_eq!(Capability::Env.to_string(), "env");
        assert_eq!(FsMode::Read.to_string(), "read");
        assert_eq!(FsMode::Write.to_string(), "write");
    }

    #[test]
    fn normalize_drops_curdir_and_resolves_parent() {
        assert_eq!(normalize_lexical(Path::new("a/./b/../c")), Path::new("a/c"));
        assert_eq!(
            normalize_lexical(Path::new("./a/b/./c/")),
            Path::new("a/b/c")
        );
        assert_eq!(normalize_lexical(Path::new(".")), Path::new("."));
        assert_eq!(normalize_lexical(Path::new("")), Path::new("."));
    }

    #[test]
    fn normalize_cannot_climb_above_absolute_root() {
        assert_eq!(normalize_lexical(Path::new("/a/../../b")), Path::new("/b"));
        assert_eq!(normalize_lexical(Path::new("/../..")), Path::new("/"));
        assert_eq!(normalize_lexical(Path::new("/a/b/../..")), Path::new("/"));
    }

    #[test]
    fn normalize_preserves_escaping_relative_parent() {
        // A relative path that escapes its base keeps the leading `..` so it
        // stays detectably out-of-scope.
        assert_eq!(
            normalize_lexical(Path::new("../escapes")),
            Path::new("../escapes")
        );
        assert_eq!(normalize_lexical(Path::new("a/../../b")), Path::new("../b"));
    }

    #[test]
    fn is_within_allows_root_and_descendants() {
        assert!(is_within(Path::new("/g/data"), Path::new("/g/data")));
        assert!(is_within(Path::new("/g/data"), Path::new("/g/data/a/b")));
        assert!(is_within(
            Path::new("/g/data"),
            Path::new("/g/data/a/../b/./c")
        ));
    }

    #[test]
    fn is_within_rejects_traversal_and_sibling_prefixes() {
        // `..` escaping the root — the canonical attack.
        assert!(!is_within(
            Path::new("/g/data"),
            Path::new("/g/data/../../etc/passwd")
        ));
        assert!(!is_within(Path::new("/g/data"), Path::new("/g/secret")));
        // Component-wise prefix: a sibling that merely shares a string
        // prefix is not inside.
        assert!(!is_within(Path::new("/g/data"), Path::new("/g/database")));
        // An escaping relative candidate is within nothing.
        assert!(!is_within(Path::new("base"), Path::new("../outside")));
    }
}
