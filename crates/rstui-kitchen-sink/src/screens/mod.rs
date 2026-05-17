//! The content screens and the dispatch that routes input + rendering to
//! whichever one the sidebar has selected.
//!
//! Two sections — the nine **Widgets** screens (a guided tour of the catalog) and
//! the **Experiences** screens — composed scenes that look and behave like
//! real apps (a chat client, mail, a file explorer, a dashboard, a music
//! player, an IDE, settings, a login, a Kanban board, a live log tail).
//!
//! Every screen owns its interactive state as a plain struct on
//! [`ScreenState`]; the active screen's handler is the only thing that
//! mutates it, exactly as the top-level [`App`](rstui_runtime::App) reducer
//! does for the shell.

pub(crate) mod analytics;
pub(crate) mod board;
pub(crate) mod chat;
pub(crate) mod colour_lab;
pub(crate) mod containers;
pub(crate) mod dashboard;
pub(crate) mod data_grid;
pub(crate) mod data_views;
pub(crate) mod feedback;
pub(crate) mod files_app;
pub(crate) mod forms;
pub(crate) mod ide;
pub(crate) mod login;
pub(crate) mod logs;
pub(crate) mod mail;
pub(crate) mod metrics;
pub(crate) mod navigation;
pub(crate) mod observability;
pub(crate) mod player;
pub(crate) mod rich_text;
pub(crate) mod settings_app;
pub(crate) mod traces;
pub(crate) mod welcome;

use rstui_core::{KeyCode, Margin, Position, Rect, TextArea, TextEdit};
use rstui_runtime::Frame;
use rstui_widgets::ToastLevel;

use crate::theme::Theme;

/// Cut `sel` out of a single-line [`TextEdit`] (the focused field): delete
/// its first occurrence and put the caret where it was. `false` if the
/// field does not contain it (nothing to cut — the caller still copied).
pub(crate) fn cut_field(te: &mut TextEdit, sel: &str) -> bool {
    let value = te.value();
    let Some(byte) = value.find(sel) else {
        return false;
    };
    let caret = value[..byte].chars().count();
    let mut next = value.to_string();
    next.replace_range(byte..byte + sel.len(), "");
    te.set_value(next);
    te.set_cursor(caret);
    true
}

/// Cut `sel` out of a multi-line [`TextArea`] (an editor buffer): delete its
/// first occurrence and re-seat the caret at the cut point.
pub(crate) fn cut_area(ta: &mut TextArea, sel: &str) -> bool {
    let value = ta.lines().join("\n");
    let Some(byte) = value.find(sel) else {
        return false;
    };
    let before = &value[..byte];
    let row = before.matches('\n').count();
    let col = before.rsplit('\n').next().unwrap_or("").chars().count();
    let mut next = value.clone();
    next.replace_range(byte..byte + sel.len(), "");
    ta.set_value(next);
    ta.set_cursor(row, col);
    true
}

/// The text-bearing rect inside a rounded framing block (every panel here is
/// `Block::bordered()` with no padding, so its inner is a one-cell margin).
/// Used by `selection_region` so a drag stays inside the *container*'s text,
/// never its border or a neighbouring panel.
pub(crate) fn block_inner(r: Rect) -> Rect {
    r.inner(Margin::new(1, 1))
}

/// The tab a click landed on in a one-row [`Tabs`](rstui_widgets::Tabs)
/// strip, or `None` if it missed every tab.
///
/// [`Tabs`](rstui_widgets::Tabs) has no per-tab geometry accessor, so this
/// replicates its render layout exactly: titles run left→right, each padded
/// with one space on **each** side, separated by a `divider_w`-wide divider
/// between (not before the first). Equal-width division — the bug this
/// replaces — is wrong because titles vary in width.
pub(crate) fn tab_index_at(
    strip: Rect,
    titles: &[&str],
    divider_w: u16,
    pos: Position,
) -> Option<usize> {
    if pos.y != strip.y || pos.x < strip.x {
        return None;
    }
    let mut x = strip.x;
    for (i, title) in titles.iter().enumerate() {
        if i > 0 {
            x = x.saturating_add(divider_w);
        }
        let cell_w = title.chars().count() as u16 + 2; // one pad each side
        if pos.x >= x && pos.x < x.saturating_add(cell_w) {
            return Some(i);
        }
        x = x.saturating_add(cell_w);
        if x >= strip.right() {
            break;
        }
    }
    None
}

