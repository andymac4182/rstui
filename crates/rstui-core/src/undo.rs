//! Caller-owned linear undo/redo history as model state.
//!
//! [`History`] is the time-travel sibling of
//! [`TextEdit`](crate::text_edit::TextEdit) /
//! [`ScrollState`](crate::scroll::ScrollState) /
//! [`FocusRing`](crate::focus::FocusRing): a pure value type that lives as a
//! *field in the application's model*, mutated only by `update` (an edit was
//! committed, Ctrl+Z, Ctrl+Y), and read by the pure `view` only to decide
//! whether the undo/redo affordances are enabled. Nothing here draws or owns a
//! widget. Per
//! [ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
//! §1 this separation is *forced* by rstui's pure-`view` / immediate-mode
//! design: a widget is handed only a [`Buffer`](crate::buffer::Buffer) at
//! render time, so it can neither own the document being edited nor roll it
//! back on a keystroke. The reducer owns the history exactly as it owns the
//! focus, the scroll, and the edited text. Per
//! [ADR 0012](https://github.com/andymac4182/rstui/blob/main/docs/adr/0012-caller-owned-state-primitives.md)
//! it is one more *optional* caller-owned primitive: an app may keep two
//! `Vec`s of its own and never name a type from here.
//!
//! `History` exists only to turn the undo/redo bookkeeping every editor
//! re-derives — and routinely gets wrong (a stale redo branch surviving a new
//! edit, an unbounded stack that grows without limit, a typing run that needs
//! *N* presses of undo because every keystroke pushed its own entry) — into
//! one reusable, panic-free primitive:
//!
//! - It is **generic over the snapshot type** `T: Clone + PartialEq`. The
//!   snapshot is whatever value the app considers "the document": a
//!   `String`, a [`TextEdit`](crate::text_edit::TextEdit), a multi-line text
//!   buffer, or a struct bundling a buffer with its cursor. `History` never
//!   inspects `T` beyond cloning it and comparing two values for equality, so
//!   it composes with any editable model without that model knowing history
//!   exists (the same caller-owned-value contract the other primitives use).
//! - It is **linear**: a fresh [`snapshot`](History::snapshot) after an
//!   [`undo`](History::undo) discards the redo branch (a new edit forks the
//!   timeline), the model every single-buffer editor actually wants — not a
//!   tree.
//! - It **coalesces** runs of fine-grained edits
//!   ([`snapshot_coalesced`](History::snapshot_coalesced)): a burst of
//!   single-character inserts collapses into **one** undo step instead of one
//!   per keystroke, the ergonomic every text editor needs.
//! - Its undo depth is **bounded** ([`with_capacity`](History::with_capacity)):
//!   keeping a clone per edit of a large document would grow memory without
//!   limit, so a non-zero `cap` is a ring that drops the *oldest* entry once
//!   the depth is reached — older history is forgotten, never the recent
//!   undos a user reaches for. `cap == 0` keeps it unbounded for small
//!   snapshots where that does not matter.
//! - Every method is **total** — no input sequence panics; an
//!   [`undo`](History::undo) with nothing to undo or a
//!   [`redo`](History::redo) with nothing to redo returns `None` (the iter-25
//!   "a pure projection must be total" rule, the same guarantee
//!   [`FocusRing`](crate::focus::FocusRing) /
//!   [`TextEdit`](crate::text_edit::TextEdit) /
//!   [`ScrollState`](crate::scroll::ScrollState) give).
//!
//! It is **single, linear history on purpose**: branching/tree undo is a
//! separate model, not a flag on this one, exactly as multi-line editing is a
//! separate model from [`TextEdit`](crate::text_edit::TextEdit).
//!
//! # Example
//!
//! ```
//! use rstui_core::undo::History;
//!
//! // The app stores one per undoable document in its model. The snapshot
//! // here is a `String`; in a real app it is typically the editor buffer.
//! let mut hist = History::new(String::from("hello"));
//! assert!(!hist.can_undo());
//!
//! // `update` records a snapshot after a committed edit. A no-op edit (the
//! // value did not change) is ignored, so undo never has dead steps.
//! hist.snapshot(&String::from("hello, world"));
//! hist.snapshot(&String::from("hello, world")); // no change -> ignored
//! assert!(hist.can_undo());
//!
//! // Undo hands back the previous state and arms redo. The reducer assigns
//! // the returned value back into the model and re-renders.
//! let restored = hist.undo(&String::from("hello, world"));
//! assert_eq!(restored.as_deref(), Some("hello"));
//! assert!(hist.can_redo());
//!
//! // Redo is the inverse.
//! assert_eq!(
//!     hist.redo(&String::from("hello")).as_deref(),
//!     Some("hello, world"),
//! );
//!
//! // A run of keystrokes coalesces into ONE undo step.
//! let mut typing = History::new(String::new());
//! for word in ["a", "ab", "abc"] {
//!     typing.snapshot_coalesced(&word.to_string(), true);
//! }
//! // One undo jumps straight back past the whole run.
//! assert_eq!(typing.undo(&String::from("abc")).as_deref(), Some(""));
//!
//! // Every input is total: undo/redo on an empty side simply returns `None`.
//! let mut empty = History::<String>::new(String::new());
//! assert_eq!(empty.undo(&String::new()), None);
//! assert_eq!(empty.redo(&String::new()), None);
//! ```

