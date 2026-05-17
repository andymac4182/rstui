//! `rstui-kitchen-sink` — one fully interactive, multi-screen, full-colour
//! terminal app that exercises **every** widget in the rstui catalog with
//! live keyboard *and* mouse, and doubles as a deterministic
//! [`Harness`](rstui_runtime::Harness)-driven snapshot test of the whole
//! stack.
//!
//! It is the worked answer to "show me everything at once": the
//! [`Sidebar`](rstui_widgets::Sidebar) navigation rail, the
//! [`StatusBar`](rstui_widgets::StatusBar) footer, and the global
//! [`HelpOverlay`](rstui_widgets::HelpOverlay) /
//! [`CommandPalette`](rstui_widgets::CommandPalette) /
//! [`Drawer`](rstui_widgets::Drawer) / [`Modal`](rstui_widgets::Modal) /
//! [`Toast`](rstui_widgets::Toast) overlays are not *described* on a slide —
//! they *are* the chrome you drive, and each of the nine content screens
//! makes its widgets respond to the arrow keys, `Tab`, `Enter`, `Space`,
//! typing, the mouse, and the scroll wheel.
//!
//! Architecture is textbook rstui (ADR 0004): one Elm-style [`App`], all
//! state caller-owned on the [`KitchenSink`] model, every mutation funnelled
//! through [`update`](App::update), and [`view`](App::view) a pure projection.
//! The exact same `App` runs headless under
//! [`Harness`](rstui_runtime::Harness) in `cargo test` and live on a TTY
//! through [`rstui_crossterm::run_app`] from the binary.
//!
//! ```text
//! cargo run -p rstui-kitchen-sink
//! ```

pub(crate) mod chrome;
pub(crate) mod clipboard;
pub(crate) mod screens;
pub(crate) mod theme;

/// The keymap engine now lives in its own reusable crate (ADR 0015); this
/// alias keeps the existing `crate::keymap::…` paths working unchanged.
pub(crate) use rstui_keymap as keymap;

use std::cell::{Cell, RefCell};

use rstui_core::{
    Buffer, Constraint, Event, KeyCode, KeyEvent, KeyModifiers, Layout, Margin, MouseButton,
    MouseEventKind, Position, Rect, Selection, Size, Style, TextEdit, selected_text,
};
use rstui_runtime::{App, Cmd, Frame};
use rstui_widgets::ToastLevel;

use screens::{Screen, ScreenState};
use theme::{Mode, Theme};

/// One queued toast: its level, body, and the animation tick it was born on
/// (so [`update`](App::update) can expire it without a wall clock).
#[derive(Debug, Clone)]
pub(crate) struct Notice {
    /// The severity, which picks the toast colour.
    pub(crate) level: ToastLevel,
    /// The single-line body.
    pub(crate) body: String,
    /// The [`KitchenSink::tick`] value when this notice was raised.
    pub(crate) born: u64,
}

/// Which floating layer, if any, is currently captured above the screen.
///
/// Each variant is one widget from the catalog shown in its *real* role
/// rather than as a static sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Overlay {
    /// Nothing floating; the screen has focus.
    #[default]
    None,
    /// The global [`HelpOverlay`](rstui_widgets::HelpOverlay) (`?`).
    Help,
    /// The global [`CommandPalette`](rstui_widgets::CommandPalette) (`:`).
    Palette,
    /// The settings [`Drawer`](rstui_widgets::Drawer) (`g`).
    Drawer,
    /// The reusable [`rstui_theme::ThemePicker`] — browse every
    /// gpui-component theme with live preview (`p` from the drawer).
    ThemePicker,
    /// The quit-confirmation [`Modal`](rstui_widgets::Modal) (`q` / `Esc`).
    QuitConfirm,
}

/// Which half of the shell the keyboard drives. `Tab` flips it; the focused
/// half draws an accent border.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Pane {
    /// The navigation rail owns the arrows / `Enter`.
    #[default]
    Sidebar,
    /// The active screen owns the arrows / `Enter` / typing.
    Content,
}

/// Every input intent the shell understands. [`on_event`](App::on_event)
/// maps a terminal [`Event`] to one of these; [`update`](App::update) is the
/// single place any of them mutates state.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The animation clock advanced one frame.
    Tick,
    /// The terminal was resized; hit-testing must follow the reflow.
    Resized(Size),
    /// A key was pressed (code + modifiers), routed by `update`.
    Key(KeyCode, KeyModifiers),
    /// Left button pressed at this cell — starts a text selection and is the
    /// first half of a click (the click acts on [`MouseUp`](Self::MouseUp)).
    MouseDown(Position),
    /// The pointer moved with the left button held — extends the selection.
    MouseDrag(Position),
    /// Left button released here — finalises a drag-selection (copy) or, if
    /// there was no drag, routes the click.
    MouseUp(Position),
    /// The scroll wheel moved (`up`) at this cell.
    Scroll {
        /// `true` for wheel-up / scroll-back.
        up: bool,
        /// Where the pointer was, so the right pane scrolls.
        at: Position,
    },
    /// A bracketed paste delivered this text in one chunk.
    Paste(String),
    /// Stop the program.
    Quit,
}

