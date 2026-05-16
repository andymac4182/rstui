//! Renders a realistic multi-hunk unified diff through [`Diff`] inside a
//! [`Block`]: a file header, two hunks with section labels, context/added/
//! deleted lines, an intra-line word edit, and a "no newline at end of file"
//! marker — the supported subset exercised end to end.
//!
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test of the diff layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example diff_demo
//! ```

use rstui_core::{Terminal, TestBackend};
use rstui_widgets::{Block, Diff};

const PATCH: &str = "\
diff --git a/src/render.rs b/src/render.rs
--- a/src/render.rs
+++ b/src/render.rs
@@ -1,4 +1,4 @@ fn paint(area: Rect)
 use crate::buffer::Buffer;
-let title = \"old report\";
+let title = \"new report\";
 let mut x = area.left();
 draw(&mut x);
@@ -20,3 +20,4 @@ fn flush(&mut self)
 self.commit();
-self.swap();
+self.swap_buffers();
+self.present();
\\ No newline at end of file";

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(56, 16)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            frame.render_widget(
                Diff::new(PATCH).block(Block::bordered().title("diff")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
