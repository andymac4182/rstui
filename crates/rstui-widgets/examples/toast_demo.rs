//! Exercises [`Toast`] the way a real editor will: a stack of transient
//! notifications floated over the working content, newest in the top-right
//! corner, one accent colour per [`ToastLevel`].
//!
//! The notification list is plain caller-owned state here — exactly the
//! `Vec<ToastMessage>` an app's model would hold, that a reducer would
//! `insert(0, …)` onto and (on a timer message) trim. [`Toast`] only ever
//! *reads* it: it projects the current list and nothing else — *when* a toast
//! expires or is dismissed is a separate, deliberately deferred reducer
//! concern, never a clock in the pure `view`. Drawing it over a filled
//! [`Paragraph`] background shows the box is **opaque** (it `clear_region`s
//! itself, the same affordance [`Modal`](rstui_widgets::Modal) uses), and
//! running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test of the toast layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example toast_demo
//! ```

use rstui_core::{Color, Constraint, Style, Terminal, TestBackend};
use rstui_widgets::{Block, BorderType, Paragraph, Toast, ToastLevel, ToastMessage, Wrap};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(54, 13)).expect("TestBackend is infallible");

    // The notification queue an app's model would own: newest first. A
    // reducer pushes to the front and expires from the back; the widget
    // never reorders or times anything out.
    let toasts = [
        ToastMessage::new(ToastLevel::Error, "Build failed: 3 errors in rstui-core"),
        ToastMessage::new(ToastLevel::Warning, "Unsaved changes in 2 files"),
        ToastMessage::new(ToastLevel::Success, "All 128 tests passed"),
        ToastMessage::new(ToastLevel::Info, "Reconnected to the language server"),
    ];

    terminal
        .draw(|frame| {
            // The working content the toasts float over — its glyphs must
            // not bleed through the opaque boxes.
            let editor = Paragraph::new(
                "fn main() {\n    let app = App::new();\n    app.run();\n}\n\n\
                 // … the rest of the editor buffer scrolls on underneath, \
                 proving each toast box clears its own region before drawing.",
            )
            .wrap(Wrap { trim: true })
            .style(Style::new().fg(Color::DarkGray))
            .block(Block::bordered().title("src/main.rs"));
            frame.render_widget(editor, frame.area());

            // The stack: newest (the error) flush to the top-right corner,
            // older toasts stacking downward, each framed and accent-tinted.
            frame.render_widget(
                Toast::new(&toasts)
                    .width(Constraint::Length(34))
                    .gap(1)
                    .max_visible(5)
                    .block(Block::bordered().border_type(BorderType::Rounded))
                    .style(Style::new().bg(Color::Black))
                    .info_style(Style::new().fg(Color::Cyan))
                    .success_style(Style::new().fg(Color::Green))
                    .warning_style(Style::new().fg(Color::Yellow))
                    .error_style(Style::new().fg(Color::Red)),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
