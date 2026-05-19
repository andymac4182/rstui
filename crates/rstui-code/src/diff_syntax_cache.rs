//! [`DiffSyntaxCache`] — the caller-owned `(patch source, theme)` memo for
//! the Tier-1 (tree-sitter) overlay [`Diff::tree_sitter`](crate::Diff)
//! precomputes (the caller-owned-cache model
//! [ADR 0025](https://github.com/andymac4182/rstui/blob/main/docs/adr/0025-caller-owned-line-cache.md)
//! / ADR 0012 §P1, the `rstui_widgets::DiagramCache` /
//! [`ScrollState`](rstui_core::ScrollState) precedent — same ethos, mirrored
//! here because `rstui-code` cannot reach `rstui-widgets`' crate-internal
//! `LineCache`).
//!
//! # The cost this removes
//!
//! With [`Diff::tree_sitter`](crate::Diff::tree_sitter) on, every layout pass
//! calls `build_tier1_map`, which runs a `Changeset::parse` over the whole
//! patch and then, **for every file in it**, reconstructs the new- and
//! old-side text and runs a full tree-sitter [`Analyzer`](crate::Analyzer)
//! parse + highlight on each side. A [`Diff`](crate::Diff) is a pure
//! projection that owns nothing across frames, so — exactly like re-wrapping
//! prose, only far heavier — that whole-patch parse runs **on every frame**:
//! the DIFF-1 `row_cap` windowing only bounds the per-row *layout*, not this
//! pre-pass. `git-review`'s review pane rebuilds
//! `Diff::new(self.diff)…tree_sitter(true)…` every frame, so a multi-file
//! patch was re-parsed with tree-sitter on **every scroll keystroke** — the
//! "really really slow" review pane (a multi-file commit dropped to single
//! digit fps while scrolling). The map is *scroll-, width- and
//! column-independent*, so paying that cost per frame is pure waste.
//!
//! # The contract
//!
//! The Tier-1 map is a pure function of `(patch source, theme)` — nothing
//! about scroll, width, or the horizontal column feeds it (those only affect
//! the per-row layout *after* the map exists). The cache memoises exactly
//! that: the first frame for a patch misses and runs `build_tier1_map` (the
//! unchanged code path — byte-identical); every later frame is an `O(1)`
//! keyed lookup plus an [`Rc`] refcount bump. Because the widget owns the
//! parse (the caller never sees the per-file overlays), the memo is a
//! *read-through* one with interior mutability — the render stays a pure
//! function of its inputs (a hit and a miss produce byte-identical
//! [`lines`](crate::Diff::lines), gate-enforced by the `diff` byte-identical
//! tests), the `RefCell` only elides recomputation.
//!
//! The key is the **complete** set of inputs `build_tier1_map` depends on:
//! the patch `source` *and* a cheap fingerprint of the [`DiffTheme`]'s twelve
//! `syntax_*` [`Style`](rstui_core::Style)s — the exact `Style`s
//! `syntax_styles(theme)` feeds the analyzer — so a theme switch (e.g.
//! git-review's `Ctrl+T`) invalidates the slot and a hit never needs any
//! value-side re-verification (the key is exact).
//!
//! Owned by the app's model like a
//! [`ScrollState`](rstui_core::ScrollState); attached with
//! [`Diff::syntax_cache`](crate::Diff::syntax_cache). **Without a cache
//! attached the widget behaves exactly as before** (a purely additive,
//! opt-in optimisation), and it is inert unless
//! [`tree_sitter(true)`](crate::Diff::tree_sitter) is also set.
//!
//! ```
//! use rstui_code::{Diff, DiffSyntaxCache};
//! use rstui_core::{Buffer, Rect, Widget};
//!
//! let patch = "--- a/x.rs\n+++ b/x.rs\n@@ -1 +1 @@\n-let a=1;\n+let b=2;\n";
//! // Owned by the model; lives across frames.
//! let cache = DiffSyntaxCache::new();
//!
//! // Every frame: the first misses and parses, the rest are O(1).
//! for _ in 0..60 {
//!     let mut buf = Buffer::empty(Rect::new(0, 0, 32, 6));
//!     Diff::new(patch)
//!         .syntax(true)
//!         .tree_sitter(true)
//!         .syntax_cache(&cache)
//!         .render(buf.area(), &mut buf);
//! }
//! assert_eq!(cache.len(), 1); // one (source,theme) slot, reused 60×
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::diff::{DiffTheme, Tier1Map};

