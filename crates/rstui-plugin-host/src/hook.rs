//! The hook extension-point vocabulary: *what* lifecycle/decision points a
//! plugin can subscribe to and *how* each one's reply reduces against host
//! control flow. This is ADR 0007 §6's deliberately deferred slice — the
//! reduction semantics ("which hook can veto, which chains") — built
//! **inside** the security boundary [ADR 0007](https://github.com/andymac4182/rstui/blob/main/docs/adr/0007-plugin-host-and-secure-execution.md)
//! already fixed, never widening it.
//!
//! This module is **self-contained and pure**: it defines the closed
//! [`HookKind`] set, each kind's fixed [`HookReduction`] strategy, and the
//! [`HookOutcome`] a plugin can return. It performs no IO and holds no host
//! state. The host (in [`host`](crate::host)) *dispatches* hooks; the
//! [`message`](crate::message) codec *frames* them. The only wire concern
//! here is the small, stable discriminant bytes the codec needs
//! ([`HookKind::to_byte`]/[`HookKind::from_byte`],
//! [`HookOutcome::to_byte`]/[`HookOutcome::from_byte`]) — no envelope, no
//! length framing, no host logic.
//!
//! # THE INVARIANT — a hook can only *narrow* authority, never widen it
//!
//! This is the entire reason hooks are safe to add "inside the boundary"
//! (ADR 0007 §6) and the security contract the host relies on:
//!
//! - [`HookKind::BeforeCapability`] is dispatched by the host **only after
//!   the [`PermissionPolicy`](crate::permission::PermissionPolicy) has
//!   already returned [`Allow`](crate::permission::Decision::Allow)** for the
//!   *canonicalised* request. A [`HookOutcome::Veto`] turns that
//!   already-permitted call into a denial — defense in depth on top of the
//!   policy. A hook can **never** convert a policy
//!   [`Deny`](crate::permission::Decision::Deny) into an allow: on the deny
//!   path the host does not even dispatch the hook, so there is no reply for
//!   it to consult. A hook adds a second lock; it can never pick the first.
//! - [`HookReduction::Observe`] hooks
//!   ([`SessionStart`](HookKind::SessionStart)/[`SessionEnd`](HookKind::SessionEnd))
//!   cannot influence control flow **whatsoever** — their reply is ignored.
//!
//! Stated as a one-liner the codec and host both depend on: the codomain of
//! every hook is "proceed, or deny something already permitted" — never
//! "permit something denied".
//!
//! # The chain (future, non-breaking)
//!
//! With a single plugin there is no "chain" yet. The
//! [`HookReduction::VetoChain`] name reflects the *intended* multi-plugin
//! reduction — dispatch each subscribed plugin in order, **first
//! [`Veto`](HookOutcome::Veto) wins and short-circuits** the rest — which is
//! a strict narrowing (more plugins can only add vetoes, never remove one)
//! and therefore a non-breaking future extension that does not weaken the
//! invariant above.

/// The closed set of points a plugin can hook (ADR 0007 §6). Closed on
/// purpose: a plugin cannot invent a new interception point, so the host
/// code that dispatches and reduces hooks is finite and auditable — the
/// same discipline as the closed [`Capability`](crate::capability::Capability)
/// set.
///
/// Each kind has a *fixed* [`reduction()`](HookKind::reduction): whether its
/// reply can affect control flow is a property of the kind, not a per-call
/// choice, so the narrow-only invariant (see the module docs) is structural.
///
/// ```
/// use rstui_plugin_host::hook::{HookKind, HookReduction};
///
/// // `BeforeCapability` is the security-relevant hook: its reply is
/// // consulted and a veto denies an *already-policy-permitted* call.
/// assert_eq!(
///     HookKind::BeforeCapability.reduction(),
///     HookReduction::VetoChain,
/// );
///
/// // The narrow-only invariant: the strongest thing any hook can do is
/// // turn a permitted action into a denial — never the reverse. A hook is
/// // dispatched *after* the policy already said Allow; it cannot widen.
/// // Lifecycle hooks cannot affect control flow at all.
/// assert_eq!(HookKind::SessionStart.reduction(), HookReduction::Observe);
/// assert_eq!(HookKind::SessionEnd.reduction(), HookReduction::Observe);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookKind {
    /// Fired **once** after the handshake completes, before any capability
    /// call is dispatched. A lifecycle notification ([`HookReduction::Observe`]):
    /// the plugin's reply is ignored and cannot block the session.
    SessionStart,
    /// Fired before the host performs an **already-policy-permitted**
    /// capability call — the security-relevant hook. Reduces by
    /// [`HookReduction::VetoChain`]: a [`HookOutcome::Veto`] turns that
    /// permitted call into a denial (defense in depth). It can only narrow:
    /// the host never dispatches this on the policy-deny path, so a hook can
    /// never convert a [`Deny`](crate::permission::Decision::Deny) into an
    /// allow.
    BeforeCapability,
    /// Fired **once** when the plugin run is ending. A lifecycle
    /// notification ([`HookReduction::Observe`]): the reply is ignored — a
    /// run that is already ending cannot be vetoed back to life.
    SessionEnd,
}

