//! Exercises [`Select`] the way a real settings form will: a focused,
//! **open** dropdown anchored to its field, one option already committed and
//! another highlighted by the keyboard.
//!
//! `open`, `selected`, `highlight`, and `offset` are plain caller-owned state
//! here — exactly the fields an app's model would hold and a reducer would
//! move on `Enter`/`Esc`/arrows. [`Select`] only ever reads them: it renders
//! the open panel but does not decide *when* it is open or *which* row is
//! committed (that is the reducer's job). The panel is opaque (it clears its
//! cells, the [`Modal`](rstui_widgets::Modal) technique) so the framed form
//! behind it cannot bleed through — but it is anchored to the field, not
//! centred, so it is deliberately not a `Modal`. Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test of the select layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example select_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Select};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(28, 10)).expect("TestBackend is infallible");

    // The form's state an app's model would own: the theme options, which one
    // is committed, whether the dropdown is open, and the keyboard row in it.
    let themes = ["Solarized", "Dracula", "Gruvbox", "Nord", "Monokai"];
    let selected = Some(0usize); // committed choice shown in the closed field
    let open = true; // the reducer toggled it open on Enter
    let highlight = 2usize; // the arrows moved the keyboard to "Gruvbox"

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("Preferences");
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            // A label row, the one-row select field, then filler the open
            // panel drops over (anchored directly below the field). `Min(0)`
            // takes the leftover so the field stays exactly one row.
            let rows = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner);
            frame.render_widget("Theme:", rows[0]);
            frame.render_widget(
                Select::new(themes)
                    .selected(selected)
                    .open(open)
                    .highlight(highlight)
                    .focused(true)
                    .focus_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .block(Block::bordered())
                    .open_height(4),
                rows[1],
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
