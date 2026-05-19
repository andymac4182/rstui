//! The rstui [`App`]: the chat model, the `update` reducer, and the pure
//! `view`. No terminal, no tokio — every screen is reachable from a `Harness`
//! test (ADR 0011's determinism mandate: the reducer never `await`s).

use std::collections::BTreeMap;
use std::path::PathBuf;

use rstui_core::{Event, KeyCode, KeyEvent, KeyModifiers, Line, Size, TextArea};
use rstui_keymap::{Action, Chord, Dispatch, Keymap, Keymaps};
use rstui_runtime::{App, Cmd, Frame};
use rstui_widgets::Markdown;

use crate::Config;
use crate::acp::{
    AcpEvent, AuthOption, DriverCmd, DriverHandle, ModeOption, ModelOption, PermissionChoice,
    PermissionOption, TodoEntry, TodoStatus, ToolCallInfo, ToolKind, ToolStatus, spawn_driver,
};
use crate::history::InputHistory;
use crate::plugin::{FooterSegment, HostEvent, PluginAction, PluginEvent, PluginHost};
use crate::registry::Registry;
use crate::sessions::{SessionRef, SessionStore};
use crate::ui;

/// acp-client's mode-independent **global** command surface as semantic
/// [`Action`]s, resolved through [`Keymaps`] so they are remappable, shown
/// in the keymap panel, and overridable from a `RSTUI_KEYMAP` config file
/// (ADR 0015 — the same engine the kitchen sink and git-review use).
///
/// The deeply contextual keys (the composer's text entry, the
/// modal/permission/ask dialogs, the slash-completion popup, plugin
/// chords) stay raw by design — ADR 0015 keeps the keymap shell-level, the
/// same boundary the other two apps draw for text/motion keys.
const COMMANDS: &[(Action, &str)] = &[
    (Action::Help, "Help overlay"),
    (Action::Drawer, "Keymap settings"),
    (Action::Quit, "Quit"),
];

/// acp-client's own keymap (no kitchen-sink leftovers): one named map of
/// the global commands, via [`Keymaps::from_maps`]. Bindings are
/// non-text chords (Fn/Ctrl) so they never shadow the composer.
fn acp_keymaps() -> Keymaps {
    let mut km = Keymap::new("acp-client");
    km.bind(Action::Quit, &["ctrl+c", "ctrl+q", "f10"]);
    km.bind(Action::Help, &["f1"]);
    km.bind(Action::Drawer, &["ctrl+k"]);
    Keymaps::from_maps(vec![km])
}

/// Split a `keys_for` display string into [`rstui_widgets::KeymapView`]
/// caps: `"⌃C / F10"` → `["⌃C", "F10"]`; `"—"` → `[]` (disabled).
fn caps(keys: &str) -> Vec<String> {
    if keys == "—" {
        return Vec::new();
    }
    keys.split(" / ").map(str::to_owned).collect()
}

/// Pure parse of the bell preference: on unless the value is explicitly a
/// falsy token (`0`/`false`/`no`/`off`, case-insensitive). Split from the
/// env read so it is unit-testable without the process-global env var.
#[must_use]
fn bell_from_env(val: Option<&str>) -> bool {
    match val {
        Some(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        None => true,
    }
}

/// The startup bell preference — `RSTUI_ACP_BELL` (the same typo-safe
/// env-override convention as `RSTUI_THEME` / `RSTUI_ACP_HISTORY`).
#[must_use]
fn bell_default() -> bool {
    bell_from_env(std::env::var("RSTUI_ACP_BELL").ok().as_deref())
}

/// Directory names never descended into by the `@`-mention file scan
/// (huge / generated / VCS internals).
const MENTION_SKIP_DIRS: &[&str] = &[
    ".git",
    ".jj",
    ".hg",
    ".svn",
    "target",
    "node_modules",
    "dist",
    "build",
    ".next",
    ".venv",
    "__pycache__",
];

/// Bounded recursive scan of `root` for `@`-mention candidates: forward-slash
/// relative paths, hidden entries and [`MENTION_SKIP_DIRS`] pruned, hard
/// caps on depth and count so the walk can never stall the UI on a huge
/// tree. Pure-ish (only reads the FS); never fails (unreadable dirs skipped).
#[must_use]
fn scan_files(root: &std::path::Path) -> Vec<String> {
    const MAX_FILES: usize = 4000;
    const MAX_DEPTH: usize = 8;
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if out.len() >= MAX_FILES {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || MENTION_SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            let path = entry.path();
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                if depth + 1 < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
            } else {
                out.push(rel);
                if out.len() >= MAX_FILES {
                    break;
                }
            }
        }
    }
    out
}

/// Ranks `candidates` for `@`-mention `query` (case-insensitive): an empty
/// query keeps the first `max` (sorted); otherwise basename-prefix beats
/// basename-substring beats path-substring, ties broken by shortest then
/// lexicographic, capped at `max`. Pure — the unit-tested core.
#[must_use]
fn rank_paths(candidates: &[String], query: &str, max: usize) -> Vec<String> {
    let q = query.to_ascii_lowercase();
    let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_ascii_lowercase();
    let mut scored: Vec<(u8, usize, String)> = candidates
        .iter()
        .filter_map(|p| {
            let lp = p.to_ascii_lowercase();
            let lb = base(p);
            let rank = if q.is_empty() {
                3
            } else if lb.starts_with(&q) {
                0
            } else if lb.contains(&q) {
                1
            } else if lp.contains(&q) {
                2
            } else {
                return None;
            };
            Some((rank, p.len(), p.clone()))
        })
        .collect();
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    scored.into_iter().take(max).map(|(_, _, p)| p).collect()
}

/// Captures `git diff HEAD` (staged + unstaged vs the last commit — what
/// the agent changed) in `cwd`, plus any untracked files. Blocking; called
/// only inside a `Cmd::perform` (the registry/curl pattern), so the
/// subprocess is correct and adds no dependency. Never fails — a non-repo /
/// missing `git` becomes a readable message.
#[must_use]
fn run_git_diff(cwd: &std::path::Path) -> String {
    let diff = std::process::Command::new("git")
        .args(["--no-pager", "diff", "HEAD"])
        .current_dir(cwd)
        .output();
    let mut out = match diff {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => {
            // No HEAD yet (fresh repo) — fall back to the index/worktree diff.
            std::process::Command::new("git")
                .args(["--no-pager", "diff"])
                .current_dir(cwd)
                .output()
                .ok()
                .filter(|o2| o2.status.success())
                .map(|o2| String::from_utf8_lossy(&o2.stdout).into_owned())
                .unwrap_or_else(|| {
                    format!("git diff failed: {}", String::from_utf8_lossy(&o.stderr))
                })
        }
        Err(e) => return format!("could not run git: {e}"),
    };
    if let Ok(o) = std::process::Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(cwd)
        .output()
    {
        let untracked = String::from_utf8_lossy(&o.stdout);
        let untracked = untracked.trim();
        if !untracked.is_empty() {
            out.push_str("\n\nUntracked files:\n");
            for f in untracked.lines() {
                out.push_str("  ");
                out.push_str(f);
                out.push('\n');
            }
        }
    }
    if out.trim().is_empty() {
        "(no changes)".to_owned()
    } else {
        out
    }
}

/// Unix seconds now (0 before the epoch — impossible in practice; only the
/// relative ordering of `/resume` entries matters anyway).
#[must_use]
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Which top-level screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Choosing an agent from the registry.
    Picker,
    /// Spawning / initializing the agent.
    Connecting,
    /// Connected; the chat transcript + composer.
    Chat,
}

/// Visibility policy for the todo sidebar (opencode parity: it auto-shows
/// while there is open work and hides once everything is done).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    /// Show only while there are todos and not all are completed.
    Auto,
    /// Always show.
    Shown,
    /// Never show.
    Hidden,
}

/// The author of a transcript entry, which drives its styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The local user.
    User,
    /// The agent's answer.
    Agent,
    /// The agent's "thinking" (rendered dim).
    Thought,
    /// A tool call the agent made.
    Tool,
    /// A plan / todo update.
    Plan,
    /// A declarative UI document the agent sent (A2UI / json-render),
    /// rendered through `rstui-jsonui` (ADR 0017). `Entry::text` holds
    /// the verbatim document source; the format is re-detected at render
    /// (a pure projection — no retained UI tree).
    RichUi,
    /// Client-generated system line.
    System,
}

/// The width the transcript parses/wraps agent markdown at (UI-1/MD-1).
///
/// Shared by the renderer (`ui::transcript_lines`) and the caller-owned
/// parse cache (`ChatApp::refresh_md_cache`) so the cached lines are
/// *exactly* what a fresh render-time parse would produce — drift here
/// would make the cache observably wrong.
pub(crate) const MD_WIDTH: u16 = 80;

/// One block in the scrolling transcript.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Who produced it.
    pub role: Role,
    /// Its text (may contain newlines; wrapped at render time).
    pub text: String,
    /// `true` while an agent turn is still appending to this entry.
    pub open: bool,
    /// UI-1/MD-1: caller-owned cache of the parsed+laid-out markdown for a
    /// `Role::Agent` entry, populated in `update` once the entry is no
    /// longer the last one (and so its `text` is immutable — only the
    /// last entry is ever mutated). `None` means "parse fresh at render"
    /// (the still-streaming last entry, or not yet populated); the
    /// renderer always falls back to a fresh parse, so this is a pure
    /// speed cache and never changes output. Not part of identity/eq.
    pub md_cache: Option<Vec<Line<'static>>>,
}

/// A permission request awaiting the user's choice.
#[derive(Debug, Clone)]
pub struct PendingPermission {
    id: u64,
    title: String,
    options: Vec<PermissionOption>,
    selected: usize,
}

/// An in-flight plugin ask-user overlay.
#[derive(Debug)]
pub struct AskState {
    plugin: String,
    id: u64,
    question: String,
    context: String,
    options: Vec<String>,
    allow_freeform: bool,
    selected: usize,
    freeform: TextArea,
    freeform_focused: bool,
}

/// An in-flight plugin modal dialog (title + body + buttons).
#[derive(Debug, Clone)]
pub struct ModalState {
    plugin: String,
    id: u64,
    title: String,
    body: Vec<String>,
    buttons: Vec<String>,
    selected: usize,
}

impl ModalState {
    /// Originating plugin.
    #[must_use]
    pub fn plugin(&self) -> &str {
        &self.plugin
    }
    /// Modal title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
    /// Body lines.
    #[must_use]
    pub fn body(&self) -> &[String] {
        &self.body
    }
    /// Button labels.
    #[must_use]
    pub fn buttons(&self) -> &[String] {
        &self.buttons
    }
    /// Highlighted button index.
    #[must_use]
    pub fn selected(&self) -> usize {
        self.selected
    }
}

/// Canonicalizes a key-chord string (`"Ctrl+ G"` → `"ctrl+g"`) through the
/// shared [`rstui_keymap::Chord`] parser (ADR 0015), so a plugin-declared
/// chord and a host-derived one compare equal *and* use the exact same
/// vocabulary the kitchen sink does. Unparseable input falls back to a
/// lowercased trim (lenient, never panics).
#[must_use]
pub fn normalize_chord(s: &str) -> String {
    Chord::parse(s).map_or_else(|| s.trim().to_ascii_lowercase(), |c| c.spec())
}

/// The canonical chord for a key event, or `None` for a bare printable key
/// (those type into the composer and must never be stolen as a shortcut).
///
/// The chord itself comes from the shared [`rstui_keymap::Chord`] so it is
/// byte-identical to what [`normalize_chord`] produces for a registered
/// keybinding; the gate (must carry ctrl/alt/super, or be a function key)
/// is the client's own "shortcut vs composer input" policy.
#[must_use]
fn chord_of(key: &KeyEvent) -> Option<String> {
    let m = key.modifiers;
    let is_fn = matches!(key.code, KeyCode::F(_));
    if !(m.contains(KeyModifiers::CONTROL)
        || m.contains(KeyModifiers::ALT)
        || m.contains(KeyModifiers::SUPER)
        || is_fn)
    {
        return None;
    }
    Some(Chord::from_event(key).spec())
}

/// A transient corner notification.
#[derive(Debug, Clone)]
pub struct Toast {
    /// Body text.
    pub text: String,
    /// Age in ticks (dropped past a threshold).
    pub age: usize,
}

/// The full-screen transcript pager (Codex's `/transcript`): a scrollable,
/// searchable projection of the whole rendered transcript. The reducer owns
/// these few fields; the renderer is a pure projection that clamps `scroll`
/// to the wrapped content and applies `query` as a line filter — so this is
/// the entire testable surface.
#[derive(Debug, Default)]
pub struct PagerState {
    open: bool,
    scroll: u16,
    follow: bool,
    query: String,
    searching: bool,
}

