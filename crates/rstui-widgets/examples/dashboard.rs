//! The flagship **business dashboard**: every chart family composed into one
//! screen the way a real ops/finance dashboard arranges them — KPI bullets, a
//! revenue trend, channel mix, a sales funnel, a P&L waterfall, a delivery
//! Gantt, and a contribution calendar.
//!
//! Every series here is plain caller-owned state; each widget only ever
//! *reads* it (the pure projection [`Sparkline`]/[`List`] use). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test for the whole chart suite:
//!
//! ```text
//! cargo run -p rstui-widgets --example dashboard
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::canvas::{Canvas, CanvasLine, Marker};
use rstui_widgets::{
    Block, Bullet, BulletChart, BulletChartDirection, CalendarHeatmap, Funnel, FunnelStage, Gantt,
    GanttTask, PieChart, Slice, Sparkline, StackMode, StackedBar, StackedBarChart, Waterfall,
    WaterfallStep,
};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(120, 34)).expect("TestBackend is infallible");

    // ---- caller-owned model (what an app's reducer would build) ----
    let revenue: Vec<(f64, f64)> = (0..48)
        .map(|i| {
            let x = f64::from(i);
            (x, 60.0 + 30.0 * (x / 7.0).sin() + x * 0.6)
        })
        .collect();
    let traffic: [u64; 32] = [
        12, 18, 15, 22, 30, 28, 35, 41, 38, 44, 40, 52, 49, 60, 57, 48, 39, 33, 41, 55, 62, 70, 66,
        58, 51, 63, 71, 80, 77, 69, 74, 88,
    ];

    terminal
        .draw(|frame| {
            let [header, body, footer] = Layout::vertical([
                Constraint::Length(3),
                Constraint::Fill(1),
                Constraint::Length(9),
            ])
            .areas(frame.area());

            // Header: three KPI bullet gauges (actual vs target over bands).
            let kpis = BulletChart::new([
                Bullet::new(86, 90, vec![50, 75, 100], "Uptime %"),
                Bullet::new(127, 100, vec![60, 90, 130], "MRR $k"),
                Bullet::new(43, 60, vec![40, 70, 100], "NPS"),
            ])
            .direction(BulletChartDirection::Horizontal)
            .block(Block::bordered().title("KPIs — actual vs target"))
            .bar_style(Style::new().fg(Color::Cyan))
            .target_style(Style::new().fg(Color::Yellow));
            frame.render_widget(kpis, header);

            let [left, mid, right] = Layout::horizontal([
                Constraint::Fill(2),
                Constraint::Fill(2),
                Constraint::Fill(1),
            ])
            .areas(body);

            // Left: revenue trend (Canvas Braille line) + traffic sparkline.
            let [trend, spark] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(4)]).areas(left);
            frame.render_widget(
                Canvas::default()
                    .block(Block::bordered().title("Revenue (90d)"))
                    .x_bounds([0.0, 47.0])
                    .y_bounds([0.0, 110.0])
                    .marker(Marker::Braille)
                    .paint(|ctx| {
                        for w in revenue.windows(2) {
                            ctx.draw(&CanvasLine {
                                x1: w[0].0,
                                y1: w[0].1,
                                x2: w[1].0,
                                y2: w[1].1,
                                color: Color::Green,
                            });
                        }
                    }),
                trend,
            );
            frame.render_widget(
                Sparkline::new(&traffic)
                    .style(Style::new().fg(Color::Magenta))
                    .max(Some(90)),
                Block::bordered().title("Sessions").inner(spark),
            );
            frame.render_widget(Block::bordered().title("Sessions"), spark);

            // Mid: revenue-by-region stacked bars + a P&L waterfall.
            let [stacked, falls] =
                Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).areas(mid);
            let seg = |a, b, c| vec![(a, Color::Blue), (b, Color::Cyan), (c, Color::Indexed(244))];
            frame.render_widget(
                StackedBarChart::new([
                    StackedBar::new("Q1", seg(40, 28, 16)),
                    StackedBar::new("Q2", seg(52, 30, 22)),
                    StackedBar::new("Q3", seg(61, 38, 25)),
                    StackedBar::new("Q4", seg(73, 44, 31)),
                ])
                .mode(StackMode::Stacked)
                .block(Block::bordered().title("Revenue by region")),
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
                .block(Block::bordered().title("ARR bridge ($k)"))
                .rise_style(Style::new().fg(Color::Green))
                .fall_style(Style::new().fg(Color::Red)),
                falls,
            );

            // Right: channel mix donut + acquisition funnel.
            let [pie, funnel] =
                Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).areas(right);
            frame.render_widget(
                PieChart::new([
                    Slice::new(48, Color::Blue, "Direct"),
                    Slice::new(27, Color::Cyan, "Search"),
                    Slice::new(15, Color::Magenta, "Social"),
                    Slice::new(10, Color::Yellow, "Email"),
                ])
                .donut(Some(0.5))
                .legend(true)
                .block(Block::bordered().title("Channel mix")),
                pie,
            );
            frame.render_widget(
                Funnel::new([
                    FunnelStage::new(1000, "Visits"),
                    FunnelStage::new(420, "Signups"),
                    FunnelStage::new(160, "Trials"),
                    FunnelStage::new(58, "Paid"),
                ])
                .block(Block::bordered().title("Funnel")),
                funnel,
            );

            // Footer: delivery Gantt + a contribution calendar.
            let [roadmap, cal] =
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
                .block(Block::bordered().title("Roadmap"))
                .bar_style(Style::new().fg(Color::Blue))
                .progress_style(Style::new().fg(Color::Green)),
                roadmap,
            );
            let activity: Vec<u64> = (0..119).map(|i| (i * 7 % 11) as u64).collect();
            frame.render_widget(
                CalendarHeatmap::new(&activity)
                    .start_weekday(0)
                    .max(Some(10))
                    .block(Block::bordered().title("Activity")),
                cal,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
