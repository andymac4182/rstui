//! An OpenTelemetry service overview — the SRE "single pane of glass": four
//! golden-signal [`StatPanel`]s (rate / errors / latency / saturation), a
//! request-vs-error [`LineChart`], a per-service health [`Heatmap`], and a
//! live error [`LogStream`]. The series animate from `tick`.
//! `←/→` selects a golden-signal tile, `↑/↓` changes the time range,
//! `Enter` drills into the selected signal.

use rstui_core::{Constraint, KeyCode, Layout, Line, Position, Rect, Style};
use rstui_runtime::Frame;
use rstui_widgets::{
    AxisBounds, Block, BorderType, Heatmap, LineChart, LogLevel, LogPalette, LogRecord, LogStream,
    Series, StatPanel, Trend,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The four golden signals (Google SRE): label, unit, and whether a rising
/// value is good (so the trend colour reads correctly).
const SIGNALS: [(&str, &str, bool); 4] = [
    ("Request rate", "req/s", true),
    ("Error rate", "%", false),
    ("p99 latency", "ms", false),
    ("Saturation", "%", false),
];

/// The selectable time ranges.
const RANGES: [&str; 3] = ["15 min", "1 hour", "24 hours"];

/// The deterministic synthetic log pool the error stream draws from.
const POOL: [(LogLevel, &str, &str); 6] = [
    (
        LogLevel::Error,
        "checkout-api",
        "payment gateway timeout after 3000ms",
    ),
    (
        LogLevel::Warn,
        "checkout-api",
        "retry budget 60% consumed for orders.create",
    ),
    (
        LogLevel::Error,
        "inventory",
        "deadlock detected, transaction rolled back",
    ),
    (
        LogLevel::Warn,
        "edge-proxy",
        "upstream p99 over SLO (182ms > 150ms)",
    ),
    (
        LogLevel::Info,
        "checkout-api",
        "circuit breaker half-open: probing inventory",
    ),
    (
        LogLevel::Error,
        "auth",
        "token signing key rotation failed, using cached",
    ),
];

/// The observability overview's caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    /// The selected golden-signal tile.
    signal: usize,
    /// The selected time range.
    range: usize,
}

impl State {
    /// First signal selected, the one-hour range.
    pub(crate) fn new() -> Self {
        Self {
            signal: 0,
            range: 1,
        }
    }

