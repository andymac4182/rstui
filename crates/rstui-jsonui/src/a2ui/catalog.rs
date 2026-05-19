//! The A2UI basic-catalog component model and its projection onto
//! [`UiNode`].
//!
//! # The adjacency list, resolved by id
//!
//! `updateComponents` delivers a **flat** list of `{id, component, …}`
//! objects; containers reference children **by id string**, never
//! inline. [`Component`] is one such entry (its raw props kept as a
//! [`Value`] so an unknown prop is preserved, not dropped).
//! [`project`] walks from `root` through a [`ComponentMap`], resolving
//! each `Dynamic*` prop against the caller-owned
//! [`DataModel`] (relative-scoped for template
//! instances) and emitting a fresh `UiNode` tree every frame — there is
//! no retained widget tree (ADR 0012).
//!
//! # The 18 basic-catalog components + the rstui chart extension
//!
//! `Text`, `Image`, `Icon`, `Video`, `AudioPlayer`, `Row`, `Column`,
//! `List`, `Card`, `Tabs`, `Modal`, `Divider`, `Button`, `TextField`,
//! `CheckBox`, `ChoicePicker`, `Slider`, `DateTimeInput`
//! (`basic_catalog.json`), plus the rstui chart extension
//! (`BarChart`/`LineChart`/`PieChart`/`Sparkline`/`ScatterPlot`/
//! `Histogram`/`StackedBarChart`/`Heatmap`) projecting to a themed
//! [`UiNode::Chart`] via the shared [`crate::chart`] builder. `Tabs`
//! headers are interactive `Button`s (`<id>#tab:<n>`); a `"color"`
//! prop is a theme token (raw `#hex` fallback). A terminal-incapable
//! component (`Image`/`Video`/`AudioPlayer`) becomes
//! [`UiNode::Media`], an unknown/missing one [`UiNode::Placeholder`] —
//! the progressive-rendering contract, never a panic.
//!
//! # `ChildList`
//!
//! Children are either a **static** `["id", …]` array or a **template**
//! `{"componentId": id, "path": ptr}`: the data-model array at `path` is
//! iterated and the template component is instantiated once per element
//! with a child scope of `path/<index>`, so the element's props bind
//! relatively (the reference `generic-binder.ts` STRUCTURAL rule). A
//! node budget ([`MAX_PROJECTION_NODES`]) bounds a fan-out-explosive id
//! graph and a depth cap ([`MAX_PROJECTION_DEPTH`]) bounds a cyclic /
//! mutually-referential one, so a hostile document can neither hang nor
//! overflow the stack — it degrades to a truncated placeholder.

use std::collections::HashMap;

use serde_json::Value;

use crate::tree::{ChartKind, CrossAlign, Justify, KeyValueRow, NodeId, TextVariant, UiNode};
use crate::value::{DataModel, resolve_scope};

use super::binding::{coerce_text, resolve, resolve_bool, resolve_number, resolve_text, truthy};

/// The instantiation budget for one [`project`] call: an upper bound on
/// emitted nodes so a fan-out-explosive id graph degrades to a truncated
/// render instead of hanging (totality over a hostile adjacency list).
pub const MAX_PROJECTION_NODES: usize = 5_000;

/// The maximum container-nesting depth one [`project`] call descends.
/// A self-/mutually-referential id graph (`root` → `["root"]`) would
/// recurse unboundedly; this caps the *stack* depth (well below any
/// overflow threshold, far deeper than any real UI) so a cycle degrades
/// to a placeholder rather than crashing — the node budget alone bounds
/// breadth but not recursion depth.
pub const MAX_PROJECTION_DEPTH: usize = 128;

/// One entry of the surface adjacency list: an id, its `component` type
/// (absent only in a malformed message), and the remaining raw
/// properties (kept whole so an unknown prop is preserved).
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    /// The component type name (`"Column"`, `"Button"`, …); empty for a
    /// component an update referenced without a type.
    pub kind: String,
    /// The raw `{…}` properties (everything but `id`/`component`).
    pub properties: Value,
}

