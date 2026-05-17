//! The `GitReview` [`App`] — the Elm loop: state is owned here, `view` is a
//! pure projection of it, `update` is the only mutator, and every `git`
//! invocation is a [`Cmd::perform`] effect that runs off the render loop.
//!
//! The scroll/offsets are deliberately *pure functions of the selection and
//! the caret* computed in `view` (the commit list follows `sel`, the editor
//! follows the cursor) — no stored offsets, no interior mutability — except
//! the diff's vertical scroll, which is genuine independent user state.

use std::path::PathBuf;

use rstui_core::{
    Constraint, Event, Frame, KeyCode, KeyModifiers, Layout, Line, Rect, Span, Style, TextArea,
};
use rstui_runtime::{App, Cmd};
use rstui_widgets::{
    Block, BorderType, Diff, Editor, HelpEntry, HelpOverlay, LineNumberGutter, List, Paragraph,
    StatusBar,
};

use crate::{Commit, Config, Loaded};

/// Whether the detail pane is reviewing the diff or editing a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Browsing commits and reading the selected commit's patch.
    Review,
    /// Editing a working-tree file in the embedded editor.
    Edit,
}

/// Which pane consumes vertical-motion keys in [`Mode::Review`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    /// The commit list owns `↑/↓`/`j`/`k`.
    Commits,
    /// The diff owns `↑/↓`/`PgUp`/`PgDn` (scroll the patch).
    Detail,
}

/// The full-screen git history review + code editing application.
///
/// All state is owned here and mutated only in [`update`](App::update);
/// [`view`](App::view) only reads it. Construct with [`GitReview::new`] and
/// run via [`crate::run`], or drive headlessly with
/// [`Harness`](rstui_runtime::Harness).
pub struct GitReview {
    repo: PathBuf,
    rev: Option<String>,
    commits: Vec<Commit>,
    sel: usize,
    branch: String,
    /// The selected commit's patch text (empty while a load is in flight).
    diff: String,
    /// The sha `diff`/`files` belong to, so a stale async result is ignored.
    detail_for: Option<String>,
    diff_scroll: u16,
    files: Vec<(String, String)>,
    mode: Mode,
    focus: Focus,
    editor: TextArea,
    edit_path: Option<String>,
    edit_dirty: bool,
    help: bool,
    /// A transient one-line message (a save result, a soft error).
    status: String,
    /// A fatal load error — when set with no commits, the whole body is the
    /// error panel (graceful degrade outside a repo / when `git` is absent).
    error: Option<String>,
}

/// Everything that can happen: user intents from
/// [`on_event`](App::on_event) and the results of `git` [`Cmd`]s.
#[derive(Debug)]
pub enum Msg {
    /// The initial `git log` + branch load resolved.
    Loaded(Result<Loaded, String>),
    /// `git show -p <sha>` resolved for `sha`.
    Diff {
        /// The commit the patch is for (ignored if no longer selected).
        sha: String,
        /// The patch text, or git's error.
        res: Result<String, String>,
    },
    /// `git show --name-status <sha>` resolved for `sha`.
    Files {
        /// The commit the file list is for.
        sha: String,
        /// `(status, path)` rows, or git's error.
        res: Result<Vec<(String, String)>, String>,
    },
    /// A working-tree file was read for editing.
    Opened {
        /// The repo-relative path that was opened.
        path: String,
        /// The file contents, or the read error.
        res: Result<String, String>,
    },
    /// A save finished: `Ok(path)` or `Err(message)`.
    Saved(Result<String, String>),
    /// A translated key press (interpreted in [`update`](App::update), the
    /// only mutator, since what a key means depends on mode/focus).
    Key(KeyCode, KeyModifiers),
}

