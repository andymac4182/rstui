//! Exercises [`FpsCounter`] the way a real app shows render performance: a
//! caller-owned [`FpsMeter`] in the model, projected by a one-line widget in
//! a status corner. The meter is plain caller-owned state (ADR 0012 §P1) and
//! the widget only samples + reads it — the same pure projection
//! [`Spinner`]/[`Gauge`] use.
//!
//! Run over a [`TestBackend`] it is TTY-free and **deterministic**: a single
//! frame has no real cadence, so the meter reports its stable `"--- fps"`
//! placeholder rather than a wall-clock number — which is exactly why this
//! doubles as a snapshot smoke test. Live, the same code shows the real rate.
//!
//! ```text
//! cargo run -p rstui-widgets --example fps_counter_demo
//! ```

use rstui_core::{Color, Modifier, Style, Terminal, TestBackend, Widget};
use rstui_widgets::{Block, FpsCounter, FpsMeter};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 5)).expect("TestBackend is infallible");

    // The meter an app owns on its model. A real app calls nothing extra —
    // `FpsCounter` samples it once per frame as it renders.
    let meter = FpsMeter::new();

    terminal
        .draw(|frame| {
            let block = Block::bordered().title("Render performance");
            let inner = block.inner(frame.area());
            block.render(frame.area(), frame.buffer_mut());

            // Drop-in: one widget, top-right of the status area.
            let w = 12u16.min(inner.width);
            let corner =
                rstui_core::Rect::new(inner.x + inner.width.saturating_sub(w), inner.y, w, 1);
            FpsCounter::new(&meter)
                .prefix("⟳ ")
                .style(Style::new().fg(Color::Green).add_modifier(Modifier::BOLD))
                .render(corner, frame.buffer_mut());
        })
        .expect("draw is infallible on TestBackend");

    let frame = terminal.backend().to_string();
    println!("{frame}");

    // Doubles as a deterministic snapshot test: a single TestBackend frame
    // has no real cadence, so the readout is the stable placeholder.
    assert!(
        frame.contains("⟳ --- fps"),
        "expected the deterministic placeholder, got:\n{frame}"
    );
}