impl PagerState {
    /// Whether the pager overlay is visible.
    #[must_use]
    pub fn open(&self) -> bool {
        self.open
    }
    /// Top scroll offset (wrapped rows); the renderer clamps to content.
    #[must_use]
    pub fn scroll(&self) -> u16 {
        self.scroll
    }
    /// Sticking to the bottom (latest) — set on open and `G`/`End`.
    #[must_use]
    pub fn follows(&self) -> bool {
        self.follow
    }
    /// The case-insensitive substring filter (empty = show everything).
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }
    /// `true` while the search query line is being typed (after `/`).
    #[must_use]
    pub fn searching(&self) -> bool {
        self.searching
    }
}

/// The `/diff` overlay: a captured `git diff HEAD` plus the scroll offset
/// (clamp-on-render, the pager's model).
#[derive(Debug, Clone)]
pub struct DiffView {
    text: String,
    scroll: u16,
}

impl DiffView {
    /// The captured diff text (`(no changes)` when the tree is clean).
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
    /// The scroll offset in wrapped rows (renderer clamps to content).
    #[must_use]
    pub fn scroll(&self) -> u16 {
        self.scroll
    }
}

/// Where a slash command comes from (drives its autocomplete tag/colour).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSource {
    /// A client built-in.
    Builtin,
    /// Contributed by a plugin (carries the plugin name for routing).
    Plugin(String),
    /// Advertised by the connected agent (`available_commands_update`).
    Agent,
}

/// One entry in the merged slash-command set.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    /// Command name without the leading slash.
    pub name: String,
    /// One-line help text.
    pub description: String,
    /// Its origin.
    pub source: CommandSource,
}

/// The live slash-command autocomplete popup state.
#[derive(Debug, Clone)]
pub struct Completion {
    /// Filtered candidates (already ranked, capped at [`COMPLETION_MAX`]).
    pub items: Vec<CommandSpec>,
    /// Highlighted index into `items`.
    pub selected: usize,
}

/// The live `@`-mention file-completion popup (Codex's `@` file mention):
/// fuzzy-matched workspace paths for the `@token` at the cursor.
#[derive(Debug, Clone)]
pub struct MentionState {
    /// The bounded cwd file scan, cached while the same `@token` is active
    /// so keystrokes only re-rank, never re-walk the tree.
    candidates: Vec<String>,
    /// Ranked, capped paths shown in the popup.
    pub items: Vec<String>,
    /// Highlighted index into `items`.
    pub selected: usize,
    /// Composer row the `@` is on.
    row: usize,
    /// Character column of the `@` on that row (the replace-span start).
    at_col: usize,
}

/// Built-in slash commands, shown in autocomplete + `/help`.
pub const BUILTIN_COMMANDS: &[(&str, &str)] = &[
    ("help", "Show keys & commands"),
    ("agents", "Switch agent (open the registry picker)"),
    ("new", "New session (back to the agent picker)"),
    ("clear", "Clear the transcript"),
    ("todos", "Toggle the todo sidebar"),
    ("details", "Show/hide completed tool-call output"),
    ("plugins", "Show loaded plugins, commands & status"),
    ("log", "Toggle the diagnostic log"),
    (
        "transcript",
        "Open the full-screen transcript pager (search with /)",
    ),
    ("status", "Show session info & token usage"),
    ("model", "Choose the model (if the agent offers a choice)"),
    ("mode", "Switch the session mode (plan / approval / …)"),
    ("resume", "Resume a previous session (session/load)"),
    ("login", "Sign in to the agent (if it requires auth)"),
    ("diff", "Show the working-tree git diff"),
    ("theme", "Pick a colour theme (browse + preview live)"),
    ("init", "Ask the agent to create/improve AGENTS.md"),
    ("review", "Ask the agent to review your uncommitted changes"),
    ("copy", "Copy the last agent answer to the clipboard"),
    ("bell", "Toggle the turn-completion bell"),
    ("cancel", "Interrupt the streaming turn"),
    ("quit", "Exit the client"),
];

/// Max rows shown in the autocomplete popup (opencode parity).
pub const COMPLETION_MAX: usize = 10;

/// The `/init` canned prompt — agent-agnostic (works with any ACP agent),
/// the same task Codex's `/init` performs.
pub const INIT_PROMPT: &str = "Explore this repository and create an AGENTS.md \
file at its root with concise, accurate instructions for AI coding agents \
working here: how to build, test and lint; the project conventions; and \
anything non-obvious an agent must know. If an AGENTS.md already exists, \
review and improve it instead of duplicating it.";

/// The `/review` canned prompt — Codex's "review current changes".
pub const REVIEW_PROMPT: &str = "Review my current uncommitted changes (the \
git diff, including untracked files). Identify bugs, regressions, missing \
tests, and anything that should change before this is committed. Be specific \
and cite files and lines.";

/// Messages the reducer folds. Input is normalized to [`Msg::Key`] /
/// [`Msg::Resize`] in `on_event` so all focus routing lives in `update`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// One-shot boot: launch plugins + registry / agent.
    Boot,
    /// A key press.
    Key(KeyEvent),
    /// Terminal resized.
    Resize(Size),
    /// Mouse wheel (positive = up / back in history).
    Scroll(i32),
    /// Bracketed-paste text into the composer.
    Paste(String),
    /// The registry finished loading.
    RegistryLoaded(Box<Registry>),
    /// `git diff` finished (the `/diff` overlay payload).
    DiffLoaded(String),
    /// A driver event (re-arms the drain unless terminal).
    Acp(AcpEvent),
    /// A plugin action (re-arms the drain).
    Plugin(PluginEvent),
    /// Animation / housekeeping tick.
    Tick,
    /// Begin shutdown.
    Quit,
}

/// The full-screen ACP chat client application state.
pub struct ChatApp {
    config: Config,
    cwd: PathBuf,
    screen: Screen,
    registry: Registry,
    picker_selected: usize,
    /// `Some` while the picker's inline "custom ACP command" input is open
    /// (typing a local-stdio command to launch instead of a registry agent).
    picker_custom: Option<TextArea>,
    transcript: Vec<Entry>,
    scroll: u16,
    follow: bool,
    /// The full-screen `/transcript` pager (scroll + search) overlay state.
    pager: PagerState,
    composer: TextArea,
    /// Submitted-prompt history, recalled with ↑/↓ on the composer and
    /// persisted across runs (readline / Codex-CLI ergonomics).
    history: InputHistory,
    status_line: String,
    agent_label: String,
    /// The last terminal title emitted via OSC 2; the next refresh only
    /// emits when the derived title differs (no per-frame escape spam).
    last_title: String,
    /// Ring the terminal bell when a turn ends (the "your turn" cue).
    /// Defaults on; `RSTUI_ACP_BELL=0|false|no|off` starts it off; `/bell`
    /// toggles it for the session.
    bell_enabled: bool,
    streaming: bool,
    spinner: usize,
    driver: Option<DriverHandle>,
    plugins: Option<PluginHost>,
    pending_permission: Option<PendingPermission>,
    ask: Option<AskState>,
    footers: BTreeMap<String, Vec<FooterSegment>>,
    statuses: BTreeMap<String, String>,
    commands: BTreeMap<String, (String, String)>,
    agent_commands: BTreeMap<String, String>,
    completion: Option<Completion>,
    /// The `@`-mention file-completion popup, if active.
    mention: Option<MentionState>,
    todos: Vec<TodoEntry>,
    sidebar: SidebarMode,
    tool_calls: Vec<ToolCallInfo>,
    /// `tool_call.id` → its index in `tool_calls` (APP-4). `tool_calls` is
    /// strictly append-only (verified: only `push` + in-place patch, never
    /// removed/reordered/cleared), so the index stays valid for the session.
    /// `tool_call()` is read once per Tool transcript entry every frame; this
    /// turns that O(toolEntries × toolCalls) per-frame scan into O(1).
    tool_index: std::collections::HashMap<String, usize>,
    details: bool,
    panels: BTreeMap<String, (String, Vec<String>)>,
    show_plugins: bool,
    /// Plugin keybindings: canonical chord → (plugin, command, description).
    keybindings: BTreeMap<String, (String, String, String)>,
    modal: Option<ModalState>,
    toasts: Vec<Toast>,
    log: Vec<String>,
    show_log: bool,
    show_help: bool,
    /// `/status` overlay open.
    show_status: bool,
    /// Latest ACP `usage_update`: `(tokens_in_context, context_window_size)`.
    /// `None` until the agent reports usage (many agents do every turn).
    usage: Option<(u64, u64)>,
    /// The agent's advertised model catalogue (empty until `NewSessionResponse`
    /// reports one) and the active model id.
    models: Vec<ModelOption>,
    current_model: Option<String>,
    /// `/model` picker overlay open + its highlighted row.
    model_picker_open: bool,
    model_sel: usize,
    /// The agent's session modes (ACP `SessionModeState`, ungated) and the
    /// active mode id — how Codex's plan/approval modes reach the client.
    modes: Vec<ModeOption>,
    current_mode: Option<String>,
    /// `/mode` picker overlay open + its highlighted row.
    mode_picker_open: bool,
    mode_sel: usize,
    /// Persisted index of sessions this client started (`/resume`), and the
    /// picker overlay state.
    sessions: SessionStore,
    resume_picker_open: bool,
    resume_sel: usize,
    /// Agent auth methods (ACP `authenticate`) + sign-in picker state. The
    /// picker auto-opens when the agent reports auth is required.
    auth_methods: Vec<AuthOption>,
    auth_picker_open: bool,
    auth_sel: usize,
    /// The `/diff` overlay (a captured `git diff`), if open.
    diff: Option<DiffView>,
    last_size: Size,
    quitting: bool,
    /// Live render-rate meter (the reusable [`rstui_widgets::FpsMeter`]),
    /// sampled once per frame in `view` and shown in the header so the
    /// client's performance is always visible.
    fps: rstui_widgets::FpsMeter,
    /// The active colour theme (any of the 36 gpui-component themes,
    /// resolved from `RSTUI_THEME` / the saved choice / the default).
    theme: crate::theme::AcpTheme,
    /// Reusable theme-picker state, driven while [`picking`](Self::picking).
    theme_picker: rstui_theme::ThemePickerState,
    /// The theme to restore if the picker is cancelled with `Esc`.
    theme_restore: Option<crate::theme::AcpTheme>,
    /// `true` while the `/theme` picker overlay is open.
    picking: bool,
    /// The customisable keymap (the app-owned global-command map;
    /// `RSTUI_KEYMAP` may load user overrides). Resolved before the
    /// screen dispatch, after the plugin-chord layer.
    keymaps: Keymaps,
    /// The keymap settings panel (the shared `KeymapView` widget) is open.
    keymap_panel: bool,
    /// Selected row in the keymap panel.
    km_sel: usize,
    /// The command armed for capture-to-rebind (next key binds it).
    km_rebind: Option<Action>,
}

