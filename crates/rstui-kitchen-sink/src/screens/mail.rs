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

const INBOX: [Letter; 12] = [
    Letter {
        from: "Grace Hopper",
        subject: "Re: kitchen-sink content slice",
        when: "09:14",
        body: "Looks great. The grouped rail reads well and the chat composer is a genuinely nice touch — it is the kind of detail that makes the whole thing feel real instead of mocked.\n\nOne thing I want to flag before this lands: the reading pane needs a body long enough that the scroll actually means something. A two-line email never exercises the offset, and an unexercised path is an untested path. This message is deliberately several paragraphs so that you can hold the Down arrow in the reader and watch it move.\n\nThe scroll itself is unbounded in the reducer and clamped nowhere here, which matches the house pattern: presentation concerns live in the view. Over-scroll just shows the tail; that is acceptable for a reading pane and consistent with how the rest of the kitchen sink behaves.\n\nShip it once the gate is green. I do not need to see it again — the design is right and the behaviour is right. Just keep the slice small and do not let a content change drag a reducer change along with it unless it genuinely has to.\n\n— Grace",
    },
    Letter {
        from: "CI Bot",
        subject: "main is green (caabe3b)",
        when: "09:02",
        body: "Pipeline summary for commit caabe3b on main.\n\nGate: cargo xtask ci\n  fmt --all --check ............ pass\n  lint-names ................... pass\n  clippy --all-targets -D ...... pass\n  doc --no-deps -D warnings .... pass\n  test --all-features .......... pass (1 487 tests)\n\nExtra legs:\n  msrv ......................... pass\n  unused-deps (cargo-machete) .. pass\n  supply-chain (cargo-deny) .... pass\n  package (publish dry-run) .... pass\n\nNo action needed. This message is sent on every green main so the inbox has a realistic cadence of automated mail mixed in with the human threads. You can safely mark it read; the next push will send another.",
    },
    Letter {
        from: "Ada Lovelace",
        subject: "Experiences screens — keep them interactive",
        when: "Tue",
        body: "Ten composed scenes is ambitious, and the only way it works is if every one of them is genuinely interactive rather than a static screenshot dressed up as an app.\n\nThe chat composer takes real input and canned-replies so the thread stays live. The file explorer expands and collapses real subtrees. The mail client moves focus across three panes and marks messages read. The editor is a real multi-line buffer with a caret you can drive. None of that is faked, and that is the whole point — a kitchen sink that only looks like software teaches nobody anything.\n\nWhat I want to confirm in this slice: the bodies are now long enough that the scrolling is a real demonstration and not a token gesture. A reader that never scrolls is not testing the reader. Make the content earn its place.\n\nReply here once it is in and I will walk all four terminal sizes.\n\n— Ada",
    },
    Letter {
        from: "Linus",
        subject: "pure projection, restated",
        when: "Tue",
        body: "No retained tree is the right call and I will keep saying it until it is reflexive for everyone on the team.\n\nThe reducer owns all mutation. The view is a pure function of state. The frame boundary sits between them and nothing crosses it in the wrong direction — events become state, state becomes cells, and the two transformations never interleave. Every guarantee we make traces back to that one line drawn through the program.\n\nThe scroll work in this slice is a clean example. The reducer adds to an integer with saturating arithmetic and does not think about the end of the document. The view asks the widget how many rows it composes at the current width, subtracts the height, and clamps. Unbounded intent, bounded reality, decoupled by the frame.\n\nThat is the pattern. Reuse it; do not reinvent it. The log tail already did it, the rich-text reader now does it, and the next scrollable surface should look identical.\n\n— L",
    },
    Letter {
        from: "Katherine",
        subject: "design review — four sizes",
        when: "Tue",
        body: "Walked the experiences at 80x24, 120x40, 160x50, and 200x60.\n\nThe three-pane mail layout holds at the narrow size because the message list truncates with an ellipsis instead of wrapping — that is the correct behaviour and it is doing it. The folder rail is a fixed sixteen columns and stays legible. The reading pane gets whatever is left and reflows cleanly, which is exactly why a long body like the ones in this inbox is worth having: it proves the wrap is deterministic at every width.\n\nThe only note is cosmetic: the unread dot and star markers should stay in the accent colour so they read as status, not decoration. They do. Approved across all four sizes with no changes requested.\n\n— Katherine",
    },
    Letter {
        from: "Releases",
        subject: "v0.0.1 tagged",
        when: "Mon",
        body: "Tarballs published and the changelog is attached below.\n\nHighlights this cut: the full widget catalogue, the composition guide, the kitchen-sink experiences, the theme system, and the long-scroll reader work that this very inbox is helping to test.\n\nNothing in this release changes a public API in a breaking way. Upgrade is a version bump. The next milestone is the docs and media sync, which is deliberately not a CI gate so that a content slice like this one can land without waiting on the VHS toolchain.",
    },
    Letter {
        from: "Grace Hopper",
        subject: "scrollback realism",
        when: "Mon",
        body: "Following up from the channel: the chat scrollback is now long enough that the bottom-anchored window actually has something to scroll through, and the mail bodies are multi-paragraph so the reader offset is exercised.\n\nThis is the difference between a demo that looks complete and one that is complete. A reviewer who opens the mail client and finds one-line messages learns nothing about whether the reader works. A reviewer who finds this — paragraphs, a scrollbar's worth of text, a clean clamp at the tail — learns everything in about four seconds.\n\nKeep the content honest and the behaviour will speak for itself.",
    },
    Letter {
        from: "newgrad",
        subject: "question about the reader clamp",
        when: "Mon",
        body: "Reading the rich-text screen source to understand the scroll model and I think I follow it, but I want to check my understanding against a real reviewer.\n\nThe reducer increments the offset with saturating_add and never clamps. The view computes the maximum scroll as the composed row count minus the visible height and pins the offset to that before handing it to the widget. So the state can hold an offset larger than the document, but it can never be rendered larger than the last screenful, and as soon as the user scrolls back up the clamp stops applying and the offset is honoured exactly again.\n\nIs that right? And is the reason the clamp lives in the view rather than the reducer simply that the view is the only place that knows both the width and the height? That is the only place the row count is computable.\n\nThanks for indulging the question — the inbox needed another human thread anyway.",
    },
    Letter {
        from: "Ada Lovelace",
        subject: "Re: question about the reader clamp",
        when: "Mon",
        body: "Your understanding is exactly right, including the part most people miss: the clamp is not a permanent ceiling on the state, it is a per-frame projection decision. Scroll back up and the same offset that was clamped a moment ago is rendered verbatim again. Nothing was lost; the view simply chose not to show blank rows.\n\nAnd yes — the clamp lives in the view because the view is the only place that knows the geometry. The reducer has no idea how tall the panel is or how wide the text wrapped, and it should not have to. That separation is the whole reason the reducer stays simple and total. Good read of the code.\n\n— Ada",
    },
    Letter {
        from: "Linus",
        subject: "do not gate content on media",
        when: "Sun",
        body: "Reminder for the slice in flight: the VHS recordings are not a CI gate and must not become one. Regenerating GIFs needs the VHS toolchain, which not every contributor has, and a content change should never be blocked on a media refresh.\n\nLand the content green, then refresh the media as a separate follow-up via the docs skill. The gate is fmt, naming, clippy, doc, test. That is the contract. Keep it that way.",
    },
    Letter {
        from: "Katherine",
        subject: "truecolor theme swap — approved",
        when: "Sun",
        body: "The truecolor theme swap is a strong standalone demo and I am approving it, but please keep it in its own slice and out of the content enlargement. Two unrelated changes in one merge make a bad bisect later.\n\nThe content slice is about bodies and scroll. The theme slice is about palette. They touch different concerns and they should land separately even though both are green.",
    },
    Letter {
        from: "Grace Hopper",
        subject: "last one, I promise",
        when: "Sun",
        body: "This is the oldest message in the inbox and it exists mostly so the message list itself is long enough to be worth navigating with the arrow keys and the wheel.\n\nA mail client with six messages is a screenshot. A mail client with a dozen, where some are read and some are not, where two are starred and the rest are not, where the bodies vary from a single automated block to several human paragraphs — that is something you can actually evaluate.\n\nThank you for reading all the way to the bottom of the inbox. The fact that you could scroll here at all is the feature working.",
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
    /// Inbox open, the first message highlighted: the recent head of the
    /// inbox is unread, the older tail already read, and a couple starred —
    /// derived from [`INBOX`]'s length so the list can grow without
    /// re-hand-indexing these fixed-size flag arrays.
    pub(crate) fn new() -> Self {
        let mut read = [false; INBOX.len()];
        for r in read.iter_mut().skip(4) {
            *r = true;
        }
        let mut star = [false; INBOX.len()];
        star[1] = true;
        star[6] = true;
        Self {
            pane: Pane::List,
            folder: 0,
            msg: 0,
            read,
            star,
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

    /// A drag-select stays inside one pane — folders, the message list, or
    /// the reading pane — never across all three. Mirrors [`view`].
    pub(crate) fn selection_region(&self, pos: Position, content: Rect) -> Option<Rect> {
        let [_, body, _] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(content);
        Self::panes(body)
            .into_iter()
            .find(|r| r.contains(pos))
            .map(crate::screens::block_inner)
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
