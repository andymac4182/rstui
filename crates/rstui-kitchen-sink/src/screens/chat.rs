//! A chat / messenger experience: a [`List`] of channels with unread
//! [`Badge`]s, a scrolling bubble thread, a presence line with a live
//! [`Spinner`] typing indicator, and a real editable composer
//! ([`Input`] over a [`TextEdit`]). Type and press Enter to send — the
//! peer canned-replies so the thread is always live.

use rstui_core::{
    Constraint, KeyCode, Layout, Line, Position, Rect, Style, TextEdit, stylize::Stylize,
};
use rstui_runtime::Frame;
use rstui_widgets::{Block, BorderType, Input, List, Paragraph, Spinner, Wrap};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// One line in a thread.
#[derive(Debug, Clone)]
struct ChatMsg {
    /// `true` if I sent it (drawn right-aligned, accent).
    mine: bool,
    /// Display sender.
    who: String,
    /// Body text.
    text: String,
}

/// One conversation: a name, an unread count, and its messages.
#[derive(Debug)]
struct Channel {
    name: &'static str,
    /// `true` for a room (`#`), `false` for a direct message.
    room: bool,
    unread: u32,
    msgs: Vec<ChatMsg>,
}

fn seed(name: &'static str, room: bool, unread: u32, msgs: &[(bool, &str, &str)]) -> Channel {
    Channel {
        name,
        room,
        unread,
        msgs: msgs
            .iter()
            .map(|(mine, who, text)| ChatMsg {
                mine: *mine,
                who: (*who).to_string(),
                text: (*text).to_string(),
            })
            .collect(),
    }
}

/// The chat client's caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    channels: Vec<Channel>,
    active: usize,
    composer: TextEdit,
    /// Lines scrolled back from the latest (0 = pinned to newest).
    scroll: u16,
    /// Set on send, cleared on the next key — drives the typing indicator.
    awaiting: bool,
}

impl State {
    /// A seeded multi-channel client.
    pub(crate) fn new() -> Self {
        Self {
            channels: vec![
                seed(
                    "#general",
                    true,
                    0,
                    &[
                        (false, "Grace", "Morning! ship the rich-rendering slice?"),
                        (false, "Ada", "Gates are green on my end."),
                        (true, "you", "Merging now — kitchen sink is live."),
                    ],
                ),
                seed(
                    "#rust",
                    true,
                    3,
                    &[
                        (false, "Linus", "pure projection is the right call"),
                        (false, "Ada", "no retained tree, no surprises"),
                    ],
                ),
                seed("#random", true, 0, &[(false, "Grace", "coffee?")]),
                seed(
                    "Ada Lovelace",
                    false,
                    1,
                    &[(false, "Ada", "ping me when the demo's ready")],
                ),
                seed(
                    "Grace Hopper",
                    false,
                    0,
                    &[(false, "Grace", "nice work on the rail")],
                ),
            ],
            active: 0,
            composer: TextEdit::new(),
            scroll: 0,
            awaiting: false,
        }
    }

    /// The current conversation.
    fn chan(&self) -> &Channel {
        &self.channels[self.active]
    }

    /// Switch conversation, clearing its unread badge.
    fn select(&mut self, idx: usize) {
        self.active = idx.min(self.channels.len() - 1);
        self.channels[self.active].unread = 0;
        self.scroll = 0;
    }

    /// Send the composer, append a canned peer reply.
    fn send(&mut self) -> ScreenOutcome {
        let body = self.composer.value().trim().to_string();
        if body.is_empty() {
            return ScreenOutcome::consumed();
        }
        let peer = self.chan().peer().to_string();
        let reply = canned_reply(&body, &peer);
        let ch = &mut self.channels[self.active];
        ch.msgs.push(ChatMsg {
            mine: true,
            who: "you".to_string(),
            text: body,
        });
        ch.msgs.push(ChatMsg {
            mine: false,
            who: peer,
            text: reply,
        });
        self.composer = TextEdit::new();
        self.scroll = 0;
        self.awaiting = true;
        ScreenOutcome::consumed()
    }

    /// Keys: type into the composer, Enter sends, ↑↓ switch channels,
    /// ←→ move the caret, PgUp/PgDn scroll the thread.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        self.awaiting = false;
        match code {
            KeyCode::Enter => return self.send(),
            KeyCode::Up => {
                let i = self.active.saturating_sub(1);
                self.select(i);
            }
            KeyCode::Down => {
                let i = self.active + 1;
                self.select(i.min(self.channels.len() - 1));
            }
            KeyCode::PageUp => self.scroll = self.scroll.saturating_add(3),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(3),
            KeyCode::Left => {
                self.composer.move_left();
            }
            KeyCode::Right => {
                self.composer.move_right();
            }
            KeyCode::Backspace => {
                self.composer.delete_backward();
            }
            KeyCode::Char(c) => self.composer.insert_char(c),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// A click on a channel row selects it.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [rail, _] = Self::split(content);
        if rail.contains(pos) {
            let row = pos.y.saturating_sub(rail.y + 1) as usize;
            if row < self.channels.len() {
                self.select(row);
                return ScreenOutcome::consumed();
            }
        }
        ScreenOutcome::ignored()
    }

    /// Wheel scrolls the thread.
    pub(crate) fn on_scroll(&mut self, up: bool) {
        if up {
            self.scroll = self.scroll.saturating_add(2);
        } else {
            self.scroll = self.scroll.saturating_sub(2);
        }
    }

    /// Pasted text drops into the composer.
    pub(crate) fn on_paste(&mut self, text: &str) {
        self.composer.insert_str(&text.replace('\n', " "));
    }

