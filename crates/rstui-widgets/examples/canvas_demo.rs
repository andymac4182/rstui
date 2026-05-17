//! Exercises [`Canvas`] the way a dashboard does: a caller-owned series plotted
//! as a Braille line over a framed surface with axis labels.
//!
//! The series is plain caller-owned state — the [`paint`] closure only ever
//! reads it (the same pure projection [`Sparkline`]/[`List`] use). Running over
//! a [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example canvas_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::Block;
use rstui_widgets::canvas::{Canvas, CanvasLine, Marker, Points};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(60, 16)).expect("TestBackend is infallible");

    // The samples an app's model would own (a metrics ring buffer).
    let series: Vec<(f64, f64)> = (0..48)
        .map(|i| {
            let x = f64::from(i);
            (x, 20.0 + 18.0 * (x / 6.0).sin())
        })
        .collect();

    terminal
        .draw(|frame| {
            let [line_area, scatter_area] =
                Layout::horizontal([Constraint::Fill(1), Constraint::Length(24)])
                    .areas(frame.area());

            // A connected Braille line with axis labels printed at data coords.
            frame.render_widget(
                Canvas::default()
                    .block(Block::bordered().title("revenue (Braille)"))
                    .x_bounds([0.0, 47.0])
                    .y_bounds([0.0, 40.0])
                    .marker(Marker::Braille)
                    .background(Style::new())
                    .paint(|ctx| {
                        for w in series.windows(2) {
                            ctx.draw(&CanvasLine {
                                x1: w[0].0,
                                y1: w[0].1,
                                x2: w[1].0,
                                y2: w[1].1,
                                color: Color::Green,
                            });
                        }
                        ctx.print(0.0, 40.0, "40");
                        ctx.print(0.0, 0.0, "0");
                    }),
                line_area,
            );

            // The same data as a half-block scatter cloud.
            frame.render_widget(
                Canvas::default()
                    .block(Block::bordered().title("points"))
                    .x_bounds([0.0, 47.0])
                    .y_bounds([0.0, 40.0])
                    .marker(Marker::HalfBlock)
                    .paint(|ctx| {
                        ctx.draw(&Points {
                            coords: &series,
                            color: Color::Cyan,
                        });
                    }),
                scatter_area,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
