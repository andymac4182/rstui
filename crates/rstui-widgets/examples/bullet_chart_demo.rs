//! Exercises [`BulletChart`] the way a dashboard does: a stack of caller-owned
//! KPIs as Few-style bullet graphs (shaded qualitative bands, a measure bar, a
//! target tick), beside a vertical variant.
//!
//! The bullets are plain caller-owned state — what an app's model holds and a
//! reducer recomputes; [`BulletChart`] only reads them (the same pure
//! projection [`BarChart`]/[`Gauge`] use). Running over a [`TestBackend`] keeps
//! it TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example bullet_chart_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Bullet, BulletChart, BulletChartDirection};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(56, 12)).expect("TestBackend is infallible");

    // The KPIs an app's model would own: each a measure, a target, and the
    // poor/satisfactory/good qualitative thresholds.
    let kpis = || {
        [
            Bullet::new(78, 85, vec![50, 75, 100], "Revenue"),
            Bullet::new(91, 80, vec![60, 80, 100], "SLA %"),
            Bullet::new(42, 60, vec![40, 70, 100], "p99 ms"),
        ]
    };

    terminal
        .draw(|frame| {
            let [horiz, vert] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(20)])
                .areas(frame.area());

            // Horizontal bullets stacked as labelled rows, scaled to 100.
            frame.render_widget(
                BulletChart::new(kpis())
                    .max(Some(100))
                    .bar_style(Style::new().fg(Color::Cyan))
                    .target_style(Style::new().fg(Color::Red))
                    .block(Block::bordered().title("KPIs vs. target")),
                horiz,
            );

            // The same KPIs as vertical columns with bottom labels.
            frame.render_widget(
                BulletChart::new(kpis())
                    .direction(BulletChartDirection::Vertical)
                    .max(Some(100))
                    .bar_style(Style::new().fg(Color::Green))
                    .target_style(Style::new().fg(Color::Yellow))
                    .block(Block::bordered().title("vertical")),
                vert,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
