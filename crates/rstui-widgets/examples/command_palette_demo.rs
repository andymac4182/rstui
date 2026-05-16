//! Exercises [`CommandPalette`] the way a real editor will: a focused fuzzy
//! palette — a query row above the matched results — centred **opaquely** over
//! the working content.
//!
//! `query`, `results`, and `highlight` are plain caller-owned state here. The
//! reducer recomputes `results` from the query on every keystroke and runs the
//! highlighted command on `Enter`; [`CommandPalette`] only ever *reads* them —
//! **it does not filter** (no matching algorithm is smuggled into the pure
//! `view`). It is the worked example of third-party *composition*: it owns no
//! glyph-stamping, assembling [`Input`](rstui_widgets::Input) +
//! [`List`](rstui_widgets::List) + [`Block`](rstui_widgets::Block) +
//! `clear_region` with the [`Modal`](rstui_widgets::Modal) centring math.
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test of the palette layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example command_palette_demo
//! ```

use rstui_core::{Color, Constraint, Line, Style, Terminal, TestBackend, TextEdit};
use rstui_widgets::{Block, CommandPalette, Paragraph};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(48, 14)).expect("TestBackend is infallible");

    // The palette state an app's model would own: the query the user typed,
    // the results the reducer matched from it, and the keyboard row.
    let query = TextEdit::from_value("op");
    let results = [
        Line::raw("Open File…"),
        Line::raw("Open Recent"),
        Line::raw("Open Folder…"),
        Line::raw("Reopen Closed Editor"),
    ];
    let highlight = 1usize; // the arrows moved the keyboard to "Open Recent"

    terminal
        .draw(|frame| {
            // The working content the palette floats over — opaque, so these
            // glyphs cannot bleed through the centred panel.
            let document = Paragraph::new("fn main() {\n    println!(\"hello\");\n}\n".repeat(5))
                .block(Block::bordered().title("main.rs"));
            frame.render_widget(document, frame.area());

            frame.render_widget(
                CommandPalette::new(&query, &results)
                    .highlight(highlight)
                    .focused(true)
                    .width(Constraint::Percentage(70))
                    .height(Constraint::Length(7))
                    .block(Block::bordered().title("Command Palette"))
                    .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .backdrop_style(Style::new().fg(Color::DarkGray)),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
