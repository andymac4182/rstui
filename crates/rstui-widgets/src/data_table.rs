//! [`DataTable`] — the comprehensive interactive data grid: sortable,
//! filterable, groupable, mouse-hit-testable, virtualized for fast scroll,
//! with **any form field per cell** — text, [`Checkbox`], [`Switch`], a
//! [`Select`](crate::Select) dropdown ([`CellField`]), or *any* other widget via the
//! [`cell_rect`](DataTable::cell_rect) accessor. The "spreadsheet pane" to
//! [`Table`](crate::Table)'s "aligned rows".
//!
//! # Why a second grid widget (and not a bigger [`Table`](crate::Table))
//!
//! [`Table`](crate::Table) is deliberately minimal — a total pure projection
//! of caller-placed [`Row`](crate::Row)s with single-row selection and nothing
//! else (ADR 0012 keeps it that way on purpose). Sorting, filtering, grouping,
//! and editing are not extra knobs on that shape; they introduce a *data
//! pipeline* and an *edit lifecycle*. Bolting them onto `Table` would either
//! make every caller pay for them or smuggle render-time mutation into a
//! widget the whole framework relies on being inert. So this is its own
//! widget, exactly as [`MaskedInput`](crate::MaskedInput) is a sibling of
//! [`Input`] rather than a flag on it.
//!
//! # The pure-projection contract holds (ADR 0012)
//!
//! Nothing here breaks immediate-mode. `DataTable` never sorts, filters,
//! groups, scrolls, or edits at render time — it is a **pure projection of
//! caller-owned state**, the same discipline [`Tree`](crate::Tree) uses for
//! its flattened `Vec`:
//!
//! * The *data pipeline* is the reducer's job. The caller owns the source
//!   `[DataRow]` and a [`DataTableState`]; in `update` it calls the pure,
//!   total [`project`] engine **once per data/spec change** to produce the
//!   flattened `[VisualRow]` (group headers interleaved with data rows,
//!   filtered, sorted, collapsed). The widget is handed that projection and
//!   only *reads* it — identical to how the `Tree` reducer splices children
//!   into a flattened list. Render is therefore O(visible window), never
//!   O(rows): scrolling a million-row table is as cheap as scrolling ten.
//! * *Scrolling* is a composed [`ScrollState`] — the
//!   reducer mutates it (wheel tick, `PageDown`, `End`), the widget reads
//!   `offset`.
//! * *Any form field per cell.* A column declares a [`CellField`] — `Text`
//!   (a borrowed [`TextEdit`] edited via a reused [`Input`], the original
//!   behaviour and still the default), `Checkbox`, `Switch`, or a `Select`
//!   dropdown. The widget renders each by **reusing the matching widget**
//!   (no second implementation); the cell's [`Line`] stays the single value
//!   of record (so sort/filter keep working and the reducer writes edits
//!   back into it — booleans via [`cell_truthy`], the dropdown via
//!   [`CellSelectState::choose`]). For *any other* control — a
//!   [`Slider`](crate::Slider), [`Radio`](crate::Radio),
//!   [`DatePicker`](crate::DatePicker), a bespoke widget — the
//!   [`cell_rect`](DataTable::cell_rect) accessor returns the cell's exact
//!   on-screen rect so the caller renders it after the table (the ADR 0012
//!   accessor escape hatch; total for any widget, no new contract). All of
//!   it stays a pure projection: the reducer owns every control's state
//!   ([`TextEdit`]/a bool in the cell/[`CellSelectState`]); the widget only
//!   reads it.
//! * *Mouse* and *change events* are surfaced as **pure geometry accessors**
//!   ([`hit`](DataTable::hit), [`cell_rect`](DataTable::cell_rect)), the
//!   recorded ADR 0012 model ([`SplitPane::divider_rect`](crate::SplitPane::divider_rect)
//!   / [`ScrollView::viewport`](crate::ScrollView::viewport) precedent). There
//!   are deliberately **no callbacks**: a callback is render-time mutation by
//!   another name and cannot exist in a `view(&self)`. The app maps a
//!   [`MouseEvent`](rstui_core::event::MouseEvent) (or a key) through
//!   [`hit`](DataTable::hit) in `update` and mutates its own data + state —
//!   that *is* the event/change hook (see [`DataTableState`] for the full
//!   edit cycle).
//!
//! # Total, never a panic
//!
//! Per the iter-25 rule every other primitive obeys
//! ([`ScrollState`]/[`Selection`](rstui_core::Selection)/[`Table`](crate::Table)):
//! an out-of-range sort/group column, a filter that matches nothing, a
//! selection or edit position off the visible window, an over-scroll, a
//! zero/one-cell area, ragged rows (fewer/more cells than columns) — all are
//! well-defined no-ops or clips, proved by a fixed-seed fuzz test.
//!
//! # Deliberately deferred (clean additives, not gaps)
//!
//! Recorded so they are not re-litigated or smuggled into this slice (the
//! [`Tree`](crate::Tree) "defer cleanly" precedent): **multi-key sort** and
//! **typed/numeric comparators** (this slice is stable single-column
//! lexicographic — deterministic and dependency-free); **per-column / regex
//! filtering** (this slice is one case-insensitive substring across all
//! cells); **multi-level grouping**; **2-D cell/column selection**; **column
//! resize/reorder by drag**; a **reserved selection gutter symbol** (the
//! [`Table`](crate::Table)/[`Tree`](crate::Tree) `highlight_symbol`). Each is
//! a future additive over this exact shape.

use std::borrow::Cow;

use crate::block::Block;
use crate::checkbox::Checkbox;
use crate::input::Input;
use crate::list::List;
use crate::switch::Switch;
use rstui_core::{
    Buffer, Constraint, Layout, Line, Position, Rect, ScrollState, Style, TextEdit, Widget,
};

/// Whether a cell's text reads as "true" for a [`CellField::Checkbox`] /
/// [`CellField::Switch`] column.
///
/// The cell's [`Line`] text *is* the boolean (so sorting/filtering keep
/// working and the reducer toggles by flipping that text — the same
/// write-back hook a text edit uses). Trimmed, case-insensitive, the truthy
/// set is `1` · `true` · `t` · `yes` · `y` · `x` · `on` · `✓` · `✔`;
/// everything else (including empty) is `false`.
#[must_use]
pub fn cell_truthy(text: &str) -> bool {
    matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "t" | "yes" | "y" | "x" | "on" | "✓" | "✔"
    )
}

/// What control a [`DataColumn`]'s cells are — so a column can be plain
/// text, a checkbox, a switch, or a dropdown. The widget renders each by
/// **reusing** the matching widget ([`Input`]/[`Checkbox`]/[`Switch`]/
/// [`Select`](crate::Select)); the cell's [`Line`] is always the value of record (the
/// reducer owns it). For *any other* control (a [`Slider`](crate::Slider),
/// [`Radio`](crate::Radio), [`DatePicker`](crate::DatePicker), a custom
/// widget) use [`DataTable::cell_rect`] to get the cell's on-screen rect and
/// render it yourself after the table — the ADR 0012 accessor escape hatch,
/// total for any widget.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum CellField<'a> {
    /// Plain text. Edits via the borrowed [`TextEdit`]
    /// ([`DataTable::edit`]) when the column is
    /// [`editable`](DataColumn::editable) and the cell is being edited; the
    /// default, byte-for-byte the original behaviour.
    #[default]
    Text,
    /// A boolean drawn as a [`Checkbox`] on every row (value =
    /// [`cell_truthy`] of the cell text). Clicking it is a
    /// [`Cell`](DataTableHit::Cell) hit the reducer flips.
    Checkbox,
    /// A boolean drawn as a [`Switch`] (value = [`cell_truthy`]).
    Switch,
    /// One choice from these options. Closed it shows the cell value with a
    /// `▾`; while the cell is being edited and the caller-owned
    /// [`CellSelectState`] ([`DataTable::cell_select`]) is open it drops a
    /// reused [`Select`](crate::Select) panel.
    Select(Vec<Cow<'a, str>>),
}

impl<'a> CellField<'a> {
    /// A [`Select`](Self::Select) column over `options` (anything a
    /// [`Cow<str>`] is built from).
    pub fn select<I, T>(options: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Cow<'a, str>>,
    {
        Self::Select(options.into_iter().map(Into::into).collect())
    }
}

/// The caller-owned open/highlight state of a [`CellField::Select`] cell
/// that is currently being edited — the dropdown sibling of
/// [`ScrollState`]/[`DataTableState`]: a pure value type the reducer
/// mutates (a click/`Enter` opens, arrows move the highlight, `Enter`
/// chooses) and the widget only reads. Every method is **total**.
///
/// `selected` is *not* stored here — it lives in the cell's data [`Line`]
/// like every other value (so sort/filter keep working); `choose` returns
/// the picked option index and the reducer writes it back, exactly the
/// text-edit `commit` hook.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CellSelectState {
    open: bool,
    highlight: usize,
    offset: usize,
}

impl CellSelectState {
    /// A fresh, closed dropdown (identical to [`Default`]).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the panel is dropped.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The highlighted option index (what `Enter` would choose).
    #[must_use]
    pub fn highlight(&self) -> usize {
        self.highlight
    }

    /// The panel scroll offset (first visible option).
    #[must_use]
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Opens the panel with `current` (the cell's existing choice, if any)
    /// pre-highlighted.
    pub fn open(&mut self, current: Option<usize>) {
        self.open = true;
        self.highlight = current.unwrap_or(0);
        self.offset = 0;
    }

    /// Closes the panel without choosing.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Moves the highlight by `delta`, saturating into `0..len` (a `len` of
    /// `0` parks at `0`). Total for any input.
    pub fn move_highlight(&mut self, delta: isize, len: usize) {
        let last = len.saturating_sub(1);
        let cur = self.highlight.min(last);
        self.highlight = if delta >= 0 {
            cur.saturating_add(delta.unsigned_abs()).min(last)
        } else {
            cur.saturating_sub(delta.unsigned_abs())
        };
    }

    /// Keeps the highlighted row visible in a `viewport`-row panel (call
    /// after [`move_highlight`](Self::move_highlight)); total for any sizes.
    pub fn reveal(&mut self, viewport: usize, len: usize) {
        if viewport == 0 {
            self.offset = 0;
            return;
        }
        if self.highlight < self.offset {
            self.offset = self.highlight;
        } else if self.highlight >= self.offset.saturating_add(viewport) {
            self.offset = self.highlight.saturating_sub(viewport - 1);
        }
        self.offset = self.offset.min(len.saturating_sub(viewport));
    }

    /// Closes the panel and returns the chosen option index (the current
    /// highlight) so the reducer can write it into the cell's data — the
    /// dropdown's "commit" hook. `None` only when there are no options.
    pub fn choose(&mut self, len: usize) -> Option<usize> {
        self.open = false;
        (len > 0).then(|| self.highlight.min(len - 1))
    }
}

/// The order [`project`] sorts the active column in.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortDirection {
    /// Smallest/A→Z first.
    #[default]
    Ascending,
    /// Largest/Z→A first.
    Descending,
}