/// A caller-owned linear undo/redo history over snapshots of type `T`.
///
/// `History` is a **pure value type** designed to live as a field in the
/// application's model. It owns *no* terminal, runtime, or widget state:
/// `update` mutates it (record a snapshot after an edit, undo, redo) and the
/// pure `view` only reads [`can_undo`](Self::can_undo) /
/// [`can_redo`](Self::can_redo) to enable or disable affordances. The
/// framework never touches it.
///
/// `T: Clone + PartialEq` is the only bound: `History` clones a snapshot to
/// store it and compares two for equality to drop no-op edits. It never
/// inspects `T` otherwise, so any editable model — a `String`, a
/// [`TextEdit`](crate::text_edit::TextEdit), a struct bundling a buffer and
/// cursor — drops in without knowing history exists.
///
/// The timeline is **linear**: `undo_stack` holds the committed past (oldest
/// first; its top is the state to restore on the next [`undo`](Self::undo)),
/// `redo_stack` holds states undone away (its top is the next
/// [`redo`](Self::redo)), and any [`snapshot`](Self::snapshot) clears
/// `redo_stack` because a new edit forks the timeline and the old future is
/// gone. The undo depth is bounded by `cap` (when non-zero) by dropping the
/// **oldest** `undo_stack` entry, so memory stays bounded for large
/// snapshots; `cap == 0` is unbounded.
///
/// Every method is **total**: any sequence — undo/redo on an empty side,
/// repeated identical snapshots, coalescing onto an empty history, a `cap` of
/// `0` or `1` — is well-defined and never panics.
#[derive(Debug, Clone)]
pub struct History<T> {
    /// The committed past, oldest entry first. The **top** (last element) is
    /// the state a subsequent [`undo`](Self::undo) restores; it is the value
    /// as of the most recently recorded edit and is what
    /// [`snapshot`](Self::snapshot) compares against to drop a no-op.
    undo_stack: Vec<T>,
    /// States that were undone away, in the order [`redo`](Self::redo) will
    /// replay them (its **top**, the last element, is the next redo). Any
    /// [`snapshot`](Self::snapshot) empties this — a new edit forks the
    /// timeline, discarding the abandoned future.
    redo_stack: Vec<T>,
    /// Maximum undo depth: when non-zero, recording an entry past this many
    /// drops the **oldest** (front) `undo_stack` entry so memory is bounded.
    /// `0` means unbounded.
    cap: usize,
    /// Whether a coalesced run is currently open — i.e. the top of
    /// `undo_stack` is a run's evolving state that the *next*
    /// [`snapshot_coalesced`](Self::snapshot_coalesced)`(_, true)` should
    /// replace in place rather than push past. The **first** coalesced edit
    /// after a sealed step pushes a fresh entry and opens the run (so one
    /// [`undo`](Self::undo) lands on the state *before* the run); subsequent
    /// ones replace it, collapsing the whole keystroke burst into that single
    /// step. A plain [`snapshot`](Self::snapshot) (`coalesce == false`) or any
    /// [`undo`](Self::undo)/[`redo`](Self::redo) **seals** the run by clearing
    /// this, so the next coalesced edit starts a new step.
    coalescing: bool,
}

impl<T: Clone + PartialEq> History<T> {
    /// A history seeded with `initial` as the only undo point and an empty
    /// redo stack — the state of a freshly opened document (nothing to undo
    /// or redo yet). Unbounded depth; use [`with_capacity`](Self::with_capacity)
    /// to bound it.
    #[must_use]
    pub fn new(initial: T) -> Self {
        Self::with_capacity(initial, 0)
    }