impl ChatApp {
    /// Builds the app from resolved [`Config`]. The terminal is not yet taken
    /// over; all I/O is deferred to [`Msg::Boot`].
    #[must_use]
    pub fn new(config: Config) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            config,
            cwd,
            screen: Screen::Picker,
            registry: Registry::default(),
            picker_selected: 0,
            picker_custom: None,
            transcript: Vec::new(),
            scroll: 0,
            follow: true,
            pager: PagerState::default(),
            composer: TextArea::new(),
            history: InputHistory::load(),
            status_line: "starting…".to_owned(),
            agent_label: String::new(),
            last_title: String::new(),
            bell_enabled: bell_default(),
            streaming: false,
            spinner: 0,
            driver: None,
            plugins: None,
            pending_permission: None,
            ask: None,
            footers: BTreeMap::new(),
            statuses: BTreeMap::new(),
            commands: BTreeMap::new(),
            agent_commands: BTreeMap::new(),
            completion: None,
            mention: None,
            todos: Vec::new(),
            sidebar: SidebarMode::Auto,
            tool_calls: Vec::new(),
            tool_index: std::collections::HashMap::new(),
            details: true,
            panels: BTreeMap::new(),
            show_plugins: false,
            keybindings: BTreeMap::new(),
            modal: None,
            toasts: Vec::new(),
            log: Vec::new(),
            show_log: false,
            show_help: false,
            show_status: false,
            usage: None,
            models: Vec::new(),
            current_model: None,
            model_picker_open: false,
            model_sel: 0,
            modes: Vec::new(),
            current_mode: None,
            mode_picker_open: false,
            mode_sel: 0,
            sessions: SessionStore::load(),
            resume_picker_open: false,
            resume_sel: 0,
            auth_methods: Vec::new(),
            auth_picker_open: false,
            auth_sel: 0,
            diff: None,
            last_size: Size::new(80, 24),
            quitting: false,
            fps: rstui_widgets::FpsMeter::new(),
            theme: crate::theme::startup_theme(),
            theme_picker: rstui_theme::ThemePickerState::new(),
            theme_restore: None,
            picking: false,
            keymaps: acp_keymaps(),
            keymap_panel: false,
            km_sel: 0,
            km_rebind: None,
        }
    }

    /// Apply a user keymap choice: a built-in map **name** (only
    /// `"acp-client"` here) or a path to a `RSTUI_KEYMAP` config file
    /// (`id = keys` lines — see `docs/keymaps.md`). Unknown name /
    /// unreadable file keeps the defaults; a typo never breaks your keys.
    /// Mirrors `RSTUI_THEME`.
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

    // ---- read-only accessors (used by the view + Harness tests) ----

    /// The current screen.
    #[must_use]
    pub fn screen(&self) -> Screen {
        self.screen
    }
    /// The transcript entries.
    #[must_use]
    pub fn transcript(&self) -> &[Entry] {
        &self.transcript
    }
    /// The text `/copy` would place on the clipboard (the most recent agent
    /// answer), or `None` if the agent has not answered yet.
    #[must_use]
    pub fn last_response(&self) -> Option<&str> {
        self.last_agent_text()
    }
    /// The registry as loaded (possibly the offline fallback).
    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
    /// The picker's selected index.
    #[must_use]
    pub fn picker_selected(&self) -> usize {
        self.picker_selected
    }
    /// The picker's inline custom-command input, while open (`c` opens it).
    #[must_use]
    pub fn picker_custom(&self) -> Option<&TextArea> {
        self.picker_custom.as_ref()
    }
    /// The connection status line.
    #[must_use]
    pub fn status_line(&self) -> &str {
        &self.status_line
    }
    /// The terminal title last emitted via OSC 2 (the pure `session_title`
    /// of the current session state); empty before the first refresh.
    /// Exposed so `Harness` tests pin the title without a terminal.
    #[must_use]
    pub fn terminal_title(&self) -> &str {
        &self.last_title
    }
    /// Whether the turn-completion bell is armed (`/bell` toggles it,
    /// `RSTUI_ACP_BELL` sets the startup default).
    #[must_use]
    pub fn bell_enabled(&self) -> bool {
        self.bell_enabled
    }
    /// The live render-rate label (`"NNN fps"`, or `"--- fps"` before the
    /// first usable sample / under the synchronous test harness).
    #[must_use]
    pub fn fps_label(&self) -> String {
        self.fps.label()
    }
    /// The composer document.
    #[must_use]
    pub fn composer(&self) -> &TextArea {
        &self.composer
    }
    /// The persisted submitted-prompt history (↑/↓ recall).
    #[must_use]
    pub fn history(&self) -> &InputHistory {
        &self.history
    }
    /// Whether a turn is streaming.
    #[must_use]
    pub fn is_streaming(&self) -> bool {
        self.streaming
    }
    /// The pending permission prompt, if any.
    #[must_use]
    pub fn pending_permission(&self) -> Option<&PendingPermission> {
        self.pending_permission.as_ref()
    }
    /// The active plugin modal, if any.
    #[must_use]
    pub fn modal(&self) -> Option<&ModalState> {
        self.modal.as_ref()
    }
    /// Plugin keybindings: chord → (plugin, command, description).
    #[must_use]
    pub fn keybindings(&self) -> &BTreeMap<String, (String, String, String)> {
        &self.keybindings
    }
    /// The ask-user overlay, if any.
    #[must_use]
    pub fn ask(&self) -> Option<&AskState> {
        self.ask.as_ref()
    }
    /// Whether the help overlay is open.
    #[must_use]
    pub fn help_visible(&self) -> bool {
        self.show_help
    }

    /// The keymap settings panel is open (rendered by `ui::render`).
    #[must_use]
    pub fn keymap_panel_open(&self) -> bool {
        self.keymap_panel
    }

    /// The keymap panel rows, projected from the live keymap.
    #[must_use]
    pub fn keymap_panel_rows(&self) -> Vec<rstui_widgets::KeymapRow<'static>> {
        self.keymap_rows()
    }

    /// `(active map name, OS label, capturing?)` for the panel chrome.
    #[must_use]
    pub fn keymap_panel_status(&self) -> (&'static str, &'static str, bool) {
        (
            self.keymaps.active_name(),
            Keymaps::os_name(),
            self.km_rebind.is_some(),
        )
    }

    /// The active colour theme.
    #[must_use]
    pub fn theme(&self) -> &crate::theme::AcpTheme {
        &self.theme
    }

    /// The theme-picker state (rendered while [`picking`](Self::picking)).
    #[must_use]
    pub fn theme_picker(&self) -> &rstui_theme::ThemePickerState {
        &self.theme_picker
    }

    /// Whether the `/theme` picker overlay is open.
    #[must_use]
    pub fn picking(&self) -> bool {
        self.picking
    }
    /// Whether the log overlay is open.
    #[must_use]
    pub fn log_visible(&self) -> bool {
        self.show_log
    }
    /// Whether the `/status` overlay is open.
    #[must_use]
    pub fn status_visible(&self) -> bool {
        self.show_status
    }
    /// Latest context-window usage `(used, size)` from ACP `usage_update`,
    /// or `None` if the agent has not reported any.
    #[must_use]
    pub fn usage(&self) -> Option<(u64, u64)> {
        self.usage
    }
    /// The agent's advertised models (empty if it advertised none).
    #[must_use]
    pub fn models(&self) -> &[ModelOption] {
        &self.models
    }
    /// The active model's id, if known.
    #[must_use]
    pub fn current_model(&self) -> Option<&str> {
        self.current_model.as_deref()
    }
    /// The active model's display name (falls back to its id, then `—`).
    #[must_use]
    pub fn current_model_name(&self) -> String {
        match self.current_model.as_deref() {
            Some(id) => self
                .models
                .iter()
                .find(|m| m.id == id)
                .map_or_else(|| id.to_owned(), |m| m.name.clone()),
            None => "—".to_owned(),
        }
    }
    /// Whether the `/model` picker overlay is open.
    #[must_use]
    pub fn model_picker_open(&self) -> bool {
        self.model_picker_open
    }
    /// The highlighted row in the `/model` picker.
    #[must_use]
    pub fn model_sel(&self) -> usize {
        self.model_sel
    }
    /// The agent's advertised session modes (empty if it advertised none).
    #[must_use]
    pub fn modes(&self) -> &[ModeOption] {
        &self.modes
    }
    /// The active mode's id, if known.
    #[must_use]
    pub fn current_mode(&self) -> Option<&str> {
        self.current_mode.as_deref()
    }
    /// The active mode's display name (falls back to its id, then `—`).
    #[must_use]
    pub fn current_mode_name(&self) -> String {
        match self.current_mode.as_deref() {
            Some(id) => self
                .modes
                .iter()
                .find(|m| m.id == id)
                .map_or_else(|| id.to_owned(), |m| m.name.clone()),
            None => "—".to_owned(),
        }
    }
    /// Whether the `/mode` picker overlay is open.
    #[must_use]
    pub fn mode_picker_open(&self) -> bool {
        self.mode_picker_open
    }
    /// The highlighted row in the `/mode` picker.
    #[must_use]
    pub fn mode_sel(&self) -> usize {
        self.mode_sel
    }
    /// The resumable sessions this client has started, newest first.
    #[must_use]
    pub fn resume_sessions(&self) -> Vec<SessionRef> {
        self.sessions.newest_first()
    }
    /// Whether the `/resume` picker overlay is open.
    #[must_use]
    pub fn resume_picker_open(&self) -> bool {
        self.resume_picker_open
    }
    /// The highlighted row in the `/resume` picker.
    #[must_use]
    pub fn resume_sel(&self) -> usize {
        self.resume_sel
    }
    /// The agent's auth methods (empty until it reports auth is required).
    #[must_use]
    pub fn auth_methods(&self) -> &[AuthOption] {
        &self.auth_methods
    }
    /// Whether the sign-in picker overlay is open.
    #[must_use]
    pub fn auth_picker_open(&self) -> bool {
        self.auth_picker_open
    }
    /// The highlighted row in the sign-in picker.
    #[must_use]
    pub fn auth_sel(&self) -> usize {
        self.auth_sel
    }
    /// The `/diff` overlay (a captured `git diff`), if open.
    #[must_use]
    pub fn diff(&self) -> Option<&DiffView> {
        self.diff.as_ref()
    }
    /// The launch command of the connected agent (empty before connect).
    #[must_use]
    pub fn agent_command(&self) -> &str {
        &self.agent_label
    }
    /// The working directory the session runs in.
    #[must_use]
    pub fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }
    /// The merged plugin footer segments (in plugin-name order), borrowed.
    ///
    /// UI-3: returns an iterator of `&FooterSegment` rather than a freshly
    /// `cloned().collect()`ed `Vec` — `render_footer` only reads them, and a
    /// powerline-style plugin updates the footer continuously, so the old
    /// deep clone of the whole `BTreeMap<_, Vec<FooterSegment>>` ran every
    /// frame for nothing.
    pub fn footer_segments(&self) -> impl Iterator<Item = &FooterSegment> + '_ {
        self.footers.values().flatten()
    }
    /// Plugin status keys.
    #[must_use]
    pub fn statuses(&self) -> &BTreeMap<String, String> {
        &self.statuses
    }
    /// Registered slash commands: name → (plugin, description).
    #[must_use]
    pub fn commands(&self) -> &BTreeMap<String, (String, String)> {
        &self.commands
    }
    /// The live slash-command autocomplete popup, if visible.
    #[must_use]
    pub fn completion(&self) -> Option<&Completion> {
        self.completion.as_ref()
    }
    /// The live `@`-mention file-completion popup, if visible.
    #[must_use]
    pub fn mention(&self) -> Option<&MentionState> {
        self.mention.as_ref()
    }
    /// The agent's current execution plan (todos), newest plan wins.
    #[must_use]
    pub fn todos(&self) -> &[TodoEntry] {
        &self.todos
    }
    /// All tool calls seen this session, in arrival order.
    #[must_use]
    pub fn tool_calls(&self) -> &[ToolCallInfo] {
        &self.tool_calls
    }
    /// Looks up a tool call by its ACP id (the transcript anchor key).
    #[must_use]
    pub fn tool_call(&self, id: &str) -> Option<&ToolCallInfo> {
        self.tool_index.get(id).map(|&i| &self.tool_calls[i])
    }
    /// Whether completed tool calls show their output body (`/details`).
    #[must_use]
    pub fn details(&self) -> bool {
        self.details
    }
    /// Whether the todo sidebar should be drawn (resolves [`SidebarMode`]).
    #[must_use]
    pub fn sidebar_visible(&self) -> bool {
        match self.sidebar {
            SidebarMode::Hidden => false,
            SidebarMode::Shown => true,
            SidebarMode::Auto => self.has_open_todos() || self.has_plugin_surface(),
        }
    }
    /// `true` while there is unfinished todo work.
    #[must_use]
    pub fn has_open_todos(&self) -> bool {
        !self.todos.is_empty() && self.todos.iter().any(|t| t.status != TodoStatus::Completed)
    }
    /// `true` when a plugin contributes sidebar content (panel/status keys),
    /// so the sidebar is worth showing even before any todos exist.
    #[must_use]
    pub fn has_plugin_surface(&self) -> bool {
        !self.panels.is_empty() || !self.statuses.is_empty()
    }
    /// Plugin-contributed sidebar panels: `plugin → (title, body lines)`.
    #[must_use]
    pub fn panels(&self) -> &BTreeMap<String, (String, Vec<String>)> {
        &self.panels
    }
    /// Whether the `/plugins` overlay is open.
    #[must_use]
    pub fn plugins_overlay(&self) -> bool {
        self.show_plugins
    }
    /// Names of the running plugins (empty headless).
    #[must_use]
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins
            .as_ref()
            .map(|h| h.names().to_vec())
            .unwrap_or_default()
    }
    /// Every plugin known by *any* surface: a running host process, a
    /// registered command, a contributed panel, or a status key it owns.
    /// (The host list is empty headless, so the union is what makes the
    /// `/plugins` overlay correct in tests *and* production.)
    #[must_use]
    pub fn plugin_set(&self) -> Vec<String> {
        let mut set: std::collections::BTreeSet<String> = self.plugin_names().into_iter().collect();
        for (plugin, _) in self.commands.values() {
            set.insert(plugin.clone());
        }
        set.extend(self.panels.keys().cloned());
        set.into_iter().collect()
    }
    /// `(completed, total)` todo counts for the sidebar header.
    #[must_use]
    pub fn todo_progress(&self) -> (usize, usize) {
        let done = self
            .todos
            .iter()
            .filter(|t| t.status == TodoStatus::Completed)
            .count();
        (done, self.todos.len())
    }
    /// Active toasts.
    #[must_use]
    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }
    /// The plugin diagnostic log.
    #[must_use]
    pub fn log(&self) -> &[String] {
        &self.log
    }

    /// Appends a diagnostic line, capping retained history (APP-3).
    ///
    /// `log` is fed by every plugin `Log`, status change, and **agent stderr
    /// line** (npx agents are chatty), so an unbounded `Vec<String>` grew
    /// forever over a long session — wasted memory, and it amplified the
    /// per-frame work while the `/log` overlay is open. Stays a `Vec<String>`
    /// (so the `log()` `&[String]` accessor and its slice consumer are
    /// unchanged); when it exceeds the cap, the oldest quarter is dropped in
    /// one shift so trimming is amortized O(1) per line, never an O(n)
    /// memmove on every push at the cap.
    fn push_log(&mut self, line: String) {
        const LOG_CAP: usize = 2000;
        self.log.push(line);
        if self.log.len() > LOG_CAP {
            let drop = self.log.len() - LOG_CAP * 3 / 4;
            self.log.drain(0..drop);
        }
    }
    /// The spinner frame index.
    #[must_use]
    pub fn spinner_frame(&self) -> usize {
        self.spinner
    }
    /// The transcript scroll offset (rows).
    #[must_use]
    pub fn scroll(&self) -> u16 {
        self.scroll
    }
    /// The full-screen `/transcript` pager state (scroll + search).
    #[must_use]
    pub fn pager(&self) -> &PagerState {
        &self.pager
    }

    // ---- internal helpers ----

    fn push_system(&mut self, text: impl Into<String>) {
        self.transcript.push(Entry {
            role: Role::System,
            text: text.into(),
            open: false,
            md_cache: None,
        });
        self.follow = true;
        self.cap_transcript();
    }

    fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast {
            text: text.into(),
            age: 0,
        });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    /// Recomputes the session terminal title and emits it via OSC 2 *only*
    /// when it changed since the last emit (best-effort, terminal-gated, so a
    /// no-op under `cargo test`). The `last_title` is still updated headless
    /// so tests can assert it.
    fn refresh_terminal_title(&mut self) {
        let title = crate::title::session_title(
            self.screen,
            &self.agent_label,
            self.streaming,
            self.pending_permission.is_some(),
        );
        if title != self.last_title {
            crate::title::set(&title);
            self.last_title = title;
        }
    }

    /// The most recent agent answer's text (the `/copy` payload), or `None`
    /// when the agent has not answered yet. Walks back over thoughts, tool
    /// calls, plans and system lines to the last `Role::Agent` prose entry —
    /// Codex's "copy last response as markdown".
    #[must_use]
    fn last_agent_text(&self) -> Option<&str> {
        self.transcript
            .iter()
            .rev()
            .find(|e| e.role == Role::Agent)
            .map(|e| e.text.as_str())
    }

    fn append_agent(&mut self, role: Role, chunk: &str) {
        if let Some(last) = self.transcript.last_mut() {
            if last.role == role && last.open {
                last.text.push_str(chunk);
                return;
            }
        }
        if let Some(last) = self.transcript.last_mut() {
            last.open = false;
        }
        self.transcript.push(Entry {
            role,
            text: chunk.to_owned(),
            open: true,
            md_cache: None,
        });
        self.follow = true;
        self.cap_transcript();
    }

    fn close_open_entry(&mut self) {
        if let Some(last) = self.transcript.last_mut() {
            last.open = false;
        }
    }

    /// UI-1/MD-1: populate the caller-owned markdown parse cache.
    ///
    /// Called once at the end of every `update`. **Only the last transcript
    /// entry can ever have its `text` change**: `append_agent` mutates only
    /// `transcript.last_mut()`, `close_open_entry` only flips `last.open`,
    /// and `cap_transcript` only drains whole entries off the front. So any
    /// `Role::Agent` entry that is no longer the last one has immutable
    /// text and its parse can be cached exactly once and reused forever.
    /// The still-streaming last entry is left uncached and re-parsed fresh
    /// by the renderer every frame, so the rendered output is byte-identical
    /// to the pre-cache behaviour — this purely removes the O(history)
    /// markdown re-parse the renderer used to pay for the whole transcript
    /// every frame (the audit's #1 cost, ~1.49 ms × N agent entries/frame).
    /// `Markdown::new(..).lines(MD_WIDTH)` is exactly what the renderer
    /// computes; the `paragraph`/`md_cache` exactness test gate-enforces it.
    fn refresh_md_cache(&mut self) {
        let n = self.transcript.len();
        for (i, e) in self.transcript.iter_mut().enumerate() {
            if e.role == Role::Agent && i + 1 < n && e.md_cache.is_none() {
                e.md_cache = Some(Markdown::new(&e.text).lines(MD_WIDTH));
            }
        }
    }

    /// Caps retained transcript history (APP-1).
    ///
    /// `transcript` is fed by every system line, agent turn, user prompt and
    /// tool/plan anchor and was never trimmed — unbounded memory over a long
    /// session, and it multiplied the per-frame re-derivation in `view`. The
    /// cap is deliberately generous (well past any test, so snapshots are
    /// unchanged); when exceeded, the oldest quarter is dropped in one shift
    /// so trimming is amortized O(1), and a one-line sentinel marks the cut.
    /// Tool/plan lookups are keyed by id (a separate append-only map), not by
    /// transcript index, so a front trim is safe.
    fn cap_transcript(&mut self) {
        const CAP: usize = 4000;
        const SENTINEL: &str = "── earlier history truncated ──";
        if self.transcript.len() <= CAP {
            return;
        }
        let drop = self.transcript.len() - CAP * 3 / 4;
        self.transcript.drain(0..drop);
        if self.transcript.first().map(|e| e.text.as_str()) != Some(SENTINEL) {
            self.transcript.insert(
                0,
                Entry {
                    role: Role::System,
                    text: SENTINEL.to_owned(),
                    open: false,
                    md_cache: None,
                },
            );
        }
    }

    fn acp_subscription(handle: DriverHandle) -> Cmd<Msg> {
        Cmd::perform(move || {
            let event = handle
                .recv_blocking()
                .unwrap_or_else(|| AcpEvent::Disconnected("driver ended".to_owned()));
            Msg::Acp(event)
        })
    }

    fn plugin_subscription(host: PluginHost) -> Cmd<Msg> {
        Cmd::perform(move || match host.recv_blocking() {
            Some(ev) => Msg::Plugin(ev),
            None => Msg::Tick,
        })
    }

    fn connect(&mut self, command: String) -> Cmd<Msg> {
        self.agent_label = command.clone();
        self.screen = Screen::Connecting;
        self.status_line = format!("connecting: {command}");
        let handle = spawn_driver(command.clone(), self.cwd.clone());
        self.driver = Some(handle.clone());
        if let Some(host) = &self.plugins {
            host.broadcast(&HostEvent::SessionStart { agent: command });
        }
        Self::acp_subscription(handle)
    }

    fn submit_composer(&mut self) -> Cmd<Msg> {
        let text = self.composer.lines().join("\n");
        let text = text.trim().to_owned();
        if text.is_empty() {
            return Cmd::none();
        }
        self.composer.clear();
        self.follow = true;
        // Record every submission (slash commands included, like Codex) so
        // ↑ recalls it; this also ends any in-progress history browse.
        self.history.record(&text);
        self.history.save();

        if let Some(rest) = text.strip_prefix('/') {
            return self.run_slash(rest);
        }

        self.send_user_prompt(text);
        Cmd::none()
    }

    /// Sends `text` as a user turn: a transcript entry, the ACP prompt, and
    /// the plugin `UserPrompt` broadcast — the one place that lives, shared
    /// by the composer and the canned-prompt builtins (`/init`, `/review`).
    /// A no-op (with a system breadcrumb) when no agent is connected.
    fn send_user_prompt(&mut self, text: String) {
        if self.driver.is_none() {
            self.push_system("not connected — pick an agent with /agents");
            return;
        }
        self.transcript.push(Entry {
            role: Role::User,
            text: text.clone(),
            open: false,
            md_cache: None,
        });
        self.cap_transcript();
        self.streaming = true;
        if let Some(driver) = &self.driver {
            driver.send(DriverCmd::Prompt(text.clone()));
        }
        if let Some(host) = &self.plugins {
            host.broadcast(&HostEvent::UserPrompt { text });
        }
    }

    fn run_slash(&mut self, rest: &str) -> Cmd<Msg> {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_owned();
        let args = parts.next().unwrap_or("").trim().to_owned();
        match name.as_str() {
            "help" => {
                self.show_help = true;
                Cmd::none()
            }
            "agents" | "new" => {
                self.screen = Screen::Picker;
                Cmd::none()
            }
            "clear" => {
                self.transcript.clear();
                self.scroll = 0;
                self.follow = true;
                self.push_system("transcript cleared");
                Cmd::none()
            }
            "todos" => {
                self.sidebar = if self.sidebar == SidebarMode::Hidden {
                    SidebarMode::Auto
                } else {
                    SidebarMode::Hidden
                };
                Cmd::none()
            }
            "plugins" => {
                self.show_plugins = !self.show_plugins;
                Cmd::none()
            }
            "details" => {
                self.details = !self.details;
                self.push_system(if self.details {
                    "tool details: shown"
                } else {
                    "tool details: hidden (completed tools collapse)"
                });
                Cmd::none()
            }
            "log" => {
                self.show_log = !self.show_log;
                Cmd::none()
            }
            "transcript" => {
                self.pager = PagerState {
                    open: true,
                    follow: true,
                    ..PagerState::default()
                };
                Cmd::none()
            }
            "status" => {
                self.show_status = !self.show_status;
                Cmd::none()
            }
            "model" => {
                if self.models.is_empty() {
                    self.push_system(
                        "this agent did not advertise selectable models (it picks its own)",
                    );
                } else {
                    self.model_sel = self
                        .current_model
                        .as_deref()
                        .and_then(|id| self.models.iter().position(|m| m.id == id))
                        .unwrap_or(0);
                    self.model_picker_open = true;
                }
                Cmd::none()
            }
            "diff" => {
                self.push_system("running git diff…");
                let cwd = self.cwd.clone();
                Cmd::perform(move || Msg::DiffLoaded(run_git_diff(&cwd)))
            }
            "login" => {
                if self.auth_methods.is_empty() {
                    self.push_system(
                        "no sign-in needed (the agent did not advertise auth methods)",
                    );
                } else {
                    self.auth_sel = 0;
                    self.auth_picker_open = true;
                }
                Cmd::none()
            }
            "resume" => {
                if self.resume_sessions().is_empty() {
                    self.push_system("no saved sessions yet (a session is saved once it starts)");
                } else {
                    self.resume_sel = 0;
                    self.resume_picker_open = true;
                }
                Cmd::none()
            }
            "mode" => {
                if self.modes.is_empty() {
                    self.push_system("this agent did not advertise session modes");
                } else {
                    self.mode_sel = self
                        .current_mode
                        .as_deref()
                        .and_then(|id| self.modes.iter().position(|m| m.id == id))
                        .unwrap_or(0);
                    self.mode_picker_open = true;
                }
                Cmd::none()
            }
            "theme" => {
                // Open the reusable picker; remember the current palette so
                // Esc can restore it.
                self.theme_restore = Some(self.theme.clone());
                self.picking = true;
                Cmd::none()
            }
            "copy" => {
                match self.last_agent_text() {
                    Some(text) => {
                        let text = text.to_owned();
                        let n = text.len();
                        if crate::clipboard::copy(&text) {
                            self.push_system(format!("copied last response ({n} bytes)"));
                            self.toast("copied to clipboard");
                        } else {
                            // Headless / non-terminal: no OS hop, but say so
                            // rather than silently doing nothing.
                            self.push_system(format!(
                                "copy unavailable here (no terminal); {n} bytes not sent"
                            ));
                        }
                    }
                    None => self.push_system("nothing to copy yet (no agent response)"),
                }
                Cmd::none()
            }
            "init" => {
                self.send_user_prompt(INIT_PROMPT.to_owned());
                Cmd::none()
            }
            "review" => {
                self.send_user_prompt(REVIEW_PROMPT.to_owned());
                Cmd::none()
            }
            "bell" => {
                self.bell_enabled = !self.bell_enabled;
                self.push_system(if self.bell_enabled {
                    "turn-completion bell: on"
                } else {
                    "turn-completion bell: off"
                });
                Cmd::none()
            }
            "cancel" => {
                if let Some(driver) = &self.driver {
                    driver.send(DriverCmd::Cancel);
                    self.push_system("cancel requested");
                }
                Cmd::none()
            }
            "quit" | "exit" => self.begin_quit(),
            other => {
                if let Some((plugin, _)) = self.commands.get(other).cloned() {
                    if let Some(host) = &self.plugins {
                        host.send_to(
                            &plugin,
                            &HostEvent::Command {
                                name: other.to_owned(),
                                args,
                            },
                        );
                    }
                    self.push_system(format!("/{other} → {plugin}"));
                    Cmd::none()
                } else if self.agent_commands.contains_key(other) {
                    // Agent-advertised command: the agent owns it, so forward
                    // the whole `/name args` line as a prompt (opencode does
                    // the same for server commands).
                    if self.driver.is_none() {
                        self.push_system("not connected — pick an agent with /agents");
                        return Cmd::none();
                    }
                    let line = if args.is_empty() {
                        format!("/{other}")
                    } else {
                        format!("/{other} {args}")
                    };
                    self.transcript.push(Entry {
                        role: Role::User,
                        text: line.clone(),
                        open: false,
                        md_cache: None,
                    });
                    self.streaming = true;
                    if let Some(driver) = &self.driver {
                        driver.send(DriverCmd::Prompt(line.clone()));
                    }
                    if let Some(host) = &self.plugins {
                        host.broadcast(&HostEvent::UserPrompt { text: line });
                    }
                    Cmd::none()
                } else {
                    self.push_system(format!("unknown command: /{other} (try /help)"));
                    Cmd::none()
                }
            }
        }
    }

    /// The merged, de-duplicated, name-sorted slash-command set:
    /// built-ins, then plugin commands, then agent-advertised commands
    /// (first source for a given name wins, matching the resolution order).
    #[must_use]
    pub fn command_specs(&self) -> Vec<CommandSpec> {
        let mut by_name: BTreeMap<String, CommandSpec> = BTreeMap::new();
        for (name, desc) in BUILTIN_COMMANDS {
            by_name.entry((*name).to_owned()).or_insert(CommandSpec {
                name: (*name).to_owned(),
                description: (*desc).to_owned(),
                source: CommandSource::Builtin,
            });
        }
        for (name, (plugin, desc)) in &self.commands {
            by_name.entry(name.clone()).or_insert(CommandSpec {
                name: name.clone(),
                description: desc.clone(),
                source: CommandSource::Plugin(plugin.clone()),
            });
        }
        for (name, desc) in &self.agent_commands {
            by_name.entry(name.clone()).or_insert(CommandSpec {
                name: name.clone(),
                description: desc.clone(),
                source: CommandSource::Agent,
            });
        }
        by_name.into_values().collect()
    }

    /// The `/`-autocomplete query, or `None` when the popup must not show:
    /// the composer is a single line starting with `/` and no whitespace has
    /// been typed yet (typing an argument closes it — opencode parity).
    fn completion_query(&self) -> Option<String> {
        if self.composer.lines().len() != 1 {
            return None;
        }
        let line = self.composer.line(0).unwrap_or("");
        let rest = line.strip_prefix('/')?;
        if rest.contains(char::is_whitespace) {
            return None;
        }
        Some(rest.to_owned())
    }

    /// Recomputes [`Completion`] from the composer (called after every
    /// composer edit). Prefix matches rank above substring matches; the
    /// previously-selected command is kept selected when still present.
    fn refresh_completion(&mut self) {
        let Some(query) = self.completion_query() else {
            self.completion = None;
            return;
        };
        let q = query.to_ascii_lowercase();
        let mut scored: Vec<(u8, CommandSpec)> = self
            .command_specs()
            .into_iter()
            .filter_map(|c| {
                let lname = c.name.to_ascii_lowercase();
                if q.is_empty() {
                    Some((1, c))
                } else if lname.starts_with(&q) {
                    Some((0, c))
                } else if lname.contains(&q) || c.description.to_ascii_lowercase().contains(&q) {
                    Some((2, c))
                } else {
                    None
                }
            })
            .collect();
        scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
        let items: Vec<CommandSpec> = scored
            .into_iter()
            .take(COMPLETION_MAX)
            .map(|(_, c)| c)
            .collect();
        if items.is_empty() {
            self.completion = None;
            return;
        }
        let keep = self
            .completion
            .as_ref()
            .and_then(|c| c.items.get(c.selected))
            .map(|c| c.name.clone());
        let selected = keep
            .and_then(|n| items.iter().position(|c| c.name == n))
            .unwrap_or(0);
        self.completion = Some(Completion { items, selected });
    }

    /// Moves the autocomplete selection by `delta`, wrapping at both ends.
    fn move_completion(&mut self, delta: i32) {
        if let Some(c) = &mut self.completion {
            let n = c.items.len() as i32;
            if n > 0 {
                c.selected = (((c.selected as i32 + delta) % n + n) % n) as usize;
            }
        }
    }

    /// Accepts the highlighted completion. `run` (Enter) executes it now;
    /// otherwise (Tab) it inserts `/<name> ` so arguments can be typed.
    fn accept_completion(&mut self, run: bool) -> Cmd<Msg> {
        let Some(c) = &self.completion else {
            return Cmd::none();
        };
        let Some(spec) = c.items.get(c.selected).cloned() else {
            self.completion = None;
            return Cmd::none();
        };
        self.completion = None;
        if run {
            self.composer.set_value(format!("/{}", spec.name));
            self.submit_composer()
        } else {
            self.composer.set_value(format!("/{} ", spec.name));
            Cmd::none()
        }
    }

    /// The `@`-mention being typed: `(row, at_col, query)` where `at_col` is
    /// the character column of the `@` on the cursor's row. `Some` only when
    /// the cursor sits in an unbroken `@<non-ws>` token whose `@` starts a
    /// word (line start or after whitespace) — so `user@host` never triggers.
    fn mention_query(&self) -> Option<(usize, usize, String)> {
        let (row, col) = self.composer.cursor();
        let line = self.composer.line(row)?;
        let chars: Vec<char> = line.chars().collect();
        if col > chars.len() {
            return None;
        }
        let mut j = col;
        while j > 0 {
            let c = chars[j - 1];
            if c.is_whitespace() {
                return None;
            }
            if c == '@' {
                let at = j - 1;
                let word_start = at == 0 || chars[at - 1].is_whitespace();
                if !word_start {
                    return None;
                }
                let query: String = chars[at + 1..col].iter().collect();
                return Some((row, at, query));
            }
            j -= 1;
        }
        None
    }

    /// Recomputes the `@`-mention popup after a composer edit. The slash
    /// popup wins if active (mutually exclusive in practice). The cwd scan
    /// is cached while the same `@token` stays open, so only the ranking
    /// re-runs per keystroke.
    fn refresh_mention(&mut self) {
        if self.completion.is_some() {
            self.mention = None;
            return;
        }
        let Some((row, at_col, query)) = self.mention_query() else {
            self.mention = None;
            return;
        };
        // Take ownership of any prior state: reuse its cached scan when the
        // same `@token` is still open, else walk the tree fresh.
        let (candidates, prev_sel) = match self.mention.take() {
            Some(m) if m.row == row && m.at_col == at_col => {
                let sel = m.items.get(m.selected).cloned();
                (m.candidates, sel)
            }
            _ => (scan_files(&self.cwd), None),
        };
        let items = rank_paths(&candidates, &query, COMPLETION_MAX);
        if items.is_empty() {
            return; // self.mention is already None (taken above)
        }
        let selected = prev_sel
            .and_then(|k| items.iter().position(|i| *i == k))
            .unwrap_or(0);
        self.mention = Some(MentionState {
            candidates,
            items,
            selected,
            row,
            at_col,
        });
    }

    /// Moves the `@`-mention selection by `delta`, wrapping at both ends.
    fn move_mention(&mut self, delta: i32) {
        if let Some(m) = &mut self.mention {
            let n = m.items.len() as i32;
            if n > 0 {
                m.selected = (((m.selected as i32 + delta) % n + n) % n) as usize;
            }
        }
    }

    /// Accepts the highlighted file mention: replaces the `@query` span on
    /// its row with the chosen path and a trailing space. The path lands in
    /// the prompt text — the agent (Codex/Claude-Code/…) resolves `@path`
    /// mentions itself, exactly the Codex composer UX.
    fn accept_mention(&mut self) {
        let Some(m) = self.mention.take() else { return };
        let Some(path) = m.items.get(m.selected).cloned() else {
            return;
        };
        let (_, col) = self.composer.cursor();
        self.composer
            .replace_span((m.row, m.at_col), (m.row, col), &format!("{path} "));
    }

    fn begin_quit(&mut self) -> Cmd<Msg> {
        self.quitting = true;
        // Hand the tab title back to the shell rather than leaving a stale
        // "working…" on exit (best-effort, terminal-gated).
        crate::title::clear();
        if let Some(driver) = &self.driver {
            driver.send(DriverCmd::Shutdown);
        }
        if let Some(host) = &self.plugins {
            host.broadcast(&HostEvent::Shutdown);
        }
        Cmd::quit()
    }

    fn page_rows(&self) -> u16 {
        self.last_size.height.saturating_sub(8).max(1)
    }

    fn apply_plugin_action(&mut self, plugin: &str, action: PluginAction) {
        match action {
            PluginAction::RegisterCommand { name, description } => {
                self.commands.insert(name, (plugin.to_owned(), description));
            }
            PluginAction::SetStatus { key, value } => {
                if value.is_empty() {
                    self.statuses.remove(&key);
                } else {
                    self.statuses.insert(key, value);
                }
            }
            PluginAction::Footer { segments } => {
                self.footers.insert(plugin.to_owned(), segments);
            }
            PluginAction::AskUser {
                id,
                question,
                context,
                options,
                allow_freeform,
            } => {
                self.ask = Some(AskState {
                    plugin: plugin.to_owned(),
                    id,
                    question,
                    context,
                    options,
                    allow_freeform,
                    selected: 0,
                    freeform: TextArea::new(),
                    freeform_focused: false,
                });
            }
            PluginAction::RegisterKeybinding {
                keys,
                command,
                description,
            } => {
                let chord = normalize_chord(&keys);
                self.keybindings
                    .insert(chord, (plugin.to_owned(), command, description));
            }
            PluginAction::Modal {
                id,
                title,
                body,
                mut buttons,
            } => {
                if buttons.is_empty() {
                    buttons.push("OK".to_owned());
                }
                self.modal = Some(ModalState {
                    plugin: plugin.to_owned(),
                    id,
                    title,
                    body,
                    buttons,
                    selected: 0,
                });
            }
            PluginAction::Panel { title, body } => {
                if body.is_empty() {
                    self.panels.remove(plugin);
                } else {
                    self.panels.insert(plugin.to_owned(), (title, body));
                }
            }
            PluginAction::Note { text } => {
                self.toast(format!("{plugin}: {text}"));
                self.push_system(format!("[{plugin}] {text}"));
            }
            PluginAction::Log { text } => {
                self.push_log(format!("[{plugin}] {text}"));
            }
        }
    }

    fn answer_ask(&mut self, cancelled: bool) {
        let Some(ask) = self.ask.take() else { return };
        let selections = if ask.options.is_empty() || cancelled {
            Vec::new()
        } else {
            vec![ask.options[ask.selected.min(ask.options.len() - 1)].clone()]
        };
        let text = if cancelled {
            String::new()
        } else {
            ask.freeform.lines().join("\n").trim().to_owned()
        };
        if let Some(host) = &self.plugins {
            host.send_to(
                &ask.plugin,
                &HostEvent::AskResponse {
                    id: ask.id,
                    selections,
                    text,
                    cancelled,
                },
            );
        }
    }

    /// Live-preview the highlighted picker theme by adopting its palette.
    fn preview_theme(&mut self) {
        let next = self
            .theme_picker
            .selected_theme()
            .map(crate::theme::AcpTheme::from_theme);
        if let Some(t) = next {
            self.theme = t;
        }
    }

    /// Key handling while the `/theme` picker is open: arrows preview, typing
    /// filters, `Enter` keeps + persists, `Esc` restores the prior palette.
    fn theme_picker_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        match key.code {
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
                self.push_system(format!("theme saved → {name}"));
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

    /// Key handling for the full-screen transcript pager. Two sub-modes: a
    /// search-entry line (after `/`, vim/less style) and plain navigation.
    /// Mirrors the chat's accepted scroll model (raw offset + `follow`; the
    /// renderer clamps), so behaviour is consistent across the two views.
    fn pager_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let page = self.last_size.height.saturating_sub(4).max(1);
        if self.pager.searching {
            match key.code {
                KeyCode::Enter => self.pager.searching = false,
                KeyCode::Esc => {
                    self.pager.searching = false;
                    self.pager.query.clear();
                    self.pager.follow = true;
                    self.pager.scroll = 0;
                }
                KeyCode::Backspace => {
                    self.pager.query.pop();
                    self.pager.follow = false;
                    self.pager.scroll = 0;
                }
                KeyCode::Char(c) => {
                    self.pager.query.push(c);
                    self.pager.follow = false;
                    self.pager.scroll = 0;
                }
                _ => {}
            }
            return Cmd::none();
        }
        match key.code {
            // Esc clears an active filter first, then closes (less/pager UX).
            KeyCode::Esc => {
                if self.pager.query.is_empty() {
                    self.pager.open = false;
                } else {
                    self.pager.query.clear();
                    self.pager.follow = true;
                    self.pager.scroll = 0;
                }
            }
            KeyCode::Char('q') => self.pager.open = false,
            KeyCode::Char('/') => {
                self.pager.searching = true;
                self.pager.query.clear();
                self.pager.follow = false;
                self.pager.scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.pager.follow = false;
                self.pager.scroll = self.pager.scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.pager.follow = false;
                self.pager.scroll = self.pager.scroll.saturating_add(1);
            }
            KeyCode::PageUp => {
                self.pager.follow = false;
                self.pager.scroll = self.pager.scroll.saturating_sub(page);
            }
            KeyCode::PageDown => {
                self.pager.follow = false;
                self.pager.scroll = self.pager.scroll.saturating_add(page);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.pager.follow = false;
                self.pager.scroll = 0;
            }
            KeyCode::End | KeyCode::Char('G') => self.pager.follow = true,
            _ => {}
        }
        Cmd::none()
    }

    /// Key handling for the `/model` picker: ↑/↓ (or `j`/`k`) move, Enter
    /// switches the session model via the driver (`session/set_model`), Esc
    /// cancels. A no-op list guard keeps it safe if models vanish.
    fn model_picker_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let last = self.models.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => self.model_picker_open = false,
            KeyCode::Up | KeyCode::Char('k') => {
                self.model_sel = self.model_sel.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.model_sel = (self.model_sel + 1).min(last);
            }
            KeyCode::Enter => {
                if let Some(m) = self.models.get(self.model_sel) {
                    let id = m.id.clone();
                    if self.driver.is_none() {
                        self.push_system("not connected — cannot switch model");
                    } else if let Some(driver) = &self.driver {
                        driver.send(DriverCmd::SetModel(id));
                    }
                }
                self.model_picker_open = false;
            }
            _ => {}
        }
        Cmd::none()
    }

    /// Key handling for the `/mode` picker (mirrors the model picker):
    /// ↑/↓ (or `j`/`k`) move, Enter switches the session mode via the
    /// driver (`session/set_mode`), Esc cancels.
    fn mode_picker_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let last = self.modes.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => self.mode_picker_open = false,
            KeyCode::Up | KeyCode::Char('k') => {
                self.mode_sel = self.mode_sel.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.mode_sel = (self.mode_sel + 1).min(last);
            }
            KeyCode::Enter => {
                if let Some(m) = self.modes.get(self.mode_sel) {
                    let id = m.id.clone();
                    if self.driver.is_none() {
                        self.push_system("not connected — cannot switch mode");
                    } else if let Some(driver) = &self.driver {
                        driver.send(DriverCmd::SetMode(id));
                    }
                }
                self.mode_picker_open = false;
            }
            _ => {}
        }
        Cmd::none()
    }

    /// Key handling for the `/resume` picker: ↑/↓ (or `j`/`k`) move, Enter
    /// asks the agent to `session/load` the chosen session (its replayed
    /// history streams back in as normal notifications, so the transcript
    /// is cleared first to avoid mixing), Esc cancels.
    fn resume_picker_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let list = self.resume_sessions();
        let last = list.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => self.resume_picker_open = false,
            KeyCode::Up | KeyCode::Char('k') => {
                self.resume_sel = self.resume_sel.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.resume_sel = (self.resume_sel + 1).min(last);
            }
            KeyCode::Enter => {
                if let Some(s) = list.get(self.resume_sel) {
                    let id = s.id.clone();
                    if self.driver.is_some() {
                        // Clear first: the agent replays the loaded
                        // conversation through the normal notification path.
                        self.transcript.clear();
                        self.scroll = 0;
                        self.follow = true;
                        self.push_system(format!("resuming session {id}…"));
                        if let Some(driver) = &self.driver {
                            driver.send(DriverCmd::LoadSession(id));
                        }
                    } else {
                        self.push_system("not connected — cannot resume");
                    }
                }
                self.resume_picker_open = false;
            }
            _ => {}
        }
        Cmd::none()
    }

    /// Key handling for the sign-in picker: ↑/↓ (or `j`/`k`) move, Enter
    /// runs the ACP `authenticate` for the chosen method (the driver then
    /// retries `session/new`), Esc dismisses (stay disconnected; pick
    /// another agent with `/agents`).
    fn auth_picker_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let last = self.auth_methods.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => self.auth_picker_open = false,
            KeyCode::Up | KeyCode::Char('k') => {
                self.auth_sel = self.auth_sel.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.auth_sel = (self.auth_sel + 1).min(last);
            }
            KeyCode::Enter => {
                if let Some(m) = self.auth_methods.get(self.auth_sel) {
                    let id = m.id.clone();
                    let name = m.name.clone();
                    if self.driver.is_some() {
                        self.push_system(format!("authenticating: {name}…"));
                        if let Some(driver) = &self.driver {
                            driver.send(DriverCmd::Authenticate(id));
                        }
                    } else {
                        self.push_system("not connected — cannot authenticate");
                    }
                }
                self.auth_picker_open = false;
            }
            _ => {}
        }
        Cmd::none()
    }

    /// Key handling for the `/diff` overlay: scroll (arrows/`jk`,
    /// PgUp/Dn, `g`/`G`), Esc/`q` close. Raw offset + clamp-on-render,
    /// the pager's model.
    fn diff_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let page = self.last_size.height.saturating_sub(4).max(1);
        let Some(d) = self.diff.as_mut() else {
            return Cmd::none();
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.diff = None,
            KeyCode::Up | KeyCode::Char('k') => d.scroll = d.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => d.scroll = d.scroll.saturating_add(1),
            KeyCode::PageUp => d.scroll = d.scroll.saturating_sub(page),
            KeyCode::PageDown => d.scroll = d.scroll.saturating_add(page),
            KeyCode::Home | KeyCode::Char('g') => d.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => d.scroll = u16::MAX,
            _ => {}
        }
        Cmd::none()
    }

    /// Routes a key by the active overlay/screen. Returns the follow-up `Cmd`.
    fn on_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        if self.picking {
            return self.theme_picker_key(key);
        }
        if self.show_help {
            // The help overlay doubles as the discoverable gateway into
            // the keymap editor: `k` (the same key in every app) opens it.
            if key.code == KeyCode::Char('k') {
                self.show_help = false;
                self.keymap_panel = true;
                self.km_sel = 0;
                self.km_rebind = None;
            } else if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::F(1)) {
                self.show_help = false;
            }
            return Cmd::none();
        }
        if self.show_log && key.code == KeyCode::Esc {
            self.show_log = false;
            return Cmd::none();
        }
        if self.show_status && matches!(key.code, KeyCode::Esc | KeyCode::F(1)) {
            self.show_status = false;
            return Cmd::none();
        }
        if self.show_plugins && matches!(key.code, KeyCode::Esc | KeyCode::F(1)) {
            self.show_plugins = false;
            return Cmd::none();
        }
        if self.pager.open {
            return self.pager_key(key);
        }
        if self.model_picker_open {
            return self.model_picker_key(key);
        }
        if self.mode_picker_open {
            return self.mode_picker_key(key);
        }
        if self.resume_picker_open {
            return self.resume_picker_key(key);
        }
        if self.auth_picker_open {
            return self.auth_picker_key(key);
        }
        if self.diff.is_some() {
            return self.diff_key(key);
        }
        if self.keymap_panel {
            return self.keymap_panel_key(key);
        }
        if self.modal.is_some() {
            return self.modal_key(key);
        }
        if self.pending_permission.is_some() {
            return self.permission_key(key);
        }
        if self.ask.is_some() {
            return self.ask_key(key);
        }
        // Plugin keybindings: a registered chord (modifier/Fn key) fires its
        // command, unless the slash-autocomplete popup is capturing input.
        if self.completion.is_none() {
            if let Some(chord) = chord_of(&key) {
                if let Some((plugin, command, _)) = self.keybindings.get(&chord).cloned() {
                    if let Some(host) = &self.plugins {
                        host.send_to(
                            &plugin,
                            &HostEvent::Command {
                                name: command.clone(),
                                args: String::new(),
                            },
                        );
                    }
                    self.push_system(format!("⌨ {chord} → /{command}"));
                    return Cmd::none();
                }
            }
        }
        // Global commands resolve through the keymap *after* the plugin
        // chords (a plugin binding still wins) and before the screen
        // dispatch — so quit/help/keymap-panel are remappable and
        // RSTUI_KEYMAP-configurable, uniformly on every screen. One call;
        // only non-text chords are bound so plain keys `Fall` straight
        // through. acp's map has no leader sequence → no clock, no loop:
        // `0` is correct forever.
        match self.keymaps.dispatch(&key, 0) {
            Dispatch::Act(action) => return self.do_action(action),
            Dispatch::Pending => return Cmd::none(),
            Dispatch::Fall => {}
        }
        match self.screen {
            Screen::Picker => self.picker_key(key),
            Screen::Connecting | Screen::Chat => self.chat_key(key),
        }
    }

    /// Perform a resolved global command — the single place an
    /// acp-client keymap binding takes effect.
    fn do_action(&mut self, action: Action) -> Cmd<Msg> {
        match action {
            Action::Quit => return self.begin_quit(),
            Action::Help => self.show_help = true,
            Action::Drawer => {
                self.keymap_panel = true;
                self.km_sel = 0;
                self.km_rebind = None;
            }
            _ => {}
        }
        Cmd::none()
    }

    /// The keymap settings panel (the shared `KeymapView` widget): browse
    /// the live bindings and **capture a key to rebind** a command or
    /// disable it — the same reducer-owned FSM the kitchen sink and
    /// git-review use.
    fn keymap_panel_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        if let Some(act) = self.km_rebind.take() {
            if key.code == KeyCode::Esc {
                self.push_system("rebind cancelled".to_owned());
            } else {
                let chord = Chord::from_event(&key);
                self.keymaps.set_override(act, chord.spec());
                self.push_system(format!("bound → {}", chord.display()));
            }
            return Cmd::none();
        }
        let last = COMMANDS.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.keymap_panel = false,
            KeyCode::Down | KeyCode::Char('j') => self.km_sel = (self.km_sel + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.km_sel = self.km_sel.saturating_sub(1),
            KeyCode::Enter | KeyCode::Char('r') => {
                self.km_rebind = Some(COMMANDS[self.km_sel.min(last)].0);
            }
            KeyCode::Char('x') => {
                let act = COMMANDS[self.km_sel.min(last)].0;
                self.keymaps.set_override(act, "none");
            }
            _ => {}
        }
        Cmd::none()
    }

    /// The keymap panel rows, projected from the **live** keymap (the
    /// reducer owns the cursor + capture FSM; `KeymapView` just draws it).
    fn keymap_rows(&self) -> Vec<rstui_widgets::KeymapRow<'static>> {
        let km = self.keymaps.effective();
        COMMANDS
            .iter()
            .enumerate()
            .map(|(i, &(action, label))| {
                let keys = km.keys_for(action);
                let state = if self.km_rebind == Some(action) {
                    rstui_widgets::RowState::Capturing
                } else if i == self.km_sel {
                    rstui_widgets::RowState::Selected
                } else if keys == "—" {
                    rstui_widgets::RowState::Disabled
                } else {
                    rstui_widgets::RowState::Normal
                };
                rstui_widgets::KeymapRow::new(label, caps(&keys))
                    .id(action.id())
                    .state(state)
            })
            .collect()
    }

    fn modal_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let Some(m) = self.modal.as_mut() else {
            return Cmd::none();
        };
        match key.code {
            KeyCode::Left | KeyCode::Up => {
                m.selected = m.selected.saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Down | KeyCode::Tab if !m.buttons.is_empty() => {
                m.selected = (m.selected + 1).min(m.buttons.len() - 1);
            }
            KeyCode::Enter => {
                let m = self.modal.take().expect("checked above");
                let button = m.buttons.get(m.selected).cloned().unwrap_or_default();
                if let Some(host) = &self.plugins {
                    host.send_to(
                        &m.plugin,
                        &HostEvent::ModalResponse {
                            id: m.id,
                            button,
                            cancelled: false,
                        },
                    );
                }
            }
            KeyCode::Esc => {
                let m = self.modal.take().expect("checked above");
                if let Some(host) = &self.plugins {
                    host.send_to(
                        &m.plugin,
                        &HostEvent::ModalResponse {
                            id: m.id,
                            button: String::new(),
                            cancelled: true,
                        },
                    );
                }
            }
            _ => {}
        }
        Cmd::none()
    }

    fn picker_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        // Inline "custom ACP command" input mode owns all keys while open
        // (so `q`/letters type, not quit/navigate). Works even with an
        // empty/loading registry — that is the whole point.
        if let Some(input) = self.picker_custom.as_mut() {
            match key.code {
                KeyCode::Esc => self.picker_custom = None,
                KeyCode::Enter => {
                    let cmd = input.lines().join(" ").trim().to_owned();
                    self.picker_custom = None;
                    if !cmd.is_empty() {
                        return self.connect(cmd);
                    }
                }
                KeyCode::Backspace => {
                    input.delete_backward();
                }
                KeyCode::Char(c) => input.insert_char(c),
                _ => {}
            }
            return Cmd::none();
        }

        let len = self.registry.agents.len();
        match key.code {
            KeyCode::Up => {
                self.picker_selected = self.picker_selected.saturating_sub(1);
                Cmd::none()
            }
            KeyCode::Down => {
                if len > 0 {
                    self.picker_selected = (self.picker_selected + 1).min(len - 1);
                }
                Cmd::none()
            }
            KeyCode::Enter => {
                if let Some(agent) = self.registry.agents.get(self.picker_selected) {
                    match agent.command.clone() {
                        Some(cmd) => return self.connect(cmd),
                        None => self.toast("no launch command for this platform"),
                    }
                }
                Cmd::none()
            }
            // The "Custom command…" affordance: open the inline input for an
            // arbitrary local-stdio ACP command without restarting.
            KeyCode::Char('c') => {
                self.picker_custom = Some(TextArea::new());
                Cmd::none()
            }
            KeyCode::Char('q') | KeyCode::Esc => self.begin_quit(),
            KeyCode::F(1) => {
                self.show_help = true;
                Cmd::none()
            }
            _ => Cmd::none(),
        }
    }

    fn permission_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let Some(perm) = self.pending_permission.as_mut() else {
            return Cmd::none();
        };
        match key.code {
            KeyCode::Up => {
                perm.selected = perm.selected.saturating_sub(1);
            }
            KeyCode::Down if !perm.options.is_empty() => {
                perm.selected = (perm.selected + 1).min(perm.options.len() - 1);
            }
            KeyCode::Enter => {
                let id = perm.id;
                let choice = perm
                    .options
                    .get(perm.selected)
                    .map(|o| PermissionChoice::Selected(o.option_id.clone()))
                    .unwrap_or(PermissionChoice::Cancelled);
                self.pending_permission = None;
                if let Some(driver) = &self.driver {
                    driver.send(DriverCmd::Permission { id, choice });
                }
            }
            KeyCode::Esc => {
                let id = perm.id;
                self.pending_permission = None;
                if let Some(driver) = &self.driver {
                    driver.send(DriverCmd::Permission {
                        id,
                        choice: PermissionChoice::Cancelled,
                    });
                }
            }
            _ => {}
        }
        Cmd::none()
    }

    fn ask_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let Some(ask) = self.ask.as_mut() else {
            return Cmd::none();
        };
        if ask.freeform_focused {
            match key.code {
                KeyCode::Esc => ask.freeform_focused = false,
                KeyCode::Enter => {
                    self.answer_ask(false);
                }
                KeyCode::Backspace => {
                    ask.freeform.delete_backward();
                }
                KeyCode::Char(c) => ask.freeform.insert_char(c),
                _ => {}
            }
            return Cmd::none();
        }
        match key.code {
            KeyCode::Up => ask.selected = ask.selected.saturating_sub(1),
            KeyCode::Down if !ask.options.is_empty() => {
                ask.selected = (ask.selected + 1).min(ask.options.len() - 1);
            }
            KeyCode::Tab if ask.allow_freeform => ask.freeform_focused = true,
            KeyCode::Enter => self.answer_ask(false),
            KeyCode::Esc => self.answer_ask(true),
            _ => {}
        }
        Cmd::none()
    }

    /// Replaces the composer with the previous (`prev`) / next history entry,
    /// readline-style: ↑ on the first row walks back, ↓ on the last row walks
    /// forward; stepping past the newest restores the half-typed draft. A
    /// no-op when there is nothing to recall (composer left untouched).
    fn recall_history(&mut self, prev: bool) {
        let current = self.composer.lines().join("\n");
        let recalled = if prev {
            self.history.older(&current)
        } else {
            self.history.newer()
        };
        if let Some(text) = recalled {
            self.composer.set_value(text);
            self.composer.move_doc_end();
        }
    }

    fn chat_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // The slash-command autocomplete owns navigation/accept keys while it
        // is visible (opencode: ↑↓/Ctrl+P/N move, Tab completes, Enter runs,
        // Esc hides); anything else falls through and re-filters the list.
        if self.completion.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.completion = None;
                    return Cmd::none();
                }
                KeyCode::Up => {
                    self.move_completion(-1);
                    return Cmd::none();
                }
                KeyCode::Down => {
                    self.move_completion(1);
                    return Cmd::none();
                }
                KeyCode::Char('p') if ctrl => {
                    self.move_completion(-1);
                    return Cmd::none();
                }
                KeyCode::Char('n') if ctrl => {
                    self.move_completion(1);
                    return Cmd::none();
                }
                KeyCode::Tab => return self.accept_completion(false),
                KeyCode::Enter => return self.accept_completion(true),
                _ => {}
            }
        }

        // The `@`-mention popup owns the same navigation/accept keys while it
        // is visible (and the slash popup is not — they are mutually
        // exclusive). Tab/Enter insert the highlighted path; Esc hides it;
        // anything else falls through and re-filters below.
        if self.completion.is_none() && self.mention.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.mention = None;
                    return Cmd::none();
                }
                KeyCode::Up => {
                    self.move_mention(-1);
                    return Cmd::none();
                }
                KeyCode::Down => {
                    self.move_mention(1);
                    return Cmd::none();
                }
                KeyCode::Char('p') if ctrl => {
                    self.move_mention(-1);
                    return Cmd::none();
                }
                KeyCode::Char('n') if ctrl => {
                    self.move_mention(1);
                    return Cmd::none();
                }
                KeyCode::Tab | KeyCode::Enter => {
                    self.accept_mention();
                    return Cmd::none();
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('c') if ctrl => return self.begin_quit(),
            KeyCode::Char('q') if ctrl => return self.begin_quit(),
            KeyCode::F(1) => {
                self.show_help = true;
            }
            KeyCode::F(10) => return self.begin_quit(),
            KeyCode::Esc if self.streaming => {
                if let Some(driver) = &self.driver {
                    driver.send(DriverCmd::Cancel);
                }
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.history.reset();
                self.composer.insert_newline();
            }
            KeyCode::Enter => return self.submit_composer(),
            KeyCode::Backspace => {
                self.history.reset();
                self.composer.delete_backward();
            }
            KeyCode::Delete => {
                self.history.reset();
                self.composer.delete_forward();
            }
            KeyCode::Left => {
                self.composer.move_left();
            }
            KeyCode::Right => {
                self.composer.move_right();
            }
            // ↑/↓ recall history when the cursor can go no further in that
            // direction (first / last composer row), else move within the
            // draft — the readline / Codex-CLI rule.
            KeyCode::Up => {
                if self.composer.cursor().0 == 0 {
                    self.recall_history(true);
                } else {
                    self.composer.move_up();
                }
            }
            KeyCode::Down => {
                let last_row = self.composer.row_count().saturating_sub(1);
                if self.composer.cursor().0 >= last_row {
                    self.recall_history(false);
                } else {
                    self.composer.move_down();
                }
            }
            KeyCode::Home => self.composer.move_home(),
            KeyCode::End => self.composer.move_end(),
            KeyCode::PageUp => {
                self.follow = false;
                self.scroll = self.scroll.saturating_sub(self.page_rows());
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(self.page_rows());
            }
            KeyCode::Char(c) => {
                self.history.reset();
                self.composer.insert_char(c);
            }
            _ => {}
        }
        // Any edit that reached the composer re-filters the popups (the
        // slash one opens on a leading `/`, the `@`-mention one on an
        // `@token` at the cursor; they are mutually exclusive).
        self.refresh_completion();
        self.refresh_mention();
        Cmd::none()
    }
}

