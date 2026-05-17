//! [`PieChart`] — a proportional disc (or donut) of coloured wedges, the
//! dashboard primitive for "this whole is split into a handful of named parts"
//! (disk by filesystem, traffic by status class, spend by team, a poll's
//! results).
//!
//! # A pure projection, like every other widget
//!
//! `PieChart` owns no state. It is a list of caller-built [`Slice`]s (a label
//! [`Line`], a `u64` weight, and a [`Color`]) plus a few layout switches; the
//! reducer decides what the slices are and the widget only projects them onto
//! the disc. That keeps it deterministically headless-testable and composes
//! with the Elm `view(&self)` model exactly like [`List`](crate::List) and
//! [`BarChart`](crate::BarChart).
//!
//! # Why sample cell centres, and the aspect correction
//!
//! A pie has no eighth-block ramp to borrow (the boundary is a *curve*, not a
//! straight rule), so — unlike [`Gauge`](crate::Gauge) and
//! [`BarChart`](crate::BarChart) — `PieChart` resolves the disc the way
//! [`Canvas`](crate::Canvas) resolves a shape: it tests each cell's **centre**
//! against the geometry. A cell whose centre falls inside the disc radius (and
//! outside the donut hole) belongs to whichever slice owns the angle from the
//! centre to that point; the cell is stamped a full `█` in that slice's colour
//! and every glyph is one Unicode scalar, so it maps 1:1 onto a
//! [`Cell`](rstui_core::Buffer) with no grapheme machinery — the same reasoning
//! the [`Block`] borders and the gauge ramp use. Terminal cells are roughly
//! twice as tall as they are wide, so a disc measured in raw cell units reads
//! as a tall ellipse; the radius test scales the vertical distance by that
//! ~2:1 ratio so the result reads visually **round**.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule a pure projection is *total*: an empty
//! area, no slices, an all-zero series (no division by a zero total), a donut
//! ratio outside `0.0..1.0` (clamped), and an area too small for even one cell
//! of disc are all safe clips/no-ops — never a panic. An optional framing
//! [`Block`] follows the container-widget convention; an exploded-slice offset
//! and value labels drawn *on* the wedges are deliberately deferred additive
//! follow-ups, not smuggled into this slice.

use rstui_core::{Buffer, Color, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The terminal-cell aspect ratio: a cell is about twice as tall as it is
/// wide, so vertical distance is scaled by this to make the disc read round.
const CELL_ASPECT: f64 = 2.0;

/// One wedge of a [`PieChart`]: a label [`Line`], its `u64` weight, and the
/// [`Color`] the wedge is filled with.
///
/// The slice's share of the disc is its weight over the sum of every weight;
/// build the label from anything a [`Line`] is built from (`&str`, `String`,
/// [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`) and style it through the
/// [`Line`] it wraps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Slice<'a> {
    /// The wedge's label, shown in the optional legend column.
    label: Line<'a>,
    /// The wedge's weight; its share is this over the sum of all weights.
    value: u64,
    /// The colour the wedge is filled with.
    color: Color,
}

impl<'a> Slice<'a> {
    /// A wedge of weight `value` filled with `color`, labelled `label`
    /// (anything convertible to a [`Line`]).
    pub fn new(value: u64, color: Color, label: impl Into<Line<'a>>) -> Self {
        Self {
            label: label.into(),
            value,
            color,
        }
    }
}

/// A proportional disc of coloured wedges with an optional centred hole, an
/// optional legend column, and an optional framing [`Block`].
///
/// Each [`Slice`] owns a contiguous angular wedge sized by its share of the
/// total weight, swept clockwise from twelve o'clock. The disc is the largest
/// aspect-corrected circle that fits the plot area; [`donut`](Self::donut)
/// punches a centred hole (an inner-radius ratio). With [`legend`](Self::legend)
/// a right-hand column lists each slice's label and percentage (one decimal).
/// Styling is a base [`Style`] (filling the area so a background covers the
/// whole pane) under each wedge's own [`Color`] and each legend label's own
/// [`Line`]/[`Span`](rstui_core::Span) styles.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Color, Position, Rect, Widget};
/// use rstui_widgets::{PieChart, Slice};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 7, 7));
/// PieChart::new([
///     Slice::new(3, Color::Red, "a"),
///     Slice::new(1, Color::Blue, "b"),
/// ])
/// .render(buf.area(), &mut buf);
///
/// // The disc centre is filled; the colour is whichever wedge owns its angle.
/// assert_eq!(buf.get(Position::new(3, 3)).unwrap().symbol, '█');
/// ```
#[derive(Debug, Default, Clone)]
pub struct PieChart<'a> {
    slices: Vec<Slice<'a>>,
    donut: Option<f64>,
    legend: bool,
    block: Option<Block<'a>>,
    style: Style,
}

