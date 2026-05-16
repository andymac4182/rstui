//! Application/widget focus as caller-owned model state.
//!
//! Per [ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md),
//! rstui never tracks focus in the runtime or inside widgets. The single
//! source of truth for *which widget the keyboard is aimed at* is a value in
//! the application's own model, mutated only by `update` and read by the pure
//! `view` to project a `focused: bool` (or an id comparison) into each widget.
//! This is forced by rstui's pure-`view` / immediate-mode design — a widget is
//! handed only a [`Buffer`](crate::buffer::Buffer) at render time and can
//! neither own nor read focus — and it is exactly the contract the
//! `Checkbox`/`Button`/`Radio` form controls already expose.
//!
//! This module is **optional**. An app may model focus with its own `enum` and
//! pass `focused: bool` into widgets with no type from here at all (the
//! zero-framework floor ratatui apps use). [`FocusId`] and [`FocusRing`] exist
//! only to turn the focus-order plus wrapping-traversal boilerplate every such
//! app re-derives into one reusable, panic-free primitive:
//!
//! - [`FocusId`] is an opaque, [`Copy`], value-identity token the app mints (a
//!   `const` per focusable, or from a counter). "Is this widget focused?" is
//!   then a cheap `==`.
//! - [`FocusRing`] is a pure value type — an explicit ordered list of
//!   `FocusId`s plus the currently-focused one — that lives as a *field in the
//!   app's model*. `update` calls [`focus`](FocusRing::focus) /
//!   [`focus_next`](FocusRing::focus_next) /
//!   [`focus_prev`](FocusRing::focus_prev) in response to click / `Tab` /
//!   `Shift+Tab` messages the app maps; `view` reads
//!   [`is_focused`](FocusRing::is_focused). It is never runtime-owned, never
//!   ambient, never in the view's mutable path.
//!
//! Focus *order is explicit data*, never derived from a widget tree: rstui is
//! immediate-mode and has no retained tree, so the ring *is* the order — a
//! deliberate divergence from OpenTUI/GPUI recorded in ADR 0004.
//!
//! ## Modal focus scopes
//!
//! A modal, overlay, or command palette must *trap* focus: while it is open,
//! `Tab` cycles only its own controls and a background widget cannot be
//! focused. Per ADR 0004 §6 that is also caller-owned model state, so
//! [`FocusRing`] carries a **scope stack**. `update` calls
//! [`push_scope`](FocusRing::push_scope) when a modal opens (passing the
//! modal's own [`FocusId`]s) and [`pop_scope`](FocusRing::pop_scope) when it
//! closes. While a scope is active every traversal/lookup
//! ([`focus`](FocusRing::focus), [`focus_next`](FocusRing::focus_next),
//! [`focus_prev`](FocusRing::focus_prev), [`contains`](FocusRing::contains),
//! [`len`](FocusRing::len)) is constrained to that scope's ids, so the global
//! `FocusTrapManager` + bounded retry loop a retained-mode framework needs
//! collapses to a single push. Pushing **captures** the previously-focused id
//! and popping **validate-restores** it (only if it is still registered;
//! otherwise focus falls back to the now-active set's first id) — the
//! pure-model form of an unmount-safe focus restore. The reducer gates
//! background key handling on [`in_scope`](FocusRing::in_scope) /
//! [`scope_depth`](FocusRing::scope_depth) (declarative modal trapping — the
//! runtime never sinks input).
//!
//! This widget/app focus is a **different concept** from terminal-window focus
//! (`Event::FocusGained` / `Event::FocusLost`, the OS telling the program its
//! window gained or lost focus); the two never share a type or a name.
//!
//! # Example
//!
//! ```
//! use rstui_core::focus::{FocusId, FocusRing};
//!
//! // The app mints a stable id per focusable widget (here as consts).
//! const NAME: FocusId = FocusId::new(0);
//! const EMAIL: FocusId = FocusId::new(1);
//! const SUBMIT: FocusId = FocusId::new(2);
//!
//! // Focus lives in the app's model as plain, ordered data. The ring focuses
//! // its first id by default, so something is focused whenever anything is.
//! let mut ring = FocusRing::with_ids([NAME, EMAIL, SUBMIT]);
//! assert_eq!(ring.focused(), Some(NAME));
//!
//! // `update` maps a `Tab` message to `focus_next` (wrapping and total):
//! ring.focus_next();
//! assert!(ring.is_focused(EMAIL));
//! ring.focus_prev();
//! ring.focus_prev();
//! assert!(ring.is_focused(SUBMIT)); // EMAIL -> NAME -> wrap -> SUBMIT
//!
//! // Click-to-focus is the app mapping a position to a known id:
//! ring.focus(SUBMIT);
//! assert_eq!(ring.focused(), Some(SUBMIT));
//!
//! // Focusing an id that is not registered is a no-op — the property that
//! // makes "restore the previously-focused id only if it still exists" safe.
//! ring.focus(FocusId::new(99));
//! assert_eq!(ring.focused(), Some(SUBMIT));
//!
//! // A modal opens: `update` pushes a scope of the modal's own ids. Focus is
//! // captured (SUBMIT) and trapped — `Tab` cycles only the modal, and a
//! // background id can no longer be focused.
//! const OK: FocusId = FocusId::new(10);
//! const CANCEL: FocusId = FocusId::new(11);
//! ring.push_scope([OK, CANCEL]);
//! assert!(ring.in_scope()); // the reducer traps background keys on this
//! assert_eq!(ring.focused(), Some(OK));
//! ring.focus_next();
//! ring.focus_next(); // wraps within the modal, never escapes to NAME
//! assert!(ring.is_focused(OK));
//! ring.focus(NAME); // a background id: no-op while trapped
//! assert!(ring.is_focused(OK));
//!
//! // The modal closes: popping validate-restores the captured focus.
//! ring.pop_scope();
//! assert!(!ring.in_scope());
//! assert_eq!(ring.focused(), Some(SUBMIT));
//! ```

