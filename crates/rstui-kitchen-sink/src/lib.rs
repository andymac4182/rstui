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

use std::cell::{Cell, RefCell};

use rstui_core::{
    Buffer, Constraint, Event, KeyCode, KeyModifiers, Layout, Margin, MouseButton, MouseEventKind,
    Position, Rect, Selection, Size, Style, TextEdit, selected_text,
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
            notices: Vec::new(),
            palette_query: TextEdit::new(),
            palette_row: 0,
            screens: ScreenState::new(),
            selection: Selection::new(),
            press: None,
            drag_moved: false,
            selected: RefCell::new(String::new()),
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
            Msg::Key(code, _mods) => {
                if self.overlay != Overlay::None {
                    return self.key_in_overlay(code);
                }
                // A focused text screen owns *every* character (so `q`, `:`,
                // digits, space all type) — non-char keys (Tab, Esc, arrows,
                // Enter) still flow through the global keymap below.
                if self.pane == Pane::Content
                    && self.screen.is_text_entry()
                    && matches!(code, KeyCode::Char(_))
                {
                    let out = self.screens.on_key(self.screen, code, self.tick);
                    if let Some((level, body)) = out.toast {
                        self.notify(level, body);
                    }
                    return Cmd::none();
                }
                // Any navigation/overlay key drops a stale mouse selection
                // (the content is about to change under it).
                self.selection.clear();
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
                    KeyCode::Char(d @ '1'..='9') => {
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
            Msg::MouseDown(pos) => {
                // A press replaces any prior selection and anchors a new one
                // (only over the content — chrome is not selectable). The
                // click itself is deferred to release so a drag can preempt it.
                self.selection.clear();
                self.drag_moved = false;
                self.press = Some(pos);
                if self.overlay == Overlay::None && self.content_rect().contains(pos) {
                    self.selection.start(pos);
                }
            }
            Msg::MouseDrag(pos) => {
                // A terminal only emits Drag on real movement; clamp to the
                // content so the row-major stream stays inside the screen.
                if !self.selection.is_empty() {
                    let c = self.content_rect();
                    let clamped = Position::new(
                        pos.x.clamp(c.x, c.right().saturating_sub(1)),
                        pos.y.clamp(c.y, c.bottom().saturating_sub(1)),
                    );
                    self.selection.extend(clamped);
                    self.drag_moved = true;
                }
            }
            Msg::MouseUp(pos) => {
                let had_press = self.press.take().is_some();
                if self.drag_moved && !self.selection.is_empty() {
                    // A real drag: the selection is the user's "copy". `view`
                    // already extracted the covered text into `selected`.
                    let txt = self.selected.borrow().clone();
                    if txt.trim().is_empty() {
                        self.selection.clear();
                    } else {
                        let n = txt.chars().count();
                        let preview: String = txt
                            .chars()
                            .take(28)
                            .map(|c| if c == '\n' { '⏎' } else { c })
                            .collect();
                        self.notify(ToastLevel::Success, format!("Copied {n} chars: {preview}"));
                    }
                } else if had_press {
                    // No drag → it was a click; selection collapses and the
                    // press is routed exactly as before.
                    self.selection.clear();
                    self.route_click(pos);
                }
                self.drag_moved = false;
            }
            Msg::Scroll { up, at } => {
                // The content shifts under a selection when it scrolls, so
                // drop it (the ADR 0012 §P1 "content changed" clear).
                self.selection.clear();
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
    /// extract the covered text (confined to the content so a multi-row
    /// stream never grabs chrome) and overlay a high-contrast highlight on
    /// the selected cells. Reads the selection, never mutates it.
    fn project_selection(&self, frame: &mut Frame<'_>) {
        if self.selection.is_empty() {
            self.selected.borrow_mut().clear();
            return;
        }
        let cr = self.content_rect();
        let buf = frame.buffer_mut();
        // A sub-buffer at the *same absolute coords* as the content, so
        // `selected_text`'s row-major stream is clipped to the screen, not
        // the whole frame (no sidebar/footer text bleeds into a copy).
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
