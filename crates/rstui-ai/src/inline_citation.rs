//! [`InlineCitation`] — an inline superscript citation marker (`[n]`) and the
//! [`InlineCitationCard`] popover listing the sources behind it.
//!
//! # Two pure projections; the popover open is caller-owned
//!
//! The ai-elements `InlineCitation` is a hover-card: a `[n]`-style badge in
//! the prose that, on hover, shows a card of `{title, url}` sources. rstui
//! forbids a callback/hover side-effect in `view`
//! ([ADR 0012](https://github.com/andymac4182/rstui/blob/main/docs/composition.md)),
//! so this splits into two pure widgets:
//!
//! - [`InlineCitation`] — a one-row marker `[n]` painting only its own cells
//!   (inline, like [`Badge`](rstui_widgets::Badge)); the host hit-tests its
//!   [`marker_rect`](InlineCitation::marker_rect) to toggle the popover.
//! - [`InlineCitationCard`] — the popover body the host renders (last, over
//!   the prose) when its caller-owned "open" is set, listing
//!   `{title, url}` lines; [`size`](InlineCitationCard::size) gives the box it
//!   needs for a [`Popover`](rstui_widgets::Popover)/[`Modal`](rstui_widgets::Modal) placement.
//!
//! Neither owns state — the citation index and "is the card open" are
//! ordinary model fields, the documented overlay-is-model-state shape.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area
//! clips the marker / the card lines — never a panic.

use rstui_core::{Buffer, Position, Rect, Size, Style, Widget};
use rstui_widgets::{Block, Borders};

/// An inline superscript citation marker — a `[n]` chip that sits *within* a
/// line of prose.
///
/// Draws `[`, the [`number`](Self::new), `]` in [`style`](Self::style),
/// painting **only** those cells (inline, the [`Badge`](rstui_widgets::Badge)
/// rule — surrounding prose to either side is untouched). `InlineCitation`
/// owns no state — see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::inline_citation::InlineCitation;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
/// for x in 0..5 { buf.set_cell(Position::new(x, 0), '.', Default::default()); }
/// InlineCitation::new(3).render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '[');
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '3');
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, ']');
/// // Past the marker the prose fill is untouched (inline, not a bar).
/// assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, '.');
/// ```
#[derive(Debug, Clone)]
pub struct InlineCitation {
    number: usize,
    style: Style,
}

impl InlineCitation {
    /// A marker citing source `number` (the `[n]` shown).
    #[must_use]
    pub fn new(number: usize) -> Self {
        Self {
            number,
            style: Style::new(),
        }
    }

    /// Sets the [`Style`] the `[n]` marker is drawn with.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The text the marker occupies (`[n]`).
    fn text(&self) -> String {
        format!("[{}]", self.number)
    }

    /// The marker's own cells within `area` (clipped to the area), or `None`
    /// for an empty area. The host hit-tests a click here to toggle the
    /// citation popover.
    #[must_use]
    pub fn marker_rect(&self, area: Rect) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        let w = (self.text().chars().count() as u16).min(area.width);
        Some(Rect::new(area.left(), area.top(), w, 1))
    }
}

impl Widget for InlineCitation {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let y = area.top();
        let right = area.right();
        let mut x = area.left();
        for ch in self.text().chars() {
            if x >= right {
                break;
            }
            buf.set_cell(Position::new(x, y), ch, self.style);
            x = x.saturating_add(1);
        }
    }
}

/// The popover body for an [`InlineCitation`] — a bordered card listing the
/// cited sources, drawn (last, over the prose) when the caller's "open" flag
/// is set.
///
/// Each source is a two-line entry: the `title` (bold), then the `url`
/// (dimmed). [`size`](Self::size) reports the box it needs so the host can
/// place it with a [`Popover`](rstui_widgets::Popover)/[`Modal`](rstui_widgets::Modal) accessor.
/// `InlineCitationCard` owns no state — "is it open" is a model field.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Size, Widget};
/// use rstui_ai::inline_citation::InlineCitationCard;
///
/// let src = [("Rust Book".to_string(), "https://doc.rs".to_string())];
/// let card = InlineCitationCard::new(&src);
/// // One source → 2 content rows + the border: a 4-row box.
/// assert_eq!(card.size(), Size::new(16, 4));
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 16, 4));
/// card.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
/// assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'R'); // title
/// ```
#[derive(Debug, Clone)]
pub struct InlineCitationCard<'a> {
    sources: &'a [(String, String)],
    style: Style,
}