impl GitReview {
    /// Build the app for `config` (nothing loads until
    /// [`init`](App::init)).
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            repo: config.repo,
            rev: config.rev,
            commits: Vec::new(),
            sel: 0,
            branch: "?".to_owned(),
            diff: String::new(),
            detail_for: None,
            diff_scroll: 0,
            files: Vec::new(),
            mode: Mode::Review,
            focus: Focus::Commits,
            editor: TextArea::new(),
            edit_path: None,
            edit_dirty: false,
            help: false,
            status: String::new(),
            error: None,
        }
    }

    /// The currently selected commit, if any.
    fn current(&self) -> Option<&Commit> {
        self.commits.get(self.sel)
    }

    /// Load the selected commit's patch + changed-files list off the render
    /// loop. Cleared eagerly so the UI shows "loading" until results arrive.
    fn reload_detail(&mut self) -> Cmd<Msg> {
        self.diff.clear();
        self.files.clear();
        self.diff_scroll = 0;
        let Some(commit) = self.current() else {
            self.detail_for = None;
            return Cmd::none();
        };
        let sha = commit.sha.clone();
        self.detail_for = Some(sha.clone());
        let repo_a = self.repo.clone();
        let repo_b = self.repo.clone();
        let sha_a = sha.clone();
        let sha_b = sha.clone();
        Cmd::batch([
            Cmd::perform(move || Msg::Diff {
                sha: sha_a.clone(),
                res: crate::show(&repo_a, &sha_a),
            }),
            Cmd::perform(move || Msg::Files {
                sha: sha_b.clone(),
                res: crate::changed_files(&repo_b, &sha_b),
            }),
        ])
    }

    /// Move the commit selection by `delta` rows and reload its detail.
    fn select(&mut self, delta: isize) -> Cmd<Msg> {
        if self.commits.is_empty() {
            return Cmd::none();
        }
        let last = self.commits.len() - 1;
        let next = (self.sel as isize + delta).clamp(0, last as isize) as usize;
        if next == self.sel {
            return Cmd::none();
        }
        self.sel = next;
        self.reload_detail()
    }

    /// Handle one key press given the current mode/focus.
    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Cmd<Msg> {
        // Ctrl+C always quits, every mode (the universal terminal reflex).
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            return Cmd::quit();
        }
        if self.help {
            // Any key dismisses the cheat-sheet.
            self.help = false;
            return Cmd::none();
        }
        match self.mode {
            Mode::Edit => self.on_key_edit(code, mods),
            Mode::Review => self.on_key_review(code),
        }
    }

    /// Keys while editing a working-tree file.
    fn on_key_edit(&mut self, code: KeyCode, mods: KeyModifiers) -> Cmd<Msg> {
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('s') {
            let Some(path) = self.edit_path.clone() else {
                return Cmd::none();
            };
            let repo = self.repo.clone();
            let body = self.editor.lines().join("\n");
            return Cmd::perform(move || {
                Msg::Saved(crate::write_file(&repo, &path, &body).map(|()| path.clone()))
            });
        }
        match code {
            KeyCode::Esc => {
                self.mode = Mode::Review;
                self.status = "stopped editing".to_owned();
            }
            KeyCode::Char(c) => {
                self.editor.insert_char(c);
                self.edit_dirty = true;
            }
            KeyCode::Enter => {
                self.editor.insert_newline();
                self.edit_dirty = true;
            }
            KeyCode::Backspace => {
                self.editor.delete_backward();
                self.edit_dirty = true;
            }
            KeyCode::Left => {
                self.editor.move_left();
            }
            KeyCode::Right => {
                self.editor.move_right();
            }
            KeyCode::Up => {
                self.editor.move_up();
            }
            KeyCode::Down => {
                self.editor.move_down();
            }
            KeyCode::Home => self.editor.move_home(),
            KeyCode::End => self.editor.move_end(),
            KeyCode::PageUp => {
                self.editor.move_page_up(10);
            }
            KeyCode::PageDown => {
                self.editor.move_page_down(10);
            }
            _ => {}
        }
        Cmd::none()
    }

    /// Keys while reviewing commits/diffs.
    fn on_key_review(&mut self, code: KeyCode) -> Cmd<Msg> {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return Cmd::quit(),
            KeyCode::Char('?') => self.help = true,
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Commits => Focus::Detail,
                    Focus::Detail => Focus::Commits,
                };
            }
            KeyCode::Char('e') => {
                if let Some((_, path)) = self.files.first() {
                    let path = path.clone();
                    let repo = self.repo.clone();
                    let p = path.clone();
                    return Cmd::perform(move || Msg::Opened {
                        path: p.clone(),
                        res: crate::read_file(&repo, &p),
                    });
                }
                self.status = "this commit changed no editable file".to_owned();
            }
            KeyCode::Char(']') | KeyCode::Char('n') => return self.select(1),
            KeyCode::Char('[') | KeyCode::Char('p') => return self.select(-1),
            KeyCode::Char('g') => return self.select(isize::MIN / 2),
            KeyCode::Char('G') => return self.select(isize::MAX / 2),
            KeyCode::Down | KeyCode::Char('j') => match self.focus {
                Focus::Commits => return self.select(1),
                Focus::Detail => self.diff_scroll = self.diff_scroll.saturating_add(1),
            },
            KeyCode::Up | KeyCode::Char('k') => match self.focus {
                Focus::Commits => return self.select(-1),
                Focus::Detail => self.diff_scroll = self.diff_scroll.saturating_sub(1),
            },
            KeyCode::PageDown => self.diff_scroll = self.diff_scroll.saturating_add(15),
            KeyCode::PageUp => self.diff_scroll = self.diff_scroll.saturating_sub(15),
            KeyCode::Home if self.focus == Focus::Detail => self.diff_scroll = 0,
            _ => {}
        }
        // Never scroll past the patch text (Diff is total past the end, but
        // this keeps the scrollbar honest).
        let max = self.diff.lines().count() as u16;
        if self.diff_scroll > max {
            self.diff_scroll = max;
        }
        Cmd::none()
    }
}

