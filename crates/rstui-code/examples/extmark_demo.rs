//! Exercises [`Extmark`] the way a real chat composer will: a focused
//! [`Editor`] whose text contains an `@mention` and a pasted-file reference
//! the reducer has marked as **atomic pills**, plus a single-line [`Input`]
//! with the same pill model.
//!
//! The [`TextArea`]/[`TextEdit`] *and* the `[Extmark]` list are plain
//! caller-owned model state here — exactly the fields an app would hold and a
//! reducer would re-derive on every keystroke (insert before a pill ⇒ shift
//! its range; the widget never mutates it). [`Editor`]/[`Input`] only ever
//! *read* them: they paint the styled spans, beneath the focus fill and the
//! caret. Running over a [`TestBackend`] keeps it TTY-free, so it doubles as
//! a deterministic snapshot smoke test of the extmark projection:
//!
//! ```text
//! cargo run -p rstui-widgets --example extmark_demo
//! ```

use rstui_code::Editor;
use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend, TextArea, TextEdit};
use rstui_widgets::{Block, Extmark, Input};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("TestBackend is infallible");

    // The composer document an app's model would own.
    let document = TextArea::from_value("hey @ada, see report.pdf for the\nnumbers — thanks!");

    // Ranges the reducer re-derives on every edit. Flattened char indices into
    // "hey @ada, see report.pdf …": "@ada" is 4..8, "report.pdf" is 14..24.
    let doc_marks = [
        Extmark::pill(4..8, Style::new().fg(Color::Black).bg(Color::Cyan)),
        Extmark::pill(14..24, Style::new().fg(Color::Black).bg(Color::Magenta)),
    ];

    // A single-line input with one @mention pill.
    let field = TextEdit::from_value("to: @team");
    let field_marks = [Extmark::pill(
        4..9,
        Style::new().fg(Color::Black).bg(Color::Green),
    )];

    terminal
        .draw(|frame| {
            let rows = Layout::vertical([Constraint::Length(5), Constraint::Length(3)])
                .split(frame.area());

            let editor = Editor::new(&document)
                .focused(true)
                .extmarks(&doc_marks)
                .block(Block::bordered().title("composer"))
                .focus_style(Style::new().bg(Color::Black))
                .cursor_style(Style::new().fg(Color::Black).bg(Color::White));
            frame.render_widget(editor, rows[0]);

            let block = Block::bordered().title("recipients");
            let inner = block.inner(rows[1]);
            frame.render_widget(block, rows[1]);
            frame.render_widget(Input::new(&field).extmarks(&field_marks), inner);
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
