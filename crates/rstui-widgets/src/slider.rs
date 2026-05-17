//! [`Slider`] — a horizontal value selector, the form family's *continuous*
//! input (the analogue [`Input`](crate::Input) is for text and
//! [`Checkbox`](crate::Checkbox) for a flag).
//!
//! # A pure projection of a caller-owned `value` in `min..=max` + `focused`
//!
//! [`Gauge`](crate::Gauge) is a pure projection of a caller-owned scalar
//! *ratio*; `Slider` is the *interactive* sibling: a pure projection of a
//! caller-owned [`value`](Slider::value) within a
//! [`min`](Slider::min)/[`max`](Slider::max) range, plus a
//! [`focused`](Slider::focused) `bool`. Both are ordinary application state the
//! reducer owns and changes in `update` (nudge `value` on the arrow keys, move
//! `focused` on `Tab`); the widget only ever reads them, so it composes with
//! the Elm `view(&self)` model exactly like every other rstui widget and never
//! mutates anything at render time.
//!
//! The displayed position is derived as a **pure function** of the range:
//! `fraction = ((value − min) / (max − min)).clamp(0, 1)`. An out-of-range
//! value clamps to a full or empty track, a degenerate `min == max` (or a
//! non-finite span, or `NaN`) maps to `0.0` — it renders something sensible
//! and **never panics**, the same total-projection rule
//! [`Gauge`](crate::Gauge) follows.
//!
//! # Sub-cell precision: the [`Gauge`](crate::Gauge) eighth-block ramp
//!
//! The thumb sits at the head of the filled run. So a 37%-of-the-way thumb
//! lands *between* two columns rather than rounding to a whole cell, the fill
//! boundary is drawn with the partial left-block glyph nearest the true
//! fraction (`▏▎▍▌▋▊▉`, the exact eight-eighths ramp
//! [`Gauge`](crate::Gauge) uses), and that partial glyph *is* the thumb head —
//! it wears [`thumb_style`](Slider::thumb_style) like the on-cell
//! [`thumb_symbol`](Slider::thumb_symbol) does. A width-`w` track therefore has
//! `8 · w` distinguishable thumb positions, not `w`. This is the same clean
//! fit for the single-[`char`] [`Cell`](rstui_core::Buffer) model
//! [`Gauge`](crate::Gauge) banked: every block element is one Unicode scalar,
//! so the symbols are plain `char`s with no `&str`/grapheme machinery.
//!
//! # A leaf control: one row, no `Block`
//!
//! Like the other form controls
//! ([`Checkbox`](crate::Checkbox)/[`Radio`](crate::Radio)/[`Input`](crate::Input))
//! and unlike the container widgets, `Slider` has **no framing
//! [`Block`](crate::Block)**: it draws on exactly the top row of its area, and
//! the surrounding [`Form`](crate::Form) / [`Layout`](rstui_core::Layout) owns
//! vertical placement, grouping, and any pane frame. An optional left
//! [`label`](Slider::label) and right [`value_label`](Slider::value_label)
//! readout are caller-built [`Line`]s — the *formatting policy* (how a number
//! becomes text) is the app's concern and a deliberately deferred additive,
//! not baked into this slice, exactly as [`Gauge`](crate::Gauge) lets the
//! caller own its label.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*):
//! an empty area, a zero/one-cell track, a value out of range, `min == max`, a
//! multi-row area, and a multi-byte label are all safe clips/no-ops — never a
//! panic.

use rstui_core::{Buffer, Line, Position, Rect, Style, Widget};

/// The eight left-aligned block elements, `1/8` … `8/8` filled — the exact
/// ramp [`Gauge`](crate::Gauge) uses, shared by intent, not by code, so each
/// widget stays a self-contained worked reference. `EIGHTHS[n - 1]` is the
/// glyph for `n` eighths.
const EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// The eight **bottom**-aligned block elements, `1/8` … `8/8` filled — the
/// vertical counterpart of [`EIGHTHS`] for a [`SliderOrientation::Vertical`]
/// track that fills bottom-up (the same ramp `Sparkline`/`BarChart`'s vertical
/// bars use, shared by intent not code). `EIGHTHS_VERTICAL[n - 1]` is `n`
/// eighths.
const EIGHTHS_VERTICAL: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The axis a [`Slider`] runs along.
///
/// [`Horizontal`](Self::Horizontal) (the default) is the original behaviour
/// unchanged: a left-to-right track. [`Vertical`](Self::Vertical) runs the
/// track up a column, **filling bottom-up** with the same eighth-block
/// sub-cell ramp.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SliderOrientation {
    /// Left-to-right track (the unchanged default).
    #[default]
    Horizontal,
    /// Bottom-to-top track.
    Vertical,
}