impl App for ChatApp {
    type Message = Msg;

    fn init(&mut self) -> Cmd<Msg> {
        Cmd::message(Msg::Boot)
    }

    fn on_event(&self, event: Event) -> Option<Msg> {
        match event {
            Event::Key(key) if key.kind == rstui_core::KeyEventKind::Press => Some(Msg::Key(key)),
            Event::Resize(size) => Some(Msg::Resize(size)),
            Event::Paste(text) => Some(Msg::Paste(text)),
            Event::Mouse(m) => match m.kind {
                rstui_core::MouseEventKind::ScrollUp => Some(Msg::Scroll(3)),
                rstui_core::MouseEventKind::ScrollDown => Some(Msg::Scroll(-3)),
                _ => None,
            },
            _ => None,
        }
    }

    fn tick_rate(&self) -> Option<std::time::Duration> {
        if self.streaming || !self.toasts.is_empty() || self.plugins.is_some() {
            Some(std::time::Duration::from_millis(120))
        } else {
            None
        }
    }

    fn on_tick(&self) -> Option<Msg> {
        Some(Msg::Tick)
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        let cmd = match message {
            Msg::Boot => {
                let mut cmds = Vec::new();
                let plugin_cmds = crate::resolve_plugins(&self.config);
                if !plugin_cmds.is_empty() {
                    let host = PluginHost::launch(&plugin_cmds, &self.cwd);
                    host.broadcast(&HostEvent::Init {
                        api_version: crate::plugin::API_VERSION.to_owned(),
                        client: "rstui-acp-client".to_owned(),
                        cwd: self.cwd.display().to_string(),
                    });
                    self.log
                        .push(format!("loaded plugins: {}", host.names().join(", ")));
                    cmds.push(Self::plugin_subscription(host.clone()));
                    self.plugins = Some(host);
                }
                match self.config.agent_command.clone() {
                    Some(cmd) => cmds.push(self.connect(cmd)),
                    None => {
                        self.status_line = "loading registry…".to_owned();
                        cmds.push(Cmd::perform(|| {
                            Msg::RegistryLoaded(Box::new(Registry::fetch_blocking()))
                        }));
                    }
                }
                Cmd::batch(cmds)
            }
            Msg::RegistryLoaded(reg) => {
                self.registry = *reg;
                self.status_line = if self.registry.offline {
                    format!(
                        "registry offline — {} built-in agents (Enter to launch)",
                        self.registry.agents.len()
                    )
                } else {
                    format!(
                        "{} agents from the ACP registry (↑↓ then Enter)",
                        self.registry.agents.len()
                    )
                };
                Cmd::none()
            }
            Msg::DiffLoaded(text) => {
                self.diff = Some(DiffView { text, scroll: 0 });
                Cmd::none()
            }
            Msg::Key(key) => self.on_key(key),
            Msg::Paste(text) => {
                if self.screen == Screen::Chat || self.screen == Screen::Connecting {
                    self.history.reset();
                    self.composer.insert_str(&text);
                    self.refresh_completion();
                    self.refresh_mention();
                }
                Cmd::none()
            }
            Msg::Resize(size) => {
                self.last_size = size;
                Cmd::none()
            }
            Msg::Scroll(delta) => {
                if delta > 0 {
                    self.follow = false;
                    self.scroll = self.scroll.saturating_sub(delta as u16);
                } else {
                    self.scroll = self.scroll.saturating_add((-delta) as u16);
                }
                Cmd::none()
            }
            Msg::Acp(event) => {
                let terminal = matches!(event, AcpEvent::Disconnected(_));
                self.handle_acp(event);
                if terminal {
                    Cmd::none()
                } else if let Some(driver) = &self.driver {
                    Self::acp_subscription(driver.clone())
                } else {
                    Cmd::none()
                }
            }
            Msg::Plugin(ev) => {
                self.apply_plugin_action(&ev.plugin, ev.action);
                if let Some(host) = &self.plugins {
                    Self::plugin_subscription(host.clone())
                } else {
                    Cmd::none()
                }
            }
            Msg::Tick => {
                self.spinner = self.spinner.wrapping_add(1);
                for toast in &mut self.toasts {
                    toast.age += 1;
                }
                self.toasts.retain(|t| t.age < 40);
                if self.spinner % 8 == 0 {
                    if let Some(host) = &self.plugins {
                        host.broadcast(&HostEvent::Refresh);
                    }
                }
                Cmd::none()
            }
            Msg::Quit => self.begin_quit(),
        };
        // UI-1/MD-1: every state transition funnels through `update`, so
        // refreshing the caller-owned markdown cache here is the single
        // total interception point — no per-mutation-site bookkeeping.
        self.refresh_md_cache();
        // Same total-interception point keeps the terminal title in step
        // with the session (emits only on an actual change).
        self.refresh_terminal_title();
        cmd
    }

