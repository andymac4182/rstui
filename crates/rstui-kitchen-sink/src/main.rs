//! The live entry point: the same [`KitchenSink`] App the headless
//! [`Harness`](rstui_runtime::Harness) tests drive, run full-screen on a real
//! terminal through [`rstui_crossterm::run_app`] — which owns the alternate
//! screen, raw mode, mouse + bracketed-paste + focus capture, the live event
//! loop, and panic-safe restore, all in one call.
//!
//! ```text
//! cargo run -p rstui-kitchen-sink
//! ```

use std::error::Error;

use rstui_core::Size;
use rstui_crossterm::run_app;
use rstui_kitchen_sink::KitchenSink;

fn main() -> Result<(), Box<dyn Error>> {
    // `--list-themes`: print every selectable theme name and exit. The VHS
    // per-theme suite (`cargo xtask record themes`) shells out to this so
    // xtask stays dependency-free — the binary owns theme knowledge.
    if std::env::args().any(|a| a == "--list-themes") {
        for theme in rstui_theme::Theme::all() {
            println!("{}", theme.name);
        }
        return Ok(());
    }

    // `RSTUI_THEME="<name>"` skins the whole app with that gpui-component
    // theme (unknown names keep the built-in default). The seed size is
    // corrected by the first live `Event::Resize`; the terminal is already
    // restored by the time this returns, on success, error, or panic.
    let mut app = KitchenSink::new(Size::new(120, 40));
    if let Ok(name) = std::env::var("RSTUI_THEME") {
        app = app.with_theme(&name);
    }
    run_app(app)?;
    Ok(())
}
