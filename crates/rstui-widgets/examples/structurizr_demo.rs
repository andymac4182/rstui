//! A Structurizr DSL workspace rendered as its C4 model views through the
//! [`Structurizr`] widget.
//!
//! One read-only widget parses the [Structurizr
//! DSL](https://docs.structurizr.com/dsl/language) — `workspace` / `model`
//! (`person`, `softwareSystem`, nested `container`/`component`,
//! relationships) / `views` (`systemContext`, `container`, …) — and lays out
//! the selected view as a **deterministic** C4 diagram: stereotyped element
//! cards, a dashed boundary box around a system's containers, and labelled
//! relationship arrows. Running over a [`TestBackend`] keeps it TTY-free, so
//! the hero render doubles as a snapshot smoke:
//!
//! ```text
//! cargo run -p rstui-widgets --example structurizr_demo
//! ```

use rstui_core::{Buffer, Position, Rect, Terminal, TestBackend, Widget};
use rstui_widgets::{Block, Structurizr};

/// A realistic "Internet Banking" workspace exercising the parsed subset:
/// people, a software system with nested containers, an external system,
/// relationships with technology, and both a System Context and a Container
/// view.
const WORKSPACE: &str = r#"workspace "Big Bank plc" "Internet banking." {
  model {
    customer = person "Personal Customer" "A customer of the bank."
    banking = softwareSystem "Internet Banking System" "Lets customers view accounts." {
      web = container "Web Application" "Delivers the static content and SPA." "Java/Spring"
      spa = container "Single-Page App" "The banking UI." "JavaScript/React"
      api = container "API Application" "Banking logic over a JSON API." "Java/Spring"
      db = container "Database" "Stores accounts, transactions." "Oracle"
    }
    mainframe = softwareSystem "Mainframe Banking" "The core banking system." "Existing System,External"
    email = softwareSystem "E-mail System" "Microsoft Exchange." "External"

    customer -> banking "Views accounts and makes payments using"
    customer -> web "Visits bigbank.com using" "HTTPS"
    web -> spa "Delivers"
    spa -> api "Makes API calls to" "JSON/HTTPS"
    api -> db "Reads from and writes to" "JDBC"
    api -> mainframe "Makes API calls to" "XML/HTTPS"
    api -> email "Sends e-mail using"
    email -> customer "Sends e-mails to"
  }
  views {
    systemContext banking "Context" "The system context for Internet Banking." {
      include *
      autolayout tb
    }
    container banking "Containers" "The containers of the Internet Banking System." {
      include *
      autolayout tb
    }
  }
}"#;

/// Render `view` of the workspace into a fresh `w`×`h` [`Buffer`]; return the
/// glyph rows joined by `\n` — the same headless snapshot the widget's own
/// tests use, so this doubles as a determinism check.
fn frame(view: usize, w: u16, h: u16) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
    Structurizr::new(WORKSPACE)
        .view(view)
        .render(buf.area(), &mut buf);
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push(buf.get(Position::new(x, y)).unwrap().symbol);
        }
        out.push('\n');
    }
    out
}

fn main() {
    // Smoke FIRST (scrolls off): the workspace parses to the expected C4
    // model, and every view renders deterministically (rendered twice,
    // byte-identical) — asserted, so a regression panics this example.
    let ws = Structurizr::parse(WORKSPACE).expect("workspace parses");
    assert_eq!(
        ws.elements.len(),
        8,
        "customer + banking + 4 containers + 2 ext"
    );
    assert_eq!(ws.views.len(), 2, "systemContext + container");
    assert!(ws.relationships.len() >= 8, "relationships resolved");
    for v in 0..ws.views.len() {
        assert_eq!(frame(v, 80, 30), frame(v, 80, 30), "view {v} deterministic");
    }
    println!(
        "✓ Structurizr DSL → C4: {} elements, {} relationships, {} views; all deterministic\n",
        ws.elements.len(),
        ws.relationships.len(),
        ws.views.len()
    );

    // Hero LAST (the frame the GIF freezes on): the Container view — a
    // dashed C4 boundary around the system's containers, externals outside,
    // labelled relationship arrows — framed in a Block.
    let mut term = Terminal::new(TestBackend::new(96, 40)).expect("TestBackend is infallible");
    term.draw(|f| {
        f.render_widget(
            Structurizr::new(WORKSPACE)
                .view(1)
                .block(Block::bordered().title("structurizr · C4 Container view")),
            f.area(),
        );
    })
    .expect("TestBackend is infallible");
    print!("{}", term.backend());
}