    fn view(&self, frame: &mut Frame<'_>) {
        // One sample per painted frame — the header reads it back.
        self.fps.record();
        ui::render(self, frame);
    }
}

impl ChatApp {
    fn handle_acp(&mut self, event: AcpEvent) {
        match event {
            AcpEvent::Connected(info) => {
                self.screen = Screen::Chat;
                self.status_line = format!("connected · {}", short(&info));
                self.push_system(format!("connected to agent: {}", short(&info)));
            }
            AcpEvent::SessionStarted(id) => {
                self.sessions.record(SessionRef {
                    id,
                    cwd: self.cwd.display().to_string(),
                    agent: self.agent_label.clone(),
                    when: unix_now(),
                });
                self.sessions.save();
            }
            AcpEvent::AuthRequired(methods) => {
                self.auth_methods = methods;
                self.auth_sel = 0;
                self.auth_picker_open = !self.auth_methods.is_empty();
                self.push_system("sign-in required — choose an auth method");
            }
            AcpEvent::Status(s) => {
                if s == "session ready" {
                    self.screen = Screen::Chat;
                }
                self.status_line = s.clone();
                self.push_log(format!("status: {s}"));
            }
            AcpEvent::AgentText(t) => self.append_agent(Role::Agent, &t),
            AcpEvent::Thought(t) => self.append_agent(Role::Thought, &t),
            AcpEvent::RichUi(payload) => {
                // The agent replied with a declarative UI document; anchor
                // it in stream order. The view re-projects it from `text`
                // every frame through `rstui-jsonui` (pure projection).
                self.close_open_entry();
                self.transcript.push(Entry {
                    role: Role::RichUi,
                    text: payload.source,
                    open: false,
                    md_cache: None,
                });
                self.follow = true;
            }
            AcpEvent::ToolCall(info) => {
                if let Some(&i) = self.tool_index.get(&info.id) {
                    self.tool_calls[i] = info;
                } else {
                    let id = info.id.clone();
                    self.tool_index.insert(id.clone(), self.tool_calls.len());
                    self.tool_calls.push(info);
                    self.close_open_entry();
                    // A transcript anchor keeps the tool card in stream order;
                    // the view resolves `text` (the id) to the live registry.
                    self.transcript.push(Entry {
                        role: Role::Tool,
                        text: id,
                        open: false,
                        md_cache: None,
                    });
                    self.follow = true;
                }
            }
            AcpEvent::ToolCallUpdate(patch) => {
                if let Some(i) = self.tool_index.get(&patch.id).copied() {
                    let c = &mut self.tool_calls[i];
                    if let Some(v) = patch.title {
                        c.title = v;
                    }
                    if let Some(v) = patch.kind {
                        c.kind = v;
                    }
                    if let Some(v) = patch.status {
                        c.status = v;
                    }
                    if let Some(v) = patch.input {
                        c.input = v;
                    }
                    if let Some(v) = patch.body {
                        c.body = v;
                    }
                } else {
                    // Update for an unseen call: synthesize a minimal entry.
                    let id = patch.id.clone();
                    self.tool_index.insert(id.clone(), self.tool_calls.len());
                    self.tool_calls.push(ToolCallInfo {
                        id: patch.id,
                        title: patch.title.unwrap_or_else(|| "tool".to_owned()),
                        kind: patch.kind.unwrap_or(ToolKind::Other),
                        status: patch.status.unwrap_or(ToolStatus::InProgress),
                        input: patch.input.unwrap_or_default(),
                        body: patch.body.unwrap_or_default(),
                    });
                    self.close_open_entry();
                    self.transcript.push(Entry {
                        role: Role::Tool,
                        text: id,
                        open: false,
                        md_cache: None,
                    });
                    self.follow = true;
                }
            }
            AcpEvent::Plan(entries) => {
                // ACP replaces the whole plan on each update.
                self.todos = entries;
                let (done, total) = self.todo_progress();
                if total > 0 {
                    self.close_open_entry();
                    self.transcript.push(Entry {
                        role: Role::Plan,
                        text: format!("plan updated — {done}/{total} done"),
                        open: false,
                        md_cache: None,
                    });
                    self.follow = true;
                }
            }
            AcpEvent::AvailableCommands(cmds) => {
                self.agent_commands = cmds.into_iter().collect();
                self.refresh_completion();
            }
            AcpEvent::Models { current, available } => {
                self.models = available;
                self.model_sel = self
                    .models
                    .iter()
                    .position(|m| m.id == current)
                    .unwrap_or(0);
                self.current_model = Some(current);
            }
            AcpEvent::ModelSelected(id) => {
                let name = self
                    .models
                    .iter()
                    .find(|m| m.id == id)
                    .map_or_else(|| id.clone(), |m| m.name.clone());
                self.current_model = Some(id);
                self.push_system(format!("model → {name}"));
            }
            AcpEvent::Modes { current, available } => {
                self.modes = available;
                self.mode_sel = self.modes.iter().position(|m| m.id == current).unwrap_or(0);
                self.current_mode = Some(current);
            }
            AcpEvent::ModeChanged(id) => {
                let name = self
                    .modes
                    .iter()
                    .find(|m| m.id == id)
                    .map_or_else(|| id.clone(), |m| m.name.clone());
                self.current_mode = Some(id);
                self.push_system(format!("mode → {name}"));
            }
            AcpEvent::Usage { used, size } => {
                self.usage = Some((used, size));
            }
            AcpEvent::TurnEnded(reason) => {
                self.close_open_entry();
                self.streaming = false;
                self.status_line = format!("turn ended: {reason}");
                if self.bell_enabled {
                    // "Your turn" — audible only if the user's terminal
                    // rings BEL; terminal-gated, so silent under tests.
                    crate::title::bell();
                }
                if let Some(host) = &self.plugins {
                    host.broadcast(&HostEvent::TurnEnded {
                        stop_reason: reason,
                    });
                }
            }
            AcpEvent::Permission { id, title, options } => {
                self.pending_permission = Some(PendingPermission {
                    id,
                    title,
                    options,
                    selected: 0,
                });
            }
            AcpEvent::Stderr(line) => self.push_log(format!("agent: {line}")),
            AcpEvent::Error(e) => {
                self.streaming = false;
                self.push_system(format!("error: {e}"));
                self.toast(format!("error: {e}"));
            }
            AcpEvent::Disconnected(why) => {
                self.streaming = false;
                self.driver = None;
                self.screen = Screen::Picker;
                self.status_line = format!("disconnected: {why}");
                self.push_system(format!("disconnected: {why} — pick an agent to reconnect"));
            }
        }
    }
}

