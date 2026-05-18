//! [`PerfSession`] — a caller-owned ring of per-frame [`FrameSample`]s
//! plus order-statistic [`Aggregate`]s (ADR 0018 §3).
//!
//! This is **model state you own**, exactly the ADR-0012
//! `ScrollState`/`Input`/`Editor` seam: the reducer (or the runtime
//! `FrameObserver`) calls [`PerfSession::record`]; the DevTools overlay is
//! a pure projection that only *reads* it. There is no global state and no
//! retained widget tree — `rstui-devtools` adds zero rendering machinery,
//! it just gives you the numbers and a widget that draws them.

use std::time::Duration;

use crate::alloc::AllocDelta;

/// Everything measured about one event-loop iteration (one rendered, or
/// skipped, frame). Phase `Duration`s sum to roughly `total`; `produced`
/// is the RT-01 flag (did `update` actually change anything — `false`
/// means the render was skipped/idle).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameSample {
    /// Monotonic frame index since the session started.
    pub frame: u64,
    /// Draining + decoding coalesced input events.
    pub input: Duration,
    /// `App::update` (all folded messages this iteration).
    pub update: Duration,
    /// `App::view` projection into the back buffer.
    pub view: Duration,
    /// Buffer diff + terminal flush.
    pub flush: Duration,
    /// Whole-iteration wall time.
    pub total: Duration,
    /// RT-01: did this iteration actually change state/render?
    pub produced: bool,
    /// How many input events were coalesced into this iteration (a high
    /// value during a mouse-move flood is the latency-risk signal).
    pub events_coalesced: u32,
    /// Worst input-event-arrival → frame-presented latency this iteration.
    pub input_latency: Duration,
    /// Heap activity attributed to this iteration (via
    /// [`CountingAllocator`](crate::alloc::CountingAllocator)).
    pub alloc: AllocDelta,
}

/// Order statistics over one selected metric across the recorded window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Aggregate {
    /// Smallest sample — the cleanest cross-run signal (mirrors the bench
    /// harness's `min` convention).
    pub min: Duration,
    /// Lower-middle element (the bench harness's `median` convention).
    pub median: Duration,
    /// Nearest-rank 95th percentile — the "slow frame" threshold.
    pub p95: Duration,
    /// Nearest-rank 99th percentile — the "jank" threshold.
    pub p99: Duration,
    /// Largest sample (the worst stall in the window).
    pub max: Duration,
    /// Arithmetic mean.
    pub mean: Duration,
}

/// A fixed-capacity ring of the most recent [`FrameSample`]s. Caller-owned
/// (ADR 0012): hold one in your app model, `record` into it from the
/// reducer/observer, hand `&` to the overlay.
#[derive(Debug, Clone)]
pub struct PerfSession {
    ring: Vec<FrameSample>,
    cap: usize,
    /// Index of the oldest element when full; insertion point otherwise.
    head: usize,
    len: usize,
    /// Total frames ever recorded (not just retained) — drives `frame`.
    count: u64,
}

impl PerfSession {
    /// A session retaining the last `capacity` frames (clamped to ≥ 1).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            ring: Vec::with_capacity(cap),
            cap,
            head: 0,
            len: 0,
            count: 0,
        }
    }

    /// Records one iteration. `sample.frame` is overwritten with the
    /// session's monotonic counter so callers need not track it.
    pub fn record(&mut self, mut sample: FrameSample) {
        sample.frame = self.count;
        self.count += 1;
        if self.len < self.cap {
            self.ring.push(sample);
            self.len += 1;
        } else {
            self.ring[self.head] = sample;
            self.head = (self.head + 1) % self.cap;
        }
    }

    /// Retained frames, oldest first.
    pub fn samples(&self) -> impl Iterator<Item = &FrameSample> {
        let (head, len, cap) = (self.head, self.len, self.cap);
        (0..len).map(move |i| &self.ring[(head + i) % cap])
    }

    /// The most recently recorded frame, if any.
    #[must_use]
    pub fn last(&self) -> Option<&FrameSample> {
        if self.len == 0 {
            None
        } else {
            let idx = (self.head + self.len - 1) % self.cap;
            Some(&self.ring[idx])
        }
    }

    /// Retained-frame count (≤ capacity).
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether nothing has been recorded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Total frames ever recorded, including evicted ones.
    #[must_use]
    pub fn total_frames(&self) -> u64 {
        self.count
    }

    /// Frames per second over the retained window, from the `total`
    /// per-iteration durations. `0.0` when empty or degenerate.
    #[must_use]
    pub fn fps(&self) -> f32 {
        if self.len == 0 {
            return 0.0;
        }
        let secs: f64 = self.samples().map(|s| s.total.as_secs_f64()).sum();
        if secs <= 0.0 {
            return 0.0;
        }
        (self.len as f64 / secs) as f32
    }

    /// Order statistics for one selected metric over the retained window.
    /// `select` projects a [`FrameSample`] to the `Duration` of interest
    /// (e.g. `|s| s.view`). Empty window → all-zero [`Aggregate`].
    #[must_use]
    pub fn aggregate(&self, select: impl Fn(&FrameSample) -> Duration) -> Aggregate {
        if self.len == 0 {
            return Aggregate::default();
        }
        let mut v: Vec<Duration> = self.samples().map(&select).collect();
        v.sort_unstable();
        let n = v.len();
        let sum: Duration = v.iter().copied().sum();
        Aggregate {
            min: v[0],
            median: v[(n - 1) / 2],
            p95: v[nearest_rank(95, n)],
            p99: v[nearest_rank(99, n)],
            max: v[n - 1],
            mean: sum / (n as u32),
        }
    }

    /// The retained frame with the largest value of the selected metric
    /// (the worst stall) — for "jump to slowest frame" in the overlay.
    #[must_use]
    pub fn worst(&self, select: impl Fn(&FrameSample) -> Duration) -> Option<&FrameSample> {
        self.samples().max_by_key(|s| select(s))
    }
}

