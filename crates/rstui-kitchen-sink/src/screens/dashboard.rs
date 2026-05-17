//! An analytics dashboard: four KPI [`Card`]s (value + delta [`Badge`] +
//! [`Sparkline`]), a [`BarChart`] of monthly revenue, a goal [`Gauge`], and
//! a recent-activity [`Table`]. Live: the series animate from `tick`.
//! `←/→` selects a KPI, `↑/↓` switches the date range, `Enter` drills in.

use rstui_core::{Constraint, KeyCode, Layout, Line, Position, Rect, Style};
use rstui_runtime::Frame;
use rstui_widgets::{
    Badge, BadgeLevel, Bar, BarChart, Block, BorderType, Gauge, Row, Sparkline, Table,
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

    /// Click a KPI card to select it (mirrors the `view` card row).
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [cards, _mid, _table] = Layout::vertical([
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Fill(1),
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

    /// Draw the dashboard. `tick` animates the sparklines + bars.
    pub(crate) fn view(&self, theme: &Theme, tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [cards, mid, table] = Layout::vertical([
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Fill(1),
        ])
        .areas(area);

        // KPI cards.
        let cols = Layout::horizontal([Constraint::Fill(1); 4]).split(cards);
        for (i, ((label, value, delta, good), cell)) in KPIS.iter().zip(cols.iter()).enumerate() {
            let focused = i == self.kpi;
            // A framed Block (its title reliably renders on the top border —
            // the same pattern every other screen's panels use).
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
                Line::from(value.to_string()).style(
                    Style::new()
                        .fg(theme.text)
                        .add_modifier(rstui_core::Modifier::BOLD),
                ),
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

        // Revenue bar chart + goal gauge.
        let [bars, goal] =
            Layout::horizontal([Constraint::Percentage(64), Constraint::Fill(1)]).areas(mid);
        let bblock = panel(theme, &format!("Revenue · {}", RANGES[self.range]));
        let bin = bblock.inner(bars);
        frame.render_widget(bblock, bars);
        let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun"];
        let bar_vec: Vec<Bar> = months
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let v = 40 + ((tick / 3 + i as u64 * 11 + self.range as u64 * 7) % 55);
                Bar::new(v, *m)
            })
            .collect();
        frame.render_widget(
            BarChart::new(bar_vec)
                .bar_width(5)
                .bar_gap(2)
                .bar_style(Style::new().fg(theme.accent))
                .label_style(theme.caption())
                .style(theme.body()),
            bin,
        );
        let gblock = panel(theme, "Quarter goal");
        let gin = gblock.inner(goal);
        frame.render_widget(gblock, goal);
        let ratio = 0.30 + f64::from((tick % 60) as u32) / 200.0;
        frame.render_widget(
            Gauge::default()
                .ratio(ratio.min(1.0))
                .label(format!("{}% of target", (ratio * 100.0) as i32))
                .style(theme.body())
                .gauge_style(Style::new().fg(theme.base).bg(theme.accent)),
            Rect::new(gin.x, gin.y + gin.height / 2, gin.width, 1),
        );

        // Recent activity table.
        let tblock = panel(theme, "Recent activity");
        let tin = tblock.inner(table);
        frame.render_widget(tblock, table);
        let rows = [
            ("09:14", "order #4821", "$129.00", "paid"),
            ("09:11", "signup", "—", "trial"),
            ("09:07", "order #4820", "$59.00", "paid"),
            ("09:01", "refund #771", "-$19.00", "done"),
            ("08:56", "order #4819", "$240.00", "paid"),
        ];
        frame.render_widget(
            Table::new(
                rows.iter()
                    .map(|(t, what, amt, st)| Row::new([*t, *what, *amt, *st])),
                [
                    Constraint::Length(7),
                    Constraint::Fill(1),
                    Constraint::Length(10),
                    Constraint::Length(8),
                ],
            )
            .header(Row::new(["Time", "Event", "Amount", "State"]).style(theme.accent_text()))
            .style(theme.body())
            .column_spacing(2),
            tin,
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
