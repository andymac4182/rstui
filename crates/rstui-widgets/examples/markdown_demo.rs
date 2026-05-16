//! Renders a real markdown document through [`Markdown`] inside a [`Block`]:
//! a heading, a soft-wrapped paragraph with inline emphasis/code, a fenced
//! code block, a block quote, and a nested bullet list — the supported subset
//! exercised end to end.
//!
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test of the markdown layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example markdown_demo
//! ```

use rstui_core::{Terminal, TestBackend};
use rstui_widgets::{Block, Markdown};

const DOC: &str = "\
# rstui Markdown

A **terminal-native** renderer with `inline code`, *emphasis*, and soft
word-wrap to the pane width.

```
fn render(area: Rect) { /* verbatim, never reflowed */ }
```

> Block quotes draw a rail and can nest.

- bullet one
- bullet two
  - nested child
1. ordered item
2. and another

| Feature  | State |
| :------- | :---: |
| headings | done  |
| tables   | done  |

HTML &amp; entities work, and a <b>bold</b> tag too.<br>Line after a break.

A [reference link][rs] resolves from a definition.

[rs]: https://rust-lang.org

---";

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(48, 34)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            frame.render_widget(
                Markdown::new(DOC).block(Block::bordered().title("markdown")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
