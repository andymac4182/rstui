//! Exercises [`StreamMarkdown`] the way an agent chat client does: a
//! reply *streams in* token by token, each token is appended in `update`
//! which calls [`StreamMarkdownState::ingest`] (the mutation step: repair,
//! segment, then fill the per-block cache), and `view` renders the
//! caller-owned state as a pure projection.
//!
//! `init` kicks the stream with the first token; each `update` appends the
//! token, re-`ingest`s, and [`Cmd::message`]s itself the next one until
//! the reply is exhausted, then quits. Running it under the headless
//! [`Harness`] settles that whole chain before the first
//! [`Harness::snapshot`], so the printed frame is the *finished* reply and
//! the example doubles as a TTY-free deterministic smoke test:
//!
//! ```text
//! cargo run -p rstui-ai --example stream_markdown_demo
//! ```
//!
//! The point it demonstrates: every intermediate prefix has an open bold
//! run, an open inline-code span, or an unterminated fenced block — the
//! exact mid-stream shapes [`remend`](rstui_ai::stream_markdown::remend)
//! repairs — yet no raw `**`/`` ` ``/```` ``` ```` marker ever reaches the
//! screen, there is no mode-flip when a closing marker finally arrives,
//! and only the changing tail block re-renders (the settled heading above
//! is served from the per-block cache).

use rstui_ai::stream_markdown::{RemendOptions, StreamMarkdown, StreamMarkdownState};
use rstui_core::Frame;
use rstui_runtime::{App, Cmd, Harness};

/// The full reply the "model" streams, one character at a time.
const FULL_REPLY: &str = "# Streaming markdown\n\nThis arrives **token by token**, \
with `inline code` and a fenced block:\n\n```rust\nfn main() {\n    \
println!(\"hi\");\n}\n```\n\nAll done.";

/// Caller-owned model: the reply received so far, the not-yet-streamed
/// tail, and the streaming-markdown view state (the per-block cache).
struct ChatModel {
    received: String,
    pending: Vec<char>,
    next_index: usize,
    view_state: StreamMarkdownState,
    options: RemendOptions,
    width: u16,
}

/// One streamed character arrived, or the producer closed the stream.
enum Msg {
    /// The next streamed character.
    Token(char),
    /// The stream finished.
    Done,
}

impl ChatModel {
    /// The message that delivers the next pending character, or
    /// [`Msg::Done`] when the reply is fully streamed.
    fn next_msg(&self) -> Msg {
        match self.pending.get(self.next_index) {
            Some(&character) => Msg::Token(character),
            None => Msg::Done,
        }
    }
}

impl App for ChatModel {
    type Message = Msg;

    fn init(&mut self) -> Cmd<Msg> {
        Cmd::message(self.next_msg())
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Token(character) => {
                self.received.push(character);
                self.next_index += 1;
                // The mutation step: repair + segment + (re-)render into
                // the caller-owned cache. Render stays a pure read.
                self.view_state
                    .ingest(&self.received, self.width, &self.options, true);
                Cmd::message(self.next_msg())
            }
            Msg::Done => {
                self.view_state.mark_settled();
                Cmd::quit()
            }
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        // Pure projection of the caller-owned source + state.
        frame.render_widget(
            StreamMarkdown::new(&self.received)
                .state(&self.view_state)
                .options(self.options.clone())
                .width(self.width),
            frame.area(),
        );
    }
}

fn main() {
    let width = 44;
    let model = ChatModel {
        received: String::new(),
        pending: FULL_REPLY.chars().collect(),
        next_index: 0,
        view_state: StreamMarkdownState::new(),
        options: RemendOptions::default(),
        width,
    };

    // `Harness::new` runs `init` and settles the whole self-chained
    // stream (Token → Token → … → Done/quit) before the first snapshot,
    // so this prints the finished, fully-rendered reply — no raw markers,
    // bumping the command budget for the long per-char chain.
    let harness = Harness::new(model, width, 22).with_command_budget(4096);
    print!("{}", harness.snapshot());
}