/// A side effect a screen handler can ask the shell to perform: a toast.
pub(crate) type Toastlet = (ToastLevel, String);

/// What a screen handler reports back to the shell reducer.
#[derive(Debug, Default)]
pub(crate) struct ScreenOutcome {
    /// Whether the screen consumed the key (so the shell does not also act,
    /// e.g. `Left` falling back to the navigation rail).
    pub(crate) handled: bool,
    /// An optional toast to raise (a button submit, a copy, …).
    pub(crate) toast: Option<Toastlet>,
}

impl ScreenOutcome {
    /// The screen consumed the input and wants nothing else.
    pub(crate) fn consumed() -> Self {
        Self {
            handled: true,
            toast: None,
        }
    }

    /// The screen ignored the input (the shell may act on it).
    pub(crate) fn ignored() -> Self {
        Self {
            handled: false,
            toast: None,
        }
    }

    /// The screen consumed the input and wants this toast raised.
    pub(crate) fn with_toast(level: ToastLevel, body: impl Into<String>) -> Self {
        Self {
            handled: true,
            toast: Some((level, body.into())),
        }
    }
}

/// Every screen, in sidebar order: the nine Widgets screens, the ten
/// Experiences screens, the three Observability screens, then the analytical
/// chart catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    /// Overview, quickstart, and the global keymap.
    Welcome,
    /// Editable inputs, toggles, sliders, buttons, a form.
    Forms,
    /// Lists, tables, trees, menus, tabs, pagination, steppers.
    Navigation,
    /// Charts, gauges, calendars, diffs, description lists, accordions.
    Data,
    /// Alerts, badges, toasts, spinners, skeletons, tooltips, popovers.
    Feedback,
    /// Blocks, cards, grids, split panes, dividers, alignment, scrolling.
    Containers,
    /// Paragraph, Markdown, Mermaid, links, the styled-text model.
    RichText,
    /// Full-colour lab: ANSI, 256-indexed, RGB truecolor, modifiers.
    Colour,
    /// The comprehensive `DataTable`: sort, filter, group, fast scroll,
    /// mouse hit-testing, and in-cell editing.
    DataGrid,
    /// A chat / messenger client: channels, a bubble thread, a composer.
    Chat,
    /// A three-pane email client.
    Mail,
    /// A file explorer: tree, listing, preview, breadcrumb.
    Files,
    /// An analytics dashboard: KPI cards, charts, an activity feed.
    Dashboard,
    /// A music player: playlist, now-playing, seek, transport.
    Player,
    /// A code editor: file tabs, a line-numbered buffer, a problems pane.
    Ide,
    /// A settings / preferences screen.
    Settings,
    /// A sign-in screen with live validation.
    Login,
    /// A Kanban board: cards moved across columns.
    Board,
    /// A live, streaming log / terminal tail.
    Logs,
    /// An OpenTelemetry service overview: golden-signal stats, a
    /// throughput/error chart, a service-health heatmap, an error stream.
    Observability,
    /// A metrics explorer: a multi-series latency chart, a distribution
    /// histogram with percentile markers, a latency heatmap.
    Metrics,
    /// A distributed-trace explorer: a span waterfall / flame graph and a
    /// selected-span attribute table.
    Traces,
    /// The analytical chart catalog: scatter, radar, box plot, candlestick,
    /// treemap and Sankey — the exploratory (non-dashboard) chart types.
    Analytics,
}

/// One visual row of the navigation rail: a section header or a screen entry.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SidebarRow {
    /// A non-selectable section header with this label.
    Group(&'static str),
    /// A selectable screen entry; the index is into [`Screen::ALL`].
    Item(usize),
}

impl Screen {
    /// Every screen in fixed display order. The sidebar, the hotkeys, and the
    /// command palette all index this, so they cannot disagree.
    pub(crate) const ALL: [Screen; 23] = [
        Screen::Welcome,
        Screen::Forms,
        Screen::Navigation,
        Screen::Data,
        Screen::Feedback,
        Screen::Containers,
        Screen::RichText,
        Screen::Colour,
        Screen::DataGrid,
        Screen::Chat,
        Screen::Mail,
        Screen::Files,
        Screen::Dashboard,
        Screen::Player,
        Screen::Ide,
        Screen::Settings,
        Screen::Login,
        Screen::Board,
        Screen::Logs,
        Screen::Observability,
        Screen::Metrics,
        Screen::Traces,
        Screen::Analytics,
    ];

