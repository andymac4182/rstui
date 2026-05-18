//! [`DiagramCache`] — the caller-owned `(source, width)` memo for the
//! diagrams [`Markdown::diagrams`](crate::Markdown::diagrams) embeds (the
//! UI-1/MD-1 caller-owned-cache model, ADR 0012 §P1, the
//! `rstui_ai::ConversationCache` / [`ScrollState`](rstui_core::ScrollState)
//! precedent).
//!
//! # The cost this removes
//!
//! `Markdown::diagrams(true)` rasterises every embedded ` ```mermaid `,
//! ` ```structurizr `, or ` ```canvas ` fence by **re-parsing the DSL,
//! re-running the diagram layout engine, and reading a scratch buffer back
//! to styled rows — on every frame**, because a widget is a pure projection
//! that owns nothing across frames. Unlike re-wrapping prose (cheap), a
//! diagram parse+layout is a `widget/markdown/render`-class cost *per
//! diagram*: a doc with one Mermaid + one Structurizr fence measured ~4.4 ms
//! a render, and the kitchen-sink Rich Text screen calls the layout twice a
//! frame (the `max_scroll` clamp + the render), so an *idle* animated screen
//! spent ~9 ms/frame on diagrams alone and dropped to ~9 fps.
//!
//! # The contract
//!
//! A diagram fence's body is **immutable** for a given document, so its
//! rasterised rows are a pure function of `(source, width)`. The cache
//! memoises exactly that: the first frame at a width misses and computes the
//! rows (the unchanged code path — byte-identical); every later frame is an
//! `O(1)` keyed lookup plus a cheap row clone. Because the widget owns the
//! parse (the caller never sees the individual fences, unlike
//! `rstui_ai::ConversationCache`'s message list), the memo is a
//! *read-through* one with interior mutability — the render stays a pure
//! function of its inputs (a hit and a miss produce byte-identical lines,
//! gate-enforced by the `markdown` byte-identical tests), the `RefCell`
//! only elides recomputation.
//!
//! Owned by the app's model like a
//! [`ScrollState`](rstui_core::ScrollState); attached with
//! [`Markdown::diagram_cache`](crate::Markdown::diagram_cache). **Without a
//! cache attached the widget behaves exactly as before** (a purely additive,
//! opt-in optimisation).
//!
//! ```
//! use rstui_core::{Buffer, Rect, Widget};
//! use rstui_widgets::{DiagramCache, Markdown};
//!
//! let doc = "intro\n\n```mermaid\ngraph TD\n  A --> B\n```\n";
//! // Owned by the model; lives across frames.
//! let cache = DiagramCache::new();
//!
//! // Every frame: the first misses and rasterises, the rest are O(1).
//! for _ in 0..60 {
//!     let mut buf = Buffer::empty(Rect::new(0, 0, 24, 12));
//!     Markdown::new(doc)
//!         .diagrams(true)
//!         .diagram_cache(&cache)
//!         .render(buf.area(), &mut buf);
//! }
//! assert_eq!(cache.len(), 1); // one (source,width) slot, reused 60×
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rstui_core::Line;

/// Upper bound on memoised `(source, width)` slots. Embedded diagrams are
/// few and widths change only on resize, so this is never approached in
/// practice; clearing wholesale on overflow keeps the cache strictly
/// bounded and total (it simply re-warms lazily) rather than growing
/// without limit over a very long resized session.
const MAX_SLOTS: usize = 128;

