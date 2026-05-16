//! The composable rendering abstraction: a [`Widget`] draws itself into a
//! [`Rect`] of a [`Buffer`].
//!
//! Up to now a view wrote raw strings straight into the buffer. A `Widget` is
//! the seam that turns "describe the screen" into reusable pieces: each widget
//! is a cheap value constructed in the render pass, handed an area (usually
//! carved out by [`Layout`](crate::Layout)), and asked to paint it. Widgets
//! compose — a widget renders sub-widgets into sub-rects — which is the
//! property the whole component set will be built on.
//!
//! `render` takes `self` by value: widgets are commands built fresh each frame
//! from borrowed app state, not retained objects, so consuming them is the
//! ergonomic and allocation-free choice for the `view` pattern. The bound is
//! still spelled `where Self: Sized` so `dyn Widget` stays a legal type for a
//! future heterogeneous widget list, even though rendering itself is always
//! monomorphized.
//!
//! It also ships the trait, trivial blanket impls (`&str`, `String`,
//! `Option<W>`), and the foundational container every TUI is built around —
//! [`Block`]: borders, a styled fill, padding, and a clipped title that is a
//! full [`Line`] (so it carries per-span styles and its own alignment), plus
//! the all-important [`Block::inner`] that hands the remaining area back to the
//! content drawn inside it.
//!
//! On top of the text model it ships [`Paragraph`]: the multi-line text widget
//! that adds the things real content panes need — soft word [`Wrap`], a
//! vertical/horizontal scroll offset, per-block alignment, and an optional
//! framing [`Block`] — without any of that leaking into the
//! [`Text`]/[`Line`]/[`Span`](crate::Span) primitives, which still render
//! exactly as written.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Block, Borders, Buffer, Position, Rect, Widget};
//!
//! let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
//! Block::bordered().title("Hi").render(buf.area(), &mut buf);
//!
//! // Corners and the clipped title are painted as box-drawing glyphs.
//! assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
//! assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'H');
//! assert_eq!(buf.get(Position::new(5, 2)).unwrap().symbol, '┘');
//!
//! // Content goes in the area borders leave behind.
//! assert_eq!(Block::bordered().inner(buf.area()), Rect::new(1, 1, 4, 1));
//! ```

use crate::buffer::Buffer;
use crate::geometry::{Position, Rect};
use crate::style::Style;
use crate::text::{Line, Text};

/// A value that can draw itself into a [`Rect`] region of a [`Buffer`].
///
/// Implement this for your own components; the runtime (via
/// [`Frame::render_widget`](crate::Frame::render_widget)) or another widget
/// supplies the area and buffer. `render` consumes `self` because widgets are
/// throwaway draw commands rebuilt every frame — see the [module
/// docs](self) for why that is the idiomatic choice here.
pub trait Widget {
    /// Draws this widget into `area` of `buf`.
    ///
    /// Implementations must stay within `area` and must tolerate an `area`
    /// smaller than they would like (including zero-sized) without panicking;
    /// the bounds-safe [`Buffer`] accessors make clipping the default.
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized;
}

/// Renders a string slice as a single clipped line at the area's origin.
///
/// Lets `frame.render_widget("hello", area)` work with no wrapper type. Text
/// is truncated at the right edge of `area`; wrapping and multi-line text are
/// a later (`Paragraph`) surface.
impl Widget for &str {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let mut x = area.left();
        for ch in self.chars() {
            if x >= area.right() {
                break;
            }
            set_cell(buf, x, area.top(), ch, Style::new());
            x = x.saturating_add(1);
        }
    }
}

/// Renders an owned `String` exactly like its [`&str`](Widget::render) slice.
impl Widget for String {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.as_str().render(area, buf);
    }
}

/// Renders the inner widget when present, and nothing when `None`.
///
/// Handy for optional UI (`frame.render_widget(error.map(...), area)`) without
/// a branch at the call site.
impl<W: Widget> Widget for Option<W> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if let Some(widget) = self {
            widget.render(area, buf);
        }
    }
}

/// Horizontal placement of content within an available span.
///
/// Currently used for a [`Block`] title; reused by future text widgets.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alignment {
    /// Flush with the start of the span.
    #[default]
    Left,
    /// Centered, with any odd remainder biased toward the start.
    Center,
    /// Flush with the end of the span.
    Right,
}

