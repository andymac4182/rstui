//! The Data dashboard: animated [`Gauge`]s, a [`Sparkline`], a [`BarChart`],
//! an interactive [`Calendar`] + [`DatePicker`], a live [`DescriptionList`],
//! a [`Diff`], and an expandable [`Accordion`]. Three focusable panels
//! (`←/→` switches, `↑/↓`/`Enter` act); the rest is live display.

use rstui_core::{Constraint, KeyCode, Layout, Line, Modifier, Position, Rect, Style};
use rstui_runtime::Frame;
use rstui_widgets::{
    Accordion, AccordionSection, Bar, BarChart, Block, BorderType, Calendar, DatePicker,
    DescriptionList, DescriptionRow, Diff, DiffLayout, Gauge, Paragraph, Sparkline, ToastLevel,
    Wrap,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// A sample unified diff for the [`Diff`] widget.
const PATCH: &str = "\
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,4 @@
 fn render(area: Rect) {
-    let pad = 1;
+    let pad = 2;
     draw(area, pad);
 }";

/// The three accordion sections' titles and bodies.
const SECTIONS: [(&str, &str); 3] = [
    (
        "Rendering",
        "Pure projection: widgets read caller-owned state and never retain a tree.",
    ),
    (
        "Layout",
        "Integer constraint solver: Length, Percentage, Ratio, Min, Max, Fill.",
    ),
    (
        "Events",
        "The reducer routes input; focus is plain model state (ADR 0004).",
    ),
];

/// Which panel owns `↑↓`/`Enter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Progress,
    Dates,
    Sections,
}

/// The dashboard's caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    panel: Panel,
    progress: f64,
    day: u32,
    dp_open: bool,
    acc_sel: usize,
    expanded: [bool; 3],
}

impl State {
    /// 45% manual progress, day 12 selected, first section open.
    pub(crate) fn new() -> Self {
        Self {
            panel: Panel::Progress,
            progress: 0.45,
            day: 12,
            dp_open: false,
            acc_sel: 0,
            expanded: [true, false, false],
        }
    }

