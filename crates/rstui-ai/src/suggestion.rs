//! [`Suggestions`] — a horizontal row of clickable prompt-suggestion pills:
//! the "ask me about…" starter chips shown above an empty composer.
//!
//! # A pure projection; picking is an intent, not a callback
//!
//! The ai-elements `Suggestions` is a horizontally scrollable strip of
//! `Suggestion` buttons. rstui forbids callbacks in `view`
//! ([ADR 0012](https://github.com/andymac4182/rstui/blob/main/docs/composition.md)),
//! so `Suggestions` owns nothing: it projects the caller's `&[String]` and an
//! optional caller-owned [`offset`](Suggestions::offset) (the first pill
//! shown, for horizontal scroll). Each pill's hit [`Rect`] is exposed by
//! [`pill_rects`](Suggestions::pill_rects); the host maps a click to the
//! pill's index and yields the reducer-consumed [`SuggestionIntent::Pick`] —
//! the same hit-test seam [`Tabs`](rstui_widgets::Tabs)/`Menu` use, never an
//! `onClick`.
//!
//! # One row of pills, clipped
//!
//! Pills are laid out left→right from [`offset`](Suggestions::offset) with a
//! one-column gap; a pill that does not fully fit is dropped (so a partial
//! pill never bleeds). It is a leaf adornment — no
//! [`Block`](rstui_widgets::Block) — composed beside the composer with a
//! [`Layout`](rstui_core::Layout) split, like
//! [`Badge`](rstui_widgets::Badge).
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area, an
//! empty slice, and an out-of-range offset are all safe (empty layout, no
//! panic).

use rstui_core::{Buffer, Position, Rect, Style, Widget};

/// The reducer-consumed intent a [`Suggestions`] surfaces — the host maps a
/// click in a [`pill_rects`](Suggestions::pill_rects) entry to
/// `Pick(index)` and the reducer submits that suggestion as the prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionIntent {
    /// The suggestion pill at this index was activated.
    Pick(usize),
}

/// A horizontal row of clickable prompt-suggestion pills.
///
/// Projects the caller's suggestion strings and a caller-owned
/// [`offset`](Self::offset) (the first pill shown). Each pill is its label
/// padded one space each side, drawn in [`pill_style`](Self::pill_style),
/// with a one-column gap between pills; a pill that would overflow the right
/// edge is dropped. `Suggestions` owns no state — see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::suggestion::Suggestions;
///
/// let picks = ["Summarise".to_string(), "Translate".to_string()];
/// let widget = Suggestions::new(&picks);
/// let area = Rect::new(0, 0, 24, 1);
///
/// // Pill 0 is " Summarise " (11 wide) at x0; pill 1 follows after a gap.
/// let rects = widget.pill_rects(area);
/// assert_eq!(rects[0], Rect::new(0, 0, 11, 1));
/// assert_eq!(rects[1].x, 12);
///
/// let mut buf = Buffer::empty(area);
/// widget.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'S');
/// ```
#[derive(Debug, Clone)]
pub struct Suggestions<'a> {
    items: &'a [String],
    offset: usize,
    gap: u16,
    style: Style,
    pill_style: Style,
}

impl<'a> Suggestions<'a> {
    /// A row of the suggestion `items`, scrolled to the first, with a
    /// one-column gap between pills.
    #[must_use]
    pub fn new(items: &'a [String]) -> Self {
        Self {
            items,
            offset: 0,
            gap: 1,
            style: Style::new(),
            pill_style: Style::new(),
        }
    }

    /// Sets the caller-owned index of the first pill shown (horizontal
    /// scroll; the reducer owns it). An out-of-range offset yields an empty
    /// row.
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Sets the blank columns between adjacent pills (default `1`).
    #[must_use]
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// Sets the base [`Style`] (the row background, beneath the pills).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] each pill is drawn with (over the base).
    #[must_use]
    pub fn pill_style(mut self, pill_style: Style) -> Self {
        self.pill_style = pill_style;
        self
    }

