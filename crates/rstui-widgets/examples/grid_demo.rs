//! Exercises [`Grid`] the way a real dashboard will: a framed 2×2 tile grid
//! with a gutter, the caller rendering its own widget into each cell.
//!
//! [`Grid`] is pure layout — it reuses the core `Constraint` divider on each
//! axis (never a new solver), hands back one `Rect` per cell, and owns no
//! state. Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test of the 2-D layout:
//!
//! ```text
//! cargo run -p rstui-widgets --example grid_demo
//! ```

use rstui_core::{Constraint, Terminal, TestBackend};
use rstui_widgets::{Block, Grid, Paragraph};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 11)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let grid = Grid::new(
                [Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)],
                [Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)],
            )
            .spacing(1)
            .block(Block::bordered().title("dashboard"));

            // Pure layout: the widget gives back the cell rects, the caller
            // renders its own children into them.
            let cells = grid.split(frame.area());
            frame.render_widget(grid, frame.area());

            for (r, row) in cells.iter().enumerate() {
                for (c, &cell) in row.iter().enumerate() {
                    frame.render_widget(
                        Paragraph::new(format!("tile {r},{c}")).block(Block::bordered()),
                        cell,
                    );
                }
            }
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
