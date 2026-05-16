//! The timing harness: warmup, measure, summarize, format.
//!
//! Deliberately tiny and dependency-free (ADR 0005). It is *not* a
//! statistical benchmarking framework: no outlier rejection, no confidence
//! intervals, no regression database. It runs an operation a fixed number of
//! times and reports `min` / `median` / `mean` per iteration, which is enough
//! to eyeball an order-of-magnitude regression in a hot path and is
//! byte-for-byte reproducible across machines for the *shape* of the result.
//! When real statistical rigor is needed, ADR 0005 records the criterion
//! escape hatch and the conditions for taking it.

use std::hint::black_box;
use std::time::Instant;

/// Benchmark loop configuration.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bench {
    /// Untimed iterations run before measurement so the measured region runs
    /// at steady state (warm instruction/data caches, trained branch
    /// predictors, lazily-faulted pages already resident).
    pub(crate) warmup: u32,
    /// Measured iterations. Each is timed individually with [`Instant`] so a
    /// single scheduler hiccup inflates one sample, not the whole run — which
    /// is why `min` is the most stable cross-machine signal.
    pub(crate) iters: u32,
}

impl Bench {
    /// Times `op` for [`Bench::iters`] measured iterations after
    /// [`Bench::warmup`] untimed ones, returning the per-iteration [`Stats`].
    ///
    /// `op`'s output is forced through [`std::hint::black_box`] *after* the
    /// elapsed time is read, so the optimizer cannot delete the work being
    /// measured yet the black-box itself stays outside the timed region. `op`
    /// must contain only the hot region; per-scenario setup (allocating
    /// buffers, building layouts) belongs *outside* the closure — see the
    /// `scenarios` module.
    pub(crate) fn run<T>(&self, mut op: impl FnMut() -> T) -> Stats {
        for _ in 0..self.warmup {
            let _ = black_box(&op());
        }
        let mut nanos: Vec<u64> = Vec::with_capacity(self.iters as usize);
        for _ in 0..self.iters {
            let start = Instant::now();
            let out = op();
            let elapsed = start.elapsed();
            let _ = black_box(&out);
            nanos.push(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX));
        }
        Stats::from_nanos(nanos)
    }
}

/// Per-iteration timing summary, in nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stats {
    /// Number of measured iterations folded into this summary.
    pub(crate) samples: u32,
    /// Fastest single iteration: the sample least polluted by scheduler
    /// noise, so the most stable number to compare across machines and runs
    /// when spotting a regression.
    pub(crate) min_ns: u64,
    /// Median iteration (the lower-middle sample for an even count).
    pub(crate) median_ns: u64,
    /// Arithmetic mean across every measured iteration.
    pub(crate) mean_ns: u64,
}

impl Stats {
    /// Summarize raw per-iteration nanosecond samples.
    ///
    /// # Panics
    ///
    /// Panics if `samples` is empty; [`Bench::run`] always supplies at least
    /// one, so an empty slice is a harness bug, not a runtime input.
    fn from_nanos(mut samples: Vec<u64>) -> Self {
        assert!(
            !samples.is_empty(),
            "a benchmark must run at least one measured iteration"
        );
        samples.sort_unstable();
        let n = samples.len();
        let sum: u128 = samples.iter().map(|&v| u128::from(v)).sum();
        Self {
            samples: u32::try_from(n).unwrap_or(u32::MAX),
            min_ns: samples[0],
            median_ns: samples[n / 2],
            mean_ns: u64::try_from(sum / n as u128).unwrap_or(u64::MAX),
        }
    }
}

/// Format `ns` nanoseconds with an auto-scaled unit and two decimals, sized
/// for eyeballing a regression at a glance (`12.30µs`, `1.05ms`, `2.00s`).
pub(crate) fn humanize(ns: u64) -> String {
    let value = ns as f64;
    if value < 1_000.0 {
        format!("{value:.2}ns")
    } else if value < 1_000_000.0 {
        format!("{:.2}µs", value / 1_000.0)
    } else if value < 1_000_000_000.0 {
        format!("{:.2}ms", value / 1_000_000.0)
    } else {
        format!("{:.2}s", value / 1_000_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn run_executes_exactly_warmup_plus_iters_times() {
        let calls = Cell::new(0_u32);
        let bench = Bench {
            warmup: 5,
            iters: 17,
        };
        let _ = bench.run(|| calls.set(calls.get() + 1));
        assert_eq!(
            calls.get(),
            22,
            "warmup and measured iterations must both invoke the closure"
        );
    }

    #[test]
    fn stats_reports_min_median_mean_and_sample_count() {
        let stats = Stats::from_nanos(vec![30, 10, 20, 50, 40]);
        assert_eq!(stats.samples, 5);
        assert_eq!(stats.min_ns, 10);
        assert_eq!(stats.median_ns, 30, "median is the lower-middle sample");
        assert_eq!(stats.mean_ns, 30);
        assert!(stats.min_ns <= stats.median_ns);
    }

    #[test]
    fn humanize_auto_scales_the_unit() {
        assert_eq!(humanize(900), "900.00ns");
        assert_eq!(humanize(12_300), "12.30µs");
        assert_eq!(humanize(1_050_000), "1.05ms");
        assert_eq!(humanize(2_000_000_000), "2.00s");
    }
}
