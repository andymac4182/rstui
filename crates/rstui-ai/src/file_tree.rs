//! [`FileTree`] — a collapsible file/folder tree with selection: the
//! workspace explorer an agent's file-edit tools project.
//!
//! # A flattened projection, reusing [`Tree`]
//!
//! The ai-elements `FileTree` is a recursive expand/collapse file explorer. A
//! tree is a graph, but rstui's `view` takes `&self` and never walks a node
//! graph at render time — the documented answer is the
//! [`Tree`] **flattened-projection** pattern: the caller
//! (who owns the real tree) flattens the *currently-visible* nodes into a
//! `Vec`, and the widget reads that plus the selected index. So `FileTree`
//! owns nothing: it projects a caller-owned `&[FileNode]` (depth + kind +
//! expanded) and a caller-owned [`selected`](FileTree::selected) /
//! [`offset`](FileTree::offset).
//!
//! It **reuses** [`Tree`]/[`TreeItem`]
//! — it does not reinvent indentation/markers — adding only a folder/file
//! glyph per row. Selection/expansion are the reducer's (splice children in
//! on expand, the documented recipe); the host hit-tests a row against the
//! same one-row-per-node math.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area, an
//! empty tree, and an out-of-range selection are all safe — never a panic.

use rstui_core::{Buffer, Rect, Style, Widget};
use rstui_widgets::{Block, Tree, TreeGuides, TreeItem};

/// One visible node of a [`FileTree`]: a name at an indentation `depth`, a
/// file or an (open/closed) folder.
///
/// The caller (who owns the real tree) flattens the currently-visible nodes
/// into a `Vec<FileNode>` — exactly the [`Tree`]
/// contract. A file is the default; a folder opts in with
/// [`folder`](Self::folder).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNode {
    name: String,
    depth: u16,
    is_dir: bool,
    expanded: bool,
}

impl FileNode {
    /// A file `name` at indentation `depth` (column 0 is depth 0).
    pub fn file(depth: u16, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            depth,
            is_dir: false,
            expanded: false,
        }
    }

    /// A folder `name` at `depth`, drawn open when `expanded`.
    pub fn folder(depth: u16, name: impl Into<String>, expanded: bool) -> Self {
        Self {
            name: name.into(),
            depth,
            is_dir: true,
            expanded,
        }
    }

    /// The glyph prefixing this node's name (`▸`/`▾` folder, `·` file).
    fn glyph(&self) -> &'static str {
        match (self.is_dir, self.expanded) {
            (true, true) => "▾ ",
            (true, false) => "▸ ",
            (false, _) => "· ",
        }
    }
}

/// A collapsible file/folder tree with selection.
///
/// Projects a caller-owned `&[FileNode]` (the flattened visible rows) and a
/// caller-owned [`selected`](Self::selected) / [`offset`](Self::offset),
/// rendered through [`Tree`] (each row a glyph + the
/// name at its depth). `FileTree` owns no state — see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::file_tree::{FileNode, FileTree};
///
/// let nodes = [
///     FileNode::folder(0, "src", true),
///     FileNode::file(1, "lib.rs"),
/// ];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 14, 2));
/// FileTree::new(&nodes).selected(Some(0)).render(buf.area(), &mut buf);
///
/// // The open-folder marker, then the depth-1 child file's glyph past its
/// // indentation (the reused `Tree` places it under its parent).
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '▾');
/// assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, '·'); // file glyph
/// ```
#[derive(Debug, Clone)]
pub struct FileTree<'a> {
    nodes: &'a [FileNode],
    block: Option<Block<'a>>,
    selected: Option<usize>,
    offset: usize,
    style: Style,
    highlight_style: Style,
}

impl<'a> FileTree<'a> {
    /// A tree of the flattened `nodes`, nothing selected, scrolled to the
    /// top.
    #[must_use]
    pub fn new(nodes: &'a [FileNode]) -> Self {
        Self {
            nodes,
            block: None,
            selected: None,
            offset: 0,
            style: Style::new(),
            highlight_style: Style::new().add_modifier(rstui_core::Modifier::REVERSED),
        }
    }