/// One column: a header [`Line`], a width [`Constraint`], and whether the
/// consumer allows its cells to be edited.
///
/// `editable` is the opt-in the caller sets when describing the data — the
/// widget only ever projects a *visual* editor when **both** the column is
/// `editable` and the caller-owned [`DataTableState::editing`] points at a
/// cell in it. A column is not editable by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataColumn<'a> {
    header: Line<'a>,
    width: Constraint,
    editable: bool,
    field: CellField<'a>,
}

impl<'a> DataColumn<'a> {
    /// A column titled `header` (anything a [`Line`] is built from), an equal
    /// [`Fill(1)`](Constraint::Fill) share of the width, not editable.
    pub fn new(header: impl Into<Line<'a>>) -> Self {
        Self {
            header: header.into(),
            width: Constraint::Fill(1),
            editable: false,
            field: CellField::Text,
        }
    }

    /// Sets this column's width [`Constraint`] (resolved through the same
    /// deterministic [`Layout`] divider top-level layout uses, so it is
    /// total and clamps rather than panicking).
    #[must_use]
    pub fn width(mut self, width: Constraint) -> Self {
        self.width = width;
        self
    }

    /// Marks this column's cells editable (default `false`). Editing still
    /// only happens when the reducer points [`DataTableState::editing`] at a
    /// cell here and supplies the [`TextEdit`](DataTable::edit) — the widget
    /// never edits anything itself.
    #[must_use]
    pub fn editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }

    /// Whether this column was marked [`editable`](Self::editable).
    #[must_use]
    pub fn is_editable(&self) -> bool {
        self.editable
    }

    /// Sets the cell control for this column (default [`CellField::Text`] —
    /// the original behaviour). [`Checkbox`](CellField::Checkbox)/
    /// [`Switch`](CellField::Switch)/[`Select`](CellField::Select) are drawn
    /// by reusing the matching widget; the cell [`Line`] stays the value the
    /// reducer owns.
    #[must_use]
    pub fn field(mut self, field: CellField<'a>) -> Self {
        self.field = field;
        self
    }

    /// This column's [`CellField`].
    #[must_use]
    pub fn cell_field(&self) -> &CellField<'a> {
        &self.field
    }
}

/// One source row: its cells (one [`Line`] per column, the
/// [`Row`](crate::Row) precedent) plus an optional explicit group key and a
/// row-wide base [`Style`].
///
/// The caller owns a `Vec<DataRow>` in stable order; [`project`] never
/// reorders this `Vec` — it returns *indices into it* ([`VisualRow`]), so a
/// source index is a stable identity that survives a re-sort (this is what
/// keeps [`DataTableState::editing`] pinned to the right cell while the user
/// sorts).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DataRow<'a> {
    cells: Vec<Line<'a>>,
    group: Option<Cow<'a, str>>,
    style: Style,
}

impl<'a> DataRow<'a> {
    /// A row whose cells are `cells` (each convertible to a [`Line`]), in no
    /// group, unstyled.
    pub fn new<I, T>(cells: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Line<'a>>,
    {
        Self {
            cells: cells.into_iter().map(Into::into).collect(),
            group: None,
            style: Style::default(),
        }
    }

    /// Sets an explicit group key. When [`DataTableState`] groups by a
    /// column, an explicit key here overrides that column's text — useful for
    /// grouping by something that is not a visible cell (a date bucket, a
    /// status tier).
    #[must_use]
    pub fn group(mut self, key: impl Into<Cow<'a, str>>) -> Self {
        self.group = Some(key.into());
        self
    }

    /// Sets the row's base [`Style`], beneath the table → row → cell → span
    /// cascade.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The cell [`Line`] at `column`, if the row has one.
    #[must_use]
    pub fn cell(&self, column: usize) -> Option<&Line<'a>> {
        self.cells.get(column)
    }
}

/// One row of the flattened projection [`project`] produces and the widget
/// renders: either a group header or a data row (an index back into the
/// caller's stable source `Vec`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisualRow {
    /// A collapsible group header: its `key`, how many data rows it holds,
    /// and whether it is currently collapsed (its members omitted below it).
    Group {
        /// The group key (an explicit [`DataRow::group`] or the group
        /// column's text).
        key: String,
        /// Number of data rows in this group (shown even when collapsed).
        count: usize,
        /// Whether the group's members are hidden.
        collapsed: bool,
    },
    /// A data row: an index into the caller-owned source `[DataRow]`.
    Data {
        /// Index into the source `Vec` the caller passed to [`project`].
        source: usize,
    },
}

/// What a screen position resolved to — the single mouse/keyboard *change
/// hook* (ADR 0012 pure-accessor model; there are deliberately no callbacks).
///
/// The reducer maps a click through [`DataTable::hit`] and acts:
/// [`Header`](Self::Header) → [`DataTableState::toggle_sort`];
/// [`Group`](Self::Group) → [`DataTableState::toggle_collapse`];
/// [`Cell`](Self::Cell) → [`DataTableState::select`] and, when the column is
/// editable, [`DataTableState::begin_edit`];
/// [`DropdownOption`](Self::DropdownOption) → write that option into the
/// cell and [`CellSelectState::close`].
///
/// An open [`CellField::Select`] panel floats **over** the rows beneath the
/// field, so [`hit`](DataTable::hit) tests it **first**: a click inside the
/// panel is a [`DropdownOption`](Self::DropdownOption), never the data row it
/// happens to cover (the off-by-the-row-below bug this prevents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataTableHit {
    /// The header cell of this column index (toggle its sort).
    Header(usize),
    /// The group-header row at this visual index (toggle its collapse).
    Group(usize),
    /// A data cell: its visual-row index, the source-row index, and the
    /// column. `source` is the stable id to write an edit back through.
    Cell {
        /// Index into the projected `[VisualRow]`.
        visual: usize,
        /// Index into the caller's source `[DataRow]`.
        source: usize,
        /// Column index.
        column: usize,
    },
    /// A click landed on the open dropdown panel of the
    /// [`CellField::Select`] cell being edited: choose option `index` for
    /// cell `(source, column)`. Resolved against the same panel geometry
    /// the overlay renders, so the option clicked is the option chosen.
    DropdownOption {
        /// Index into the caller's source `[DataRow]` (the edited cell).
        source: usize,
        /// The edited cell's column.
        column: usize,
        /// The chosen option's index into the column's
        /// [`CellField::Select`] options.
        index: usize,
    },
}

/// The whole caller-owned interaction state — the `ScrollState`/`Selection`
/// sibling, specific to a data grid (so it lives here, not in dependency-free
/// `rstui-core`). A pure value type designed to be a field in the app model,
/// mutated only by `update`, read by the pure `view`. Every method is
/// **total**.
///
/// `new()` equals [`Default`] (the [`Selection`](rstui_core::Selection)
/// convention — there is no useful non-inert starting state for a grid).
///
/// # The edit cycle (the "pass the changes back" hooks)
///
/// The consumer owns its data; the widget never writes it. The full loop, all
/// in the reducer:
///
/// 1. A click on an editable [`Cell`](DataTableHit::Cell) (resolved via
///    [`DataTable::hit`]) → `state.`[`begin_edit`](Self::begin_edit)`(source,
///    column)` and seed a model-owned [`TextEdit`] from
///    that cell.
/// 2. Keystrokes route to that `TextEdit` (the [`Input`] model).
/// 3. `Enter` → read `text_edit.value()`, **write it into your own
///    `Vec<DataRow>`**, then `state.`[`commit_edit`](Self::commit_edit) —
///    which returns the `(source, column)` just committed so you know exactly
///    what to persist — and re-run [`project`].
/// 4. `Esc` → [`cancel_edit`](Self::cancel_edit) (no write-back).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DataTableState {
    /// Ordered sort keys (primary first). Empty = unsorted. Within a group
    /// these order the rows; ungrouped they order the whole table.
    sort: Vec<(usize, SortDirection)>,
    filter: String,
    /// The grouping column — chosen **independently** of the sort keys.
    group_by: Option<usize>,
    /// The order the *groups themselves* are listed in (tier 1); the sort
    /// keys then order rows *within* each group (tier 2).
    group_dir: SortDirection,
    collapsed: Vec<String>,
    selected: Option<usize>,
    vertical: ScrollState,
    editing: Option<(usize, usize)>,
}

impl DataTableState {
    /// A fresh, inert state (identical to [`Default`]): no sort, no filter,
    /// no grouping, nothing selected, scrolled to the top, not editing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // ---- sorting (ordered multi-key; primary first) ----

    /// The **primary** sort `(column, direction)`, or `None` — the
    /// back-compatible single-key view (the header arrow, the simple
    /// "sorted by" readout). Use [`sort_keys`](Self::sort_keys) for the
    /// full ordered list.
    #[must_use]
    pub fn sort(&self) -> Option<(usize, SortDirection)> {
        self.sort.first().copied()
    }

    /// All sort keys in priority order (primary first). Empty = unsorted.
    /// [`project`] orders rows by these in turn — *within* each group when
    /// grouping (tier 2), or the whole table when not.
    #[must_use]
    pub fn sort_keys(&self) -> &[(usize, SortDirection)] {
        &self.sort
    }

    /// Cycles the **primary** sort on a header click: `col` unsorted →
    /// Ascending → Descending → unsorted; a *different* column starts it
    /// Ascending. The standard tri-state header toggle; it replaces the key
    /// list with this single key (multi-key is set explicitly via
    /// [`set_sort_keys`](Self::set_sort_keys)/[`push_sort`](Self::push_sort)).
    pub fn toggle_sort(&mut self, column: usize) {
        self.sort = match self.sort.first().copied() {
            Some((c, SortDirection::Ascending)) if c == column => {
                vec![(column, SortDirection::Descending)]
            }
            Some((c, SortDirection::Descending)) if c == column => Vec::new(),
            _ => vec![(column, SortDirection::Ascending)],
        };
    }

