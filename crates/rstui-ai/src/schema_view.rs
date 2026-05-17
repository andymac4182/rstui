//! [`SchemaView`] — a recursive JSON-schema / endpoint viewer: the
//! "this tool takes …" / "GET /users → { … }" tree an agent renders for a
//! tool's parameter or response shape.
//!
//! # A flattened, recursive projection (the [`Tree`](rstui_widgets::Tree) idiom)
//!
//! The ai-elements `SchemaDisplay` is a method/path header over a recursive,
//! collapsible property tree. A schema is a tree, but rstui's `view` takes
//! `&self` and never walks a graph at render time — the documented answer is
//! the [`Tree`](rstui_widgets::Tree) flattened-projection pattern. So
//! `SchemaView` owns nothing: the caller builds a recursive
//! [`SchemaNode`] (`{name, type, required, children}`), and the widget
//! *flattens* it (depth-first, depth = indentation) into rows it draws. An
//! optional [`method`](SchemaView::method)/[`path`](SchemaView::path) prefix
//! is a method [`Badge`] + the path on row 0.
//!
//! Per-node collapse is the caller's: a node with no
//! [`children`](SchemaNode::children) (or one the caller chose not to expand)
//! simply contributes one row — the same "flatten only what is visible"
//! contract [`Tree`](rstui_widgets::Tree) uses; nothing is smuggled into the
//! widget.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area, an
//! empty schema, and a deep/wide tree are all safe clips — never a panic.

use rstui_core::{Buffer, Color, Modifier, Position, Rect, Style, Widget};
use rstui_widgets::{Badge, BadgeLevel};

/// One node of a schema tree: a name, a type, whether it is required, and its
/// (caller-flattened-on-demand) children.
///
/// The caller builds this recursively; an empty
/// [`children`](Self::children) is a leaf. To render a node *collapsed* the
/// caller simply builds it with no children — the widget flattens whatever
/// it is given (the [`Tree`](rstui_widgets::Tree) "visible rows only"
/// contract).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchemaNode {
    /// The property name.
    pub name: String,
    /// The type string (e.g. `string`, `object`, `array<int>`).
    pub type_name: String,
    /// Whether the property is required.
    pub required: bool,
    /// Nested properties (empty for a leaf / a collapsed node).
    pub children: Vec<SchemaNode>,
}

impl SchemaNode {
    /// A leaf property `name` of type `type_name`.
    pub fn leaf(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
            required: false,
            children: Vec::new(),
        }
    }

    /// Marks this node required.
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Sets this node's nested properties.
    #[must_use]
    pub fn children(mut self, children: Vec<SchemaNode>) -> Self {
        self.children = children;
        self
    }

    /// Appends `(depth, self)` then each descendant (depth-first) into
    /// `out` — the flattening the widget renders.
    fn flatten<'n>(&'n self, depth: u16, out: &mut Vec<(u16, &'n SchemaNode)>) {
        out.push((depth, self));
        for child in &self.children {
            child.flatten(depth.saturating_add(1), out);
        }
    }
}

/// A recursive JSON-schema / endpoint viewer.
///
/// If a [`method`](Self::method) is set, row 0 is a method
/// [`Badge`] + the [`path`](Self::path). Then one row
/// per flattened [`SchemaNode`]: `<indent>name: type` with a trailing `*`
/// when required (required names are bold). `SchemaView` owns no state — see
/// the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::schema_view::{SchemaNode, SchemaView};
///
/// let root = SchemaNode::leaf("user", "object").children(vec![
///     SchemaNode::leaf("id", "string").required(true),
/// ]);
/// let view = SchemaView::new(&root).method("GET").path("/user");
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 24, 4));
/// view.render(buf.area(), &mut buf);
/// // Row 0: the method badge + path; row 1: the root; row 2: the child.
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'G'); // "GET"
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'u'); // user
/// ```
#[derive(Debug, Clone)]
pub struct SchemaView<'a> {
    root: &'a SchemaNode,
    method: Option<&'a str>,
    path: &'a str,
    indent: u16,
    style: Style,
}

impl<'a> SchemaView<'a> {
    /// A viewer of the schema rooted at `root`, no method/path prefix, with
    /// a two-column indent per depth.
    #[must_use]
    pub fn new(root: &'a SchemaNode) -> Self {
        Self {
            root,
            method: None,
            path: "",
            indent: 2,
            style: Style::new(),
        }
    }

    /// Sets the HTTP method shown as a badge on row 0 (enables the
    /// method/path header row).
    #[must_use]
    pub fn method(mut self, method: &'a str) -> Self {
        self.method = Some(method);
        self
    }

    /// Sets the endpoint path shown beside the method badge.
    #[must_use]
    pub fn path(mut self, path: &'a str) -> Self {
        self.path = path;
        self
    }

    /// Sets the columns of indentation per depth level (default `2`).
    #[must_use]
    pub fn indent(mut self, indent: u16) -> Self {
        self.indent = indent;
        self
    }

    /// Sets the base [`Style`].
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The schema flattened depth-first as `(depth, node)` — the rows the
    /// widget draws (after the optional method/path header).
    #[must_use]
    pub fn rows(&self) -> Vec<(u16, &SchemaNode)> {
        let mut out = Vec::new();
        self.root.flatten(0, &mut out);
        out
    }