/// `(diagram source, layout width)` → its memoised band-extracted rows.
/// `Rc` so a cache hit is a refcount bump, not a deep copy, before the
/// per-frame row clone into the document line vector. A named alias
/// (clippy `type_complexity`, the `rstui-bench` `Scenario` precedent).
type DiagramSlots = HashMap<(String, u16), Rc<[Line<'static>]>>;

/// The caller-owned memo of rasterised embedded-diagram rows, keyed by the
/// fence body and the layout width. See the [module docs](self).
#[derive(Debug, Default)]
pub struct DiagramCache {
    slots: RefCell<DiagramSlots>,
}

impl DiagramCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many `(source, width)` slots are memoised. For tests /
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

    /// Drop every memoised slot (e.g. the document the diagrams live in was
    /// replaced wholesale). The next render re-warms lazily.
    pub fn clear(&self) {
        self.slots.borrow_mut().clear();
    }

    /// The rasterised rows for `source` at `width`: a memoised slot if one
    /// exists, else `compute()` is run once, stored, and returned. The
    /// returned [`Rc`] is cloned (refcount bump) — the caller clones the
    /// individual [`Line`]s out of it into the document.
    ///
    /// Internal: the one seam `Markdown`'s diagram rasteriser calls. `()`
    /// the compute closure must be deterministic in `(source, width)` so a
    /// hit and a miss are byte-identical (the gate test enforces this).
    pub(crate) fn resolve(
        &self,
        source: &str,
        width: u16,
        compute: impl FnOnce() -> Vec<Line<'static>>,
    ) -> Rc<[Line<'static>]> {
        if let Some(hit) = self.slots.borrow().get(&(source.to_owned(), width)) {
            // Hit cloned under the shared borrow, then it is released.
            return Rc::clone(hit);
        }
        let rows: Rc<[Line<'static>]> = Rc::from(compute());
        let mut slots = self.slots.borrow_mut();
        // Strictly bounded: a pathological number of distinct widths cannot
        // grow this without limit (it re-warms after a wholesale clear).
        if slots.len() >= MAX_SLOTS {
            slots.clear();
        }
        slots.insert((source.to_owned(), width), Rc::clone(&rows));
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
    fn a_miss_computes_and_a_hit_reuses_the_same_rows() {
        let cache = DiagramCache::new();
        let calls = RefCell::new(0);
        let mk = || {
            *calls.borrow_mut() += 1;
            vec![row("A"), row("B")]
        };
        let first = cache.resolve("graph TD\nA-->B", 40, mk);
        let second = cache.resolve("graph TD\nA-->B", 40, || {
            *calls.borrow_mut() += 1;
            vec![row("DIFFERENT")] // must NOT run on a hit
        });
        assert_eq!(*calls.borrow(), 1, "compute ran once; the hit reused it");
        assert!(Rc::ptr_eq(&first, &second), "the same Rc is handed back");
        assert_eq!(second.len(), 2);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn width_and_source_are_both_part_of_the_key() {
        let cache = DiagramCache::new();
        let _ = cache.resolve("S", 40, || vec![row("w40")]);
        let _ = cache.resolve("S", 80, || vec![row("w80")]); // width differs
        let _ = cache.resolve("T", 40, || vec![row("srcT")]); // source differs
        assert_eq!(cache.len(), 3, "(source,width) is the key");
        // The width-40 slot is still the original, not overwritten.
        let again = cache.resolve("S", 40, || vec![row("SHOULD-NOT-RUN")]);
        assert_eq!(again.first().unwrap().spans[0].content, "w40");
    }

    #[test]
    fn clear_drops_every_slot_and_it_rewarms() {
        let cache = DiagramCache::new();
        let _ = cache.resolve("S", 10, || vec![row("x")]);
        assert!(!cache.is_empty());
        cache.clear();
        assert!(cache.is_empty());
        let mut ran = false;
        let _ = cache.resolve("S", 10, || {
            ran = true;
            vec![row("x")]
        });
        assert!(ran, "after clear the next resolve recomputes");
    }

    #[test]
    fn overflow_clears_and_stays_bounded() {
        let cache = DiagramCache::new();
        for i in 0..(MAX_SLOTS + 5) {
            let _ = cache.resolve("S", u16::try_from(i).unwrap(), || vec![row("r")]);
        }
        assert!(
            cache.len() <= MAX_SLOTS,
            "never grows past the bound (got {})",
            cache.len()
        );
    }
}
