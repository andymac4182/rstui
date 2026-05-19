//! [`ProjectionCache`] — the caller-owned memo of a [`DataTable`]'s
//! [`project`](crate::data_table::project) result (DT-OPT-1, the
//! ADR 0012 §P1 / ADR 0025 caller-owned-cache seam; the
//! [`MarkdownCache`](crate::MarkdownCache) / `LineCache` shape, here a
//! single slot because a grid has exactly one *current* projection).
//!
//! # The cost this removes
//!
//! `project()` is the once-per-state-change pipeline (filter → group → sort
//! → collapse). Its dominant term is the multi-key sort; even with the
//! per-comparison `String` allocation removed (DT-OPT-1's in-`project`
//! precompute), a re-sort of a million rows is hundreds of milliseconds.
//! A scroll, a selection move, or a redraw that re-derives the projection
//! with **unchanged inputs** should pay `O(1)`, not re-run the pipeline —
//! exactly the immediate-mode caller-owned-cache discipline `Markdown`,
//! `Diagram`, and `rstui_ai::ConversationCache` already use.
//!
//! # The contract
//!
//! The projection is a pure, deterministic function of `(columns, rows, the
//! projecting state)`. The cache fingerprints every input
//! [`project`](crate::data_table::project) reads — the `rows`/`columns`
//! slice identity (pointer + length) and the projecting
//! [`DataTableState`](crate::DataTableState) fields (sort keys, filter,
//! grouping column + direction, collapsed groups). An identical fingerprint
//! *is* an identical projection (no value-side re-verification).
//!
//! Like [`MarkdownCache`](crate::MarkdownCache)'s source-keyed contract, an
//! **in-place mutation of a row's contents** (same `Vec`, same length, edited
//! cell text) does not change the slice identity, so the caller must
//! [`clear`](ProjectionCache::clear) the cache when it writes a cell back —
//! which is the same single place it already re-projects (its `reproject`
//! chokepoint). **Without a cache attached `project()` is unchanged** — this
//! is a purely additive, opt-in optimisation, gate-enforced cached≡uncached.
//!
//! ```
//! use rstui_widgets::{DataColumn, DataRow, DataTableState, ProjectionCache};
//! use rstui_widgets::data_table::project_cached;
//!
//! let cols = [DataColumn::new("a")];
//! let rows = [DataRow::new(["x"]), DataRow::new(["y"])];
//! let state = DataTableState::new();
//! let cache = ProjectionCache::new(); // owned by the model, lives across frames
//! let a = project_cached(&cols, &rows, &state, &cache);
//! let b = project_cached(&cols, &rows, &state, &cache); // O(1) hit
//! assert_eq!(a, b);
//! ```

use std::cell::RefCell;

use crate::SortDirection;
use crate::data_table::{VisualRow, project};
use crate::{DataColumn, DataRow, DataTableState};

/// The complete set of inputs [`project`](crate::data_table::project)
/// derives its output from — the exact cache fingerprint (an identical
/// fingerprint *is* an identical projection; there is no value-side
/// re-verification).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjKey {
    rows_ptr: usize,
    rows_len: usize,
    cols_len: usize,
    sort: Vec<(usize, SortDirection)>,
    filter: String,
    group_by: Option<usize>,
    group_dir: SortDirection,
    collapsed: Vec<String>,
}

impl ProjKey {
    fn of(columns: &[DataColumn], rows: &[DataRow], state: &DataTableState) -> Self {
        Self {
            rows_ptr: rows.as_ptr() as usize,
            rows_len: rows.len(),
            cols_len: columns.len(),
            sort: state.sort_keys().to_vec(),
            filter: state.filter().to_owned(),
            group_by: state.grouped_by(),
            group_dir: state.group_direction(),
            collapsed: state.collapsed().to_vec(),
        }
    }
}

/// The caller-owned single-slot memo of a [`DataTable`](crate::DataTable)'s
/// flattened projection. See the [module docs](self). Owned by the app's
/// model like a [`ScrollState`](rstui_core::ScrollState); read through it
/// with [`project_cached`](crate::data_table::project_cached).
#[derive(Debug, Default)]
pub struct ProjectionCache {
    slot: RefCell<Option<(ProjKey, Vec<VisualRow>)>>,
}

impl ProjectionCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a projection is currently memoised.
    #[must_use]
    pub fn is_warm(&self) -> bool {
        self.slot.borrow().is_some()
    }

    /// Drop the memoised projection — call this when the row **contents**
    /// were mutated in place (the slice identity is unchanged, so the
    /// fingerprint cannot see the edit). The next call re-projects.
    pub fn clear(&self) {
        *self.slot.borrow_mut() = None;
    }

    /// The projection for these exact inputs: the memoised slot when its
    /// fingerprint matches, else [`project`](crate::data_table::project) is
    /// run **once** on its unchanged code path, stored, and returned. A hit
    /// and a miss are byte-identical (gate-enforced).
    pub(crate) fn resolve(
        &self,
        columns: &[DataColumn],
        rows: &[DataRow],
        state: &DataTableState,
    ) -> Vec<VisualRow> {
        let key = ProjKey::of(columns, rows, state);
        if let Some((k, v)) = self.slot.borrow().as_ref() {
            if *k == key {
                return v.clone();
            }
        }
        let projected = project(columns, rows, state);
        *self.slot.borrow_mut() = Some((key, projected.clone()));
        projected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Vec<DataColumn<'static>>, Vec<DataRow<'static>>) {
        let cols = vec![DataColumn::new("name"), DataColumn::new("team")];
        let rows = vec![
            DataRow::new(["bea", "x"]),
            DataRow::new(["ada", "y"]),
            DataRow::new(["cy", "x"]),
        ];
        (cols, rows)
    }

    #[test]
    fn a_hit_is_byte_identical_to_an_uncached_project() {
        let (cols, rows) = fixture();
        let cache = ProjectionCache::new();
        for spec in [
            DataTableState::new(),
            {
                let mut s = DataTableState::new();
                s.set_sort(Some((0, SortDirection::Ascending)));
                s
            },
            {
                let mut s = DataTableState::new();
                s.set_filter("x");
                s
            },
            {
                let mut s = DataTableState::new();
                s.set_group_by(Some(1));
                s.set_sort(Some((0, SortDirection::Descending)));
                s
            },
        ] {
            cache.clear();
            let uncached = project(&cols, &rows, &spec);
            let miss = cache.resolve(&cols, &rows, &spec);
            let hit = cache.resolve(&cols, &rows, &spec);
            assert_eq!(uncached, miss, "a miss equals an uncached project");
            assert_eq!(uncached, hit, "a hit equals an uncached project");
        }
    }

    #[test]
    fn a_changed_input_misses_and_re_projects() {
        let (cols, rows) = fixture();
        let cache = ProjectionCache::new();
        let mut s = DataTableState::new();
        let a = cache.resolve(&cols, &rows, &s);
        s.set_sort(Some((0, SortDirection::Ascending)));
        let b = cache.resolve(&cols, &rows, &s); // sort changed ⇒ miss
        assert_ne!(a, b, "a changed sort key re-projects");
        assert_eq!(b, project(&cols, &rows, &s));
    }

    #[test]
    fn clear_forces_a_re_projection() {
        let (cols, rows) = fixture();
        let cache = ProjectionCache::new();
        let s = DataTableState::new();
        let _ = cache.resolve(&cols, &rows, &s);
        assert!(cache.is_warm());
        cache.clear();
        assert!(!cache.is_warm());
        assert_eq!(cache.resolve(&cols, &rows, &s), project(&cols, &rows, &s));
    }
}
