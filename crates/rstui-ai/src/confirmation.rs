//! [`Confirmation`] — a human-in-the-loop approve/deny gate projecting a tool
//! call that is paused for the user's go-ahead.
//!
//! # A pure projection of a `&ToolUiPart` + caller-owned answer
//!
//! The ai-elements `Confirmation` blocks a tool until the human approves or
//! denies it. The authoritative state is the shared
//! [`ToolUiPart`] (its
//! [`ToolState`] seven-state machine) plus the
//! caller-owned answer. `Confirmation` owns nothing: it *reads* a
//! `&ToolUiPart` and a caller-owned `approval: Option<bool>`
//! and renders one of three faces:
//!
//! - a non-terminal **input** state (`InputStreaming`/`InputAvailable`): the
//!   tool is not asking yet → it renders **nothing** (a gate only appears
//!   once asked);
//! - [`ApprovalRequested`](crate::model::ToolState::ApprovalRequested): the
//!   request line + an `[ Approve ]` / `[ Deny ]` affordance pair;
//! - any other state (responded / resolved / errored): a single resolved
//!   line (the [`ToolState::label`](crate::model::ToolState::label), tinted by
//!   the caller's answer if any).
//!
//! Activation is the documented hit-test seam, never a callback: the host
//! maps a click in [`approve_rect`](Confirmation::approve_rect) /
//! [`deny_rect`](Confirmation::deny_rect) to a
//! [`ConfirmationIntent`] the reducer consumes (it sets `approval` and
//! advances the tool).
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area
//! clips and the rects return `None` when there is no room — never a panic.

use rstui_core::{Buffer, Color, Modifier, Position, Rect, Style, Widget};

use crate::model::{ToolState, ToolUiPart};

/// The reducer-consumed intent a [`Confirmation`] surfaces — the host maps a
/// click in [`approve_rect`](Confirmation::approve_rect) /
/// [`deny_rect`](Confirmation::deny_rect) to this and the reducer sets the
/// tool's `approval` and advances it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationIntent {
    /// The user approved the call.
    Approve,
    /// The user denied the call.
    Deny,
}

/// A human-in-the-loop approve/deny gate for one tool call.
///
/// Projects a [`&ToolUiPart`](crate::model::ToolUiPart) and a caller-owned
/// `approval`; see the [module docs](self) for the
/// three-face contract. The request face is the tool name + a prompt on row
/// 0 and `[ Approve ] [ Deny ]` on row 1 (when there is a second row).
/// `Confirmation` owns no state.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::confirmation::{Confirmation, ConfirmationIntent};
/// use rstui_ai::model::{ToolState, ToolUiPart};
///
/// let tool = ToolUiPart {
///     tool_name: "delete_file".into(),
///     tool_call_id: "t1".into(),
///     state: ToolState::ApprovalRequested,
///     input: None, output: None, error_text: None,
/// };
/// let gate = Confirmation::new(&tool, None);
/// let area = Rect::new(0, 0, 30, 2);
///
/// // Asked → the two affordances are hit-testable on row 1.
/// assert!(gate.approve_rect(area).is_some());
/// assert!(gate.deny_rect(area).is_some());
///
/// let mut buf = Buffer::empty(area);
/// gate.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'A'); // "Approve…?"
/// ```
#[derive(Debug, Clone)]
pub struct Confirmation<'a> {
    tool: &'a ToolUiPart,
    approval: Option<bool>,
    style: Style,
    approve_style: Style,
    deny_style: Style,
}

const APPROVE: &str = "[ Approve ]";
const DENY: &str = "[ Deny ]";

impl<'a> Confirmation<'a> {
    /// A gate projecting `tool` with the caller-owned `approval`
    /// (`Some(true)` approved, `Some(false)` denied, `None` un-answered).
    #[must_use]
    pub fn new(tool: &'a ToolUiPart, approval: Option<bool>) -> Self {
        Self {
            tool,
            approval,
            style: Style::new(),
            approve_style: Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
            deny_style: Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        }
    }

    /// Sets the base [`Style`], beneath the affordance styles.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] the `[ Approve ]` affordance is drawn with.
    #[must_use]
    pub fn approve_style(mut self, approve_style: Style) -> Self {
        self.approve_style = approve_style;
        self
    }

    /// Sets the [`Style`] the `[ Deny ]` affordance is drawn with.
    #[must_use]
    pub fn deny_style(mut self, deny_style: Style) -> Self {
        self.deny_style = deny_style;
        self
    }

    /// `true` when the gate is actively asking (state
    /// [`ApprovalRequested`](crate::model::ToolState::ApprovalRequested)) — the
    /// only state with clickable affordances.
    #[must_use]
    pub fn is_asking(&self) -> bool {
        self.tool.state == ToolState::ApprovalRequested
    }

    /// `true` when the gate renders nothing — a non-terminal input state
    /// (`InputStreaming`/`InputAvailable`); the tool has not asked yet.
    #[must_use]
    pub fn is_hidden(&self) -> bool {
        matches!(
            self.tool.state,
            ToolState::InputStreaming | ToolState::InputAvailable
        )
    }

    /// The `[ Approve ]` affordance [`Rect`] when [`is_asking`](Self::is_asking)
    /// and the area has a second row for it, else `None`. The host hit-tests
    /// a click here → [`ConfirmationIntent::Approve`].
    #[must_use]
    pub fn approve_rect(&self, area: Rect) -> Option<Rect> {
        if !self.is_asking() || area.is_empty() || area.height < 2 {
            return None;
        }
        let w = (APPROVE.chars().count() as u16).min(area.width);
        Some(Rect::new(area.left(), area.top().saturating_add(1), w, 1))
    }

