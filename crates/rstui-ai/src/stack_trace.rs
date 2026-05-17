//! [`StackTrace`] — a parsed, collapsible error stack: the
//! `TypeError: … \n at fn (file:line:col)` block an agent surfaces, turned
//! into navigable frames.
//!
//! # A total parser, then a pure projection
//!
//! The ai-elements `StackTrace` regex-parses a trace string into
//! `{ errorType, errorMessage, frames }` and renders a collapsible list with
//! internal frames hidden by default and a click-to-open-file affordance.
//! Here:
//!
//! - [`ParsedStackTrace::parse`] is a **total** classifier (the
//!   [`UiPart::from_value`](crate::model::UiPart::from_value) precedent): every
//!   line that does not match a frame pattern still becomes a frame (raw, no
//!   location) — it never errors. It handles the Node forms
//!   `at fn (path:line:col)` / `at path:line:col` *and* the Rust backtrace
//!   form `<n>: fn` then `at src/…:line:col` (the brief's scope), and tags a
//!   frame internal if its path is `node_modules` / `node:` / `internal/` /
//!   the Rust std (`/rustc/` or `library/`).
//! - [`StackTrace`] is a pure projection of that parse plus caller-owned
//!   [`open`](StackTrace::open) (collapsed disclosure) and
//!   [`hide_internal`](StackTrace::hide_internal) (filter). Opening a frame
//!   is the documented hit-test seam: the host maps a click in a
//!   [`frame_rects`](StackTrace::frame_rects) entry to a
//!   [`StackTraceIntent::OpenFrame`], never a callback.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area, an
//! empty trace, and over-many frames are all safe clips — never a panic.

use rstui_core::{Buffer, Color, Modifier, Position, Rect, Style, Widget};

/// One parsed stack frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    /// The function/symbol name, if the line carried one.
    pub function: Option<String>,
    /// The source file path, if the line carried one.
    pub file: Option<String>,
    /// The 1-based line number, if present.
    pub line: Option<u32>,
    /// The 1-based column, if present.
    pub column: Option<u32>,
    /// `true` for a runtime/dependency frame (`node_modules`, `node:`,
    /// `internal/`, Rust std) — hidden by default.
    pub is_internal: bool,
    /// The original line, verbatim (for an unparsed frame this is all there
    /// is).
    pub raw: String,
}

/// A parsed error stack: the error head and its frames.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedStackTrace {
    /// The error type (`TypeError`, `Error`, a Rust `panicked at` is `None`),
    /// if the first line matched `Type: message`.
    pub error_type: Option<String>,
    /// The error message (the whole first line if no `Type:` prefix).
    pub error_message: String,
    /// The frames, in order.
    pub frames: Vec<StackFrame>,
}

/// `true` if `path` is a runtime/dependency/std location.
fn path_is_internal(path: &str) -> bool {
    path.contains("node_modules")
        || path.starts_with("node:")
        || path.contains("internal/")
        || path.contains("/rustc/")
        || path.starts_with("library/")
        || path.contains("/library/")
}

/// Splits a `…:line:col` (or `…:line`) tail off `loc`, returning
/// `(file, line, col)`.
fn split_location(loc: &str) -> (String, Option<u32>, Option<u32>) {
    let parts: Vec<&str> = loc.rsplitn(3, ':').collect();
    // rsplitn yields reversed: [col, line, file] | [line, file] | [file].
    match parts.as_slice() {
        [col, line, file] if col.parse::<u32>().is_ok() && line.parse::<u32>().is_ok() => {
            ((*file).to_owned(), line.parse().ok(), col.parse().ok())
        }
        [line, file] if line.parse::<u32>().is_ok() => {
            ((*file).to_owned(), line.parse().ok(), None)
        }
        _ => (loc.to_owned(), None, None),
    }
}

impl ParsedStackTrace {
    /// Parses `trace`. **Total** — an unparseable line still becomes a
    /// (raw) frame, an empty trace is an empty parse; never errors (see the
    /// [module docs](self)).
    #[must_use]
    pub fn parse(trace: &str) -> Self {
        let mut lines = trace.lines().map(str::trim).filter(|l| !l.is_empty());
        let Some(first) = lines.next() else {
            return Self::default();
        };

        let (error_type, error_message) = match first.split_once(": ") {
            Some((ty, msg)) if ty.ends_with("Error") || ty == "Error" => {
                (Some(ty.to_owned()), msg.to_owned())
            }
            _ => (None, first.to_owned()),
        };

        let frames = trace
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .skip(1)
            .map(Self::parse_frame)
            .collect();

        Self {
            error_type,
            error_message,
            frames,
        }
    }