    /// Sets (or clears) the sort as a **single** key (back-compatible).
    pub fn set_sort(&mut self, sort: Option<(usize, SortDirection)>) {
        self.sort = sort.into_iter().collect();
    }

    /// Replaces the full ordered key list (primary first) — multi-tier
    /// sort, e.g. from the group/sort config panel.
    pub fn set_sort_keys<I>(&mut self, keys: I)
    where
        I: IntoIterator<Item = (usize, SortDirection)>,
    {
        self.sort = keys.into_iter().collect();
    }

    /// Appends a secondary (then tertiary, …) sort key after the existing
    /// ones; a no-op repeat of the same column is de-duplicated to its
    /// latest direction so the panel can toggle a key without growing the
    /// list.
    pub fn push_sort(&mut self, column: usize, direction: SortDirection) {
        if let Some(slot) = self.sort.iter_mut().find(|(c, _)| *c == column) {
            slot.1 = direction;
        } else {
            self.sort.push((column, direction));
        }
    }

    /// Clears every sort key.
    pub fn clear_sort(&mut self) {
        self.sort.clear();
    }

    // ---- filtering ----

    /// The current filter needle (empty = no filtering).
    #[must_use]
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Sets the case-insensitive substring filter applied across every cell.
    pub fn set_filter(&mut self, filter: impl Into<String>) {
        self.filter = filter.into();
    }

    /// Clears the filter.
    pub fn clear_filter(&mut self) {
        self.filter.clear();
    }

    // ---- grouping ----

    /// The column rows are grouped by, or `None`.
    #[must_use]
    pub fn grouped_by(&self) -> Option<usize> {
        self.group_by
    }

    /// Groups by `column` (or `None` to ungroup) — chosen **independently**
    /// of the sort keys.
    pub fn set_group_by(&mut self, column: Option<usize>) {
        self.group_by = column;
    }

    /// The order the *groups* are listed in (tier 1) — independent of the
    /// sort keys, which order rows *within* a group (tier 2).
    #[must_use]
    pub fn group_direction(&self) -> SortDirection {
        self.group_dir
    }

    /// Sets the group-listing order (tier 1).
    pub fn set_group_direction(&mut self, direction: SortDirection) {
        self.group_dir = direction;
    }

    /// Flips the group-listing order Ascending ⇄ Descending.
    pub fn toggle_group_direction(&mut self) {
        self.group_dir = match self.group_dir {
            SortDirection::Ascending => SortDirection::Descending,
            SortDirection::Descending => SortDirection::Ascending,
        };
    }

    /// Whether the group with this key is collapsed.
    #[must_use]
    pub fn is_collapsed(&self, key: &str) -> bool {
        self.collapsed.iter().any(|k| k == key)
    }

    /// Collapses an expanded group / expands a collapsed one.
    pub fn toggle_collapse(&mut self, key: impl Into<String>) {
        let key = key.into();
        if let Some(i) = self.collapsed.iter().position(|k| *k == key) {
            self.collapsed.remove(i);
        } else {
            self.collapsed.push(key);
        }
    }

    /// Forgets every collapsed-group memory (everything expands).
    pub fn expand_all(&mut self) {
        self.collapsed.clear();
    }

    // ---- selection ----

    /// The selected **visual-row** index, or `None`.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Sets the selected visual-row index.
    pub fn select(&mut self, visual: Option<usize>) {
        self.selected = visual;
    }

    /// Moves the selection by `delta` visual rows, saturating into
    /// `0..visual_len` (no selection starts the move from row 0). A
    /// `visual_len` of `0` clears the selection. Total for any input.
    pub fn move_selection(&mut self, delta: isize, visual_len: usize) {
        if visual_len == 0 {
            self.selected = None;
            return;
        }
        let last = visual_len - 1;
        let cur = self.selected.unwrap_or(0).min(last);
        let next = if delta >= 0 {
            cur.saturating_add(delta.unsigned_abs())
        } else {
            cur.saturating_sub(delta.unsigned_abs())
        };
        self.selected = Some(next.min(last));
    }

    // ---- scrolling (a composed ScrollState — fast, sticky, total) ----

    /// The first visible visual-row index (the scroll offset).
    #[must_use]
    pub fn offset(&self) -> usize {
        self.vertical.offset()
    }

    /// The composed vertical [`ScrollState`] (e.g. to drive a paired
    /// [`Scrollbar`](crate::Scrollbar)).
    #[must_use]
    pub fn vertical(&self) -> ScrollState {
        self.vertical
    }

    /// Scrolls by `delta` visual rows (wheel/PageUp), clamped — see
    /// [`ScrollState::scroll_by`].
    pub fn scroll_by(&mut self, delta: isize, visual_len: usize, viewport_len: usize) {
        self.vertical.scroll_by(delta, visual_len, viewport_len);
    }

    /// Jumps to the top.
    pub fn scroll_to_top(&mut self) {
        self.vertical.scroll_to_top();
    }

    /// Jumps to the last full window.
    pub fn scroll_to_end(&mut self, visual_len: usize, viewport_len: usize) {
        self.vertical.scroll_to_end(visual_len, viewport_len);
    }

    /// Reconciles the offset after the projection length changed (re-filter,
    /// expand/collapse) — see [`ScrollState::on_content_change`].
    pub fn on_content_change(&mut self, visual_len: usize, viewport_len: usize) {
        self.vertical.on_content_change(visual_len, viewport_len);
    }

    /// Clamps the offset into bounds (call after a resize / set).
    pub fn clamp(&mut self, visual_len: usize, viewport_len: usize) {
        self.vertical.clamp(visual_len, viewport_len);
    }

    /// Scrolls the selected row into view with the smallest move (call after
    /// [`move_selection`](Self::move_selection)); no-op when nothing is
    /// selected. See [`ScrollState::show`].
    pub fn reveal_selected(&mut self, viewport_len: usize, visual_len: usize) {
        if let Some(sel) = self.selected {
            self.vertical.show(sel, 1, viewport_len, visual_len);
        }
    }

    // ---- editing ----

    /// The `(source_row, column)` being edited, or `None`.
    #[must_use]
    pub fn editing(&self) -> Option<(usize, usize)> {
        self.editing
    }

    /// Whether this exact source cell is the one being edited.
    #[must_use]
    pub fn is_editing(&self, source: usize, column: usize) -> bool {
        self.editing == Some((source, column))
    }

    /// Begins editing the cell at `(source_row, column)`. The caller seeds and
    /// owns the [`TextEdit`]; the widget only projects
    /// it. (The reducer should verify the column is
    /// [`editable`](DataColumn::editable) first — the widget also refuses to
    /// draw an editor for a non-editable column, so this is total either way.)
    pub fn begin_edit(&mut self, source: usize, column: usize) {
        self.editing = Some((source, column));
    }

    /// Ends editing and returns the `(source_row, column)` that *was* being
    /// edited (so the reducer knows exactly what to write back), or `None` if
    /// nothing was. This is the explicit change hook — call it after copying
    /// the [`TextEdit`] value into your own data.
    pub fn commit_edit(&mut self) -> Option<(usize, usize)> {
        self.editing.take()
    }

    /// Abandons the edit with no write-back.
    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }
}

/// Flattens the caller's source rows into the `[VisualRow]` the widget
/// renders, applying the [`DataTableState`] pipeline **filter → two-tier
/// group/sort → collapse**. Pure, deterministic, and total — the reducer
/// calls this once per data/spec change (the [`Tree`](crate::Tree) flatten
/// precedent), never the widget per frame.
///
/// - **Filter:** an empty [`filter`](DataTableState::filter) keeps every row;
///   otherwise a row is kept iff some cell's text contains the needle
///   (case-insensitive).
/// - **Two-tier order.** The grouping column
///   ([`grouped_by`](DataTableState::grouped_by)) is chosen **independently**
///   of the sort keys ([`sort_keys`](DataTableState::sort_keys)):
///   - *Grouped* — rows are partitioned by [`DataRow::group`] (falling back
///     to the group column's text); **tier 1** lists the *groups* by their
///     key in [`group_direction`](DataTableState::group_direction); **tier
///     2** orders the rows *within* each group by the ordered sort keys
///     (primary first, each Ascending/Descending). Each group is preceded
///     by a [`VisualRow::Group`]; a collapsed group contributes only its
///     header.
///   - *Ungrouped* — the whole kept set is ordered by the sort keys.
///
///   Every compare is a **stable**, deterministic, dependency-free
///   lexicographic compare on the cell text (typed/numeric comparators
///   remain a documented future additive); an out-of-range column is an
///   inert key, so the projection is total.
#[must_use]
pub fn project(columns: &[DataColumn], rows: &[DataRow], state: &DataTableState) -> Vec<VisualRow> {
    // 1. Filter — case-insensitive substring across all cells.
    let needle = state.filter.to_lowercase();
    let mut kept: Vec<usize> = (0..rows.len())
        .filter(|&i| {
            needle.is_empty()
                || rows[i]
                    .cells
                    .iter()
                    .any(|c| line_text(c).to_lowercase().contains(&needle))
        })
        .collect();

    // The multi-key row comparator (tier 2): each `(col, dir)` in priority
    // order; the first non-equal column decides. Stable, so equal rows keep
    // their source order. An out-of-range column compares as empty (a
    // no-op key), so it is total.
    let cmp_keys = |&a: &usize, &b: &usize| -> std::cmp::Ordering {
        for &(col, dir) in state.sort_keys() {
            let ka = rows[a].cell(col).map(line_text).unwrap_or_default();
            let kb = rows[b].cell(col).map(line_text).unwrap_or_default();
            let ord = match dir {
                SortDirection::Ascending => ka.cmp(&kb),
                SortDirection::Descending => kb.cmp(&ka),
            };
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    };

    // Two-tier: when grouping, order the *groups* (tier 1, by group key in
    // the caller-chosen `group_direction`) then the rows *within* each group
    // (tier 2, the multi-key sort) — group column independent of sort keys.
    // Ungrouped, just the multi-key sort over the whole table.
    match state.group_by {
        Some(col) if col < columns.len() => {
            let mut buckets: Vec<(String, Vec<usize>)> = Vec::new();
            for s in kept {
                let key = group_key(&rows[s], col);
                match buckets.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, members)) => members.push(s),
                    None => buckets.push((key, vec![s])),
                }
            }
            // Tier 1: order the groups by their key.
            buckets.sort_by(|(ka, _), (kb, _)| match state.group_direction() {
                SortDirection::Ascending => ka.cmp(kb),
                SortDirection::Descending => kb.cmp(ka),
            });
            let mut out = Vec::with_capacity(buckets.len());
            for (key, mut members) in buckets {
                // Tier 2: sort rows within the group (stable multi-key).
                members.sort_by(&cmp_keys);
                let collapsed = state.is_collapsed(&key);
                out.push(VisualRow::Group {
                    key,
                    count: members.len(),
                    collapsed,
                });
                if !collapsed {
                    out.extend(members.into_iter().map(|source| VisualRow::Data { source }));
                }
            }
            out
        }
        _ => {
            kept.sort_by(&cmp_keys);
            kept.into_iter()
                .map(|source| VisualRow::Data { source })
                .collect()
        }
    }
}

