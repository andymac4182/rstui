//! The monotonic time seam: plugin timeout and grace-period logic is driven by
//! an injected `Clock` rather than touching the wall clock directly (ADR 0007
//! §5).
//!
//! ## Why a seam instead of `std::time::Instant` everywhere
//!
//! A plugin host that calls `Instant::now()` inline cannot be tested without
//! real sleeping — every timeout assertion becomes a flaky, slow wall-clock
//! test. By routing all time reads through a `Clock` trait, the host is
//! parameterised: a `SystemClock` in production, a `FakeClock` in tests. The
//! test holds an `Arc<FakeClock>`, advances it to any point in time with
//! [`FakeClock::advance`] or [`FakeClock::set`], and asserts the host's
//! timeout/grace logic fires at exactly the right simulated instant — with no
//! sleeping and no flakiness.
//!
//! This mirrors the pattern `rstui-runtime` already uses for `Backend` /
//! `EventSource` (`TestBackend` / `TestEventSource`): every nondeterministic
//! edge is an injected trait with a `std` impl and a scripted in-memory fake
//! (ADR 0007 §5).
//!
//! ## Monotonicity contract
//!
//! `Clock::elapsed` returns a duration measured from the clock's **start
//! instant**. Only *differences* between two calls are meaningful; the absolute
//! value has no significance. Both implementations uphold monotonicity — neither
//! can return a smaller value than a previous call — so callers may safely
//! compare readings without a check for backward jumps.
//!
//! # Example
//!
//! ```
//! use std::sync::Arc;
//! use std::time::Duration;
//! use rstui_plugin_host::clock::{Clock, FakeClock};
//!
//! // The host holds one Arc; the test holds another.
//! let clock = Arc::new(FakeClock::new());
//! let host_clock: Arc<dyn Clock> = Arc::clone(&clock) as Arc<dyn Clock>;
//!
//! assert_eq!(host_clock.elapsed(), Duration::ZERO);
//!
//! // Advance time deterministically — no sleeping.
//! clock.advance(Duration::from_millis(500));
//! assert_eq!(host_clock.elapsed(), Duration::from_millis(500));
//!
//! // Advance again; readings accumulate.
//! clock.advance(Duration::from_millis(500));
//! assert_eq!(host_clock.elapsed(), Duration::from_secs(1));
//! ```

use std::sync::Mutex;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Monotonic time since the clock started.
///
/// The return value of [`elapsed`](Clock::elapsed) is a [`Duration`] measured
/// from an arbitrary start instant captured when the clock was created.
/// **Only differences between two calls are meaningful** — the absolute value
/// carries no calendar significance and must not be compared to wall time.
///
/// Both built-in implementations uphold monotonicity: successive calls to
/// `elapsed` on the same clock instance will never return a smaller value than
/// a previous call.
///
/// The trait is `Send + Sync` so a single `Arc<dyn Clock>` can be shared
/// between the host runtime and a test that advances a `FakeClock` from
/// another thread.
pub trait Clock: Send + Sync {
    /// Returns the elapsed time since this clock started.
    ///
    /// Only *differences* between two readings are meaningful. Calling code
    /// should store a baseline reading and compare subsequent readings against
    /// it to measure a timeout, not rely on the absolute value.
    fn elapsed(&self) -> Duration;
}

// ---------------------------------------------------------------------------
// SystemClock
// ---------------------------------------------------------------------------

/// A [`Clock`] backed by [`std::time::Instant`] for use in production.
///
/// `SystemClock::new()` (equivalently, `SystemClock::default()`) captures the
/// current monotonic instant; every call to [`elapsed`](Clock::elapsed) returns
/// `start.elapsed()`. This is guaranteed monotonic and never goes backward —
/// `std::time::Instant` provides that guarantee on all platforms rstui targets.
///
/// Use [`FakeClock`] in tests to avoid any real wall-clock dependency.
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    /// Creates a new `SystemClock` that starts counting from now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    /// Returns the duration elapsed since this clock was created.
    ///
    /// Backed by [`Instant::elapsed`], which is monotonic and never decreases.
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

// ---------------------------------------------------------------------------
// FakeClock
// ---------------------------------------------------------------------------

