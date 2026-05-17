//! A Kanban-board experience: four columns of cards. `↑/↓` pick a card,
//! `←/→` move the selected card to the previous / next column (carrying the
//! selection), `Enter` opens it. Column headers carry a count [`Badge`].
//!
//! **Mouse**: press a card and drag it into another column to move it — the
//! dragged card follows the pointer as a ghost and the column under it
//! highlights as the drop target; releasing over a different column moves
//! the card there (a release that did not cross a column is just a select).
//! The shell routes the press/drag/release here via the
//! [`on_press`](State::on_press) / [`on_pointer_drag`](State::on_pointer_drag)
//! / [`on_release`](State::on_release) seam (the same `Cell`/geometry mouse
//! discipline the rest of the kitchen sink uses).

use rstui_core::{Constraint, KeyCode, Layout, Line, Position, Rect, Style, stylize::Stylize};
use rstui_runtime::Frame;
use rstui_widgets::{Badge, BadgeLevel, Block, BorderType, Paragraph, Wrap};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

const COLUMNS: [&str; 4] = ["Backlog", "In Progress", "Review", "Done"];

/// An in-flight card drag: where it was picked up + where the pointer is.
#[derive(Debug, Clone, Copy)]
struct Drag {
    from_col: usize,
    from_row: usize,
    at: Position,
}