/// An opaque, value-identity focus token the application mints.
///
/// A `FocusId` is a stable identity key for one focusable widget — a newtype
/// over a small integer with no inherent meaning or order. Mint one per
/// focusable as a `const` (`const SAVE: FocusId = FocusId::new(0);`) or from a
/// monotonic counter. Identity is by value, so "is this widget focused?" is a
/// cheap `==` against [`FocusRing::focused`].
///
/// It is deliberately **not** a bool and **not** a window-backed handle (rstui
/// has no window): the raw integer is private and carries no ordering — focus
/// *order* is the explicit [`FocusRing`] list, never the id values themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusId(u64);

impl FocusId {
    /// Mints the id with raw identity `raw`.
    ///
    /// `raw` is an opaque identity key, not a position: two `FocusId`s are the
    /// same focus target iff their `raw` values are equal. The app is
    /// responsible for keeping ids unique across the focusables it registers
    /// in one [`FocusRing`].
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }
}

/// An explicit, ordered ring of [`FocusId`]s plus the currently-focused one.
///
/// `FocusRing` is a **pure value type** designed to live as a field in the
/// application's model (it derives [`Default`] so it drops into a
/// `#[derive(Default)]` model as an empty ring). It owns *no* terminal,
/// runtime, or widget state: `update` mutates it in response to focus messages
/// the app maps (a `Tab` key, a click resolved to an id), and the pure `view`
/// only reads it. The framework never touches it.
///
/// Order is **explicit data**: the ring is exactly the sequence passed to
/// [`with_ids`](FocusRing::with_ids), in that order, and
/// [`focus_next`](FocusRing::focus_next) /
/// [`focus_prev`](FocusRing::focus_prev) step through it with wraparound.
/// Every method is **total** — no input, including ids that were never
/// registered, can panic — because focus order and ids are caller-owned (the
/// "a pure projection must be total" rule applied to focus).
///
/// Ids are expected to be unique within one ring (the caller's invariant, like
/// `Radio` group exclusivity); the ring stays total regardless and navigates
/// from the first occurrence of the focused id.
///
/// A modal/overlay traps focus by [`push_scope`](Self::push_scope)-ing its own
/// ids onto an internal scope stack; while a scope is active every
/// traversal/lookup is constrained to that scope (ADR 0004 §6 modal focus).
/// With no scope pushed the ring behaves exactly as an un-scoped ring, so this
/// is fully backward compatible.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusRing {
    order: Vec<FocusId>,
    focused: Option<FocusId>,
    /// Active modal focus scopes, innermost last. Empty = the base ring.
    scopes: Vec<FocusScope>,
}

