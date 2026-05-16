//! Exercises [`ScrollView`] the way a real chat transcript / log pane will:
//! the caller renders its *full*, oversized content into its own off-screen
//! [`Buffer`] once, then `ScrollView` clips the caller-owned
//! `(col, row)`-offset window onto the screen and draws a [`Scrollbar`] on
//! each overflowing axis.
//!
//! The scroll offset is a plain `u16` here — exactly as it would be a field of
//! an app's model that the reducer grows/shrinks on a wheel/key event.
//! [`ScrollView`] only ever *reads* it and the borrowed content buffer; it
//! owns neither. Running over a [`TestBackend`] keeps it TTY-free, so it
//! doubles as a deterministic snapshot smoke test of the clip:
//!
//! ```text
//! cargo run -p rstui-widgets --example scroll_view_demo
//! ```

use rstui_core::{Buffer, Color, Rect, Style, Terminal, TestBackend, Widget};
use rstui_widgets::{Block, Paragraph, ScrollView};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(44, 12)).expect("TestBackend is infallible");

    // The caller renders its full, oversized content into its own buffer once
    // (a 60×40 scrollback the 44×12 screen can only show a window of).
    let log = (0..40)
        .map(|i| format!("line {i:02}  an event recorded in the scrollback log"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut content = Buffer::empty(Rect::new(0, 0, 60, 40));
    Paragraph::new(log).render(content.area(), &mut content);

    // Caller-owned scroll offset: the reducer would mutate this on ▲▼/wheel.
    let row_offset = 14u16;

    terminal
        .draw(|frame| {
            let view = ScrollView::new(&content)
                .offset(0, row_offset)
                .block(Block::bordered().title("scrollback  (▲▼ to scroll)"))
                .thumb_style(Style::new().fg(Color::Cyan));
            frame.render_widget(view, frame.area());
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