/// Upper bound on memoised slots. Real keys are few (a session reviews a
/// handful of commits, each one patch source × the active theme); clearing
/// wholesale on overflow keeps the cache strictly bounded and total — it
/// simply re-warms lazily — rather than growing without limit over a very
/// long review session. Smaller than the generic `LineCache`'s 128 because a
/// `Tier1Map` is a whole patch's overlays, not a single document's rows.
const MAX_SLOTS: usize = 64;

/// The complete set of inputs a [`Tier1Map`] is a pure function of: the patch
/// source, and a cheap fingerprint of the [`DiffTheme`]'s twelve `syntax_*`
/// [`Style`](rstui_core::Style)s — exactly the `Style`s `syntax_styles(theme)`
/// reads. Hashing only those (not the whole `DiffTheme`) keeps the key the
/// *minimal exact* dependency set: row/gutter/word-mark colours never reach
/// `build_tier1_map`, so changing them must **not** invalidate, while a
/// theme switch that recolours syntax **must** (git-review's `Ctrl+T`).
type Key = (String, u64);

/// A bounded, caller-owned `(source, theme) → Rc<Tier1Map>` read-through
/// memo of [`Diff`](crate::Diff)'s Tier-1 (tree-sitter) overlay.
///
/// Owned by the app's model (the [`ScrollState`](rstui_core::ScrollState)
/// seam, ADR 0012 §P1); the widget reads through it via the crate-internal
/// `resolve` seam. The key is the *complete* set of inputs the map depends
/// on (so a hit is only ever returned for an identical derivation — there is
/// no value-side re-verification, the key is exact). `Rc` so a hit is a
/// refcount bump, not a re-parse, and the borrowed map lives for the one
/// `laid_out` call.
///
/// API mirrors `rstui_widgets::DiagramCache` (`new`/`len`/`is_empty`/`clear`,
/// opt-in, **byte-identical with/without the cache, gate-enforced**) — the
/// one read-through memo, bounded, keyed by the complete inputs.
#[derive(Debug, Default)]
pub struct DiffSyntaxCache {
    slots: RefCell<HashMap<Key, Rc<Tier1Map>>>,
}

