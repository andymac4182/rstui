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
    Constraint, DocSelection, Event, Frame, History, KeyCode, KeyEvent, KeyModifiers, Layout, Line,
    MouseButton, MouseEventKind, Position, Query, Rect, SelKind, Span, Style, TextArea,
};
use rstui_keymap::{Action, Capture, Chord, Dispatch, Keymap, Keymaps};
use rstui_runtime::{App, Cmd};
// ADR 0024: the code-editing widgets moved to `rstui-code`; the general
// chrome stays in `rstui-widgets`. git-review's behaviour is unchanged — it
// keeps lexing with the dependency-free Tier-0 `syntax::line_overlay`.
use rstui_code::{
    Changeset, Diff, DiffSyntaxCache, Editor, Language, LineNumberGutter, Outline, SymbolKind,
    outline, syntax,
};
use rstui_widgets::{
    Block, BorderType, HelpEntry, HelpOverlay, KeymapRow, KeymapView, List, Paragraph, RowState,
    StatusBar,
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
const THEME: Action = Action::Custom("git.theme");
/// Toggle the symbol/outline side panel (gap H) — editor symbols in Edit
/// mode, the commit's changed files in Review.
const OUTLINE: Action = Action::Custom("git.outline");
/// Open the find-in-content prompt (gap J) — searches the editor buffer in
/// Edit mode, the patch text in Review.
const SEARCH: Action = Action::Custom("git.search");

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
    (OUTLINE, "Toggle the symbol / outline panel"),
    (SEARCH, "Find in the file / patch"),
    (THEME, "Theme picker (browse + preview live)"),
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
    // `ctrl+o` toggles the outline panel; `ctrl+f` opens find-in-content.
    // Control chords (not bare letters) so they are *not* editor text: the
    // same chord drives the action in Review (through the keymap,
    // remappable) and in Edit mode (handled raw in `on_key_edit`, the way
    // Ctrl-S/Ctrl-R already are — the keymap is bypassed there by the
    // Capture::Text context). The editor also accepts the vim `/` to open
    // the same find prompt.
    km.bind(OUTLINE, &["ctrl+o"]);
    km.bind(SEARCH, &["ctrl+f"]);
    km.bind(THEME, &["ctrl+t"]);
    let mut k = Keymaps::from_maps(vec![km]);
    // The filter row and the file editor are text inputs: a Capture::Text
    // context, so while one is focused every command key is raw input
    // (`Fall`) — no hand-ordered guards, no command can fire mid-type.
    // ADR 0020.
    k.register_context("input", Capture::Text);
    k
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
    /// The Edit-mode editor text rect (inside the block + line-number
    /// gutter), recorded by `view_detail` so the reducer can
    /// `scroll_into_view` against the *real* laid-out viewport — the
    /// model←geometry feedback the scroll fix needs. `None` outside Edit.
    edit_text: Option<Rect>,
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
    /// Caller-owned read-through memo of the review pane's Tier-1
    /// (tree-sitter) overlay (ADR 0025 / ADR 0012 §P1). The review `Diff` is
    /// rebuilt every frame with `.tree_sitter(true)`, which otherwise
    /// re-parses the *whole patch* with tree-sitter on every scroll
    /// keystroke; the map is a pure function of `(self.diff, theme)`, so this
    /// makes the first frame for a commit a miss and every later scroll frame
    /// an `O(1)` hit. Cleared when `self.diff` is replaced (commit change /
    /// reload) to stay strictly bounded across a long review session.
    diff_syntax_cache: DiffSyntaxCache,
    /// The sha `diff`/`files` belong to, so a stale async result is ignored.
    detail_for: Option<String>,
    /// Vertical scroll of the diff (now `usize`: `Diff::scroll` widened so
    /// patches taller than 65 535 rows are reachable — gap K).
    diff_scroll: usize,
    /// First content column drawn in the diff — horizontal scroll for long
    /// code lines (`Diff::col`, gap B).
    diff_col: usize,
    /// Caller-owned 2D editor scroll, kept caret-visible by
    /// [`TextArea::scroll_into_view`] after every motion/edit (the deferred
    /// `scroll_into_view` seam — fixes the computed/unclamped scroll defect).
    editor_scroll: (usize, usize),
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

    // ── CE-3B/CE-3C: editor capabilities, all caller-owned model state the
    // pure `view` only reads (ADR 0004/0012). ──────────────────────────────
    /// Undo/redo of the *edit buffer* (gap **E**). Seeded with the loaded
    /// file when Edit mode is entered, snapshotted **before** every mutating
    /// edit (single-char inserts coalesced into one step). This is what makes
    /// the `Ctrl-S`-writes-the-working-tree path recoverable: a mis-edit is
    /// `u` away even though the save still goes straight to disk — the live
    /// data-loss path the deep-dive flags as S1 is now closed.
    undo: History<TextArea>,
    /// The logical *text* selection (gap **F**) — named `tsel` so it does
    /// not collide with the commit-ordinal `sel`. Shift+motion / mouse drag
    /// extend it; a `Char`/`Enter`/paste with a non-empty selection replaces
    /// the span (snapshotting first so the replace is undoable). Projected by
    /// `Editor::selection` — the widget reads it, never mutates it.
    tsel: DocSelection,
    /// The yank register `y` fills from the selection and `p` pastes.
    yank: String,
    /// The flattened per-char syntax overlay for the *edit buffer* (gap
    /// **G**), rebuilt by [`rebuild_edit_overlays`](Self::rebuild_edit_overlays)
    /// only on a text change. Colours come from [`theme`](Self::theme).
    edit_syntax: Vec<Style>,
    /// `edit_syntax` with the active search matches patched on top — the
    /// single overlay handed to `Editor::syntax`. Rebuilt on a text change
    /// *or* a search move (the match set depends on the live query).
    edit_overlay: Vec<Style>,
    /// The outline / symbol panel is open (gap **H**).
    outline_open: bool,
    /// Selected row in the outline panel (a symbol in Edit, a changed file
    /// in Review). The reducer owns the cursor; `Outline`/`Changeset` are the
    /// ordered substrate it indexes.
    outline_sel: usize,
    /// The find-in-content query (gap **J**). Empty ⇒ no matches / no
    /// highlight (the `Query` inert convention).
    query: Query,
    /// The find prompt owns the keyboard (type the pattern; Enter commits,
    /// Esc clears). Reuses the filter-input idiom.
    searching: bool,
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
            diff_syntax_cache: DiffSyntaxCache::new(),
            detail_for: None,
            diff_scroll: 0,
            diff_col: 0,
            editor_scroll: (0, 0),
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
            keymap_panel: false,
            km_sel: 0,
            km_rebind: None,
            status: String::new(),
            error: None,
            theme: crate::theme::startup_theme(),
            theme_picker: rstui_theme::ThemePickerState::new(),
            theme_restore: None,
            picking: false,
            undo: History::new(TextArea::new()),
            tsel: DocSelection::new(),
            yank: String::new(),
            edit_syntax: Vec::new(),
            edit_overlay: Vec::new(),
            outline_open: false,
            outline_sel: 0,
            query: Query::new(""),
            searching: false,
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
        // The reviewed patch is being replaced wholesale (commit change /
        // reload): drop the memoised Tier-1 slot so the cache stays strictly
        // bounded across a long session (mirrors `DiagramCache::clear()` on
        // wholesale content replacement — the key alone already guarantees
        // correctness, this just frees the old patch's overlays).
        self.diff_syntax_cache.clear();
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
            self.help = false; // Any key dismisses the cheat-sheet…
            // …and `k` (the universal gateway, same key in every app)
            // turns the cheat-sheet into the keymap editor.
            if code == KeyCode::Char('k') {
                self.keymap_panel = true;
                self.km_sel = 0;
                self.km_rebind = None;
            }
            return Cmd::none();
        }
        if self.picking {
            return self.theme_picker_key(code);
        }
        if self.keymap_panel {
            return self.on_key_keymap(code, mods);
        }
        // Outline panel navigation (gap H). `Tab`/`BackTab` step the panel
        // when it is open — chosen because they are *not* editor text (so
        // typing `.`/`,` in Edit mode is unaffected) and only intercepted
        // while the panel is open (so the Review `Tab` = focus-swap and the
        // editor are untouched otherwise). Edit: live-jumps the caret to the
        // symbol. Review: steps file-by-file (each file is its hunk group —
        // the `Changeset::hunk_index` substrate) and scrolls there.
        if self.outline_open
            && !self.searching
            && !self.filtering
            && matches!(code, KeyCode::Tab | KeyCode::BackTab)
        {
            self.outline_nav(if code == KeyCode::Tab { 1 } else { -1 });
            return Cmd::none();
        }
        // Text inputs are a Capture::Text *context*, not a hand-ordered
        // guard cascade (ADR 0020): the filter row / editor activates
        // "input", so `dispatch` returns `Fall` for typed keys — a command
        // *cannot* fire mid-type, by construction — and the raw key is
        // routed to that text handler. One value, set on focus/mode; no
        // leader in this map so the clock is `0`.
        self.keymaps.set_context(
            (self.filtering || self.searching || self.mode == Mode::Edit).then_some("input"),
        );
        match self.keymaps.dispatch(&KeyEvent::new(code, mods), 0) {
            Dispatch::Act(action) => self.do_action(action),
            Dispatch::Pending => Cmd::none(), // leader armed — swallow
            // The find prompt is a text input that overlays *either* mode, so
            // it wins ahead of the filter row and the editor.
            Dispatch::Fall if self.searching => self.on_key_search(code),
            Dispatch::Fall if self.filtering => self.on_key_filter(code),
            Dispatch::Fall if self.mode == Mode::Edit => self.on_key_edit(code, mods),
            Dispatch::Fall => self.on_key_motion(code),
        }
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
            THEME => {
                // Open the theme picker directly (Ctrl+T); Esc restores the
                // pre-picker palette via `theme_restore`.
                self.theme_restore = Some(self.theme.clone());
                self.picking = true;
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
            OUTLINE => {
                self.outline_open = !self.outline_open;
                self.outline_sel = 0;
                self.status = if self.outline_open {
                    "outline: Tab step symbol/file · ⌃O close".to_owned()
                } else {
                    "outline closed".to_owned()
                };
            }
            SEARCH => {
                self.searching = true;
                self.status = "find: type · Enter go · n/N next/prev · Esc clear".to_owned();
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
    ///
    /// The buffer is **undoable** (gap E): every mutating key snapshots into
    /// [`undo`](Self::undo) *before* it runs (single-char inserts coalesced),
    /// so `u`/`Ctrl-R` recover it. The `Ctrl-S` save path is **unchanged** —
    /// it still writes the working tree immediately — but a mis-edit is no
    /// longer lost: `u` walks it back. That closes the S1 live-data-loss path
    /// the deep-dive flags (the *recoverability* requirement, not gating the
    /// write). Shift+motion extends a [`DocSelection`] (gap F); a typed
    /// char / Enter / paste over a non-empty selection replaces it.
    fn on_key_edit(&mut self, code: KeyCode, mods: KeyModifiers) -> Cmd<Msg> {
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let shift = mods.contains(KeyModifiers::SHIFT);
        if ctrl && code == KeyCode::Char('s') {
            // Save is deliberately unchanged: it goes straight to disk. Undo
            // makes a wrong save recoverable in-buffer (then re-save) — the
            // recoverability the S1 gap is really about.
            let Some(path) = self.edit_path.clone() else {
                return Cmd::none();
            };
            let repo = self.repo.clone();
            let body = self.editor.lines().join("\n");
            return Cmd::perform(move || {
                Msg::Saved(crate::write_file(&repo, &path, &body).map(|()| path.clone()))
            });
        }
        // Undo / redo (gap E). Apply the returned buffer wholesale, then
        // rebuild the overlays + reclamp scroll for the restored text.
        if !ctrl && code == KeyCode::Char('u') {
            if let Some(prev) = self.undo.undo(&self.editor) {
                self.editor = prev;
                self.tsel.clear();
                self.edit_dirty = true;
                self.rebuild_edit_overlays();
                self.caret_into_view();
                self.status = "undo".to_owned();
            } else {
                self.status = "nothing to undo".to_owned();
            }
            return Cmd::none();
        }
        if ctrl && code == KeyCode::Char('r') {
            if let Some(next) = self.undo.redo(&self.editor) {
                self.editor = next;
                self.tsel.clear();
                self.edit_dirty = true;
                self.rebuild_edit_overlays();
                self.caret_into_view();
                self.status = "redo".to_owned();
            } else {
                self.status = "nothing to redo".to_owned();
            }
            return Cmd::none();
        }
        // `Ctrl-O` toggles the symbol/outline panel from Edit mode too (the
        // keymap is bypassed here by the Text context, so this mirrors the
        // Review `OUTLINE` action — same chord, every mode; gap H).
        if ctrl && code == KeyCode::Char('o') {
            self.outline_open = !self.outline_open;
            self.outline_sel = 0;
            self.status = if self.outline_open {
                "outline: Tab step symbol/file · ⌃O close".to_owned()
            } else {
                "outline closed".to_owned()
            };
            return Cmd::none();
        }
        // `/` (vim) or `Ctrl-F` opens the find prompt over the buffer (gap
        // J). `/` is otherwise a literal slash; find is far more useful and
        // is the universal editor convention.
        if (!ctrl && code == KeyCode::Char('/')) || (ctrl && code == KeyCode::Char('f')) {
            self.searching = true;
            self.status = "find: type · Enter go · n/N next/prev · Esc clear".to_owned();
            return Cmd::none();
        }
        // `n`/`N` step matches while a query is live (otherwise they are
        // literal text).
        if !self.query.is_empty() && !ctrl && code == KeyCode::Char('n') {
            self.search_jump(true);
            return Cmd::none();
        }
        if !self.query.is_empty() && !ctrl && code == KeyCode::Char('N') {
            self.search_jump(false);
            return Cmd::none();
        }
        // Yank / paste of the selection (gap F).
        if !ctrl && code == KeyCode::Char('y') {
            if let Some((a, b)) = self.tsel.range() {
                if !self.tsel.is_empty() {
                    self.yank = self.editor.span_text(a, b);
                    self.status = format!("yanked {} chars", self.yank.chars().count());
                    return Cmd::none();
                }
            }
            self.status = "nothing selected to yank".to_owned();
            return Cmd::none();
        }
        if !ctrl && code == KeyCode::Char('p') && !self.yank.is_empty() {
            let paste = self.yank.clone();
            self.insert_text(&paste, false);
            return Cmd::none();
        }
        match code {
            KeyCode::Esc => {
                self.tsel.clear();
                self.mode = Mode::Review;
                self.status = "stopped editing".to_owned();
            }
            // Typing over a non-empty selection replaces it (gap F);
            // otherwise a coalesced single-char insert (gap E). Newline
            // seals the coalescing run.
            KeyCode::Char(c) => self.insert_text(&c.to_string(), true),
            KeyCode::Enter => self.insert_text("\n", false),
            KeyCode::Backspace => self.delete_selection_or(|e| {
                e.delete_backward();
            }),
            KeyCode::Delete => self.delete_selection_or(|e| {
                e.delete_forward();
            }),
            KeyCode::Left => self.move_caret(shift, |e| {
                e.move_left();
            }),
            KeyCode::Right => self.move_caret(shift, |e| {
                e.move_right();
            }),
            KeyCode::Up => self.move_caret(shift, |e| {
                e.move_up();
            }),
            KeyCode::Down => self.move_caret(shift, |e| {
                e.move_down();
            }),
            KeyCode::Home => self.move_caret(shift, TextArea::move_home),
            KeyCode::End => self.move_caret(shift, TextArea::move_end),
            KeyCode::PageUp => self.move_caret(shift, |e| {
                e.move_page_up(10);
            }),
            KeyCode::PageDown => self.move_caret(shift, |e| {
                e.move_page_down(10);
            }),
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
            KeyCode::Left if self.focus == Focus::Detail => {
                self.diff_col = self.diff_col.saturating_sub(8);
            }
            KeyCode::Right if self.focus == Focus::Detail => {
                self.diff_col = self.diff_col.saturating_add(8);
            }
            KeyCode::Home if self.focus == Focus::Detail => {
                self.diff_scroll = 0;
                self.diff_col = 0;
            }
            _ => {}
        }
        // Clamp to the real viewport so the diff never scrolls into blank
        // space past the end (fixes the clamp-to-total-rows defect; uses
        // the cheap `Diff::row_count` seam, not an O(parse) `lines()` per
        // key).
        self.clamp_detail_scroll();
        Cmd::none()
    }

    /// Clamp [`diff_scroll`](Self::diff_scroll) so the diff never scrolls
    /// past the last screenful into blank space. Reads the real detail-pane
    /// viewport recorded by the last frame ([`Geom`]) and the cheap
    /// [`Diff::row_count`] accessor — the model←geometry feedback the
    /// scroll fix needs (ADR 0004: scroll is reducer-owned; the reducer
    /// learns the laid-out extent from the frame it drew).
    fn clamp_detail_scroll(&mut self) {
        let Some(g) = self.geom.get() else { return };
        let vw = g.detail.width.saturating_sub(2); // pane border
        let vh = g.detail.height.saturating_sub(2) as usize;
        if self.diff.is_empty() || vw == 0 {
            return;
        }
        let mut d = Diff::new(self.diff.as_str()).syntax(true);
        if self.diff_split {
            d = d.side_by_side();
        }
        let max = d.row_count(vw).saturating_sub(vh);
        self.diff_scroll = self.diff_scroll.min(max);
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

// ── CE-3B/CE-3C: editor / diff capability wiring (gaps E·F·G·H·J). Every
// method here only mutates the model in `update`; the pure `view` reads the
// resulting fields (ADR 0004/0012). ────────────────────────────────────────
impl GitReview {
    /// The four syntax token [`Style`]s, from the **active theme** (gap G is
    /// "colours come from rstui-theme, not hard-coded"). Keywords use the
    /// brand accent, strings the good colour, numbers the graph colour,
    /// comments the dim colour — the reviewer's own role palette, so picking
    /// a theme reskins the code too.
    fn syntax_styles(&self) -> syntax::SyntaxStyles {
        // git-review stays on the Tier-0 `syntax::line_overlay`, which only
        // ever fills these four. The richer Tier-1-only semantic classes
        // (`function`/`type_`/…) default to no colour — correct here: the
        // dependency-free scanner cannot detect them, so leaving them empty
        // keeps git-review's rendering byte-identical.
        syntax::SyntaxStyles {
            comment: self.theme.dim(),
            string: self.theme.good(),
            number: self.theme.graph(),
            keyword: self.theme.accent(),
            ..Default::default()
        }
    }

    /// The lexer language for the file being edited (`Unknown` ⇒ the
    /// byte-identical common core).
    fn edit_lang(&self) -> Language {
        Language::from_path(self.edit_path.as_deref().unwrap_or(""))
    }

    /// The outline language for the file being edited (the `outline` module
    /// has its own `Language`; `Unknown` ⇒ an empty outline).
    fn outline_lang(&self) -> outline::Language {
        outline::Language::from_path(self.edit_path.as_deref().unwrap_or(""))
    }

    /// The first `+++ b/<path>` in the patch (best-effort) → the language the
    /// `Diff` should lex with; no such header ⇒ `Unknown`, which keeps the
    /// render byte-identical to the historical built-in tinter.
    fn diff_lang(&self) -> Language {
        for line in self.diff.lines() {
            if let Some(p) = line.strip_prefix("+++ b/") {
                return Language::from_path(p.trim());
            }
            if let Some(p) = line.strip_prefix("+++ ") {
                // `+++ /dev/null` (a delete) or an `a/`-stripped path.
                let p = p.trim();
                if p != "/dev/null" {
                    return Language::from_path(p.strip_prefix("b/").unwrap_or(p));
                }
            }
        }
        Language::Unknown
    }

    /// The editor buffer's lines as owned `String`s — the unit
    /// [`Query`]/[`DocSelection`] math works in.
    fn editor_lines(&self) -> Vec<String> {
        self.editor.lines().to_vec()
    }

    /// Rebuild the per-char syntax overlay for the edit buffer, threading
    /// [`syntax::LexState`] line→line so a multi-line string/comment colours
    /// the lines under it, and concatenating with one empty slot per `'\n'`
    /// (the exact flattened layout `Editor::syntax` reads). Then re-apply the
    /// search highlight on top via [`recolor_search`](Self::recolor_search).
    /// Called only on a *text change* — it is the caller-owned memo the
    /// widget reads, not derived per frame.
    fn rebuild_edit_overlays(&mut self) {
        let lang = self.edit_lang();
        let styles = self.syntax_styles();
        let mut flat: Vec<Style> = Vec::new();
        let mut st = syntax::LexState::default();
        let lines = self.editor.lines();
        for (i, line) in lines.iter().enumerate() {
            let (ov, next) = syntax::line_overlay(line, lang, &styles, st);
            st = next;
            flat.extend(ov);
            if i + 1 < lines.len() {
                flat.push(Style::new()); // the '\n' between rows
            }
        }
        self.edit_syntax = flat;
        self.recolor_search();
    }

    /// Patch the live search matches over a *copy* of `edit_syntax` into
    /// `edit_overlay` (the single overlay handed to `Editor::syntax`). The
    /// match style is a distinct, themed reverse so a hit stands out from
    /// both plain and syntax-coloured text. Rebuilt on a text change or a
    /// search move (the match set depends on the query). No query ⇒ the
    /// overlay is exactly the syntax one (byte-identical).
    fn recolor_search(&mut self) {
        self.edit_overlay = self.edit_syntax.clone();
        if self.query.is_empty() {
            return;
        }
        let hit = self.theme.selection();
        let lines = self.editor.lines();
        // Flat index of the first char of each row (rows joined by one '\n'
        // slot — the same layout `rebuild_edit_overlays` built).
        let mut base = 0usize;
        let matches = self.query.find_all(&self.editor_lines());
        let mut mi = matches.iter().peekable();
        for (row, line) in lines.iter().enumerate() {
            let len = line.chars().count();
            while let Some(m) = mi.peek() {
                if m.row != row {
                    break;
                }
                for c in m.start..m.end.min(len) {
                    if let Some(slot) = self.edit_overlay.get_mut(base + c) {
                        *slot = hit;
                    }
                }
                mi.next();
            }
            base += len + 1; // + the '\n' slot
        }
    }

    /// Re-clamp the caller-owned editor scroll so the caret is visible
    /// against the real text viewport the last frame laid out (the deferred
    /// `scroll_into_view` seam — model←geometry feedback). `None` only before
    /// the first frame, where the caret is at the origin anyway.
    fn caret_into_view(&mut self) {
        if let Some(tr) = self.geom.get().and_then(|g| g.edit_text) {
            self.editor_scroll =
                self.editor
                    .scroll_into_view(self.editor_scroll, (tr.width, tr.height), 3);
        }
    }

    /// Run after a *mutating* edit: mark dirty, rebuild the syntax/search
    /// overlay, keep the caret on screen.
    fn after_edit(&mut self) {
        self.edit_dirty = true;
        self.rebuild_edit_overlays();
        self.caret_into_view();
    }

    /// Snapshot the buffer for undo *before* a mutating edit. `coalesce`
    /// folds a run of single-char inserts into one step (so a typed word is
    /// one `u`); any other edit seals the run.
    fn snapshot(&mut self, coalesce: bool) {
        let cur = self.editor.clone();
        self.undo.snapshot_coalesced(&cur, coalesce);
    }

    /// If a selection is non-empty, snapshot + `replace_span` it with `s`
    /// and clear the selection; returns whether it did (so the caller knows
    /// the keystroke was consumed by the replace). The select-then-replace
    /// primitive (gap F).
    fn replace_selection(&mut self, s: &str) -> bool {
        let Some((a, b)) = self.tsel.range() else {
            return false;
        };
        if self.tsel.is_empty() {
            return false;
        }
        self.snapshot(false);
        self.editor.replace_span(a, b, s);
        self.tsel.clear();
        self.after_edit();
        true
    }

    /// Insert `s`: replace a non-empty selection with it (gap F), else a
    /// plain insert. `coalesce` only matters for the no-selection path (a
    /// run of single-char inserts → one undo step, gap E). Snapshots first.
    fn insert_text(&mut self, s: &str, coalesce: bool) {
        if self.replace_selection(s) {
            return;
        }
        self.tsel.clear();
        self.snapshot(coalesce);
        self.editor.insert_str(s);
        self.after_edit();
    }

    /// Delete the selection if there is one, otherwise run `fallback` (the
    /// single-char Backspace/Delete). Always snapshots first and seals the
    /// coalescing run (a delete is never coalesced).
    fn delete_selection_or(&mut self, fallback: impl FnOnce(&mut TextArea)) {
        self.snapshot(false);
        if let Some((a, b)) = self.tsel.range().filter(|_| !self.tsel.is_empty()) {
            self.editor.delete_span(a, b);
            self.tsel.clear();
        } else {
            fallback(&mut self.editor);
        }
        self.after_edit();
    }

    /// A caret motion in Edit mode. With `shift` it extends a charwise
    /// [`DocSelection`] (starting one at the *pre-move* caret on the first
    /// Shift-move); without it any selection is cleared first — the exact
    /// "Shift extends, a plain move drops it" rule. `mv` performs the actual
    /// `TextArea` move.
    fn move_caret(&mut self, shift: bool, mv: impl FnOnce(&mut TextArea)) {
        if shift {
            if self.tsel.is_empty() {
                self.tsel.start(self.editor.cursor(), SelKind::Char);
            }
            mv(&mut self.editor);
            self.tsel.extend(self.editor.cursor());
        } else {
            self.tsel.clear();
            mv(&mut self.editor);
        }
        self.caret_into_view();
    }

    /// The find prompt's keys (a Capture::Text sub-mode, like the filter
    /// row). Reuses the filter-input idiom: type the pattern, Enter jumps to
    /// the first match, Esc clears.
    fn on_key_search(&mut self, code: KeyCode) -> Cmd<Msg> {
        match code {
            KeyCode::Esc => {
                self.query = Query::new("");
                self.searching = false;
                if self.mode == Mode::Edit {
                    self.recolor_search();
                }
                self.status = "find cleared".to_owned();
            }
            KeyCode::Enter => {
                self.searching = false;
                self.search_jump(true);
            }
            KeyCode::Backspace => {
                let mut p = self.query.pattern().to_owned();
                p.pop();
                self.query = Query::new(p);
                if self.mode == Mode::Edit {
                    self.recolor_search();
                }
            }
            KeyCode::Char(c) => {
                let mut p = self.query.pattern().to_owned();
                p.push(c);
                self.query = Query::new(p);
                if self.mode == Mode::Edit {
                    self.recolor_search();
                }
            }
            _ => {}
        }
        Cmd::none()
    }

    /// Jump to the next (`forward`) / previous match of the live query and
    /// bring it on screen. In Edit mode it moves the `TextArea` caret and
    /// `scroll_into_view`s it; in Review it scrolls the patch so the matched
    /// patch line is visible (the `Diff` widget owns its own per-cell
    /// rendering, so there is no caller highlight seam there — this is the
    /// honest best-effort scroll-to-match the deep-dive's Part 8 anticipates).
    fn search_jump(&mut self, forward: bool) {
        if self.query.is_empty() {
            return;
        }
        if self.mode == Mode::Edit {
            let lines = self.editor_lines();
            let from = self.editor.cursor();
            let hit = if forward {
                self.query.next_from(&lines, from, true)
            } else {
                self.query.prev_from(&lines, from, true)
            };
            if let Some(m) = hit {
                self.editor.set_cursor(m.row, m.start);
                self.recolor_search();
                self.caret_into_view();
                self.status = format!("/{}", self.query.pattern());
            } else {
                self.status = format!("no match for /{}", self.query.pattern());
            }
        } else {
            // Review: search the raw patch lines; Diff renders ~one row per
            // patch line, so scrolling to the matched line index is a sound
            // best-effort that keeps the model totally clamped. `next` starts
            // one line *past* the current scroll so repeated `n` advances.
            let lines: Vec<String> = self.diff.lines().map(str::to_owned).collect();
            let hit = if forward {
                self.query
                    .next_from(&lines, (self.diff_scroll.saturating_add(1), 0), true)
            } else {
                self.query.prev_from(&lines, (self.diff_scroll, 0), true)
            };
            if let Some(m) = hit {
                self.diff_scroll = m.row;
                self.diff_col = 0;
                self.clamp_detail_scroll();
                self.status = format!("/{} — patch line {}", self.query.pattern(), m.row + 1);
            } else {
                self.status = format!("no match for /{}", self.query.pattern());
            }
        }
    }

    /// The outline rows for the panel + the index of the symbol/file the
    /// caret (Edit) or selection (Review) is currently in, so the panel can
    /// highlight "where you are". In Edit it is the scanned [`Outline`]; in
    /// Review the [`Changeset`] file list, each annotated with the symbol its
    /// first hunk falls in (`Outline::at_line(hunk.new_start)`).
    fn outline_rows(&self) -> (Vec<Line<'static>>, Option<usize>) {
        if self.mode == Mode::Edit {
            let o = Outline::scan(&self.editor.to_string(), self.outline_lang());
            // "Where you are": the deepest symbol whose [line,end_line] holds
            // the caret row (same rule as `Outline::at_line`), by index.
            let caret = self.editor.cursor().0;
            let here =
                o.0.iter()
                    .enumerate()
                    .filter(|(_, s)| s.line <= caret && caret <= s.end_line)
                    .max_by_key(|(_, s)| (s.depth, s.line))
                    .map(|(i, _)| i);
            let rows =
                o.0.iter()
                    .map(|s| {
                        let pad = "  ".repeat(s.depth as usize);
                        Line::from(vec![
                            Span::styled(format!("{pad}{} ", kind_glyph(s.kind)), self.theme.dim()),
                            Span::styled(
                                if s.name.is_empty() {
                                    "·".to_owned()
                                } else {
                                    s.name.clone()
                                },
                                self.theme.accent(),
                            ),
                        ])
                    })
                    .collect();
            (rows, here)
        } else {
            let cs = Changeset::parse(&self.diff);
            let rows = cs
                .files
                .iter()
                .map(|f| {
                    let lang = outline::Language::from_path(&f.path);
                    // Annotate with the symbol the file's first hunk lands in
                    // (best-effort over the reconstructed new-side text).
                    let sym = f.hunks.first().and_then(|h| {
                        let new_side = reconstruct_new_side(f.patch());
                        Outline::scan(&new_side, lang)
                            .at_line((h.new_start as usize).saturating_sub(1))
                            .map(|s| s.name.clone())
                    });
                    let mut spans = vec![
                        Span::styled(format!("{} ", status_glyph(f)), self.theme.graph()),
                        Span::styled(f.path.clone(), self.theme.accent()),
                        Span::styled(
                            format!("  +{} -{}", f.additions, f.deletions),
                            self.theme.dim(),
                        ),
                    ];
                    if let Some(name) = sym.filter(|n| !n.is_empty()) {
                        spans.push(Span::styled(format!("  · {name}"), self.theme.dim()));
                    }
                    Line::from(spans)
                })
                .collect();
            // "Where you are" = the file the current diff-scroll line is in,
            // via the global hunk index (reducer arithmetic over the ordered
            // substrate — `Changeset` holds no cursor).
            let here = cs.files.iter().position(|f| !f.hunks.is_empty());
            (rows, here)
        }
    }

    /// Step the outline selection by `delta` (clamped) and *follow* it:
    /// Edit-mode jumps the editor caret to the symbol's line (`set_cursor` +
    /// `scroll_into_view` — exactly the deep-dive's "selecting a symbol moves
    /// the caret"); Review-mode scrolls the patch to the selected file's
    /// first hunk (the patch-line of its first `@@`, found over the
    /// `Changeset` ordered substrate). Total: an empty outline is a no-op.
    fn outline_nav(&mut self, delta: isize) {
        let total = self.outline_rows().0.len();
        if total == 0 {
            self.status = "outline is empty".to_owned();
            return;
        }
        let next = (self.outline_sel as isize + delta).clamp(0, total as isize - 1) as usize;
        self.outline_sel = next;
        if self.mode == Mode::Edit {
            let o = Outline::scan(&self.editor.to_string(), self.outline_lang());
            if let Some(sym) = o.0.get(next) {
                self.editor.set_cursor(sym.line, 0);
                self.tsel.clear();
                self.caret_into_view();
                self.status = format!("→ {}", sym.name);
            }
        } else {
            let cs = Changeset::parse(&self.diff);
            if let Some(f) = cs.files.get(next) {
                // Scroll the patch so this file's first hunk is in view. The
                // patch-line of the file's first `@@` within the whole diff
                // (Diff renders ≈ one row per patch line — the same
                // best-effort the diff search uses).
                let target = self
                    .diff
                    .lines()
                    .position(|l| l.starts_with("+++ ") && l.contains(&f.path))
                    .or_else(|| self.diff.lines().position(|l| l.starts_with("@@")))
                    .unwrap_or(0);
                self.diff_scroll = target;
                self.diff_col = 0;
                self.clamp_detail_scroll();
                self.status = format!("→ {}", f.path);
            }
        }
    }
}

/// A one-glyph badge for a [`SymbolKind`] (kept ASCII-light so it renders in
/// any terminal/theme).
fn kind_glyph(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Module => "▢",
        SymbolKind::Struct => "◧",
        SymbolKind::Enum => "◑",
        SymbolKind::Trait => "◇",
        SymbolKind::Impl => "◈",
        SymbolKind::Function => "ƒ",
        SymbolKind::Method => "→",
        SymbolKind::Class => "Ⓒ",
        SymbolKind::Constant => "□",
        SymbolKind::Field => "·",
        SymbolKind::Heading => "#",
        SymbolKind::Other => "•",
    }
}

/// A one-glyph badge for a changed file's status.
fn status_glyph(f: &rstui_code::DiffFile) -> &'static str {
    use rstui_code::FileStatus::{Added, Binary, Copied, Deleted, Modified, Renamed};
    match f.status {
        Added => "A",
        Deleted => "D",
        Modified => "M",
        Renamed => "R",
        Copied => "C",
        Binary => "B",
    }
}

/// Reconstruct a file's *new-side* text from its unified patch slice (keep
/// context and added lines, drop removed lines and headers) so an
/// [`Outline`] can be scanned for the diff side. Best-effort: with no hunks
/// it is empty. Total.
fn reconstruct_new_side(patch: &str) -> String {
    let mut out = String::new();
    let mut in_hunk = false;
    for line in patch.lines() {
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        if let Some(rest) = line.strip_prefix(' ') {
            out.push_str(rest);
            out.push('\n');
        } else if let Some(rest) = line.strip_prefix('+') {
            out.push_str(rest);
            out.push('\n');
        }
        // `-` lines and `\ No newline` markers are not on the new side.
    }
    out
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

    /// Map a click/drag in the editor's text rect to a caret + selection
    /// (gap F, the `Editor::cell_to_doc` mouse seam + the
    /// `on_press`/`on_pointer_drag` pointer-gesture pattern). The doc
    /// position is computed *before* any mutation so the borrows don't
    /// overlap. Returns whether the event was inside the editor (consumed).
    fn mouse_edit(&mut self, kind: MouseEventKind, pos: Position) -> bool {
        if self.mode != Mode::Edit {
            return false;
        }
        let Some(tr) = self.geom.get().and_then(|g| g.edit_text) else {
            return false;
        };
        // Reconstruct the same Editor projection `view_detail` drew so its
        // pure `cell_to_doc` inverse matches the render exactly (no Block —
        // `text_rect` is already the inner area).
        let doc = Editor::new(&self.editor)
            .scroll(self.editor_scroll)
            .cell_to_doc(tr, pos);
        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some((r, c)) = doc {
                    self.editor.set_cursor(r, c);
                    // Anchor a fresh selection at the click (extended on drag).
                    self.tsel.start((r, c), SelKind::Char);
                    self.caret_into_view();
                    return true;
                }
                tr.contains(pos)
            }
            MouseEventKind::Drag(MouseButton::Left) if !self.tsel.is_empty() => {
                if let Some((r, c)) = doc {
                    self.editor.set_cursor(r, c);
                    self.tsel.extend((r, c));
                    self.caret_into_view();
                }
                true
            }
            MouseEventKind::ScrollDown if tr.contains(pos) => {
                self.move_caret(false, |e| {
                    e.move_down();
                });
                true
            }
            MouseEventKind::ScrollUp if tr.contains(pos) => {
                self.move_caret(false, |e| {
                    e.move_up();
                });
                true
            }
            _ => false,
        }
    }

    /// Handle one mouse event against the geometry `view` recorded.
    fn on_mouse(&mut self, kind: MouseEventKind, pos: Position) -> Cmd<Msg> {
        let Some(g) = self.geom.get() else {
            return Cmd::none();
        };
        // Edit-mode editor selection/scroll wins inside its text rect.
        if self.mouse_edit(kind, pos) {
            return Cmd::none();
        }
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
        self.clamp_detail_scroll();
        Cmd::none()
    }

    /// The detail pane: the patch (Review) or the editor (Edit). When the
    /// outline panel is open it claims a column on the right via the same
    /// `Layout` split idiom the main split uses (the `SplitPane` discipline:
    /// a pure `Rect`-accessor, no retained widget tree — ADR 0012).
    fn view_detail(&self, frame: &mut Frame<'_>, area: Rect) {
        let (content, panel) = if self.outline_open {
            // ≤ 36 cells, ≥ a quarter of the pane, never wider than the pane.
            let w = (area.width / 4)
                .clamp(0, 36)
                .min(area.width.saturating_sub(1));
            if w >= 8 {
                let [c, p] =
                    Layout::horizontal([Constraint::Fill(1), Constraint::Length(w)]).areas(area);
                (c, Some(p))
            } else {
                (area, None) // too narrow to split — degrade gracefully
            }
        } else {
            (area, None)
        };
        if let Some(p) = panel {
            self.view_outline(frame, p);
        }
        if self.mode == Mode::Edit {
            let path = self.edit_path.as_deref().unwrap_or("(file)");
            let dirty = if self.edit_dirty { " ●" } else { "" };
            let block = self.pane(format!(" {path}{dirty}  ·  Ctrl-S save · Esc back "), true);
            let inner = block.inner(content);
            frame.render_widget(block, content);
            // The gutter is scrolled WITH the text (first visible number =
            // first visible doc row + 1) — fixes the gutter/content desync.
            let gutter = LineNumberGutter::new(
                self.editor_scroll.0 as u64 + 1,
                self.editor.row_count().saturating_sub(self.editor_scroll.0),
            )
            .style(self.theme.dim())
            .min_number_width(3);
            let text_rect = gutter.inner(inner);
            frame.render_widget(gutter, inner);
            // Record the real laid-out text viewport so the reducer can
            // scroll_into_view against it (model←geometry feedback).
            let mut gm = self.geom.get();
            if let Some(g) = gm.as_mut() {
                g.edit_text = Some(text_rect);
            }
            self.geom.set(gm);
            frame.render_widget(
                Editor::new(&self.editor)
                    .focused(true)
                    .scroll(self.editor_scroll)
                    // Caller-owned syntax + search overlay (gaps G/J): the
                    // reducer rebuilds it on edit/search, the widget reads it.
                    .syntax(&self.edit_overlay)
                    // Caller-owned selection (gap F): Shift+motion / mouse
                    // drive `self.tsel`; the widget projects it per cell.
                    .selection(&self.tsel)
                    .selection_style(self.theme.selection())
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
                content,
            );
        } else {
            let d = Diff::new(self.diff.as_str())
                .syntax(true)
                // Tier-1 tree-sitter colour (ADR 0024): a real grammar parse
                // of each file's reconstructed text — the same accuracy
                // upgrade the kitchen-sink IDE editor got. `Changeset` (inside
                // `Diff`) splits the possibly-multi-file patch and resolves a
                // grammar per file; an unknown-language file transparently
                // falls back to the Tier-0 lexer below. Tier-0 stays the
                // always-present floor.
                .tree_sitter(true)
                // Caller-owned read-through memo of the whole-patch Tier-1
                // parse (ADR 0025): this `Diff` is rebuilt every frame, and
                // `tree_sitter(true)` re-parses every file in the patch each
                // time — the cost the DIFF-1 row windowing does not bound.
                // Keyed on `(self.diff, theme)`, so the selected commit's
                // patch is parsed once and every subsequent scroll frame is
                // an O(1) hit (the "really really slow" review-pane fix); a
                // `Ctrl+T` theme switch invalidates it (the key's theme
                // fingerprint changes). Byte-identical to no cache.
                .syntax_cache(&self.diff_syntax_cache)
                // Language-aware Tier-0 fallback colour (gap G): resolved
                // best-effort from the first `+++ b/<path>` in the patch (the
                // git Cmd seam — the widget stays pure). No header ⇒
                // `Unknown`, which is byte-identical to the historical
                // built-in tinter.
                .language(self.diff_lang())
                .scroll(self.diff_scroll)
                .col(self.diff_col)
                // Same gutter floor as the edit-mode `LineNumberGutter`
                // (`.min_number_width(3)`), so the line-number column does
                // not shift width when switching between the editor and the
                // review pane.
                .min_number_width(3);
            let d = if self.diff_split { d.side_by_side() } else { d };
            frame.render_widget(d.block(block), content);
        }
    }

    /// The symbol / outline side panel (gap H): the scanned [`Outline`] of
    /// the edited file, or the [`Changeset`] file list of the reviewed
    /// commit. Pure projection of [`outline_rows`](Self::outline_rows)
    /// through the existing [`List`]; the reducer owns the cursor.
    fn view_outline(&self, frame: &mut Frame<'_>, area: Rect) {
        let (rows, here) = self.outline_rows();
        let total = rows.len();
        let title = if self.mode == Mode::Edit {
            format!(" ⌘ Symbols {total} ")
        } else {
            format!(" ⌘ Files {total} ")
        };
        let sel = if total == 0 {
            None
        } else {
            Some(self.outline_sel.min(total - 1))
        };
        // Mark "where you are" (the symbol/file the caret-or-scroll is in)
        // with a leading ‣, so it reads even when the selection bar is on a
        // different row (`Outline::at_line`, projected — gap H).
        let mut lines: Vec<Line> = Vec::with_capacity(total);
        for (i, l) in rows.into_iter().enumerate() {
            let mut spans = l.spans;
            let lead = if Some(i) == here { "‣ " } else { "  " };
            spans.insert(0, Span::styled(lead, self.theme.good()));
            lines.push(Line::from(spans));
        }
        frame.render_widget(
            List::new(lines)
                .selected(sel)
                .offset(sel.unwrap_or(0).saturating_sub(4))
                .highlight_style(self.theme.selection())
                .block(self.pane(title, false)),
            area,
        );
    }

    /// The bottom status strip.
    fn view_status(&self, frame: &mut Frame<'_>, area: Rect) {
        let repo = self.repo.display().to_string();
        let left = Line::styled(format!(" {repo} · ⎇ {} ", self.branch), self.theme.accent());
        let center: Line = if self.searching {
            Line::styled(
                format!(" find: {}_ ", self.query.pattern()),
                self.theme.good(),
            )
        } else if self.filtering {
            Line::styled(format!(" filter: {}_ ", self.filter), self.theme.good())
        } else if !self.status.is_empty() {
            Line::styled(format!(" {} ", self.status), self.theme.good())
        } else if self.mode == Mode::Edit {
            Line::styled(
                "edit · ⌃S save · u undo/⌃R · Shift+arrows select · y/p · / find · ⌃O outline · Esc back",
                self.theme.dim(),
            )
        } else {
            Line::styled(
                "[ ]: commit · s: split · t: top/left · \\: tree · /: filter · ⌃O: outline · ⌃F: find · ?: help · ⌃K/?→k: keymap",
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
                    // The patch text was just replaced wholesale; drop any
                    // memoised slot for the prior content so the cache stays
                    // bounded (the next render is a miss that warms the new
                    // patch once, then every scroll frame is an O(1) hit).
                    self.diff_syntax_cache.clear();
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
                        // Entering Edit mode seeds the undo history with the
                        // loaded buffer (gap E: the loaded file is the first
                        // undo point, so the *first* edit is recoverable too)
                        // and resets the selection / query / scroll for the
                        // fresh file.
                        self.undo = History::new(self.editor.clone());
                        self.tsel.clear();
                        self.query = Query::new("");
                        self.searching = false;
                        self.editor_scroll = (0, 0);
                        self.rebuild_edit_overlays();
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
            edit_text: None, // set by view_detail's Edit branch below
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
                HelpEntry::new(["Ctrl", "T"], "Theme picker (browse + preview live)"),
                HelpEntry::new(["Ctrl", "O"], "Symbol / outline panel (Tab steps it)"),
                HelpEntry::new(["Ctrl", "F"], "Find — n/N next/prev (/ in Edit too)"),
                HelpEntry::new(["e"], "Edit the commit's first changed file"),
                HelpEntry::new(
                    ["Ctrl", "S"],
                    "Save (Edit) — u undo / Ctrl-R redo, recoverable",
                ),
                HelpEntry::new(
                    ["Shift", "↑↓"],
                    "Extend a selection · y yank · p paste (Edit)",
                ),
                HelpEntry::new(
                    ["Mouse"],
                    "Click a commit · drag in the editor selects · border resizes · wheel scrolls",
                ),
                HelpEntry::new(["Esc"], "Leave Edit / clear find / close this help"),
                HelpEntry::new(["q"], "Quit"),
                HelpEntry::new(["k"], "Customise these keybindings (keymap editor)"),
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
