//! Renders a markdown document with links, then shows the
//! [`Link`](rstui_widgets::Link) activation model: the document exposes its
//! links in reading order via [`Markdown::links`], the app owns a focused
//! index, and the reducer turns that into a
//! [`LinkActivation`](rstui_widgets::LinkActivation) — no callback smuggled
//! into the widget.
//!
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic smoke test of the link layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example markdown_links_demo
//! ```

use rstui_core::{Terminal, TestBackend};
use rstui_widgets::{Block, Markdown};

const DOC: &str = "\
# Project Links

See the [getting started](https://example.com/start) guide, the
[API reference](https://example.com/api), or open an
<https://github.com/andymac4182/rstui/issues> ticket.

- File a [bug report](https://example.com/bug)
- Mail the [maintainers](mailto:team@example.com)";

fn main() {
    let doc = Markdown::new(DOC);

    // The registry the host drives focus/activation from.
    let links = doc.links();
    println!("links in reading order ({}):", links.len());
    for (i, l) in links.iter().enumerate() {
        println!("  [{i}] {:<16} -> {}", l.label, l.href);
    }

    // App owns the focused index; the reducer activates that entry.
    let focused = 1;
    let event = links[focused].activate(focused);
    println!(
        "\nactivate(focused = {focused}) => index {} open {}",
        event.index, event.href
    );

    let mut terminal = Terminal::new(TestBackend::new(56, 12)).expect("TestBackend is infallible");
    terminal
        .draw(|frame| {
            frame.render_widget(
                Markdown::new(DOC).block(Block::bordered().title("links")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    println!();
    print!("{}", terminal.backend());
}
