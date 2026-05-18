//! The `rstui-devtools` overlay, the way an app actually wires it: a
//! caller-owned [`PerfMeter`] fed by the runtime
//! [`FrameObserver`](rstui_runtime::FrameObserver) (here a handful of
//! representative [`FrameMetrics`] stand in for the live loop), projected
//! by the [`DevTools`] overlay. The meter is plain model state (ADR 0012
//! §P1, the `FpsMeter` precedent); the overlay only *reads* it, and the
//! shown tab is ordinary caller-owned state — the same pure projection
//! every rstui widget is.
//!
//! Live, you install [`DevToolsAdapter`](rstui_devtools::DevToolsAdapter)
//! via `rstui_runtime::run_with_observer` (or, for a full-screen
//! crossterm app, `rstui_crossterm::run_app_with_observer`) and toggle the
//! overlay from a hotkey — see [`docs/devtools.md`] and the kitchen sink
//! (`F12`). Here it runs over a [`TestBackend`] with fixed metrics, so it
//! is TTY-free and **deterministic**: it doubles as a snapshot smoke test.
//!
//! ```text
//! cargo run -p rstui-devtools --example devtools_demo
//! ```
//!
//! [`docs/devtools.md`]: https://github.com/andymac4182/rstui/blob/main/docs/devtools.md

use std::time::Duration;

use rstui_core::{Terminal, TestBackend, Widget};
use rstui_devtools::overlay::TABS;
use rstui_devtools::{DevTools, PerfMeter};
use rstui_runtime::FrameMetrics;
use rstui_widgets::Block;

/// One synthetic loop iteration. A real app never builds these — the
/// `DevToolsAdapter` is handed them by the runtime; this is only so the
/// example is deterministic.
fn frame(i: u64, total_us: u64, produced: bool, coalesced: u32) -> FrameMetrics {
    FrameMetrics {
        frame: i,
        // logic ≈ Scripting, view ≈ Rendering, flush ≈ Painting.
        logic: Duration::from_micros(total_us / 2),
        view: Duration::from_micros(total_us / 4),
        flush: Duration::from_micros(total_us / 4),
        total: Duration::from_micros(total_us),
        produced,
        events_coalesced: coalesced,
        input_latency: Duration::from_micros(total_us + 200),
    }
}

fn main() {
    // The meter an app owns on its model (typically behind `Rc` so the
    // observer and `view` can share it). `record` is what the
    // `DevToolsAdapter` calls once per observed loop iteration.
    let meter = PerfMeter::with_capacity(64);
    let frames = [
        frame(0, 1_800, true, 1),
        frame(1, 2_400, true, 1),
        frame(2, 9_600, false, 64), // an RT-01 mouse-move no-op flood
        frame(3, 2_100, true, 1),
        frame(4, 1_500, true, 1),
        frame(5, 5_200, true, 3),
        frame(6, 2_000, true, 1),
        frame(7, 12_800, false, 96), // a pointer-drag flood, coalesced
        frame(8, 1_900, true, 1),
        frame(9, 2_600, true, 2),
    ];
    for f in &frames {
        meter.record(f);
    }

    let mut terminal = Terminal::new(TestBackend::new(96, 26)).expect("TestBackend is infallible");

    // One tab per browser-style pane: Performance / Memory / Events /
    // Inspect. An app keeps the index in its model and renders the active
    // one; here we render and assert each.
    for (tab, name) in TABS.iter().enumerate() {
        terminal
            .draw(|f| {
                let area = f.area();
                DevTools::new(&meter)
                    .tab(tab)
                    .block(Block::bordered().title(" DevTools "))
                    .render(area, f.buffer_mut());
            })
            .expect("draw is infallible on TestBackend");
        let out = terminal.backend().to_string();
        println!("=== {name} ===\n{out}");

        // The tab bar lists every pane; the active pane shows its
        // signature content. Deterministic — the metrics above are fixed.
        for label in TABS {
            assert!(out.contains(label), "tab bar missing {label:?} on {name}");
        }
        let signature = match tab {
            0 => "FPS (window)",
            1 => "live heap",
            2 => "RT-01",
            _ => "frames recorded (window)",
        };
        assert!(
            out.contains(signature),
            "{name} tab missing its signature {signature:?}:\n{out}"
        );
    }

    // The Events tab must surface the coalesced floods (frames 2 & 7) as
    // the RT-01 saturation signal — the "freeze while moving the mouse"
    // class this tooling exists to make visible.
    terminal
        .draw(|f| {
            let area = f.area();
            DevTools::new(&meter).tab(2).render(area, f.buffer_mut());
        })
        .expect("draw is infallible on TestBackend");
    let events = terminal.backend().to_string();
    assert!(
        events.contains("96") && events.contains("RT-01"),
        "Events tab must show the worst coalesced burst (96) + the RT-01 line:\n{events}"
    );

    println!("devtools_demo: all four tabs render their signature content.");
}
