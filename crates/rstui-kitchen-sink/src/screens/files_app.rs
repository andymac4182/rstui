//! A file-explorer experience: an expandable [`Tree`] of the project, a
//! live [`Breadcrumb`] of the selection's path, a preview pane
//! ([`Paragraph`] for files / [`DescriptionList`] for folders), and a
//! [`StatusBar`]. `↑/↓` select, `Enter` expands a folder or opens a file.

use rstui_core::{Constraint, KeyCode, Layout, Line, Margin, Position, Rect, Style};
use rstui_runtime::Frame;
use rstui_widgets::{
    Block, BorderType, Breadcrumb, DescriptionList, DescriptionRow, Paragraph, StatusBar, Tree,
    TreeItem, Wrap,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// One filesystem entry.
struct Node {
    depth: u16,
    name: &'static str,
    dir: bool,
    /// For files: a size label and a preview body.
    size: &'static str,
    body: &'static str,
}

const fn d(depth: u16, name: &'static str) -> Node {
    Node {
        depth,
        name,
        dir: true,
        size: "—",
        body: "",
    }
}
const fn f(depth: u16, name: &'static str, size: &'static str, body: &'static str) -> Node {
    Node {
        depth,
        name,
        dir: false,
        size,
        body,
    }
}

const TREE: [Node; 27] = [
    d(0, "crates"),
    d(1, "rstui-core"),
    d(2, "src"),
    f(
        3,
        "lib.rs",
        "6 KB",
        "//! rstui-core — the cell buffer, the geometry, and the three-level\n//! styled-text model every widget projects into. No widget, no runtime,\n//! no terminal backend lives here: this crate is pure data and pure math\n//! so that everything above it can stay a deterministic function of it.\n\n#![forbid(unsafe_code)]\n\nmod buffer;\nmod layout;\nmod style;\nmod text;\n\npub use buffer::{Buffer, Cell};\npub use layout::{Constraint, Layout, Margin, Rect};\npub use style::{Color, Modifier, Style};\npub use text::{Alignment, Line, Span, Text};\n\n/// A point in cell space. The origin is the top-left of the terminal and\n/// `y` grows downward, matching how a buffer is addressed and how a\n/// terminal delivers mouse positions — no coordinate flip anywhere.\n#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]\npub struct Position {\n    pub x: u16,\n    pub y: u16,\n}\n\nimpl Position {\n    /// A point at `(x, y)`.\n    pub const fn new(x: u16, y: u16) -> Self {\n        Self { x, y }\n    }\n}\n\nimpl From<(u16, u16)> for Position {\n    fn from((x, y): (u16, u16)) -> Self {\n        Self::new(x, y)\n    }\n}\n",
    ),
    f(
        3,
        "buffer.rs",
        "9 KB",
        "//! The cell buffer: a flat `width * height` grid of `Cell`s that every\n//! widget stamps into and the backend diffs against the previous frame.\n//! This is the one piece of mutable state a render touches, and it is\n//! handed in, written, and read back — never retained by a widget.\n\nuse crate::{Position, Rect, Style};\n\n/// One screen cell: a single glyph and its resolved style.\n#[derive(Debug, Clone, PartialEq)]\npub struct Cell {\n    pub glyph: char,\n    pub style: Style,\n}\n\nimpl Default for Cell {\n    fn default() -> Self {\n        Self { glyph: ' ', style: Style::default() }\n    }\n}\n\n/// A `width * height` grid addressed in row-major order.\n#[derive(Debug, Clone)]\npub struct Buffer {\n    area: Rect,\n    cells: Vec<Cell>,\n}\n\nimpl Buffer {\n    /// An all-blank buffer covering `area`.\n    pub fn empty(area: Rect) -> Self {\n        let len = area.width as usize * area.height as usize;\n        Self { area, cells: vec![Cell::default(); len] }\n    }\n\n    /// The flat index of `(x, y)`, or `None` if it is outside the area.\n    fn index_of(&self, x: u16, y: u16) -> Option<usize> {\n        if x < self.area.x || y < self.area.y {\n            return None;\n        }\n        let (cx, cy) = (x - self.area.x, y - self.area.y);\n        if cx >= self.area.width || cy >= self.area.height {\n            return None;\n        }\n        Some(cy as usize * self.area.width as usize + cx as usize)\n    }\n\n    /// Stamps one glyph, patching the existing style rather than replacing\n    /// it so a later pass can restyle without knowing the glyph.\n    pub fn set(&mut self, pos: Position, glyph: char, style: Style) {\n        if let Some(i) = self.index_of(pos.x, pos.y) {\n            let cell = &mut self.cells[i];\n            cell.glyph = glyph;\n            cell.style = cell.style.patch(style);\n        }\n    }\n}\n",
    ),
    f(
        3,
        "layout.rs",
        "8 KB",
        "//! The constraint solver: a list of constraints over a rectangle, run\n//! fresh every frame. The sub-rects are computed, used to render, and\n//! discarded — nothing is stored, so a resize is just the next solve.\n\n/// A rectangle in cell space.\n#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]\npub struct Rect {\n    pub x: u16,\n    pub y: u16,\n    pub width: u16,\n    pub height: u16,\n}\n\nimpl Rect {\n    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {\n        Self { x, y, width, height }\n    }\n\n    /// The right edge, one past the last column.\n    pub const fn right(&self) -> u16 {\n        self.x + self.width\n    }\n\n    /// Whether `pos` falls inside this rectangle.\n    pub fn contains(&self, pos: crate::Position) -> bool {\n        pos.x >= self.x\n            && pos.x < self.right()\n            && pos.y >= self.y\n            && pos.y < self.y + self.height\n    }\n}\n\n/// One split rule: a fixed length, a proportional fill, a percentage, or\n/// a minimum. The solver hands fixed and minimum sizes out first, then\n/// distributes the remainder across the fills by weight.\n#[derive(Debug, Clone, Copy)]\npub enum Constraint {\n    Length(u16),\n    Fill(u16),\n    Percentage(u16),\n    Min(u16),\n}\n",
    ),
    f(
        3,
        "style.rs",
        "4 KB",
        "//! A `Style` is a patch, not a value: every field is optional, and\n//! composition is a fold from the outside in. A widget restyles a\n//! subtree by setting one field and letting the rest fall through.\n\nbitflags::bitflags! {\n    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]\n    pub struct Modifier: u8 {\n        const BOLD = 0b0000_0001;\n        const DIM = 0b0000_0010;\n        const ITALIC = 0b0000_0100;\n        const UNDERLINED = 0b0000_1000;\n        const REVERSED = 0b0001_0000;\n    }\n}\n\n#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]\npub struct Style {\n    pub fg: Option<Color>,\n    pub bg: Option<Color>,\n    pub add: Modifier,\n}\n\nimpl Style {\n    /// `other`'s set fields win; its unset fields fall through to `self`.\n    pub fn patch(self, other: Style) -> Style {\n        Style {\n            fg: other.fg.or(self.fg),\n            bg: other.bg.or(self.bg),\n            add: self.add | other.add,\n        }\n    }\n}\n",
    ),
    f(
        2,
        "Cargo.toml",
        "0.3 KB",
        "[package]\nname = \"rstui-core\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n[dependencies]\nbitflags = \"2\"\n",
    ),
    d(1, "rstui-widgets"),
    d(2, "src"),
    f(
        3,
        "paragraph.rs",
        "11 KB",
        "//! A multi-line text widget with optional word wrap, a scroll offset,\n//! per-block alignment, and an optional framing block — the workhorse\n//! behind every read-only body in the kitchen sink.\n\nuse rstui_core::{Buffer, Position, Rect, Style};\n\npub struct Paragraph<'a> {\n    text: Text<'a>,\n    wrap: Option<Wrap>,\n    scroll: Position,\n    block: Option<Block<'a>>,\n    style: Style,\n}\n\nimpl<'a> Paragraph<'a> {\n    /// The number of rows this paragraph composes into at content `width`.\n    /// Exactly the count `render` lays out — both go through one wrap\n    /// path — so a caller that must size a box to its wrapped text, or\n    /// clamp a scroll offset against it, does so without a second wrap.\n    pub fn line_count(&self, width: u16) -> usize {\n        compose_rows(&self.text, self.style, self.wrap, width as usize).len()\n    }\n}\n\nimpl Widget for Paragraph<'_> {\n    fn render(self, area: Rect, buf: &mut Buffer) {\n        let inner = self.block.inner(area);\n        for (y, row) in compose_rows(&self.text, self.style, self.wrap, inner.width as usize)\n            .into_iter()\n            .skip(self.scroll.y as usize)\n            .take(inner.height as usize)\n            .enumerate()\n        {\n            row.stamp(buf, inner.x, inner.y + y as u16);\n        }\n    }\n}\n",
    ),
    f(
        3,
        "markdown.rs",
        "22 KB",
        "//! A hand-written CommonMark-ish renderer that projects straight into\n//! the cell buffer — headings, lists, code fences, blockquotes, tables,\n//! rules, and inline emphasis — with a clickable link registry. No DOM:\n//! a link is a question the reducer asks of the current frame.\n\nuse rstui_core::{Line, Position, Rect};\n\npub struct Markdown<'a> {\n    source: std::borrow::Cow<'a, str>,\n    scroll: u16,\n    block: Option<Block<'a>>,\n}\n\nimpl<'a> Markdown<'a> {\n    /// Parses the source and lays it out to display rows for content\n    /// `width`. The `.len()` is the post-wrap row count — the number a\n    /// caller clamps a scroll offset against, with no re-render.\n    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {\n        if width == 0 {\n            return Vec::new();\n        }\n        let blocks = parse(&self.source);\n        let mut rows = Vec::new();\n        layout_blocks(&blocks, width as usize, &mut rows);\n        rows\n    }\n\n    /// The link whose drawn rectangle contains `position` when rendered\n    /// into `area` with this widget's block and scroll, or `None`. The\n    /// reducer calls this against the exact area it rendered into.\n    pub fn link_at(&self, position: Position, area: Rect) -> Option<usize> {\n        self.link_regions(area)\n            .into_iter()\n            .find(|r| r.rect.contains(position))\n            .map(|r| r.index)\n    }\n}\n",
    ),
    f(
        3,
        "lib.rs",
        "5 KB",
        "//! rstui-widgets — every widget is a pure projection of caller-owned\n//! state into the cell buffer. Construct cheaply, configure with builder\n//! calls, consume in one render, drop. No lifecycle, no retained tree.\n\npub use block::{Block, BorderType};\npub use list::List;\npub use markdown::Markdown;\npub use paragraph::{Paragraph, Wrap};\npub use table::Table;\npub use tree::{Tree, TreeItem};\n\n/// The one trait a widget implements: take an area and a buffer, stamp\n/// the cells you own, return. That is the entire widget API surface.\npub trait Widget {\n    fn render(self, area: rstui_core::Rect, buf: &mut rstui_core::Buffer);\n}\n",
    ),
    d(1, "rstui-kitchen-sink"),
    d(2, "src"),
    f(
        3,
        "lib.rs",
        "7 KB",
        "//! rstui-kitchen-sink — one interactive App that tours the catalogue\n//! and then composes it into ten experiences that behave like real\n//! software. Every screen owns its state as a plain struct; the active\n//! screen's reducer is the only thing that mutates it.\n\npub(crate) mod chrome;\npub(crate) mod screens;\npub(crate) mod theme;\n\nuse rstui_runtime::{App, Cmd, Frame};\n\npub struct KitchenSink {\n    screen: screens::Screen,\n    state: screens::ScreenState,\n    theme: theme::Theme,\n}\n\nimpl App for KitchenSink {\n    type Message = Msg;\n\n    fn update(&mut self, msg: Msg) -> Cmd<Msg> {\n        // The shell reducer routes input to the active screen, which\n        // reports back whether it consumed the key and any toast to\n        // raise. The shell never reaches into a screen's state.\n        Cmd::none()\n    }\n\n    fn view(&self, frame: &mut Frame<'_>) {\n        let area = chrome::shell(&self.theme, self.screen, frame);\n        self.state.view(self.screen, &self.theme, 0, frame, area);\n    }\n}\n",
    ),
    d(3, "screens"),
    f(
        4,
        "rich_text.rs",
        "13 KB",
        "//! The Rich Text screen: a Tabs strip over a scrollable Paragraph, a\n//! scrollable Markdown handbook whose links exercise the Link type, a\n//! Mermaid flowchart, and a styled Span/Line sampler.\n//!\n//! The scroll offset grows unbounded with saturating arithmetic in the\n//! reducer and is clamped in the view against the widget's composed row\n//! count, so a long document scrolls deep and over-scroll pins to the\n//! tail instead of revealing blank rows — the same view-time clamp the\n//! live log tail uses. This is the file the Files preview is pointing\n//! at right now, which is appropriately recursive.\n\nfn max_scroll(&self, body: Rect) -> u16 {\n    let inner = crate::screens::block_inner(body);\n    let rows = match self.tab {\n        0 => Paragraph::new(PROSE).wrap(Wrap { trim: true }).line_count(inner.width),\n        1 => Markdown::new(DOC).lines(inner.width).len(),\n        _ => return 0,\n    };\n    u16::try_from(rows.saturating_sub(inner.height as usize)).unwrap_or(u16::MAX)\n}\n",
    ),
    f(
        4,
        "chat.rs",
        "9 KB",
        "//! A chat experience: a List of channels with unread Badges, a\n//! bottom-anchored bubble thread, a live typing Spinner, and a real\n//! editable composer. Type and press Enter to send; the peer canned-\n//! replies so the thread is always live. The scrollback is seeded long\n//! enough that the bottom-anchored window has something to move over.\n\nfn thread_lines(&self, theme: &Theme, width: u16) -> Vec<Line<'static>> {\n    let w = width.max(8) as usize;\n    let mut out = Vec::new();\n    for m in &self.chan().msgs {\n        if m.mine {\n            let pad = w.saturating_sub(m.text.chars().count() + 6);\n            out.push(Line::from(vec![\n                \" \".repeat(pad).into(),\n                m.text.clone().fg(theme.base).bg(theme.accent).bold(),\n            ]));\n        } else {\n            out.push(Line::from(vec![\n                format!(\"{} > \", m.who).fg(theme.accent_alt).bold(),\n                m.text.clone().fg(theme.text),\n            ]));\n        }\n        out.push(Line::from(\"\"));\n    }\n    out\n}\n",
    ),
    f(
        4,
        "mail.rs",
        "10 KB",
        "//! A three-pane email client: a folder List, a message List with\n//! unread and star markers, and a reading pane whose body is a\n//! scrollable Paragraph. The inbox is seeded with a dozen letters and\n//! multi-paragraph bodies so the reader offset is genuinely exercised.\n\nfn step(&mut self, d: i32) {\n    match self.pane {\n        Pane::Reader => {\n            // Unbounded saturating intent; the Paragraph shows the tail\n            // past the end, consistent with the rest of the sink.\n            if d < 0 {\n                self.scroll = self.scroll.saturating_sub(1);\n            } else {\n                self.scroll = self.scroll.saturating_add(1);\n            }\n        }\n        _ => { /* folder / list navigation */ }\n    }\n}\n",
    ),
    f(
        3,
        "chrome.rs",
        "9 KB",
        "//! The persistent shell: a header with the screen title and an FPS\n//! readout, the grouped navigation rail, a status bar, and the floating\n//! overlays (command palette, help, toasts). All of it is redrawn every\n//! frame from the shell state — there is no retained chrome.\n\npub(crate) fn shell(theme: &Theme, screen: Screen, frame: &mut Frame) -> Rect {\n    let [header, body, status] = Layout::vertical([\n        Constraint::Length(1),\n        Constraint::Fill(1),\n        Constraint::Length(1),\n    ])\n    .areas(frame.area());\n    // ... draw header / rail / status, return the content rect ...\n    body\n}\n",
    ),
    f(
        2,
        "Cargo.toml",
        "0.4 KB",
        "[package]\nname = \"rstui-kitchen-sink\"\nversion = \"0.0.1\"\nedition = \"2021\"\npublish = false\n\n[dependencies]\nrstui-core = { path = \"../rstui-core\" }\nrstui-widgets = { path = \"../rstui-widgets\" }\nrstui-runtime = { path = \"../rstui-runtime\" }\n",
    ),
    d(0, "docs"),
    f(
        1,
        "composition.md",
        "31 KB",
        "# Composing rstui\n\nThis guide is the long form of the rule the whole framework rests on:\n**pure projection across a frame boundary.**\n\n## The boundary\n\nOn one side is the reducer: event plus state to next state. On the other\nis the view: state to buffer. The reducer never paints; the view never\nmutates. Events become state, state becomes cells, and the two never\ninterleave.\n\n## Caller-owned state\n\nA widget owns none of the data it draws. A list's selected index, a\ncursor, a scroll offset — all of it is application data, mutated only by\nreducers and merely read by widgets. There is exactly one place any\npiece of state can change, and it is never inside a widget.\n\n## Clamp in the view\n\nInput handlers stay simple and total: they express intent with\nsaturating arithmetic and do not think about geometry. Clamping,\ntruncation, and layout are presentation concerns recomputed every frame\nin the view, where the real width and height are known.\n\n## Hit-testing without a DOM\n\nThe same function that lays a thing out can be asked where it rendered.\nThe reducer compares the click against that geometry. A clickable link\nis a question asked of the current frame, not an object that persists.\n",
    ),
    f(
        1,
        "README.md",
        "44 KB",
        "# rstui\n\nAn idiomatic Rust TUI framework built on one idea: pure projection of\ncaller-owned state into a cell buffer, every frame, with a hard frame\nboundary between the reducer and the view.\n\n- 56 widgets and a flagship kitchen-sink tour\n- a hand-written Markdown and Mermaid renderer\n- a 36-theme system with live switching\n- deterministic layout and text wrap, no float math\n\nSee `docs/composition.md` for the long form and run\n`cargo run -p rstui-kitchen-sink` to tour the catalogue.\n",
    ),
    f(
        0,
        "Cargo.toml",
        "0.6 KB",
        "[workspace]\nresolver = \"2\"\nmembers = [\n    \"crates/rstui-core\",\n    \"crates/rstui-widgets\",\n    \"crates/rstui-runtime\",\n    \"crates/rstui-kitchen-sink\",\n    \"crates/xtask\",\n]\n\n[workspace.package]\nversion = \"0.0.1\"\nedition = \"2021\"\n",
    ),
    f(
        0,
        "Cargo.lock",
        "92 KB",
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n",
    ),
];

/// The explorer's caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    sel: usize,
    expanded: [bool; TREE.len()],
    /// Rows the file-preview pane is scrolled down by. Grows unbounded
    /// with saturating arithmetic; [`view`](Self::view) clamps it to the
    /// preview's composed row count, and it resets to `0` whenever the
    /// selection (and therefore the previewed file) changes.
    scroll: u16,
}

