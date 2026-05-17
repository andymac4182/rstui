//! [`Sources`] — a collapsible "Used N sources" disclosure listing the web
//! sources an agent grounded a reply on.
//!
//! # A pure projection of caller-owned `open`
//!
//! The ai-elements `Sources` is a `Collapsible` with a trigger ("Used N
//! sources" + a chevron) and a body of `Source` links. Whether it is open is
//! ordinary application state — exactly like
//! [`Accordion`](rstui_widgets::Accordion)'s `expanded`. So `Sources` owns
//! none of it: it projects the caller's `&[(title, href)]` and a caller-owned
//! [`open`](Sources::open) `bool` the reducer flips on a header click (the
//! host hit-tests [`header_rect`](Sources::header_rect), the documented mouse
//! seam). The widget only ever reads that state — no callback, the
//! [`Accordion`](rstui_widgets::Accordion) discipline.
//!
//! Like [`Accordion`](rstui_widgets::Accordion) it draws the header itself and
//! exposes [`body_rect`](Sources::body_rect) for the source lines (each a
//! `Link`-style underlined line: a book glyph + the title). The caller never
//! needs to render the body separately — the widget draws the visible
//! sources into that rect; [`body_rect`](Sources::body_rect) is for click
//! routing.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area
//! clips, a closed disclosure reserves no body, and over-many sources clip at
//! the bottom — never a panic.

use rstui_core::{Buffer, Modifier, Position, Rect, Style, Widget};

/// A collapsible "Used N sources" disclosure listing grounding sources.
///
/// Row 0 is the trigger — a `▾`/`▸` chevron and `Used N sources` (the count
/// is the slice length). When [`open`](Self::open) the rows below are one
/// `Link`-style line per source: a `▸` book glyph then the title, underlined
/// in [`link_style`](Self::link_style). `Sources` owns no state — see the
/// [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::sources::Sources;
///
/// let refs = [
///     ("Rust Book".to_string(), "https://doc.rust-lang.org".to_string()),
///     ("RFC 2056".to_string(), "https://rfcs".to_string()),
/// ];
/// let widget = Sources::new(&refs).open(true);
/// let area = Rect::new(0, 0, 24, 4);
///
/// // The header is row 0; the open body is the rows below it.
/// assert_eq!(widget.header_rect(area), Rect::new(0, 0, 24, 1));
/// assert_eq!(widget.body_rect(area), Some(Rect::new(0, 1, 24, 3)));
///
/// let mut buf = Buffer::empty(area);
/// widget.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '▾');
/// assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'U'); // "Used 2…"
/// ```
#[derive(Debug, Clone)]
pub struct Sources<'a> {
    sources: &'a [(String, String)],
    open: bool,
    style: Style,
    header_style: Style,
    link_style: Style,
}

impl<'a> Sources<'a> {
    /// A closed disclosure over `sources` (`(title, href)` pairs).
    #[must_use]
    pub fn new(sources: &'a [(String, String)]) -> Self {
        Self {
            sources,
            open: false,
            style: Style::new(),
            header_style: Style::new().add_modifier(Modifier::BOLD),
            link_style: Style::new().add_modifier(Modifier::UNDERLINED),
        }
    }

    /// Sets the caller-owned open flag (the reducer flips it on a
    /// [`header_rect`](Self::header_rect) click; the widget only reads it).
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the base [`Style`], beneath the header and link styles.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] the "Used N sources" trigger row is drawn with.
    #[must_use]
    pub fn header_style(mut self, header_style: Style) -> Self {
        self.header_style = header_style;
        self
    }

    /// Sets the [`Style`] each source line is drawn with (default
    /// underlined, like a [`Link`](rstui_widgets::Link)).
    #[must_use]
    pub fn link_style(mut self, link_style: Style) -> Self {
        self.link_style = link_style;
        self
    }

    /// The 1-row trigger [`Rect`] (the top row), or `None` for an empty
    /// area. The host hit-tests a click against this to toggle
    /// [`open`](Self::open).
    #[must_use]
    pub fn header_rect(&self, area: Rect) -> Rect {
        Rect::new(area.left(), area.top(), area.width, area.height.min(1))
    }

    /// The body [`Rect`] (the rows below the header) when
    /// [`open`](Self::open) and there is room, else `None`. The widget draws
    /// the source lines into it; the host hit-tests a click here against
    /// `(click_y - rect.top())` to pick a source.
    #[must_use]
    pub fn body_rect(&self, area: Rect) -> Option<Rect> {
        if !self.open || area.is_empty() || area.height <= 1 {
            return None;
        }
        Some(Rect::new(
            area.left(),
            area.top().saturating_add(1),
            area.width,
            area.height.saturating_sub(1),
        ))
    }
}