/// One trapped focus context (a modal/overlay/command palette): the explicit
/// ordered ids that are focusable while it is active, plus the id that was
/// focused when it was pushed so [`FocusRing::pop_scope`] can validate-restore
/// it. Private — the caller pushes ids and never constructs this directly
/// (introduce a public type only when real API surface needs it).
#[derive(Debug, Clone, PartialEq, Eq)]
struct FocusScope {
    ids: Vec<FocusId>,
    saved: Option<FocusId>,
}

impl FocusRing {
    /// An empty ring: no focusables, nothing focused.
    ///
    /// [`focused`](Self::focused) is `None` and the traversal methods are
    /// no-ops until ids are supplied via [`with_ids`](Self::with_ids).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A ring over `ids`, in iteration order, with the **first** id focused
    /// (or nothing focused if `ids` is empty).
    ///
    /// Focusing the first id by default matches the usual TUI expectation that
    /// something is focused as soon as there is anything focusable, and is the
    /// boilerplate this primitive exists to remove.
    #[must_use]
    pub fn with_ids<I>(ids: I) -> Self
    where
        I: IntoIterator<Item = FocusId>,
    {
        let order: Vec<FocusId> = ids.into_iter().collect();
        let focused = order.first().copied();
        Self {
            order,
            focused,
            scopes: Vec::new(),
        }
    }

    /// The ids that are focusable *right now*: the innermost active scope, or
    /// the base ring when no scope is pushed.
    ///
    /// Every traversal/lookup routes through this single accessor so modal
    /// trapping is one concept, not re-implemented per method, and so an
    /// un-scoped ring is byte-for-byte the pre-scope behavior.
    fn active_ids(&self) -> &[FocusId] {
        self.scopes
            .last()
            .map_or(self.order.as_slice(), |scope| scope.ids.as_slice())
    }

    /// The number of focusables in the active scope (the whole ring when no
    /// scope is pushed).
    #[must_use]
    pub fn len(&self) -> usize {
        self.active_ids().len()
    }

