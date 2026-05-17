//! [`LogStream`] — a structured, severity-coloured log viewer, the
//! observability "logs" pane (an OpenTelemetry log-records view): one
//! [`LogRecord`] per row, a fixed timestamp gutter, a bold colour-coded level
//! tag, an optional target gutter, then the message.
//!
//! # A pure projection, like every other widget
//!
//! `LogStream` owns no state. It borrows a caller-owned `&[LogRecord]` plus a
//! caller-owned scroll [`offset`](LogStream::offset) and config; the reducer
//! decides *what* the records are and *where* the window sits (newest-first or
//! oldest-first is the caller's convention) and the widget only projects
//! `records[offset..]` top→down clipped to the area height. That keeps it
//! deterministically headless-testable and composes with the Elm `view(&self)`
//! model exactly like [`List`](crate::List) and
//! [`BarChart`](crate::BarChart) — the scroll offset is ordinary application
//! state the widget reads but never writes.
//!
//! # A fixed-column projection, reusing the [`List`](crate::List) idea
//!
//! [`List`](crate::List) shows the window of items `[offset, offset + height)`,
//! one row per item. `LogStream` is the same window over caller-owned scroll
//! with a fixed multi-column layout per row: a [`timestamp_width`](LogStream::timestamp_width)
//! gutter, the 5-wide [`LogLevel`] tag (drawn bold in the
//! [`LogPalette`] colour), a [`target_width`](LogStream::target_width) gutter,
//! then the message clipped to whatever width remains. Hidden or `None`
//! columns are blank-padded so every row's columns stay aligned, exactly like
//! [`List`](crate::List)'s reserved highlight gutter.
//!
//! # Total, never a panic
//!
//! Per the [`BarChart`](crate::BarChart) rule a pure projection is *total*: an
//! empty area, no records, an offset past the end (shows nothing), a `None`
//! optional column, and an area too narrow for the gutters (the optional
//! columns drop — timestamp first, then target) are all safe clips/no-ops —
//! never a panic. An optional framing [`Block`] follows the
//! container-widget convention; live filtering, wrapped multi-line records, and
//! a structured key/value attribute table are deliberately deferred additive
//! follow-ups, not smuggled into this slice.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Position, Rect, Widget};
//! use rstui_widgets::{LogLevel, LogRecord, LogStream};
//!
//! let records = [
//!     LogRecord::new(LogLevel::Info, "started").timestamp("12:00:00"),
//!     LogRecord::new(LogLevel::Error, "boom").timestamp("12:00:01"),
//! ];
//! let mut buf = Buffer::empty(Rect::new(0, 0, 40, 2));
//! LogStream::new(&records).offset(0).render(buf.area(), &mut buf);
//!
//! // `[timestamp ][LEVEL ][target ] message`, columns one space apart.
//! assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '1'); // timestamp
//! assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, '1');
//! ```

use rstui_core::{Buffer, Color, Line, Position, Rect, Style, Widget};

use crate::block::Block;

/// The severity of a single [`LogRecord`], lowest (`Trace`) to highest
/// (`Error`); the [`Ord`] order is that severity order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    /// The most verbose level: fine-grained tracing spans and step-by-step
    /// diagnostics, normally filtered out in production.
    Trace,
    /// Developer-facing debugging detail, more selective than [`Trace`](Self::Trace).
    Debug,
    /// A normal, expected operational event (the default).
    #[default]
    Info,
    /// A recoverable problem or a degraded condition worth attention.
    Warn,
    /// A failure: an operation did not complete or invariants were violated.
    Error,
}

impl LogLevel {
    /// The fixed-width (5-column) uppercase tag for this level: `"TRACE"`,
    /// `"DEBUG"`, `"INFO "`, `"WARN "`, `"ERROR"` (the shorter names are
    /// space-padded so the tag column never shifts).
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO ",
            Self::Warn => "WARN ",
            Self::Error => "ERROR",
        }
    }
}

/// The per-level colours a [`LogStream`] draws the [`LogLevel`] tag in.
///
/// [`LogPalette::default`] is sensible ANSI defaults (`trace` dark gray,
/// `debug` blue, `info` green, `warn` yellow, `error` red); override any field
/// for a theme.
#[derive(Debug, Clone, Copy)]
pub struct LogPalette {
    /// The colour of the [`LogLevel::Trace`] tag.
    pub trace: Color,
    /// The colour of the [`LogLevel::Debug`] tag.
    pub debug: Color,
    /// The colour of the [`LogLevel::Info`] tag.
    pub info: Color,
    /// The colour of the [`LogLevel::Warn`] tag.
    pub warn: Color,
    /// The colour of the [`LogLevel::Error`] tag.
    pub error: Color,
}

