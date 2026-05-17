//! [`Reasoning`] — a collapsible "thinking" panel, the rstui translation
//! of the ai-elements `Reasoning` / `ReasoningTrigger` /
//! `ReasoningContent` family (`reasoning.tsx`).
//!
//! # A pure projection — the timer and auto-open are the reducer's
//!
//! In ai-elements `Reasoning` keeps a pile of `useState`/`useEffect`: it
//! auto-opens when streaming starts, starts a wall clock, auto-closes a
//! second after streaming ends, and derives a duration. **None of that
//! belongs in an rstui widget** — a wall clock smuggled into a pure
//! `view` is exactly what the [`Spinner`](rstui_widgets::Spinner)
//! caller-owned-tick precedent (and [`Accordion`](rstui_widgets::Accordion)'s
//! `expanded`) forbid. So this widget is the *pure projection* of three
//! pieces of caller-owned model state:
//!
//! - `is_streaming: bool` — whether the agent is still emitting reasoning
//!   (the reducer sets it from the [`StreamState`](crate::model::StreamState)
//!   of the [`UiPart::Reasoning`](crate::model::UiPart::Reasoning) part).
//! - `elapsed_secs: u64` — seconds spent thinking, accumulated by the
//!   reducer from a timer `Cmd` (started when streaming begins, frozen
//!   when it ends). The widget never reads a clock.
//! - `open: bool` — whether the panel is expanded; the reducer's policy
//!   (auto-open on stream start, auto-close ~1s after it ends, or a
//!   header click toggling it) lives in `update`. This is documented
//!   here and **not** enforced by the widget, the same split
//!   [`Accordion`](rstui_widgets::Accordion) records for `expanded`.
//!
//! The header reads `Thinking…` while streaming (or `elapsed == 0`) and
//! `Thought for N seconds` once it has settled — the ai-elements
//! `defaultGetThinkingMessage`. The body renders the reasoning text via
//! [`Markdown`] (the house renderer, the `ReasoningContent` Streamdown
//! analogue) only when [`open`](Reasoning::open).
//!
//! # The collapse seam
//!
//! [`Reasoning::header_rect`] / [`Reasoning::body_rect`] are pure geometry
//! accessors, exactly like [`Accordion::layout`](rstui_widgets::Accordion::layout):
//! the reducer hit-tests a click against the header and flips the
//! caller-owned `open` `bool`.
//!
//! # Total, never a panic
//!
//! An empty area, a zero-size area, an empty reasoning string, and a body
//! taller than the area are all safe clips/no-ops (the
//! [`Gauge`](rstui_widgets::Gauge) totality rule).

use rstui_core::{Buffer, Color, Line, Modifier, Rect, Span, Style, Widget};
use rstui_widgets::Markdown;

/// The header message ai-elements shows for a thinking panel: `Thinking…`
/// while streaming (or before the timer has ticked), else
/// `Thought for N seconds` (`1 second` is singularised, the same polish
/// the ai-elements copy implies).
#[must_use]
pub fn thinking_message(is_streaming: bool, elapsed_secs: u64) -> String {
    if is_streaming || elapsed_secs == 0 {
        "Thinking…".to_owned()
    } else if elapsed_secs == 1 {
        "Thought for 1 second".to_owned()
    } else {
        format!("Thought for {elapsed_secs} seconds")
    }
}

/// A collapsible reasoning ("thinking") panel — a pure projection of the
/// reasoning text plus caller-owned [`is_streaming`](Self::is_streaming),
/// [`elapsed_secs`](Self::elapsed_secs), and [`open`](Self::open).
///
/// The header (a brain glyph, the [`thinking_message`], a ▾/▸ marker) is
/// always drawn; the markdown body only when [`open`](Self::open).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_ai::reasoning::Reasoning;
///
/// // All three flags are caller-owned model state the widget only reads.
/// let r = Reasoning::new("Let me **think**…")
///     .is_streaming(false)
///     .elapsed_secs(3)
///     .open(true);
/// let mut buf = Buffer::empty(Rect::new(0, 0, 30, 4));
/// r.render(buf.area(), &mut buf);
/// ```
#[derive(Debug, Clone)]
pub struct Reasoning<'a> {
    text: &'a str,
    is_streaming: bool,
    elapsed_secs: u64,
    open: bool,
    style: Style,
    header_style: Style,
}