/// The shell rectangles the last [`view`](App::view) actually drew into.
///
/// Hit-testing must use *what was rendered*, never a guessed terminal size:
/// a real terminal does not always send an initial `Resize`, so deriving the
/// layout from a seed size makes every click land in the wrong place. `view`
/// records the true geometry here each frame and the click/scroll reducer
/// reads it back, so what the user sees and what a click selects can never
/// drift.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ShellGeom {
    /// The navigation rail rect.
    pub(crate) sidebar: Rect,
    /// The active screen's drawable rect (inside its frame border).
    pub(crate) content: Rect,
}

/// The whole application: the active screen, navigation + focus state, the
/// animation clock, the live toast queue, and every screen's interactive
/// model. All of it is plain caller-owned data — widgets only ever read it.
pub struct KitchenSink {
    /// The geometry the last frame drew, captured by `view` for hit-testing.
    geom: Cell<ShellGeom>,
    /// The screen the sidebar has selected.
    screen: Screen,
    /// The sidebar cursor (may differ from [`screen`](Self::screen) until
    /// `Enter`).
    nav: usize,
    /// Which half of the shell the keyboard drives.
    pane: Pane,
    /// The floating layer, if any.
    overlay: Overlay,
    /// The monotonically increasing animation frame counter (spinners,
    /// skeleton shimmer, the header clock, toast expiry).
    tick: u64,
    /// The active colour palette.
    theme: Theme,
    /// The active theme's display name (the built-in `rstui Dark`/`Light`,
    /// or a gpui-component theme picked via `RSTUI_THEME`). Shown in the
    /// settings drawer so the live colour source is always visible.
    theme_name: String,
    /// The reusable theme-picker state (catalogue + highlight + filter),
    /// driven while [`Overlay::ThemePicker`] is open.
    theme_picker: rstui_theme::ThemePickerState,
    /// The `(palette, name)` to restore if the picker is cancelled with
    /// `Esc` (so live preview never permanently changes the theme).
    theme_restore: Option<(Theme, String)>,
    /// The live toast queue; [`update`](App::update) expires old entries.
    notices: Vec<Notice>,
    /// The command-palette query buffer (a real editable [`TextEdit`]).
    palette_query: TextEdit,
    /// The command-palette keyboard cursor.
    palette_row: usize,
    /// Every screen's interactive state.
    screens: ScreenState,
    /// The caller-owned text selection (ADR 0012 §P1): `update` mutates it
    /// from the mouse, `view` projects it as a highlight and reads it back
    /// with [`selected_text`]. Coordinates are content-buffer cells.
    selection: Selection,
    /// The cell the left button went down on, while it is still held.
    press: Option<Position>,
    /// Whether the pointer moved since [`press`](Self::press) (a drag, not a
    /// click) — decides copy-vs-click on release.
    drag_moved: bool,
    /// The text the current selection covers, recomputed by `view` from the
    /// rendered cells (interior-mutable so the pure `view` can fill it; read
    /// by the copy path on release). The framework-pure analogue of a
    /// clipboard read.
    selected: RefCell<String>,
    /// The container rect the in-progress selection is confined to (the
    /// text panel the press landed in). A drag is clamped to this and the
    /// extraction/highlight is restricted to it, so a selection never
    /// crosses into a neighbouring panel or the chrome.
    sel_region: Cell<Option<Rect>>,
    /// The in-app clipboard: the last text copied or cut. `Ctrl+V` pastes
    /// this into the focused editable (the OS clipboard is also set via
    /// OSC 52, and the terminal's own paste arrives as [`Event::Paste`]).
    clipboard: String,
    /// The customisable keymap registry (multiple maps, per-OS, overrides,
    /// leader sequences). Every key event is resolved through this.
    keymaps: keymap::Keymaps,
    /// The selected action row in the settings drawer's keymap table.
    drawer_sel: usize,
    /// `Some(action)` while the drawer is capturing a key to rebind it to —
    /// the next key press becomes that action's new binding.
    rebind: Option<keymap::Action>,
    /// Live render-rate meter (the reusable [`rstui_widgets::FpsMeter`]),
    /// sampled once per frame in `view` and shown in the header so the app's
    /// performance is always visible.
    fps: rstui_widgets::FpsMeter,
}

