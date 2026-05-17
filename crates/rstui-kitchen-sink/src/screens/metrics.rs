//! A metrics explorer: a multi-series latency [`LineChart`] (`p50`/`p95`/`p99`
//! over time), a latency-distribution [`Histogram`] with `p50`/`p95`/`p99`
//! [`Percentile`] markers, and a latency-over-time [`Heatmap`]. The series
//! animate from `tick`. `←/→` (or `Tab`) changes the time range, `↑/↓`
//! selects the highlighted percentile series, `Enter` exports.

use rstui_core::{Constraint, KeyCode, Layout, Line, Rect, Style};
use rstui_runtime::Frame;
use rstui_widgets::{
    AxisBounds, Block, BorderType, Heatmap, Histogram, HistogramBucket, LineChart, Percentile,
    Series,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The percentile series, in legend order.
const SERIES: [&str; 3] = ["p50", "p95", "p99"];

/// The selectable time ranges.
const RANGES: [&str; 4] = ["5 min", "1 hour", "6 hours", "24 hours"];

/// The latency-distribution bucket boundaries (ms), shared by the histogram.
const BUCKETS: [&str; 8] = ["10", "25", "50", "75", "100", "150", "250", "500"];

/// The metrics explorer's caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    /// The highlighted percentile series.
    series: usize,
    /// The selected time range.
    range: usize,
}

impl State {
    /// p99 highlighted, the one-hour range.
    pub(crate) fn new() -> Self {
        Self {
            series: 2,
            range: 1,
        }
    }

    /// `↑/↓` pick a series, `←/→`/`Tab` change range, `Enter` exports.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Up => self.series = self.series.saturating_sub(1),
            KeyCode::Down => self.series = (self.series + 1).min(SERIES.len() - 1),
            KeyCode::Left => {
                if self.range == 0 {
                    return ScreenOutcome::ignored();
                }
                self.range -= 1;
            }
            KeyCode::Right | KeyCode::Tab => {
                self.range = (self.range + 1).min(RANGES.len() - 1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                return ScreenOutcome::with_toast(
                    crate::screens::ToastLevel::Success,
                    format!(
                        "Exported {} ({}) to CSV",
                        SERIES[self.series], RANGES[self.range]
                    ),
                );
            }
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Draw the explorer. `tick` animates every series.
    pub(crate) fn view(&self, theme: &Theme, tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [bar, main] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);

        // Range strip.
        let mut spans = vec![Line::from(" range: ").style(theme.caption())];
        for (i, r) in RANGES.iter().enumerate() {
            let sel = i == self.range;
            spans.push(Line::from(format!(" {r} ")).style(if sel {
                theme.selection()
            } else {
                theme.caption()
            }));
        }
        let cols = Layout::horizontal(
            std::iter::once(Constraint::Length(8))
                .chain(
                    RANGES
                        .iter()
                        .map(|r| Constraint::Length(r.len() as u16 + 2)),
                )
                .collect::<Vec<_>>(),
        )
        .split(bar);
        for (s, c) in spans.into_iter().zip(cols.iter()) {
            frame.render_widget(s, *c);
        }

        let [chart, side] =
            Layout::horizontal([Constraint::Percentage(58), Constraint::Fill(1)]).areas(main);

        // Multi-series latency line chart.
        let span = 80usize;
        let scale = [1.0_f64, 1.8, 2.7];
        let pts: Vec<Vec<(f64, f64)>> = (0..3)
            .map(|s| {
                (0..span)
                    .map(|x| {
                        let t = x as f64 * 0.18 + f64::from((tick % 60) as u32) * 0.1;
                        (
                            x as f64,
                            (t.sin() * 18.0 + 60.0) * scale[s] + s as f64 * 6.0,
                        )
                    })
                    .collect()
            })
            .collect();
        let series: Vec<Series> = (0..3)
            .map(|s| {
                let hot = s == self.series;
                let col = match s {
                    0 => theme.ok,
                    1 => theme.warn,
                    _ => theme.err,
                };
                Series::new(SERIES[s].to_string(), &pts[s])
                    .marker(if hot { '●' } else { '·' })
                    .style(Style::new().fg(col))
            })
            .collect();
        let cblock = panel(theme, &format!("Latency · {}", RANGES[self.range]));
        let cin = cblock.inner(chart);
        frame.render_widget(cblock, chart);
        frame.render_widget(
            LineChart::new(&series)
                .x_bounds(AxisBounds::new(0.0, span as f64))
                .y_bounds(AxisBounds::new(0.0, 360.0))
                .axis_style(theme.caption())
                .style(theme.body()),
            cin,
        );

        let [hist, heat] =
            Layout::vertical([Constraint::Percentage(52), Constraint::Fill(1)]).areas(side);

        // Latency distribution with percentile markers.
        let buckets: Vec<HistogramBucket> = BUCKETS
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let bell = 1.0 - ((i as f64 - 3.0) / 3.5).powi(2);
                let v = (bell.max(0.05) * 90.0) as u64 + (tick % 7);
                HistogramBucket::new(v, format!("≤{b}"))
            })
            .collect();
        let pcts = [
            Percentile::new(0.5, "p50").style(Style::new().fg(theme.ok)),
            Percentile::new(0.95, "p95").style(Style::new().fg(theme.warn)),
            Percentile::new(0.99, "p99").style(Style::new().fg(theme.err)),
        ];
        let hblock = panel(theme, "Distribution");
        let hin = hblock.inner(hist);
        frame.render_widget(hblock, hist);
        frame.render_widget(
            Histogram::new(&buckets)
                .bar_width(3)
                .bar_gap(1)
                .percentiles(&pcts)
                .bar_style(Style::new().fg(theme.accent))
                .label_style(theme.caption())
                .style(theme.body()),
            hin,
        );

        // Latency heatmap (bucket × time).
        let rows = BUCKETS.len();
        let span2 = 24usize;
        let cells: Vec<f64> = (0..rows * span2)
            .map(|n| {
                let r = n / span2;
                let c = n % span2;
                let bell = 1.0 - ((r as f64 - 3.0) / 4.0).powi(2);
                let t = c as f64 * 0.4 + f64::from((tick % 30) as u32) * 0.2;
                (bell.max(0.0)) * (t.sin() * 0.4 + 0.6)
            })
            .collect();
        let hmblock = panel(theme, "Latency heatmap");
        let hmin = hmblock.inner(heat);
        frame.render_widget(hmblock, heat);
        frame.render_widget(
            Heatmap::new(&cells, span2)
                .min(Some(0.0))
                .max(Some(1.0))
                .glyph_ramp(true)
                .style(Style::new().fg(theme.accent).bg(theme.surface)),
            hmin,
        );
    }
}

/// A rounded display panel.
fn panel(theme: &Theme, title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {title} ")).style(theme.caption()))
        .border_style(theme.border())
        .style(theme.body())
}
