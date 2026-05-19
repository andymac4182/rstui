//! The **renderer walk** — `spec.root` → [`UiNode`], mapping the 26
//! json-render standard components onto the projection target. Ported
//! from `packages/ink/src/renderer.tsx` (the element walk, `visible`,
//! `repeat`, child-by-key recursion) and `components/standard.tsx` (the
//! per-component terminal rendering).
//!
//! # The walk
//!
//! From the root key, per element: evaluate `visible` (skip if false);
//! collect `$bindState`/`$bindItem` write-back paths; resolve props;
//! expand `repeat` (render children once per item of the state array,
//! with a [`RepeatScope`]); recurse children by key (a missing key is a
//! visible `[Missing: key]` placeholder unless a `loading` flag is set,
//! mirroring the reference). An unknown component type is
//! [`UiNode::Placeholder`] so progressive rendering degrades instead of
//! breaking.
//!
//! `safeColor`/`safeBoxProps` are honoured: a `flexDirection`/justify maps
//! onto [`Justify`]/[`CrossAlign`], and colours invisible on a dark
//! terminal (`black`/`#000`) are dropped. The component set that has no
//! terminal analogue (`Sparkline`/`BarChart` exact charts) degrades to a
//! block-glyph `Text` or a `Placeholder`, never a panic.

use serde_json::Value;

use rstui_core::{Alignment, Modifier, Style};

use super::expr::{
    RepeatScope, ResolveScope, coerce_to_string, evaluate_visibility, resolve_bindings,
    resolve_element_props,
};
use super::spec::Spec;
use crate::tree::{CrossAlign, Justify, KeyValueRow, Severity, TextVariant, UiNode};

/// Whether the spec is still streaming. When `true`, a child key that is
/// not yet in the element map renders nothing (the reference suppresses
/// the `[Missing]` warning while `loading`); when `false` it renders a
/// visible `[Missing: key]` placeholder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Loading(pub bool);

/// Projects a parsed [`Spec`] to a [`UiNode`] by walking from the root,
/// resolving every binding/expression against `scope`. Total: a missing
/// root, a cycle, or any malformed element degrades to a placeholder.
#[must_use]
pub fn project(spec: &Spec, scope: &ResolveScope<'_>, loading: Loading) -> UiNode {
    if spec.root.is_empty() {
        return UiNode::Placeholder(String::new());
    }
    let mut guard = RecursionGuard::new();
    project_element(&spec.root, spec, scope, loading, &mut guard)
}

/// Bounds recursion so a self-referential child list (an LLM cycle)
/// degrades to a placeholder instead of overflowing the stack.
struct RecursionGuard {
    depth: usize,
}

impl RecursionGuard {
    const MAX_DEPTH: usize = 256;

    fn new() -> Self {
        Self { depth: 0 }
    }
}

fn project_element(
    key: &str,
    spec: &Spec,
    scope: &ResolveScope<'_>,
    loading: Loading,
    guard: &mut RecursionGuard,
) -> UiNode {
    if guard.depth >= RecursionGuard::MAX_DEPTH {
        return UiNode::Placeholder("cycle".to_owned());
    }
    let Some(element) = spec.element(key) else {
        return if loading.0 {
            UiNode::Spacer
        } else {
            UiNode::Placeholder(format!("Missing: {key}"))
        };
    };

    // visible: skip the element entirely when false.
    if !evaluate_visibility(element.visible.as_ref(), scope) {
        return UiNode::Spacer;
    }

    // Bindings are collected from the RAW props (before resolution) so a
    // projected input carries its write-back pointer.
    let bindings = resolve_bindings(&element.props, scope);
    let props = Value::Object(resolve_element_props(&element.props, scope));

    guard.depth += 1;
    let children = if let Some(repeat) = &element.repeat {
        project_repeat_children(element, repeat, spec, scope, loading, guard)
    } else {
        element
            .children
            .iter()
            .map(|child_key| project_element(child_key, spec, scope, loading, guard))
            .collect()
    };
    guard.depth -= 1;

    map_component(
        &element.type_name,
        &props,
        children,
        key,
        &bindings,
        &element.on,
        scope.palette,
    )
}

