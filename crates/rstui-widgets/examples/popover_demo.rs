//! Exercises [`Popover`] the way a real UI will: two **opaque** anchored
//! panels with caller-owned content — one dropping below its control, one
//! **flipped above** because it would overflow the bottom of the screen.
//!
//! The anchor rects are plain caller-owned state (the rects of the
//! hovered/focused controls); *whether* a popover is shown is the reducer's
//! job. [`Popover`] only ever reads the anchor + size and places itself with
//! the pure [`placement`](rstui_widgets::Popover::placement) accessor `render`
//! itself calls (the [`Tooltip`](rstui_widgets::Tooltip) flip, widened to four
//! sides); the caller draws its own content into
//! [`inner`](rstui_widgets::Popover::inner) — the `Modal` render-then-fill
//! pattern, anchored. Running over a [`TestBackend`] keeps it TTY-free, so it
//! doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example popover_demo
//! ```

use rstui_core::{Color, Rect, Style, Terminal, TestBackend};
use rstui_widgets::{Block, List, Paragraph, Popover};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("TestBackend is infallible");

    // The two controls' rects an app's model would own (their focused rect).
    let top_field = Rect::new(2, 1, 12, 1);
    let bottom_field = Rect::new(2, 10, 12, 1);

    terminal
        .draw(|frame| {
            let form = Paragraph::new("Name:  [__________]\n\n\n\n\n\n\n\n\nRole:  [__________]")
                .block(Block::bordered().title("Edit member"));
            frame.render_widget(form, frame.area());

            // Anchored to the top field: room below, so it drops down. The
            // caller owns the content — here a framed help paragraph.
            let help = Popover::new()
                .width(20)
                .height(4)
                .block(Block::bordered().title("Hint"))
                .style(Style::new().fg(Color::Black).bg(Color::Cyan));
            let help_inner = help.inner(top_field, frame.area());
            frame.render_widget(help, top_field);
            frame.render_widget(
                Paragraph::new("Your full legal name").style(Style::new().fg(Color::Black)),
                help_inner,
            );

            // Anchored to the bottom field: no room below, so it flips above.
            // Content here is a caller-owned option List.
            let menu = Popover::new()
                .width(14)
                .height(4)
                .block(Block::bordered())
                .style(Style::new().fg(Color::Black).bg(Color::Cyan));
            let menu_inner = menu.inner(bottom_field, frame.area());
            frame.render_widget(menu, bottom_field);
            frame.render_widget(
                List::new(["Owner", "Admin", "Guest"])
                    .selected(Some(1))
                    .highlight_style(Style::new().bg(Color::Blue).fg(Color::White)),
                menu_inner,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
