//! [`MarkdownCache`] — the caller-owned per-`(source, width, …)` memo for
//! [`Markdown::lines`](crate::Markdown::lines) (perf-review-3 R3-2, the
//! UI-1/MD-1 caller-owned-cache model, ADR 0012 §P1; the
//! [`DiagramCache`](crate::DiagramCache) / `LineCache` shape).
//!
//! # The cost this removes
//!
//! `Markdown` is immediate-mode pure projection: `lines(width)` runs the
//! whole hand-written CommonMark parser (`collect_defs` + `blocks_into`)
//! **and** the width-aware layout (`layout_blocks`) **every frame** —
//! ~1.5 ms for a real document (`widget/markdown/render`), the single
//! heaviest widget by ~30× and ~18 % of a 120 fps (8.33 ms) frame, and a
//! scroll-clamp screen calls it *twice* a frame. Unlike re-wrapping prose
//! (cheap), parsing a *language* is not free, and it is re-done from
//! scratch and ~94 % discarded every frame.
//!
//! # The contract
//!
//! A document's rendered lines are a pure, deterministic function of
//! **(source, width, focused-link, `diagrams`, theme)** — every input
//! `lines()` reads. The cache memoises exactly that tuple: the first frame
//! at a given key misses and computes it on the **unchanged code path**
//! (byte-identical); every later frame is an `O(1)` lookup plus a cheap row
//! clone. `focused_link` and the [`MarkdownTheme`]
//! are part of the key because the parse itself depends on them (a
//! source-only cache would render stale link highlighting — the real
//! review-1 *MD-1 wrinkle*, here made explicit and impossible to get
//! wrong).
//!
//! Owned by the app's model like a
//! [`ScrollState`](rstui_core::ScrollState); attached with
//! [`Markdown::cache`](crate::Markdown::cache). **Without a cache attached
//! the widget behaves exactly as before** — a purely additive, opt-in
//! optimisation, gate-enforced byte-identical (`markdown_cache_*` tests).
//!
//! ```
//! use rstui_core::{Buffer, Rect, Widget};
//! use rstui_widgets::{Markdown, MarkdownCache};
//!
//! let doc = "# Title\n\nA *paragraph* with a [link](x) and a list:\n\n- a\n- b\n";
//! let cache = MarkdownCache::new(); // owned by the model, lives across frames
//! for _ in 0..120 {
//!     let mut buf = Buffer::empty(Rect::new(0, 0, 32, 16));
//!     Markdown::new(doc).cache(&cache).render(buf.area(), &mut buf);
//! }
//! assert_eq!(cache.len(), 1); // parsed+laid-out once, reused 120×
//! ```

use std::rc::Rc;

use rstui_core::Line;

use crate::MarkdownTheme;
use crate::line_cache::LineCache;

/// The complete set of inputs [`Markdown::lines`](crate::Markdown::lines)
/// derives its output from — the exact cache key (no value-side
/// re-verification: an identical key *is* an identical render).
///
/// `diagram_cache` is deliberately **not** here: attaching a
/// [`DiagramCache`](crate::DiagramCache) only memoises work *inside* the
/// computation, it never changes the produced lines, so it is not an input
/// to the result.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MdKey {
    source: String,
    width: u16,
    focused_link: Option<usize>,
    diagrams: bool,
    theme: MarkdownTheme,
}

/// The caller-owned memo of a [`Markdown`](crate::Markdown) document's
/// laid-out lines. See the [module docs](self).
#[derive(Debug, Default)]
pub struct MarkdownCache {
    inner: LineCache<MdKey>,
}

impl MarkdownCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many distinct `(source, width, …)` renders are memoised. For
    /// tests / introspection.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether nothing is memoised yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Drop every memoised render (the document was replaced wholesale).
    /// The next frame re-warms lazily.
    pub fn clear(&self) {
        self.inner.clear();
    }

    /// The laid-out lines for this exact `(source, width, focused_link,
    /// diagrams, theme)`: a memoised slot if one exists, else `compute()`
    /// runs once on the unchanged `lines()` path, is stored, and returned.
    ///
    /// Internal: the one seam [`Markdown::lines`](crate::Markdown::lines)
    /// calls. `compute` is the unmodified parse+layout, so a hit and a miss
    /// are byte-identical (gate-enforced).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve(
        &self,
        source: &str,
        width: u16,
        focused_link: Option<usize>,
        diagrams: bool,
        theme: &MarkdownTheme,
        compute: impl FnOnce() -> Vec<Line<'static>>,
    ) -> Rc<[Line<'static>]> {
        self.inner.resolve(
            MdKey {
                source: source.to_owned(),
                width,
                focused_link,
                diagrams,
                theme: *theme,
            },
            compute,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_full_input_tuple_is_the_key() {
        let cache = MarkdownCache::new();
        let theme = MarkdownTheme::default();
        let other = MarkdownTheme {
            link: rstui_core::Style::new().fg(rstui_core::Color::Red),
            ..MarkdownTheme::default()
        };

        let mk = |tag: &'static str| move || vec![Line::from(rstui_core::Span::raw(tag))];
        // Base render.
        let _ = cache.resolve("src", 40, None, false, &theme, mk("base"));
        // A hit: identical key reuses it (compute must not run).
        let hit = cache.resolve("src", 40, None, false, &theme, mk("NO"));
        assert_eq!(hit.first().unwrap().spans[0].content, "base");
        assert_eq!(cache.len(), 1);
        // Each of the five inputs is part of the key ⇒ a distinct slot.
        let _ = cache.resolve("OTHER", 40, None, false, &theme, mk("a"));
        let _ = cache.resolve("src", 41, None, false, &theme, mk("b"));
        let _ = cache.resolve("src", 40, Some(0), false, &theme, mk("c"));
        let _ = cache.resolve("src", 40, None, true, &theme, mk("d"));
        let _ = cache.resolve("src", 40, None, false, &other, mk("e"));
        assert_eq!(cache.len(), 6, "source/width/focus/diagrams/theme all key");
    }

    #[test]
    fn clear_drops_slots_and_rewarms() {
        let cache = MarkdownCache::new();
        let t = MarkdownTheme::default();
        let _ = cache.resolve("s", 10, None, false, &t, || vec![Line::default()]);
        assert!(!cache.is_empty());
        cache.clear();
        assert!(cache.is_empty());
        let mut ran = false;
        let _ = cache.resolve("s", 10, None, false, &t, || {
            ran = true;
            vec![Line::default()]
        });
        assert!(ran, "after clear the next resolve recomputes");
    }
}
