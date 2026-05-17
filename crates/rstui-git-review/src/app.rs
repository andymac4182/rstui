//! The `GitReview` [`App`] — the Elm loop: state is owned here, `view` is a
//! pure projection of it, `update` is the only mutator, and every `git`
//! invocation is a [`Cmd::perform`] effect that runs off the render loop.
//!
//! Layout/diff/graph/filter are all plain caller-owned model state the pure
//! `view` reads; the scroll/offsets are pure functions of the selection and
//! the caret (no stored offsets, no interior mutability) — only the diff's
//! vertical scroll is genuine independent user state.

use std::cell::Cell;
use std::path::PathBuf;

use rstui_core::{
    Constraint, Event, Frame, KeyCode, KeyModifiers, Layout, Line, MouseButton, MouseEventKind,
    Position, Rect, Span, Style, TextArea,
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
    /// The history list owns `↑/↓`/`j`/`k`.
    History,
    /// The diff owns `↑/↓`/`PgUp`/`PgDn` (scroll the patch).
    Detail,
}

/// Where the history list sits relative to the detail pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Orient {
    /// History on the left, detail on the right (a tall commit column).
    Left,
    /// History on top, detail below (a wide commit strip).
    Top,
}

/// The rects the last frame laid out, captured by `view` so the mouse
/// reducer hit-tests *what was rendered* (a real terminal does not always
/// send an initial resize, so a guessed size mis-places every click — the
/// same `Cell<Geom>` discipline the kitchen-sink uses).
#[derive(Debug, Clone, Copy)]
struct Geom {
    /// The body (everything above the status row).
    body: Rect,
    /// The history pane (outer, including its border).
    list: Rect,
    /// The detail pane (outer, including its border).
    detail: Rect,
    /// `true` when the split is vertical (history on top).
    vertical: bool,
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
    /// `git log` rows (commit + connector rows, newest first).
    rows: Vec<crate::LogRow>,
    /// Selected commit *ordinal* within the currently visible (filtered) set.
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
    /// `git log --graph` ASCII DAG on (the visual commit tree).
    graph: bool,
    /// The diff is rendered side-by-side instead of unified.
    diff_split: bool,
    /// History pane position.
    orient: Orient,
    /// History pane size as a percent of the body (resizable, clamped).
    split_pct: u16,
    /// A divider drag is in progress (mouse held on the split boundary).
    resizing: bool,
    /// The geometry the last frame drew, for mouse hit-testing. `None`
    /// until the first frame (no mouse can arrive before then).
    geom: Cell<Option<Geom>>,
    /// Case-insensitive commit filter (empty = show everything).
    filter: String,
    /// The filter input row currently owns the keyboard.
    filtering: bool,
    editor: TextArea,
    edit_path: Option<String>,
    edit_dirty: bool,
    help: bool,
    /// A transient one-line message (a save result, a soft error).
    status: String,
    /// A fatal load error — when set with no rows, the whole body is the
    /// error panel (graceful degrade outside a repo / when `git` is absent).
    error: Option<String>,
}

/// Everything that can happen: user intents from
/// [`on_event`](App::on_event) and the results of `git` [`Cmd`]s.
#[derive(Debug)]
pub enum Msg {
    /// A `git log` (history) load resolved.
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
    /// A mouse event (kind + cell position), hit-tested in
    /// [`update`](App::update) against the geometry `view` recorded.
    Mouse(MouseEventKind, Position),
}