/// Which sides of a [`Block`] draw a border, as a small bitset.
///
/// Hand-rolled rather than pulling in `bitflags`, matching
/// [`Modifier`](crate::Modifier) — the core crate stays dependency-free.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Borders(u8);

impl Borders {
    /// No border on any side.
    pub const NONE: Self = Self(0);
    /// The top edge.
    pub const TOP: Self = Self(1 << 0);
    /// The right edge.
    pub const RIGHT: Self = Self(1 << 1);
    /// The bottom edge.
    pub const BOTTOM: Self = Self(1 << 2);
    /// The left edge.
    pub const LEFT: Self = Self(1 << 3);
    /// All four edges.
    pub const ALL: Self = Self(0b1111);

    /// Returns `true` if every side in `other` is also set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns `self` with the sides in `other` added.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns `true` if no side is set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for Borders {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for Borders {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

/// The six box-drawing glyphs that draw a border.
///
/// Every glyph is a single [`char`] (Unicode box-drawing is single-scalar), so
/// it maps 1:1 onto a [`Cell`](crate::Cell). Construct one with
/// [`BorderType::set`] or build a custom set for bespoke frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BorderSet {
    /// Top-left corner.
    pub top_left: char,
    /// Top-right corner.
    pub top_right: char,
    /// Bottom-left corner.
    pub bottom_left: char,
    /// Bottom-right corner.
    pub bottom_right: char,
    /// Vertical edge (left and right sides).
    pub vertical: char,
    /// Horizontal edge (top and bottom sides).
    pub horizontal: char,
}

/// The visual style of a [`Block`]'s border lines.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BorderType {
    /// Single line with square corners (`┌─┐│└┘`).
    #[default]
    Plain,
    /// Single line with rounded corners (`╭─╮│╰╯`).
    Rounded,
    /// Double line (`╔═╗║╚╝`).
    Double,
    /// Heavy line (`┏━┓┃┗┛`).
    Thick,
}

impl BorderType {
    /// The [`BorderSet`] of glyphs this border type draws with.
    #[must_use]
    pub const fn set(self) -> BorderSet {
        match self {
            Self::Plain => BorderSet {
                top_left: '┌',
                top_right: '┐',
                bottom_left: '└',
                bottom_right: '┘',
                vertical: '│',
                horizontal: '─',
            },
            Self::Rounded => BorderSet {
                top_left: '╭',
                top_right: '╮',
                bottom_left: '╰',
                bottom_right: '╯',
                vertical: '│',
                horizontal: '─',
            },
            Self::Double => BorderSet {
                top_left: '╔',
                top_right: '╗',
                bottom_left: '╚',
                bottom_right: '╝',
                vertical: '║',
                horizontal: '═',
            },
            Self::Thick => BorderSet {
                top_left: '┏',
                top_right: '┓',
                bottom_left: '┗',
                bottom_right: '┛',
                vertical: '┃',
                horizontal: '━',
            },
        }
    }
}

/// Empty cells reserved inside a [`Block`], between its border and its content.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Padding {
    /// Columns reserved on the left.
    pub left: u16,
    /// Columns reserved on the right.
    pub right: u16,
    /// Rows reserved on the top.
    pub top: u16,
    /// Rows reserved on the bottom.
    pub bottom: u16,
}

impl Padding {
    /// No padding on any side.
    pub const ZERO: Self = Self {
        left: 0,
        right: 0,
        top: 0,
        bottom: 0,
    };

    /// Padding with each side set explicitly.
    #[must_use]
    pub const fn new(left: u16, right: u16, top: u16, bottom: u16) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    /// The same padding on all four sides.
    #[must_use]
    pub const fn uniform(value: u16) -> Self {
        Self::new(value, value, value, value)
    }

    /// `horizontal` on the left/right and `vertical` on the top/bottom.
    #[must_use]
    pub const fn symmetric(horizontal: u16, vertical: u16) -> Self {
        Self::new(horizontal, horizontal, vertical, vertical)
    }
}

/// The foundational container widget: an optional border with a styled fill,
/// padding, and a clipped title.
///
/// A `Block` is the frame nearly every other widget renders inside. The usual
/// pattern is to render the block, then render content into the area it
/// reserves:
///
/// ```
/// use rstui_core::{Block, Buffer, Rect, Widget};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 5));
/// let block = Block::bordered().title("Logs");
/// let inner = block.inner(buf.area());
/// block.render(buf.area(), &mut buf);
/// "first log line".render(inner, &mut buf);
/// ```
#[derive(Debug, Default, Clone)]
pub struct Block<'a> {
    borders: Borders,
    border_type: BorderType,
    border_style: Style,
    style: Style,
    title: Option<Line<'a>>,
    title_alignment: Alignment,
    title_style: Style,
    padding: Padding,
}

