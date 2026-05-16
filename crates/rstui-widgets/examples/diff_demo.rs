//! Renders a realistic multi-hunk diff through [`Diff`] inside a [`Block`]: a
//! file header, two hunks with section labels, context/added/deleted lines,
//! an intra-line word edit, and a "no newline at end of file" marker — the
//! supported subset exercised end to end, drawn first in the default unified
//! layout and then in the opt-in side-by-side layout via
//! [`Diff::side_by_side`] so the two renderings can be eyeballed against the
//! same patch.
//!
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test of the diff layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example diff_demo
//! ```

use rstui_core::{Terminal, TestBackend};
// `DiffLayout` lives in the (public) `diff` module; the crate root currently
// re-exports only `Diff`/`DiffTheme`.
use rstui_widgets::diff::DiffLayout;
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

/// Draws `patch` through [`Diff`] in `layout`, framed and titled, over a
/// fresh [`TestBackend`] of `width`×`height`, and returns the rendered frame
/// as text. The wider area for the split layout gives each column room.
fn frame(patch: &str, layout: DiffLayout, title: &str, width: u16, height: u16) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("TestBackend is infallible");
    terminal
        .draw(|f| {
            f.render_widget(
                Diff::new(patch)
                    .layout(layout)
                    .block(Block::bordered().title(title)),
                f.area(),
            );
        })
        .expect("TestBackend is infallible");
    terminal.backend().to_string()
}

fn main() {
    // The default unified layout: one column, the change sign in the gutter.
    print!(
        "{}",
        frame(PATCH, DiffLayout::Unified, "diff (unified)", 56, 16)
    );
    // The opt-in side-by-side layout: old left, new right, a `│` between,
    // change groups paired row-for-row. Wider so both columns breathe.
    print!(
        "{}",
        frame(PATCH, DiffLayout::Split, "diff (side-by-side)", 84, 16)
    );
}