impl Component {
    /// Builds a [`Component`] from one `updateComponents` entry. A
    /// non-object entry yields an empty-kind placeholder component
    /// (totality).
    #[must_use]
    pub fn from_entry(entry: &Value) -> Self {
        let Some(fields) = entry.as_object() else {
            return Self {
                kind: String::new(),
                properties: Value::Null,
            };
        };
        let kind = fields
            .get("component")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let mut properties = fields.clone();
        properties.remove("id");
        properties.remove("component");
        Self {
            kind,
            properties: Value::Object(properties),
        }
    }

    /// A raw property by name (`None` if absent).
    #[must_use]
    pub fn prop(&self, name: &str) -> Option<&Value> {
        self.properties.as_object().and_then(|map| map.get(name))
    }
}

/// The surface's `id → component` adjacency list.
pub type ComponentMap = HashMap<NodeId, Component>;

/// Caller-owned per-surface selection state for the stateful catalog
/// components (`Tabs` active index, open `Modal` ids). It lives on the
/// surface so the projection stays a pure function of state (ADR 0012);
/// the reducer mutates it, the projection only reads it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SelectionState {
    /// `Tabs` component id → selected tab index.
    pub active_tab: HashMap<NodeId, usize>,
    /// Ids of currently-open `Modal` components.
    pub open_modals: Vec<NodeId>,
}

impl SelectionState {
    /// The selected tab index for a `Tabs` id (0 when unset).
    #[must_use]
    pub fn tab(&self, id: &str) -> usize {
        self.active_tab.get(id).copied().unwrap_or(0)
    }

    /// Whether a `Modal` id is currently open.
    #[must_use]
    pub fn modal_open(&self, id: &str) -> bool {
        self.open_modals.iter().any(|open| open == id)
    }
}

/// The focus/edit projection inputs the reducer owns (which interactive
/// node is focused; the in-progress text-field buffer). Kept caller-side
/// so the widget never mutates — the pure-projection model.
#[derive(Debug, Clone, Default)]
pub struct InteractionState {
    /// The currently-focused interactive component id, if any.
    pub focused: Option<NodeId>,
}

impl InteractionState {
    fn is_focused(&self, id: &str) -> bool {
        self.focused.as_deref() == Some(id)
    }
}

/// The bounded recursion cursor for one [`project`] walk: the immutable
/// surface inputs plus the remaining node budget and the current
/// container depth. Bundling them keeps every recursive call a single
/// argument and makes the breadth ([`MAX_PROJECTION_NODES`]) and depth
/// ([`MAX_PROJECTION_DEPTH`]) guards one-liners.
struct Walk<'a> {
    components: &'a ComponentMap,
    model: &'a DataModel,
    selection: &'a SelectionState,
    interaction: &'a InteractionState,
    /// The active theme-token colour palette (a `"color"` prop / chart
    /// series resolves against it).
    palette: &'a crate::color::Palette,
    /// Remaining nodes that may still be emitted (breadth bound).
    budget: usize,
    /// Containers descended so far on this path (stack-depth bound).
    depth: usize,
}

/// Process-wide default palette so the 4-arg [`project`] keeps its
/// signature (existing callers / tests unchanged); a themed host calls
/// [`project_with_palette`].
static DEFAULT_PALETTE: crate::color::Palette = crate::color::Palette::ANSI;

/// Projects the surface rooted at `root` into a [`UiNode`].
///
/// `model` is the caller-owned data model; `selection`/`interaction`
/// carry the reducer-owned tab/modal/focus state. A missing `root`, a
/// dangling child id, an unknown component, or budget/depth exhaustion
/// all degrade to a [`UiNode::Placeholder`] (totality / progressive
/// rendering).
#[must_use]
pub fn project(
    components: &ComponentMap,
    model: &DataModel,
    selection: &SelectionState,
    interaction: &InteractionState,
) -> UiNode {
    project_with_palette(components, model, selection, interaction, &DEFAULT_PALETTE)
}

