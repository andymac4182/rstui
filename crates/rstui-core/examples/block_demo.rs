//! Composes [`Layout`], [`Block`], and [`Frame::render_widget`] the way a real
//! view will: split the screen, frame each region in a titled [`Block`], then
//! render content into the area the block leaves behind ([`Block::inner`]).
//!
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic smoke test of the widget layer:
//!
//! ```text
//! cargo run -p rstui-core --example block_demo
//! ```

use rstui_core::{
    Alignment, Block, BorderType, Color, Constraint, Layout, Modifier, Padding, Style, Terminal,
    TestBackend,
};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(34, 9)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            // A sidebar reserved at a fixed width, the rest for the body.
            let [sidebar, body] = Layout::horizontal([Constraint::Length(12), Constraint::Min(0)])
                .areas(frame.area());

            let sidebar_block = Block::bordered()
                .border_type(BorderType::Rounded)
                .title("Menu")
                .title_style(Style::new().add_modifier(Modifier::BOLD));
            let sidebar_inner = sidebar_block.inner(sidebar);
            frame.render_widget(sidebar_block, sidebar);

            for (i, item) in ["Home", "Logs", "About"].iter().enumerate() {
                let row = Layout::vertical([Constraint::Length(1); 3]).split(sidebar_inner)[i];
                frame.render_widget(*item, row);
            }

            let body_block = Block::bordered()
                .border_type(BorderType::Double)
                .border_style(Style::new().fg(Color::Cyan))
                .title("Body")
                .title_alignment(Alignment::Center)
                .padding(Padding::symmetric(1, 0));
            let body_inner = body_block.inner(body);
            frame.render_widget(body_block, body);
            frame.render_widget("rstui widgets compose.", body_inner);
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