impl<'a> PieChart<'a> {
    /// A solid disc of `slices`, no hole, no legend, no frame.
    pub fn new<I>(slices: I) -> Self
    where
        I: IntoIterator<Item = Slice<'a>>,
    {
        Self {
            slices: slices.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Punches a centred hole of `Some(ratio)` of the radius (a donut), or
    /// `None` for a solid disc.
    ///
    /// The ratio is clamped to `0.0..1.0` (a value at or above `1.0` would
    /// erase the whole disc; a negative one is meaningless) — never a panic,
    /// the [`Gauge`](crate::Gauge) totality rule.
    #[must_use]
    pub fn donut(mut self, donut: Option<f64>) -> Self {
        self.donut = donut;
        self
    }

    /// Sets whether a right-hand legend column lists each slice's label and
    /// percentage (one decimal); off by default.
    #[must_use]
    pub fn legend(mut self, legend: bool) -> Self {
        self.legend = legend;
        self
    }

    /// Frames the chart in `block`; the disc renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the content area so a background
    /// covers the whole pane beneath the wedges and legend.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

/// Stamps `line` left-to-right from `x0` on row `y`, clipped at `right`, with
/// `base` beneath the line→span cascade.
fn stamp_line(buf: &mut Buffer, line: &Line, base: Style, x0: u16, y: u16, right: u16) {
    let line_base = base.patch(line.style);
    let mut x = x0;
    'line: for span in &line.spans {
        let style = line_base.patch(span.style);
        for ch in span.content.chars() {
            if x >= right {
                break 'line;
            }
            buf.set_cell(Position::new(x, y), ch, style);
            x = x.saturating_add(1);
        }
    }
}

impl Widget for PieChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let PieChart {
            slices,
            donut,
            legend,
            block,
            style,
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

        // Base fills the content area so a background covers the whole pane.
        buf.set_style(inner, style);
        if slices.is_empty() {
            return;
        }

        // The total weight; a zero total (every slice zero) has no proportions
        // to draw, so the disc is just the background — never a divide-by-zero.
        let total: u128 = slices.iter().map(|s| u128::from(s.value)).sum();
        if total == 0 {
            return;
        }

        // A legend, when shown, takes a right-hand column at most half the
        // width, sized to the longest `● label … 100.0%` row (bullet + gap +
        // label + gap + the widest percentage, `100.0%` = 6); the disc fills
        // the rest. It is capped, not dropped — a tight column just clips.
        let legend_w = if legend {
            let widest = slices
                .iter()
                .map(|s| s.label.width() + 9)
                .max()
                .unwrap_or(0) as u16;
            let cap = inner.width / 2;
            widest.min(cap)
        } else {
            0
        };
        let disc = Rect::new(
            inner.left(),
            inner.top(),
            inner.width.saturating_sub(legend_w),
            inner.height,
        );

        if !disc.is_empty() {
            // The disc centre, in fractional cell coordinates, and the largest
            // aspect-corrected radius that fits both axes. Vertical distance is
            // scaled up by CELL_ASPECT so a disc measured in cells reads round.
            let cx = f64::from(disc.width) / 2.0;
            let cy = f64::from(disc.height) / 2.0;
            let r = (cx).min(cy * CELL_ASPECT);
            if r > 0.0 {
                // The inner (hole) radius: the clamped donut ratio of `r`.
                let inner_r = donut.map_or(0.0, |d| d.clamp(0.0, 1.0) * r);

                // Cumulative slice boundaries as fractions of a full turn.
                let mut bounds = Vec::with_capacity(slices.len());
                let mut acc: u128 = 0;
                for s in &slices {
                    acc += u128::from(s.value);
                    bounds.push(acc as f64 / total as f64);
                }
                let tau = std::f64::consts::TAU;

                for ry in 0..disc.height {
                    for rx in 0..disc.width {
                        // The cell centre relative to the disc centre, with the
                        // vertical axis aspect-corrected to match the radius.
                        let dx = (f64::from(rx) + 0.5) - cx;
                        let dy = ((f64::from(ry) + 0.5) - cy) * CELL_ASPECT;
                        let dist = (dx * dx + dy * dy).sqrt();
                        if dist > r || dist < inner_r {
                            continue;
                        }
                        // The angle clockwise from twelve o'clock, in `0.0..1.0`
                        // turns: screen y grows downward, so `-dy` points up.
                        let mut frac = dx.atan2(-dy) / tau;
                        if frac < 0.0 {
                            frac += 1.0;
                        }
                        // The first slice whose cumulative boundary covers this
                        // angle owns the cell (the last slice mops up rounding).
                        let idx = bounds
                            .iter()
                            .position(|&b| frac < b)
                            .unwrap_or(slices.len() - 1);
                        buf.set_cell(
                            Position::new(disc.left() + rx, disc.top() + ry),
                            '█',
                            style.fg(slices[idx].color),
                        );
                    }
                }
            }
        }

        // The legend column: one row per slice, "● label … pp.p%" — the
        // bullet, the label, then the right-aligned percentage, clipped to the
        // column and the available rows. The label is clipped one cell short
        // of the percentage so the two never run together.
        if legend_w > 0 {
            let lx = inner.right().saturating_sub(legend_w);
            let right = inner.right();
            let label_x = lx.saturating_add(2);
            for (i, s) in slices.iter().enumerate() {
                let y = inner.top().saturating_add(i as u16);
                if y >= inner.bottom() {
                    break;
                }
                let pct = (s.value as f64) * 100.0 / total as f64;
                let pcts = format!("{pct:.1}%");
                let pw = pcts.chars().count() as u16;
                // The percentage is right-aligned; it only fits if it leaves
                // at least the bullet + a label cell to its left.
                let pct_x = right.saturating_sub(pw);
                let pct_fits = right > label_x && pct_x > label_x;
                // Stop the label a blank cell before the percentage (or the
                // column edge when the percentage did not fit).
                let label_end = if pct_fits {
                    pct_x.saturating_sub(1)
                } else {
                    right
                };
                buf.set_cell(Position::new(lx, y), '●', style.fg(s.color));
                stamp_line(buf, &s.label, style, label_x, y, label_end);
                if pct_fits {
                    buf.set_str(Position::new(pct_x, y), &pcts, style);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Modifier, Span};

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
    fn a_solid_disc_fills_a_round_region_of_blocks() {
        // One slice → every in-radius cell is that slice's block. The disc is
        // wider than it is tall in *cells* (~2:1) precisely so it reads round
        // on screen; the corner cells fall outside the radius and stay blank.
        let chart = PieChart::new([Slice::new(1, Color::Red, "x")]);
        assert_eq!(
            lines(chart, 9, 5),
            "  █████  \n█████████\n█████████\n█████████\n  █████  \n"
        );
    }

    #[test]
    fn each_slice_owns_its_angular_wedge_in_its_own_colour() {
        // Two equal slices split the disc into a left and a right half; the
        // split runs through twelve/six o'clock so the colours differ across x.
        let chart = PieChart::new([
            Slice::new(1, Color::Red, "a"),
            Slice::new(1, Color::Blue, "b"),
        ]);
        let mut buf = Buffer::empty(Rect::new(0, 0, 7, 7));
        chart.render(buf.area(), &mut buf);
        // First slice sweeps clockwise from 12 o'clock → it owns the right
        // half; the second owns the left half.
        let right = buf.get(Position::new(4, 3)).unwrap();
        let left = buf.get(Position::new(2, 3)).unwrap();
        assert_eq!(right.symbol, '█');
        assert_eq!(right.fg, Color::Red);
        assert_eq!(left.fg, Color::Blue);
    }

    #[test]
    fn a_donut_punches_a_centred_hole() {
        // A 0.5 inner-radius ratio clears the middle; the centre cell is back
        // to the blank track while the rim stays filled.
        let chart = PieChart::new([Slice::new(1, Color::Red, "x")]).donut(Some(0.5));
        let mut buf = Buffer::empty(Rect::new(0, 0, 9, 9));
        chart.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(4, 4)).unwrap().symbol, ' ');
        // A rim cell on the centre row is still filled.
        assert_eq!(buf.get(Position::new(1, 4)).unwrap().symbol, '█');
    }

    #[test]
    fn a_donut_ratio_at_or_above_one_is_clamped_and_draws_nothing() {
        // Clamped to <1.0 would still erase everything at exactly 1.0; the
        // clamp keeps it total (no panic) and the disc is empty.
        let chart = PieChart::new([Slice::new(1, Color::Red, "x")]).donut(Some(1.5));
        assert_eq!(lines(chart, 5, 5), "     \n     \n     \n     \n     \n");
    }

    #[test]
    fn the_legend_lists_each_label_and_percentage() {
        let chart = PieChart::new([
            Slice::new(3, Color::Red, "a"),
            Slice::new(1, Color::Blue, "b"),
        ])
        .legend(true);
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 3));
        chart.render(buf.area(), &mut buf);
        // The legend is the right-hand column: bullet, label, then the
        // right-aligned percentage (3/4 = 75.0%, 1/4 = 25.0%).
        let row0: String = (0..24)
            .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
            .collect();
        let row1: String = (0..24)
            .map(|x| buf.get(Position::new(x, 1)).unwrap().symbol)
            .collect();
        assert!(row0.contains("● a"), "row0 = {row0:?}");
        assert!(row0.contains("75.0%"), "row0 = {row0:?}");
        assert!(row1.contains("● b"), "row1 = {row1:?}");
        assert!(row1.contains("25.0%"), "row1 = {row1:?}");
    }

