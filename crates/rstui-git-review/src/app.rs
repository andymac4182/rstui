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
    Constraint, Event, Frame, KeyCode, KeyEvent, KeyModifiers, Layout, Line, MouseButton,
    MouseEventKind, Position, Rect, Span, Style, TextArea,
};
use rstui_keymap::{Action, Chord, Keymap, Keymaps};
use rstui_runtime::{App, Cmd};
use rstui_widgets::{
    Block, BorderType, Diff, Editor, HelpEntry, HelpOverlay, KeymapRow, KeymapView,
    LineNumberGutter, List, Paragraph, RowState, StatusBar,
};

use crate::{Commit, Config, Loaded};

/// `git-review`'s command surface as semantic [`Action`]s — every toggle and
/// command routes through [`Keymaps`], so all of them are remappable, shown
/// in the keymap panel, and overridable from a `RSTUI_KEYMAP` config file.
///
/// Pane-relative **motions** (`j`/`k`, `g`/`G`, arrows, `[`/`]`, page) stay
/// raw screen keys by design — ADR 0015 keeps the keymap shell-level, and
/// `Chord` folds letter case (Shift+g == g) so vim's case-sensitive `g`/`G`
/// could not be distinct actions anyway. The same boundary the kitchen
/// sink draws for arrows/typing.
const FILTER: Action = Action::Custom("git.filter");
const FOCUS: Action = Action::Custom("git.focus");
const EDIT: Action = Action::Custom("git.edit");
const SPLIT: Action = Action::Custom("git.split");
const ORIENT: Action = Action::Custom("git.orient");
const SHRINK: Action = Action::Custom("git.shrink");
const GROW: Action = Action::Custom("git.grow");
const GRAPH: Action = Action::Custom("git.graph");

/// `(action, label)` in keymap-panel display order — the single source the
/// app keymap is built from *and* the panel renders.
const COMMANDS: &[(Action, &str)] = &[
    (FILTER, "Filter commits"),
    (FOCUS, "Switch focus: history ⇄ patch"),
    (EDIT, "Edit the first changed file"),
    (SPLIT, "Toggle side-by-side diff"),
    (ORIENT, "History pane: left ⇄ top"),
    (SHRINK, "Shrink the history/diff split"),
    (GROW, "Grow the history/diff split"),
    (GRAPH, "Toggle the commit graph tree"),
    (Action::Drawer, "Keymap settings"),
    (Action::Help, "Help"),
    (Action::Quit, "Quit"),
];

/// The app's own keymap (no kitchen-sink leftovers): one named map built
/// from [`COMMANDS`], via [`Keymaps::from_maps`].
fn git_review_keymaps() -> Keymaps {
    let mut km = Keymap::new("git-review");
    km.bind(Action::Quit, &["q", "esc", "ctrl+c"]);
    km.bind(Action::Help, &["?"]);
    km.bind(Action::Drawer, &["ctrl+k"]);
    km.bind(FILTER, &["/"]);
    km.bind(FOCUS, &["tab"]);
    km.bind(EDIT, &["e"]);
    km.bind(SPLIT, &["s"]);
    km.bind(ORIENT, &["t"]);
    km.bind(SHRINK, &["-"]);
    km.bind(GROW, &["=", "+"]);
    km.bind(GRAPH, &["\\"]);
    Keymaps::from_maps(vec![km])
}

