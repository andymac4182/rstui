//! Exercises [`Tooltip`] the way a real form will: two **opaque** hint popups
//! anchored to their controls — one dropping below, one **flipped above**
//! because it would overflow the bottom of the screen.
//!
//! The anchor rects are plain caller-owned state here — the rects of the
//! hovered/focused controls. *Whether* a tip is shown is the reducer's job;
//! [`Tooltip`] only ever *reads* the text + anchor and places itself with the
//! pure [`placement`](rstui_widgets::Tooltip::placement) accessor `render`
//! itself calls (the [`Select`](rstui_widgets::Select) flip pattern), reusing
//! [`Paragraph`](rstui_widgets::Paragraph) for the body. Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test of the tooltip layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example tooltip_demo
//! ```

use rstui_core::{Color, Rect, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Paragraph, Tooltip};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("TestBackend is infallible");

    // The two controls' rects an app's model would own (their focused rect).
    let top_field = Rect::new(2, 1, 14, 1);
    let bottom_field = Rect::new(2, 10, 14, 1);

    terminal
        .draw(|frame| {
            let form =
                Paragraph::new("Name:  [_____________]\n\n\n\n\n\n\n\n\nEmail: [_____________]")
                    .block(Block::bordered().title("Sign up"));
            frame.render_widget(form, frame.area());

            // `Widget::render` anchors off the area it is handed and uses the
            // whole buffer as the screen, so `render_widget(tip, anchor)` is
            // exactly "show this tip beside this control".

            // Anchored to the top field: there is room below, so it drops down.
            frame.render_widget(
                Tooltip::new("Your full legal name")
                    .block(Block::bordered())
                    .style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                top_field,
            );

            // Anchored to the bottom field: no room below, so it flips above.
            frame.render_widget(
                Tooltip::new("We never share this")
                    .block(Block::bordered())
                    .style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                bottom_field,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