    /// Whether the active scope has no focusables (the whole ring when no
    /// scope is pushed).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.active_ids().is_empty()
    }

    /// Whether `id` is registered in the active scope (the whole ring when no
    /// scope is pushed).
    ///
    /// This is the membership check the "restore the previously-focused id
    /// only if it still exists" pattern (ADR 0004 §6) is built on: a remembered
    /// id whose widget is gone — or that belongs to a now-trapped background —
    /// is simply not contained any more.
    #[must_use]
    pub fn contains(&self, id: FocusId) -> bool {
        self.active_ids().contains(&id)
    }

    /// The currently-focused id, or `None` if nothing is focused.
    #[must_use]
    pub fn focused(&self) -> Option<FocusId> {
        self.focused
    }

    /// Whether `id` is the currently-focused one — the cheap `==` a `view`
    /// uses to pass `focused: bool` into a widget.
    #[must_use]
    pub fn is_focused(&self, id: FocusId) -> bool {
        self.focused == Some(id)
    }

    /// Focuses `id` **if it is registered in the active scope**, and returns
    /// [`focused`](Self::focused) afterwards.
    ///
    /// Focusing an id that is not in the active scope is a deliberate no-op
    /// (focus is left unchanged): an app can call `focus(saved_id)` to restore
    /// a remembered focus and it will simply not take effect if that widget is
    /// gone — the pure-model form of an unmount-safe focus restore — and a
    /// background id cannot steal focus while a modal scope traps it. Compare
    /// the return value (or use [`is_focused`](Self::is_focused)) to detect
    /// that case.
    pub fn focus(&mut self, id: FocusId) -> Option<FocusId> {
        if self.active_ids().contains(&id) {
            self.focused = Some(id);
        }
        self.focused
    }

    /// Moves focus to the next id, wrapping past the end, and returns
    /// [`focused`](Self::focused) afterwards.
    ///
    /// Total in every case: an empty ring stays unfocused (`None`); a
    /// non-empty ring with nothing (or a stale id) focused focuses the first
    /// id; otherwise focus advances one step with wraparound.
    pub fn focus_next(&mut self) -> Option<FocusId> {
        self.step(Step::Next)
    }

    /// Moves focus to the previous id, wrapping past the start, and returns
    /// [`focused`](Self::focused) afterwards.
    ///
    /// Total in every case: an empty ring stays unfocused (`None`); a
    /// non-empty ring with nothing (or a stale id) focused focuses the last
    /// id; otherwise focus retreats one step with wraparound.
    pub fn focus_prev(&mut self) -> Option<FocusId> {
        self.step(Step::Prev)
    }

    /// Pushes a modal focus **scope** of `ids`, trapping focus inside it, and
    /// returns [`focused`](Self::focused) afterwards.
    ///
    /// Call this from `update` when a modal/overlay/command palette opens
    /// (ADR 0004 §6). While the scope is active, every traversal/lookup
    /// ([`focus`](Self::focus) / [`focus_next`](Self::focus_next) /
    /// [`focus_prev`](Self::focus_prev) / [`contains`](Self::contains) /
    /// [`len`](Self::len)) is constrained to `ids` and a background id cannot
    /// be focused — the modal-containment retry loop a retained-mode framework
    /// needs collapses to this one push because order is model-owned.
    ///
    /// The currently-focused id is **captured** so
    /// [`pop_scope`](Self::pop_scope) can validate-restore it, and the scope's
    /// first id is focused (or nothing if `ids` is empty — a total
    /// "no focusable in this modal yet" state). Scopes nest; the innermost is
    /// the active one.
    pub fn push_scope<I>(&mut self, ids: I) -> Option<FocusId>
    where
        I: IntoIterator<Item = FocusId>,
    {
        let ids: Vec<FocusId> = ids.into_iter().collect();
        let focused = ids.first().copied();
        self.scopes.push(FocusScope {
            ids,
            saved: self.focused,
        });
        self.focused = focused;
        self.focused
    }

    /// Pops the innermost focus scope, **validate-restoring** the focus that
    /// was captured when it was pushed, and returns
    /// [`focused`](Self::focused) afterwards.
    ///
    /// Call this from `update` when the modal closes (ADR 0004 §6). The
    /// captured id is refocused **only if it is still registered** in the
    /// now-active set; a stale id (its widget gone while the modal was open) or
    /// a `None` capture falls back to that set's first id (or nothing if it is
    /// empty). This is OpenCode's unmount-safe restore expressed in the
    /// pure-model idiom: a tree-walk validation becomes a membership check.
    ///
    /// Popping when no scope is active is a deliberate, total no-op.
    pub fn pop_scope(&mut self) -> Option<FocusId> {
        if let Some(scope) = self.scopes.pop() {
            let ids = self.active_ids();
            // The `Some(s)` *miss* arm (captured id no longer in the now-active
            // set) is defensive totality for a future order-mutating API: with
            // scope/base id sets immutable today, a captured id is always still
            // present on pop, so only the `None`-capture fallback is reachable.
            let restored = match scope.saved {
                Some(s) if ids.contains(&s) => Some(s),
                _ => ids.first().copied(),
            };
            self.focused = restored;
        }
        self.focused
    }

    /// The number of active modal focus scopes — `0` is the base ring, `> 0`
    /// means a modal/overlay is trapping focus.
    ///
    /// The reducer reads this (or [`in_scope`](Self::in_scope)) to **gate
    /// background input declaratively** (ADR 0004 §6): background key handling
    /// is predicated on the scope stack, the runtime never sinks input. The
    /// count (not just a bool) also lets nested-modal logic see how deep it is.
    #[must_use]
    pub fn scope_depth(&self) -> usize {
        self.scopes.len()
    }

    /// Whether any modal focus scope is active — the declarative "is a modal
    /// open" predicate (`scope_depth() > 0`) the reducer gates background key
    /// handling on (ADR 0004 §6).
    #[must_use]
    pub fn in_scope(&self) -> bool {
        !self.scopes.is_empty()
    }

    /// Shared, total traversal core for [`focus_next`](Self::focus_next) and
    /// [`focus_prev`](Self::focus_prev), so the wrap/totality rules cannot
    /// drift between the two directions.
    fn step(&mut self, dir: Step) -> Option<FocusId> {
        let ids = self.active_ids();
        let len = ids.len();
        if len == 0 {
            self.focused = None;
            return None;
        }
        // First occurrence of the focused id, or `None` if nothing valid is
        // focused. The `None` arms below are defensive totality for a future
        // order-mutating API (ADR 0004 §6); they are unreachable through the
        // public API today because every constructor / `push_scope` /
        // `pop_scope` over a non-empty active set leaves a valid id focused.
        let current = self.focused.and_then(|f| ids.iter().position(|x| *x == f));
        let next = match (current, dir) {
            (Some(i), Step::Next) => (i + 1) % len,
            (Some(i), Step::Prev) => (i + len - 1) % len,
            (None, Step::Next) => 0,
            (None, Step::Prev) => len - 1,
        };
        // Copy the id out before the borrow of `ids` (which borrows `self`)
        // ends, so the `self.focused` write below is borrow-check clean.
        let id = ids[next];
        self.focused = Some(id);
        self.focused
    }
}

