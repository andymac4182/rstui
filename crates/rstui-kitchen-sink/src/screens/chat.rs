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
                        (
                            false,
                            "Grace",
                            "Morning all — standup in 10. What landed overnight?",
                        ),
                        (
                            false,
                            "Ada",
                            "The rich-text reader got the long-scroll fix. Offset is unbounded in the reducer now, clamped in the view against the composed row count.",
                        ),
                        (
                            false,
                            "Ada",
                            "Same idiom the live log tail already uses, so it reads as the house style rather than a one-off.",
                        ),
                        (
                            true,
                            "you",
                            "Nice. Did the markdown tab get the same treatment?",
                        ),
                        (
                            false,
                            "Ada",
                            "Yes — Markdown::lines(width).len() feeds the same clamp. Links still hit-test against the clamped offset so a click lands on exactly the drawn label.",
                        ),
                        (
                            false,
                            "Grace",
                            "Gates green on my branch. fmt, naming, clippy -D warnings, doc -D warnings, full test suite.",
                        ),
                        (
                            false,
                            "Linus",
                            "Did anyone measure the per-frame cost with the long document loaded?",
                        ),
                        (
                            false,
                            "Ada",
                            "It does not move. The cost tracks the visible window, not the document length — composing rows you scrolled past is work the widget never does.",
                        ),
                        (
                            false,
                            "Linus",
                            "That is the whole point of the shape. Good.",
                        ),
                        (
                            true,
                            "you",
                            "Pushing the kitchen-sink content enlargement now. Five screens get realistic bodies so we can actually feel the scrolling.",
                        ),
                        (false, "Grace", "Which five?"),
                        (
                            true,
                            "you",
                            "Rich Text, Chat, Mail, Files, Code Editor. This thread is part of the test, by the way.",
                        ),
                        (false, "Grace", "Meta. I approve."),
                        (
                            false,
                            "Katherine",
                            "Design note: keep the unread badges legible at the 80x24 size. They wrap badly under 16 columns of rail.",
                        ),
                        (
                            false,
                            "Ada",
                            "Rail is a fixed 20 columns, so we are fine. The thread is the part that reflows.",
                        ),
                        (
                            false,
                            "Katherine",
                            "Then we are good. Shipping the truecolor theme swap demo alongside?",
                        ),
                        (
                            true,
                            "you",
                            "Separate slice. This one is purely content + the scroll clamp.",
                        ),
                        (
                            false,
                            "Linus",
                            "Keep the slices small. A content change should never need a reducer change to land.",
                        ),
                        (
                            true,
                            "you",
                            "This one needed exactly one: removing the artificial .min(60) cap that capped scroll at 60 rows. Hard to test long scrolling against a 60-row ceiling.",
                        ),
                        (false, "Linus", "Fair. That was always a placeholder."),
                        (
                            false,
                            "Grace",
                            "CI bot says main is green at the last merge. Go.",
                        ),
                        (true, "you", "Merging."),
                        (
                            false,
                            "Ada",
                            "While you are in there — the Files preview had no scroll at all. Worth wiring the wheel + PageUp/PageDown so long previews are not just clipped.",
                        ),
                        (
                            true,
                            "you",
                            "Already in the slice. Wheel + paging, scroll resets when the selection changes so a new file opens at the top.",
                        ),
                        (false, "Ada", "Perfect."),
                        (false, "Grace", "Standup proper: blockers?"),
                        (false, "Linus", "None. Reviewing the editor buffers next."),
                        (false, "Ada", "None. On the docs sync after this lands."),
                        (
                            true,
                            "you",
                            "None. Content slice is the only thing in flight from me.",
                        ),
                        (false, "Grace", "Short and green. Best kind. Back to it."),
                        (
                            false,
                            "Katherine",
                            "One more — can we get a longer sample email so the reading pane scroll is exercised too?",
                        ),
                        (
                            true,
                            "you",
                            "Mail is in the slice. Twelve letters, multi-paragraph bodies, the reader was already unbounded so it just works.",
                        ),
                        (false, "Katherine", "Then I have nothing. Thank you."),
                        (
                            false,
                            "Grace",
                            "Closing standup. Ping here if anything goes red.",
                        ),
                        (true, "you", "Will do."),
                        (
                            false,
                            "Ada",
                            "It will not. The shape does not have a slow path to fall off.",
                        ),
                        (false, "Linus", "Famous last words. But correct ones."),
                        (
                            false,
                            "Grace",
                            "Coffee for whoever lands it green first try.",
                        ),
                        (true, "you", "Deal."),
                    ],
                ),
                seed(
                    "#rust",
                    true,
                    4,
                    &[
                        (
                            false,
                            "Linus",
                            "Restating the model for the new folks: a Span is one styled run, a Line is a row of Spans, a Text is a stack of Lines. Widgets never retain any of it.",
                        ),
                        (
                            false,
                            "Ada",
                            "They are handed an area and a mutable buffer, they stamp cells, they are dropped. There is no element that survives to the next frame.",
                        ),
                        (
                            false,
                            "newgrad",
                            "Coming from a retained UI background — does rebuilding the whole view every frame not get expensive?",
                        ),
                        (
                            false,
                            "Linus",
                            "In a terminal it inverts. The grid is ~10k cells. Composing it from scratch is a few hundred microseconds of cache-friendly arithmetic. Diffing a tree to produce the same grid is more work, not less.",
                        ),
                        (
                            false,
                            "Ada",
                            "And it drags a whole class of stale-state bugs with it. Immediate mode just deletes that category.",
                        ),
                        (false, "newgrad", "Where does state live then?"),
                        (
                            false,
                            "Ada",
                            "Outside the widgets, in plain structs the app owns. A list's selected index, a cursor, a scroll offset — app data, mutated only by reducers, read by widgets.",
                        ),
                        (
                            false,
                            "Linus",
                            "Exactly one place anything can change, and it is never inside a widget. Debugging is mostly reading reducers.",
                        ),
                        (
                            false,
                            "newgrad",
                            "How does a clickable link work with no retained node?",
                        ),
                        (
                            false,
                            "Linus",
                            "The same function that lays the doc out can be asked where each link rendered. The reducer compares the click against those rects. The link is a question asked of the current frame, not an object that persists.",
                        ),
                        (
                            false,
                            "Ada",
                            "Nothing can go stale because nothing is kept. That is the recurring theme.",
                        ),
                        (false, "newgrad", "And layout?"),
                        (
                            false,
                            "Linus",
                            "A constraint solve over a rectangle, run fresh every frame. The sub-rects are computed, used, discarded. Resize just reruns the solve. Same idea as text wrap, one level up.",
                        ),
                        (
                            false,
                            "Ada",
                            "Wrapping turns a width and text into rows. Layout turns a rect and constraints into rects. Neither remembers its last answer because recomputing is cheap and correct by construction.",
                        ),
                        (false, "newgrad", "That actually clears it up. Thanks both."),
                        (
                            false,
                            "Linus",
                            "Read docs/composition.md. It is the long form of this thread.",
                        ),
                        (false, "Grace", "Pinning that."),
                        (
                            true,
                            "you",
                            "The rich-text screen prose is basically this conversation written out, for what it is worth.",
                        ),
                        (
                            false,
                            "Ada",
                            "Then it will wrap deterministically no matter how many times you scroll it. Poetic.",
                        ),
                        (false, "Linus", "Ship it."),
                    ],
                ),
                seed(
                    "#design",
                    true,
                    0,
                    &[
                        (
                            false,
                            "Katherine",
                            "Reviewing the experiences screens at 80x24, 120x40, 160x50, 200x60.",
                        ),
                        (
                            false,
                            "Katherine",
                            "Chat thread reads well. The right-aligned own-messages on accent are clear against the peer rows.",
                        ),
                        (
                            false,
                            "Grace",
                            "The dim sender tag pulls its weight — you can scan who said what without reading.",
                        ),
                        (
                            false,
                            "Katherine",
                            "Mail three-pane holds at the narrow size if the message list truncates with an ellipsis. It does, good.",
                        ),
                        (
                            true,
                            "you",
                            "Files breadcrumb is built from the selected node's ancestry, so it stays correct as you move.",
                        ),
                        (
                            false,
                            "Katherine",
                            "Confirmed. The folder vs file preview swap is a nice touch — description list for dirs, body for files.",
                        ),
                        (
                            false,
                            "Grace",
                            "Code editor gutter width adapts to the line count. With the long buffers it goes to 3 digits cleanly.",
                        ),
                        (
                            false,
                            "Katherine",
                            "That is the detail people notice subconsciously. Keep it.",
                        ),
                        (
                            true,
                            "you",
                            "All of it is pure projection, so the narrow sizes are just smaller solves, not special cases.",
                        ),
                        (
                            false,
                            "Katherine",
                            "Which is why it holds up. Approved across all four sizes.",
                        ),
                        (
                            false,
                            "Grace",
                            "Recording the GIFs after the content lands?",
                        ),
                        (
                            true,
                            "you",
                            "rstui-docs skill handles that — it is not a CI gate, so it is a follow-up, not a blocker.",
                        ),
                        (
                            false,
                            "Katherine",
                            "Good. Content first, media second. Sign-off from design.",
                        ),
                    ],
                ),
                seed(
                    "#incidents",
                    true,
                    0,
                    &[
                        (
                            false,
                            "CI Bot",
                            "[resolved] main went fmt-red for 6 minutes after a doc-only merge. Fixed forward.",
                        ),
                        (
                            false,
                            "Grace",
                            "Postmortem: a content edit shifted a rustfmt-sensitive string. Lesson: run the gate before the merge-back, not after.",
                        ),
                        (
                            false,
                            "Linus",
                            "The gate is fmt, naming, clippy -D, doc -D, test. None of it gates content text, but fmt will reformat a literal if it can.",
                        ),
                        (
                            false,
                            "Ada",
                            "So keep long string literals in the explicit continuation style the file already uses and fmt leaves them alone.",
                        ),
                        (
                            true,
                            "you",
                            "Noted for this slice — the prose and markdown consts use the leading-backslash continuation, same as the originals.",
                        ),
                        (false, "Grace", "Good. No repeat."),
                        (
                            false,
                            "CI Bot",
                            "[info] all gates green on the last 12 merges.",
                        ),
                        (false, "Linus", "Boring is the goal."),
                    ],
                ),
                seed(
                    "#random",
                    true,
                    0,
                    &[
                        (false, "Grace", "Coffee?"),
                        (true, "you", "Always. Once this slice is green."),
                        (false, "Ada", "It will be green before the kettle boils."),
                        (false, "Linus", "Optimist."),
                        (false, "Grace", "Realist. The shape has no slow path."),
                        (false, "Katherine", "Adding that to a sticker."),
                        (true, "you", "I would buy that sticker."),
                        (false, "Ada", "Kettle is on. Clock is running."),
                    ],
                ),
                seed(
                    "Ada Lovelace",
                    false,
                    1,
                    &[
                        (
                            false,
                            "Ada",
                            "Ping me when the long-scroll demo is ready — want to feel it before sign-off.",
                        ),
                        (
                            true,
                            "you",
                            "It is in. Rich Text tab 1 and 2 now hold a full handbook each. Hold Down or PageDown and it never bottoms out into blank.",
                        ),
                        (false, "Ada", "Trying it now."),
                        (
                            false,
                            "Ada",
                            "Smooth the whole way, and the tail clamps exactly at the last screenful. That is the behaviour I wanted.",
                        ),
                        (
                            true,
                            "you",
                            "The clamp is computed in the view from the composed row count, so it is correct at every width.",
                        ),
                        (
                            false,
                            "Ada",
                            "Resized mid-scroll — it just reflowed and stayed clamped. No drift.",
                        ),
                        (
                            true,
                            "you",
                            "Pure function of width. Nothing remembers the old wrap.",
                        ),
                        (false, "Ada", "Sign-off from me. Good slice."),
                        (
                            true,
                            "you",
                            "Thanks. Doing the other four screens now for realistic bodies.",
                        ),
                        (
                            false,
                            "Ada",
                            "The chat one is going to be recursive, is it not.",
                        ),
                        (true, "you", "You are reading the test data right now."),
                        (false, "Ada", "Delightful. Carry on."),
                    ],
                ),
                seed(
                    "Grace Hopper",
                    false,
                    0,
                    &[
                        (
                            false,
                            "Grace",
                            "Nice work on the rail — the grouped Widgets / Experiences split reads instantly.",
                        ),
                        (
                            true,
                            "you",
                            "All from one ALL array, so the sidebar, hotkeys, and palette cannot disagree.",
                        ),
                        (
                            false,
                            "Grace",
                            "That is the kind of single-source-of-truth I like. Click hit-test builds from the same rows?",
                        ),
                        (
                            true,
                            "you",
                            "Same function. What is drawn and what a click selects cannot drift.",
                        ),
                        (
                            false,
                            "Grace",
                            "Good. Now make the bodies long enough that the scroll actually means something.",
                        ),
                        (
                            true,
                            "you",
                            "That is exactly the slice in flight. This DM included.",
                        ),
                        (false, "Grace", "Then I will stop adding to your test data."),
                        (true, "you", "Every message helps, honestly."),
                        (
                            false,
                            "Grace",
                            "In that case: keep shipping small green slices. That is the whole job.",
                        ),
                        (true, "you", "On it."),
                    ],
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

        // The bubble thread, bottom-anchored by *rendered* rows: compose
        // the whole thread, then scroll so the newest message sits at the
        // bottom, minus `self.scroll` rows of scrollback. Anchoring by
        // wrapped rows — not logical lines — is what keeps a long message
        // from clipping the newest content off the bottom; it is the same
        // view-time row-count clamp the rich-text reader and log tail use.
        let para = Paragraph::new(self.thread_lines(theme, thread.width))
            .style(theme.body())
            .wrap(Wrap { trim: false });
        let max_off = para
            .line_count(thread.width)
            .saturating_sub(thread.height as usize);
        let off = max_off.saturating_sub(self.scroll as usize);
        frame.render_widget(
            para.scroll(Position::new(0, u16::try_from(off).unwrap_or(u16::MAX))),
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
