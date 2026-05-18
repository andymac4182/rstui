//! [`DevTools`] — a Chrome-DevTools-style overlay (ADR 0018 §5).
//!
//! A **pure projection** of a caller-owned [`PerfMeter`] (ADR 0012): it
//! reads the recorded [`PerfSession`], owns nothing,
//! mutates only the [`Buffer`], and is built entirely from existing
//! `rstui-widgets` primitives — it adds no rendering machinery. The
//! selected tab and whether it is shown at all are ordinary caller-owned
//! state (`tab: usize`, a `show` bool) the app toggles in `update`, never
//! widget-driven.
//!
//! Four tabs, mirroring a browser's: **Performance** (per-phase frame
//! cost + FPS + history), **Memory** (live/peak/per-frame allocation +
//! leak hint), **Events** (input→frame latency + the mouse-flood / RT-01
//! signal), **Inspect** (session summary + RT-01 skip ratio).

use rstui_core::{Buffer, Constraint, Layout, Line, Rect, Style, Widget};
use rstui_widgets::{Block, Paragraph, StatPanel, Tabs, Wrap};

use crate::observer::PerfMeter;
use crate::session::PerfSession;
use std::time::Duration;

/// The overlay tab labels, in order; index with [`DevTools::tab`].
pub const TABS: [&str; 4] = ["Performance", "Memory", "Events", "Inspect"];

/// A Chrome-DevTools-style overlay projecting a borrowed [`PerfMeter`].
///
/// Construct in `view`, select the tab from caller-owned state, optionally
/// frame it, and render it over your UI (typically gated behind a
/// caller-owned "show devtools" flag toggled by a hotkey).
#[derive(Debug)]
pub struct DevTools<'a> {
    meter: &'a PerfMeter,
    tab: usize,
    block: Option<Block<'a>>,
    style: Style,
}

impl<'a> DevTools<'a> {
    /// An overlay projecting `meter`, Performance tab, unframed.
    #[must_use]
    pub fn new(meter: &'a PerfMeter) -> Self {
        Self {
            meter,
            tab: 0,
            block: None,
            style: Style::new(),
        }
    }

    /// Selects the visible tab by index (clamped into [`TABS`]). The index
    /// is caller-owned state — the overlay never changes it itself.
    #[must_use]
    pub fn tab(mut self, tab: usize) -> Self {
        self.tab = tab;
        self
    }

    /// Frames the overlay in `block`; content renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`] (also fills the content area).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

/// Adaptive `ns`/`µs`/`ms` formatting (the bench-harness convention).
fn dur(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 1_000 {
        format!("{ns} ns")
    } else if ns < 1_000_000 {
        format!("{:.2} µs", ns as f64 / 1_000.0)
    } else {
        format!("{:.2} ms", ns as f64 / 1_000_000.0)
    }
}

/// Adaptive `B`/`KiB`/`MiB` formatting.
fn bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", n as f64 / (1024.0 * 1024.0))
    }
}

/// One labelled card: a bordered [`StatPanel`] with a caption, a big
/// value, and an optional inline sparkline.
fn card(area: Rect, buf: &mut Buffer, caption: &str, value: &str, spark: &[u64]) {
    if area.is_empty() {
        return;
    }
    let panel = StatPanel::new(Line::raw(value.to_owned()))
        .caption(Line::raw(caption.to_owned()))
        .sparkline(spark)
        .block(Block::bordered());
    panel.render(area, buf);
}

/// Splits `body` into a 2×2 card grid above a full-width history strip.
fn grid(body: Rect) -> ([Rect; 4], Rect) {
    let [cards, hist] = Layout::vertical([Constraint::Fill(1), Constraint::Length(7)]).areas(body);
    let [top, bot] = Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).areas(cards);
    let [a, b] = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(top);
    let [c, d] = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(bot);
    ([a, b, c, d], hist)
}

