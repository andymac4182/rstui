//! [`LineCache`] — the generic caller-owned, key→`Rc<[Line]>` read-through
//! memo behind [`DiagramCache`](crate::DiagramCache) and
//! [`MarkdownCache`](crate::MarkdownCache).
//!
//! Crate-internal. It is the one shared implementation of the
//! caller-owned-cache seam perf-review-3 (R3-1) calls for: the four ad-hoc
//! caches (`Entry.md_cache`, `rstui_ai::ConversationCache`, `DiagramCache`,
//! `Mermaid::from_graph`) were four spellings of *one* idea — memoise an
//! immutable, deterministic per-`(key)` `Vec<Line<'static>>` so an
//! immediate-mode (ADR 0012) widget that re-derives heavy content every
//! frame pays the derivation **once per key**, not once per frame.
//!
//! The widget output stays a pure function of its inputs: a cache hit and a
//! miss return byte-identical lines (the `compute` closure is the unchanged
//! code path); the `RefCell` only elides recomputation, it never changes
//! what is rendered. Each public wrapper gate-enforces that with a
//! cached≡uncached exactness test.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use rstui_core::Line;

/// Upper bound on memoised slots. Real keys are few (a document's source ×
/// the handful of widths it is rendered at across a session); clearing
/// wholesale on overflow keeps the cache strictly bounded and total — it
/// simply re-warms lazily — rather than growing without limit over a very
/// long resized session.
const MAX_SLOTS: usize = 128;

/// A bounded, caller-owned `K → Rc<[Line<'static>]>` read-through memo.
///
/// Owned by the app's model (the [`ScrollState`](rstui_core::ScrollState)
/// seam, ADR 0012 §P1); the widget reads through it via [`resolve`]. `K` is
/// the *complete* set of inputs the cached lines depend on (so a hit is
/// only ever returned for an identical derivation — there is no value-side
/// re-verification, the key is exact). `Rc` so a hit is a refcount bump,
/// not a deep copy, before the per-frame clone into the document.
///
/// [`resolve`]: LineCache::resolve
#[derive(Debug)]
pub(crate) struct LineCache<K: Eq + Hash + Clone> {
    slots: RefCell<HashMap<K, Rc<[Line<'static>]>>>,
}

impl<K: Eq + Hash + Clone> Default for LineCache<K> {
    fn default() -> Self {
        Self {
            slots: RefCell::new(HashMap::new()),
        }
    }
}

impl<K: Eq + Hash + Clone> LineCache<K> {
    /// How many slots are memoised.
    pub(crate) fn len(&self) -> usize {
        self.slots.borrow().len()
    }

    /// Whether nothing is memoised yet.
    pub(crate) fn is_empty(&self) -> bool {
        self.slots.borrow().is_empty()
    }

    /// Drop every memoised slot (the cached content was replaced
    /// wholesale). The next render re-warms lazily.
    pub(crate) fn clear(&self) {
        self.slots.borrow_mut().clear();
    }

    /// The rows for `key`: a memoised slot if one exists, else `compute()`
    /// is run **once**, stored, and returned. The returned [`Rc`] is cloned
    /// (a refcount bump) — the caller clones the individual [`Line`]s out of
    /// it into the document.
    ///
    /// `compute` must be deterministic in `key` so a hit and a miss are
    /// byte-identical (each wrapper's exactness test gate-enforces this).
    pub(crate) fn resolve(
        &self,
        key: K,
        compute: impl FnOnce() -> Vec<Line<'static>>,
    ) -> Rc<[Line<'static>]> {
        if let Some(hit) = self.slots.borrow().get(&key) {
            // Hit cloned under the shared borrow, then it is released.
            return Rc::clone(hit);
        }
        let rows: Rc<[Line<'static>]> = Rc::from(compute());
        let mut slots = self.slots.borrow_mut();
        // Strictly bounded: a pathological number of distinct keys cannot
        // grow this without limit (it re-warms after a wholesale clear).
        if slots.len() >= MAX_SLOTS {
            slots.clear();
        }
        slots.insert(key, Rc::clone(&rows));
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Span;

    fn row(s: &str) -> Line<'static> {
        Line::from(Span::raw(s.to_owned()))
    }

    #[test]
    fn a_miss_computes_once_and_a_hit_reuses_the_same_rc() {
        let cache: LineCache<(String, u16)> = LineCache::default();
        let mut calls = 0;
        let first = cache.resolve(("k".to_owned(), 40), || {
            calls += 1;
            vec![row("A"), row("B")]
        });
        let second = cache.resolve(("k".to_owned(), 40), || {
            calls += 1;
            vec![row("SHOULD-NOT-RUN")]
        });
        assert_eq!(calls, 1, "compute ran once; the hit reused it");
        assert!(Rc::ptr_eq(&first, &second), "the same Rc is handed back");
        assert_eq!(second.len(), 2);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn distinct_keys_are_distinct_slots() {
        let cache: LineCache<(String, u16)> = LineCache::default();
        let _ = cache.resolve(("k".to_owned(), 40), || vec![row("w40")]);
        let _ = cache.resolve(("k".to_owned(), 80), || vec![row("w80")]);
        let _ = cache.resolve(("j".to_owned(), 40), || vec![row("j")]);
        assert_eq!(cache.len(), 3);
        let again = cache.resolve(("k".to_owned(), 40), || vec![row("NO")]);
        assert_eq!(again.first().unwrap().spans[0].content, "w40");
    }

    #[test]
    fn clear_drops_every_slot_and_it_rewarms() {
        let cache: LineCache<u8> = LineCache::default();
        let _ = cache.resolve(1, || vec![row("x")]);
        assert!(!cache.is_empty());
        cache.clear();
        assert!(cache.is_empty());
        let mut ran = false;
        let _ = cache.resolve(1, || {
            ran = true;
            vec![row("x")]
        });
        assert!(ran, "after clear the next resolve recomputes");
    }

    #[test]
    fn overflow_clears_and_stays_bounded() {
        let cache: LineCache<u32> = LineCache::default();
        for i in 0..(MAX_SLOTS as u32 + 5) {
            let _ = cache.resolve(i, || vec![row("r")]);
        }
        assert!(
            cache.len() <= MAX_SLOTS,
            "never grows past the bound (got {})",
            cache.len()
        );
    }
}