impl KitchenSink {
    /// The app at startup `size`: the welcome screen, sidebar focused, the
    /// dark palette, no overlays, an empty toast queue.
    #[must_use]
    pub fn new(size: Size) -> Self {
        Self {
            // A best-effort seed; `view` overwrites it with the real
            // geometry on the very first frame (before any click).
            geom: Cell::new(Self::compute_geom(Rect::from_size(size))),
            screen: Screen::Welcome,
            nav: 0,
            pane: Pane::Sidebar,
            overlay: Overlay::None,
            tick: 0,
            theme: Theme::new(Mode::Dark),
            theme_name: format!("rstui {}", Mode::Dark.label()),
            theme_picker: rstui_theme::ThemePickerState::new(),
            theme_restore: None,
            notices: Vec::new(),
            palette_query: TextEdit::new(),
            palette_row: 0,
            screens: ScreenState::new(),
            selection: Selection::new(),
            press: None,
            drag_moved: false,
            selected: RefCell::new(String::new()),
            sel_region: Cell::new(None),
            clipboard: String::new(),
            keymaps: keymap::Keymaps::new(),
            drawer_sel: 0,
            rebind: None,
            fps: rstui_widgets::FpsMeter::new(),
        }
    }

    /// Skin the whole app with a theme: either a built-in gpui-component
    /// theme by name (any of [`rstui_theme::Theme::all`], case-insensitive)
    /// **or a path to your own `ThemeSet` JSON file** — so users theme the
    /// app without rebuilding. An unknown name or unreadable/invalid file
    /// keeps the built-in default (a typo never leaves a blank screen). This
    /// is the one seam `RSTUI_THEME` flows through, so every screen reskins
    /// for free.
    #[must_use]
    pub fn with_theme(mut self, name_or_path: &str) -> Self {
        // A path (exists on disk) → user's own theme file; pick its default
        // theme, else the first. Otherwise treat it as a built-in name.
        let picked = if std::path::Path::new(name_or_path).is_file() {
            rstui_theme::Theme::from_set_file(name_or_path)
                .ok()
                .and_then(|themes| {
                    themes
                        .iter()
                        .find(|t| t.is_default)
                        .or_else(|| themes.first())
                        .cloned()
                })
        } else {
            rstui_theme::Theme::by_name(name_or_path)
        };
        if let Some(t) = picked {
            self.theme = Theme::from_palette(&t.palette);
            self.theme_name = t.name;
        }
        self
    }

    /// Apply a previously picker-saved theme ([`rstui_theme::Theme::write_choice`])
    /// if one exists. `main.rs` calls this when `RSTUI_THEME` is unset, so a
    /// theme chosen in the in-app picker survives a restart.
    #[must_use]
    pub fn load_saved_theme(self) -> Self {
        match rstui_theme::Theme::read_choice(Self::theme_config_path()) {
            Some(t) => self.with_theme(&t.name),
            None => self,
        }
    }

