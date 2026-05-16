//! Exercises [`Tree`] the way a real file explorer will: a framed, indented
//! column of expand/collapse rows with `│` ancestor guides and one row
//! selected (the keyboard target).
//!
//! The flattened `Vec<TreeItem>` is plain caller-owned state here — exactly
//! the rows an app's model would rebuild in its reducer when a node is
//! expanded or collapsed (expanding is "splice this node's children into the
//! list"). [`Tree`] only ever *reads* that list and the `selected` index: it
//! renders a selected, partially-expanded tree but does not own the tree, the
//! expansion, or the cursor (those are the reducer's, exactly like
//! [`List`]'s `selected`/`offset`). The surrounding [`Block`] owns the frame.
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test of the tree layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example tree_demo
//! ```

use rstui_core::{Color, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Tree, TreeGuides, TreeItem};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(28, 10)).expect("TestBackend is infallible");

    // The flattened visible rows an app's model would own: `src/` and
    // `src/widgets/` expanded, `src/bin/` collapsed. The reducer rebuilds
    // this `Vec` on every expand/collapse; the widget just projects it.
    let rows = [
        TreeItem::new(0, "src").expandable(true),
        TreeItem::new(1, "main.rs"),
        TreeItem::new(1, "widgets").expandable(true),
        TreeItem::new(2, "list.rs"),
        TreeItem::new(2, "tree.rs"),
        TreeItem::new(1, "bin").expandable(false),
        TreeItem::new(0, "Cargo.toml"),
        TreeItem::new(0, "README.md"),
    ];
    // Which row the keyboard is aimed at — just app state, like `List`.
    let selected = 4usize;

    terminal
        .draw(|frame| {
            frame.render_widget(
                Tree::new(rows)
                    .block(Block::bordered().title("project"))
                    .guides(TreeGuides::Lines)
                    .guide_style(Style::new().fg(Color::DarkGray))
                    .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .selected(Some(selected)),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
