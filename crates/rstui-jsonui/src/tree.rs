//! [`UiNode`] — the one projection target both formats compile to, plus
//! the immediate-mode `render` walker and the `hit` accessor.
//!
//! # Resolved, concrete, re-projected every frame
//!
//! A `UiNode` carries **only resolved values** — the format layer
//! ([`a2ui`](crate::a2ui) / [`jsonrender`](crate::jsonrender)) has
//! already evaluated every binding/expression against the caller-owned
//! [`DataModel`](crate::value::DataModel). There is no retained tree
//! (ADR 0012): the parsed document is re-projected to a fresh `UiNode`
//! and re-walked each frame, so an agent UI is just more caller-owned
//! state in the pure-projection model.
//!
//! Interaction is **not** a callback (ADR 0012 §P1). [`render`](UiNode::render)
//! records every interactive node's screen [`Rect`] into a caller-held
//! [`HitMap`]; the next frame, a click is resolved with
//! [`HitMap::at`] to a [`NodeId`], which the format layer turns back into
//! the agent's action/event JSON. Every node clips or no-ops on a tiny,
//! zero, or oversized area — never a panic (the rstui totality rule).

use rstui_core::{
    Alignment, Buffer, Color, Constraint, Direction, Layout, Line, Modifier, Position, Rect, Span,
    Style, Widget,
};
use rstui_widgets::{
    Badge, BadgeLevel, Block, Borders, Button, Gauge, Markdown, Paragraph, Spinner, Wrap,
};

/// The format-assigned identity of a node (A2UI component `id` /
/// json-render element key), used for hit-testing and action routing.
pub type NodeId = String;

/// Main-axis distribution of a [`UiNode::Row`]/[`UiNode::Column`] (the
/// union of the A2UI `justify` and json-render `justifyContent` enums).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Justify {
    /// Pack children at the start (the default).
    #[default]
    Start,
    /// Centre the run.
    Center,
    /// Pack children at the end.
    End,
    /// Equal space between children.
    SpaceBetween,
    /// Equal space around children.
    SpaceAround,
    /// Stretch children to fill.
    Stretch,
}

/// Cross-axis alignment of a container's children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossAlign {
    /// Align to the cross-axis start.
    Start,
    /// Centre on the cross axis.
    Center,
    /// Align to the cross-axis end.
    End,
    /// Stretch to the cross-axis extent (the default).
    #[default]
    Stretch,
}

/// The semantic level of a [`UiNode::Text`] (A2UI `Text.variant` /
/// json-render `Heading.level`), mapped to a terminal text style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextVariant {
    /// Largest heading.
    H1,
    /// Section heading.
    H2,
    /// Sub-heading.
    H3,
    /// Minor heading.
    H4,
    /// Body text (the default).
    #[default]
    Body,
    /// De-emphasised caption.
    Caption,
}

impl TextVariant {
    /// The terminal [`Style`] this variant renders with.
    #[must_use]
    pub fn style(self) -> Style {
        match self {
            Self::H1 => Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            Self::H2 => Style::new().add_modifier(Modifier::BOLD),
            Self::H3 => Style::new().add_modifier(Modifier::BOLD).fg(Color::Cyan),
            Self::H4 => Style::new().add_modifier(Modifier::BOLD | Modifier::DIM),
            Self::Body => Style::new(),
            Self::Caption => Style::new().add_modifier(Modifier::DIM),
        }
    }
}

/// A status/severity accent shared by badges, callouts, and status lines
/// (maps to [`BadgeLevel`] and a glyph).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Severity {
    /// Neutral / default.
    #[default]
    Neutral,
    /// Informational.
    Info,
    /// Success / done.
    Success,
    /// Caution.
    Warning,
    /// Error.
    Error,
}

impl Severity {
    /// The [`BadgeLevel`] this severity maps to.
    #[must_use]
    pub fn badge_level(self) -> BadgeLevel {
        match self {
            Self::Neutral => BadgeLevel::Neutral,
            Self::Info => BadgeLevel::Info,
            Self::Success => BadgeLevel::Success,
            Self::Warning => BadgeLevel::Warning,
            Self::Error => BadgeLevel::Error,
        }
    }

