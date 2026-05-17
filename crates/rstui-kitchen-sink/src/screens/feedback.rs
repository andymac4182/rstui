//! The Feedback screen: every [`Alert`] level, every [`Badge`] level, an
//! animated [`Spinner`] + [`Skeleton`], an inline [`StatusBar`], and a
//! [`Tooltip`] + [`Popover`] anchored to a target. Press `t` (or
//! `Enter`/`Space`) to fire a real [`Toast`](rstui_widgets::Toast) into the
//! global queue, cycling severity.

use rstui_core::{Constraint, KeyCode, Layout, Line, Modifier, Position, Rect, Style};
use rstui_runtime::Frame;
use rstui_widgets::{
    Alert, AlertLevel, Badge, BadgeLevel, Block, BorderType, Paragraph, Popover, PopoverSide,
    Skeleton, SkeletonShape, Spinner, StatusBar, ToastLevel, Tooltip, Wrap,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The toast severities `t` cycles through.
const LEVELS: [(ToastLevel, &str); 4] = [
    (ToastLevel::Info, "info"),
    (ToastLevel::Success, "success"),
    (ToastLevel::Warning, "warning"),
    (ToastLevel::Error, "error"),
];

/// Which severity the next `t` will fire.
#[derive(Debug)]
pub(crate) struct State {
    next: usize,
}

impl State {
    /// Start on the info toast.
    pub(crate) fn new() -> Self {
        Self { next: 0 }
    }

    /// `t` / `Enter` / `Space` fire a toast (cycling level); `←` falls back
    /// to the rail.
    pub(crate) fn on_key(&mut self, code: KeyCode, tick: u64) -> ScreenOutcome {
        match code {
            KeyCode::Char('t') | KeyCode::Enter | KeyCode::Char(' ') => {
                let (level, name) = LEVELS[self.next];
                self.next = (self.next + 1) % LEVELS.len();
                ScreenOutcome::with_toast(level, format!("{name} toast @ tick {tick}"))
            }
            KeyCode::Left => ScreenOutcome::ignored(),
            _ => ScreenOutcome::ignored(),
        }
    }

    /// A click anywhere fires the next toast — the same affordance as `t`,
    /// so the mouse exercises the live [`Toast`](rstui_widgets::Toast) queue.
    pub(crate) fn on_click(&mut self, _pos: Position, _content: Rect) -> ScreenOutcome {
        let (level, name) = LEVELS[self.next];
        self.next = (self.next + 1) % LEVELS.len();
        ScreenOutcome::with_toast(level, format!("{name} toast (clicked)"))
    }

    /// Draw the feedback gallery. `tick` animates the spinner + skeleton.
    pub(crate) fn view(&self, theme: &Theme, tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [alerts, badges, busy, anchored, foot] = Layout::vertical([
            Constraint::Length(9),
            Constraint::Length(2),
            Constraint::Length(5),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);

        // Every alert level.
        let alert_rows = Layout::vertical([Constraint::Length(2); 4]).split(alerts);
        let specs = [
            (
                AlertLevel::Info,
                "Heads up",
                "Pure projection — the alert only reads its level.",
            ),
            (
                AlertLevel::Success,
                "Saved",
                "All gates green; nothing left to do.",
            ),
            (
                AlertLevel::Warning,
                "Careful",
                "Unsaved edits will be lost on quit.",
            ),
            (
                AlertLevel::Error,
                "Failed",
                "Could not reach the backend; retrying.",
            ),
        ];
        for (row, (level, title, body)) in alert_rows.iter().zip(specs) {
            frame.render_widget(
                Alert::new(level, title)
                    .body(body)
                    .style(theme.body())
                    .info_style(Style::new().fg(theme.info).add_modifier(Modifier::BOLD))
                    .success_style(Style::new().fg(theme.ok).add_modifier(Modifier::BOLD))
                    .warning_style(Style::new().fg(theme.warn).add_modifier(Modifier::BOLD))
                    .error_style(Style::new().fg(theme.err).add_modifier(Modifier::BOLD)),
                *row,
            );
        }

        // Every badge level.
        frame.render_widget(Line::from("Badges:").style(theme.caption()), badges);
        let badge_row = Rect::new(badges.x, badges.y + 1, badges.width, 1);
        let cells = Layout::horizontal([Constraint::Length(12); 5]).split(badge_row);
        let blevels = [
            (BadgeLevel::Neutral, "neutral"),
            (BadgeLevel::Info, "info"),
            (BadgeLevel::Success, "ok"),
            (BadgeLevel::Warning, "warn"),
            (BadgeLevel::Error, "error"),
        ];
        for (cell, (level, label)) in cells.iter().zip(blevels) {
            frame.render_widget(Badge::new(label).level(level), *cell);
        }

        // Animated spinner + skeleton.
        let [spin, skel] =
            Layout::horizontal([Constraint::Percentage(35), Constraint::Fill(1)]).areas(busy);
        let spin_block = framed(theme, "Spinner");
        let spin_in = spin_block.inner(spin);
        frame.render_widget(spin_block, spin);
        frame.render_widget(
            Spinner::new()
                .tick(tick as usize)
                .style(Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Rect::new(spin_in.x + 1, spin_in.y + 1, 2, 1),
        );
        frame.render_widget(
            Line::from("loading…").style(theme.caption()),
            Rect::new(
                spin_in.x + 4,
                spin_in.y + 1,
                spin_in.width.saturating_sub(4),
                1,
            ),
        );
        let skel_block = framed(theme, "Skeleton (shimmer)");
        let skel_in = skel_block.inner(skel);
        frame.render_widget(skel_block, skel);
        frame.render_widget(
            Skeleton::new()
                .shape(SkeletonShape::Lines(2))
                .tick(tick as usize)
                .style(Style::new().fg(theme.border).bg(theme.surface))
                .shimmer_style(Style::new().fg(theme.dim).bg(theme.raised)),
            skel_in,
        );

        // Tooltip + Popover anchored to a target.
        let [tip_area, pop_area] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Fill(1)]).areas(anchored);
        let tip_anchor = Rect::new(tip_area.x + 4, tip_area.y + 4, 18, 1);
        frame.render_widget(
            Line::from("● tooltip anchor").style(theme.accent_text()),
            tip_anchor,
        );
        let tooltip = Tooltip::new("Anchored & auto-flipped\nto stay on screen")
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(theme.border()),
            )
            .style(theme.body());
        tooltip.render_anchored(tip_anchor, frame.buffer_mut());

        let pop_anchor = Rect::new(pop_area.x + 4, pop_area.y + 4, 18, 1);
        frame.render_widget(
            Line::from("● popover anchor").style(theme.accent_text()),
            pop_anchor,
        );
        let popover = Popover::new()
            .side(PopoverSide::Bottom)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(theme.border()),
            )
            .style(theme.body());
        let pop_inner = popover.inner(pop_anchor, area);
        popover.render_anchored(pop_anchor, frame.buffer_mut());
        frame.render_widget(
            Paragraph::new("Popover body — a generic\nanchored panel primitive.")
                .style(theme.body())
                .wrap(Wrap { trim: true }),
            pop_inner,
        );

        // Inline status bar.
        frame.render_widget(
            StatusBar::new()
                .left(Line::from(" t: fire a toast ").style(theme.caption()))
                .center(
                    Line::from(format!("next: {}", LEVELS[self.next].1))
                        .style(Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)),
                )
                .right(Line::from(format!(" tick {tick} ")).style(theme.caption()))
                .style(Style::new().fg(theme.dim).bg(theme.raised)),
            foot,
        );
    }
}

/// A plain rounded framing block.
fn framed(theme: &Theme, title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {title} ")).style(theme.caption()))
        .border_style(theme.border())
        .style(theme.body())
}