impl<'a> Block<'a> {
    /// A block with no border, no title, and no fill — a transparent region.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A block bordered on all four sides ([`Borders::ALL`]).
    #[must_use]
    pub fn bordered() -> Self {
        Self::default().borders(Borders::ALL)
    }

    /// Sets which sides draw a border.
    #[must_use]
    pub fn borders(mut self, borders: Borders) -> Self {
        self.borders = borders;
        self
    }

    /// Sets the line style the border is drawn with.
    #[must_use]
    pub fn border_type(mut self, border_type: BorderType) -> Self {
        self.border_type = border_type;
        self
    }

    /// Sets the [`Style`] applied to the border glyphs.
    #[must_use]
    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    /// Sets the [`Style`] used to fill the whole block area (its background).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the title drawn on the top edge.
    ///
    /// The title is a full [`Line`], so `"plain"`, a styled [`Span`], or a
    /// `Vec<Span>` of differently-styled runs all work. A [`Line`] with its
    /// own [`alignment`](Line::alignment) overrides
    /// [`title_alignment`](Self::title_alignment); per-span styles cascade over
    /// [`title_style`](Self::title_style) — the same text→line→span model
    /// [`Text`](crate::Text) uses.
    ///
    /// [`Span`]: crate::Span
    #[must_use]
    pub fn title(mut self, title: impl Into<Line<'a>>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the default title alignment, used when the title [`Line`] does not
    /// set its own.
    #[must_use]
    pub fn title_alignment(mut self, alignment: Alignment) -> Self {
        self.title_alignment = alignment;
        self
    }

    /// Sets the base [`Style`] for the title, beneath each title span's own
    /// style in the cascade.
    #[must_use]
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// Sets the [`Padding`] reserved between the border and the content.
    #[must_use]
    pub fn padding(mut self, padding: Padding) -> Self {
        self.padding = padding;
        self
    }

    /// The area left for content after removing borders and padding.
    ///
    /// This is how a block composes with what it contains: render the block
    /// into `area`, then render content into `block.inner(area)`. The result
    /// is clamped (never larger than `area`, never negative) so a block in a
    /// tiny region degrades to an empty inner rect instead of panicking.
    #[must_use]
    pub fn inner(&self, area: Rect) -> Rect {
        let left = u16::from(self.borders.contains(Borders::LEFT)) + self.padding.left;
        let right = u16::from(self.borders.contains(Borders::RIGHT)) + self.padding.right;
        let top = u16::from(self.borders.contains(Borders::TOP)) + self.padding.top;
        let bottom = u16::from(self.borders.contains(Borders::BOTTOM)) + self.padding.bottom;
        Rect::new(
            area.x.saturating_add(left),
            area.y.saturating_add(top),
            area.width.saturating_sub(left.saturating_add(right)),
            area.height.saturating_sub(top.saturating_add(bottom)),
        )
    }
}

impl Widget for Block<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // Fill first so borders and the title layer on top of the background.
        buf.set_style(area, self.style);

        let borders = self.borders;
        let set = self.border_type.set();
        let bs = self.border_style;
        let (left, right) = (area.left(), area.right());
        let (top, bottom) = (area.top(), area.bottom());

        // Draw full-length edges, then stamp corners only where two edges
        // meet — a single present edge stays a straight rule with no corner.
        if borders.contains(Borders::TOP) {
            for x in left..right {
                set_cell(buf, x, top, set.horizontal, bs);
            }
        }
        if borders.contains(Borders::BOTTOM) {
            for x in left..right {
                set_cell(buf, x, bottom - 1, set.horizontal, bs);
            }
        }
        if borders.contains(Borders::LEFT) {
            for y in top..bottom {
                set_cell(buf, left, y, set.vertical, bs);
            }
        }
        if borders.contains(Borders::RIGHT) {
            for y in top..bottom {
                set_cell(buf, right - 1, y, set.vertical, bs);
            }
        }
        if borders.contains(Borders::TOP) && borders.contains(Borders::LEFT) {
            set_cell(buf, left, top, set.top_left, bs);
        }
        if borders.contains(Borders::TOP) && borders.contains(Borders::RIGHT) {
            set_cell(buf, right - 1, top, set.top_right, bs);
        }
        if borders.contains(Borders::BOTTOM) && borders.contains(Borders::LEFT) {
            set_cell(buf, left, bottom - 1, set.bottom_left, bs);
        }
        if borders.contains(Borders::BOTTOM) && borders.contains(Borders::RIGHT) {
            set_cell(buf, right - 1, bottom - 1, set.bottom_right, bs);
        }

