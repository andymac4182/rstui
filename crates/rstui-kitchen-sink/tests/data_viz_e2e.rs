//! App-scale E2E for the **palette-only data-viz screens**. The generic
//! `render_e2e` suite drives screens by number hotkey, which only reaches the
//! first eight (`1`–`8`) — so every screen below it in `Screen::ALL` is
//! reachable *only* via the command palette and has no app-scale coverage.
//! This pins the six chart-bearing ones:
//!
//! - `Observability`, `Metrics`, `Traces` — the suite that showcases the
//!   observability widget family (`LineChart`, `Heatmap`, `Histogram`,
//!   `StatPanel`, `FlameGraph`, `TraceWaterfall`, `LogStream`);
//! - `Dashboard` (the business-dashboard chart suite: `Canvas` revenue line,
//!   `StackedBarChart`, `Waterfall`, `Funnel`, `PieChart`, `BulletChart`,
//!   `Gantt`, `CalendarHeatmap`, KPI `Sparkline`s);
//! - `Analytics` (the exploratory chart catalog: `ScatterPlot`, `RadarChart`,
//!   `BoxPlot`, `Candlestick`, `Treemap`, `Sankey`) and `Live Logs` (the
//!   filtered tail) — the sibling palette-only screens the hotkey suite
//!   also cannot reach.
//!
//! Each must be reachable via the palette, render its signature content, be
//! actually coloured through the real theme, and stay total (no panic) under
//! resize + animation ticks — the contract `render_e2e` proves for the
//! hotkey screens, here for the palette-only ones.

use std::collections::HashSet;

use rstui_core::{Color, Event, KeyCode, KeyEvent, Position, Size};
use rstui_kitchen_sink::KitchenSink;
use rstui_runtime::Harness;

fn harness() -> Harness<KitchenSink> {
    Harness::new(KitchenSink::new(Size::new(120, 40)), 120, 40)
}
fn ch(c: char) -> Event {
    Event::from(KeyEvent::char(c))
}
fn key(code: KeyCode) -> Event {
    Event::from(KeyEvent::from_code(code))
}

/// Distinct `(fg, bg)` pairs across the whole frame — a monochrome
/// (uncoloured) frame yields one pair, a themed UI many. The same
/// end-to-end colour proof `render_e2e` uses.
fn distinct_color_pairs(h: &Harness<KitchenSink>) -> HashSet<(Color, Color)> {
    let buf = h.backend().buffer();
    let a = buf.area();
    let mut set = HashSet::new();
    for y in a.top()..a.bottom() {
        for x in a.left()..a.right() {
            if let Some(c) = buf.get(Position::new(x, y)) {
                set.insert((c.fg, c.bg));
            }
        }
    }
    set
}

/// Open the command palette (`:`), type a query that *uniquely* matches one
/// screen's label/title, and activate it — the exact mechanism the
/// kitchen-sink tour and real users use for screens with no number hotkey.
fn goto(h: &mut Harness<KitchenSink>, query: &str) {
    h.handle(ch(':'));
    for c in query.chars() {
        h.handle(ch(c));
    }
    h.handle(key(KeyCode::Enter));
    h.tick();
}

/// `(palette query, header-title marker, signature body marker)`. The body
/// marker is a panel/border title each screen always renders (verified
/// against the screen modules), so a present marker proves the screen's
/// content actually composed, not just the global chrome.
const DATA_VIZ: [(&str, &str, &str); 6] = [
    ("dashboard", "Dashboard", "Roadmap"),
    ("live logs", "Live Logs", "application.log"),
    ("observability", "Observability", "Throughput vs errors"),
    ("metrics", "Metrics", "Latency heatmap"),
    ("traces", "Traces", "Span waterfall"),
    ("analytics", "Analytics", "Treemap"),
];

#[test]
fn data_viz_screens_reachable_and_render_their_widgets() {
    for (query, title, body) in DATA_VIZ {
        let mut h = harness();
        goto(&mut h, query);

        assert!(
            h.is_running(),
            "{title}: app keeps running after palette navigation"
        );
        let snap = h.snapshot();
        assert!(
            snap.contains(title),
            "{title}: header title must render; got:\n{snap}"
        );
        assert!(
            snap.contains(body),
            "{title}: signature widget content {body:?} must render; got:\n{snap}"
        );
        assert!(
            snap.chars().any(|c| !c.is_whitespace()),
            "{title}: rendered a blank frame"
        );
        let pairs = distinct_color_pairs(&h);
        assert!(
            pairs.len() >= 3,
            "{title}: only {} colour pair(s) — theme not applied end to end",
            pairs.len()
        );
    }
}

#[test]
fn data_viz_screens_survive_resize_and_ticks() {
    // Walk every data-viz screen, resize through normal → tiny → large, feed
    // input, advance the animation clock — the composed render path must stay
    // total and keep running at every size (the `render_e2e` contract, here
    // for the palette-only screens).
    for (query, title, _body) in DATA_VIZ {
        let mut h = harness();
        goto(&mut h, query);
        for (i, (w, hgt)) in [(160u16, 50u16), (80, 24), (8, 4), (200, 60)]
            .into_iter()
            .enumerate()
        {
            h.resize(w, hgt);
            h.handle(key(KeyCode::Down));
            h.handle(key(KeyCode::Right));
            for _ in 0..3 {
                h.tick();
            }
            assert!(
                h.is_running(),
                "{title}: survived input+resize+ticks at {w}x{hgt} (step {i})"
            );
            assert!(
                !h.snapshot().is_empty(),
                "{title}: still renders at {w}x{hgt}"
            );
        }
    }
}