    /// Like [`new`](Self::new) but bounds the undo depth to `cap` snapshots:
    /// once that many are recorded, each new one drops the **oldest** entry
    /// (a ring), so memory stays bounded for large snapshots. `cap == 0`
    /// means **unbounded** (keep every snapshot — fine for small `T`).
    ///
    /// The seed counts toward the bound, so e.g. `with_capacity(v, 1)` keeps
    /// only the single most recent state (every new snapshot evicts the
    /// previous one, leaving nothing to undo to — a valid, total degenerate
    /// case, not a panic).
    ///
    /// # Example
    ///
    /// ```
    /// use rstui_core::undo::History;
    ///
    /// // Depth 2: only the two most recent snapshots are kept.
    /// let mut h = History::with_capacity(0_i32, 2);
    /// h.snapshot(&1);
    /// h.snapshot(&2); // drops the original `0`
    /// assert_eq!(h.undo(&2), Some(1));
    /// assert_eq!(h.undo(&1), None); // `0` was evicted by the bound
    /// ```
    #[must_use]
    pub fn with_capacity(initial: T, cap: usize) -> Self {
        Self {
            undo_stack: vec![initial],
            redo_stack: Vec::new(),
            cap,
            coalescing: false,
        }
    }

    /// Records `state` as a new undo point.
    ///
    /// A **no-op edit is ignored**: if `state` equals the current top (the
    /// document did not actually change) nothing is pushed, so `undo` never
    /// has a dead step that appears to do nothing. Any real change **clears
    /// the redo stack** — a fresh edit forks the timeline, so the abandoned
    /// redo branch is discarded (the linear-history rule). When `cap` is
    /// non-zero and the depth would exceed it, the **oldest** entry is
    /// dropped (the bounded-memory ring).
    pub fn snapshot(&mut self, state: &T) {
        self.snapshot_coalesced(state, false);
    }

    /// Like [`snapshot`](Self::snapshot) but, when `coalesce` is `true`,
    /// **merges** the change into the current coalesced run instead of
    /// pushing a separate undo step. This is how a run of single-character
    /// inserts becomes **one** [`undo`](Self::undo): map each keystroke to
    /// `snapshot_coalesced(&doc, true)` while the run continues, then a plain
    /// [`snapshot`](Self::snapshot) (or `coalesce == false`) to *seal* the
    /// run so the next edit starts a new step.
    ///
    /// Precisely, with `coalesce == true`: the **first** such call after a
    /// sealed step *pushes* a new entry and opens the run, so one
    /// [`undo`](Self::undo) lands on the state that existed *before* the run
    /// began; every **subsequent** call while the run is open *replaces* that
    /// top in place, so the whole burst collapses into that single step. Any
    /// [`undo`](Self::undo)/[`redo`](Self::redo) or a non-coalescing snapshot
    /// seals the run, so a coalesced edit after one of those opens a fresh
    /// run rather than rewriting a restored state.
    ///
    /// With `coalesce == false` this is exactly [`snapshot`](Self::snapshot).
    /// In **both** modes a no-op (`state` equals the current top) is ignored —
    /// it leaves any open run open (an unchanged keystroke does not break the
    /// burst) — and any real change clears the redo stack. Opening a run when
    /// the undo stack holds only the seed simply pushes the first run entry
    /// above it (still a single, total step).
    pub fn snapshot_coalesced(&mut self, state: &T, coalesce: bool) {
        // A no-op edit: the document is unchanged, so neither a new step nor a
        // coalesced replacement is warranted, the redo branch survives
        // (nothing forked), and an open run stays open (an unchanged
        // keystroke must not break a coalescing burst).
        if self.undo_stack.last() == Some(state) {
            return;
        }
        // A real edit forks the timeline: the abandoned redo future is gone.
        self.redo_stack.clear();

        if coalesce && self.coalescing {
            // A run is already open: the whole burst collapses onto the single
            // pre-run step already below, so just rewrite the run's top.
            if let Some(top) = self.undo_stack.last_mut() {
                *top = state.clone();
                return;
            }
            // Unreachable in practice (the stack always holds at least the
            // seed), but stay total: fall through to a push rather than panic.
        }

        // Either a sealed/normal edit, or the FIRST edit of a new coalesced
        // run: push a fresh step. `coalesce` then records whether subsequent
        // calls should merge into this one (run open) or it is sealed.
        self.undo_stack.push(state.clone());
        self.coalescing = coalesce;
        // Bounded-memory ring: keep at most `cap` entries by dropping the
        // oldest. `> cap` (not `>=`) so `cap` itself is the retained depth;
        // `Vec::remove(0)` is O(n) but n is the small bounded `cap`.
        if self.cap != 0 && self.undo_stack.len() > self.cap {
            self.undo_stack.remove(0);
        }
    }