impl HookKind {
    /// The stable wire/manifest name: `"session_start"` /
    /// `"before_capability"` / `"session_end"`.
    ///
    /// This single spelling is used **both** in the manifest `[hooks]`
    /// `subscribe = "..."` declaration and as the human-facing name, so the
    /// two cannot drift.
    #[must_use]
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::BeforeCapability => "before_capability",
            Self::SessionEnd => "session_end",
        }
    }

    /// Parse a manifest/wire name back to a [`HookKind`], **exact match
    /// only**.
    ///
    /// An unknown name yields `None`. Callers (the manifest parser) treat
    /// `None` as a **hard manifest error and fail closed** — an unrecognised
    /// hook subscription is never silently ignored, mirroring the rest of
    /// the fail-closed manifest grammar (ADR 0007 §2).
    #[must_use]
    pub fn from_wire_name(s: &str) -> Option<Self> {
        match s {
            "session_start" => Some(Self::SessionStart),
            "before_capability" => Some(Self::BeforeCapability),
            "session_end" => Some(Self::SessionEnd),
            _ => None,
        }
    }

    /// The stable discriminant byte for the [`message`](crate::message)
    /// codec.
    ///
    /// The values are **fixed protocol constants** and must never change
    /// (the exhaustive test asserts the literals so a reorder cannot
    /// silently shift them):
    ///
    /// - [`SessionStart`](HookKind::SessionStart) ⇒ `0x01`
    /// - [`BeforeCapability`](HookKind::BeforeCapability) ⇒ `0x02`
    /// - [`SessionEnd`](HookKind::SessionEnd) ⇒ `0x03`
    #[must_use]
    pub fn to_byte(self) -> u8 {
        match self {
            Self::SessionStart => 0x01,
            Self::BeforeCapability => 0x02,
            Self::SessionEnd => 0x03,
        }
    }

    /// Decode a discriminant byte produced by [`to_byte`](HookKind::to_byte).
    ///
    /// An unknown byte yields `None`; the codec treats that as a fatal
    /// framing error and **terminates the connection — no
    /// skip-and-continue** (ADR 0007 §4 fail-closed rule).
    #[must_use]
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x01 => Some(Self::SessionStart),
            0x02 => Some(Self::BeforeCapability),
            0x03 => Some(Self::SessionEnd),
            _ => None,
        }
    }

    /// This kind's **fixed** reduction strategy — the property that makes
    /// the narrow-only invariant structural rather than a runtime choice
    /// (see the module docs):
    ///
    /// - [`SessionStart`](HookKind::SessionStart) /
    ///   [`SessionEnd`](HookKind::SessionEnd) ⇒ [`HookReduction::Observe`]
    ///   (lifecycle notifications, reply ignored).
    /// - [`BeforeCapability`](HookKind::BeforeCapability) ⇒
    ///   [`HookReduction::VetoChain`] (the security-relevant decision hook).
    #[must_use]
    pub fn reduction(self) -> HookReduction {
        match self {
            Self::SessionStart | Self::SessionEnd => HookReduction::Observe,
            Self::BeforeCapability => HookReduction::VetoChain,
        }
    }
}

/// How a [`HookKind`]'s reply reduces against host control flow. A kind's
/// strategy is fixed by [`HookKind::reduction`]; this enum just names the
/// two possible strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookReduction {
    /// The plugin's reply is **ignored**: the hook cannot affect control
    /// flow at all (it is a pure lifecycle notification). The result is
    /// **not consulted** by the host.
    ///
    /// A malformed or absent reply for an `Observe` hook is still a protocol
    /// error for the *host* to handle (every framing/decode error terminates
    /// the connection, ADR 0007 §4) — this module only fixes the *semantics*
    /// that the reply, when present, does not influence anything.
    Observe,
    /// The plugin's reply **is** consulted: a [`HookOutcome::Veto`] denies
    /// the pending (already-policy-permitted) action;
    /// [`HookOutcome::Continue`] lets it proceed.
    ///
    /// With one plugin there is no "chain" yet; the name reflects the
    /// intended multi-plugin reduction — dispatch subscribers in order,
    /// **first [`Veto`](HookOutcome::Veto) wins, short-circuit** the rest —
    /// a strict narrowing and therefore a non-breaking future extension (see
    /// the module docs).
    VetoChain,
}