    /// This screen's index into [`ALL`](Self::ALL).
    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|&s| s == self).unwrap_or(0)
    }

    /// Screens whose primary affordance is a text field, so the shell hands
    /// them every character key (even `q` / `:` / digits) instead of eating
    /// it as a global chord — the composer / editor / filter must be typeable.
    pub(crate) fn is_text_entry(self) -> bool {
        matches!(
            self,
            Screen::Chat | Screen::Ide | Screen::Login | Screen::Logs | Screen::DataGrid
        )
    }

    /// Which sidebar section this screen belongs to.
    pub(crate) fn group(self) -> &'static str {
        if self.index() < 9 {
            "WIDGETS"
        } else if self.index() < 19 {
            "EXPERIENCES"
        } else if self.index() < 22 {
            "OBSERVABILITY"
        } else {
            "CHART CATALOG"
        }
    }

    /// The sidebar label.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Screen::Welcome => "Welcome",
            Screen::Forms => "Forms & Input",
            Screen::Navigation => "Navigation",
            Screen::Data => "Data Display",
            Screen::Feedback => "Feedback",
            Screen::Containers => "Containers",
            Screen::RichText => "Rich Text",
            Screen::Colour => "Colour Lab",
            Screen::DataGrid => "Data Grid",
            Screen::Chat => "Chat",
            Screen::Mail => "Mail",
            Screen::Files => "Files",
            Screen::Dashboard => "Dashboard",
            Screen::Player => "Music Player",
            Screen::Ide => "Code Editor",
            Screen::Settings => "Settings",
            Screen::Login => "Sign In",
            Screen::Board => "Kanban Board",
            Screen::Logs => "Live Logs",
            Screen::Observability => "Observability",
            Screen::Metrics => "Metrics",
            Screen::Traces => "Traces",
            Screen::Analytics => "Analytics",
        }
    }

    /// A one-glyph icon for the sidebar.
    pub(crate) fn icon(self) -> char {
        match self {
            Screen::Welcome => '★',
            Screen::Forms => '✎',
            Screen::Navigation => '☰',
            Screen::Data => '▤',
            Screen::Feedback => '◈',
            Screen::Containers => '▦',
            Screen::RichText => '¶',
            Screen::Colour => '✸',
            Screen::DataGrid => '⊞',
            Screen::Chat => '✉',
            Screen::Mail => '@',
            Screen::Files => '▣',
            Screen::Dashboard => '◫',
            Screen::Player => '►',
            Screen::Ide => '⌗',
            Screen::Settings => '⚙',
            Screen::Login => '⚿',
            Screen::Board => '▥',
            Screen::Logs => '☷',
            Screen::Observability => '◉',
            Screen::Metrics => '∿',
            Screen::Traces => '⋔',
            Screen::Analytics => '◔',
        }
    }

    /// The one-line title shown in the header.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Screen::Welcome => "Welcome to the rstui kitchen sink",
            Screen::Forms => "Forms & Input — editable, focusable controls",
            Screen::Navigation => "Navigation — lists, tables, trees, tabs",
            Screen::Data => "Data Display — charts, calendars, diffs",
            Screen::Feedback => "Feedback — alerts, toasts, spinners",
            Screen::Containers => "Containers — blocks, grids, scrolling",
            Screen::RichText => "Rich Text — Markdown, Mermaid, styled spans",
            Screen::Colour => "Colour Lab — ANSI · 256 · truecolor",
            Screen::DataGrid => "Data Grid — sort · filter · group · scroll · edit",
            Screen::Chat => "Chat — channels, threads, a live composer",
            Screen::Mail => "Mail — a three-pane email client",
            Screen::Files => "Files — explorer with tree, list & preview",
            Screen::Dashboard => "Dashboard — KPIs, charts, activity",
            Screen::Player => "Music Player — playlist, seek, transport",
            Screen::Ide => "Code Editor — tabs, buffer, problems",
            Screen::Settings => "Settings — categories & live preferences",
            Screen::Login => "Sign In — inputs with live validation",
            Screen::Board => "Kanban — move cards across columns",
            Screen::Logs => "Live Logs — a streaming, filtered tail",
            Screen::Observability => "Observability — OTel golden signals, charts, errors",
            Screen::Metrics => "Metrics — latency series, distribution, heatmap",
            Screen::Traces => "Traces — span waterfall, flame graph, attributes",
            Screen::Analytics => {
                "Analytics — scatter · radar · box · candlestick · treemap · Sankey"
            }
        }
    }

    /// The rail rows: a `Group` header wherever the section changes, then an
    /// `Item` per screen. The renderer and the click hit-test both build
    /// from this, so what is drawn and what a click selects cannot drift.
    pub(crate) fn sidebar_rows() -> Vec<SidebarRow> {
        let mut rows = Vec::new();
        let mut section = "";
        for (i, s) in Self::ALL.iter().enumerate() {
            if s.group() != section {
                section = s.group();
                rows.push(SidebarRow::Group(section));
            }
            rows.push(SidebarRow::Item(i));
        }
        rows
    }

    /// The visual rail row of the entry for screen index `nav`.
    pub(crate) fn sidebar_selected_row(nav: usize) -> usize {
        Self::sidebar_rows()
            .iter()
            .position(|r| matches!(r, SidebarRow::Item(i) if *i == nav))
            .unwrap_or(0)
    }

    /// First rail row to show so `nav`'s entry stays visible in `height`
    /// item rows (the rail scrolls when there are more rows than space).
    pub(crate) fn sidebar_offset(nav: usize, height: u16) -> usize {
        let sel = Self::sidebar_selected_row(nav);
        let h = height.max(1) as usize;
        if sel >= h { sel - h + 1 } else { 0 }
    }

    /// The screen index for the rail row at visual position `row`
    /// (accounting for `offset`), or `None` if it is a section header.
    pub(crate) fn screen_at_row(row: usize, offset: usize) -> Option<usize> {
        match Self::sidebar_rows().get(row + offset)? {
            SidebarRow::Item(i) => Some(*i),
            SidebarRow::Group(_) => None,
        }
    }
}