    /// Returns the state to restore (the snapshot **before** the last
    /// recorded change), moving `current` onto the redo stack, or `None` when
    /// there is nothing to undo (only the seed remains, or the history is
    /// degenerate under a tiny `cap`).
    ///
    /// `current` is the live document the reducer holds; passing it in (rather
    /// than `History` storing a duplicate of the present) keeps the single
    /// source of truth in the model and means a redo can faithfully return to
    /// it. The caller assigns the returned value back into the model.
    pub fn undo(&mut self, current: &T) -> Option<T> {
        if self.undo_stack.len() < 2 {
            // Only the seed (or nothing) — no prior state to go back to.
            return None;
        }
        // An undo seals any open coalesced run: a coalesced edit after this
        // must start a fresh step, not rewrite the state we just restored.
        self.coalescing = false;
        // Drop the state as-of the last edit; the new top is the one before
        // it, which is what we restore to.
        self.undo_stack.pop();
        self.redo_stack.push(current.clone());
        self.undo_stack.last().cloned()
    }

    /// Returns the next redo state (re-applying the most recently undone
    /// change), moving `current` back onto the undo stack, or `None` when
    /// there is nothing to redo (no [`undo`](Self::undo) is pending, or a
    /// [`snapshot`](Self::snapshot) forked the timeline and cleared it).
    ///
    /// The caller assigns the returned value back into the model. This is the
    /// exact inverse of [`undo`](Self::undo).
    pub fn redo(&mut self, current: &T) -> Option<T> {
        let next = self.redo_stack.pop()?;
        // A redo likewise seals any open coalesced run.
        self.coalescing = false;
        self.undo_stack.push(current.clone());
        // Re-applying a redone state must not exceed the bound either.
        if self.cap != 0 && self.undo_stack.len() > self.cap {
            self.undo_stack.remove(0);
        }
        Some(next)
    }

