//! A real business dashboard composing the chart suite: four clickable KPI
//! [`Card`]s (value + delta [`Badge`] + [`Sparkline`]), a Braille revenue
//! [`Canvas`] line, a [`StackedBarChart`] of revenue by region, an ARR
//! [`Waterfall`], an acquisition [`Funnel`], a channel-mix donut
//! [`PieChart`], a [`BulletChart`] KPI strip, a roadmap [`Gantt`], and a
//! contribution [`CalendarHeatmap`]. Live: series animate from `tick`.
//! `←/→` selects a KPI, `↑/↓` switches the date range, `Enter` drills in.

use rstui_core::{Color, Constraint, KeyCode, Layout, Line, Modifier, Position, Rect, Style};
use rstui_runtime::Frame;
use rstui_widgets::canvas::{Canvas, CanvasLine, Marker};
use rstui_widgets::{
    Badge, BadgeLevel, Block, BorderType, Bullet, BulletChart, BulletChartDirection,
    CalendarHeatmap, Funnel, FunnelStage, Gantt, GanttTask, PieChart, Slice, Sparkline, StackMode,
    StackedBar, StackedBarChart, Waterfall, WaterfallStep,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The KPI tiles: label, value, delta %, positive?
const KPIS: [(&str, &str, &str, bool); 4] = [
    ("Revenue", "$48.2k", "+12.4%", true),
    ("Active users", "9,134", "+3.1%", true),
    ("Churn", "1.8%", "-0.4%", true),
    ("Latency p95", "182ms", "+9ms", false),
];

const RANGES: [&str; 3] = ["7 days", "30 days", "Quarter"];

/// The dashboard's caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    kpi: usize,
    range: usize,
}

impl State {
    /// First KPI selected, 30-day range.
    pub(crate) fn new() -> Self {
        Self { kpi: 0, range: 1 }
    }