/// A small, terminal-portable palette (the standard ANSI indices, valid at
/// every colour level).
mod palette {
    use rstui_core::{Color, Style};

    pub fn dim() -> Style {
        Style::new().fg(Color::Indexed(8))
    }
    pub fn accent() -> Style {
        Style::new().fg(Color::Indexed(4))
    }
    pub fn good() -> Style {
        Style::new().fg(Color::Indexed(2))
    }
    pub fn bad() -> Style {
        Style::new().fg(Color::Indexed(1))
    }
    pub fn selection() -> Style {
        Style::new().fg(Color::Indexed(0)).bg(Color::Indexed(4))
    }
}

impl GitReview {
    /// A framed pane block with `title`, highlighted when `focused`.
    fn pane<'t>(&self, title: impl Into<Line<'t>>, focused: bool) -> Block<'t> {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .title(title.into())
            .border_style(if focused {
                palette::accent()
            } else {
                palette::dim()
            })
    }

    /// The commit list pane.
    fn view_commits(&self, frame: &mut Frame<'_>, area: Rect) {
        let rows: Vec<Line> = self
            .commits
            .iter()
            .map(|c| {
                let subj: String = c.subject.chars().take(72).collect();
                Line::from(vec![
                    Span::styled(format!("{} ", c.short), palette::accent()),
                    Span::styled(format!("{} ", c.date), palette::dim()),
                    Span::raw(subj),
                ])
            })
            .collect();
        let title = format!(" Commits {} · {} ", self.commits.len(), self.branch);
        frame.render_widget(
            List::new(rows)
                .selected(Some(self.sel))
                // Pure scroll: keep the selection on screen with a 3-row lead,
                // for any pane height — no stored offset, no geometry.
                .offset(self.sel.saturating_sub(3))
                .highlight_style(palette::selection())
                .block(self.pane(title, self.focus == Focus::Commits)),
            area,
        );
    }

    /// The detail pane: the patch (Review) or the editor (Edit).
    fn view_detail(&self, frame: &mut Frame<'_>, area: Rect) {
        if self.mode == Mode::Edit {
            let path = self.edit_path.as_deref().unwrap_or("(file)");
            let dirty = if self.edit_dirty { " ●" } else { "" };
            let block = self.pane(format!(" {path}{dirty}  ·  Ctrl-S save · Esc back "), true);
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let gutter = LineNumberGutter::new(1, self.editor.row_count())
                .style(palette::dim())
                .min_number_width(3);
            let text_rect = gutter.inner(inner);
            frame.render_widget(gutter, inner);
            // Pure: the viewport follows the caret with a 4-row lead.
            let (crow, _) = self.editor.cursor();
            frame.render_widget(
                Editor::new(&self.editor)
                    .focused(true)
                    .scroll((crow.saturating_sub(4), 0))
                    .cursor_style(palette::selection()),
                text_rect,
            );
            return;
        }

        let title = match self.current() {
            Some(c) => {
                let subj: String = c.subject.chars().take(60).collect();
                format!(" {} · {} — {} ", c.short, c.date, subj)
            }
            None => " (no commit) ".to_owned(),
        };
        let block = self.pane(title, self.focus == Focus::Detail);
        if self.diff.is_empty() {
            let msg = self
                .current()
                .map(|_| "loading patch…")
                .unwrap_or("no commits to review");
            frame.render_widget(Paragraph::new(msg).style(palette::dim()).block(block), area);
        } else {
            frame.render_widget(
                Diff::new(self.diff.as_str())
                    .syntax(true)
                    .scroll(self.diff_scroll)
                    .block(block),
                area,
            );
        }
    }

    /// The bottom status strip.
    fn view_status(&self, frame: &mut Frame<'_>, area: Rect) {
        let repo = self.repo.display().to_string();
        let left = Line::styled(format!(" {repo} · ⎇ {} ", self.branch), palette::accent());
        let center: Line = if !self.status.is_empty() {
            Line::styled(format!(" {} ", self.status), palette::good())
        } else if self.mode == Mode::Edit {
            Line::styled("type to edit · Ctrl-S save · Esc back", palette::dim())
        } else {
            Line::styled(
                "[ / ]: prev/next commit · Tab: focus · e: edit file · ?: help · q: quit",
                palette::dim(),
            )
        };
        let pos = if self.commits.is_empty() {
            " 0/0 ".to_owned()
        } else {
            let mode = if self.mode == Mode::Edit {
                "EDIT"
            } else {
                "REVIEW"
            };
            format!(" {}/{} · {mode} ", self.sel + 1, self.commits.len())
        };
        frame.render_widget(
            StatusBar::new()
                .left(left)
                .center(center)
                .right(Line::styled(pos, palette::dim())),
            area,
        );
    }
}

