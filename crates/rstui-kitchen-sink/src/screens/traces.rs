//! A distributed-trace explorer: one trace's spans as a [`TraceWaterfall`] on
//! a shared time axis, toggleable to a [`FlameGraph`] (the same flattened
//! frame list), with a [`Table`] of the selected span's attributes.
//! `↑/↓` (or the wheel) selects a span, `f` toggles waterfall/flame,
//! `Enter` copies the span id.

use rstui_core::{Constraint, KeyCode, Layout, Line, Position, Rect, Style};
use rstui_runtime::Frame;
use rstui_widgets::{
    Block, BorderType, FlameFrame, FlameGraph, Row, Table, TraceSpan, TraceWaterfall,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// One span of the synthetic checkout trace: depth, start, duration (µs in
/// this demo's opaque units), operation name, and service tag.
struct Span {
    /// The stack depth (0 = the root server span).
    depth: u16,
    /// The offset on the shared `[0, total]` time axis.
    start: u64,
    /// The span length in the same axis units.
    duration: u64,
    /// The operation name.
    name: &'static str,
    /// The owning service.
    service: &'static str,
}

/// The trace, flattened in display order (the reducer owns this shape; both
/// widgets only read it — the [`Tree`](rstui_widgets::Tree) discipline).
const SPANS: [Span; 8] = [
    Span {
        depth: 0,
        start: 0,
        duration: 1000,
        name: "GET /checkout",
        service: "edge",
    },
    Span {
        depth: 1,
        start: 20,
        duration: 120,
        name: "auth.verify",
        service: "auth",
    },
    Span {
        depth: 1,
        start: 150,
        duration: 620,
        name: "checkout.create",
        service: "checkout",
    },
    Span {
        depth: 2,
        start: 170,
        duration: 200,
        name: "inventory.reserve",
        service: "inventory",
    },
    Span {
        depth: 2,
        start: 380,
        duration: 280,
        name: "payment.charge",
        service: "payment",
    },
    Span {
        depth: 3,
        start: 400,
        duration: 200,
        name: "POST stripe.com/charges",
        service: "stripe",
    },
    Span {
        depth: 1,
        start: 780,
        duration: 200,
        name: "ledger.write",
        service: "ledger",
    },
    Span {
        depth: 2,
        start: 800,
        duration: 140,
        name: "db.insert orders",
        service: "postgres",
    },
];

/// The trace explorer's caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    /// The selected span index into [`SPANS`].
    selected: usize,
    /// `true` shows the flame graph; `false` the waterfall.
    flame: bool,
}

impl State {
    /// The root span selected, waterfall view.
    pub(crate) fn new() -> Self {
        Self {
            selected: 0,
            flame: false,
        }
    }

    /// The colour a service's bars/frames are drawn in.
    fn service_color(theme: &Theme, service: &str) -> rstui_core::Color {
        match service {
            "edge" => theme.accent,
            "auth" => theme.accent_alt,
            "checkout" => theme.ok,
            "inventory" => theme.warn,
            "payment" => theme.err,
            "stripe" => theme.info,
            "ledger" => theme.accent_alt,
            _ => theme.dim,
        }
    }

    /// `↑/↓` select a span, `f` toggles view, `Enter` copies the span id.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(SPANS.len() - 1),
            KeyCode::Char('f') => self.flame = !self.flame,
            KeyCode::Enter | KeyCode::Char(' ') => {
                return ScreenOutcome::with_toast(
                    crate::screens::ToastLevel::Success,
                    format!(
                        "Copied span id 7f3a…{:02x} ({})",
                        self.selected, SPANS[self.selected].name
                    ),
                );
            }
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Wheel scroll moves the span selection.
    pub(crate) fn on_scroll(&mut self, up: bool) {
        if up {
            self.selected = self.selected.saturating_sub(1);
        } else {
            self.selected = (self.selected + 1).min(SPANS.len() - 1);
        }
    }

