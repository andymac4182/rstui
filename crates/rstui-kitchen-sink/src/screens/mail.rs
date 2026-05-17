//! A three-pane email client: a folder [`List`], a message [`List`] with
//! unread/star markers, and a reading pane ([`Block`] + header
//! [`DescriptionList`] + [`Paragraph`] body), tied together by a live
//! [`Breadcrumb`] and a [`StatusBar`]. `←/→` move focus across the panes.

use rstui_core::{Constraint, KeyCode, Layout, Line, Position, Rect, Style, stylize::Stylize};
use rstui_runtime::Frame;
use rstui_widgets::{
    Block, BorderType, Breadcrumb, DescriptionList, DescriptionRow, List, Paragraph, StatusBar,
    Wrap,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The mailbox folders and their unread counts.
const FOLDERS: [(&str, char); 5] = [
    ("Inbox", '✉'),
    ("Starred", '★'),
    ("Sent", '➤'),
    ("Drafts", '✎'),
    ("Trash", '🗑'),
];

/// One message.
struct Letter {
    from: &'static str,
    subject: &'static str,
    when: &'static str,
    body: &'static str,
}

const INBOX: [Letter; 6] = [
    Letter {
        from: "Grace Hopper",
        subject: "Re: kitchen-sink merge",
        when: "09:14",
        body: "Looks great. The grouped rail reads well and the chat composer is a nice touch. Ship it.",
    },
    Letter {
        from: "CI Bot",
        subject: "✓ main is green (caabe3b)",
        when: "09:02",
        body: "All 5 gates passed plus msrv / unused-deps / supply-chain. No action needed.",
    },
    Letter {
        from: "Ada Lovelace",
        subject: "Experiences screens",
        when: "Tue",
        body: "Ten composed scenes is ambitious — make sure each is interactive, not a static mock.",
    },
    Letter {
        from: "Linus",
        subject: "pure projection",
        when: "Tue",
        body: "No retained tree is the right call. Keep reducers owning all mutation.",
    },
    Letter {
        from: "Releases",
        subject: "v0.0.1 tagged",
        when: "Mon",
        body: "Tarballs published. Changelog attached.",
    },
    Letter {
        from: "Katherine",
        subject: "design review",
        when: "Mon",
        body: "The truecolor theme swap is a strong demo. Approved.",
    },
];

/// Which pane owns `↑/↓`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Folders,
    List,
    Reader,
}

/// The mail client's caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    pane: Pane,
    folder: usize,
    msg: usize,
    read: [bool; INBOX.len()],
    star: [bool; INBOX.len()],
    /// Reader scroll offset.
    scroll: u16,
}

impl State {
    /// Inbox open, nothing read, the first message highlighted.
    pub(crate) fn new() -> Self {
        let mut read = [false; INBOX.len()];
        read[4] = true;
        read[5] = true;
        Self {
            pane: Pane::List,
            folder: 0,
            msg: 0,
            read,
            star: [false, true, false, false, false, false],
            scroll: 0,
        }
    }

    fn unread(&self) -> usize {
        self.read.iter().filter(|r| !**r).count()
    }