fn project_repeat_children(
    element: &super::spec::UiElement,
    repeat: &super::spec::RepeatSpec,
    spec: &Spec,
    scope: &ResolveScope<'_>,
    loading: Loading,
    guard: &mut RecursionGuard,
) -> Vec<UiNode> {
    let array = match scope.model.get(&repeat.state_path) {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    let mut rendered = Vec::new();
    for (index, item) in array.into_iter().enumerate() {
        let item_scope = RepeatScope {
            item,
            index,
            base_path: format!("{}/{index}", repeat.state_path),
        };
        let child_scope = scope.with_repeat(&item_scope);
        for child_key in &element.children {
            rendered.push(project_element(
                child_key,
                spec,
                &child_scope,
                loading,
                guard,
            ));
        }
    }
    rendered
}

/// `safeColor`: drop colours invisible on a dark terminal (the reference
/// `INVISIBLE_COLORS` set).
fn is_invisible_color(color: &str) -> bool {
    matches!(color, "black" | "#000" | "#000000")
}

/// Maps a JSON colour name to an rstui [`Color`](rstui_core::Color), or
/// `None` for absent/invisible (so the terminal default applies).
fn parse_color(value: Option<&Value>) -> Option<rstui_core::Color> {
    let name = value?.as_str()?;
    if is_invisible_color(name) {
        return None;
    }
    Some(match name {
        "red" => rstui_core::Color::Red,
        "green" => rstui_core::Color::Green,
        "yellow" => rstui_core::Color::Yellow,
        "blue" => rstui_core::Color::Blue,
        "magenta" => rstui_core::Color::Magenta,
        "cyan" => rstui_core::Color::Cyan,
        "white" => rstui_core::Color::White,
        "gray" | "grey" => rstui_core::Color::Gray,
        _ => return None,
    })
}

fn prop_str<'value>(props: &'value Value, key: &str) -> Option<&'value str> {
    props.as_object()?.get(key)?.as_str()
}

fn prop_bool(props: &Value, key: &str) -> bool {
    props
        .as_object()
        .and_then(|object| object.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn prop_f64(props: &Value, key: &str) -> Option<f64> {
    props.as_object()?.get(key)?.as_f64()
}

fn prop_value<'value>(props: &'value Value, key: &str) -> Option<&'value Value> {
    props.as_object()?.get(key)
}

