//! [`ChainOfThought`] — a stepwise reasoning timeline, the rstui
//! translation of the ai-elements `ChainOfThought` /
//! `ChainOfThoughtHeader` / `ChainOfThoughtStep` /
//! `ChainOfThoughtContent` family (`chain-of-thought.tsx`).
//!
//! # A pure projection of caller-owned steps + an open flag
//!
//! ai-elements' `ChainOfThought` is a `Collapsible` whose content is a
//! vertical list of `ChainOfThoughtStep`s, each a status dot, a
//! connector line, a label, and an optional description. The status
//! (`complete`/`active`/`pending`) tints the row. Here the steps and
//! *which open* are caller-owned model state ([`ChainStep`] is a plain
//! value type); [`ChainOfThought`] only *reads* them (ADR 0012 §P1).
//!
//! Each step is drawn as a status **dot** + a vertical **connector**
//! (`│`) joining it to the next, then the label, then (on the next row,
//! indented) the dim description:
//!
//! - [`ChainStepStatus::Complete`] → `●` (a settled, dim step);
//! - [`ChainStepStatus::Active`] → `◆` (the current, foreground step);
//! - [`ChainStepStatus::Pending`] → `○` (a not-yet-reached, faint step).
//!
//! # The collapse seam
//!
//! [`ChainOfThought::header_rect`] / [`ChainOfThought::body_rect`] are
//! pure geometry accessors; the reducer flips the caller-owned `open`
//! `bool` on a header click — exactly the
//! [`Accordion::layout`](rstui_widgets::Accordion::layout) contract.
//!
//! # Total, never a panic
//!
//! An empty area, a zero-size area, no steps, and more step rows than
//! fit are all safe clips/no-ops (the [`Gauge`](rstui_widgets::Gauge)
//! totality rule).

use rstui_core::{Buffer, Color, Line, Modifier, Position, Rect, Span, Style, Widget};

/// The lifecycle of a [`ChainStep`] (the ai-elements step `status`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ChainStepStatus {
    /// A finished step (`complete`) — the default, drawn dim with a `●`.
    #[default]
    Complete,
    /// The step in progress (`active`) — foreground, drawn with a `◆`.
    Active,
    /// A not-yet-reached step (`pending`) — faint, drawn with a `○`.
    Pending,
}

impl ChainStepStatus {
    /// The status dot glyph (`●` complete / `◆` active / `○` pending).
    #[must_use]
    pub fn dot(self) -> char {
        match self {
            Self::Complete => '●',
            Self::Active => '◆',
            Self::Pending => '○',
        }
    }

    /// The accent the row is drawn with (the ai-elements
    /// `stepStatusStyles`: active foreground, complete/pending dim).
    #[must_use]
    pub fn style(self) -> Style {
        match self {
            Self::Active => Style::new().add_modifier(Modifier::BOLD),
            Self::Complete => Style::new().fg(Color::Gray),
            Self::Pending => Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        }
    }
}

/// One step in a [`ChainOfThought`]: a label, an optional description, and
/// a [`ChainStepStatus`]. A plain value type the caller owns; the widget
/// only reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainStep {
    /// The step's one-line label.
    pub label: String,
    /// An optional second (dim) description line.
    pub description: Option<String>,
    /// Where the step is in its lifecycle.
    pub status: ChainStepStatus,
}

impl ChainStep {
    /// A step labelled `label` with no description, defaulting to
    /// [`ChainStepStatus::Complete`].
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: None,
            status: ChainStepStatus::Complete,
        }
    }

    /// Sets the description line.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the [`ChainStepStatus`].
    #[must_use]
    pub fn status(mut self, status: ChainStepStatus) -> Self {
        self.status = status;
        self
    }

    /// The rows this step occupies: 1 (label) + 1 if it has a
    /// description.
    fn rows(&self) -> u16 {
        1 + u16::from(self.description.is_some())
    }
}

/// A stepwise reasoning timeline — a pure projection of caller-owned
/// `&[ChainStep]` plus a caller-owned [`open`](Self::open) `bool`.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_ai::chain_of_thought::{ChainOfThought, ChainStep, ChainStepStatus};
///
/// let steps = [
///     ChainStep::new("Parsed the request").status(ChainStepStatus::Complete),
///     ChainStep::new("Searching").status(ChainStepStatus::Active),
///     ChainStep::new("Answer").status(ChainStepStatus::Pending),
/// ];
/// let cot = ChainOfThought::new(&steps).open(true);
/// let mut buf = Buffer::empty(Rect::new(0, 0, 30, 6));
/// cot.render(buf.area(), &mut buf);
/// ```
#[derive(Debug, Clone)]
pub struct ChainOfThought<'a> {
    steps: &'a [ChainStep],
    title: &'a str,
    open: bool,
    style: Style,
    header_style: Style,
}

