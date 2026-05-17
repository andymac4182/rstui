//! [`Stepper`] — a wizard / progress-steps strip: numbered (or checked) step
//! nodes joined by connectors with labels, the "Step 2 of 4" rail an
//! installer, an onboarding flow, or a checkout pins along an edge.
//!
//! # A pure projection of caller-owned steps + `current`
//!
//! Like every rstui widget `Stepper` is a **pure projection**: it renders the
//! caller-owned `&[Step]` and the [`current`](Stepper::current) index it is
//! handed and reads nothing else. Both are ordinary application state the
//! reducer owns and moves in `update` (advance on "Next", jump on a click);
//! the widget only ever reads, exactly the read-only-state rule
//! [`List`](crate::List) establishes. A step **before** `current` is *done*
//! (its node is a check `✓`); the one **at** `current` and those **after** it
//! show their 1-based number — each in its own [`Style`].
//!
//! # Horizontal or vertical, composes with `Block`
//!
//! [`StepperOrientation::Horizontal`] (the default) lays the steps left to
//! right on one row joined by ` ── ` connectors;
//! [`StepperOrientation::Vertical`] lays one step per row joined by a `│`
//! connector. Like the container widgets it takes an optional framing
//! [`Block`]; the steps render into [`Block::inner`] and the base
//! [`style`](Stepper::style) fills that content area, the same compose pattern
//! [`Calendar`](crate::Calendar)/[`List`](crate::List) use.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*): no
//! steps (just the base/frame), a single step (no connector), a `current` past
//! the end (every step simply reads as done), an empty area, a narrow row /
//! short column (the strip clips), and a multi-row area are all safe
//! clips/no-ops — never a panic.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The glyph a completed step's node is drawn as.
const CHECK: char = '✓';

/// The connector drawn between adjacent horizontal steps.
const CONNECTOR_H: &str = " ── ";

/// The connector drawn between adjacent vertical steps.
const CONNECTOR_V: char = '│';

/// Which way a [`Stepper`] lays its steps out.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum StepperOrientation {
    /// Left to right on one row, joined by ` ── ` (the default).
    #[default]
    Horizontal,
    /// Top to bottom, one step per row, joined by `│`.
    Vertical,
}

/// One step of a [`Stepper`]: a single [`Line`] label.
///
/// Build one from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`]); style it through the [`Line`] it
/// wraps (it cascades over the step's state style).
#[derive(Debug, Default, Clone)]
pub struct Step<'a> {
    label: Line<'a>,
}

impl<'a> Step<'a> {
    /// A step labelled `label` (any value convertible to a [`Line`]).
    pub fn new(label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

/// A wizard progress strip — a pure projection of caller-owned steps and a
/// [`current`](Self::current) index.
///
/// Each node is a check `✓` when the step is *done* (before `current`) or its
/// 1-based number otherwise. A step's node and label take
/// [`done_style`](Self::done_style) / [`current_style`](Self::current_style) /
/// [`pending_style`](Self::pending_style) by state (each patched over the base
/// [`style`](Self::style), then the label's own [`Line`] styles cascade on
/// top); connectors take [`connector_style`](Self::connector_style).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Step, Stepper};
///
/// // `current` is plain caller-owned model state the widget only reads —
/// // advancing the wizard is the reducer's job.
/// let steps = [Step::new("One"), Step::new("Two"), Step::new("Three")];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 30, 1));
/// Stepper::new(steps).current(1).render(buf.area(), &mut buf);
///
/// // Step 0 is done (a check), step 1 (current) and step 2 show numbers.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '✓');
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'O'); // "One"
/// assert_eq!(buf.get(Position::new(9, 0)).unwrap().symbol, '2');
/// ```
#[derive(Debug, Default, Clone)]
pub struct Stepper<'a> {
    steps: Vec<Step<'a>>,
    current: usize,
    orientation: StepperOrientation,
    block: Option<Block<'a>>,
    style: Style,
    done_style: Style,
    current_style: Style,
    pending_style: Style,
    connector_style: Style,
}