fn short(s: &str) -> String {
    let s = s.trim();
    if s.len() > 60 {
        format!("{}…", &s[..60])
    } else {
        s.to_owned()
    }
}

// --- view-facing accessors for the overlay state (kept here, near the model) ---

impl PendingPermission {
    /// The agent's request title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
    /// The offered options.
    #[must_use]
    pub fn options(&self) -> &[PermissionOption] {
        &self.options
    }
    /// The highlighted option index.
    #[must_use]
    pub fn selected(&self) -> usize {
        self.selected
    }
}

impl AskState {
    /// The originating plugin.
    #[must_use]
    pub fn plugin(&self) -> &str {
        &self.plugin
    }
    /// The question text.
    #[must_use]
    pub fn question(&self) -> &str {
        &self.question
    }
    /// Optional context.
    #[must_use]
    pub fn context(&self) -> &str {
        &self.context
    }
    /// The selectable options.
    #[must_use]
    pub fn options(&self) -> &[String] {
        &self.options
    }
    /// The highlighted option.
    #[must_use]
    pub fn selected(&self) -> usize {
        self.selected
    }
    /// Whether freeform entry is allowed.
    #[must_use]
    pub fn allow_freeform(&self) -> bool {
        self.allow_freeform
    }
    /// Whether the freeform field has focus.
    #[must_use]
    pub fn freeform_focused(&self) -> bool {
        self.freeform_focused
    }
    /// The freeform document.
    #[must_use]
    pub fn freeform(&self) -> &TextArea {
        &self.freeform
    }
}