/// [`project`] with an explicit theme-token palette (a `"color"` prop /
/// chart series resolves against it). `A2uiSurface::project` passes its
/// host-supplied palette here.
#[must_use]
pub fn project_with_palette(
    components: &ComponentMap,
    model: &DataModel,
    selection: &SelectionState,
    interaction: &InteractionState,
    palette: &crate::color::Palette,
) -> UiNode {
    let mut walk = Walk {
        components,
        model,
        selection,
        interaction,
        palette,
        budget: MAX_PROJECTION_NODES,
        depth: 0,
    };
    project_id("root", "", &mut walk)
}

fn project_id(id: &str, scope: &str, walk: &mut Walk<'_>) -> UiNode {
    // A cyclic / mutually-referential id graph would recurse forever;
    // the depth cap terminates it well below any stack-overflow point,
    // the node budget bounds a fan-out-explosive (but acyclic) graph.
    if walk.budget == 0 || walk.depth >= MAX_PROJECTION_DEPTH {
        return UiNode::Placeholder("…(truncated)".to_owned());
    }
    walk.budget -= 1;
    let Some(component) = walk.components.get(id) else {
        return UiNode::Placeholder(format!("missing:{id}"));
    };
    // The component map is borrowed for the whole arm; clone the small
    // bits the recursive arms need so `walk` can be re-borrowed mutably.
    let kind = component.kind.clone();
    let properties = component.properties.clone();
    let prop = |name: &str| properties.as_object().and_then(|map| map.get(name));
    let model = walk.model;
    let interaction = walk.interaction;
    match kind.as_str() {
        "Text" => {
            let text = prop("text")
                .map(|raw| resolve_text(raw, model, scope))
                .unwrap_or_default();
            let variant = text_variant(prop("variant"));
            // An optional `"color"` is a theme token (`success`,
            // `chart2`, …) or a raw `#hex`/named fallback, resolved
            // against the active palette.
            let style = prop("color")
                .and_then(Value::as_str)
                .and_then(crate::color::parse_token)
                .map_or_else(rstui_core::Style::new, |token| {
                    rstui_core::Style::new().fg(walk.palette.resolve(token))
                });
            // Simple-markdown-capable: bold/italic survive Markdown.
            if text.contains('*') || text.contains('`') || text.contains('_') {
                UiNode::Markdown(text)
            } else {
                UiNode::Text {
                    spans: vec![(text, style)],
                    variant,
                    align: rstui_core::Alignment::Left,
                    wrap: true,
                }
            }
        }
        "Image" => media("image", prop("description"), model, scope),
        "Video" => media("video", prop("description"), model, scope),
        "AudioPlayer" => media("audio", prop("description"), model, scope),
        "Icon" => {
            let name = match prop("name") {
                Some(Value::String(text)) => text.clone(),
                Some(other) => coerce_text(other),
                None => String::new(),
            };
            UiNode::Text {
                spans: vec![(format!("[{name}]"), rstui_core::Style::new())],
                variant: TextVariant::Body,
                align: rstui_core::Alignment::Left,
                wrap: false,
            }
        }
        "Row" => {
            let children = child_nodes(prop("children"), scope, walk);
            UiNode::Row {
                children,
                justify: justify_of(prop("justify")),
                align: cross_align_of(prop("align")),
            }
        }
        "Column" => {
            let children = child_nodes(prop("children"), scope, walk);
            UiNode::Column {
                children,
                justify: justify_of(prop("justify")),
                align: cross_align_of(prop("align")),
            }
        }
        "List" => {
            let children = child_nodes(prop("children"), scope, walk);
            let horizontal = prop("direction").and_then(Value::as_str) == Some("horizontal");
            if horizontal {
                UiNode::Row {
                    children,
                    justify: Justify::Start,
                    align: cross_align_of(prop("align")),
                }
            } else {
                UiNode::Column {
                    children,
                    justify: Justify::Start,
                    align: cross_align_of(prop("align")),
                }
            }
        }
        "Card" => {
            let child = match prop("child").and_then(Value::as_str) {
                Some(child_id) => {
                    walk.depth += 1;
                    let node = project_id(child_id, scope, walk);
                    walk.depth -= 1;
                    node
                }
                None => UiNode::Placeholder("Card(no child)".to_owned()),
            };
            UiNode::Card {
                title: None,
                child: Box::new(child),
            }
        }
        "Tabs" => project_tabs(id, prop("tabs"), scope, walk),
        "Modal" => {
            // The trigger is always rendered; the content overlays only
            // while the reducer has marked the modal open.
            let mut layers = Vec::new();
            walk.depth += 1;
            if let Some(trigger) = prop("trigger").and_then(Value::as_str) {
                layers.push(project_id(trigger, scope, walk));
            }
            if walk.selection.modal_open(id) {
                if let Some(content) = prop("content").and_then(Value::as_str) {
                    layers.push(project_id(content, scope, walk));
                }
            }
            walk.depth -= 1;
            if layers.is_empty() {
                UiNode::Placeholder("Modal".to_owned())
            } else {
                UiNode::Stack(layers)
            }
        }
        "Divider" => UiNode::Divider {
            vertical: prop("axis").and_then(Value::as_str) == Some("vertical"),
            label: None,
        },
        "Button" => {
            let label = match prop("child").and_then(Value::as_str) {
                Some(child_id) => {
                    walk.depth += 1;
                    let node = project_id(child_id, scope, walk);
                    walk.depth -= 1;
                    node.to_plain()
                }
                None => String::new(),
            };
            let variant = prop("variant").and_then(Value::as_str).unwrap_or("default");
            let disabled = !checks_pass(prop("checks"), model, scope);
            UiNode::Button {
                id: id.to_owned(),
                label,
                primary: variant == "primary",
                disabled,
                focused: interaction.is_focused(id),
            }
        }
        "TextField" => {
            let masked = prop("variant").and_then(Value::as_str) == Some("obscured");
            UiNode::TextField {
                id: id.to_owned(),
                label: prop("label")
                    .map(|raw| resolve_text(raw, model, scope))
                    .unwrap_or_default(),
                value: prop("value")
                    .map(|raw| resolve_text(raw, model, scope))
                    .unwrap_or_default(),
                placeholder: prop("placeholder")
                    .map(|raw| resolve_text(raw, model, scope))
                    .unwrap_or_default(),
                masked,
                focused: interaction.is_focused(id),
            }
        }
        "DateTimeInput" => UiNode::TextField {
            id: id.to_owned(),
            label: prop("label")
                .map(|raw| resolve_text(raw, model, scope))
                .unwrap_or_else(|| "Date/Time".to_owned()),
            value: prop("value")
                .map(|raw| resolve_text(raw, model, scope))
                .unwrap_or_default(),
            placeholder: "YYYY-MM-DD".to_owned(),
            masked: false,
            focused: interaction.is_focused(id),
        },
        "CheckBox" => UiNode::Checkbox {
            id: id.to_owned(),
            label: prop("label")
                .map(|raw| resolve_text(raw, model, scope))
                .unwrap_or_default(),
            checked: prop("value").is_some_and(|raw| resolve_bool(raw, model, scope)),
            focused: interaction.is_focused(id),
        },
        "ChoicePicker" => project_choice_picker(id, &properties, scope, model, interaction),
        "Slider" => {
            let value = prop("value")
                .and_then(|raw| resolve_number(raw, model, scope))
                .unwrap_or(0.0);
            let min = prop("min").and_then(Value::as_f64).unwrap_or(0.0);
            let max = prop("max").and_then(Value::as_f64).unwrap_or(1.0);
            let span = max - min;
            let ratio = if span.abs() < f64::EPSILON {
                0.0
            } else {
                ((value - min) / span).clamp(0.0, 1.0)
            };
            let label = prop("label")
                .map(|raw| resolve_text(raw, model, scope))
                .filter(|text| !text.is_empty())
                .map(|text| format!("{text}: {value}"))
                .unwrap_or_else(|| value.to_string());
            UiNode::Gauge {
                ratio,
                label: Some(label),
            }
        }
        "BarChart" | "Bar" => a2ui_chart(ChartKind::Bar, &properties, walk.palette),
        "LineChart" | "Line" => a2ui_chart(ChartKind::Line, &properties, walk.palette),
        "AreaChart" | "Area" => a2ui_chart(ChartKind::Area, &properties, walk.palette),
        "PieChart" | "Pie" | "DonutChart" | "Donut" => {
            a2ui_chart(ChartKind::Pie, &properties, walk.palette)
        }
        "Sparkline" => a2ui_chart(ChartKind::Sparkline, &properties, walk.palette),
        "ScatterPlot" | "Scatter" => a2ui_chart(ChartKind::Scatter, &properties, walk.palette),
        "Histogram" => a2ui_chart(ChartKind::Histogram, &properties, walk.palette),
        "StackedBarChart" | "StackedBar" => {
            a2ui_chart(ChartKind::StackedBar, &properties, walk.palette)
        }
        "Heatmap" => a2ui_chart(ChartKind::Heatmap, &properties, walk.palette),
        "" => UiNode::Placeholder(format!("untyped:{id}")),
        other => UiNode::Placeholder(other.to_owned()),
    }
}