impl DiffSyntaxCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many `(source, theme)` slots are memoised. For tests /
    /// introspection.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.borrow().len()
    }

    /// Whether nothing is memoised yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.borrow().is_empty()
    }

    /// Drop every memoised slot (e.g. the reviewed patch was replaced
    /// wholesale — the selected commit changed). The next render re-warms
    /// lazily. Mirrors `DiagramCache::clear` on wholesale content
    /// replacement.
    pub fn clear(&self) {
        self.slots.borrow_mut().clear();
    }

    /// A cheap `u64` fingerprint of exactly the twelve `syntax_*`
    /// [`Style`](rstui_core::Style)s `syntax_styles(theme)` feeds the
    /// analyzer — the *only* part of a [`DiffTheme`] `build_tier1_map`
    /// depends on. `Style` derives [`Hash`], so this is a stable hash of the
    /// minimal dependency set: a theme switch that recolours syntax changes
    /// it (a miss → re-parse), changing an unrelated row/gutter colour does
    /// not (a hit stays valid).
    fn theme_fingerprint(theme: &DiffTheme) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        theme.syntax_string.hash(&mut h);
        theme.syntax_number.hash(&mut h);
        theme.syntax_comment.hash(&mut h);
        theme.syntax_keyword.hash(&mut h);
        theme.syntax_function.hash(&mut h);
        theme.syntax_type.hash(&mut h);
        theme.syntax_constant.hash(&mut h);
        theme.syntax_variable.hash(&mut h);
        theme.syntax_operator.hash(&mut h);
        theme.syntax_punctuation.hash(&mut h);
        theme.syntax_attribute.hash(&mut h);
        theme.syntax_namespace.hash(&mut h);
        h.finish()
    }

    /// The Tier-1 map for `source` under `theme`: a memoised slot if one
    /// exists, else `compute()` is run **once**, stored, and returned. The
    /// returned [`Rc`] is cloned (a refcount bump) — `Diff` borrows the map
    /// out of it for the one `laid_out` pass.
    ///
    /// Internal: the one seam [`Diff`](crate::Diff)'s `laid_out` calls. The
    /// `compute` closure must be deterministic in `(source, theme)` so a hit
    /// and a miss are byte-identical (the `diff` byte-identical test
    /// gate-enforces this).
    pub(crate) fn resolve(
        &self,
        source: &str,
        theme: &DiffTheme,
        compute: impl FnOnce() -> Tier1Map,
    ) -> Rc<Tier1Map> {
        let key = (source.to_owned(), Self::theme_fingerprint(theme));
        if let Some(hit) = self.slots.borrow().get(&key) {
            // Hit cloned under the shared borrow, then it is released.
            return Rc::clone(hit);
        }
        let map = Rc::new(compute());
        let mut slots = self.slots.borrow_mut();
        // Strictly bounded: a pathological number of distinct keys cannot
        // grow this without limit (it re-warms after a wholesale clear).
        if slots.len() >= MAX_SLOTS {
            slots.clear();
        }
        slots.insert(key, Rc::clone(&map));
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Style};

    #[test]
    fn a_miss_computes_and_a_hit_reuses_the_same_map() {
        let cache = DiffSyntaxCache::new();
        let theme = DiffTheme::default();
        let mut calls = 0;
        let first = cache.resolve("--- a/x\n+++ b/x", &theme, || {
            calls += 1;
            let mut m = Tier1Map::new();
            m.insert("x".to_owned(), (HashMap::new(), HashMap::new()));
            m
        });
        let second = cache.resolve("--- a/x\n+++ b/x", &theme, || {
            calls += 1;
            Tier1Map::new() // must NOT run on a hit
        });
        assert_eq!(calls, 1, "compute ran once; the hit reused it");
        assert!(Rc::ptr_eq(&first, &second), "the same Rc is handed back");
        assert_eq!(second.len(), 1);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn source_and_theme_are_both_part_of_the_key() {
        let cache = DiffSyntaxCache::new();
        let a = DiffTheme::default();
        // A theme that differs only in a `syntax_*` colour must be a
        // distinct slot (a Ctrl+T switch invalidates).
        let b = DiffTheme {
            syntax_keyword: Style::new().fg(Color::Red),
            ..DiffTheme::default()
        };

        let _ = cache.resolve("S", &a, Tier1Map::new);
        let _ = cache.resolve("S", &b, Tier1Map::new); // theme differs
        let _ = cache.resolve("T", &a, Tier1Map::new); // source differs
        assert_eq!(cache.len(), 3, "(source, syntax-theme) is the key");

        // The (S, a) slot is still the original, not overwritten.
        let mut ran = false;
        let _ = cache.resolve("S", &a, || {
            ran = true;
            Tier1Map::new()
        });
        assert!(!ran, "an identical (source, theme) is a hit");
    }

    #[test]
    fn a_non_syntax_theme_change_does_not_invalidate() {
        let cache = DiffSyntaxCache::new();
        let a = DiffTheme::default();
        // Change ONLY a non-syntax colour (a row colour): `build_tier1_map`
        // never reads it, so it must stay a hit (the key is the minimal
        // exact dependency set).
        let b = DiffTheme {
            addition: Style::new().fg(Color::Magenta),
            ..DiffTheme::default()
        };

        let first = cache.resolve("S", &a, || {
            let mut m = Tier1Map::new();
            m.insert("k".to_owned(), (HashMap::new(), HashMap::new()));
            m
        });
        let mut ran = false;
        let second = cache.resolve("S", &b, || {
            ran = true;
            Tier1Map::new()
        });
        assert!(!ran, "a non-syntax theme tweak is still a hit");
        assert!(Rc::ptr_eq(&first, &second));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn clear_drops_every_slot_and_it_rewarms() {
        let cache = DiffSyntaxCache::new();
        let theme = DiffTheme::default();
        let _ = cache.resolve("S", &theme, Tier1Map::new);
        assert!(!cache.is_empty());
        cache.clear();
        assert!(cache.is_empty());
        let mut ran = false;
        let _ = cache.resolve("S", &theme, || {
            ran = true;
            Tier1Map::new()
        });
        assert!(ran, "after clear the next resolve recomputes");
    }

    #[test]
    fn overflow_clears_and_stays_bounded() {
        let cache = DiffSyntaxCache::new();
        let theme = DiffTheme::default();
        for i in 0..(MAX_SLOTS + 5) {
            let _ = cache.resolve(&format!("patch {i}"), &theme, Tier1Map::new);
        }
        assert!(
            cache.len() <= MAX_SLOTS,
            "never grows past the bound (got {})",
            cache.len()
        );
    }
}
