//! The capability descriptors this client advertises to an agent so the
//! agent knows it may stream rich UI — **and the full component catalog
//! it must target** (plus the diagram DSL it may emit).
//!
//! Both UI formats require the client to *send the catalog* to the
//! agent, not merely a name:
//!
//! - **A2UI** negotiates via transport metadata. The client sends
//!   `a2uiClientCapabilities` (`{ "v0.10": { "supportedCatalogIds":
//!   [...], "inlineCatalogs": [Catalog…] } }`). [`client_capabilities`]
//!   advertises the canonical basic-catalog id **and** ships the full,
//!   self-contained [`a2ui_inline_catalog`] (every component's JSON
//!   Schema, the catalog functions, the theme) so an agent renders
//!   exactly what this terminal supports without fetching anything.
//! - **json-render** has no wire handshake — the host hands the model a
//!   catalog + prompt. [`json_render_catalog`] is the component schema
//!   and [`json_render_prompt`] the LLM instructions; both are folded
//!   into [`render_capability_summary`] so the ACP client carries them
//!   in the `initialize` client-capabilities `_meta`.
//! - **Diagrams** ([`diagram_capability`]) are emitted as a fenced
//!   ` ```mermaid ` / ` ```structurizr ` code block — also advertised so
//!   a model answers *with* a diagram instead of describing one.
//!
//! Building the inline catalog is **total**: the canonical schema is a
//! compile-time embedded asset, and any (impossible) parse failure
//! degrades to a minimal catalog rather than panicking.

use std::sync::OnceLock;

use serde_json::{Map, Value, json};

/// The catalog id this client advertises as renderable — the upstream
/// A2UI basic-catalog id. Every component in it maps to a
/// [`UiNode`](crate::tree::UiNode), so an agent targeting the basic
/// catalog renders faithfully in the terminal.
pub const A2UI_CATALOG_ID: &str = "https://a2ui.org/specification/v0_10/basic_catalog.json";

/// The A2UI protocol version this client speaks.
pub const A2UI_VERSION: &str = "v0.10";

/// The json-render catalog identifier this client advertises (the
/// upstream "standard components" set; informational — json-render has no
/// wire negotiation).
pub const JSON_RENDER_CATALOG_ID: &str = "rstui-jsonui/json-render/standard";

/// The diagram DSL this client renders, stated as the contract an agent
/// follows to *output a diagram* — delegated to the deterministic
/// `rstui_ai::diagram::Diagram` →
/// [`Mermaid`](rstui_widgets::Mermaid)/[`Structurizr`](rstui_widgets::Structurizr)/[`JsonCanvas`](rstui_widgets::JsonCanvas)
/// renderers. Injecting this into a system prompt / ACP client-capabilities
/// lets a model answer *with* a diagram instead of describing one in prose —
/// and, via JSON Canvas, control the exact layout when it needs to.
pub const DIAGRAM_DSL_NOTE: &str = "To output a diagram, emit a fenced code block. \
    For auto-laid-out diagrams: ```mermaid … ``` for any Mermaid diagram type \
    (flowchart/graph, sequenceDiagram, classDiagram, stateDiagram-v2, erDiagram, gantt, \
    pie, gitGraph, mindmap, timeline, journey, quadrantChart, requirementDiagram, \
    sankey-beta, xychart-beta, block-beta, packet-beta, kanban, architecture-beta, \
    radar-beta, C4*, zenuml), or ```structurizr … ``` for a Structurizr DSL / C4 \
    workspace. To control the exact layout yourself (Mermaid/Structurizr are \
    auto-layout and cannot place a node at a position), emit ```canvas … ``` \
    containing JSON Canvas 1.0 — {\"nodes\":[{\"id\",\"type\":text|file|link|group,\
    \"x\",\"y\",\"width\",\"height\",\"text\"|\"file\"|\"url\"|\"label\",\"color\"}],\
    \"edges\":[{\"id\",\"fromNode\",\"toNode\",\"fromSide\",\"toSide\",\
    \"toEnd\":none|arrow,\"label\"}]} — where every node carries integer pixel \
    coordinates. All render as a deterministic terminal diagram; an unterminated \
    block still renders while streaming.";