    /// Classifies one frame line — Node `at fn (loc)` / `at loc`, or Rust
    /// `<n>: fn` and `at src/…:l:c`. Unmatched → a raw frame.
    fn parse_frame(line: &str) -> StackFrame {
        let raw = line.to_owned();

        // Rust backtrace location line: "at src/main.rs:10:5".
        if let Some(loc) = line.strip_prefix("at ") {
            // Node "at fn (path:line:col)".
            if let Some((func, rest)) = loc.split_once(" (") {
                let inner = rest.strip_suffix(')').unwrap_or(rest);
                let (file, ln, col) = split_location(inner);
                let is_internal = path_is_internal(&file);
                return StackFrame {
                    function: Some(func.to_owned()),
                    file: Some(file),
                    line: ln,
                    column: col,
                    is_internal,
                    raw,
                };
            }
            // Node/Rust "at path:line:col" (no function).
            let (file, ln, col) = split_location(loc);
            let is_internal = path_is_internal(&file);
            return StackFrame {
                function: None,
                file: Some(file),
                line: ln,
                column: col,
                is_internal,
                raw,
            };
        }

        // Rust backtrace symbol line: "12: my_crate::do_thing".
        if let Some((idx, func)) = line.split_once(": ") {
            if idx.chars().all(|c| c.is_ascii_digit()) && !idx.is_empty() {
                let func = func.trim();
                let is_internal = func.starts_with("std::")
                    || func.starts_with("core::")
                    || func.starts_with("alloc::");
                return StackFrame {
                    function: Some(func.to_owned()),
                    file: None,
                    line: None,
                    column: None,
                    is_internal,
                    raw,
                };
            }
        }

        StackFrame {
            function: None,
            file: None,
            line: None,
            column: None,
            is_internal: line.contains("node_modules") || line.contains("node:"),
            raw,
        }
    }
}

/// The reducer-consumed intent a [`StackTrace`] surfaces — the host maps a
/// click in a [`frame_rects`](StackTrace::frame_rects) entry to
/// `OpenFrame(index)` (index into the *full* frame list) and the reducer
/// opens that file in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackTraceIntent {
    /// Open the source for the frame at this index in
    /// [`ParsedStackTrace::frames`].
    OpenFrame(usize),
}

/// A parsed, collapsible error stack.
///
/// Row 0 is the error head (`▾`/`▸` chevron + `Type: message`, accented).
/// When [`open`](Self::open) the visible frames follow — internal frames
/// dropped when [`hide_internal`](Self::hide_internal) — each a `fn  file:l:c`
/// (or its raw text). `StackTrace` owns no state — see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::stack_trace::{ParsedStackTrace, StackTrace};
///
/// let trace = "TypeError: bad\n  at run (src/app.ts:3:9)\n  at node:internal/x:1:1";
/// let parsed = ParsedStackTrace::parse(trace);
/// assert_eq!(parsed.error_type.as_deref(), Some("TypeError"));
/// assert_eq!(parsed.frames.len(), 2);
/// assert!(parsed.frames[1].is_internal); // node:internal/…
///
/// let widget = StackTrace::new(&parsed).open(true);
/// let area = Rect::new(0, 0, 30, 3);
/// // With internals hidden only the app frame is hit-testable.
/// assert_eq!(widget.frame_rects(area).len(), 1);
///
/// let mut buf = Buffer::empty(area);
/// widget.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '▾');
/// ```
#[derive(Debug, Clone)]
pub struct StackTrace<'a> {
    parsed: &'a ParsedStackTrace,
    open: bool,
    hide_internal: bool,
    style: Style,
    header_style: Style,
}

impl<'a> StackTrace<'a> {
    /// A collapsed view of `parsed`, with internal frames hidden.
    #[must_use]
    pub fn new(parsed: &'a ParsedStackTrace) -> Self {
        Self {
            parsed,
            open: false,
            hide_internal: true,
            style: Style::new(),
            header_style: Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        }
    }

