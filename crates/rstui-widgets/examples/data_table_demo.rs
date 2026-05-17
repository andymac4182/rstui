//! Exercises [`DataTable`] the way a real data grid will: a framed,
//! grouped + sorted + row-selected grid beside a filtered grid with an
//! **in-cell edit in progress** (a borrowed [`TextEdit`] with a live caret).
//!
//! Every bit of interaction state — the sort, the collapsed group, the
//! selection, the scroll, the filter, and which cell is being edited — is
//! plain [`DataTableState`] / [`TextEdit`] the way it would be fields of an
//! app's model. The reducer flattens once with [`project`]; the widget only
//! ever *reads* the result. Running over a [`TestBackend`] keeps it TTY-free,
//! so it doubles as a deterministic snapshot smoke test of the grid:
//!
//! ```text
//! cargo run -p rstui-widgets --example data_table_demo
//! ```

use rstui_core::{Color, Constraint, Modifier, Style, Terminal, TestBackend, TextEdit};
use rstui_widgets::{Block, DataColumn, DataRow, DataTable, DataTableState, data_table::project};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(72, 9)).expect("TestBackend is infallible");

    // Caller-owned data: a "role" column the consumer marked editable.
    let columns = [
        DataColumn::new("name").width(Constraint::Fill(1)),
        DataColumn::new("role")
            .width(Constraint::Length(8))
            .editable(true),
        DataColumn::new("team").width(Constraint::Length(4)),
    ];
    let rows = [
        DataRow::new(["Ada", "math", "Eng"]).group("Eng"),
        DataRow::new(["Cy", "infra", "Ops"]).group("Ops"),
        DataRow::new(["Bo", "ui", "Eng"]).group("Eng"),
        DataRow::new(["Di", "sre", "Ops"]).group("Ops"),
    ];

    terminal
        .draw(|frame| {
            let [grid, edit] = rstui_core::Layout::horizontal([
                Constraint::Percentage(52),
                Constraint::Percentage(48),
            ])
            .areas(frame.area());

            // Left: grouped by "team", sorted by "name" ascending (the header
            // shows a ▲), the "Ops" group collapsed, and a selected row — all
            // caller-owned state the reducer set in `update`.
            let mut state = DataTableState::new();
            state.set_group_by(Some(2));
            state.toggle_sort(0); // name ▲
            state.toggle_collapse("Ops"); // collapse the Ops group
            state.select(Some(2)); // the "Bo" row under the Eng header
            let visual = project(&columns, &rows, &state);
            frame.render_widget(
                DataTable::new(&columns, &rows, &visual, &state)
                    .block(Block::bordered().title("people · grouped/sorted"))
                    .header_style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                    .group_style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                    .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                grid,
            );

            // Right: a filter ("a" → only rows containing it) and an in-cell
            // edit underway. The reducer began the edit and owns the TextEdit;
            // the widget renders it (reusing Input) with a live caret.
            let mut estate = DataTableState::new();
            estate.set_filter("a");
            let evisual = project(&columns, &rows, &estate);
            // Edit "Ada"'s role cell — source row 0, the editable column 1.
            estate.begin_edit(0, 1);
            let mut buffer = TextEdit::from_value("algebra");
            buffer.move_end();
            frame.render_widget(
                DataTable::new(&columns, &rows, &evisual, &estate)
                    .edit(&buffer)
                    .block(Block::bordered().title("filter \"a\" · editing role"))
                    .header_style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                    .cursor_style(Style::new().bg(Color::Green).fg(Color::Black)),
                edit,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