/// The canonical A2UI v0.10 basic catalog (verbatim upstream schema,
/// vendored — see `assets/a2ui/PROVENANCE.md`).
const BASIC_CATALOG_JSON: &str = include_str!("../assets/a2ui/basic_catalog.json");
/// The canonical A2UI v0.10 shared `$defs` (`common_types.json`); merged
/// into the inline catalog so cross-file `$ref`s resolve with no fetch.
const COMMON_TYPES_JSON: &str = include_str!("../assets/a2ui/common_types.json");

/// The diagram-DSL capability descriptor — the [`DIAGRAM_DSL_NOTE`]
/// contract plus the fenced-code tags an agent uses, as a
/// [`Value`] the ACP layer folds into the advertised client capabilities.
#[must_use]
pub fn diagram_capability() -> Value {
    json!({
        "languages": ["mermaid", "structurizr", "jsoncanvas"],
        "fencedAs": ["```mermaid", "```structurizr", "```canvas"],
        "autoLayout": ["mermaid", "structurizr"],
        "explicitLayout": ["jsoncanvas"],
        "note": DIAGRAM_DSL_NOTE,
    })
}

/// Recursively rewrites every `"common_types.json#/$defs/X"` `$ref`
/// string to a catalog-local `"#/$defs/X"` so the inline catalog is
/// self-contained (the agent never has to resolve an external file).
fn localize_refs(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some(rest) = text.strip_prefix("common_types.json#/$defs/") {
                *text = format!("#/$defs/{rest}");
            }
        }
        Value::Array(items) => items.iter_mut().for_each(localize_refs),
        Value::Object(map) => map.values_mut().for_each(localize_refs),
        _ => {}
    }
}

/// Builds the self-contained A2UI inline catalog once: the upstream
/// basic catalog with `common_types.json`'s `$defs` merged in and every
/// cross-file `$ref` localized. Shape (per
/// `client_capabilities.json#/$defs/Catalog`): `{ catalogId, components,
/// functions, theme, $defs }`.
fn build_inline_catalog() -> Value {
    let minimal = || {
        json!({
            "catalogId": A2UI_CATALOG_ID,
            "components": {},
            "functions": [],
            "theme": {},
        })
    };
    let Ok(mut catalog) = serde_json::from_str::<Value>(BASIC_CATALOG_JSON) else {
        return minimal();
    };
    let common = serde_json::from_str::<Value>(COMMON_TYPES_JSON).unwrap_or(Value::Null);

    // Merge common_types `$defs` into the catalog's `$defs`.
    let common_defs = common
        .get("$defs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let Some(catalog_obj) = catalog.as_object_mut() else {
        return minimal();
    };
    let defs = catalog_obj
        .entry("$defs")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(defs_map) = defs.as_object_mut() {
        for (name, schema) in common_defs {
            defs_map.entry(name).or_insert(schema);
        }
    }
    // Localize every `common_types.json#/$defs/...` ref to `#/$defs/...`.
    localize_refs(&mut catalog);

    // Reshape to exactly the negotiated `Catalog` object the agent
    // expects (keep `$defs` so the localized refs resolve in place).
    let object = catalog.as_object().cloned().unwrap_or_default();
    let pick = |key: &str| object.get(key).cloned();
    json!({
        "catalogId": pick("catalogId").unwrap_or_else(|| json!(A2UI_CATALOG_ID)),
        "components": pick("components").unwrap_or_else(|| json!({})),
        "functions": pick("functions").unwrap_or_else(|| json!({})),
        "theme": object
            .get("$defs")
            .and_then(|defs| defs.get("theme"))
            .cloned()
            .unwrap_or_else(|| json!({})),
        "$defs": pick("$defs").unwrap_or_else(|| json!({})),
    })
}

