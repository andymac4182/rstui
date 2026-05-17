//! The persistent shell around every screen: the header bar, the
//! [`Sidebar`] navigation rail, the framed content panel, the [`StatusBar`]
//! footer, and the global [`HelpOverlay`] / [`CommandPalette`] / [`Drawer`] /
//! [`Modal`] / [`Toast`] layers.
//!
//! Each of those is a real catalog widget doing its real job, not a static
//! sample — driving the app *is* the demo of the chrome.

use rstui_core::{Constraint, Line, Modifier, Position, Rect, Style, TextEdit, stylize::Stylize};
use rstui_runtime::Frame;
use rstui_widgets::{
    Block, BorderType, CommandPalette, Drawer, DrawerSide, HelpEntry, HelpOverlay, Modal,
    Paragraph, Sidebar, SidebarItem, StatusBar, Toast, ToastCorner, ToastMessage, Wrap,
};

use crate::screens::Screen;
use crate::{KitchenSink, Overlay, Pane};

/// The global keymap, shown in the help overlay and (abbreviated) the footer.
const KEYMAP: &[(&str, &str)] = &[
    ("1-8", "jump to a screen"),
    ("Tab", "move focus rail / screen"),
    ("↑ ↓ ← →", "navigate / adjust"),
    ("Enter Space", "activate / toggle"),
    (":", "command palette"),
    ("?", "this help"),
    ("g", "settings drawer"),
    ("q Esc", "quit (confirm)"),
];

/// The screen indices whose label or title contains `query`
/// (case-insensitive). The command palette is a pure projection of this.
pub(crate) fn palette_matches(query: &TextEdit) -> Vec<usize> {
    let q = query.value().to_lowercase();
    Screen::ALL
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            q.is_empty()
                || s.label().to_lowercase().contains(&q)
                || s.title().to_lowercase().contains(&q)
        })
        .map(|(i, _)| i)
        .collect()
}

/// The title bar: brand on the left, the active screen title centred, the
/// animation clock + palette mode on the right.
pub(crate) fn view_header(ks: &KitchenSink, frame: &mut Frame<'_>, area: Rect) {
    let theme = ks.theme();
    let bar = Style::new().fg(theme.text).bg(theme.raised);
    let buffer = frame.buffer_mut();
    buffer.set_style(area, bar);
    buffer.set_str(
        area.position(),
        " rstui ✦ kitchen-sink",
        bar.fg(theme.accent).add_modifier(Modifier::BOLD),
    );

    let title = ks.screen().title();
    let cx = area.x + area.width.saturating_sub(title.len() as u16) / 2;
    buffer.set_str(Position::new(cx, area.y), title, bar);

    let right = format!("{} · ◴ {:04}", theme.mode.label(), ks.tick() % 10_000);
    let rx = area.right().saturating_sub(right.len() as u16 + 1);
    buffer.set_str(
        Position::new(rx, area.y),
        &right,
        bar.fg(theme.dim).add_modifier(Modifier::BOLD),
    );
}

/// The navigation rail — a live [`Sidebar`] whose selection is the model's
/// `nav` cursor and whose border brightens when the rail has focus.
pub(crate) fn view_sidebar(ks: &KitchenSink, frame: &mut Frame<'_>, area: Rect) {
    let theme = ks.theme();
    let focused = ks.pane() == Pane::Sidebar;
    let items: Vec<SidebarItem> = Screen::ALL
        .iter()
        .map(|s| SidebarItem::new(s.label()).icon(s.icon()))
        .collect();

    let border = if focused {
        theme.border_focused()
    } else {
        theme.border()
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(" Screens ").style(theme.heading()))
        .border_style(border)
        .style(Style::new().bg(theme.raised));

    frame.render_widget(
        Sidebar::new(&items)
            .selected(Some(ks.nav()))
            .block(block)
            .style(Style::new().fg(theme.text).bg(theme.raised))
            .highlight_style(theme.selection()),
        area,
    );
}

/// The framed content panel; its border brightens when the screen has focus.
pub(crate) fn view_content(ks: &KitchenSink, frame: &mut Frame<'_>, area: Rect) {
    let theme = ks.theme();
    let focused = ks.pane() == Pane::Content;
    let border = if focused {
        theme.border_focused()
    } else {
        theme.border()
    };
    let title = format!(" {}  ", ks.screen().label());
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(title).style(theme.heading()))
        .border_style(border)
        .style(theme.body());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    ks.screen_state()
        .view(ks.screen(), theme, ks.tick(), frame, inner);
}

/// The footer — a real [`StatusBar`]: focus hint left, the screen position
/// centred, the palette mode right.
pub(crate) fn view_footer(ks: &KitchenSink, frame: &mut Frame<'_>, area: Rect) {
    let theme = ks.theme();
    let style = Style::new().fg(theme.dim).bg(theme.raised);
    let pane = match ks.pane() {
        Pane::Sidebar => "RAIL",
        Pane::Content => "SCREEN",
    };
    frame.render_widget(
        StatusBar::new()
            .left(Line::from(" : palette  ? help  g settings  q quit").style(style))
            .center(
                Line::from(format!("[{pane}]  {} / 8", ks.screen().index() + 1))
                    .style(style.fg(theme.accent).add_modifier(Modifier::BOLD)),
            )
            .right(Line::from(format!("rstui · {} ", theme.mode.label())).style(style))
            .style(style),
        area,
    );
}

