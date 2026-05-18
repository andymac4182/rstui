//! Exercises [`WhichKey`] the way a leader-armed app does: a small,
//! bottom-anchored, opaque `key → action` hint popup floated over busy
//! content while a prefix is held — the opencode / Helix / which-key.nvim
//! discoverability affordance.
//!
//! Whether a prefix is armed (and the continuation rows) is plain
//! caller-owned model an app's reducer holds — typically fed straight
//! from its keymap's "what can follow the armed leader" query. The widget
//! only ever renders it. Running over a [`TestBackend`] keeps it TTY-free,
//! so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example which_key_demo
//! ```

use std::borrow::Cow;

use rstui_core::{Color, Line, Style, Terminal, TestBackend};
use rstui_widgets::{Block, BorderType, Paragraph, WhichKey, Wrap};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(46, 14)).expect("TestBackend is infallible");

    // The caller-owned continuation rows — `(second key, action label)` —
    // an app builds from its keymap while the leader is armed.
    let rows: Vec<(Cow<'_, str>, Line<'_>)> = [
        ("P", "Command palette"),
        ("Q", "Quit"),
        ("S", "Settings"),
        ("W", "Move focus"),
        ("Y", "Copy selection"),
    ]
    .into_iter()
    .map(|(k, l)| (Cow::Borrowed(k), Line::from(l)))
    .collect();

    terminal
        .draw(|frame| {
            // Busy background the opaque popup must not let bleed through.
            let bg = Paragraph::new(
                "the app keeps rendering underneath; the which-key popup \
                 clears its own region and sits just above the footer.",
            )
            .wrap(Wrap { trim: true })
            .style(Style::new().fg(Color::DarkGray));
            frame.render_widget(bg, frame.area());

            frame.render_widget(
                WhichKey::new(&rows)
                    .block(
                        Block::bordered()
                            .border_type(BorderType::Rounded)
                            .title(" ⟨leader⟩ "),
                    )
                    .max_height(8)
                    .style(Style::new().bg(Color::Rgb(20, 22, 30)))
                    .key_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .label_style(Style::new().fg(Color::White)),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