impl<'a> Reasoning<'a> {
    /// A collapsed, not-streaming panel projecting `text` with a zero
    /// elapsed time, otherwise unstyled (a dim header by default).
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            is_streaming: false,
            elapsed_secs: 0,
            open: false,
            style: Style::new(),
            header_style: Style::new().fg(Color::DarkGray),
        }
    }

    /// Sets whether the agent is still emitting reasoning — caller-owned
    /// state (from the part's [`StreamState`](crate::model::StreamState));
    /// the widget only reads it for the header copy.
    #[must_use]
    pub fn is_streaming(mut self, is_streaming: bool) -> Self {
        self.is_streaming = is_streaming;
        self
    }

    /// Sets the caller-owned elapsed thinking time in seconds (the reducer
    /// accumulates it from a timer `Cmd`; the widget never reads a clock).
    #[must_use]
    pub fn elapsed_secs(mut self, elapsed_secs: u64) -> Self {
        self.elapsed_secs = elapsed_secs;
        self
    }

    /// Sets whether the panel is expanded — caller-owned state the reducer
    /// flips (auto-open/close policy or a header click); the widget only
    /// reads it (see the [module docs](self)).
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the base [`Style`] (also fills the panel region).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] of the header row (default a dim foreground).
    #[must_use]
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// The header row rect (the brain + message + marker), or `None` for
    /// an empty area. A pure function of `area` — the reducer hit-tests a
    /// click against it (mirrors
    /// [`Accordion::layout`](rstui_widgets::Accordion::layout)).
    #[must_use]
    pub fn header_rect(&self, area: Rect) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        Some(Rect::new(area.left(), area.top(), area.width, 1))
    }

    /// The markdown-body rect, or `None` when collapsed or there is no row
    /// below the header. A pure function of `area` and
    /// [`open`](Self::open) — render nothing into a `None`.
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

    /// The header [`Line`]: a brain glyph, the [`thinking_message`], and a
    /// ▾ (open) / ▸ (collapsed) disclosure marker.
    fn header_line(&self, base: Style) -> Line<'static> {
        let marker = if self.open { '▾' } else { '▸' };
        let msg_style = if self.is_streaming {
            base.add_modifier(Modifier::DIM | Modifier::ITALIC)
        } else {
            base
        };
        Line::from(vec![
            Span::raw("🧠 "),
            Span::styled(
                thinking_message(self.is_streaming, self.elapsed_secs),
                msg_style,
            ),
            Span::raw(format!(" {marker}")),
        ])
    }
}

impl Widget for Reasoning<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let base = self.style.patch(self.header_style);
        if let Some(header) = self.header_rect(area) {
            buf.set_style(header, self.style);
            self.header_line(base).render(header, buf);
        }
        if let Some(body) = self.body_rect(area) {
            buf.set_style(body, self.style);
            Markdown::new(self.text).style(self.style).render(body, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Position;

    fn row(buf: &Buffer, y: u16, w: u16) -> String {
        (0..w)
            .map(|x| buf.get(Position::new(x, y)).unwrap().symbol)
            .collect()
    }

    #[test]
    fn streaming_header_says_thinking() {
        assert_eq!(thinking_message(true, 0), "Thinking…");
        assert_eq!(thinking_message(true, 9), "Thinking…");
        // elapsed == 0 with no stream still reads as thinking (ai-elements).
        assert_eq!(thinking_message(false, 0), "Thinking…");
    }

    #[test]
    fn settled_header_reports_the_duration_singularised() {
        assert_eq!(thinking_message(false, 1), "Thought for 1 second");
        assert_eq!(thinking_message(false, 7), "Thought for 7 seconds");
    }

    #[test]
    fn the_header_is_always_drawn_with_a_marker() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 3));
        Reasoning::new("body")
            .elapsed_secs(4)
            .render(buf.area(), &mut buf);
        let header = row(&buf, 0, 30);
        assert!(header.contains("Thought for 4 seconds"), "{header:?}");
        assert!(header.contains('▸'), "collapsed marker: {header:?}");
    }

    #[test]
    fn collapsed_draws_no_body() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 4));
        Reasoning::new("secret reasoning").render(buf.area(), &mut buf);
        assert_eq!(Reasoning::new("x").body_rect(Rect::new(0, 0, 30, 4)), None);
        let mut text = String::new();
        for y in 1..4 {
            text.push_str(&row(&buf, y, 30));
        }
        assert!(!text.contains("secret"), "collapsed must hide the body");
    }

    #[test]
    fn open_renders_the_markdown_body() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 4));
        Reasoning::new("hello reasoning")
            .elapsed_secs(2)
            .open(true)
            .render(buf.area(), &mut buf);
        let header = row(&buf, 0, 30);
        assert!(header.contains('▾'), "open marker: {header:?}");
        let mut body = String::new();
        for y in 1..4 {
            body.push_str(&row(&buf, y, 30));
        }
        assert!(body.contains("hello reasoning"), "body was {body:?}");
    }

    #[test]
    fn body_rect_is_below_the_header_when_open() {
        let area = Rect::new(0, 0, 20, 5);
        let r = Reasoning::new("x").open(true);
        let h = r.header_rect(area).unwrap();
        let b = r.body_rect(area).unwrap();
        assert_eq!(h.height, 1);
        assert_eq!(b.top(), h.bottom());
        assert_eq!(b.height, 4);
    }

    #[test]
    fn a_one_row_area_open_has_a_header_but_no_body() {
        let area = Rect::new(0, 0, 20, 1);
        let r = Reasoning::new("x").open(true);
        assert!(r.header_rect(area).is_some());
        assert_eq!(r.body_rect(area), None);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
        Reasoning::new("x")
            .open(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        assert_eq!(Reasoning::new("x").header_rect(Rect::new(0, 0, 0, 0)), None);
    }
}
