//! A code-editor experience: a file [`Tabs`] strip, a real multi-line
//! [`Editor`] over a caller-owned [`TextArea`] (type, arrows move the caret,
//! Enter splits lines), a [`List`] of problems, and a [`StatusBar`] with the
//! live cursor position. `PgUp/PgDn` switch files.

use rstui_core::{
    Constraint, KeyCode, Layout, Line, Position, Rect, Style, TextArea, stylize::Stylize,
};
use rstui_runtime::Frame;
use rstui_widgets::{Block, BorderType, Editor, LineNumberGutter, List, StatusBar, Tabs};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// A problems-pane entry: line, severity (`true` = error), message.
const PROBLEMS: [(u32, bool, &str); 4] = [
    (3, false, "unused import: `Rect`"),
    (12, true, "mismatched types: expected u16"),
    (28, false, "function is never used: `helper`"),
    (41, true, "cannot borrow `*frame` as mutable twice"),
];

/// The editor's caller-owned state: one [`TextArea`] per open file.
#[derive(Debug)]
pub(crate) struct State {
    files: Vec<(&'static str, TextArea)>,
    active: usize,
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
                (
                    "main.rs",
                    open("fn main() {\n    let app = KitchenSink::new();\n    run_app(app);\n}"),
                ),
                (
                    "lib.rs",
                    open("//! kitchen sink\npub struct KitchenSink;\n"),
                ),
                (
                    "notes.md",
                    open("# TODO\n- type in here\n- arrows move the caret\n"),
                ),
            ],
            active: 0,
        }
    }

    fn doc(&mut self) -> &mut TextArea {
        &mut self.files[self.active].1
    }

    /// Typing edits the buffer; arrows move the caret; `PgUp/PgDn` switch
    /// files. There is no rail fall-back on `←` here — like a real editor,
    /// `←` is a caret move; use `Tab` / the rail to leave.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::PageUp => {
                self.active = self.active.saturating_sub(1);
            }
            KeyCode::PageDown => {
                self.active = (self.active + 1).min(self.files.len() - 1);
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
        // Frame the pane, then put a real line-number gutter in front of the
        // editor and render the buffer into the rect the gutter hands back.
        let eblock = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Line::from(format!(" {name} ")).style(theme.accent_text()))
            .border_style(theme.border_focused())
            .style(theme.body());
        let ia = eblock.inner(editor_a);
        frame.render_widget(eblock, editor_a);
        let gutter = LineNumberGutter::new(1, doc.row_count())
            .style(theme.caption())
            .min_number_width(3);
        let text_rect = gutter.inner(ia);
        frame.render_widget(gutter, ia);
        frame.render_widget(
            Editor::new(doc)
                .focused(true)
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