/// A horizontal value selector rendered as a pure projection of a caller-owned
/// [`value`](Self::value) within [`min`](Self::min)/[`max`](Self::max) and a
/// [`focused`](Self::focused) `bool`.
///
/// Layout on one row is `[label] <track> [value]`: an optional left
/// [`label`](Self::label), the track filling the middle, and an optional right
/// [`value_label`](Self::value_label) readout. The filled run uses
/// [`filled_symbol`](Self::filled_symbol) (default `'━'`); its sub-cell
/// boundary is the eighth-block glyph nearest the true fraction; the unfilled
/// remainder uses [`track_symbol`](Self::track_symbol) (default `'─'`); the
/// thumb is [`thumb_symbol`](Self::thumb_symbol) (default `'●'`) on a cell
/// boundary, or the partial-block boundary glyph between cells.
///
/// Styling cascades base → track-region → thumb (the same
/// [`Style::patch`](rstui_core::Style) model the text model uses); the base
/// [`style`](Self::style) fills the row so a background reads as one bar. When
/// [`focused`](Self::focused), [`focus_style`](Self::focus_style) is patched
/// **last** across the full row — over the fill, the thumb, the labels, and
/// the padding — so the focus emphasis overrides per-span colours and reads as
/// one contiguous bar, exactly as [`List`](crate::List)'s `highlight_style`
/// does for the selected row.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::Slider;
///
/// // `value` is plain caller-owned model state the widget only reads; the
/// // reducer mutates it in `update` (e.g. on the arrow keys).
/// let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
/// Slider::new().range(0.0, 100.0).value(50.0).render(buf.area(), &mut buf);
///
/// // Half-way: five filled cells, then the thumb knob.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '━');
/// assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, '●');
/// assert_eq!(buf.get(Position::new(9, 0)).unwrap().symbol, '─');
/// ```
#[derive(Debug, Clone)]
pub struct Slider<'a> {
    value: f64,
    min: f64,
    max: f64,
    orientation: SliderOrientation,
    focused: bool,
    label: Option<Line<'a>>,
    value_label: Option<Line<'a>>,
    style: Style,
    track_style: Style,
    thumb_style: Style,
    focus_style: Style,
    filled_symbol: char,
    track_symbol: char,
    thumb_symbol: char,
}

impl<'a> Slider<'a> {
    /// A slider over the unit range (`0.0..=1.0`) at `0.0`: unfocused, no
    /// labels, default glyphs (`'━'` / `'─'` / `'●'`) and empty styles.
    #[must_use]
    pub fn new() -> Self {
        Self {
            value: 0.0,
            min: 0.0,
            max: 1.0,
            orientation: SliderOrientation::Horizontal,
            focused: false,
            label: None,
            value_label: None,
            style: Style::new(),
            track_style: Style::new(),
            thumb_style: Style::new(),
            focus_style: Style::new(),
            filled_symbol: '━',
            track_symbol: '─',
            thumb_symbol: '●',
        }
    }

    /// Sets the current value — caller-owned state the widget only reads
    /// (nudge it in `update`). Out-of-range values clamp the thumb to an end;
    /// the value itself is never mutated here.
    #[must_use]
    pub fn value(mut self, value: f64) -> Self {
        self.value = value;
        self
    }

    /// Sets the range minimum (default `0.0`).
    #[must_use]
    pub fn min(mut self, min: f64) -> Self {
        self.min = min;
        self
    }

    /// Sets the range maximum (default `1.0`).
    #[must_use]
    pub fn max(mut self, max: f64) -> Self {
        self.max = max;
        self
    }

