//! [`Tool`] — the keystone AI-elements widget: a collapsible card that
//! projects one [`ToolUiPart`] (a single tool call within an assistant
//! turn).
//!
//! # A pure projection of a caller-owned [`ToolUiPart`] + a [`ToolState`]
//!
//! This is the rstui translation of the ai-elements `Tool` /
//! `ToolHeader` / `ToolInput` / `ToolOutput` family (`tool.tsx`). There a
//! `Collapsible` owns its open state and the header derives a status
//! `Badge` from the seven-state [`ToolState`]; here the part is
//! caller-owned model data ([`crate::model::ToolUiPart`], parsed from the
//! wire by [`crate::model`]) and *which open* is a caller-owned `bool`,
//! exactly as [`Accordion`](rstui_widgets::Accordion) takes a caller-owned
//! `expanded` (ADR 0012 §P1). The widget only ever *reads* both — it fits
//! `App::view(&self)` and is deterministically headless-testable, and it
//! never mutates anything at render time.
//!
//! # What it draws
//!
//! A bordered frame whose **header row** is a wrench glyph, the tool name
//! (or a caller override), and a status tag whose accent is mapped from
//! [`ToolState`] via [`tool_state_level`] (running/awaiting →
//! neutral/info, completed → success, denied → warning, errored →
//! error) — the same level→accent mapping a standalone
//! [`Badge`](rstui_widgets::Badge) uses, so a theme reads consistently.
//! When open, the **body** renders the `input` pretty-printed as JSON
//! ([`serde_json::to_string_pretty`]) under a `PARAMETERS` caption, then
//! the `output` (an object pretty-printed as JSON, a string verbatim) or
//! the `error_text` (error-styled) under a `RESULT` / `ERROR` caption.
//!
//! Per the ai-elements `statusIcons` (a pulsing clock while
//! `input-available`) the header carries a caller-owned-tick
//! [`Spinner`] glyph **while [`ToolState::is_terminal`] is false** — the
//! [`Spinner`] caller-owned-`tick` precedent (no wall clock smuggled into
//! a pure `view`; the reducer advances the tick from a timer `Cmd`).
//!
//! # The collapse seam (mirrors [`Accordion`](rstui_widgets::Accordion))
//!
//! [`Tool::header_rect`] and [`Tool::body_rect`] are pure geometry
//! accessors: the reducer hit-tests a click against `header_rect` and
//! flips its caller-owned open `bool` in `update`; the auto-open driven
//! by [`ToolState`] (open while running, collapse when terminal) is the
//! reducer's policy, *documented here but not enforced by the widget*
//! (the same split [`Accordion`](rstui_widgets::Accordion) records for its
//! `expanded`). `body_rect` is `None` when collapsed or there is no room.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) rule a pure projection is
//! *total*: an empty area, a zero-size area, a header wider than the area,
//! a missing `input`/`output`, and a body taller than the card are all
//! safe clips/no-ops — never a panic.

use rstui_core::{Buffer, Color, Line, Modifier, Rect, Span, Style, Widget};
use rstui_widgets::{BadgeLevel, Block, Borders, Markdown, Spinner};

use crate::model::{ToolState, ToolUiPart};

/// The status [`BadgeLevel`] ai-elements paints for a [`ToolState`].
///
/// Mirrors the `tool.tsx` `statusIcons` accents: pending is neutral,
/// running/awaiting-approval are informational, a responded/completed
/// call is a success, a denied one a (non-fatal) warning, and an errored
/// one an error.
#[must_use]
pub fn tool_state_level(state: ToolState) -> BadgeLevel {
    match state {
        ToolState::InputStreaming => BadgeLevel::Neutral,
        ToolState::InputAvailable | ToolState::ApprovalRequested => BadgeLevel::Info,
        ToolState::ApprovalResponded | ToolState::OutputAvailable => BadgeLevel::Success,
        ToolState::OutputDenied => BadgeLevel::Warning,
        ToolState::OutputError => BadgeLevel::Error,
    }
}

/// The accent foreground for a [`BadgeLevel`], used by the inline status
/// tag in a [`Tool`] header (a [`Badge`](rstui_widgets::Badge) is the
/// standalone pill; the header tag reuses the same level→color mapping so
/// a theme reads consistently).
#[must_use]
pub fn level_color(level: BadgeLevel) -> Color {
    match level {
        BadgeLevel::Neutral => Color::Gray,
        BadgeLevel::Info => Color::Cyan,
        BadgeLevel::Success => Color::Green,
        BadgeLevel::Warning => Color::Yellow,
        BadgeLevel::Error => Color::Red,
    }
}