    /// Where the in-app picker persists the chosen theme name
    /// (`$XDG_CONFIG_HOME` or `~/.config` → `rstui/kitchen-sink.theme`).
    fn theme_config_path() -> std::path::PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
            })
            .unwrap_or_else(|| std::path::PathBuf::from(".rstui"));
        base.join("rstui").join("kitchen-sink.theme")
    }

    /// Live-preview the highlighted picker theme by applying its palette to
    /// the whole app (the `Esc` path restores the pre-picker palette from
    /// [`theme_restore`](Self::theme_restore)).
    fn preview_picked(&mut self) {
        let pick = self
            .theme_picker
            .selected_theme()
            .map(|t| (Theme::from_palette(&t.palette), t.name.clone()));
        if let Some((theme, name)) = pick {
            self.theme = theme;
            self.theme_name = name;
        }
    }

    /// Apply a user keymap choice: either the **name** of a built-in map
    /// ([`keymap::Keymaps::map_names`] — `Default`/`Vim`/`Leader`,
    /// case-insensitive) **or a path to a keymap config file** (`id = keys`
    /// lines; see `docs/keymaps.md`). An unknown name or unreadable/invalid
    /// file keeps the defaults — a typo never breaks your keys. This is the
    /// one seam `RSTUI_KEYMAP` flows through, mirroring [`Self::with_theme`],
    /// so users remap or switch keymaps without rebuilding and without the
    /// in-app drawer.
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

    /// Raise a toast that will live ~40 ticks.
    pub(crate) fn notify(&mut self, level: ToastLevel, body: impl Into<String>) {
        self.notices.push(Notice {
            level,
            body: body.into(),
            born: self.tick,
        });
    }

    /// The header / body / footer rows for the whole screen.
    fn shell_rows(area: Rect) -> [Rect; 3] {
        Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area)
    }

    /// The sidebar / content split of the body row.
    fn body_split(body: Rect) -> [Rect; 2] {
        Layout::horizontal([Constraint::Length(22), Constraint::Fill(1)]).areas(body)
    }

    /// The shell geometry for a whole-screen `area` — the single source the
    /// renderer (`view`) and the hit-test (`update`) both go through, so they
    /// cannot disagree.
    fn compute_geom(area: Rect) -> ShellGeom {
        let [_, body, _] = Self::shell_rows(area);
        let [sidebar, content_outer] = Self::body_split(body);
        ShellGeom {
            sidebar,
            // Mirror the one-cell rounded frame `chrome::view_content` draws
            // (`Block::bordered().inner()` == a 1-cell margin, no padding).
            content: content_outer.inner(Margin::new(1, 1)),
        }
    }

    /// The navigation rail rect the last frame actually rendered.
    fn sidebar_rect(&self) -> Rect {
        self.geom.get().sidebar
    }

    /// The active screen's drawable rect the last frame actually rendered.
    fn content_rect(&self) -> Rect {
        self.geom.get().content
    }

    /// Drops the selection *and* the container it was confined to, so the
    /// two never drift (every place a selection ends goes through here).
    fn clear_selection(&mut self) {
        self.selection.clear();
        self.sel_region.set(None);
    }

    /// Puts `text` on the in-app clipboard *and* the host clipboard (OSC 52,
    /// best-effort), and toasts what happened. `verb` is "Copied" or "Cut".
    fn do_copy(&mut self, text: String, verb: &str) {
        if text.is_empty() {
            return;
        }
        let n = text.chars().count();
        let to_os = clipboard::copy(&text);
        self.clipboard = text;
        let where_ = if to_os {
            "clipboard"
        } else {
            "in-app clipboard"
        };
        self.notify(ToastLevel::Success, format!("{verb} {n} chars → {where_}"));
    }

    /// Pastes the in-app clipboard into whatever editable currently has
    /// focus (the same sink the terminal's own paste reaches).
    fn paste_clipboard(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let text = self.clipboard.clone();
        if self.overlay == Overlay::Palette {
            self.palette_query.insert_str(&text);
        } else if self.overlay == Overlay::None && self.pane == Pane::Content {
            self.screens.on_paste(self.screen, &text);
        }
    }

    /// Performs a resolved keymap [`Action`](keymap::Action). The single
    /// place a shell binding takes effect, so every keymap (and any user
    /// remap) routes through one switch.
    fn do_action(&mut self, action: keymap::Action) -> Cmd<Msg> {
        use keymap::Action;
        match action {
            Action::Quit => self.overlay = Overlay::QuitConfirm,
            Action::Help => self.overlay = Overlay::Help,
            Action::Palette => {
                self.overlay = Overlay::Palette;
                self.palette_query = TextEdit::new();
                self.palette_row = 0;
            }
            Action::Drawer => {
                self.overlay = Overlay::Drawer;
                self.drawer_sel = 0;
            }
            Action::FocusToggle => {
                self.pane = match self.pane {
                    Pane::Sidebar => Pane::Content,
                    Pane::Content => Pane::Sidebar,
                };
            }
            Action::Goto(n) => {
                let idx = usize::from(n.saturating_sub(1));
                if idx < Screen::ALL.len() {
                    self.nav = idx;
                    self.screen = Screen::ALL[idx];
                    self.pane = Pane::Content;
                }
            }
            Action::CycleKeymap => {
                let name = self.keymaps.cycle();
                self.notify(ToastLevel::Info, format!("Keymap → {name}"));
            }
            Action::Copy => {
                if self.selection.is_empty() {
                    // Nothing selected → Ctrl+C keeps its quit meaning.
                    return Cmd::quit();
                }
                let txt = self.selected.borrow().clone();
                self.do_copy(txt, "Copied");
            }
            Action::Cut => {
                if !self.selection.is_empty() {
                    let txt = self.selected.borrow().clone();
                    let cut = self.screens.cut(self.screen, &txt);
                    self.do_copy(txt, if cut { "Cut" } else { "Copied" });
                    self.clear_selection();
                }
            }
            Action::Paste => self.paste_clipboard(),
            // The kitchen sink defines no app-specific actions; the engine
            // supports them (`rstui_keymap::Action::Custom`) for other apps.
            Action::Custom(_) => {}
        }
        Cmd::none()
    }

    /// Routes a completed click (a mouse-up with no intervening drag):
    /// dismiss a passive overlay, pick a rail row, or hand the active screen
    /// its click — the same routing the old single `Click` message did.
    fn route_click(&mut self, pos: Position) {
        if self.overlay != Overlay::None {
            if matches!(self.overlay, Overlay::Help | Overlay::QuitConfirm) {
                self.overlay = Overlay::None;
            }
            return;
        }
        let sidebar = self.sidebar_rect();
        if sidebar.contains(pos) {
            let inner_rows = sidebar.height.saturating_sub(2);
            let offset = Screen::sidebar_offset(self.nav, inner_rows);
            let row = pos.y.saturating_sub(sidebar.y + 1) as usize;
            if let Some(i) = Screen::screen_at_row(row, offset) {
                self.nav = i;
                self.screen = Screen::ALL[i];
                self.pane = Pane::Content;
            }
        } else {
            let content = self.content_rect();
            if content.contains(pos) {
                self.pane = Pane::Content;
                let out = self.screens.on_click(self.screen, pos, content);
                if let Some((level, body)) = out.toast {
                    self.notify(level, body);
                }
            }
        }
    }

    /// Move the sidebar cursor by `delta`, clamped to the screen list.
    fn step_nav(&mut self, delta: isize) {
        let last = Screen::ALL.len() as isize - 1;
        let next = (self.nav as isize + delta).clamp(0, last);
        self.nav = next as usize;
    }

    /// Commit the sidebar cursor: switch screens and hand focus to content.
    fn open_selected(&mut self) {
        self.screen = Screen::ALL[self.nav];
        self.pane = Pane::Content;
    }

    /// Handle a key while an overlay is captured. Returns the follow-up
    /// command (only ever `quit` or `none`).
    fn key_in_overlay(&mut self, code: KeyCode) -> Cmd<Msg> {
        match self.overlay {
            Overlay::QuitConfirm => match code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => return Cmd::quit(),
                _ => self.overlay = Overlay::None,
            },
            Overlay::Palette => match code {
                KeyCode::Esc => {
                    self.overlay = Overlay::None;
                }
                KeyCode::Up => self.palette_row = self.palette_row.saturating_sub(1),
                KeyCode::Down => {
                    self.palette_row = (self.palette_row + 1).min(
                        chrome::palette_matches(&self.palette_query)
                            .len()
                            .saturating_sub(1),
                    );
                }
                KeyCode::Enter => {
                    let matches = chrome::palette_matches(&self.palette_query);
                    if let Some(&idx) = matches.get(self.palette_row) {
                        self.screen = Screen::ALL[idx];
                        self.nav = idx;
                        self.pane = Pane::Content;
                    }
                    self.overlay = Overlay::None;
                    self.palette_query = TextEdit::new();
                    self.palette_row = 0;
                }
                KeyCode::Backspace => {
                    self.palette_query.delete_backward();
                    self.palette_row = 0;
                }
                KeyCode::Char(c) => {
                    self.palette_query.insert_char(c);
                    self.palette_row = 0;
                }
                _ => {}
            },
            Overlay::Drawer => {
                let shown = keymap::Action::shown();
                match code {
                    KeyCode::Esc | KeyCode::Char('g') => self.overlay = Overlay::None,
                    KeyCode::Char('t') | KeyCode::Char(' ') => {
                        self.theme = Theme::new(self.theme.mode.toggled());
                        self.theme_name = format!("rstui {}", self.theme.mode.label());
                    }
                    KeyCode::Char('p') => {
                        // Open the reusable theme picker; remember the
                        // current palette so Esc can restore it.
                        self.theme_restore = Some((self.theme, self.theme_name.clone()));
                        self.overlay = Overlay::ThemePicker;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.drawer_sel = self.drawer_sel.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.drawer_sel = (self.drawer_sel + 1).min(shown.len() - 1);
                    }
                    KeyCode::Char('c') => {
                        let name = self.keymaps.cycle();
                        self.notify(ToastLevel::Info, format!("Keymap → {name}"));
                    }
                    KeyCode::Char('r') | KeyCode::Enter => {
                        // Arm capture: the next key (handled in `Msg::Key`)
                        // becomes this action's binding.
                        let act = shown[self.drawer_sel.min(shown.len() - 1)];
                        self.rebind = Some(act);
                        self.notify(
                            ToastLevel::Info,
                            format!("Press a key to bind “{}” (Esc cancels)", act.help()),
                        );
                    }
                    KeyCode::Char('x') => {
                        let act = shown[self.drawer_sel.min(shown.len() - 1)];
                        self.keymaps.set_override(act, "none");
                        self.notify(ToastLevel::Warning, format!("Disabled “{}”", act.help()));
                    }
                    _ => {}
                }
            }
            Overlay::ThemePicker => match code {
                KeyCode::Esc => {
                    // Cancel: restore the palette we had before opening.
                    if let Some((t, n)) = self.theme_restore.take() {
                        self.theme = t;
                        self.theme_name = n;
                    }
                    self.overlay = Overlay::None;
                }
                KeyCode::Enter => {
                    // Apply the highlighted theme (covers "Enter without
                    // navigating first") and persist it for next run.
                    self.preview_picked();
                    if self.theme_picker.selected_theme().is_some() {
                        let name = self.theme_name.clone();
                        let _ = rstui_theme::Theme::write_choice(Self::theme_config_path(), &name);
                        self.notify(ToastLevel::Info, format!("Theme saved → {name}"));
                    }
                    self.theme_restore = None;
                    self.overlay = Overlay::None;
                }
                KeyCode::Up => {
                    self.theme_picker.prev();
                    self.preview_picked();
                }
                KeyCode::Down => {
                    self.theme_picker.next();
                    self.preview_picked();
                }
                KeyCode::Backspace => {
                    self.theme_picker.pop_filter();
                    self.preview_picked();
                }
                KeyCode::Char(c) => {
                    self.theme_picker.push_filter(c);
                    self.preview_picked();
                }
                _ => {}
            },
            Overlay::Help => {
                self.overlay = Overlay::None;
            }
            Overlay::None => {}
        }
        Cmd::none()
    }
}

