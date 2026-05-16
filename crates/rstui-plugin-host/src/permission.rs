//! The host's grant decision over a *canonicalised* capability request
//! (ADR 0007 §2/§3).
//!
//! The split this module encodes: a [`PluginManifest`] is the plugin's
//! *request* for authority; a [`PermissionPolicy`] is the host's *grant*.
//! The shipped [`ManifestPolicy`] grants **exactly** what the manifest
//! declared and nothing else — default-deny: a request that matches no
//! declared [`CapabilityGrant`] is refused with a reason, never allowed by
//! omission (this is secure-exec's "deny if no checker present", not "allow
//! if unspecified").
//!
//! ## The canonicalisation precondition (ADR 0007 §3)
//!
//! [`PermissionPolicy::check`] assumes its [`CapabilityRequest`] is already
//! *canonical*: in particular a [`CapabilityRequest::Filesystem`] `path` must
//! have been made absolute and [`normalize_lexical`](crate::capability::normalize_lexical)-d
//! **by the host, before the policy sees it**. The policy then does pure,
//! filesystem-free containment checks ([`is_within`]) — so a plugin cannot
//! defeat a path grant with `..`, and the decision is deterministic and
//! TOCTOU-free. A policy is a trait so an operator can wrap or replace the
//! default (e.g. add interactive `ask`, audit logging — [`RecordingPolicy`]
//! is the test-facing instance of that pattern).

use std::sync::Mutex;

use crate::capability::{CapabilityGrant, CapabilityRequest, is_within};
use crate::manifest::PluginManifest;

/// The host's ruling on one [`CapabilityRequest`].
///
/// `Deny` always carries an operator-readable `reason` (the host logs it and
/// returns it to the plugin as the capability-call failure), so a refusal is
/// never silent or unexplained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The host will perform the requested effect.
    Allow,
    /// The host refuses; `reason` explains why (no matching grant, wrong
    /// mode, path escapes the granted subtree, …).
    Deny {
        /// Operator-readable explanation of the refusal.
        reason: String,
    },
}

impl Decision {
    /// Whether this is an [`Allow`](Decision::Allow).
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }

    /// The denial reason, or `None` if this is an `Allow`.
    #[must_use]
    pub fn denied_reason(&self) -> Option<&str> {
        match self {
            Self::Allow => None,
            Self::Deny { reason } => Some(reason),
        }
    }
}

/// The host-side authority that decides whether a plugin's
/// [`CapabilityRequest`] is permitted.
///
/// Implementors must be `Send + Sync` (a single policy is shared across the
/// host's mediation of every plugin) and must treat the request as already
/// canonical (see the [module docs](self)). Default-deny is a contract, not
/// a suggestion: return [`Decision::Deny`] for anything not explicitly
/// granted.
pub trait PermissionPolicy: Send + Sync {
    /// Rule on `request`. The host calls this *after* canonicalising the
    /// request and *before* performing any effect.
    fn check(&self, request: &CapabilityRequest) -> Decision;
}

/// The default policy: grants precisely the [`CapabilityGrant`]s a manifest
/// declared, and nothing else (ADR 0007 §2).
///
/// Matching is exact and least-privilege:
/// - **Filesystem**: the request's [`FsMode`](crate::capability::FsMode) must
///   equal the grant's (a `write` grant does not imply `read`), and the
///   request path must be [`is_within`] the grant's normalised `root`.
/// - **Network**: host string and port must match exactly.
/// - **Command**: `program` must match exactly and the request's `args` must
///   begin with the grant's `args_prefix` (an empty prefix allows any args).
/// - **Env**: the variable name must match exactly.
pub struct ManifestPolicy {
    grants: Vec<CapabilityGrant>,
}

impl ManifestPolicy {
    /// A policy granting exactly `grants`.
    #[must_use]
    pub fn new(grants: Vec<CapabilityGrant>) -> Self {
        Self { grants }
    }

