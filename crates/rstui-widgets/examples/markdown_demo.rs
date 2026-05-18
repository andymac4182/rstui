//! Renders a markdown document through [`Markdown`] inside a [`Block`],
//! exercising the **whole** supported surface end to end so it can be
//! eyeballed and doubles as a deterministic, TTY-free smoke test:
//!
//! - ATX **and** setext headings, soft-wrapped paragraph with inline
//!   **bold**/*italic*/`code`/escapes,
//! - a **fenced code block with a language** (dim caption + generic syntax
//!   highlight) and a 4-space **indented** code block,
//! - a **nested** block quote, a **tight** list, a **loose** list (blank
//!   line between items), an ordered list,
//! - a GFM **table** with per-column alignment,
//! - an `![image]()`, an inline `[link]()`, a **reference-style** link, an
//!   `<autolink>`,
//! - **HTML passthrough** (entities, a `<b>` tag, a comment, `<br>`), a
//!   thematic break, and
//! - **embedded diagrams** — with `.diagrams(true)` a fenced ` ```mermaid `
//!   and a fenced ` ```structurizr ` block render *inline as the diagram*
//!   (delegated to `Mermaid`/`Structurizr`), not as verbatim code.
//!
//! ```text
//! cargo run -p rstui-widgets --example markdown_demo
//! ```

use rstui_core::{Terminal, TestBackend};
use rstui_widgets::{Block, Markdown};

const DOC: &str = "\
# rstui Markdown

Setext Heading
==============

A **terminal-native** renderer with `inline code`, *emphasis*, an escaped
\\*star\\*, and soft word-wrap to the pane width.

```rust
fn render(area: Rect) -> usize {
    let n = 42; // a comment
    let s = \"a string\";
    /* block
       comment */
    return n;
}
```

    // indented code block: 4 spaces, kept verbatim, *not* italic

> Block quotes draw a rail
> > and can nest.

- tight one
- tight two
  - nested child

1. loose first

2. loose second (blank line above ⇒ spacer rows)

| Feature  | State |
| :------- | :---: |
| headings | done  |
| tables   | done  |

An ![inline image](logo.png), an [inline link](https://rust-lang.org), a
[reference link][rs], and an <https://github.com/andymac4182/rstui> autolink.

HTML &amp; entities (&copy; &mdash; &#x2713;), a <b>bold</b> tag, a
<!-- hidden --> comment, and a hard break<br>onto the next line.

[rs]: https://rust-lang.org

---

A fenced `mermaid` block renders *inline as the diagram*, not as code:

```mermaid
graph TD
  A[fence] --> B[Markdown]
  B --> C{diagram?}
  C -->|yes| D[render widget]
  C -->|no| E[code block]
```

…and a `structurizr` C4 model, auto-laid-out from structure:

```structurizr
workspace \"demo\" {
  model {
    u = person \"Reader\"
    s = softwareSystem \"Markdown\" \"Embeds diagrams inline\"
    u -> s \"Scrolls\"
  }
  views {
    systemContext s \"Ctx\" { include * }
  }
}
```

---";

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(60, 92)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            frame.render_widget(
                // `.diagrams(true)` rasterises the fenced ```mermaid /
                // ```structurizr blocks inline as their diagrams (opt-in;
                // a non-diagram fence stays verbatim code).
                Markdown::new(DOC)
                    .diagrams(true)
                    .block(Block::bordered().title("markdown")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
