//! [`Form`] / [`FormField`] — a vertical label-and-control layout primitive
//! that owns **no** application state.
//!
//! # A pure *layout* projection, never a state owner
//!
//! Every other form-family widget
//! ([`Checkbox`](crate::Checkbox)/[`Radio`](crate::Radio)/[`Input`](crate::Input)/[`Slider`](crate::Slider)/[`Switch`](crate::Switch))
//! is a pure projection of *caller-owned data*. `Form` is one rung up and
//! deliberately holds **even less**: it is a pure projection of *caller-owned
//! geometry intent only*. It does not own — and cannot read — any field's
//! value, focus, validity, or selection. Those are the app's model, exactly as
//! before; `Form` just answers "given this area, where does each field's
//! control go, and where do its label and help line go", then draws the
//! labels, the optional help lines, and an optional frame. The controls
//! themselves are rendered by the caller into the rects `Form` hands back —
//! the same render-then-fill-`inner` contract [`Block`] and
//! [`Modal`](crate::Modal) use.
//!
//! This keeps the load-bearing brief constraint literally true: *form
//! composition primitives that do not own application state*. The widget is a
//! geometry function plus label/help/frame chrome; the values live in the
//! model and the reducer mutates them.
//!
//! # The `layout` accessor is a pure geometry function
//!
//! [`Form::layout`] returns the control [`Rect`] for every field, in field
//! order, as a pure function of the area and the configuration — exactly like
//! [`Modal::inner`](crate::Modal::inner) and [`Block::inner`](crate::Block::inner).
//! The caller renders each field's own widget into its rect:
//!
//! ```text
//! let rects = form.layout(area);          // pure geometry, no state
//! form.render(area, buf);                 // labels + help + frame chrome
//! Input::new(&model.name).render(rects[0], buf);   // caller owns the control
//! Switch::new().on(model.dark).render(rects[1], buf);
//! ```
//!
//! `render` and `layout` agree on the geometry by construction (one shared
//! private placement pass), so a control always lands exactly where its label
//! was drawn for it.
//!
//! # A container: composes with `Block`, reuses core `Layout`
//!
//! Unlike the leaf controls, `Form` *is* a container, so an optional framing
//! [`Block`] draws the border/title and the fields are laid out inside
//! [`Block::inner`]. The label/control column split reuses the core
//! [`Layout`]/[`Constraint`]
//! divider rather than inventing a second one — the same "reuse the existing
//! layout vocabulary" stance [`Modal`](crate::Modal)/[`Table`](crate::Table)
//! take. The label column auto-sizes to the widest label unless pinned with
//! [`label_width`](Form::label_width).
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*):
//! an empty area, an area shorter than the fields, a frame larger than the
//! area, zero-height fields, and a label column wider than the area all clip
//! to safe (possibly empty) rects — [`layout`](Form::layout) still returns one
//! rect per field so caller indices stay aligned — never a panic.

use rstui_core::{Buffer, Constraint, Layout, Line, Position, Rect, Style, Widget};

use crate::Block;

/// One row group of a [`Form`]: a label, the control's body height, and an
/// optional help/validation line shown beneath the control.
///
/// `FormField` carries **no value** — only the label text, how many rows the
/// caller's control needs, and an optional help [`Line`] (a hint, or a
/// validation error the *reducer* decided to surface). The control's actual
/// state stays in the app's model.
#[derive(Debug, Default, Clone)]
pub struct FormField<'a> {
    label: Line<'a>,
    body_height: u16,
    help: Option<Line<'a>>,
}

impl<'a> FormField<'a> {
    /// A field labelled `label` whose control occupies `body_height` rows
    /// (commonly `1` for a leaf control), with no help line.
    pub fn new(label: impl Into<Line<'a>>, body_height: u16) -> Self {
        Self {
            label: label.into(),
            body_height,
            help: None,
        }
    }

    /// Sets the help/validation [`Line`] drawn on one row beneath the control
    /// (default none). The caller decides its content and styling — `Form`
    /// owns no validity state.
    #[must_use]
    pub fn help(mut self, help: impl Into<Line<'a>>) -> Self {
        self.help = Some(help.into());
        self
    }
}

/// The placement of one field, shared by [`Form::layout`] and [`Form::render`]
/// so the control always lands where its label was drawn.
#[derive(Debug, Clone, Copy)]
struct FieldRects {
    label: Rect,
    control: Rect,
    /// Zero-height when the field has no help line or it was clipped away.
    help: Rect,
}

