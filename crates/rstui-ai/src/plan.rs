//! [`Plan`] — a collapsible plan card, the rstui translation of the
//! ai-elements `Plan` / `PlanHeader` / `PlanTitle` / `PlanDescription` /
//! `PlanContent` family (`plan.tsx`).
//!
//! # A pure projection of caller-owned state
//!
//! ai-elements' `Plan` is a `Card` wrapped in a `Collapsible`; a
//! `streaming` flag wraps the title/description in a `Shimmer`. Here the
//! title, description, [`steps`](Plan::new), the `streaming` flag, and
//! *which open* are all caller-owned model state; [`Plan`] only *reads*
//! them (ADR 0012 §P1).
//!
//! The shimmer is **not** an animation here: there is no animation crate
//! and a wall clock cannot enter a pure `view` (the
//! [`Spinner`](rstui_widgets::Spinner) caller-owned-tick precedent). Per
//! the spec, [`streaming`](Plan::streaming) simply *styles* the title
//! dim+italic (a "still being written" affordance) — a static projection,
//! not a frame loop.
//!
//! # What it draws
//!
//! A bordered [`Card`](rstui_widgets::Card): the header is the title
//! (with a ▾/▸ disclosure marker), then a dim description line; when
//! [`open`](Plan::open) the body is the numbered steps. The collapse seam
//! is the usual [`Plan::header_rect`] / [`Plan::body_rect`] pair (the
//! reducer flips the caller-owned `open` `bool` on a header click) —
//! exactly the [`Accordion::layout`](rstui_widgets::Accordion::layout)
//! contract.
//!
//! # Total, never a panic
//!
//! An empty area, a zero-size area, no steps, and more steps than rows
//! are all safe clips/no-ops (the [`Gauge`](rstui_widgets::Gauge)
//! totality rule).

use rstui_core::{Buffer, Color, Line, Modifier, Position, Rect, Span, Style, Widget};
use rstui_widgets::{Block, Borders};

/// A collapsible plan card — a pure projection of a title, an optional
/// description, caller-owned `&[String]` steps, and caller-owned
/// [`streaming`](Self::streaming) / [`open`](Self::open) flags.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_ai::plan::Plan;
///
/// let steps = ["Read the spec".to_owned(), "Write the code".to_owned()];
/// let plan = Plan::new("Implement feature", &steps)
///     .description("A two-step plan")
///     .open(true);
/// let mut buf = Buffer::empty(Rect::new(0, 0, 30, 8));
/// plan.render(buf.area(), &mut buf);
/// ```
#[derive(Debug, Clone)]
pub struct Plan<'a> {
    title: &'a str,
    description: Option<&'a str>,
    steps: &'a [String],
    streaming: bool,
    open: bool,
    style: Style,
    title_style: Style,
    description_style: Style,
}

impl<'a> Plan<'a> {
    /// A collapsed, not-streaming plan titled `title` over `steps`, no
    /// description, unstyled (a bold title, a dim description by default).
    #[must_use]
    pub fn new(title: &'a str, steps: &'a [String]) -> Self {
        Self {
            title,
            description: None,
            steps,
            streaming: false,
            open: false,
            style: Style::new(),
            title_style: Style::new().add_modifier(Modifier::BOLD),
            description_style: Style::new().fg(Color::DarkGray),
        }
    }

    /// Sets the description line shown under the title (the ai-elements
    /// `PlanDescription`).
    #[must_use]
    pub fn description(mut self, description: &'a str) -> Self {
        self.description = Some(description);
        self
    }

    /// Sets whether the plan is still being written — caller-owned state
    /// (from the streaming turn). Per the spec this **styles** the title
    /// dim+italic (the `Shimmer` analogue); it is not animated (see the
    /// [module docs](self)).
    #[must_use]
    pub fn streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }

    /// Sets whether the card is expanded — caller-owned state the reducer
    /// flips on a [`header_rect`](Self::header_rect) click; the widget only
    /// reads it.
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the base [`Style`] (also fills the card region).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the title [`Style`] (default bold).
    #[must_use]
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// Sets the description [`Style`] (default a dim foreground).
    #[must_use]
    pub fn description_style(mut self, style: Style) -> Self {
        self.description_style = style;
        self
    }

    /// The framing [`Block`] — the single definition the rects and
    /// [`render`](Widget::render) share so they never disagree.
    fn frame() -> Block<'static> {
        Block::new().borders(Borders::ALL)
    }

    /// The header row rect (title + marker) inside the frame, or `None`
    /// when there is no room. A pure function of `area` — the reducer
    /// hit-tests a click against it.
    #[must_use]
    pub fn header_rect(&self, area: Rect) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        let inner = Self::frame().inner(area);
        if inner.is_empty() {
            return None;
        }
        Some(Rect::new(inner.left(), inner.top(), inner.width, 1))
    }

    /// The steps-body rect, or `None` when collapsed or there is no row
    /// below the header (+ description). A pure function of `area` and
    /// [`open`](Self::open).
    #[must_use]
    pub fn body_rect(&self, area: Rect) -> Option<Rect> {
        if !self.open {
            return None;
        }
        let header = self.header_rect(area)?;
        let inner = Self::frame().inner(area);
        // The description (if any) takes the row directly below the title.
        let desc_rows = u16::from(self.description.is_some());
        let body_top = header.bottom().saturating_add(desc_rows);
        if body_top >= inner.bottom() {
            return None;
        }
        Some(Rect::new(
            inner.left(),
            body_top,
            inner.width,
            inner.bottom().saturating_sub(body_top),
        ))
    }

    /// The header [`Line`]: the title (dim+italic while
    /// [`streaming`](Self::streaming)) and a ▾/▸ disclosure marker.
    fn header_line(&self, base: Style) -> Line<'static> {
        let marker = if self.open { '▾' } else { '▸' };
        let title_style = if self.streaming {
            base.patch(self.title_style)
                .add_modifier(Modifier::DIM | Modifier::ITALIC)
        } else {
            base.patch(self.title_style)
        };
        Line::from(vec![
            Span::styled(format!("{marker} "), base),
            Span::styled(self.title.to_owned(), title_style),
        ])
    }
}

