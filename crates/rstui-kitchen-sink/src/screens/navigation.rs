//! The Navigation browser: a [`Tabs`] strip over five sub-views — a
//! selectable [`List`], a [`Table`], an expandable [`Tree`], a [`Menu`], and
//! a [`Select`] beside an inline [`Sidebar`] — with a live [`Breadcrumb`],
//! [`Pagination`], and [`Stepper`] band underneath. Every collection's
//! selection/offset is plain caller-owned state the widgets only read.

use rstui_core::{Constraint, KeyCode, Layout, Line, Position, Rect, Style, stylize::Stylize};
use rstui_runtime::Frame;
use rstui_widgets::{
    Block, BorderType, Breadcrumb, List, Menu, MenuItem, Pagination, Row, Select, Sidebar,
    SidebarItem, Step, Stepper, Table, Tabs, ToastLevel, Tree, TreeItem,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The sub-view tabs, in order.
const TABS: [&str; 5] = ["List", "Table", "Tree", "Menu", "Select"];

/// The list data (also paged by the pagination control).
const FRUIT: [&str; 12] = [
    "Apricot",
    "Blueberry",
    "Cherry",
    "Date",
    "Elderberry",
    "Fig",
    "Grape",
    "Honeydew",
    "Kiwi",
    "Lemon",
    "Mango",
    "Nectarine",
];

/// The table rows.
const PEOPLE: [(&str, &str, &str); 5] = [
    ("Ada", "Lovelace", "Analyst"),
    ("Alan", "Turing", "Cryptographer"),
    ("Grace", "Hopper", "Admiral"),
    ("Katherine", "Johnson", "Mathematician"),
    ("Linus", "Torvalds", "Maintainer"),
];

/// One file-tree node: indent depth, label, and whether it is a directory.
struct Node {
    depth: u16,
    label: &'static str,
    dir: bool,
}

/// The static file tree; directory rows expand/collapse.
const TREE: [Node; 8] = [
    Node {
        depth: 0,
        label: "src",
        dir: true,
    },
    Node {
        depth: 1,
        label: "main.rs",
        dir: false,
    },
    Node {
        depth: 1,
        label: "lib.rs",
        dir: false,
    },
    Node {
        depth: 0,
        label: "docs",
        dir: true,
    },
    Node {
        depth: 1,
        label: "adr",
        dir: true,
    },
    Node {
        depth: 2,
        label: "0001.md",
        dir: false,
    },
    Node {
        depth: 1,
        label: "README.md",
        dir: false,
    },
    Node {
        depth: 0,
        label: "Cargo.toml",
        dir: false,
    },
];

/// The menu items (with a separator and a disabled entry).
const MENU: [(&str, &str, bool, bool); 7] = [
    ("New", "Ctrl+N", false, false),
    ("Open", "Ctrl+O", false, false),
    ("Save", "Ctrl+S", false, false),
    ("", "", true, false),         // separator
    ("Rename", "F2", false, true), // disabled
    ("Export", "Ctrl+E", false, false),
    ("Quit", "Ctrl+Q", false, false),
];

/// The select / wizard options (also the stepper steps).
const PLANS: [&str; 4] = ["Account", "Profile", "Billing", "Review"];

/// All ten widgets' caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    tab: usize,
    list_sel: usize,
    page: usize,
    table_sel: usize,
    tree_sel: usize,
    /// One expand flag per directory node, indexed by node position.
    expanded: [bool; TREE.len()],
    menu_hi: usize,
    select_open: bool,
    select_sel: usize,
    select_hi: usize,
}

impl State {
    /// First tab, first row of everything, directories collapsed.
    pub(crate) fn new() -> Self {
        let mut expanded = [false; TREE.len()];
        expanded[0] = true; // `src` open so there is something to see
        Self {
            tab: 0,
            list_sel: 0,
            page: 0,
            table_sel: 0,
            tree_sel: 0,
            expanded,
            menu_hi: 0,
            select_open: false,
            select_sel: 0,
            select_hi: 0,
        }
    }

    /// Indices of the currently visible tree rows (a parent collapsed hides
    /// its whole subtree). Reducer logic — the [`Tree`] widget only renders
    /// the flattened result.
    fn visible_tree(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut hide_above: Option<u16> = None;
        for (i, n) in TREE.iter().enumerate() {
            if let Some(d) = hide_above {
                if n.depth > d {
                    continue;
                }
                hide_above = None;
            }
            out.push(i);
            if n.dir && !self.expanded[i] {
                hide_above = Some(n.depth);
            }
        }
        out
    }

    /// How many list pages at six rows each.
    fn page_count() -> usize {
        FRUIT.len().div_ceil(6)
    }