/// A vertical label-and-control layout primitive owning no application state.
///
/// `Form` lays out a column of [`FormField`]s: each gets a left-aligned label
/// in a shared, auto-sized label column, a control [`Rect`] the caller renders
/// into ([`Form::layout`]), and an optional help line beneath it. An optional
/// framing [`Block`] draws a border/title; [`row_spacing`](Self::row_spacing)
/// adds blank rows between fields.
///
/// It never owns or reads field values — see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_widgets::{Form, FormField};
///
/// let form = Form::new()
///     .field(FormField::new("Name", 1))
///     .field(FormField::new("Bio", 2).help("optional"));
///
/// // `layout` is a pure geometry function — no state, like `Block::inner`.
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 6));
/// let rects = form.clone().layout(buf.area());
/// assert_eq!(rects.len(), 2);
/// // Field 0's control sits right of the 4-wide "Name"/"Bio" label column
/// // (+ a one-cell gap) on the first row.
/// assert_eq!(rects[0], Rect::new(5, 0, 15, 1));
///
/// form.render(buf.area(), &mut buf);
/// // The control's *value* lives in the app's model; the reducer mutates it.
/// // `Form` only placed the rect and drew the "Name" label into it.
/// ```
#[derive(Debug, Clone)]
pub struct Form<'a> {
    fields: Vec<FormField<'a>>,
    block: Option<Block<'a>>,
    label_width: Option<u16>,
    label_gap: u16,
    row_spacing: u16,
    style: Style,
    label_style: Style,
    help_style: Style,
}

impl<'a> Form<'a> {
    /// An empty form: no fields, no frame, an auto-sized label column, a
    /// one-cell label/control gap, no row spacing, and empty styles.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fields: Vec::new(),
            block: None,
            label_width: None,
            label_gap: 1,
            row_spacing: 0,
            style: Style::new(),
            label_style: Style::new(),
            help_style: Style::new(),
        }
    }

    /// Appends one [`FormField`].
    #[must_use]
    pub fn field(mut self, field: FormField<'a>) -> Self {
        self.fields.push(field);
        self
    }

    /// Replaces all fields with `fields`.
    #[must_use]
    pub fn fields<I>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = FormField<'a>>,
    {
        self.fields = fields.into_iter().collect();
        self
    }

    /// Frames the form in `block`; fields are laid out in [`Block::inner`].
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Pins the label column width (default: auto — the widest label).
    #[must_use]
    pub fn label_width(mut self, width: u16) -> Self {
        self.label_width = Some(width);
        self
    }

    /// Sets the gap between the label column and the control column (default
    /// `1`).
    #[must_use]
    pub fn label_gap(mut self, gap: u16) -> Self {
        self.label_gap = gap;
        self
    }

    /// Sets the blank rows inserted between fields (default `0`).
    #[must_use]
    pub fn row_spacing(mut self, rows: u16) -> Self {
        self.row_spacing = rows;
        self
    }

    /// Sets the base [`Style`], filling the content area beneath the labels
    /// and help lines (and the [`block`](Self::block) frame's fill).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the base [`Style`] for labels, beneath each label line/span style.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Sets the base [`Style`] for help lines, beneath each help line/span
    /// style.
    #[must_use]
    pub fn help_style(mut self, style: Style) -> Self {
        self.help_style = style;
        self
    }

    /// The content area: [`area`] minus the framing [`block`](Self::block), or
    /// `area` itself when there is no block — the same rule
    /// [`Modal::inner`](crate::Modal::inner) uses.
    ///
    /// [`area`]: Self::layout
    fn content(&self, area: Rect) -> Rect {
        match &self.block {
            Some(block) => block.inner(area),
            None => area,
        }
    }

    /// The label column width: [`label_width`](Self::label_width) if pinned,
    /// else the widest label, clamped to the content width.
    fn label_column(&self, content_width: u16) -> u16 {
        let auto = self
            .fields
            .iter()
            .map(|f| f.label.width() as u16)
            .max()
            .unwrap_or(0);
        self.label_width.unwrap_or(auto).min(content_width)
    }

    /// The per-field placement, the single source both [`layout`](Self::layout)
    /// and [`Widget::render`] use so a control always lands where its label
    /// was drawn. Always returns exactly one entry per field (out-of-area
    /// rows clip to empty rects), so caller indices stay aligned — total.
    fn place(&self, area: Rect) -> Vec<FieldRects> {
        let content = self.content(area);
        let label_col = self.label_column(content.width);

        // Reuse the core layout divider for the label / control column split:
        // a fixed label+gap band, the rest to the control column.
        let band = label_col.saturating_add(self.label_gap);
        let [label_area, control_area] =
            Layout::horizontal([Constraint::Length(band), Constraint::Min(0)]).areas(content);

        let bottom = content.y.saturating_add(content.height);
        let last = self.fields.len().saturating_sub(1);
        let mut y = content.y;
        let mut out = Vec::with_capacity(self.fields.len());

        for (i, field) in self.fields.iter().enumerate() {
            let body_avail = bottom.saturating_sub(y);
            let body_h = field.body_height.min(body_avail);

            let label = Rect::new(label_area.x, y, label_col, body_h);
            let control = Rect::new(control_area.x, y, control_area.width, body_h);

            let help_y = y.saturating_add(field.body_height);
            let help_h = if field.help.is_some() {
                bottom.saturating_sub(help_y).min(1)
            } else {
                0
            };
            let help = Rect::new(control_area.x, help_y, control_area.width, help_h);

            out.push(FieldRects {
                label,
                control,
                help,
            });

            let mut used = field.body_height;
            used += u16::from(field.help.is_some());
            if i != last {
                used = used.saturating_add(self.row_spacing);
            }
            y = y.saturating_add(used);
        }
        out
    }

    /// The control [`Rect`] for every field, in field order — a **pure
    /// geometry function** of `area` and the configuration (no state, like
    /// [`Block::inner`](crate::Block::inner)). The caller renders each field's
    /// own widget into its rect; `Form` never touches the control's value.
    ///
    /// Always returns exactly `self.fields().len()` rects (clipped, possibly
    /// empty, when the area is too small) so caller indexing is total.
    #[must_use]
    pub fn layout(&self, area: Rect) -> Vec<Rect> {
        self.place(area).into_iter().map(|r| r.control).collect()
    }

    /// The number of fields (so a caller can size its own per-field state).
    #[must_use]
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Whether the form has no fields.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