    /// Sets the caller-owned open flag (the reducer flips it on a header
    /// click; the widget only reads it).
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets whether internal (runtime/dependency/std) frames are filtered
    /// out (caller-owned; default `true`).
    #[must_use]
    pub fn hide_internal(mut self, hide_internal: bool) -> Self {
        self.hide_internal = hide_internal;
        self
    }

    /// Sets the base [`Style`], beneath the header style.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] the error-head row is drawn with.
    #[must_use]
    pub fn header_style(mut self, header_style: Style) -> Self {
        self.header_style = header_style;
        self
    }

    /// The error-head line shown on row 0.
    fn head_line(&self) -> String {
        match &self.parsed.error_type {
            Some(ty) => format!("{}: {}", ty, self.parsed.error_message),
            None => self.parsed.error_message.clone(),
        }
    }

    /// `(full_index, frame)` for every frame that passes the
    /// [`hide_internal`](Self::hide_internal) filter, in order.
    fn visible_frames(&self) -> Vec<(usize, &StackFrame)> {
        self.parsed
            .frames
            .iter()
            .enumerate()
            .filter(|(_, f)| !(self.hide_internal && f.is_internal))
            .collect()
    }

    /// One presentable line for `frame`.
    fn frame_line(frame: &StackFrame) -> String {
        match (&frame.function, &frame.file) {
            (_, Some(file)) => {
                let func = frame.function.as_deref().unwrap_or("<anon>");
                let loc = match (frame.line, frame.column) {
                    (Some(l), Some(c)) => format!("{file}:{l}:{c}"),
                    (Some(l), None) => format!("{file}:{l}"),
                    _ => file.clone(),
                };
                format!("{func}  {loc}")
            }
            (Some(func), None) => func.clone(),
            (None, None) => frame.raw.clone(),
        }
    }

    /// The hit [`Rect`] of every visible frame row when [`open`](Self::open),
    /// in order below the head row. Result index *i* corresponds to the
    /// *full* frame index in `pair.0`; the host maps a click to
    /// [`StackTraceIntent::OpenFrame`] with that index.
    #[must_use]
    pub fn frame_rects(&self, area: Rect) -> Vec<Rect> {
        if !self.open || area.is_empty() || area.height <= 1 {
            return Vec::new();
        }
        let rows = area.height as usize - 1;
        self.visible_frames()
            .iter()
            .take(rows)
            .enumerate()
            .map(|(row, _)| {
                Rect::new(
                    area.left(),
                    area.top().saturating_add(1).saturating_add(row as u16),
                    area.width,
                    1,
                )
            })
            .collect()
    }

    /// The full frame index each [`frame_rects`](Self::frame_rects) entry
    /// maps to (parallel to that vec) — the host pairs a click's row with
    /// this to build [`StackTraceIntent::OpenFrame`].
    #[must_use]
    pub fn visible_frame_indices(&self, area: Rect) -> Vec<usize> {
        if !self.open || area.is_empty() || area.height <= 1 {
            return Vec::new();
        }
        let rows = area.height as usize - 1;
        self.visible_frames()
            .iter()
            .take(rows)
            .map(|(idx, _)| *idx)
            .collect()
    }
}

