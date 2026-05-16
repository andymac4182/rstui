//! [`Grid`] — a two-dimensional layout primitive that tiles an area into
//! rows × columns of [`Rect`] cells; the basis for dashboards, button pads,
//! property sheets, and any regular 2-D arrangement.
//!
//! # Pure layout, owns no state — and never reinvents the solver
//!
//! Like [`SplitPane`](crate::SplitPane) and
//! [`Accordion`](crate::Accordion), `Grid` takes **no child widgets**: it is
//! pure geometry. [`split`](Grid::split) is a pure function of the area and the
//! two [`Constraint`] lists — it hands back one [`Rect`] per cell and the
//! caller renders its own content into them, so a grid composes with anything
//! (including a nested `Grid`) without the widget knowing what lives inside. It
//! mutates nothing at render time, so it fits `App::view(&self)` and is
//! deterministically headless-testable.
//!
//! The cell sizing is **not** a new algorithm: each axis is resolved by the
//! core [`Layout`] divider — the very same deterministic, integer-only,
//! float-free solver [`SplitPane`](crate::SplitPane) and
//! [`Table`](crate::Table) columns use. `Grid` only composes it twice (rows
//! down the area, then columns across each row band), so a degenerate
//! constraint, an oversized one, and an area too small all clamp exactly the
//! way every other rstui layout does — fully tiled, never a panic.
//!
//! # Deliberately deferred
//!
//! Cell spanning / merged cells, a per-cell minimum, and grid rule lines drawn
//! between cells are additive follow-ups that compose from this row×column
//! shape rather than changing it — so they are not smuggled in here. The
//! inter-cell [`row_spacing`](Grid::row_spacing) /
//! [`column_spacing`](Grid::column_spacing) gutters (the core
//! [`Layout::spacing`] every divider already reserves) are the only spacing
//! this slice grows.

use rstui_core::{Buffer, Constraint, Direction, Layout, Rect, Style, Widget};

use crate::Block;

/// A 2-D layout that tiles an area into `rows × columns` cells sized by core
/// [`Constraint`]s.
///
/// [`split`](Self::split) divides the area top-to-bottom by the
/// [`rows`](Self::rows) constraints into row bands, then divides each band
/// left-to-right by the [`columns`](Self::columns) constraints — reusing the
/// core [`Layout`] divider on each axis (never a new solver). The result is a
/// `Vec<Vec<Rect>>` (`cells[row][column]`); render your own content into each.
/// An optional framing [`Block`] composes exactly as it does for every other
/// container widget, and the base [`style`](Self::style) fills the content area
/// so a background covers the whole region.
///
/// # Example
///
/// ```
/// use rstui_core::{Constraint, Rect};
/// use rstui_widgets::Grid;
///
/// // A 2×2 grid: two equal rows, a fixed 4-wide first column then the rest.
/// let grid = Grid::new(
///     [Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)],
///     [Constraint::Length(4), Constraint::Fill(1)],
/// );
/// let cells = grid.split(Rect::new(0, 0, 10, 4));
/// assert_eq!(cells[0][0], Rect::new(0, 0, 4, 2)); // top-left
/// assert_eq!(cells[0][1], Rect::new(4, 0, 6, 2)); // top-right
/// assert_eq!(cells[1][1], Rect::new(4, 2, 6, 2)); // bottom-right
///
/// // Or address a single cell directly.
/// assert_eq!(grid.cell(Rect::new(0, 0, 10, 4), 1, 0), Some(Rect::new(0, 2, 4, 2)));
/// ```
#[derive(Debug, Clone)]
pub struct Grid<'a> {
    rows: Vec<Constraint>,
    columns: Vec<Constraint>,
    row_spacing: u16,
    column_spacing: u16,
    style: Style,
    block: Option<Block<'a>>,
}

impl<'a> Grid<'a> {
    /// A grid whose row heights are sized by `rows` (top to bottom) and column
    /// widths by `columns` (left to right), with no gutters or frame.
    pub fn new<R, C, IR, IC>(rows: IR, columns: IC) -> Self
    where
        IR: IntoIterator<Item = R>,
        IC: IntoIterator<Item = C>,
        R: Into<Constraint>,
        C: Into<Constraint>,
    {
        Self {
            rows: rows.into_iter().map(Into::into).collect(),
            columns: columns.into_iter().map(Into::into).collect(),
            row_spacing: 0,
            column_spacing: 0,
            style: Style::new(),
            block: None,
        }
    }

