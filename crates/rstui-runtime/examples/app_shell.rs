//! A whole-terminal application shell, driven headless by the deterministic
//! [`Harness`] so it doubles as an L3 snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-runtime --example app_shell
//! cargo test -p rstui-runtime --examples
//! ```
//!
//! Where `counter` proves the *reducer* contract in one line, this proves the
//! *full-screen runtime* contract end to end on a real layout:
//!
//! - **Header / body / footer** carved with a vertical [`Layout`], the body
//!   split again horizontally into three focusable panes — the OpenTUI-style
//!   app frame, expressed entirely with `rstui-core` [`Buffer`] primitives (no
//!   widget catalog).
//! - **`Tab` / `BackTab` focus traversal** across the panes via a
//!   [`FocusRing`] that lives in the model, mutated only by `update` and read
//!   by the pure `view` — exactly the ADR 0004 caller-owned focus contract.
//!   The focused pane is drawn with a double-ruled border so the focus move is
//!   visible in the glyph-only snapshot, not just in colour.
//! - **Live resize reflow**: an [`Event::Resize`] updates the model's known
//!   size, and the next frame re-runs the same layout so the three panes
//!   re-tile to the new width — and a click still hit-tests the boxes the user
//!   now sees.
//! - **Mouse / paste / terminal-focus events**: a left click selects the pane
//!   under the pointer (hit-tested with [`Rect::contains`]), a bracketed
//!   [`Event::Paste`] drops its text into the focused pane, and
//!   [`Event::FocusGained`] / [`Event::FocusLost`] dim the whole shell — the
//!   OS window-focus concept, kept distinct from the widget [`FocusRing`].
//!
//! Every step below asserts [`Harness::snapshot`], and the same assertions run
//! under `#[test]` so `cargo test -p rstui-runtime --examples` exercises the
//! whole shell with no TTY, threads, or clock.

use rstui_core::focus::{FocusId, FocusRing};
use rstui_core::{
    Color, Constraint, KeyCode, KeyEvent, KeyModifiers, Layout, Modifier, MouseButton, MouseEvent,
    MouseEventKind, Position, Rect, Size, Style,
};
use rstui_runtime::{App, Cmd, Event, Frame, Harness};

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

/// The fixed surface the scripted `main` and the tests render into. A live
/// backend reports this via the terminal; here it is pinned so snapshots are
/// exact.
const SURFACE: Size = Size::new(44, 12);

fn main() {
    let mut harness = Harness::new(AppShell::new(SURFACE), SURFACE.width, SURFACE.height);
    println!("start (Navigator focused, window focused):");
    println!("{}", harness.snapshot());

    // A scripted whole-terminal session. Each line is one Event flowing
    // through on_event -> update -> view, exactly as a live terminal would
    // deliver it — and each is asserted in the test module below.
    let steps: &[(&str, Event)] = &[
        ("Tab -> Editor focused", tab()),
        ("Tab -> Inspector focused", tab()),
        ("BackTab -> Editor focused", back_tab()),
        ("Paste into Editor", Event::Paste("hello shell".to_string())),
        ("Click Navigator box", click(3, 5)),
        ("FocusLost -> shell dims", Event::FocusLost),
        ("FocusGained -> shell restored", Event::FocusGained),
    ];
    for (note, event) in steps {
        harness.handle(event.clone());
        println!("{note}:");
        println!("{}", harness.snapshot());
    }

    // Resize: the next frame re-runs the same layout, so the three panes
    // re-tile to the new width with no app-side bookkeeping.
    harness.resize(60, 12);
    println!("Resize 60x12 -> panes reflow:");
    println!("{}", harness.snapshot());

    harness.handle(Event::from(KeyEvent::char('q')));
    assert!(!harness.is_running(), "q must quit the shell");
    println!("q -> quit (running = {})", harness.is_running());
}

/// A `Tab` key press event.
fn tab() -> Event {
    Event::from(KeyEvent::from_code(KeyCode::Tab))
}

/// A `Shift+Tab` (back-tab) key press event.
fn back_tab() -> Event {
    Event::from(KeyEvent::from_code(KeyCode::BackTab))
}