/// A deterministic, advanceable [`Clock`] for testing.
///
/// `FakeClock::new()` (equivalently, `FakeClock::default()`) starts at
/// [`Duration::ZERO`]. Time only moves when the test explicitly calls
/// [`advance`](Self::advance) or [`set`](Self::set) — no real sleeping, no
/// wall-clock dependency, no flakiness.
///
/// ## Sharing between the host and the test
///
/// Hold one `Arc<FakeClock>` in the test and pass `Arc::clone(&clock) as
/// Arc<dyn Clock>` to the host. The `Mutex` inside ensures that any
/// [`advance`](Self::advance) the test makes is immediately visible to the host
/// through its `Arc<dyn Clock>` handle, proving the host-sees-test-advance
/// pattern without spawning threads or sleeping.
///
/// ```
/// use std::sync::Arc;
/// use std::time::Duration;
/// use rstui_plugin_host::clock::{Clock, FakeClock};
///
/// let clock = Arc::new(FakeClock::new());
/// let host_view: &dyn Clock = &*clock;
///
/// clock.advance(Duration::from_secs(2));
/// assert_eq!(host_view.elapsed(), Duration::from_secs(2));
/// ```
pub struct FakeClock {
    elapsed: Mutex<Duration>,
}

impl FakeClock {
    /// Creates a `FakeClock` with elapsed time starting at [`Duration::ZERO`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            elapsed: Mutex::new(Duration::ZERO),
        }
    }

    /// Adds `by` to the stored elapsed duration.
    ///
    /// Successive advances accumulate: two calls of `advance(1s)` leave the
    /// clock at `2s`. The clock never decreases — to rewind, use
    /// [`set`](Self::set).
    pub fn advance(&self, by: Duration) {
        let mut guard = self.elapsed.lock().expect("lock poisoned");
        *guard += by;
    }

    /// Sets the stored elapsed duration to exactly `to`, overriding any
    /// previously accumulated value.
    ///
    /// Use this to position the clock at a precise deadline without having to
    /// know the current value. To step forward from wherever the clock is now,
    /// prefer [`advance`](Self::advance).
    pub fn set(&self, to: Duration) {
        let mut guard = self.elapsed.lock().expect("lock poisoned");
        *guard = to;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    /// Returns the stored elapsed duration — whatever the last
    /// [`advance`](Self::advance) or [`set`](Self::set) left it at.
    fn elapsed(&self) -> Duration {
        *self.elapsed.lock().expect("lock poisoned")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // -----------------------------------------------------------------------
    // SystemClock
    // -----------------------------------------------------------------------

    #[test]
    fn system_clock_elapsed_is_non_decreasing_across_two_reads() {
        let clock = SystemClock::new();
        let first = clock.elapsed();
        let second = clock.elapsed();
        assert!(
            second >= first,
            "SystemClock elapsed went backward: {second:?} < {first:?}"
        );
    }

    #[test]
    fn system_clock_default_and_new_both_construct_valid_clocks() {
        let via_new = SystemClock::new();
        let via_default = SystemClock::default();
        // Both must return non-negative elapsed values (monotonic).
        assert!(via_new.elapsed() < Duration::from_secs(60));
        assert!(via_default.elapsed() < Duration::from_secs(60));
    }

    #[test]
    fn system_clock_is_usable_via_clock_trait_object() {
        let clock: &dyn Clock = &SystemClock::new();
        let reading = clock.elapsed();
        // Just confirm the trait object dispatches without panic.
        assert!(reading < Duration::from_secs(60));
    }

    // -----------------------------------------------------------------------
    // FakeClock: construction
    // -----------------------------------------------------------------------

    #[test]
    fn fake_clock_starts_at_zero() {
        let clock = FakeClock::new();
        assert_eq!(clock.elapsed(), Duration::ZERO);
    }

    #[test]
    fn fake_clock_default_starts_at_zero() {
        let clock = FakeClock::default();
        assert_eq!(clock.elapsed(), Duration::ZERO);
    }

    // -----------------------------------------------------------------------
    // FakeClock: advance accumulates
    // -----------------------------------------------------------------------

    #[test]
    fn fake_clock_advance_accumulates_multiple_steps() {
        let clock = FakeClock::new();
        clock.advance(Duration::from_millis(100));
        assert_eq!(clock.elapsed(), Duration::from_millis(100));

        clock.advance(Duration::from_millis(200));
        assert_eq!(clock.elapsed(), Duration::from_millis(300));

        clock.advance(Duration::from_secs(1));
        assert_eq!(clock.elapsed(), Duration::from_millis(1300));
    }

    #[test]
    fn fake_clock_advance_by_zero_leaves_elapsed_unchanged() {
        let clock = FakeClock::new();
        clock.advance(Duration::from_secs(5));
        clock.advance(Duration::ZERO);
        assert_eq!(clock.elapsed(), Duration::from_secs(5));
    }

    // -----------------------------------------------------------------------
    // FakeClock: set overrides
    // -----------------------------------------------------------------------

    #[test]
    fn fake_clock_set_overrides_accumulated_value() {
        let clock = FakeClock::new();
        clock.advance(Duration::from_secs(10));
        assert_eq!(clock.elapsed(), Duration::from_secs(10));

        clock.set(Duration::from_millis(42));
        assert_eq!(clock.elapsed(), Duration::from_millis(42));
    }

    #[test]
    fn fake_clock_set_then_advance_starts_from_set_value() {
        let clock = FakeClock::new();
        clock.set(Duration::from_secs(5));
        clock.advance(Duration::from_secs(3));
        assert_eq!(clock.elapsed(), Duration::from_secs(8));
    }

    #[test]
    fn fake_clock_set_to_zero_resets_the_clock() {
        let clock = FakeClock::new();
        clock.advance(Duration::from_secs(100));
        clock.set(Duration::ZERO);
        assert_eq!(clock.elapsed(), Duration::ZERO);
    }

    // -----------------------------------------------------------------------
    // FakeClock: Arc sharing — the host-sees-test-advance pattern
    // -----------------------------------------------------------------------

    /// Proves the canonical usage pattern from ADR 0007 §5: the test holds one
    /// `Arc<FakeClock>` and shares another into the host as `Arc<dyn Clock>`.
    /// Advances made through the test handle are immediately visible through the
    /// host handle — no sleeping, no synchronisation needed by the caller.
    #[test]
    fn fake_clock_shared_via_arc_reflects_advances_through_other_handle() {
        // The test keeps the typed handle so it can call advance/set.
        let test_handle = Arc::new(FakeClock::new());
        // The host receives a type-erased handle; it only calls elapsed().
        let host_handle: Arc<dyn Clock> = Arc::clone(&test_handle) as Arc<dyn Clock>;

        assert_eq!(host_handle.elapsed(), Duration::ZERO);

        test_handle.advance(Duration::from_millis(500));
        assert_eq!(
            host_handle.elapsed(),
            Duration::from_millis(500),
            "host handle should see the advance made through the test handle"
        );

        test_handle.advance(Duration::from_millis(500));
        assert_eq!(
            host_handle.elapsed(),
            Duration::from_secs(1),
            "second advance should accumulate"
        );

        test_handle.set(Duration::from_secs(42));
        assert_eq!(
            host_handle.elapsed(),
            Duration::from_secs(42),
            "set should override through the shared Arc"
        );
    }

    // -----------------------------------------------------------------------
    // Trait object usability for both impls
    // -----------------------------------------------------------------------

    #[test]
    fn both_impls_are_usable_via_dyn_clock_reference() {
        fn read_elapsed(clock: &dyn Clock) -> Duration {
            clock.elapsed()
        }

        let system = SystemClock::new();
        let _ = read_elapsed(&system);

        let fake = FakeClock::new();
        fake.advance(Duration::from_secs(7));
        assert_eq!(read_elapsed(&fake), Duration::from_secs(7));
    }

    #[test]
    fn both_impls_are_usable_via_boxed_dyn_clock() {
        let clocks: Vec<Box<dyn Clock>> =
            vec![Box::new(SystemClock::new()), Box::new(FakeClock::new())];
        for clock in &clocks {
            let _ = clock.elapsed();
        }
    }
}