/// Every screen's interactive state, plus the dispatch to it.
#[derive(Debug)]
pub(crate) struct ScreenState {
    pub(crate) forms: forms::State,
    pub(crate) navigation: navigation::State,
    pub(crate) data: data_views::State,
    pub(crate) feedback: feedback::State,
    pub(crate) containers: containers::State,
    pub(crate) rich_text: rich_text::State,
    pub(crate) colour: colour_lab::State,
    pub(crate) data_grid: data_grid::State,
    pub(crate) chat: chat::State,
    pub(crate) mail: mail::State,
    pub(crate) files: files_app::State,
    pub(crate) dashboard: dashboard::State,
    pub(crate) player: player::State,
    pub(crate) ide: ide::State,
    pub(crate) settings: settings_app::State,
    pub(crate) login: login::State,
    pub(crate) board: board::State,
    pub(crate) logs: logs::State,
    pub(crate) observability: observability::State,
    pub(crate) metrics: metrics::State,
    pub(crate) traces: traces::State,
    pub(crate) analytics: analytics::State,
}

impl ScreenState {
    /// Fresh state for every screen.
    pub(crate) fn new() -> Self {
        Self {
            forms: forms::State::new(),
            navigation: navigation::State::new(),
            data: data_views::State::new(),
            feedback: feedback::State::new(),
            containers: containers::State::new(),
            rich_text: rich_text::State::new(),
            colour: colour_lab::State::new(),
            data_grid: data_grid::State::new(),
            chat: chat::State::new(),
            mail: mail::State::new(),
            files: files_app::State::new(),
            dashboard: dashboard::State::new(),
            player: player::State::new(),
            ide: ide::State::new(),
            settings: settings_app::State::new(),
            login: login::State::new(),
            board: board::State::new(),
            logs: logs::State::new(),
            observability: observability::State::new(),
            metrics: metrics::State::new(),
            traces: traces::State::new(),
            analytics: analytics::State::new(),
        }
    }

    /// Route a key to the active screen.
    pub(crate) fn on_key(&mut self, screen: Screen, code: KeyCode, tick: u64) -> ScreenOutcome {
        match screen {
            Screen::Welcome => welcome::on_key(code),
            Screen::Forms => self.forms.on_key(code),
            Screen::Navigation => self.navigation.on_key(code),
            Screen::Data => self.data.on_key(code),
            Screen::Feedback => self.feedback.on_key(code, tick),
            Screen::Containers => self.containers.on_key(code),
            Screen::RichText => self.rich_text.on_key(code),
            Screen::Colour => self.colour.on_key(code),
            Screen::DataGrid => self.data_grid.on_key(code),
            Screen::Chat => self.chat.on_key(code),
            Screen::Mail => self.mail.on_key(code),
            Screen::Files => self.files.on_key(code),
            Screen::Dashboard => self.dashboard.on_key(code),
            Screen::Player => self.player.on_key(code),
            Screen::Ide => self.ide.on_key(code),
            Screen::Settings => self.settings.on_key(code),
            Screen::Login => self.login.on_key(code),
            Screen::Board => self.board.on_key(code),
            Screen::Logs => self.logs.on_key(code),
            Screen::Observability => self.observability.on_key(code),
            Screen::Metrics => self.metrics.on_key(code),
            Screen::Traces => self.traces.on_key(code),
            Screen::Analytics => self.analytics.on_key(code),
        }
    }

