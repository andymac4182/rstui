//! [`Tree`] — a single-select column of indented, expand/collapse rows: the
//! basis for file explorers, outline panes, and nested settings.
//!
//! # A flattened projection, on purpose
//!
//! A tree is a graph, but rstui's `App::view` (in `rstui-runtime`) takes
//! `&self` — a view never walks or mutates a node graph at render time. So,
//! exactly like [`List`](crate::List), `Tree` is a pure projection of a
//! **caller-owned flattened `Vec`** of the rows that are *currently visible*:
//! each [`TreeItem`] carries only its [`depth`](TreeItem::new), whether it
//! [`has_children`](TreeItem::expandable), and whether it is
//! [`expanded`](TreeItem::expandable). Which nodes exist, which are expanded,
//! and which is [`selected`](Tree::selected) is ordinary application state the
//! reducer owns and rebuilds in `update` (expanding a node is "splice this
//! node's children into the flattened list"); the widget reads that list and
//! the `selected`/`offset` indices into it — it never writes them.
//!
//! That keeps the one-row-per-visible-node index math unambiguous (identical
//! to [`List`](crate::List)) and the whole widget deterministically
//! headless-testable.
//!
//! Because the widget is handed only the flattened rows and *not* the full
//! tree, it cannot know whether a node is its parent's last child, so the
//! true last-sibling elbow connectors (`├`/`└`) of [`TreeGuides::Lines`] are
//! **deliberately out of scope** for this slice: a future additive
//! `last_sibling_mask` on [`TreeItem`] (the only place the caller, who *does*
//! own the tree, can supply that bit) would carry it — a clean extension, not
//! something smuggled into this slice's row contract.

use std::borrow::Cow;

use crate::block::Block;
use rstui_core::{Buffer, Line, Position, Rect, Span, Style, Widget};

/// One visible row of a [`Tree`]: a [`Line`] label at an indentation
/// [`depth`](TreeItem::new), optionally an expandable node that is open or
/// closed.
///
/// The caller (who owns the real tree) flattens the currently-visible nodes
/// into a `Vec<TreeItem>`; a leaf is the default and an expandable node opts
/// in with [`expandable`](Self::expandable). Build a depth-0 leaf from
/// anything a [`Line`] is built from (`&str`, `String`, [`Span`], [`Line`], a
/// `Vec<Span>`); style it through the [`Line`] it wraps with
/// [`style`](Self::style).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TreeItem<'a> {
    label: Line<'a>,
    depth: u16,
    has_children: bool,
    expanded: bool,
}

impl<'a> TreeItem<'a> {
    /// A leaf row at `depth` displaying `label` (any value convertible to a
    /// [`Line`]). `depth` is the caller's tree depth; column 0 is depth 0.
    pub fn new(depth: u16, label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            depth,
            has_children: false,
            expanded: false,
        }
    }

    /// Marks this row an expandable node, drawn open when `expanded` and
    /// closed otherwise (a leaf, the default, draws neither marker).
    #[must_use]
    pub fn expandable(mut self, expanded: bool) -> Self {
        self.has_children = true;
        self.expanded = expanded;
        self
    }

    /// Replaces this row's base [`Style`] (beneath each label span's style).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.label = self.label.style(style);
        self
    }
}

impl<'a> From<&'a str> for TreeItem<'a> {
    fn from(s: &'a str) -> Self {
        Self::new(0, Line::from(s))
    }
}

impl From<String> for TreeItem<'_> {
    fn from(s: String) -> Self {
        Self::new(0, Line::from(s))
    }
}

impl<'a> From<Cow<'a, str>> for TreeItem<'a> {
    fn from(s: Cow<'a, str>) -> Self {
        Self::new(0, Line::from(s))
    }
}

impl<'a> From<Span<'a>> for TreeItem<'a> {
    fn from(span: Span<'a>) -> Self {
        Self::new(0, Line::from(span))
    }
}

impl<'a> From<Line<'a>> for TreeItem<'a> {
    fn from(line: Line<'a>) -> Self {
        Self::new(0, line)
    }
}