impl Default for Form<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Stamps `line`'s glyphs across one row of `rect`, clipped at its right edge,
/// cascading `base` → line → span. A no-op for an empty rect (total).
fn stamp_line(buf: &mut Buffer, line: &Line<'_>, rect: Rect, base: Style) {
    if rect.is_empty() {
        return;
    }
    let y = rect.top();
    let right = rect.right();
    let line_base = base.patch(line.style);
    let mut x = rect.left();
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
}

impl Widget for Form<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        let placement = self.place(area);
        let content = self.content(area);

        // The frame (if any) reserves `content`; the base fill covers the
        // content area so a background reads as one pane (the List idiom).
        if let Some(block) = &self.block {
            block.clone().render(area, buf);
        }
        if !content.is_empty() {
            buf.set_style(content, self.style);
        }

        let label_base = self.style.patch(self.label_style);
        let help_base = self.style.patch(self.help_style);

        for (field, rects) in self.fields.iter().zip(placement) {
            stamp_line(buf, &field.label, rects.label, label_base);
            if let Some(help) = &field.help {
                stamp_line(buf, help, rects.help, help_base);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier, Position, Span};

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

    fn sample() -> Form<'static> {
        Form::new()
            .field(FormField::new("Name", 1))
            .field(FormField::new("Bio", 2).help("optional"))
    }

    #[test]
    fn layout_returns_one_control_rect_per_field_right_of_the_label_column() {
        // Widest label "Name" = 4, + 1 gap ⇒ controls start at x = 5.
        let rects = sample().layout(Rect::new(0, 0, 20, 6));
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0], Rect::new(5, 0, 15, 1));
        // Field 1 starts after field 0's 1 body row ⇒ y = 1, 2 body rows.
        assert_eq!(rects[1], Rect::new(5, 1, 15, 2));
    }

    #[test]
    fn render_draws_the_labels_in_the_aligned_column_only() {
        // Labels are left-aligned in a 4-wide column; controls (x>=5) are the
        // caller's to draw, so they stay blank. The help line is clipped to
        // the 7-wide control column ("optional" -> "optiona").
        let expected = [
            "Name        ",
            "Bio         ",
            "            ",
            "     optiona",
        ]
        .join("\n")
            + "\n";
        assert_eq!(lines(sample(), 12, 4), expected);
    }

    #[test]
    fn labels_and_help_land_on_the_rows_layout_assigns() {
        let form = sample();
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 4));
        form.render(buf.area(), &mut buf);
        // Row 0: "Name" label.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'N');
        // Row 1: "Bio" label (field 1's 2-row body starts here).
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'B');
        // Row 3: the help line, beneath field 1's 2-row body, in the control
        // column (x = 5).
        assert_eq!(buf.get(Position::new(5, 3)).unwrap().symbol, 'o');
    }

    #[test]
    fn a_pinned_label_width_overrides_the_auto_column() {
        let form = sample().label_width(8);
        let rects = form.layout(Rect::new(0, 0, 20, 6));
        // Pinned 8 + 1 gap ⇒ controls at x = 9.
        assert_eq!(rects[0], Rect::new(9, 0, 11, 1));
    }

    #[test]
    fn row_spacing_inserts_blank_rows_between_fields() {
        let form = Form::new()
            .field(FormField::new("A", 1))
            .field(FormField::new("B", 1))
            .row_spacing(1);
        let rects = form.layout(Rect::new(0, 0, 10, 5));
        assert_eq!(rects[0], Rect::new(2, 0, 8, 1));
        // 1 body row + 1 spacing row ⇒ field 1 at y = 2.
        assert_eq!(rects[1], Rect::new(2, 2, 8, 1));
    }

    #[test]
    fn a_block_frames_the_fields_in_the_inner_area() {
        let form = Form::new()
            .field(FormField::new("X", 1))
            .block(Block::bordered());
        let rects = form.layout(Rect::new(0, 0, 8, 3));
        // Inside the border: content origin (1,1); label "X"=1 + 1 gap ⇒ x=3.
        assert_eq!(rects[0], Rect::new(3, 1, 4, 1));
        assert_eq!(lines(form, 8, 3), "┌──────┐\n│X     │\n└──────┘\n");
    }

    #[test]
    fn an_area_too_short_clips_trailing_fields_to_empty_rects() {
        // Only 1 row, but two 1-row fields: field 1's rect clips to height 0,
        // and the count still matches the field count (total).
        let rects = sample().layout(Rect::new(0, 0, 10, 1));
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].height, 1);
        assert_eq!(rects[1].height, 0);
    }

    #[test]
    fn an_empty_form_lays_out_nothing_and_still_renders_its_block() {
        let form = Form::new().block(Block::bordered());
        assert!(form.layout(Rect::new(0, 0, 4, 3)).is_empty());
        assert_eq!(lines(form, 4, 3), "┌──┐\n│  │\n└──┘\n");
    }

    #[test]
    fn the_label_column_clamps_to_the_content_width() {
        // Label far wider than the area: the column clamps, no panic, the
        // control column collapses to width 0.
        let form = Form::new().field(FormField::new("a very long label", 1));
        let rects = form.layout(Rect::new(0, 0, 5, 1));
        assert_eq!(rects[0].width, 0);
        assert_eq!(rects.len(), 1);
    }

    #[test]
    fn label_style_cascades_base_then_label_then_span() {
        let label = Line::from(vec![
            Span::styled("E", Style::new().fg(Color::Red)),
            Span::raw("rr"),
        ])
        .style(Style::new().add_modifier(Modifier::BOLD));
        let form = Form::new()
            .field(FormField::new(label, 1))
            .style(Style::new().bg(Color::Blue))
            .label_style(Style::new().fg(Color::Green));
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        form.render(buf.area(), &mut buf);

        let e = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(e.symbol, 'E');
        assert_eq!(e.fg, Color::Red); // span fg wins
        assert_eq!(e.bg, Color::Blue); // form base fill
        assert!(e.modifier.contains(Modifier::BOLD)); // label line modifier

        let r = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(r.symbol, 'r');
        assert_eq!(r.fg, Color::Green); // label_style base (no span fg)
    }

    #[test]
    fn render_and_layout_agree_on_geometry() {
        // The control rect `layout` hands back is exactly where `render` left
        // the row blank for the caller's widget.
        let form = sample();
        let rects = form.clone().layout(Rect::new(0, 0, 14, 4));
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 4));
        form.render(buf.area(), &mut buf);
        // Field 0's control row/col is untouched by `render`.
        let c = rects[0];
        for x in c.left()..c.right() {
            assert_eq!(buf.get(Position::new(x, c.top())).unwrap().symbol, ' ');
        }
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        sample().render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn layout_is_total_for_a_zero_area() {
        // Still one rect per field, all empty — caller indices stay aligned.
        let rects = sample().layout(Rect::new(0, 0, 0, 0));
        assert_eq!(rects.len(), 2);
        assert!(rects.iter().all(|r| r.is_empty()));
    }
}