    /// Cut `sel` out of the composer.
    pub(crate) fn cut(&mut self, sel: &str) -> bool {
        crate::screens::cut_field(&mut self.composer, sel)
    }

    /// The channel rail / conversation split.
    fn split(area: Rect) -> [Rect; 2] {
        Layout::horizontal([Constraint::Length(20), Constraint::Fill(1)]).areas(area)
    }

    /// Draw the chat client. `tick` animates the typing spinner.
    pub(crate) fn view(&self, theme: &Theme, tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [rail, convo] = Self::split(area);

        // Channel rail with unread badges.
        let items: Vec<Line> = self
            .channels
            .iter()
            .map(|c| {
                let glyph = if c.room { '#' } else { '✦' };
                let mut spans = vec![
                    format!("{glyph} ").fg(theme.dim),
                    c.name.trim_start_matches('#').fg(theme.text),
                ];
                if c.unread > 0 {
                    spans.push(format!("  {}", c.unread).fg(theme.base).bg(theme.accent));
                }
                Line::from(spans)
            })
            .collect();
        frame.render_widget(
            List::new(items)
                .selected(Some(self.active))
                .highlight_symbol("▌")
                .highlight_style(theme.selection())
                .style(theme.body())
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .title(Line::from(" Channels ").style(theme.heading()))
                        .border_style(theme.border())
                        .style(theme.body()),
                ),
            rail,
        );

        let [header, thread, composer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(3),
        ])
        .areas(convo);

        // Presence header + live typing indicator.
        let ch = self.chan();
        frame.buffer_mut().set_style(header, theme.body());
        frame.render_widget(
            Line::from(vec![
                format!(" {} ", ch.name).fg(theme.accent).bold(),
                if ch.room {
                    format!("· {} members", 4 + self.active).fg(theme.dim)
                } else {
                    "· online".fg(theme.ok)
                },
            ]),
            header,
        );
        if self.awaiting && !ch.room {
            let label = format!("{} is typing ", ch.peer());
            let lw = label.len() as u16;
            let lx = header.right().saturating_sub(lw + 3);
            frame.render_widget(
                Line::from(label.fg(theme.dim)),
                Rect::new(lx, header.y, lw, 1),
            );
            frame.render_widget(
                Spinner::new()
                    .tick(tick as usize)
                    .style(Style::new().fg(theme.accent)),
                Rect::new(header.right().saturating_sub(2), header.y, 1, 1),
            );
        }

        // The bubble thread (bottom-anchored, scrollable).
        let lines = self.thread_lines(theme, thread.width);
        let vh = thread.height as usize;
        let total = lines.len();
        let end = total.saturating_sub(self.scroll as usize);
        let start = end.saturating_sub(vh);
        let window: Vec<Line> = lines[start..end.max(start)].to_vec();
        frame.render_widget(
            Paragraph::new(window)
                .style(theme.body())
                .wrap(Wrap { trim: false }),
            thread,
        );

        // Composer.
        let composer_inner = inner_box(
            frame,
            theme,
            composer,
            &format!(" to {} ", self.chan().name),
        );
        frame.render_widget(
            Input::new(&self.composer)
                .focused(true)
                .placeholder("Write a message · Enter sends")
                .style(theme.body())
                .focus_style(Style::new().fg(theme.text).bg(theme.surface))
                .cursor_style(Style::new().fg(theme.base).bg(theme.accent))
                .placeholder_style(theme.caption()),
            composer_inner,
        );
    }

    /// The thread rendered as styled lines: mine right-aligned & accented,
    /// the peer's left with a dim sender tag.
    fn thread_lines(&self, theme: &Theme, width: u16) -> Vec<Line<'static>> {
        let w = width.max(8) as usize;
        let mut out = Vec::new();
        for m in &self.chan().msgs {
            if m.mine {
                let body = format!("{}  ‹ you", m.text);
                let pad = w.saturating_sub(body.chars().count());
                out.push(Line::from(vec![
                    " ".repeat(pad).into(),
                    m.text.clone().fg(theme.base).bg(theme.accent).bold(),
                    "  ‹ you".fg(theme.dim),
                ]));
            } else {
                out.push(Line::from(vec![
                    format!("{} › ", m.who).fg(theme.accent_alt).bold(),
                    m.text.clone().fg(theme.text),
                ]));
            }
            out.push(Line::from(""));
        }
        out
    }
}

impl Channel {
    /// The display name of the other party.
    fn peer(&self) -> &str {
        if self.room {
            "room"
        } else {
            self.name.split_whitespace().next().unwrap_or(self.name)
        }
    }
}

/// A deterministic canned reply (no async, so tests stay deterministic).
fn canned_reply(body: &str, peer: &str) -> String {
    let lc = body.to_lowercase();
    if lc.contains('?') {
        format!("Good question — let me check, {peer} here.")
    } else if lc.contains("ship") || lc.contains("merge") || lc.contains("done") {
        "shipped — nice work.".to_string()
    } else {
        let snip: String = body.chars().take(28).collect();
        format!("Noted: \"{snip}\"")
    }
}

/// Draws a rounded titled box into `area` and returns the one-row inner rect
/// for the composer input.
fn inner_box(frame: &mut Frame<'_>, theme: &Theme, area: Rect, title: &str) -> Rect {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(title.to_string()).style(theme.caption()))
        .border_style(theme.border_focused())
        .style(theme.body());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

/// A drag-select stays inside the message thread (a block-less
/// [`Paragraph`], so no inset) — never the channel rail or the composer.
pub(crate) fn selection_region(pos: Position, content: Rect) -> Option<Rect> {
    let [_rail, convo] = State::split(content);
    let [_, thread, _] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(3),
    ])
    .areas(convo);
    thread.contains(pos).then_some(thread)
}