    /// Frames the tree in `block`.
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the caller-owned selected row — an index into the flattened
    /// `nodes` (the reducer owns it; clamped by
    /// [`Tree`]).
    #[must_use]
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the caller-owned scroll offset into the flattened `nodes`.
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Sets the base [`Style`].
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] the selected row is highlighted with.
    #[must_use]
    pub fn highlight_style(mut self, highlight_style: Style) -> Self {
        self.highlight_style = highlight_style;
        self
    }

    /// The currently-selected node, if [`selected`](Self::selected) is a
    /// valid index — what the reducer reads to open a file or toggle a
    /// folder.
    #[must_use]
    pub fn selected_node(&self) -> Option<&FileNode> {
        self.selected.and_then(|i| self.nodes.get(i))
    }
}

impl Widget for FileTree<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let items: Vec<TreeItem<'static>> = self
            .nodes
            .iter()
            .map(|node| {
                let label = format!("{}{}", node.glyph(), node.name);
                let item = TreeItem::new(node.depth, label);
                if node.is_dir {
                    item.expandable(node.expanded)
                } else {
                    item
                }
            })
            .collect();
        let mut tree = Tree::new(items)
            .guides(TreeGuides::Lines)
            .style(self.style)
            .highlight_style(self.highlight_style)
            .selected(self.selected)
            .offset(self.offset);
        if let Some(block) = self.block {
            tree = tree.block(block);
        }
        tree.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Position;

    fn nodes() -> Vec<FileNode> {
        vec![
            FileNode::folder(0, "src", true),
            FileNode::file(1, "lib.rs"),
            FileNode::folder(0, "docs", false),
        ]
    }

    fn lines(widget: FileTree<'_>, w: u16, h: u16) -> String {
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
    fn it_renders_folder_and_file_glyphs_at_depth() {
        let n = nodes();
        let out = lines(FileTree::new(&n), 14, 3);
        // Row 0: open folder "src"; row 1: child file "lib.rs"; row 2:
        // closed folder "docs".
        assert!(out.contains("▾ src"), "got {out:?}");
        assert!(out.contains("· lib.rs"), "got {out:?}");
        assert!(out.contains("▸ docs"), "got {out:?}");
    }

    #[test]
    fn the_child_is_indented_under_its_folder() {
        let n = nodes();
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 3));
        FileTree::new(&n).render(buf.area(), &mut buf);
        // depth-0 folder marker flush at col 0.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '▾');
        // depth-1 file sits past the reserved marker column.
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, '·');
    }

    #[test]
    fn selected_node_reads_the_flattened_index() {
        let n = nodes();
        assert_eq!(
            FileTree::new(&n).selected(Some(1)).selected_node(),
            Some(&n[1])
        );
        // Out of range → None, not a panic.
        assert_eq!(FileTree::new(&n).selected(Some(9)).selected_node(), None);
        assert_eq!(FileTree::new(&n).selected_node(), None);
    }

    #[test]
    fn the_selection_is_highlighted() {
        let n = nodes();
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 3));
        FileTree::new(&n)
            .selected(Some(0))
            .render(buf.area(), &mut buf);
        assert!(
            buf.get(Position::new(0, 0))
                .unwrap()
                .modifier
                .contains(rstui_core::Modifier::REVERSED)
        );
    }

    #[test]
    fn an_empty_tree_is_safe() {
        let empty: [FileNode; 0] = [];
        assert_eq!(lines(FileTree::new(&empty), 4, 2), "    \n    \n");
    }

    #[test]
    fn a_block_frames_the_tree() {
        let n = nodes();
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 4));
        FileTree::new(&n)
            .block(Block::bordered())
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let n = nodes();
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        FileTree::new(&n).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