/// What a plugin returns from a [`HookReduction::VetoChain`] hook.
///
/// # The narrow-only invariant
///
/// This type is the codomain of every consulted hook, and that codomain is
/// the whole security argument: the strongest value here,
/// [`Veto`](HookOutcome::Veto), turns an **already-policy-permitted** action
/// into a denial (defense in depth). There is **no variant that permits
/// something the policy denied** — and the host only ever dispatches a
/// [`HookKind::BeforeCapability`] hook *after* the
/// [`PermissionPolicy`](crate::permission::PermissionPolicy) returned
/// [`Allow`](crate::permission::Decision::Allow), never on the deny path. A
/// hook can only ever **narrow** authority, never widen it (ADR 0007 §6; see
/// the module docs).
///
/// For an [`Observe`](HookReduction::Observe) hook this value, if returned at
/// all, is **ignored**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookOutcome {
    /// Let the pending action proceed. (For an
    /// [`Observe`](HookReduction::Observe) hook this is also the no-op
    /// "acknowledged" reply; either way the action is unaffected by the
    /// hook.)
    Continue,
    /// Deny the pending (already-policy-permitted) action. `reason` is
    /// surfaced in the host's audit record so a defense-in-depth denial is
    /// attributable. Only meaningful for a [`HookReduction::VetoChain`]
    /// hook; never widens authority (see the type docs).
    Veto {
        /// Operator-facing explanation for the veto, recorded by the host.
        reason: String,
    },
}

impl HookOutcome {
    /// Whether this outcome denies the pending action (i.e. is a
    /// [`Veto`](HookOutcome::Veto)).
    #[must_use]
    pub fn is_veto(&self) -> bool {
        matches!(self, Self::Veto { .. })
    }

    /// The veto reason, or `None` for [`Continue`](HookOutcome::Continue).
    #[must_use]
    pub fn veto_reason(&self) -> Option<&str> {
        match self {
            Self::Continue => None,
            Self::Veto { reason } => Some(reason),
        }
    }

    /// The stable discriminant byte for the [`message`](crate::message)
    /// codec.
    ///
    /// **Fixed protocol constants** (the exhaustive test asserts the
    /// literals so they cannot silently change):
    ///
    /// - [`Continue`](HookOutcome::Continue) ⇒ `0x00`
    /// - [`Veto`](HookOutcome::Veto) ⇒ `0x01`
    ///
    /// The `reason` string is **length-framed by the codec**, not encoded
    /// here — this byte is only the variant tag.
    #[must_use]
    pub fn to_byte(&self) -> u8 {
        match self {
            Self::Continue => 0x00,
            Self::Veto { .. } => 0x01,
        }
    }