/// Traversal direction for [`FocusRing::step`].
enum Step {
    Next,
    Prev,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_id_is_a_copy_value_identity_token() {
        use std::collections::HashMap;

        let a = FocusId::new(42);
        let b = a; // Copy, not move.
        assert_eq!(a, b);
        assert_eq!(a, FocusId::new(42));
        assert_ne!(a, FocusId::new(43));

        // Value identity means a `FocusId` keys a map directly.
        let mut by_id: HashMap<FocusId, &str> = HashMap::new();
        by_id.insert(FocusId::new(1), "name");
        by_id.insert(FocusId::new(2), "email");
        assert_eq!(by_id.get(&FocusId::new(1)), Some(&"name"));
        assert_eq!(by_id.get(&FocusId::new(9)), None);
    }

    #[test]
    fn new_and_default_are_an_empty_unfocused_ring() {
        assert_eq!(FocusRing::new(), FocusRing::default());
        let ring = FocusRing::new();
        assert!(ring.is_empty());
        assert_eq!(ring.len(), 0);
        assert_eq!(ring.focused(), None);
    }

    #[test]
    fn with_ids_keeps_order_and_focuses_the_first_id() {
        let a = FocusId::new(10);
        let b = FocusId::new(20);
        let c = FocusId::new(30);
        let ring = FocusRing::with_ids([a, b, c]);

        assert_eq!(ring.len(), 3);
        assert!(!ring.is_empty());
        assert!(ring.contains(b));
        assert!(!ring.contains(FocusId::new(99)));
        assert_eq!(ring.focused(), Some(a));
        assert!(ring.is_focused(a));
        assert!(!ring.is_focused(b));
    }

    #[test]
    fn with_ids_over_an_empty_iterator_is_an_empty_ring() {
        let ring = FocusRing::with_ids(Vec::<FocusId>::new());
        assert_eq!(ring, FocusRing::new());
        assert!(ring.is_empty());
        assert_eq!(ring.focused(), None);
    }

    #[test]
    fn an_empty_ring_is_total_and_focuses_nothing() {
        let mut ring = FocusRing::new();
        let ghost = FocusId::new(0);
        assert_eq!(ring.focus_next(), None);
        assert_eq!(ring.focus_prev(), None);
        assert_eq!(ring.focus(ghost), None);
        assert!(!ring.is_focused(ghost));
        assert_eq!(ring.focused(), None);
    }

    #[test]
    fn focus_next_and_prev_wrap_around_the_ring() {
        let a = FocusId::new(10);
        let b = FocusId::new(20);
        let c = FocusId::new(30);
        let mut ring = FocusRing::with_ids([a, b, c]);

        assert_eq!(ring.focused(), Some(a));
        assert_eq!(ring.focus_next(), Some(b));
        assert_eq!(ring.focus_next(), Some(c));
        assert_eq!(ring.focus_next(), Some(a)); // wrap forward over the end
        assert_eq!(ring.focus_prev(), Some(c)); // wrap backward over the start
        assert_eq!(ring.focus_prev(), Some(b));
    }

