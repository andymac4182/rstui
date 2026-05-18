//! [`CountingAllocator`] — a `#[global_allocator]`-installable shim over
//! the system allocator that counts allocations, deallocations, and
//! live/peak/total bytes with relaxed atomics (ADR 0018 §2).
//!
//! This is the **only** module in `rstui-devtools` that uses `unsafe`: a
//! single `#[allow(unsafe_code)]` on the `GlobalAlloc` impl, which is a
//! thin pass-through to [`std::alloc::System`] bracketed by counters. The
//! workspace forbids `unsafe_code` and no `#[allow]` lifts a `forbid`,
//! which is why this lives in an opt-in leaf crate (ADR 0018 §1) rather
//! than in `rstui-core`.
//!
//! # Install (one line, in your binary)
//!
//! ```ignore
//! #[global_allocator]
//! static GLOBAL: rstui_devtools::alloc::CountingAllocator =
//!     rstui_devtools::alloc::CountingAllocator::system();
//! ```
//!
//! Counting is process-global (a `#[global_allocator]` is): the counters
//! reflect *every* allocation in the process. Use [`snapshot`] and
//! [`AllocSnapshot::delta`] to attribute a span of work; [`reset_peak`] to
//! re-baseline the peak for a fresh measurement window. [`snapshot`] only
//! reads atomics — it never allocates — so it is safe inside a hot frame
//! observer.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const REL: Ordering = Ordering::Relaxed;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);
static TOTAL_ALLOCS: AtomicU64 = AtomicU64::new(0);
static TOTAL_DEALLOCS: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES: AtomicU64 = AtomicU64::new(0);

/// Bumps the live-bytes counter and best-effort-tracks the peak. Shared by
/// `alloc`/`alloc_zeroed`/the growth side of `realloc`.
fn note_alloc(size: usize) {
    TOTAL_ALLOCS.fetch_add(1, REL);
    TOTAL_BYTES.fetch_add(size as u64, REL);
    let live = LIVE_BYTES.fetch_add(size, REL) + size;
    PEAK_BYTES.fetch_max(live, REL);
}

/// A counting global allocator: a pass-through to the system allocator
/// with atomic instrumentation (ADR 0018 §2).
///
/// `alloc`/`alloc_zeroed`/`dealloc` each adjust the counters by the
/// `Layout` size; `realloc` adjusts live/peak/total by the *growth* (or
/// shrink) and does **not** count as a separate alloc+dealloc pair — a
/// resize is one event, so `total_allocs`/`total_deallocs` stay an honest
/// count of distinct allocation/free calls.
#[derive(Debug, Clone, Copy, Default)]
pub struct CountingAllocator {
    _private: (),
}

impl CountingAllocator {
    /// A counting allocator wrapping [`std::alloc::System`]. `const` so it
    /// can initialise a `#[global_allocator]` static.
    #[must_use]
    pub const fn system() -> Self {
        Self { _private: () }
    }
}

// ADR 0018 §2: the single audited `unsafe` surface in this crate. Each
// method is a pass-through to `System` (whose own contract is upheld by
// the caller of a `GlobalAlloc`, exactly as for `System` itself) bracketed
// by counter updates that cannot themselves allocate or panic.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            note_alloc(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            note_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        TOTAL_DEALLOCS.fetch_add(1, REL);
        LIVE_BYTES.fetch_sub(layout.size(), REL);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            let old = layout.size();
            if new_size >= old {
                let grew = new_size - old;
                TOTAL_BYTES.fetch_add(grew as u64, REL);
                let live = LIVE_BYTES.fetch_add(grew, REL) + grew;
                PEAK_BYTES.fetch_max(live, REL);
            } else {
                LIVE_BYTES.fetch_sub(old - new_size, REL);
            }
        }
        new_ptr
    }
}

/// An immutable read of the process-global allocation counters at one
/// instant. Cheap (five relaxed atomic loads); never allocates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AllocSnapshot {
    /// Bytes currently outstanding (alloc'd minus freed), by `Layout` size.
    pub live_bytes: usize,
    /// The high-water mark of `live_bytes` since process start (or the last
    /// [`reset_peak`]).
    pub peak_bytes: usize,
    /// Distinct allocation calls (`alloc` + `alloc_zeroed`) since start.
    pub total_allocs: u64,
    /// Distinct deallocation calls since start.
    pub total_deallocs: u64,
    /// Cumulative bytes ever requested (never decreases) — the allocation
    /// *throughput*, distinct from `live_bytes`.
    pub total_bytes: u64,
}