    /// `←/→` pick a KPI, `↑/↓` change range, `Enter` drills in.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Left => {
                if self.kpi == 0 {
                    return ScreenOutcome::ignored();
                }
                self.kpi -= 1;
            }
            KeyCode::Right => self.kpi = (self.kpi + 1).min(KPIS.len() - 1),
            KeyCode::Up => self.range = self.range.saturating_sub(1),
            KeyCode::Down => self.range = (self.range + 1).min(RANGES.len() - 1),
            KeyCode::Enter | KeyCode::Char(' ') => {
                return ScreenOutcome::with_toast(
                    crate::screens::ToastLevel::Info,
                    format!("Drill into {} ({})", KPIS[self.kpi].0, RANGES[self.range]),
                );
            }
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Click a KPI card to select it (mirrors the `view` card row: the first
    /// `Length(6)` band split into four equal columns).
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [cards, _body, _footer] = Layout::vertical([
            Constraint::Length(6),
            Constraint::Fill(1),
            Constraint::Length(9),
        ])
        .areas(content);
        let cols = Layout::horizontal([Constraint::Fill(1); 4]).split(cards);
        for (i, c) in cols.iter().enumerate() {
            if c.contains(pos) {
                self.kpi = i;
                return ScreenOutcome::with_toast(
                    crate::screens::ToastLevel::Info,
                    format!("Selected {}", KPIS[i].0),
                );
            }
        }
        ScreenOutcome::ignored()
    }

    /// Draw the dashboard. `tick` animates the series; `range`/`kpi` drive
    /// the revenue scale and the selected-card highlight.
    pub(crate) fn view(&self, theme: &Theme, tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [cards, body, footer] = Layout::vertical([
            Constraint::Length(6),
            Constraint::Fill(1),
            Constraint::Length(9),
        ])
        .areas(area);

        // --- KPI cards (the clickable row on_click hit-tests) ---
        let cols = Layout::horizontal([Constraint::Fill(1); 4]).split(cards);
        for (i, ((label, value, delta, good), cell)) in KPIS.iter().zip(cols.iter()).enumerate() {
            let focused = i == self.kpi;
            let card = Block::bordered()
                .border_type(BorderType::Rounded)
                .title(Line::from(format!(" {label} ")).style(if focused {
                    theme.accent_text()
                } else {
                    theme.caption()
                }))
                .border_style(if focused {
                    theme.border_focused()
                } else {
                    theme.border()
                })
                .style(theme.body());
            let cin = card.inner(*cell);
            frame.render_widget(card, *cell);
            let [vrow, drow, srow] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .areas(cin);
            frame.render_widget(
                Line::from(value.to_string())
                    .style(Style::new().fg(theme.text).add_modifier(Modifier::BOLD)),
                vrow,
            );
            frame.render_widget(
                Badge::new(*delta).level(if *good {
                    BadgeLevel::Success
                } else {
                    BadgeLevel::Warning
                }),
                drow,
            );
            let series: Vec<u64> = (0..srow.width.max(1))
                .map(|x| {
                    let t = f64::from(x) * 0.5 + f64::from((tick % 50) as u32) * 0.2 + i as f64;
                    (t.sin() * 8.0 + 12.0) as u64
                })
                .collect();
            frame.render_widget(
                Sparkline::new(&series).style(Style::new().fg(theme.accent)),
                srow,
            );
        }

        // --- body: three columns of charts ---
        let [left, mid, right] = Layout::horizontal([
            Constraint::Fill(2),
            Constraint::Fill(2),
            Constraint::Fill(1),
        ])
        .areas(body);

        // Left: a Braille revenue trend (scaled by the selected range) + a
        // bottom acquisition funnel.
        let [trend, funnel] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(8)]).areas(left);
        let scale = 0.7 + self.range as f64 * 0.35;
        let phase = f64::from((tick % 60) as u32) * 0.05;
        let revenue: Vec<(f64, f64)> = (0..48)
            .map(|i| {
                let x = f64::from(i);
                (x, (45.0 + 30.0 * (x / 7.0 + phase).sin() + x * 0.7) * scale)
            })
            .collect();
        frame.render_widget(
            Canvas::default()
                .block(panel(theme, &format!("Revenue · {}", RANGES[self.range])))
                .x_bounds([0.0, 47.0])
                .y_bounds([0.0, 140.0])
                .marker(Marker::Braille)
                .background(theme.body())
                .paint(|ctx| {
                    for w in revenue.windows(2) {
                        ctx.draw(&CanvasLine {
                            x1: w[0].0,
                            y1: w[0].1,
                            x2: w[1].0,
                            y2: w[1].1,
                            color: theme.accent,
                        });
                    }
                }),
            trend,
        );
        frame.render_widget(
            Funnel::new([
                FunnelStage::new(1000, "Visits"),
                FunnelStage::new(420, "Signups"),
                FunnelStage::new(160, "Trials"),
                FunnelStage::new(58, "Paid"),
            ])
            .block(panel(theme, "Acquisition funnel"))
            .style(theme.body())
            .bar_style(Style::new().fg(theme.accent))
            .label_style(theme.caption()),
            funnel,
        );

        // Mid: revenue-by-region stacked bars + an ARR bridge waterfall.
        let [stacked, bridge] =
            Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).areas(mid);
        let seg = |a: u64, b: u64, c: u64| {
            vec![(a, Color::Blue), (b, Color::Cyan), (c, Color::Indexed(244))]
        };
        let d = (tick / 4) % 18;
        frame.render_widget(
            StackedBarChart::new([
                StackedBar::new("Q1", seg(40 + d, 28, 16)),
                StackedBar::new("Q2", seg(52, 30 + d, 22)),
                StackedBar::new("Q3", seg(61, 38, 25 + d)),
                StackedBar::new("Q4", seg(73, 44, 31)),
            ])
            .mode(StackMode::Stacked)
            .block(panel(theme, "Revenue by region"))
            .style(theme.body())
            .label_style(theme.caption()),
            stacked,
        );
        frame.render_widget(
            Waterfall::new([
                WaterfallStep::delta(120, "Open"),
                WaterfallStep::delta(45, "New"),
                WaterfallStep::delta(-30, "Churn"),
                WaterfallStep::delta(18, "Expand"),
                WaterfallStep::total("Close"),
            ])
            .block(panel(theme, "ARR bridge ($k)"))
            .style(theme.body())
            .rise_style(Style::new().fg(Color::Green))
            .fall_style(Style::new().fg(Color::Red))
            .total_style(Style::new().fg(theme.accent))
            .label_style(theme.caption()),
            bridge,
        );

        // Right: channel-mix donut + a KPI bullet strip.
        let [pie, bullets] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(8)]).areas(right);
        frame.render_widget(
            PieChart::new([
                Slice::new(48, Color::Blue, "Direct"),
                Slice::new(27, Color::Cyan, "Search"),
                Slice::new(15, Color::Magenta, "Social"),
                Slice::new(10, Color::Yellow, "Email"),
            ])
            .donut(Some(0.5))
            .legend(true)
            .block(panel(theme, "Channel mix"))
            .style(theme.body()),
            pie,
        );
        frame.render_widget(
            BulletChart::new([
                Bullet::new(86, 90, vec![50, 75, 100], "Uptime"),
                Bullet::new(127, 100, vec![60, 90, 130], "MRR"),
                Bullet::new(43, 60, vec![40, 70, 100], "NPS"),
            ])
            .direction(BulletChartDirection::Horizontal)
            .block(panel(theme, "KPIs vs target"))
            .style(theme.body())
            .bar_style(Style::new().fg(theme.accent))
            .target_style(Style::new().fg(Color::Yellow))
            .label_style(theme.caption()),
            bullets,
        );

        // --- footer: delivery Gantt + contribution calendar ---
        let [roadmap, activity] =
            Layout::horizontal([Constraint::Fill(2), Constraint::Fill(1)]).areas(footer);
        frame.render_widget(
            Gantt::new([
                GanttTask::new(0, 6, "Discovery").progress(100),
                GanttTask::new(4, 12, "Build").progress(70),
                GanttTask::new(10, 16, "Beta").progress(25),
                GanttTask::new(15, 20, "GA").progress(0),
            ])
            .range(Some((0, 20)))
            .today(Some(11))
            .block(panel(theme, "Roadmap"))
            .style(theme.body())
            .bar_style(Style::new().fg(Color::Blue))
            .progress_style(Style::new().fg(Color::Green))
            .today_style(Style::new().fg(Color::Yellow))
            .label_style(theme.caption()),
            roadmap,
        );
        let acts: Vec<u64> = (0..119)
            .map(|i| ((i * 7 + tick as usize) % 11) as u64)
            .collect();
        frame.render_widget(
            CalendarHeatmap::new(&acts)
                .start_weekday(0)
                .max(Some(10))
                .block(panel(theme, "Activity"))
                .style(theme.body()),
            activity,
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
