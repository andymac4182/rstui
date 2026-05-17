//! A settings / preferences experience: a category [`List`] and a panel of
//! live controls — [`Switch`], [`Select`], [`Slider`] — plus an About card
//! ([`DescriptionList`]). `↑/↓` move the field, `Space` toggles/cycles,
//! `-/+` nudges a slider, `←/→` switch category.

use rstui_core::{Constraint, KeyCode, Layout, Line, Margin, Position, Rect, Style};
use rstui_runtime::Frame;
use rstui_widgets::{
    Block, BorderType, DescriptionList, DescriptionRow, List, Select, Slider, StatusBar, Switch,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// One control's kind + value.
#[derive(Debug)]
enum Setting {
    /// An on/off [`Switch`].
    Toggle(bool),
    /// A [`Select`] over `options`, with the chosen index.
    Choice(&'static [&'static str], usize),
    /// A [`Slider`] value within `0..=max`.
    Range(f64, f64),
}

/// One labelled field.
#[derive(Debug)]
struct Field {
    label: &'static str,
    setting: Setting,
}

fn toggle(label: &'static str, on: bool) -> Field {
    Field {
        label,
        setting: Setting::Toggle(on),
    }
}
fn choice(label: &'static str, opts: &'static [&'static str], idx: usize) -> Field {
    Field {
        label,
        setting: Setting::Choice(opts, idx),
    }
}
fn range(label: &'static str, v: f64, max: f64) -> Field {
    Field {
        label,
        setting: Setting::Range(v, max),
    }
}

const CATEGORIES: [&str; 5] = ["General", "Appearance", "Privacy", "Notifications", "About"];

/// The settings screen's caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    cat: usize,
    field: usize,
    groups: Vec<Vec<Field>>,
}

impl State {
    /// Sensible seeded defaults across the categories.
    pub(crate) fn new() -> Self {
        Self {
            cat: 0,
            field: 0,
            groups: vec![
                vec![
                    toggle("Start on launch", true),
                    choice("Default screen", &["Welcome", "Chat", "Dashboard"], 1),
                    toggle("Confirm before quit", true),
                ],
                vec![
                    choice("Theme", &["Dark", "Light", "System"], 0),
                    range("Font scale", 1.0, 2.0),
                    toggle("Animations", true),
                ],
                vec![
                    toggle("Telemetry", false),
                    toggle("Crash reports", true),
                    choice("Retention", &["30d", "90d", "1y"], 1),
                ],
                vec![
                    toggle("Desktop alerts", true),
                    range("Quiet after (h)", 22.0, 24.0),
                    choice("Sound", &["None", "Chime", "Ping"], 1),
                ],
                vec![],
            ],
        }
    }

    fn fields(&self) -> &[Field] {
        &self.groups[self.cat]
    }