impl State {
    /// Top-level dirs open so the tree is non-trivial at first paint.
    pub(crate) fn new() -> Self {
        let mut expanded = [false; TREE.len()];
        for (i, n) in TREE.iter().enumerate() {
            if n.dir && n.depth <= 1 {
                expanded[i] = true;
            }
        }
        Self {
            sel: 0,
            expanded,
            scroll: 0,
        }
    }

    /// Indices of the visible rows (collapsed folders hide their subtree).
    fn visible(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut hide: Option<u16> = None;
        for (i, n) in TREE.iter().enumerate() {
            if let Some(dp) = hide {
                if n.depth > dp {
                    continue;
                }
                hide = None;
            }
            out.push(i);
            if n.dir && !self.expanded[i] {
                hide = Some(n.depth);
            }
        }
        out
    }

    /// `↑/↓` select (resetting the preview scroll to the new file's top),
    /// `Enter` expand-folder / open-file, `←` collapses or falls back to
    /// the rail, `PgUp/PgDn` scroll the file preview. The preview offset
    /// grows unbounded here; [`view`](Self::view) clamps it.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        let vis = self.visible();
        match code {
            KeyCode::Up => {
                self.sel = self.sel.saturating_sub(1);
                self.scroll = 0;
            }
            KeyCode::Down => {
                self.sel = (self.sel + 1).min(vis.len() - 1);
                self.scroll = 0;
            }
            KeyCode::Enter | KeyCode::Right => {
                let n = vis[self.sel];
                if TREE[n].dir {
                    self.expanded[n] = !self.expanded[n];
                    self.scroll = 0;
                } else {
                    return ScreenOutcome::with_toast(
                        crate::screens::ToastLevel::Info,
                        format!("Open {}", TREE[n].name),
                    );
                }
            }
            KeyCode::Left => {
                let n = vis[self.sel];
                if TREE[n].dir && self.expanded[n] {
                    self.expanded[n] = false;
                    self.scroll = 0;
                } else {
                    return ScreenOutcome::ignored();
                }
            }
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Wheel scrolls the file preview (clamped in [`view`](Self::view)).
    pub(crate) fn on_scroll(&mut self, up: bool) {
        if up {
            self.scroll = self.scroll.saturating_sub(2);
        } else {
            self.scroll = self.scroll.saturating_add(2);
        }
    }

    /// Click a tree row to select it (mirrors the `view` tree pane).
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [tree_a, _right] =
            Layout::horizontal([Constraint::Percentage(42), Constraint::Fill(1)]).areas(content);
        let tin = tree_a.inner(Margin::new(1, 1));
        if tin.contains(pos) {
            let vis = self.visible();
            let i = (pos.y - tin.y) as usize;
            if i < vis.len() {
                self.sel = i;
                self.scroll = 0;
                return ScreenOutcome::consumed();
            }
        }
        ScreenOutcome::ignored()
    }