impl Widget for Sources<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        buf.set_style(area, self.style);

        // The trigger row: chevron + "Used N sources".
        let header_base = self.style.patch(self.header_style);
        let y = area.top();
        let chevron = if self.open { '▾' } else { '▸' };
        let mut x = area.left();
        buf.set_cell(Position::new(x, y), chevron, header_base);
        x = x.saturating_add(2);
        let title = format!("Used {} sources", self.sources.len());
        for ch in title.chars() {
            if x >= area.right() {
                break;
            }
            buf.set_cell(Position::new(x, y), ch, header_base);
            x = x.saturating_add(1);
        }

        // The body: one Link-style line per source.
        if let Some(body) = self.body_rect(area) {
            let link_base = self.style.patch(self.link_style);
            for (row, (source_title, _href)) in
                self.sources.iter().take(body.height as usize).enumerate()
            {
                let ly = body.top().saturating_add(row as u16);
                let mut lx = body.left();
                buf.set_cell(Position::new(lx, ly), '▸', self.style);
                lx = lx.saturating_add(2);
                for ch in source_title.chars() {
                    if lx >= body.right() {
                        break;
                    }
                    buf.set_cell(Position::new(lx, ly), ch, link_base);
                    lx = lx.saturating_add(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs() -> Vec<(String, String)> {
        vec![
            ("Rust".to_string(), "https://a".to_string()),
            ("RFC".to_string(), "https://b".to_string()),
        ]
    }

    fn lines(widget: Sources<'_>, w: u16, h: u16) -> String {
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
    fn a_closed_disclosure_shows_only_the_count_trigger() {
        let r = refs();
        assert_eq!(
            lines(Sources::new(&r), 16, 2),
            "▸ Used 2 sources\n                \n"
        );
    }

    #[test]
    fn an_open_disclosure_lists_the_sources() {
        let r = refs();
        assert_eq!(
            lines(Sources::new(&r).open(true), 16, 3),
            "▾ Used 2 sources\n▸ Rust          \n▸ RFC           \n"
        );
    }

    #[test]
    fn header_rect_is_the_top_row_body_rect_is_below_when_open() {
        let r = refs();
        let area = Rect::new(0, 0, 16, 4);
        assert_eq!(Sources::new(&r).header_rect(area), Rect::new(0, 0, 16, 1));
        assert_eq!(
            Sources::new(&r).open(true).body_rect(area),
            Some(Rect::new(0, 1, 16, 3))
        );
        // Closed → no body.
        assert_eq!(Sources::new(&r).body_rect(area), None);
    }

    #[test]
    fn over_many_sources_clip_at_the_bottom() {
        let r = refs();
        // height 2 → header + only the first source.
        assert_eq!(
            lines(Sources::new(&r).open(true), 16, 2),
            "▾ Used 2 sources\n▸ Rust          \n"
        );
    }

    #[test]
    fn zero_count_is_handled() {
        let empty: [(String, String); 0] = [];
        assert_eq!(lines(Sources::new(&empty), 16, 1), "▸ Used 0 sources\n");
    }

    #[test]
    fn tiny_and_zero_areas_are_safe() {
        let r = refs();
        assert_eq!(
            Sources::new(&r).open(true).body_rect(Rect::new(0, 0, 5, 1)),
            None
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Sources::new(&r).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