    /// The `[ Deny ]` affordance [`Rect`] when [`is_asking`](Self::is_asking)
    /// and it fits to the right of `[ Approve ]`, else `None`. The host
    /// hit-tests a click here → [`ConfirmationIntent::Deny`].
    #[must_use]
    pub fn deny_rect(&self, area: Rect) -> Option<Rect> {
        let approve = self.approve_rect(area)?;
        let x = approve.right().saturating_add(1);
        if x >= area.right() {
            return None;
        }
        let w = (DENY.chars().count() as u16).min(area.right().saturating_sub(x));
        Some(Rect::new(x, approve.top(), w, 1))
    }

    /// The single line a resolved/responded/errored gate shows.
    fn resolved_line(&self) -> String {
        let verb = match self.approval {
            Some(true) => "Approved",
            Some(false) => "Denied",
            None => self.tool.state.label(),
        };
        format!("{} · {}", self.tool.tool_name, verb)
    }
}

/// Draws `text` at `(x0, y)`, clipped at `right`, in `style`.
fn draw(buf: &mut Buffer, x0: u16, y: u16, right: u16, text: &str, style: Style) {
    let mut x = x0;
    for ch in text.chars() {
        if x >= right {
            break;
        }
        buf.set_cell(Position::new(x, y), ch, style);
        x = x.saturating_add(1);
    }
}

impl Widget for Confirmation<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() || self.is_hidden() {
            return;
        }
        let right = area.right();
        let top = area.top();

        if self.is_asking() {
            let prompt = format!("Approve {}?", self.tool.tool_name);
            draw(buf, area.left(), top, right, &prompt, self.style);
            if let Some(ar) = self.approve_rect(area) {
                draw(
                    buf,
                    ar.left(),
                    ar.top(),
                    right,
                    APPROVE,
                    self.style.patch(self.approve_style),
                );
            }
            if let Some(dr) = self.deny_rect(area) {
                draw(
                    buf,
                    dr.left(),
                    dr.top(),
                    right,
                    DENY,
                    self.style.patch(self.deny_style),
                );
            }
        } else {
            let tint = match self.approval {
                Some(true) => self.style.patch(self.approve_style),
                Some(false) => self.style.patch(self.deny_style),
                None => self.style,
            };
            draw(buf, area.left(), top, right, &self.resolved_line(), tint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(state: ToolState) -> ToolUiPart {
        ToolUiPart {
            tool_name: "rm".into(),
            tool_call_id: "t".into(),
            state,
            input: None,
            output: None,
            error_text: None,
        }
    }

    fn lines(widget: Confirmation<'_>, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn a_non_terminal_input_state_renders_nothing() {
        for state in [ToolState::InputStreaming, ToolState::InputAvailable] {
            let t = tool(state);
            let widget = Confirmation::new(&t, None);
            assert!(widget.is_hidden());
            assert_eq!(lines(widget, 12, 2), "            \n            \n");
        }
    }

    #[test]
    fn the_request_face_shows_the_prompt_and_two_affordances() {
        let t = tool(ToolState::ApprovalRequested);
        let widget = Confirmation::new(&t, None);
        assert!(widget.is_asking());
        assert_eq!(
            lines(widget, 22, 2),
            "Approve rm?           \n[ Approve ] [ Deny ]  \n"
        );
    }

    #[test]
    fn approve_and_deny_rects_are_on_the_second_row() {
        let t = tool(ToolState::ApprovalRequested);
        let widget = Confirmation::new(&t, None);
        let area = Rect::new(0, 0, 22, 2);
        assert_eq!(widget.approve_rect(area), Some(Rect::new(0, 1, 11, 1)));
        assert_eq!(widget.deny_rect(area), Some(Rect::new(12, 1, 8, 1)));
    }

    #[test]
    fn a_resolved_state_shows_one_tinted_line() {
        let t = tool(ToolState::OutputAvailable);
        // Approved tint.
        assert_eq!(
            lines(Confirmation::new(&t, Some(true)), 20, 1),
            "rm · Approved       \n"
        );
        let d = tool(ToolState::OutputDenied);
        assert_eq!(
            lines(Confirmation::new(&d, Some(false)), 20, 1),
            "rm · Denied         \n"
        );
        // No caller answer → the tool state label.
        let e = tool(ToolState::OutputError);
        assert_eq!(
            lines(Confirmation::new(&e, None), 20, 1),
            "rm · Error          \n"
        );
    }

    #[test]
    fn resolved_states_have_no_clickable_rects() {
        let t = tool(ToolState::OutputAvailable);
        let widget = Confirmation::new(&t, Some(true));
        let area = Rect::new(0, 0, 22, 2);
        assert_eq!(widget.approve_rect(area), None);
        assert_eq!(widget.deny_rect(area), None);
    }

    #[test]
    fn a_one_row_area_has_no_affordance_rects_but_still_draws_the_prompt() {
        let t = tool(ToolState::ApprovalRequested);
        let widget = Confirmation::new(&t, None);
        let area = Rect::new(0, 0, 22, 1);
        assert_eq!(widget.approve_rect(area), None);
        assert_eq!(
            lines(Confirmation::new(&t, None), 22, 1),
            "Approve rm?           \n"
        );
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let t = tool(ToolState::ApprovalRequested);
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Confirmation::new(&t, None).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
