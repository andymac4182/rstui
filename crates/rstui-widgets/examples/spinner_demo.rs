//! Exercises [`Spinner`] the way a real app shows "work is happening": a
//! single animated cell composed beside a label with a [`Layout`] split, the
//! spinner's `tick` being the same `frame.count()` animation clock the
//! `Terminal` driver has exposed since it landed (here a `Spinner` is its
//! first consumer).
//!
//! `tick` is plain caller-owned state — `frame.count()`, or a model field a
//! `Cmd` advances — and [`Spinner`] only ever reads it (the same pure
//! projection [`List`]/[`Gauge`]/[`Scrollbar`] use). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test of the spinner layer, including the tick→frame
//! projection across a built-in set:
//!
//! ```text
//! cargo run -p rstui-widgets --example spinner_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Paragraph, Spinner};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(28, 8)).expect("TestBackend is infallible");

    // The animation index an app would own: here the deterministic
    // `frame.count()` clock (it is 0 on the first and only frame), but it
    // could equally be a model field a periodic `Cmd` advances.
    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("Tasks");
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            let tick = frame.count();
            let [busy, strip] =
                Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

            // The real-app shape: a one-cell spinner beside a label, split off
            // with a Layout so the widget stays exactly its one
            // responsibility and the text is ordinary composed content.
            let [glyph, label] =
                Layout::horizontal([Constraint::Length(2), Constraint::Fill(1)]).areas(busy);
            frame.render_widget(
                Spinner::new()
                    .tick(tick)
                    .style(Style::new().fg(Color::Cyan)),
                glyph,
            );
            frame.render_widget(Paragraph::new("Working…"), label);

            // A filmstrip proving the pure tick→glyph projection: the LINE
            // set advanced one tick per column, deterministic and TTY-free.
            for (i, cell) in Layout::horizontal([Constraint::Length(1); 8])
                .split(strip)
                .iter()
                .enumerate()
            {
                frame.render_widget(Spinner::new().frames(Spinner::LINE).tick(i), *cell);
            }
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
