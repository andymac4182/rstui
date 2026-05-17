//! [`TestResults`] — a test-run summary: the pass/fail/skip headline plus a
//! collapsible suite/test breakdown an agent's test tool projects.
//!
//! # A pure projection of caller-owned summary + suites + `open`
//!
//! The ai-elements `TestResults` shows counts, a progress bar, and an
//! expandable per-suite list. The counts/suites are the caller's data and
//! whether the breakdown is open is ordinary application state (the
//! [`Accordion`](rstui_widgets::Accordion) `expanded` precedent). So
//! `TestResults` owns nothing: it projects a caller-owned [`Summary`] +
//! `&[TestSuite]` and a caller-owned [`open`](TestResults::open) `bool`.
//!
//! The progress bar is a [`Gauge`]
//! (passed / total) — we *reuse* the widget. The host hit-tests
//! [`header_rect`](TestResults::header_rect) to toggle the breakdown (no
//! callback, the documented seam).
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`] totality rule a zero/tiny area, a
//! zero-total summary (an empty bar, not a divide-by-zero), and over-many
//! rows are all safe clips — never a panic.

use rstui_core::{Buffer, Color, Constraint, Layout, Position, Rect, Style, Widget};
use rstui_widgets::Gauge;

/// The headline counts of a test run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    /// Tests that passed.
    pub passed: u32,
    /// Tests that failed.
    pub failed: u32,
    /// Tests that were skipped.
    pub skipped: u32,
    /// The total test count.
    pub total: u32,
}

impl Summary {
    /// A summary of `passed`/`failed`/`skipped` out of `total`.
    #[must_use]
    pub fn new(passed: u32, failed: u32, skipped: u32, total: u32) -> Self {
        Self {
            passed,
            failed,
            skipped,
            total,
        }
    }

    /// The passed fraction of the total, clamped to `0.0..=1.0` (a zero
    /// total is `0.0`, never a divide-by-zero).
    #[must_use]
    pub fn fraction(self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (f64::from(self.passed) / f64::from(self.total)).clamp(0.0, 1.0)
    }

    /// `true` when nothing failed (an all-green run).
    #[must_use]
    pub fn is_green(self) -> bool {
        self.failed == 0
    }
}

/// The outcome of a single test, selecting its glyph.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    /// Passed (`✓`) — the default.
    #[default]
    Passed,
    /// Failed (`✗`).
    Failed,
    /// Skipped (`○`).
    Skipped,
}

impl TestStatus {
    /// The glyph prefixing a test of this status.
    #[must_use]
    pub fn glyph(self) -> char {
        match self {
            Self::Passed => '✓',
            Self::Failed => '✗',
            Self::Skipped => '○',
        }
    }
}

/// One test within a [`TestSuite`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCase {
    /// The test name.
    pub name: String,
    /// Its outcome.
    pub status: TestStatus,
}

impl TestCase {
    /// A test `name` with `status`.
    pub fn new(name: impl Into<String>, status: TestStatus) -> Self {
        Self {
            name: name.into(),
            status,
        }
    }
}

/// A named group of [`TestCase`]s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestSuite {
    /// The suite name.
    pub name: String,
    /// Its tests.
    pub tests: Vec<TestCase>,
}

impl TestSuite {
    /// A suite `name` with `tests`.
    pub fn new(name: impl Into<String>, tests: Vec<TestCase>) -> Self {
        Self {
            name: name.into(),
            tests,
        }
    }
}

/// A test-run summary with a collapsible suite/test breakdown.
///
/// Row 0 is the headline (`✓N ✗N ○N`), row 1 a
/// [`Gauge`] of `passed / total`. When
/// [`open`](Self::open) the suites follow, each suite name then its tests
/// (`<glyph> name`). `TestResults` owns no state — see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::test_results::{Summary, TestCase, TestResults, TestStatus, TestSuite};
///
/// let summary = Summary::new(2, 1, 0, 3);
/// let suites = [TestSuite::new("parser", vec![
///     TestCase::new("ok", TestStatus::Passed),
///     TestCase::new("bad", TestStatus::Failed),
/// ])];
/// let widget = TestResults::new(summary, &suites).open(true);
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 5));
/// widget.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '✓'); // headline
/// ```
#[derive(Debug, Clone)]
pub struct TestResults<'a> {
    summary: Summary,
    suites: &'a [TestSuite],
    open: bool,
    style: Style,
}

impl<'a> TestResults<'a> {
    /// A summary card for `summary` with its `suites`, breakdown collapsed.
    #[must_use]
    pub fn new(summary: Summary, suites: &'a [TestSuite]) -> Self {
        Self {
            summary,
            suites,
            open: false,
            style: Style::new(),
        }
    }

    /// Sets the caller-owned open flag (the reducer flips it on a
    /// [`header_rect`](Self::header_rect) click; the widget only reads it).
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the base [`Style`].
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The headline row [`Rect`] — the host hit-tests a click here to toggle
    /// [`open`](Self::open).
    #[must_use]
    pub fn header_rect(&self, area: Rect) -> Rect {
        Rect::new(area.left(), area.top(), area.width, area.height.min(1))
    }