    /// `←/→` move focus across panes, `↑/↓` move within the focused pane,
    /// `Enter` opens (marking read), `s` stars.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Left => match self.pane {
                Pane::Folders => return ScreenOutcome::ignored(),
                Pane::List => self.pane = Pane::Folders,
                Pane::Reader => self.pane = Pane::List,
            },
            KeyCode::Right => {
                self.pane = match self.pane {
                    Pane::Folders => Pane::List,
                    Pane::List | Pane::Reader => Pane::Reader,
                }
            }
            KeyCode::Up => self.step(-1),
            KeyCode::Down => self.step(1),
            KeyCode::Enter => {
                if self.pane == Pane::Folders {
                    self.pane = Pane::List;
                } else {
                    self.read[self.msg] = true;
                    self.pane = Pane::Reader;
                    self.scroll = 0;
                    return ScreenOutcome::with_toast(
                        crate::screens::ToastLevel::Info,
                        format!("Opened: {}", INBOX[self.msg].subject),
                    );
                }
            }
            KeyCode::Char('s') => {
                self.star[self.msg] = !self.star[self.msg];
            }
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    fn step(&mut self, d: i32) {
        match self.pane {
            Pane::Folders => {
                let n = FOLDERS.len() as i32;
                self.folder = ((self.folder as i32 + d).rem_euclid(n)) as usize;
            }
            Pane::List => {
                let n = INBOX.len() as i32;
                self.msg = ((self.msg as i32 + d).rem_euclid(n)) as usize;
                self.scroll = 0;
            }
            Pane::Reader => {
                if d < 0 {
                    self.scroll = self.scroll.saturating_sub(1);
                } else {
                    self.scroll = self.scroll.saturating_add(1);
                }
            }
        }
    }

    /// Click a folder or message row to select it.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [folders, list, _] = Self::panes(content);
        if folders.contains(pos) {
            let r = pos.y.saturating_sub(folders.y + 1) as usize;
            if r < FOLDERS.len() {
                self.folder = r;
                self.pane = Pane::Folders;
                return ScreenOutcome::consumed();
            }
        }
        if list.contains(pos) {
            let r = pos.y.saturating_sub(list.y + 1) as usize;
            if r < INBOX.len() {
                self.msg = r;
                self.read[r] = true;
                self.pane = Pane::Reader;
                return ScreenOutcome::consumed();
            }
        }
        ScreenOutcome::ignored()
    }

    /// Wheel scrolls the reader.
    pub(crate) fn on_scroll(&mut self, up: bool) {
        if up {
            self.scroll = self.scroll.saturating_sub(1);
        } else {
            self.scroll = self.scroll.saturating_add(1);
        }
    }

    /// folders | message list | reader.
    fn panes(area: Rect) -> [Rect; 3] {
        Layout::horizontal([
            Constraint::Length(16),
            Constraint::Percentage(42),
            Constraint::Fill(1),
        ])
        .areas(area)
    }

    /// Draw the mail client.
    pub(crate) fn view(&self, theme: &Theme, _tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [top, body, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);

        let crumb = [
            Line::from("Mail".fg(theme.dim)),
            Line::from(FOLDERS[self.folder].0.fg(theme.dim)),
            Line::from(INBOX[self.msg].subject.fg(theme.text)),
        ];
        frame.render_widget(
            Breadcrumb::new(&crumb)
                .separator('›')
                .style(theme.caption())
                .emphasis_style(theme.accent_text()),
            top,
        );

        let [folders, list, reader] = Self::panes(body);

        let fitems: Vec<Line> = FOLDERS
            .iter()
            .enumerate()
            .map(|(i, (name, icon))| {
                let tag = if i == 0 && self.unread() > 0 {
                    format!("  {}", self.unread())
                } else {
                    String::new()
                };
                Line::from(vec![
                    format!("{icon} ").fg(theme.dim),
                    (*name).fg(theme.text),
                    tag.fg(theme.base).bg(theme.accent),
                ])
            })
            .collect();
        frame.render_widget(
            List::new(fitems)
                .selected(Some(self.folder))
                .highlight_style(theme.selection())
                .style(theme.body())
                .block(framed(theme, "Folders", self.pane == Pane::Folders)),
            folders,
        );

        let mitems: Vec<Line> = INBOX
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let dot = if self.read[i] { ' ' } else { '●' };
                let star = if self.star[i] { '★' } else { ' ' };
                let name_style = if self.read[i] {
                    Style::new().fg(theme.dim)
                } else {
                    Style::new()
                        .fg(theme.text)
                        .add_modifier(rstui_core::Modifier::BOLD)
                };
                Line::from(vec![
                    format!("{dot}{star} ").fg(theme.accent),
                    rstui_core::Span::styled(format!("{:<14}", trunc(m.from, 14)), name_style),
                    format!(" {}", trunc(m.subject, 26)).fg(theme.text),
                    format!("  {}", m.when).fg(theme.dim),
                ])
            })
            .collect();
        frame.render_widget(
            List::new(mitems)
                .selected(Some(self.msg))
                .highlight_symbol("▌")
                .highlight_style(theme.selection())
                .style(theme.body())
                .block(framed(
                    theme,
                    FOLDERS[self.folder].0,
                    self.pane == Pane::List,
                )),
            list,
        );

        let m = &INBOX[self.msg];
        let rblock = framed(theme, "Reading", self.pane == Pane::Reader);
        let rin = rblock.inner(reader);
        frame.render_widget(rblock, reader);
        let [head, divider, text] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(rin);
        frame.render_widget(
            DescriptionList::new([
                DescriptionRow::new("From", m.from.to_string()),
                DescriptionRow::new("Subject", m.subject.to_string()),
                DescriptionRow::new("When", m.when.to_string()),
            ])
            .key_style(theme.caption())
            .value_style(theme.body())
            .style(theme.body()),
            head,
        );
        frame.render_widget(
            Line::from("─".repeat(divider.width as usize)).style(theme.border()),
            divider,
        );
        frame.render_widget(
            Paragraph::new(m.body)
                .scroll(rstui_core::Position::new(0, self.scroll))
                .wrap(Wrap { trim: true })
                .style(theme.body()),
            text,
        );

        frame.render_widget(
            StatusBar::new()
                .left(
                    Line::from(format!(
                        " {} · {} unread ",
                        FOLDERS[self.folder].0,
                        self.unread()
                    ))
                    .style(theme.caption()),
                )
                .center(
                    Line::from("←→ pane · ↑↓ move · Enter open · s star").style(theme.caption()),
                )
                .right(
                    Line::from(format!(" {}/{} ", self.msg + 1, INBOX.len()))
                        .style(theme.caption()),
                )
                .style(Style::new().fg(theme.dim).bg(theme.raised)),
            foot,
        );
    }
}

/// Truncate with an ellipsis to `max` chars.
fn trunc(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// A rounded panel whose border brightens when it owns the keyboard.
fn framed(theme: &Theme, title: &str, focused: bool) -> Block<'static> {
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