/// The self-contained A2UI inline catalog (cached): the full JSON Schema
/// for every one of the 18 basic-catalog components, the 14 catalog
/// functions, and the theme — exactly what an agent must target to have
/// its UI render faithfully in this terminal client.
#[must_use]
pub fn a2ui_inline_catalog() -> &'static Value {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(build_inline_catalog)
}

/// The A2UI `a2uiClientCapabilities` object to place in client→server
/// transport metadata (the ACP `initialize` client-capabilities
/// `_meta`). It advertises the canonical catalog id **and ships the full
/// catalog inline** so the agent knows every component, prop, enum and
/// function this terminal renders — no external fetch required.
///
/// ```
/// # use rstui_jsonui::capability::client_capabilities;
/// let caps = client_capabilities();
/// assert_eq!(
///     caps["v0.10"]["supportedCatalogIds"][0],
///     "https://a2ui.org/specification/v0_10/basic_catalog.json"
/// );
/// // The full catalog travels inline (18 components).
/// let catalog = &caps["v0.10"]["inlineCatalogs"][0];
/// assert!(catalog["components"]["Button"].is_object());
/// ```
#[must_use]
pub fn client_capabilities() -> Value {
    json!({
        A2UI_VERSION: {
            "supportedCatalogIds": [A2UI_CATALOG_ID],
            "inlineCatalogs": [a2ui_inline_catalog()],
        }
    })
}

/// Declares the json-render component catalog **once**, in one
/// declarative table, and generates everything derived from it: the
/// [`json_render_catalog`] descriptor (`type` → props/slots/events/desc),
/// the [`JSON_RENDER_COMPONENT_NAMES`] name list (which
/// [`json_render_prompt`] reads), and the per-component accessor used by
/// the drift guard. There is no second hand-maintained copy — and the
/// `catalog_matches_the_renderer_coverage` test locks this table to the
/// set the renderer (`jsonrender::project`) actually handles, so the two
/// can never silently diverge.
///
/// Grammar: `"Type" [children] (prop, …) [event, …] => "description";`
/// — `children` present ⇒ the component accepts a child slot.
macro_rules! declare_json_render_catalog {
    ($(
        $name:literal $([$children:ident])? ( $($prop:literal),* $(,)? )
            [ $($event:literal),* $(,)? ] => $desc:literal
    );+ $(;)?) => {
        /// Every json-render component `type` this client renders (each
        /// maps to a [`UiNode`](crate::tree::UiNode)). The single source
        /// of truth — the catalog and prompt are derived from this.
        pub const JSON_RENDER_COMPONENT_NAMES: &[&str] = &[ $($name),+ ];

        /// The json-render standard component catalog (the upstream Ink
        /// standard set), keyed by `type` — generated from the single
        /// `declare_json_render_catalog!` table. This is the catalog an
        /// agent must target; shipped via [`render_capability_summary`].
        #[must_use]
        pub fn json_render_catalog() -> Value {
            let mut catalog = Map::new();
            $(
                #[allow(unused_mut, unused_variables)]
                let takes_children = false;
                $( let takes_children = { stringify!($children); true }; )?
                catalog.insert($name.to_owned(), json!({
                    "props": [ $($prop),* ],
                    "slots": if takes_children { json!(["default"]) } else { json!([]) },
                    "events": [ $($event),* ],
                    "description": $desc,
                }));
            )+
            Value::Object(catalog)
        }
    };
}

