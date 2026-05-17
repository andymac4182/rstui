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
//! they *are* the chrome you drive, and each of the eight content screens
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
pub(crate) mod screens;
pub(crate) mod theme;

use rstui_core::{
    Constraint, Event, KeyCode, KeyModifiers, Layout, MouseButton, MouseEventKind, Position, Rect,
    Size, TextEdit,
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
    /// A left-button press at this cell.
    Click(Position),
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

/// The whole application: the active screen, navigation + focus state, the
/// animation clock, the live toast queue, and every screen's interactive
/// model. All of it is plain caller-owned data — widgets only ever read it.
pub struct KitchenSink {
    /// The terminal size last reported, so a click hit-tests the layout the
    /// user currently sees (the model-owned live-resize idiom).
    size: Size,
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
    /// The live toast queue; [`update`](App::update) expires old entries.
    notices: Vec<Notice>,
    /// The command-palette query buffer (a real editable [`TextEdit`]).
    palette_query: TextEdit,
    /// The command-palette keyboard cursor.
    palette_row: usize,
    /// Every screen's interactive state.
    screens: ScreenState,
}

impl KitchenSink {
    /// The app at startup `size`: the welcome screen, sidebar focused, the
    /// dark palette, no overlays, an empty toast queue.
    #[must_use]
    pub fn new(size: Size) -> Self {
        Self {
            size,
            screen: Screen::Welcome,
            nav: 0,
            pane: Pane::Sidebar,
            overlay: Overlay::None,
            tick: 0,
            theme: Theme::new(Mode::Dark),
            notices: Vec::new(),
            palette_query: TextEdit::new(),
            palette_row: 0,
            screens: ScreenState::new(),
        }
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

    /// The sidebar rect for the current size — the click hit-test and the
    /// renderer go through this one function so they cannot drift.
    fn sidebar_rect(&self) -> Rect {
        let [_, body, _] = Self::shell_rows(Rect::from_size(self.size));
        Self::body_split(body)[0]
    }

    /// The content rect (inside its frame border) for the current size.
    fn content_rect(&self) -> Rect {
        let [_, body, _] = Self::shell_rows(Rect::from_size(self.size));
        let inner = Self::body_split(body)[1];
        // Mirror the one-cell frame `view` draws around the screen.
        inner.inner(rstui_core::Margin::new(1, 1))
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
            Overlay::Drawer => match code {
                KeyCode::Esc | KeyCode::Char('g') => self.overlay = Overlay::None,
                KeyCode::Char('t') | KeyCode::Enter | KeyCode::Char(' ') => {
                    self.theme = Theme::new(self.theme.mode.toggled());
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
                MouseEventKind::Down(MouseButton::Left) => Some(Msg::Click(m.position)),
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
                let key = event.as_key_press()?;
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Some(Msg::Quit);
                }
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
            }
            Msg::Resized(size) => self.size = size,
            Msg::Paste(text) => {
                if self.overlay == Overlay::Palette {
                    self.palette_query.insert_str(&text);
                } else if self.pane == Pane::Content {
                    self.screens.on_paste(self.screen, &text);
                }
            }
            Msg::Key(code, _mods) => {
                if self.overlay != Overlay::None {
                    return self.key_in_overlay(code);
                }
                // Global chords first.
                match code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        self.overlay = Overlay::QuitConfirm;
                        return Cmd::none();
                    }
                    KeyCode::Char('?') => {
                        self.overlay = Overlay::Help;
                        return Cmd::none();
                    }
                    KeyCode::Char(':') => {
                        self.overlay = Overlay::Palette;
                        self.palette_query = TextEdit::new();
                        self.palette_row = 0;
                        return Cmd::none();
                    }
                    KeyCode::Char('g') => {
                        self.overlay = Overlay::Drawer;
                        return Cmd::none();
                    }
                    KeyCode::Tab => {
                        self.pane = match self.pane {
                            Pane::Sidebar => Pane::Content,
                            Pane::Content => Pane::Sidebar,
                        };
                        return Cmd::none();
                    }
                    KeyCode::Char(d @ '1'..='8') => {
                        let idx = (d as u8 - b'1') as usize;
                        if idx < Screen::ALL.len() {
                            self.nav = idx;
                            self.screen = Screen::ALL[idx];
                            self.pane = Pane::Content;
                        }
                        return Cmd::none();
                    }
                    _ => {}
                }
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
            Msg::Click(pos) => {
                if self.overlay != Overlay::None {
                    // A click anywhere dismisses a passive overlay; the
                    // drawer/palette keep their own keys.
                    if matches!(self.overlay, Overlay::Help | Overlay::QuitConfirm) {
                        self.overlay = Overlay::None;
                    }
                    return Cmd::none();
                }
                let sidebar = self.sidebar_rect();
                if sidebar.contains(pos) {
                    // The sidebar inner starts one row down (frame title).
                    let row = pos.y.saturating_sub(sidebar.y + 1) as usize;
                    if row < Screen::ALL.len() {
                        self.nav = row;
                        self.screen = Screen::ALL[row];
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
            Msg::Scroll { up, at } => {
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
        let area = frame.area();
        frame.buffer_mut().set_style(area, self.theme.screen());
        let [header, body, footer] = Self::shell_rows(area);
        let [sidebar, content] = Self::body_split(body);

        chrome::view_header(self, frame, header);
        chrome::view_sidebar(self, frame, sidebar);
        chrome::view_content(self, frame, content);
        chrome::view_footer(self, frame, footer);
        chrome::view_overlays(self, frame, area);
    }
}

// Accessors the chrome / screens read (kept here so the model fields stay
// private to this module).
impl KitchenSink {
    pub(crate) fn theme(&self) -> &Theme {
        &self.theme
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
