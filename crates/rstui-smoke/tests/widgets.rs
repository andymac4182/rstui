//! Breadth smoke: render the richer `rstui-widgets` (Tree, Toast, Markdown)
//! through the runtime `Frame::render_widget` seam, and pin the crossterm
//! full-screen `run_app` shell's public signature at compile time.
//!
//! These widgets just landed from other streams; this is the cross-cutting
//! guard that they still compose with the runtime + core public contract.
//! Assertions are behavioral (a known label appears, nothing panics), never
//! exact glyph snapshots, so another stream restyling a widget does not
//! false-positive this gate. `run_app` needs a real terminal and cannot run
//! headless, so its signature is pinned at compile time instead — a break
//! there must still fail this crate's build.

use rstui_runtime::{App, Cmd, Frame, Harness};
use rstui_widgets::{Markdown, Toast, ToastLevel, ToastMessage, Tree, TreeItem};

/// A one-frame app whose `view` is a plain `fn` pointer, so a single generic
/// harness drives any widget without a bespoke `App` per case. Closures that
/// capture nothing coerce to this `fn` type.
struct DrawApp(fn(&mut Frame<'_>));

impl App for DrawApp {
    type Message = ();

    fn update(&mut self, (): ()) -> Cmd<()> {
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        (self.0)(frame)
    }
}

/// Render `draw` once on a `w`×`h` headless surface and return the snapshot.
fn render(draw: fn(&mut Frame<'_>), w: u16, h: u16) -> String {
    Harness::new(DrawApp(draw), w, h).snapshot()
}

#[test]
fn tree_widget_composes_with_the_runtime_seam() {
    let snap = render(
        |frame| {
            let area = frame.area();
            frame.render_widget(
                Tree::new([
                    TreeItem::new(0, "root").expandable(true),
                    TreeItem::new(1, "child-a"),
                    TreeItem::new(1, "child-b"),
                ]),
                area,
            );
        },
        28,
        5,
    );
    assert!(
        snap.contains("root") && snap.contains("child-a"),
        "Tree must render its labels through the runtime seam; got:\n{snap}"
    );
}

#[test]
fn toast_widget_composes_with_the_runtime_seam() {
    let snap = render(
        |frame| {
            let area = frame.area();
            let messages = [
                ToastMessage::new(ToastLevel::Info, "saved ok"),
                ToastMessage::new(ToastLevel::Error, "disk full"),
            ];
            frame.render_widget(Toast::new(&messages[..]), area);
        },
        32,
        8,
    );
    assert!(
        snap.contains("saved ok") || snap.contains("disk full"),
        "Toast must render at least one message; got:\n{snap}"
    );
}

#[test]
fn markdown_widget_composes_with_the_runtime_seam() {
    let snap = render(
        |frame| {
            let area = frame.area();
            frame.render_widget(Markdown::new("# Heading\n\nbody **bold**"), area);
        },
        22,
        4,
    );
    assert!(
        snap.contains("Heading"),
        "Markdown must render its heading text; got:\n{snap}"
    );
}

/// Compile-time only: the crossterm full-screen shell needs a real terminal,
/// so it cannot be driven headless — but its public signature breaking must
/// still fail this cross-cutting crate's build. Binding the fn items as typed
/// `fn` pointers pins `run_app` / `run_app_with` (the `App` bound and the
/// return type) without ever calling them.
#[test]
fn crossterm_run_app_shell_signature_is_pinned() {
    let _: fn(
        rstui_smoke::SmokeApp,
    ) -> Result<rstui_smoke::SmokeApp, rstui_crossterm::CrosstermRunError> =
        rstui_crossterm::run_app::<rstui_smoke::SmokeApp>;
    let _: fn(
        rstui_smoke::SmokeApp,
        rstui_crossterm::LifecycleOptions,
    ) -> Result<rstui_smoke::SmokeApp, rstui_crossterm::CrosstermRunError> =
        rstui_crossterm::run_app_with::<rstui_smoke::SmokeApp>;
}