impl<'a> InlineCitationCard<'a> {
    /// A citation popover over `sources` (`(title, url)` pairs).
    #[must_use]
    pub fn new(sources: &'a [(String, String)]) -> Self {
        Self {
            sources,
            style: Style::new(),
        }
    }

    /// Sets the base [`Style`] (the card background and border).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The box this card needs: wide enough for the longest title/url
    /// (clamped to a sensible minimum), tall enough for two rows per source
    /// plus the border. Feed this to a
    /// [`Popover`](rstui_widgets::Popover)/[`Modal`](rstui_widgets::Modal) placement.
    #[must_use]
    pub fn size(&self) -> Size {
        let widest = self
            .sources
            .iter()
            .flat_map(|(title, url)| [title.chars().count(), url.chars().count()])
            .max()
            .unwrap_or(0);
        let w = (widest as u16).saturating_add(2).max(16);
        let rows = (self.sources.len() as u16).saturating_mul(2).max(1);
        Size::new(w, rows.saturating_add(2))
    }
}

impl Widget for InlineCitationCard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let block = Block::new().borders(Borders::ALL).style(self.style);
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.is_empty() {
            return;
        }
        let dim = self.style.add_modifier(rstui_core::Modifier::DIM);
        let bold = self.style.add_modifier(rstui_core::Modifier::BOLD);
        let mut row = 0u16;
        for (title, url) in self.sources {
            for (text, style) in [(title, bold), (url, dim)] {
                if row >= inner.height {
                    return;
                }
                let y = inner.top().saturating_add(row);
                let mut x = inner.left();
                for ch in text.chars() {
                    if x >= inner.right() {
                        break;
                    }
                    buf.set_cell(Position::new(x, y), ch, style);
                    x = x.saturating_add(1);
                }
                row = row.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier};

    #[test]
    fn the_marker_is_an_inline_chip() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        for x in 0..5 {
            buf.set_cell(Position::new(x, 0), '.', Style::new());
        }
        InlineCitation::new(12).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '[');
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '1');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, '2');
        assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, ']');
        // Past "[12]" the fill is untouched.
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().symbol, '.');
    }

    #[test]
    fn marker_rect_is_the_chip_cells_clipped() {
        let cite = InlineCitation::new(7);
        assert_eq!(
            cite.marker_rect(Rect::new(2, 1, 10, 1)),
            Some(Rect::new(2, 1, 3, 1))
        );
        // Clipped to a narrow area.
        assert_eq!(
            InlineCitation::new(7).marker_rect(Rect::new(0, 0, 2, 1)),
            Some(Rect::new(0, 0, 2, 1))
        );
        assert_eq!(
            InlineCitation::new(7).marker_rect(Rect::new(0, 0, 0, 0)),
            None
        );
    }

    #[test]
    fn the_card_lists_title_then_url_per_source() {
        let src = [
            ("Rust".to_string(), "https://r".to_string()),
            ("RFC".to_string(), "https://f".to_string()),
        ];
        let card = InlineCitationCard::new(&src);
        assert_eq!(card.size(), Size::new(16, 6)); // 2 sources × 2 + border
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 6));
        card.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'R'); // Rust
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, 'h'); // https
        assert_eq!(buf.get(Position::new(1, 3)).unwrap().symbol, 'R'); // RFC
        // The title row is bold, the url row dim.
        assert!(
            buf.get(Position::new(1, 1))
                .unwrap()
                .modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            buf.get(Position::new(1, 2))
                .unwrap()
                .modifier
                .contains(Modifier::DIM)
        );
    }

    #[test]
    fn an_empty_card_still_has_a_minimum_size() {
        let empty: [(String, String); 0] = [];
        assert_eq!(InlineCitationCard::new(&empty).size(), Size::new(16, 3));
    }

    #[test]
    fn the_card_clips_rows_in_a_short_box() {
        let src = [
            ("A".to_string(), "u".to_string()),
            ("B".to_string(), "v".to_string()),
        ];
        // Only 2 inner rows: title A, url u — B/v clip, no panic.
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        InlineCitationCard::new(&src).render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'A');
        assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, 'u');
    }

    #[test]
    fn the_style_cascades_into_the_card() {
        let src = [("T".to_string(), "U".to_string())];
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        InlineCitationCard::new(&src)
            .style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(1, 1)).unwrap().bg, Color::Blue);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let src = [("T".to_string(), "U".to_string())];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        InlineCitationCard::new(&src).render(Rect::new(0, 0, 0, 0), &mut buf);
        InlineCitation::new(1).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