/// Project an A2UI chart component (an rstui extension to the basic
/// catalog) into a themed [`UiNode::Chart`], delegating to the shared
/// [`crate::chart::build_chart`] so it parses identical data shapes /
/// palette rules to the json-render path.
fn a2ui_chart(kind: ChartKind, props: &Value, palette: &crate::color::Palette) -> UiNode {
    let num = |name: &str| props.as_object().and_then(|map| map.get(name))?.as_f64();
    let val = |name: &str| props.as_object().and_then(|map| map.get(name));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let height = num("height").map_or(10, |h| h as u16).clamp(3, 40);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let cols = num("cols").map_or(0, |c| c as usize);
    crate::chart::build_chart(
        kind,
        val("series"),
        val("data"),
        val("label").and_then(Value::as_str),
        val("color")
            .and_then(Value::as_str)
            .and_then(crate::color::parse_token),
        height,
        cols,
        palette,
    )
}

/// Resolves a `ChildList` — a static `["id", …]` array or a
/// `{componentId, path}` template — to projected child nodes, descending
/// one container level (the depth guard is enforced in `project_id`).
fn child_nodes(child_list: Option<&Value>, scope: &str, walk: &mut Walk<'_>) -> Vec<UiNode> {
    walk.depth += 1;
    let children = match child_list {
        // Static list: ["id", …].
        Some(Value::Array(ids)) => ids
            .iter()
            .filter_map(Value::as_str)
            .map(|child_id| project_id(child_id, scope, walk))
            .collect(),
        // Template: {componentId, path} — instantiate once per element
        // of the data-model array at `path`, scoped to `path/<index>`.
        Some(Value::Object(template)) => {
            match (
                template.get("componentId").and_then(Value::as_str),
                template.get("path").and_then(Value::as_str),
            ) {
                (Some(component_id), Some(path)) => {
                    let absolute = resolve_scope(scope, path);
                    let count = walk
                        .model
                        .get(&absolute)
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    (0..count)
                        .map(|index| {
                            let item_scope = format!("{absolute}/{index}");
                            project_id(component_id, &item_scope, walk)
                        })
                        .collect()
                }
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    };
    walk.depth -= 1;
    children
}

fn project_tabs(id: &str, tabs: Option<&Value>, scope: &str, walk: &mut Walk<'_>) -> UiNode {
    let Some(Value::Array(entries)) = tabs else {
        return UiNode::Placeholder("Tabs".to_owned());
    };
    if entries.is_empty() {
        return UiNode::Placeholder("Tabs(empty)".to_owned());
    }
    let active = walk.selection.tab(id).min(entries.len() - 1);
    // A header row of tab titles, then the selected tab's child below.
    // Each title is an interactive `Button` carrying the sub-id
    // `"<tabsId>#tab:<index>"` so a click / keyboard activation can
    // switch the reducer-owned active tab (the A2UI selection model —
    // the reducer maps that id to `selection_mut().active_tab`). The
    // active tab is `primary`; the `[*]`/`[ ]` marker is kept in the
    // label so the plain-text projection is unchanged.
    let titles: Vec<UiNode> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let title = entry
                .get("title")
                .map(|raw| resolve_text(raw, walk.model, scope))
                .unwrap_or_default();
            let marker = if index == active { "[*]" } else { "[ ]" };
            UiNode::Button {
                id: format!("{id}#tab:{index}"),
                label: format!("{marker} {title}"),
                primary: index == active,
                disabled: false,
                focused: false,
            }
        })
        .collect();
    let child = match entries[active].get("child").and_then(Value::as_str) {
        Some(child_id) => {
            walk.depth += 1;
            let node = project_id(child_id, scope, walk);
            walk.depth -= 1;
            node
        }
        None => UiNode::Placeholder("Tab(no child)".to_owned()),
    };
    UiNode::Column {
        children: vec![
            UiNode::Row {
                children: titles,
                justify: Justify::Start,
                align: CrossAlign::Start,
            },
            child,
        ],
        justify: Justify::Start,
        align: CrossAlign::Stretch,
    }
}

