//! [`AgentCard`] — an agent-definition card: the "this is the sub-agent I'm
//! delegating to" panel (name, model, instructions, its tools, an optional
//! output schema).
//!
//! # A pure projection, reusing [`Card`]/[`Badge`]
//!
//! The ai-elements `Agent` card shows an agent's definition — no interaction,
//! all the caller's data. So `AgentCard` owns nothing: it projects a
//! caller-owned [`AgentDef`].
//!
//! It is a framed [`Card`] (title = the agent name) with
//! the model as a [`Badge`] — *reusing* both — then the
//! instructions, a tools list (`name — description`), and, if present, the
//! output schema in a fenced code block.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area, no
//! tools, no schema, and long text are all safe clips — never a panic.

use rstui_core::{Buffer, Color, Modifier, Position, Rect, Style, Widget};
use rstui_widgets::{Badge, BadgeLevel, Block, Card};

/// One tool an agent can call (a name + a one-line description).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTool {
    /// The tool name.
    pub name: String,
    /// A one-line description.
    pub description: String,
}

impl AgentTool {
    /// A tool `name` with `description`.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }
}

/// The definition of an agent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentDef {
    /// The agent's name (the card title).
    pub name: String,
    /// The model it runs on (shown as a badge).
    pub model: String,
    /// Its system instructions.
    pub instructions: String,
    /// The tools it can call.
    pub tools: Vec<AgentTool>,
    /// An optional output-schema JSON string (shown fenced).
    pub output_schema: Option<String>,
}

impl AgentDef {
    /// An agent `name` on `model` with `instructions` (no tools/schema).
    pub fn new(
        name: impl Into<String>,
        model: impl Into<String>,
        instructions: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            model: model.into(),
            instructions: instructions.into(),
            tools: Vec::new(),
            output_schema: None,
        }
    }

    /// Sets the agent's callable tools.
    #[must_use]
    pub fn tools(mut self, tools: Vec<AgentTool>) -> Self {
        self.tools = tools;
        self
    }

    /// Sets the agent's output-schema JSON string.
    #[must_use]
    pub fn output_schema(mut self, schema: impl Into<String>) -> Self {
        self.output_schema = Some(schema.into());
        self
    }
}

/// An agent-definition card.
///
/// A framed [`Card`] titled with the agent name; the
/// body is a model [`Badge`], the instructions, a
/// `Tools:` list (`· name — description`), then (if present) the output
/// schema in a ```` ``` ```` fence. `AgentCard` owns no state — see the
/// [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::agent_card::{AgentCard, AgentDef, AgentTool};
///
/// let agent = AgentDef::new("Researcher", "gpt-5", "Find sources.")
///     .tools(vec![AgentTool::new("search", "web search")]);
/// let mut buf = Buffer::empty(Rect::new(0, 0, 30, 8));
/// AgentCard::new(&agent).render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌'); // framed
/// ```
#[derive(Debug, Clone)]
pub struct AgentCard<'a> {
    agent: &'a AgentDef,
    style: Style,
}

impl<'a> AgentCard<'a> {
    /// A card for `agent`.
    #[must_use]
    pub fn new(agent: &'a AgentDef) -> Self {
        Self {
            agent,
            style: Style::new(),
        }
    }

    /// Sets the base [`Style`] (the card frame/background).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The framing card, titled with the agent name.
    fn card(&self) -> Card<'a> {
        Card::new().block(
            Block::bordered()
                .style(self.style)
                .title(self.agent.name.clone()),
        )
    }

    /// The body lines (after the model badge row) — instructions, the tools
    /// list, then the fenced schema.
    fn body_lines(&self) -> Vec<(String, Style)> {
        let plain = self.style;
        let bold = self.style.add_modifier(Modifier::BOLD);
        let dim = self.style.add_modifier(Modifier::DIM);
        let mut out = vec![(self.agent.instructions.clone(), plain)];
        if !self.agent.tools.is_empty() {
            out.push(("Tools:".to_string(), bold));
            for tool in &self.agent.tools {
                out.push((format!("· {} — {}", tool.name, tool.description), plain));
            }
        }
        if let Some(schema) = &self.agent.output_schema {
            out.push(("```".to_string(), dim));
            for line in schema.lines() {
                out.push((line.to_string(), dim));
            }
            out.push(("```".to_string(), dim));
        }
        out
    }
}

impl Widget for AgentCard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let card = self.card();
        let body = card.inner(area);
        card.render(area, buf);
        if body.is_empty() {
            return;
        }

        // Row 0: the model badge.
        Badge::new(format!("model: {}", self.agent.model))
            .level(BadgeLevel::Info)
            .info_style(Style::new().fg(Color::Black).bg(Color::Cyan))
            .render(Rect::new(body.left(), body.top(), body.width, 1), buf);

        // Rows 1..: the body lines.
        for (n, (text, style)) in self.body_lines().iter().enumerate() {
            let row = 1u16.saturating_add(n as u16);
            if row >= body.height {
                break;
            }
            let y = body.top().saturating_add(row);
            let mut x = body.left();
            for ch in text.chars() {
                if x >= body.right() {
                    break;
                }
                buf.set_cell(Position::new(x, y), ch, *style);
                x = x.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> AgentDef {
        AgentDef::new("Researcher", "gpt-5", "Find good sources.")
            .tools(vec![
                AgentTool::new("search", "web search"),
                AgentTool::new("fetch", "read a page"),
            ])
            .output_schema("{\n  \"answer\": \"string\"\n}")
    }

    fn lines(widget: AgentCard<'_>, w: u16, h: u16) -> String {
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
    fn the_card_is_titled_with_the_agent_name() {
        let a = agent();
        let out = lines(AgentCard::new(&a), 30, 10);
        assert!(out.contains("Researcher"), "got {out:?}");
    }

    #[test]
    fn the_body_is_badge_instructions_tools_then_schema() {
        let a = agent();
        let out = lines(AgentCard::new(&a), 30, 12);
        assert!(out.contains("model: gpt-5"), "got {out:?}");
        assert!(out.contains("Find good sources."), "got {out:?}");
        assert!(out.contains("Tools:"), "got {out:?}");
        assert!(out.contains("· search — web search"), "got {out:?}");
        assert!(out.contains("· fetch — read a page"), "got {out:?}");
        assert!(out.contains("```"), "got {out:?}");
        assert!(out.contains("\"answer\": \"string\""), "got {out:?}");
    }

    #[test]
    fn no_tools_and_no_schema_is_safe() {
        let a = AgentDef::new("Bare", "m", "do it");
        let out = lines(AgentCard::new(&a), 20, 6);
        assert!(out.contains("model: m"), "got {out:?}");
        assert!(out.contains("do it"), "got {out:?}");
        assert!(!out.contains("Tools:"), "got {out:?}");
    }

    #[test]
    fn a_tiny_card_clips_without_panicking() {
        let a = agent();
        let out = lines(AgentCard::new(&a), 6, 2);
        assert!(out.starts_with('┌'), "got {out:?}");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let a = agent();
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        AgentCard::new(&a).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