/// A collapsible tool-call card, a pure projection of a borrowed
/// [`ToolUiPart`] plus a caller-owned [`open`](Self::open) `bool` and a
/// caller-owned [`tick`](Self::tick) for the running spinner.
///
/// The header is always drawn (a 1-row bar inside the frame); the body —
/// the JSON `input` and the `output`/`error_text` — is drawn only when
/// [`open`](Self::open). Styling cascades base → caption/error styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_ai::model::{ToolState, ToolUiPart};
/// use rstui_ai::tool::Tool;
/// use serde_json::json;
///
/// let part = ToolUiPart {
///     tool_name: "search".into(),
///     tool_call_id: "t1".into(),
///     state: ToolState::OutputAvailable,
///     input: Some(json!({ "q": "rust" })),
///     output: Some(json!("ok")),
///     error_text: None,
/// };
/// // `open` is caller state — the reducer flips it on a header click.
/// let tool = Tool::new(&part).open(true);
/// let mut buf = Buffer::empty(Rect::new(0, 0, 30, 8));
/// tool.render(buf.area(), &mut buf);
/// ```
#[derive(Debug, Clone)]
pub struct Tool<'a> {
    part: &'a ToolUiPart,
    title: Option<&'a str>,
    open: bool,
    tick: u64,
    style: Style,
    caption_style: Style,
    error_style: Style,
}

impl<'a> Tool<'a> {
    /// A collapsed card projecting `part`, no title override, tick `0`,
    /// otherwise unstyled (a dim caption, a red error body by default).
    #[must_use]
    pub fn new(part: &'a ToolUiPart) -> Self {
        Self {
            part,
            title: None,
            open: false,
            tick: 0,
            style: Style::new(),
            caption_style: Style::new().fg(Color::DarkGray),
            error_style: Style::new().fg(Color::Red),
        }
    }

    /// Overrides the header label (ai-elements `ToolHeader title`); the
    /// default is the part's [`tool_name`](ToolUiPart::tool_name).
    #[must_use]
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Sets whether the card is expanded — caller-owned state the reducer
    /// flips on a [`header_rect`](Self::header_rect) click; the widget only
    /// reads it (see the [module docs](self)).
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the caller-owned spinner tick, shown in the header **while the
    /// call is not [terminal](ToolState::is_terminal)** (the reducer
    /// advances it from a timer `Cmd`; the widget never animates itself).
    #[must_use]
    pub fn tick(mut self, tick: u64) -> Self {
        self.tick = tick;
        self
    }

    /// Sets the base [`Style`] (also fills the card region).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] of the `PARAMETERS`/`RESULT`/`ERROR` captions
    /// (default a dim foreground).
    #[must_use]
    pub fn caption_style(mut self, style: Style) -> Self {
        self.caption_style = style;
        self
    }

    /// Sets the [`Style`] the `error_text` body is drawn with (default a
    /// red foreground).
    #[must_use]
    pub fn error_style(mut self, style: Style) -> Self {
        self.error_style = style;
        self
    }

    /// The framing [`Block`] — the single definition
    /// [`header_rect`](Self::header_rect) /
    /// [`body_rect`](Self::body_rect) / [`render`](Widget::render) all use
    /// so they never disagree.
    fn frame() -> Block<'static> {
        Block::new().borders(Borders::ALL)
    }

    /// The header row rect (the wrench + name + status tag), or `None`
    /// when the area has no room for a framed 1-row header.
    ///
    /// A pure function of `area` — the reducer hit-tests a click against
    /// it and flips the caller-owned open `bool`, exactly as with
    /// [`Accordion::layout`](rstui_widgets::Accordion::layout).
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

    /// The body rect (the JSON `input` + the `output`/`error_text`), or
    /// `None` when collapsed, the area is empty, or there is no row below
    /// the header.
    ///
    /// A pure function of `area` and the caller-owned [`open`](Self::open)
    /// — render nothing into a `None`, exactly the
    /// [`Accordion::layout`](rstui_widgets::Accordion::layout) contract.
    #[must_use]
    pub fn body_rect(&self, area: Rect) -> Option<Rect> {
        if !self.open {
            return None;
        }
        let header = self.header_rect(area)?;
        let inner = Self::frame().inner(area);
        let body_top = header.bottom();
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

    /// The header [`Line`]: a wrench glyph, the name (title override or
    /// [`tool_name`](ToolUiPart::tool_name)), then a spinner glyph (while
    /// non-terminal) and the `[state-label]` status tag.
    fn header_line(&self, base: Style) -> Line<'static> {
        let name = self.title.unwrap_or(self.part.tool_name.as_str());
        let mut spans = vec![
            Span::styled("🔧 ", base.patch(self.caption_style)),
            Span::styled(name.to_owned(), base.add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ];
        if !self.part.state.is_terminal() {
            // The ai-elements pulsing-clock equivalent: a caller-tick
            // spinner glyph while the call is still in flight.
            if let Some(glyph) = Spinner::new().tick(self.tick as usize).glyph() {
                spans.push(Span::styled(
                    format!("{glyph} "),
                    base.patch(self.caption_style),
                ));
            }
        }
        let accent = Style::new()
            .fg(level_color(tool_state_level(self.part.state)))
            .add_modifier(Modifier::BOLD);
        spans.push(Span::styled(
            format!("[{}]", self.part.state.label()),
            base.patch(accent),
        ));
        Line::from(spans)
    }
}

