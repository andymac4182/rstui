//! [`ModelSelector`] — a command-palette-style model picker: the "switch
//! model" dropdown filtered as you type.
//!
//! # A pure projection of `&[Model]` + caller-owned filter/selection/open
//!
//! The ai-elements `ModelSelector` is a combobox over the available models.
//! The filter text, the selected index, and whether the panel is open are all
//! ordinary application state (the documented
//! [`CommandPalette`](rstui_widgets::CommandPalette)/overlay shape). So
//! `ModelSelector` owns nothing: it projects the caller's `&[Model]` and a
//! caller-owned [`filter`](ModelSelector::filter) /
//! [`selected`](ModelSelector::selected) / [`open`](ModelSelector::open).
//!
//! The list is a [`List`] — we *reuse* the widget, not
//! reinvent it: matching rows (a case-insensitive substring of
//! `provider/name`) become [`ListItem`]s. Picking is
//! the documented hit-test seam: [`row_rects`](ModelSelector::row_rects) maps
//! a click to a *matched* position; the host turns it into a
//! [`ModelSelectorIntent::Pick`] (carrying the index into the *original*
//! slice), never a callback.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area, a
//! filter that matches nothing, and an out-of-range selection are all safe —
//! never a panic.

use rstui_core::{Buffer, Color, Modifier, Rect, Style, Widget};
use rstui_widgets::{Block, List, ListItem};

/// One selectable model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// The model id (e.g. `gpt-5`), the value the reducer commits.
    pub id: String,
    /// The provider label (e.g. `OpenAI`).
    pub provider: String,
    /// The human display name.
    pub name: String,
}

impl Model {
    /// A model with `id`, `provider`, and display `name`.
    pub fn new(
        id: impl Into<String>,
        provider: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            provider: provider.into(),
            name: name.into(),
        }
    }

    /// The text a filter matches and a row shows (`provider / name`).
    fn label(&self) -> String {
        format!("{} / {}", self.provider, self.name)
    }
}

/// The reducer-consumed intent a [`ModelSelector`] surfaces — the host maps a
/// click in a [`row_rects`](ModelSelector::row_rects) entry to `Pick(i)`
/// where `i` indexes the **original** model slice, and the reducer commits
/// that model and closes the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSelectorIntent {
    /// Commit the model at this index in the original slice.
    Pick(usize),
}

/// A command-palette-style model picker.
///
/// Projects the caller's `&[Model]` and a caller-owned
/// [`filter`](Self::filter) / [`selected`](Self::selected) /
/// [`open`](Self::open). When [`open`](Self::open) it draws a framed
/// [`List`] of the rows whose `provider / name` contains
/// [`filter`](Self::filter) (case-insensitive), the
/// [`selected`](Self::selected)-th highlighted. `ModelSelector` owns no state
/// — see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::model_selector::{Model, ModelSelector};
///
/// let models = [
///     Model::new("gpt-5", "OpenAI", "GPT-5"),
///     Model::new("opus-4", "Anthropic", "Claude Opus 4"),
/// ];
/// let sel = ModelSelector::new(&models).filter("anthropic").open(true);
///
/// // Only the Anthropic row matches; its original index is 1.
/// assert_eq!(sel.matches(), vec![1]);
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 24, 3));
/// sel.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌'); // framed
/// ```
#[derive(Debug, Clone)]
pub struct ModelSelector<'a> {
    models: &'a [Model],
    filter: &'a str,
    selected: usize,
    open: bool,
    style: Style,
    highlight_style: Style,
}

impl<'a> ModelSelector<'a> {
    /// A closed picker over `models`, no filter, first row selected.
    #[must_use]
    pub fn new(models: &'a [Model]) -> Self {
        Self {
            models,
            filter: "",
            selected: 0,
            open: false,
            style: Style::new(),
            highlight_style: Style::new()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        }
    }

    /// Sets the caller-owned filter text (the reducer owns it; matched
    /// case-insensitively against `provider / name`).
    #[must_use]
    pub fn filter(mut self, filter: &'a str) -> Self {
        self.filter = filter;
        self
    }

    /// Sets the caller-owned selection — an index into the **matched** rows
    /// (the reducer owns it; clamped by [`List`]).
    #[must_use]
    pub fn selected(mut self, selected: usize) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the caller-owned open flag (the reducer flips it; closed renders
    /// nothing — the panel is an overlay).
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the base [`Style`] (the panel background/frame).
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

    /// The **original-slice indices** of the models matching
    /// [`filter`](Self::filter), in order. The *n*-th matched row is
    /// `matches()[n]`; a [`row_rects`](Self::row_rects) click at row *n*
    /// maps to [`ModelSelectorIntent::Pick(matches()[n])`](ModelSelectorIntent::Pick).
    #[must_use]
    pub fn matches(&self) -> Vec<usize> {
        let needle = self.filter.to_lowercase();
        self.models
            .iter()
            .enumerate()
            .filter(|(_, m)| needle.is_empty() || m.label().to_lowercase().contains(&needle))
            .map(|(idx, _)| idx)
            .collect()
    }

    /// The framing block of the open panel.
    fn block() -> Block<'a> {
        Block::bordered().title("Select model")
    }