impl<'a> ChainOfThought<'a> {
    /// A collapsed timeline over `steps` titled "Chain of Thought"
    /// (the ai-elements default), unstyled (a dim header by default).
    #[must_use]
    pub fn new(steps: &'a [ChainStep]) -> Self {
        Self {
            steps,
            title: "Chain of Thought",
            open: false,
            style: Style::new(),
            header_style: Style::new().fg(Color::DarkGray),
        }
    }

    /// Overrides the header title (default "Chain of Thought").
    #[must_use]
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    /// Sets whether the timeline is expanded — caller-owned state the
    /// reducer flips on a [`header_rect`](Self::header_rect) click; the
    /// widget only reads it.
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the base [`Style`] (also fills the region).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the header [`Style`] (default a dim foreground).
    #[must_use]
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// The header row rect (🧠 + title + marker), or `None` for an empty
    /// area. A pure function of `area` — the reducer hit-tests a click
    /// against it.
    #[must_use]
    pub fn header_rect(&self, area: Rect) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        Some(Rect::new(area.left(), area.top(), area.width, 1))
    }

    /// The timeline-body rect, or `None` when collapsed or there is no
    /// row below the header. A pure function of `area` and
    /// [`open`](Self::open).
    #[must_use]
    pub fn body_rect(&self, area: Rect) -> Option<Rect> {
        if !self.open || area.is_empty() || area.height < 2 {
            return None;
        }
        Some(Rect::new(
            area.left(),
            area.top().saturating_add(1),
            area.width,
            area.height.saturating_sub(1),
        ))
    }

    /// The total body rows the steps need (1 per step, plus 1 more for
    /// each step that has a description). A pure measurement, for a caller
    /// sizing the timeline; saturates at [`u16::MAX`].
    #[must_use]
    pub fn body_height(&self) -> u16 {
        let rows: u32 = self.steps.iter().map(|s| u32::from(s.rows())).sum();
        u16::try_from(rows).unwrap_or(u16::MAX)
    }

    /// The header [`Line`]: a 🧠 glyph, the title, a ▾/▸ marker.
    fn header_line(&self, base: Style) -> Line<'static> {
        let marker = if self.open { '▾' } else { '▸' };
        Line::from(vec![
            Span::raw("🧠 "),
            Span::styled(self.title.to_owned(), base),
            Span::raw(format!(" {marker}")),
        ])
    }
}