impl App for KitchenSink {
    type Message = Msg;

    fn tick_rate(&self) -> Option<std::time::Duration> {
        // ~8 fps: enough for the spinner / shimmer / clock, cheap on a TTY.
        Some(std::time::Duration::from_millis(120))
    }

    fn on_tick(&self) -> Option<Msg> {
        Some(Msg::Tick)
    }

    fn on_event(&self, event: Event) -> Option<Msg> {
        match event {
            Event::Resize(size) => Some(Msg::Resized(size)),
            Event::Paste(text) => Some(Msg::Paste(text)),
            Event::Mouse(m) => match m.kind {
                MouseEventKind::Down(MouseButton::Left) => Some(Msg::MouseDown(m.position)),
                MouseEventKind::Drag(MouseButton::Left) => Some(Msg::MouseDrag(m.position)),
                MouseEventKind::Up(MouseButton::Left) => Some(Msg::MouseUp(m.position)),
                MouseEventKind::ScrollUp => Some(Msg::Scroll {
                    up: true,
                    at: m.position,
                }),
                MouseEventKind::ScrollDown => Some(Msg::Scroll {
                    up: false,
                    at: m.position,
                }),
                _ => None,
            },
            Event::Key(_) => {
                // Clipboard chords (Ctrl+C/X/V) are decided in `update`,
                // where the selection/focus state is known — Ctrl+C only
                // quits when there is *nothing* selected.
                let key = event.as_key_press()?;
                Some(Msg::Key(key.code, key.modifiers))
            }
            Event::FocusGained | Event::FocusLost => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Quit => return Cmd::quit(),
            Msg::Tick => {
                self.tick = self.tick.wrapping_add(1);
                let now = self.tick;
                // Toasts live ~40 frames (~5s at 8fps).
                self.notices.retain(|n| now.saturating_sub(n.born) < 40);
                // Drop a leader sequence that was never completed in time.
                self.keymaps.expire(now);
            }
            // No-op: `view` recaptures the true geometry from the resized
            // frame, so hit-testing follows the reflow automatically.
            Msg::Resized(_) => {}
            Msg::Paste(text) => {
                if self.overlay == Overlay::Palette {
                    self.palette_query.insert_str(&text);
                } else if self.pane == Pane::Content {
                    self.screens.on_paste(self.screen, &text);
                }
            }
            Msg::Key(code, mods) => {
                use keymap::Action;
                // Interactive re-bind capture (the settings drawer armed it):
                // the very next key becomes that action's binding; `Esc`
                // cancels. Short-circuits the whole keymap.
                if let Some(act) = self.rebind.take() {
                    if code == KeyCode::Esc {
                        self.notify(ToastLevel::Info, "Rebind cancelled");
                    } else {
                        let chord = keymap::Chord::from_event(&KeyEvent::new(code, mods));
                        self.keymaps.set_override(act, chord.spec());
                        self.notify(
                            ToastLevel::Success,
                            format!("Bound {} → {}", act.help(), chord.display()),
                        );
                    }
                    return Cmd::none();
                }
                let ctrl = mods.contains(KeyModifiers::CONTROL);
                // Resolve the event through the *active* keymap (this also
                // advances the leader-sequence state machine).
                let action = self.keymaps.resolve(&KeyEvent::new(code, mods), self.tick);
                let clip = matches!(action, Some(Action::Copy | Action::Cut | Action::Paste));

                // While an overlay is captured, only the clipboard actions
                // act; everything else is the overlay's own raw input
                // (palette typing, drawer keys, …).
                if self.overlay != Overlay::None {
                    if clip {
                        return self.do_action(action.unwrap());
                    }
                    return self.key_in_overlay(code);
                }
                // Clipboard chords act on every screen (they carry Ctrl/Cmd
                // and are never a typed character).
                if clip {
                    return self.do_action(action.unwrap());
                }
                // A focused text screen owns every plain character (so a
                // keymap key like `q`/`:`/a digit *types* instead of firing
                // its action) — non-char keys still reach the keymap.
                if self.pane == Pane::Content
                    && self.screen.is_text_entry()
                    && !ctrl
                    && matches!(code, KeyCode::Char(_))
                {
                    let out = self.screens.on_key(self.screen, code, self.tick);
                    if let Some((level, body)) = out.toast {
                        self.notify(level, body);
                    }
                    return Cmd::none();
                }
                // A resolved shell action.
                if let Some(a) = action {
                    self.clear_selection();
                    return self.do_action(a);
                }
                // A leader/prefix was just armed — swallow this key and wait
                // for the rest of the sequence.
                if self.keymaps.armed() {
                    return Cmd::none();
                }
                // Unbound by the keymap: raw rail / screen routing
                // (arrows, Enter, hjkl, PageUp, typing into a field, …).
                self.clear_selection();
                match self.pane {
                    Pane::Sidebar => match code {
                        KeyCode::Up | KeyCode::Char('k') => self.step_nav(-1),
                        KeyCode::Down | KeyCode::Char('j') => self.step_nav(1),
                        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                            self.open_selected()
                        }
                        _ => {}
                    },
                    Pane::Content => {
                        if code == KeyCode::Left {
                            // Left at a screen's edge falls back to the rail.
                            if !self.screens.on_key(self.screen, code, self.tick).handled {
                                self.pane = Pane::Sidebar;
                            }
                            return Cmd::none();
                        }
                        let out = self.screens.on_key(self.screen, code, self.tick);
                        if let Some((level, body)) = out.toast {
                            self.notify(level, body);
                        }
                    }
                }
            }
            Msg::MouseDown(pos) => {
                // A press replaces any prior selection and anchors a new one
                // *inside the text container under the pointer* (the screen
                // reports it, mirroring its own layout). The click itself is
                // deferred to release so a drag can preempt it.
                self.clear_selection();
                self.drag_moved = false;
                self.press = Some(pos);
                if self.overlay == Overlay::None {
                    let content = self.content_rect();
                    if content.contains(pos) {
                        if let Some(region) =
                            self.screens.selection_region(self.screen, pos, content)
                        {
                            if !region.is_empty() && region.contains(pos) {
                                self.sel_region.set(Some(region));
                                self.selection.start(pos);
                            }
                        }
                    }
                }
            }
            Msg::MouseDrag(pos) => {
                // A terminal only emits Drag on real movement; clamp to the
                // *container* the press began in so the row-major stream can
                // never spill into a neighbouring panel or the chrome.
                if !self.selection.is_empty() {
                    if let Some(r) = self.sel_region.get() {
                        let clamped = Position::new(
                            pos.x.clamp(r.x, r.right().saturating_sub(1)),
                            pos.y.clamp(r.y, r.bottom().saturating_sub(1)),
                        );
                        self.selection.extend(clamped);
                        self.drag_moved = true;
                    }
                }
            }
            Msg::MouseUp(pos) => {
                let had_press = self.press.take().is_some();
                if self.drag_moved && !self.selection.is_empty() {
                    // A real drag finished. `view` already extracted the
                    // covered text into `selected`.
                    let txt = self.selected.borrow().clone();
                    if txt.trim().is_empty() {
                        self.clear_selection();
                    } else if self.screens.selection_auto_copy(self.screen) {
                        // Auto-copy containers (read-only renders): the
                        // selection *is* a copy — straight to the clipboard.
                        self.do_copy(txt, "Copied");
                        self.clear_selection();
                    } else {
                        // Editable containers: keep the selection live so the
                        // user can Ctrl+C to copy or Ctrl+X to cut it.
                        let n = txt.chars().count();
                        self.notify(
                            ToastLevel::Info,
                            format!("Selected {n} chars — Ctrl+C copy · Ctrl+X cut"),
                        );
                    }
                } else if had_press {
                    // No drag → it was a click; selection collapses and the
                    // press is routed exactly as before.
                    self.clear_selection();
                    self.route_click(pos);
                }
                self.drag_moved = false;
            }
            Msg::Scroll { up, at } => {
                // The content shifts under a selection when it scrolls, so
                // drop it (the ADR 0012 §P1 "content changed" clear).
                self.clear_selection();
                if self.overlay == Overlay::None && self.content_rect().contains(at) {
                    self.screens.on_scroll(self.screen, up);
                } else if self.sidebar_rect().contains(at) {
                    self.step_nav(if up { -1 } else { 1 });
                }
            }
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        // One sample per painted frame — the header reads it back.
        self.fps.record();
        let area = frame.area();
        // Record exactly what this frame lays out so the click/scroll
        // reducer hit-tests the real geometry, not a guessed size.
        self.geom.set(Self::compute_geom(area));
        frame.buffer_mut().set_style(area, self.theme.screen());
        let [header, body, footer] = Self::shell_rows(area);
        let [sidebar, content] = Self::body_split(body);

        chrome::view_header(self, frame, header);
        chrome::view_sidebar(self, frame, sidebar);
        chrome::view_content(self, frame, content);
        // Project the selection over the freshly-drawn screen, then draw
        // overlays last so a modal/toast still sits above it.
        self.project_selection(frame);
        chrome::view_footer(self, frame, footer);
        chrome::view_overlays(self, frame, area);
    }
}