    /// The hit [`Rect`] of every visible matched row when [`open`](Self::open),
    /// in order (parallel to [`matches`](Self::matches), clipped to the
    /// panel). The host pairs a click's row index with
    /// [`matches`](Self::matches) to build the intent.
    #[must_use]
    pub fn row_rects(&self, area: Rect) -> Vec<Rect> {
        if !self.open || area.is_empty() {
            return Vec::new();
        }
        let inner = Self::block().inner(area);
        if inner.is_empty() {
            return Vec::new();
        }
        let count = self.matches().len().min(inner.height as usize);
        (0..count)
            .map(|row| {
                Rect::new(
                    inner.left(),
                    inner.top().saturating_add(row as u16),
                    inner.width,
                    1,
                )
            })
            .collect()
    }
}

impl Widget for ModelSelector<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.open || area.is_empty() {
            return;
        }
        let items: Vec<ListItem> = self
            .matches()
            .iter()
            .map(|&idx| ListItem::new(self.models[idx].label()))
            .collect();
        List::new(items)
            .block(Self::block())
            .style(self.style)
            .highlight_style(self.highlight_style)
            .highlight_symbol("> ")
            .selected(Some(self.selected))
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Position;

    fn models() -> Vec<Model> {
        vec![
            Model::new("gpt-5", "OpenAI", "GPT-5"),
            Model::new("opus", "Anthropic", "Claude Opus"),
            Model::new("haiku", "Anthropic", "Claude Haiku"),
        ]
    }

    fn lines(widget: ModelSelector<'_>, w: u16, h: u16) -> String {
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
    fn a_closed_selector_renders_nothing() {
        let m = models();
        assert_eq!(
            lines(ModelSelector::new(&m), 10, 2),
            "          \n          \n"
        );
    }

    #[test]
    fn an_open_selector_lists_matched_models_in_a_frame() {
        let m = models();
        let out = lines(ModelSelector::new(&m).open(true), 22, 5);
        assert!(out.starts_with("┌Select model────────┐"), "got {out:?}");
        assert!(out.contains("OpenAI / GPT-5"), "got {out:?}");
        // The inner width clips the longer label (totality, no overflow).
        assert!(out.contains("Anthropic / Claude"), "got {out:?}");
    }

    #[test]
    fn the_filter_narrows_matches_to_original_indices() {
        let m = models();
        assert_eq!(ModelSelector::new(&m).matches(), vec![0, 1, 2]);
        assert_eq!(
            ModelSelector::new(&m).filter("anthropic").matches(),
            vec![1, 2]
        );
        assert_eq!(ModelSelector::new(&m).filter("GPT").matches(), vec![0]);
        // Case-insensitive, no match → empty.
        assert!(ModelSelector::new(&m).filter("zzz").matches().is_empty());
    }

    #[test]
    fn row_rects_track_each_matched_row_parallel_to_matches() {
        let m = models();
        let sel = ModelSelector::new(&m).filter("anthropic").open(true);
        let rects = sel.row_rects(Rect::new(0, 0, 22, 6));
        assert_eq!(rects.len(), 2);
        // Row 0 → matches()[0] == original index 1, etc.
        assert_eq!(sel.matches(), vec![1, 2]);
        assert_eq!(rects[0].y, 1); // inside the bordered panel
        // Closed → no rects.
        assert!(
            ModelSelector::new(&m)
                .row_rects(Rect::new(0, 0, 22, 6))
                .is_empty()
        );
    }

    #[test]
    fn the_selection_highlights_a_matched_row() {
        let m = models();
        let mut buf = Buffer::empty(Rect::new(0, 0, 22, 5));
        ModelSelector::new(&m)
            .open(true)
            .selected(1)
            .render(buf.area(), &mut buf);
        // The selected (2nd) row carries the "> " highlight symbol.
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, '>');
    }

    #[test]
    fn no_matches_is_an_empty_framed_panel_not_a_panic() {
        let m = models();
        let sel = ModelSelector::new(&m).filter("nope").open(true);
        assert!(sel.matches().is_empty());
        let out = lines(sel, 22, 4);
        assert!(out.starts_with('┌'), "got {out:?}");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let m = models();
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        ModelSelector::new(&m)
            .open(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
