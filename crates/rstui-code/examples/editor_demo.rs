//! Exercises [`Editor`] the way a real code/notes pane will: a framed
//! multi-line [`TextArea`] with a focused 2D caret, scrolled on both axes so
//! the visible window is a sub-rectangle of a larger document.
//!
//! The [`TextArea`] and the `(row, col)` scroll offset are plain caller-owned
//! state here — exactly the model fields an app would hold and a reducer
//! would mutate on key events (`insert_char`/`insert_newline`/`move_*` for
//! the text, a `scroll_into_view`-style adjustment for the offset).
//! [`Editor`] only ever *reads* them: it renders a focused panel and a caret
//! but does not decide *which* panel is focused or how it scrolls (those are
//! separate reducer concerns). Running over a [`TestBackend`] keeps it
//! TTY-free, so it doubles as a deterministic snapshot smoke test of the
//! editor layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example editor_demo
//! ```

use rstui_code::Editor;
use rstui_core::{Color, Style, Terminal, TestBackend, TextArea};
use rstui_widgets::Block;

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(28, 7)).expect("TestBackend is infallible");

    // The pane's state an app's model would own: the document and where the
    // viewport is scrolled. The cursor lands at (row 2, col 7) — "the" —
    // which is inside the scrolled window, so a reversed caret is drawn.
    let mut document = TextArea::from_value(
        "fn main() {\n    let mut x = 0;\n    while the loop runs {\n        x += 1;\n    }\n}",
    );
    document.set_cursor(2, 7);

    // Scrolled down one row and right four columns: the visible window is a
    // sub-rectangle of the document, the caller-owned 2D-offset model.
    let scroll = (1, 4);

    terminal
        .draw(|frame| {
            let editor = Editor::new(&document)
                .focused(true)
                .scroll(scroll)
                .block(Block::bordered().title("source"))
                .focus_style(Style::new().bg(Color::Black))
                .cursor_style(Style::new().fg(Color::Black).bg(Color::Cyan));
            frame.render_widget(editor, frame.area());
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