impl App for GitReview {
    type Message = Msg;

    fn init(&mut self) -> Cmd<Msg> {
        let repo = self.repo.clone();
        let rev = self.rev.clone();
        Cmd::perform(move || Msg::Loaded(crate::load(&repo, rev.as_deref())))
    }

    fn on_event(&self, event: Event) -> Option<Msg> {
        let key = event.as_key_press()?;
        Some(Msg::Key(key.code, key.modifiers))
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Loaded(Ok(loaded)) => {
                self.commits = loaded.commits;
                self.branch = loaded.branch;
                self.sel = 0;
                self.error = None;
                self.reload_detail()
            }
            Msg::Loaded(Err(e)) => {
                self.error = Some(e);
                Cmd::none()
            }
            Msg::Diff { sha, res } => {
                if self.detail_for.as_deref() == Some(sha.as_str()) {
                    self.diff = res.unwrap_or_else(|e| format!("(patch unavailable: {e})"));
                    self.diff_scroll = 0;
                }
                Cmd::none()
            }
            Msg::Files { sha, res } => {
                if self.detail_for.as_deref() == Some(sha.as_str()) {
                    self.files = res.unwrap_or_default();
                }
                Cmd::none()
            }
            Msg::Opened { path, res } => {
                match res {
                    Ok(text) => {
                        self.editor = TextArea::from_value(text);
                        self.edit_path = Some(path.clone());
                        self.edit_dirty = false;
                        self.mode = Mode::Edit;
                        self.status = format!("editing {path}");
                    }
                    Err(e) => self.status = e,
                }
                Cmd::none()
            }
            Msg::Saved(Ok(path)) => {
                self.edit_dirty = false;
                self.status = format!("saved {path}");
                Cmd::none()
            }
            Msg::Saved(Err(e)) => {
                self.status = format!("save failed: {e}");
                Cmd::none()
            }
            Msg::Key(code, mods) => self.on_key(code, mods),
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        if area.width < 8 || area.height < 4 {
            return; // Degenerate terminal: a safe no-op, never a panic.
        }
        let [body, status] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

        // Fatal load failure with nothing to show: the body is one panel.
        if self.commits.is_empty() {
            if let Some(err) = &self.error {
                frame.render_widget(
                    Paragraph::new(format!(
                        "Cannot review this directory.\n\n{err}\n\n\
                         Open rstui-git-review inside a git working tree, or pass one:\n  \
                         rstui-git-review path/to/repo\n\nPress q to quit."
                    ))
                    .style(palette::bad())
                    .block(self.pane(" git-review ", true)),
                    body,
                );
                self.view_status(frame, status);
                return;
            }
        }

        let list_w = ((u32::from(area.width) * 32 / 100) as u16).clamp(24, 52);
        let [list_a, detail_a] =
            Layout::horizontal([Constraint::Length(list_w), Constraint::Fill(1)]).areas(body);
        self.view_commits(frame, list_a);
        self.view_detail(frame, detail_a);
        self.view_status(frame, status);

        if self.help {
            let entries = [
                HelpEntry::new(["[", "]"], "Previous / next commit"),
                HelpEntry::new(["j", "k"], "Move selection / scroll the patch"),
                HelpEntry::new(["g", "G"], "Jump to newest / oldest commit"),
                HelpEntry::new(["Tab"], "Switch focus: commit list ⇄ patch"),
                HelpEntry::new(["e"], "Edit the commit's first changed file"),
                HelpEntry::new(["Ctrl", "S"], "Save the edited file (Edit mode)"),
                HelpEntry::new(["Esc"], "Leave Edit / close this help"),
                HelpEntry::new(["q"], "Quit"),
            ];
            frame.render_widget(
                HelpOverlay::new(&entries)
                    .block(self.pane(" Keys ", true))
                    .key_style(palette::accent())
                    .style(Style::new()),
                area,
            );
        }
    }
}
