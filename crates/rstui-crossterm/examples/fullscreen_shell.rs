//! The whole-terminal app shell from
//! `rstui-runtime/examples/app_shell.rs`, run **live** on a real terminal in
//! one call.
//!
//! ```text
//! cargo run -p rstui-crossterm --example fullscreen_shell
//! ```
//!
//! This is the live twin of the headless `app_shell` snapshot test: the
//! *identical* `App` (copied verbatim — Cargo examples cannot share a module —
//! and kept byte-for-byte equal in logic) that the deterministic
//! [`Harness`](rstui_runtime::Harness) drives in `cargo test` here runs
//! unchanged on an actual TTY through [`run_app`], which owns the alternate
//! screen, raw mode, mouse + bracketed-paste + focus capture, the live event
//! loop, and panic-safe restore.
//!
//! Drive it by hand:
//!
//! - **`Tab` / `Shift+Tab`** moves focus across the Navigator / Editor /
//!   Inspector panes — the focused one is double-ruled and marked `▸`.
//! - **Click** a pane box to select it (the footer names the selection).
//! - **Paste** (bracketed paste) drops the text into the focused pane.
//! - **Resize** the terminal: the three panes reflow to the new width.
//! - Switch away from and back to the terminal: the shell **dims** while the
//!   window is unfocused (`[blur ]`), restoring on `[focus]`.
//! - **`q`** / **`Esc`** quits.
//!
//! It needs a TTY, so CI builds it — proving the whole full-screen stack
//! type-checks and composes — but does not execute it.

use std::error::Error;

use rstui_core::focus::{FocusId, FocusRing};
use rstui_core::{
    Color, Constraint, KeyCode, Layout, Modifier, MouseButton, MouseEvent, MouseEventKind,
    Position, Rect, Size, Style,
};
use rstui_crossterm::run_app;
use rstui_runtime::{App, Cmd, Event, Frame};

// === Shared App (kept byte-identical in logic with the headless ===========
// === `rstui-runtime/examples/app_shell.rs`; Cargo examples cannot =========
// === share a module, so it is copied verbatim) ============================

/// Stable focus identities for the three body panes, in `Tab` order.
///
/// Minted as `const`s per the [`FocusRing`] contract: focus *order* is the
/// explicit list passed to [`FocusRing::with_ids`], never derived from these
/// raw values, and "is this pane focused?" is a cheap `==`.
const NAVIGATOR: FocusId = FocusId::new(0);
const EDITOR: FocusId = FocusId::new(1);
const INSPECTOR: FocusId = FocusId::new(2);

/// The three panes in their fixed left-to-right / `Tab` order, paired with the
/// title each draws. A single source of truth for both the layout split and
/// the focus ring so the two can never disagree.
const PANES: [(FocusId, &str); 3] = [
    (NAVIGATOR, "Navigator"),
    (EDITOR, "Editor"),
    (INSPECTOR, "Inspector"),
];

/// The whole-terminal app: a header/body/footer shell whose body is three
/// focusable panes.
///
/// State is deliberately tiny — the focus ring, the latest pasted text, the
/// last pane a click selected, the size last reported by a resize, and whether
/// the OS says the terminal window is focused. Everything the screen shows is
/// a pure projection of these by [`view`](App::view); every change goes
/// through [`update`](App::update).
struct AppShell {
    /// Which pane the keyboard is aimed at. Caller-owned model state (ADR
    /// 0004): `update` steps it on `Tab`/click, `view` only reads it.
    focus: FocusRing,
    /// The most recent bracketed-paste payload, shown in the focused pane.
    pasted: String,
    /// The pane a left click last selected, for the footer read-out.
    last_click: Option<FocusId>,
    /// The terminal size last reported by an [`Event::Resize`] (seeded with
    /// the startup size). `update` hit-tests a click against the layout this
    /// size produces, so a click always maps to the boxes the user currently
    /// sees — the model-owned form of live resize reflow.
    size: Size,
    /// Whether the terminal *window* currently has OS focus. Distinct from
    /// [`focus`](Self::focus): this is `Event::FocusGained`/`Lost`, not the
    /// widget [`FocusRing`], and the two never share a type.
    window_focused: bool,
}