impl<'a> From<Vec<Span<'a>>> for TreeItem<'a> {
    fn from(spans: Vec<Span<'a>>) -> Self {
        Self::new(0, Line::from(spans))
    }
}

/// How a [`Tree`] draws the indentation prefix that precedes every label.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TreeGuides {
    /// Blank indentation — only the per-row expansion marker is drawn.
    #[default]
    Markers,
    /// A `│` continuation rule at every ancestor level (see the
    /// [module docs](self) on why last-sibling elbows are out of scope).
    Lines,
}

/// A vertical column of indented, selectable, expand/collapse rows with an
/// optional framing [`Block`].
///
/// `Tree` shows the window of visible rows `[offset, offset + height)` — one
/// row per [`TreeItem`] — exactly like [`List`](crate::List), so
/// [`selected`](Self::selected) and [`offset`](Self::offset) are caller-owned
/// indices into the *flattened* list, never mutated here (see the
/// [module docs](self) for why).
///
/// Each row is `depth * indent` columns of prefix, then the label. The
/// prefix's last two cells are the expansion marker — `▾ ` open, `▸ ` closed,
/// two blanks for a leaf — so a leaf's label still aligns with an expandable
/// sibling at the same depth (the reserved-gutter idiom
/// [`List`](crate::List) uses for `highlight_symbol`). With
/// [`TreeGuides::Lines`] each shallower level draws a `│` continuation rule.
///
/// Styling cascades tree → guide/label-line → span (the same
/// [`Style::patch`](rstui_core::Style) model the text model uses); the tree
/// base style also fills the content area. On the selected row
/// [`highlight_style`](Self::highlight_style) is patched **last** across the
/// full inner width, so the guides, marker, label, and trailing padding read
/// as one contiguous bar. An optional [`highlight_symbol`](Self::highlight_symbol)
/// gutter is reserved (blank) on every row and painted only on the selected
/// one, before the depth prefix.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{Tree, TreeItem};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
/// Tree::new([
///     TreeItem::new(0, "src").expandable(true),
///     TreeItem::new(1, "lib.rs"),
/// ])
/// .selected(Some(0))
/// .render(buf.area(), &mut buf);
///
/// // A depth-0 row renders flush, exactly like a `List` row…
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 's');
/// // …and the depth-1 child sits past the reserved two-cell marker column.
/// assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, 'l');
///
/// // Expanding/collapsing a node and moving the selection happen in the
/// // reducer — it owns the flattened `items` and the `selected` index; the
/// // widget only projects them.
/// ```
#[derive(Debug, Default, Clone)]
pub struct Tree<'a> {
    items: Vec<TreeItem<'a>>,
    block: Option<Block<'a>>,
    style: Style,
    highlight_style: Style,
    highlight_symbol: Option<Cow<'a, str>>,
    selected: Option<usize>,
    offset: usize,
    indent: u16,
    guides: TreeGuides,
    guide_style: Style,
}