impl Widget for StackTrace<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        buf.set_style(area, self.style);

        // The error head row.
        let head_base = self.style.patch(self.header_style);
        let chevron = if self.open { '▾' } else { '▸' };
        let mut x = area.left();
        let y = area.top();
        buf.set_cell(Position::new(x, y), chevron, head_base);
        x = x.saturating_add(2);
        for ch in self.head_line().chars() {
            if x >= area.right() {
                break;
            }
            buf.set_cell(Position::new(x, y), ch, head_base);
            x = x.saturating_add(1);
        }

        // The frames.
        if !self.open || area.height <= 1 {
            return;
        }
        let rows = area.height as usize - 1;
        for (row, (_, frame)) in self.visible_frames().iter().take(rows).enumerate() {
            let fy = area.top().saturating_add(1).saturating_add(row as u16);
            let mut fx = area.left().saturating_add(2);
            for ch in Self::frame_line(frame).chars() {
                if fx >= area.right() {
                    break;
                }
                buf.set_cell(Position::new(fx, fy), ch, self.style);
                fx = fx.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(widget: StackTrace<'_>, w: u16, h: u16) -> String {
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
    fn it_parses_a_node_trace_with_internal_tagging() {
        let trace = "TypeError: x is not a function\n  at run (/app/src/a.ts:3:9)\n  at /app/node_modules/lib/b.js:1:1\n  at node:internal/process:2:2";
        let p = ParsedStackTrace::parse(trace);
        assert_eq!(p.error_type.as_deref(), Some("TypeError"));
        assert_eq!(p.error_message, "x is not a function");
        assert_eq!(p.frames.len(), 3);
        assert_eq!(p.frames[0].function.as_deref(), Some("run"));
        assert_eq!(p.frames[0].file.as_deref(), Some("/app/src/a.ts"));
        assert_eq!(p.frames[0].line, Some(3));
        assert_eq!(p.frames[0].column, Some(9));
        assert!(!p.frames[0].is_internal);
        assert!(p.frames[1].is_internal); // node_modules
        assert!(p.frames[2].is_internal); // node:internal/
    }

    #[test]
    fn it_parses_a_rust_backtrace() {
        let trace = "thread 'main' panicked at boom\n  12: my_crate::do_thing\n  at src/main.rs:10:5\n  13: std::rt::lang_start";
        let p = ParsedStackTrace::parse(trace);
        assert_eq!(p.error_type, None);
        assert_eq!(p.error_message, "thread 'main' panicked at boom");
        assert_eq!(p.frames.len(), 3);
        assert_eq!(p.frames[0].function.as_deref(), Some("my_crate::do_thing"));
        assert!(!p.frames[0].is_internal);
        assert_eq!(p.frames[1].file.as_deref(), Some("src/main.rs"));
        assert_eq!(p.frames[1].line, Some(10));
        assert!(p.frames[2].is_internal); // std::
    }

    #[test]
    fn totality_unparseable_lines_become_raw_frames() {
        let p = ParsedStackTrace::parse("just a message\n???garbage\n  at x:1:2");
        assert_eq!(p.error_message, "just a message");
        assert_eq!(p.frames.len(), 2);
        assert_eq!(p.frames[0].raw, "???garbage");
        assert!(p.frames[0].function.is_none());
        assert!(p.frames[0].file.is_none());
        // An empty trace is the default.
        assert_eq!(ParsedStackTrace::parse(""), ParsedStackTrace::default());
        assert_eq!(
            ParsedStackTrace::parse("   \n  "),
            ParsedStackTrace::default()
        );
    }

    #[test]
    fn closed_shows_only_the_head_open_shows_frames() {
        let p = ParsedStackTrace::parse("Error: nope\n  at f (a.ts:1:2)");
        assert_eq!(
            lines(StackTrace::new(&p), 16, 2),
            "▸ Error: nope   \n                \n"
        );
        assert_eq!(
            lines(StackTrace::new(&p).open(true), 16, 2),
            "▾ Error: nope   \n  f  a.ts:1:2   \n"
        );
    }

    #[test]
    fn hide_internal_filters_runtime_frames() {
        let trace = "Error: e\n  at app (src/a.ts:1:1)\n  at /node_modules/x.js:2:2";
        let p = ParsedStackTrace::parse(trace);
        // Hidden (default): only the app frame.
        let hidden = StackTrace::new(&p).open(true);
        assert_eq!(hidden.frame_rects(Rect::new(0, 0, 20, 4)).len(), 1);
        assert_eq!(
            hidden.visible_frame_indices(Rect::new(0, 0, 20, 4)),
            vec![0]
        );
        // Shown: both frames, indices into the full list.
        let shown = StackTrace::new(&p).open(true).hide_internal(false);
        assert_eq!(
            shown.visible_frame_indices(Rect::new(0, 0, 20, 4)),
            vec![0, 1]
        );
    }

    #[test]
    fn frame_rects_are_empty_when_closed() {
        let p = ParsedStackTrace::parse("Error: e\n  at f (a.ts:1:1)");
        assert!(
            StackTrace::new(&p)
                .frame_rects(Rect::new(0, 0, 20, 4))
                .is_empty()
        );
    }

    #[test]
    fn over_many_frames_clip_to_the_area() {
        let trace = "Error: e\n  at a (s:1:1)\n  at b (s:2:2)\n  at c (s:3:3)";
        let p = ParsedStackTrace::parse(trace);
        // height 3 → head + only 2 frame rows.
        let widget = StackTrace::new(&p).open(true);
        assert_eq!(widget.frame_rects(Rect::new(0, 0, 20, 3)).len(), 2);
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let p = ParsedStackTrace::parse("Error: e\n  at f (a:1:1)");
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        StackTrace::new(&p).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