impl AppShell {
    /// The shell at the startup `size`, Navigator focused, window focused.
    fn new(size: Size) -> Self {
        Self {
            focus: FocusRing::with_ids(PANES.map(|(id, _)| id)),
            pasted: String::new(),
            last_click: None,
            size,
            window_focused: true,
        }
    }
}

/// Everything that can happen to the shell, mapped from input by
/// [`on_event`](App::on_event) and folded in by [`update`](App::update).
enum Msg {
    /// `Tab`: advance focus to the next pane (wraps).
    FocusNext,
    /// `Shift+Tab`: retreat focus to the previous pane (wraps).
    FocusPrev,
    /// A left click at this position: select the pane it falls in, if any.
    ClickAt(Position),
    /// A bracketed paste delivered this text in one chunk.
    Pasted(String),
    /// The terminal was resized to this new size.
    Resized(Size),
    /// The terminal window gained (`true`) or lost (`false`) OS focus.
    WindowFocus(bool),
    /// `q` / `Esc`: stop the program.
    Quit,
}

impl App for AppShell {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        match event {
            // Window focus is the OS telling the program its terminal gained
            // or lost focus — handled before the keymap because it is not a
            // key at all.
            Event::FocusGained => Some(Msg::WindowFocus(true)),
            Event::FocusLost => Some(Msg::WindowFocus(false)),
            // A resize must update the model's known size so the next click
            // hit-tests against the reflowed layout.
            Event::Resize(size) => Some(Msg::Resized(size)),
            // A bracketed paste arrives as one chunk, never as keystrokes.
            Event::Paste(text) => Some(Msg::Pasted(text)),
            // Only a left button *press* selects a pane; releases/moves/scroll
            // are ignored so a drag does not thrash the selection.
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                position,
                ..
            }) => Some(Msg::ClickAt(position)),
            // The keymap. `BackTab` is its own `KeyCode` (Shift+Tab), so the
            // back-traversal needs no modifier inspection.
            Event::Key(_) => match event.as_key_press()?.code {
                KeyCode::Tab => Some(Msg::FocusNext),
                KeyCode::BackTab => Some(Msg::FocusPrev),
                KeyCode::Char('q') | KeyCode::Esc => Some(Msg::Quit),
                _ => None,
            },
            Event::Mouse(_) => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::FocusNext => {
                self.focus.focus_next();
                Cmd::none()
            }
            Msg::FocusPrev => {
                self.focus.focus_prev();
                Cmd::none()
            }
            Msg::ClickAt(position) => {
                // Hit-test against the layout the *current* size produces, so
                // a click maps to exactly the box the user sees — at whatever
                // size the last resize left the terminal.
                let panes = pane_rects(Rect::from_size(self.size));
                if let Some(id) = pane_at(panes, position) {
                    self.focus.focus(id);
                    self.last_click = Some(id);
                }
                Cmd::none()
            }
            Msg::Pasted(text) => {
                self.pasted = text;
                Cmd::none()
            }
            Msg::Resized(size) => {
                self.size = size;
                Cmd::none()
            }
            Msg::WindowFocus(focused) => {
                self.window_focused = focused;
                Cmd::none()
            }
            Msg::Quit => Cmd::quit(),
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        // Header (1) / body (rest) / footer (1): the canonical full-screen
        // shell split. `Fill(1)` gives the body every row the bars leave, so
        // the frame reflows automatically on resize.
        let [header, body, footer] = shell_rows(area);

        // A blurred window dims every glyph; a focused one draws normally.
        // This is the OS window-focus concept (Event::FocusGained/Lost), not
        // the pane FocusRing — the shell-wide cue the two stay distinct.
        let shell = if self.window_focused {
            Style::new()
        } else {
            Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM)
        };

        self.view_header(frame, header, shell);
        self.view_body(frame, body, shell);
        self.view_footer(frame, footer, shell);
    }
}

