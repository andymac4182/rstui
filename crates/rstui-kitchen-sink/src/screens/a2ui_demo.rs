//! The **A2UI** scene: the verbatim Google A2UI v0.10 document an agent
//! sent on the left, its live `rstui-jsonui` projection on the right.
//!
//! ←/→ switches between the worked examples; ↑/↓ scrolls the (often
//! multi-line) agent response. The screen owns only `(example, scroll)`;
//! `view` re-parses and re-projects every frame — the same pure
//! projection the ACP client uses for agent-driven UI.

use rstui_core::{KeyCode, Rect};
use rstui_runtime::Frame;

use crate::screens::ScreenOutcome;
use crate::screens::agent_ui::{self, A2UI_SAMPLES};
use crate::theme::Theme;

/// Caller-owned scene state: which worked example, and the source-pane
/// scroll offset.
#[derive(Debug)]
pub(crate) struct State {
    example: usize,
    scroll: u16,
}

impl State {
    /// The scene, opened on the first example.
    pub(crate) fn new() -> Self {
        Self {
            example: 0,
            scroll: 0,
        }
    }

    /// ←/→ cycle examples (resetting scroll); ↑/↓ scroll the response.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        let count = A2UI_SAMPLES.len().max(1);
        match code {
            KeyCode::Right | KeyCode::Tab => {
                self.example = (self.example + 1) % count;
                self.scroll = 0;
            }
            KeyCode::Left | KeyCode::BackTab => {
                self.example = (self.example + count - 1) % count;
                self.scroll = 0;
            }
            KeyCode::Down => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Mouse-wheel scrolls the agent-response pane.
    pub(crate) fn on_scroll(&mut self, up: bool) {
        if up {
            self.scroll = self.scroll.saturating_sub(1);
        } else {
            self.scroll = self.scroll.saturating_add(1);
        }
    }

    /// Re-project the selected document and draw the split.
    pub(crate) fn view(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let index = self.example.min(A2UI_SAMPLES.len().saturating_sub(1));
        let node = agent_ui::a2ui_node(A2UI_SAMPLES[index].source);
        agent_ui::render_split(
            theme,
            frame,
            area,
            "A2UI v0.10",
            A2UI_SAMPLES,
            self.example,
            self.scroll,
            node,
        );
    }
}
