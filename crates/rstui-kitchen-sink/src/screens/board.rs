//! A Kanban-board experience: four columns of cards. `↑/↓` pick a card,
//! `←/→` move the selected card to the previous / next column (carrying the
//! selection), `Enter` opens it. Column headers carry a count [`Badge`].

use rstui_core::{Constraint, KeyCode, Layout, Line, Position, Rect, Style, stylize::Stylize};
use rstui_runtime::Frame;
use rstui_widgets::{Badge, BadgeLevel, Block, BorderType, Paragraph, Wrap};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

const COLUMNS: [&str; 4] = ["Backlog", "In Progress", "Review", "Done"];

/// The board's caller-owned state: a card list per column + the cursor.
#[derive(Debug)]
pub(crate) struct State {
    cards: Vec<Vec<String>>,
    col: usize,
    row: usize,
}

impl State {
    /// A seeded board.
    pub(crate) fn new() -> Self {
        let cards = vec![
            vec![
                "Audit widget catalog".to_string(),
                "Design 10 experiences".to_string(),
                "Theme: light mode".to_string(),
            ],
            vec!["Chat composer".to_string(), "Grouped nav rail".to_string()],
            vec!["Merge-back protocol".to_string()],
            vec![
                "Kitchen-sink crate".to_string(),
                "Harness tests".to_string(),
            ],
        ];
        Self {
            cards,
            col: 0,
            row: 0,
        }
    }

    fn clamp(&mut self) {
        self.col = self.col.min(COLUMNS.len() - 1);
        let n = self.cards[self.col].len();
        self.row = if n == 0 { 0 } else { self.row.min(n - 1) };
    }

    /// `↑/↓` pick a card, `←/→` move it across columns, `Enter` opens it.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Up => self.row = self.row.saturating_sub(1),
            KeyCode::Down => {
                let n = self.cards[self.col].len();
                if n > 0 {
                    self.row = (self.row + 1).min(n - 1);
                }
            }
            KeyCode::Left | KeyCode::Right => {
                let dst = if code == KeyCode::Left {
                    if self.col == 0 {
                        return ScreenOutcome::ignored();
                    }
                    self.col - 1
                } else {
                    (self.col + 1).min(COLUMNS.len() - 1)
                };
                if dst != self.col && !self.cards[self.col].is_empty() {
                    let card = self.cards[self.col].remove(self.row);
                    self.cards[dst].push(card);
                    self.col = dst;
                    self.row = self.cards[dst].len() - 1;
                    return ScreenOutcome::with_toast(
                        crate::screens::ToastLevel::Info,
                        format!("Moved to {}", COLUMNS[dst]),
                    );
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(c) = self.cards[self.col].get(self.row) {
                    return ScreenOutcome::with_toast(
                        crate::screens::ToastLevel::Info,
                        format!("Open: {c}"),
                    );
                }
            }
            _ => return ScreenOutcome::ignored(),
        }
        self.clamp();
        ScreenOutcome::consumed()
    }

    /// Click a card to select it.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let cols = Layout::horizontal([Constraint::Fill(1); 4]).split(content);
        for (ci, c) in cols.iter().enumerate() {
            if c.contains(pos) {
                // Cards start 3 rows down (header + count) at 3 rows each.
                let rel = pos.y.saturating_sub(c.y + 3);
                let idx = (rel / 3) as usize;
                if idx < self.cards[ci].len() {
                    self.col = ci;
                    self.row = idx;
                    return ScreenOutcome::consumed();
                }
            }
        }
        ScreenOutcome::ignored()
    }

    /// Draw the board.
    pub(crate) fn view(&self, theme: &Theme, _tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let cols = Layout::horizontal([Constraint::Fill(1); 4]).split(area);
        let levels = [
            BadgeLevel::Neutral,
            BadgeLevel::Info,
            BadgeLevel::Warning,
            BadgeLevel::Success,
        ];
        for (ci, col_rect) in cols.iter().enumerate() {
            let block = Block::bordered()
                .border_type(BorderType::Rounded)
                .title(
                    Line::from(format!(" {} ", COLUMNS[ci])).style(if ci == self.col {
                        theme.accent_text()
                    } else {
                        theme.caption()
                    }),
                )
                .border_style(if ci == self.col {
                    theme.border_focused()
                } else {
                    theme.border()
                })
                .style(theme.body());
            let inner = block.inner(*col_rect);
            frame.render_widget(block, *col_rect);

            frame.render_widget(
                Badge::new(format!("{} cards", self.cards[ci].len())).level(levels[ci]),
                Rect::new(inner.x, inner.y, inner.width, 1),
            );

            for (ri, card) in self.cards[ci].iter().enumerate() {
                let y = inner.y + 2 + ri as u16 * 3;
                if y + 2 > inner.bottom() {
                    break;
                }
                let here = ci == self.col && ri == self.row;
                let crect = Rect::new(inner.x, y, inner.width, 2);
                let cblock = Block::bordered()
                    .border_type(if here {
                        BorderType::Thick
                    } else {
                        BorderType::Plain
                    })
                    .border_style(if here {
                        theme.border_focused()
                    } else {
                        theme.border()
                    })
                    .style(Style::new().bg(if here { theme.raised } else { theme.surface }));
                let ci_in = cblock.inner(crect);
                frame.render_widget(cblock, crect);
                frame.render_widget(
                    Paragraph::new(Line::from(if here {
                        card.clone().fg(theme.accent).bold()
                    } else {
                        card.clone().fg(theme.text)
                    }))
                    .wrap(Wrap { trim: true })
                    .style(theme.body()),
                    ci_in,
                );
            }
        }
    }
}
