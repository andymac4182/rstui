//! [`Shimmer`] — an animated-text loading shimmer: a "thinking…" / "still
//! generating" affordance that sweeps a brightened window across a line of
//! text while the agent is busy.
//!
//! # A pure projection of a caller-owned tick — no wall clock
//!
//! The ai-elements `Shimmer` is a `motion` component animating a moving
//! gradient over text. rstui forbids a wall clock in `view`
//! ([ADR 0012](https://github.com/andymac4182/rstui/blob/main/docs/composition.md)):
//! [`Spinner`](rstui_widgets::Spinner)/[`Skeleton`](rstui_widgets::Skeleton)
//! established the answer — a caller-owned [`tick`](Shimmer::tick) advanced by
//! the reducer (or `frame.count()`), never the widget. `Shimmer` is exactly
//! that contract for *text*: it draws the text dim, then a small window of
//! [`spread`](Shimmer::spread) columns brightened, centred on a position that
//! sweeps left→right and wraps with the tick. The widget advances nothing at
//! render time.
//!
//! Like [`Skeleton`](rstui_widgets::Skeleton) it is a leaf adornment: no
//! [`Block`](rstui_widgets::Block), the sweep is a single moving span over the
//! caller's `&str`.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule an empty area, empty
//! text, a zero `spread`, and any (however large) `tick` are all safe
//! clips/wraps — never a modulo-by-zero or an out-of-range panic.

use rstui_core::{Buffer, Modifier, Position, Rect, Style, Widget};

/// An animated-text loading shimmer — a pure projection of a caller-owned
/// animation [`tick`](Self::tick).
///
/// Draws [`text`](Self::new) in the dim [`style`](Self::style); a window of
/// [`spread`](Self::spread) columns each side of the sweep centre is redrawn
/// in the bright [`shimmer_style`](Self::shimmer_style). There is no clock —
/// advance [`tick`](Self::tick) in `update` (or pass `frame.count()`) and the
/// highlight travels, exactly like [`Spinner`](rstui_widgets::Spinner).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::shimmer::Shimmer;
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 9, 1));
/// Shimmer::new("Thinking…").tick(0).spread(1).render(buf.area(), &mut buf);
/// // The whole string is laid down (dim + the bright window at the head).
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'T');
/// assert_eq!(buf.get(Position::new(8, 0)).unwrap().symbol, '…');
/// // The bright window centre wraps the text width with the tick.
/// assert_eq!(Shimmer::new("abcd").tick(6).bright_center(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct Shimmer<'a> {
    text: &'a str,
    tick: u64,
    spread: u16,
    style: Style,
    shimmer_style: Style,
}

impl<'a> Shimmer<'a> {
    /// A shimmer over `text`, at tick `0`, with a default
    /// [`spread`](Self::spread) of `2` and the default dim/bright styles.
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            tick: 0,
            spread: 2,
            style: Style::new().add_modifier(Modifier::DIM),
            shimmer_style: Style::new().add_modifier(Modifier::BOLD),
        }
    }

    /// Sets the caller-owned animation tick (the reducer advances it; the
    /// widget only reads it). Any value is in range — it wraps the text
    /// width.
    #[must_use]
    pub fn tick(mut self, tick: u64) -> Self {
        self.tick = tick;
        self
    }

    /// Sets the half-width of the bright window in columns: a `spread` of `n`
    /// brightens `n` columns each side of the sweep centre (default `2`).
    #[must_use]
    pub fn spread(mut self, spread: u16) -> Self {
        self.spread = spread;
        self
    }

    /// Sets the dim base [`Style`] the un-lit text is drawn with.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the bright [`Style`] the swept window is drawn with.
    #[must_use]
    pub fn shimmer_style(mut self, shimmer_style: Style) -> Self {
        self.shimmer_style = shimmer_style;
        self
    }

    /// The column the bright window is centred on this tick — `tick` wrapped
    /// over the text's character width (or `0` for empty text).
    #[must_use]
    pub fn bright_center(&self) -> u16 {
        let width = self.text.chars().count();
        if width == 0 {
            return 0;
        }
        (self.tick % width as u64) as u16
    }
}

impl Widget for Shimmer<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() || self.text.is_empty() {
            return;
        }
        let center = self.bright_center();
        let y = area.top();
        let right = area.right();
        let mut x = area.left();
        for (col, ch) in self.text.chars().enumerate() {
            if x >= right {
                break;
            }
            let dist = (col as i64 - i64::from(center)).unsigned_abs();
            let style = if dist <= u64::from(self.spread) {
                self.shimmer_style
            } else {
                self.style
            };
            buf.set_cell(Position::new(x, y), ch, style);
            x = x.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(widget: Shimmer<'_>, width: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
        widget.render(buf.area(), &mut buf);
        (0..width)
            .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
            .collect()
    }

    #[test]
    fn it_lays_down_the_whole_text() {
        assert_eq!(row(Shimmer::new("hello"), 5), "hello");
    }

    #[test]
    fn the_bright_window_is_centred_on_the_swept_column() {
        // tick 2, spread 1 → columns 1..=3 bright, the rest dim.
        let widget = Shimmer::new("abcde").tick(2).spread(1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        widget.render(buf.area(), &mut buf);
        let bright: Vec<bool> = (0..5)
            .map(|x| {
                buf.get(Position::new(x, 0))
                    .unwrap()
                    .modifier
                    .contains(Modifier::BOLD)
            })
            .collect();
        assert_eq!(bright, vec![false, true, true, true, false]);
    }

    #[test]
    fn the_sweep_wraps_the_text_width() {
        assert_eq!(Shimmer::new("abcd").tick(0).bright_center(), 0);
        assert_eq!(Shimmer::new("abcd").tick(3).bright_center(), 3);
        assert_eq!(Shimmer::new("abcd").tick(4).bright_center(), 0);
        assert_eq!(Shimmer::new("abcd").tick(6).bright_center(), 2);
        // A huge tick still wraps cleanly (no panic, in range).
        assert!(Shimmer::new("abcd").tick(u64::MAX).bright_center() < 4);
    }

    #[test]
    fn empty_text_has_a_zero_center_and_renders_nothing() {
        assert_eq!(Shimmer::new("").bright_center(), 0);
        assert_eq!(row(Shimmer::new(""), 4), "    ");
    }

    #[test]
    fn the_text_clips_at_the_right_edge() {
        assert_eq!(row(Shimmer::new("overlong"), 4), "over");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Shimmer::new("hi").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
