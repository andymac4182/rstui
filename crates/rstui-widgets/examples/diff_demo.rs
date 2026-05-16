//! Renders realistic diffs through [`Diff`] inside a [`Block`], exercising
//! the whole supported surface end to end so it can be eyeballed and doubles
//! as a deterministic, TTY-free snapshot smoke test:
//!
//! 1. a multi-hunk patch with `git` metadata (`old/new mode`, `similarity`,
//!    `rename`, `index`), a file header, two hunks with section labels,
//!    context/added/deleted lines and an intra-line word edit, drawn in the
//!    default unified layout and then the opt-in side-by-side layout via
//!    [`Diff::side_by_side`];
//! 2. the same patch with generic syntax highlighting on via
//!    [`Diff::syntax`] (keywords / strings / numbers / comments tinted under
//!    the diff colours);
//! 3. a combined (merge, `@@@`) conflict-style hunk with a 2-wide sign
//!    gutter; and
//! 4. a binary-file patch rendered as a clear "binary file changed" row.
//!
//! ```text
//! cargo run -p rstui-widgets --example diff_demo
//! ```

use rstui_core::{Terminal, TestBackend};
// `DiffLayout` lives in the (public) `diff` module; the crate root currently
// re-exports only `Diff`/`DiffTheme`.
use rstui_widgets::diff::DiffLayout;
use rstui_widgets::{Block, Diff};

/// A real-world patch: a rename with mode/similarity/index metadata, two
/// hunks with section labels, an intra-line edit, and the no-newline marker.
const PATCH: &str = "\
diff --git a/src/render.rs b/src/paint.rs
old mode 100644
new mode 100755
similarity index 94%
rename from src/render.rs
rename to src/paint.rs
index 1a2b3c4..5d6e7f8 100755
--- a/src/render.rs
+++ b/src/paint.rs
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

/// A combined (merge) diff: two parents, so `@@@ … @@@` and 2-wide body sign
/// columns — `- ` removed in parent 1, ` -` in parent 2, `++` added vs both.
const MERGE_PATCH: &str = "\
diff --cc src/merge.rs
index aaaaaaa,bbbbbbb..ccccccc
--- a/src/merge.rs
+++ b/src/merge.rs
@@@ -1,3 -1,3 +1,4 @@@ fn resolve()
  const LIMIT: usize = 64;
- let mode = Mode::Fast; // ours
 -let mode = Mode::Safe; // theirs
++let mode = Mode::Auto; // merged
  return mode;";

/// A binary-file change: never silently dropped, shown as a themed row.
const BINARY_PATCH: &str = "\
diff --git a/assets/logo.png b/assets/logo.png
index 0000000..1111111 100644
Binary files a/assets/logo.png and b/assets/logo.png differ";

/// A newly-added file: `--- /dev/null` and a `@@ -0,0 +1,N @@` hunk — every
/// body line is an addition with no old-side number.
const ADDED_FILE: &str = "\
diff --git a/src/new.rs b/src/new.rs
new file mode 100644
--- /dev/null
+++ b/src/new.rs
@@ -0,0 +1,3 @@
+pub fn hello() -> &'static str {
+    \"hi\"
+}";

/// Draws `patch` through [`Diff`] in `layout` (syntax highlight per
/// `syntax`), framed and titled, over a fresh [`TestBackend`] of
/// `width`×`height`, returning the rendered frame as text. The wider area
/// for the split layout gives each column room.
fn frame(
    patch: &str,
    layout: DiffLayout,
    syntax: bool,
    title: &str,
    width: u16,
    height: u16,
) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("TestBackend is infallible");
    terminal
        .draw(|f| {
            f.render_widget(
                Diff::new(patch)
                    .layout(layout)
                    .syntax(syntax)
                    .block(Block::bordered().title(title)),
                f.area(),
            );
        })
        .expect("TestBackend is infallible");
    terminal.backend().to_string()
}

fn main() {
    // The default unified layout: one column, the change sign in the gutter,
    // the rename/mode/index metadata rows above the file header.
    print!(
        "{}",
        frame(PATCH, DiffLayout::Unified, false, "diff (unified)", 60, 18)
    );
    // The opt-in side-by-side layout: old left, new right, a `│` between,
    // change groups paired row-for-row. Wider so both columns breathe.
    print!(
        "{}",
        frame(
            PATCH,
            DiffLayout::Split,
            false,
            "diff (side-by-side)",
            88,
            18
        )
    );
    // The same patch with generic syntax highlighting on (keywords/strings/
    // numbers/comments tinted under the add/del colours).
    print!(
        "{}",
        frame(PATCH, DiffLayout::Unified, true, "diff (syntax on)", 60, 18)
    );
    // A combined (merge) conflict-style hunk: `@@@ … @@@`, 2-wide signs.
    print!(
        "{}",
        frame(
            MERGE_PATCH,
            DiffLayout::Unified,
            true,
            "diff (combined merge)",
            56,
            10
        )
    );
    // A binary patch: a clear themed "binary file changed" row.
    print!(
        "{}",
        frame(
            BINARY_PATCH,
            DiffLayout::Unified,
            false,
            "diff (binary)",
            56,
            5
        )
    );
    // A newly-added file (`--- /dev/null`): all-addition hunk, no old number.
    print!(
        "{}",
        frame(
            ADDED_FILE,
            DiffLayout::Unified,
            true,
            "diff (added file)",
            56,
            8
        )
    );
}
