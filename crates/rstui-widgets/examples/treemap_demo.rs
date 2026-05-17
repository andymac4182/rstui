//! Exercises [`Treemap`] the way a dashboard does: an area-proportional
//! breakdown of caller-owned category weights (disk usage by directory), framed
//! and with a 1-cell gap between tiles.
//!
//! The tiles are plain caller-owned state — what an app's model holds and a
//! reducer recomputes (a `du -s` pass); [`Treemap`] only ever reads them (the
//! same pure projection [`List`]/[`BarChart`] use). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example treemap_demo
//! ```

use rstui_core::{Color, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Treemap, TreemapTile};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(48, 14)).expect("TestBackend is infallible");

    // The per-directory byte totals an app's model would own.
    let usage = [
        TreemapTile::new(420, Color::Blue, "target"),
        TreemapTile::new(180, Color::Green, ".git"),
        TreemapTile::new(96, Color::Magenta, "node_modules"),
        TreemapTile::new(40, Color::Cyan, "src"),
        TreemapTile::new(22, Color::Yellow, "docs"),
        TreemapTile::new(11, Color::Red, "assets"),
    ];

    terminal
        .draw(|frame| {
            frame.render_widget(
                Treemap::new(usage)
                    .padding(1)
                    .label_style(Style::new().fg(Color::Black))
                    .block(Block::bordered().title("disk by directory")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