    #[test]
    fn a_single_element_ring_keeps_focus_on_traversal() {
        let only = FocusId::new(7);
        let mut ring = FocusRing::with_ids([only]);

        assert_eq!(ring.focused(), Some(only));
        assert_eq!(ring.focus_next(), Some(only));
        assert_eq!(ring.focus_prev(), Some(only));
        assert!(ring.is_focused(only));
    }

    #[test]
    fn focus_jumps_to_a_registered_id_and_ignores_an_unregistered_one() {
        let a = FocusId::new(1);
        let b = FocusId::new(2);
        let c = FocusId::new(3);
        let mut ring = FocusRing::with_ids([a, b, c]);

        // Jump straight to a known id (e.g. click-to-focus).
        assert_eq!(ring.focus(c), Some(c));
        assert!(ring.is_focused(c));

        // Focusing an unregistered id is a no-op: focus is preserved and
        // nothing panics — the validated-restore safety property.
        let stale = FocusId::new(999);
        assert!(!ring.contains(stale));
        assert_eq!(ring.focus(stale), Some(c));
        assert!(ring.is_focused(c));

        // Traversal still resumes from the jumped-to position.
        assert_eq!(ring.focus_next(), Some(a)); // c is last -> wrap to a
    }

    #[test]
    fn push_scope_traps_focus_and_pop_validate_restores_the_capture() {
        let name = FocusId::new(1);
        let email = FocusId::new(2);
        let submit = FocusId::new(3);
        let mut ring = FocusRing::with_ids([name, email, submit]);
        ring.focus(submit);

        // A modal opens with its own ids. Focus is captured + trapped.
        let ok = FocusId::new(10);
        let cancel = FocusId::new(11);
        assert_eq!(ring.push_scope([ok, cancel]), Some(ok));
        assert!(ring.in_scope());
        assert_eq!(ring.scope_depth(), 1);

        // Lookups are scope-relative while trapped.
        assert_eq!(ring.len(), 2);
        assert!(ring.contains(ok));
        assert!(!ring.contains(name)); // background id is not in scope

        // Traversal wraps within the modal and never escapes to the form.
        assert_eq!(ring.focus_next(), Some(cancel));
        assert_eq!(ring.focus_next(), Some(ok)); // wrap inside the scope
        // A background id cannot steal focus while trapped.
        assert_eq!(ring.focus(name), Some(ok));
        assert!(ring.is_focused(ok));

        // Closing the modal validate-restores the captured focus.
        assert_eq!(ring.pop_scope(), Some(submit));
        assert!(!ring.in_scope());
        assert_eq!(ring.scope_depth(), 0);
        assert_eq!(ring.len(), 3); // base ring is active again
        assert!(ring.contains(name));
    }

    #[test]
    fn nested_scopes_each_restore_their_own_captured_focus() {
        let a = FocusId::new(1);
        let b = FocusId::new(2);
        let mut ring = FocusRing::with_ids([a, b]);
        ring.focus(b);

        let m1 = FocusId::new(10);
        let m2 = FocusId::new(20);
        ring.push_scope([m1]); // captures b, focuses m1
        ring.push_scope([m2]); // captures m1, focuses m2
        assert_eq!(ring.scope_depth(), 2);
        assert!(ring.is_focused(m2));

        assert_eq!(ring.pop_scope(), Some(m1)); // inner restores its capture
        assert_eq!(ring.scope_depth(), 1);
        assert_eq!(ring.pop_scope(), Some(b)); // outer restores the base focus
        assert_eq!(ring.scope_depth(), 0);
    }