declare_json_render_catalog! {
    "Box" [children] (
        "flexDirection", "justifyContent", "alignItems", "gap", "padding",
        "margin", "width", "height", "borderStyle", "borderColor", "backgroundColor"
    ) [] => "Flex layout container (row/column).";
    "Text" (
        "text", "color", "backgroundColor", "bold", "italic", "underline",
        "strikethrough", "dimColor", "wrap"
    ) [] => "A run of styled text.";
    "Newline" ("count") [] => "Blank line(s).";
    "Spacer" () [] => "Flexible empty space.";
    "Heading" ("text", "level", "color") [] => "A heading (level h1-h4).";
    "Divider" ("title", "character", "color") [] => "A horizontal rule.";
    "Badge" ("label", "variant") [] =>
        "Inline status pill (default/info/success/warning/error).";
    "Spinner" ("label", "color") [] => "Animated busy indicator.";
    "ProgressBar" ("progress", "label", "color") [] => "Progress bar in 0..1.";
    "Sparkline" ("data", "color", "label") [] => "Compact trend line.";
    "BarChart" ("data", "color", "height") [] =>
        "Bar chart. data:[{label,value}]; color is a theme token.";
    "LineChart" ("series", "color", "height") [] =>
        "Multi-series line. series:[{name,color?,points:[[x,y]]}].";
    "AreaChart" ("series", "color", "height") [] => "Filled line chart (as LineChart).";
    "PieChart" ("data", "color", "height") [] =>
        "Pie/share. data:[{label,value}]; slices cycle the theme palette.";
    "ScatterPlot" ("series", "color", "height") [] =>
        "XY scatter. series:[{name,color?,points:[[x,y]]}].";
    "Histogram" ("data", "color", "height") [] =>
        "Bucket counts. data:[{label,value}].";
    "StackedBarChart" ("series", "height") [] =>
        "Stacked bars. series:[{name,color?,data:[value]}] per category.";
    "Heatmap" ("data", "cols", "height") [] =>
        "Intensity grid. data:[number]; cols = grid width.";
    "Table" ("columns", "rows") [] => "Column-aligned grid.";
    "List" ("items", "ordered", "bulletChar") [] => "Bulleted/ordered list.";
    "ListItem" ("title", "subtitle", "leading", "trailing") [] => "One list row.";
    "Card" [children] ("title", "backgroundColor", "padding") [] => "Titled container.";
    "KeyValue" ("label", "value", "separator") [] => "Aligned key->value row.";
    "Link" ("url", "label", "color") [] => "Hyperlink.";
    "StatusLine" ("text", "status", "icon") [] =>
        "Leading-glyph status line (info/success/warning/error).";
    "Metric" ("label", "value", "detail", "trend") [] => "A KPI metric.";
    "Callout" ("type", "title", "content") [] =>
        "Accented note (info/tip/warning/important).";
    "Timeline" ("items") [] => "A vertical timeline.";
    "TextInput" ("value", "label", "placeholder", "mask") ["submit", "change"] =>
        "Single-line text entry.";
    "Select" ("options", "value", "label") ["change"] => "Single-choice picker.";
    "MultiSelect" ("options", "value", "label", "min", "max") ["change", "submit"] =>
        "Multi-choice picker.";
    "ConfirmInput" ("message", "defaultValue", "yesLabel", "noLabel") ["confirm", "deny"] =>
        "Yes/No confirm.";
    "Button" ("label", "text", "variant", "disabled") ["press"] =>
        "Action button. on.press runs a builtin (setState/…) locally, \
         or sends a host action {action,params} back to the agent.";
    "Checkbox" ("label", "value") [] =>
        "Boolean toggle (two-way: value:{\"$bindState\":\"/ptr\"}).";
    "Slider" ("label", "value", "min", "max", "step") [] =>
        "Bounded numeric stepper (two-way: value:{\"$bindState\":\"/ptr\"}).";
    "Tabs" [children] ("tabs", "value", "color") ["change"] =>
        "Tab strip + active panel.";
    "Markdown" ("text") [] => "A markdown document.";
}

/// The json-render LLM instruction prompt: the document shape, the
/// streaming-patch protocol, and the available components. An ACP host
/// can prepend this to the model context (it travels in the capability
/// `_meta`). Mirrors the upstream `catalog.prompt()`.
#[must_use]
pub fn json_render_prompt() -> String {
    let mut names: Vec<&str> = JSON_RENDER_COMPONENT_NAMES.to_vec();
    names.sort_unstable();
    format!(
        "You may reply with a json-render UI document this terminal client \
         will render. A document is a flat element map: {{ \"root\": \
         \"<key>\", \"elements\": {{ \"<key>\": {{ \"type\": \"<Component>\", \
         \"props\": {{...}}, \"children\": [\"<key>\", ...] }} }}, \"state\": \
         {{...}} }}. Children are referenced by string key (not inlined). \
         `visible`, `on`, `repeat`, `watch` are siblings of \
         `type`/`props`/`children`, never inside `props`. Prop expressions: \
         {{\"$state\":\"/ptr\"}}, {{\"$bindState\":\"/ptr\"}} (two-way), \
         {{\"$template\":\"...{{/ptr}}...\"}}, \
         {{\"$cond\":...,\"$then\":...,\"$else\":...}}. You may stream the \
         document as RFC-6902 JSON-Patch lines (one JSON object per line) \
         inside a ```spec fenced block. Available components: {}.",
        names.join(", ")
    )
}

