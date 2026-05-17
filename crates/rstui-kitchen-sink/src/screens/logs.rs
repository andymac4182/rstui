//! A live log / terminal-tail experience: a stream that grows with `tick`,
//! a `grep`-style [`Input`] filter, level colouring, a [`Scrollbar`], and a
//! [`StatusBar`]. Type to filter, `↑/↓`/`PgUp`/`PgDn` scroll (pausing the
//! tail), `End` re-follows, `Home` jumps to the top.

use rstui_core::{
    Constraint, KeyCode, Layout, Line, Position, Rect, Style, TextEdit, stylize::Stylize,
};
use rstui_runtime::Frame;
use rstui_widgets::{Block, BorderType, Input, Scrollbar, ScrollbarOrientation, StatusBar};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The deterministic message pool the synthetic stream draws from.
const POOL: [(u8, &str); 10] = [
    (0, "GET /api/widgets 200 12ms"),
    (0, "render frame ok (cells=4096)"),
    (1, "slow paint: 34ms over budget"),
    (0, "focus moved: rail -> screen"),
    (2, "backend error: device not configured"),
    (0, "event: MouseDown(Left) @ (12,4)"),
    (1, "retrying connection (attempt 2)"),
    (0, "tick: spinner advanced"),
    (2, "panic guard: terminal restored"),
    (0, "merge-check: all gates green"),
];

/// The log viewer's caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    query: TextEdit,
    /// Lines scrolled back from the tail; `0` + follow = pinned to newest.
    scroll: usize,
    follow: bool,
}

impl State {
    /// Following the tail, no filter.
    pub(crate) fn new() -> Self {
        Self {
            query: TextEdit::new(),
            scroll: 0,
            follow: true,
        }
    }

    /// The whole synthetic stream up to `tick` (deterministic).
    fn stream(tick: u64) -> Vec<(u8, String)> {
        let count = (tick as usize).min(600);
        (0..count)
            .map(|i| {
                let (lvl, msg) = POOL[(i * 7 + i / 5) % POOL.len()];
                (lvl, format!("{:>5}  {msg}", i + 1))
            })
            .collect()
    }

    /// Apply the live substring filter.
    fn filtered(&self, all: Vec<(u8, String)>) -> Vec<(u8, String)> {
        let q = self.query.value().to_lowercase();
        if q.is_empty() {
            return all;
        }
        all.into_iter()
            .filter(|(_, m)| m.to_lowercase().contains(&q))
            .collect()
    }

    /// Type to filter; arrows / pages scroll (pausing follow); `End`
    /// re-follows; `Home` jumps to the top.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Up => {
                self.follow = false;
                self.scroll += 1;
            }
            KeyCode::Down => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageUp => {
                self.follow = false;
                self.scroll += 10;
            }
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::End => {
                self.follow = true;
                self.scroll = 0;
            }
            KeyCode::Home => {
                self.follow = false;
                self.scroll = usize::MAX / 2;
            }
            KeyCode::Backspace => {
                self.query.delete_backward();
            }
            KeyCode::Left => {
                if self.query.value().is_empty() {
                    return ScreenOutcome::ignored();
                }
                self.query.move_left();
            }
            KeyCode::Right => {
                self.query.move_right();
            }
            KeyCode::Char(c) => self.query.insert_char(c),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Wheel scroll (pauses follow on the way up).
    pub(crate) fn on_scroll(&mut self, up: bool) {
        if up {
            self.follow = false;
            self.scroll += 3;
        } else {
            self.scroll = self.scroll.saturating_sub(3);
            if self.scroll == 0 {
                self.follow = true;
            }
        }
    }

    /// Draw the live tail. `tick` grows the stream.
    pub(crate) fn view(&self, theme: &Theme, tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [bar, body, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);

        // Filter bar.
        let [flabel, finput, fstate] = Layout::horizontal([
            Constraint::Length(9),
            Constraint::Fill(1),
            Constraint::Length(14),
        ])
        .areas(bar);
        frame.render_widget(Line::from(" filter: ").style(theme.caption()), flabel);
        frame.render_widget(
            Input::new(&self.query)
                .focused(true)
                .placeholder("substring — e.g. error")
                .style(theme.body())
                .focus_style(Style::new().fg(theme.text).bg(theme.surface))
                .cursor_style(Style::new().fg(theme.base).bg(theme.accent))
                .placeholder_style(theme.caption()),
            finput,
        );
        let follow_tag = if self.follow {
            "● TAIL".to_string()
        } else {
            "‖ PAUSED".to_string()
        };
        frame.render_widget(
            Line::from(follow_tag.fg(if self.follow { theme.ok } else { theme.warn }))
                .style(theme.body()),
            fstate,
        );

        // Log body.
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Line::from(" application.log ").style(theme.caption()))
            .border_style(theme.border())
            .style(theme.body());
        let inner = block.inner(body);
        frame.render_widget(block, body);

        let all = Self::stream(tick);
        let total = all.len();
        let lines = self.filtered(all);
        let shown = lines.len();
        let h = inner.height as usize;
        let max_scroll = shown.saturating_sub(h);
        let scroll = if self.follow {
            0
        } else {
            self.scroll.min(max_scroll)
        };
        let end = shown.saturating_sub(scroll);
        let start = end.saturating_sub(h);

        let q = self.query.value().to_lowercase();
        for (vi, (lvl, msg)) in lines[start..end].iter().enumerate() {
            let (tag, col) = match lvl {
                1 => ("WARN ", theme.warn),
                2 => ("ERROR", theme.err),
                _ => ("INFO ", theme.ok),
            };
            let mut spans = vec![format!("{tag} ").fg(col).bold(), msg.clone().fg(theme.text)];
            if !q.is_empty() {
                spans.push(format!("   ⟵ {q}").fg(theme.accent_alt));
            }
            frame.render_widget(
                Line::from(spans),
                Rect::new(
                    inner.x,
                    inner.y + vi as u16,
                    inner.width.saturating_sub(1),
                    1,
                ),
            );
        }
        frame.render_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .content_length(shown)
                .viewport_length(h)
                .position(start)
                .style(theme.border())
                .thumb_style(Style::new().fg(theme.accent)),
            Rect::new(inner.right().saturating_sub(1), inner.y, 1, inner.height),
        );

        frame.render_widget(
            StatusBar::new()
                .left(Line::from(format!(" {shown}/{total} lines ")).style(theme.caption()))
                .center(
                    Line::from("type to filter · ↑↓ PgUp/Dn scroll · End follow")
                        .style(theme.caption()),
                )
                .right(
                    Line::from(if self.follow { " tailing " } else { " paused " }.to_string())
                        .style(theme.caption()),
                )
                .style(Style::new().fg(theme.dim).bg(theme.raised)),
            foot,
        );
    }
}

/// A drag-select stays inside the log body (the framed stream), never the
/// filter bar or the status footer. Mirrors [`State::view`]'s split.
pub(crate) fn selection_region(pos: Position, content: Rect) -> Option<Rect> {
    let [_, body, _] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(content);
    body.contains(pos)
        .then(|| crate::screens::block_inner(body))
}
