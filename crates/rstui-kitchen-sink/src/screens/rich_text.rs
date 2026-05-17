//! The Rich Text screen: a [`Tabs`] strip over a scrollable [`Paragraph`], a
//! scrollable [`Markdown`] document (whose `[text](href)` exercises
//! [`Link`](rstui_widgets::Link)), a [`Mermaid`] flowchart, and a styled
//! [`Span`]/[`Line`] sampler — with a persistent [`Kbd`] strip. `←/→`
//! switches tabs, `↑/↓` scrolls.

use rstui_core::{
    Color, Constraint, KeyCode, Layout, Line, Modifier, Position, Rect, Span, Style,
    stylize::Stylize,
};
use rstui_runtime::Frame;
use rstui_widgets::{Block, BorderType, Kbd, Markdown, Mermaid, Paragraph, Tabs, Wrap};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The four sub-views.
const TABS: [&str; 4] = ["Paragraph", "Markdown", "Mermaid", "Spans"];

/// The wrapped-paragraph body.
const PROSE: &str = "rstui renders styled text through a three-level model: a Span is one run with one Style, a Line is a row of Spans with an optional Alignment, and a Text is a stack of Lines. Widgets never retain this — they project it into the cell Buffer every frame. This Paragraph turns on trimming soft word wrap, so the prose reflows to whatever width the panel currently has; scroll it with the arrow keys or the mouse wheel and the offset is plain caller-owned state the widget only reads. Resize the terminal and the wrap recomputes deterministically with no float math. The same Buffer-stamping contract is everything a third-party widget needs.";

/// The Markdown document (its link exercises the Link type).
const DOC: &str = "\
# Markdown

A hand-written CommonMark-ish renderer — **bold**, *italic*, `code`.

- pure projection, no retained tree
- headings, lists, rules, code blocks
- links like [the rstui repo](https://github.com/andymac4182/rstui)

> Blockquotes and rules render too.

---

```
fn render(area: Rect, buf: &mut Buffer) { /* … */ }
```

Scroll with the arrows / wheel.";

/// The Mermaid flowchart source.
const GRAPH: &str = "\
graph TD
A[Event] --> B{on_event}
B -->|Some msg| C[update]
B -->|None| A
C --> D[view]
D --> A";

/// The active tab and the document scroll offset.
#[derive(Debug)]
pub(crate) struct State {
    tab: usize,
    scroll: u16,
}

impl State {
    /// Paragraph tab, scrolled to the top.
    pub(crate) fn new() -> Self {
        Self { tab: 0, scroll: 0 }
    }

    /// `←/→` switch tabs (resetting scroll), `↑/↓` scroll the document.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Left => {
                if self.tab == 0 {
                    return ScreenOutcome::ignored();
                }
                self.tab -= 1;
                self.scroll = 0;
            }
            KeyCode::Right => {
                self.tab = (self.tab + 1).min(TABS.len() - 1);
                self.scroll = 0;
            }
            KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down => self.scroll = (self.scroll + 1).min(60),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// A click on the tab strip switches sub-view.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [tabs, _body, _foot] = Self::rows(content);
        if tabs.contains(pos) {
            let w = (tabs.width.max(1) / TABS.len() as u16).max(1);
            self.tab = (((pos.x - tabs.x) / w) as usize).min(TABS.len() - 1);
            self.scroll = 0;
            return ScreenOutcome::consumed();
        }
        ScreenOutcome::ignored()
    }

    /// Wheel scroll moves the document.
    pub(crate) fn on_scroll(&mut self, up: bool) {
        if up {
            self.scroll = self.scroll.saturating_sub(2);
        } else {
            self.scroll = (self.scroll + 2).min(60);
        }
    }

    /// The three stacked bands shared by the renderer and the hit-test.
    fn rows(area: Rect) -> [Rect; 3] {
        Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(2),
        ])
        .areas(area)
    }

    /// Draw the rich-text screen.
    pub(crate) fn view(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let [tabs, body, foot] = Self::rows(area);

        frame.render_widget(
            Tabs::new(TABS)
                .selected(Some(self.tab))
                .divider("  ")
                .style(theme.body())
                .highlight_style(theme.selection()),
            tabs,
        );

        match self.tab {
            0 => frame.render_widget(
                Paragraph::new(PROSE)
                    .wrap(Wrap { trim: true })
                    .scroll(Position::new(0, self.scroll))
                    .style(theme.body())
                    .block(framed(theme, "Paragraph · ↑↓ scroll")),
                body,
            ),
            1 => frame.render_widget(
                Markdown::new(DOC)
                    .scroll(self.scroll)
                    .style(theme.body())
                    .block(framed(theme, "Markdown · links + ↑↓ scroll")),
                body,
            ),
            2 => frame.render_widget(
                Mermaid::new(GRAPH)
                    .style(theme.body())
                    .block(framed(theme, "Mermaid · the rstui event loop")),
                body,
            ),
            _ => self.view_spans(theme, frame, body),
        }

        // Persistent Kbd strip.
        frame.render_widget(
            Kbd::new(["←", "→", "↑", "↓", "click a tab"])
                .style(theme.body())
                .key_style(Style::new().fg(theme.base).bg(theme.accent))
                .separator_style(Style::new().fg(theme.dim)),
            Rect::new(foot.x, foot.y + 1, foot.width, 1),
        );
    }

    /// The styled-text sampler: every Span/Line capability at once.
    fn view_spans(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let lines = vec![
            Line::from(vec![
                Span::raw("Span runs: "),
                "red ".red(),
                "green ".green(),
                "blue ".blue(),
                "on-accent".fg(theme.base).bg(theme.accent),
            ]),
            Line::from(vec![
                Span::raw("Modifiers: "),
                "bold ".bold(),
                "italic ".italic(),
                "underline ".underlined(),
                "reversed ".reversed(),
                "dim".add_modifier(Modifier::DIM),
            ]),
            Line::from(vec![
                Span::raw("24-bit RGB: "),
                Span::styled("■", Style::new().fg(Color::Rgb(255, 90, 95))),
                Span::styled("■", Style::new().fg(Color::Rgb(255, 170, 60))),
                Span::styled("■", Style::new().fg(Color::Rgb(120, 200, 80))),
                Span::styled("■", Style::new().fg(Color::Rgb(80, 170, 255))),
                Span::styled(" gradient-ready", Style::new().fg(theme.accent_alt)),
            ]),
            Line::from("Alignment: this line is centred")
                .style(theme.body())
                .centered(),
            Line::from("…and this one right-aligned")
                .style(theme.caption())
                .right_aligned(),
            Line::from(vec![
                Span::raw("Links live in Markdown/Mermaid — see the "),
                "Markdown".fg(theme.accent).underlined(),
                Span::raw(" tab’s "),
                "[the rstui repo]".fg(theme.accent),
                Span::raw(" anchor."),
            ]),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: true })
                .style(theme.body())
                .block(framed(theme, "Span / Line / Text sampler")),
            area,
        );
    }
}

/// A plain rounded framing block.
fn framed(theme: &Theme, title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {title} ")).style(theme.caption()))
        .border_style(theme.border())
        .style(theme.body())
}