    /// Route a click to the active screen. `content` is the screen's drawable
    /// rect, so handlers hit-test exactly what the user sees.
    pub(crate) fn on_click(
        &mut self,
        screen: Screen,
        pos: Position,
        content: Rect,
    ) -> ScreenOutcome {
        match screen {
            Screen::Welcome => welcome::on_click(pos, content),
            Screen::Forms => self.forms.on_click(pos, content),
            Screen::Navigation => self.navigation.on_click(pos, content),
            Screen::Colour => self.colour.on_click(pos, content),
            Screen::DataGrid => self.data_grid.on_click(pos, content),
            Screen::RichText => self.rich_text.on_click(pos, content),
            Screen::Chat => self.chat.on_click(pos, content),
            Screen::Mail => self.mail.on_click(pos, content),
            Screen::Board => self.board.on_click(pos, content),
            Screen::Data => self.data.on_click(pos, content),
            Screen::Dashboard => self.dashboard.on_click(pos, content),
            Screen::Player => self.player.on_click(pos, content),
            Screen::Ide => self.ide.on_click(pos, content),
            Screen::Settings => self.settings.on_click(pos, content),
            Screen::Login => self.login.on_click(pos, content),
            Screen::Files => self.files.on_click(pos, content),
            Screen::Feedback => self.feedback.on_click(pos, content),
            Screen::Observability => self.observability.on_click(pos, content),
            Screen::Traces => self.traces.on_click(pos, content),
            Screen::Analytics => self.analytics.on_click(pos, content),
            Screen::Containers | Screen::Logs | Screen::Metrics => ScreenOutcome::ignored(),
        }
    }

    /// Pointer pressed in the content. A screen returns *handled* to claim
    /// the whole press→drag→release gesture (the shell then routes
    /// [`on_pointer_drag`](Self::on_pointer_drag) /
    /// [`on_release`](Self::on_release) here instead of starting a text
    /// selection). Only the drag-aware screens act; the rest defer to the
    /// existing click/selection path by ignoring it.
    pub(crate) fn on_press(
        &mut self,
        screen: Screen,
        pos: Position,
        content: Rect,
    ) -> ScreenOutcome {
        match screen {
            Screen::Board => self.board.on_press(pos, content),
            _ => ScreenOutcome::ignored(),
        }
    }

    /// Pointer moved while a screen owns the gesture (see
    /// [`on_press`](Self::on_press)).
    pub(crate) fn on_pointer_drag(
        &mut self,
        screen: Screen,
        pos: Position,
        content: Rect,
    ) -> ScreenOutcome {
        match screen {
            Screen::Board => self.board.on_pointer_drag(pos, content),
            _ => ScreenOutcome::ignored(),
        }
    }

    /// Pointer released, ending a screen-owned gesture (see
    /// [`on_press`](Self::on_press)).
    pub(crate) fn on_release(
        &mut self,
        screen: Screen,
        pos: Position,
        content: Rect,
    ) -> ScreenOutcome {
        match screen {
            Screen::Board => self.board.on_release(pos, content),
            _ => ScreenOutcome::ignored(),
        }
    }

    /// Route a wheel scroll to the active screen.
    pub(crate) fn on_scroll(&mut self, screen: Screen, up: bool) {
        match screen {
            Screen::Navigation => self.navigation.on_scroll(up),
            Screen::DataGrid => self.data_grid.on_scroll(up),
            Screen::Containers => self.containers.on_scroll(up),
            Screen::RichText => self.rich_text.on_scroll(up),
            Screen::Data => self.data.on_scroll(up),
            Screen::Chat => self.chat.on_scroll(up),
            Screen::Mail => self.mail.on_scroll(up),
            Screen::Logs => self.logs.on_scroll(up),
            Screen::Ide => self.ide.on_scroll(up),
            Screen::Traces => self.traces.on_scroll(up),
            _ => {}
        }
    }

