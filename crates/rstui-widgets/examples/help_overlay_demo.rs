//! Exercises [`HelpOverlay`] the way a `?`-summoned cheat-sheet does: a
//! centred, opaque keybinding panel floated over busy background content.
//!
//! Whether the cheat-sheet is shown is a plain caller-owned flag an app's
//! model would hold (toggled on `?`/`Esc` in the reducer); [`HelpOverlay`]
//! only ever renders it. The panel is opaque (it clears its cells, the
//! [`Modal`](rstui_widgets::Modal) technique) so the background cannot bleed
//! through. Running over a [`TestBackend`] keeps it TTY-free, so it doubles as
//! a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example help_overlay_demo
//! ```

use rstui_core::{Color, Constraint, Style, Terminal, TestBackend};
use rstui_widgets::{Block, HelpEntry, HelpOverlay, Paragraph, Wrap};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("TestBackend is infallible");

    // The caller-owned cheat-sheet rows an app's model would hold.
    let entries = [
        HelpEntry::new(["Ctrl", "S"], "Save the current buffer"),
        HelpEntry::new(["Ctrl", "P"], "Open the command palette"),
        HelpEntry::new(["g", "g"], "Jump to the top"),
        HelpEntry::new(["Esc"], "Close this help"),
    ];

    terminal
        .draw(|frame| {
            // Busy background the opaque overlay must not let bleed through.
            let editor = Paragraph::new(
                "fn main() {\n    let app = App::new();\n    app.run();\n}\n\n\
                 // … the editor buffer scrolls on underneath, proving the \
                 cheat-sheet clears its own region before drawing.",
            )
            .wrap(Wrap { trim: true })
            .style(Style::new().fg(Color::DarkGray))
            .block(Block::bordered().title("src/main.rs"));
            frame.render_widget(editor, frame.area());

            frame.render_widget(
                HelpOverlay::new(&entries)
                    .width(Constraint::Length(34))
                    .height(Constraint::Length(6))
                    .separator("+")
                    .block(Block::bordered().title("Keybindings"))
                    .key_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .backdrop_style(Style::new().bg(Color::Black)),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
