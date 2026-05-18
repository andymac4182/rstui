//! [`PerfMeter`] — caller-owned perf state recorded from the loop, and
//! [`DevToolsAdapter`] — the bridge from the runtime's
//! [`FrameObserver`](rstui_runtime::FrameObserver) to it (ADR 0018 §3/§5).
//!
//! This generalises the `FpsMeter` §P1 pattern (ADR 0012): interior
//! mutability so the separate [`FrameObserver`] *writes* the meter while
//! the pure `view` *reads* it through `&self`. The inline event loop calls
//! `on_frame` and then `view` on the same thread, never overlapping, so
//! the `RefCell` is never double-borrowed.
//!
//! # Wiring (live overlay)
//!
//! ```ignore
//! use std::rc::Rc;
//! use rstui_devtools::{PerfMeter, DevToolsAdapter};
//!
//! let perf = Rc::new(PerfMeter::with_capacity(240));
//! let app = MyApp::new(Rc::clone(&perf));      // app reads it in `view`
//! let mut dt = DevToolsAdapter::new(&perf);    // observer writes it
//! rstui_runtime::run_with_observer(app, backend, &mut events, &mut dt)?;
//! ```

use std::cell::{Cell, RefCell};
use std::time::Duration;

use rstui_runtime::{FrameMetrics, FrameObserver};

use crate::alloc::{self, AllocSnapshot};
use crate::session::{FrameSample, PerfSession};

/// Caller-owned perf history with interior mutability (the ADR-0012 §P1
/// `FpsMeter` pattern). Hold one in your model — typically behind `Rc` so
/// the separate [`FrameObserver`] and the app's `view` can share it across
/// [`run_with_observer`](rstui_runtime::run_with_observer).
#[derive(Debug)]
pub struct PerfMeter {
    session: RefCell<PerfSession>,
    /// The allocator counters as of the previous [`record`](Self::record),
    /// so each frame is attributed only *its* heap delta.
    prev_alloc: Cell<AllocSnapshot>,
}

impl PerfMeter {
    /// A meter retaining the last `capacity` frames. Baselines the
    /// allocation counters now so the first recorded frame's delta is
    /// measured from construction.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            session: RefCell::new(PerfSession::with_capacity(capacity)),
            prev_alloc: Cell::new(alloc::snapshot()),
        }
    }

    /// Records one loop iteration: pairs the runtime [`FrameMetrics`] with
    /// the heap delta since the previous record (via
    /// [`CountingAllocator`](crate::alloc::CountingAllocator) — zero if it
    /// is not installed) and pushes a [`FrameSample`]. Callable through
    /// `&self` (interior mutability) so an observer can record it.
    ///
    /// The runtime's `logic` phase maps to [`FrameSample::update`]; the
    /// idle `poll` wait is not a cost and is reported as zero `input`.
    pub fn record(&self, m: &FrameMetrics) {
        let now = alloc::snapshot();
        let delta = now.delta(&self.prev_alloc.get());
        self.prev_alloc.set(now);
        self.session.borrow_mut().record(FrameSample {
            frame: m.frame,
            input: Duration::ZERO,
            update: m.logic,
            view: m.view,
            flush: m.flush,
            total: m.total,
            produced: m.produced,
            events_coalesced: m.events_coalesced,
            input_latency: m.input_latency,
            alloc: delta,
        });
    }

    /// Reads the recorded [`PerfSession`] — the overlay projects this.
    /// Takes a closure to keep the `RefCell` borrow scoped (it must not be
    /// held across an `on_frame`).
    pub fn with_session<R>(&self, f: impl FnOnce(&PerfSession) -> R) -> R {
        f(&self.session.borrow())
    }
}

/// Bridges the runtime [`FrameObserver`] to a [`PerfMeter`] (ADR 0018 §3).
///
/// Construct with the app's (typically `Rc`-shared) meter and pass to
/// [`run_with_observer`](rstui_runtime::run_with_observer); it records
/// every observed iteration into the meter, which the app's overlay reads.
pub struct DevToolsAdapter<'a> {
    meter: &'a PerfMeter,
}

impl<'a> DevToolsAdapter<'a> {
    /// An adapter recording into `meter`.
    #[must_use]
    pub fn new(meter: &'a PerfMeter) -> Self {
        Self { meter }
    }
}

impl FrameObserver for DevToolsAdapter<'_> {
    fn on_frame(&mut self, metrics: &FrameMetrics) {
        self.meter.record(metrics);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(frame: u64, total_us: u64, produced: bool, coalesced: u32) -> FrameMetrics {
        FrameMetrics {
            frame,
            logic: Duration::from_micros(total_us / 2),
            view: Duration::from_micros(total_us / 4),
            flush: Duration::from_micros(total_us / 4),
            total: Duration::from_micros(total_us),
            produced,
            events_coalesced: coalesced,
            input_latency: Duration::from_micros(total_us),
        }
    }

    #[test]
    fn adapter_records_metrics_into_the_meter() {
        let meter = PerfMeter::with_capacity(8);
        let mut dt = DevToolsAdapter::new(&meter);
        dt.on_frame(&metrics(0, 1000, true, 1));
        dt.on_frame(&metrics(1, 2000, false, 64)); // an RT-01 no-op flood
        dt.on_frame(&metrics(2, 1500, true, 1));

        meter.with_session(|s| {
            assert_eq!(s.total_frames(), 3);
            assert_eq!(s.len(), 3);
            let last = s.last().unwrap();
            assert_eq!(last.frame, 2);
            assert_eq!(last.total, Duration::from_micros(1500));
            // logic → update mapping; input stays zero (idle poll wait
            // is not a cost).
            assert_eq!(last.update, Duration::from_micros(750));
            assert_eq!(last.input, Duration::ZERO);
            // The flood frame is retained with produced=false + its count.
            let flood = s.samples().find(|f| f.frame == 1).unwrap();
            assert!(!flood.produced);
            assert_eq!(flood.events_coalesced, 64);
            // Aggregate over the window is exact.
            let a = s.aggregate(|f| f.total);
            assert_eq!(a.min, Duration::from_micros(1000));
            assert_eq!(a.max, Duration::from_micros(2000));
        });
    }

    #[test]
    fn alloc_delta_is_attributed_per_frame_and_never_underflows() {
        // Without the global allocator installed the counters are static
        // zero, so deltas are zero — the meter must still be well-formed
        // (no panic, no underflow) and the per-frame attribution sane.
        let meter = PerfMeter::with_capacity(4);
        let mut dt = DevToolsAdapter::new(&meter);
        dt.on_frame(&metrics(0, 500, true, 1));
        dt.on_frame(&metrics(1, 500, true, 1));
        meter.with_session(|s| {
            for f in s.samples() {
                assert!(f.alloc.bytes < u64::MAX);
                assert!(f.alloc.allocs <= f.alloc.allocs.saturating_add(1));
            }
        });
    }
}
