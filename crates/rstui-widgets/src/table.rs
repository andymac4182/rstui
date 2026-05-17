//! [`Table`] — a column-aligned grid of [`Row`]s with an optional fixed
//! [`header`](Table::header) and single-row selection: the basis for data
//! grids, process/file listings, key/value inspectors, and result panes.
//!
//! # The 2D generalization of [`List`](crate::List)
//!
//! [`List`](crate::List) is a column of single-cell rows. `Table` is the same
//! pure projection one dimension wider: each [`Row`] is several cells whose
//! columns are placed by the very same [`Constraint`] vocabulary and
//! deterministic divider [`Layout`] uses, so column widths,
//! percentages, fills, and inter-column spacing all behave exactly as they do
//! for top-level layout — no second width algorithm to learn.
//!
//! Everything else is [`List`](crate::List)'s proven shape, unchanged:
//!
//! * The selection index and scroll [`offset`](Table::offset) are ordinary
//!   application state the reducer owns and mutates in `update`. `Table` only
//!   ever *reads* them — there is no render-time mutation — so it fits
//!   `App::view(&self)` and is deterministically headless-testable. (ratatui's
//!   table is a `StatefulWidget` that mutates the scroll offset during render;
//!   that pattern does not fit rstui — see [`List`](crate::List)'s module docs.)
//! * A pure projection must be **total**: where ratatui *panics* if a width
//!   [`Constraint::Percentage`] exceeds 100, `Table` cannot — a caller-owned
//!   number must never abort the TUI — so it leans on
//!   [`Layout`], which already clamps. Out-of-range
//!   selection/offset simply paint no bar; an empty area is a no-op.
//!
//! Per-cell alignment, a `footer`, column and cell (2D) selection, and column
//! spanning remain deliberately out of scope. Rich styled cells, auto-fit
//! column sizing ([`TableColumnFit`]), and opt-in in-cell soft wrap
//! ([`wrap_cells`](Table::wrap_cells)) are the ADR 0012 §P2 *additive* below —
//! each strictly opt-in, leaving the original `Row`/`Table` API and its
//! single-row, manual-width default behaviour unchanged.

use std::borrow::Cow;

use crate::block::Block;
use crate::paragraph::{Paragraph, Wrap};
use rstui_core::{Buffer, Constraint, Layout, Line, Position, Rect, Style, Widget};

/// How [`Table`] derives its column widths when not sized by hand.
///
/// The default ([`Manual`](Self::Manual)) is the original behaviour: use the
/// caller's [`widths`](Table::widths) constraints (or an equal share when
/// none are given). The two auto-fit modes are opt-in and ignore `widths`,
/// feeding their derived constraints through the *same* deterministic
/// [`Layout`] divider, so they still clamp to the area and never panic.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TableColumnFit {
    /// Use the caller's [`widths`](Table::widths) (equal [`Fill`] when empty)
    /// — the unchanged default.
    ///
    /// [`Fill`]: rstui_core::Constraint::Fill
    #[default]
    Manual,
    /// Size each column to its widest cell (header included), as a
    /// [`Length`](rstui_core::Constraint::Length) per column. The layout
    /// divider scales them down proportionally if their sum overflows the
    /// area, so it is total.
    Proportional,
    /// Split the columns into an equal share regardless of content or the
    /// caller's `widths` (an explicit, `widths`-overriding even split).
    Balanced,
}

/// One row of a [`Table`]: an ordered list of cells, one per column.
///
/// A cell is exactly one [`Line`] (the same single-visual-row scoping
/// [`List`](crate::List) uses), so build a row from anything a [`Line`] is
/// built from — `&str`, `String`, [`Span`](rstui_core::Span), [`Line`] — and
/// style individual cells through the [`Line`]s themselves. A row-wide base
/// [`Style`] sits beneath every cell via [`style`](Row::style).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Row<'a> {
    cells: Vec<Line<'a>>,
    style: Style,
}