fn project_choice_picker(
    id: &str,
    properties: &Value,
    scope: &str,
    model: &DataModel,
    interaction: &InteractionState,
) -> UiNode {
    let pick = |name: &str| properties.as_object().and_then(|map| map.get(name));
    let selected: Vec<String> = match pick("value").map(|raw| resolve(raw, model, scope)) {
        Some(Value::Array(items)) => items.iter().map(coerce_text).collect(),
        Some(Value::String(text)) => vec![text],
        _ => Vec::new(),
    };
    let mut rows = Vec::new();
    if let Some(label) = pick("label") {
        let text = resolve_text(label, model, scope);
        if !text.is_empty() {
            rows.push(UiNode::Text {
                spans: vec![(text, rstui_core::Style::new())],
                variant: TextVariant::Caption,
                align: rstui_core::Alignment::Left,
                wrap: false,
            });
        }
    }
    if let Some(Value::Array(options)) = pick("options") {
        for (index, option) in options.iter().enumerate() {
            let value = option
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let label = option
                .get("label")
                .map(|raw| resolve_text(raw, model, scope))
                .unwrap_or_else(|| value.clone());
            rows.push(UiNode::Checkbox {
                id: format!("{id}/{index}"),
                label,
                checked: selected.iter().any(|chosen| chosen == &value),
                focused: interaction.is_focused(&format!("{id}/{index}")),
            });
        }
    }
    if rows.is_empty() {
        return UiNode::Placeholder("ChoicePicker".to_owned());
    }
    UiNode::Column {
        children: rows,
        justify: Justify::Start,
        align: CrossAlign::Stretch,
    }
}