/// A description of everything this client can render — the A2UI catalog
/// id + inline-catalog flag, the **full json-render catalog and
/// prompt**, and the diagram DSL — for the ACP `initialize`
/// client-capabilities `_meta` so a model knows it may reply with A2UI /
/// json-render / a diagram and exactly which components, props and events
/// are available.
#[must_use]
pub fn render_capability_summary() -> Value {
    json!({
        "rstuiJsonUi": {
            "a2ui": {
                "version": A2UI_VERSION,
                "supportedCatalogIds": [A2UI_CATALOG_ID],
                "inlineCatalogProvided": true,
            },
            "jsonRender": {
                "catalogId": JSON_RENDER_CATALOG_ID,
                "format": "flat-element-map (root/elements/state) + RFC6902 patch stream",
                "catalog": json_render_catalog(),
                "prompt": json_render_prompt(),
            },
            "diagram": diagram_capability(),
            "note": "This client renders A2UI and json-render UI documents in a terminal, \
                     and Mermaid/Structurizr diagram DSL emitted as a fenced code block \
                     (see `diagram`); unsupported components degrade to a visible \
                     placeholder.",
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a2ui_capabilities_ship_the_full_inline_catalog() {
        let caps = client_capabilities();
        let block = &caps["v0.10"];
        assert!(
            block["supportedCatalogIds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|id| id == A2UI_CATALOG_ID)
        );
        let catalog = &block["inlineCatalogs"][0];
        assert_eq!(catalog["catalogId"], A2UI_CATALOG_ID);
        let components = catalog["components"].as_object().expect("components map");
        // All 18 basic-catalog components travel inline.
        for name in [
            "Text",
            "Image",
            "Icon",
            "Video",
            "AudioPlayer",
            "Row",
            "Column",
            "List",
            "Card",
            "Tabs",
            "Modal",
            "Divider",
            "Button",
            "TextField",
            "CheckBox",
            "ChoicePicker",
            "Slider",
            "DateTimeInput",
        ] {
            assert!(components.contains_key(name), "missing component {name}");
        }
        assert!(catalog["functions"]["formatString"].is_object());
        // Refs are localized — no external file to fetch.
        let serialized = serde_json::to_string(catalog).unwrap();
        assert!(
            !serialized.contains("common_types.json#"),
            "all cross-file refs must be localized to #/$defs"
        );
        // The merged $defs make those local refs resolvable.
        assert!(catalog["$defs"]["DynamicString"].is_object());
    }

    #[test]
    fn json_render_catalog_and_prompt_are_complete() {
        let catalog = json_render_catalog();
        let map = catalog.as_object().unwrap();
        // The upstream Ink standard set (Box…Markdown) + the rstui
        // chart extension + the form-element set (Button/Checkbox/
        // Slider added beside TextInput/Select/ConfirmInput).
        assert_eq!(
            map.len(),
            37,
            "the standard component set + charts + form elements"
        );
        for form in ["Button", "Checkbox", "Slider", "TextInput"] {
            assert!(map.contains_key(form), "json-render advertises {form}");
        }
        assert!(
            catalog["Button"]["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event == "press"),
            "Button advertises the press event the agent binds an action to"
        );
        assert!(map.contains_key("Box") && map.contains_key("Markdown"));
        assert!(
            catalog["TextInput"]["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event == "submit")
        );
        let prompt = json_render_prompt();
        assert!(prompt.contains("\"root\"") && prompt.contains("RFC-6902"));
        assert!(prompt.contains("Markdown") && prompt.contains("Box"));

        let summary = render_capability_summary();
        assert_eq!(summary["rstuiJsonUi"]["a2ui"]["version"], A2UI_VERSION);
        assert!(summary["rstuiJsonUi"]["jsonRender"]["catalog"]["Card"].is_object());
        assert!(summary["rstuiJsonUi"]["jsonRender"]["prompt"].is_string());
    }

    #[test]
    fn catalog_is_macro_generated_single_source_and_matches_the_renderer() {
        use crate::jsonrender::JsonRenderDoc;
        use crate::tree::UiNode;
        use serde_json::json;

        // The catalog map is exactly the macro's name list — one source.
        let catalog = json_render_catalog();
        let keys: std::collections::BTreeSet<&str> = catalog
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let declared: std::collections::BTreeSet<&str> =
            JSON_RENDER_COMPONENT_NAMES.iter().copied().collect();
        assert_eq!(keys, declared, "catalog == declared name list");

        // Drift guard: every component the catalog advertises must hit a
        // real arm in the renderer (`jsonrender::project`), not its
        // `other => Placeholder(other)` catch-all — so the macro table
        // can never claim a type the renderer does not actually map.
        // Data-driven components are fed minimal valid data (a tiny test
        // fixture, not a second catalog) so they render structurally
        // rather than their own total "needs data" placeholder.
        for name in JSON_RENDER_COMPONENT_NAMES {
            let props = match *name {
                "BarChart" | "PieChart" | "Histogram" => {
                    json!({ "data": [{ "label": "a", "value": 1 }] })
                }
                "Sparkline" | "Heatmap" => json!({ "data": [1, 2, 3] }),
                "LineChart" | "AreaChart" | "ScatterPlot" | "StackedBarChart" => json!({
                    "series": [{ "name": "s", "points": [[0, 1], [1, 2]] }]
                }),
                "Table" => json!({
                    "columns": [{ "header": "H", "key": "k" }],
                    "rows": [{ "k": "v" }]
                }),
                "Select" | "MultiSelect" => json!({
                    "options": [{ "label": "o", "value": "v" }]
                }),
                "Tabs" => json!({ "tabs": [{ "label": "t", "value": "v" }] }),
                _ => json!({}),
            };
            let spec = json!({
                "root": "x",
                "elements": { "x": { "type": name, "props": props } }
            });
            let rendered = JsonRenderDoc::from_flat_value(&spec).view();
            assert!(
                !matches!(&rendered, UiNode::Placeholder(unknown) if unknown == name),
                "catalog advertises `{name}` but the renderer projected it to a \
                 Placeholder — the macro table and `jsonrender::project` have \
                 drifted (a catalog entry with no renderer arm)"
            );
        }
    }

    #[test]
    fn the_diagram_dsl_is_advertised_to_the_agent() {
        let cap = diagram_capability();
        let langs = cap["languages"].as_array().unwrap();
        assert!(langs.iter().any(|l| l == "mermaid"));
        assert!(langs.iter().any(|l| l == "structurizr"));
        assert!(langs.iter().any(|l| l == "jsoncanvas"));
        assert!(cap["note"].as_str().unwrap().contains("```mermaid"));
        // JSON Canvas is advertised as the explicit-layout escape hatch.
        assert!(
            cap["explicitLayout"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l == "jsoncanvas")
        );
        assert!(cap["note"].as_str().unwrap().contains("```canvas"));
        // Folded into the agent-readable render summary.
        let summary = render_capability_summary();
        assert_eq!(summary["rstuiJsonUi"]["diagram"]["languages"][0], "mermaid");
        assert!(
            DIAGRAM_DSL_NOTE.contains("structurizr")
                && DIAGRAM_DSL_NOTE.contains("JSON Canvas")
                && DIAGRAM_DSL_NOTE.contains("control the exact layout")
        );
    }
}