    /// The text container under `pos` for the active screen — the rect a
    /// drag-selection must stay inside (so it never crosses into a
    /// neighbouring panel or the chrome). `None` means "nothing selectable
    /// here" (the press is then a plain click). Screens that are a single
    /// text surface fall back to the whole `content`.
    pub(crate) fn selection_region(
        &self,
        screen: Screen,
        pos: Position,
        content: Rect,
    ) -> Option<Rect> {
        match screen {
            Screen::Welcome => welcome::selection_region(pos, content),
            Screen::RichText => self.rich_text.selection_region(pos, content),
            Screen::Data => self.data.selection_region(pos, content),
            Screen::Mail => self.mail.selection_region(pos, content),
            Screen::Logs => logs::selection_region(pos, content),
            Screen::Ide => self.ide.selection_region(pos, content),
            Screen::Chat => chat::selection_region(pos, content),
            Screen::Containers => containers::selection_region(pos, content),
            // Single-surface screens: confine to the whole content (still
            // never the sidebar/header/footer), not a sub-panel.
            _ => Some(content),
        }
    }

    /// Route a paste to the active screen (text-entry screens accept it).
    pub(crate) fn on_paste(&mut self, screen: Screen, text: &str) {
        match screen {
            Screen::Forms => self.forms.on_paste(text),
            Screen::Chat => self.chat.on_paste(text),
            Screen::Login => self.login.on_paste(text),
            Screen::Ide => self.ide.on_paste(text),
            Screen::Logs => self.logs.on_paste(text),
            _ => {}
        }
    }

    /// Whether a finished drag-selection on `screen` is auto-copied to the
    /// clipboard on release. Read-only renders auto-copy (the selection *is*
    /// the copy); editable screens do **not**, so the selection stays live
    /// for `Ctrl+C` (copy) or `Ctrl+X` (cut) — the per-container option.
    pub(crate) fn selection_auto_copy(&self, screen: Screen) -> bool {
        !matches!(
            screen,
            Screen::Forms | Screen::Chat | Screen::Login | Screen::Ide | Screen::Logs
        )
    }

    /// Cut: delete the selected text `sel` from the active screen's focused
    /// editable (the caller has already copied it). Returns `true` if the
    /// screen is editable and removed it — `false` means "copy only".
    pub(crate) fn cut(&mut self, screen: Screen, sel: &str) -> bool {
        match screen {
            Screen::Forms => self.forms.cut(sel),
            Screen::Chat => self.chat.cut(sel),
            Screen::Login => self.login.cut(sel),
            Screen::Ide => self.ide.cut(sel),
            Screen::Logs => self.logs.cut(sel),
            _ => false,
        }
    }

    /// Draw the active screen into `area`.
    pub(crate) fn view(
        &self,
        screen: Screen,
        theme: &Theme,
        tick: u64,
        frame: &mut Frame<'_>,
        area: Rect,
    ) {
        match screen {
            Screen::Welcome => welcome::view(theme, frame, area),
            Screen::Forms => self.forms.view(theme, frame, area),
            Screen::Navigation => self.navigation.view(theme, frame, area),
            Screen::Data => self.data.view(theme, tick, frame, area),
            Screen::Feedback => self.feedback.view(theme, tick, frame, area),
            Screen::Containers => self.containers.view(theme, frame, area),
            Screen::RichText => self.rich_text.view(theme, frame, area),
            Screen::Colour => self.colour.view(theme, frame, area),
            Screen::DataGrid => self.data_grid.view(theme, frame, area),
            Screen::Chat => self.chat.view(theme, tick, frame, area),
            Screen::Mail => self.mail.view(theme, tick, frame, area),
            Screen::Files => self.files.view(theme, tick, frame, area),
            Screen::Dashboard => self.dashboard.view(theme, tick, frame, area),
            Screen::Player => self.player.view(theme, tick, frame, area),
            Screen::Ide => self.ide.view(theme, tick, frame, area),
            Screen::Settings => self.settings.view(theme, tick, frame, area),
            Screen::Login => self.login.view(theme, tick, frame, area),
            Screen::Board => self.board.view(theme, tick, frame, area),
            Screen::Logs => self.logs.view(theme, tick, frame, area),
            Screen::Observability => self.observability.view(theme, tick, frame, area),
            Screen::Metrics => self.metrics.view(theme, tick, frame, area),
            Screen::Traces => self.traces.view(theme, tick, frame, area),
            Screen::Analytics => self.analytics.view(theme, tick, frame, area),
        }
    }
}
