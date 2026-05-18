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
    Block, BorderType, CommandPalette, Drawer, DrawerSide, HelpEntry, HelpOverlay, KeymapRow,
    KeymapView, Modal, Paragraph, RowState, Sidebar, SidebarItem, StatusBar, Toast, ToastCorner,
    ToastMessage,
};

use crate::keymap;
use crate::screens::Screen;
use crate::{KitchenSink, Overlay, Pane};

/// Split a `keys_for` display string into [`KeymapView`] caps:
/// `"⌘K / :"` → `["⌘K", ":"]`; the unbound sentinel `"—"` → `[]` (so the
/// row reads disabled). The one adapter between `rstui-keymap` and the
/// engine-agnostic widget.
fn caps(keys: &str) -> Vec<String> {
    if keys == "—" {
        return Vec::new();
    }
    keys.split(" / ").map(str::to_owned).collect()
}

/// Raw screen-level keys the keymap deliberately doesn't own (they fall
/// through to the focused screen) — appended to help for completeness.
const RAW_HELP: &[(&str, &str)] = &[
    ("↑ ↓ ← →", "navigate / adjust"),
    ("Enter Space", "activate / toggle"),
    ("PgUp PgDn", "page / scroll"),
    ("Ctrl+T", "theme picker (browse + preview live)"),
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

    let right = format!(
        "{} · {} · ◴ {:04}",
        ks.fps_label(),
        theme.mode.label(),
        ks.tick() % 10_000
    );
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
        Overlay::ThemePicker => view_theme_picker(ks, frame, area),
        Overlay::QuitConfirm => view_quit_confirm(ks, frame, area),
        Overlay::DevTools => view_devtools(ks, frame, area),
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
    // The universal gateway, listed right where a lost user looks: from
    // this help overlay, `k` opens the keymap editor (same key in every
    // app). Shown last so it reads as the call-to-action.
    rows.push(("k".to_string(), "Customise these keybindings ↵"));
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

    // The keymap manager is now the shared `KeymapView` widget (the same
    // one git-review and acp-client use) — a pure projection of the *live*
    // keymap, so a switch or a user remap is reflected immediately. The
    // reducer still owns `drawer_sel` and the capture FSM (`rebind`); the
    // widget only draws state and (when wired) reports the clicked row.
    let km = ks.keymaps().effective();
    let leader = km.leader.map_or_else(|| "—".to_string(), |c| c.display());
    let sel = ks.drawer_sel();
    let rows: Vec<KeymapRow> = keymap::Action::shown()
        .into_iter()
        .enumerate()
        .map(|(i, a)| {
            let keys = km.keys_for(a);
            let state = if ks.rebind() == Some(a) {
                RowState::Capturing
            } else if i == sel {
                RowState::Selected
            } else if keys == "—" {
                RowState::Disabled
            } else {
                RowState::Normal
            };
            KeymapRow::new(a.help(), caps(&keys))
                .id(a.id())
                .state(state)
        })
        .collect();
    let header = format!(
        " {} · {} · leader {} · {}",
        ks.keymaps().active_name(),
        keymap::Keymaps::os_name(),
        leader,
        ks.theme_name(),
    );
    let footer = if let Some(act) = ks.rebind() {
        format!("● press a key to bind “{}” — Esc cancels", act.help())
    } else {
        "↑↓ select · r/⏎ rebind · x disable · c keymap · t theme · Esc close".to_string()
    };
    frame.render_widget(
        KeymapView::new(&rows)
            .header(Line::from(header).style(theme.heading()))
            .footer(Line::from(footer).style(if ks.rebind().is_some() {
                theme.accent_text()
            } else {
                theme.caption()
            }))
            .separator("")
            .style(theme.body())
            .label_style(Style::new().fg(theme.text))
            .id_style(Style::new().fg(theme.dim))
            .key_style(Style::new().fg(theme.accent))
            .selected_style(theme.selection())
            .capturing_style(theme.accent_text())
            .disabled_style(Style::new().fg(theme.dim)),
        inner,
    );
}

/// The theme picker — the reusable [`rstui_theme::ThemePicker`] in a centred
/// [`Modal`]. The whole app is already painted in the highlighted theme
/// (live preview), so the modal frame previews it too.
fn view_theme_picker(ks: &KitchenSink, frame: &mut Frame<'_>, area: Rect) {
    let theme = ks.theme();
    let modal = Modal::new()
        .width(Constraint::Percentage(60))
        .height(Constraint::Percentage(70))
        .block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .title(Line::from(" Theme picker ").style(theme.heading())),
        )
        .style(theme.body())
        .backdrop_style(Style::new().fg(theme.dim));
    let inner = modal.inner(area);
    frame.render_widget(modal, area);
    frame.render_widget(
        rstui_theme::ThemePicker::new(ks.theme_picker())
            .title("Browse · preview live")
            .style(theme.body())
            .highlight_style(theme.selection()),
        inner,
    );
}

/// The quit-confirmation dialog — a real centred [`Modal`].
/// The opt-in DevTools performance overlay (`F12`) — a pure projection of
/// the app's caller-owned [`rstui_devtools::PerfMeter`], framed like the
/// other overlays. The meter is fed by the
/// [`DevToolsAdapter`](rstui_devtools::DevToolsAdapter) the live loop
/// installs (see `main`); here `view` only *reads* it (ADR 0018, ADR 0012
/// §P1).
fn view_devtools(ks: &KitchenSink, frame: &mut Frame<'_>, area: Rect) {
    let theme = ks.theme();
    frame.render_widget(
        rstui_devtools::DevTools::new(ks.perf())
            .tab(ks.devtools_tab())
            .block(Block::bordered().border_type(BorderType::Rounded).title(
                Line::from(" DevTools · Tab/1–4 switch · F12/Esc close ").style(theme.heading()),
            ))
            .style(theme.body()),
        area,
    );
}

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