fn media(kind: &str, description: Option<&Value>, model: &DataModel, scope: &str) -> UiNode {
    UiNode::Media {
        kind: kind.to_owned(),
        alt: description
            .map(|raw| resolve_text(raw, model, scope))
            .unwrap_or_default(),
    }
}

/// Evaluates a `Checkable.checks` array: every `condition` must be
/// truthy for the control to be enabled. Absent/empty ⇒ enabled.
fn checks_pass(checks: Option<&Value>, model: &DataModel, scope: &str) -> bool {
    let Some(Value::Array(rules)) = checks else {
        return true;
    };
    rules.iter().all(|rule| {
        rule.get("condition")
            .map(|condition| truthy(&resolve(condition, model, scope)))
            .unwrap_or(true)
    })
}

fn text_variant(variant: Option<&Value>) -> TextVariant {
    match variant.and_then(Value::as_str) {
        Some("h1") => TextVariant::H1,
        Some("h2") => TextVariant::H2,
        Some("h3") => TextVariant::H3,
        // No H5 in the target enum: the smallest heading maps to H4.
        Some("h4" | "h5") => TextVariant::H4,
        Some("caption") => TextVariant::Caption,
        _ => TextVariant::Body,
    }
}

fn justify_of(value: Option<&Value>) -> Justify {
    match value.and_then(Value::as_str) {
        Some("center") => Justify::Center,
        Some("end") => Justify::End,
        Some("spaceBetween") => Justify::SpaceBetween,
        Some("spaceAround" | "spaceEvenly") => Justify::SpaceAround,
        Some("stretch") => Justify::Stretch,
        _ => Justify::Start,
    }
}