impl GitReview {
    /// Build the app for `config` (nothing loads until
    /// [`init`](App::init)).
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            repo: config.repo,
            rev: config.rev,
            rows: Vec::new(),
            sel: 0,
            branch: "?".to_owned(),
            diff: String::new(),
            detail_for: None,
            diff_scroll: 0,
            files: Vec::new(),
            mode: Mode::Review,
            focus: Focus::History,
            graph: true,
            diff_split: false,
            orient: Orient::Left,
            split_pct: 34,
            resizing: false,
            geom: Cell::new(None),
            filter: String::new(),
            filtering: false,
            editor: TextArea::new(),
            edit_path: None,
            edit_dirty: false,
            help: false,
            status: String::new(),
            error: None,
        }
    }

    /// Does `c` match the active filter (case-insensitive, any of
    /// sha/short/subject/author/date)?
    fn matches(&self, c: &Commit) -> bool {
        if self.filter.is_empty() {
            return true;
        }
        let q = self.filter.to_lowercase();
        c.short.to_lowercase().contains(&q)
            || c.sha.to_lowercase().contains(&q)
            || c.subject.to_lowercase().contains(&q)
            || c.author.to_lowercase().contains(&q)
            || c.date.contains(&q)
    }

    /// Row indices of the visible commits (commit rows passing the filter),
    /// in display order — the spine every selection/nav computation uses.
    fn visible(&self) -> Vec<usize> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.commit.as_ref().is_some_and(|c| self.matches(c)))
            .map(|(i, _)| i)
            .collect()
    }

    /// The currently selected commit, if any.
    fn current(&self) -> Option<&Commit> {
        let vis = self.visible();
        vis.get(self.sel)
            .and_then(|&i| self.rows[i].commit.as_ref())
    }

    /// Re-run `git log` (history) off the render loop, honoring `graph`.
    fn reload_history(&self) -> Cmd<Msg> {
        let repo = self.repo.clone();
        let rev = self.rev.clone();
        let graph = self.graph;
        Cmd::perform(move || Msg::Loaded(crate::load(&repo, rev.as_deref(), graph)))
    }

    /// Load the selected commit's patch + changed-files list off the render
    /// loop. Cleared eagerly so the UI shows "loading" until results arrive.
    fn reload_detail(&mut self) -> Cmd<Msg> {
        self.diff.clear();
        self.files.clear();
        self.diff_scroll = 0;
        let Some(sha) = self.current().map(|c| c.sha.clone()) else {
            self.detail_for = None;
            return Cmd::none();
        };
        self.detail_for = Some(sha.clone());
        let repo_a = self.repo.clone();
        let repo_b = self.repo.clone();
        let sha_a = sha.clone();
        let sha_b = sha;
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

    /// Move the commit selection by `delta` (over the visible set) and
    /// reload its detail.
    fn select(&mut self, delta: isize) -> Cmd<Msg> {
        let n = self.visible().len();
        if n == 0 {
            return Cmd::none();
        }
        let next = (self.sel as isize + delta).clamp(0, n as isize - 1) as usize;
        if next == self.sel {
            return Cmd::none();
        }
        self.sel = next;
        self.reload_detail()
    }

    /// Re-clamp the selection after the visible set changed (a filter edit)
    /// and reload the now-selected commit.
    fn after_filter_change(&mut self) -> Cmd<Msg> {
        let n = self.visible().len();
        self.sel = if n == 0 { 0 } else { self.sel.min(n - 1) };
        self.reload_detail()
    }

    /// Handle one key press given the current mode/overlay.
    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Cmd<Msg> {
        // Ctrl+C always quits, every mode (the universal terminal reflex).
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            return Cmd::quit();
        }
        if self.help {
            self.help = false; // Any key dismisses the cheat-sheet.
            return Cmd::none();
        }
        if self.filtering {
            return self.on_key_filter(code);
        }
        match self.mode {
            Mode::Edit => self.on_key_edit(code, mods),
            Mode::Review => self.on_key_review(code),
        }
    }

    /// Keys while the filter input row is focused.
    fn on_key_filter(&mut self, code: KeyCode) -> Cmd<Msg> {
        match code {
            KeyCode::Esc => {
                self.filter.clear();
                self.filtering = false;
                self.after_filter_change()
            }
            KeyCode::Enter => {
                self.filtering = false;
                Cmd::none()
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.after_filter_change()
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.after_filter_change()
            }
            _ => Cmd::none(),
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
            KeyCode::Char('/') => {
                self.filtering = true;
                self.status = "filter: type to narrow · Enter keep · Esc clear".to_owned();
            }
            KeyCode::Char('s') => self.diff_split = !self.diff_split,
            KeyCode::Char('t') => {
                self.orient = match self.orient {
                    Orient::Left => Orient::Top,
                    Orient::Top => Orient::Left,
                };
            }
            KeyCode::Char('-') => self.split_pct = self.split_pct.saturating_sub(4).max(15),
            KeyCode::Char('=') | KeyCode::Char('+') => {
                self.split_pct = (self.split_pct + 4).min(75);
            }
            KeyCode::Char('\\') => {
                self.graph = !self.graph;
                self.status = if self.graph {
                    "graph tree on".to_owned()
                } else {
                    "graph tree off".to_owned()
                };
                return self.reload_history();
            }
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::History => Focus::Detail,
                    Focus::Detail => Focus::History,
                };
            }
            KeyCode::Char('e') => {
                if let Some((_, path)) = self.files.first() {
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
                Focus::History => return self.select(1),
                Focus::Detail => self.diff_scroll = self.diff_scroll.saturating_add(1),
            },
            KeyCode::Up | KeyCode::Char('k') => match self.focus {
                Focus::History => return self.select(-1),
                Focus::Detail => self.diff_scroll = self.diff_scroll.saturating_sub(1),
            },
            KeyCode::PageDown => self.diff_scroll = self.diff_scroll.saturating_add(15),
            KeyCode::PageUp => self.diff_scroll = self.diff_scroll.saturating_sub(15),
            KeyCode::Home if self.focus == Focus::Detail => self.diff_scroll = 0,
            _ => {}
        }
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
    pub fn graph() -> Style {
        Style::new().fg(Color::Indexed(6))
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

    /// The history pane: the `git log --graph` DAG (or a plain/filtered
    /// list), the selected commit highlighted.
    /// The history as `(commit-ordinal, line)` display rows + the selected
    /// display index + the visible-commit count. The single source both
    /// `view_history` (to render) and the mouse reducer (to map a click row
    /// back to a commit) read, so they can never disagree.
    fn history_lines(&self) -> (Vec<(Option<usize>, Line<'static>)>, usize, usize) {
        let vis = self.visible();
        if self.graph && self.filter.is_empty() {
            // Every physical row, art included; a connector row maps to no
            // commit (`None`) so a click on it selects nothing.
            let mut out = Vec::with_capacity(self.rows.len());
            let mut ord = 0usize;
            let mut sel_disp = 0usize;
            for (i, row) in self.rows.iter().enumerate() {
                match &row.commit {
                    Some(c) => {
                        if ord == self.sel {
                            sel_disp = i;
                        }
                        let this = ord;
                        ord += 1;
                        let subj: String = c.subject.chars().take(64).collect();
                        out.push((
                            Some(this),
                            Line::from(vec![
                                Span::styled(format!("{} ", row.art), palette::graph()),
                                Span::styled(format!("{} ", c.short), palette::accent()),
                                Span::raw(subj),
                            ]),
                        ));
                    }
                    None => {
                        out.push((None, Line::styled(row.art.clone(), palette::graph())));
                    }
                }
            }
            (out, sel_disp, vis.len())
        } else {
            // Plain (graph off, or a filter is narrowing): one row per
            // visible commit, no art; display index == commit ordinal.
            let out: Vec<(Option<usize>, Line<'static>)> = vis
                .iter()
                .enumerate()
                .map(|(ord, &i)| {
                    let c = self.rows[i].commit.as_ref().expect("visible ⇒ commit");
                    let subj: String = c.subject.chars().take(64).collect();
                    (
                        Some(ord),
                        Line::from(vec![
                            Span::styled(format!("{} ", c.short), palette::accent()),
                            Span::styled(format!("{} ", c.date), palette::dim()),
                            Span::raw(subj),
                        ]),
                    )
                })
                .collect();
            let sel_disp = self.sel.min(out.len().saturating_sub(1));
            (out, sel_disp, vis.len())
        }
    }

    fn view_history(&self, frame: &mut Frame<'_>, area: Rect) {
        let (rows, sel_disp, vis_len) = self.history_lines();
        let lines: Vec<Line> = rows.into_iter().map(|(_, l)| l).collect();
        let mut title = format!(" Commits {vis_len} · {}", self.branch);
        if !self.filter.is_empty() {
            title.push_str(&format!(" · /{}", self.filter));
        } else if self.graph {
            title.push_str(" · tree");
        }
        title.push(' ');
        frame.render_widget(
            List::new(lines)
                .selected(Some(sel_disp))
                .offset(sel_disp.saturating_sub(3))
                .highlight_style(palette::selection())
                .block(self.pane(title, self.focus == Focus::History && !self.filtering)),
            area,
        );
    }

    /// Map a clicked cell to a visible-commit ordinal, using the same
    /// display rows + offset `view_history` rendered (the list pane's inner
    /// area is its rect inset by the 1-cell border).
    fn commit_at(&self, list: Rect, pos: Position) -> Option<usize> {
        let inner_top = list.y + 1;
        if pos.x <= list.x
            || pos.x >= list.right().saturating_sub(1)
            || pos.y < inner_top
            || pos.y >= list.bottom().saturating_sub(1)
        {
            return None;
        }
        let (rows, sel_disp, _) = self.history_lines();
        let offset = sel_disp.saturating_sub(3);
        let disp = offset + usize::from(pos.y - inner_top);
        rows.get(disp).and_then(|&(ord, _)| ord)
    }

    /// Handle one mouse event against the geometry `view` recorded.
    fn on_mouse(&mut self, kind: MouseEventKind, pos: Position) -> Cmd<Msg> {
        let Some(g) = self.geom.get() else {
            return Cmd::none();
        };
        // The split boundary: a 3-cell-wide hot zone straddling the seam
        // between the two panes (forgiving to grab).
        let on_divider = if g.vertical {
            pos.x >= g.body.x && pos.x < g.body.right() && g.list.bottom().abs_diff(pos.y) <= 1
        } else {
            pos.y >= g.body.y && pos.y < g.body.bottom() && g.list.right().abs_diff(pos.x) <= 1
        };
        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if on_divider {
                    self.resizing = true;
                } else if g.list.contains(pos) {
                    self.focus = Focus::History;
                    if let Some(ord) = self.commit_at(g.list, pos) {
                        if ord != self.sel {
                            self.sel = ord;
                            return self.reload_detail();
                        }
                    }
                } else if g.detail.contains(pos) {
                    self.focus = Focus::Detail;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.resizing => {
                let pct = if g.vertical {
                    u32::from(pos.y.saturating_sub(g.body.y)) * 100
                        / u32::from(g.body.height.max(1))
                } else {
                    u32::from(pos.x.saturating_sub(g.body.x)) * 100 / u32::from(g.body.width.max(1))
                };
                self.split_pct = (pct as u16).clamp(15, 75);
            }
            MouseEventKind::Up(MouseButton::Left) => self.resizing = false,
            MouseEventKind::ScrollDown => {
                if g.detail.contains(pos) {
                    self.diff_scroll = self.diff_scroll.saturating_add(3);
                } else if g.list.contains(pos) {
                    return self.select(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if g.detail.contains(pos) {
                    self.diff_scroll = self.diff_scroll.saturating_sub(3);
                } else if g.list.contains(pos) {
                    return self.select(-1);
                }
            }
            _ => {}
        }
        Cmd::none()
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
                let subj: String = c.subject.chars().take(56).collect();
                let kind = if self.diff_split { "◫" } else { "≡" };
                format!(" {kind} {} · {} — {} ", c.short, c.date, subj)
            }
            None => " (no commit) ".to_owned(),
        };
        let block = self.pane(title, self.focus == Focus::Detail);
        if self.diff.is_empty() {
            let msg = if self.current().is_some() {
                "loading patch…"
            } else if self.filter.is_empty() {
                "no commits to review"
            } else {
                "no commits match the filter"
            };
            frame.render_widget(Paragraph::new(msg).style(palette::dim()).block(block), area);
        } else {
            let d = Diff::new(self.diff.as_str())
                .syntax(true)
                .scroll(self.diff_scroll);
            let d = if self.diff_split { d.side_by_side() } else { d };
            frame.render_widget(d.block(block), area);
        }
    }

    /// The bottom status strip.
    fn view_status(&self, frame: &mut Frame<'_>, area: Rect) {
        let repo = self.repo.display().to_string();
        let left = Line::styled(format!(" {repo} · ⎇ {} ", self.branch), palette::accent());
        let center: Line = if self.filtering {
            Line::styled(format!(" filter: {}_ ", self.filter), palette::good())
        } else if !self.status.is_empty() {
            Line::styled(format!(" {} ", self.status), palette::good())
        } else if self.mode == Mode::Edit {
            Line::styled("type to edit · Ctrl-S save · Esc back", palette::dim())
        } else {
            Line::styled(
                "[ ]: commit · s: side-by-side · t: top/left · \\: tree · /: filter · ?: help",
                palette::dim(),
            )
        };
        let n = self.visible().len();
        let pos = if n == 0 {
            " 0/0 ".to_owned()
        } else {
            let mode = if self.mode == Mode::Edit {
                "EDIT"
            } else {
                "REVIEW"
            };
            let split = if self.diff_split { " ◫" } else { "" };
            let tree = if self.graph { " ⫶" } else { "" };
            format!(" {}/{n} · {mode}{split}{tree} ", self.sel + 1)
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
        self.reload_history()
    }

    fn on_event(&self, event: Event) -> Option<Msg> {
        if let Some(m) = event.as_mouse() {
            return Some(Msg::Mouse(m.kind, m.position));
        }
        let key = event.as_key_press()?;
        Some(Msg::Key(key.code, key.modifiers))
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Loaded(Ok(loaded)) => {
                self.rows = loaded.rows;
                self.branch = loaded.branch;
                let n = self.visible().len();
                self.sel = if n == 0 { 0 } else { self.sel.min(n - 1) };
                self.error = None;
                self.reload_detail()
            }
            Msg::Loaded(Err(e)) => {
                self.rows.clear();
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
            Msg::Mouse(kind, pos) => self.on_mouse(kind, pos),
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
        if self.rows.is_empty() {
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

        // The resizable, re-orientable split (Layout clamps an oversized
        // length to the area, so this is total at any terminal size).
        let pct = u32::from(self.split_pct);
        let [list_a, detail_a] = match self.orient {
            Orient::Left => {
                let w = ((u32::from(body.width) * pct / 100) as u16).clamp(8, 90);
                Layout::horizontal([Constraint::Length(w), Constraint::Fill(1)]).areas(body)
            }
            Orient::Top => {
                let h = ((u32::from(body.height) * pct / 100) as u16).clamp(3, 40);
                Layout::vertical([Constraint::Length(h), Constraint::Fill(1)]).areas(body)
            }
        };
        // Record exactly what this frame laid out so the mouse reducer
        // hit-tests the real geometry, not a guessed size.
        self.geom.set(Some(Geom {
            body,
            list: list_a,
            detail: detail_a,
            vertical: self.orient == Orient::Top,
        }));
        self.view_history(frame, list_a);
        self.view_detail(frame, detail_a);
        self.view_status(frame, status);

        if self.help {
            let entries = [
                HelpEntry::new(["[", "]"], "Previous / next commit (or p / n)"),
                HelpEntry::new(["j", "k"], "Move selection / scroll the patch"),
                HelpEntry::new(["g", "G"], "Jump to newest / oldest commit"),
                HelpEntry::new(["Tab"], "Switch focus: history ⇄ patch"),
                HelpEntry::new(["s"], "Toggle side-by-side / unified diff"),
                HelpEntry::new(["t"], "Move history pane: left ⇄ top"),
                HelpEntry::new(["-", "="], "Resize the history / diff split"),
                HelpEntry::new(["\\"], "Toggle the visual commit tree (graph)"),
                HelpEntry::new(["/"], "Filter commits (Enter keep · Esc clear)"),
                HelpEntry::new(["e"], "Edit the commit's first changed file"),
                HelpEntry::new(["Ctrl", "S"], "Save the edited file (Edit mode)"),
                HelpEntry::new(
                    ["Mouse"],
                    "Click a commit · drag the pane border to resize · wheel scrolls",
                ),
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
