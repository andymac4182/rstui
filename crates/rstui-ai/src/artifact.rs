//! [`Artifact`] — a bordered artifact panel: the framed "here is the document
//! / file I produced" container an agent renders a generated artifact into,
//! with a header (title + description + actions) and a scrollable body.
//!
//! # A pure projection + a body `Rect` accessor (like [`Card`](rstui_widgets::Card))
//!
//! The ai-elements `Artifact` is a panel: a header row (title, description,
//! action buttons) over the artifact content. rstui's container pattern is
//! the [`Card::inner`](rstui_widgets::Card)/[`Block::inner`](rstui_widgets::Block)
//! seam — the widget draws the chrome and hands back the body `Rect` for the
//! caller to render its own content into. So `Artifact` owns nothing: it
//! projects the caller's title/description + `&[ArtifactAction]`, draws the
//! header, and exposes [`body`](Artifact::body) (the content rect) plus
//! [`action_rects`](Artifact::action_rects) for click routing — the actions
//! surface as an [`ArtifactIntent::Action`] index, never a callback.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area
//! clips to an empty body, and over-many actions clip — never a panic.

use rstui_core::{Buffer, Modifier, Position, Rect, Style, Widget};
use rstui_widgets::{Block, Borders};

/// One header action of an [`Artifact`] (a labelled affordance, e.g.
/// `Copy` / `Download`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactAction {
    label: String,
}

impl ArtifactAction {
    /// An action shown as `[ label ]` in the header's actions row.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }

    /// The rendered chip width (`[ label ]`).
    fn width(&self) -> u16 {
        (self.label.chars().count() as u16).saturating_add(4)
    }
}

/// The reducer-consumed intent an [`Artifact`] surfaces — the host maps a
/// click in an [`action_rects`](Artifact::action_rects) entry to
/// `Action(index)` and the reducer runs that header action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactIntent {
    /// The header action at this index was activated.
    Action(usize),
}

/// A bordered artifact panel: a header (title, description, actions) over a
/// scrollable body the caller fills.
///
/// Inside a [`Block`] the header is the
/// [`title`](Self::new) (bold), the [`description`](Self::description) (dim,
/// on the next row), then a row of `[ label ]` action chips; the rows below
/// are the body returned by [`body`](Self::body). `Artifact` owns no state —
/// see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::artifact::{Artifact, ArtifactAction};
///
/// let actions = [ArtifactAction::new("Copy")];
/// let art = Artifact::new("plan.md").description("the plan").actions(&actions);
/// let area = Rect::new(0, 0, 24, 8);
///
/// // The body is the rows below the 3-row header, inside the border.
/// let body = art.body(area);
/// assert_eq!(body.x, 1);
/// assert!(body.height >= 1);
///
/// let mut buf = Buffer::empty(area);
/// art.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌'); // framed
/// assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'p'); // title
/// ```
#[derive(Debug, Clone)]
pub struct Artifact<'a> {
    title: &'a str,
    description: &'a str,
    actions: &'a [ArtifactAction],
    style: Style,
}

impl<'a> Artifact<'a> {
    /// A panel titled `title`, no description, no actions.
    #[must_use]
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            description: "",
            actions: &[],
            style: Style::new(),
        }
    }

    /// Sets the header description line (drawn dim below the title).
    #[must_use]
    pub fn description(mut self, description: &'a str) -> Self {
        self.description = description;
        self
    }

    /// Sets the header action chips.
    #[must_use]
    pub fn actions(mut self, actions: &'a [ArtifactAction]) -> Self {
        self.actions = actions;
        self
    }

    /// Sets the base [`Style`] (the panel frame/background).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The framing block.
    fn block(&self) -> Block<'a> {
        Block::new().borders(Borders::ALL).style(self.style)
    }

    /// Rows the header reserves inside the border: title (1) + description
    /// (1 if set) + actions (1 if any).
    fn header_rows(&self) -> u16 {
        1 + u16::from(!self.description.is_empty()) + u16::from(!self.actions.is_empty())
    }

    /// The body content [`Rect`] — the inner area below the header (clipped
    /// to nothing if the panel is too small). Render the artifact's own
    /// content into this, exactly like
    /// [`Card::inner`](rstui_widgets::Card::inner).
    #[must_use]
    pub fn body(&self, area: Rect) -> Rect {
        let inner = self.block().inner(area);
        let used = self.header_rows().min(inner.height);
        Rect::new(
            inner.left(),
            inner.top().saturating_add(used),
            inner.width,
            inner.height.saturating_sub(used),
        )
    }

    /// The actions row inside the border (the inner row at the header's
    /// last line), or `None` if there are no actions / no room.
    fn actions_row(&self, area: Rect) -> Option<Rect> {
        if self.actions.is_empty() {
            return None;
        }
        let inner = self.block().inner(area);
        let row = self.header_rows().saturating_sub(1);
        if row >= inner.height {
            return None;
        }
        Some(Rect::new(
            inner.left(),
            inner.top().saturating_add(row),
            inner.width,
            1,
        ))
    }

    /// The hit [`Rect`] of every header action chip, in order (parallel to
    /// the actions slice, clipped to the row). The host maps a click to
    /// [`ArtifactIntent::Action`] with that index.
    #[must_use]
    pub fn action_rects(&self, area: Rect) -> Vec<Rect> {
        let Some(row) = self.actions_row(area) else {
            return Vec::new();
        };
        let mut rects = Vec::new();
        let mut x = row.left();
        for action in self.actions {
            let w = action.width();
            if x.saturating_add(w) > row.right() {
                break;
            }
            rects.push(Rect::new(x, row.top(), w, 1));
            x = x.saturating_add(w).saturating_add(1);
        }
        rects
    }
}

