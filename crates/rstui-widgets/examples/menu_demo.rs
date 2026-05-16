//! Exercises [`Menu`] the way a real editor will: an **opaque** context menu
//! (key hints, a separator, a disabled row) floated over working content,
//! one row highlighted by the keyboard.
//!
//! `highlight` is plain caller-owned state here — exactly the index an app's
//! model would hold and a reducer would move on the arrows (skipping the
//! separator and the disabled row) and *commit as an action* on `Enter`.
//! [`Menu`] only ever reads it: it reuses [`List`](rstui_widgets::List) for
//! the column and clears its cells (the [`Modal`](rstui_widgets::Modal)
//! technique) so the framed document behind it cannot bleed through — but it
//! is not a `Select` (it commits an action, no closed field). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test of the menu layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example menu_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Rect, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Menu, MenuItem, Paragraph};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("TestBackend is infallible");

    // The menu's rows an app's model would own, and the keyboard row in it.
    let items = [
        MenuItem::new("Cut").key_hint("Ctrl+X"),
        MenuItem::new("Copy").key_hint("Ctrl+C"),
        MenuItem::new("Paste").key_hint("Ctrl+V").disabled(true),
        MenuItem::separator(),
        MenuItem::new("Select All").key_hint("Ctrl+A"),
    ];
    let highlight = 1usize; // the arrows moved the keyboard to "Copy"

    terminal
        .draw(|frame| {
            // The working document the menu floats over — its glyphs must not
            // bleed through the opaque menu box.
            let document =
                Paragraph::new("the quick brown fox\njumps over the lazy dog\n".repeat(6))
                    .block(Block::bordered().title("editor.rs"));
            frame.render_widget(document, frame.area());

            // The context menu, anchored where the cursor was right-clicked.
            let [_, menu_col] = Layout::horizontal([Constraint::Length(14), Constraint::Min(0)])
                .areas(frame.area());
            let [_, menu_row] =
                Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(menu_col);
            let menu_rect = Rect::new(menu_row.x, menu_row.y, 22, 7);

            frame.render_widget(
                Menu::new(&items)
                    .highlight(highlight)
                    .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .disabled_style(Style::new().fg(Color::DarkGray))
                    .block(Block::bordered().title("Edit")),
                menu_rect,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