impl<'a> Row<'a> {
    /// A row whose cells are `cells` (each convertible to a [`Line`]).
    pub fn new<I, T>(cells: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Line<'a>>,
    {
        Self {
            cells: cells.into_iter().map(Into::into).collect(),
            style: Style::default(),
        }
    }

    /// Sets the row's base [`Style`], beneath the table → row → cell → span
    /// cascade.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

/// A column-aligned grid of [`Row`]s with single-row selection.
///
/// `Table` shows the data rows in the window `[offset, offset + visible)` — one
/// per row — below an optional non-scrolling [`header`](Self::header). Columns
/// are placed by resolving [`widths`](Self::widths) (any [`Constraint`]s) with
/// [`column_spacing`](Self::column_spacing) through the same deterministic
/// [`Layout`] divider top-level layout uses; when no widths
/// are given the columns split the space equally.
///
/// Styling cascades table → row → cell-line → span (the
/// [`Style::patch`](rstui_core::Style) model the text model uses); the table
/// base style also fills the content area so a background covers the whole
/// pane. On the [`selected`](Self::selected) data row
/// [`highlight_style`](Self::highlight_style) is patched **last** and applied
/// across the full inner width, so the selection reads as one contiguous bar
/// over the gutter, every column, the inter-column gaps, and trailing padding —
/// exactly like [`List`](crate::List).
///
/// The [`highlight_symbol`](Self::highlight_symbol) gutter is reserved (blank)
/// on every row — header and data — whenever one is set, so cell columns never
/// shift as the selection moves.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Constraint, Position, Rect, Widget};
/// use rstui_widgets::{Row, Table};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 9, 3));
/// Table::new(
///     [Row::new(["a", "1"]), Row::new(["b", "2"])],
///     [Constraint::Length(3), Constraint::Length(3)],
/// )
/// .header(Row::new(["L", "R"]))
/// .highlight_symbol("> ")
/// .selected(Some(1))
/// .render(buf.area(), &mut buf);
///
/// // Header on the top row, columns aligned with the data below it.
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'L');
/// assert_eq!(buf.get(Position::new(6, 0)).unwrap().symbol, 'R');
/// assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, 'a');
/// // The gutter is reserved on every row but painted only on the selection…
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, ' ');
/// assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, '>');
/// assert_eq!(buf.get(Position::new(2, 2)).unwrap().symbol, 'b');
/// ```
#[derive(Debug, Clone)]
pub struct Table<'a> {
    rows: Vec<Row<'a>>,
    header: Option<Row<'a>>,
    widths: Vec<Constraint>,
    column_spacing: u16,
    column_fit: TableColumnFit,
    wrap_cells: bool,
    block: Option<Block<'a>>,
    style: Style,
    highlight_style: Style,
    highlight_symbol: Option<Cow<'a, str>>,
    selected: Option<usize>,
    offset: usize,
}

impl Default for Table<'_> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            header: None,
            widths: Vec::new(),
            // ratatui's default, and the sensible one: one blank column
            // between cells so adjacent values never visually merge.
            column_spacing: 1,
            column_fit: TableColumnFit::Manual,
            wrap_cells: false,
            block: None,
            style: Style::default(),
            highlight_style: Style::default(),
            highlight_symbol: None,
            selected: None,
            offset: 0,
        }
    }
}