    /// Route a key to the active sub-view.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Left => {
                if self.tab == 0 {
                    return ScreenOutcome::ignored();
                }
                self.tab -= 1;
            }
            KeyCode::Right => self.tab = (self.tab + 1).min(TABS.len() - 1),
            KeyCode::PageUp => self.page = self.page.saturating_sub(1),
            KeyCode::PageDown => self.page = (self.page + 1).min(Self::page_count() - 1),
            KeyCode::Up => self.step(-1),
            KeyCode::Down => self.step(1),
            KeyCode::Enter | KeyCode::Char(' ') => return self.activate(),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Move the active sub-view's selection by `delta`.
    fn step(&mut self, delta: isize) {
        let mv = |cur: usize, len: usize| -> usize {
            if len == 0 {
                return 0;
            }
            (cur as isize + delta).rem_euclid(len as isize) as usize
        };
        match self.tab {
            0 => {
                self.list_sel = mv(self.list_sel, FRUIT.len());
                self.page = self.list_sel / 6;
            }
            1 => self.table_sel = mv(self.table_sel, PEOPLE.len()),
            2 => self.tree_sel = mv(self.tree_sel, self.visible_tree().len()),
            3 => {
                // Skip separators/disabled — reducer's job, not the widget's.
                let len = MENU.len();
                for _ in 0..len {
                    self.menu_hi = mv(self.menu_hi, len);
                    let (_, _, sep, dis) = MENU[self.menu_hi];
                    if !sep && !dis {
                        break;
                    }
                }
            }
            _ => {
                if self.select_open {
                    self.select_hi = mv(self.select_hi, PLANS.len());
                } else {
                    self.select_sel = mv(self.select_sel, PLANS.len());
                }
            }
        }
    }

    /// Enter / Space on the active sub-view.
    fn activate(&mut self) -> ScreenOutcome {
        match self.tab {
            0 => ScreenOutcome::with_toast(
                ToastLevel::Info,
                format!("Picked {}", FRUIT[self.list_sel]),
            ),
            1 => {
                let p = PEOPLE[self.table_sel];
                ScreenOutcome::with_toast(ToastLevel::Info, format!("Row: {} {}", p.0, p.1))
            }
            2 => {
                let node = self.visible_tree()[self.tree_sel];
                if TREE[node].dir {
                    self.expanded[node] = !self.expanded[node];
                    ScreenOutcome::consumed()
                } else {
                    ScreenOutcome::with_toast(
                        ToastLevel::Info,
                        format!("Open {}", TREE[node].label),
                    )
                }
            }
            3 => ScreenOutcome::with_toast(
                ToastLevel::Success,
                format!("Menu: {}", MENU[self.menu_hi].0),
            ),
            _ => {
                if self.select_open {
                    self.select_sel = self.select_hi;
                    self.select_open = false;
                    ScreenOutcome::with_toast(
                        ToastLevel::Success,
                        format!("Step → {}", PLANS[self.select_sel]),
                    )
                } else {
                    self.select_open = true;
                    self.select_hi = self.select_sel;
                    ScreenOutcome::consumed()
                }
            }
        }
    }

    /// A click on the tab strip switches sub-view; a click on a list row
    /// selects it.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [tabs, body, _crumb, _band] = Self::rows(content);
        if let Some(i) = crate::screens::tab_index_at(tabs, &TABS, 2, pos) {
            self.tab = i;
            return ScreenOutcome::consumed();
        }
        if self.tab == 0 && body.contains(pos) {
            // List rows start one row inside the framing block.
            let row = pos.y.saturating_sub(body.y + 1) as usize;
            let base = self.page * 6;
            if base + row < FRUIT.len() && row < 6 {
                self.list_sel = base + row;
                return ScreenOutcome::with_toast(
                    ToastLevel::Info,
                    format!("Picked {}", FRUIT[self.list_sel]),
                );
            }
        }
        ScreenOutcome::ignored()
    }

    /// Wheel scroll moves the active selection.
    pub(crate) fn on_scroll(&mut self, up: bool) {
        self.step(if up { -1 } else { 1 });
    }

    /// The four stacked bands the renderer and hit-test share.
    fn rows(area: Rect) -> [Rect; 4] {
        Layout::vertical([
            Constraint::Length(1), // tabs
            Constraint::Fill(1),   // active sub-view
            Constraint::Length(1), // breadcrumb
            Constraint::Length(2), // pagination + stepper
        ])
        .areas(area)
    }

    /// Draw the navigation browser.
    pub(crate) fn view(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let [tabs, body, crumb, band] = Self::rows(area);

        frame.render_widget(
            Tabs::new(TABS)
                .selected(Some(self.tab))
                .divider("  ")
                .style(theme.body())
                .highlight_style(theme.selection()),
            tabs,
        );

        match self.tab {
            0 => self.view_list(theme, frame, body),
            1 => self.view_table(theme, frame, body),
            2 => self.view_tree(theme, frame, body),
            3 => self.view_menu(theme, frame, body),
            _ => self.view_select(theme, frame, body),
        }

        // Live breadcrumb of where we are.
        let trail = [
            Line::from("nav".fg(theme.dim)),
            Line::from(TABS[self.tab].fg(theme.dim)),
            Line::from(self.crumb_leaf().fg(theme.text)),
        ];
        frame.render_widget(
            Breadcrumb::new(&trail)
                .separator('›')
                .style(theme.caption())
                .emphasis_style(theme.accent_text()),
            crumb,
        );

        // Pagination (left) + Stepper (right) reflect the live state.
        let [pag, step] =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Fill(1)]).areas(band);
        frame.render_widget(
            Pagination::new(self.page, Self::page_count())
                .style(theme.caption())
                .current_style(theme.selection()),
            Rect::new(pag.x, pag.y, pag.width, 1),
        );
        let steps: Vec<Step> = PLANS.iter().map(|s| Step::new(*s)).collect();
        frame.render_widget(
            Stepper::new(steps)
                .current(self.select_sel)
                .style(theme.caption())
                .current_style(theme.accent_text())
                .done_style(Style::new().fg(theme.ok)),
            Rect::new(step.x, step.y, step.width, 2),
        );
    }

    /// The breadcrumb leaf for the active sub-view.
    fn crumb_leaf(&self) -> String {
        match self.tab {
            0 => FRUIT[self.list_sel].to_string(),
            1 => PEOPLE[self.table_sel].0.to_string(),
            2 => TREE[self.visible_tree()[self.tree_sel]].label.to_string(),
            3 => MENU[self.menu_hi].0.to_string(),
            _ => PLANS[self.select_sel].to_string(),
        }
    }

    fn view_list(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let base = self.page * 6;
        let rows: Vec<&str> = FRUIT.iter().skip(base).take(6).copied().collect();
        frame.render_widget(
            List::new(rows)
                .selected(self.list_sel.checked_sub(base))
                .highlight_symbol("▶ ")
                .highlight_style(theme.selection())
                .style(theme.body())
                .block(framed(theme, "fruit · ↑↓ select · PgUp/PgDn page")),
            area,
        );
    }

    fn view_table(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let rows = PEOPLE.iter().map(|(f, l, r)| Row::new([*f, *l, *r]));
        frame.render_widget(
            Table::new(
                rows,
                [
                    Constraint::Length(12),
                    Constraint::Length(12),
                    Constraint::Fill(1),
                ],
            )
            .header(Row::new(["First", "Last", "Role"]).style(theme.accent_text()))
            .selected(Some(self.table_sel))
            .highlight_symbol("▶ ")
            .highlight_style(theme.selection())
            .style(theme.body())
            .block(framed(theme, "people · ↑↓ select")),
            area,
        );
    }

    fn view_tree(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let visible = self.visible_tree();
        let items: Vec<TreeItem> = visible
            .iter()
            .map(|&i| {
                let n = &TREE[i];
                let label = if n.dir {
                    format!("{} {}", if self.expanded[i] { '▾' } else { '▸' }, n.label)
                } else {
                    n.label.to_string()
                };
                TreeItem::new(n.depth, label).expandable(n.dir && self.expanded[i])
            })
            .collect();
        frame.render_widget(
            Tree::new(items)
                .selected(Some(self.tree_sel))
                .highlight_symbol("▶ ")
                .highlight_style(theme.selection())
                .style(theme.body())
                .block(framed(theme, "files · Enter expand/open")),
            area,
        );
    }

    fn view_menu(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let items: Vec<MenuItem> = MENU
            .iter()
            .map(|(label, hint, sep, dis)| {
                if *sep {
                    MenuItem::separator()
                } else {
                    MenuItem::new(*label).key_hint(*hint).disabled(*dis)
                }
            })
            .collect();
        frame.render_widget(
            Menu::new(&items)
                .highlight(self.menu_hi)
                .style(theme.body())
                .highlight_style(theme.selection())
                .disabled_style(Style::new().fg(theme.dim))
                .block(framed(theme, "actions · ↑↓ skips disabled")),
            area,
        );
    }

    fn view_select(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Fill(1)]).areas(area);
        let [label, field, _] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(left);
        frame.render_widget(
            Line::from("Wizard step (Enter opens, ↑↓ choose)".fg(theme.dim)),
            label,
        );
        frame.render_widget(
            Select::new(PLANS)
                .selected(Some(self.select_sel))
                .highlight(self.select_hi)
                .open(self.select_open)
                .focused(true)
                .style(theme.body())
                .focus_style(theme.focus_field())
                .highlight_style(theme.selection())
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .border_style(theme.border()),
                )
                .open_height(4),
            field,
        );

        let nav_items: Vec<SidebarItem> = [
            SidebarItem::group("Workspace"),
            SidebarItem::new("Dashboard").icon('▤'),
            SidebarItem::new("Sources").icon('⎘'),
            SidebarItem::group("Account"),
            SidebarItem::new("Profile").icon('☺'),
            SidebarItem::new("Billing").icon('▣'),
        ]
        .into_iter()
        .collect();
        frame.render_widget(
            Sidebar::new(&nav_items)
                .selected(Some(self.select_sel + 1))
                .style(theme.body())
                .highlight_style(theme.selection())
                .group_style(theme.caption())
                .block(framed(theme, "inline Sidebar")),
            right,
        );
    }
}

/// A rounded framing block with a caption title in the theme's accent. The
/// title is copied into the block, so the result owns its text (`'static`).
fn framed(theme: &Theme, title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {title} ")).style(theme.caption()))
        .border_style(theme.border())
        .style(theme.body())
}
