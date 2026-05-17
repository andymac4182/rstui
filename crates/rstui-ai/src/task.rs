//! [`Task`] — a collapsible agent-task group, the rstui translation of
//! the ai-elements `Task` / `TaskTrigger` / `TaskContent` / `TaskItem` /
//! `TaskItemFile` family (`task.tsx`).
//!
//! # A pure projection of caller-owned items + an open flag
//!
//! ai-elements' `Task` is a `Collapsible` (defaulting open) with a
//! search-icon title and a left-railed list of `TaskItem`s, some of which
//! are inline `TaskItemFile` chips. Here the title, the items, and *which
//! open* are all caller-owned model state ([`TaskItem`] is a plain value
//! type); [`Task`] only *reads* them (ADR 0012 §P1) and draws the header
//! (a 🔍 glyph, the title, a ▾/▸ marker) always, the rail + items only
//! when [`open`](Task::open). A [`TaskItem::file`] is rendered as a
//! `[name]` chip (the `TaskItemFile` bordered pill, in a terminal a
//! bracketed accent run); a [`TaskItem::text`] as a plain dim line.
//!
//! # The collapse seam (mirrors [`Accordion`](rstui_widgets::Accordion))
//!
//! [`Task::header_rect`] / [`Task::body_rect`] are pure geometry
//! accessors; the reducer hit-tests a header click and flips the
//! caller-owned `open` `bool` in `update`. `body_rect` is `None` when
//! collapsed or there is no room — exactly the
//! [`Accordion::layout`](rstui_widgets::Accordion::layout) contract.
//!
//! # Total, never a panic
//!
//! An empty area, a zero-size area, no items, and more items than rows
//! are all safe clips/no-ops (the [`Gauge`](rstui_widgets::Gauge)
//! totality rule — items past the area simply clip).

use rstui_core::{Buffer, Color, Line, Modifier, Position, Rect, Span, Style, Widget};

/// One row inside a [`Task`]: either a plain status line or a file chip
/// (the ai-elements `TaskItem` / `TaskItemFile`). A plain value type the
/// caller owns; the widget only reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskItem {
    /// A `TaskItem`: a plain (dim) status line.
    Text(String),
    /// A `TaskItemFile`: a `[name]` chip (the bordered-pill analogue).
    File(String),
}

impl TaskItem {
    /// A plain status-line item.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// A file-chip item.
    #[must_use]
    pub fn file(name: impl Into<String>) -> Self {
        Self::File(name.into())
    }
}

/// A collapsible agent-task group — a pure projection of a title, a
/// caller-owned `&[TaskItem]`, and a caller-owned [`open`](Self::open)
/// `bool`.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_ai::task::{Task, TaskItem};
///
/// let items = [TaskItem::text("Reading"), TaskItem::file("main.rs")];
/// // `open` is caller state — the reducer flips it on a header click.
/// let task = Task::new("Searching the codebase", &items).open(true);
/// let mut buf = Buffer::empty(Rect::new(0, 0, 30, 4));
/// task.render(buf.area(), &mut buf);
/// ```
#[derive(Debug, Clone)]
pub struct Task<'a> {
    title: &'a str,
    items: &'a [TaskItem],
    open: bool,
    style: Style,
    header_style: Style,
    chip_style: Style,
}

impl<'a> Task<'a> {
    /// A collapsed task titled `title` over `items`, unstyled (a dim
    /// header, an accented chip by default).
    #[must_use]
    pub fn new(title: &'a str, items: &'a [TaskItem]) -> Self {
        Self {
            title,
            items,
            open: false,
            style: Style::new(),
            header_style: Style::new().fg(Color::DarkGray),
            chip_style: Style::new().fg(Color::Cyan),
        }
    }

    /// Sets whether the group is expanded — caller-owned state the reducer
    /// flips on a [`header_rect`](Self::header_rect) click; the widget only
    /// reads it (see the [module docs](self)).
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the base [`Style`] (also fills the region).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the header [`Style`] (default a dim foreground).
    #[must_use]
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Sets the file-chip [`Style`] (default a cyan foreground).
    #[must_use]
    pub fn chip_style(mut self, style: Style) -> Self {
        self.chip_style = style;
        self
    }

    /// The header row rect (🔍 + title + marker), or `None` for an empty
    /// area. A pure function of `area` — the reducer hit-tests a click
    /// against it (mirrors
    /// [`Accordion::layout`](rstui_widgets::Accordion::layout)).
    #[must_use]
    pub fn header_rect(&self, area: Rect) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        Some(Rect::new(area.left(), area.top(), area.width, 1))
    }

    /// The items-body rect (the left-railed list), or `None` when
    /// collapsed or there is no row below the header. A pure function of
    /// `area` and [`open`](Self::open).
    #[must_use]
    pub fn body_rect(&self, area: Rect) -> Option<Rect> {
        if !self.open || area.is_empty() || area.height < 2 {
            return None;
        }
        Some(Rect::new(
            area.left(),
            area.top().saturating_add(1),
            area.width,
            area.height.saturating_sub(1),
        ))
    }

    /// The header [`Line`]: a 🔍 glyph, the title, a ▾/▸ marker.
    fn header_line(&self, base: Style) -> Line<'static> {
        let marker = if self.open { '▾' } else { '▸' };
        Line::from(vec![
            Span::raw("🔍 "),
            Span::styled(self.title.to_owned(), base),
            Span::raw(format!(" {marker}")),
        ])
    }
}

