//! Composes [`Span`], [`Line`], and [`Text`] inside a [`Block`] the way a real
//! view will: styled runs on a line, a right-aligned line, and a multi-line
//! block whose text-level style cascades into its lines.
//!
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test of the text layer:
//!
//! ```text
//! cargo run -p rstui-core --example text_demo
//! ```

use rstui_core::{
    Block, Color, Constraint, Layout, Line, Modifier, Span, Style, Terminal, TestBackend, Text,
};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(34, 7)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let block = Block::bordered().title("text");
            let inner = block.inner(frame.area());
            frame.render_widget(block, frame.area());

            let [status, gap, body] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .areas(inner);

            // A line of differently-styled runs.
            frame.render_widget(
                Line::from(vec![
                    Span::styled(" OK ", Style::new().fg(Color::Green)),
                    Span::raw(" build "),
                    Span::styled("passed", Style::new().add_modifier(Modifier::BOLD)),
                ]),
                status,
            );

            // A right-aligned line in the same row band.
            frame.render_widget(
                Line::raw("rstui")
                    .right_aligned()
                    .style(Style::new().fg(Color::DarkGray)),
                gap,
            );

            // A multi-line block: the text-level cyan cascades into every line.
            frame.render_widget(
                Text::raw("Span → Line → Text.\nOne committed model.\nStyles cascade by patch.")
                    .style(Style::new().fg(Color::Cyan)),
                body,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