impl<'a> Table<'a> {
    /// A table of `rows` whose columns are sized by `widths`.
    ///
    /// An empty `widths` makes every column an equal share of the area.
    pub fn new<R, C>(rows: R, widths: C) -> Self
    where
        R: IntoIterator<Item = Row<'a>>,
        C: IntoIterator<Item = Constraint>,
    {
        Self {
            rows: rows.into_iter().collect(),
            widths: widths.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Replaces the data rows.
    #[must_use]
    pub fn rows<R>(mut self, rows: R) -> Self
    where
        R: IntoIterator<Item = Row<'a>>,
    {
        self.rows = rows.into_iter().collect();
        self
    }

    /// Sets a fixed header row drawn above the data, never scrolled or
    /// selected. Its cells align with the data columns.
    #[must_use]
    pub fn header(mut self, header: Row<'a>) -> Self {
        self.header = Some(header);
        self
    }

    /// Sets the per-column width [`Constraint`]s.
    ///
    /// Resolved through the same deterministic [`Layout`]
    /// divider as top-level layout. Unlike ratatui this never panics on an
    /// out-of-range [`Constraint::Percentage`]; it is clamped.
    #[must_use]
    pub fn widths<C>(mut self, widths: C) -> Self
    where
        C: IntoIterator<Item = Constraint>,
    {
        self.widths = widths.into_iter().collect();
        self
    }

    /// Sets the number of blank cells between adjacent columns (default `1`).
    #[must_use]
    pub fn column_spacing(mut self, spacing: u16) -> Self {
        self.column_spacing = spacing;
        self
    }

    /// Sets how columns are sized (default [`TableColumnFit::Manual`], the
    /// original [`widths`](Self::widths)-driven behaviour).
    ///
    /// [`Proportional`](TableColumnFit::Proportional) /
    /// [`Balanced`](TableColumnFit::Balanced) are opt-in auto-fit modes that
    /// ignore `widths`; both still resolve through the same clamping
    /// [`Layout`] divider, so an over-wide table scales down rather than
    /// panicking.
    #[must_use]
    pub fn column_fit(mut self, fit: TableColumnFit) -> Self {
        self.column_fit = fit;
        self
    }

    /// Enables opt-in in-cell soft word wrap (default `false`).
    ///
    /// When `true`, a cell whose [`Line`] is wider than its column wraps onto
    /// extra rows by **reusing [`Paragraph`]'s** soft wrap (so the wrap is
    /// computed exactly one way, via
    /// [`Paragraph::line_count`](crate::Paragraph::line_count)); each data
    /// row's height becomes the tallest of its wrapped cells and the
    /// selection bar/gutter span the whole row. The header stays one row.
    /// Off (the default) every row is exactly one visual row, unchanged.
    #[must_use]
    pub fn wrap_cells(mut self, wrap: bool) -> Self {
        self.wrap_cells = wrap;
        self
    }

    /// Frames the table in `block`; rows render into [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`], beneath the table → row → cell → span cascade.
    /// It also fills the content area so a background covers the whole pane.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched over the selected data row.
    ///
    /// Patched **last** in the cascade and applied across the full inner
    /// width, so the selection reads as one bar (the same idiom as
    /// [`List`](crate::List)).
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Sets the gutter string drawn before the selected data row.
    ///
    /// The gutter is reserved (blank) on every other row — and on the header —
    /// so cell columns keep their position as the selection moves.
    #[must_use]
    pub fn highlight_symbol(mut self, symbol: impl Into<Cow<'a, str>>) -> Self {
        self.highlight_symbol = Some(symbol.into());
        self
    }

    /// Sets which data-row index is highlighted, or `None` for no selection.
    ///
    /// An index outside the visible window simply paints no bar — the caller
    /// owns scrolling (see the [module docs](self)).
    #[must_use]
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the index of the first visible data row (the scroll offset).
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }
}

impl Widget for Table<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Table {
            rows,
            header,
            widths,
            column_spacing,
            column_fit,
            wrap_cells,
            block,
            style,
            highlight_style,
            highlight_symbol,
            selected,
            offset,
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

        // Table base fills the content area so a background covers the whole
        // pane (gutter, gaps, and rows past the last item included); glyphs
        // layer the table → row → cell → span cascade on top.
        buf.set_style(inner, style);

        // The selection gutter is reserved on every row (including the
        // header), exactly like List, so columns never shift on selection.
        let gutter = highlight_symbol.as_deref().unwrap_or("");
        let gutter_width = gutter.chars().count() as u16;
        let bar_style = style.patch(highlight_style);