impl AppShell {
    /// Draws the title bar: the app name and the OS window-focus state.
    fn view_header(&self, frame: &mut Frame<'_>, area: Rect, shell: Style) {
        let title = Style::new()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
            .patch(shell);
        let buffer = frame.buffer_mut();
        buffer.set_style(area, Style::new().bg(Color::Blue).patch(shell));
        buffer.set_str(area.position(), " rstui app-shell", title);
        // A glyph-level (not just colour) window-focus cue so the blur is
        // visible in the deterministic snapshot.
        let badge = if self.window_focused {
            "[focus]"
        } else {
            "[blur ]"
        };
        let x = area.right().saturating_sub(badge.len() as u16 + 1);
        buffer.set_str(Position::new(x, area.y), badge, title);
    }

    /// Draws the three panes, the focused one ruled with a double border so
    /// the focus move shows up in the glyph-only snapshot.
    fn view_body(&self, frame: &mut Frame<'_>, body: Rect, shell: Style) {
        for (rect, (id, title)) in body_columns(body).into_iter().zip(PANES) {
            let focused = self.focus.is_focused(id);
            self.view_pane(frame, rect, title, focused, shell);
        }
    }

    /// Draws one pane: a bordered box, its title, and — when focused — the
    /// live body content (the latest paste).
    fn view_pane(
        &self,
        frame: &mut Frame<'_>,
        rect: Rect,
        title: &str,
        focused: bool,
        shell: Style,
    ) {
        if rect.width < 2 || rect.height < 2 {
            return;
        }
        // The focused pane is double-ruled and bright; the rest single-ruled
        // and dim. The border *glyphs* differ, which is what makes the focus
        // move assertable in a colour-free snapshot.
        let edges = if focused {
            BorderGlyphs::DOUBLE
        } else {
            BorderGlyphs::SINGLE
        };
        let border = if focused {
            Style::new()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
                .patch(shell)
        } else {
            Style::new().fg(Color::DarkGray).patch(shell)
        };
        draw_box(frame, rect, edges, border);

        let label = format!("{}{}", if focused { "▸ " } else { "  " }, title);
        let text = Style::new()
            .add_modifier(if focused {
                Modifier::BOLD
            } else {
                Modifier::EMPTY
            })
            .patch(shell);
        frame
            .buffer_mut()
            .set_str(Position::new(rect.x + 1, rect.y + 1), &label, text);

        // Only the focused pane shows the live body content, so the snapshot
        // proves body content tracks focus.
        if focused && rect.height >= 4 {
            let shown = if self.pasted.is_empty() {
                "(empty)".to_string()
            } else {
                format!("paste: {}", self.pasted)
            };
            let inner = rect.width.saturating_sub(2) as usize;
            let mut clipped = shown;
            clipped.truncate(inner);
            frame
                .buffer_mut()
                .set_str(Position::new(rect.x + 1, rect.y + 3), &clipped, shell);
        }
    }

    /// Draws the footer: the keymap hint and the last click read-out.
    fn view_footer(&self, frame: &mut Frame<'_>, area: Rect, shell: Style) {
        let hint = Style::new()
            .fg(Color::Gray)
            .bg(Color::DarkGray)
            .patch(shell);
        let buffer = frame.buffer_mut();
        buffer.set_style(area, Style::new().bg(Color::DarkGray).patch(shell));
        // Kept short enough that the right-aligned selection read-out below
        // never overwrites it mid-word, so the footer stays legible.
        buffer.set_str(area.position(), " Tab/BackTab focus · q quit", hint);
        if let Some(id) = self.last_click {
            let name = PANES
                .iter()
                .find(|(pid, _)| *pid == id)
                .map_or("?", |(_, n)| *n);
            let read_out = format!("sel:{name}");
            let x = area.right().saturating_sub(read_out.len() as u16 + 1);
            buffer.set_str(Position::new(x, area.y), &read_out, hint);
        }
    }
}

/// The header/body/footer rows for a whole-terminal `area`.
///
/// The single vertical split both [`view`](App::view) and the pane
/// hit-test go through, so the bars and the body can never disagree and a
/// resize reflows every row at once.
fn shell_rows(area: Rect) -> [Rect; 3] {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area)
}

