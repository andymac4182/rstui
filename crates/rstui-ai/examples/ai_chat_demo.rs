//! A headless AI chat client driven by the runtime [`Harness`], the
//! capstone for the `rstui-ai` ai-elements vocabulary: it composes
//! [`Conversation`] + [`Message`] + [`Tool`] + [`PromptInput`] into one
//! [`App`] with no terminal.
//!
//! It exercises the whole pure-projection contract end to end:
//!
//! - the transcript is `Vec<UiMessage>` (the shared [`crate::model`]),
//!   scrolled by a caller-owned [`ScrollState`] the reducer drives
//!   (sticky-bottom-while-streaming);
//! - the composer projects a caller-owned [`TextArea`]; the reducer maps
//!   keys to edits — **Enter submits**, **Shift+Enter is a newline** (the
//!   documented [`PromptInput`] keymap), and the
//!   [`PromptInputIntent`](rstui_ai::prompt_input::PromptInputIntent) the
//!   reducer derives drives Submit;
//! - a scripted "assistant" turn with a [`Tool`] card lands when the user
//!   submits — no callbacks anywhere, every widget a pure read of the
//!   model.
//!
//! Running over a [`TestBackend`](rstui_core::TestBackend) keeps it
//! TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-ai --example ai_chat_demo
//! ```

use rstui_ai::conversation::Conversation;
use rstui_ai::model::{ChatStatus, ToolState, UiMessage};
use rstui_ai::prompt_input::PromptInput;
use rstui_ai::tool::Tool;
use rstui_core::scroll::ScrollState;
use rstui_core::{Constraint, KeyCode, KeyEvent, KeyModifiers, Layout, Rect, TextArea};
use rstui_runtime::{App, Cmd, Event, Frame, Harness};
use rstui_widgets::{Block, Borders};
use serde_json::json;

/// The whole chat app's state — all caller-owned model data the pure
/// widgets only read.
struct Chat {
    /// The transcript (the shared AI SDK message model).
    messages: Vec<UiMessage>,
    /// The composer's multi-line input model.
    composer: TextArea,
    /// The transcript scroll (sticky-bottom while streaming).
    scroll: ScrollState,
    /// The chat lifecycle; drives the composer's send/stop glyph.
    status: ChatStatus,
    /// Whether the assistant's tool card is expanded (caller-owned; the
    /// reducer flips it — the [`Tool`] widget only reads it).
    tool_open: bool,
    /// The viewport the last `view` laid the transcript out into — the
    /// geometry seam for the scroll math (docs/composition.md).
    transcript: std::cell::Cell<Rect>,
}

impl Default for Chat {
    fn default() -> Self {
        let user = UiMessage::from_value(&json!({
            "id": "u1", "role": "user",
            "parts": [{ "type": "text", "text": "What is 2 + 2?" }]
        }));
        Self {
            messages: vec![user],
            composer: TextArea::new(),
            scroll: ScrollState::new(),
            status: ChatStatus::Ready,
            tool_open: true,
            transcript: std::cell::Cell::new(Rect::new(0, 0, 0, 0)),
        }
    }
}

impl Chat {
    /// The assistant's scripted reply: a text part plus a completed
    /// calculator [`Tool`] call.
    fn assistant_reply() -> UiMessage {
        UiMessage::from_value(&json!({
            "id": "a1", "role": "assistant",
            "parts": [
                { "type": "text", "text": "It is **4**." },
                {
                    "type": "tool-calculator", "toolCallId": "c1",
                    "state": "output-available",
                    "input": { "expr": "2 + 2" },
                    "output": { "result": 4 }
                }
            ]
        }))
    }

    /// Sends the composer's text as a user turn, then lands the scripted
    /// assistant reply, and re-pins the transcript to the tail.
    fn submit(&mut self) {
        let text = self.composer.to_string();
        if text.trim().is_empty() {
            return;
        }
        self.messages.push(UiMessage::from_value(&json!({
            "id": format!("u{}", self.messages.len()),
            "role": "user",
            "parts": [{ "type": "text", "text": text }]
        })));
        self.composer.clear();
        self.messages.push(Self::assistant_reply());
        self.status = ChatStatus::Ready;

        // Sticky-bottom: after content grows, snap the (following) scroll
        // to the new tail — the reducer's job (the widget only reads it).
        let area = self.transcript.get();
        let total =
            Conversation::new(&self.messages, &self.scroll).content_rows(area.width) as usize;
        self.scroll.on_content_change(total, area.height as usize);
    }
}

/// The reducer's intents — every state change funnels through `update`.
enum Msg {
    /// A character typed into the composer.
    Type(char),
    /// Backspace in the composer.
    Backspace,
    /// Shift+Enter: a newline in the composer (the documented keymap).
    Newline,
    /// Enter / the send glyph: submit the prompt (the documented keymap).
    Submit,
    /// Scroll the transcript by a delta (wheel/keys).
    Scroll(isize),
    /// Toggle the assistant tool card open/closed.
    ToggleTool,
    /// Quit.
    Quit,
}

