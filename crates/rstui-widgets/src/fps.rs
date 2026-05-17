//! [`FpsCounter`] — a live render-rate readout, so any app can make its
//! frame performance visible with one line.
//!
//! # A pure projection of a caller-owned [`FpsMeter`]
//!
//! [`Spinner`](crate::Spinner) is a pure projection of a caller-owned
//! animation *tick*; `FpsCounter` is a pure projection of a caller-owned
//! *render-rate meter*. The widget holds no state and never reads a clock at
//! render: it borrows an [`FpsMeter`] the app owns (a model field, exactly
//! like `ScrollState` or a [`Spinner`](crate::Spinner) tick) and displays
//! its current reading.
//!
//! [`FpsMeter`] *is* allowed to sample the wall clock — it is caller-owned
//! state, and sampling it once per painted frame is the interior-mutable
//! caller-owned-state pattern ADR 0012 §P1 blesses (the same one the
//! kitchen-sink uses to record geometry/selection during `view`). For a
//! true one-liner, [`FpsCounter::render`] samples the borrowed meter for
//! you, so the whole feature is:
//!
//! ```
//! use rstui_widgets::{FpsCounter, FpsMeter};
//! use rstui_core::{Buffer, Rect, Widget};
//!
//! struct App { fps: FpsMeter }                 // own one in your model
//! # let app = App { fps: FpsMeter::new() };
//! # let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
//! // …then once per frame, in view:
//! FpsCounter::new(&app.fps).render(Rect::new(0, 0, 12, 1), &mut buf);
//! ```
//!
//! # Honest under test
//!
//! A widget demo is a deterministic `TestBackend` snapshot test, and the
//! synchronous `Harness` drives frames with no real delay. A naive
//! wall-clock rate would make every such snapshot nondeterministic.
//! [`FpsMeter`] therefore ignores sub-4ms gaps (the synchronous-loop
//! signature) and reports the fixed placeholder `"--- fps"` until it has a
//! real cadence — so the same code shows a true rate live and a stable
//! string in tests.

use rstui_core::{Buffer, Position, Rect, Style, Widget};
use std::cell::Cell;
use std::time::Instant;

/// Caller-owned render-rate state: an exponential moving average of the
/// interval between painted frames.
///
/// Keep one in your model and let [`FpsCounter`] sample and display it, or
/// call [`record`](Self::record) yourself once per frame and read
/// [`fps`](Self::fps) / [`label`](Self::label) directly. Interior mutability
/// (`Cell`) so it can be sampled through the `&self` of a pure `view` — the
/// ADR 0012 §P1 caller-owned-state pattern, never a render-time animation
/// the widget drives itself.
#[derive(Debug, Default)]
pub struct FpsMeter {
    /// When the previous frame was recorded.
    last: Cell<Option<Instant>>,
    /// EMA of the inter-frame interval, in seconds (`0.0` = no usable sample).
    ema: Cell<f32>,
}

impl FpsMeter {
    /// A meter with no samples yet (renders the `"--- fps"` placeholder).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that a frame was painted *now*. Call exactly once per frame
    /// ([`FpsCounter`] does this for you). Sub-4ms gaps are treated as the
    /// synchronous test loop and ignored, so snapshots stay deterministic
    /// and a near-zero interval can't spike the rate.
    pub fn record(&self) {
        let now = Instant::now();
        if let Some(prev) = self.last.get() {
            let dt = now.duration_since(prev).as_secs_f32();
            if dt >= 0.004 {
                let cur = self.ema.get();
                // 0.2 smoothing: responsive without jitter.
                self.ema.set(if cur == 0.0 {
                    dt
                } else {
                    cur + 0.2 * (dt - cur)
                });
            }
        }
        self.last.set(Some(now));
    }

    /// The current rate in frames per second, or `None` until there is a
    /// usable live sample (e.g. under the synchronous test harness).
    #[must_use]
    pub fn fps(&self) -> Option<f32> {
        let ema = self.ema.get();
        (ema > 0.0).then(|| 1.0 / ema)
    }

    /// A fixed-width `"NNN fps"` label, or `"--- fps"` before the first
    /// usable sample. ASCII-only, so it is safe in terminal captures and
    /// snapshot tests.
    #[must_use]
    pub fn label(&self) -> String {
        match self.fps() {
            Some(f) => {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = f.round().clamp(0.0, 999.0) as u32;
                format!("{n:>3} fps")
            }
            None => "--- fps".to_owned(),
        }
    }
}

/// A one-line widget that samples and displays a borrowed [`FpsMeter`].
///
/// A pure projection: it owns nothing, mutates only the caller's meter
/// (sampling it, the §P1 pattern) and the [`Buffer`]. Place it anywhere a
/// status line has room — it writes a single left-aligned run and clips to
/// `area` (total: a no-op at zero size, never a panic).
#[derive(Debug)]
pub struct FpsCounter<'a> {
    meter: &'a FpsMeter,
    style: Style,
    prefix: &'a str,
}

impl<'a> FpsCounter<'a> {
    /// A counter projecting `meter`, default style, no prefix.
    #[must_use]
    pub fn new(meter: &'a FpsMeter) -> Self {
        Self {
            meter,
            style: Style::new(),
            prefix: "",
        }
    }

    /// Sets the text style of the readout.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets a literal prefix drawn before the rate (e.g. `"⟳ "`), for a
    /// labelled readout without a separate [`Layout`](rstui_core::Layout)
    /// split.
    #[must_use]
    pub fn prefix(mut self, prefix: &'a str) -> Self {
        self.prefix = prefix;
        self
    }
}

impl Widget for FpsCounter<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Sampling the caller's meter is the once-per-frame record; doing it
        // here makes the widget a true one-liner. A no-op at zero size — the
        // "a pure projection must be total" rule (no panic, ever).
        self.meter.record();
        if area.is_empty() {
            return;
        }
        let text = format!("{}{}", self.prefix, self.meter.label());
        buf.set_str(Position::new(area.x, area.y), &text, self.style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_until_a_real_cadence_then_a_rate() {
        let m = FpsMeter::new();
        assert_eq!(m.fps(), None);
        assert_eq!(m.label(), "--- fps");
        // Two sub-4ms samples (the synchronous-test signature) stay None,
        // so a single-shot snapshot is deterministic.
        m.record();
        m.record();
        assert_eq!(m.label(), "--- fps");
        // A synthesized real cadence reads back as a plausible rate.
        m.ema.set(1.0 / 60.0);
        assert_eq!(m.fps().map(f32::round), Some(60.0));
        assert_eq!(m.label(), " 60 fps");
    }

    #[test]
    fn renders_into_a_buffer_and_is_total() {
        let m = FpsMeter::new();
        // Zero-size: a total no-op, no panic.
        let mut empty = Buffer::empty(Rect::new(0, 0, 0, 0));
        FpsCounter::new(&m).render(Rect::new(0, 0, 0, 0), &mut empty);

        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
        FpsCounter::new(&m)
            .prefix("⟳ ")
            .render(Rect::new(0, 0, 12, 1), &mut buf);
        let row: String = (0..12)
            .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
            .collect();
        assert!(row.starts_with("⟳ --- fps"), "got {row:?}");
    }
}