/// Every floating layer, drawn last so it sits above the screen. Toasts are
/// always drawn (even over an overlay) since they are transient status.
pub(crate) fn view_overlays(ks: &KitchenSink, frame: &mut Frame<'_>, area: Rect) {
    let theme = ks.theme();
    match ks.overlay() {
        Overlay::None => {}
        Overlay::Help => view_help(ks, frame, area),
        Overlay::Palette => view_palette(ks, frame, area),
        Overlay::Drawer => view_drawer(ks, frame, area),
        Overlay::QuitConfirm => view_quit_confirm(ks, frame, area),
    }

    if !ks.notices().is_empty() {
        let msgs: Vec<ToastMessage> = ks
            .notices()
            .iter()
            .rev()
            .take(4)
            .map(|n| ToastMessage::new(n.level, n.body.as_str()))
            .collect();
        frame.render_widget(
            Toast::new(&msgs)
                .corner(ToastCorner::TopRight)
                .max_visible(4)
                .gap(1)
                .style(Style::new().fg(theme.text).bg(theme.surface)),
            area,
        );
    }
    let _ = theme;
}

/// The global help overlay — a real [`HelpOverlay`] over the keymap.
fn view_help(ks: &KitchenSink, frame: &mut Frame<'_>, area: Rect) {
    let theme = ks.theme();
    let entries: Vec<HelpEntry> = KEYMAP
        .iter()
        .map(|(keys, desc)| HelpEntry::new([*keys], *desc))
        .collect();
    frame.render_widget(
        HelpOverlay::new(&entries)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(Line::from(" Keyboard ").style(theme.heading())),
            )
            .style(theme.body())
            .key_style(theme.accent_text())
            .description_style(theme.body())
            .backdrop_style(Style::new().fg(theme.dim)),
        area,
    );
}

/// The command palette — a real [`CommandPalette`] over an editable query
/// and the live screen matches.
fn view_palette(ks: &KitchenSink, frame: &mut Frame<'_>, area: Rect) {
    let theme = ks.theme();
    let results: Vec<Line> = palette_matches(ks.palette_query())
        .into_iter()
        .map(|i| {
            Line::from(format!(
                "{}  {}",
                Screen::ALL[i].icon(),
                Screen::ALL[i].label()
            ))
            .style(theme.body())
        })
        .collect();
    frame.render_widget(
        CommandPalette::new(ks.palette_query(), &results)
            .highlight(ks.palette_row())
            .focused(true)
            .prompt("› ")
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(Line::from(" Go to screen ").style(theme.heading())),
            )
            .style(theme.body())
            .highlight_style(theme.selection())
            .backdrop_style(Style::new().fg(theme.dim)),
        area,
    );
}

/// The settings drawer — a real right-side [`Drawer`]; `t` toggles the
/// palette live so the whole colour path is seen to reflow.
fn view_drawer(ks: &KitchenSink, frame: &mut Frame<'_>, area: Rect) {
    let theme = ks.theme();
    let drawer = Drawer::new()
        .open(true)
        .side(DrawerSide::Right)
        .size(Constraint::Length(36))
        .block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .title(Line::from(" Settings ").style(theme.heading())),
        )
        .style(theme.body())
        .backdrop_style(Style::new().fg(theme.dim));
    let inner = drawer.inner(area);
    frame.render_widget(drawer, area);

    let body = format!(
        "Theme palette\n\n  ● {}  (every colour is 24-bit RGB)\n\n  t / Space / Enter  toggle Dark / Light\n  Esc / g            close\n\nThe swap repaints the whole app, proving\nthe full-colour path is live, not baked.",
        theme.mode.label()
    );
    frame.render_widget(
        Paragraph::new(body)
            .style(theme.body())
            .wrap(Wrap { trim: true }),
        inner,
    );
}

/// The quit-confirmation dialog — a real centred [`Modal`].
fn view_quit_confirm(ks: &KitchenSink, frame: &mut Frame<'_>, area: Rect) {
    let theme = ks.theme();
    let modal = Modal::new()
        .block(
            Block::bordered()
                .border_type(BorderType::Double)
                .title(Line::from(" Quit? ").style(theme.heading())),
        )
        .style(theme.body())
        .backdrop_style(Style::new().fg(theme.dim));
    let inner = modal.inner(area);
    frame.render_widget(modal, area);
    frame.render_widget(
        Paragraph::new(
            Line::from("Leave the kitchen sink?")
                .style(theme.body())
                .centered(),
        ),
        inner,
    );
    if inner.height >= 3 {
        let prompt = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
        frame.render_widget(
            Line::from(vec![
                " y ".bold().fg(theme.base).bg(theme.err),
                "  Yes      ".fg(theme.dim),
                " n ".bold().fg(theme.base).bg(theme.accent),
                "  Stay".fg(theme.dim),
            ]),
            prompt,
        );
    }
}