    /// A policy granting exactly what `manifest` declared.
    ///
    /// ```
    /// use rstui_plugin_host::manifest::PluginManifest;
    /// use rstui_plugin_host::permission::{ManifestPolicy, PermissionPolicy};
    /// use rstui_plugin_host::capability::{CapabilityRequest, FsMode};
    /// use std::path::PathBuf;
    ///
    /// let manifest = PluginManifest::parse(concat!(
    ///     "name = \"demo\"\n",
    ///     "version = \"0.1.0\"\n",
    ///     "api_version = \"1\"\n",
    ///     "entry = \"bin/demo\"\n",
    ///     "[filesystem]\n",
    ///     "read = \"/srv/demo/data\"\n",
    /// )).unwrap();
    /// let policy = ManifestPolicy::from_manifest(&manifest);
    ///
    /// // A read inside the granted subtree is allowed...
    /// assert!(policy.check(&CapabilityRequest::Filesystem {
    ///     mode: FsMode::Read,
    ///     path: PathBuf::from("/srv/demo/data/notes.txt"),
    ///     contents: Vec::new(),
    /// }).is_allowed());
    ///
    /// // ...a write is not (the grant was read-only), and the refusal
    /// // explains itself.
    /// let denied = policy.check(&CapabilityRequest::Filesystem {
    ///     mode: FsMode::Write,
    ///     path: PathBuf::from("/srv/demo/data/notes.txt"),
    ///     contents: Vec::new(),
    /// });
    /// assert!(!denied.is_allowed());
    /// assert!(denied.denied_reason().is_some());
    /// ```
    #[must_use]
    pub fn from_manifest(manifest: &PluginManifest) -> Self {
        Self {
            grants: manifest.grants.clone(),
        }
    }
}

impl PermissionPolicy for ManifestPolicy {
    fn check(&self, request: &CapabilityRequest) -> Decision {
        if self
            .grants
            .iter()
            .any(|grant| grant_permits(grant, request))
        {
            Decision::Allow
        } else {
            Decision::Deny {
                reason: format!(
                    "no declared {} grant permits {}",
                    request.capability(),
                    describe(request)
                ),
            }
        }
    }
}

/// Whether `grant` permits `request`. Pure, filesystem-free (the request is
/// assumed canonical — see the module docs).
fn grant_permits(grant: &CapabilityGrant, request: &CapabilityRequest) -> bool {
    match (grant, request) {
        (
            CapabilityGrant::Filesystem {
                mode: grant_mode,
                root,
            },
            CapabilityRequest::Filesystem {
                mode: request_mode,
                path,
                // Write data never widens authority: the policy scopes by
                // path + mode only (ADR 0007 §3).
                ..
            },
        ) => grant_mode == request_mode && is_within(root, path),
        (
            CapabilityGrant::Network {
                host: grant_host,
                port: grant_port,
            },
            CapabilityRequest::Network {
                host: request_host,
                port: request_port,
            },
        ) => grant_host == request_host && grant_port == request_port,
        (
            CapabilityGrant::Command {
                program: grant_program,
                args_prefix,
            },
            CapabilityRequest::Command {
                program: request_program,
                args,
            },
        ) => {
            grant_program == request_program
                && args.len() >= args_prefix.len()
                && args[..args_prefix.len()] == args_prefix[..]
        }
        (CapabilityGrant::Env { key: grant_key }, CapabilityRequest::Env { key: request_key }) => {
            grant_key == request_key
        }
        // Different capability kinds never match.
        _ => false,
    }
}

/// A short, deterministic description of a request for a denial reason.
fn describe(request: &CapabilityRequest) -> String {
    match request {
        CapabilityRequest::Filesystem { mode, path, .. } => {
            format!("{mode} of {}", path.display())
        }
        CapabilityRequest::Network { host, port } => format!("connect to {host}:{port}"),
        CapabilityRequest::Command { program, args } => {
            if args.is_empty() {
                format!("run `{program}`")
            } else {
                format!("run `{program} {}`", args.join(" "))
            }
        }
        CapabilityRequest::Env { key } => format!("read env `{key}`"),
    }
}

