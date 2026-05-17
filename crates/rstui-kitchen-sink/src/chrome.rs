//! The persistent shell around every screen: the header bar, the
//! [`Sidebar`] navigation rail, the framed content panel, the [`StatusBar`]
//! footer, and the global [`HelpOverlay`] / [`CommandPalette`] / [`Drawer`] /
//! [`Modal`] / [`Toast`] layers.
//!
//! Each of those is a real catalog widget doing its real job, not a static
//! sample — driving the app *is* the demo of the chrome.

use rstui_core::{
    Constraint, Layout, Line, Modifier, Position, Rect, Style, TextEdit, stylize::Stylize,
};
use rstui_runtime::Frame;
use rstui_widgets::{
    Block, BorderType, CommandPalette, Drawer, DrawerSide, HelpEntry, HelpOverlay, List, Modal,
    Paragraph, Sidebar, SidebarItem, StatusBar, Toast, ToastCorner, ToastMessage, Wrap,
};

use crate::keymap;
use crate::screens::Screen;
use crate::{KitchenSink, Overlay, Pane};

/// Raw screen-level keys the keymap deliberately doesn't own (they fall
/// through to the focused screen) — appended to help for completeness.
const RAW_HELP: &[(&str, &str)] = &[
    ("↑ ↓ ← →", "navigate / adjust"),
    ("Enter Space", "activate / toggle"),
    ("PgUp PgDn", "page / scroll"),
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
    let items: Vec<SidebarItem> = Screen::sidebar_rows()
        .into_iter()
        .map(|row| match row {
            crate::screens::SidebarRow::Group(g) => SidebarItem::group(g),
            crate::screens::SidebarRow::Item(i) => {
                let s = Screen::ALL[i];
                SidebarItem::new(s.label()).icon(s.icon())
            }
        })
        .collect();
    let inner_rows = area.height.saturating_sub(2);
    let selected = Screen::sidebar_selected_row(ks.nav());
    let offset = Screen::sidebar_offset(ks.nav(), inner_rows);

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
            .selected(Some(selected))
            .offset(offset)
            .block(block)
            .style(Style::new().fg(theme.text).bg(theme.raised))
            .group_style(theme.caption())
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
    // Hints derive from the *live* keymap, so they always show the real
    // keys (per-OS, per-keymap, after any user remap).
    let km = ks.keymaps().effective();
    let (kmname, armed) = ks.keymaps().status();
    let left = format!(
        " {} palette · {} help · {} settings · {} quit",
        km.keys_for(keymap::Action::Palette),
        km.keys_for(keymap::Action::Help),
        km.keys_for(keymap::Action::Drawer),
        km.keys_for(keymap::Action::Quit),
    );
    let right = if armed {
        format!(" ⟨leader⟩… · {} · {} ", kmname, keymap::Keymaps::os_name())
    } else {
        format!(" {} · {} ", kmname, keymap::Keymaps::os_name())
    };
    frame.render_widget(
        StatusBar::new()
            .left(Line::from(left).style(style))
            .center(
                Line::from(format!(
                    "[{pane}]  {} / {}",
                    ks.screen().index() + 1,
                    Screen::ALL.len()
                ))
                .style(style.fg(theme.accent).add_modifier(Modifier::BOLD)),
            )
            .right(Line::from(right).style(style))
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

/// The global help overlay — a real [`HelpOverlay`] built from the **live**
/// keymap (reverse lookup), so a keymap switch or a user remap is reflected
/// immediately (the Textual footer-doesn't-follow bug, done right).
fn view_help(ks: &KitchenSink, frame: &mut Frame<'_>, area: Rect) {
    let theme = ks.theme();
    let km = ks.keymaps().effective();
    // Owned so the borrowed `HelpEntry`s outlive the render call.
    let mut rows: Vec<(String, &'static str)> = keymap::Action::shown()
        .into_iter()
        .map(|a| (km.keys_for(a), a.help()))
        .collect();
    for (k, d) in RAW_HELP {
        rows.push(((*k).to_string(), *d));
    }
    let entries: Vec<HelpEntry> = rows
        .iter()
        .map(|(k, d)| HelpEntry::new([k.as_str()], *d))
        .collect();
    let title = format!(
        " Keyboard · {} · {} ",
        ks.keymaps().active_name(),
        keymap::Keymaps::os_name()
    );
    frame.render_widget(
        HelpOverlay::new(&entries)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(Line::from(title).style(theme.heading())),
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

    let km = ks.keymaps().effective();
    let leader = km.leader.map_or_else(|| "—".to_string(), |c| c.display());
    let [head, list_a, legend] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Fill(1),
        Constraint::Length(5),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(format!(
            "OS:      {}\nKeymap:  {}\nLeader:  {}\nTheme:   {}",
            keymap::Keymaps::os_name(),
            ks.keymaps().active_name(),
            leader,
            ks.theme_name(),
        ))
        .style(theme.body())
        .wrap(Wrap { trim: true }),
        head,
    );

    // The live action → keys table; `keys_for` reflects any user remap.
    let items: Vec<Line> = keymap::Action::shown()
        .into_iter()
        .map(|a| {
            Line::from(vec![
                format!("{:<18}", a.help()).fg(theme.text),
                // The stable config id (Textual's binding id) — the key a
                // user puts in a config file to remap this action.
                format!("{:<16}", a.id()).fg(theme.dim),
                km.keys_for(a).fg(theme.accent),
            ])
        })
        .collect();
    frame.render_widget(
        List::new(items)
            .selected(Some(ks.drawer_sel()))
            .highlight_symbol("▶ ")
            .highlight_style(theme.selection())
            .style(theme.body()),
        list_a,
    );

    let legend_text = if let Some(act) = ks.rebind() {
        format!("● Press a key to bind\n  “{}”  (Esc cancels)", act.help())
    } else {
        "↑↓ select · r/⏎ rebind · x disable\nc next keymap · t theme · Esc close".to_string()
    };
    frame.render_widget(
        Paragraph::new(legend_text)
            .style(if ks.rebind().is_some() {
                theme.accent_text()
            } else {
                theme.caption()
            })
            .wrap(Wrap { trim: true }),
        legend,
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