        if let Some(title) = self.title {
            // The title lives on the top row, inside whatever vertical
            // borders are present so it never overwrites a corner.
            let start = left + u16::from(borders.contains(Borders::LEFT));
            let end = right - u16::from(borders.contains(Borders::RIGHT));
            if end > start {
                let avail = end - start;
                let len = (title.width() as u16).min(avail);
                // A Line's own alignment wins; otherwise the block default.
                let alignment = title.alignment.unwrap_or(self.title_alignment);
                let x0 = match alignment {
                    Alignment::Left => start,
                    Alignment::Right => end - len,
                    Alignment::Center => start + (avail - len) / 2,
                };
                // Cascade: block title style → line style → span style, the
                // same text→line→span model `Text` uses. Only the title's own
                // glyph cells are stamped, so the border still shows through
                // around a short title.
                let base = self.title_style.patch(title.style);
                let mut x = x0;
                'title: for span in title.spans {
                    let style = base.patch(span.style);
                    for ch in span.content.chars() {
                        if x >= end {
                            break 'title;
                        }
                        set_cell(buf, x, top, ch, style);
                        x = x.saturating_add(1);
                    }
                }
            }
        }
    }
}

/// How [`Paragraph`] reflows lines too wide for its area.
///
/// `trim` controls leading whitespace on every wrapped row: `true` reflows
/// `"  - a long bullet …"` flush-left on each row, `false` keeps the original
/// indentation so continuations line up under the text rather than the bullet.
/// Trailing whitespace at a wrap point is always dropped so alignment stays
/// exact regardless of `trim`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Wrap {
    /// Strip leading whitespace from every wrapped row.
    pub trim: bool,
}

/// A multi-line text widget with optional word [`Wrap`], scroll, alignment,
/// and a framing [`Block`].
///
/// [`Text`]/[`Line`]/[`Span`](crate::Span) render exactly as written and clip
/// at the right edge — that is the whole text model. `Paragraph` is the widget
/// that adds what real content panes need on top of it: soft word wrapping to
/// the available width, a vertical/horizontal scroll offset, per-block
/// alignment, and an optional surrounding [`Block`]. It is the basis for logs,
/// help text, descriptions, and any scrollable read-only copy.
///
/// Styling cascades paragraph → text → line → span (the same
/// [`Style::patch`](crate::Style) model [`Text`] uses); the paragraph style
/// also fills the content area so a background covers the whole region.
///
/// Without [`wrap`](Self::wrap) each [`Line`] is one row (offset by the
/// horizontal scroll, clipped at the right edge). With it, lines too wide for
/// the area reflow at word boundaries and a single word wider than the area is
/// hard split across rows. A blank source line always occupies a row so
/// vertical spacing is preserved.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Paragraph, Position, Rect, Widget, Wrap};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
/// Paragraph::new("the quick brown")
///     .wrap(Wrap { trim: true })
///     .render(buf.area(), &mut buf);
///
/// // Soft-wrapped at word boundaries to the 6-cell width.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 't'); // "the"
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'q'); // "quick"
/// assert_eq!(buf.get(Position::new(0, 2)).unwrap().symbol, 'b'); // "brown"
/// ```
#[derive(Debug, Default, Clone)]
pub struct Paragraph<'a> {
    text: Text<'a>,
    block: Option<Block<'a>>,
    style: Style,
    wrap: Option<Wrap>,
    scroll: Position,
    alignment: Option<Alignment>,
}

