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
    // The seed size is corrected by the first live `Event::Resize`; the
    // terminal is already restored by the time this returns, on success,
    // error, or panic.
    run_app(KitchenSink::new(Size::new(120, 40)))?;
    Ok(())
}