impl Widget for Task<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let base = self.style.patch(self.header_style);
        if let Some(header) = self.header_rect(area) {
            buf.set_style(header, self.style);
            self.header_line(base).render(header, buf);
        }

        let Some(body) = self.body_rect(area) else {
            return;
        };
        buf.set_style(body, self.style);

        // The ai-elements left rail (`border-l-2 pl-4`): a `│ ` gutter,
        // then one item per row, clipped to the body height.
        let rail = self.style.patch(self.header_style);
        for (i, item) in self.items.iter().enumerate() {
            if i as u16 >= body.height {
                break;
            }
            let y = body.top().saturating_add(i as u16);
            buf.set_cell(Position::new(body.left(), y), '│', rail);
            let text_x = body.left().saturating_add(2);
            let text_w = body.width.saturating_sub(2);
            if text_w == 0 {
                continue;
            }
            let text_area = Rect::new(text_x, y, text_w, 1);
            let line = match item {
                TaskItem::Text(text) => {
                    Line::styled(text.clone(), self.style.patch(self.header_style))
                }
                TaskItem::File(name) => Line::from(Span::styled(
                    format!("[{name}]"),
                    self.style
                        .patch(self.chip_style)
                        .add_modifier(Modifier::BOLD),
                )),
            };
            line.render(text_area, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(buf: &Buffer, y: u16, w: u16) -> String {
        (0..w)
            .map(|x| buf.get(Position::new(x, y)).unwrap().symbol)
            .collect()
    }

    #[test]
    fn the_header_is_always_drawn_with_a_marker() {
        let items = [TaskItem::text("a")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 3));
        Task::new("Searching", &items).render(buf.area(), &mut buf);
        let header = row(&buf, 0, 30);
        assert!(header.contains("Searching"), "{header:?}");
        assert!(header.contains('▸'), "collapsed marker: {header:?}");
    }

    #[test]
    fn collapsed_draws_no_items() {
        let items = [TaskItem::text("hidden item")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 4));
        Task::new("T", &items).render(buf.area(), &mut buf);
        assert_eq!(
            Task::new("T", &items).body_rect(Rect::new(0, 0, 30, 4)),
            None
        );
        let mut text = String::new();
        for y in 1..4 {
            text.push_str(&row(&buf, y, 30));
        }
        assert!(!text.contains("hidden"), "collapsed must hide items");
    }

    #[test]
    fn open_renders_a_railed_item_list() {
        let items = [TaskItem::text("Reading docs"), TaskItem::file("main.rs")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 4));
        Task::new("Search", &items)
            .open(true)
            .render(buf.area(), &mut buf);
        let header = row(&buf, 0, 30);
        assert!(header.contains('▾'), "open marker: {header:?}");
        // Row 1: rail + first item; row 2: rail + file chip.
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '│');
        assert!(row(&buf, 1, 30).contains("Reading docs"));
        assert!(row(&buf, 2, 30).contains("[main.rs]"));
    }

    #[test]
    fn a_file_item_is_an_accented_chip() {
        let items = [TaskItem::file("lib.rs")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
        Task::new("T", &items)
            .open(true)
            .render(buf.area(), &mut buf);
        // The '[' of the chip is accented bold cyan.
        let row1 = row(&buf, 1, 20);
        assert!(row1.contains("[lib.rs]"), "{row1:?}");
        // Find the '[' cell and check its style.
        for x in 0..20 {
            let cell = buf.get(Position::new(x, 1)).unwrap();
            if cell.symbol == '[' {
                assert_eq!(cell.fg, Color::Cyan);
                assert!(cell.modifier.contains(Modifier::BOLD));
            }
        }
    }

    #[test]
    fn body_rect_is_below_the_header_when_open() {
        let items = [TaskItem::text("x")];
        let area = Rect::new(0, 0, 20, 5);
        let t = Task::new("T", &items).open(true);
        let h = t.header_rect(area).unwrap();
        let b = t.body_rect(area).unwrap();
        assert_eq!(h.height, 1);
        assert_eq!(b.top(), h.bottom());
        assert_eq!(b.height, 4);
    }

    #[test]
    fn more_items_than_rows_clip_without_a_panic() {
        let items: Vec<_> = (0..50)
            .map(|i| TaskItem::text(format!("item{i}")))
            .collect();
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 4));
        Task::new("Big", &items)
            .open(true)
            .render(buf.area(), &mut buf);
        // Only the first 3 items fit below the 1-row header.
        assert!(row(&buf, 1, 12).contains("item0"));
        assert!(row(&buf, 3, 12).contains("item2"));
    }

    #[test]
    fn no_items_open_is_total() {
        let items: [TaskItem; 0] = [];
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
        Task::new("Empty", &items)
            .open(true)
            .render(buf.area(), &mut buf);
        assert!(row(&buf, 0, 20).contains("Empty"));
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let items = [TaskItem::text("x")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
        Task::new("T", &items)
            .open(true)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
        assert_eq!(
            Task::new("T", &items).header_rect(Rect::new(0, 0, 0, 0)),
            None
        );
    }

    #[test]
    fn task_item_constructors_build_the_right_variant() {
        assert_eq!(TaskItem::text("a"), TaskItem::Text("a".into()));
        assert_eq!(TaskItem::file("b"), TaskItem::File("b".into()));
    }
}