/// The plain text of a [`Line`] (its spans concatenated) — the key sort,
/// filter, and grouping compare on.
fn line_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// A row's group key: its explicit [`DataRow::group`], else the group
/// column's cell text, else the empty group.
fn group_key(row: &DataRow, col: usize) -> String {
    match &row.group {
        Some(k) => k.as_ref().to_string(),
        None => row.cell(col).map(line_text).unwrap_or_default(),
    }
}

/// A comprehensive, sortable/filterable/groupable, mouse-hit-testable,
/// virtualized data grid with optional in-cell editing — a **pure projection**
/// of caller-owned [`columns`](DataTable::new)/[`rows`](DataTable::new)/a
/// flattened [`projection`](project)/[`DataTableState`]/an optional editing
/// [`TextEdit`].
///
/// Layout: an optional framing [`Block`]; inside it an optional fixed header
/// row (sort arrows on the sorted column) above the body. The body shows only
/// the projected window `[offset, offset + body_height)` — one
/// [`VisualRow`] per row — so render cost is independent of total row count.
/// Columns are placed by the same deterministic [`Layout`] divider
/// [`Table`](crate::Table) and top-level layout use. Styling cascades base →
/// header/group/row → cell-line → span, with the selection
/// [`highlight_style`](Self::highlight_style) patched **last** across the full
/// width (the [`List`]/[`Table`](crate::Table) bar idiom). An
/// edited cell is drawn by reusing [`Input`] (one caret implementation).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, TextEdit, Widget};
/// use rstui_widgets::{
///     data_table::project, DataColumn, DataRow, DataTable, DataTableState,
/// };
///
/// // Caller-owned data + state (these are app model fields).
/// let columns = [
///     DataColumn::new("name"),
///     DataColumn::new("role").editable(true),
/// ];
/// let rows = [
///     DataRow::new(["Ada", "math"]),
///     DataRow::new(["Bob", "ops"]),
/// ];
/// let mut state = DataTableState::new();
/// state.toggle_sort(0); // sort by "name" ascending (in the reducer)
///
/// // The reducer flattens once per change; the widget reads the result.
/// let visual = project(&columns, &rows, &state);
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 3));
/// DataTable::new(&columns, &rows, &visual, &state)
///     .render(buf.area(), &mut buf);
///
/// // Header on row 0, sorted rows below it.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'n'); // "name"
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'A'); // "Ada"
/// assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, 'B'); // "Bob"
/// ```
#[derive(Debug, Clone)]
pub struct DataTable<'a> {
    columns: &'a [DataColumn<'a>],
    rows: &'a [DataRow<'a>],
    visual: &'a [VisualRow],
    state: &'a DataTableState,
    edit: Option<&'a TextEdit>,
    cell_select: Option<&'a CellSelectState>,
    block: Option<Block<'a>>,
    column_spacing: u16,
    show_header: bool,
    style: Style,
    header_style: Style,
    group_style: Style,
    highlight_style: Style,
    cursor_style: Style,
}

impl<'a> DataTable<'a> {
    /// A grid over `columns`, the caller's source `rows`, the flattened
    /// `visual` projection (from [`project`]), and the caller-owned `state` —
    /// header shown, default column spacing of 1, otherwise unstyled.
    #[must_use]
    pub fn new(
        columns: &'a [DataColumn<'a>],
        rows: &'a [DataRow<'a>],
        visual: &'a [VisualRow],
        state: &'a DataTableState,
    ) -> Self {
        Self {
            columns,
            rows,
            visual,
            state,
            edit: None,
            cell_select: None,
            block: None,
            column_spacing: 1,
            show_header: true,
            style: Style::new(),
            header_style: Style::new(),
            group_style: Style::new(),
            highlight_style: Style::new(),
            cursor_style: Style::new(),
        }
    }

    /// Supplies the caller-owned [`TextEdit`] used to
    /// render the cell [`DataTableState::editing`] points at (only when that
    /// column is [`editable`](DataColumn::editable)). Without it an "editing"
    /// cell just shows its static text — the widget never edits.
    #[must_use]
    pub fn edit(mut self, edit: &'a TextEdit) -> Self {
        self.edit = Some(edit);
        self
    }

    /// Supplies the caller-owned [`CellSelectState`] for the
    /// [`CellField::Select`] cell that is currently being edited (the
    /// dropdown's open/highlight). Without it, or when closed, a `Select`
    /// cell just shows its value with a `▾` — the widget never opens
    /// anything itself.
    #[must_use]
    pub fn cell_select(mut self, cell_select: &'a CellSelectState) -> Self {
        self.cell_select = Some(cell_select);
        self
    }

    /// Frames the grid in `block`; rows render into [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the blank columns between adjacent cells (default `1`).
    #[must_use]
    pub fn column_spacing(mut self, spacing: u16) -> Self {
        self.column_spacing = spacing;
        self
    }

    /// Sets whether the fixed header row is drawn (default `true`).
    #[must_use]
    pub fn show_header(mut self, show: bool) -> Self {
        self.show_header = show;
        self
    }

    /// Sets the base [`Style`]; also fills the content area so a background
    /// covers the whole pane.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the header-row [`Style`] (patched over the base; the sort arrow
    /// shares it).
    #[must_use]
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Sets the group-header-row [`Style`] (patched over the base).
    #[must_use]
    pub fn group_style(mut self, style: Style) -> Self {
        self.group_style = style;
        self
    }

    /// Sets the [`Style`] patched **last** over the selected visual row,
    /// across the full inner width — the one-bar selection idiom.
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Sets the caret-cell [`Style`] of an edited cell (forwarded to the
    /// reused [`Input`]; default is `Input`'s reversed block).
    #[must_use]
    pub fn cursor_style(mut self, style: Style) -> Self {
        self.cursor_style = style;
        self
    }

    /// `(inner, header, body, columns)`: the framed area, the header row rect
    /// (height 0 when hidden), the body rect, and each column's rect within
    /// the body. Computed exactly one way so the accessors and
    /// [`render`](Widget::render) never disagree (the
    /// [`ScrollView`](crate::ScrollView) precedent).
    fn geometry(&self, area: Rect) -> (Rect, Rect, Rect, Vec<Rect>) {
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if inner.is_empty() {
            return (inner, Rect::ZERO, Rect::ZERO, Vec::new());
        }
        let header_h = u16::from(self.show_header && !self.columns.is_empty()).min(inner.height);
        let header = Rect::new(inner.x, inner.y, inner.width, header_h);
        let body = Rect::new(
            inner.x,
            inner.y.saturating_add(header_h),
            inner.width,
            inner.height.saturating_sub(header_h),
        );
        let constraints: Vec<Constraint> = if self.columns.is_empty() {
            Vec::new()
        } else {
            self.columns.iter().map(|c| c.width).collect()
        };
        let columns = if constraints.is_empty() {
            Vec::new()
        } else {
            Layout::horizontal(constraints)
                .spacing(self.column_spacing)
                .split(Rect::new(inner.x, inner.y, inner.width, 1))
        };
        (inner, header, body, columns)
    }

    /// The clamped first-visible visual-row index for this `area` (an
    /// over-scroll parks at the last full window — the caller-owned-offset
    /// contract every scrolling widget here uses).
    fn first_visible(&self, body_h: u16) -> usize {
        let max = self.visual.len().saturating_sub(body_h as usize);
        self.state.offset().min(max)
    }

    /// Resolves a screen position to a [`DataTableHit`] — the mouse/keyboard
    /// change hook (see [`DataTableHit`]). `None` for the frame, padding, or
    /// outside the grid. Pure; safe for any position.
    #[must_use]
    pub fn hit(&self, area: Rect, pos: Position) -> Option<DataTableHit> {
        if area.is_empty() {
            return None;
        }
        // The open dropdown panel is the top-most overlay (drawn last over
        // the rows below), so it is hit-tested FIRST — a click inside it is
        // the option it visually covers, never the data row underneath.
        if let Some(es) = self.editing_select(area) {
            let panel = self.dropdown_panel(area, es.field, es.options.len());
            if !panel.is_empty() && panel.contains(pos) {
                let row = (pos.y - panel.y) as usize;
                let index = es.state.offset().saturating_add(row);
                return (index < es.options.len()).then_some(DataTableHit::DropdownOption {
                    source: es.source,
                    column: es.column,
                    index,
                });
            }
        }
        let (inner, header, body, columns) = self.geometry(area);
        if !inner.contains(pos) {
            return None;
        }
        // Header → which column.
        if header.height > 0 && header.contains(pos) {
            return columns
                .iter()
                .position(|c| pos.x >= c.x && pos.x < c.x.saturating_add(c.width.max(1)))
                .map(DataTableHit::Header);
        }
        if !body.contains(pos) || body.height == 0 {
            return None;
        }
        let offset = self.first_visible(body.height);
        let row_in_body = (pos.y - body.top()) as usize;
        let visual = offset.checked_add(row_in_body)?;
        match self.visual.get(visual)? {
            VisualRow::Group { .. } => Some(DataTableHit::Group(visual)),
            VisualRow::Data { source } => {
                let column = columns
                    .iter()
                    .position(|c| pos.x >= c.x && pos.x < c.x.saturating_add(c.width.max(1)))?;
                Some(DataTableHit::Cell {
                    visual,
                    source: *source,
                    column,
                })
            }
        }
    }

    /// The on-screen rect of the data cell at source-row `source`, column
    /// `column`, **iff it is currently visible** (else `None`). For placing a
    /// hardware cursor over an edited cell or scroll-into-view math (the
    /// [`ScrollView::viewport`](crate::ScrollView::viewport) precedent).
    #[must_use]
    pub fn cell_rect(&self, area: Rect, source: usize, column: usize) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        let (_, _, body, columns) = self.geometry(area);
        if body.height == 0 {
            return None;
        }
        let col = columns.get(column)?;
        let offset = self.first_visible(body.height);
        let visible = self
            .visual
            .iter()
            .enumerate()
            .skip(offset)
            .take(body.height as usize);
        for (vi, row) in visible {
            if let VisualRow::Data { source: s } = row {
                if *s == source {
                    let y = body.top().saturating_add((vi - offset) as u16);
                    let w = col.width.min(body.right().saturating_sub(col.x));
                    return Some(Rect::new(col.x, y, w, 1));
                }
            }
        }
        None
    }

    /// The on-screen field rect of the [`CellField::Select`] cell being
    /// edited with an **open** dropdown, plus its options / source /
    /// column / state — `None` unless one is open and that cell is
    /// currently visible. Reuses [`cell_rect`](Self::cell_rect) for the
    /// field, so the rendered panel and [`hit`](Self::hit) cannot
    /// disagree on where it is.
    fn editing_select(&self, area: Rect) -> Option<EditingSelect<'_>> {
        let st = self.cell_select.filter(|s| s.is_open())?;
        let (src, col) = self.state.editing()?;
        let CellField::Select(opts) = self.columns.get(col)?.cell_field() else {
            return None;
        };
        let field = self.cell_rect(area, src, col)?;
        Some(EditingSelect {
            field,
            source: src,
            column: col,
            options: opts.as_slice(),
            state: st,
        })
    }

    /// The open dropdown's panel rect: anchored directly **below** the
    /// `field`, flipped **above** (or clamped) when the space below within
    /// the framed `inner` is short — the same rule a standalone
    /// [`Select`](crate::Select) applies, but a pure function of `area` so
    /// the overlay render and [`hit`](Self::hit) place it identically
    /// (the [`ScrollView`](crate::ScrollView) "computed one way" precedent).
    /// Empty when there are no options.
    fn dropdown_panel(&self, area: Rect, field: Rect, opts_len: usize) -> Rect {
        let inner = self.geometry(area).0;
        let rows =
            u16::try_from(opts_len.min(DROPDOWN_MAX_ROWS as usize)).unwrap_or(DROPDOWN_MAX_ROWS);
        if rows == 0 {
            return Rect::ZERO;
        }
        let below = inner.bottom().saturating_sub(field.bottom());
        let above = field.top().saturating_sub(inner.top());
        if rows <= below {
            Rect::new(field.x, field.bottom(), field.width, rows)
        } else if rows <= above {
            Rect::new(field.x, field.top().saturating_sub(rows), field.width, rows)
        } else if below >= above {
            Rect::new(field.x, field.bottom(), field.width, below)
        } else {
            Rect::new(
                field.x,
                field.top().saturating_sub(above),
                field.width,
                above,
            )
        }
    }
}