    /// The headline string (`✓P ✗F ○S`).
    fn headline(&self) -> String {
        format!(
            "✓{} ✗{} ○{}",
            self.summary.passed, self.summary.failed, self.summary.skipped
        )
    }
}

impl Widget for TestResults<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        buf.set_style(area, self.style);
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

        // The headline.
        let mut x = rows[0].left();
        for ch in self.headline().chars() {
            if x >= rows[0].right() {
                break;
            }
            buf.set_cell(Position::new(x, rows[0].top()), ch, self.style);
            x = x.saturating_add(1);
        }

        // The progress bar.
        if !rows[1].is_empty() {
            let bar_fg = if self.summary.is_green() {
                Color::Green
            } else {
                Color::Red
            };
            Gauge::default()
                .ratio(self.summary.fraction())
                .label("")
                .style(self.style)
                .gauge_style(Style::new().fg(bar_fg))
                .render(rows[1], buf);
        }

        // The collapsible breakdown.
        if !self.open || rows[2].is_empty() {
            return;
        }
        let body = rows[2];
        let mut row = 0u16;
        'outer: for suite in self.suites {
            if row >= body.height {
                break;
            }
            let sy = body.top().saturating_add(row);
            let mut sx = body.left();
            for ch in suite.name.chars() {
                if sx >= body.right() {
                    break;
                }
                buf.set_cell(
                    Position::new(sx, sy),
                    ch,
                    self.style.add_modifier(rstui_core::Modifier::BOLD),
                );
                sx = sx.saturating_add(1);
            }
            row = row.saturating_add(1);
            for test in &suite.tests {
                if row >= body.height {
                    break 'outer;
                }
                let ty = body.top().saturating_add(row);
                let line = format!("  {} {}", test.status.glyph(), test.name);
                let mut tx = body.left();
                for ch in line.chars() {
                    if tx >= body.right() {
                        break;
                    }
                    buf.set_cell(Position::new(tx, ty), ch, self.style);
                    tx = tx.saturating_add(1);
                }
                row = row.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suites() -> Vec<TestSuite> {
        vec![TestSuite::new(
            "parser",
            vec![
                TestCase::new("ok", TestStatus::Passed),
                TestCase::new("bad", TestStatus::Failed),
            ],
        )]
    }

    fn lines(widget: TestResults<'_>, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_headline_shows_the_three_counts() {
        let s = suites();
        let out = lines(TestResults::new(Summary::new(2, 1, 3, 6), &s), 12, 2);
        assert!(out.starts_with("✓2 ✗1 ○3"), "got {out:?}");
    }

    #[test]
    fn the_fraction_is_passed_over_total_clamped() {
        assert!((Summary::new(3, 0, 0, 4).fraction() - 0.75).abs() < 1e-9);
        // A zero total is empty, not a divide-by-zero.
        assert_eq!(Summary::new(0, 0, 0, 0).fraction(), 0.0);
        assert!(Summary::new(5, 0, 0, 5).is_green());
        assert!(!Summary::new(4, 1, 0, 5).is_green());
    }

    #[test]
    fn the_bar_fills_to_the_pass_ratio() {
        let s = suites();
        // 2/4 passed → ~half the 8-col bar filled on row 1.
        let out = lines(TestResults::new(Summary::new(2, 0, 0, 4), &s), 8, 2);
        let bar = out.lines().nth(1).unwrap();
        assert!(bar.starts_with("████"), "got {bar:?}");
    }

    #[test]
    fn a_closed_breakdown_hides_the_suites() {
        let s = suites();
        let out = lines(TestResults::new(Summary::new(1, 1, 0, 2), &s), 16, 5);
        assert!(!out.contains("parser"), "got {out:?}");
    }

    #[test]
    fn an_open_breakdown_lists_suites_and_tests() {
        let s = suites();
        let out = lines(
            TestResults::new(Summary::new(1, 1, 0, 2), &s).open(true),
            16,
            5,
        );
        assert!(out.contains("parser"), "got {out:?}");
        assert!(out.contains("✓ ok"), "got {out:?}");
        assert!(out.contains("✗ bad"), "got {out:?}");
    }

    #[test]
    fn header_rect_is_the_headline_row() {
        let s = suites();
        let area = Rect::new(0, 0, 16, 5);
        assert_eq!(
            TestResults::new(Summary::default(), &s).header_rect(area),
            Rect::new(0, 0, 16, 1)
        );
    }

    #[test]
    fn test_status_glyphs_are_distinct() {
        assert_eq!(TestStatus::Passed.glyph(), '✓');
        assert_eq!(TestStatus::Failed.glyph(), '✗');
        assert_eq!(TestStatus::Skipped.glyph(), '○');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let s = suites();
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        TestResults::new(Summary::default(), &s).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