impl Default for LogPalette {
    fn default() -> Self {
        Self {
            trace: Color::DarkGray,
            debug: Color::Blue,
            info: Color::Green,
            warn: Color::Yellow,
            error: Color::Red,
        }
    }
}

impl LogPalette {
    /// The colour for `level`'s tag in this palette.
    #[must_use]
    pub fn color(self, level: LogLevel) -> Color {
        match level {
            LogLevel::Trace => self.trace,
            LogLevel::Debug => self.debug,
            LogLevel::Info => self.info,
            LogLevel::Warn => self.warn,
            LogLevel::Error => self.error,
        }
    }
}

/// One row of a [`LogStream`]: a [`LogLevel`], an optional timestamp and
/// target gutter [`Line`], and the message [`Line`].
///
/// Build the timestamp/target/message from anything a [`Line`] is built from
/// (`&str`, `String`, [`Span`](rstui_core::Span), [`Line`], `Vec<Span>`); style
/// each through the [`Line`] it wraps (the per-record style cascades beneath
/// the [`LogStream`]'s column styles, the same
/// [`Style::patch`](rstui_core::Style) model the text model uses).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LogRecord<'a> {
    level: LogLevel,
    timestamp: Option<Line<'a>>,
    target: Option<Line<'a>>,
    message: Line<'a>,
}

impl<'a> LogRecord<'a> {
    /// A record at `level` whose body is `message` (anything convertible to a
    /// [`Line`]), with no timestamp or target.
    pub fn new(level: LogLevel, message: impl Into<Line<'a>>) -> Self {
        Self {
            level,
            timestamp: None,
            target: None,
            message: message.into(),
        }
    }

    /// Sets the timestamp gutter [`Line`] (anything convertible to a [`Line`]).
    #[must_use]
    pub fn timestamp(mut self, timestamp: impl Into<Line<'a>>) -> Self {
        self.timestamp = Some(timestamp.into());
        self
    }

    /// Sets the target/source gutter [`Line`] — typically a logger name or
    /// module path (anything convertible to a [`Line`]).
    #[must_use]
    pub fn target(mut self, target: impl Into<Line<'a>>) -> Self {
        self.target = Some(target.into());
        self
    }
}

/// A structured, severity-coloured log viewer with an optional framing
/// [`Block`].
///
/// `LogStream` shows the window of records `[offset, offset + height)` — one
/// row per record — laid out left→right as `[timestamp ][LEVEL][target ]
/// message`, one blank space between columns. The
/// [`offset`](Self::offset) is caller-supplied scroll state, clamped to the
/// record count and never mutated here (see the [module docs](self) for why);
/// an offset past the end shows nothing.
///
/// Each row: the timestamp gutter in [`timestamp_style`](Self::timestamp_style),
/// then the 5-wide [`LogLevel`] tag **bold** in
/// [`palette`](Self::palette)`.color(level)`, then the target gutter in
/// [`target_style`](Self::target_style), then the message in
/// [`message_style`](Self::message_style) clipped to the remaining width.
/// Hidden columns ([`show_timestamp`](Self::show_timestamp) /
/// [`show_target`](Self::show_target) `false`) and `None` fields render as
/// blank padding so the columns stay aligned. If the area is too narrow the
/// optional columns drop gracefully (timestamp first, then target). The base
/// [`Style`] also fills the content area so a background covers the whole pane.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{LogLevel, LogRecord, LogStream};
///
/// let records = [LogRecord::new(LogLevel::Warn, "disk 90%")];
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
/// LogStream::new(&records)
///     .show_timestamp(false)
///     .show_target(false)
///     .render(buf.area(), &mut buf);
///
/// // No gutters: the bold "WARN " tag starts at column 0, message follows.
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'W');
/// assert_eq!(buf.get(Position::new(6, 0)).unwrap().symbol, 'd');
/// ```
#[derive(Debug, Clone)]
pub struct LogStream<'a> {
    records: &'a [LogRecord<'a>],
    offset: usize,
    palette: LogPalette,
    show_timestamp: bool,
    show_target: bool,
    timestamp_width: u16,
    target_width: u16,
    block: Option<Block<'a>>,
    style: Style,
    timestamp_style: Style,
    target_style: Style,
    message_style: Style,
}