impl<'a> Stepper<'a> {
    /// A stepper over `steps`, the first one current, laid out horizontally.
    pub fn new<I>(steps: I) -> Self
    where
        I: IntoIterator<Item = Step<'a>>,
    {
        Self {
            steps: steps.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Sets the current step index — caller-owned state the widget only reads.
    /// Steps before it are *done* (a check); an index past the end simply
    /// reads every step as done.
    #[must_use]
    pub fn current(mut self, current: usize) -> Self {
        self.current = current;
        self
    }

    /// Sets the layout [`StepperOrientation`] (default
    /// [`Horizontal`](StepperOrientation::Horizontal)).
    #[must_use]
    pub fn orientation(mut self, orientation: StepperOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Frames the stepper in `block`; the steps render into
    /// [`Block::inner`].
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a background
    /// covers the whole pane.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched over a *done* step (before `current`).
    #[must_use]
    pub fn done_style(mut self, style: Style) -> Self {
        self.done_style = style;
        self
    }

    /// Sets the [`Style`] patched over the *current* step.
    #[must_use]
    pub fn current_style(mut self, style: Style) -> Self {
        self.current_style = style;
        self
    }

    /// Sets the [`Style`] patched over a *pending* step (after `current`).
    #[must_use]
    pub fn pending_style(mut self, style: Style) -> Self {
        self.pending_style = style;
        self
    }

    /// Sets the [`Style`] patched over the connectors between steps.
    #[must_use]
    pub fn connector_style(mut self, style: Style) -> Self {
        self.connector_style = style;
        self
    }

    /// The state style for step `i`, patched over the base.
    fn state_style(&self, i: usize) -> Style {
        let state = if i < self.current {
            self.done_style
        } else if i == self.current {
            self.current_style
        } else {
            self.pending_style
        };
        self.style.patch(state)
    }

    /// Stamps step `i`'s node — a check glyph when done, else its 1-based
    /// number — left-to-right from `x` on row `y`, clipped at `right`;
    /// returns the new x. Writes the digits from a stack buffer, so there is
    /// no per-step `String` allocation every frame (W5-STEP-2). `usize` is at
    /// most 20 decimal digits, so `[u8; 20]` never overflows.
    fn stamp_node(
        &self,
        buf: &mut Buffer,
        i: usize,
        mut x: u16,
        y: u16,
        right: u16,
        st: Style,
    ) -> u16 {
        if i < self.current {
            if x < right {
                buf.set_cell(Position::new(x, y), CHECK, st);
                x = x.saturating_add(1);
            }
            return x;
        }
        let mut n = i + 1;
        let mut digits = [0u8; 20];
        let mut len = 0;
        loop {
            digits[len] = b'0' + (n % 10) as u8;
            len += 1;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        while len > 0 {
            len -= 1;
            if x >= right {
                break;
            }
            buf.set_cell(Position::new(x, y), digits[len] as char, st);
            x = x.saturating_add(1);
        }
        x
    }
}

impl Widget for Stepper<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // The block (if any) frames the content and reserves the inner area.
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if let Some(b) = &self.block {
            b.render_ref(area, buf);
        }
        if inner.is_empty() {
            return;
        }

        // Base fills the content area so a background covers the whole pane.
        buf.set_style(inner, self.style);

        let right = inner.right();
        let conn_style = self.style.patch(self.connector_style);
        let n = self.steps.len();

        match self.orientation {
            StepperOrientation::Horizontal => {
                let y = inner.top();
                let mut x = inner.left();
                'row: for (i, step) in self.steps.iter().enumerate() {
                    if i > 0 {
                        for ch in CONNECTOR_H.chars() {
                            if x >= right {
                                break 'row;
                            }
                            buf.set_cell(Position::new(x, y), ch, conn_style);
                            x = x.saturating_add(1);
                        }
                    }
                    let st = self.state_style(i);
                    x = self.stamp_node(buf, i, x, y, right, st);
                    if x >= right {
                        break 'row;
                    }
                    buf.set_cell(Position::new(x, y), ' ', st);
                    x = x.saturating_add(1);
                    let line_base = st.patch(step.label.style);
                    for span in &step.label.spans {
                        let span_style = line_base.patch(span.style);
                        for ch in span.content.chars() {
                            if x >= right {
                                break 'row;
                            }
                            buf.set_cell(Position::new(x, y), ch, span_style);
                            x = x.saturating_add(1);
                        }
                    }
                }
            }
            StepperOrientation::Vertical => {
                let bottom = inner.bottom();
                let mut y = inner.top();
                for (i, step) in self.steps.iter().enumerate() {
                    if y >= bottom {
                        break;
                    }
                    let st = self.state_style(i);
                    let mut x = self.stamp_node(buf, i, inner.left(), y, right, st);
                    if x < right {
                        buf.set_cell(Position::new(x, y), ' ', st);
                        x = x.saturating_add(1);
                    }
                    let line_base = st.patch(step.label.style);
                    'label: for span in &step.label.spans {
                        let span_style = line_base.patch(span.style);
                        for ch in span.content.chars() {
                            if x >= right {
                                break 'label;
                            }
                            buf.set_cell(Position::new(x, y), ch, span_style);
                            x = x.saturating_add(1);
                        }
                    }
                    y = y.saturating_add(1);
                    if i + 1 < n && y < bottom {
                        buf.set_cell(Position::new(inner.left(), y), CONNECTOR_V, conn_style);
                        y = y.saturating_add(1);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Span};

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row.
    fn lines<W: Widget>(widget: W, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    fn steps() -> [Step<'static>; 3] {
        [Step::new("One"), Step::new("Two"), Step::new("Three")]
    }

    #[test]
    fn horizontal_lays_nodes_connectors_and_labels_on_one_row() {
        assert_eq!(
            lines(Stepper::new(steps()).current(1), 25, 1),
            "✓ One ── 2 Two ── 3 Three\n"
        );
    }

    #[test]
    fn done_steps_show_a_check_others_their_number() {
        // current 2: steps 0,1 done (checks), step 2 current ("3").
        assert_eq!(
            lines(
                Stepper::new([Step::new("X"), Step::new("Y"), Step::new("Z")]).current(2),
                17,
                1
            ),
            "✓ X ── ✓ Y ── 3 Z\n"
        );
    }

    #[test]
    fn a_current_past_the_end_reads_every_step_as_done() {
        assert_eq!(
            lines(
                Stepper::new([Step::new("A"), Step::new("B")]).current(9),
                10,
                1
            ),
            "✓ A ── ✓ B\n"
        );
    }

    #[test]
    fn vertical_lays_one_step_per_row_joined_by_a_connector() {
        let stepper = Stepper::new([Step::new("A"), Step::new("B")])
            .current(0)
            .orientation(StepperOrientation::Vertical);
        assert_eq!(lines(stepper, 4, 3), "1 A \n│   \n2 B \n");
    }

    #[test]
    fn the_current_step_takes_the_current_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        Stepper::new([Step::new("A")])
            .current(0)
            .current_style(Style::new().bg(Color::Cyan))
            .render(buf.area(), &mut buf);
        // "1 A" — node, space, and label all carry the current bg.
        for x in 0..3 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Cyan);
        }
    }

    #[test]
    fn a_done_step_takes_the_done_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        Stepper::new([Step::new("A"), Step::new("B")])
            .current(1)
            .done_style(Style::new().bg(Color::Green))
            .render(buf.area(), &mut buf);
        // "✓ A ── 2 B": step 0 (cols 0..3) is done → green.
        for x in 0..3 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Green);
        }
        // The connector (col 4) and the current step (col 7) are not green.
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().bg, Color::Reset);
        assert_eq!(buf.get(Position::new(7, 0)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn a_pending_step_takes_the_pending_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        Stepper::new([Step::new("A"), Step::new("B")])
            .current(0)
            .pending_style(Style::new().bg(Color::Yellow))
            .render(buf.area(), &mut buf);
        // "1 A ── 2 B": step 1 starts at col 7 ("2 B").
        for x in 7..10 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Yellow);
        }
        // step 0 (current, unset) is not yellow.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn connectors_take_the_connector_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        Stepper::new([Step::new("A"), Step::new("B")])
            .current(0)
            .connector_style(Style::new().fg(Color::Red))
            .render(buf.area(), &mut buf);
        // "1 A ── 2 B": the connector occupies cols 3..7.
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, '─');
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().fg, Color::Red);
        // The node keeps its own (base) style, not the connector style.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Reset);
    }

    #[test]
    fn the_vertical_connector_takes_the_connector_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        Stepper::new([Step::new("A"), Step::new("B")])
            .current(0)
            .orientation(StepperOrientation::Vertical)
            .connector_style(Style::new().fg(Color::Red))
            .render(buf.area(), &mut buf);
        let conn = buf.get(Position::new(0, 1)).unwrap();
        assert_eq!(conn.symbol, '│');
        assert_eq!(conn.fg, Color::Red);
    }

    #[test]
    fn a_block_frames_the_stepper_in_the_inner_area() {
        let stepper = Stepper::new([Step::new("A")])
            .current(0)
            .block(Block::bordered());
        assert_eq!(lines(stepper, 4, 3), "┌──┐\n│1 │\n└──┘\n");
    }

    #[test]
    fn the_base_style_fills_the_whole_content_area() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        Stepper::new([Step::new("A")])
            .current(0)
            .style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Blue);
            }
        }
    }

    #[test]
    fn no_steps_with_a_block_still_renders_the_block() {
        let stepper = Stepper::new(Vec::<Step>::new()).block(Block::bordered());
        assert_eq!(lines(stepper, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn one_step_has_no_connector() {
        assert_eq!(
            lines(Stepper::new([Step::new("Only")]).current(0), 8, 1),
            "1 Only  \n"
        );
    }

    #[test]
    fn a_narrow_row_clips_the_strip() {
        assert_eq!(
            lines(
                Stepper::new([Step::new("AAAA"), Step::new("BBBB")]).current(0),
                5,
                1
            ),
            "1 AAA\n"
        );
    }

    #[test]
    fn a_short_column_clips_the_vertical_strip() {
        let stepper = Stepper::new([Step::new("A"), Step::new("B"), Step::new("C")])
            .current(0)
            .orientation(StepperOrientation::Vertical);
        // Height 2: row 0 is step 0, row 1 the connector; step 1 is clipped.
        assert_eq!(lines(stepper, 3, 2), "1 A\n│  \n");
    }

    #[test]
    fn a_step_label_span_keeps_its_own_style_over_the_state_style() {
        let label = Line::from(Span::styled("R", Style::new().fg(Color::Red)));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        Stepper::new([Step::new(label)])
            .current(0)
            .current_style(Style::new().bg(Color::Cyan))
            .render(buf.area(), &mut buf);
        // "1 R": the label glyph keeps its red fg over the cyan state bg.
        let cell = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(cell.symbol, 'R');
        assert_eq!(cell.fg, Color::Red);
        assert_eq!(cell.bg, Color::Cyan);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Stepper::new(steps())
            .current(1)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
