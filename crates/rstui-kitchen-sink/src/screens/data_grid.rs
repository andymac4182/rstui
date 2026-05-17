//! The comprehensive [`DataTable`] showcase: a sortable / filterable /
//! groupable, virtualized, mouse-hit-testable grid whose cells are **real
//! form fields** — an editable text `name`, a [`Select`](rstui_widgets::Select)
//! dropdown `role`, a [`Checkbox`](rstui_widgets::Checkbox) `active` — beside
//! a grouped `team`.
//!
//! Every bit of interaction — sort, filter, group/collapse, scroll,
//! selection, the in-progress text edit, *and* the open dropdown — is plain
//! caller-owned [`DataTableState`] / [`TextEdit`] / [`CellSelectState`]
//! mutated only here; the reducer re-runs the pure [`project`] pipeline and
//! the widget only *reads* it (ADR 0012 / ADR 0014). The cell [`Line`] is
//! always the value of record — toggling a checkbox or choosing a dropdown
//! option just writes that text back, the same hook a text edit uses.

use rstui_core::{Constraint, KeyCode, Layout, Line, Position, Rect, TextEdit, stylize::Stylize};
use rstui_runtime::Frame;
use rstui_widgets::data_table::project;
use rstui_widgets::{
    Block, BorderType, CellField, CellSelectState, DataColumn, DataRow, DataTable, DataTableHit,
    DataTableState, Paragraph, SortDirection, VisualRow, Wrap, cell_truthy,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

const HEADERS: [&str; 4] = ["name", "role", "team", "active"];
/// The `role` dropdown's options (also the seed values).
const ROLES: [&str; 6] = ["Engineer", "Designer", "SRE", "PM", "Analyst", "Lead"];
/// The group-by column (`team`).
const GROUP_COL: usize = 2;

/// A tiny employee directory — `name`, `role` (dropdown), `team` (group),
/// `active` (checkbox). Enough rows to make the virtualized scroll visible.
const DATA: [[&str; 4]; 18] = [
    ["Ada", "Engineer", "Platform", "true"],
    ["Bo", "Designer", "Product", "true"],
    ["Cy", "SRE", "Platform", "false"],
    ["Di", "PM", "Product", "true"],
    ["Eve", "Engineer", "Platform", "false"],
    ["Fox", "Analyst", "Data", "true"],
    ["Gem", "Engineer", "Data", "true"],
    ["Hal", "SRE", "Platform", "false"],
    ["Ivy", "Designer", "Product", "true"],
    ["Jo", "Engineer", "Data", "true"],
    ["Kai", "Lead", "Platform", "true"],
    ["Lee", "Analyst", "Data", "false"],
    ["Mia", "PM", "Product", "true"],
    ["Ned", "Engineer", "Platform", "true"],
    ["Oz", "SRE", "Data", "false"],
    ["Pia", "Designer", "Product", "true"],
    ["Quinn", "Engineer", "Platform", "false"],
    ["Ravi", "Lead", "Data", "true"],
];

/// Everything the grid screen owns. The widget reads it; only `on_*`
/// mutates it.
#[derive(Debug)]
pub(crate) struct State {
    columns: Vec<DataColumn<'static>>,
    rows: Vec<DataRow<'static>>,
    grid: DataTableState,
    /// The `name` text editor (borrowed by the widget while editing).
    edit: TextEdit,
    /// The `role` dropdown's open/highlight (borrowed while open).
    choice: CellSelectState,
    visual: Vec<VisualRow>,
    /// The column `s`/`e` act on (the keyboard's "active column").
    active_col: usize,
    /// `/` filter-entry mode: typed chars extend the filter live.
    filtering: bool,
    /// `G` opens the modal group/sort config panel overlay.
    config_open: bool,
    /// Body rows the last `view` showed — so `on_scroll`/paging know the
    /// viewport length without an area (the lib.rs geom-cache idiom).
    body_rows: std::cell::Cell<usize>,
}

impl State {
    /// A fresh grid: the directory unsorted, ungrouped, nothing selected.
    pub(crate) fn new() -> Self {
        let columns = vec![
            DataColumn::new(HEADERS[0])
                .width(Constraint::Length(8))
                .editable(true),
            DataColumn::new(HEADERS[1])
                .width(Constraint::Length(12))
                .field(CellField::select(ROLES)),
            DataColumn::new(HEADERS[2]).width(Constraint::Length(10)),
            DataColumn::new(HEADERS[3])
                .width(Constraint::Length(8))
                .field(CellField::Checkbox),
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
            choice: CellSelectState::new(),
            visual,
            active_col: 0,
            filtering: false,
            config_open: false,
            body_rows: std::cell::Cell::new(10),
        }
    }

    /// Re-run the pure pipeline after any data/spec change.
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

    /// The plain text of cell `(src, col)`.
    fn cell_text(&self, src: usize, col: usize) -> String {
        self.rows
            .get(src)
            .and_then(|r| r.cell(col))
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .unwrap_or_default()
    }

    /// Write `value` into cell `(src, col)` (the consumer's data — the
    /// "pass the change back" hook), then re-project.
    fn set_cell(&mut self, src: usize, col: usize, value: String) {
        if let Some(row) = self.rows.get_mut(src) {
            let mut cells: Vec<Line<'static>> = (0..self.columns.len())
                .map(|i| row.cell(i).cloned().unwrap_or_else(|| Line::from("")))
                .collect();
            if col < cells.len() {
                cells[col] = Line::from(value);
            }
            *row = DataRow::new(cells);
        }
        self.reproject();
    }

    /// This column's dropdown options, if it is a [`CellField::Select`].
    fn options(&self, col: usize) -> Option<Vec<String>> {
        match self.columns.get(col)?.cell_field() {
            CellField::Select(opts) => Some(opts.iter().map(|c| c.as_ref().to_string()).collect()),
            _ => None,
        }
    }

    /// Activate the selected cell's control, dispatching on its field kind:
    /// checkbox → toggle, dropdown → open, editable text → begin edit.
    fn activate(&mut self) -> ScreenOutcome {
        let Some(src) = self.selected_source() else {
            return ScreenOutcome::ignored();
        };
        let col = self.active_col;
        match self.columns.get(col).map(DataColumn::cell_field) {
            Some(CellField::Checkbox) => {
                let now = !cell_truthy(&self.cell_text(src, col));
                self.set_cell(src, col, if now { "true" } else { "false" }.into());
                ScreenOutcome::with_toast(
                    rstui_widgets::ToastLevel::Info,
                    format!("active = {now}"),
                )
            }
            Some(CellField::Select(_)) => {
                let cur = self.options(col).and_then(|o| {
                    let t = self.cell_text(src, col);
                    o.iter().position(|x| *x == t)
                });
                self.grid.begin_edit(src, col);
                self.choice.open(cur);
                ScreenOutcome::consumed()
            }
            _ if self.columns.get(col).is_some_and(DataColumn::is_editable) => {
                let seed = self.cell_text(src, col);
                self.edit = TextEdit::from_value(seed);
                self.edit.move_end();
                self.grid.begin_edit(src, col);
                ScreenOutcome::consumed()
            }
            _ => ScreenOutcome::ignored(),
        }
    }

    /// Commit the open dropdown: write the highlighted option back.
    fn commit_choice(&mut self) -> ScreenOutcome {
        let Some((src, col)) = self.grid.editing() else {
            return ScreenOutcome::ignored();
        };
        let opts = self.options(col).unwrap_or_default();
        let picked = self.choice.choose(opts.len());
        self.grid.commit_edit();
        if let Some(i) = picked {
            if let Some(v) = opts.get(i).cloned() {
                self.set_cell(src, col, v.clone());
                return ScreenOutcome::with_toast(
                    rstui_widgets::ToastLevel::Success,
                    format!("role = {v}"),
                );
            }
        }
        self.reproject();
        ScreenOutcome::consumed()
    }

    /// A click chose option `index` directly (the mouse dual of
    /// `commit_choice` — `DataTable::hit` already mapped the panel click to
    /// the exact option, so there is no off-by-the-row-below).
    fn pick_option(&mut self, index: usize) -> ScreenOutcome {
        let Some((src, col)) = self.grid.editing() else {
            return ScreenOutcome::ignored();
        };
        let opts = self.options(col).unwrap_or_default();
        self.choice.close();
        self.grid.commit_edit();
        if let Some(v) = opts.get(index).cloned() {
            self.set_cell(src, col, v.clone());
            return ScreenOutcome::with_toast(
                rstui_widgets::ToastLevel::Success,
                format!("role = {v}"),
            );
        }
        self.reproject();
        ScreenOutcome::consumed()
    }

    /// Commit the in-progress text edit.
    fn commit_edit(&mut self) -> ScreenOutcome {
        let Some((src, col)) = self.grid.editing() else {
            return ScreenOutcome::ignored();
        };
        let value = self.edit.value().to_string();
        self.grid.commit_edit();
        self.set_cell(src, col, value);
        ScreenOutcome::with_toast(rstui_widgets::ToastLevel::Success, "cell updated")
    }

    /// `(grid, help, readout)` from the screen area — one definition so
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

    /// Route a key to the grid.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        let vlen = self.visual.len();
        let rows = self.body_rows.get().max(1);

        // 1. An open dropdown owns navigation until a choice is made.
        if self.choice.is_open() {
            let len = self
                .grid
                .editing()
                .and_then(|(_, c)| self.options(c))
                .map_or(0, |o| o.len());
            match code {
                KeyCode::Up => {
                    self.choice.move_highlight(-1, len);
                    self.choice.reveal(6, len);
                }
                KeyCode::Down => {
                    self.choice.move_highlight(1, len);
                    self.choice.reveal(6, len);
                }
                KeyCode::Enter => return self.commit_choice(),
                _ => return ScreenOutcome::ignored(),
            }
            return ScreenOutcome::consumed();
        }

        // 2. A text edit routes keystrokes to the borrowed TextEdit.
        if self.grid.editing().is_some() {
            match code {
                KeyCode::Enter => return self.commit_edit(),
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

        // 3. Filter-entry mode (`/`).
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

        // 4. Command mode.
        match code {
            KeyCode::Up => {
                self.grid.move_selection(-1, vlen);
                self.grid.reveal_selected(rows, vlen);
            }
            KeyCode::Down => {
                self.grid.move_selection(1, vlen);
                self.grid.reveal_selected(rows, vlen);
            }
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
                    Some(GROUP_COL)
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
            // The modal group/sort config panel — pick the group column and
            // the (multi-key) sort independently. Mouse-driven once open.
            KeyCode::Char('G') => self.config_open = !self.config_open,
            KeyCode::Char('e' | ' ') | KeyCode::Enter => return self.activate(),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Route a click: real widget geometry via [`DataTable::hit`] — header →
    /// sort, group → collapse, cell → select then activate its control
    /// (toggle a checkbox, open a dropdown, edit text).
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [grid, ..] = Self::layout(content);
        // Build it EXACTLY as `view` does (incl. `.cell_select`) so `hit`
        // resolves clicks against the same geometry — including the open
        // dropdown panel overlay.
        let hit = DataTable::new(&self.columns, &self.rows, &self.visual, &self.grid)
            .cell_select(&self.choice)
            .config(self.config_open)
            .block(Block::bordered().border_type(BorderType::Rounded))
            .hit(grid, pos);
        match hit {
            // ---- the modal group/sort config panel ----
            Some(DataTableHit::ConfigGroup(col)) => {
                // Toggle this column as the (independent) group column.
                let g = (self.grid.grouped_by() != Some(col)).then_some(col);
                self.grid.set_group_by(g);
                self.reproject();
                ScreenOutcome::consumed()
            }
            Some(DataTableHit::ConfigSort(col)) => {
                // Cycle this column in the ordered sort keys:
                // absent → Ascending → Descending → removed.
                let mut keys: Vec<_> = self.grid.sort_keys().to_vec();
                match keys.iter().position(|(c, _)| *c == col) {
                    None => keys.push((col, SortDirection::Ascending)),
                    Some(i) if keys[i].1 == SortDirection::Ascending => {
                        keys[i].1 = SortDirection::Descending;
                    }
                    Some(i) => {
                        keys.remove(i);
                    }
                }
                self.grid.set_sort_keys(keys);
                self.reproject();
                ScreenOutcome::consumed()
            }
            Some(DataTableHit::ConfigGroupDirection) => {
                self.grid.toggle_group_direction();
                self.reproject();
                ScreenOutcome::consumed()
            }
            Some(DataTableHit::ConfigClose) => {
                self.config_open = false;
                ScreenOutcome::consumed()
            }
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
            Some(DataTableHit::DropdownOption { index, .. }) => self.pick_option(index),
            Some(DataTableHit::Cell { visual, column, .. }) => {
                self.grid.select(Some(visual));
                self.active_col = column;
                self.activate()
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
        self.body_rows
            .set((grid_area.height.saturating_sub(3)).max(1) as usize);

        frame.render_widget(
            DataTable::new(&self.columns, &self.rows, &self.visual, &self.grid)
                .edit(&self.edit)
                .cell_select(&self.choice)
                .config(self.config_open)
                .block(Block::bordered().border_type(BorderType::Rounded))
                .style(theme.body())
                .header_style(theme.caption())
                .group_style(theme.border_focused())
                .highlight_style(theme.focus_field()),
            grid_area,
        );

        let active = HEADERS.get(self.active_col).copied().unwrap_or("?");
        let hint = if self.config_open {
            "group/sort panel — click G | sort per column · order row · outside: close".to_string()
        } else if self.choice.is_open() {
            "dropdown — ↑↓ highlight · Enter choose".to_string()
        } else if self.grid.editing().is_some() {
            "editing — type · ←→ caret · Backspace · Enter save".to_string()
        } else if self.filtering {
            "filter — type to match · Backspace · Enter done".to_string()
        } else {
            format!(
                "↑↓ select · [ ] col=({active}) · s sort · o group · c collapse · / filter · G panel · Space/e field"
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
                 rows={} sel={:?} fields: name=text role=▾ active=☑",
                self.visual.len(),
                self.grid.selected(),
            ))
            .style(theme.caption())
            .wrap(Wrap { trim: true }),
            readout,
        );
    }
}
