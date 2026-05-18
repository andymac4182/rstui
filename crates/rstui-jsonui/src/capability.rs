//! The capability descriptors this client advertises to an agent so the
//! agent knows it may stream rich UI — and which catalog to target.
//!
//! A2UI negotiates via transport metadata: the client sends
//! `a2uiClientCapabilities` (`{ "v0.10": { "supportedCatalogIds": [...]
//! } }`) on every client→server message; the agent must only emit
//! components from a mutually-supported catalog. json-render has no wire
//! handshake — the host instead hands the model a catalog/prompt; we
//! expose the supported component list for the same purpose. This module
//! produces both as plain [`serde_json::Value`]s the
//! [`a2ui`](crate::a2ui)/ACP layers attach to their transports.

use serde_json::{Value, json};

/// The catalog id this client advertises as renderable. It is the
/// upstream A2UI basic-catalog id: every component in it maps to a
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
/// [`Mermaid`](rstui_widgets::Mermaid)/[`Structurizr`](rstui_widgets::Structurizr)
/// renderers. Injecting this into a system prompt / ACP client-capabilities
/// lets a model answer *with* a diagram instead of describing one in prose.
pub const DIAGRAM_DSL_NOTE: &str = "To output a diagram, emit a fenced code block: \
    ```mermaid … ``` for any Mermaid diagram type (flowchart/graph, sequenceDiagram, \
    classDiagram, stateDiagram-v2, erDiagram, gantt, pie, gitGraph, mindmap, timeline, \
    journey, quadrantChart, requirementDiagram, sankey-beta, xychart-beta, block-beta, \
    packet-beta, kanban, architecture-beta, radar-beta, C4*, zenuml), or \
    ```structurizr … ``` for a Structurizr DSL / C4 workspace. It renders as a \
    deterministic terminal diagram; an unterminated block still renders while streaming.";

/// The diagram-DSL capability descriptor — the [`DIAGRAM_DSL_NOTE`]
/// contract plus the fenced-code tags an agent uses, as a
/// [`Value`] the ACP layer folds into the advertised client capabilities.
#[must_use]
pub fn diagram_capability() -> Value {
    json!({
        "languages": ["mermaid", "structurizr"],
        "fencedAs": ["```mermaid", "```structurizr"],
        "note": DIAGRAM_DSL_NOTE,
    })
}

/// The A2UI `a2uiClientCapabilities` object to place in client→server
/// transport metadata (ACP session/message metadata): it tells the agent
/// the exact catalog id(s) this terminal client can render.
///
/// ```
/// # use rstui_jsonui::capability::client_capabilities;
/// let caps = client_capabilities();
/// assert_eq!(
///     caps["v0.10"]["supportedCatalogIds"][0],
///     "https://a2ui.org/specification/v0_10/basic_catalog.json"
/// );
/// ```
#[must_use]
pub fn client_capabilities() -> Value {
    json!({
        A2UI_VERSION: {
            "supportedCatalogIds": [A2UI_CATALOG_ID],
        }
    })
}

/// A compact, human/agent-readable description of everything this client
/// can render, suitable for injecting into a system prompt or an ACP
/// `initialize` client-capabilities extension so a model knows it may
/// reply with A2UI / json-render instead of plain text.
#[must_use]
pub fn render_capability_summary() -> Value {
    json!({
        "rstuiJsonUi": {
            "a2ui": {
                "version": A2UI_VERSION,
                "supportedCatalogIds": [A2UI_CATALOG_ID],
            },
            "jsonRender": {
                "catalogId": JSON_RENDER_CATALOG_ID,
                "format": "flat-element-map (root/elements/state) + RFC6902 patch stream",
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
    fn capabilities_carry_the_basic_catalog_id() {
        let caps = client_capabilities();
        let ids = caps["v0.10"]["supportedCatalogIds"].as_array().unwrap();
        assert!(ids.iter().any(|id| id == A2UI_CATALOG_ID));
        let summary = render_capability_summary();
        assert_eq!(summary["rstuiJsonUi"]["a2ui"]["version"], A2UI_VERSION);
    }

    #[test]
    fn the_diagram_dsl_is_advertised_to_the_agent() {
        let cap = diagram_capability();
        let langs = cap["languages"].as_array().unwrap();
        assert!(langs.iter().any(|l| l == "mermaid"));
        assert!(langs.iter().any(|l| l == "structurizr"));
        assert!(cap["note"].as_str().unwrap().contains("```mermaid"));
        // Folded into the agent-readable render summary.
        let summary = render_capability_summary();
        assert_eq!(summary["rstuiJsonUi"]["diagram"]["languages"][0], "mermaid");
        assert!(
            DIAGRAM_DSL_NOTE.contains("structurizr")
                && DIAGRAM_DSL_NOTE.contains("Mermaid diagram type")
        );
    }
}
