//! Exercises [`Card`] the way a real dashboard will: two titled tiles laid
//! out side by side, each a framed box with a header line, a caller-filled
//! body, and a footer line.
//!
//! `Card` is a thin composition over [`Block`] — it owns the frame and adds
//! the header/footer rows, exposing [`Card::inner`] as the body exactly the
//! [`Block::inner`] contract. The body content is the caller's, rendered into
//! that rect. Running over a [`TestBackend`] keeps it TTY-free, so it doubles
//! as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example card_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Card, Paragraph};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(44, 7)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(frame.area());

            let build = Card::new()
                .title("build")
                .header("status: passing")
                .footer("[r] rerun")
                .header_style(Style::new().fg(Color::Black).bg(Color::Green));
            let build_body = build.inner(left);
            frame.render_widget(build, left);
            frame.render_widget(
                Paragraph::new("42 tests\n0 failed").style(Style::new().fg(Color::DarkGray)),
                build_body,
            );

            let deploy = Card::new()
                .block(Block::bordered().title("deploy"))
                .header("status: blocked")
                .footer("[d] deploy")
                .header_style(Style::new().fg(Color::Black).bg(Color::Red));
            let deploy_body = deploy.inner(right);
            frame.render_widget(deploy, right);
            frame.render_widget(
                Paragraph::new("waiting on\nbuild").style(Style::new().fg(Color::DarkGray)),
                deploy_body,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