/// The dropdown panel's option-row cap (mirrors [`Select`](crate::Select)'s default
/// `open_height` so the in-cell dropdown matches a standalone one).
const DROPDOWN_MAX_ROWS: u16 = 8;

/// The open [`CellField::Select`] cell being edited, located on screen —
/// shared by the overlay render and [`DataTable::hit`] so they resolve the
/// exact same panel. A private render/hit detail, not public API.
struct EditingSelect<'a> {
    /// The cell's on-screen field rect (one row).
    field: Rect,
    /// Source-row index of the edited cell.
    source: usize,
    /// Column index of the edited cell.
    column: usize,
    /// The column's dropdown options.
    options: &'a [Cow<'a, str>],
    /// The caller-owned open/highlight state.
    state: &'a CellSelectState,
}

impl Widget for DataTable<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let (inner, header, body, columns) = self.geometry(area);

        if let Some(b) = &self.block {
            b.render_ref(area, buf);
        }
        if inner.is_empty() {
            return;
        }
        // Base fills the whole pane so a background covers gutters, gaps, and
        // rows past the last item; glyphs cascade on top.
        buf.set_style(inner, self.style);

        // ---- header row, with a sort arrow on the sorted column ----
        if header.height > 0 {
            let hbase = self.style.patch(self.header_style);
            buf.set_style(header, hbase);
            for (ci, (col, rect)) in self.columns.iter().zip(&columns).enumerate() {
                stamp_line(buf, &col.header, rect, header.right(), header.top(), hbase);
                if let Some((sc, dir)) = self.state.sort() {
                    if sc == ci && rect.width > 0 {
                        let arrow = match dir {
                            SortDirection::Ascending => '▲',
                            SortDirection::Descending => '▼',
                        };
                        let ax = rect
                            .x
                            .saturating_add(rect.width.saturating_sub(1))
                            .min(header.right().saturating_sub(1));
                        buf.set_cell(Position::new(ax, header.top()), arrow, hbase);
                    }
                }
            }
        }

        if body.height == 0 || self.visual.is_empty() {
            return;
        }

        // ---- virtualized body: only the visible window is touched ----
        let offset = self.first_visible(body.height);
        // The row loop draws every Select cell's *closed* look (value + ▾).
        // An open dropdown's panel is a single overlay rendered after the
        // body (so it floats over the rows below) via `editing_select` —
        // the SAME geometry `hit` resolves clicks against.
        for (row_i, (vi, vrow)) in self
            .visual
            .iter()
            .enumerate()
            .skip(offset)
            .take(body.height as usize)
            .enumerate()
        {
            let y = body.top().saturating_add(row_i as u16);
            let is_selected = self.state.selected() == Some(vi);

            match vrow {
                VisualRow::Group {
                    key,
                    count,
                    collapsed,
                } => {
                    let gbase = self.style.patch(self.group_style);
                    buf.set_style(Rect::new(inner.left(), y, inner.width, 1), gbase);
                    let marker = if *collapsed { '▸' } else { '▾' };
                    let label = format!("{marker} {key} ({count})");
                    let mut x = inner.left();
                    for ch in label.chars() {
                        if x >= inner.right() {
                            break;
                        }
                        buf.set_cell(Position::new(x, y), ch, gbase);
                        x = x.saturating_add(1);
                    }
                    if is_selected {
                        buf.set_style(
                            Rect::new(inner.left(), y, inner.width, 1),
                            self.highlight_style,
                        );
                    }
                }
                VisualRow::Data { source } => {
                    let Some(row) = self.rows.get(*source) else {
                        continue;
                    };
                    let row_base = self.style.patch(row.style);
                    if is_selected {
                        buf.set_style(
                            Rect::new(inner.left(), y, inner.width, 1),
                            self.highlight_style,
                        );
                    }
                    for (ci, rect) in columns.iter().enumerate() {
                        let cell_w = rect.width.min(inner.right().saturating_sub(rect.x));
                        if cell_w == 0 {
                            continue;
                        }
                        let column = self.columns.get(ci);
                        let editable = column.is_some_and(DataColumn::is_editable);
                        let editing_here = self.state.is_editing(*source, ci);
                        let cell_area = Rect::new(rect.x, y, cell_w, 1);
                        let hl = is_selected.then_some(self.highlight_style);
                        // The selection bar is already painted across the
                        // row; patch it into a reused control's base so the
                        // bar stays contiguous over it.
                        let base = if is_selected {
                            row_base.patch(self.highlight_style)
                        } else {
                            row_base
                        };
                        let text = row.cell(ci).map(line_text).unwrap_or_default();

                        match column.map(DataColumn::cell_field) {
                            // The cell *is* a checkbox/switch on every row —
                            // value parsed from its text (the reducer flips
                            // that text on a Cell hit).
                            Some(CellField::Checkbox) => {
                                Checkbox::new("")
                                    .checked(cell_truthy(&text))
                                    .focused(editing_here)
                                    .style(base)
                                    .render(cell_area, buf);
                            }
                            Some(CellField::Switch) => {
                                Switch::new()
                                    .on(cell_truthy(&text))
                                    .focused(editing_here)
                                    .style(base)
                                    .render(cell_area, buf);
                            }
                            Some(CellField::Select(_)) => {
                                // Always draw the closed look in-loop (value +
                                // ▾). An open dropdown's panel is deferred to
                                // a single overlay after the body so it floats
                                // over the rows below (and `hit` resolves
                                // clicks against that same panel geometry).
                                if let Some(cell) = row.cell(ci) {
                                    stamp_cell(buf, cell, rect, inner.right(), y, row_base, hl);
                                }
                                let mx = rect
                                    .x
                                    .saturating_add(rect.width.saturating_sub(1))
                                    .min(inner.right().saturating_sub(1));
                                buf.set_cell(Position::new(mx, y), '▾', base);
                            }
                            // CellField::Text (or no column): the original
                            // text behaviour, byte-for-byte unchanged.
                            _ => {
                                let editor = if editing_here && editable {
                                    self.edit
                                } else {
                                    None
                                };
                                if let Some(te) = editor {
                                    let mut field = Input::new(te).focused(true).style(row_base);
                                    if self.cursor_style != Style::new() {
                                        field = field.cursor_style(self.cursor_style);
                                    }
                                    field.render(cell_area, buf);
                                } else if let Some(cell) = row.cell(ci) {
                                    stamp_cell(buf, cell, rect, inner.right(), y, row_base, hl);
                                }
                            }
                        }
                    }
                }
            }
        }

        // ---- the open Select cell's dropdown, as a single overlay ----
        // Drawn AFTER every row so the panel floats *over* the rows below
        // (the next loop iterations cannot overwrite it). The panel rect is
        // the one `editing_select`/`dropdown_panel` compute — the very same
        // geometry `hit` resolves a click against, so the option clicked is
        // the option chosen (no off-by-the-row-below). Opaque via
        // `clear_region`, then the options are a reused `List` (exactly the
        // technique `Select` uses internally).
        if let Some(es) = self.editing_select(area) {
            let panel = self.dropdown_panel(area, es.field, es.options.len());
            if !panel.is_empty() {
                buf.clear_region(panel);
                List::new(es.options.iter().map(|c| Line::from(c.as_ref())))
                    .selected(Some(es.state.highlight()))
                    .offset(es.state.offset())
                    .highlight_style(self.highlight_style)
                    .render(panel, buf);
            }
        }
    }
}

