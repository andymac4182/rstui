//! A real-terminal [`DataTable`] fixture for the VHS end-to-end gate.
//!
//! `vhs/e2e/data-table.tape` runs this unmodified binary on an actual
//! terminal (ttyd) and drives it with real keystrokes — move the selection,
//! toggle the sort, begin an in-cell edit, type a new value, commit it, then
//! apply a filter — and asserts the exact final `STATE …` line in
//! `data-table.expect`. That proves the *whole* pipeline end to end on a
//! real terminal: crossterm key translation → the `run` loop → the
//! caller-owned [`DataTableState`] + the reducer-run
//! [`project`](rstui_widgets::data_table::project) pipeline → a borrowed
//! [`TextEdit`] edit written back into the data → the rendered frame.
//!
//! Mouse hit-testing is covered deterministically by the
//! `DataTable::hit`/`cell_rect` unit tests in `rstui-widgets`; this fixture
//! deliberately drives by keys only, exactly as `text_input_e2e` does, since
//! that is the part a *real terminal* must deliver and the part VHS scripts
//! reliably.
//!
//! ```text
//! cargo run -p rstui-crossterm --example data_table_e2e
//! ```
//!
//! `Esc` quits (and cancels an in-progress edit first). It needs a TTY, so
//! CI type-checks it (`--all-targets`) while the VHS gate runs it.

use std::error::Error;

use rstui_core::{Color, KeyCode, Line, Modifier, Position, Rect, Style, TextEdit, Widget};
use rstui_crossterm::run_app;
use rstui_runtime::{App, Cmd, Event, Frame};
use rstui_widgets::data_table::project;
use rstui_widgets::{Block, DataColumn, DataRow, DataTable, DataTableState, SortDirection};

/// The intents the fixture maps real terminal input to.
enum Msg {
    Down,
    Up,
    ToggleSort,
    ToggleGroup,
    BeginEdit,
    EditChar(char),
    EditBackspace,
    CommitEdit,
    CancelEdit,
    Filter,
    ClearFilter,
    Quit,
}

/// A key-driven [`DataTable`] over a tiny fixed data set. Everything the
/// widget reads — the columns, the source rows, the flattened projection,
/// the interaction state, the in-progress edit — is plain owned model state,
/// mutated only here in `update`, exactly the ADR 0012 contract.
struct DataTableFixture {
    columns: Vec<DataColumn<'static>>,
    rows: Vec<DataRow<'static>>,
    state: DataTableState,
    edit: TextEdit,
    visual: Vec<rstui_widgets::VisualRow>,
    /// The last committed `(source, column, value)` — surfaced in `STATE`
    /// so the gate can assert the edit was written back through the model.
    committed: Option<(usize, usize, String)>,
}

impl Default for DataTableFixture {
    fn default() -> Self {
        let columns = vec![
            DataColumn::new("name").width(rstui_core::Constraint::Length(6)),
            DataColumn::new("role")
                .width(rstui_core::Constraint::Length(8))
                .editable(true),
            DataColumn::new("team").width(rstui_core::Constraint::Length(5)),
        ];
        // Sorting col0 ascending yields Ada(src1), Bo(src2), Cy(src0): a
        // deterministic order the tape's scripted keystrokes rely on.
        let rows = vec![
            DataRow::new(["Cy", "infra", "Ops"]),
            DataRow::new(["Ada", "math", "Eng"]),
            DataRow::new(["Bo", "ui", "Eng"]),
        ];
        let state = DataTableState::new();
        let visual = project(&columns, &rows, &state);
        Self {
            columns,
            rows,
            state,
            edit: TextEdit::new(),
            visual,
            committed: None,
        }
    }
}

impl DataTableFixture {
    /// Re-run the pure pipeline after any data/spec change (the reducer's
    /// job — never the widget's).
    fn reproject(&mut self) {
        self.visual = project(&self.columns, &self.rows, &self.state);
    }

    /// The source-row index the current selection points at, if it is a
    /// data row (not a group header).
    fn selected_source(&self) -> Option<usize> {
        match self.visual.get(self.state.selected()?)? {
            rstui_widgets::VisualRow::Data { source } => Some(*source),
            rstui_widgets::VisualRow::Group { .. } => None,
        }
    }
}

