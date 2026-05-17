//! Exercises [`KeymapView`] the way a settings/rebind panel does: a framed,
//! opaque keybinding table with a selection cursor, a row armed for capture,
//! and a disabled row — floated over busy background content.
//!
//! The selection index, the capture state, and the live keymap are all
//! plain caller-owned model an app's reducer holds (the widget reports the
//! row under a click via [`KeymapView::hit`] and never decides anything).
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example keymap_view_demo
//! ```

use rstui_core::{Color, Style, Terminal, TestBackend};
use rstui_widgets::{Block, KeymapRow, KeymapView, Paragraph, RowState, Wrap};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(52, 14)).expect("TestBackend is infallible");

    // The caller-owned rows an app builds from its keymap each frame
    // (label = action help, keys = OS-aware caps, id = config key).
    let rows = [
        KeymapRow::new("Command palette", ["⌘", "K"])
            .id("app.palette")
            .state(RowState::Selected),
        KeymapRow::new("Toggle help", ["?"]).id("app.help"),
        KeymapRow::new("Settings drawer", ["g"])
            .id("app.drawer")
            .state(RowState::Capturing),
        KeymapRow::new("Copy selection", ["⌘", "C"]).id("edit.copy"),
        KeymapRow::new("Next keymap", ["—"])
            .id("app.cycle_keymap")
            .state(RowState::Disabled),
    ];

    terminal
        .draw(|frame| {
            // Busy background the opaque panel must not let bleed through.
            let bg = Paragraph::new(
                "the app keeps rendering underneath — the keymap panel clears \
                 its own region first, the Modal/HelpOverlay opacity idiom.",
            )
            .wrap(Wrap { trim: true })
            .style(Style::new().fg(Color::DarkGray));
            frame.render_widget(bg, frame.area());

            frame.render_widget(
                KeymapView::new(&rows)
                    .block(Block::bordered().title(" Keymap "))
                    .header("Vim · macOS · leader ⌃X")
                    .footer("● press a key to bind \"Settings drawer\" — Esc cancels")
                    .separator("")
                    .style(Style::new().bg(Color::Rgb(20, 22, 30)))
                    .label_style(Style::new().fg(Color::White))
                    .id_style(Style::new().fg(Color::DarkGray))
                    .key_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .selected_style(Style::new().bg(Color::Rgb(40, 48, 70)))
                    .capturing_style(Style::new().fg(Color::Yellow))
                    .disabled_style(Style::new().fg(Color::DarkGray))
                    .backdrop_style(Style::new().bg(Color::Black)),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