    /// `←/→` pick a signal, `↑/↓` change range, `Enter` drills in.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Left => {
                if self.signal == 0 {
                    return ScreenOutcome::ignored();
                }
                self.signal -= 1;
            }
            KeyCode::Right => self.signal = (self.signal + 1).min(SIGNALS.len() - 1),
            KeyCode::Up => self.range = self.range.saturating_sub(1),
            KeyCode::Down => self.range = (self.range + 1).min(RANGES.len() - 1),
            KeyCode::Enter | KeyCode::Char(' ') => {
                return ScreenOutcome::with_toast(
                    crate::screens::ToastLevel::Info,
                    format!(
                        "Drill into {} ({})",
                        SIGNALS[self.signal].0, RANGES[self.range]
                    ),
                );
            }
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Click a golden-signal tile to select it (mirrors the `view` tile row).
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [tiles, _, _] = Layout::vertical([
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Fill(1),
        ])
        .areas(content);
        let cols = Layout::horizontal([Constraint::Fill(1); 4]).split(tiles);
        for (i, c) in cols.iter().enumerate() {
            if c.contains(pos) {
                self.signal = i;
                return ScreenOutcome::with_toast(
                    crate::screens::ToastLevel::Info,
                    format!("Selected {}", SIGNALS[i].0),
                );
            }
        }
        ScreenOutcome::ignored()
    }

    /// Draw the overview. `tick` animates every series.
    pub(crate) fn view(&self, theme: &Theme, tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [tiles, mid, logs] = Layout::vertical([
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Fill(1),
        ])
        .areas(area);

        // Golden-signal stat tiles.
        let cols = Layout::horizontal([Constraint::Fill(1); 4]).split(tiles);
        for (i, ((label, unit, rising_good), cell)) in SIGNALS.iter().zip(cols.iter()).enumerate() {
            let focused = i == self.signal;
            let spark: Vec<u64> = (0..cell.width.max(1))
                .map(|x| {
                    let t =
                        f64::from(x) * 0.4 + f64::from((tick % 60) as u32) * 0.2 + i as f64 * 1.7;
                    (t.sin() * 7.0 + 11.0) as u64
                })
                .collect();
            let value = match i {
                0 => format!("{:.1}k", 11.0 + (f64::from((tick % 30) as u32)) / 10.0),
                1 => format!("{:.2}", 0.30 + f64::from((tick % 17) as u32) / 100.0),
                2 => format!("{}", 150 + (tick % 40)),
                _ => format!("{}", 60 + (tick % 25)),
            };
            let up = (tick / 3 + i as u64) % 2 == 0;
            let (trend, good) = if up {
                (Trend::Up, *rising_good)
            } else {
                (Trend::Down, !*rising_good)
            };
            let accent = if good { theme.ok } else { theme.err };
            let card = panel(theme, label, focused);
            let cin = card.inner(*cell);
            frame.render_widget(card, *cell);
            frame.render_widget(
                StatPanel::new(Line::from(value).style(Style::new().fg(theme.text)))
                    .caption(Line::from(format!("last {}", RANGES[self.range])))
                    .delta((*unit).to_string())
                    .trend(trend)
                    .trend_style(Style::new().fg(accent))
                    .sparkline(&spark)
                    .spark_style(Style::new().fg(if focused { theme.accent } else { theme.dim }))
                    .caption_style(theme.caption())
                    .style(theme.body()),
                cin,
            );
        }

        // Request/error line chart + per-service health heatmap.
        let [chart, heat] =
            Layout::horizontal([Constraint::Percentage(58), Constraint::Fill(1)]).areas(mid);
        let span = 60usize;
        let req: Vec<(f64, f64)> = (0..span)
            .map(|x| {
                let t = x as f64 * 0.2 + f64::from((tick % 50) as u32) * 0.1;
                (x as f64, t.sin() * 30.0 + 70.0)
            })
            .collect();
        let err: Vec<(f64, f64)> = (0..span)
            .map(|x| {
                let t = x as f64 * 0.3 + f64::from((tick % 50) as u32) * 0.1;
                (x as f64, (t.cos() * 6.0 + 8.0).max(0.0))
            })
            .collect();
        let series = [
            Series::new("req/s".to_string(), &req).style(Style::new().fg(theme.accent)),
            Series::new("err/s".to_string(), &err).style(Style::new().fg(theme.err)),
        ];
        let cblock = panel(theme, "Throughput vs errors", false);
        let cin = cblock.inner(chart);
        frame.render_widget(cblock, chart);
        frame.render_widget(
            LineChart::new(&series)
                .x_bounds(AxisBounds::new(0.0, span as f64))
                .y_bounds(AxisBounds::new(0.0, 110.0))
                .axis_style(theme.caption())
                .style(theme.body()),
            cin,
        );

        let services = ["edge", "auth", "checkout", "inventory", "search", "ledger"];
        let cells: Vec<f64> = (0..services.len() * 16)
            .map(|n| {
                let row = n / 16;
                let t = (n % 16) as f64 * 0.5 + row as f64 + f64::from((tick % 40) as u32) * 0.15;
                (t.sin() * 0.5 + 0.5) * if row == 3 { 1.0 } else { 0.6 }
            })
            .collect();
        let hblock = panel(theme, "Service health", false);
        let hin = hblock.inner(heat);
        frame.render_widget(hblock, heat);
        frame.render_widget(
            Heatmap::new(&cells, 16)
                .min(Some(0.0))
                .max(Some(1.0))
                .row_labels(&services)
                .label_style(theme.caption())
                .low_color(theme.ok)
                .high_color(theme.err)
                .glyph_ramp(false)
                .style(theme.body()),
            hin,
        );

        // Live error stream.
        let recs: Vec<LogRecord> = (0..logs.height.max(1) as usize)
            .map(|i| {
                let n = (tick as usize).saturating_sub(i);
                let (lvl, target, msg) = POOL[n % POOL.len()];
                LogRecord::new(lvl, msg)
                    .timestamp(format!("12:{:02}:{:02}", (n / 60) % 60, n % 60))
                    .target(target)
            })
            .collect();
        let lblock = panel(theme, "Recent errors & warnings", false);
        let lin = lblock.inner(logs);
        frame.render_widget(lblock, logs);
        frame.render_widget(
            LogStream::new(&recs)
                .palette(LogPalette {
                    trace: theme.dim,
                    debug: theme.accent_alt,
                    info: theme.ok,
                    warn: theme.warn,
                    error: theme.err,
                })
                .timestamp_style(theme.caption())
                .target_style(Style::new().fg(theme.accent_alt))
                .message_style(Style::new().fg(theme.text))
                .style(theme.body()),
            lin,
        );
    }
}

/// A rounded display panel; the title brightens when `focused`.
fn panel(theme: &Theme, title: &str, focused: bool) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {title} ")).style(if focused {
            theme.accent_text()
        } else {
            theme.caption()
        }))
        .border_style(if focused {
            theme.border_focused()
        } else {
            theme.border()
        })
        .style(theme.body())
}