    /// Decode a discriminant byte produced by
    /// [`to_byte`](HookOutcome::to_byte).
    ///
    /// Returns the variant *tag* only; for [`Veto`](HookOutcome::Veto) the
    /// `reason` is filled with an empty string because the reason bytes are
    /// framed separately by the codec (the codec replaces it with the
    /// decoded string). An unknown byte yields `None`, which the codec
    /// treats as a fatal framing error — **no skip-and-continue** (ADR 0007
    /// §4 fail-closed rule).
    #[must_use]
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Self::Continue),
            0x01 => Some(Self::Veto {
                reason: String::new(),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind in the closed set, so a new variant cannot be added
    /// without the round-trip/invariant tests covering it too.
    const ALL_KINDS: [HookKind; 3] = [
        HookKind::SessionStart,
        HookKind::BeforeCapability,
        HookKind::SessionEnd,
    ];

    #[test]
    fn wire_name_round_trips_for_every_kind() {
        for kind in ALL_KINDS {
            assert_eq!(
                HookKind::from_wire_name(kind.wire_name()),
                Some(kind),
                "wire_name round-trip failed for {kind:?}",
            );
        }
        // The exact spellings are part of the manifest grammar and the
        // protocol — pin them so a rename is a visible, deliberate break.
        assert_eq!(HookKind::SessionStart.wire_name(), "session_start");
        assert_eq!(HookKind::BeforeCapability.wire_name(), "before_capability");
        assert_eq!(HookKind::SessionEnd.wire_name(), "session_end");
    }

    #[test]
    fn unknown_wire_name_is_none_so_callers_fail_closed() {
        assert_eq!(HookKind::from_wire_name(""), None);
        assert_eq!(HookKind::from_wire_name("SessionStart"), None); // exact match: case matters
        assert_eq!(HookKind::from_wire_name("session-start"), None); // exact match: separator matters
        assert_eq!(HookKind::from_wire_name("before_capabilities"), None);
        assert_eq!(HookKind::from_wire_name("after_capability"), None);
    }

    #[test]
    fn to_byte_from_byte_round_trips_with_pinned_constants() {
        for kind in ALL_KINDS {
            assert_eq!(
                HookKind::from_byte(kind.to_byte()),
                Some(kind),
                "byte round-trip failed for {kind:?}",
            );
        }
        // Assert the literal stable discriminant values so a variant
        // reorder cannot silently shift the wire protocol.
        assert_eq!(HookKind::SessionStart.to_byte(), 0x01);
        assert_eq!(HookKind::BeforeCapability.to_byte(), 0x02);
        assert_eq!(HookKind::SessionEnd.to_byte(), 0x03);
    }

    #[test]
    fn unknown_hook_kind_byte_is_none_so_codec_fails_closed() {
        assert_eq!(HookKind::from_byte(0x00), None);
        assert_eq!(HookKind::from_byte(0x04), None);
        assert_eq!(HookKind::from_byte(0xFF), None);
        // Every byte outside the three documented constants is rejected.
        for b in 0u8..=255 {
            let known = matches!(b, 0x01..=0x03);
            assert_eq!(
                HookKind::from_byte(b).is_some(),
                known,
                "byte 0x{b:02X} acceptance disagrees with the documented set",
            );
        }
    }

    #[test]
    fn reduction_strategy_is_fixed_per_kind() {
        // Lifecycle notifications cannot influence control flow.
        assert_eq!(HookKind::SessionStart.reduction(), HookReduction::Observe);
        assert_eq!(HookKind::SessionEnd.reduction(), HookReduction::Observe);
        // The security-relevant decision hook is consulted.
        assert_eq!(
            HookKind::BeforeCapability.reduction(),
            HookReduction::VetoChain,
        );
        // Stated as the invariant: exactly one kind can affect control flow,
        // and the strongest thing it can do is veto (narrow), never widen.
        let vetoers: Vec<_> = ALL_KINDS
            .into_iter()
            .filter(|k| k.reduction() == HookReduction::VetoChain)
            .collect();
        assert_eq!(vetoers, vec![HookKind::BeforeCapability]);
    }

    #[test]
    fn hook_outcome_is_veto_and_veto_reason_agree() {
        let cont = HookOutcome::Continue;
        assert!(!cont.is_veto());
        assert_eq!(cont.veto_reason(), None);

        let veto = HookOutcome::Veto {
            reason: "path not in audited set".to_string(),
        };
        assert!(veto.is_veto());
        assert_eq!(veto.veto_reason(), Some("path not in audited set"));

        // An empty reason is still a veto (the action is denied; only the
        // explanation is blank).
        let bare = HookOutcome::Veto {
            reason: String::new(),
        };
        assert!(bare.is_veto());
        assert_eq!(bare.veto_reason(), Some(""));
    }

    #[test]
    fn hook_outcome_byte_round_trips_with_pinned_constants() {
        // Tag-only round-trip: the reason is framed by the codec, so
        // `from_byte(to_byte(Veto))` yields a Veto with an empty reason.
        assert_eq!(
            HookOutcome::from_byte(HookOutcome::Continue.to_byte()),
            Some(HookOutcome::Continue),
        );
        let veto = HookOutcome::Veto {
            reason: "ignored on the tag round-trip".to_string(),
        };
        let decoded = HookOutcome::from_byte(veto.to_byte());
        assert_eq!(
            decoded,
            Some(HookOutcome::Veto {
                reason: String::new(),
            }),
        );
        assert!(decoded.expect("known tag decodes").is_veto());

        // Pin the literal stable discriminant values.
        assert_eq!(HookOutcome::Continue.to_byte(), 0x00);
        assert_eq!(
            HookOutcome::Veto {
                reason: "any".to_string()
            }
            .to_byte(),
            0x01,
        );
    }

    #[test]
    fn unknown_hook_outcome_byte_is_none_so_codec_fails_closed() {
        assert_eq!(HookOutcome::from_byte(0x02), None);
        assert_eq!(HookOutcome::from_byte(0xFF), None);
        for b in 0u8..=255 {
            let known = matches!(b, 0x00..=0x01);
            assert_eq!(
                HookOutcome::from_byte(b).is_some(),
                known,
                "byte 0x{b:02X} acceptance disagrees with the documented set",
            );
        }
    }
}