impl Widget for Plan<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let base = self.style;
        let frame = Self::frame();
        let inner = frame.inner(area);
        frame.style(base).render(area, buf);
        if inner.is_empty() {
            return;
        }

        if let Some(header) = self.header_rect(area) {
            buf.set_style(header, base);
            self.header_line(base).render(header, buf);

            // The description rides the row directly under the title.
            if let Some(desc) = self.description {
                let desc_y = header.bottom();
                if desc_y < inner.bottom() {
                    let desc_area = Rect::new(inner.left(), desc_y, inner.width, 1);
                    let style = if self.streaming {
                        base.patch(self.description_style)
                            .add_modifier(Modifier::DIM | Modifier::ITALIC)
                    } else {
                        base.patch(self.description_style)
                    };
                    Line::styled(desc.to_owned(), style).render(desc_area, buf);
                }
            }
        }

        let Some(body) = self.body_rect(area) else {
            return;
        };
        buf.set_style(body, base);
        // Numbered steps (`1. …`), one per row, clipped to the body.
        for (i, step) in self.steps.iter().enumerate() {
            if i as u16 >= body.height {
                break;
            }
            let y = body.top().saturating_add(i as u16);
            let line = Line::from(vec![
                Span::styled(format!("{}. ", i + 1), base.fg(Color::DarkGray)),
                Span::styled(step.clone(), base),
            ]);
            line.render(Rect::new(body.left(), y, body.width, 1), buf);
            // Keep the gutter glyph visible even with a 0-width body slice.
            if body.width == 0 {
                buf.set_cell(Position::new(body.left(), y), ' ', base);
            }
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

    fn steps() -> Vec<String> {
        vec!["First step".to_owned(), "Second step".to_owned()]
    }

    #[test]
    fn the_header_shows_the_title_and_a_marker() {
        let s = steps();
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 3));
        Plan::new("My Plan", &s).render(buf.area(), &mut buf);
        let header = row(&buf, 1, 30); // row 0 is the border
        assert!(header.contains("My Plan"), "{header:?}");
        assert!(header.contains('▸'), "collapsed marker: {header:?}");
    }

    #[test]
    fn collapsed_draws_no_steps() {
        let s = steps();
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 8));
        Plan::new("P", &s).render(buf.area(), &mut buf);
        assert_eq!(Plan::new("P", &s).body_rect(Rect::new(0, 0, 30, 8)), None);
        assert!(
            !dump(&buf, 30, 8).contains("First step"),
            "collapsed must hide the steps"
        );
    }

    #[test]
    fn open_renders_the_numbered_steps() {
        let s = steps();
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 8));
        Plan::new("Plan", &s)
            .open(true)
            .render(buf.area(), &mut buf);
        let text = dump(&buf, 30, 8);
        assert!(text.contains("1. First step"), "{text}");
        assert!(text.contains("2. Second step"), "{text}");
    }

    #[test]
    fn a_description_sits_under_the_title_and_pushes_the_body_down() {
        let s = steps();
        let area = Rect::new(0, 0, 30, 10);
        // The rects are a pure function of the config (computed before
        // `render` consumes the builder).
        let probe = Plan::new("Plan", &s).description("the why").open(true);
        let header = probe.header_rect(area).unwrap();
        let body = probe.body_rect(area).unwrap();
        let mut buf = Buffer::empty(area);
        Plan::new("Plan", &s)
            .description("the why")
            .open(true)
            .render(area, &mut buf);
        let text = dump(&buf, 30, 10);
        assert!(text.contains("the why"), "description shown: {text}");
        assert!(text.contains("1. First step"), "{text}");
        // Body starts below header + description row.
        assert_eq!(body.top(), header.bottom() + 1);
    }

    #[test]
    fn streaming_styles_the_title_dim_italic_not_animated() {
        let s = steps();
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 3));
        Plan::new("Live Plan", &s)
            .streaming(true)
            .render(buf.area(), &mut buf);
        // Find the 'L' of the title and check it is dim+italic.
        let mut checked = false;
        for x in 0..30 {
            let cell = buf.get(Position::new(x, 1)).unwrap();
            if cell.symbol == 'L' {
                assert!(cell.modifier.contains(Modifier::DIM));
                assert!(cell.modifier.contains(Modifier::ITALIC));
                checked = true;
                break;
            }
        }
        assert!(checked, "title 'L' not found on the header row");
    }

    #[test]
    fn more_steps_than_rows_clip_without_a_panic() {
        let s: Vec<_> = (0..40).map(|i| format!("step{i}")).collect();
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 5));
        Plan::new("Big", &s).open(true).render(buf.area(), &mut buf);
        assert!(dump(&buf, 16, 5).contains("1. step0"));
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let s = steps();
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 8));
        Plan::new("P", &s)
            .open(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        assert_eq!(Plan::new("P", &s).header_rect(Rect::new(0, 0, 0, 0)), None);
        assert_eq!(Plan::new("P", &s).body_rect(Rect::new(0, 0, 0, 0)), None);
    }

    #[test]
    fn a_tiny_area_with_no_inner_is_total() {
        let s = steps();
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        Plan::new("P", &s).open(true).render(buf.area(), &mut buf);
        assert_eq!(Plan::new("P", &s).header_rect(Rect::new(0, 0, 2, 2)), None);
    }
}