impl App for DataTableFixture {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        let key = event.as_key_press()?;
        // While editing, keystrokes route to the borrowed TextEdit.
        if self.state.editing().is_some() {
            return Some(match key.code {
                KeyCode::Esc => Msg::CancelEdit,
                KeyCode::Enter => Msg::CommitEdit,
                KeyCode::Backspace => Msg::EditBackspace,
                KeyCode::Char(c) => Msg::EditChar(c),
                _ => return None,
            });
        }
        Some(match key.code {
            KeyCode::Esc => Msg::Quit,
            KeyCode::Down => Msg::Down,
            KeyCode::Up => Msg::Up,
            KeyCode::Char('s') => Msg::ToggleSort,
            KeyCode::Char('g') => Msg::ToggleGroup,
            KeyCode::Char('e') | KeyCode::Enter => Msg::BeginEdit,
            KeyCode::Char('F') => Msg::Filter,
            KeyCode::Char('f') => Msg::ClearFilter,
            _ => return None,
        })
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        let len = self.visual.len();
        match message {
            Msg::Down => {
                self.state.move_selection(1, len);
                self.state.reveal_selected(10, len);
            }
            Msg::Up => {
                self.state.move_selection(-1, len);
                self.state.reveal_selected(10, len);
            }
            Msg::ToggleSort => {
                self.state.toggle_sort(0);
                self.reproject();
            }
            Msg::ToggleGroup => {
                let g = if self.state.grouped_by().is_some() {
                    None
                } else {
                    Some(2)
                };
                self.state.set_group_by(g);
                self.reproject();
            }
            Msg::BeginEdit => {
                if let Some(src) = self.selected_source() {
                    self.state.begin_edit(src, 1);
                    // Seed empty so the typed value is exact for the gate.
                    self.edit = TextEdit::new();
                }
            }
            Msg::EditChar(c) => self.edit.insert_char(c),
            Msg::EditBackspace => {
                self.edit.delete_backward();
            }
            Msg::CommitEdit => {
                if let Some((src, col)) = self.state.editing() {
                    let value = self.edit.value().to_string();
                    if let Some(row) = self.rows.get_mut(src) {
                        // Write the edit back into the consumer's own data.
                        let mut cells: Vec<Line<'static>> = (0..self.columns.len())
                            .map(|i| row.cell(i).cloned().unwrap_or_else(|| Line::from("")))
                            .collect();
                        if col < cells.len() {
                            cells[col] = Line::from(value.clone());
                        }
                        *row = DataRow::new(cells);
                    }
                    self.committed = Some((src, col, value));
                    self.state.commit_edit();
                    self.reproject();
                }
            }
            Msg::CancelEdit => self.state.cancel_edit(),
            Msg::Filter => {
                self.state.set_filter("o");
                self.reproject();
                self.state.clamp(self.visual.len(), 10);
            }
            Msg::ClearFilter => {
                self.state.clear_filter();
                self.reproject();
            }
            Msg::Quit => return Cmd::quit(),
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let buf = frame.buffer_mut();
        buf.set_str(
            Position::new(0, 0),
            "rstui data-table e2e — Esc to quit",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        );

        DataTable::new(&self.columns, &self.rows, &self.visual, &self.state)
            .edit(&self.edit)
            .block(Block::bordered())
            .header_style(Style::new().fg(Color::Yellow))
            .highlight_style(Style::new().bg(Color::Blue))
            .render(Rect::new(0, 2, 28, 9), buf);

        // The deterministic marker line the .expect asserts. ASCII + exact.
        let sort = match self.state.sort() {
            None => "none".to_string(),
            Some((c, SortDirection::Ascending)) => format!("{c}:asc"),
            Some((c, SortDirection::Descending)) => format!("{c}:desc"),
        };
        let filter = {
            let f = self.state.filter();
            if f.is_empty() { "-" } else { f }
        };
        let group = self
            .state
            .grouped_by()
            .map_or_else(|| "none".to_string(), |c| c.to_string());
        let sel = self
            .state
            .selected()
            .map_or_else(|| "none".to_string(), |s| s.to_string());
        let edit = self
            .state
            .editing()
            .map_or_else(|| "none".to_string(), |(r, c)| format!("{r},{c}"));
        let commit = self
            .committed
            .as_ref()
            .map_or_else(|| "none".to_string(), |(r, c, v)| format!("{r},{c}={v}"));
        buf.set_str(
            Position::new(0, 12),
            &format!(
                "STATE sort={sort} filter={filter} group={group} \
                 sel={sel} edit={edit} commit={commit} rows={}",
                self.visual.len()
            ),
            Style::new(),
        );
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    run_app(DataTableFixture::default())?;
    Ok(())
}