impl<'a> Paragraph<'a> {
    /// A left-aligned, unwrapped paragraph of `text` with no block.
    pub fn new(text: impl Into<Text<'a>>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// Frames the paragraph in `block`; content renders into
    /// [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`], beneath the text→line→span cascade. It also
    /// fills the content area so a background covers the whole region.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Enables soft word wrapping with the given [`Wrap`] options.
    #[must_use]
    pub fn wrap(mut self, wrap: Wrap) -> Self {
        self.wrap = Some(wrap);
        self
    }

    /// Sets the scroll offset into the composed text: `x` columns from the
    /// left (only meaningful without [`wrap`](Self::wrap)) and `y` rows from
    /// the top. Accepts a [`Position`] or an `(x, y)` tuple.
    #[must_use]
    pub fn scroll(mut self, offset: impl Into<Position>) -> Self {
        self.scroll = offset.into();
        self
    }

    /// Sets the default alignment for lines that do not set their own.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Left-aligns lines without their own alignment.
    #[must_use]
    pub fn left_aligned(self) -> Self {
        self.alignment(Alignment::Left)
    }

    /// Centers lines without their own alignment.
    #[must_use]
    pub fn centered(self) -> Self {
        self.alignment(Alignment::Center)
    }

    /// Right-aligns lines without their own alignment.
    #[must_use]
    pub fn right_aligned(self) -> Self {
        self.alignment(Alignment::Right)
    }
}

/// One composed output row: its resolved glyph cells and alignment.
struct ParaRow {
    cells: Vec<(char, Style)>,
    align: Alignment,
}

/// Trims trailing whitespace off `cur`, then pushes it as a finished row.
///
/// Trailing-whitespace trimming keeps the row's reported width equal to its
/// visible width so right/center alignment positions exactly.
fn flush_row(cur: &mut Vec<(char, Style)>, align: Alignment, out: &mut Vec<ParaRow>) {
    while matches!(cur.last(), Some((c, _)) if c.is_whitespace()) {
        cur.pop();
    }
    out.push(ParaRow {
        cells: std::mem::take(cur),
        align,
    });
}

/// Soft-wraps one source line's `cells` to `width`, appending the rows.
///
/// Word boundaries are maximal runs of (non-)whitespace `char`s. A row is
/// flushed before a word that would overflow; `trim` additionally drops
/// leading whitespace at the start of every row; a single word wider than
/// `width` is hard split across rows. Empty input still yields one row so a
/// blank source line occupies a row.
fn wrap_cells(
    cells: &[(char, Style)],
    width: usize,
    trim: bool,
    align: Alignment,
    out: &mut Vec<ParaRow>,
) {
    if width == 0 {
        out.push(ParaRow {
            cells: Vec::new(),
            align,
        });
        return;
    }
    let mut cur: Vec<(char, Style)> = Vec::new();
    let n = cells.len();
    let mut i = 0;
    while i < n {
        let ws = cells[i].0.is_whitespace();
        let mut j = i;
        while j < n && cells[j].0.is_whitespace() == ws {
            j += 1;
        }
        let token = &cells[i..j];
        i = j;
        if ws {
            if trim && cur.is_empty() {
                // Drop leading whitespace at the start of a row.
            } else if cur.len() + token.len() <= width {
                cur.extend_from_slice(token);
            } else {
                // Whitespace would overflow: end the row, drop the spaces.
                flush_row(&mut cur, align, out);
            }
        } else if token.len() <= width {
            if cur.len() + token.len() > width {
                flush_row(&mut cur, align, out);
            }
            cur.extend_from_slice(token);
        } else {
            // A single word wider than the whole row: hard split it.
            let mut k = 0;
            while k < token.len() {
                if cur.len() == width {
                    flush_row(&mut cur, align, out);
                }
                let take = (width - cur.len()).min(token.len() - k);
                cur.extend_from_slice(&token[k..k + take]);
                k += take;
            }
        }
    }
    flush_row(&mut cur, align, out);
}

impl Widget for Paragraph<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let Paragraph {
            text,
            block,
            style,
            wrap,
            scroll,
            alignment,
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

        // Paragraph base fills the content area so a background covers the
        // whole region; glyphs then layer the text→line→span cascade on top.
        buf.set_style(inner, style);

        let width = inner.width as usize;
        let text_base = style.patch(text.style);
        let para_align = text.alignment.or(alignment);