/// Nearest-rank index for the `p`-th percentile of `n` sorted samples:
/// `ceil(p/100 · n) − 1`, clamped to `0..n`.
fn nearest_rank(p: u32, n: usize) -> usize {
    debug_assert!(n > 0 && p <= 100);
    let rank = ((u64::from(p) * n as u64).div_ceil(100)) as usize;
    rank.saturating_sub(1).min(n - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn sample(total_ms: u64) -> FrameSample {
        FrameSample {
            total: ms(total_ms),
            view: ms(total_ms / 2),
            ..FrameSample::default()
        }
    }

    #[test]
    fn ring_wraps_keeping_the_last_capacity_in_order() {
        let mut s = PerfSession::with_capacity(3);
        assert!(s.is_empty());
        for i in 1..=5 {
            s.record(sample(i));
        }
        assert_eq!(s.len(), 3);
        assert_eq!(s.total_frames(), 5);
        // Oldest-first: the last 3 recorded (totals 3,4,5).
        let totals: Vec<u64> = s.samples().map(|x| x.total.as_millis() as u64).collect();
        assert_eq!(totals, vec![3, 4, 5]);
        // Monotonic frame indices were assigned (0..5), last three kept.
        let frames: Vec<u64> = s.samples().map(|x| x.frame).collect();
        assert_eq!(frames, vec![2, 3, 4]);
        assert_eq!(s.last().unwrap().frame, 4);
    }

    #[test]
    fn capacity_is_clamped_to_at_least_one() {
        let mut s = PerfSession::with_capacity(0);
        s.record(sample(7));
        s.record(sample(9));
        assert_eq!(s.len(), 1);
        assert_eq!(s.last().unwrap().total, ms(9));
        assert_eq!(s.total_frames(), 2);
    }

    #[test]
    fn aggregate_order_statistics_are_exact() {
        let mut s = PerfSession::with_capacity(16);
        // Deterministic spread 1..=10 ms.
        for n in 1..=10 {
            s.record(sample(n));
        }
        let a = s.aggregate(|x| x.total);
        assert_eq!(a.min, ms(1));
        assert_eq!(a.max, ms(10));
        // n=10 → median = sorted[(10-1)/2] = sorted[4] = 5 ms.
        assert_eq!(a.median, ms(5));
        // nearest-rank: p95 → ceil(0.95·10)=10 → idx 9 → 10 ms;
        // p99 → ceil(0.99·10)=10 → idx 9 → 10 ms.
        assert_eq!(a.p95, ms(10));
        assert_eq!(a.p99, ms(10));
        // mean = (1+..+10)/10 = 5.5 ms.
        assert_eq!(a.mean, Duration::from_micros(5_500));
        // A selector projecting a different field still works.
        assert_eq!(s.aggregate(|x| x.view).max, ms(5)); // view = total/2, max at total=10
    }

    #[test]
    fn empty_aggregate_and_fps_are_zero_not_a_panic() {
        let s = PerfSession::with_capacity(4);
        assert_eq!(s.aggregate(|x| x.total), Aggregate::default());
        assert_eq!(s.fps(), 0.0);
        assert!(s.worst(|x| x.total).is_none());
        assert!(s.last().is_none());
    }

    #[test]
    fn fps_and_worst_match_the_recorded_window() {
        let mut s = PerfSession::with_capacity(8);
        // Four frames at 10 ms each → 40 ms total → 100 fps.
        for _ in 0..4 {
            s.record(sample(10));
        }
        assert!((s.fps() - 100.0).abs() < 0.01);
        // Introduce a 50 ms stall; it must be the `worst` by total.
        s.record(sample(50));
        assert_eq!(s.worst(|x| x.total).unwrap().total, ms(50));
        let a = s.aggregate(|x| x.total);
        assert_eq!(a.max, ms(50));
        assert_eq!(a.min, ms(10));
    }

    #[test]
    fn nearest_rank_matches_the_classic_definition() {
        // 100 samples 1..=100 ms: p95 → 95 ms, p99 → 99 ms, p50 (median
        // via (n-1)/2) → sorted[49] = 50 ms.
        let mut s = PerfSession::with_capacity(128);
        for n in 1..=100 {
            s.record(sample(n));
        }
        let a = s.aggregate(|x| x.total);
        assert_eq!(a.p95, ms(95));
        assert_eq!(a.p99, ms(99));
        assert_eq!(a.median, ms(50));
        assert_eq!(a.min, ms(1));
        assert_eq!(a.max, ms(100));
    }
}