    /// `←/→` category, `↑/↓` field, `Space`/`Enter` toggle or cycle,
    /// `-/+` nudge a slider.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Left => {
                if self.cat == 0 {
                    return ScreenOutcome::ignored();
                }
                self.cat -= 1;
                self.field = 0;
            }
            KeyCode::Right => {
                self.cat = (self.cat + 1).min(CATEGORIES.len() - 1);
                self.field = 0;
            }
            KeyCode::Up => self.field = self.field.saturating_sub(1),
            KeyCode::Down => {
                let n = self.fields().len();
                if n > 0 {
                    self.field = (self.field + 1).min(n - 1);
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                let f = self.field;
                if let Some(field) = self.groups[self.cat].get_mut(f) {
                    match &mut field.setting {
                        Setting::Toggle(b) => *b = !*b,
                        Setting::Choice(opts, idx) => *idx = (*idx + 1) % opts.len(),
                        Setting::Range(..) => {}
                    }
                    return ScreenOutcome::with_toast(
                        crate::screens::ToastLevel::Success,
                        format!("Updated “{}”", field.label),
                    );
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') => self.nudge(1.0),
            KeyCode::Char('-') => self.nudge(-1.0),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    fn nudge(&mut self, dir: f64) {
        let f = self.field;
        if let Some(field) = self.groups[self.cat].get_mut(f) {
            if let Setting::Range(v, max) = &mut field.setting {
                *v = (*v + dir * (*max / 20.0)).clamp(0.0, *max);
            }
        }
    }

    /// Click a category to open it, or a field to focus it. Geometry
    /// mirrors [`view`].
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [body, _foot] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(content);
        let [cats, panel_a] =
            Layout::horizontal([Constraint::Length(20), Constraint::Fill(1)]).areas(body);
        let cin = cats.inner(Margin::new(1, 1));
        if cin.contains(pos) {
            let i = (pos.y - cin.y) as usize;
            if i < CATEGORIES.len() {
                self.cat = i;
                self.field = 0;
                return ScreenOutcome::consumed();
            }
        }
        if self.cat != 4 {
            let pin = panel_a.inner(Margin::new(1, 1));
            if pin.contains(pos) {
                let rows = Layout::vertical([Constraint::Length(2); 3]).split(pin);
                let n = self.fields().len();
                for (i, r) in rows.iter().enumerate() {
                    if i < n && r.contains(pos) {
                        self.field = i;
                        return ScreenOutcome::consumed();
                    }
                }
            }
        }
        ScreenOutcome::ignored()
    }

    /// Draw the settings screen.
    pub(crate) fn view(&self, theme: &Theme, _tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [body, foot] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
        let [cats, panel_a] =
            Layout::horizontal([Constraint::Length(20), Constraint::Fill(1)]).areas(body);

        frame.render_widget(
            List::new(CATEGORIES.iter().map(|c| format!("  {c}")))
                .selected(Some(self.cat))
                .highlight_symbol("▌ ")
                .highlight_style(theme.selection())
                .style(theme.body())
                .block(framed(theme, "Settings")),
            cats,
        );

        let pblock = framed(theme, CATEGORIES[self.cat]);
        let pin = pblock.inner(panel_a);
        frame.render_widget(pblock, panel_a);

        if self.cat == 4 {
            frame.render_widget(
                DescriptionList::new([
                    DescriptionRow::new("App", "rstui kitchen-sink".to_string()),
                    DescriptionRow::new("Version", "0.0.1".to_string()),
                    DescriptionRow::new("Screens", "18 (8 widgets · 10 experiences)".to_string()),
                    DescriptionRow::new(
                        "Renderer",
                        "pure projection, no retained tree".to_string(),
                    ),
                    DescriptionRow::new("License", "Apache-2.0".to_string()),
                ])
                .key_style(theme.caption())
                .value_style(theme.body())
                .style(theme.body()),
                pin,
            );
        } else {
            let rows = Layout::vertical([Constraint::Length(2); 3]).split(pin);
            for (i, field) in self.fields().iter().enumerate() {
                let Some(row) = rows.get(i) else { continue };
                let focused = i == self.field;
                let [label_a, control_a] =
                    Layout::horizontal([Constraint::Length(18), Constraint::Fill(1)]).areas(*row);
                frame.render_widget(
                    Line::from(field.label).style(if focused {
                        theme.accent_text()
                    } else {
                        theme.caption()
                    }),
                    label_a,
                );
                match &field.setting {
                    Setting::Toggle(b) => frame.render_widget(
                        Switch::new()
                            .on(*b)
                            .focused(focused)
                            .on_label("on")
                            .off_label("off")
                            .style(theme.body())
                            .focus_style(theme.focus_field()),
                        control_a,
                    ),
                    Setting::Choice(opts, idx) => frame.render_widget(
                        Select::new(opts.iter().copied())
                            .selected(Some(*idx))
                            .focused(focused)
                            .style(theme.body())
                            .focus_style(theme.focus_field())
                            .block(
                                Block::bordered()
                                    .border_type(BorderType::Plain)
                                    .border_style(theme.border()),
                            ),
                        control_a,
                    ),
                    Setting::Range(v, max) => frame.render_widget(
                        Slider::new()
                            .range(0.0, *max)
                            .value(*v)
                            .value_label(Line::from(format!("{v:.1}")).style(theme.body()))
                            .focused(focused)
                            .style(theme.body())
                            .thumb_style(Style::new().fg(theme.accent))
                            .focus_style(theme.focus_field()),
                        Rect::new(control_a.x, control_a.y, control_a.width, 1),
                    ),
                }
            }
        }

        frame.render_widget(
            StatusBar::new()
                .left(Line::from(" ←→ category · ↑↓ field ").style(theme.caption()))
                .center(Line::from("Space toggle/cycle · -/+ slider").style(theme.caption()))
                .right(Line::from(format!(" {} ", CATEGORIES[self.cat])).style(theme.caption()))
                .style(Style::new().fg(theme.dim).bg(theme.raised)),
            foot,
        );
    }
}

/// A rounded panel.
fn framed(theme: &Theme, title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {title} ")).style(theme.caption()))
        .border_style(theme.border())
        .style(theme.body())
}
