//! `Big Grid` — a **1,000,000 × 100** `DataTable` you can actually scroll.
//!
//! The worked reference for the data-table optimisations (see
//! `docs/datatable-optimization-roadmap.md`): the rows are **never
//! materialized**. A [`BigGrid`] generator implements [`RowSource`]
//! (DT-OPT-5b) and fabricates a [`DataRow`] only when one is asked for; the
//! screen owns just the flattened [`project_source`] index
//! (`Vec<VisualRow>`, ≈ 16 bytes/row ≈ 16 MB for a million rows — versus the
//! ≈ 10 GiB a materialized `Vec<DataRow>` of a million × 100 owned cells
//! would cost). Each frame [`materialize_window`] builds **only the ~visible
//! window** of rows, so the frame cost is flat in the total row count
//! (DataTable's vertical virtualization) and `←`/`→` scroll a horizontal
//! **column window** (DT-OPT-3); a vertical + horizontal [`Scrollbar`]
//! (DT-OPT-4) projects the true position over a million rows / a hundred
//! columns. Read-only — this screen is about *scale*, not editing.

use std::cell::Cell;

use rstui_core::{Constraint, KeyCode, Rect};
use rstui_runtime::Frame;
use rstui_widgets::data_table::{materialize_window, project_source};
use rstui_widgets::{
    Block, BorderType, DataColumn, DataRow, DataTable, DataTableState, RowSource, Scrollbar,
    ScrollbarOrientation, VisualRow,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// How many rows / columns the demo generates. A million rows × a hundred
/// columns = 100 M cells; materialized that is ≈ 10 GiB — here it is a
/// generator plus a ≈ 16 MB index, and only the visible window is ever
/// built.
const ROWS: usize = 1_000_000;
const COLS: usize = 100;

/// A [`RowSource`] that **generates** rows on demand and stores none —
/// DT-OPT-5b. Fabricating a [`DataRow`] for index `i` is deterministic and
/// `O(cols)`; the table never holds more than the visible window.
#[derive(Debug)]
struct BigGrid {
    rows: usize,
    cols: usize,
}

impl RowSource for BigGrid {
    fn row_count(&self) -> usize {
        self.rows
    }

    fn with_row<R>(&self, index: usize, f: impl FnOnce(&DataRow) -> R) -> Option<R> {
        (index < self.rows).then(|| {
            // Lightweight (DT-OPT-5a) `Cow<str>` cells, built on the stack
            // for this one call and dropped when it returns.
            let row = DataRow::text((0..self.cols).map(|c| format!("r{index}·c{c}")));
            f(&row)
        })
    }
}

#[derive(Debug)]
pub(crate) struct State {
    src: BigGrid,
    columns: Vec<DataColumn<'static>>,
    grid: DataTableState,
    /// The flattened projection, computed **once** (no sort/filter here, so
    /// it is identity) — the only large allocation: ≈ 16 B/row, not the
    /// ≈ 10 GiB a materialized million-row × 100-col table would be.
    visual: Vec<VisualRow>,
    body_rows: Cell<usize>,
}

impl State {
    pub(crate) fn new() -> Self {
        let src = BigGrid {
            rows: ROWS,
            cols: COLS,
        };
        // Fixed-width columns so the table is far wider than any terminal —
        // `←`/`→` scroll the DT-OPT-3 column window.
        let columns: Vec<DataColumn> = (0..src.cols)
            .map(|c| DataColumn::new(format!("col {c}")).width(Constraint::Length(12)))
            .collect();
        let grid = DataTableState::new();
        // DT-OPT-5b: project the *generator* — visits each row once, never
        // materializes the table. ≈ 16 MB for a million rows.
        let visual = project_source(&columns, &src, &grid);
        Self {
            src,
            columns,
            grid,
            visual,
            body_rows: Cell::new(20),
        }
    }

    fn metrics(&self) -> (usize, usize) {
        (self.visual.len(), self.body_rows.get().max(1))
    }

    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        let (total, rows) = self.metrics();
        match code {
            KeyCode::Up => self.grid.scroll_by(-1, total, rows),
            KeyCode::Down => self.grid.scroll_by(1, total, rows),
            KeyCode::PageUp => self.grid.scroll_by(-(rows as isize), total, rows),
            KeyCode::PageDown => self.grid.scroll_by(rows as isize, total, rows),
            KeyCode::Home => self.grid.scroll_to_top(),
            KeyCode::End => self.grid.scroll_to_end(total, rows),
            KeyCode::Left => self.grid.scroll_columns_by(-1, self.columns.len(), 1),
            KeyCode::Right => self.grid.scroll_columns_by(1, self.columns.len(), 1),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    pub(crate) fn on_scroll(&mut self, up: bool) {
        let (total, rows) = self.metrics();
        self.grid.scroll_by(if up { -3 } else { 3 }, total, rows);
    }

    fn layout(area: Rect) -> [Rect; 2] {
        let [grid, help] =
            rstui_core::Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
        [grid, help]
    }

    pub(crate) fn view(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let [grid_area, help_area] = Self::layout(area);
        // body = inner height minus the rounded border (2) and the header (1).
        let body_h = (grid_area.height.saturating_sub(3)).max(1) as usize;
        self.body_rows.set(body_h);

        let total = self.visual.len();
        let off = self.grid.offset().min(total.saturating_sub(body_h));
        let end = (off + body_h).min(total);

        // DT-OPT-5b: realize ONLY the visible window from the generator. The
        // projection indexes the full million-row space; the windowed render
        // is a fresh table at offset 0 over just these rows, with the source
        // indices rebased (there are no groups in this demo).
        let window = materialize_window(&self.src, &self.visual[off..end]);
        let local_rows: Vec<DataRow> = window.into_iter().map(|(_, r)| r).collect();
        let local_visual: Vec<VisualRow> = (0..local_rows.len())
            .map(|i| VisualRow::Data { source: i })
            .collect();

        // Replay only the horizontal column window onto the render state;
        // the vertical position is already applied by the windowing above.
        let mut rstate = DataTableState::new();
        if self.grid.col_offset() > 0 {
            rstate.scroll_columns_by(self.grid.col_offset() as isize, self.columns.len(), 1);
        }

        frame.render_widget(
            DataTable::new(&self.columns, &local_rows, &local_visual, &rstate)
                .block(Block::bordered().border_type(BorderType::Rounded))
                .style(theme.body())
                .header_style(theme.caption())
                .highlight_style(theme.focus_field()),
            grid_area,
        );

        // DT-OPT-4: scrollbars are pure projections of the true full-size
        // metrics (the widget draws none), overlaid on the rounded border.
        if grid_area.width > 2 && grid_area.height > 2 {
            frame.render_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .content_length(total)
                    .viewport_length(body_h)
                    .position(off)
                    .style(theme.body())
                    .thumb_style(theme.border_focused()),
                Rect::new(
                    grid_area.right().saturating_sub(1),
                    grid_area.y.saturating_add(1),
                    1,
                    grid_area.height.saturating_sub(2),
                ),
            );
            frame.render_widget(
                Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
                    .content_length(self.columns.len())
                    .viewport_length(1)
                    .position(self.grid.col_offset())
                    .style(theme.body())
                    .thumb_style(theme.border_focused()),
                Rect::new(
                    grid_area.x.saturating_add(1),
                    grid_area.bottom().saturating_sub(1),
                    grid_area.width.saturating_sub(2),
                    1,
                ),
            );
        }

        let col0 = self.grid.col_offset();
        frame.render_widget(
            rstui_widgets::Paragraph::new(format!(
                "{ROWS} rows × {COLS} cols — generated on demand (RowSource), \
                 never materialized · ↑↓/PgUp/PgDn/Home/End rows · ←→ cols \
                 (col {col0}) · row {off}",
            ))
            .style(theme.caption()),
            help_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The screen owns only the ≈16 B/row projection index — no
    /// `Vec<DataRow>` — and a window deep in the million-row space is
    /// generated on demand with the right content (DT-OPT-5b).
    #[test]
    fn windows_a_deep_band_from_the_generator_without_materializing() {
        let st = State::new();
        assert_eq!(st.visual.len(), ROWS, "projected, not materialized");

        let off = 987_654;
        let body = 40;
        let win = materialize_window(&st.src, &st.visual[off..off + body]);
        assert_eq!(win.len(), body, "only the visible band is realized");
        assert_eq!(win[0].0, off, "source index is preserved");
        // Generated content at depth — no row before `off` was ever built.
        assert_eq!(win[0].1.cell_text(0).as_deref(), Some("r987654·c0"));
        assert_eq!(
            win[body - 1].1.cell_text(COLS - 1).as_deref(),
            Some(format!("r{}·c{}", off + body - 1, COLS - 1).as_str())
        );
    }

    /// The scroll contract is total and clamped over a million rows / a
    /// hundred columns (the keys `view()` reads back for the scrollbars).
    #[test]
    fn scroll_keys_are_total_and_clamped() {
        let mut st = State::new();
        st.body_rows.set(40);
        st.on_key(KeyCode::Down);
        assert_eq!(st.grid.offset(), 1);
        st.on_key(KeyCode::PageDown);
        assert_eq!(st.grid.offset(), 41);
        st.on_key(KeyCode::Home);
        assert_eq!(st.grid.offset(), 0);
        st.on_key(KeyCode::End);
        assert_eq!(
            st.grid.offset(),
            ROWS - 40,
            "End parks the last full window"
        );
        st.on_key(KeyCode::Up); // can't over-scroll past the top
        st.on_key(KeyCode::Home);
        assert_eq!(st.grid.offset(), 0);

        assert_eq!(st.grid.col_offset(), 0);
        st.on_key(KeyCode::Right);
        assert_eq!(st.grid.col_offset(), 1);
        st.on_key(KeyCode::Left);
        st.on_key(KeyCode::Left); // clamped, never negative/panics
        assert_eq!(st.grid.col_offset(), 0);

        assert!(
            !st.on_key(KeyCode::Char('z')).handled,
            "an unhandled key is ignored so the shell can act"
        );
    }
}