    #[test]
    fn an_empty_series_just_fills_the_area() {
        let chart = PieChart::new(Vec::<Slice>::new());
        assert_eq!(lines(chart, 3, 3), "   \n   \n   \n");
    }

    #[test]
    fn an_all_zero_series_draws_no_wedges_and_does_not_divide_by_zero() {
        let chart = PieChart::new([
            Slice::new(0, Color::Red, "a"),
            Slice::new(0, Color::Blue, "b"),
        ]);
        assert_eq!(lines(chart, 5, 5), "     \n     \n     \n     \n     \n");
    }

    #[test]
    fn a_tiny_area_clips_the_disc_without_a_panic() {
        let chart = PieChart::new([Slice::new(1, Color::Red, "x")]);
        // 1×1: the single cell centre is the disc centre, inside the radius.
        assert_eq!(lines(chart, 1, 1), "█\n");
    }

    #[test]
    fn a_block_frames_the_chart_in_the_inner_area() {
        let chart = PieChart::new([Slice::new(1, Color::Red, "x")]).block(Block::bordered());
        // inner is the 1×1 centre cell → one block inside the border.
        assert_eq!(lines(chart, 3, 3), "┌─┐\n│█│\n└─┘\n");
    }

    #[test]
    fn no_slices_with_a_block_still_renders_the_block() {
        let chart = PieChart::new(Vec::<Slice>::new()).block(Block::bordered());
        assert_eq!(lines(chart, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn style_cascades_base_under_the_wedge_colour_and_legend_label() {
        let chart = PieChart::new([Slice::new(
            1,
            Color::Red,
            Line::from(Span::styled("L", Style::new().add_modifier(Modifier::BOLD))),
        )])
        .legend(true)
        .style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 3));
        chart.render(buf.area(), &mut buf);

        // A disc cell: base bg cascades, the wedge colour is the fg.
        let cell = buf.get(Position::new(4, 1)).unwrap();
        assert_eq!(cell.symbol, '█');
        assert_eq!(cell.fg, Color::Red);
        assert_eq!(cell.bg, Color::Blue);

        // The legend bullet carries the slice colour over the base bg.
        let bullet = buf.get(Position::new(14, 0)).unwrap();
        assert_eq!(bullet.symbol, '●');
        assert_eq!(bullet.fg, Color::Red);
        assert_eq!(bullet.bg, Color::Blue);

        // The legend label keeps its own span modifier over the base bg.
        let label = buf.get(Position::new(16, 0)).unwrap();
        assert_eq!(label.symbol, 'L');
        assert!(label.modifier.contains(Modifier::BOLD));
        assert_eq!(label.bg, Color::Blue);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        PieChart::new([Slice::new(1, Color::Red, "x")]).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
