//! Exercises [`Accordion`] the way a real settings/inspector pane will: a
//! framed stack of titled sections, some open and some collapsed, each
//! reserving a body the caller fills.
//!
//! Every section's `expanded` flag and `body_height` are plain values here —
//! exactly as they would be fields of an app's model the reducer flips when
//! the user toggles a header. [`Accordion`] only ever *reads* them, draws the
//! ▾/▸ headers itself, and hands back the open sections' body rects via
//! [`Accordion::layout`]. Running over a [`TestBackend`] keeps it TTY-free, so
//! it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example accordion_demo
//! ```

use rstui_core::{Color, Style, Terminal, TestBackend};
use rstui_widgets::{Accordion, AccordionSection, Block, List, Paragraph};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(34, 12)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            // Caller-owned expansion state: section 0 and 2 are open here.
            let acc = Accordion::new([
                AccordionSection::new("Appearance")
                    .expanded(true)
                    .body_height(2),
                AccordionSection::new("Keybindings"),
                AccordionSection::new("Advanced")
                    .expanded(true)
                    .body_height(3),
            ])
            .block(Block::bordered().title("Settings"))
            .header_style(Style::new().fg(Color::Black).bg(Color::Cyan));

            // Pure layout: the widget draws the headers, the caller fills the
            // bodies it reserved.
            let bodies = acc.layout(frame.area());
            frame.render_widget(acc, frame.area());

            if let Some(Some(rect)) = bodies.first() {
                frame.render_widget(
                    Paragraph::new("Theme: dark\nFont:  14pt")
                        .style(Style::new().fg(Color::DarkGray)),
                    *rect,
                );
            }
            if let Some(Some(rect)) = bodies.get(2) {
                frame.render_widget(
                    List::new(["Telemetry: off", "Beta:      on", "Logs:      verbose"])
                        .style(Style::new().fg(Color::DarkGray)),
                    *rect,
                );
            }
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