impl KitchenSink {
    /// Pure projection of the caller-owned [`Selection`] (ADR 0012 §P1):
    /// extract the covered text and overlay a high-contrast highlight — both
    /// **confined to the container** the drag began in (`sel_region`), so a
    /// multi-row stream never grabs a neighbouring panel or the chrome.
    /// Reads the selection, never mutates it.
    fn project_selection(&self, frame: &mut Frame<'_>) {
        let Some(cr) = self.sel_region.get().filter(|_| !self.selection.is_empty()) else {
            self.selected.borrow_mut().clear();
            return;
        };
        let buf = frame.buffer_mut();
        // A sub-buffer at the *same absolute coords* as the container, so
        // `selected_text`'s row-major stream is clipped to that panel, not
        // the whole frame (no border/neighbour/chrome bleeds into a copy).
        let mut sub = Buffer::empty(cr);
        for p in cr.positions() {
            if let Some(cell) = buf.get(p) {
                sub.set_cell(p, cell.symbol, cell.style());
            }
        }
        *self.selected.borrow_mut() = selected_text(&sub, &self.selection);
        let sel_style = Style::new().fg(self.theme.base).bg(self.theme.accent);
        for p in cr.positions() {
            if self.selection.contains(p) {
                if let Some(cell) = buf.get_mut(p) {
                    cell.apply_style(sel_style);
                }
            }
        }
    }
}