impl<'a> Tree<'a> {
    /// A tree of `items`, nothing selected, scrolled to the top, with the
    /// default two-column [`indent`](Self::indent) and
    /// [`TreeGuides::Markers`].
    pub fn new<I, T>(items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<TreeItem<'a>>,
    {
        Self {
            items: items.into_iter().map(Into::into).collect(),
            indent: 2,
            ..Self::default()
        }
    }

    /// Frames the tree in `block`; rows render into [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`], beneath the tree → line → span cascade. It
    /// also fills the content area so a background covers the whole pane.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched over the selected row.
    ///
    /// Patched **last** in the cascade, so it overrides per-item styling, and
    /// applied across the full inner width so the selection reads as one bar.
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Sets the gutter string drawn before the selected row, ahead of the
    /// depth prefix.
    ///
    /// The gutter is reserved (blank) on unselected rows too, so row text
    /// keeps its column as the selection moves.
    #[must_use]
    pub fn highlight_symbol(mut self, symbol: impl Into<Cow<'a, str>>) -> Self {
        self.highlight_symbol = Some(symbol.into());
        self
    }

    /// Sets which flattened-row index is highlighted, or `None` for none.
    ///
    /// An index outside the visible window simply paints no bar — the caller
    /// owns scrolling (see the [module docs](self)).
    #[must_use]
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the index of the first visible row (the scroll offset).
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Sets the columns added per [`depth`](TreeItem::new) level (default 2,
    /// the width the expansion marker and a [`TreeGuides::Lines`] `│ ` rule
    /// occupy).
    #[must_use]
    pub fn indent(mut self, indent: u16) -> Self {
        self.indent = indent;
        self
    }

    /// Sets how the indentation prefix is drawn (default
    /// [`TreeGuides::Markers`]).
    #[must_use]
    pub fn guides(mut self, guides: TreeGuides) -> Self {
        self.guides = guides;
        self
    }

    /// Sets the [`Style`] of the indentation prefix (guides and the marker),
    /// patched over the base and beneath the selected-row highlight.
    #[must_use]
    pub fn guide_style(mut self, style: Style) -> Self {
        self.guide_style = style;
        self
    }
}

impl Widget for Tree<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Tree {
            items,
            block,
            style,
            highlight_style,
            highlight_symbol,
            selected,
            offset,
            indent,
            guides,
            guide_style,
        } = self;

        // The block (if any) frames the content and reserves the inner area.
        let inner = match &block {
            Some(b) => b.inner(area),
            None => area,
        };
        if let Some(b) = block {
            b.render(area, buf);
        }
        if inner.is_empty() {
            return;
        }

        // Tree base fills the content area so a background covers the whole
        // pane (including the indentation and rows past the last item);
        // glyphs layer the tree → guide/line → span cascade on top.
        buf.set_style(inner, style);

        let gutter = highlight_symbol.as_deref().unwrap_or("");
        let gutter_width = gutter.chars().count() as u16;
        let bar_style = style.patch(highlight_style);

        let left = inner.left();
        let right = inner.right();
        let top = inner.top();
        // The gutter is reserved before the depth prefix, exactly as `List`
        // reserves it before the label.
        let prefix_left = left.saturating_add(gutter_width);

        for (row, (idx, item)) in items
            .into_iter()
            .enumerate()
            .skip(offset)
            .take(inner.height as usize)
            .enumerate()
        {
            let y = top.saturating_add(row as u16);
            let is_selected = selected == Some(idx);

            // The flattened-row index math is identical to `List`; the prefix
            // is exactly `depth * indent` columns (saturating, so a pathologic
            // depth clips instead of overflowing).
            let depth_cols = item.depth.saturating_mul(indent);
            let content_x0 = prefix_left.saturating_add(depth_cols);

            if is_selected {
                // The selection bar: highlight patched over the base fill
                // across the full inner width, so the gutter, guides, marker,
                // label, and trailing padding read as one contiguous block.
                buf.set_style(Rect::new(left, y, inner.width, 1), highlight_style);

                // The gutter symbol only paints on the selected row; every
                // other row leaves it blank so columns stay put.
                let mut x = left;
                for ch in gutter.chars() {
                    if x >= prefix_left || x >= right {
                        break;
                    }
                    buf.set_cell(Position::new(x, y), ch, bar_style);
                    x = x.saturating_add(1);
                }
            }

            // The depth prefix, styled tree → guide, with the highlight
            // patched **last** on the selected row so it stays one bar.
            let mut prefix_style = style.patch(guide_style);
            if is_selected {
                prefix_style = prefix_style.patch(highlight_style);
            }
            // The marker occupies the prefix's last two cells, just before the
            // label; a leaf leaves them blank so its label still aligns with
            // an expandable sibling at the same depth.
            let marker_glyph = if !item.has_children {
                ' '
            } else if item.expanded {
                '▾'
            } else {
                '▸'
            };
            let marker_x0 = content_x0.saturating_sub(2);
            let marker_x1 = content_x0.saturating_sub(1);
            let has_marker_cells = depth_cols >= 2;

            for x in prefix_left..content_x0.min(right) {
                let col = x - prefix_left;
                let symbol = if has_marker_cells && x == marker_x0 {
                    marker_glyph
                } else if has_marker_cells && x == marker_x1 {
                    ' '
                } else if guides == TreeGuides::Lines
                    && indent > 0
                    && col % indent == 0
                    && col / indent < item.depth.saturating_sub(1)
                {
                    // A continuation rule for an ancestor level — never the
                    // item's own deepest slot (that is the marker's).
                    '│'
                } else {
                    ' '
                };
                buf.set_cell(Position::new(x, y), symbol, prefix_style);
            }

            // Resolve each label glyph through tree → line → span, then patch
            // the highlight last on the selected row — the SAME cascade and
            // right-edge clip loop `List` uses, starting past the prefix (a
            // prefix wider than the inner area clips to nothing here).
            let line = item.label;
            let line_base = style.patch(line.style);
            let mut x = content_x0;
            'row: for span in line.spans {
                let mut span_style = line_base.patch(span.style);
                if is_selected {
                    span_style = span_style.patch(highlight_style);
                }
                for ch in span.content.chars() {
                    if x >= right {
                        break 'row;
                    }
                    buf.set_cell(Position::new(x, y), ch, span_style);
                    x = x.saturating_add(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier};

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row.
    fn lines<W: Widget>(widget: W, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn flat_depth_zero_items_render_like_a_list() {
        // Depth-0 leaves have no prefix at all, so the output is byte-for-byte
        // what `List::new(["abcdef", "XY"])` produces.
        assert_eq!(
            lines(Tree::new(["abcdef", "XY"]), 4, 3),
            "abcd\nXY  \n    \n"
        );
    }

    #[test]
    fn each_depth_level_indents_by_indent_columns() {
        let tree = Tree::new([
            TreeItem::new(0, "a"),
            TreeItem::new(1, "b"),
            TreeItem::new(2, "c"),
        ]);
        // Default indent 2: each level shifts the label two more columns; the
        // reserved (blank, here) marker cells are the last two before it.
        assert_eq!(lines(tree, 6, 3), "a     \n  b   \n    c \n");
    }

    #[test]
    fn expanded_parent_shows_open_marker_collapsed_shows_closed() {
        let tree = Tree::new([
            TreeItem::new(1, "open").expandable(true),
            TreeItem::new(1, "shut").expandable(false),
        ]);
        assert_eq!(lines(tree, 7, 2), "▾ open \n▸ shut \n");
    }

    #[test]
    fn leaf_rows_reserve_the_marker_column_so_labels_align() {
        // The two marker cells are blank on a leaf, so "leaf" and "dir" start
        // in the same column — the reserved-gutter idiom, on the marker.
        let tree = Tree::new([
            TreeItem::new(1, "leaf"),
            TreeItem::new(1, "dir").expandable(false),
        ]);
        assert_eq!(lines(tree, 6, 2), "  leaf\n▸ dir \n");
    }

    #[test]
    fn lines_guides_draw_box_drawing_per_ancestor_level() {
        let tree = Tree::new([
            TreeItem::new(0, "root").expandable(true),
            TreeItem::new(1, "mid").expandable(true),
            TreeItem::new(2, "leaf"),
        ])
        .guides(TreeGuides::Lines);
        // The depth-2 row draws a `│` for its single ancestor level (depth 0),
        // then its own blank leaf marker, then the label.
        assert_eq!(lines(tree, 8, 3), "root    \n▾ mid   \n│   leaf\n");
    }

    #[test]
    fn offset_skips_leading_visible_rows_and_height_clips_trailing() {
        let tree = Tree::new(["i0", "i1", "i2", "i3"]).offset(1);
        assert_eq!(lines(tree, 2, 2), "i1\ni2\n");
    }

    #[test]
    fn an_offset_past_the_end_renders_nothing() {
        let tree = Tree::new(["a", "b"]).offset(5);
        assert_eq!(lines(tree, 3, 2), "   \n   \n");
    }

    #[test]
    fn selection_is_a_full_width_bar_over_guides_label_and_padding() {
        let tree = Tree::new([TreeItem::new(1, "x").expandable(true)])
            .highlight_symbol("> ")
            .selected(Some(0))
            .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        tree.render(buf.area(), &mut buf);
        // Gutter, depth prefix, the ▾ marker, the label, and the trailing
        // padding all share the highlight background — one contiguous bar.
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '>');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, '▾');
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, 'x');
        assert_eq!(buf.get(Position::new(7, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn a_selection_outside_the_visible_window_paints_no_bar() {
        // Row 3 is selected but the window only shows 0..2; nothing is
        // highlighted and rendering does not panic.
        let tree = Tree::new(["a", "b", "c", "d"])
            .selected(Some(3))
            .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        tree.render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Reset);
            }
        }
    }

    #[test]
    fn style_cascades_tree_guide_label_span_and_highlight_wins_last() {
        // Label line is BOLD; one span is red. The tree base is green and the
        // guide style is yellow. On the selected row the highlight bg is
        // patched last (over the guides and the label alike).
        let item = TreeItem::new(
            1,
            Line::from(vec![
                Span::styled("X", Style::new().fg(Color::Red)),
                Span::raw("y"),
            ])
            .style(Style::new().add_modifier(Modifier::BOLD)),
        )
        .expandable(true);
        let tree = Tree::new([item])
            .style(Style::new().fg(Color::Green))
            .guide_style(Style::new().fg(Color::Yellow))
            .selected(Some(0))
            .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        tree.render(buf.area(), &mut buf);

        // The marker is a guide cell: yellow fg from guide_style, blue bg
        // from the highlight patched last.
        let marker = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(marker.symbol, '▾');
        assert_eq!(marker.fg, Color::Yellow);
        assert_eq!(marker.bg, Color::Blue);

        let x = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(x.symbol, 'X');
        assert_eq!(x.fg, Color::Red); // span fg survives
        assert_eq!(x.bg, Color::Blue); // highlight patched last
        assert!(x.modifier.contains(Modifier::BOLD)); // line modifier cascades

        let y = buf.get(Position::new(3, 0)).unwrap();
        assert_eq!(y.symbol, 'y');
        assert_eq!(y.fg, Color::Green); // inherits tree base (no span fg)
        assert_eq!(y.bg, Color::Blue);
        assert!(y.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn block_frames_rows_in_the_inner_area() {
        assert_eq!(
            lines(Tree::new(["hi"]).block(Block::bordered()), 4, 3),
            "┌──┐\n│hi│\n└──┘\n"
        );
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_rows() {
        assert_eq!(
            lines(Tree::new(["Z"]).block(Block::bordered()), 2, 2),
            "┌┐\n└┘\n"
        );
    }

    #[test]
    fn an_empty_tree_with_a_block_still_renders_the_block() {
        assert_eq!(
            lines(Tree::new(Vec::<&str>::new()).block(Block::bordered()), 3, 3),
            "┌─┐\n│ │\n└─┘\n"
        );
    }

    #[test]
    fn indent_zero_collapses_every_depth_to_the_left_edge() {
        // indent 0 → every depth has a zero-width prefix (no marker room);
        // every label is flush left and nothing panics.
        let tree = Tree::new([
            TreeItem::new(0, "a"),
            TreeItem::new(3, "b").expandable(true),
        ])
        .indent(0);
        assert_eq!(lines(tree, 3, 2), "a  \nb  \n");
    }

    #[test]
    fn a_multibyte_label_maps_each_char_to_one_column() {
        // "é" and "日" are multi-byte; each is one column past the reserved
        // (blank, leaf) two-cell marker at depth 1.
        let tree = Tree::new([TreeItem::new(1, "é日")]);
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        tree.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'é');
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, '日');
    }

    #[test]
    fn a_deep_indent_wider_than_the_area_clips_without_panic() {
        // depth 50 × indent 2 = column 100, far past a width-4 area: the
        // prefix clips and the label never starts — no panic.
        let tree = Tree::new([TreeItem::new(50, "x").expandable(true)]);
        assert_eq!(lines(tree, 4, 1), "    \n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Tree::new(["hello"])
            .selected(Some(0))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