/// The display caps for a `keys_for` string: `"⌃K / :"` → `["⌃K", ":"]`,
/// the unbound sentinel `"—"` → `[]` (so the row reads disabled).
fn caps(keys: &str) -> Vec<String> {
    if keys == "—" {
        return Vec::new();
    }
    keys.split(" / ").map(str::to_owned).collect()
}

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
    /// The customisable keymap (one app-owned map; `RSTUI_KEYMAP` may load
    /// user overrides). Every command resolves through it.
    keymaps: Keymaps,
    /// A monotonic per-key counter — the deterministic clock
    /// [`Keymaps::resolve`]/[`Keymaps::expire`] need (this app has no
    /// animation tick; its map has no leader, so this only has to advance).
    tick: u64,
    /// The keymap settings panel ([`KeymapView`]) is open.
    keymap_panel: bool,
    /// The selected row in the keymap panel.
    km_sel: usize,
    /// The command armed for capture-to-rebind (the next key binds it).
    km_rebind: Option<Action>,
    /// A transient one-line message (a save result, a soft error).
    status: String,
    /// A fatal load error — when set with no rows, the whole body is the
    /// error panel (graceful degrade outside a repo / when `git` is absent).
    error: Option<String>,
    /// The active colour theme (any of the 36 gpui-component themes,
    /// resolved from `RSTUI_THEME` / the saved choice / the default).
    theme: crate::theme::GrTheme,
    /// Reusable theme-picker state, driven while [`picking`](Self::picking).
    theme_picker: rstui_theme::ThemePickerState,
    /// The theme to restore if the picker is cancelled with `Esc`.
    theme_restore: Option<crate::theme::GrTheme>,
    /// `true` while the theme picker overlay is open.
    picking: bool,
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
            keymaps: git_review_keymaps(),
            tick: 0,
            keymap_panel: false,
            km_sel: 0,
            km_rebind: None,
            status: String::new(),
            error: None,
            theme: crate::theme::startup_theme(),
            theme_picker: rstui_theme::ThemePickerState::new(),
            theme_restore: None,
            picking: false,
        }
    }

    /// Apply a user keymap choice: a built-in map **name** (only
    /// `"git-review"` here) or a path to a `RSTUI_KEYMAP` config file
    /// (`id = keys` lines — see `docs/keymaps.md`). Unknown name /
    /// unreadable file keeps the defaults; a typo never breaks your keys.
    /// The one seam `RSTUI_KEYMAP` flows through, mirroring `RSTUI_THEME`.
    #[must_use]
    pub fn with_keymap(mut self, name_or_path: &str) -> Self {
        if std::path::Path::new(name_or_path).is_file() {
            if let Ok(text) = std::fs::read_to_string(name_or_path) {
                self.keymaps.load_overrides(&text);
            }
        } else {
            self.keymaps.set_active(name_or_path);
        }
        self
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
    ///
    /// Routing: text-entry surfaces (the help dismiss, the keymap panel, the
    /// filter input, the editor) own their keys raw; in Review every
    /// **command** resolves through [`Keymaps`] (so it is remappable), and
    /// only pane-relative **motions** fall through raw — the ADR 0015
    /// shell-level boundary, the same one the kitchen sink draws.
    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Cmd<Msg> {
        // Ctrl+C always quits, every mode (the universal terminal reflex).
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            return Cmd::quit();
        }
        if self.help {
            self.help = false; // Any key dismisses the cheat-sheet.
            return Cmd::none();
        }
        if self.picking {
            return self.theme_picker_key(code);
        }
        if self.keymap_panel {
            return self.on_key_keymap(code, mods);
        }
        if self.filtering {
            return self.on_key_filter(code);
        }
        if self.mode == Mode::Edit {
            return self.on_key_edit(code, mods);
        }
        // Review: commands through the keymap, motions raw.
        self.tick = self.tick.wrapping_add(1);
        let ev = KeyEvent::new(code, mods);
        if let Some(action) = self.keymaps.resolve(&ev, self.tick) {
            return self.do_action(action);
        }
        if self.keymaps.armed() {
            return Cmd::none(); // a leader/prefix was pressed — swallow it
        }
        self.on_key_motion(code)
    }

    /// Perform a resolved command [`Action`] — the single place a Review
    /// binding takes effect, so every keymap/remap routes through one switch.
    fn do_action(&mut self, action: Action) -> Cmd<Msg> {
        match action {
            Action::Quit => return Cmd::quit(),
            Action::Help => self.help = true,
            Action::Drawer => {
                self.keymap_panel = true;
                self.km_sel = 0;
                self.km_rebind = None;
                self.status = "keymap: ↑↓ select · ⏎/r rebind · x disable · Esc close".to_owned();
            }
            FILTER => {
                self.filtering = true;
                self.status = "filter: type to narrow · Enter keep · Esc clear".to_owned();
            }
            FOCUS => {
                self.focus = match self.focus {
                    Focus::History => Focus::Detail,
                    Focus::Detail => Focus::History,
                };
            }
            SPLIT => self.diff_split = !self.diff_split,
            ORIENT => {
                self.orient = match self.orient {
                    Orient::Left => Orient::Top,
                    Orient::Top => Orient::Left,
                };
            }
            SHRINK => self.split_pct = self.split_pct.saturating_sub(4).max(6),
            GROW => self.split_pct = (self.split_pct + 4).min(94),
            GRAPH => {
                self.graph = !self.graph;
                self.status = if self.graph {
                    "graph tree on".to_owned()
                } else {
                    "graph tree off".to_owned()
                };
                return self.reload_history();
            }
            EDIT => {
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
            _ => {}
        }
        Cmd::none()
    }

    /// Live-preview the highlighted picker theme by adopting its palette.
    fn preview_theme(&mut self) {
        let next = self
            .theme_picker
            .selected_theme()
            .map(crate::theme::GrTheme::from_theme);
        if let Some(t) = next {
            self.theme = t;
        }
    }

    /// Keys while the theme picker is open (opened with `p` in the keymap
    /// panel): arrows preview, typing filters, `Enter` keeps + persists,
    /// `Esc` restores the prior palette.
    fn theme_picker_key(&mut self, code: KeyCode) -> Cmd<Msg> {
        match code {
            KeyCode::Esc => {
                if let Some(prev) = self.theme_restore.take() {
                    self.theme = prev;
                }
                self.picking = false;
            }
            KeyCode::Enter => {
                self.preview_theme();
                let name = self.theme.name.clone();
                let _ = rstui_theme::Theme::write_choice(crate::theme::theme_config_path(), &name);
                self.status = format!("theme saved → {name}");
                self.theme_restore = None;
                self.picking = false;
            }
            KeyCode::Up => {
                self.theme_picker.prev();
                self.preview_theme();
            }
            KeyCode::Down => {
                self.theme_picker.next();
                self.preview_theme();
            }
            KeyCode::Backspace => {
                self.theme_picker.pop_filter();
                self.preview_theme();
            }
            KeyCode::Char(c) => {
                self.theme_picker.push_filter(c);
                self.preview_theme();
            }
            _ => {}
        }
        Cmd::none()
    }

    /// The keymap settings panel ([`KeymapView`]): browse the live bindings
    /// and **capture a key to rebind** a command or disable it — proving the
    /// override path end to end, the same FSM the kitchen sink uses.
    fn on_key_keymap(&mut self, code: KeyCode, mods: KeyModifiers) -> Cmd<Msg> {
        // Armed: the next key *is* the new binding (Esc cancels).
        if let Some(act) = self.km_rebind.take() {
            if code == KeyCode::Esc {
                self.status = "rebind cancelled".to_owned();
            } else {
                let chord = Chord::from_event(&KeyEvent::new(code, mods));
                self.keymaps.set_override(act, chord.spec());
                self.status = format!("bound → {}", chord.display());
            }
            return Cmd::none();
        }
        let last = COMMANDS.len().saturating_sub(1);
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.keymap_panel = false,
            KeyCode::Down | KeyCode::Char('j') => self.km_sel = (self.km_sel + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.km_sel = self.km_sel.saturating_sub(1),
            KeyCode::Enter | KeyCode::Char('r') => {
                self.km_rebind = Some(COMMANDS[self.km_sel.min(last)].0);
                self.status = "press a key to bind — Esc cancels".to_owned();
            }
            KeyCode::Char('x') => {
                let act = COMMANDS[self.km_sel.min(last)].0;
                self.keymaps.set_override(act, "none");
                self.status = "disabled".to_owned();
            }
            KeyCode::Char('p') => {
                // Open the reusable theme picker; remember the current
                // palette so Esc can restore it.
                self.theme_restore = Some(self.theme.clone());
                self.picking = true;
                self.keymap_panel = false;
            }
            _ => {}
        }
        Cmd::none()
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

    /// Pane-relative **motions** while reviewing — raw screen keys by
    /// design (ADR 0015 keeps the keymap shell-level; `Chord` folds letter
    /// case so vim's `g`/`G` could not be distinct actions). Commands live
    /// in [`do_action`](Self::do_action); these only move the cursor/scroll.
    fn on_key_motion(&mut self, code: KeyCode) -> Cmd<Msg> {
        match code {
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

    /// The keymap settings panel rows, projected from the **live** keymap
    /// (so a switch or remap shows immediately) — the reducer owns the
    /// cursor and the capture FSM; [`KeymapView`] just draws this.
    fn keymap_rows(&self) -> Vec<KeymapRow<'static>> {
        let km = self.keymaps.effective();
        COMMANDS
            .iter()
            .enumerate()
            .map(|(i, &(action, label))| {
                let keys = km.keys_for(action);
                let state = if self.km_rebind == Some(action) {
                    RowState::Capturing
                } else if i == self.km_sel {
                    RowState::Selected
                } else if keys == "—" {
                    RowState::Disabled
                } else {
                    RowState::Normal
                };
                KeymapRow::new(label, caps(&keys))
                    .id(action.id())
                    .state(state)
            })
            .collect()
    }
}

impl GitReview {
    /// A framed pane block with `title`, highlighted when `focused`.
    fn pane<'t>(&self, title: impl Into<Line<'t>>, focused: bool) -> Block<'t> {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .title(title.into())
            .border_style(if focused {
                self.theme.accent()
            } else {
                self.theme.dim()
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
                        // Full subject — `List` clips the line to the pane's
                        // width, so widening the pane reveals more (a fixed
                        // cap here would defeat the resize).
                        out.push((
                            Some(this),
                            Line::from(vec![
                                Span::styled(format!("{} ", row.art), self.theme.graph()),
                                Span::styled(format!("{} ", c.short), self.theme.accent()),
                                Span::raw(c.subject.clone()),
                            ]),
                        ));
                    }
                    None => {
                        out.push((None, Line::styled(row.art.clone(), self.theme.graph())));
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
                    (
                        Some(ord),
                        Line::from(vec![
                            Span::styled(format!("{} ", c.short), self.theme.accent()),
                            Span::styled(format!("{} ", c.date), self.theme.dim()),
                            Span::raw(c.subject.clone()),
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
                .highlight_style(self.theme.selection())
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
                self.split_pct = (pct as u16).clamp(6, 94);
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
                .style(self.theme.dim())
                .min_number_width(3);
            let text_rect = gutter.inner(inner);
            frame.render_widget(gutter, inner);
            let (crow, _) = self.editor.cursor();
            frame.render_widget(
                Editor::new(&self.editor)
                    .focused(true)
                    .scroll((crow.saturating_sub(4), 0))
                    .cursor_style(self.theme.selection()),
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
            frame.render_widget(
                Paragraph::new(msg).style(self.theme.dim()).block(block),
                area,
            );
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
        let left = Line::styled(format!(" {repo} · ⎇ {} ", self.branch), self.theme.accent());
        let center: Line = if self.filtering {
            Line::styled(format!(" filter: {}_ ", self.filter), self.theme.good())
        } else if !self.status.is_empty() {
            Line::styled(format!(" {} ", self.status), self.theme.good())
        } else if self.mode == Mode::Edit {
            Line::styled("type to edit · Ctrl-S save · Esc back", self.theme.dim())
        } else {
            Line::styled(
                "[ ]: commit · s: side-by-side · t: top/left · \\: tree · /: filter · ?: help",
                self.theme.dim(),
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
                .right(Line::styled(pos, self.theme.dim())),
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
                    .style(self.theme.bad())
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
                // Body-relative bounds (not fixed 8..90): the divider can go
                // nearly full-width either way, always leaving ≥3 cells for
                // the other pane. `max(3)` keeps the clamp lo ≤ hi at any
                // size, so it is still total.
                let hi = body.width.saturating_sub(3).max(3);
                let w = ((u32::from(body.width) * pct / 100) as u16).clamp(3, hi);
                Layout::horizontal([Constraint::Length(w), Constraint::Fill(1)]).areas(body)
            }
            Orient::Top => {
                let hi = body.height.saturating_sub(2).max(2);
                let h = ((u32::from(body.height) * pct / 100) as u16).clamp(2, hi);
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
                    .key_style(self.theme.accent())
                    .style(Style::new()),
                area,
            );
        }

        if self.keymap_panel {
            let rows = self.keymap_rows();
            let header = format!(
                " {} · {} — every command, remappable",
                self.keymaps.active_name(),
                Keymaps::os_name()
            );
            let footer = if self.km_rebind.is_some() {
                "● press a key to bind — Esc cancels".to_owned()
            } else {
                "↑↓/jk select · ⏎/r rebind · x disable · p theme · Esc close".to_owned()
            };
            frame.render_widget(
                KeymapView::new(&rows)
                    .block(self.pane(" Keymap ", true))
                    .header(Line::styled(header, self.theme.accent()))
                    .footer(Line::styled(footer, self.theme.dim()))
                    .separator("")
                    .style(Style::new())
                    .label_style(Style::new())
                    .id_style(self.theme.dim())
                    .key_style(self.theme.accent())
                    .selected_style(self.theme.selection())
                    .capturing_style(self.theme.good())
                    .disabled_style(self.theme.dim()),
                area,
            );
        }

        if self.picking {
            // The whole reviewer is already painted in the highlighted
            // theme (live preview), so this panel previews it too.
            let a = frame.area();
            let w = ((u32::from(a.width) * 3 / 5) as u16)
                .clamp(28, 72)
                .min(a.width);
            let h = ((u32::from(a.height) * 7 / 10) as u16)
                .clamp(8, 30)
                .min(a.height);
            let rect = Rect::new(
                a.x + a.width.saturating_sub(w) / 2,
                a.y + a.height.saturating_sub(h) / 2,
                w,
                h,
            );
            frame.buffer_mut().set_style(rect, self.theme.base());
            let block = self.pane(format!(" Theme — {} ", self.theme.name), true);
            let inner = block.inner(rect);
            frame.render_widget(block, rect);
            frame.render_widget(
                rstui_theme::ThemePicker::new(&self.theme_picker)
                    .title("Browse · preview live")
                    .style(self.theme.base())
                    .highlight_style(self.theme.selection()),
                inner,
            );
        }
    }
}