        // Compose every source line into output rows: one per source line, or
        // several when wrapping. Each row carries its source line's resolved
        // alignment so a wrapped continuation stays aligned with it.
        let mut rows: Vec<ParaRow> = Vec::new();
        for line in text.lines {
            let align = line.alignment.or(para_align).unwrap_or_default();
            let line_base = text_base.patch(line.style);
            let mut cells: Vec<(char, Style)> = Vec::new();
            for span in &line.spans {
                let span_style = line_base.patch(span.style);
                cells.extend(span.content.chars().map(|ch| (ch, span_style)));
            }
            match wrap {
                Some(w) => wrap_cells(&cells, width, w.trim, align, &mut rows),
                None => rows.push(ParaRow { cells, align }),
            }
        }

        // Vertical scroll, then paint the visible window of rows.
        let top = inner.top();
        let right = inner.right();
        for (i, row) in rows
            .into_iter()
            .skip(scroll.y as usize)
            .take(inner.height as usize)
            .enumerate()
        {
            // Horizontal scroll only applies without wrapping — wrapped text
            // has no off-screen horizontal extent to scroll into.
            let visible: &[(char, Style)] = if wrap.is_none() {
                let off = (scroll.x as usize).min(row.cells.len());
                &row.cells[off..]
            } else {
                &row.cells
            };
            let drawn = visible.len().min(width) as u16;
            let start = match row.align {
                Alignment::Left => inner.left(),
                Alignment::Right => right.saturating_sub(drawn),
                Alignment::Center => inner
                    .left()
                    .saturating_add((inner.width.saturating_sub(drawn)) / 2),
            };
            let y = top.saturating_add(i as u16);
            let mut x = start;
            for &(ch, st) in visible {
                if x >= right {
                    break;
                }
                set_cell(buf, x, y, ch, st);
                x = x.saturating_add(1);
            }
        }
    }
}

