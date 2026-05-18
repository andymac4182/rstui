//! A code-editor experience: a file [`Tabs`] strip, a real multi-line
//! [`Editor`] over a caller-owned [`TextArea`] (type, arrows move the caret,
//! Enter splits lines), a [`List`] of problems, and a [`StatusBar`] with the
//! live cursor position. `PgUp/PgDn` switch files.

use std::cell::Cell;

use rstui_core::{
    Constraint, KeyCode, Layout, Line, Position, Rect, Style, TextArea, stylize::Stylize,
};
use rstui_runtime::Frame;
// ADR 0024: `Editor`/`LineNumberGutter` + the syntax engine live in `rstui-code`.
use rstui_code::treesitter::{Analyzer, TsLanguage};
use rstui_code::{Editor, LineNumberGutter, syntax};
use rstui_widgets::{Block, BorderType, List, StatusBar, Tabs};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// A problems-pane entry: line, severity (`true` = error), message. The
/// line numbers point into the (now realistically long) `main.rs` buffer
/// so the pane reads like a real diagnostics list against real code.
const PROBLEMS: [(u32, bool, &str); 6] = [
    (4, false, "unused import: `Constraint`"),
    (29, true, "mismatched types: expected `u16`, found `usize`"),
    (47, false, "variable does not need to be mutable: `offset`"),
    (63, true, "cannot borrow `*frame` as mutable more than once"),
    (88, false, "function `unused_probe` is never used"),
    (119, true, "expected `;`, found `}`"),
];

/// `main.rs` — a realistically long buffer so the editor, the line-number
/// gutter, and caret-driven scrolling all get a real workout.
const MAIN_RS: &str = r#"//! A tiny terminal counter app, written against the same pure-projection
//! contract the rest of rstui uses: a reducer owns all mutation, the view
//! is a pure function of state, and the frame boundary sits between them.

use std::process::ExitCode;

use rstui_core::{Constraint, KeyCode, Layout, Line, Rect, Style};
use rstui_runtime::{App, Cmd, Frame, Terminal};
use rstui_widgets::{Block, BorderType, Gauge, Paragraph, StatusBar};

/// Everything the app can be told to do. Events are mapped to one of
/// these in `on_event`; nothing else mutates state.
#[derive(Debug, Clone, Copy)]
enum Msg {
    Increment,
    Decrement,
    Reset,
    Quit,
}

/// The whole application state — a single counter and a running flag.
/// There is exactly one place this changes: the `update` reducer.
struct Counter {
    value: i64,
    history: Vec<i64>,
    running: bool,
}

impl Counter {
    fn new() -> Self {
        Self {
            value: 0,
            history: Vec::new(),
            running: true,
        }
    }

    /// The fraction of the way to the goal, clamped to `0.0..=1.0`. This
    /// is presentation math, so it lives next to the view, not the
    /// reducer — the reducer never thinks about the gauge.
    fn ratio(&self) -> f64 {
        const GOAL: f64 = 100.0;
        (self.value as f64 / GOAL).clamp(0.0, 1.0)
    }

    /// A short, human label for the status bar.
    fn label(&self) -> String {
        let offset = self.history.len();
        format!("value {} · {} steps", self.value, offset)
    }
}

impl App for Counter {
    type Message = Msg;

    /// Pure input mapping: a key becomes an intent, or nothing. No state
    /// is touched here — that is the reducer's sole responsibility.
    fn on_event(&self, event: rstui_core::Event) -> Option<Msg> {
        let key = event.as_key()?;
        match key.code {
            KeyCode::Up | KeyCode::Char('+') => Some(Msg::Increment),
            KeyCode::Down | KeyCode::Char('-') => Some(Msg::Decrement),
            KeyCode::Char('r') => Some(Msg::Reset),
            KeyCode::Char('q') | KeyCode::Esc => Some(Msg::Quit),
            _ => None,
        }
    }

    /// The reducer. Every mutation in the program funnels through here,
    /// which is why the app can be reasoned about by reading one method.
    fn update(&mut self, msg: Msg) -> Cmd<Msg> {
        match msg {
            Msg::Increment => {
                self.history.push(self.value);
                self.value += 1;
            }
            Msg::Decrement => {
                self.history.push(self.value);
                self.value -= 1;
            }
            Msg::Reset => {
                self.history.clear();
                self.value = 0;
            }
            Msg::Quit => self.running = false,
        }
        Cmd::none()
    }

