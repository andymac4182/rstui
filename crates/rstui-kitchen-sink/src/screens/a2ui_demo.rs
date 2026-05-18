//! The **A2UI** scene: an editable code editor holding the Google A2UI
//! v0.10 document an agent sent on the left, its live `rstui-jsonui`
//! projection on the right.
//!
//! Type to edit the JSON and the right pane re-renders live;
//! `PgUp`/`PgDn` switch the worked examples (edits persist per example).
//! All real state is the caller-owned [`TextArea`](rstui_core::TextArea)
//! the [`Editor`](rstui_widgets::Editor) projects — the same pure
//! projection the ACP client uses for agent-driven UI.

use rstui_core::{KeyCode, Position, Rect};
use rstui_runtime::Frame;

use crate::screens::ScreenOutcome;
use crate::screens::agent_ui::{Format, Scene};
use crate::theme::Theme;

/// The A2UI scene (a [`Scene`] fixed to [`Format::A2ui`]).
#[derive(Debug)]
pub(crate) struct State(Scene);

impl State {
    /// The scene, seeded from the A2UI worked examples.
    pub(crate) fn new() -> Self {
        Self(Scene::new(Format::A2ui))
    }

    /// Editing keys + `PgUp`/`PgDn` example switch (see [`Scene::on_key`]).
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        self.0.on_key(code)
    }

    /// Paste into the buffer.
    pub(crate) fn on_paste(&mut self, text: &str) {
        self.0.on_paste(text);
    }

    /// Cut `sel` from the buffer.
    pub(crate) fn cut(&mut self, sel: &str) -> bool {
        self.0.cut(sel)
    }

    /// Wheel scroll moves the caret.
    pub(crate) fn on_scroll(&mut self, up: bool) {
        self.0.on_scroll(up);
    }

    /// The editor text rect, so a drag-select stays in the buffer.
    pub(crate) fn selection_region(&self, pos: Position, content: Rect) -> Option<Rect> {
        self.0.selection_region(pos, content)
    }

    /// Draw the editor ⇆ live-projection split.
    pub(crate) fn view(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        self.0.view(theme, frame, area);
    }
}