    /// Sets both range ends at once (`min`, then `max`).
    #[must_use]
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    /// Sets the track axis (default [`SliderOrientation::Horizontal`], the
    /// unchanged original behaviour). [`Vertical`](SliderOrientation::Vertical)
    /// fills bottom-up with the same eighth-block sub-cell ramp.
    #[must_use]
    pub fn orientation(mut self, orientation: SliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Sets whether this control is focused — caller-owned state the widget
    /// only reads (move it in `update`, typically on `Tab`). When `true` the
    /// [`focus_style`](Self::focus_style) bar is applied.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Sets the optional caller-built [`Line`] drawn left of the track,
    /// followed by a one-cell gap (default none).
    #[must_use]
    pub fn label(mut self, label: impl Into<Line<'a>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the optional caller-built readout [`Line`] drawn right of the
    /// track, preceded by a one-cell gap (default none).
    ///
    /// The widget never *formats* the value (number → text is app policy, a
    /// deliberately deferred additive); the caller passes the text it wants.
    #[must_use]
    pub fn value_label(mut self, value_label: impl Into<Line<'a>>) -> Self {
        self.value_label = Some(value_label.into());
        self
    }

    /// Sets the base [`Style`], beneath the base → track → thumb cascade. It
    /// also fills the control's row so a background covers it edge to edge.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched over the whole track region (fill, boundary,
    /// unfilled remainder), beneath the thumb — the role
    /// [`Gauge`](crate::Gauge)'s `gauge_style` plays for its bar/track.
    #[must_use]
    pub fn track_style(mut self, style: Style) -> Self {
        self.track_style = style;
        self
    }

    /// Sets the [`Style`] patched over the thumb cell, above the track style.
    #[must_use]
    pub fn thumb_style(mut self, style: Style) -> Self {
        self.thumb_style = style;
        self
    }

    /// Sets the [`Style`] applied when [`focused`](Self::focused).
    ///
    /// Patched **last** across the full row, so the focus emphasis overrides
    /// the track/thumb/label colours and reads as one bar — the same role
    /// [`List`](crate::List)'s `highlight_style` plays for selection.
    #[must_use]
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = style;
        self
    }

    /// Sets the glyph for the filled run, left of the thumb (default `'━'`).
    #[must_use]
    pub fn filled_symbol(mut self, symbol: char) -> Self {
        self.filled_symbol = symbol;
        self
    }

    /// Sets the glyph for the unfilled track, right of the thumb (default
    /// `'─'`).
    #[must_use]
    pub fn track_symbol(mut self, symbol: char) -> Self {
        self.track_symbol = symbol;
        self
    }

    /// Sets the thumb glyph drawn on a cell boundary (default `'●'`). Between
    /// cells the sub-cell partial-block glyph is the thumb head instead.
    #[must_use]
    pub fn thumb_symbol(mut self, symbol: char) -> Self {
        self.thumb_symbol = symbol;
        self
    }

    /// The displayed fill fraction: a **pure, total** projection of the range.
    ///
    /// `((value − min) / (max − min)).clamp(0, 1)`, with a degenerate or
    /// non-finite span (including `min == max`) and `NaN` mapped to `0.0` — it
    /// never panics (the [`Gauge`](crate::Gauge) clamp-don't-panic rule).
    #[must_use]
    pub fn fraction(&self) -> f64 {
        let span = self.max - self.min;
        if !span.is_finite() || span <= 0.0 {
            return 0.0;
        }
        let f = (self.value - self.min) / span;
        if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) }
    }
}

impl Default for Slider<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Stamps `line`'s glyphs left to right from `x0`, clipped at `right`,
/// cascading `base` → line → span. Returns the column just past the text.
fn stamp_line(buf: &mut Buffer, line: &Line<'_>, x0: u16, y: u16, right: u16, base: Style) -> u16 {
    let line_base = base.patch(line.style);
    let mut x = x0;
    'line: for span in &line.spans {
        let span_style = line_base.patch(span.style);
        for ch in span.content.chars() {
            if x >= right {
                break 'line;
            }
            buf.set_cell(Position::new(x, y), ch, span_style);
            x = x.saturating_add(1);
        }
    }
    x
}