    /// The leading glyph + accent colour for a status line / callout.
    #[must_use]
    pub fn marker(self) -> (&'static str, Color) {
        match self {
            Self::Neutral => ("•", Color::Gray),
            Self::Info => ("ℹ", Color::Blue),
            Self::Success => ("✔", Color::Green),
            Self::Warning => ("⚠", Color::Yellow),
            Self::Error => ("✖", Color::Red),
        }
    }
}

/// One key→value row for [`UiNode::KeyValue`].
#[derive(Debug, Clone, PartialEq)]
pub struct KeyValueRow {
    /// The key label.
    pub key: String,
    /// The value text.
    pub value: String,
}

/// The resolved, concrete UI node both formats project to. Containers
/// reference children inline (the format already flattened its
/// adjacency/element map); leaves carry resolved display values.
#[derive(Debug, Clone, PartialEq)]
pub enum UiNode {
    /// Vertical stack.
    Column {
        /// Child nodes, top to bottom.
        children: Vec<UiNode>,
        /// Main-axis (vertical) distribution.
        justify: Justify,
        /// Cross-axis (horizontal) alignment.
        align: CrossAlign,
    },
    /// Horizontal run.
    Row {
        /// Child nodes, left to right.
        children: Vec<UiNode>,
        /// Main-axis (horizontal) distribution.
        justify: Justify,
        /// Cross-axis (vertical) alignment.
        align: CrossAlign,
    },
    /// Overlapping children drawn in order (A2UI `Modal` content / a
    /// json-render absolute box) — each fills the area.
    Stack(Vec<UiNode>),
    /// A titled, bordered container around one child (A2UI `Card`).
    Card {
        /// Optional header title.
        title: Option<String>,
        /// The body node.
        child: Box<UiNode>,
    },
    /// A scrollable viewport over one child at a resolved row offset.
    Scroll {
        /// The (taller) content node.
        child: Box<UiNode>,
        /// Caller-owned vertical row offset (already resolved).
        offset: u16,
    },
    /// A horizontal/vertical rule with an optional label.
    Divider {
        /// `true` for a vertical rule.
        vertical: bool,
        /// Optional inline label.
        label: Option<String>,
    },
    /// Flexible empty space (a json-render `Spacer`).
    Spacer,
    /// A run of styled text at a semantic level.
    Text {
        /// The styled spans (already resolved/interpolated).
        spans: Vec<(String, Style)>,
        /// The semantic level/style.
        variant: TextVariant,
        /// Horizontal alignment.
        align: Alignment,
        /// Soft-wrap long lines.
        wrap: bool,
    },
    /// A markdown document (delegates to `rstui_widgets::Markdown`).
    Markdown(String),
    /// A focusable, optionally-disabled action label.
    Button {
        /// The node id (action routing).
        id: NodeId,
        /// The button label.
        label: String,
        /// Accent the primary action.
        primary: bool,
        /// Render disabled and non-interactive.
        disabled: bool,
        /// Render the focus ring.
        focused: bool,
    },
    /// A hyperlink (activates an `openUrl`/navigation action).
    Link {
        /// The node id (action routing).
        id: NodeId,
        /// The link text (falls back to the URL).
        label: String,
        /// The destination URL.
        href: String,
        /// Render the focus ring.
        focused: bool,
    },
    /// A small inline status pill.
    Badge {
        /// The pill text.
        label: String,
        /// The accent level.
        severity: Severity,
    },
    /// A horizontal progress bar in `0.0..=1.0` with an optional label.
    Gauge {
        /// The fill ratio.
        ratio: f64,
        /// Optional centred label.
        label: Option<String>,
    },
    /// A one-cell animated busy indicator projecting a caller tick.
    Spinner {
        /// The caller-owned animation tick.
        tick: u64,
        /// Optional trailing label.
        label: Option<String>,
    },
    /// An aligned key→value pane.
    KeyValue(Vec<KeyValueRow>),
    /// A leading-glyph status line.
    StatusLine {
        /// The accent/severity.
        severity: Severity,
        /// The line text.
        text: String,
    },
    /// A single-line text-entry field (display projection; the format
    /// reducer owns the edit and writes it back to the data model).
    TextField {
        /// The node id (action routing on submit).
        id: NodeId,
        /// The field label.
        label: String,
        /// The current value.
        value: String,
        /// Placeholder shown when empty.
        placeholder: String,
        /// Mask the value (password).
        masked: bool,
        /// Render the focus ring.
        focused: bool,
    },
    /// A labelled boolean control.
    Checkbox {
        /// The node id (toggle action routing).
        id: NodeId,
        /// The control label.
        label: String,
        /// The checked state.
        checked: bool,
        /// Render the focus ring.
        focused: bool,
    },
    /// A placeholder for media that a terminal cannot render inline
    /// (A2UI `Image`/`Video`/`AudioPlayer`).
    Media {
        /// A short kind tag (`image`/`video`/`audio`).
        kind: String,
        /// Alt/description text.
        alt: String,
    },
    /// An unknown / unsupported component, rendered as a visible
    /// placeholder so progressive rendering degrades instead of breaking.
    Placeholder(String),
}