/// Writes one glyph and patches its style, ignoring out-of-bounds positions.
///
/// Shared with [`text`](crate::text) so widgets and text primitives stamp
/// cells through one bounds-safe path.
pub(crate) fn set_cell(buf: &mut Buffer, x: u16, y: u16, symbol: char, style: Style) {
    if let Some(cell) = buf.get_mut(Position::new(x, y)) {
        cell.symbol = symbol;
        cell.apply_style(style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Color, Modifier};
    use crate::text::Span;

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row — legible for asserting
    /// border art directly.
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
    fn str_widget_clips_to_the_area_not_the_buffer() {
        // Render into a sub-area narrower than the buffer: text must stop at
        // the area's right edge, leaving the rest of the row blank.
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        "hello world".render(Rect::new(2, 0, 4, 1), &mut buf);
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'h');
        assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, 'l');
        assert_eq!(buf.get(Position::new(6, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn option_widget_renders_only_when_some() {
        assert_eq!(lines(None::<&str>, 3, 1), "   \n");
        assert_eq!(lines(Some("ab"), 3, 1), "ab \n");
    }

    #[test]
    fn bordered_block_draws_a_box() {
        assert_eq!(
            lines(Block::bordered(), 4, 3),
            "┌──┐\n\
             │  │\n\
             └──┘\n"
        );
    }

    #[test]
    fn border_type_selects_the_glyph_set() {
        assert_eq!(
            lines(Block::bordered().border_type(BorderType::Rounded), 3, 2),
            "╭─╮\n╰─╯\n"
        );
        assert_eq!(
            lines(Block::bordered().border_type(BorderType::Double), 3, 2),
            "╔═╗\n╚═╝\n"
        );
        assert_eq!(
            lines(Block::bordered().border_type(BorderType::Thick), 3, 2),
            "┏━┓\n┗━┛\n"
        );
    }

    #[test]
    fn a_single_edge_is_a_rule_with_no_corners() {
        assert_eq!(
            lines(Block::new().borders(Borders::TOP), 4, 2),
            "────\n    \n"
        );
        // Two adjacent edges meet at exactly one corner.
        assert_eq!(
            lines(Block::new().borders(Borders::TOP | Borders::LEFT), 3, 2),
            "┌──\n│  \n"
        );
    }

    #[test]
    fn title_is_aligned_and_clipped_inside_the_borders() {
        assert_eq!(
            lines(Block::bordered().title("Hi"), 6, 2),
            "┌Hi──┐\n└────┘\n"
        );
        assert_eq!(
            lines(
                Block::bordered()
                    .title("Hi")
                    .title_alignment(Alignment::Right),
                6,
                2
            ),
            "┌──Hi┐\n└────┘\n"
        );
        assert_eq!(
            lines(
                Block::bordered()
                    .title("Hi")
                    .title_alignment(Alignment::Center),
                7,
                2
            ),
            "┌─Hi──┐\n└─────┘\n"
        );
        // Overlong titles are truncated to the span between the borders.
        assert_eq!(
            lines(Block::bordered().title("overlong"), 5, 2),
            "┌ove┐\n└───┘\n"
        );
    }

    #[test]
    fn title_spans_keep_their_own_style_over_the_block_title_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 7, 2));
        Block::bordered()
            .title(Line::from(vec![
                Span::styled("E", Style::new().fg(Color::Red)),
                Span::raw("rr"),
            ]))
            .title_style(Style::new().add_modifier(Modifier::BOLD))
            .render(buf.area(), &mut buf);

        // ALL borders → the title starts just inside the left edge (x = 1).
        let e = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(e.symbol, 'E');
        assert_eq!(e.fg, Color::Red); // the span's own fg wins
        assert!(e.modifier.contains(Modifier::BOLD)); // block title_style base cascades

        let r = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(r.symbol, 'r');
        assert_eq!(r.fg, Color::Reset); // raw span sets no fg → not red
        assert!(r.modifier.contains(Modifier::BOLD)); // still inherits the base
    }

    #[test]
    fn title_line_alignment_overrides_the_block_default() {
        // The block defaults titles to the left, but this Line asks for the
        // right — the Line wins, mirroring the Text→Line alignment cascade.
        assert_eq!(
            lines(
                Block::bordered()
                    .title(Line::raw("Hi").right_aligned())
                    .title_alignment(Alignment::Left),
                6,
                2
            ),
            "┌──Hi┐\n└────┘\n"
        );
    }

    #[test]
    fn border_around_a_short_title_keeps_the_border_style_not_the_title_style() {
        // Regression: only the title's own cells are stamped. The horizontal
        // border cells beside a short title must keep the border style, not
        // pick up the title style (i.e. no whole-row fill).
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 2));
        Block::bordered()
            .border_style(Style::new().fg(Color::Cyan))
            .title(Span::styled("Hi", Style::new().fg(Color::Red)))
            .render(buf.area(), &mut buf);

        // x=1,2 are the title; x=3..6 are the top border between title and
        // the top-right corner.
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().fg, Color::Red);
        let border = buf.get(Position::new(4, 0)).unwrap();
        assert_eq!(border.symbol, '─');
        assert_eq!(border.fg, Color::Cyan); // border style untouched by the title
    }

    #[test]
    fn inner_subtracts_borders_and_padding_and_saturates() {
        let block = Block::bordered().padding(Padding::uniform(1));
        assert_eq!(block.inner(Rect::new(0, 0, 10, 6)), Rect::new(2, 2, 6, 2));

        // Asymmetric: only left+top borders, extra right padding.
        let block = Block::new()
            .borders(Borders::LEFT | Borders::TOP)
            .padding(Padding::new(0, 3, 0, 0));
        assert_eq!(block.inner(Rect::new(4, 5, 20, 10)), Rect::new(5, 6, 16, 9));

        // Too small to fit the frame: inner collapses instead of underflowing.
        assert!(Block::bordered().inner(Rect::new(0, 0, 1, 1)).is_empty());
    }

    #[test]
    fn style_fills_the_area_and_border_style_overrides_the_edges() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 3));
        Block::bordered()
            .style(Style::new().bg(Color::Blue))
            .border_style(Style::new().fg(Color::Red).add_modifier(Modifier::BOLD))
            .render(buf.area(), &mut buf);

        // The interior cell keeps the fill background.
        let inner = buf.get(Position::new(1, 1)).unwrap();
        assert_eq!(inner.bg, Color::Blue);
        assert_eq!(inner.symbol, ' ');

        // A border cell keeps the fill background but takes the border fg.
        let corner = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(corner.symbol, '┌');
        assert_eq!(corner.bg, Color::Blue);
        assert_eq!(corner.fg, Color::Red);
        assert!(corner.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn zero_sized_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Block::bordered()
            .title("x")
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn paragraph_without_wrap_is_one_clipped_row_per_source_line() {
        // No wrap: each `\n`-split line is exactly one row, clipped at the
        // right edge — the same rule the bare text model already follows.
        assert_eq!(
            lines(Paragraph::new("abcdef\nXY"), 4, 3),
            "abcd\nXY  \n    \n"
        );
    }

    #[test]
    fn wrap_breaks_at_word_boundaries() {
        assert_eq!(
            lines(
                Paragraph::new("the quick brown").wrap(Wrap { trim: true }),
                6,
                3
            ),
            "the   \nquick \nbrown \n"
        );
    }

    #[test]
    fn wrap_trim_controls_leading_whitespace_only() {
        // trim:false keeps the original indentation on the first row…
        assert_eq!(
            lines(Paragraph::new("  ab cd").wrap(Wrap { trim: false }), 4, 2),
            "  ab\ncd  \n"
        );
        // …trim:true strips it. Both wrap "cd" identically (no leading space
        // on a continuation row either way).
        assert_eq!(
            lines(Paragraph::new("  ab cd").wrap(Wrap { trim: true }), 4, 2),
            "ab  \ncd  \n"
        );
    }

    #[test]
    fn wrap_hard_splits_a_word_wider_than_the_area() {
        assert_eq!(
            lines(Paragraph::new("abcdefg").wrap(Wrap { trim: false }), 3, 3),
            "abc\ndef\ng  \n"
        );
    }

    #[test]
    fn the_space_at_a_wrap_point_is_dropped() {
        // "aaa bbb" at width 3 yields two full rows, not "aaa " (width 4) or a
        // leading-space "bbb" — trailing-whitespace trim keeps widths exact.
        assert_eq!(
            lines(Paragraph::new("aaa bbb").wrap(Wrap { trim: false }), 3, 2),
            "aaa\nbbb\n"
        );
    }

    #[test]
    fn a_blank_source_line_still_occupies_a_row() {
        assert_eq!(lines(Paragraph::new("a\n\nb"), 3, 3), "a  \n   \nb  \n");
    }

    #[test]
    fn vertical_scroll_skips_composed_rows() {
        let p = Paragraph::new("l0\nl1\nl2\nl3").scroll((0, 2));
        assert_eq!(lines(p, 2, 2), "l2\nl3\n");
    }

    #[test]
    fn horizontal_scroll_skips_columns_without_wrap() {
        // x scroll drops leading columns of each row (no-wrap only).
        assert_eq!(
            lines(Paragraph::new("abcdef").scroll((2, 0)), 3, 1),
            "cde\n"
        );
    }

    #[test]
    fn block_frames_content_in_the_inner_area() {
        assert_eq!(
            lines(Paragraph::new("hi").block(Block::bordered()), 4, 3),
            "┌──┐\n│hi│\n└──┘\n"
        );
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_content() {
        // inner() collapses to empty; the block still renders, content does
        // not (and nothing panics).
        assert_eq!(
            lines(Paragraph::new("Z").block(Block::bordered()), 2, 2),
            "┌┐\n└┘\n"
        );
    }

    #[test]
    fn alignment_positions_each_row() {
        assert_eq!(lines(Paragraph::new("hi").right_aligned(), 5, 1), "   hi\n");
        assert_eq!(lines(Paragraph::new("hi").centered(), 5, 1), " hi  \n");
    }

    #[test]
    fn style_cascades_paragraph_text_line_span_and_fills_the_area() {
        let p = Paragraph::new(
            Text::from(vec![
                Line::from(vec![
                    Span::styled("X", Style::new().fg(Color::Red)),
                    Span::raw("y"),
                ])
                .style(Style::new().add_modifier(Modifier::BOLD)),
            ])
            .style(Style::new().fg(Color::Green)),
        )
        .style(Style::new().bg(Color::Blue));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        p.render(buf.area(), &mut buf);

        // Span fg wins; line BOLD + paragraph bg cascade through.
        let x = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(x.symbol, 'X');
        assert_eq!(x.fg, Color::Red);
        assert_eq!(x.bg, Color::Blue);
        assert!(x.modifier.contains(Modifier::BOLD));

        // The raw span inherits the text-level green (no span fg of its own).
        let y = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(y.symbol, 'y');
        assert_eq!(y.fg, Color::Green);
        assert!(y.modifier.contains(Modifier::BOLD));

        // The paragraph style also fills the empty cell past the text.
        let pad = buf.get(Position::new(2, 0)).unwrap();
        assert_eq!(pad.symbol, ' ');
        assert_eq!(pad.bg, Color::Blue);
        assert_eq!(pad.fg, Color::Reset);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Paragraph::new("hello")
            .wrap(Wrap { trim: true })
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