    /// Whether there is a prior state to go back to — i.e. the next
    /// [`undo`](Self::undo) would return `Some`. The pure `view` reads this
    /// to enable/disable the undo affordance.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.undo_stack.len() >= 2
    }

    /// Whether there is an undone state to re-apply — i.e. the next
    /// [`redo`](Self::redo) would return `Some`. The pure `view` reads this
    /// to enable/disable the redo affordance.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_with_nothing_to_undo_or_redo() {
        let h = History::new(String::from("seed"));
        assert!(!h.can_undo());
        assert!(!h.can_redo());
    }

    #[test]
    fn snapshot_undo_redo_round_trip() {
        let mut h = History::new(String::from("v0"));
        h.snapshot(&String::from("v1"));
        h.snapshot(&String::from("v2"));
        assert!(h.can_undo());
        assert!(!h.can_redo());

        // Undo walks back one step at a time, arming redo.
        assert_eq!(h.undo(&String::from("v2")).as_deref(), Some("v1"));
        assert_eq!(h.undo(&String::from("v1")).as_deref(), Some("v0"));
        assert!(!h.can_undo()); // back at the seed
        assert!(h.can_redo());

        // Redo replays them in order — the exact inverse.
        assert_eq!(h.redo(&String::from("v0")).as_deref(), Some("v1"));
        assert_eq!(h.redo(&String::from("v1")).as_deref(), Some("v2"));
        assert!(!h.can_redo());
        assert!(h.can_undo());
    }

    #[test]
    fn an_unchanged_snapshot_is_ignored_and_keeps_redo() {
        let mut h = History::new(String::from("a"));
        h.snapshot(&String::from("b"));
        assert_eq!(h.undo(&String::from("b")).as_deref(), Some("a"));
        assert!(h.can_redo());

        // Re-recording the *same* current value is a no-op: it must NOT push a
        // dead step and must NOT fork (redo survives).
        h.snapshot(&String::from("a"));
        assert!(!h.can_undo());
        assert!(h.can_redo());
        assert_eq!(h.redo(&String::from("a")).as_deref(), Some("b"));
    }

    #[test]
    fn coalescing_collapses_a_typing_run_to_one_undo() {
        let mut h = History::new(String::new());
        // Seal a deliberate first step, then "type" a run that coalesces.
        h.snapshot(&String::from("start"));
        for s in ["s", "sa", "sav", "save"] {
            h.snapshot_coalesced(&s.to_string(), true);
        }
        // ONE undo jumps past the whole coalesced run to the sealed step.
        assert_eq!(h.undo(&String::from("save")).as_deref(), Some("start"));
        // And exactly one more reaches the original seed.
        assert_eq!(h.undo(&String::from("start")).as_deref(), Some(""));
        assert!(!h.can_undo());
    }

    #[test]
    fn coalescing_onto_only_the_seed_is_one_step_back_to_the_seed() {
        // No sealed step yet: the FIRST coalesced edit opens a run by pushing
        // above the seed; later ones merge into it. The whole burst is a
        // single, total step whose one undo lands back on the seed.
        let mut h = History::new(String::from("seed"));
        h.snapshot_coalesced(&String::from("x"), true);
        h.snapshot_coalesced(&String::from("xy"), true);
        h.snapshot_coalesced(&String::from("xyz"), true);
        assert!(h.can_undo());
        assert_eq!(h.undo(&String::from("xyz")).as_deref(), Some("seed"));
        assert!(!h.can_undo()); // exactly one step for the whole run
    }

    #[test]
    fn an_undo_seals_the_run_so_the_next_coalesced_edit_is_a_fresh_step() {
        let mut h = History::new(String::from("v0"));
        // An open coalesced run.
        h.snapshot_coalesced(&String::from("a"), true);
        h.snapshot_coalesced(&String::from("ab"), true);
        // Undo seals it and restores the pre-run state.
        assert_eq!(h.undo(&String::from("ab")).as_deref(), Some("v0"));

        // A coalesced edit now must NOT rewrite the restored "v0"; it opens a
        // brand-new run above it (one undo still gets back to "v0").
        h.snapshot_coalesced(&String::from("z"), true);
        h.snapshot_coalesced(&String::from("zz"), true);
        assert!(h.can_undo());
        assert_eq!(h.undo(&String::from("zz")).as_deref(), Some("v0"));
        assert!(!h.can_undo());
    }

    #[test]
    fn an_unchanged_coalesced_keystroke_keeps_the_run_open() {
        let mut h = History::new(String::from("seed"));
        h.snapshot(&String::from("base")); // sealed step
        h.snapshot_coalesced(&String::from("b"), true); // opens a run
        // A no-op "keystroke" (value unchanged) must not seal the run…
        h.snapshot_coalesced(&String::from("b"), true);
        // …so the following real coalesced edit still merges into the SAME
        // step rather than starting a second one.
        h.snapshot_coalesced(&String::from("bc"), true);
        assert_eq!(h.undo(&String::from("bc")).as_deref(), Some("base"));
        assert_eq!(h.undo(&String::from("base")).as_deref(), Some("seed"));
        assert!(!h.can_undo());
    }

    #[test]
    fn a_snapshot_after_undo_forks_and_clears_redo() {
        let mut h = History::new(String::from("v0"));
        h.snapshot(&String::from("v1"));
        h.snapshot(&String::from("v2"));
        assert_eq!(h.undo(&String::from("v2")).as_deref(), Some("v1"));
        assert!(h.can_redo());

        // A fresh edit from here forks the timeline: the "v2" redo branch is
        // discarded (linear history, not a tree).
        h.snapshot(&String::from("v1-branch"));
        assert!(!h.can_redo());
        assert_eq!(
            h.undo(&String::from("v1-branch")).as_deref(),
            Some("v1"),
        );
    }

    #[test]
    fn capacity_drops_the_oldest_entry() {
        // Depth 3 (seed + 2 edits retained at most).
        let mut h = History::with_capacity(0_i32, 3);
        h.snapshot(&1);
        h.snapshot(&2);
        h.snapshot(&3); // pushes past 3 -> drops the original `0`
        assert_eq!(h.undo(&3), Some(2));
        assert_eq!(h.undo(&2), Some(1));
        assert_eq!(h.undo(&1), None); // `0` was evicted by the bound

        // cap == 1 keeps only the most recent state: nothing to undo to.
        let mut one = History::with_capacity("a".to_string(), 1);
        one.snapshot(&"b".to_string());
        assert!(!one.can_undo());
        assert_eq!(one.undo(&"b".to_string()), None);
    }

    #[test]
    fn undo_and_redo_on_an_empty_side_return_none() {
        let mut h = History::new(String::from("only"));
        // Nothing recorded: undo has no prior state, redo has no future.
        assert_eq!(h.undo(&String::from("only")), None);
        assert_eq!(h.redo(&String::from("only")), None);

        // After a single undo, redo is armed but a second undo is not.
        h.snapshot(&String::from("edit"));
        assert_eq!(h.undo(&String::from("edit")).as_deref(), Some("only"));
        assert_eq!(h.undo(&String::from("only")), None);
        assert_eq!(h.redo(&String::from("only")).as_deref(), Some("edit"));
        assert_eq!(h.redo(&String::from("edit")), None);
    }

    /// The totality property (the iter-25 rule, mirroring
    /// [`TextEdit`](crate::text_edit::TextEdit)'s and
    /// [`ScrollState`](crate::scroll::ScrollState)'s): any sequence of any
    /// operation — snapshots, coalesced snapshots, undo and redo, under a
    /// random bounded or unbounded `cap` — never panics, keeps
    /// [`can_undo`](History::can_undo) / [`can_redo`](History::can_redo)
    /// exactly consistent with what the next [`undo`](History::undo) /
    /// [`redo`](History::redo) actually returns, and (with the redo branch
    /// intact) a full undo-to-bottom then redo-to-top returns to the original
    /// top state.
    #[test]
    fn any_sequence_of_operations_is_total_and_consistent() {
        // Fixed-seed LCG keeps the run deterministic with no rand dep
        // (rstui-core is dependency-free) — the same technique text_edit.rs,
        // text_area.rs and scroll.rs use.
        let mut state: u64 = 0x0bad_f00d_dead_beef;
        let mut rng = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };

        // Span unbounded and several tight bounds (incl. the degenerate 1).
        for cap in [0_usize, 1, 2, 5, 64] {
            let mut h = History::with_capacity(String::from("seed"), cap);
            // The live document the reducer would own, kept in lock-step.
            let mut doc = String::from("seed");

            for _ in 0..5_000 {
                match rng() % 6 {
                    0 | 1 => {
                        // A (possibly no-op) edit, then record it.
                        doc = format!("e{}", rng() % 8);
                        h.snapshot(&doc);
                    }
                    2 => {
                        // A coalesced keystroke-style edit.
                        doc = format!("c{}", rng() % 8);
                        h.snapshot_coalesced(&doc, true);
                    }
                    3 => {
                        // Re-record the *same* value: must be a no-op.
                        h.snapshot(&doc.clone());
                    }
                    4 => {
                        // can_undo must agree with what undo returns.
                        let expected = h.can_undo();
                        let got = h.undo(&doc);
                        assert_eq!(
                            got.is_some(),
                            expected,
                            "can_undo disagreed with undo",
                        );
                        if let Some(restored) = got {
                            doc = restored;
                        }
                    }
                    _ => {
                        // can_redo must agree with what redo returns.
                        let expected = h.can_redo();
                        let got = h.redo(&doc);
                        assert_eq!(
                            got.is_some(),
                            expected,
                            "can_redo disagreed with redo",
                        );
                        if let Some(re) = got {
                            doc = re;
                        }
                    }
                }
            }

            // With the redo branch intact (no snapshot since the last undo),
            // a full unwind then rewind must round-trip to the same top.
            // Re-seal so the timeline below is a pure undo/redo ladder.
            doc = String::from("anchor");
            h.snapshot(&doc);
            let top = doc.clone();

            let mut depth = 0_usize;
            while let Some(prev) = h.undo(&doc) {
                doc = prev;
                depth += 1;
            }
            assert!(!h.can_undo(), "can_undo true after unwinding fully");
            for _ in 0..depth {
                let re = h.redo(&doc).expect("redo must mirror each undo");
                doc = re;
            }
            assert!(!h.can_redo(), "can_redo true after rewinding fully");
            assert_eq!(doc, top, "undo-to-bottom then redo-to-top diverged");
        }
        // Reaching here proves no operation panicked for any input.
    }
}