fn cross_align_of(value: Option<&Value>) -> CrossAlign {
    match value.and_then(Value::as_str) {
        Some("start") => CrossAlign::Start,
        Some("center") => CrossAlign::Center,
        Some("end") => CrossAlign::End,
        _ => CrossAlign::Stretch,
    }
}

/// A compact key→value pane projected from a JSON object in the data
/// model — a convenience for an agent that binds a flat record (not part
/// of the catalog grammar, but a common shape worth a clean projection).
#[must_use]
pub fn key_value_of(object: &Value) -> UiNode {
    match object {
        Value::Object(map) => UiNode::KeyValue(
            map.iter()
                .map(|(key, value)| KeyValueRow {
                    key: key.clone(),
                    value: coerce_text(value),
                })
                .collect(),
        ),
        _ => UiNode::Placeholder("KeyValue".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map_of(entries: &[Value]) -> ComponentMap {
        let mut map = ComponentMap::new();
        for entry in entries {
            if let Some(id) = entry.get("id").and_then(Value::as_str) {
                map.insert(id.to_owned(), Component::from_entry(entry));
            }
        }
        map
    }

    #[test]
    fn column_text_button_projects_and_hits() {
        let components = map_of(&[
            json!({"id": "root", "component": "Column", "children": ["title", "go"]}),
            json!({"id": "title", "component": "Text", "text": "Hello", "variant": "h1"}),
            json!({"id": "go_label", "component": "Text", "text": "Go"}),
            json!({"id": "go", "component": "Button", "child": "go_label",
                   "variant": "primary", "action": {"event": {"name": "go"}}}),
        ]);
        let node = project(
            &components,
            &DataModel::new(),
            &SelectionState::default(),
            &InteractionState::default(),
        );
        assert_eq!(node.to_plain(), "Hello Go");
        if let UiNode::Column { children, .. } = &node {
            assert!(matches!(
                children[0],
                UiNode::Text {
                    variant: TextVariant::H1,
                    ..
                }
            ));
            assert!(matches!(
                &children[1],
                UiNode::Button {
                    primary: true,
                    disabled: false,
                    ..
                }
            ));
        } else {
            panic!("expected a Column root");
        }
    }

    #[test]
    fn dynamic_binding_and_template_children() {
        let model = DataModel::from_root(json!({
            "name": "Ada",
            "todos": [{ "task": "write" }, { "task": "test" }]
        }));
        let components = map_of(&[
            json!({"id": "root", "component": "Column", "children": ["greet", "list"]}),
            json!({"id": "greet", "component": "Text", "text": {"path": "/name"}}),
            json!({"id": "list", "component": "Column",
                   "children": {"componentId": "row", "path": "/todos"}}),
            json!({"id": "row", "component": "Text", "text": {"path": "task"}}),
        ]);
        let node = project(
            &components,
            &model,
            &SelectionState::default(),
            &InteractionState::default(),
        );
        // greeting bound + two template instances each scoped to its index
        assert_eq!(node.to_plain(), "Ada write test");
    }

    #[test]
    fn button_disabled_when_a_check_fails() {
        let model = DataModel::from_root(json!({ "email": "bad" }));
        let components = map_of(&[
            json!({"id": "root", "component": "Button", "child": "l",
            "action": {"event": {"name": "submit"}},
            "checks": [{
                "condition": {"call": "email", "args": {"value": {"path": "/email"}}},
                "message": "bad email"
            }]}),
            json!({"id": "l", "component": "Text", "text": "Submit"}),
        ]);
        let node = project(
            &components,
            &model,
            &SelectionState::default(),
            &InteractionState::default(),
        );
        assert!(matches!(node, UiNode::Button { disabled: true, .. }));
    }

    #[test]
    fn tabs_render_selected_child() {
        let components = map_of(&[
            json!({"id": "root", "component": "Tabs", "tabs": [
                {"title": "One", "child": "a"},
                {"title": "Two", "child": "b"}
            ]}),
            json!({"id": "a", "component": "Text", "text": "First"}),
            json!({"id": "b", "component": "Text", "text": "Second"}),
        ]);
        let mut selection = SelectionState::default();
        selection.active_tab.insert("root".to_owned(), 1);
        let node = project(
            &components,
            &DataModel::new(),
            &selection,
            &InteractionState::default(),
        );
        // selected (Two) child is shown, not the first
        assert!(node.to_plain().contains("Second"));
        assert!(!node.to_plain().contains("First"));
    }

    #[test]
    fn totality_missing_root_dangling_ref_and_cycle() {
        // missing root
        assert!(matches!(
            project(
                &ComponentMap::new(),
                &DataModel::new(),
                &SelectionState::default(),
                &InteractionState::default()
            ),
            UiNode::Placeholder(_)
        ));
        // dangling child reference
        let dangling = map_of(&[json!({"id": "root", "component": "Card", "child": "nope"})]);
        let node = project(
            &dangling,
            &DataModel::new(),
            &SelectionState::default(),
            &InteractionState::default(),
        );
        assert!(matches!(node, UiNode::Card { .. }));
        // a self-referential cycle terminates via the node budget
        let cyclic = map_of(&[json!({"id": "root", "component": "Column", "children": ["root"]})]);
        let _ = project(
            &cyclic,
            &DataModel::new(),
            &SelectionState::default(),
            &InteractionState::default(),
        );
        // unknown component → visible placeholder
        let unknown = map_of(&[json!({"id": "root", "component": "Hologram"})]);
        assert_eq!(
            project(
                &unknown,
                &DataModel::new(),
                &SelectionState::default(),
                &InteractionState::default()
            ),
            UiNode::Placeholder("Hologram".to_owned())
        );
    }

    #[test]
    fn a2ui_chart_component_projects_to_a_themed_chart_node() {
        let components = map_of(&[json!({
            "id": "root", "component": "BarChart",
            "data": [{ "label": "a", "value": 2 }, { "label": "b", "value": 5 }],
            "color": "chart3"
        })]);
        let mut palette = crate::color::Palette::ANSI;
        palette.chart[2] = rstui_core::Color::Rgb(7, 7, 7); // chart3 (1-based)
        let node = project_with_palette(
            &components,
            &DataModel::new(),
            &SelectionState::default(),
            &InteractionState::default(),
            &palette,
        );
        let UiNode::Chart { kind, series, .. } = node else {
            panic!("A2UI BarChart must project to a Chart node, got {node:?}");
        };
        assert_eq!(kind, ChartKind::Bar);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].color, rstui_core::Color::Rgb(7, 7, 7));
        assert_eq!(series[0].points, vec![(0.0, 2.0), (1.0, 5.0)]);
        assert_eq!(series[0].labels, vec!["a".to_owned(), "b".to_owned()]);
    }
}