impl Widget for ChainOfThought<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let header_base = self.style.patch(self.header_style);
        if let Some(header) = self.header_rect(area) {
            buf.set_style(header, self.style);
            self.header_line(header_base).render(header, buf);
        }

        let Some(body) = self.body_rect(area) else {
            return;
        };
        buf.set_style(body, self.style);

        let dot_x = body.left();
        let label_x = body.left().saturating_add(2);
        let label_w = body.width.saturating_sub(2);
        let last = self.steps.len().saturating_sub(1);
        let mut y = body.top();
        for (i, step) in self.steps.iter().enumerate() {
            if y >= body.bottom() {
                break;
            }
            let accent = self.style.patch(step.status.style());

            // The status dot.
            buf.set_cell(Position::new(dot_x, y), step.status.dot(), accent);

            // The label on the dot's row.
            if label_w > 0 {
                Line::styled(step.label.clone(), accent)
                    .render(Rect::new(label_x, y, label_w, 1), buf);
            }

            // The optional description on the next (indented) row.
            let mut next_y = y.saturating_add(1);
            if let Some(desc) = &step.description {
                if next_y < body.bottom() {
                    // A connector glyph in the gutter beside the description.
                    if i != last {
                        buf.set_cell(
                            Position::new(dot_x, next_y),
                            '│',
                            self.style.patch(self.header_style),
                        );
                    }
                    if label_w > 0 {
                        Line::styled(
                            desc.clone(),
                            self.style.fg(Color::DarkGray).add_modifier(Modifier::DIM),
                        )
                        .render(Rect::new(label_x, next_y, label_w, 1), buf);
                    }
                    next_y = next_y.saturating_add(1);
                }
            } else if i != last && next_y < body.bottom() {
                // No description: still draw the connector to the next dot.
                buf.set_cell(
                    Position::new(dot_x, next_y),
                    '│',
                    self.style.patch(self.header_style),
                );
            }

            y = next_y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(buf: &Buffer, y: u16, w: u16) -> String {
        (0..w)
            .map(|x| buf.get(Position::new(x, y)).unwrap().symbol)
            .collect()
    }

    fn dump(buf: &Buffer, w: u16, h: u16) -> String {
        let mut out = String::new();
        for y in 0..h {
            out.push_str(&row(buf, y, w));
            out.push('\n');
        }
        out
    }

    fn steps() -> Vec<ChainStep> {
        vec![
            ChainStep::new("Parsed").status(ChainStepStatus::Complete),
            ChainStep::new("Searching").status(ChainStepStatus::Active),
            ChainStep::new("Answer").status(ChainStepStatus::Pending),
        ]
    }

    #[test]
    fn status_dots_and_styles_match_the_spec() {
        assert_eq!(ChainStepStatus::Complete.dot(), '●');
        assert_eq!(ChainStepStatus::Active.dot(), '◆');
        assert_eq!(ChainStepStatus::Pending.dot(), '○');
        assert!(
            ChainStepStatus::Active
                .style()
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn the_header_is_always_drawn_with_a_marker() {
        let s = steps();
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 3));
        ChainOfThought::new(&s).render(buf.area(), &mut buf);
        let header = row(&buf, 0, 30);
        assert!(header.contains("Chain of Thought"), "{header:?}");
        assert!(header.contains('▸'), "collapsed marker: {header:?}");
    }

    #[test]
    fn collapsed_draws_no_steps() {
        let s = steps();
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 6));
        ChainOfThought::new(&s).render(buf.area(), &mut buf);
        assert_eq!(
            ChainOfThought::new(&s).body_rect(Rect::new(0, 0, 30, 6)),
            None
        );
        let mut text = String::new();
        for y in 1..6 {
            text.push_str(&row(&buf, y, 30));
        }
        assert!(!text.contains("Parsed"), "collapsed must hide the steps");
    }

    #[test]
    fn open_renders_the_dotted_timeline() {
        let s = steps();
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 6));
        ChainOfThought::new(&s)
            .open(true)
            .render(buf.area(), &mut buf);
        let header = row(&buf, 0, 30);
        assert!(header.contains('▾'), "open marker: {header:?}");
        // Row 1: complete dot ● + "Parsed".
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '●');
        assert!(row(&buf, 1, 30).contains("Parsed"));
        // Row 2: active dot ◆ + "Searching".
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, '◆');
        assert!(row(&buf, 2, 30).contains("Searching"));
        // Row 3: pending dot ○ + "Answer".
        assert_eq!(buf.get(Position::new(0, 3)).unwrap().symbol, '○');
    }

    #[test]
    fn a_description_takes_the_next_row_with_a_connector() {
        let s = vec![
            ChainStep::new("Step one").description("the detail"),
            ChainStep::new("Step two"),
        ];
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 6));
        ChainOfThought::new(&s)
            .open(true)
            .render(buf.area(), &mut buf);
        let text = dump(&buf, 30, 6);
        assert!(text.contains("Step one"), "{text}");
        assert!(text.contains("the detail"), "description shown: {text}");
        // The connector sits in the gutter on the description row (row 2).
        assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, '│');
    }

    #[test]
    fn the_active_step_label_is_foreground_bold() {
        let s = vec![ChainStep::new("Now").status(ChainStepStatus::Active)];
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
        ChainOfThought::new(&s)
            .open(true)
            .render(buf.area(), &mut buf);
        // 'N' of "Now" at x=2, row 1.
        let cell = buf.get(Position::new(2, 1)).unwrap();
        assert_eq!(cell.symbol, 'N');
        assert!(cell.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn body_height_sums_step_rows() {
        let s = vec![ChainStep::new("a"), ChainStep::new("b").description("d")];
        // 1 + (1 + 1) = 3.
        assert_eq!(ChainOfThought::new(&s).body_height(), 3);
    }

    #[test]
    fn more_steps_than_rows_clip_without_a_panic() {
        let s: Vec<_> = (0..40).map(|i| ChainStep::new(format!("s{i}"))).collect();
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 4));
        ChainOfThought::new(&s)
            .open(true)
            .render(buf.area(), &mut buf);
        assert!(row(&buf, 1, 12).contains("s0"));
    }

    #[test]
    fn no_steps_open_is_total() {
        let s: [ChainStep; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
        ChainOfThought::new(&s)
            .open(true)
            .render(buf.area(), &mut buf);
        assert!(row(&buf, 0, 20).contains("Chain of Thought"));
        assert_eq!(ChainOfThought::new(&s).body_height(), 0);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let s = steps();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
        ChainOfThought::new(&s)
            .open(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        assert_eq!(
            ChainOfThought::new(&s).header_rect(Rect::new(0, 0, 0, 0)),
            None
        );
    }
}
