//! `rstui-smoke` — the cross-cutting headless smoke test.
//!
//! Every other crate is unit-tested in isolation. Nothing, until now,
//! exercised the crates *together* through their public APIs — so on
//! 2026-05-17 two slices that were each green alone broke the workspace when
//! merged (a rustdoc-gate regression that only the quality stream caught, by
//! hand, at merge time). This crate closes that gap: it composes the
//! `rstui-core` rendering substrate, the `rstui-runtime` event loop, and a
//! `rstui-widgets` widget end to end, and asserts the composition still works
//! — under `cargo test`, so the integration-regression class fails the `test`
//! gate (and `cargo xtask ci`) for *every* stream before push, not only at
//! merge.
//!
//! It is deliberately a *consumer* of the published surface, exactly like a
//! downstream application: it never reaches into another stream's internals,
//! so it breaks only when the public contract between crates breaks — which
//! is precisely the signal it exists to give. Because it is the smallest real
//! app that uses all three layers, [`SmokeApp`] also doubles as the
//! composable seed a fuller kitchen-sink harness can grow from.
//!
//! The assertions live in `tests/`: `headless.rs` drives [`SmokeApp`] under
//! both the headless `Harness` and the real `rstui_runtime::run` loop (over
//! an in-memory backend and scripted input, so the *production* loop itself
//! is on the smoke path); `widgets.rs` renders the richer widgets (Tree,
//! Toast, Markdown) through the same `Frame::render_widget` seam and pins the
//! crossterm full-screen `run_app` shell's signature at compile time (it
//! needs a real TTY, so it cannot run headless).

use rstui_runtime::{App, Cmd, Event, Frame};
use rstui_widgets::{Block, Borders};

/// The messages [`SmokeApp`] reduces. Intentionally tiny: the point is to
/// exercise the event → `update` → `view` path across crates, not to model a
/// real domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmokeMessage {
    /// Increment the visible counter.
    Increment,
    /// Stop the loop via [`Cmd::quit`](rstui_runtime::Cmd::quit).
    Quit,
}

/// The smallest app that still touches all three layers: it keeps a counter
/// (runtime state + reducer), maps keys to messages (runtime event routing),
/// and draws a `rstui-widgets` `Block` plus a status line through
/// `Frame::render_widget` (the core widget seam).
///
/// `i` increments; `q` quits. Construct with [`SmokeApp::default`], drive it
/// with the headless `Harness` or the real `rstui_runtime::run` loop.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SmokeApp {
    count: u32,
}

impl SmokeApp {
    /// The counter the view renders. Lets a test assert on reduced state
    /// without rendering, the cross-crate analogue of `Harness::app`.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.count
    }

    /// The status line the view stamps inside the block. Kept here (not
    /// inlined into `view`) so a test can assert the exact text the
    /// integration is expected to render without scraping the snapshot.
    #[must_use]
    pub fn status_line(&self) -> String {
        format!("rstui smoke - count: {}", self.count)
    }
}

impl App for SmokeApp {
    type Message = SmokeMessage;

    fn on_event(&self, event: Event) -> Option<SmokeMessage> {
        match event.as_key_press()?.code {
            rstui_core::KeyCode::Char('i') => Some(SmokeMessage::Increment),
            rstui_core::KeyCode::Char('q') => Some(SmokeMessage::Quit),
            _ => None,
        }
    }

    fn update(&mut self, message: SmokeMessage) -> Cmd<SmokeMessage> {
        match message {
            SmokeMessage::Increment => {
                self.count = self.count.saturating_add(1);
                Cmd::none()
            }
            SmokeMessage::Quit => Cmd::quit(),
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        // Widget #1: the foundational rstui-widgets container, through the
        // core `Frame::render_widget` seam.
        frame.render_widget(Block::new().borders(Borders::ALL), area);
        // Widget #2: the status line via core's blanket `Widget for String`,
        // placed inside the block's border so the two compose.
        let inner = rstui_core::Rect::new(
            area.x.saturating_add(1),
            area.y.saturating_add(1),
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        frame.render_widget(self.status_line(), inner);
    }
}