/// The three equal pane columns for a body `area`.
fn body_columns(area: Rect) -> [Rect; 3] {
    Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Fill(1),
        Constraint::Fill(1),
    ])
    .areas(area)
}

/// The three pane rectangles for a whole-terminal `area`: the body row split
/// into columns.
///
/// Composed from [`shell_rows`] + [`body_columns`] so the click hit-test in
/// `update` uses the *exact* geometry `view` draws — what the user sees and
/// what a click selects cannot drift, and resizing reflows both together.
fn pane_rects(area: Rect) -> [Rect; 3] {
    let [_, body, _] = shell_rows(area);
    body_columns(body)
}

/// The six box-drawing glyphs for one border style.
///
/// A tiny value type rather than six loose `char`s so [`draw_box`] takes one
/// argument and the focused/unfocused choice is one swap.
#[derive(Clone, Copy)]
struct BorderGlyphs {
    horizontal: char,
    vertical: char,
    top_left: char,
    top_right: char,
    bottom_left: char,
    bottom_right: char,
}

impl BorderGlyphs {
    /// A single-ruled border, for unfocused panes.
    const SINGLE: Self = Self {
        horizontal: '─',
        vertical: '│',
        top_left: '┌',
        top_right: '┐',
        bottom_left: '└',
        bottom_right: '┘',
    };

    /// A double-ruled border, for the focused pane — the glyph-level focus cue
    /// that survives a colour-free snapshot.
    const DOUBLE: Self = Self {
        horizontal: '═',
        vertical: '║',
        top_left: '╔',
        top_right: '╗',
        bottom_left: '╚',
        bottom_right: '╝',
    };
}

/// Strokes a one-cell border around `rect` using `glyphs`, styled with
/// `style`.
///
/// A plain `rstui-core` [`Buffer`] primitive — the example deliberately draws
/// its own box rather than pulling in the widget catalog, to prove the
/// full-screen shell needs only the core surface.
fn draw_box(frame: &mut Frame<'_>, rect: Rect, glyphs: BorderGlyphs, style: Style) {
    if rect.width < 2 || rect.height < 2 {
        return;
    }
    let buffer = frame.buffer_mut();
    let (left, right) = (rect.x, rect.right() - 1);
    let (top, bottom) = (rect.y, rect.bottom() - 1);

    for x in left..=right {
        buffer.set_cell(Position::new(x, top), glyphs.horizontal, style);
        buffer.set_cell(Position::new(x, bottom), glyphs.horizontal, style);
    }
    for y in top..=bottom {
        buffer.set_cell(Position::new(left, y), glyphs.vertical, style);
        buffer.set_cell(Position::new(right, y), glyphs.vertical, style);
    }
    buffer.set_cell(Position::new(left, top), glyphs.top_left, style);
    buffer.set_cell(Position::new(right, top), glyphs.top_right, style);
    buffer.set_cell(Position::new(left, bottom), glyphs.bottom_left, style);
    buffer.set_cell(Position::new(right, bottom), glyphs.bottom_right, style);
}

/// The pane whose rectangle contains `position`, or `None` if the click
/// landed outside every pane box (the header, footer, or a zero-area pane).
///
/// Pure geometry over [`Rect::contains`]: the same hit-test a live mouse
/// handler runs, with no terminal involved.
fn pane_at(panes: [Rect; 3], position: Position) -> Option<FocusId> {
    panes
        .into_iter()
        .zip(PANES)
        .find(|(rect, _)| rect.contains(position))
        .map(|(_, (id, _))| id)
}

// === Live entry point =====================================================

fn main() -> Result<(), Box<dyn Error>> {
    // The whole stack — alternate screen, raw mode, mouse/paste/focus capture,
    // panic-safe restore, the live event loop — in one call, driving the exact
    // `App` the headless `app_shell` snapshot test exercises. The seed size is
    // corrected by the first `Event::Resize` the live loop delivers; `?`
    // bubbles a `CrosstermRunError` and the terminal is already restored by
    // the time it returns, on success, error, or panic.
    run_app(AppShell::new(Size::new(44, 12)))?;
    Ok(())
}