/// The board's caller-owned state: a card list per column, the cursor, and
/// the optional in-flight drag (all plain model state the pure `view` reads).
#[derive(Debug)]
pub(crate) struct State {
    cards: Vec<Vec<String>>,
    col: usize,
    row: usize,
    drag: Option<Drag>,
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
            drag: None,
        }
    }

    fn clamp(&mut self) {
        self.col = self.col.min(COLUMNS.len() - 1);
        let n = self.cards[self.col].len();
        self.row = if n == 0 { 0 } else { self.row.min(n - 1) };
    }

    /// The four column rects for a `content` area — the single source both
    /// `view` and the mouse hit-tests derive geometry from, so they can
    /// never disagree.
    fn columns(content: Rect) -> Vec<Rect> {
        Layout::horizontal([Constraint::Fill(1); 4]).split(content)
    }

    /// The `(column, card)` under `pos`, if any. Cards start 3 rows down
    /// (border + count) at 3 rows each — the inverse of `view`'s layout.
    fn card_at(&self, pos: Position, content: Rect) -> Option<(usize, usize)> {
        for (ci, c) in Self::columns(content).into_iter().enumerate() {
            if c.contains(pos) {
                let idx = usize::from(pos.y.saturating_sub(c.y + 3) / 3);
                if idx < self.cards[ci].len() {
                    return Some((ci, idx));
                }
            }
        }
        None
    }

    /// The column whose rect contains `pos` (the drop target).
    fn col_at(&self, pos: Position, content: Rect) -> Option<usize> {
        Self::columns(content).iter().position(|c| c.contains(pos))
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

    /// Click a card to select it (the non-drag click path the shell routes
    /// when a press did not land on a card).
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        if let Some((ci, idx)) = self.card_at(pos, content) {
            self.col = ci;
            self.row = idx;
            return ScreenOutcome::consumed();
        }
        ScreenOutcome::ignored()
    }

    /// Pointer pressed: if it landed on a card, select it and pick it up
    /// (return *consumed* so the shell routes the rest of the gesture here).
    /// Otherwise ignore it so the shell treats it as a plain click.
    pub(crate) fn on_press(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        if let Some((ci, idx)) = self.card_at(pos, content) {
            self.col = ci;
            self.row = idx;
            self.drag = Some(Drag {
                from_col: ci,
                from_row: idx,
                at: pos,
            });
            return ScreenOutcome::consumed();
        }
        self.drag = None;
        ScreenOutcome::ignored()
    }

    /// Pointer moved while carrying a card — track it for the ghost +
    /// drop-target highlight.
    pub(crate) fn on_pointer_drag(&mut self, pos: Position, _content: Rect) -> ScreenOutcome {
        if let Some(d) = &mut self.drag {
            d.at = pos;
            return ScreenOutcome::consumed();
        }
        ScreenOutcome::ignored()
    }

    /// Pointer released: drop the carried card into the column under it. A
    /// release that did not cross into another column is just the select
    /// the press already made.
    pub(crate) fn on_release(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let Some(d) = self.drag.take() else {
            return ScreenOutcome::ignored();
        };
        if let Some(dst) = self.col_at(pos, content) {
            if dst != d.from_col && d.from_row < self.cards[d.from_col].len() {
                let card = self.cards[d.from_col].remove(d.from_row);
                self.cards[dst].push(card);
                self.col = dst;
                self.row = self.cards[dst].len() - 1;
                return ScreenOutcome::with_toast(
                    crate::screens::ToastLevel::Info,
                    format!("Moved to {}", COLUMNS[dst]),
                );
            }
        }
        self.clamp();
        ScreenOutcome::consumed()
    }

    /// Draw the board.
    pub(crate) fn view(&self, theme: &Theme, _tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let cols = Self::columns(area);
        let levels = [
            BadgeLevel::Neutral,
            BadgeLevel::Info,
            BadgeLevel::Warning,
            BadgeLevel::Success,
        ];
        for (ci, col_rect) in cols.iter().enumerate() {
            // The column under the pointer while dragging is the drop target.
            let is_target = self
                .drag
                .as_ref()
                .is_some_and(|d| col_rect.contains(d.at) && ci != d.from_col);
            let active = ci == self.col || is_target;
            let block = Block::bordered()
                .border_type(BorderType::Rounded)
                .title(
                    Line::from(format!(
                        " {}{} ",
                        COLUMNS[ci],
                        if is_target { " ⤵" } else { "" }
                    ))
                    .style(if active {
                        theme.accent_text()
                    } else {
                        theme.caption()
                    }),
                )
                .border_style(if active {
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
                // 3-row stride == the `card_at` hit-test's `/ 3` (unchanged,
                // so click/drag still lands on the same card). The box is a
                // full 3 rows so a bordered card actually has a content row
                // for its title — a 2-row box left zero inner height, which
                // is why card text was missing.
                let y = inner.y + 2 + ri as u16 * 3;
                if y + 3 > inner.bottom() {
                    break;
                }
                let here = ci == self.col && ri == self.row;
                // The card being carried is dimmed in place (its ghost is
                // drawn under the pointer instead).
                let lifted = self
                    .drag
                    .as_ref()
                    .is_some_and(|d| d.from_col == ci && d.from_row == ri);
                let crect = Rect::new(inner.x, y, inner.width, 3);
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
                let text = if lifted {
                    card.clone().fg(theme.dim)
                } else if here {
                    card.clone().fg(theme.accent).bold()
                } else {
                    card.clone().fg(theme.text)
                };
                frame.render_widget(
                    Paragraph::new(Line::from(text))
                        .wrap(Wrap { trim: true })
                        .style(theme.body()),
                    ci_in,
                );
            }
        }

        // The dragged card as a ghost that follows the pointer.
        if let Some(d) = self.drag {
            if let Some(card) = self.cards.get(d.from_col).and_then(|c| c.get(d.from_row)) {
                let w = (card.chars().count() as u16 + 4).min(24).min(area.width);
                let h = 3u16.min(area.height);
                let gx = d.at.x.min(area.right().saturating_sub(w)).max(area.x);
                let gy = d.at.y.min(area.bottom().saturating_sub(h)).max(area.y);
                let grect = Rect::new(gx, gy, w, h);
                let gblock = Block::bordered()
                    .border_type(BorderType::Thick)
                    .border_style(theme.border_focused())
                    .style(Style::new().bg(theme.raised));
                let gin = gblock.inner(grect);
                frame.render_widget(gblock, grect);
                frame.render_widget(
                    Paragraph::new(Line::from(card.clone().fg(theme.accent).bold()))
                        .wrap(Wrap { trim: true })
                        .style(Style::new().bg(theme.raised)),
                    gin,
                );
            }
        }
    }
}