impl Widget for Slider<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let fraction = self.fraction();
        let Slider {
            orientation,
            focused,
            label,
            value_label,
            style,
            track_style,
            thumb_style,
            focus_style,
            filled_symbol,
            track_symbol,
            thumb_symbol,
            ..
        } = self;

        if orientation == SliderOrientation::Vertical {
            // Vertical track, filling bottom-up with the same eighth-block
            // sub-cell ramp (the proven horizontal metric, axis-flipped). An
            // optional `label` takes the top row and `value_label` the bottom
            // row, bracketing the track exactly as left/right do horizontally.
            buf.set_style(area, style);
            let left = area.left();
            let right = area.right();
            let mut track_top = area.top();
            let mut track_bottom = area.bottom();
            if let Some(label) = &label {
                stamp_line(buf, label, left, track_top, right, style);
                track_top = track_top.saturating_add(1).min(track_bottom);
            }
            if let Some(value_label) = &value_label {
                let row = track_bottom.saturating_sub(1);
                if row >= track_top {
                    stamp_line(buf, value_label, left, row, right, style);
                    track_bottom = row;
                }
            }
            let track_height = track_bottom.saturating_sub(track_top);
            if track_height > 0 {
                let track_base = style.patch(track_style);
                let thumb_base = track_base.patch(thumb_style);
                let filled = f64::from(track_height) * fraction;
                let full = filled.floor() as u16;
                let eighths = ((filled - f64::from(full)) * 8.0).round() as u16;
                let (thumb_pos, thumb_glyph) = if eighths > 0 && full < track_height {
                    (full, EIGHTHS_VERTICAL[(eighths - 1) as usize])
                } else {
                    (full.min(track_height - 1), thumb_symbol)
                };
                for row in track_top..track_bottom {
                    // 0 at the bottom-most track row so the bar grows upward.
                    let from_bottom = track_bottom - 1 - row;
                    let (glyph, cell_style) = if from_bottom == thumb_pos {
                        (thumb_glyph, thumb_base)
                    } else if from_bottom < full {
                        ('█', track_base)
                    } else {
                        (track_symbol, track_base)
                    };
                    for x in left..right {
                        buf.set_cell(Position::new(x, row), glyph, cell_style);
                    }
                }
            }
            if focused {
                buf.set_style(area, focus_style);
            }
            return;
        }

        let y = area.top();
        let left = area.left();
        let right = area.right();

        // The base fills the whole row so a background reads as one bar; the
        // focus emphasis is patched LAST over everything (below), exactly as
        // Checkbox/List make a focused control read as one contiguous bar.
        buf.set_style(Rect::new(left, y, area.width, 1), style);

        // Left label, then a one-cell gap. Clipped at the right edge.
        let mut track_left = left;
        if let Some(label) = &label {
            track_left = stamp_line(buf, label, track_left, y, right, style);
            track_left = track_left.saturating_add(1).min(right);
        }

        // Reserve the right readout (a one-cell gap before it).
        let mut track_right = right;
        if let Some(value_label) = &value_label {
            let w = (value_label.width() as u16).saturating_add(1);
            track_right = track_right.saturating_sub(w).max(track_left);
        }

        // The track spans [track_left, track_right). Gauge's exact model: whole
        // filled columns plus one sub-cell boundary glyph; the thumb is that
        // boundary glyph between cells, or `thumb_symbol` on a cell.
        let track_width = track_right.saturating_sub(track_left);
        if track_width > 0 {
            let track_base = style.patch(track_style);
            let filled = f64::from(track_width) * fraction;
            let full = filled.floor() as u16;
            let eighths = ((filled - f64::from(full)) * 8.0).round() as u16;
            let thumb_base = track_base.patch(thumb_style);

            let (thumb_col, thumb_glyph) = if eighths > 0 && full < track_width {
                // Between cells: the partial-block boundary IS the thumb head.
                (full, EIGHTHS[(eighths - 1) as usize])
            } else {
                // On a cell boundary (clamped to the last cell at fraction 1).
                (full.min(track_width - 1), thumb_symbol)
            };

            for i in 0..track_width {
                let x = track_left + i;
                let (glyph, cell_style) = if i == thumb_col {
                    (thumb_glyph, thumb_base)
                } else if i < full {
                    (filled_symbol, track_base)
                } else {
                    (track_symbol, track_base)
                };
                buf.set_cell(Position::new(x, y), glyph, cell_style);
            }
        }

        // The right readout, after the gap.
        if let Some(value_label) = &value_label {
            let w = value_label.width() as u16;
            let x0 = right.saturating_sub(w);
            stamp_line(buf, value_label, x0, y, right, style);
        }

        // Focus wins LAST, patched across the full row so it reads as one
        // contiguous bar over the track, thumb, labels and padding alike —
        // the List/Checkbox highlight-bar idiom keyed by `focused`.
        if focused {
            buf.set_style(Rect::new(left, y, area.width, 1), focus_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier, Span};

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
    fn an_empty_value_puts_the_thumb_at_the_start() {
        assert_eq!(lines(Slider::new(), 10, 1), "●─────────\n");
    }

    #[test]
    fn a_full_value_puts_the_thumb_at_the_end() {
        assert_eq!(lines(Slider::new().value(1.0), 10, 1), "━━━━━━━━━●\n");
    }

    #[test]
    fn a_half_value_fills_to_the_thumb_in_the_middle() {
        assert_eq!(
            lines(Slider::new().range(0.0, 100.0).value(50.0), 10, 1),
            "━━━━━●────\n"
        );
    }

    #[test]
    fn a_fractional_position_uses_a_sub_cell_block_glyph_as_the_thumb_head() {
        // 10·0.37 = 3.7 ⇒ 3 full + round(0.7·8)=6 eighths ⇒ '▊' is the thumb.
        assert_eq!(lines(Slider::new().value(0.37), 10, 1), "━━━▊──────\n");
    }

    #[test]
    fn the_eighth_ramp_maps_each_sub_cell_fraction_to_its_block() {
        // One-cell track: the whole bar is the single boundary/thumb glyph.
        for (value, glyph) in [
            (0.125, '▏'),
            (0.250, '▎'),
            (0.375, '▍'),
            (0.500, '▌'),
            (0.625, '▋'),
            (0.750, '▊'),
            (0.875, '▉'),
        ] {
            assert_eq!(
                lines(Slider::new().value(value), 1, 1),
                format!("{glyph}\n"),
                "value {value}"
            );
        }
    }

    #[test]
    fn an_out_of_range_value_clamps_and_never_panics() {
        assert_eq!(
            lines(Slider::new().range(0.0, 10.0).value(99.0), 5, 1),
            "━━━━●\n"
        );
        assert_eq!(
            lines(Slider::new().range(0.0, 10.0).value(-5.0), 5, 1),
            "●────\n"
        );
        assert_eq!(lines(Slider::new().value(f64::NAN), 5, 1), "●────\n");
    }

    #[test]
    fn a_degenerate_min_equals_max_range_is_a_total_empty_track() {
        assert_eq!(
            lines(Slider::new().range(5.0, 5.0).value(5.0), 4, 1),
            "●───\n"
        );
    }

    #[test]
    fn a_left_label_and_right_readout_bracket_the_track() {
        let s = Slider::new().value(1.0).label("Vol").value_label("100");
        // "Vol" + gap + track(3) + gap + "100" in width 11.
        assert_eq!(lines(s, 11, 1), "Vol ━━● 100\n");
    }

    #[test]
    fn a_multibyte_label_maps_each_char_to_one_column() {
        let s = Slider::new().value(0.0).label("é日");
        assert_eq!(lines(s, 6, 1), "é日 ●──\n");
    }

    #[test]
    fn focus_style_is_a_full_width_bar_over_track_labels_and_padding() {
        let s = Slider::new()
            .value(0.5)
            .label("V")
            .value_label("x")
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 9, 1));
        s.render(buf.area(), &mut buf);
        for x in 0..9 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Blue);
        }
    }

    #[test]
    fn unfocused_paints_no_focus_style() {
        let s = Slider::new()
            .value(0.5)
            .focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        s.render(buf.area(), &mut buf);
        for x in 0..8 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Reset);
        }
    }

    #[test]
    fn base_style_fills_the_whole_row_and_thumb_style_patches_the_thumb() {
        let s = Slider::new()
            .value(0.5)
            .style(Style::new().bg(Color::Red))
            .thumb_style(Style::new().fg(Color::Green));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        s.render(buf.area(), &mut buf);
        for x in 0..4 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Red);
        }
        // 4·0.5 = 2.0 ⇒ thumb on cell 2.
        let thumb = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(thumb.symbol, '●');
        assert_eq!(thumb.fg, Color::Green);
    }

    #[test]
    fn the_track_style_colours_the_fill_and_the_remainder() {
        let s = Slider::new()
            .value(0.5)
            .track_style(Style::new().fg(Color::Green).bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        s.render(buf.area(), &mut buf);
        let fill = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(fill.symbol, '━');
        assert_eq!(fill.fg, Color::Green);
        assert_eq!(fill.bg, Color::Blue);
        let track = buf.get(Position::new(3, 0)).unwrap();
        assert_eq!(track.symbol, '─');
        assert_eq!(track.bg, Color::Blue);
    }

    #[test]
    fn a_value_label_span_keeps_its_own_style_over_the_base() {
        let s = Slider::new()
            .value(0.0)
            .value_label(Span::styled("9", Style::new().fg(Color::Red)))
            .style(Style::new().add_modifier(Modifier::BOLD));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        s.render(buf.area(), &mut buf);
        let nine = buf.get(Position::new(3, 0)).unwrap();
        assert_eq!(nine.symbol, '9');
        assert_eq!(nine.fg, Color::Red);
        assert!(nine.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_one_cell_area_is_total() {
        assert_eq!(lines(Slider::new().value(1.0), 1, 1), "●\n");
        assert_eq!(lines(Slider::new().value(0.0), 1, 1), "●\n");
    }

    #[test]
    fn only_the_top_row_of_a_taller_area_is_touched() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 3));
        Slider::new().value(0.5).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '━');
        for cell_y in 1..3 {
            for x in 0..5 {
                assert_eq!(buf.get(Position::new(x, cell_y)).unwrap().symbol, ' ');
            }
        }
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));
        Slider::new()
            .value(0.0)
            .render(Rect::new(2, 3, 6, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 3)).unwrap().symbol, '●');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        Slider::new()
            .value(0.5)
            .focused(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    // ---- ADR 0012 §P2 additive: vertical orientation ----

    fn v<'a>() -> Slider<'a> {
        Slider::new().orientation(SliderOrientation::Vertical)
    }

    #[test]
    fn a_vertical_slider_at_zero_parks_the_thumb_at_the_bottom() {
        // Empty value ⇒ thumb on the bottom-most row, track above it.
        assert_eq!(lines(v().value(0.0), 1, 4), "─\n─\n─\n●\n");
    }

    #[test]
    fn a_vertical_slider_at_full_value_fills_the_whole_track() {
        // Full value ⇒ filled bottom-up, thumb on the top row.
        assert_eq!(lines(v().value(1.0), 1, 4), "●\n█\n█\n█\n");
    }

    #[test]
    fn a_vertical_half_value_fills_the_lower_half() {
        // range 0..100 @ 50 over a 10-tall track: 5 filled rows from the
        // bottom, thumb at the boundary, empty rail above.
        assert_eq!(
            lines(v().range(0.0, 100.0).value(50.0), 1, 10),
            "─\n─\n─\n─\n●\n█\n█\n█\n█\n█\n"
        );
    }

    #[test]
    fn a_vertical_slider_uses_the_vertical_eighth_ramp_between_cells() {
        // One-cell track: round(0.5·8)=4 eighths ⇒ the bottom-block '▄'.
        assert_eq!(lines(v().value(0.5), 1, 1), "▄\n");
    }

    #[test]
    fn a_vertical_label_and_readout_bracket_the_track_top_and_bottom() {
        // "V" on the top row, the track in the middle (thumb at the bottom
        // for value 0), "0" on the bottom row — the horizontal label/readout
        // idiom rotated.
        let s = Slider::new()
            .orientation(SliderOrientation::Vertical)
            .value(0.0)
            .label("V")
            .value_label("0");
        assert_eq!(lines(s, 1, 4), "V\n─\n●\n0\n");
    }

    #[test]
    fn a_focused_vertical_slider_reads_as_one_bar_over_the_whole_area() {
        let s = Slider::new()
            .orientation(SliderOrientation::Vertical)
            .value(0.5)
            .focused(true)
            .focus_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 4));
        s.render(buf.area(), &mut buf);
        for y in 0..4 {
            for x in 0..2 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Blue);
            }
        }
    }
}