    /// The view. A pure function of state: same state, same buffer, with
    /// no float math in the layout and nothing retained between frames.
    fn view(&self, frame: &mut Frame<'_>) {
        let [header, body, gauge, status] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        frame.render_widget(
            Line::from(" rstui counter — ↑/↓ adjust · r reset · q quit ")
                .style(Style::new().fg(rstui_core::Color::Cyan)),
            header,
        );

        let big = format!("\n   {}\n", self.value);
        frame.render_widget(
            Paragraph::new(big).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(Line::from(" count ")),
            ),
            body,
        );

        frame.render_widget(Gauge::new().ratio(self.ratio()), gauge);

        frame.render_widget(
            StatusBar::new()
                .left(Line::from(self.label()))
                .right(Line::from(" UTF-8 · rust ")),
            status,
        );
    }

    fn running(&self) -> bool {
        self.running
    }
}

fn main() -> ExitCode {
    let mut terminal = match Terminal::new() {
        Ok(t) => t,
        Err(err) => {
            eprintln!("could not open the terminal: {err}");
            return ExitCode::FAILURE;
        }
    };
    let app = Counter::new();
    if let Err(err) = terminal.run(app) {
        eprintln!("run loop exited with: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
"#;

/// `lib.rs` — a realistically long widget module so switching to it shows
/// a different, equally scrollable buffer.
const LIB_RS: &str = r#"//! A small reusable widget: a labelled progress bar that is a pure
//! projection of a caller-owned ratio. It owns no state, registers no
//! callback, and is consumed by a single render — the house contract.

use rstui_core::{Buffer, Rect, Style};

/// How the percentage label is rendered relative to the filled track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelMode {
    /// No label at all — just the track.
    Hidden,
    /// Centered over the whole bar.
    #[default]
    Centered,
    /// Tucked against the right edge.
    Trailing,
}

/// A horizontal progress bar. Construct it, set the ratio and style,
/// hand it to `render`, drop it. Nothing survives the call.
#[derive(Debug, Clone)]
pub struct ProgressBar {
    ratio: f64,
    filled_style: Style,
    track_style: Style,
    label: LabelMode,
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self {
            ratio: 0.0,
            filled_style: Style::default(),
            track_style: Style::default(),
            label: LabelMode::default(),
        }
    }
}

impl ProgressBar {
    /// A new bar at zero. Use the builders to configure it.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the fill fraction, clamped to a sane `0.0..=1.0` so a caller
    /// cannot drive the bar off the end of its own track.
    #[must_use]
    pub fn ratio(mut self, ratio: f64) -> Self {
        self.ratio = ratio.clamp(0.0, 1.0);
        self
    }

    /// Sets the style of the filled portion.
    #[must_use]
    pub fn filled_style(mut self, style: Style) -> Self {
        self.filled_style = style;
        self
    }

    /// Chooses where (or whether) the percentage label is drawn.
    #[must_use]
    pub fn label(mut self, mode: LabelMode) -> Self {
        self.label = mode;
        self
    }

    /// The number of fully filled cells for a track `width` wide. Integer
    /// math only: the same width and ratio always fill the same cells.
    fn filled_cells(&self, width: u16) -> u16 {
        let scaled = self.ratio * width as f64;
        scaled.round() as u16
    }
}

/// The one method the framework calls. It stamps cells into the area it
/// is given and returns; it never reads the previous frame.
pub fn render(bar: &ProgressBar, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let filled = bar.filled_cells(area.width);
    for x in 0..area.width {
        let style = if x < filled {
            bar.filled_style
        } else {
            bar.track_style
        };
        for y in 0..area.height {
            buf.set(
                rstui_core::Position::new(area.x + x, area.y + y),
                ' ',
                style,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_is_clamped_both_ends() {
        assert_eq!(ProgressBar::new().ratio(2.0).ratio, 1.0);
        assert_eq!(ProgressBar::new().ratio(-1.0).ratio, 0.0);
    }

    #[test]
    fn fill_is_deterministic_for_a_width() {
        let bar = ProgressBar::new().ratio(0.5);
        assert_eq!(bar.filled_cells(10), bar.filled_cells(10));
    }
}
"#;

/// `notes.md` — a long design-notes buffer so the third tab is a markdown
/// document of real length rather than a three-line stub.
const NOTES_MD: &str = r#"# Design notes — the counter app

These notes track why the app is shaped the way it is. They are
deliberately long so the editor has a third buffer worth scrolling.

## Goals

- Demonstrate the reducer / view split in the smallest possible app.
- Show that integer-only layout math means no visual drift on resize.
- Keep every mutation in exactly one method (`update`).

## Non-goals

- Persistence. The counter resets on every launch, on purpose.
- Configuration. There are no flags; the keymap is fixed and tiny.
- Async. Everything is synchronous so the model stays obvious.

## The keymap

| Key        | Intent      |
|------------|-------------|
| Up / +     | Increment   |
| Down / -   | Decrement   |
| r          | Reset       |
| q / Esc    | Quit        |

The keymap lives in `on_event` and produces a `Msg`. It never touches
state — that separation is the whole point of the exercise.

## Why a history vector

The `history` vector is not used by the view yet. It exists so that a
later slice can add an undo command without changing the shape of the
reducer: undo will simply pop the vector and assign. Designing the
state for the change you know is coming is cheaper than refactoring
for it later.

## Open questions

1. Should reset clear the history or push a sentinel? Currently it
   clears. An undo across a reset is therefore not possible, which is
   probably the behaviour a user expects anyway.
2. Should the gauge goal be configurable? Not until there is a real
   reason; a hard-coded 100 keeps the demo legible.
3. Is a status bar overkill for a counter? Maybe, but it exercises
   another widget for free, and free coverage is worth a row.

## Checklist before this lands

- [x] Reducer is the only mutator.
- [x] View is a pure function of state.
- [x] Layout uses integer constraints only.
- [x] Ratio is clamped so the gauge cannot overflow its track.
- [ ] Undo command (next slice).
- [ ] A test that drives the loop headlessly through `Harness`.

## Footnote

If you scrolled all the way here with the arrow keys, the caret-driven
scroll and the line-number gutter both did their jobs. That is the
only assertion this file is making.
"#;

/// The editor's caller-owned state: one [`TextArea`] per open file.
#[derive(Debug)]
pub(crate) struct State {
    files: Vec<(&'static str, TextArea)>,
    active: usize,
    /// Caller-owned editor scroll, kept caret-visible via
    /// [`TextArea::scroll_into_view`]. `Cell` so the pure `view` can persist
    /// the minimal-movement offset frame-to-frame (the git-review `Geom`
    /// precedent — ADR 0004: scroll is reducer/model state, the gutter
    /// tracks it so the two never desync).
    scroll: Cell<(usize, usize)>,
}

impl State {
    /// Three open buffers with seeded contents, each opened at the top
    /// (`from_value` leaves the caret at the end).
    pub(crate) fn new() -> Self {
        let open = |text: &str| {
            let mut doc = TextArea::from_value(text);
            doc.set_cursor(0, 0);
            doc
        };
        Self {
            files: vec![
                ("main.rs", open(MAIN_RS)),
                ("lib.rs", open(LIB_RS)),
                ("notes.md", open(NOTES_MD)),
            ],
            active: 0,
            scroll: Cell::new((0, 0)),
        }
    }

    fn doc(&mut self) -> &mut TextArea {
        &mut self.files[self.active].1
    }

    /// The flattened per-char syntax overlay for the active buffer:
    /// language resolved from the file name, token colours from the live
    /// `theme` (so it follows theme switches). One [`Style`] slot per source
    /// char and a blank slot per `'\n'` — exactly the flattened index
    /// [`Editor::syntax`] reads.
    ///
    /// This screen uses the **tree-sitter `Analyzer` (Tier-1)**: a real
    /// parse tree resolves function names, types, attributes, namespaces …
    /// into the widened [`syntax::SyntaxStyles`] semantic classes, so the
    /// editor is genuinely multi-colour (not "everything one colour +
    /// comments grey", which is all the Tier-0 four-bucket lexer can do).
    /// The three demo files (`*.rs` → Rust, `notes.md` → Markdown) are all
    /// tree-sitter-supported in the default feature set. An unknown
    /// extension would resolve to `None` and fall back to the Tier-0
    /// `syntax::line_overlay` loop. A per-frame parse is fine for this demo
    /// screen. Pure: rebuilt fresh in `view`.
    fn overlay(&self, theme: &Theme) -> Vec<Style> {
        let active = &self.files[self.active];
        // A rich themed palette — every Tier-1 semantic class mapped to a
        // theme role, so picking a theme reskins the whole editor.
        let styles = syntax::SyntaxStyles {
            keyword: Style::new().fg(theme.accent),
            function: Style::new().fg(theme.info),
            type_: Style::new().fg(theme.warn),
            constant: Style::new().fg(theme.accent_alt),
            number: Style::new().fg(theme.accent_alt),
            string: Style::new().fg(theme.ok),
            comment: Style::new().fg(theme.dim),
            attribute: Style::new().fg(theme.accent_alt),
            namespace: Style::new().fg(theme.accent_alt),
            operator: Style::new().fg(theme.dim),
            punctuation: Style::new().fg(theme.dim),
            variable: Style::default(), // plain identifiers stay the body fg
        };
        match TsLanguage::from_path(active.0) {
            Some(lang) => {
                // Tier-1: one real parse → the flattened `Vec<Style>`
                // (one slot per source char incl. each `'\n'`) — the exact
                // same shape/length the Tier-0 loop below produces, a true
                // drop-in for `Editor::syntax`.
                let mut a = Analyzer::new(lang);
                a.set_source(&active.1.to_string());
                a.highlight(&styles)
            }
            None => {
                // Tier-0 fallback for an unknown extension: the
                // dependency-free lexer threaded with `LexState` so
                // multi-line strings / comments stay coloured across rows.
                let lang = syntax::Language::from_path(active.0);
                let mut flat: Vec<Style> = Vec::new();
                let mut st = syntax::LexState::default();
                let lines = active.1.lines();
                for (i, line) in lines.iter().enumerate() {
                    let (ov, next) = syntax::line_overlay(line, lang, &styles, st);
                    st = next;
                    flat.extend(ov);
                    if i + 1 < lines.len() {
                        flat.push(Style::new()); // the '\n' between rows
                    }
                }
                flat
            }
        }
    }

    /// Typing edits the buffer; arrows move the caret; `PgUp/PgDn` switch
    /// files. There is no rail fall-back on `←` here — like a real editor,
    /// `←` is a caret move; use `Tab` / the rail to leave.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::PageUp => {
                self.active = self.active.saturating_sub(1);
                self.scroll.set((0, 0)); // a freshly-shown file opens at the top
            }
            KeyCode::PageDown => {
                self.active = (self.active + 1).min(self.files.len() - 1);
                self.scroll.set((0, 0));
            }
            KeyCode::Left => {
                self.doc().move_left();
            }
            KeyCode::Right => {
                self.doc().move_right();
            }
            KeyCode::Up => {
                self.doc().move_up();
            }
            KeyCode::Down => {
                self.doc().move_down();
            }
            KeyCode::Enter => self.doc().insert_newline(),
            KeyCode::Backspace => {
                self.doc().delete_backward();
            }
            KeyCode::Char(c) => self.doc().insert_char(c),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Pasted text is inserted at the caret.
    pub(crate) fn on_paste(&mut self, text: &str) {
        self.doc().insert_str(text);
    }

    /// Cut `sel` out of the active file buffer.
    pub(crate) fn cut(&mut self, sel: &str) -> bool {
        crate::screens::cut_area(self.doc(), sel)
    }

    /// A click on the file-tab strip switches files.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [tabs, _body, _foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(content);
        let names: Vec<&str> = self.files.iter().map(|(n, _)| *n).collect();
        if let Some(i) = crate::screens::tab_index_at(tabs, &names, 1, pos) {
            self.active = i;
            return ScreenOutcome::consumed();
        }
        ScreenOutcome::ignored()
    }

    /// A drag-select stays inside the code area (the rect *after* the
    /// line-number gutter) or the Problems list — never across the two or
    /// over the gutter. Mirrors [`view`]'s editor composition exactly.
    pub(crate) fn selection_region(&self, pos: Position, content: Rect) -> Option<Rect> {
        let [_, body, _] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(content);
        let [editor_a, problems] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(34)]).areas(body);
        if editor_a.contains(pos) {
            let ia = crate::screens::block_inner(editor_a);
            let rows = self.files[self.active].1.row_count();
            let gutter = LineNumberGutter::new(1, rows).min_number_width(3);
            return Some(gutter.inner(ia));
        }
        problems
            .contains(pos)
            .then(|| crate::screens::block_inner(problems))
    }

    /// Wheel scroll moves the caret a line at a time.
    pub(crate) fn on_scroll(&mut self, up: bool) {
        if up {
            self.doc().move_up();
        } else {
            self.doc().move_down();
        }
    }

    /// Draw the editor.
    pub(crate) fn view(&self, theme: &Theme, _tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [tabs, body, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);

        frame.render_widget(
            Tabs::new(self.files.iter().map(|(n, _)| *n))
                .selected(Some(self.active))
                .divider(" ")
                .style(theme.body())
                .highlight_style(theme.selection()),
            tabs,
        );

        let [editor_a, problems] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(34)]).areas(body);

        let (name, doc) = &self.files[self.active];
        // Frame the pane, then a real line-number gutter in front of the
        // editor. The gutter column width is fixed by the largest line
        // number (the same whether scrolled or not), so derive the text
        // rect once, compute the caret-visible scroll against it, then
        // render the gutter starting at the first *visible* line so the
        // numbers track the scrolled buffer — no gutter/content desync, and
        // the caret is never scrolled off-screen (`scroll_into_view`).
        let eblock = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Line::from(format!(" {name} ")).style(theme.accent_text()))
            .border_style(theme.border_focused())
            .style(theme.body());
        let ia = eblock.inner(editor_a);
        frame.render_widget(eblock, editor_a);
        let rows = doc.row_count();
        let text_rect = LineNumberGutter::new(1, rows).min_number_width(3).inner(ia);
        let s = doc.scroll_into_view(self.scroll.get(), (text_rect.width, text_rect.height), 3);
        self.scroll.set(s);
        let gutter = LineNumberGutter::new(s.0 as u64 + 1, rows.saturating_sub(s.0))
            .style(theme.caption())
            .min_number_width(3);
        frame.render_widget(gutter, ia);
        let ov = self.overlay(theme);
        frame.render_widget(
            Editor::new(doc)
                .focused(true)
                .scroll(s)
                .syntax(&ov)
                .style(theme.body())
                .focus_style(theme.border_focused())
                .cursor_style(Style::new().fg(theme.base).bg(theme.accent)),
            text_rect,
        );

        let pitems: Vec<Line> = PROBLEMS
            .iter()
            .map(|(ln, err, msg)| {
                let (glyph, col) = if *err {
                    ('✖', theme.err)
                } else {
                    ('⚠', theme.warn)
                };
                Line::from(vec![
                    format!("{glyph} ").fg(col),
                    format!("{ln}:").fg(theme.dim),
                    format!(" {msg}").fg(theme.text),
                ])
            })
            .collect();
        frame.render_widget(
            List::new(pitems).style(theme.body()).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(Line::from(" Problems ").style(theme.caption()))
                    .border_style(theme.border())
                    .style(theme.body()),
            ),
            problems,
        );

        let (row, col) = doc.cursor();
        frame.render_widget(
            StatusBar::new()
                .left(Line::from(format!(" {name} — rust ")).style(theme.caption()))
                .center(
                    Line::from("type · arrows move caret · PgUp/PgDn switch file")
                        .style(theme.caption()),
                )
                .right(
                    Line::from(format!(" Ln {}, Col {} · UTF-8 ", row + 1, col + 1))
                        .style(theme.caption()),
                )
                .style(Style::new().fg(theme.dim).bg(theme.raised)),
            foot,
        );
    }
}
