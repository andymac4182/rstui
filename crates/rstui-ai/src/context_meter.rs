//! [`ContextMeter`] — a token-usage meter: the "context window 12k / 200k"
//! bar with a per-bucket breakdown popover.
//!
//! # A pure projection of [`TokenUsage`] + caller-owned `open`
//!
//! The ai-elements `Context` shows usage against a model's max with a
//! hover-card breakdown (input / output / reasoning / cache). The
//! authoritative usage is the shared
//! [`TokenUsage`]; whether the breakdown is open is
//! ordinary application state (the documented overlay-is-model-state shape).
//! So `ContextMeter` owns nothing: it projects a
//! [`TokenUsage`], a
//! `max` budget, and a caller-owned
//! [`open`](ContextMeter::open) `bool`.
//!
//! The bar itself is a [`Gauge`] (`used / max`,
//! clamped) — we *reuse* the widget, not reinvent it. When
//! [`open`](ContextMeter::open) the host renders the
//! [`Breakdown`](ContextMeter::breakdown) lines (input/output/reasoning/cache)
//! into the rect a [`Popover`](rstui_widgets::Popover)/[`Modal`](rstui_widgets::Modal) places using
//! [`breakdown_size`](ContextMeter::breakdown_size).
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`] totality rule a zero/tiny area, a
//! zero `max` (treated as empty, not a divide-by-zero), and over-budget
//! usage (clamped to a full bar) are all safe — never a panic.

use rstui_core::{Buffer, Color, Position, Rect, Size, Style, Widget};
use rstui_widgets::{Block, Borders, Gauge};

use crate::model::TokenUsage;

/// A token-usage meter — a bar of `used / max` with an optional breakdown
/// popover.
///
/// Projects a [`TokenUsage`], a `max`
/// budget, and a caller-owned [`open`](Self::open) flag. The bar is a
/// [`Gauge`] filled to `used / max` (clamped) and
/// labelled `used / max`. `ContextMeter` owns no state — see the
/// [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::context_meter::ContextMeter;
/// use rstui_ai::model::TokenUsage;
///
/// let usage = TokenUsage {
///     input_tokens: Some(40),
///     output_tokens: Some(60),
///     reasoning_tokens: Some(10),
///     cached_input_tokens: Some(5),
/// };
/// let meter = ContextMeter::new(usage, 1000);
/// assert_eq!(meter.used(), 100); // input + output
/// assert!((meter.fraction() - 0.1).abs() < 1e-9);
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
/// meter.render(buf.area(), &mut buf);
/// // The Gauge's label carries the used / max readout.
/// assert!(buf.get(Position::new(0, 0)).is_some());
/// ```
#[derive(Debug, Clone)]
pub struct ContextMeter {
    usage: TokenUsage,
    max: u64,
    open: bool,
    style: Style,
    gauge_style: Style,
}

impl ContextMeter {
    /// A meter of `usage` against a `max` token budget, breakdown closed.
    #[must_use]
    pub fn new(usage: TokenUsage, max: u64) -> Self {
        Self {
            usage,
            max,
            open: false,
            style: Style::new(),
            gauge_style: Style::new().fg(Color::Cyan),
        }
    }

    /// Sets the caller-owned breakdown-open flag (the reducer flips it; the
    /// widget only reads it — the host renders the breakdown itself).
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the base [`Style`] (the bar track / frame).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the bar fill [`Style`].
    #[must_use]
    pub fn gauge_style(mut self, gauge_style: Style) -> Self {
        self.gauge_style = gauge_style;
        self
    }

    /// The tokens used this turn ([`TokenUsage::total`](crate::model::TokenUsage::total) —
    /// input + output).
    #[must_use]
    pub fn used(&self) -> u64 {
        self.usage.total()
    }

    /// The used fraction of the budget, clamped to `0.0..=1.0` (a zero
    /// `max` is `0.0`, never a divide-by-zero).
    #[must_use]
    pub fn fraction(&self) -> f64 {
        if self.max == 0 {
            return 0.0;
        }
        (self.used() as f64 / self.max as f64).clamp(0.0, 1.0)
    }

    /// The `used / max` readout shown on the bar.
    #[must_use]
    pub fn readout(&self) -> String {
        format!("{} / {}", self.used(), self.max)
    }

