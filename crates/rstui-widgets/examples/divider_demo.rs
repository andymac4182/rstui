//! Exercises [`Divider`] as a section separator: a captioned horizontal rule
//! between two stacked panes and a vertical rule splitting a row, all matching
//! the surrounding [`Block`] glyphs.
//!
//! A divider owns no state — orientation, caption, and style are caller-owned
//! (the pure-projection leaf shape [`StatusBar`] uses). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example divider_demo
//! ```

use rstui_core::{Alignment, Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Divider, DividerOrientation};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(36, 7)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("settings");
            let area = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            let [top, rule, bottom] = Layout::vertical([
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .areas(area);

            frame.render_widget(Block::new().title("General"), top);

            // A captioned horizontal section break.
            frame.render_widget(
                Divider::new()
                    .label("Advanced")
                    .label_alignment(Alignment::Left)
                    .style(Style::new().fg(Color::DarkGray))
                    .label_style(Style::new().fg(Color::Yellow)),
                rule,
            );

            // The lower pane split by a vertical rule.
            let [left, bar, right] = Layout::horizontal([
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .areas(bottom);
            frame.render_widget(Block::new().title("keys"), left);
            frame.render_widget(
                Divider::new()
                    .orientation(DividerOrientation::Vertical)
                    .style(Style::new().fg(Color::DarkGray)),
                bar,
            );
            frame.render_widget(Block::new().title("theme"), right);
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