#[cfg(test)]
mod md_cache_tests {
    use super::*;

    /// UI-1/MD-1 exactness gate: the caller-owned `md_cache` must be
    /// *exactly* what the renderer's fresh parse produces, for every
    /// non-last `Role::Agent` entry — that equality is the entire
    /// correctness contract (the renderer falls back to a fresh parse, so
    /// a populated cache that differed would be the only way to change
    /// output). Drives two agent turns separated by a system line so turn
    /// one is a finalized, non-last agent entry, with a trailing open
    /// agent entry and a non-agent entry to cover every branch.
    #[test]
    fn agent_md_cache_equals_a_fresh_parse_for_every_non_last_entry() {
        let mut app = ChatApp::new(Config::default());
        app.append_agent(
            Role::Agent,
            "# Heading\n\nA **bold** word and a [link](http://example.com).\n",
        );
        app.append_agent(Role::Agent, "more streamed text in the same turn\n");
        app.push_system("a system notice between turns");
        app.append_agent(Role::Agent, "## Second turn\n\n- alpha\n- beta\n");
        app.refresh_md_cache();

        let n = app.transcript.len();
        assert!(n >= 3, "expected several entries, got {n}");
        let mut saw_cached_agent = false;
        for (i, e) in app.transcript.iter().enumerate() {
            let is_last = i + 1 == n;
            if e.role == Role::Agent && !is_last {
                let fresh = Markdown::new(&e.text).lines(MD_WIDTH);
                assert_eq!(
                    e.md_cache.as_ref(),
                    Some(&fresh),
                    "non-last agent entry {i} cache must equal a fresh parse"
                );
                saw_cached_agent = true;
            } else if e.role == Role::Agent {
                // The streaming last entry is parsed fresh by the renderer;
                // if a cache exists it must still be exact.
                if let Some(c) = &e.md_cache {
                    assert_eq!(c, &Markdown::new(&e.text).lines(MD_WIDTH));
                }
            } else {
                assert!(
                    e.md_cache.is_none(),
                    "non-agent entry {i} must never be markdown-cached"
                );
            }
        }
        assert!(
            saw_cached_agent,
            "test must exercise at least one cached non-last agent entry"
        );
    }
}