impl Default for UiNode {
    fn default() -> Self {
        Self::Placeholder(String::new())
    }
}

/// One interactive node's screen rectangle, recorded by
/// [`render`](UiNode::render) for next-frame hit-testing.
#[derive(Debug, Clone, PartialEq)]
pub struct HitRect {
    /// The interactive node's id.
    pub id: NodeId,
    /// Its screen rectangle on the frame it was drawn.
    pub area: Rect,
}

/// The interactive rectangles from the last [`render`](UiNode::render),
/// queried by [`at`](HitMap::at) to resolve a click to a [`NodeId`]
/// (immediate-mode hit-testing, the `docs/composition.md` accessor
/// pattern).
#[derive(Debug, Clone, Default)]
pub struct HitMap {
    rects: Vec<HitRect>,
}

impl HitMap {
    /// An empty hit map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears the recorded rectangles (call before re-rendering).
    pub fn clear(&mut self) {
        self.rects.clear();
    }

    /// Records an interactive node's rectangle.
    pub fn push(&mut self, id: NodeId, area: Rect) {
        self.rects.push(HitRect { id, area });
    }

    /// The id of the top-most interactive node containing `position`
    /// (last recorded wins — children draw after parents).
    #[must_use]
    pub fn at(&self, position: Position) -> Option<&str> {
        self.rects
            .iter()
            .rev()
            .find(|hit| contains(hit.area, position))
            .map(|hit| hit.id.as_str())
    }

    /// Every interactive node's `(id, area)` in **draw order** (the
    /// order [`render`](UiNode::render) recorded them — parents before
    /// children, top-to-bottom). A reducer building a keyboard focus
    /// ring (Tab / Shift+Tab) or drawing a focus highlight reads this
    /// instead of re-deriving the tree: a thin accessor over the
    /// already-recorded rectangles (pure, ADR 0012 — no retained tree).
    #[must_use]
    pub fn entries(&self) -> &[HitRect] {
        &self.rects
    }
}

fn contains(area: Rect, position: Position) -> bool {
    area.width > 0
        && area.height > 0
        && position.x >= area.x
        && position.x < area.x.saturating_add(area.width)
        && position.y >= area.y
        && position.y < area.y.saturating_add(area.height)
}