impl AllocSnapshot {
    /// The allocation activity that happened between `earlier` and `self`
    /// (assumes `self` was taken no earlier than `earlier`). Counter
    /// differences saturate at zero so a snapshot pair taken across a
    /// [`reset_peak`] never underflows.
    #[must_use]
    pub fn delta(&self, earlier: &AllocSnapshot) -> AllocDelta {
        AllocDelta {
            bytes: self.total_bytes.saturating_sub(earlier.total_bytes),
            allocs: self.total_allocs.saturating_sub(earlier.total_allocs),
            deallocs: self.total_deallocs.saturating_sub(earlier.total_deallocs),
            net_live: i64::try_from(self.live_bytes).unwrap_or(i64::MAX)
                - i64::try_from(earlier.live_bytes).unwrap_or(i64::MAX),
        }
    }
}

/// The allocation activity over a span — the per-frame signal a
/// [`PerfSession`](crate::PerfSession) records.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AllocDelta {
    /// Bytes requested during the span (allocation throughput).
    pub bytes: u64,
    /// Allocation calls during the span.
    pub allocs: u64,
    /// Deallocation calls during the span.
    pub deallocs: u64,
    /// Change in outstanding bytes (`+` grew, `−` shrank). A span that
    /// ends with `net_live > 0` and `allocs > deallocs` every time is the
    /// signature of a per-frame leak.
    pub net_live: i64,
}

/// Reads the process-global allocation counters. Never allocates.
#[must_use]
pub fn snapshot() -> AllocSnapshot {
    AllocSnapshot {
        live_bytes: LIVE_BYTES.load(REL),
        peak_bytes: PEAK_BYTES.load(REL),
        total_allocs: TOTAL_ALLOCS.load(REL),
        total_deallocs: TOTAL_DEALLOCS.load(REL),
        total_bytes: TOTAL_BYTES.load(REL),
    }
}

/// Re-baselines the peak to the current live bytes, so a subsequent
/// `snapshot().peak_bytes` measures only the next window's high-water mark.
pub fn reset_peak() {
    PEAK_BYTES.store(LIVE_BYTES.load(REL), REL);
}

#[cfg(test)]
mod tests {
    use super::*;

    // `delta` arithmetic is pure — no globals, fully deterministic.
    #[test]
    fn delta_is_saturating_and_signed_exactly() {
        let a = AllocSnapshot {
            live_bytes: 100,
            peak_bytes: 200,
            total_allocs: 10,
            total_deallocs: 4,
            total_bytes: 1_000,
        };
        let b = AllocSnapshot {
            live_bytes: 60,
            peak_bytes: 250,
            total_allocs: 17,
            total_deallocs: 9,
            total_bytes: 1_512,
        };
        let d = b.delta(&a);
        assert_eq!(d.bytes, 512);
        assert_eq!(d.allocs, 7);
        assert_eq!(d.deallocs, 5);
        assert_eq!(d.net_live, -40);
        // Reversed (earlier > later) saturates the unsigned counters at 0
        // and signs the live delta the other way.
        let r = a.delta(&b);
        assert_eq!(r.bytes, 0);
        assert_eq!(r.allocs, 0);
        assert_eq!(r.deallocs, 0);
        assert_eq!(r.net_live, 40);
    }

    // The counters are process-global statics and `cargo test` is
    // multi-threaded, so other tests allocate against the same atomics
    // between our two reads. We therefore assert *lower bounds* (which are
    // deterministic regardless of concurrent noise): a real alloc moves
    // each counter by at least the layout, in the right direction.
    // Exercises the `GlobalAlloc` methods directly (no global install), so
    // the test itself dips into `unsafe` — scoped allow, same as the impl.
    #[test]
    #[allow(unsafe_code)]
    fn direct_alloc_dealloc_moves_counters_monotonically() {
        let a = CountingAllocator::system();
        let layout = Layout::from_size_align(4096, 8).unwrap();

        let before = snapshot();
        // SAFETY (test-local, mirrors `Vec`'s own usage): `layout` is
        // non-zero-size and well-formed; the pointer is freed below with
        // the same layout and never read.
        let ptr = unsafe { a.alloc(layout) };
        assert!(!ptr.is_null(), "system alloc failed");
        let after_alloc = snapshot();
        assert!(after_alloc.total_allocs > before.total_allocs);
        assert!(after_alloc.total_bytes >= before.total_bytes + 4096);
        assert!(after_alloc.peak_bytes >= after_alloc.live_bytes);

        unsafe { a.dealloc(ptr, layout) };
        let after_free = snapshot();
        assert!(after_free.total_deallocs > after_alloc.total_deallocs);
        // The delta over the whole span counted at least our 4096 bytes
        // and one alloc + one dealloc.
        let d = after_free.delta(&before);
        assert!(d.bytes >= 4096);
        assert!(d.allocs >= 1);
        assert!(d.deallocs >= 1);
    }

    #[test]
    fn snapshot_is_cheap_and_peak_never_below_live() {
        let s = snapshot();
        assert!(s.peak_bytes >= s.live_bytes);
        reset_peak();
        let s2 = snapshot();
        assert!(s2.peak_bytes >= s2.live_bytes);
    }
}