    /// `←/→` switch panels, `↑/↓` act on the focused one, `Enter` activates.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Left => match self.panel {
                Panel::Progress => return ScreenOutcome::ignored(),
                Panel::Dates => self.panel = Panel::Progress,
                Panel::Sections => self.panel = Panel::Dates,
            },
            KeyCode::Right => {
                self.panel = match self.panel {
                    Panel::Progress => Panel::Dates,
                    Panel::Dates => Panel::Sections,
                    Panel::Sections => Panel::Sections,
                }
            }
            KeyCode::Up => self.adjust(-1),
            KeyCode::Down => self.adjust(1),
            KeyCode::Enter | KeyCode::Char(' ') => return self.activate(),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Wheel scroll acts like `↑/↓` on the focused panel.
    pub(crate) fn on_scroll(&mut self, up: bool) {
        self.adjust(if up { -1 } else { 1 });
    }

    /// `↑/↓` (or scroll) on the focused panel.
    fn adjust(&mut self, delta: i32) {
        match self.panel {
            Panel::Progress => {
                self.progress = (self.progress + f64::from(delta) * 0.05).clamp(0.0, 1.0);
            }
            Panel::Dates => {
                let d = (self.day as i32 + delta).clamp(1, 30);
                self.day = d as u32;
            }
            Panel::Sections => {
                let n = SECTIONS.len() as i32;
                self.acc_sel = ((self.acc_sel as i32 + delta).rem_euclid(n)) as usize;
            }
        }
    }

    /// `Enter`/`Space` on the focused panel.
    fn activate(&mut self) -> ScreenOutcome {
        match self.panel {
            Panel::Progress => ScreenOutcome::with_toast(
                ToastLevel::Info,
                format!("Progress {}%", (self.progress * 100.0) as i32),
            ),
            Panel::Dates => {
                self.dp_open = !self.dp_open;
                ScreenOutcome::with_toast(ToastLevel::Info, format!("2026-06-{:02}", self.day))
            }
            Panel::Sections => {
                self.expanded[self.acc_sel] = !self.expanded[self.acc_sel];
                ScreenOutcome::consumed()
            }
        }
    }

    /// Click a panel to focus it; clicking the concepts panel also toggles
    /// the selected accordion section. Geometry mirrors [`view`] exactly.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [top, mid, bottom] = Layout::vertical([
            Constraint::Length(8),
            Constraint::Length(11),
            Constraint::Fill(1),
        ])
        .areas(content);
        let [prog, _b] =
            Layout::horizontal([Constraint::Percentage(52), Constraint::Fill(1)]).areas(top);
        let [cal, _d] =
            Layout::horizontal([Constraint::Percentage(52), Constraint::Fill(1)]).areas(mid);
        let [_diff, acc] =
            Layout::horizontal([Constraint::Percentage(52), Constraint::Fill(1)]).areas(bottom);
        if acc.contains(pos) {
            self.panel = Panel::Sections;
            self.expanded[self.acc_sel] = !self.expanded[self.acc_sel];
            return ScreenOutcome::consumed();
        }
        if cal.contains(pos) {
            self.panel = Panel::Dates;
            return ScreenOutcome::consumed();
        }
        if prog.contains(pos) {
            self.panel = Panel::Progress;
            return ScreenOutcome::consumed();
        }
        ScreenOutcome::ignored()
    }

    /// Draw the dashboard. `tick` animates the auto gauge / sparkline / bars.
    pub(crate) fn view(&self, theme: &Theme, tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [top, mid, bottom] = Layout::vertical([
            Constraint::Length(8),
            Constraint::Length(11),
            Constraint::Fill(1),
        ])
        .areas(area);

        // --- Top: progress panel (gauges + sparkline) | bar chart --------
        let [prog, bars] =
            Layout::horizontal([Constraint::Percentage(52), Constraint::Fill(1)]).areas(top);
        let prog_block = panel(theme, "Progress · ↑↓ adjust", self.panel == Panel::Progress);
        let prog_in = prog_block.inner(prog);
        frame.render_widget(prog_block, prog);
        let [g1, g2, spark] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(prog_in);
        frame.render_widget(
            Gauge::default()
                .ratio(self.progress)
                .label(format!("manual {}%", (self.progress * 100.0) as i32))
                .style(theme.body())
                .gauge_style(Style::new().fg(theme.base).bg(theme.accent)),
            g1,
        );
        let auto = f64::from((tick % 80) as u32) / 80.0;
        frame.render_widget(
            Gauge::default()
                .ratio(auto)
                .label(format!("auto {}%", (auto * 100.0) as i32))
                .style(theme.body())
                .gauge_style(Style::new().fg(theme.base).bg(theme.accent_alt)),
            g2,
        );
        let series: Vec<u64> = (0..spark.width.max(1))
            .map(|x| {
                let t = f64::from(x) * 0.4 + f64::from((tick % 64) as u32) * 0.2;
                (t.sin() * 20.0 + 24.0) as u64
            })
            .collect();
        frame.render_widget(
            Sparkline::new(&series).style(Style::new().fg(theme.accent_alt)),
            spark,
        );

        let bar_block = panel(theme, "Throughput", false);
        let bar_in = bar_block.inner(bars);
        frame.render_widget(bar_block, bars);
        let labels = ["Mon", "Tue", "Wed", "Thu", "Fri"];
        let bar_vec: Vec<Bar> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let v = 30 + ((tick / 4 + i as u64 * 7) % 50);
                Bar::new(v, *l)
            })
            .collect();
        frame.render_widget(
            BarChart::new(bar_vec)
                .bar_width(4)
                .bar_gap(2)
                .bar_style(Style::new().fg(theme.accent))
                .label_style(theme.caption())
                .style(theme.body()),
            bar_in,
        );

        // --- Mid: calendar + datepicker | description list ---------------
        let [cal, desc] =
            Layout::horizontal([Constraint::Percentage(52), Constraint::Fill(1)]).areas(mid);
        let cal_block = panel(
            theme,
            "Dates · ↑↓ day · Enter pick",
            self.panel == Panel::Dates,
        );
        let cal_in = cal_block.inner(cal);
        frame.render_widget(cal_block, cal);
        let [field, grid] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(cal_in);
        frame.render_widget(
            DatePicker::new(2026, 6, 30, 1)
                .selected(Some(self.day))
                .today(Some(17))
                .open(false)
                .focused(self.panel == Panel::Dates)
                .style(theme.body())
                .focus_style(theme.focus_field())
                .selected_style(theme.selection()),
            field,
        );
        frame.render_widget(
            Calendar::new(2026, 6, 30, 1)
                .selected(Some(self.day))
                .today(Some(17))
                .style(theme.body())
                .header_style(theme.accent_text())
                .weekday_style(theme.caption())
                .selected_style(theme.selection())
                .today_style(Style::new().fg(theme.warn).add_modifier(Modifier::BOLD)),
            grid,
        );

        let desc_block = panel(theme, "Live model", false);
        let desc_in = desc_block.inner(desc);
        frame.render_widget(desc_block, desc);
        frame.render_widget(
            DescriptionList::new([
                DescriptionRow::new("panel", format!("{:?}", self.panel)),
                DescriptionRow::new("progress", format!("{}%", (self.progress * 100.0) as i32)),
                DescriptionRow::new("auto", format!("{}%", (auto * 100.0) as i32)),
                DescriptionRow::new("day", format!("2026-06-{:02}", self.day)),
                DescriptionRow::new(
                    "picker",
                    if self.dp_open { "open" } else { "closed" }.to_string(),
                ),
                DescriptionRow::new("tick", tick.to_string()),
            ])
            .key_style(theme.caption())
            .value_style(theme.body())
            .style(theme.body()),
            desc_in,
        );

        // --- Bottom: diff | accordion -----------------------------------
        let [diff, acc] =
            Layout::horizontal([Constraint::Percentage(52), Constraint::Fill(1)]).areas(bottom);
        frame.render_widget(
            Diff::new(PATCH)
                .layout(DiffLayout::Unified)
                .style(theme.body())
                .block(framed(theme, "diff · src/lib.rs")),
            diff,
        );

        let acc_block = panel(
            theme,
            "Concepts · ↑↓ + Enter",
            self.panel == Panel::Sections,
        );
        let acc_in = acc_block.inner(acc);
        frame.render_widget(acc_block, acc);
        let sections: Vec<AccordionSection> = SECTIONS
            .iter()
            .enumerate()
            .map(|(i, (title, _))| {
                let mark = if i == self.acc_sel { "▶ " } else { "  " };
                AccordionSection::new(format!("{mark}{title}"))
                    .expanded(self.expanded[i])
                    .body_height(3)
            })
            .collect();
        let accordion = Accordion::new(sections)
            .style(theme.body())
            .header_style(theme.accent_text());
        let bodies = accordion.layout(acc_in);
        frame.render_widget(accordion, acc_in);
        for (i, slot) in bodies.into_iter().enumerate() {
            if let Some(rect) = slot {
                frame.render_widget(
                    Paragraph::new(SECTIONS[i].1)
                        .style(theme.caption())
                        .wrap(Wrap { trim: true }),
                    rect,
                );
            }
        }
    }
}

/// A focusable panel block — its border brightens when it owns the keyboard.
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

/// A plain rounded framing block (non-focusable display panels).
fn framed(theme: &Theme, title: &str) -> Block<'static> {
    panel(theme, title, false)
}