impl DevTools<'_> {
    fn performance(&self, s: &PerfSession, body: Rect, buf: &mut Buffer) {
        let total = s.aggregate(|f| f.total);
        let view = s.aggregate(|f| f.view);
        let flush = s.aggregate(|f| f.flush);
        let logic = s.aggregate(|f| f.update);
        let totals: Vec<u64> = s.samples().map(|f| f.total.as_micros() as u64).collect();
        let (cells, hist) = grid(body);
        card(
            cells[0],
            buf,
            "FPS (window)",
            &format!("{:.0}", s.fps()),
            &totals,
        );
        card(
            cells[1],
            buf,
            "frame p50 / p99",
            &format!("{} / {}", dur(total.median), dur(total.p99)),
            &totals,
        );
        card(
            cells[2],
            buf,
            "logic p50 (Scripting)",
            &dur(logic.median),
            &[],
        );
        card(
            cells[3],
            buf,
            "view / flush p50",
            &format!("{} / {}", dur(view.median), dur(flush.median)),
            &[],
        );
        let label = format!(
            "frame total — min {}  p50 {}  p95 {}  p99 {}  max {}   ({} frames)",
            dur(total.min),
            dur(total.median),
            dur(total.p95),
            dur(total.p99),
            dur(total.max),
            s.len(),
        );
        Paragraph::new(label)
            .wrap(Wrap { trim: true })
            .block(Block::bordered())
            .render(hist, buf);
    }

    fn memory(&self, s: &PerfSession, body: Rect, buf: &mut Buffer) {
        let snap = crate::alloc::snapshot();
        let net: i64 = s.samples().map(|f| f.alloc.net_live).sum();
        let allocs: u64 = s.samples().map(|f| f.alloc.allocs).sum();
        let frees: u64 = s.samples().map(|f| f.alloc.deallocs).sum();
        let window_bytes: u64 = s.samples().map(|f| f.alloc.bytes).sum();
        let per_frame: Vec<u64> = s.samples().map(|f| f.alloc.bytes).collect();
        let (cells, hist) = grid(body);
        card(
            cells[0],
            buf,
            "live heap",
            &bytes(snap.live_bytes as u64),
            &per_frame,
        );
        card(
            cells[1],
            buf,
            "peak heap",
            &bytes(snap.peak_bytes as u64),
            &[],
        );
        card(
            cells[2],
            buf,
            "alloc bytes (window)",
            &bytes(window_bytes),
            &per_frame,
        );
        card(
            cells[3],
            buf,
            "allocs vs frees (window)",
            &format!("{allocs} / {frees}"),
            &[],
        );
        // A persistently positive net-live with allocs > frees across the
        // window is the per-frame-leak signature (ADR 0018 §2).
        let leak = net > 0 && allocs > frees && !s.is_empty();
        let verdict = if s.is_empty() {
            "no frames recorded".to_owned()
        } else if leak {
            format!(
                "LEAK SUSPECT — net live +{} over {} frames ({} allocs, {} frees). \
                 If this only grows the app retains per frame.",
                bytes(net.unsigned_abs()),
                s.len(),
                allocs,
                frees,
            )
        } else {
            format!(
                "steady — net live {}{} over {} frames; allocations balance.",
                if net >= 0 { "+" } else { "-" },
                bytes(net.unsigned_abs()),
                s.len(),
            )
        };
        Paragraph::new(verdict)
            .wrap(Wrap { trim: true })
            .block(Block::bordered())
            .render(hist, buf);
    }

    fn events(&self, s: &PerfSession, body: Rect, buf: &mut Buffer) {
        let lat = s.aggregate(|f| f.input_latency);
        let worst = s.worst(|f| f.input_latency);
        let max_coalesced = s.samples().map(|f| f.events_coalesced).max().unwrap_or(0);
        let coalesced: Vec<u64> = s.samples().map(|f| u64::from(f.events_coalesced)).collect();
        let produced = s.samples().filter(|f| f.produced).count();
        let (cells, hist) = grid(body);
        card(
            cells[0],
            buf,
            "input→frame p50",
            &dur(lat.median),
            &coalesced,
        );
        card(cells[1], buf, "input→frame p99", &dur(lat.p99), &[]);
        card(
            cells[2],
            buf,
            "worst stall",
            &worst.map_or_else(|| "—".to_owned(), |f| dur(f.input_latency)),
            &[],
        );
        card(
            cells[3],
            buf,
            "max events / frame",
            &max_coalesced.to_string(),
            &coalesced,
        );
        let skipped = s.len().saturating_sub(produced);
        let msg = format!(
            "RT-01: {produced} of {} iterations repainted, {skipped} no-op floods coalesced \
             & skipped (max {max_coalesced} events folded into one). A high skip count under \
             pointer motion with low latency is the saturation guard working.",
            s.len(),
        );
        Paragraph::new(msg)
            .wrap(Wrap { trim: true })
            .block(Block::bordered())
            .render(hist, buf);
    }

    fn inspect(&self, s: &PerfSession, body: Rect, buf: &mut Buffer) {
        let produced = s.samples().filter(|f| f.produced).count();
        let lines = [
            format!("frames recorded (window) : {}", s.len()),
            format!("frames total (all-time)  : {}", s.total_frames()),
            format!(
                "repainted / skipped      : {} / {}",
                produced,
                s.len() - produced
            ),
            format!("fps (window)             : {:.1}", s.fps()),
            format!(
                "logic  p50 / p99         : {} / {}",
                dur(s.aggregate(|f| f.update).median),
                dur(s.aggregate(|f| f.update).p99)
            ),
            format!(
                "view   p50 / p99         : {} / {}",
                dur(s.aggregate(|f| f.view).median),
                dur(s.aggregate(|f| f.view).p99)
            ),
            format!(
                "flush  p50 / p99         : {} / {}",
                dur(s.aggregate(|f| f.flush).median),
                dur(s.aggregate(|f| f.flush).p99)
            ),
            format!(
                "frame  p50 / p99 / max   : {} / {} / {}",
                dur(s.aggregate(|f| f.total).median),
                dur(s.aggregate(|f| f.total).p99),
                dur(s.aggregate(|f| f.total).max)
            ),
        ];
        let text = lines.join("\n");
        Paragraph::new(text)
            .block(Block::bordered().title(Line::raw(" session ")))
            .render(body, buf);
    }
}