impl App for Chat {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        let key = event.as_key_press()?;
        match key.code {
            KeyCode::Esc => Some(Msg::Quit),
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => Some(Msg::Newline),
            KeyCode::Enter => Some(Msg::Submit),
            KeyCode::Backspace => Some(Msg::Backspace),
            KeyCode::Up => Some(Msg::Scroll(-1)),
            KeyCode::Down => Some(Msg::Scroll(1)),
            KeyCode::Tab => Some(Msg::ToggleTool),
            KeyCode::Char(c) => Some(Msg::Type(c)),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Type(c) => self.composer.insert_char(c),
            Msg::Backspace => {
                self.composer.delete_backward();
            }
            Msg::Newline => self.composer.insert_newline(),
            Msg::Submit => self.submit(),
            Msg::Scroll(delta) => {
                let area = self.transcript.get();
                let total = Conversation::new(&self.messages, &self.scroll).content_rows(area.width)
                    as usize;
                self.scroll.scroll_by(delta, total, area.height as usize);
            }
            Msg::ToggleTool => self.tool_open = !self.tool_open,
            Msg::Quit => return Cmd::quit(),
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let outer = Block::new()
            .borders(Borders::ALL)
            .title("rstui-ai chat demo");
        let inner = outer.inner(frame.area());
        frame.render_widget(outer, frame.area());

        // Transcript fills; the composer is a fixed 4-row panel pinned
        // to the bottom (the four-moves layout, docs/composition.md).
        let [transcript_area, composer_area] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(4)]).areas(inner);

        // Record the laid-out transcript rect (the geometry seam) so the
        // reducer's scroll math hit-tests what was actually rendered.
        self.transcript.set(transcript_area);

        frame.render_widget(
            Conversation::new(&self.messages, &self.scroll),
            transcript_area,
        );

        // Surface the most recent tool call (if any) as the keystone
        // expandable [`Tool`] card, overlaying the transcript's lower
        // region — the way an app spotlights an in-progress/finished call
        // distinct from its compact in-transcript line.
        if let Some(tool) = self
            .messages
            .iter()
            .rev()
            .find_map(|m| m.tool_parts().next())
        {
            let card_h = if self.tool_open { 8 } else { 3 };
            let h = card_h.min(transcript_area.height);
            if h > 0 {
                let card_area = Rect::new(
                    transcript_area.left(),
                    transcript_area.bottom().saturating_sub(h),
                    transcript_area.width,
                    h,
                );
                frame.render_widget(Tool::new(tool).open(self.tool_open), card_area);
            }
        }

        frame.render_widget(
            PromptInput::new(&self.composer, self.status)
                .focused(true)
                .placeholder("Type a message — Enter sends, Shift+Enter newline"),
            composer_area,
        );
    }
}

fn main() {
    let mut harness = Harness::new(Chat::default(), 60, 18);
    println!("start (one user turn, empty composer):");
    println!("{}", harness.snapshot());

    let typing = |c: char| Event::from(KeyEvent::char(c));
    let enter = || Event::from(KeyEvent::from_code(KeyCode::Enter));
    let shift_enter = || Event::from(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));

    // Type a multi-line prompt: Shift+Enter inserts a newline (the keymap).
    for c in "Compute".chars() {
        harness.handle(typing(c));
    }
    harness.handle(shift_enter());
    for c in "2 + 2".chars() {
        harness.handle(typing(c));
    }
    println!("\nafter typing 'Compute' <Shift+Enter> '2 + 2':");
    println!("{}", harness.snapshot());

    // Enter submits: a user turn + the scripted assistant reply (a Tool
    // card) land, and the sticky-bottom scroll pins to the newest turn.
    harness.handle(enter());
    println!("\nafter Enter (submit) — assistant reply with a Tool card:");
    println!("{}", harness.snapshot());

    // Tab collapses the tool card (caller-owned open flag the reducer
    // flips; the widget only reads it).
    harness.handle(Event::from(KeyEvent::from_code(KeyCode::Tab)));
    println!("\nafter Tab (collapse the Tool card):");
    println!("{}", harness.snapshot());

    harness.handle(Event::from(KeyEvent::from_code(KeyCode::Esc)));
    harness.handle(typing('x')); // ignored: already quit

    // The model is the single source of truth — assert on it directly.
    let app = harness.app();
    assert_eq!(app.messages.len(), 3, "user + submitted user + assistant");
    assert_eq!(app.messages[2].role, rstui_ai::model::Role::Assistant);
    let tool: Vec<_> = app.messages[2].tool_parts().collect();
    assert_eq!(tool.len(), 1);
    assert_eq!(tool[0].tool_name, "calculator");
    assert_eq!(tool[0].state, ToolState::OutputAvailable);
    assert!(app.composer.is_empty(), "composer cleared on submit");
    assert!(!app.tool_open, "Tab collapsed the tool card");
    assert!(!harness.is_running(), "Esc quit the app");

    // The final frame still shows the assistant answer and the collapsed
    // tool header (a deterministic snapshot assertion).
    let snap = harness.snapshot();
    assert!(snap.contains("calculator"), "tool header in the frame");
    println!("\nfinal (asserts passed): 3 messages, tool card collapsed.");
}