    /// The hit [`Rect`] of every fully-visible pill, in order from
    /// [`offset`](Self::offset). The vec is shorter than the slice when
    /// pills are scrolled off or clipped; index *i* of the result is the
    /// *(offset + i)*-th suggestion — the host maps a click to
    /// [`SuggestionIntent::Pick(offset + i)`](SuggestionIntent::Pick).
    #[must_use]
    pub fn pill_rects(&self, area: Rect) -> Vec<Rect> {
        if area.is_empty() {
            return Vec::new();
        }
        let y = area.top();
        let right = area.right();
        let mut x = area.left();
        let mut rects = Vec::new();
        for label in self.items.iter().skip(self.offset) {
            let pill_w = (label.chars().count() as u16).saturating_add(2);
            if x.saturating_add(pill_w) > right {
                break;
            }
            rects.push(Rect::new(x, y, pill_w, 1));
            x = x.saturating_add(pill_w).saturating_add(self.gap);
            if x >= right {
                break;
            }
        }
        rects
    }
}

impl Widget for Suggestions<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        buf.set_style(area, self.style);
        let pill_style = self.style.patch(self.pill_style);
        let rects = self.pill_rects(area);
        for (rect, label) in rects.iter().zip(self.items.iter().skip(self.offset)) {
            buf.set_style(*rect, pill_style);
            let mut x = rect.left().saturating_add(1);
            for ch in label.chars() {
                buf.set_cell(Position::new(x, rect.top()), ch, pill_style);
                x = x.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Color;

    fn row(widget: Suggestions<'_>, width: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
        widget.render(buf.area(), &mut buf);
        (0..width)
            .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
            .collect()
    }

    #[test]
    fn pills_are_padded_labels_with_a_gap() {
        let picks = ["Hi".to_string(), "Yo".to_string()];
        assert_eq!(row(Suggestions::new(&picks), 12), " Hi   Yo    ");
    }

    #[test]
    fn pill_rects_track_each_label_width() {
        let picks = ["abc".to_string(), "de".to_string()];
        let rects = Suggestions::new(&picks).pill_rects(Rect::new(0, 0, 20, 1));
        assert_eq!(rects, vec![Rect::new(0, 0, 5, 1), Rect::new(6, 0, 4, 1)]);
    }

    #[test]
    fn an_overflowing_pill_is_dropped_not_clipped() {
        let picks = ["ok".to_string(), "toolong".to_string()];
        // " ok " fits in width 6, the next pill (9 wide) does not → dropped.
        let rects = Suggestions::new(&picks).pill_rects(Rect::new(0, 0, 6, 1));
        assert_eq!(rects, vec![Rect::new(0, 0, 4, 1)]);
        assert_eq!(row(Suggestions::new(&picks), 6), " ok   ");
    }

    #[test]
    fn offset_scrolls_the_row() {
        let picks = ["aa".to_string(), "bb".to_string(), "cc".to_string()];
        // offset 1 → the row starts at "bb".
        assert_eq!(row(Suggestions::new(&picks).offset(1), 10), " bb   cc  ");
    }

    #[test]
    fn an_out_of_range_offset_is_empty() {
        let picks = ["x".to_string()];
        assert!(
            Suggestions::new(&picks)
                .offset(5)
                .pill_rects(Rect::new(0, 0, 10, 1))
                .is_empty()
        );
    }

    #[test]
    fn the_pill_style_cascades_over_the_base() {
        let picks = ["A".to_string()];
        let widget = Suggestions::new(&picks).pill_style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        widget.render(buf.area(), &mut buf);
        // Pill cells (0..3) are accented; past the pill is the base.
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().bg, Color::Blue);
        assert_eq!(buf.get(Position::new(4, 0)).unwrap().bg, Color::Reset);
    }

    #[test]
    fn empty_and_zero_are_safe() {
        let empty: [String; 0] = [];
        assert!(
            Suggestions::new(&empty)
                .pill_rects(Rect::new(0, 0, 10, 1))
                .is_empty()
        );
        let picks = ["x".to_string()];
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Suggestions::new(&picks).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