// Accessors the chrome / screens read (kept here so the model fields stay
// private to this module).
impl KitchenSink {
    /// The exact text the current drag-selection covers — what a copy would
    /// yield. Empty when nothing is selected. Exposed for tests and
    /// embedders so the confinement guarantee can be asserted precisely.
    #[must_use]
    pub fn last_selection(&self) -> String {
        self.selected.borrow().clone()
    }

    /// The in-app clipboard (what `Ctrl+V` would paste). Exposed for tests
    /// and embedders.
    #[must_use]
    pub fn clipboard(&self) -> &str {
        &self.clipboard
    }

    pub(crate) fn theme(&self) -> &Theme {
        &self.theme
    }
    /// The keymap registry (chrome reads it to draw live help/footer/drawer).
    pub(crate) fn keymaps(&self) -> &keymap::Keymaps {
        &self.keymaps
    }
    /// The selected action row in the drawer's keymap table.
    pub(crate) fn drawer_sel(&self) -> usize {
        self.drawer_sel
    }
    /// The action currently awaiting a captured key (rebind in progress).
    pub(crate) fn rebind(&self) -> Option<keymap::Action> {
        self.rebind
    }
    /// The active keymap's name (exposed for tests/embedders).
    #[must_use]
    pub fn active_keymap(&self) -> &'static str {
        self.keymaps.active_name()
    }
    pub(crate) fn theme_name(&self) -> &str {
        &self.theme_name
    }
    pub(crate) fn theme_picker(&self) -> &rstui_theme::ThemePickerState {
        &self.theme_picker
    }
    pub(crate) fn fps_label(&self) -> String {
        self.fps.label()
    }
    pub(crate) fn screen(&self) -> Screen {
        self.screen
    }
    pub(crate) fn nav(&self) -> usize {
        self.nav
    }
    pub(crate) fn pane(&self) -> Pane {
        self.pane
    }
    pub(crate) fn overlay(&self) -> Overlay {
        self.overlay
    }
    pub(crate) fn tick(&self) -> u64 {
        self.tick
    }
    pub(crate) fn notices(&self) -> &[Notice] {
        &self.notices
    }
    pub(crate) fn palette_query(&self) -> &TextEdit {
        &self.palette_query
    }
    pub(crate) fn palette_row(&self) -> usize {
        self.palette_row
    }
    pub(crate) fn screen_state(&self) -> &ScreenState {
        &self.screens
    }
}