#[cfg(test)]
mod bell_env_tests {
    use super::bell_from_env;

    #[test]
    fn bell_is_on_by_default_and_off_only_for_explicit_falsy_values() {
        assert!(bell_from_env(None), "unset → on");
        assert!(bell_from_env(Some("")), "empty is not a falsy token → on");
        assert!(bell_from_env(Some("1")));
        assert!(bell_from_env(Some("anything")));
        for off in ["0", "false", "no", "off", "OFF", " Off "] {
            assert!(!bell_from_env(Some(off)), "{off:?} → off");
        }
    }
}

#[cfg(test)]
mod mention_rank_tests {
    use super::rank_paths;

    #[test]
    fn ranks_basename_prefix_over_substring_over_path_and_caps() {
        let cands: Vec<String> = [
            "src/app.rs",
            "src/ui.rs",
            "docs/app-notes.md",
            "src/sub/apparatus.rs",
            "README.md",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();

        // basename starts with "app" (app.rs, apparatus.rs) rank first,
        // then basename-substring (app-notes.md), then path-substring.
        let r = rank_paths(&cands, "app", 10);
        assert_eq!(r[0], "src/app.rs", "shortest basename-prefix wins");
        assert!(r.contains(&"src/sub/apparatus.rs".to_owned()));
        assert!(r.contains(&"docs/app-notes.md".to_owned()));
        assert!(!r.contains(&"README.md".to_owned()), "non-matches dropped");

        // Empty query keeps everything, capped.
        assert_eq!(rank_paths(&cands, "", 2).len(), 2, "cap honoured");
        assert_eq!(rank_paths(&cands, "", 99).len(), cands.len());
        // Case-insensitive.
        assert_eq!(rank_paths(&cands, "READ", 9), vec!["README.md".to_owned()]);
    }
}

#[cfg(test)]
mod canned_prompt_tests {
    use super::{INIT_PROMPT, REVIEW_PROMPT};

    #[test]
    fn canned_prompts_are_substantial_and_on_topic() {
        assert!(
            INIT_PROMPT.contains("AGENTS.md"),
            "/init asks for AGENTS.md"
        );
        assert!(
            REVIEW_PROMPT.to_ascii_lowercase().contains("review")
                && REVIEW_PROMPT.contains("git diff"),
            "/review asks for a diff review"
        );
        // Agent-agnostic: no vendor name baked in (works with any ACP agent).
        for p in [INIT_PROMPT, REVIEW_PROMPT] {
            assert!(p.len() > 80);
            let l = p.to_ascii_lowercase();
            assert!(!l.contains("codex") && !l.contains("claude"));
        }
    }
}