impl Default for LogStream<'_> {
    fn default() -> Self {
        Self {
            records: &[],
            offset: 0,
            palette: LogPalette::default(),
            show_timestamp: true,
            show_target: true,
            // A 12-wide timestamp fits `HH:MM:SS.mmm`; a 16-wide target fits a
            // short module path. Both are fixed gutters (Table's reasoning):
            // columns never shift as records scroll.
            timestamp_width: 12,
            target_width: 16,
            block: None,
            style: Style::default(),
            timestamp_style: Style::default(),
            target_style: Style::default(),
            message_style: Style::default(),
        }
    }
}

impl<'a> LogStream<'a> {
    /// A log stream projecting `records`, scrolled to the top, with default
    /// gutters, the default [`LogPalette`], and no frame.
    #[must_use]
    pub fn new(records: &'a [LogRecord<'a>]) -> Self {
        Self {
            records,
            ..Self::default()
        }
    }

    /// Sets the index of the first record to draw (the scroll offset).
    ///
    /// Clamped to the record count: an offset past the end simply shows
    /// nothing — the caller owns scrolling (see the [module docs](self)).
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Sets the per-level [`LogPalette`] the [`LogLevel`] tag is coloured with.
    #[must_use]
    pub fn palette(mut self, palette: LogPalette) -> Self {
        self.palette = palette;
        self
    }

    /// Sets whether the timestamp gutter is drawn (default `true`).
    ///
    /// When `false` the timestamp column is omitted entirely (no reserved
    /// padding); when `true` a record with no timestamp renders blank padding
    /// so columns stay aligned.
    #[must_use]
    pub fn show_timestamp(mut self, show_timestamp: bool) -> Self {
        self.show_timestamp = show_timestamp;
        self
    }

    /// Sets whether the target gutter is drawn (default `true`).
    ///
    /// When `false` the target column is omitted entirely (no reserved
    /// padding); when `true` a record with no target renders blank padding so
    /// columns stay aligned.
    #[must_use]
    pub fn show_target(mut self, show_target: bool) -> Self {
        self.show_target = show_target;
        self
    }

    /// Sets the fixed width of the timestamp gutter (default `12`); content is
    /// clipped/elided to it so the level tag never shifts.
    #[must_use]
    pub fn timestamp_width(mut self, timestamp_width: u16) -> Self {
        self.timestamp_width = timestamp_width;
        self
    }

    /// Sets the fixed width of the target gutter (default `16`); content is
    /// clipped/elided to it so the message never shifts.
    #[must_use]
    pub fn target_width(mut self, target_width: u16) -> Self {
        self.target_width = target_width;
        self
    }

    /// Frames the stream in `block`; rows render into [`block.inner`](Block::inner).
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`], beneath every column's style. It also fills
    /// the content area so a background covers the whole pane.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] for the timestamp gutter, over the base and beneath
    /// the timestamp [`Line`]/[`Span`](rstui_core::Span) styles.
    #[must_use]
    pub fn timestamp_style(mut self, style: Style) -> Self {
        self.timestamp_style = style;
        self
    }

    /// Sets the [`Style`] for the target gutter, over the base and beneath the
    /// target [`Line`]/[`Span`](rstui_core::Span) styles.
    #[must_use]
    pub fn target_style(mut self, style: Style) -> Self {
        self.target_style = style;
        self
    }

    /// Sets the [`Style`] for the message, over the base and beneath the
    /// message [`Line`]/[`Span`](rstui_core::Span) styles.
    #[must_use]
    pub fn message_style(mut self, style: Style) -> Self {
        self.message_style = style;
        self
    }
}

/// Stamps `line` left-to-right from `x0` on row `y`, clipped at `end`, with
/// `base` beneath the line→span cascade. Returns the column one past the last
/// glyph that fit (so a caller can pad the rest of a fixed column).
fn stamp_line(buf: &mut Buffer, line: &Line, base: Style, x0: u16, y: u16, end: u16) -> u16 {
    let line_base = base.patch(line.style);
    let mut x = x0;
    'line: for span in &line.spans {
        let style = line_base.patch(span.style);
        for ch in span.content.chars() {
            if x >= end {
                break 'line;
            }
            buf.set_cell(Position::new(x, y), ch, style);
            x = x.saturating_add(1);
        }
    }
    x
}

/// Fills `[x0, end)` on row `y` with blank cells in `style` (a fixed column's
/// reserved padding, so the next column never shifts).
fn pad_blank(buf: &mut Buffer, style: Style, x0: u16, y: u16, end: u16) {
    let mut x = x0;
    while x < end {
        buf.set_cell(Position::new(x, y), ' ', style);
        x = x.saturating_add(1);
    }
}

impl Widget for LogStream<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let LogStream {
            records,
            offset,
            palette,
            show_timestamp,
            show_target,
            timestamp_width,
            target_width,
            block,
            style,
            timestamp_style,
            target_style,
            message_style,
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

        // Base fills the content area so a background covers the whole pane
        // (including rows past the last record); columns layer on top.
        buf.set_style(inner, style);
        if records.is_empty() {
            return;
        }

        let left = inner.left();
        let right = inner.right();
        let top = inner.top();
        let width = inner.width;

        // The level tag is always 5 wide. Decide which optional gutters fit:
        // each gutter costs its width plus a one-space separator. Drop them in
        // priority order (timestamp first, then target) when too narrow so the
        // tag + message always fit; this is total, never a panic.
        const TAG_WIDTH: u16 = 5;
        let mut ts_cols = if show_timestamp {
            timestamp_width.saturating_add(1)
        } else {
            0
        };
        let mut tg_cols = if show_target {
            target_width.saturating_add(1)
        } else {
            0
        };
        if ts_cols.saturating_add(tg_cols).saturating_add(TAG_WIDTH) > width {
            ts_cols = 0;
            if tg_cols.saturating_add(TAG_WIDTH) > width {
                tg_cols = 0;
            }
        }

        for (row, record) in records
            .iter()
            .skip(offset)
            .take(inner.height as usize)
            .enumerate()
        {
            let y = top.saturating_add(row as u16);
            let mut x = left;

            // Timestamp gutter: blank-padded to its fixed width (so the tag
            // never shifts), then a one-space separator.
            if ts_cols > 0 {
                let col_end = x.saturating_add(timestamp_width).min(right);
                let ts_base = style.patch(timestamp_style);
                let drawn = match &record.timestamp {
                    Some(line) => stamp_line(buf, line, ts_base, x, y, col_end),
                    None => x,
                };
                pad_blank(buf, ts_base, drawn, y, col_end);
                x = x.saturating_add(ts_cols).min(right);
            }

            // The level tag, bold in the palette colour, over the base.
            let tag = record.level.tag();
            let tag_style = style
                .patch(Style::new().fg(palette.color(record.level)))
                .patch(Style::new().add_modifier(rstui_core::Modifier::BOLD));
            let mut tx = x;
            for ch in tag.chars() {
                if tx >= right {
                    break;
                }
                buf.set_cell(Position::new(tx, y), ch, tag_style);
                tx = tx.saturating_add(1);
            }
            x = x.saturating_add(TAG_WIDTH).min(right);

            // Target gutter: a one-space separator then blank-padded to its
            // fixed width (so the message never shifts).
            if tg_cols > 0 {
                let sep_end = x.saturating_add(1).min(right);
                pad_blank(buf, style, x, y, sep_end);
                let col_start = sep_end;
                let col_end = col_start.saturating_add(target_width).min(right);
                let tg_base = style.patch(target_style);
                let drawn = match &record.target {
                    Some(line) => stamp_line(buf, line, tg_base, col_start, y, col_end),
                    None => col_start,
                };
                pad_blank(buf, tg_base, drawn, y, col_end);
                x = x.saturating_add(tg_cols).min(right);
            }

            // One blank separator before the message, then the message
            // clipped to whatever width remains.
            let sep_end = x.saturating_add(1).min(right);
            pad_blank(buf, style, x, y, sep_end);
            let msg_base = style.patch(message_style);
            stamp_line(buf, &record.message, msg_base, sep_end, y, right);
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
    fn the_tag_is_five_wide_and_space_padded() {
        assert_eq!(LogLevel::Trace.tag(), "TRACE");
        assert_eq!(LogLevel::Debug.tag(), "DEBUG");
        assert_eq!(LogLevel::Info.tag(), "INFO ");
        assert_eq!(LogLevel::Warn.tag(), "WARN ");
        assert_eq!(LogLevel::Error.tag(), "ERROR");
        for lvl in [
            LogLevel::Trace,
            LogLevel::Debug,
            LogLevel::Info,
            LogLevel::Warn,
            LogLevel::Error,
        ] {
            assert_eq!(lvl.tag().chars().count(), 5);
        }
    }

    #[test]
    fn the_default_level_is_info_and_severity_orders() {
        assert_eq!(LogLevel::default(), LogLevel::Info);
        assert!(LogLevel::Trace < LogLevel::Debug);
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
    }

    #[test]
    fn the_default_palette_maps_each_level_to_its_ansi_colour() {
        let p = LogPalette::default();
        assert_eq!(p.color(LogLevel::Trace), Color::DarkGray);
        assert_eq!(p.color(LogLevel::Debug), Color::Blue);
        assert_eq!(p.color(LogLevel::Info), Color::Green);
        assert_eq!(p.color(LogLevel::Warn), Color::Yellow);
        assert_eq!(p.color(LogLevel::Error), Color::Red);
    }

    #[test]
    fn a_row_lays_columns_out_left_to_right_one_space_apart() {
        let records = [LogRecord::new(LogLevel::Info, "hello")
            .timestamp("12:00:00")
            .target("net")];
        // ts(12) + sep(1) + tag(5) + sep(1) + tg(16) + sep(1) + msg.
        let out = lines(LogStream::new(&records), 50, 1);
        assert_eq!(out, "12:00:00     INFO  net              hello         \n");
    }

    #[test]
    fn offset_skips_leading_records_and_height_clips_trailing() {
        let records = [
            LogRecord::new(LogLevel::Info, "r0"),
            LogRecord::new(LogLevel::Info, "r1"),
            LogRecord::new(LogLevel::Info, "r2"),
            LogRecord::new(LogLevel::Info, "r3"),
        ];
        let stream = LogStream::new(&records)
            .offset(1)
            .show_timestamp(false)
            .show_target(false);
        // Window is records[1..3]: "INFO  r1" / "INFO  r2".
        assert_eq!(lines(stream, 8, 2), "INFO  r1\nINFO  r2\n");
    }

    #[test]
    fn an_offset_past_the_end_shows_nothing() {
        let records = [LogRecord::new(LogLevel::Info, "only")];
        let stream = LogStream::new(&records).offset(5);
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
        stream.render(buf.area(), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn a_none_timestamp_or_target_renders_blank_padding_keeping_columns_aligned() {
        let records = [
            LogRecord::new(LogLevel::Info, "with")
                .timestamp("01")
                .target("t"),
            LogRecord::new(LogLevel::Info, "without"),
        ];
        let out = lines(LogStream::new(&records), 40, 2);
        // Both messages start at the same column even though row 2 has no
        // timestamp/target — blank-padded gutters hold the columns.
        let row0: Vec<char> = out.lines().next().unwrap().chars().collect();
        let row1: Vec<char> = out.lines().nth(1).unwrap().chars().collect();
        let i0 = row0.iter().position(|&c| c == 'w').unwrap();
        let i1 = row1.iter().position(|&c| c == 'w').unwrap();
        assert_eq!(i0, i1);
    }

    #[test]
    fn hidden_columns_are_omitted_entirely_with_no_reserved_padding() {
        let records = [LogRecord::new(LogLevel::Error, "msg")
            .timestamp("ts")
            .target("tg")];
        let stream = LogStream::new(&records)
            .show_timestamp(false)
            .show_target(false);
        // No gutters at all: tag at col 0, one sep, then the message.
        assert_eq!(lines(stream, 12, 1), "ERROR msg   \n");
    }

    #[test]
    fn fixed_gutter_widths_clip_overlong_content() {
        let records = [LogRecord::new(LogLevel::Info, "m")
            .timestamp("0123456789ABCDEF")
            .target("module::path::name")];
        let stream = LogStream::new(&records).timestamp_width(4).target_width(3);
        // ts clipped to 4, tg clipped to 3, one space between each column.
        assert_eq!(lines(stream, 22, 1), "0123 INFO  mod m      \n");
    }

    #[test]
    fn a_too_narrow_area_drops_the_timestamp_then_the_target() {
        let records = [LogRecord::new(LogLevel::Warn, "hi")
            .timestamp("12:00")
            .target("svc")];
        // Width 10: ts(12)+1+tag(5)+tg(16)+1 > 10 → drop ts. Then
        // tag(5)+tg(16)+1 > 10 → drop tg too. Only tag + sep + message.
        assert_eq!(lines(LogStream::new(&records), 10, 1), "WARN  hi  \n");
    }

    #[test]
    fn a_narrow_area_keeps_the_target_when_only_the_timestamp_must_drop() {
        let records = [LogRecord::new(LogLevel::Info, "ok").target("db")];
        // ts default 12 (+1) makes it overflow at width 28, but
        // tag(5)+tg(16+1)+sep(1) = 23 ≤ 28 → keep the target, drop only ts.
        let out = lines(LogStream::new(&records).timestamp_width(20), 28, 1);
        assert_eq!(out, "INFO  db               ok   \n");
    }

    #[test]
    fn the_message_is_clipped_to_the_remaining_width() {
        let records = [LogRecord::new(LogLevel::Info, "abcdefghij")];
        let stream = LogStream::new(&records)
            .show_timestamp(false)
            .show_target(false);
        // tag(5) + sep(1) = 6 columns used; 4 left → "abcd".
        assert_eq!(lines(stream, 10, 1), "INFO  abcd\n");
    }

    #[test]
    fn the_level_tag_is_bold_in_the_palette_colour() {
        let records = [LogRecord::new(LogLevel::Error, "x")];
        let stream = LogStream::new(&records)
            .show_timestamp(false)
            .show_target(false);
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        stream.render(buf.area(), &mut buf);
        let e = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(e.symbol, 'E');
        assert_eq!(e.fg, Color::Red); // default error colour
        assert!(e.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_custom_palette_recolours_the_tag() {
        let records = [LogRecord::new(LogLevel::Info, "x")];
        let stream = LogStream::new(&records)
            .palette(LogPalette {
                info: Color::Magenta,
                ..LogPalette::default()
            })
            .show_timestamp(false)
            .show_target(false);
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        stream.render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().fg, Color::Magenta);
    }

    #[test]
    fn style_cascades_base_then_column_then_line_then_span() {
        let records = [LogRecord::new(
            LogLevel::Info,
            Line::from(Span::styled("M", Style::new().fg(Color::Cyan))),
        )
        .timestamp("T")];
        let stream = LogStream::new(&records)
            .show_target(false)
            .style(Style::new().bg(Color::Blue))
            .timestamp_style(Style::new().add_modifier(Modifier::ITALIC))
            .message_style(Style::new().add_modifier(Modifier::UNDERLINED));
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 1));
        stream.render(buf.area(), &mut buf);

        // Timestamp glyph: base bg + timestamp_style modifier.
        let t = buf.get(Position::new(0, 0)).unwrap();
        assert_eq!(t.symbol, 'T');
        assert_eq!(t.bg, Color::Blue);
        assert!(t.modifier.contains(Modifier::ITALIC));

        // Message glyph "M": base bg, message_style underline, span fg wins.
        let m = (0..30)
            .map(|x| buf.get(Position::new(x, 0)).unwrap().clone())
            .find(|c| c.symbol == 'M')
            .unwrap();
        assert_eq!(m.fg, Color::Cyan);
        assert_eq!(m.bg, Color::Blue);
        assert!(m.modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn the_base_style_fills_the_whole_content_area() {
        let records = [LogRecord::new(LogLevel::Info, "x")];
        let stream = LogStream::new(&records).style(Style::new().bg(Color::Red));
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 3));
        stream.render(buf.area(), &mut buf);
        // The single record is row 0; rows 1 and 2 are still filled.
        for y in 0..3 {
            for x in 0..30 {
                assert_eq!(buf.get(Position::new(x, y)).unwrap().bg, Color::Red);
            }
        }
    }

    #[test]
    fn a_block_frames_the_stream_in_the_inner_area() {
        let records = [LogRecord::new(LogLevel::Info, "ab")];
        let stream = LogStream::new(&records)
            .show_timestamp(false)
            .show_target(false)
            .block(Block::bordered());
        // inner is 8×1: "INFO  ab".
        assert_eq!(lines(stream, 10, 3), "┌────────┐\n│INFO  ab│\n└────────┘\n");
    }

    #[test]
    fn no_records_with_a_block_still_renders_the_block() {
        let records: [LogRecord; 0] = [];
        let stream = LogStream::new(&records).block(Block::bordered());
        assert_eq!(lines(stream, 3, 3), "┌─┐\n│ │\n└─┘\n");
    }

    #[test]
    fn a_block_too_small_for_an_inner_area_draws_no_rows() {
        let records = [LogRecord::new(LogLevel::Info, "z")];
        let stream = LogStream::new(&records).block(Block::bordered());
        assert_eq!(lines(stream, 2, 2), "┌┐\n└┘\n");
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let records = [LogRecord::new(LogLevel::Info, "z")];
        let stream = LogStream::new(&records)
            .show_timestamp(false)
            .show_target(false);
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 4));
        stream.render(Rect::new(2, 1, 8, 1), &mut buf);
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, 'I');
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let records = [LogRecord::new(LogLevel::Info, "x")];
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        LogStream::new(&records).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