    /// Draw the explorer.
    pub(crate) fn view(&self, theme: &Theme, _tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let vis = self.visible();
        let sel = self.sel.min(vis.len() - 1);
        let node = &TREE[vis[sel]];

        let [tree_a, right] =
            Layout::horizontal([Constraint::Percentage(42), Constraint::Fill(1)]).areas(area);

        let items: Vec<TreeItem> = vis
            .iter()
            .map(|&i| {
                let n = &TREE[i];
                let label = if n.dir {
                    format!("{} {}/", if self.expanded[i] { '▾' } else { '▸' }, n.name)
                } else {
                    format!("  {}", n.name)
                };
                TreeItem::new(n.depth, label).expandable(n.dir && self.expanded[i])
            })
            .collect();
        frame.render_widget(
            Tree::new(items)
                .selected(Some(sel))
                .highlight_symbol("▌")
                .highlight_style(theme.selection())
                .style(theme.body())
                .block(framed(theme, "Explorer")),
            tree_a,
        );

        let [crumb, preview, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(right);

        // Path breadcrumb built from the selected node's ancestry.
        let mut path: Vec<Line> = Vec::new();
        let mut want = node.depth;
        for &i in vis[..=sel].iter().rev() {
            if TREE[i].depth == want {
                path.push(Line::from(TREE[i].name.to_string()));
                if want == 0 {
                    break;
                }
                want -= 1;
            }
        }
        path.reverse();
        frame.render_widget(
            Breadcrumb::new(&path)
                .separator('/')
                .style(theme.caption())
                .emphasis_style(theme.accent_text()),
            crumb,
        );

        let pblock = framed(theme, if node.dir { "Folder" } else { node.name });
        let pin = pblock.inner(preview);
        frame.render_widget(pblock, preview);
        if node.dir {
            let children = vis
                .iter()
                .filter(|&&i| TREE[i].depth == node.depth + 1)
                .count();
            frame.render_widget(
                DescriptionList::new([
                    DescriptionRow::new("Type", "directory".to_string()),
                    DescriptionRow::new("Name", node.name.to_string()),
                    DescriptionRow::new("Depth", node.depth.to_string()),
                    DescriptionRow::new("Shown children", children.to_string()),
                ])
                .key_style(theme.caption())
                .value_style(theme.body())
                .style(theme.body()),
                pin,
            );
        } else {
            // Clamp the unbounded preview offset to the composed row
            // count here, where the inner geometry is known — the same
            // view-time clamp the rich-text reader and log tail use.
            let para = Paragraph::new(node.body).wrap(Wrap { trim: false });
            let rows = para.line_count(pin.width);
            let max = u16::try_from(rows.saturating_sub(pin.height as usize)).unwrap_or(u16::MAX);
            frame.render_widget(
                para.scroll(Position::new(0, self.scroll.min(max)))
                    .style(theme.body()),
                pin,
            );
        }

        frame.render_widget(
            StatusBar::new()
                .left(
                    Line::from(format!(" {} ", if node.dir { "dir" } else { "file" }))
                        .style(theme.caption()),
                )
                .center(
                    Line::from("↑↓ select · Enter open · ← collapse · PgUp/Dn scroll")
                        .style(theme.caption()),
                )
                .right(Line::from(format!(" {} ", node.size)).style(theme.caption()))
                .style(Style::new().fg(theme.dim).bg(theme.raised)),
            foot,
        );
    }
}

/// A plain rounded panel.
fn framed(theme: &Theme, title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {title} ")).style(theme.caption()))
        .border_style(theme.border())
        .style(theme.body())
}