impl Widget for DevTools<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if let Some(b) = &self.block {
            b.render_ref(area, buf);
        }
        if inner.is_empty() {
            return;
        }
        buf.set_style(inner, self.style);

        let [tab_strip, body] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
        let tab = self.tab.min(TABS.len() - 1);
        Tabs::new(TABS).selected(Some(tab)).render(tab_strip, buf);

        self.meter.with_session(|s| match tab {
            0 => self.performance(s, body, buf),
            1 => self.memory(s, body, buf),
            2 => self.events(s, body, buf),
            _ => self.inspect(s, body, buf),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observer::DevToolsAdapter;
    use rstui_core::Position;
    use rstui_runtime::{FrameMetrics, FrameObserver};

    fn feed(meter: &PerfMeter, n: u64) {
        let mut dt = DevToolsAdapter::new(meter);
        for i in 0..n {
            dt.on_frame(&FrameMetrics {
                frame: i,
                logic: Duration::from_micros(40),
                view: Duration::from_micros(30),
                flush: Duration::from_micros(12),
                total: Duration::from_micros(82),
                produced: i % 5 != 0, // some RT-01 skips
                events_coalesced: if i % 5 == 0 { 64 } else { 1 },
                input_latency: Duration::from_micros(82),
            });
        }
    }

    fn render_tab(meter: &PerfMeter, tab: usize) -> String {
        let area = Rect::new(0, 0, 64, 16);
        let mut b = Buffer::empty(area);
        DevTools::new(meter)
            .tab(tab)
            .block(Block::bordered())
            .render(area, &mut b);
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push(b.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn every_tab_renders_a_non_empty_panel_and_is_total() {
        let meter = PerfMeter::with_capacity(120);
        feed(&meter, 60);
        for tab in 0..TABS.len() {
            let s = render_tab(&meter, tab);
            assert!(s.contains('│'), "tab {tab}: expected a bordered panel");
            // The tab strip shows all four labels.
            assert!(
                s.contains("Performance") && s.contains("Inspect"),
                "tab {tab}: tab strip"
            );
        }
        // Performance shows the FPS and a frame stat; Events explains RT-01.
        let perf = render_tab(&meter, 0);
        assert!(perf.contains("FPS") || perf.contains("frame"));
        let events = render_tab(&meter, 2);
        assert!(events.contains("RT-01"));
        let inspect = render_tab(&meter, 3);
        assert!(inspect.contains("frames recorded"));
    }

    #[test]
    fn empty_session_and_zero_area_are_no_ops_not_panics() {
        let meter = PerfMeter::with_capacity(8);
        // Empty session: still renders the chrome, no panic, no divide-by-zero.
        let s = render_tab(&meter, 1);
        assert!(s.contains("no frames recorded") || s.contains('│'));
        // Zero area: total no-op.
        let mut b = Buffer::empty(Rect::new(0, 0, 1, 1));
        DevTools::new(&meter).render(Rect::new(0, 0, 0, 0), &mut b);
        // An out-of-range tab clamps rather than panicking.
        let mut b2 = Buffer::empty(Rect::new(0, 0, 20, 6));
        DevTools::new(&meter)
            .tab(99)
            .render(Rect::new(0, 0, 20, 6), &mut b2);
    }
}