impl UiNode {
    /// Draws the node into `area` of `buf`, recording every interactive
    /// descendant's rectangle into `hits`. Total: a zero/oversized area
    /// or empty content is a safe no-op.
    pub fn render(&self, area: Rect, buf: &mut Buffer, hits: &mut HitMap) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        match self {
            Self::Column {
                children,
                justify,
                align,
            } => self.render_axis(
                children,
                *justify,
                *align,
                Direction::Vertical,
                area,
                buf,
                hits,
            ),
            Self::Row {
                children,
                justify,
                align,
            } => self.render_axis(
                children,
                *justify,
                *align,
                Direction::Horizontal,
                area,
                buf,
                hits,
            ),
            Self::Stack(children) => {
                for child in children {
                    child.render(area, buf, hits);
                }
            }
            Self::Card { title, child } => {
                let mut block = Block::default().borders(Borders::ALL);
                if let Some(text) = title {
                    block = block.title(text.as_str());
                }
                let inner = block.inner(area);
                block.render(area, buf);
                child.render(inner, buf, hits);
            }
            Self::Scroll { child, offset } => {
                // A pure clip: render the child shifted up by `offset`
                // rows into a scratch then blit (kept simple + total).
                let mut scratch = Buffer::empty(Rect::new(
                    0,
                    0,
                    area.width,
                    area.height.saturating_add(*offset).max(1),
                ));
                child.render(scratch.area(), &mut scratch, &mut HitMap::new());
                for row in 0..area.height {
                    let src = row.saturating_add(*offset);
                    for col in 0..area.width {
                        if let Some(cell) = scratch.get(Position::new(col, src)).cloned() {
                            if let Some(slot) =
                                buf.get_mut(Position::new(area.x + col, area.y + row))
                            {
                                *slot = cell;
                            }
                        }
                    }
                }
            }
            Self::Divider { vertical, label } => render_divider(*vertical, label, area, buf),
            Self::Spacer => {}
            Self::Text {
                spans,
                variant,
                align,
                wrap,
            } => {
                let base = variant.style();
                let line = Line::from(
                    spans
                        .iter()
                        .map(|(text, style)| Span::styled(text.clone(), base.patch(*style)))
                        .collect::<Vec<_>>(),
                )
                .alignment(*align);
                let mut paragraph = Paragraph::new(line);
                if *wrap {
                    paragraph = paragraph.wrap(Wrap { trim: false });
                }
                paragraph.render(area, buf);
            }
            Self::Markdown(source) => Markdown::new(source.as_str()).render(area, buf),
            Self::Button {
                id,
                label,
                primary,
                disabled,
                focused,
            } => {
                let mut style = Style::new();
                if *primary {
                    style = style.fg(Color::Black).bg(Color::Cyan);
                }
                if *disabled {
                    style = style.add_modifier(Modifier::DIM);
                }
                Button::new(label.as_str())
                    .style(style)
                    .focused(*focused && !*disabled)
                    .render(area, buf);
                if !*disabled {
                    hits.push(id.clone(), area);
                }
            }
            Self::Link {
                id,
                label,
                href,
                focused,
            } => {
                let text = if label.is_empty() { href } else { label };
                let mut style = Style::new()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::UNDERLINED);
                if *focused {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                Paragraph::new(Line::from(Span::styled(text.clone(), style))).render(area, buf);
                hits.push(id.clone(), area);
            }
            Self::Badge { label, severity } => Badge::new(Line::from(label.as_str()))
                .level(severity.badge_level())
                .render(area, buf),
            Self::Gauge { ratio, label } => {
                let mut gauge = Gauge::default().ratio(ratio.clamp(0.0, 1.0));
                if let Some(text) = label {
                    gauge = gauge.label(text.as_str());
                }
                gauge.render(area, buf);
            }
            Self::Spinner { tick, label } => {
                let columns =
                    Layout::horizontal([Constraint::Length(1), Constraint::Min(0)]).split(area);
                Spinner::new()
                    .tick(usize::try_from(*tick).unwrap_or(usize::MAX))
                    .render(columns[0], buf);
                if let Some(text) = label {
                    Paragraph::new(Line::from(format!(" {text}"))).render(columns[1], buf);
                }
            }
            Self::KeyValue(rows) => render_key_value(rows, area, buf),
            Self::StatusLine { severity, text } => {
                let (glyph, color) = severity.marker();
                Paragraph::new(Line::from(vec![
                    Span::styled(format!("{glyph} "), Style::new().fg(color)),
                    Span::raw(text.clone()),
                ]))
                .render(area, buf);
            }
            Self::TextField {
                id,
                label,
                value,
                placeholder,
                masked,
                focused,
            } => {
                let shown = if value.is_empty() {
                    placeholder.clone()
                } else if *masked {
                    "•".repeat(value.chars().count())
                } else {
                    value.clone()
                };
                let mut style = Style::new();
                if *focused {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                let dim = value.is_empty();
                let value_style = if dim {
                    style.add_modifier(Modifier::DIM)
                } else {
                    style
                };
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        format!("{label}: "),
                        Style::new().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(shown, value_style),
                ]))
                .render(area, buf);
                hits.push(id.clone(), area);
            }
            Self::Checkbox {
                id,
                label,
                checked,
                focused,
            } => {
                let glyph = if *checked { "[x]" } else { "[ ]" };
                let mut style = Style::new();
                if *focused {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                Paragraph::new(Line::from(Span::styled(format!("{glyph} {label}"), style)))
                    .render(area, buf);
                hits.push(id.clone(), area);
            }
            Self::Media { kind, alt } => {
                let text = if alt.is_empty() {
                    format!("[{kind}]")
                } else {
                    format!("[{kind}: {alt}]")
                };
                Paragraph::new(Line::from(Span::styled(
                    text,
                    Style::new().add_modifier(Modifier::DIM),
                )))
                .render(area, buf);
            }
            Self::Placeholder(what) => {
                let text = if what.is_empty() {
                    "[unsupported]".to_owned()
                } else {
                    format!("[unsupported: {what}]")
                };
                Paragraph::new(Line::from(Span::styled(
                    text,
                    Style::new().fg(Color::Red).add_modifier(Modifier::DIM),
                )))
                .render(area, buf);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_axis(
        &self,
        children: &[UiNode],
        _justify: Justify,
        _align: CrossAlign,
        direction: Direction,
        area: Rect,
        buf: &mut Buffer,
        hits: &mut HitMap,
    ) {
        if children.is_empty() {
            return;
        }
        // Equal-share layout is the total, predictable default; richer
        // justify/weight handling is a deliberate later additive (the
        // ADR 0012 "start minimal, total" rule).
        let constraints = vec![Constraint::Ratio(1, children.len() as u32); children.len()];
        let slots = Layout::new(direction, constraints).split(area);
        for (child, slot) in children.iter().zip(slots.iter()) {
            child.render(*slot, buf, hits);
        }
    }

    /// The single-line plain-text projection of this node (for a
    /// compact/log rendering and snapshot tests).
    #[must_use]
    pub fn to_plain(&self) -> String {
        match self {
            Self::Text { spans, .. } => spans.iter().map(|(text, _)| text.as_str()).collect(),
            Self::Markdown(source) => source.clone(),
            Self::Button { label, .. } | Self::Badge { label, .. } => label.clone(),
            Self::Link { label, href, .. } => {
                if label.is_empty() {
                    href.clone()
                } else {
                    label.clone()
                }
            }
            Self::Placeholder(what) => format!("[unsupported: {what}]"),
            Self::Column { children, .. } | Self::Row { children, .. } | Self::Stack(children) => {
                children
                    .iter()
                    .map(Self::to_plain)
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            Self::Card { child, .. } | Self::Scroll { child, .. } => child.to_plain(),
            _ => String::new(),
        }
    }

    /// The node's natural content height in rows at `width` — total,
    /// panic-free, never `0`.
    ///
    /// This is what a *line-oriented* host (e.g. an ACP transcript that
    /// embeds a rendered document) sizes its scratch buffer to, so a
    /// region/bordered widget renders at its content size instead of
    /// expanding to fill an arbitrary area (a `Card` given 40 rows would
    /// otherwise become a 40-row box with the content only at the top).
    /// Leaves are one row; containers compose their children the way
    /// [`render`](Self::render) lays them out.
    #[must_use]
    pub fn measure_height(&self, width: u16) -> u16 {
        let width = width.max(1);
        let height = match self {
            Self::Column { children, .. } => children
                .iter()
                .map(|child| u32::from(child.measure_height(width)))
                .sum(),
            Self::Row { children, .. } | Self::Stack(children) => children
                .iter()
                .map(|child| u32::from(child.measure_height(width)))
                .max()
                .unwrap_or(1),
            // A bordered frame: the child plus the top/bottom border.
            Self::Card { child, .. } => {
                u32::from(child.measure_height(width.saturating_sub(2))) + 2
            }
            Self::Scroll { child, .. } => u32::from(child.measure_height(width)),
            Self::Markdown(source) => {
                Markdown::new(source.as_str()).lines(width).len().max(1) as u32
            }
            Self::Text { spans, .. } => {
                let text: String = spans.iter().map(|(run, _)| run.as_str()).collect();
                text.split('\n')
                    .map(|line| {
                        let columns = line.chars().count().max(1);
                        columns.div_ceil(width as usize).max(1) as u32
                    })
                    .sum::<u32>()
                    .max(1)
            }
            Self::KeyValue(rows) => (rows.len().max(1)) as u32,
            // Single-row leaves (Button, Link, Badge, Gauge, Spinner,
            // StatusLine, TextField, Checkbox, Media, Divider, Spacer,
            // Placeholder).
            _ => 1,
        };
        height.clamp(1, u32::from(u16::MAX)) as u16
    }
}

fn render_divider(vertical: bool, label: &Option<String>, area: Rect, buf: &mut Buffer) {
    let style = Style::new().add_modifier(Modifier::DIM);
    if vertical {
        for row in 0..area.height {
            buf.set_str(Position::new(area.x, area.y + row), "│", style);
        }
        return;
    }
    let rule: String = "─".repeat(area.width as usize);
    buf.set_str(Position::new(area.x, area.y), &rule, style);
    if let Some(text) = label {
        if !text.is_empty() && area.width as usize > text.len() + 2 {
            buf.set_str(
                Position::new(area.x + 1, area.y),
                &format!(" {text} "),
                Style::new(),
            );
        }
    }
}

fn render_key_value(rows: &[KeyValueRow], area: Rect, buf: &mut Buffer) {
    let key_width = rows
        .iter()
        .map(|row| row.key.chars().count())
        .max()
        .unwrap_or(0)
        .min(area.width as usize / 2);
    for (index, row) in rows.iter().enumerate() {
        if index as u16 >= area.height {
            break;
        }
        let line = Line::from(vec![
            Span::styled(
                format!("{:<key_width$}  ", row.key),
                Style::new().add_modifier(Modifier::BOLD),
            ),
            Span::raw(row.value.clone()),
        ]);
        Paragraph::new(line).render(Rect::new(area.x, area.y + index as u16, area.width, 1), buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_to(node: &UiNode, width: u16, height: u16) -> (Buffer, HitMap) {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        let mut hits = HitMap::new();
        node.render(buf.area(), &mut buf, &mut hits);
        (buf, hits)
    }

    #[test]
    fn column_of_text_and_button_hit_tests() {
        let node = UiNode::Column {
            justify: Justify::Start,
            align: CrossAlign::Stretch,
            children: vec![
                UiNode::Text {
                    spans: vec![("Title".to_owned(), Style::new())],
                    variant: TextVariant::H1,
                    align: Alignment::Left,
                    wrap: false,
                },
                UiNode::Button {
                    id: "ok".to_owned(),
                    label: "OK".to_owned(),
                    primary: true,
                    disabled: false,
                    focused: false,
                },
            ],
        };
        let (buf, hits) = render_to(&node, 12, 4);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'T');
        // The button occupies the lower half; a click there resolves.
        let target = hits.at(Position::new(1, 3));
        assert_eq!(target, Some("ok"));
        assert_eq!(hits.at(Position::new(1, 0)), None);
        assert_eq!(node.to_plain(), "Title OK");
    }

    #[test]
    fn entries_lists_interactive_nodes_in_draw_order() {
        let node = UiNode::Column {
            justify: Justify::Start,
            align: CrossAlign::Stretch,
            children: vec![
                UiNode::TextField {
                    id: "/name".to_owned(),
                    label: "Name".to_owned(),
                    value: String::new(),
                    placeholder: String::new(),
                    masked: false,
                    focused: false,
                },
                UiNode::Checkbox {
                    id: "agree".to_owned(),
                    label: "Agree".to_owned(),
                    checked: false,
                    focused: false,
                },
                UiNode::Button {
                    id: "go".to_owned(),
                    label: "Go".to_owned(),
                    primary: true,
                    disabled: false,
                    focused: false,
                },
            ],
        };
        let (_buf, hits) = render_to(&node, 20, 6);
        let ids: Vec<&str> = hits.entries().iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, vec!["/name", "agree", "go"], "draw order preserved");
        // Each recorded rect is the same one `at` resolves (no drift).
        for entry in hits.entries() {
            let mid = Position::new(
                entry.area.x + entry.area.width / 2,
                entry.area.y + entry.area.height / 2,
            );
            assert_eq!(hits.at(mid), Some(entry.id.as_str()));
        }
    }

    #[test]
    fn totality_zero_area_and_unknown_are_safe() {
        let node = UiNode::Placeholder("FancyThing".to_owned());
        let mut buf = Buffer::empty(Rect::new(0, 0, 0, 0));
        let mut hits = HitMap::new();
        node.render(buf.area(), &mut buf, &mut hits); // no panic on 0×0
        let (buf, _) = render_to(&node, 24, 1);
        let rendered: String = (0..24)
            .filter_map(|x| buf.get(Position::new(x, 0)).map(|c| c.symbol))
            .collect();
        assert!(rendered.contains("FancyThing"));
        let disabled = UiNode::Button {
            id: "x".to_owned(),
            label: "x".to_owned(),
            primary: false,
            disabled: true,
            focused: false,
        };
        let (_, hits) = render_to(&disabled, 8, 1);
        assert_eq!(hits.at(Position::new(0, 0)), None); // disabled = no hit
    }
}
