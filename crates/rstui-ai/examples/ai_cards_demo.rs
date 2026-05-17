//! A small agent-results screen composing several `rstui-ai` cards under the
//! runtime, driven by the headless [`Harness`] so it is TTY-free and doubles
//! as a deterministic snapshot smoke test.
//!
//! It is the worked proof of the crate's discipline (ADR 0012,
//! [`docs/composition.md`](https://github.com/andymac4182/rstui/blob/main/docs/composition.md)):
//! every card is a **pure projection of caller-owned model state**, and
//! interaction is the reducer flipping that state — never a widget callback.
//! The model here owns:
//!
//! - a [`Shimmer`] animation `tick` (advanced by the reducer on a key, the
//!   [`Spinner`](rstui_widgets::Spinner)-style no-wall-clock contract);
//! - a [`Sources`] `open` flag (toggled on a key — what a header click would
//!   set, via the documented hit-test seam);
//! - a [`ContextMeter`] over a [`TokenUsage`], and a [`Confirmation`] gate
//!   projecting a [`ToolUiPart`] plus the caller-owned `approval`.
//!
//! ```text
//! cargo run -p rstui-ai --example ai_cards_demo
//! ```

use rstui_ai::confirmation::Confirmation;
use rstui_ai::context_meter::ContextMeter;
use rstui_ai::model::{TokenUsage, ToolState, ToolUiPart};
use rstui_ai::shimmer::Shimmer;
use rstui_ai::snippet::Snippet;
use rstui_ai::sources::Sources;
use rstui_core::{Constraint, KeyCode, KeyEvent, Layout};
use rstui_runtime::{App, Cmd, Event, Frame, Harness};
use rstui_widgets::{Block, Borders};

/// The whole screen's state — every field is plain caller-owned model data
/// the cards only ever read.
struct AgentScreen {
    /// The [`Shimmer`] animation tick (reducer-advanced, no clock).
    tick: u64,
    /// Whether the [`Sources`] disclosure is open.
    sources_open: bool,
    /// The human-in-the-loop tool and its caller-owned approval.
    tool: ToolUiPart,
    approval: Option<bool>,
    /// The grounding sources (`(title, href)` pairs).
    sources: Vec<(String, String)>,
    /// Token usage projected by the [`ContextMeter`].
    usage: TokenUsage,
}

impl Default for AgentScreen {
    fn default() -> Self {
        Self {
            tick: 0,
            sources_open: false,
            tool: ToolUiPart {
                tool_name: "write_file".into(),
                tool_call_id: "call-1".into(),
                state: ToolState::ApprovalRequested,
                input: None,
                output: None,
                error_text: None,
            },
            approval: None,
            sources: vec![
                ("Rust Book".into(), "https://doc.rust-lang.org".into()),
                ("RFC 2056".into(), "https://rfcs.rs/2056".into()),
            ],
            usage: TokenUsage {
                input_tokens: Some(1_200),
                output_tokens: Some(800),
                reasoning_tokens: Some(150),
                cached_input_tokens: Some(400),
            },
        }
    }
}

/// Reducer intents — what a key (or, in a real app, a hit-tested click) maps
/// to. No widget ever calls back; the reducer owns every mutation.
enum Msg {
    Tick,
    ToggleSources,
    Approve,
    Deny,
    Quit,
}

impl App for AgentScreen {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        let key = event.as_key_press()?;
        match key.code {
            KeyCode::Char(' ') => Some(Msg::Tick),
            KeyCode::Char('s') => Some(Msg::ToggleSources),
            KeyCode::Char('a') => Some(Msg::Approve),
            KeyCode::Char('d') => Some(Msg::Deny),
            KeyCode::Esc => Some(Msg::Quit),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Tick => self.tick = self.tick.wrapping_add(1),
            Msg::ToggleSources => self.sources_open = !self.sources_open,
            Msg::Approve => {
                self.approval = Some(true);
                self.tool.state = ToolState::OutputAvailable;
            }
            Msg::Deny => {
                self.approval = Some(false);
                self.tool.state = ToolState::OutputDenied;
            }
            Msg::Quit => return Cmd::quit(),
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let outer = Block::new().borders(Borders::ALL).title("Agent results");
        let inner = outer.inner(frame.area());
        frame.render_widget(outer, frame.area());

        let [shimmer_row, snippet_row, sources_area, meter_row, gate_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(2),
        ])
        .areas(inner);

        // Each widget is built from model state every frame (cheap; the
        // Terminal diffs). None of them owns the state they read.
        frame.render_widget(
            Shimmer::new("Generating answer…").tick(self.tick).spread(2),
            shimmer_row,
        );
        frame.render_widget(Snippet::new("cargo add rstui-ai"), snippet_row);
        frame.render_widget(
            Sources::new(&self.sources).open(self.sources_open),
            sources_area,
        );
        frame.render_widget(ContextMeter::new(self.usage, 8_000), meter_row);
        frame.render_widget(Confirmation::new(&self.tool, self.approval), gate_area);
    }
}

fn main() {
    let mut harness = Harness::new(AgentScreen::default(), 40, 12);
    println!("start (shimmer @0, sources closed, gate asking):");
    println!("{}", harness.snapshot());

    let key = |c: char| Event::from(KeyEvent::char(c));

    // Advance the shimmer a few ticks (what a timer Cmd would do).
    for _ in 0..3 {
        harness.handle(key(' '));
    }
    // Open the sources disclosure (what a header click would do).
    harness.handle(key('s'));
    println!("\nafter 3 ticks + opening sources:");
    println!("{}", harness.snapshot());

    // Approve the gated tool (what an Approve-button click would do).
    harness.handle(key('a'));
    println!("\nafter approving the tool gate:");
    println!("{}", harness.snapshot());

    harness.handle(Event::from(KeyEvent::from_code(KeyCode::Esc)));
    harness.handle(key(' ')); // ignored — already quit

    // The model is the single source of truth — assert on it directly.
    assert_eq!(harness.app().tick, 3, "the reducer advanced the shimmer");
    assert!(harness.app().sources_open, "sources were toggled open");
    assert_eq!(harness.app().approval, Some(true), "the gate was approved");
    assert_eq!(harness.app().tool.state, ToolState::OutputAvailable);
    assert!(!harness.is_running(), "Esc quit the app");
    println!("\nfinal: tick=3, sources open, tool approved (asserts passed)");
}