    #[test]
    fn an_empty_scope_is_total_and_pop_restores_the_parent() {
        let a = FocusId::new(1);
        let b = FocusId::new(2);
        let mut ring = FocusRing::with_ids([a, b]);
        ring.focus(b);

        // A modal with no focusables yet: total "nothing focused here" state.
        assert_eq!(ring.push_scope(Vec::<FocusId>::new()), None);
        assert!(ring.in_scope());
        assert!(ring.is_empty());
        assert_eq!(ring.focus_next(), None);
        assert_eq!(ring.focus_prev(), None);
        assert_eq!(ring.focus(a), None); // a is trapped out

        // Popping the empty scope validate-restores the captured base focus.
        assert_eq!(ring.pop_scope(), Some(b));
        assert!(!ring.in_scope());
    }

    #[test]
    fn pop_scope_with_no_active_scope_is_a_total_no_op() {
        let a = FocusId::new(1);
        let mut ring = FocusRing::with_ids([a]);
        assert_eq!(ring.scope_depth(), 0);
        assert_eq!(ring.pop_scope(), Some(a)); // no-op, focus preserved
        assert_eq!(ring.scope_depth(), 0);
        assert!(ring.is_focused(a));

        let mut empty = FocusRing::new();
        assert_eq!(empty.pop_scope(), None);
        assert!(!empty.in_scope());
    }

    #[test]
    fn scope_depth_and_in_scope_track_the_stack() {
        let mut ring = FocusRing::with_ids([FocusId::new(1)]);
        assert!(!ring.in_scope() && ring.scope_depth() == 0);
        ring.push_scope([FocusId::new(10)]);
        assert!(ring.in_scope() && ring.scope_depth() == 1);
        ring.push_scope([FocusId::new(20)]);
        assert!(ring.in_scope() && ring.scope_depth() == 2);
        ring.pop_scope();
        assert!(ring.in_scope() && ring.scope_depth() == 1);
        ring.pop_scope();
        assert!(!ring.in_scope() && ring.scope_depth() == 0);
    }

    /// The totality property ADR 0004 Follow-up §1 (and §6 for scopes)
    /// requires: any sequence of `focus_next` / `focus_prev` / `focus`
    /// (including unregistered ids) / `push_scope` / `pop_scope`, over rings of
    /// any size, never panics and always leaves `focused()` either `None` or an
    /// id that is still in the *active* set ("stays in-set"), with `in_scope`
    /// and `scope_depth` consistent.
    #[test]
    fn any_sequence_of_operations_is_total_and_stays_in_set() {
        // A fixed-seed LCG keeps the property run deterministic with no rand
        // dependency (rstui-core is dependency-free).
        let mut state: u64 = 0x1234_5678_9abc_def0;
        let mut rng = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };

        for size in 0u64..=4 {
            let ids: Vec<FocusId> = (0..size).map(FocusId::new).collect();
            let mut ring = FocusRing::with_ids(ids.clone());

            for _ in 0..2_000 {
                match rng() % 6 {
                    0 => {
                        ring.focus_next();
                    }
                    1 => {
                        ring.focus_prev();
                    }
                    2 => {
                        // Sometimes a registered id, sometimes a stale one.
                        ring.focus(FocusId::new(rng() % (size + 2)));
                    }
                    3 => {
                        // Push a scope that is a (possibly empty) sub-range of
                        // the base ids, so a focused id stays within `ids`.
                        let scope: Vec<FocusId> = if size == 0 {
                            Vec::new()
                        } else {
                            let s = (rng() % size) as usize;
                            let e = s + (rng() % (size - s as u64 + 1)) as usize;
                            ids[s..e].to_vec()
                        };
                        ring.push_scope(scope);
                    }
                    4 => {
                        ring.pop_scope();
                    }
                    _ => {
                        ring = FocusRing::with_ids(ids.clone());
                    }
                }

                // Invariant 1: focused is None or still in the active set
                // (and, since scopes are sub-ranges, still a base id).
                if let Some(f) = ring.focused() {
                    assert!(ring.contains(f), "focused id escaped the scope");
                    assert!(ids.contains(&f));
                }
                // Invariant 2: is_focused agrees with focused for every id.
                for &id in &ids {
                    assert_eq!(ring.is_focused(id), ring.focused() == Some(id));
                }
                // Invariant 3: the scope predicates stay consistent.
                assert_eq!(ring.in_scope(), ring.scope_depth() > 0);
            }
            // Reaching here for every size proves no operation panicked.
        }
    }
}
