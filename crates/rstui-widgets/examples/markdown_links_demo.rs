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

use rstui_core::{Position, Rect, Terminal, TestBackend};
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

    // Mouse half: a click position resolves to a link via deterministic
    // geometry, then the same activation path.
    let area = Rect::new(0, 0, 56, 12);
    let doc = Markdown::new(DOC).block(Block::bordered().title("links"));
    if let Some(first) = doc.link_regions(area).first() {
        let click = Position::new(first.rect.x, first.rect.y);
        let hit = doc
            .link_at(click, area)
            .and_then(|i| links.get(i).map(|l| l.activate(i)));
        println!(
            "click {:?} => {}",
            (click.x, click.y),
            hit.map(|e| e.href).unwrap_or_else(|| "(miss)".into())
        );
    }

    let mut terminal = Terminal::new(TestBackend::new(56, 12)).expect("TestBackend is infallible");
    terminal
        .draw(|frame| {
            // The focused link renders with the selection style — the visual
            // half of the keyboard focus loop a host drives over `links()`.
            frame.render_widget(
                Markdown::new(DOC)
                    .focused_link(focused)
                    .block(Block::bordered().title("links")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    println!();
    print!("{}", terminal.backend());
}
