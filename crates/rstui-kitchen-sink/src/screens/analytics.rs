//! The analytical chart catalog: the six exploratory chart types that are
//! not business-dashboard tiles — a [`ScatterPlot`] cloud, a [`RadarChart`]
//! spider, a [`BoxPlot`] distribution, a [`Candlestick`] OHLC series, a
//! [`Treemap`], and a [`Sankey`] flow — in a selectable `2×3` grid.
//! `←/→/↑/↓` move the highlight, `Enter` names the focused chart.

use rstui_core::{Color, Constraint, KeyCode, Layout, Line, Position, Rect, Style};
use rstui_runtime::Frame;
use rstui_widgets::canvas::Marker;
use rstui_widgets::scatter_plot::Series;
use rstui_widgets::{
    Block, BorderType, BoxPlot, BoxPlotOrientation, BoxStats, Candle, Candlestick, RadarAxis,
    RadarChart, RadarSeries, Sankey, SankeyLink, SankeyNode, ScatterPlot, Treemap, TreemapTile,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The six panels, in grid order (row-major, 3 per row).
const PANELS: [&str; 6] = [
    "Scatter",
    "Radar",
    "Box plot",
    "Candlestick",
    "Treemap",
    "Sankey",
];

/// The catalog's caller-owned state: which of the six panels is highlighted.
#[derive(Debug)]
pub(crate) struct State {
    sel: usize,
}

impl State {
    /// First panel selected.
    pub(crate) fn new() -> Self {
        Self { sel: 0 }
    }

    /// `←/→` step within a row, `↑/↓` jump a row (3-wide grid), `Enter`
    /// names the focused chart.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Left => self.sel = self.sel.saturating_sub(1),
            KeyCode::Right => self.sel = (self.sel + 1).min(PANELS.len() - 1),
            KeyCode::Up => self.sel = self.sel.saturating_sub(3),
            KeyCode::Down => self.sel = (self.sel + 3).min(PANELS.len() - 1),
            KeyCode::Enter | KeyCode::Char(' ') => {
                return ScreenOutcome::with_toast(
                    crate::screens::ToastLevel::Info,
                    format!("{} chart", PANELS[self.sel]),
                );
            }
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Click a panel to highlight it. Geometry mirrors [`view`] exactly: a
    /// two-row grid, three equal columns per row.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let rows = Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).split(content);
        for (r, row) in rows.iter().enumerate() {
            let cols = Layout::horizontal([Constraint::Fill(1); 3]).split(*row);
            for (c, cell) in cols.iter().enumerate() {
                let idx = r * 3 + c;
                if idx < PANELS.len() && cell.contains(pos) {
                    self.sel = idx;
                    return ScreenOutcome::with_toast(
                        crate::screens::ToastLevel::Info,
                        format!("{} chart", PANELS[idx]),
                    );
                }
            }
        }
        ScreenOutcome::ignored()
    }

    /// Draw the six analytical charts. `tick` animates the scatter and
    /// candlestick series; the selected panel takes the focused border.
    pub(crate) fn view(&self, theme: &Theme, tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let rows = Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).split(area);
        let mut cells: Vec<Rect> = Vec::with_capacity(6);
        for row in rows.iter() {
            for cell in Layout::horizontal([Constraint::Fill(1); 3]).split(*row) {
                cells.push(cell);
            }
        }
        let pane = |i: usize| panel(theme, PANELS[i], i == self.sel);
        let t = f64::from((tick % 60) as u32) * 0.1;

        // 0 — ScatterPlot: two correlated point clouds.
        let cloud_a: Vec<(f64, f64)> = (0..60)
            .map(|k| {
                let x = f64::from(k);
                (x, x * 0.8 + 12.0 * ((x * 0.4 + t).sin()))
            })
            .collect();
        let cloud_b: Vec<(f64, f64)> = (0..40)
            .map(|k| {
                let x = f64::from(k) * 1.5;
                (x, 70.0 - x * 0.5 + 8.0 * ((x * 0.3).cos()))
            })
            .collect();
        frame.render_widget(
            ScatterPlot::new([
                Series::new(&cloud_a, theme.accent).marker(Marker::Braille),
                Series::new(&cloud_b, Color::Magenta).marker(Marker::Braille),
            ])
            .block(pane(0))
            .style(theme.body()),
            cells[0],
        );

        // 1 — RadarChart: two profiles over five axes.
        let axes = [
            RadarAxis::new(10.0, "Speed"),
            RadarAxis::new(10.0, "Power"),
            RadarAxis::new(10.0, "Range"),
            RadarAxis::new(10.0, "Cost"),
            RadarAxis::new(10.0, "UX"),
        ];
        let prof_a = [8.0, 6.0, 7.0, 4.0, 9.0];
        let prof_b = [5.0, 9.0, 4.0, 8.0, 6.0];
        let series = [
            RadarSeries::new(&prof_a, theme.accent),
            RadarSeries::new(&prof_b, Color::Yellow),
        ];
        frame.render_widget(
            RadarChart::new(&axes, &series)
                .rings(4)
                .block(pane(1))
                .style(theme.body())
                .grid_style(theme.caption()),
            cells[1],
        );

        // 2 — BoxPlot: three distributions on a shared scale.
        frame.render_widget(
            BoxPlot::new([
                BoxStats::new("API", 8.0, 22.0, 31.0, 44.0, 60.0).outliers(vec![3.0, 71.0]),
                BoxStats::new("DB", 12.0, 26.0, 34.0, 49.0, 66.0).outliers(vec![78.0]),
                BoxStats::new("Cache", 2.0, 6.0, 9.0, 14.0, 22.0).outliers(vec![]),
            ])
            .orientation(BoxPlotOrientation::Horizontal)
            .block(pane(2))
            .style(theme.body())
            .box_style(Style::new().fg(theme.accent))
            .median_style(Style::new().fg(Color::Yellow))
            .outlier_style(Style::new().fg(Color::Red)),
            cells[2],
        );

        // 3 — Candlestick: an animated OHLC walk.
        let candles: Vec<Candle> = (0..24)
            .map(|k| {
                let b = 40.0 + 9.0 * (f64::from(k) * 0.5 + t).sin();
                let o = b;
                let c = b + 3.0 * (f64::from(k) * 0.9).cos();
                Candle::new(o, b.max(c) + 2.5, b.min(c) - 2.5, c)
            })
            .collect();
        frame.render_widget(
            Candlestick::new(candles)
                .block(pane(3))
                .style(theme.body())
                .bullish_style(Style::new().fg(Color::Green))
                .bearish_style(Style::new().fg(Color::Red)),
            cells[3],
        );

        // 4 — Treemap: weighted categories.
        frame.render_widget(
            Treemap::new([
                TreemapTile::new(46, Color::Blue, "Compute"),
                TreemapTile::new(28, Color::Cyan, "Storage"),
                TreemapTile::new(17, Color::Magenta, "Network"),
                TreemapTile::new(9, Color::Yellow, "Other"),
            ])
            .padding(1)
            .block(pane(4))
            .style(theme.body())
            .label_style(theme.caption()),
            cells[4],
        );

        // 5 — Sankey: a three-column flow.
        let nodes = [
            SankeyNode::new(0, "Visits"),
            SankeyNode::new(1, "Signup"),
            SankeyNode::new(1, "Bounce"),
            SankeyNode::new(2, "Paid"),
            SankeyNode::new(2, "Churn"),
        ];
        let links = [
            SankeyLink::new(0, 1, 60),
            SankeyLink::new(0, 2, 40),
            SankeyLink::new(1, 3, 38),
            SankeyLink::new(1, 4, 22),
        ];
        frame.render_widget(
            Sankey::new(&nodes, &links)
                .block(pane(5))
                .style(theme.body())
                .node_style(Style::new().fg(theme.accent))
                .link_style(theme.caption())
                .label_style(theme.caption()),
            cells[5],
        );
    }
}

/// A rounded display panel; the focused one takes the accent border.
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