    /// Click a span row in the waterfall to select it.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        if self.flame {
            return ScreenOutcome::ignored();
        }
        let [_, body, _] = Self::rows(content);
        let inner = crate::screens::block_inner(body);
        if inner.contains(pos) {
            let row = pos.y.saturating_sub(inner.y) as usize;
            if row < SPANS.len() {
                self.selected = row;
                return ScreenOutcome::with_toast(
                    crate::screens::ToastLevel::Info,
                    format!("Selected {}", SPANS[row].name),
                );
            }
        }
        ScreenOutcome::ignored()
    }

    /// The header / body / attributes split.
    fn rows(area: Rect) -> [Rect; 3] {
        Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(8),
        ])
        .areas(area)
    }

    /// Draw the trace explorer. `tick` is unused here — a trace is a fixed
    /// captured artifact, not a live series.
    pub(crate) fn view(&self, theme: &Theme, _tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [head, body, attrs] = Self::rows(area);
        let total: u64 = SPANS
            .iter()
            .map(|s| s.start + s.duration)
            .max()
            .unwrap_or(1);

        // Header: trace id + total duration + the view toggle hint.
        frame.render_widget(
            Line::from(vec![
                rstui_core::Span::styled(" trace ", theme.caption()),
                rstui_core::Span::styled("7f3a91c2e4d6b8a0", theme.accent_text()),
                rstui_core::Span::styled(
                    format!("  ·  {total} µs  ·  {} spans", SPANS.len()),
                    theme.caption(),
                ),
                rstui_core::Span::styled(
                    if self.flame {
                        "   [f] → waterfall"
                    } else {
                        "   [f] → flame"
                    },
                    theme.caption(),
                ),
            ]),
            head,
        );

        let title = if self.flame {
            "Flame graph"
        } else {
            "Span waterfall"
        };
        let block = panel(theme, title);
        let bin = block.inner(body);
        frame.render_widget(block, body);

        if self.flame {
            let frames: Vec<FlameFrame> = SPANS
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let col = Self::service_color(theme, s.service);
                    let st = if i == self.selected {
                        Style::new().fg(theme.base).bg(theme.accent)
                    } else {
                        Style::new().fg(theme.base).bg(col)
                    };
                    FlameFrame::new(s.depth, s.start, s.duration, s.name).style(st)
                })
                .collect();
            frame.render_widget(
                FlameGraph::new(&frames)
                    .total(Some(total))
                    .selected(Some(self.selected))
                    .selected_style(theme.selection())
                    .style(theme.body()),
                bin,
            );
        } else {
            let spans: Vec<TraceSpan> = SPANS
                .iter()
                .map(|s| {
                    let col = Self::service_color(theme, s.service);
                    let indent = "  ".repeat(s.depth as usize);
                    TraceSpan::new(s.depth, s.start, s.duration, format!("{indent}{}", s.name))
                        .style(Style::new().fg(col))
                })
                .collect();
            frame.render_widget(
                TraceWaterfall::new(&spans)
                    .total(Some(total))
                    .name_width(26)
                    .selected(Some(self.selected))
                    .selected_style(theme.selection())
                    .name_style(theme.body())
                    .bar_style(Style::new().fg(theme.accent))
                    .style(theme.body()),
                bin,
            );
        }

        // Selected-span attribute table.
        let s = &SPANS[self.selected];
        let pct = (s.duration * 100).checked_div(total).unwrap_or(0);
        let ablock = panel(theme, &format!("Span · {}", s.name));
        let ain = ablock.inner(attrs);
        frame.render_widget(ablock, attrs);
        let rows = [
            ("service.name", s.service.to_string()),
            (
                "span.kind",
                if s.depth == 0 {
                    "SERVER".into()
                } else {
                    "CLIENT".into()
                },
            ),
            ("duration", format!("{} µs ({pct}% of trace)", s.duration)),
            ("start.offset", format!("{} µs", s.start)),
            ("status.code", "OK".to_string()),
        ];
        frame.render_widget(
            Table::new(
                rows.iter().map(|(k, v)| Row::new([*k, v.as_str()])),
                [Constraint::Length(16), Constraint::Fill(1)],
            )
            .header(Row::new(["attribute", "value"]).style(theme.accent_text()))
            .style(theme.body())
            .column_spacing(2),
            ain,
        );
    }
}

/// A rounded display panel.
fn panel(theme: &Theme, title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {title} ")).style(theme.caption()))
        .border_style(theme.border())
        .style(theme.body())
}