    /// The breakdown lines (one per non-`None` counter), the popover body the
    /// host renders when [`open`](Self::open).
    #[must_use]
    pub fn breakdown(&self) -> Vec<String> {
        let mut rows = Vec::new();
        let mut push = |name: &str, value: Option<u64>| {
            if let Some(count) = value {
                rows.push(format!("{name}: {count}"));
            }
        };
        push("input", self.usage.input_tokens);
        push("output", self.usage.output_tokens);
        push("reasoning", self.usage.reasoning_tokens);
        push("cache", self.usage.cached_input_tokens);
        rows
    }

    /// The box the breakdown popover needs (widest line + a border), for a
    /// [`Popover`](rstui_widgets::Popover)/[`Modal`](rstui_widgets::Modal) placement.
    #[must_use]
    pub fn breakdown_size(&self) -> Size {
        let rows = self.breakdown();
        let widest = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        Size::new(
            (widest as u16).saturating_add(2).max(8),
            (rows.len() as u16).max(1).saturating_add(2),
        )
    }

    /// Renders the breakdown popover body into `area` (the host calls this
    /// when [`open`](Self::open), into the placed rect). A bordered list of
    /// the [`breakdown`](Self::breakdown) lines.
    pub fn render_breakdown(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let block = Block::new().borders(Borders::ALL).style(self.style);
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.is_empty() {
            return;
        }
        for (row, line) in self.breakdown().iter().enumerate() {
            if row as u16 >= inner.height {
                break;
            }
            let y = inner.top().saturating_add(row as u16);
            let mut x = inner.left();
            for ch in line.chars() {
                if x >= inner.right() {
                    break;
                }
                buf.set_cell(Position::new(x, y), ch, self.style);
                x = x.saturating_add(1);
            }
        }
    }
}

impl Widget for ContextMeter {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        Gauge::default()
            .ratio(self.fraction())
            .label(self.readout())
            .style(self.style)
            .gauge_style(self.gauge_style)
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage() -> TokenUsage {
        TokenUsage {
            input_tokens: Some(40),
            output_tokens: Some(60),
            reasoning_tokens: Some(10),
            cached_input_tokens: Some(5),
        }
    }

    #[test]
    fn used_is_input_plus_output_and_fraction_is_clamped() {
        let m = ContextMeter::new(usage(), 1000);
        assert_eq!(m.used(), 100);
        assert!((m.fraction() - 0.1).abs() < 1e-9);
        // Over budget → clamped to a full bar, not >1.
        let over = ContextMeter::new(usage(), 50);
        assert!((over.fraction() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_zero_max_is_empty_not_a_divide_by_zero() {
        assert_eq!(ContextMeter::new(usage(), 0).fraction(), 0.0);
    }

    #[test]
    fn the_readout_is_used_over_max() {
        assert_eq!(ContextMeter::new(usage(), 200).readout(), "100 / 200");
    }

    #[test]
    fn the_breakdown_lists_only_present_counters() {
        let partial = TokenUsage {
            input_tokens: Some(7),
            output_tokens: None,
            reasoning_tokens: None,
            cached_input_tokens: Some(3),
        };
        let m = ContextMeter::new(partial, 100);
        assert_eq!(m.breakdown(), vec!["input: 7", "cache: 3"]);
        // Full usage → all four lines.
        assert_eq!(
            ContextMeter::new(usage(), 100).breakdown(),
            vec!["input: 40", "output: 60", "reasoning: 10", "cache: 5"]
        );
    }

    #[test]
    fn the_bar_renders_the_readout_label() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
        ContextMeter::new(usage(), 200).render(buf.area(), &mut buf);
        let row: String = (0..12)
            .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
            .collect();
        // The Gauge centres the "100 / 200" label in the bar.
        assert!(row.contains("100 / 200"), "got {row:?}");
    }

    #[test]
    fn the_breakdown_popover_is_a_bordered_list() {
        let m = ContextMeter::new(usage(), 100);
        let size = m.breakdown_size();
        let mut buf = Buffer::empty(Rect::new(0, 0, size.width, size.height));
        m.render_breakdown(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'i'); // input:
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        ContextMeter::new(usage(), 100).render(Rect::new(0, 0, 0, 0), &mut buf);
        ContextMeter::new(usage(), 100).render_breakdown(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