        // Columns are placed in the area *after* the gutter, by the same
        // deterministic divider top-level layout uses.
        let columns_area = Rect::new(
            inner.x.saturating_add(gutter_width),
            inner.y,
            inner.width.saturating_sub(gutter_width),
            inner.height,
        );
        let col_count = if widths.is_empty() {
            rows.iter()
                .chain(header.iter())
                .map(|r| r.cells.len())
                .max()
                .unwrap_or(0)
        } else {
            widths.len()
        };
        if col_count == 0 {
            return;
        }
        // Manual (default) is unchanged: caller widths, or an equal Fill when
        // none. The two auto-fit modes are opt-in and override `widths`.
        let constraints: Vec<Constraint> = match column_fit {
            TableColumnFit::Manual => {
                if widths.is_empty() {
                    vec![Constraint::Fill(1); col_count]
                } else {
                    widths
                }
            }
            TableColumnFit::Balanced => vec![Constraint::Fill(1); col_count],
            TableColumnFit::Proportional => {
                // Each column's widest cell (header included) as a Length; the
                // divider scales them down if their sum overflows the area.
                let mut max_w = vec![0u16; col_count];
                for r in rows.iter().chain(header.iter()) {
                    for (i, cell) in r.cells.iter().enumerate().take(col_count) {
                        let w = u16::try_from(cell.width()).unwrap_or(u16::MAX);
                        max_w[i] = max_w[i].max(w);
                    }
                }
                max_w.into_iter().map(Constraint::Length).collect()
            }
        };
        let column_rects = Layout::horizontal(constraints)
            .spacing(column_spacing)
            .split(columns_area);

        // An optional header takes the top inner row; data scrolls below it.
        let has_header = header.is_some();
        let header_rows = u16::from(has_header);
        if let Some(header) = header {
            render_row(
                buf,
                &header,
                &column_rects,
                inner.top(),
                inner.right(),
                style,
                None,
            );
        }
        let data_top = inner.top().saturating_add(header_rows);
        let data_height = inner.height.saturating_sub(header_rows);

        if wrap_cells {
            // Opt-in multi-row path: each cell soft-wraps by reusing
            // Paragraph (one wrap implementation, via Paragraph::line_count),
            // so a row is as tall as its tallest wrapped cell. The selection
            // bar/gutter span the whole row and the highlight is patched LAST
            // over the rendered glyphs (set_style patches), keeping the
            // single-bar idiom even when rows are multi-line.
            let data_bottom = data_top.saturating_add(data_height);
            let mut y = data_top;
            for (idx, row) in rows.iter().enumerate().skip(offset) {
                if y >= data_bottom {
                    break;
                }
                let row_base = style.patch(row.style);
                // T1: build each cell's Paragraph ONCE (one Line clone) and
                // reuse the same instance for the height measure *and* the
                // render. The two-pass shape (measure every cell → render all
                // at the shared row_h) previously deep-cloned every visible
                // cell's `Line` (Vec<Span> + Cow) TWICE per frame; stashing
                // the Paragraphs halves that to one clone/cell. The extra
                // per-row Vec is column-count sized (tiny, bounded).
                let mut paras: Vec<Paragraph<'_>> = Vec::with_capacity(row.cells.len());
                let mut row_h: u16 = 1;
                for (cell, col) in row.cells.iter().zip(&column_rects) {
                    let p = Paragraph::new(cell.clone()).wrap(Wrap { trim: false });
                    let h = u16::try_from(p.line_count(col.width))
                        .unwrap_or(u16::MAX)
                        .max(1);
                    row_h = row_h.max(h);
                    paras.push(p);
                }
                let row_h = row_h.min(data_bottom.saturating_sub(y));
                let is_selected = selected == Some(idx);

                for (p, col) in paras.into_iter().zip(&column_rects) {
                    let cell_w = col.width.min(inner.right().saturating_sub(col.x));
                    let cell_area = Rect::new(col.x, y, cell_w, row_h);
                    // Paragraph cascades base → line(cell) → span itself, so
                    // the table → row base is enough here. `p` already has
                    // `wrap` set (built above) — same config as before.
                    p.style(row_base).render(cell_area, buf);
                }

                if is_selected {
                    // Highlight patched LAST across the full row height: the
                    // gutter, columns, gaps, and padding read as one bar.
                    buf.set_style(
                        Rect::new(inner.left(), y, inner.width, row_h),
                        highlight_style,
                    );
                    let mut x = inner.left();
                    for ch in gutter.chars() {
                        if x >= columns_area.x || x >= inner.right() {
                            break;
                        }
                        buf.set_cell(Position::new(x, y), ch, bar_style);
                        x = x.saturating_add(1);
                    }
                }
                y = y.saturating_add(row_h);
            }
            return;
        }

