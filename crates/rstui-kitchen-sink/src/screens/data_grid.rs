//! The comprehensive [`DataTable`] showcase: a sortable / filterable /
//! groupable, virtualized, mouse-hit-testable grid with optional in-cell
//! editing.
//!
//! Every bit of interaction — the sort, the filter string, the group + its
//! collapsed set, the scroll offset, the selection, and the in-progress
//! cell edit — is plain caller-owned [`DataTableState`] / [`TextEdit`]
//! mutated only here in the screen handler; the reducer re-runs the pure
//! [`project`] pipeline and the widget only *reads* the result (ADR 0012 /
//! ADR 0014). Keys drive everything and the mouse hits real geometry via
//! [`DataTable::hit`].

use rstui_core::{Constraint, KeyCode, Layout, Line, Position, Rect, TextEdit, stylize::Stylize};
use rstui_runtime::Frame;
use rstui_widgets::data_table::project;
use rstui_widgets::{
    Block, BorderType, DataColumn, DataRow, DataTable, DataTableHit, DataTableState, Paragraph,
    SortDirection, VisualRow, Wrap,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The columns: `team` is the group key; `role` and `status` are editable.
const HEADERS: [&str; 4] = ["name", "role", "team", "status"];

/// A tiny employee directory — enough rows to make the virtualized scroll
/// and grouping visible.
const DATA: [[&str; 4]; 18] = [
    ["Ada", "Engineer", "Platform", "active"],
    ["Bo", "Designer", "Product", "active"],
    ["Cy", "SRE", "Platform", "on-call"],
    ["Di", "PM", "Product", "active"],
    ["Eve", "Engineer", "Platform", "leave"],
    ["Fox", "Analyst", "Data", "active"],
    ["Gem", "Engineer", "Data", "active"],
    ["Hal", "SRE", "Platform", "on-call"],
    ["Ivy", "Designer", "Product", "leave"],
    ["Jo", "Engineer", "Data", "active"],
    ["Kai", "Lead", "Platform", "active"],
    ["Lee", "Analyst", "Data", "active"],
    ["Mia", "PM", "Product", "active"],
    ["Ned", "Engineer", "Platform", "active"],
    ["Oz", "SRE", "Data", "on-call"],
    ["Pia", "Designer", "Product", "active"],
    ["Quinn", "Engineer", "Platform", "leave"],
    ["Ravi", "Lead", "Data", "active"],
];

/// Everything the grid screen owns. The widget reads it; only `on_*`
/// mutates it.
#[derive(Debug)]
pub(crate) struct State {
    columns: Vec<DataColumn<'static>>,
    rows: Vec<DataRow<'static>>,
    grid: DataTableState,
    edit: TextEdit,
    visual: Vec<VisualRow>,
    /// The column `s`/`o`/`e` act on (the keyboard's "active column").
    active_col: usize,
    /// `/` filter-entry mode: typed chars extend the filter live.
    filtering: bool,
    /// Body rows the last `view` showed — so `on_scroll` / paging know the
    /// viewport length without an area (the lib.rs geom-cache idiom).
    body_rows: std::cell::Cell<usize>,
}

impl State {
    /// A fresh grid: the directory unsorted, ungrouped, nothing selected.
    pub(crate) fn new() -> Self {
        let columns = vec![
            DataColumn::new(HEADERS[0]).width(Constraint::Length(8)),
            DataColumn::new(HEADERS[1])
                .width(Constraint::Length(10))
                .editable(true),
            DataColumn::new(HEADERS[2]).width(Constraint::Length(10)),
            DataColumn::new(HEADERS[3])
                .width(Constraint::Length(8))
                .editable(true),
        ];
        let rows: Vec<DataRow<'static>> = DATA
            .iter()
            .map(|r| DataRow::new(r.iter().copied()))
            .collect();
        let grid = DataTableState::new();
        let visual = project(&columns, &rows, &grid);
        Self {
            columns,
            rows,
            grid,
            edit: TextEdit::new(),
            visual,
            active_col: 0,
            filtering: false,
            body_rows: std::cell::Cell::new(10),
        }
    }

    /// Re-run the pure pipeline after any data/spec change (the reducer's
    /// job — never the widget's).
    fn reproject(&mut self) {
        self.visual = project(&self.columns, &self.rows, &self.grid);
    }

    /// The source-row index the selection points at, if it is a data row.
    fn selected_source(&self) -> Option<usize> {
        match self.visual.get(self.grid.selected()?)? {
            VisualRow::Data { source } => Some(*source),
            VisualRow::Group { .. } => None,
        }
    }

    /// Begin editing the selected row's active column (if editable).
    fn begin_edit(&mut self) {
        let Some(src) = self.selected_source() else {
            return;
        };
        if !self
            .columns
            .get(self.active_col)
            .is_some_and(rstui_widgets::DataColumn::is_editable)
        {
            return;
        }
        let seed = self
            .rows
            .get(src)
            .and_then(|r| r.cell(self.active_col))
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .unwrap_or_default();
        self.edit = TextEdit::from_value(seed);
        self.edit.move_end();
        self.grid.begin_edit(src, self.active_col);
    }

    /// Write the in-progress edit back into the owned data (the consumer's
    /// job — the "pass the change back" hook), then re-project.
    fn commit_edit(&mut self) -> bool {
        let Some((src, col)) = self.grid.editing() else {
            return false;
        };
        let value = self.edit.value().to_string();
        if let Some(row) = self.rows.get_mut(src) {
            let mut cells: Vec<Line<'static>> = (0..self.columns.len())
                .map(|i| row.cell(i).cloned().unwrap_or_else(|| Line::from("")))
                .collect();
            if col < cells.len() {
                cells[col] = Line::from(value);
            }
            *row = DataRow::new(cells);
        }
        self.grid.commit_edit();
        self.reproject();
        true
    }

    /// `(grid, help, readout)` from the screen area — computed one way so
    /// `view` and `on_click` hit-test identically (the forms.rs idiom).
    fn layout(area: Rect) -> [Rect; 3] {
        let [grid, help, readout] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .areas(area);
        [grid, help, readout]
    }

    /// The configured widget — built one way so `view` render and
    /// `on_click` [`DataTable::hit`] always agree on the geometry.
    fn widget<'a>(&'a self, theme: &Theme) -> DataTable<'a> {
        DataTable::new(&self.columns, &self.rows, &self.visual, &self.grid)
            .edit(&self.edit)
            .block(Block::bordered().border_type(BorderType::Rounded))
            .style(theme.body())
            .header_style(theme.caption())
            .group_style(theme.border_focused())
            .highlight_style(theme.focus_field())
    }

    /// Route a key to the grid.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        let vlen = self.visual.len();
        let rows = self.body_rows.get().max(1);

        // 1. Editing a cell: keystrokes route to the borrowed TextEdit.
        if self.grid.editing().is_some() {
            match code {
                KeyCode::Enter => {
                    self.commit_edit();
                    return ScreenOutcome::with_toast(
                        rstui_widgets::ToastLevel::Success,
                        "cell updated",
                    );
                }
                KeyCode::Backspace => {
                    self.edit.delete_backward();
                }
                KeyCode::Left => {
                    self.edit.move_left();
                }
                KeyCode::Right => {
                    self.edit.move_right();
                }
                KeyCode::Char(c) => self.edit.insert_char(c),
                _ => return ScreenOutcome::ignored(),
            }
            return ScreenOutcome::consumed();
        }

        // 2. Filter-entry mode (`/`): typed chars extend the filter live.
        if self.filtering {
            match code {
                KeyCode::Enter => self.filtering = false,
                KeyCode::Backspace => {
                    let mut f = self.grid.filter().to_string();
                    f.pop();
                    self.grid.set_filter(f);
                    self.reproject();
                }
                KeyCode::Char(c) => {
                    let mut f = self.grid.filter().to_string();
                    f.push(c);
                    self.grid.set_filter(f);
                    self.reproject();
                }
                _ => return ScreenOutcome::ignored(),
            }
            return ScreenOutcome::consumed();
        }

        // 3. Command mode.
        match code {
            KeyCode::Up => {
                self.grid.move_selection(-1, vlen);
                self.grid.reveal_selected(rows, vlen);
            }
            KeyCode::Down => {
                self.grid.move_selection(1, vlen);
                self.grid.reveal_selected(rows, vlen);
            }
            // Left at the screen edge falls back to the rail (shell idiom).
            KeyCode::Left => return ScreenOutcome::ignored(),
            KeyCode::PageUp => self.grid.scroll_by(-(rows as isize), vlen, rows),
            KeyCode::PageDown => self.grid.scroll_by(rows as isize, vlen, rows),
            KeyCode::Char('[') => self.active_col = self.active_col.saturating_sub(1),
            KeyCode::Char(']') => {
                self.active_col = (self.active_col + 1).min(self.columns.len().saturating_sub(1));
            }
            KeyCode::Char('s') => {
                self.grid.toggle_sort(self.active_col);
                self.reproject();
            }
            KeyCode::Char('o') => {
                let g = if self.grid.grouped_by().is_some() {
                    None
                } else {
                    Some(2) // group by `team`
                };
                self.grid.set_group_by(g);
                self.reproject();
            }
            KeyCode::Char('c') => {
                if let Some(VisualRow::Group { key, .. }) =
                    self.grid.selected().and_then(|i| self.visual.get(i))
                {
                    let key = key.clone();
                    self.grid.toggle_collapse(key);
                    self.reproject();
                }
            }
            KeyCode::Char('/') => {
                self.filtering = true;
                self.grid.set_filter(String::new());
                self.reproject();
            }
            KeyCode::Char('e') | KeyCode::Enter => self.begin_edit(),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Route a click: the mouse hits real widget geometry via
    /// [`DataTable::hit`] — header → sort, group row → collapse, cell →
    /// select (+ begin edit when the column is editable).
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [grid, ..] = Self::layout(content);
        // Build the widget exactly as `view` does so the hit geometry agrees.
        let hit = DataTable::new(&self.columns, &self.rows, &self.visual, &self.grid)
            .block(Block::bordered().border_type(BorderType::Rounded))
            .hit(grid, pos);
        match hit {
            Some(DataTableHit::Header(col)) => {
                self.active_col = col;
                self.grid.toggle_sort(col);
                self.reproject();
                ScreenOutcome::consumed()
            }
            Some(DataTableHit::Group(visual)) => {
                self.grid.select(Some(visual));
                if let Some(VisualRow::Group { key, .. }) = self.visual.get(visual) {
                    let key = key.clone();
                    self.grid.toggle_collapse(key);
                    self.reproject();
                }
                ScreenOutcome::consumed()
            }
            Some(DataTableHit::Cell {
                visual,
                source: _,
                column,
            }) => {
                self.grid.select(Some(visual));
                self.active_col = column;
                self.begin_edit();
                ScreenOutcome::consumed()
            }
            None => ScreenOutcome::ignored(),
        }
    }

    /// Wheel scroll — fast, virtualized (the whole point of the grid).
    pub(crate) fn on_scroll(&mut self, up: bool) {
        let vlen = self.visual.len();
        let rows = self.body_rows.get().max(1);
        self.grid.scroll_by(if up { -3 } else { 3 }, vlen, rows);
    }

    /// Draw the grid, a keymap hint, and a live state readout.
    pub(crate) fn view(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let [grid_area, help_area, readout] = Self::layout(area);
        // Header row + frame borders take 3 rows; the rest is the body —
        // remember it so scroll/paging know the viewport length.
        self.body_rows
            .set((grid_area.height.saturating_sub(3)).max(1) as usize);

        frame.render_widget(self.widget(theme), grid_area);

        let active = HEADERS.get(self.active_col).copied().unwrap_or("?");
        let hint = if self.grid.editing().is_some() {
            "editing — type · ←→ caret · Backspace · Enter save".to_string()
        } else if self.filtering {
            "filter — type to match · Backspace · Enter done".to_string()
        } else {
            format!(
                "↑↓ select · [ ] col=({active}) · s sort · o group · c collapse · / filter · e edit"
            )
        };
        frame.render_widget(Line::from(hint.fg(theme.dim)), help_area);

        let sort = match self.grid.sort() {
            None => "none".to_string(),
            Some((c, SortDirection::Ascending)) => format!("{}▲", HEADERS.get(c).unwrap_or(&"?")),
            Some((c, SortDirection::Descending)) => {
                format!("{}▼", HEADERS.get(c).unwrap_or(&"?"))
            }
        };
        let filter = {
            let f = self.grid.filter();
            if f.is_empty() {
                "-".to_string()
            } else {
                format!("{f:?}")
            }
        };
        let group = self
            .grid
            .grouped_by()
            .map_or_else(|| "none".to_string(), |_| "team".to_string());
        frame.render_widget(
            Paragraph::new(format!(
                "live model → sort={sort} filter={filter} group={group} \
                 rows={} sel={:?} editing={:?}",
                self.visual.len(),
                self.grid.selected(),
                self.grid.editing(),
            ))
            .style(theme.caption())
            .wrap(Wrap { trim: true }),
            readout,
        );
    }
}