/// A left-button press at `(x, y)`.
fn click(x: u16, y: u16) -> Event {
    Event::from(MouseEvent::new(
        MouseEventKind::Down(MouseButton::Left),
        Position::new(x, y),
        KeyModifiers::NONE,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a harness on the pinned surface, so every test sees the exact
    /// layout the scripted `main` does.
    fn shell() -> Harness<AppShell> {
        Harness::new(AppShell::new(SURFACE), SURFACE.width, SURFACE.height)
    }

    /// The initial frame: header/body/footer, Navigator focused (double-ruled,
    /// `▸` marker, body content shown), the window focused.
    #[test]
    fn initial_shell_has_navigator_focused_and_window_focused() {
        let harness = shell();
        assert_eq!(
            harness.snapshot(),
            " rstui app-shell                    [focus] \n\
╔═════════════╗┌─────────────┐┌────────────┐\n\
║▸ Navigator  ║│  Editor     ││  Inspector │\n\
║             ║│             ││            │\n\
║(empty)      ║│             ││            │\n\
║             ║│             ││            │\n\
║             ║│             ││            │\n\
║             ║│             ││            │\n\
║             ║│             ││            │\n\
║             ║│             ││            │\n\
╚═════════════╝└─────────────┘└────────────┘\n\
\x20Tab/BackTab focus · q quit                 \n"
        );
        assert!(harness.is_running());
    }

    /// `Tab` advances focus Navigator -> Editor: the double border and the
    /// `▸` marker move to the middle pane, and the body content follows.
    #[test]
    fn tab_moves_focus_and_body_content_to_the_next_pane() {
        let mut harness = shell();
        harness.handle(tab());
        assert_eq!(
            harness.snapshot(),
            " rstui app-shell                    [focus] \n\
┌─────────────┐╔═════════════╗┌────────────┐\n\
│  Navigator  │║▸ Editor     ║│  Inspector │\n\
│             │║             ║│            │\n\
│             │║(empty)      ║│            │\n\
│             │║             ║│            │\n\
│             │║             ║│            │\n\
│             │║             ║│            │\n\
│             │║             ║│            │\n\
│             │║             ║│            │\n\
└─────────────┘╚═════════════╝└────────────┘\n\
\x20Tab/BackTab focus · q quit                 \n"
        );
    }

    /// `Tab` twice lands on Inspector; `BackTab` then steps back to Editor —
    /// proving forward and backward traversal across panes.
    #[test]
    fn tab_then_back_tab_traverses_panes_both_ways() {
        let mut harness = shell();
        harness.handle(tab());
        harness.handle(tab());
        assert!(harness.app().focus.is_focused(INSPECTOR));
        harness.handle(back_tab());
        assert!(harness.app().focus.is_focused(EDITOR));
    }

    /// `Tab` from the last pane wraps back to the first (the [`FocusRing`]
    /// traversal is total and wrapping).
    #[test]
    fn tab_wraps_from_the_last_pane_to_the_first() {
        let mut harness = shell();
        harness.handle(tab());
        harness.handle(tab());
        harness.handle(tab());
        assert!(harness.app().focus.is_focused(NAVIGATOR));
    }

    /// A bracketed paste lands in the focused pane's body and nowhere else.
    #[test]
    fn paste_text_appears_in_the_focused_pane() {
        let mut harness = shell();
        harness.handle(tab()); // focus Editor
        harness.handle(Event::Paste("hello shell".to_string()));
        assert_eq!(
            harness.snapshot(),
            " rstui app-shell                    [focus] \n\
┌─────────────┐╔═════════════╗┌────────────┐\n\
│  Navigator  │║▸ Editor     ║│  Inspector │\n\
│             │║             ║│            │\n\
│             │║paste: hello ║│            │\n\
│             │║             ║│            │\n\
│             │║             ║│            │\n\
│             │║             ║│            │\n\
│             │║             ║│            │\n\
│             │║             ║│            │\n\
└─────────────┘╚═════════════╝└────────────┘\n\
\x20Tab/BackTab focus · q quit                 \n"
        );
    }

    /// A left click selects the pane under the pointer: clicking the
    /// Navigator box while Editor is focused moves focus back and records the
    /// selection in the footer.
    #[test]
    fn left_click_selects_the_pane_under_the_pointer() {
        let mut harness = shell();
        harness.handle(tab()); // focus Editor
        assert!(harness.app().focus.is_focused(EDITOR));
        harness.handle(click(3, 5)); // inside the Navigator box
        assert!(harness.app().focus.is_focused(NAVIGATOR));
        assert_eq!(harness.app().last_click, Some(NAVIGATOR));
        // The footer read-out names the selected pane.
        assert!(harness.snapshot().contains("sel:Navigator"));
    }

    /// A click on the header row selects nothing and leaves focus and the
    /// read-out untouched.
    #[test]
    fn a_click_outside_every_pane_box_is_ignored() {
        let mut harness = shell();
        // Row 0 is the header bar, never a pane.
        harness.handle(click(5, 0));
        assert!(harness.app().focus.is_focused(NAVIGATOR));
        assert_eq!(harness.app().last_click, None);
    }

    /// `Event::FocusLost` dims the whole shell and `FocusGained` restores it —
    /// the OS window-focus cue, distinct from the pane [`FocusRing`], visible
    /// in the snapshot via the `[blur ]` / `[focus]` badge.
    #[test]
    fn window_focus_lost_then_gained_dims_then_restores_the_shell() {
        let mut harness = shell();
        harness.handle(Event::FocusLost);
        assert!(!harness.app().window_focused);
        assert!(harness.snapshot().contains("[blur ]"));
        // The pane focus is unaffected by window focus — different concept.
        assert!(harness.app().focus.is_focused(NAVIGATOR));

        harness.handle(Event::FocusGained);
        assert!(harness.app().window_focused);
        assert!(harness.snapshot().contains("[focus]"));
    }

    /// A resize re-runs the same layout, so the three panes re-tile to the new
    /// width with no app-side bookkeeping — live reflow. A click then still
    /// hit-tests the boxes the user now sees.
    #[test]
    fn resize_reflows_the_three_panes_and_keeps_clicks_accurate() {
        let mut harness = shell();
        harness.resize(60, 12);
        let snap = harness.snapshot();
        // Every row is now 60 wide.
        for line in snap.lines() {
            assert_eq!(line.chars().count(), 60, "row not reflowed: {line:?}");
        }
        // Still three pane boxes, Navigator still focused (double-ruled).
        assert!(snap.contains('╔') && snap.contains('▸'));

        // A click far to the right now lands in the (reflowed) Inspector pane;
        // it would have been out of bounds at the old 44-wide size.
        harness.handle(click(50, 5));
        assert!(harness.app().focus.is_focused(INSPECTOR));
    }

    /// `q` quits the shell and further input is frozen — the runtime
    /// `Cmd::quit` contract, exercised through the full-screen app.
    #[test]
    fn q_quits_and_freezes_further_input() {
        let mut harness = shell();
        harness.handle(tab());
        assert!(harness.app().focus.is_focused(EDITOR));
        harness.handle(Event::from(KeyEvent::char('q')));
        assert!(!harness.is_running());

        // Input after quit is ignored; focus stays where it was.
        harness.handle(tab());
        assert!(harness.app().focus.is_focused(EDITOR));
    }
}