/// Stamps a header [`Line`] into its column rect (cascade `base → line →
/// span`), clipped to the column and the inner right edge.
fn stamp_line(buf: &mut Buffer, line: &Line, rect: &Rect, right: u16, y: u16, base: Style) {
    let line_base = base.patch(line.style);
    let col_right = rect.x.saturating_add(rect.width).min(right);
    let mut x = rect.x;
    'line: for span in &line.spans {
        let span_style = line_base.patch(span.style);
        for ch in span.content.chars() {
            if x >= col_right {
                break 'line;
            }
            buf.set_cell(Position::new(x, y), ch, span_style);
            x = x.saturating_add(1);
        }
    }
}

/// Stamps a data cell [`Line`] into its column rect with the full table → row
/// → cell-line → span cascade, the selection `highlight` patched last (the
/// [`Table`](crate::Table) `render_row` discipline).
fn stamp_cell(
    buf: &mut Buffer,
    cell: &Line,
    rect: &Rect,
    right: u16,
    y: u16,
    row_base: Style,
    highlight: Option<Style>,
) {
    let cell_base = row_base.patch(cell.style);
    let col_right = rect.x.saturating_add(rect.width).min(right);
    let mut x = rect.x;
    'cell: for span in &cell.spans {
        let mut span_style = cell_base.patch(span.style);
        if let Some(hl) = highlight {
            span_style = span_style.patch(hl);
        }
        for ch in span.content.chars() {
            if x >= col_right {
                break 'cell;
            }
            buf.set_cell(Position::new(x, y), ch, span_style);
            x = x.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier};

    /// Renders `widget` into a fresh buffer and returns the glyphs as one
    /// newline-terminated line per row.
    fn grid(widget: DataTable, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    fn cols() -> Vec<DataColumn<'static>> {
        vec![
            DataColumn::new("name").width(Constraint::Length(4)),
            DataColumn::new("city").width(Constraint::Length(4)),
        ]
    }

    #[test]
    fn header_then_rows_in_source_order_when_unsorted() {
        let c = cols();
        let r = [DataRow::new(["bob", "nyc"]), DataRow::new(["ada", "sfo"])];
        let st = DataTableState::new();
        let v = project(&c, &r, &st);
        // 4 + 1 spacing + 4 = 9 wide.
        assert_eq!(
            grid(DataTable::new(&c, &r, &v, &st), 9, 3),
            "name city\nbob  nyc \nada  sfo \n"
        );
    }

    #[test]
    fn toggle_sort_cycles_asc_desc_off_and_projects_in_that_order() {
        let c = cols();
        let r = [
            DataRow::new(["bob"]),
            DataRow::new(["ada"]),
            DataRow::new(["cy"]),
        ];
        let mut st = DataTableState::new();

        st.toggle_sort(0); // ascending
        assert_eq!(st.sort(), Some((0, SortDirection::Ascending)));
        let v = project(&c, &r, &st);
        assert_eq!(
            v,
            vec![
                VisualRow::Data { source: 1 }, // ada
                VisualRow::Data { source: 0 }, // bob
                VisualRow::Data { source: 2 }, // cy
            ]
        );

        st.toggle_sort(0); // descending
        assert_eq!(st.sort(), Some((0, SortDirection::Descending)));
        let v = project(&c, &r, &st);
        assert_eq!(v[0], VisualRow::Data { source: 2 }); // cy first

        st.toggle_sort(0); // off
        assert_eq!(st.sort(), None);
        st.toggle_sort(1); // a different column starts ascending
        assert_eq!(st.sort(), Some((1, SortDirection::Ascending)));
    }

    #[test]
    fn the_sorted_column_header_gets_an_arrow() {
        let c = cols();
        let r = [DataRow::new(["ab", "cd"])];
        let mut st = DataTableState::new();
        st.toggle_sort(0);
        let v = project(&c, &r, &st);
        // Column 0 is cells [0,4); its last cell (x=3) shows ▲.
        let out = grid(DataTable::new(&c, &r, &v, &st), 9, 2);
        assert_eq!(out.lines().next().unwrap().chars().nth(3).unwrap(), '▲');
        st.toggle_sort(0);
        let v = project(&c, &r, &st);
        let out = grid(DataTable::new(&c, &r, &v, &st), 9, 2);
        assert_eq!(out.lines().next().unwrap().chars().nth(3).unwrap(), '▼');
    }

    #[test]
    fn filter_is_a_case_insensitive_substring_over_all_cells() {
        let c = cols();
        let r = [
            DataRow::new(["Ada", "NYC"]),
            DataRow::new(["Bob", "SFO"]),
            DataRow::new(["Cy", "nyc2"]),
        ];
        let mut st = DataTableState::new();
        st.set_filter("nyc");
        let v = project(&c, &r, &st);
        // Rows 0 ("NYC") and 2 ("nyc2") match; row 1 does not.
        assert_eq!(
            v,
            vec![VisualRow::Data { source: 0 }, VisualRow::Data { source: 2 }]
        );
        st.clear_filter();
        assert_eq!(project(&c, &r, &st).len(), 3);
    }

    #[test]
    fn grouping_emits_stable_headers_and_collapse_hides_members() {
        let c = cols();
        let r = [
            DataRow::new(["a1"]).group("A"),
            DataRow::new(["b1"]).group("B"),
            DataRow::new(["a2"]).group("A"),
        ];
        let mut st = DataTableState::new();
        st.set_group_by(Some(0));
        let v = project(&c, &r, &st);
        // First-seen group order A then B; A holds 2, B holds 1.
        assert_eq!(
            v,
            vec![
                VisualRow::Group {
                    key: "A".into(),
                    count: 2,
                    collapsed: false
                },
                VisualRow::Data { source: 0 },
                VisualRow::Data { source: 2 },
                VisualRow::Group {
                    key: "B".into(),
                    count: 1,
                    collapsed: false
                },
                VisualRow::Data { source: 1 },
            ]
        );

        st.toggle_collapse("A");
        let v = project(&c, &r, &st);
        // Collapsed A keeps its header (count still 2) but drops its members.
        assert_eq!(
            v,
            vec![
                VisualRow::Group {
                    key: "A".into(),
                    count: 2,
                    collapsed: true
                },
                VisualRow::Group {
                    key: "B".into(),
                    count: 1,
                    collapsed: false
                },
                VisualRow::Data { source: 1 },
            ]
        );
    }

    #[test]
    fn two_tier_groups_ordered_by_key_dir_rows_within_by_sort_keys() {
        // Group column (0) is independent of the sort key (1). Tier 1: the
        // groups are listed by key in `group_direction`. Tier 2: rows
        // *within* a group are ordered by the sort key.
        let c = cols(); // 2 cols
        let r = [
            DataRow::new(["B", "2"]),
            DataRow::new(["A", "3"]),
            DataRow::new(["B", "1"]),
            DataRow::new(["A", "1"]),
        ];
        let mut st = DataTableState::new();
        st.set_group_by(Some(0)); // group by col 0
        st.set_sort_keys([(1, SortDirection::Ascending)]); // sort rows by col 1
        let v = project(&c, &r, &st);
        // Tier1 ascending: group "A" then "B". Tier2: within A by col1 asc
        // → src3("1") then src1("3"); within B → src2("1") then src0("2").
        assert_eq!(
            v,
            vec![
                VisualRow::Group {
                    key: "A".into(),
                    count: 2,
                    collapsed: false
                },
                VisualRow::Data { source: 3 },
                VisualRow::Data { source: 1 },
                VisualRow::Group {
                    key: "B".into(),
                    count: 2,
                    collapsed: false
                },
                VisualRow::Data { source: 2 },
                VisualRow::Data { source: 0 },
            ]
        );

        // Tier 1 only flips with the group direction (rows within unchanged).
        st.set_group_direction(SortDirection::Descending);
        let v = project(&c, &r, &st);
        assert_eq!(
            v[0],
            VisualRow::Group {
                key: "B".into(),
                count: 2,
                collapsed: false
            }
        );
        assert_eq!(v[1], VisualRow::Data { source: 2 }); // B still col1-asc
        assert_eq!(
            v[3],
            VisualRow::Group {
                key: "A".into(),
                count: 2,
                collapsed: false
            }
        );
    }

    #[test]
    fn multi_key_sort_breaks_ties_with_the_secondary_key() {
        let c = cols();
        let r = [
            DataRow::new(["x", "2"]),
            DataRow::new(["x", "1"]),
            DataRow::new(["a", "9"]),
        ];
        let mut st = DataTableState::new();
        st.set_sort_keys([(0, SortDirection::Ascending), (1, SortDirection::Ascending)]);
        // col0: "a" < "x"; the two "x" rows tie on col0 → broken by col1.
        assert_eq!(
            project(&c, &r, &st),
            vec![
                VisualRow::Data { source: 2 }, // a,9
                VisualRow::Data { source: 1 }, // x,1
                VisualRow::Data { source: 0 }, // x,2
            ]
        );
        // `sort()` stays the back-compatible primary view; `sort_keys` the
        // full list.
        assert_eq!(st.sort(), Some((0, SortDirection::Ascending)));
        assert_eq!(st.sort_keys().len(), 2);
    }

    #[test]
    fn group_column_is_independent_of_the_sort_column() {
        // Group by col0, sort rows by col0 too is the *same* column — must
        // still work — but the point: changing the sort key does not change
        // the grouping column, and vice-versa.
        let mut st = DataTableState::new();
        st.set_group_by(Some(2));
        st.toggle_sort(0); // primary sort col 0
        assert_eq!(st.grouped_by(), Some(2));
        assert_eq!(st.sort(), Some((0, SortDirection::Ascending)));
        st.toggle_sort(1); // re-point the sort column
        assert_eq!(st.grouped_by(), Some(2)); // grouping untouched
        assert_eq!(st.sort(), Some((1, SortDirection::Ascending)));
        st.set_group_by(Some(3)); // re-point grouping
        assert_eq!(st.sort(), Some((1, SortDirection::Ascending))); // sort untouched
    }

    #[test]
    fn a_group_header_row_renders_a_marker_key_and_count() {
        let c = cols();
        let r = [
            DataRow::new(["x"]).group("Eng"),
            DataRow::new(["y"]).group("Eng"),
        ];
        let mut st = DataTableState::new();
        st.set_group_by(Some(0));
        let v = project(&c, &r, &st);
        let out = grid(DataTable::new(&c, &r, &v, &st).show_header(false), 12, 3);
        assert_eq!(out.lines().next().unwrap(), "▾ Eng (2)   ");
        st.toggle_collapse("Eng");
        let v = project(&c, &r, &st);
        let out = grid(DataTable::new(&c, &r, &v, &st).show_header(false), 12, 1);
        assert_eq!(out.lines().next().unwrap(), "▸ Eng (2)   ");
    }

    #[test]
    fn the_body_is_virtualized_to_the_offset_window() {
        let c = vec![DataColumn::new("n").width(Constraint::Length(2))];
        let r: Vec<DataRow> = (0..100)
            .map(|i| DataRow::new([format!("{i:02}")]))
            .collect();
        let mut st = DataTableState::new();
        st.scroll_by(40, 100, 2); // offset 40
        let v = project(&c, &r, &st);
        // Header hidden, 2-row body: only rows 40 and 41 are drawn — render
        // cost is the window, not the 100 rows.
        let out = grid(DataTable::new(&c, &r, &v, &st).show_header(false), 2, 2);
        assert_eq!(out, "40\n41\n");
    }

    #[test]
    fn an_over_scroll_parks_at_the_last_window_not_a_panic() {
        let c = vec![DataColumn::new("n").width(Constraint::Length(1))];
        let r: Vec<DataRow> = (0..5).map(|i| DataRow::new([i.to_string()])).collect();
        let mut st = DataTableState::new();
        st.select(None);
        st.vertical = {
            let mut s = ScrollState::new();
            s.set_offset(9_999);
            s
        };
        let v = project(&c, &r, &st);
        // 5 rows, 2-row body: clamps to offset 3 → rows "3","4".
        assert_eq!(
            grid(DataTable::new(&c, &r, &v, &st).show_header(false), 1, 2),
            "3\n4\n"
        );
    }

    #[test]
    fn selection_is_a_full_width_bar_over_the_visual_row() {
        let c = cols();
        let r = [DataRow::new(["ab", "cd"]), DataRow::new(["ef", "gh"])];
        let mut st = DataTableState::new();
        st.select(Some(1)); // second visual row (no header offset in indices)
        let v = project(&c, &r, &st);
        let mut buf = Buffer::empty(Rect::new(0, 0, 9, 3));
        DataTable::new(&c, &r, &v, &st)
            .highlight_style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        // Header row 0 and first data row (y=1) are not highlighted; the
        // selected visual row 1 → screen y=2 is one contiguous blue bar.
        for x in 0..9 {
            assert_eq!(buf.get(Position::new(x, 2)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn an_editable_cell_renders_the_borrowed_textedit_with_a_caret() {
        let c = vec![
            DataColumn::new("name").width(Constraint::Length(5)),
            DataColumn::new("role")
                .width(Constraint::Length(6))
                .editable(true),
        ];
        let r = [DataRow::new(["Ada", "math"])];
        let mut st = DataTableState::new();
        st.begin_edit(0, 1); // editing source row 0, column 1
        let mut te = TextEdit::from_value("mathx");
        te.set_cursor(5); // caret at end
        let v = project(&c, &r, &st);
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 2));
        DataTable::new(&c, &r, &v, &st)
            .edit(&te)
            .render(buf.area(), &mut buf);
        // Column 1 starts at x = 5 + 1 spacing = 6; the edited value shows
        // there, and the focused caret (reversed) sits just past it.
        assert_eq!(buf.get(Position::new(6, 1)).unwrap().symbol, 'm');
        assert_eq!(buf.get(Position::new(10, 1)).unwrap().symbol, 'x');
        let caret = buf.get(Position::new(11, 1)).unwrap();
        assert!(caret.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn a_non_editable_column_never_shows_an_editor_even_if_state_says_so() {
        // Column 0 is not editable; even with `editing` pointed at it and a
        // TextEdit supplied, the static cell text is what renders.
        let c = vec![DataColumn::new("name").width(Constraint::Length(5))];
        let r = [DataRow::new(["Ada"])];
        let mut st = DataTableState::new();
        st.begin_edit(0, 0);
        let te = TextEdit::from_value("ZZZZ");
        let v = project(&c, &r, &st);
        let out = grid(
            DataTable::new(&c, &r, &v, &st).edit(&te).show_header(false),
            5,
            1,
        );
        assert_eq!(out, "Ada  \n");
    }

    #[test]
    fn text_is_the_default_cell_field_unchanged() {
        // The default is Text — the original behaviour, so every existing
        // text test still holds. This pins the non-breaking guarantee.
        assert_eq!(DataColumn::new("x").cell_field(), &CellField::Text);
    }

    #[test]
    fn a_checkbox_column_renders_a_box_from_the_cell_text() {
        // The cell text *is* the boolean (cell_truthy); the column draws a
        // reused Checkbox every row. Default symbols are "[x] " / "[ ] ".
        let c = vec![
            DataColumn::new("ok")
                .width(Constraint::Length(4))
                .field(CellField::Checkbox),
        ];
        let r = [DataRow::new(["true"]), DataRow::new(["no"])];
        let st = DataTableState::new();
        let v = project(&c, &r, &st);
        assert_eq!(
            grid(DataTable::new(&c, &r, &v, &st).show_header(false), 4, 2),
            "[x] \n[ ] \n"
        );
    }

    #[test]
    fn a_switch_column_renders_a_sliding_track_from_truthy() {
        let c = vec![
            DataColumn::new("on")
                .width(Constraint::Length(4))
                .field(CellField::Switch),
        ];
        let r = [DataRow::new(["yes"]), DataRow::new(["0"])];
        let st = DataTableState::new();
        let v = project(&c, &r, &st);
        // on → knob slid right, off → knob left ([ ●] / [● ]).
        assert_eq!(
            grid(DataTable::new(&c, &r, &v, &st).show_header(false), 4, 2),
            "[ ●]\n[● ]\n"
        );
    }

    #[test]
    fn a_select_cell_shows_its_value_and_a_marker_when_closed() {
        let c = vec![
            DataColumn::new("role")
                .width(Constraint::Length(6))
                .field(CellField::select(["dev", "ops"])),
        ];
        let r = [DataRow::new(["dev"])];
        let st = DataTableState::new(); // not editing ⇒ closed
        let v = project(&c, &r, &st);
        // value text, padding, then the ▾ affordance in the last cell.
        assert_eq!(
            grid(DataTable::new(&c, &r, &v, &st).show_header(false), 6, 1),
            "dev  ▾\n"
        );
    }

    #[test]
    fn a_select_cell_drops_the_panel_when_edited_and_open() {
        let c = vec![
            DataColumn::new("role")
                .width(Constraint::Length(6))
                .field(CellField::select(["dev", "ops"])),
        ];
        let r = [DataRow::new(["dev"])];
        let mut st = DataTableState::new();
        st.begin_edit(0, 0);
        let mut cs = CellSelectState::new();
        cs.open(Some(0));
        let v = project(&c, &r, &st);
        let out = grid(
            DataTable::new(&c, &r, &v, &st)
                .cell_select(&cs)
                .show_header(false),
            8,
            4,
        );
        // The reused Select panel lists the other option below the field.
        assert!(out.contains("ops"), "open dropdown lists options:\n{out}");
        // Closed again ⇒ no panel (just the value + marker).
        cs.close();
        let out = grid(
            DataTable::new(&c, &r, &v, &st)
                .cell_select(&cs)
                .show_header(false),
            8,
            4,
        );
        assert!(!out.contains("ops"), "closed ⇒ no panel:\n{out}");
    }

    #[test]
    fn an_open_select_panel_floats_opaquely_over_the_rows_below() {
        // The reported bug: an open dropdown must load *over the top* of the
        // rows beneath it. It is deferred to a single overlay drawn after
        // the body and `clear_region`d opaque, so the rows under the panel
        // are covered — not painted over the panel.
        let c = vec![
            DataColumn::new("k")
                .width(Constraint::Length(6))
                .field(CellField::select(["alpha", "beta"])),
        ];
        let r = [
            DataRow::new(["alpha"]),
            DataRow::new(["ZZZZ"]),
            DataRow::new(["YYYY"]),
        ];
        let mut st = DataTableState::new();
        st.begin_edit(0, 0); // editing the row-0 Select cell
        let mut cs = CellSelectState::new();
        cs.open(Some(0));
        let v = project(&c, &r, &st);
        let out = grid(
            DataTable::new(&c, &r, &v, &st)
                .cell_select(&cs)
                .show_header(false),
            6,
            5,
        );
        // Panel lists "beta" below the field; the data rows it covers
        // ("ZZZZ"/"YYYY") are hidden — proof it floats opaquely on top and
        // is not overwritten by the row loop.
        assert!(out.contains("beta"), "panel option shows:\n{out}");
        assert!(
            !out.contains("ZZZZ") && !out.contains("YYYY"),
            "rows under the panel are covered (opaque, drawn last):\n{out}"
        );
    }

    #[test]
    fn clicking_an_open_dropdown_option_hits_that_option_not_the_row_below() {
        // The reported bug: clicking the shown option selected the data row
        // the panel floats over (or nothing). `hit` must resolve a click
        // inside the panel to *that option*, tested before the rows.
        let c = vec![
            DataColumn::new("k")
                .width(Constraint::Length(6))
                .field(CellField::select(["A", "B", "C"])),
        ];
        let r = [
            DataRow::new(["A"]),
            DataRow::new(["zzz"]),
            DataRow::new(["yyy"]),
        ];
        let mut st = DataTableState::new();
        st.begin_edit(0, 0);
        let mut cs = CellSelectState::new();
        cs.open(Some(0));
        let v = project(&c, &r, &st);
        let dt = DataTable::new(&c, &r, &v, &st)
            .cell_select(&cs)
            .show_header(false);
        let area = Rect::new(0, 0, 6, 6);
        // field at y=0; the panel drops below: option A=y1, B=y2, C=y3.
        for (y, index) in [(1, 0), (2, 1), (3, 2)] {
            assert_eq!(
                dt.hit(area, Position::new(2, y)),
                Some(DataTableHit::DropdownOption {
                    source: 0,
                    column: 0,
                    index,
                }),
                "click at y={y} is option {index}, not the row it covers"
            );
        }
        // The field row itself stays the cell (so clicking it can re-handle
        // the dropdown), not an option.
        assert_eq!(
            dt.hit(area, Position::new(2, 0)),
            Some(DataTableHit::Cell {
                visual: 0,
                source: 0,
                column: 0,
            })
        );
        // With the dropdown CLOSED there is no panel, so the same y maps to
        // the data row again (the only time that is correct).
        let cs_closed = CellSelectState::new();
        let dt2 = DataTable::new(&c, &r, &v, &st)
            .cell_select(&cs_closed)
            .show_header(false);
        assert_eq!(
            dt2.hit(area, Position::new(2, 1)),
            Some(DataTableHit::Cell {
                visual: 1,
                source: 1,
                column: 0,
            })
        );
    }

    #[test]
    fn a_non_editable_text_path_is_byte_for_byte_unchanged_by_the_field_addition() {
        // Same inputs as the original text test; default Text field ⇒ the
        // exact prior output (the additive change touched nothing here).
        let c = cols();
        let r = [DataRow::new(["ab", "cd"]), DataRow::new(["ef", "gh"])];
        let st = DataTableState::new();
        let v = project(&c, &r, &st);
        assert_eq!(
            grid(DataTable::new(&c, &r, &v, &st).show_header(false), 9, 2),
            "ab   cd  \nef   gh  \n"
        );
    }

    #[test]
    fn cell_select_state_is_total_for_any_sequence() {
        // The CellSelectState totality property (the iter-25 rule).
        let mut s: u64 = 0xCE11_5E1E_C7ED_0001;
        let mut rng = || {
            s = s
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            s
        };
        let mut cs = CellSelectState::new();
        for _ in 0..10_000 {
            let len = match rng() % 4 {
                0 => 0,
                1 => 1,
                2 => (rng() >> 8) as usize % 32,
                _ => usize::MAX,
            };
            let viewport = (rng() >> 9) as usize % 8;
            match rng() % 6 {
                0 => cs.open(Some((rng() >> 7) as usize % 40)),
                1 => cs.close(),
                2 => cs.move_highlight((rng() as i64 as isize) % 11 - 5, len),
                3 => cs.reveal(viewport, len),
                4 => {
                    let _ = cs.choose(len);
                }
                _ => cs = CellSelectState::new(),
            }
            // choose() is always in-bounds when there are options.
            if let Some(i) = cs.choose(len) {
                assert!(i < len, "choose() escaped 0..len");
            }
        }
    }

    #[test]
    fn commit_edit_returns_the_cell_then_clears_editing() {
        let mut st = DataTableState::new();
        assert_eq!(st.commit_edit(), None);
        st.begin_edit(3, 2);
        assert_eq!(st.editing(), Some((3, 2)));
        assert_eq!(st.commit_edit(), Some((3, 2)));
        assert_eq!(st.editing(), None);
        st.begin_edit(1, 0);
        st.cancel_edit();
        assert_eq!(st.editing(), None);
    }

    #[test]
    fn hit_resolves_header_group_and_cell() {
        let c = cols(); // two Length(4) cols, spacing 1: col0 x∈[0,4), col1 x∈[5,9)
        let r = [
            DataRow::new(["a1", "x"]).group("G"),
            DataRow::new(["a2", "y"]).group("G"),
        ];
        let mut st = DataTableState::new();
        st.set_group_by(Some(0));
        let v = project(&c, &r, &st);
        let dt = DataTable::new(&c, &r, &v, &st);
        let area = Rect::new(0, 0, 9, 5);

        // Row 0 = header; click col 1's header.
        assert_eq!(
            dt.hit(area, Position::new(6, 0)),
            Some(DataTableHit::Header(1))
        );
        // Row 1 (body y=1) = the group header (visual index 0).
        assert_eq!(
            dt.hit(area, Position::new(2, 1)),
            Some(DataTableHit::Group(0))
        );
        // Row 2 (body y=2) = first data row (visual 1, source 0); col 1.
        assert_eq!(
            dt.hit(area, Position::new(6, 2)),
            Some(DataTableHit::Cell {
                visual: 1,
                source: 0,
                column: 1
            })
        );
        // Outside the grid.
        assert_eq!(dt.hit(area, Position::new(50, 50)), None);
    }

    #[test]
    fn cell_rect_is_some_only_while_the_cell_is_visible() {
        let c = vec![DataColumn::new("n").width(Constraint::Length(3))];
        let r: Vec<DataRow> = (0..10).map(|i| DataRow::new([i.to_string()])).collect();
        let st = DataTableState::new();
        let v = project(&c, &r, &st);
        let dt = DataTable::new(&c, &r, &v, &st).show_header(false);
        let area = Rect::new(0, 0, 3, 3);
        // Source 1 is visible at offset 0 (body rows 0..3) → row y=1.
        assert_eq!(dt.cell_rect(area, 1, 0), Some(Rect::new(0, 1, 3, 1)));
        // Source 8 is past the 3-row window → not visible.
        assert_eq!(dt.cell_rect(area, 8, 0), None);
        // Out-of-range column → None.
        assert_eq!(dt.cell_rect(area, 1, 9), None);
    }

    #[test]
    fn move_selection_saturates_within_the_visual_length() {
        let mut st = DataTableState::new();
        st.move_selection(3, 5);
        assert_eq!(st.selected(), Some(3));
        st.move_selection(99, 5);
        assert_eq!(st.selected(), Some(4)); // clamped to last
        st.move_selection(-99, 5);
        assert_eq!(st.selected(), Some(0)); // clamped to first
        st.move_selection(1, 0);
        assert_eq!(st.selected(), None); // empty clears
    }

    #[test]
    fn block_frames_the_grid_in_the_inner_area() {
        let c = vec![DataColumn::new("h").width(Constraint::Length(2))];
        let r = [DataRow::new(["hi"])];
        let st = DataTableState::new();
        let v = project(&c, &r, &st);
        assert_eq!(
            grid(
                DataTable::new(&c, &r, &v, &st).block(Block::bordered()),
                4,
                4
            ),
            "┌──┐\n│h │\n│hi│\n└──┘\n"
        );
    }

    #[test]
    fn ragged_rows_and_out_of_range_state_are_total() {
        // More cells than columns, fewer cells than columns, an out-of-range
        // sort and group column, a selection and edit past the end: no panic.
        let c = vec![DataColumn::new("a").width(Constraint::Length(2))];
        let r = [
            DataRow::new(["x", "extra"]),
            DataRow::new(Vec::<&str>::new()),
        ];
        let mut st = DataTableState::new();
        st.set_sort(Some((9, SortDirection::Ascending)));
        st.set_group_by(Some(9));
        st.select(Some(999));
        st.begin_edit(999, 999);
        let v = project(&c, &r, &st);
        let te = TextEdit::from_value("z");
        // Just must not panic.
        let _ = grid(DataTable::new(&c, &r, &v, &st).edit(&te), 2, 4);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let c = cols();
        let r = [DataRow::new(["a", "b"])];
        let st = DataTableState::new();
        let v = project(&c, &r, &st);
        let mut buf = Buffer::empty(Rect::new(0, 0, 9, 3));
        DataTable::new(&c, &r, &v, &st).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    /// The totality property (the iter-25 rule, mirroring
    /// [`ScrollState`]/[`Selection`]): any sequence of any state operation,
    /// followed by a [`project`] and a [`render`](Widget::render) /
    /// [`hit`](DataTable::hit) over randomly-sized data and areas (including
    /// the degenerate zeros), never panics.
    #[test]
    fn any_sequence_of_operations_then_project_and_render_is_total() {
        // Fixed-seed LCG keeps the run deterministic with no rand dep — the
        // technique scroll.rs / selection.rs use.
        let mut s: u64 = 0xDA7A_7AB1_E5EE_D001;
        let mut rng = || {
            s = s
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            s
        };

        let columns: Vec<DataColumn> = (0..3)
            .map(|i| {
                DataColumn::new(format!("c{i}"))
                    .width(Constraint::Length((i as u16 % 4) + 1))
                    .editable(i == 1)
            })
            .collect();

        let mut st = DataTableState::new();
        for _ in 0..4_000 {
            // A randomly-sized, randomly-grouped data set each iteration.
            let n = (rng() % 40) as usize;
            let rows: Vec<DataRow> = (0..n)
                .map(|_| {
                    let r = DataRow::new([
                        format!("n{}", rng() % 7),
                        format!("v{}", rng() % 5),
                        String::new(),
                    ]);
                    if rng() % 2 == 0 {
                        r.group(format!("g{}", rng() % 3))
                    } else {
                        r
                    }
                })
                .collect();

            let dir = |r: u64| {
                if r % 2 == 0 {
                    SortDirection::Ascending
                } else {
                    SortDirection::Descending
                }
            };
            match rng() % 15 {
                0 => st.toggle_sort((rng() % 5) as usize),
                1 => st.set_filter(format!("n{}", rng() % 9)),
                2 => st.clear_filter(),
                3 => st.set_group_by(Some((rng() % 5) as usize)),
                4 => st.set_group_by(None),
                5 => st.toggle_collapse(format!("g{}", rng() % 4)),
                6 => st.select(Some((rng() >> 3) as usize % 64)),
                7 => st.move_selection((rng() as i64 as isize) % 9 - 4, n),
                8 => st.scroll_by((rng() as i64 as isize) % 99 - 49, n, 4),
                9 => st.begin_edit((rng() >> 5) as usize % 50, (rng() % 5) as usize),
                10 => {
                    let _ = st.commit_edit();
                }
                11 => st.set_sort_keys([
                    ((rng() % 7) as usize, dir(rng())),
                    ((rng() % 7) as usize, dir(rng())),
                ]),
                12 => st.push_sort((rng() % 7) as usize, dir(rng())),
                13 => {
                    st.toggle_group_direction();
                    if rng() % 2 == 0 {
                        st.clear_sort();
                    }
                }
                _ => st = DataTableState::new(),
            }

            let visual = project(&columns, &rows, &st);
            st.clamp(visual.len(), 4);
            st.on_content_change(visual.len(), 4);

            let te = TextEdit::from_value("edit");
            let w = (rng() % 14) as u16;
            let h = (rng() % 8) as u16;
            let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
            let dt = DataTable::new(&columns, &rows, &visual, &st).edit(&te);
            // Render and every accessor over a random in/out-of-bounds point.
            dt.clone().render(buf.area(), &mut buf);
            let p = Position::new((rng() % 16) as u16, (rng() % 10) as u16);
            let _ = dt.hit(Rect::new(0, 0, w, h), p);
            let _ = dt.cell_rect(
                Rect::new(0, 0, w, h),
                (rng() % 50) as usize,
                (rng() % 5) as usize,
            );
        }
        // Reaching here proves no operation panicked for any input.
    }
}