impl Widget for Artifact<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let block = self.block();
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.is_empty() {
            return;
        }

        let draw = |buf: &mut Buffer, y: u16, text: &str, style: Style| {
            let mut x = inner.left();
            for ch in text.chars() {
                if x >= inner.right() {
                    break;
                }
                buf.set_cell(Position::new(x, y), ch, style);
                x = x.saturating_add(1);
            }
        };

        // The title (bold).
        draw(
            buf,
            inner.top(),
            self.title,
            self.style.add_modifier(Modifier::BOLD),
        );
        // The description (dim).
        if !self.description.is_empty() && inner.height > 1 {
            draw(
                buf,
                inner.top().saturating_add(1),
                self.description,
                self.style.add_modifier(Modifier::DIM),
            );
        }
        // The action chips.
        if let Some(row) = self.actions_row(area) {
            for (rect, action) in self.action_rects(area).iter().zip(self.actions) {
                let chip = format!("[ {} ]", action.label);
                let mut x = rect.left();
                for ch in chip.chars() {
                    if x >= row.right() {
                        break;
                    }
                    buf.set_cell(
                        Position::new(x, rect.top()),
                        ch,
                        self.style.add_modifier(Modifier::REVERSED),
                    );
                    x = x.saturating_add(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actions() -> Vec<ArtifactAction> {
        vec![ArtifactAction::new("Copy"), ArtifactAction::new("Save")]
    }

    fn lines(widget: Artifact<'_>, w: u16, h: u16) -> String {
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
    fn the_header_is_title_description_then_actions() {
        let a = actions();
        let out = lines(
            Artifact::new("plan.md").description("the plan").actions(&a),
            22,
            8,
        );
        assert!(out.contains("plan.md"), "got {out:?}");
        assert!(out.contains("the plan"), "got {out:?}");
        assert!(out.contains("[ Copy ]"), "got {out:?}");
        assert!(out.contains("[ Save ]"), "got {out:?}");
    }

    #[test]
    fn the_body_is_below_the_header_inside_the_border() {
        let a = actions();
        let art = Artifact::new("t").description("d").actions(&a);
        // border (1) + 3 header rows → body starts at inner y 1+3 = 4.
        let body = art.body(Rect::new(0, 0, 20, 10));
        assert_eq!(body, Rect::new(1, 4, 18, 5));
    }

    #[test]
    fn the_header_shrinks_when_description_or_actions_are_absent() {
        // Title only → 1 header row, body right under it.
        let art = Artifact::new("t");
        assert_eq!(art.body(Rect::new(0, 0, 20, 10)), Rect::new(1, 2, 18, 7));
    }

    #[test]
    fn action_rects_track_each_chip() {
        let a = actions();
        let art = Artifact::new("t").actions(&a);
        let rects = art.action_rects(Rect::new(0, 0, 24, 6));
        // "[ Copy ]" is 8 wide, then a gap, then "[ Save ]".
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].width, 8);
        assert_eq!(rects[1].x, rects[0].x + 9);
        // No actions → no rects.
        assert!(
            Artifact::new("t")
                .action_rects(Rect::new(0, 0, 24, 6))
                .is_empty()
        );
    }

    #[test]
    fn a_tiny_panel_clips_to_an_empty_body_without_panicking() {
        let a = actions();
        let art = Artifact::new("t").description("d").actions(&a);
        // No inner room → empty body, zero action rects, no panic.
        let body = art.body(Rect::new(0, 0, 2, 2));
        assert_eq!(body.height, 0);
        let out = lines(Artifact::new("t"), 4, 2);
        assert!(out.starts_with('┌'), "got {out:?}");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Artifact::new("t").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