/// Pretty-prints a [`serde_json::Value`] (the ai-elements
/// `JSON.stringify(x, null, 2)`); a string value is returned verbatim
/// (ai-elements renders a string `output` as a plain code block), and a
/// serialization failure degrades to the value's `Display` (totality —
/// never an error path in a pure `view`).
fn pretty_json(value: &serde_json::Value) -> String {
    if let serde_json::Value::String(text) = value {
        return text.clone();
    }
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

impl Widget for Tool<'_> {
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

        // The header bar always draws (the collapsed affordance).
        if let Some(header) = self.header_rect(area) {
            buf.set_style(header, base);
            self.header_line(base).render(header, buf);
        }

        // The body (caption + JSON input, then output/error) only when open.
        let Some(body) = self.body_rect(area) else {
            return;
        };
        buf.set_style(body, base);

        // Build the body as a small markdown document of captioned code
        // blocks; Markdown is the house renderer (do not reinvent it) and
        // gives wrapping + a code-fence background for free.
        let mut doc = String::new();
        if let Some(input) = &self.part.input {
            doc.push_str("PARAMETERS\n\n```json\n");
            doc.push_str(&pretty_json(input));
            doc.push_str("\n```\n");
        }
        match (&self.part.output, &self.part.error_text) {
            (_, Some(err)) => {
                doc.push_str("\nERROR\n\n");
                doc.push_str(err);
                doc.push('\n');
            }
            (Some(out), None) => {
                doc.push_str("\nRESULT\n\n```json\n");
                doc.push_str(&pretty_json(out));
                doc.push_str("\n```\n");
            }
            (None, None) => {}
        }

        if doc.is_empty() {
            return;
        }
        // The error body is tinted; otherwise the base. (Markdown styles
        // code fences itself; the captions ride the body base.)
        let body_base = if self.part.error_text.is_some() {
            base.patch(self.error_style)
        } else {
            base
        };
        Markdown::new(doc).style(body_base).render(body, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Position;
    use serde_json::json;

    fn part(state: ToolState) -> ToolUiPart {
        ToolUiPart {
            tool_name: "search".into(),
            tool_call_id: "t1".into(),
            state,
            input: Some(json!({ "q": "rust" })),
            output: Some(json!("done")),
            error_text: None,
        }
    }

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

    #[test]
    fn the_header_shows_the_name_and_a_status_tag() {
        let p = part(ToolState::OutputAvailable);
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 3));
        Tool::new(&p).render(buf.area(), &mut buf);
        // Row 0 is the top border; row 1 is the header inside it.
        let header = row(&buf, 1, 30);
        assert!(header.contains("search"), "header was {header:?}");
        assert!(header.contains("[Completed]"), "header was {header:?}");
    }

    #[test]
    fn collapsed_draws_only_the_header_no_body() {
        let p = part(ToolState::OutputAvailable);
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 8));
        Tool::new(&p).render(buf.area(), &mut buf);
        assert_eq!(Tool::new(&p).body_rect(Rect::new(0, 0, 30, 8)), None);
        assert!(
            !dump(&buf, 30, 8).contains("PARAMETERS"),
            "a collapsed card must not draw the body"
        );
    }

    #[test]
    fn open_draws_the_json_input_and_result() {
        let p = part(ToolState::OutputAvailable);
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 14));
        Tool::new(&p).open(true).render(buf.area(), &mut buf);
        let text = dump(&buf, 30, 14);
        assert!(text.contains("PARAMETERS"), "{text}");
        assert!(text.contains("\"q\""), "the pretty JSON input: {text}");
        assert!(text.contains("RESULT"), "{text}");
        assert!(text.contains("done"), "the string output verbatim: {text}");
    }

    #[test]
    fn an_error_state_renders_the_error_text_under_an_error_caption() {
        let mut p = part(ToolState::OutputError);
        p.output = None;
        p.error_text = Some("boom".into());
        // Tall enough to clear the header + the JSON input block so the
        // error caption + text are within the rendered region.
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 16));
        Tool::new(&p).open(true).render(buf.area(), &mut buf);
        let text = dump(&buf, 30, 16);
        assert!(text.contains("[Error]"), "{text}");
        assert!(text.contains("ERROR"), "{text}");
        assert!(text.contains("boom"), "{text}");
    }

    #[test]
    fn a_non_terminal_state_puts_a_spinner_glyph_in_the_header() {
        let p = part(ToolState::InputAvailable);
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 3));
        Tool::new(&p).tick(0).render(buf.area(), &mut buf);
        let header = row(&buf, 1, 40);
        // The first BRAILLE frame is '⠋'; non-terminal => it is present.
        assert!(header.contains('⠋'), "header was {header:?}");
        assert!(header.contains("[Running]"), "header was {header:?}");
    }

    #[test]
    fn a_terminal_state_has_no_spinner_glyph() {
        let p = part(ToolState::OutputAvailable);
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 3));
        Tool::new(&p).render(buf.area(), &mut buf);
        let header = row(&buf, 1, 40);
        assert!(!header.contains('⠋'), "terminal => no spinner: {header:?}");
    }

    #[test]
    fn the_state_level_mapping_is_the_ai_elements_one() {
        assert_eq!(
            tool_state_level(ToolState::InputStreaming),
            BadgeLevel::Neutral
        );
        assert_eq!(
            tool_state_level(ToolState::InputAvailable),
            BadgeLevel::Info
        );
        assert_eq!(
            tool_state_level(ToolState::OutputAvailable),
            BadgeLevel::Success
        );
        assert_eq!(
            tool_state_level(ToolState::OutputDenied),
            BadgeLevel::Warning
        );
        assert_eq!(tool_state_level(ToolState::OutputError), BadgeLevel::Error);
    }

    #[test]
    fn header_and_body_rects_are_consistent_with_the_frame() {
        let p = part(ToolState::OutputAvailable);
        let area = Rect::new(0, 0, 30, 8);
        let tool = Tool::new(&p).open(true);
        let header = tool.header_rect(area).unwrap();
        let body = tool.body_rect(area).unwrap();
        assert_eq!(header.height, 1);
        assert_eq!(body.top(), header.bottom());
        assert_eq!(body.left(), header.left());
        assert!(body.left() >= 1, "body is inside the left border");
    }

    #[test]
    fn a_title_override_replaces_the_tool_name() {
        let p = part(ToolState::OutputAvailable);
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 3));
        Tool::new(&p)
            .title("Web Search")
            .render(buf.area(), &mut buf);
        let header = row(&buf, 1, 30);
        assert!(header.contains("Web Search"), "header was {header:?}");
        assert!(!header.contains("search "), "name overridden: {header:?}");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let p = part(ToolState::OutputAvailable);
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 8));
        Tool::new(&p)
            .open(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        assert_eq!(Tool::new(&p).header_rect(Rect::new(0, 0, 0, 0)), None);
        assert_eq!(Tool::new(&p).body_rect(Rect::new(0, 0, 0, 0)), None);
    }

    #[test]
    fn a_tiny_area_with_no_inner_is_total() {
        let p = part(ToolState::OutputAvailable);
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        // 2x2: the border consumes everything, no inner — must not panic.
        Tool::new(&p).open(true).render(buf.area(), &mut buf);
        assert_eq!(Tool::new(&p).header_rect(Rect::new(0, 0, 2, 2)), None);
    }

    #[test]
    fn a_string_output_is_verbatim_and_an_object_is_pretty_json() {
        assert_eq!(pretty_json(&json!("plain")), "plain");
        let obj = pretty_json(&json!({ "a": 1 }));
        assert!(obj.contains("\"a\": 1"), "pretty object was {obj:?}");
    }
}