    /// Replaces the row-height [`Constraint`]s (resolved top to bottom).
    #[must_use]
    pub fn rows<I, T>(mut self, rows: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Constraint>,
    {
        self.rows = rows.into_iter().map(Into::into).collect();
        self
    }

    /// Replaces the column-width [`Constraint`]s (resolved left to right).
    #[must_use]
    pub fn columns<I, T>(mut self, columns: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Constraint>,
    {
        self.columns = columns.into_iter().map(Into::into).collect();
        self
    }

    /// Sets both the row and column gutter to `spacing` cells (the core
    /// [`Layout::spacing`] every divider reserves).
    #[must_use]
    pub fn spacing(mut self, spacing: u16) -> Self {
        self.row_spacing = spacing;
        self.column_spacing = spacing;
        self
    }

    /// Sets the gutter reserved between adjacent rows (default `0`).
    #[must_use]
    pub fn row_spacing(mut self, spacing: u16) -> Self {
        self.row_spacing = spacing;
        self
    }

    /// Sets the gutter reserved between adjacent columns (default `0`).
    #[must_use]
    pub fn column_spacing(mut self, spacing: u16) -> Self {
        self.column_spacing = spacing;
        self
    }

    /// Sets the base [`Style`], beneath the caller's cells. It fills the
    /// content area so a background covers the whole region.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Frames the grid in `block`; the cells are placed inside
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// The framed content rect: [`block.inner`](Block::inner) of `area`, or the
    /// whole `area` when there is no block. The cells are tiled inside this,
    /// exactly as with [`SplitPane::inner`](crate::SplitPane::inner).
    #[must_use]
    pub fn inner(&self, area: Rect) -> Rect {
        match &self.block {
            Some(block) => block.inner(area),
            None => area,
        }
    }

    /// The cell rects for `area`, as `cells[row][column]`.
    ///
    /// A pure function of `area` and the configuration: the inner area is
    /// divided top-to-bottom by [`rows`](Self::rows), then each band
    /// left-to-right by [`columns`](Self::columns), each axis through the core
    /// [`Layout`] divider. With no rows the result is empty; with rows but no
    /// columns each row is an empty `Vec`; a zero-area or oversized area clamps
    /// to well-formed (possibly zero-sized) cells rather than panicking.
    #[must_use]
    pub fn split(&self, area: Rect) -> Vec<Vec<Rect>> {
        if self.rows.is_empty() {
            return Vec::new();
        }
        let bands = Layout::new(Direction::Vertical, self.rows.clone())
            .spacing(self.row_spacing)
            .split(self.inner(area));
        bands
            .into_iter()
            .map(|band| {
                if self.columns.is_empty() {
                    Vec::new()
                } else {
                    Layout::new(Direction::Horizontal, self.columns.clone())
                        .spacing(self.column_spacing)
                        .split(band)
                }
            })
            .collect()
    }

    /// The rect of a single cell, or `None` if `row`/`column` is out of range.
    ///
    /// The flat-indexed companion to [`split`](Self::split) for the common
    /// "just give me cell (r, c)" call site.
    #[must_use]
    pub fn cell(&self, area: Rect, row: usize, column: usize) -> Option<Rect> {
        self.split(area)
            .get(row)
            .and_then(|cols| cols.get(column))
            .copied()
    }
}

impl Widget for Grid<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // The block (if any) frames the content and reserves the inner area.
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if let Some(b) = self.block {
            b.render(area, buf);
        }
        if inner.is_empty() {
            return;
        }

        // Base fills the content area so a background covers the whole region;
        // the caller's per-cell content layers on top (Grid draws no content).
        buf.set_style(inner, self.style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Position};

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row (the list.rs helper).
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
    fn tiles_rows_then_columns_into_a_rect_grid() {
        let grid = Grid::new(
            [Constraint::Length(1), Constraint::Fill(1)],
            [Constraint::Length(2), Constraint::Fill(1)],
        );
        let cells = grid.split(Rect::new(0, 0, 6, 4));
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0], vec![Rect::new(0, 0, 2, 1), Rect::new(2, 0, 4, 1)]);
        assert_eq!(cells[1], vec![Rect::new(0, 1, 2, 3), Rect::new(2, 1, 4, 3)]);
    }

    #[test]
    fn equal_ratio_rows_and_columns_split_evenly() {
        let grid = Grid::new(
            [Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)],
            [Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)],
        );
        let cells = grid.split(Rect::new(0, 0, 8, 6));
        assert_eq!(cells[0][0], Rect::new(0, 0, 4, 3));
        assert_eq!(cells[1][1], Rect::new(4, 3, 4, 3));
    }

    #[test]
    fn cell_addresses_a_single_rect_and_clamps_out_of_range_to_none() {
        let grid = Grid::new([Constraint::Fill(1); 2], [Constraint::Fill(1); 2]);
        let area = Rect::new(0, 0, 10, 4);
        assert_eq!(grid.cell(area, 0, 1), Some(Rect::new(5, 0, 5, 2)));
        assert_eq!(grid.cell(area, 1, 0), Some(Rect::new(0, 2, 5, 2)));
        assert_eq!(grid.cell(area, 2, 0), None); // row out of range
        assert_eq!(grid.cell(area, 0, 9), None); // column out of range
    }

    #[test]
    fn spacing_reserves_gutters_between_rows_and_columns() {
        let grid = Grid::new(
            [Constraint::Fill(1), Constraint::Fill(1)],
            [Constraint::Fill(1), Constraint::Fill(1)],
        )
        .spacing(1);
        let cells = grid.split(Rect::new(0, 0, 5, 5));
        // 5 cells, 1 gap → 2-cell segments on each axis with a 1-cell gutter.
        assert_eq!(cells[0][0], Rect::new(0, 0, 2, 2));
        assert_eq!(cells[1][1], Rect::new(3, 3, 2, 2));
    }

    #[test]
    fn per_axis_spacing_is_independent() {
        let grid = Grid::new(
            [Constraint::Fill(1), Constraint::Fill(1)],
            [Constraint::Fill(1), Constraint::Fill(1)],
        )
        .row_spacing(2)
        .column_spacing(0);
        let cells = grid.split(Rect::new(0, 0, 4, 6));
        // Columns are contiguous (gap 0); rows have a 2-row gutter.
        assert_eq!(cells[0][0], Rect::new(0, 0, 2, 2));
        assert_eq!(cells[0][1], Rect::new(2, 0, 2, 2));
        assert_eq!(cells[1][0], Rect::new(0, 4, 2, 2));
    }

    #[test]
    fn no_rows_is_an_empty_grid() {
        let grid = Grid::new(Vec::<Constraint>::new(), [Constraint::Fill(1)]);
        assert!(grid.split(Rect::new(0, 0, 10, 10)).is_empty());
        assert_eq!(grid.cell(Rect::new(0, 0, 10, 10), 0, 0), None);
    }

    #[test]
    fn rows_but_no_columns_yields_empty_rows() {
        let grid = Grid::new(
            [Constraint::Fill(1), Constraint::Fill(1)],
            Vec::<Constraint>::new(),
        );
        let cells = grid.split(Rect::new(0, 0, 10, 10));
        assert_eq!(cells.len(), 2);
        assert!(cells.iter().all(Vec::is_empty));
    }

    #[test]
    fn a_zero_area_yields_well_formed_zero_sized_cells_not_a_panic() {
        let grid = Grid::new([Constraint::Fill(1); 2], [Constraint::Fill(1); 3]);
        let cells = grid.split(Rect::new(4, 5, 0, 0));
        assert_eq!(cells.len(), 2);
        for row in &cells {
            assert_eq!(row.len(), 3);
            assert!(row.iter().all(|r| r.is_empty()));
        }
    }

    #[test]
    fn an_oversized_fixed_constraint_is_clamped_to_fit_not_a_panic() {
        let grid = Grid::new([Constraint::Length(999)], [Constraint::Length(999)]);
        let cells = grid.split(Rect::new(0, 0, 6, 3));
        // The single cell is scaled into the area, never exceeding it.
        assert_eq!(cells[0][0], Rect::new(0, 0, 6, 3));
    }

    #[test]
    fn a_block_frames_the_grid_in_the_inner_area() {
        let grid = Grid::new([Constraint::Fill(1)], [Constraint::Fill(1)]).block(Block::bordered());
        assert_eq!(grid.inner(Rect::new(0, 0, 6, 3)), Rect::new(1, 1, 4, 1));
        assert_eq!(
            grid.cell(Rect::new(0, 0, 6, 3), 0, 0),
            Some(Rect::new(1, 1, 4, 1))
        );
        assert_eq!(lines(grid, 6, 3), "┌────┐\n│    │\n└────┘\n");
    }

    #[test]
    fn base_style_fills_the_whole_content_area() {
        let grid = Grid::new([Constraint::Fill(1)], [Constraint::Fill(1)])
            .style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        grid.render(buf.area(), &mut buf);
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
    }

    #[test]
    fn render_draws_no_cell_content_only_frame_and_fill() {
        // Grid is pure layout: render paints the block + base fill, never the
        // cells (those are the caller's).
        let grid = Grid::new([Constraint::Fill(1); 2], [Constraint::Fill(1); 2]);
        assert_eq!(lines(grid, 3, 2), "   \n   \n");
    }

    #[test]
    fn zero_area_render_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Grid::new([Constraint::Fill(1)], [Constraint::Fill(1)])
            .style(Style::new().bg(Color::Red))
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(
            buf.cells()
                .iter()
                .all(|c| c.symbol == ' ' && c.bg == Color::Reset)
        );
    }
}