        for (row_i, (idx, row)) in rows
            .iter()
            .enumerate()
            .skip(offset)
            .take(data_height as usize)
            .enumerate()
        {
            let y = data_top.saturating_add(row_i as u16);
            let is_selected = selected == Some(idx);

            if is_selected {
                // The selection bar: highlight patched over the base fill
                // across the full inner width, so the gutter, every column,
                // the inter-column gaps, and trailing padding read as one
                // contiguous block — not just the glyph cells.
                buf.set_style(Rect::new(inner.left(), y, inner.width, 1), highlight_style);

                // The gutter symbol only paints on the selected row; every
                // other row leaves it blank so columns stay put.
                let mut x = inner.left();
                for ch in gutter.chars() {
                    if x >= columns_area.x || x >= inner.right() {
                        break;
                    }
                    buf.set_cell(Position::new(x, y), ch, bar_style);
                    x = x.saturating_add(1);
                }
            }

            let highlight = is_selected.then_some(highlight_style);
            render_row(buf, row, &column_rects, y, inner.right(), style, highlight);
        }
    }
}

/// Stamps one [`Row`]'s cells into their column rects on row `y`, resolving
/// each glyph through table → row → cell-line → span and patching `highlight`
/// last when the row is the selection.
fn render_row(
    buf: &mut Buffer,
    row: &Row,
    column_rects: &[Rect],
    y: u16,
    right: u16,
    table_style: Style,
    highlight: Option<Style>,
) {
    let row_base = table_style.patch(row.style);
    for (cell, col) in row.cells.iter().zip(column_rects) {
        let cell_base = row_base.patch(cell.style);
        let col_right = col.x.saturating_add(col.width).min(right);
        let mut x = col.x;
        'cell: for span in &cell.spans {
            let mut span_style = cell_base.patch(span.style);
            if let Some(hl) = highlight {
                span_style = span_style.patch(hl);
            }
            for ch in span.content.chars() {
                if x >= col_right {
                    break 'cell;
                }
                buf.set_cell(Position::new(x, y), ch, span_style);
                x = x.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier, Span};

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row.
    fn grid<W: Widget>(widget: W, width: u16, height: u16) -> String {
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
    fn columns_are_placed_by_constraints_with_spacing() {
        let table = Table::new(
            [Row::new(["ab", "cd"]), Row::new(["ef", "gh"])],
            [Constraint::Length(2), Constraint::Length(2)],
        );
        // width 5 = 2 + 1 (default column_spacing) + 2.
        assert_eq!(grid(table, 5, 2), "ab cd\nef gh\n");
    }

    #[test]
    fn empty_widths_split_columns_equally() {
        let table =
            Table::new([Row::new(["a", "b", "c"])], Vec::<Constraint>::new()).column_spacing(0);
        // 6 / 3 == 2 cells per column.
        assert_eq!(grid(table, 6, 1), "a b c \n");
    }

    #[test]
    fn header_is_the_top_row_and_data_scrolls_below_it() {
        let table = Table::new(
            [Row::new(["r0"]), Row::new(["r1"]), Row::new(["r2"])],
            [Constraint::Length(2)],
        )
        .header(Row::new(["HD"]))
        .offset(1);
        // Header is fixed on row 0; data starts at the offset (r1), unaffected
        // by the header occupying its own row.
        assert_eq!(grid(table, 2, 3), "HD\nr1\nr2\n");
    }

    #[test]
    fn highlight_symbol_gutter_is_reserved_on_every_row_including_header() {
        let table = Table::new(
            [Row::new(["one"]), Row::new(["two"])],
            [Constraint::Length(3)],
        )
        .header(Row::new(["hdr"]))
        .highlight_symbol("> ")
        .selected(Some(1));
        // Gutter blank on the header and the unselected row; painted only on
        // the selected data row — every cell stays in the same column.
        assert_eq!(grid(table, 5, 3), "  hdr\n  one\n> two\n");
    }

    #[test]
    fn no_selection_paints_no_gutter_symbol_anywhere() {
        let table = Table::new(
            [Row::new(["one"]), Row::new(["two"])],
            [Constraint::Length(3)],
        )
        .highlight_symbol("> ");
        assert_eq!(grid(table, 5, 2), "  one\n  two\n");
    }

    #[test]
    fn highlight_style_is_a_full_width_bar_over_gutter_columns_gaps_and_padding() {
        let table = Table::new(
            [Row::new(["x", "y"])],
            [Constraint::Length(1), Constraint::Length(1)],
        )
        .highlight_symbol("> ")
        .selected(Some(0))
        .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 7, 1));
        table.render(buf.area(), &mut buf);
        // Gutter, both columns, the inter-column gap, and trailing padding all
        // share the highlight background — one contiguous bar.
        for x in 0..7 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '>');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'x');
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, 'y');
    }

    #[test]
    fn style_cascades_table_row_cell_span_and_highlight_wins_last() {
        // Cell line is BOLD; its one span is red. The row base is yellow, the
        // table base is green. On the selected row the highlight bg is patched
        // last (over everything).
        let cell = Line::from(vec![Span::styled("X", Style::new().fg(Color::Red))])
            .style(Style::new().add_modifier(Modifier::BOLD));
        let row = Row::new([cell]).style(Style::new().fg(Color::Yellow));
        let table = Table::new([row], [Constraint::Length(1)])
            .style(Style::new().fg(Color::Green))
            .selected(Some(0))
            .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        table.render(buf.area(), &mut buf);

        let c = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(c.symbol, 'X');
        assert_eq!(c.fg, Color::Red); // span fg survives the cascade
        assert_eq!(c.bg, Color::Blue); // highlight patched last
        assert!(c.modifier.contains(Modifier::BOLD)); // cell-line modifier
    }

    #[test]
    fn row_base_style_shows_through_where_a_span_sets_nothing() {
        let row = Row::new(["ab"]).style(Style::new().fg(Color::Magenta));
        let table = Table::new([row], [Constraint::Length(2)]);
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        table.render(buf.area(), &mut buf);
        for x in 0..2 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().fg, Color::Magenta);
        }
    }

    #[test]
    fn base_style_fills_the_whole_content_area() {
        let table = Table::new([Row::new(["x"])], [Constraint::Length(1)])
            .style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        table.render(buf.area(), &mut buf);
        // The single row is line 0; the empty cells and row are still filled.
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
    }

    #[test]
    fn block_frames_the_table_in_the_inner_area() {
        let table =
            Table::new([Row::new(["hi"])], [Constraint::Length(2)]).block(Block::bordered());
        assert_eq!(grid(table, 4, 3), "┌──┐\n│hi│\n└──┘\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_rows() {
        let table = Table::new([Row::new(["Z"])], [Constraint::Length(1)]).block(Block::bordered());
        assert_eq!(grid(table, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn an_empty_table_with_a_block_still_renders_the_block() {
        let table = Table::new(Vec::<Row>::new(), [Constraint::Length(1)]).block(Block::bordered());
        assert_eq!(grid(table, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn a_selection_outside_the_visible_window_paints_no_bar() {
        // Row 3 is selected but the offset/height window only shows 0..2;
        // nothing is highlighted and rendering does not panic.
        let table = Table::new(
            [
                Row::new(["a"]),
                Row::new(["b"]),
                Row::new(["c"]),
                Row::new(["d"]),
            ],
            [Constraint::Length(1)],
        )
        .selected(Some(3))
        .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 2));
        table.render(buf.area(), &mut buf);
        for y in 0..2 {
            assert_eq!(buf.get(Position::new(0, y)).unwrap().bg, Color::Reset);
        }
    }

    #[test]
    fn an_out_of_range_percentage_is_clamped_not_a_panic() {
        // ratatui panics in `widths()` on Percentage > 100; a pure projection
        // must be total, so this clamps and renders without aborting.
        let table = Table::new([Row::new(["ab"])], [Constraint::Percentage(250)]);
        assert_eq!(grid(table, 2, 1), "ab\n");
    }

    #[test]
    fn extra_cells_and_extra_columns_are_both_safe() {
        // More cells than resolved columns: the surplus cell is clipped away.
        let wide = Table::new([Row::new(["a", "b", "c"])], [Constraint::Length(1)]);
        assert_eq!(grid(wide, 1, 1), "a\n");
        // Fewer cells than columns: trailing columns are simply blank.
        let narrow = Table::new(
            [Row::new(["a"])],
            [Constraint::Length(1), Constraint::Length(1)],
        )
        .column_spacing(0);
        assert_eq!(grid(narrow, 2, 1), "a \n");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Table::new([Row::new(["hello"])], [Constraint::Length(5)])
            .selected(Some(0))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    // ---- ADR 0012 §P2 additive: column-fit + in-cell wrap ----

    #[test]
    fn proportional_fit_sizes_each_column_to_its_widest_cell() {
        // col0 widest = 1 ("x"), col1 widest = 4 ("yyyy"); Proportional ⇒
        // Length(1), Length(4) ⇒ exactly fills width 5 (widths ignored).
        let table = Table::new(
            [Row::new(["x", "yyyy"]), Row::new(["a", "b"])],
            Vec::<Constraint>::new(),
        )
        .column_spacing(0)
        .column_fit(TableColumnFit::Proportional);
        assert_eq!(grid(table, 5, 2), "xyyyy\nab   \n");
    }

    #[test]
    fn balanced_fit_overrides_widths_with_an_even_split() {
        // Widths say [1, 1] but Balanced forces an equal Fill split: in
        // width 4 each column is 2 cells wide.
        let table = Table::new(
            [Row::new(["a", "b"])],
            [Constraint::Length(1), Constraint::Length(1)],
        )
        .column_spacing(0)
        .column_fit(TableColumnFit::Balanced);
        assert_eq!(grid(table, 4, 1), "a b \n");
    }

    #[test]
    fn manual_fit_is_the_unchanged_default_behaviour() {
        // Explicit Manual == the original path: caller widths honoured.
        let table = Table::new(
            [Row::new(["ab", "cd"])],
            [Constraint::Length(2), Constraint::Length(2)],
        )
        .column_fit(TableColumnFit::Manual);
        assert_eq!(grid(table, 5, 1), "ab cd\n");
    }

    #[test]
    fn wrap_cells_makes_a_row_as_tall_as_its_tallest_wrapped_cell() {
        // col0 "abcd" hard-wraps to 2 rows at width 2; col1 "z" is 1 row, so
        // the row is 2 tall. Reuses Paragraph's wrap (one implementation).
        let table = Table::new(
            [Row::new(["abcd", "z"])],
            [Constraint::Length(2), Constraint::Length(1)],
        )
        .column_spacing(0)
        .wrap_cells(true);
        assert_eq!(grid(table, 3, 2), "abz\ncd \n");
    }

    #[test]
    fn wrap_cells_stacks_successive_rows_below_their_full_height() {
        let table = Table::new(
            [Row::new(["abcd"]), Row::new(["ef"])],
            [Constraint::Length(2)],
        )
        .column_spacing(0)
        .wrap_cells(true);
        // Row 0 is 2 tall ("ab"/"cd"); row 1 ("ef") starts on line 2.
        assert_eq!(grid(table, 2, 3), "ab\ncd\nef\n");
    }

    #[test]
    fn wrap_cells_off_is_still_exactly_one_visual_row() {
        // The default path is byte-for-byte unchanged: long cell is clipped,
        // not wrapped.
        let table = Table::new([Row::new(["abcd"])], [Constraint::Length(2)]);
        assert_eq!(grid(table, 2, 2), "ab\n  \n");
    }

    #[test]
    fn wrap_cells_selection_bar_spans_the_full_row_height() {
        let table = Table::new([Row::new(["abcd"])], [Constraint::Length(2)])
            .column_spacing(0)
            .wrap_cells(true)
            .selected(Some(0))
            .highlight_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        table.render(buf.area(), &mut buf);
        // Both wrapped rows of the selected cell carry the highlight bg.
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Blue);
            }
        }
    }

    #[test]
    fn wrap_cells_keeps_a_styled_line_cells_span_styles() {
        // Rich cell: a multi-span styled Line still cascades under the wrap
        // path (table → row → cell-line → span via the reused Paragraph).
        let cell = Line::from(vec![
            Span::styled("ab", Style::new().fg(Color::Red)),
            Span::styled("cd", Style::new().fg(Color::Green)),
        ]);
        let table = Table::new([Row::new([cell])], [Constraint::Length(2)])
            .column_spacing(0)
            .wrap_cells(true);
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        table.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Red); // "ab"
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().fg, Color::Green); // "cd"
    }
}