/// The text style for an Ink-style `Text` element's modifier props
/// (`bold`/`italic`/`underline`/`strikethrough`/`dimColor`/`inverse` +
/// `color`).
fn text_style(props: &Value, palette: &crate::color::Palette) -> Style {
    let mut style = Style::new();
    // A `"color"` prop is a theme token (`success`, `chart2`, …) or a
    // raw `#hex`/named fallback, resolved against the active palette;
    // the legacy named-only `parse_color` is the last resort.
    if let Some(color) = prop_str(props, "color")
        .and_then(crate::color::parse_token)
        .map(|token| palette.resolve(token))
        .or_else(|| parse_color(prop_value(props, "color")))
    {
        style = style.fg(color);
    }
    if prop_bool(props, "bold") {
        style = style.add_modifier(Modifier::BOLD);
    }
    if prop_bool(props, "italic") {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if prop_bool(props, "underline") {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if prop_bool(props, "strikethrough") {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    if prop_bool(props, "dimColor") {
        style = style.add_modifier(Modifier::DIM);
    }
    if prop_bool(props, "inverse") {
        style = style.add_modifier(Modifier::REVERSED);
    }
    style
}

fn justify_from(value: Option<&str>) -> Justify {
    match value {
        Some("center") => Justify::Center,
        Some("flex-end") => Justify::End,
        Some("space-between") => Justify::SpaceBetween,
        Some("space-around" | "space-evenly") => Justify::SpaceAround,
        _ => Justify::Start,
    }
}

fn align_from(value: Option<&str>) -> CrossAlign {
    match value {
        Some("flex-start") => CrossAlign::Start,
        Some("center") => CrossAlign::Center,
        Some("flex-end") => CrossAlign::End,
        _ => CrossAlign::Stretch,
    }
}

fn severity_from_variant(value: Option<&str>) -> Severity {
    match value {
        Some("info") => Severity::Info,
        Some("success") => Severity::Success,
        Some("warning") => Severity::Warning,
        Some("error") => Severity::Error,
        _ => Severity::Neutral,
    }
}

fn one_styled(text: String, style: Style) -> UiNode {
    UiNode::Text {
        spans: vec![(text, style)],
        variant: TextVariant::Body,
        align: Alignment::Left,
        wrap: false,
    }
}

/// Coerces a `KeyValue.value` (string | number | array) to display text
/// the way the reference `coerceToString` does.
fn coerce_kv_value(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(coerce_to_string)
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => coerce_to_string(other),
    }
}

/// Maps one resolved element (its type, resolved props, already-projected
/// children) onto a [`UiNode`]. The 26 standard components plus the
/// unknown-type fallback. `node_id`/`bindings`/`on` thread interaction
/// identity through to the interactive variants.
#[allow(clippy::too_many_lines)]
fn map_component(
    type_name: &str,
    props: &Value,
    children: Vec<UiNode>,
    node_id: &str,
    bindings: &std::collections::BTreeMap<String, String>,
    on: &serde_json::Map<String, Value>,
    palette: &crate::color::Palette,
) -> UiNode {
    match type_name {
        "Box" => {
            let direction = prop_str(props, "flexDirection");
            let justify = justify_from(prop_str(props, "justifyContent"));
            let align = align_from(prop_str(props, "alignItems"));
            if matches!(direction, Some("row") | Some("row-reverse")) {
                UiNode::Row {
                    children,
                    justify,
                    align,
                }
            } else {
                // Ink's default flexDirection is column for our purposes
                // (a terminal stacks); row only when explicitly asked.
                UiNode::Column {
                    children,
                    justify,
                    align,
                }
            }
        }
        "Text" => {
            let text = prop_str(props, "text").unwrap_or("").to_owned();
            UiNode::Text {
                spans: vec![(text, text_style(props, palette))],
                variant: TextVariant::Body,
                align: Alignment::Left,
                wrap: matches!(prop_str(props, "wrap"), Some("wrap") | None),
            }
        }
        "Newline" | "Spacer" => UiNode::Spacer,
        "Heading" => {
            let variant = match prop_str(props, "level") {
                Some("h1") => TextVariant::H1,
                Some("h3") => TextVariant::H3,
                Some("h4") => TextVariant::H4,
                _ => TextVariant::H2,
            };
            let mut style = Style::new();
            if let Some(color) = prop_str(props, "color")
                .and_then(crate::color::parse_token)
                .map(|token| palette.resolve(token))
                .or_else(|| parse_color(prop_value(props, "color")))
            {
                style = style.fg(color);
            }
            UiNode::Text {
                spans: vec![(prop_str(props, "text").unwrap_or("").to_owned(), style)],
                variant,
                align: Alignment::Left,
                wrap: false,
            }
        }
        "Divider" => UiNode::Divider {
            vertical: false,
            label: prop_str(props, "title").map(str::to_owned),
        },
        "Badge" => UiNode::Badge {
            label: prop_str(props, "label").unwrap_or("").to_owned(),
            severity: severity_from_variant(prop_str(props, "variant")),
        },
        "Spinner" => UiNode::Spinner {
            tick: 0,
            label: prop_str(props, "label").map(str::to_owned),
        },
        "ProgressBar" => UiNode::Gauge {
            ratio: prop_f64(props, "progress").unwrap_or(0.0).clamp(0.0, 1.0),
            label: prop_str(props, "label").map(str::to_owned),
        },
        "Sparkline" => {
            // No terminal sparkline widget — render block glyphs as text
            // (the reference uses the same ▁▂▃▄▅▆▇█ ramp).
            let label = prop_str(props, "label").map(str::to_owned);
            let blocks = sparkline_blocks(prop_value(props, "data"));
            let text = match label {
                Some(name) => format!("{name} {blocks}"),
                None => blocks,
            };
            one_styled(text, Style::new().fg(rstui_core::Color::Green))
        }
        "BarChart" => bar_chart(props),
        "Table" => table(props),
        "List" => list(props),
        "ListItem" => {
            let mut spans = Vec::new();
            if let Some(leading) = prop_str(props, "leading") {
                spans.push((format!("{leading} "), Style::new()));
            }
            spans.push((
                prop_str(props, "title").unwrap_or("").to_owned(),
                Style::new().add_modifier(Modifier::BOLD),
            ));
            if let Some(subtitle) = prop_str(props, "subtitle") {
                spans.push((
                    format!("  {subtitle}"),
                    Style::new().add_modifier(Modifier::DIM),
                ));
            }
            if let Some(trailing) = prop_str(props, "trailing") {
                spans.push((format!("  {trailing}"), Style::new()));
            }
            UiNode::Text {
                spans,
                variant: TextVariant::Body,
                align: Alignment::Left,
                wrap: false,
            }
        }
        "Card" => UiNode::Card {
            title: prop_str(props, "title").map(str::to_owned),
            child: Box::new(if children.len() == 1 {
                children.into_iter().next().unwrap_or(UiNode::Spacer)
            } else {
                UiNode::Column {
                    children,
                    justify: Justify::Start,
                    align: CrossAlign::Stretch,
                }
            }),
        },
        "KeyValue" => UiNode::KeyValue(vec![KeyValueRow {
            key: prop_str(props, "label").unwrap_or("").to_owned(),
            value: coerce_kv_value(prop_value(props, "value")),
        }]),
        "Link" => {
            let href = prop_str(props, "url").unwrap_or("").to_owned();
            UiNode::Link {
                id: node_id.to_owned(),
                label: prop_str(props, "label").unwrap_or("").to_owned(),
                href,
                focused: false,
            }
        }
        "StatusLine" => UiNode::StatusLine {
            severity: severity_from_variant(prop_str(props, "status")),
            text: prop_str(props, "text").unwrap_or("").to_owned(),
        },
        "Metric" => {
            let mut spans = vec![
                (
                    format!("{} ", prop_str(props, "label").unwrap_or("")),
                    Style::new().add_modifier(Modifier::DIM),
                ),
                (
                    prop_str(props, "value").unwrap_or("").to_owned(),
                    Style::new().add_modifier(Modifier::BOLD),
                ),
            ];
            if let Some(detail) = prop_str(props, "detail") {
                let (prefix, color) = match prop_str(props, "trend") {
                    Some("up") => ("+", rstui_core::Color::Green),
                    Some("down") => ("", rstui_core::Color::Red),
                    _ => ("~", rstui_core::Color::Gray),
                };
                spans.push((format!(" {prefix}{detail}"), Style::new().fg(color)));
            }
            UiNode::Text {
                spans,
                variant: TextVariant::Body,
                align: Alignment::Left,
                wrap: false,
            }
        }
        "Callout" => {
            let severity = match prop_str(props, "type") {
                Some("warning") => Severity::Warning,
                Some("important") => Severity::Error,
                Some("tip") => Severity::Success,
                _ => Severity::Info,
            };
            let title = prop_str(props, "title");
            let content = prop_str(props, "content").unwrap_or("");
            let body = match title {
                Some(heading) => format!("{heading}: {content}"),
                None => content.to_owned(),
            };
            UiNode::StatusLine {
                severity,
                text: body,
            }
        }
        "Timeline" => timeline(props),
        "TextInput" => UiNode::TextField {
            id: bound_id(node_id, bindings, "value"),
            label: prop_str(props, "label").unwrap_or("").to_owned(),
            value: prop_str(props, "value").unwrap_or("").to_owned(),
            placeholder: prop_str(props, "placeholder").unwrap_or("").to_owned(),
            masked: prop_str(props, "mask").is_some_and(|mask| !mask.is_empty()),
            focused: false,
        },
        "Select" | "MultiSelect" => select(props, node_id, on),
        "ConfirmInput" => {
            let yes = prop_str(props, "yesLabel").unwrap_or("Yes").to_owned();
            let no = prop_str(props, "noLabel").unwrap_or("No").to_owned();
            let mut row = Vec::new();
            if let Some(message) = prop_str(props, "message") {
                row.push(one_styled(
                    format!("{message} "),
                    Style::new().add_modifier(Modifier::BOLD),
                ));
            }
            row.push(UiNode::Button {
                id: format!("{node_id}#confirm"),
                label: yes,
                primary: true,
                disabled: false,
                focused: false,
            });
            row.push(UiNode::Button {
                id: format!("{node_id}#deny"),
                label: no,
                primary: false,
                disabled: false,
                focused: false,
            });
            UiNode::Row {
                children: row,
                justify: Justify::Start,
                align: CrossAlign::Stretch,
            }
        }
        "Tabs" => tabs(props, children),
        "Markdown" => UiNode::Markdown(prop_str(props, "text").unwrap_or("").to_owned()),
        // Unknown component → visible placeholder (progressive degrade).
        other => UiNode::Placeholder(other.to_owned()),
    }
}

/// The interactive node id for a bound control: the binding's write-back
/// pointer if present (so the reducer knows where to write), else the
/// element key. The `#`-suffixed forms route sub-events (confirm/deny).
fn bound_id(
    node_id: &str,
    bindings: &std::collections::BTreeMap<String, String>,
    prop: &str,
) -> String {
    bindings
        .get(prop)
        .cloned()
        .unwrap_or_else(|| node_id.to_owned())
}

const SPARK_BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

fn sparkline_blocks(data: Option<&Value>) -> String {
    let Some(Value::Array(items)) = data else {
        return String::new();
    };
    let numbers: Vec<f64> = items.iter().filter_map(Value::as_f64).collect();
    if numbers.is_empty() {
        return String::new();
    }
    let min = numbers.iter().copied().fold(f64::INFINITY, f64::min);
    let max = numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let range = if (max - min).abs() < f64::EPSILON {
        1.0
    } else {
        max - min
    };
    numbers
        .iter()
        .map(|value| {
            let normalised = (value - min) / range;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let index = (normalised * (SPARK_BLOCKS.len() - 1) as f64).round() as usize;
            SPARK_BLOCKS[index.min(SPARK_BLOCKS.len() - 1)]
        })
        .collect()
}

fn bar_chart(props: &Value) -> UiNode {
    let Some(Value::Array(items)) = prop_value(props, "data") else {
        return UiNode::Placeholder("BarChart".to_owned());
    };
    let entries: Vec<(String, f64)> = items
        .iter()
        .filter_map(|entry| {
            let object = entry.as_object()?;
            Some((
                object.get("label")?.as_str()?.to_owned(),
                object.get("value")?.as_f64()?,
            ))
        })
        .collect();
    if entries.is_empty() {
        return UiNode::Placeholder("BarChart".to_owned());
    }
    let max = entries
        .iter()
        .map(|(_, value)| *value)
        .fold(1.0_f64, f64::max);
    let rows = entries
        .into_iter()
        .map(|(label, value)| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let filled = ((value / max) * 24.0).round() as usize;
            UiNode::Text {
                spans: vec![
                    (format!("{label:<12} "), Style::new()),
                    (
                        "█".repeat(filled),
                        Style::new().fg(rstui_core::Color::Green),
                    ),
                    (
                        format!(" {value}"),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                ],
                variant: TextVariant::Body,
                align: Alignment::Left,
                wrap: false,
            }
        })
        .collect();
    UiNode::Column {
        children: rows,
        justify: Justify::Start,
        align: CrossAlign::Stretch,
    }
}

fn table(props: &Value) -> UiNode {
    let columns: Vec<(String, String)> = prop_value(props, "columns")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|column| {
                    let object = column.as_object()?;
                    Some((
                        object.get("header")?.as_str()?.to_owned(),
                        object.get("key")?.as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let rows = prop_value(props, "rows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if columns.is_empty() {
        return UiNode::Placeholder("Table".to_owned());
    }

    let header = UiNode::Text {
        spans: columns
            .iter()
            .map(|(header, _)| {
                (
                    format!("{header:<14}"),
                    Style::new().add_modifier(Modifier::BOLD),
                )
            })
            .collect(),
        variant: TextVariant::Body,
        align: Alignment::Left,
        wrap: false,
    };
    let mut grid = vec![header];
    for row in &rows {
        let spans = columns
            .iter()
            .map(|(_, key)| {
                let cell = row
                    .as_object()
                    .and_then(|object| object.get(key))
                    .map_or_else(|| "—".to_owned(), coerce_to_string);
                (format!("{cell:<14}"), Style::new())
            })
            .collect();
        grid.push(UiNode::Text {
            spans,
            variant: TextVariant::Body,
            align: Alignment::Left,
            wrap: false,
        });
    }
    UiNode::Card {
        title: None,
        child: Box::new(UiNode::Column {
            children: grid,
            justify: Justify::Start,
            align: CrossAlign::Stretch,
        }),
    }
}

fn list(props: &Value) -> UiNode {
    let items = prop_value(props, "items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let ordered = prop_bool(props, "ordered");
    let bullet = prop_str(props, "bulletChar").unwrap_or("•").to_owned();
    let children = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let marker = if ordered {
                format!("{}. ", index + 1)
            } else {
                format!("{bullet} ")
            };
            UiNode::Text {
                spans: vec![
                    (marker, Style::new().add_modifier(Modifier::DIM)),
                    (coerce_to_string(item), Style::new()),
                ],
                variant: TextVariant::Body,
                align: Alignment::Left,
                wrap: false,
            }
        })
        .collect();
    UiNode::Column {
        children,
        justify: Justify::Start,
        align: CrossAlign::Stretch,
    }
}

fn timeline(props: &Value) -> UiNode {
    let items = prop_value(props, "items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let children = items
        .iter()
        .map(|item| {
            let object = item.as_object();
            let title = object
                .and_then(|map| map.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let date = object
                .and_then(|map| map.get("date"))
                .and_then(Value::as_str);
            let (dot, color) = match object
                .and_then(|map| map.get("status"))
                .and_then(Value::as_str)
            {
                Some("completed") => ('●', rstui_core::Color::Green),
                Some("current") => ('◆', rstui_core::Color::Cyan),
                _ => ('○', rstui_core::Color::Gray),
            };
            let mut spans = vec![
                (format!("{dot} "), Style::new().fg(color)),
                (title.to_owned(), Style::new().add_modifier(Modifier::BOLD)),
            ];
            if let Some(date) = date {
                spans.push((
                    format!("  {date}"),
                    Style::new().add_modifier(Modifier::DIM),
                ));
            }
            UiNode::Text {
                spans,
                variant: TextVariant::Body,
                align: Alignment::Left,
                wrap: false,
            }
        })
        .collect();
    UiNode::Column {
        children,
        justify: Justify::Start,
        align: CrossAlign::Stretch,
    }
}

/// `Select`/`MultiSelect` → a column of [`UiNode::Button`]s (one per
/// option), with the selected option(s) accented. Each button routes a
/// `select:<value>` sub-event so the reducer can apply the bound write.
fn select(props: &Value, node_id: &str, _on: &serde_json::Map<String, Value>) -> UiNode {
    let options = prop_value(props, "options")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let selected_one = prop_str(props, "value").map(str::to_owned);
    let selected_many: Vec<String> = prop_value(props, "value")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let label = prop_str(props, "label").map(str::to_owned);

    let mut children = Vec::new();
    if let Some(name) = label {
        children.push(one_styled(
            format!("{name}:"),
            Style::new().add_modifier(Modifier::BOLD),
        ));
    }
    for option in &options {
        let Some(object) = option.as_object() else {
            continue;
        };
        let value = object.get("value").and_then(Value::as_str).unwrap_or("");
        let text = object
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or(value)
            .to_owned();
        let is_selected =
            selected_one.as_deref() == Some(value) || selected_many.iter().any(|v| v == value);
        children.push(UiNode::Button {
            id: format!("{node_id}#select:{value}"),
            label: if is_selected {
                format!("[x] {text}")
            } else {
                format!("[ ] {text}")
            },
            primary: is_selected,
            disabled: false,
            focused: false,
        });
    }
    UiNode::Column {
        children,
        justify: Justify::Start,
        align: CrossAlign::Stretch,
    }
}

/// `Tabs` → a header row of the tab labels (active accented) above the
/// child content (the reference renders children below the bar; `visible`
/// conditions on children gate the active tab's content).
fn tabs(props: &Value, children: Vec<UiNode>) -> UiNode {
    let tabs = prop_value(props, "tabs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let active = prop_str(props, "value").map(str::to_owned);
    let header_spans = tabs
        .iter()
        .filter_map(|tab| {
            let object = tab.as_object()?;
            let value = object.get("value")?.as_str()?;
            let label = object.get("label")?.as_str()?;
            let is_active = active.as_deref() == Some(value);
            let style = if is_active {
                Style::new()
                    .fg(rstui_core::Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().add_modifier(Modifier::DIM)
            };
            Some((format!("{label}  "), style))
        })
        .collect::<Vec<_>>();
    let header = UiNode::Text {
        spans: header_spans,
        variant: TextVariant::Body,
        align: Alignment::Left,
        wrap: false,
    };
    let mut column = vec![header];
    column.extend(children);
    UiNode::Column {
        children: column,
        justify: Justify::Start,
        align: CrossAlign::Stretch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrender::directives::DirectiveRegistry;
    use crate::jsonrender::expr::ComputedFn;
    use crate::jsonrender::spec::spec_from_value;
    use crate::value::DataModel;
    use serde_json::json;

    fn project_json(spec: &Value, state: Value) -> UiNode {
        let model = DataModel::from_root(state);
        let functions: std::collections::BTreeMap<String, ComputedFn> =
            std::collections::BTreeMap::new();
        let registry = DirectiveRegistry::with_builtins();
        let scope = ResolveScope::new(&model, &functions, &registry);
        project(&spec_from_value(spec), &scope, Loading(false))
    }

    #[test]
    fn box_text_and_heading_project() {
        let node = project_json(
            &json!({
                "root": "root",
                "elements": {
                    "root": { "type": "Box", "props": { "flexDirection": "column" }, "children": ["h", "t"] },
                    "h": { "type": "Heading", "props": { "text": "Title", "level": "h1" } },
                    "t": { "type": "Text", "props": { "text": "Body", "bold": true } },
                },
            }),
            json!({}),
        );
        assert_eq!(node.to_plain(), "Title Body");
        match node {
            UiNode::Column { children, .. } => {
                assert!(matches!(
                    children[0],
                    UiNode::Text {
                        variant: TextVariant::H1,
                        ..
                    }
                ));
            }
            other => panic!("expected Column, got {other:?}"),
        }
    }

    #[test]
    fn state_binding_and_visibility_drive_projection() {
        let spec = json!({
            "root": "root",
            "elements": {
                "root": { "type": "Box", "children": ["msg", "hidden"] },
                "msg": { "type": "Text", "props": { "text": { "$state": "/greeting" } } },
                "hidden": {
                    "type": "Text",
                    "props": { "text": "secret" },
                    "visible": { "$state": "/show", "eq": true },
                },
            },
        });
        let visible = project_json(&spec, json!({ "greeting": "Hi Ada", "show": false }));
        assert_eq!(visible.to_plain(), "Hi Ada ");
        let shown = project_json(&spec, json!({ "greeting": "Hey", "show": true }));
        assert_eq!(shown.to_plain(), "Hey secret");
    }

    #[test]
    fn repeat_renders_children_per_item_with_item_scope() {
        let node = project_json(
            &json!({
                "root": "list",
                "elements": {
                    "list": { "type": "Box", "repeat": { "statePath": "/todos" }, "children": ["row"] },
                    "row": { "type": "Text", "props": { "text": { "$template": "#${/n} ${title}" } } },
                },
            }),
            json!({ "n": 9, "todos": [{ "title": "a" }, { "title": "b" }] }),
        );
        assert_eq!(node.to_plain(), "#9 a #9 b");
    }

    #[test]
    fn unknown_component_and_missing_child_degrade_not_panic() {
        let node = project_json(
            &json!({
                "root": "root",
                "elements": {
                    "root": { "type": "Box", "children": ["weird", "ghost"] },
                    "weird": { "type": "QuantumWidget", "props": {} },
                },
            }),
            json!({}),
        );
        let plain = node.to_plain();
        assert!(plain.contains("[unsupported: QuantumWidget]"), "{plain}");
        assert!(plain.contains("[unsupported: Missing: ghost]"), "{plain}");
    }

    #[test]
    fn interactive_textinput_carries_bind_writeback_pointer() {
        let node = project_json(
            &json!({
                "root": "f",
                "elements": {
                    "f": {
                        "type": "TextInput",
                        "props": { "label": "Email", "value": { "$bindState": "/form/email" } },
                    },
                },
            }),
            json!({ "form": { "email": "a@b.c" } }),
        );
        match node {
            UiNode::TextField {
                id, label, value, ..
            } => {
                assert_eq!(id, "/form/email"); // write-back pointer
                assert_eq!(label, "Email");
                assert_eq!(value, "a@b.c");
            }
            other => panic!("expected TextField, got {other:?}"),
        }
    }

    #[test]
    fn empty_root_is_a_total_placeholder() {
        let node = project_json(&json!({ "elements": {} }), json!({}));
        assert!(matches!(node, UiNode::Placeholder(text) if text.is_empty()));
    }

    #[test]
    fn cyclic_children_degrade_to_placeholder_not_overflow() {
        // a → b → a … : the recursion guard caps it.
        let node = project_json(
            &json!({
                "root": "a",
                "elements": {
                    "a": { "type": "Box", "children": ["b"] },
                    "b": { "type": "Box", "children": ["a"] },
                },
            }),
            json!({}),
        );
        assert!(node.to_plain().contains("[unsupported: cycle]"));
    }
}