    /// The accent [`BadgeLevel`] for `method` (GET info, POST success,
    /// DELETE error, PUT/PATCH warning, else neutral).
    fn method_level(method: &str) -> BadgeLevel {
        match method.to_ascii_uppercase().as_str() {
            "GET" => BadgeLevel::Info,
            "POST" => BadgeLevel::Success,
            "DELETE" => BadgeLevel::Error,
            "PUT" | "PATCH" => BadgeLevel::Warning,
            _ => BadgeLevel::Neutral,
        }
    }
}

impl Widget for SchemaView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        buf.set_style(area, self.style);

        // The optional method/path header row.
        let first_node_row = if let Some(method) = self.method {
            let badge_w = (method.chars().count() as u16).saturating_add(2);
            Badge::new(method)
                .level(Self::method_level(method))
                .info_style(Style::new().fg(Color::Black).bg(Color::Blue))
                .success_style(Style::new().fg(Color::Black).bg(Color::Green))
                .warning_style(Style::new().fg(Color::Black).bg(Color::Yellow))
                .error_style(Style::new().fg(Color::Black).bg(Color::Red))
                .render(Rect::new(area.left(), area.top(), area.width, 1), buf);
            let mut x = area.left().saturating_add(badge_w).saturating_add(1);
            for ch in self.path.chars() {
                if x >= area.right() {
                    break;
                }
                buf.set_cell(Position::new(x, area.top()), ch, self.style);
                x = x.saturating_add(1);
            }
            1u16
        } else {
            0u16
        };

        // The flattened nodes.
        let avail = area.height.saturating_sub(first_node_row);
        for (n, (depth, node)) in self.rows().iter().take(avail as usize).enumerate() {
            let y = area
                .top()
                .saturating_add(first_node_row)
                .saturating_add(n as u16);
            let star = if node.required { "*" } else { "" };
            let text = format!("{}: {}{}", node.name, node.type_name, star);
            let name_style = if node.required {
                self.style.add_modifier(Modifier::BOLD)
            } else {
                self.style
            };
            let mut x = area
                .left()
                .saturating_add(depth.saturating_mul(self.indent));
            for ch in text.chars() {
                if x >= area.right() {
                    break;
                }
                buf.set_cell(Position::new(x, y), ch, name_style);
                x = x.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> SchemaNode {
        SchemaNode::leaf("user", "object").children(vec![
            SchemaNode::leaf("id", "string").required(true),
            SchemaNode::leaf("tags", "array").children(vec![SchemaNode::leaf("0", "string")]),
        ])
    }

    fn lines(widget: SchemaView<'_>, w: u16, h: u16) -> String {
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
    fn it_flattens_the_schema_depth_first_with_indentation() {
        let s = schema();
        let out = lines(SchemaView::new(&s), 20, 4);
        // root depth 0, its two children depth 1, the array's child depth 2.
        assert_eq!(
            out,
            "user: object        \n  id: string*       \n  tags: array       \n    0: string       \n"
        );
    }

    #[test]
    fn rows_reports_each_node_with_its_depth() {
        let s = schema();
        let view = SchemaView::new(&s);
        let rows = view.rows();
        let shape: Vec<(u16, &str)> = rows.iter().map(|(d, n)| (*d, n.name.as_str())).collect();
        assert_eq!(shape, vec![(0, "user"), (1, "id"), (1, "tags"), (2, "0")]);
    }

    #[test]
    fn required_nodes_get_a_star_and_bold_name() {
        let s = schema();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        SchemaView::new(&s).render(buf.area(), &mut buf);
        // "id" (row 1) is required → bold.
        assert!(
            buf.get(Position::new(2, 1))
                .unwrap()
                .modifier
                .contains(Modifier::BOLD)
        );
        // "user" (row 0) is not required → not bold.
        assert!(
            !buf.get(Position::new(0, 0))
                .unwrap()
                .modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn a_method_and_path_render_as_a_header_row() {
        let s = SchemaNode::leaf("body", "object");
        let out = lines(SchemaView::new(&s).method("GET").path("/users"), 20, 2);
        // Row 0: " GET " badge then "/users"; row 1: the root node.
        assert!(out.starts_with(" GET  /users"), "got {out:?}");
        assert!(out.contains("body: object"), "got {out:?}");
    }

    #[test]
    fn method_levels_are_distinct() {
        assert_eq!(SchemaView::method_level("get"), BadgeLevel::Info);
        assert_eq!(SchemaView::method_level("POST"), BadgeLevel::Success);
        assert_eq!(SchemaView::method_level("DELETE"), BadgeLevel::Error);
        assert_eq!(SchemaView::method_level("put"), BadgeLevel::Warning);
        assert_eq!(SchemaView::method_level("HEAD"), BadgeLevel::Neutral);
    }

    #[test]
    fn a_deep_tree_clips_to_the_area() {
        let s = schema();
        // Only 2 rows of height → root + first child, no panic.
        let out = lines(SchemaView::new(&s), 20, 2);
        assert!(out.contains("user: object"), "got {out:?}");
        assert!(out.contains("id: string*"), "got {out:?}");
    }

    #[test]
    fn an_empty_leaf_schema_is_one_row() {
        let s = SchemaNode::leaf("x", "null");
        assert_eq!(SchemaView::new(&s).rows().len(), 1);
        assert_eq!(lines(SchemaView::new(&s), 10, 1), "x: null   \n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let s = schema();
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        SchemaView::new(&s).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