/// A [`PermissionPolicy`] decorator that delegates to an inner policy and
/// **records every `(request, decision)` pair** it saw, in order.
///
/// This is the test-facing instance of the "wrap the policy" extension point
/// (ADR 0007 §2): a security test asserts not just the returned
/// [`Decision`] but that the host actually consulted the policy for the
/// request — the precondition for proving "a denied request never reached
/// the host effect".
pub struct RecordingPolicy<P: PermissionPolicy> {
    inner: P,
    log: Mutex<Vec<(CapabilityRequest, Decision)>>,
}

impl<P: PermissionPolicy> RecordingPolicy<P> {
    /// Wrap `inner`, recording every decision it makes.
    pub fn new(inner: P) -> Self {
        Self {
            inner,
            log: Mutex::new(Vec::new()),
        }
    }

    /// Every `(request, decision)` pair seen so far, in call order.
    #[must_use]
    pub fn records(&self) -> Vec<(CapabilityRequest, Decision)> {
        self.log.lock().expect("recording log poisoned").clone()
    }

    /// How many requests have been checked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.log.lock().expect("recording log poisoned").len()
    }

    /// Whether no request has been checked yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.log.lock().expect("recording log poisoned").is_empty()
    }
}

impl<P: PermissionPolicy> PermissionPolicy for RecordingPolicy<P> {
    fn check(&self, request: &CapabilityRequest) -> Decision {
        let decision = self.inner.check(request);
        self.log
            .lock()
            .expect("recording log poisoned")
            .push((request.clone(), decision.clone()));
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::FsMode;
    use std::path::PathBuf;

    fn fs_grant(mode: FsMode, root: &str) -> CapabilityGrant {
        CapabilityGrant::Filesystem {
            mode,
            root: PathBuf::from(root),
        }
    }

    #[test]
    fn filesystem_mode_is_exact_and_path_is_scoped() {
        let policy = ManifestPolicy::new(vec![fs_grant(FsMode::Read, "/g/data")]);

        // Allowed: read within the granted subtree.
        assert!(
            policy
                .check(&CapabilityRequest::Filesystem {
                    mode: FsMode::Read,
                    path: PathBuf::from("/g/data/sub/file"),
                    contents: Vec::new(),
                })
                .is_allowed()
        );

        // Denied: write, when only read was granted (no implicit upgrade).
        assert!(
            !policy
                .check(&CapabilityRequest::Filesystem {
                    mode: FsMode::Write,
                    path: PathBuf::from("/g/data/sub/file"),
                    contents: Vec::new(),
                })
                .is_allowed()
        );

        // Denied: a `..` that escapes the granted root — the canonical attack.
        let escape = policy.check(&CapabilityRequest::Filesystem {
            mode: FsMode::Read,
            path: PathBuf::from("/g/data/../secret"),
            contents: Vec::new(),
        });
        assert!(!escape.is_allowed());
        assert!(escape.denied_reason().unwrap().contains("filesystem"));

        // Denied: sibling sharing a string (not path) prefix.
        assert!(
            !policy
                .check(&CapabilityRequest::Filesystem {
                    mode: FsMode::Read,
                    path: PathBuf::from("/g/database/x"),
                    contents: Vec::new(),
                })
                .is_allowed()
        );
    }

    #[test]
    fn network_requires_exact_host_and_port() {
        let policy = ManifestPolicy::new(vec![CapabilityGrant::Network {
            host: "api.example.com".into(),
            port: 443,
        }]);
        assert!(
            policy
                .check(&CapabilityRequest::Network {
                    host: "api.example.com".into(),
                    port: 443
                })
                .is_allowed()
        );
        assert!(
            !policy
                .check(&CapabilityRequest::Network {
                    host: "api.example.com".into(),
                    port: 80
                })
                .is_allowed()
        );
        assert!(
            !policy
                .check(&CapabilityRequest::Network {
                    host: "evil.example.com".into(),
                    port: 443
                })
                .is_allowed()
        );
    }

    #[test]
    fn command_matches_program_and_arg_prefix() {
        let policy = ManifestPolicy::new(vec![CapabilityGrant::Command {
            program: "git".into(),
            args_prefix: vec!["log".into()],
        }]);
        // Prefix satisfied, extra args allowed.
        assert!(
            policy
                .check(&CapabilityRequest::Command {
                    program: "git".into(),
                    args: vec!["log".into(), "--oneline".into()],
                })
                .is_allowed()
        );
        // Prefix not satisfied.
        assert!(
            !policy
                .check(&CapabilityRequest::Command {
                    program: "git".into(),
                    args: vec!["push".into()],
                })
                .is_allowed()
        );
        // Args shorter than the required prefix.
        assert!(
            !policy
                .check(&CapabilityRequest::Command {
                    program: "git".into(),
                    args: vec![],
                })
                .is_allowed()
        );
        // Wrong program.
        assert!(
            !policy
                .check(&CapabilityRequest::Command {
                    program: "rm".into(),
                    args: vec!["log".into()],
                })
                .is_allowed()
        );
    }

    #[test]
    fn empty_arg_prefix_allows_any_args() {
        let policy = ManifestPolicy::new(vec![CapabilityGrant::Command {
            program: "ls".into(),
            args_prefix: vec![],
        }]);
        assert!(
            policy
                .check(&CapabilityRequest::Command {
                    program: "ls".into(),
                    args: vec!["-la".into(), "/".into()],
                })
                .is_allowed()
        );
    }

    #[test]
    fn env_is_exact_and_absent_capability_is_denied_by_default() {
        let policy = ManifestPolicy::new(vec![CapabilityGrant::Env { key: "PATH".into() }]);
        assert!(
            policy
                .check(&CapabilityRequest::Env { key: "PATH".into() })
                .is_allowed()
        );
        assert!(
            !policy
                .check(&CapabilityRequest::Env {
                    key: "SECRET".into()
                })
                .is_allowed()
        );
        // A capability kind the manifest never declared at all is denied by
        // omission, with an explanatory reason.
        let denied = policy.check(&CapabilityRequest::Network {
            host: "x".into(),
            port: 1,
        });
        assert_eq!(
            denied,
            Decision::Deny {
                reason: "no declared network grant permits connect to x:1".into()
            }
        );
    }

    #[test]
    fn empty_policy_denies_everything() {
        let policy = ManifestPolicy::new(vec![]);
        assert!(
            !policy
                .check(&CapabilityRequest::Env { key: "PATH".into() })
                .is_allowed()
        );
    }

    #[test]
    fn recording_policy_logs_every_request_and_decision_in_order() {
        let inner = ManifestPolicy::new(vec![CapabilityGrant::Env { key: "PATH".into() }]);
        let policy = RecordingPolicy::new(inner);
        assert!(policy.is_empty());

        let allowed = CapabilityRequest::Env { key: "PATH".into() };
        let denied = CapabilityRequest::Env {
            key: "SECRET".into(),
        };
        assert!(policy.check(&allowed).is_allowed());
        assert!(!policy.check(&denied).is_allowed());

        let records = policy.records();
        assert_eq!(policy.len(), 2);
        assert_eq!(records[0].0, allowed);
        assert_eq!(records[0].1, Decision::Allow);
        assert_eq!(records[1].0, denied);
        assert!(!records[1].1.is_allowed());
    }

    #[test]
    fn from_manifest_grants_exactly_what_was_declared() {
        let manifest = PluginManifest::parse(concat!(
            "name = \"p\"\n",
            "version = \"0.1.0\"\n",
            "api_version = \"1\"\n",
            "entry = \"bin/p\"\n",
            "[command]\n",
            "allow = \"git status\"\n",
        ))
        .unwrap();
        let policy = ManifestPolicy::from_manifest(&manifest);
        assert!(
            policy
                .check(&CapabilityRequest::Command {
                    program: "git".into(),
                    args: vec!["status".into()],
                })
                .is_allowed()
        );
        assert!(
            !policy
                .check(&CapabilityRequest::Command {
                    program: "git".into(),
                    args: vec!["commit".into()],
                })
                .is_allowed()
        );
    }
}
