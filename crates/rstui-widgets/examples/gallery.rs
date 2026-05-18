//! `gallery` — the flagship, fully-dynamic widget gallery for rstui.
//!
//! A single whole-terminal [`App`] (the Elm contract: caller-owned `model`,
//! `update(&mut self, Msg) -> Cmd`, pure `view(&self, &mut Frame)`) that
//! exercises **every widget in `rstui-widgets`**, driven by the *real*
//! runtime. It is run headless by the deterministic [`Harness`] over a
//! `TestBackend`, so the same reducer logic that drives a live terminal also
//! drives this file's scripted session and its `#[test]`s — the rstui
//! "TTY-free" discipline (`run()` and `Harness` share one settling core, so
//! `cargo run` here is literally the production loop with scripted input).
//!
//! ```text
//! cargo run  -p rstui-widgets --example gallery     # scripted snapshot tour
//! cargo test -p rstui-widgets --example gallery      # the same tour, asserted
//! ```
//!
//! # Shape (the composition model, ADR 0002/0004)
//!
//! - **Full-screen frame** carved with `rstui-core` [`Layout`] only: a top
//!   [`Tabs`] strip, a left [`Sidebar`] rail, a main content pane, a bottom
//!   [`StatusBar`] with live key hints. No retained widget tree — every screen
//!   is a *pure projection* of the model by [`view`](App::view); every change
//!   goes through [`update`](App::update).
//! - **Caller-owned state**: text via [`TextEdit`]/[`TextArea`] the reducer
//!   edits on `Char`/`Backspace`; focus via [`FocusRing`] the reducer steps on
//!   `Tab` and the widgets only *read*. Widgets never mutate at render time.
//! - **Genuinely dynamic**: a model `tick` (advanced by the runtime timer via
//!   [`on_tick`](App::on_tick), or `harness.tick()` headless) rolls the
//!   [`Sparkline`], spins the [`Spinner`]/[`Skeleton`], advances the
//!   [`Gauge`]/[`Stepper`], and expires [`Toast`]s. Selection, scrolling,
//!   typing, and overlays are all live reducer state.
//! - **Overlays** stack the [`FocusRing`]-scope way: [`CommandPalette`]
//!   (Ctrl-P), [`Modal`] (`m`), [`Drawer`] (`d`), [`HelpOverlay`] (`?`),
//!   [`Toast`] (`t` / on actions). `Esc` pops the topmost; otherwise quits.
//! - **Total**: resizes and tiny terminals clip instead of panicking — the
//!   layout and every widget are pure projections, so a 3×2 surface is just a
//!   smaller projection, asserted in the tests below.
//!
//! Keep the file one coherent unit: the model + reducer up top, then one
//! `view_*` helper per section/overlay, then the scripted `main`, then the
//! headless tests.

use rstui_core::{
    Alignment, Buffer, Color, Constraint, FocusId, FocusRing, KeyCode, KeyEvent, KeyModifiers,
    Layout, Line, Modifier, Rect, Span, Style, TextArea, TextEdit, Widget,
};
use rstui_runtime::{App, Cmd, Event, Frame, Harness};
// ADR 0024: `Editor`/`Diff`/`DiffLayout` moved to the `rstui-code` crate,
// which `rstui-widgets` cannot depend on (cycle). The code-editor + diff
// showcases now live in `crates/rstui-code/examples/`.
use rstui_widgets::{
    Accordion, AccordionSection, Alert, AlertLevel, Align, Avatar, Badge, BadgeLevel, Bar,
    BarChart, BarChartDirection, Block, BorderType, Breadcrumb, Button, Calendar, Card, Checkbox,
    CommandPalette, DatePicker, DescriptionList, DescriptionRow, Divider, DividerOrientation,
    Drawer, DrawerSide, Form, FormField, Gauge, Grid, HelpEntry, HelpOverlay, Input, Kbd, List,
    Markdown, MaskedInput, Menu, MenuItem, Modal, Pagination, Paragraph, Popover, PopoverSide,
    Radio, Row, ScrollView, Scrollbar, ScrollbarOrientation, Select, Sidebar, SidebarItem,
    Skeleton, Slider, Sparkline, Spinner, SplitPane, StatusBar, Step, Stepper, Switch, Table, Tabs,
    Toast, ToastLevel, ToastMessage, Tooltip, Tree, TreeGuides, TreeItem, VerticalAlignment, Wrap,
};
use rstui_widgets::{
    AxisBounds, FlameFrame, FlameGraph, Heatmap, Histogram, HistogramBucket, LineChart, LogLevel,
    LogPalette, LogRecord, LogStream, Percentile, Series, StatPanel, TraceSpan, TraceWaterfall,
    Trend,
};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Focus identities (Inputs section). The ring *order* below is the Tab order —
// never these raw values (ADR 0004); "is this focused?" is a cheap `==`.
// ---------------------------------------------------------------------------
const F_NAME: FocusId = FocusId::new(0);
const F_SECRET: FocusId = FocusId::new(1);
const F_DOC: FocusId = FocusId::new(2);
const F_VOLUME: FocusId = FocusId::new(3);
const F_AGREE: FocusId = FocusId::new(4);
const F_QUALITY: FocusId = FocusId::new(5);
const F_WIFI: FocusId = FocusId::new(6);
const F_SUBMIT: FocusId = FocusId::new(7);
const INPUT_RING: [FocusId; 8] = [
    F_NAME, F_SECRET, F_DOC, F_VOLUME, F_AGREE, F_QUALITY, F_WIFI, F_SUBMIT,
];

/// The six widget categories, in `Tabs` / digit-key order.
const SECTIONS: [&str; 6] = ["Inputs", "Select", "Data", "Feedback", "Layout", "Observe"];

/// Commands offered by the palette; the reducer filters these by the query
/// (the palette widget is a pure projection — it never filters).
const COMMANDS: [&str; 8] = [
    "Go to Inputs",
    "Go to Select",
    "Go to Data",
    "Go to Feedback",
    "Go to Layout",
    "Toggle Modal",
    "Toggle Drawer",
    "Push Toast",
];

/// One live toast: its level kind (`0..=3`), text, and the model tick it was
/// born on. The reducer expires it on a later tick — [`Toast`] only ever
/// *projects* this list.
struct ToastSpec {
    kind: u8,
    text: String,
    born: usize,
}

fn toast_level(kind: u8) -> ToastLevel {
    match kind {
        0 => ToastLevel::Info,
        1 => ToastLevel::Success,
        2 => ToastLevel::Warning,
        _ => ToastLevel::Error,
    }
}

/// The whole gallery state. Every visible glyph is a pure projection of these
/// fields; every mutation flows through [`update`](App::update).
struct Gallery {
    /// Current category (index into [`SECTIONS`]); the [`Tabs`] selection.
    section: usize,
    /// [`Sidebar`] rail selection (mirrors `section`).
    /// Animation clock — advanced by the runtime timer (or `harness.tick()`),
    /// never by `view`. Spins/rolls/expires everything time-driven.
    tick: usize,
    /// Rolling [`Sparkline`] window the tick pushes onto.
    spark: Vec<u64>,

    // -- Inputs section: caller-owned editors + a focus ring over controls. --
    name: TextEdit,
    secret: TextEdit,
    doc: TextArea,
    unmask: bool,
    volume: f64,
    agree: bool,
    quality: usize,
    wifi: bool,
    focus: FocusRing,

    // -- Selection section. --
    nav: usize,
    select_open: bool,
    select_sel: usize,
    page: usize,

    // -- Data section. --
    md_scroll: u16,
    cal_day: u32,

    // -- Layout section. --
    accordion: [bool; 3],

    // -- Overlays. --
    palette_open: bool,
    palette_query: TextEdit,
    palette_hl: usize,
    modal_open: bool,
    drawer_open: bool,
    help_open: bool,
    show_popover: bool,
    toasts: Vec<ToastSpec>,
}

impl Default for Gallery {
    fn default() -> Self {
        Self {
            section: 0,
            tick: 0,
            spark: vec![3, 5, 8, 4, 6, 9, 7, 5, 4, 6],
            name: TextEdit::from_value("ada"),
            secret: TextEdit::from_value("hunter2"),
            doc: TextArea::from_value("edit me — Enter splits a line,\narrows move the caret."),
            unmask: false,
            volume: 45.0,
            agree: true,
            quality: 1,
            wifi: true,
            focus: FocusRing::with_ids(INPUT_RING),
            nav: 0,
            select_open: false,
            select_sel: 1,
            page: 2,
            md_scroll: 0,
            cal_day: 17,
            accordion: [true, false, false],
            palette_open: false,
            palette_query: TextEdit::new(),
            palette_hl: 0,
            modal_open: false,
            drawer_open: false,
            help_open: false,
            show_popover: false,
            toasts: Vec::new(),
        }
    }
}

impl Gallery {
    /// True when *every* keystroke is text: only while the command palette
    /// owns the keyboard. Elsewhere the reserved hotkeys (digits, `[`/`]`,
    /// `m`/`d`/`t`/`v`/`?`/`q`, space) stay global so the gallery is always
    /// navigable; any *other* char still routes to the focused Input/Editor
    /// via the `Char(c) -> Type` fall-through (see `type_char`).
    fn capturing_text(&self) -> bool {
        self.palette_open
    }

    /// Whether any overlay is up (so `Esc` pops instead of quitting).
    fn overlay_open(&self) -> bool {
        self.palette_open || self.modal_open || self.drawer_open || self.help_open
    }

    /// The palette results for the current query (reducer-side filter).
    fn palette_matches(&self) -> Vec<&'static str> {
        let q = self.palette_query.value().to_lowercase();
        COMMANDS
            .iter()
            .copied()
            .filter(|c| q.is_empty() || c.to_lowercase().contains(&q))
            .collect()
    }

    fn push_toast(&mut self, kind: u8, text: &str) {
        self.toasts.insert(
            0,
            ToastSpec {
                kind,
                text: text.to_string(),
                born: self.tick,
            },
        );
        if self.toasts.len() > 4 {
            self.toasts.truncate(4);
        }
    }

    /// Route a typed char to whatever owns text right now.
    fn type_char(&mut self, c: char) {
        if self.palette_open {
            self.palette_query.insert_char(c);
            self.palette_hl = 0;
            return;
        }
        match self.focus.focused() {
            Some(id) if id == F_NAME => self.name.insert_char(c),
            Some(id) if id == F_SECRET => self.secret.insert_char(c),
            Some(id) if id == F_DOC => self.doc.insert_char(c),
            _ => {}
        }
    }

    fn backspace(&mut self) {
        if self.palette_open {
            self.palette_query.delete_backward();
            self.palette_hl = 0;
            return;
        }
        match self.focus.focused() {
            Some(id) if id == F_NAME => {
                self.name.delete_backward();
            }
            Some(id) if id == F_SECRET => {
                self.secret.delete_backward();
            }
            Some(id) if id == F_DOC => {
                self.doc.delete_backward();
            }
            _ => {}
        }
    }
}

/// Everything that can happen, mapped from input by [`on_event`](App::on_event)
/// and folded in by [`update`](App::update).
enum Msg {
    Tick,
    Section(usize),
    NextSection,
    PrevSection,
    FocusNext,
    FocusPrev,
    Up,
    Down,
    Left,
    Right,
    Type(char),
    Backspace,
    Enter,
    Space,
    OpenPalette,
    ToggleModal,
    ToggleDrawer,
    ToggleHelp,
    TogglePopover,
    PushToast,
    Escape,
    Quit,
}

impl App for Gallery {
    type Message = Msg;

    /// A steady timer so the live binary animates; headless tests drive the
    /// same path with `harness.tick()`.
    fn tick_rate(&self) -> Option<Duration> {
        Some(Duration::from_millis(120))
    }

    fn on_tick(&self) -> Option<Msg> {
        Some(Msg::Tick)
    }

    fn on_event(&self, event: Event) -> Option<Msg> {
        let key = event.as_key_press()?;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Ctrl-P toggles the palette from anywhere.
        if ctrl && matches!(key.code, KeyCode::Char('p')) {
            return Some(if self.palette_open {
                Msg::Escape
            } else {
                Msg::OpenPalette
            });
        }

        match key.code {
            KeyCode::Esc => Some(if self.overlay_open() {
                Msg::Escape
            } else {
                Msg::Quit
            }),
            KeyCode::Tab => Some(Msg::FocusNext),
            KeyCode::BackTab => Some(Msg::FocusPrev),
            KeyCode::Up => Some(Msg::Up),
            KeyCode::Down => Some(Msg::Down),
            KeyCode::Left => Some(Msg::Left),
            KeyCode::Right => Some(Msg::Right),
            KeyCode::Backspace => Some(Msg::Backspace),
            KeyCode::Enter => Some(Msg::Enter),
            // Digits jump sections — unless text is being captured.
            KeyCode::Char(d @ '1'..='6') if !self.capturing_text() => {
                Some(Msg::Section(d as usize - '1' as usize))
            }
            KeyCode::Char(' ') if !self.capturing_text() => Some(Msg::Space),
            KeyCode::Char('[') if !self.capturing_text() => Some(Msg::PrevSection),
            KeyCode::Char(']') if !self.capturing_text() => Some(Msg::NextSection),
            KeyCode::Char('?') if !self.capturing_text() => Some(Msg::ToggleHelp),
            KeyCode::Char('m') if !self.capturing_text() => Some(Msg::ToggleModal),
            KeyCode::Char('d') if !self.capturing_text() => Some(Msg::ToggleDrawer),
            KeyCode::Char('t') if !self.capturing_text() => Some(Msg::PushToast),
            KeyCode::Char('v') if !self.capturing_text() => Some(Msg::TogglePopover),
            KeyCode::Char('q') if !self.capturing_text() => Some(Msg::Quit),
            KeyCode::Char(c) => Some(Msg::Type(c)),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Tick => {
                self.tick = self.tick.wrapping_add(1);
                // Roll a deterministic sample onto the sparkline window.
                let sample = ((self.tick * 37 + 11) % 23) as u64 + 2;
                self.spark.push(sample);
                if self.spark.len() > 40 {
                    self.spark.remove(0);
                }
                // Expire toasts older than 24 ticks.
                let now = self.tick;
                self.toasts.retain(|t| now.saturating_sub(t.born) < 24);
            }
            Msg::Section(i) => self.section = i.min(SECTIONS.len() - 1),
            Msg::NextSection => self.section = (self.section + 1) % SECTIONS.len(),
            Msg::PrevSection => self.section = (self.section + SECTIONS.len() - 1) % SECTIONS.len(),
            Msg::FocusNext => {
                if self.section == 0 {
                    self.focus.focus_next();
                } else if self.section == 1 {
                    self.nav = (self.nav + 1) % 6;
                }
            }
            Msg::FocusPrev => {
                if self.section == 0 {
                    self.focus.focus_prev();
                } else if self.section == 1 {
                    self.nav = (self.nav + 5) % 6;
                }
            }
            Msg::Up => match self.section {
                1 => self.nav = self.nav.saturating_sub(1),
                2 => self.md_scroll = self.md_scroll.saturating_sub(1),
                _ => {}
            },
            Msg::Down => match self.section {
                1 => self.nav = (self.nav + 1).min(5),
                2 => self.md_scroll = (self.md_scroll + 1).min(40),
                _ => {}
            },
            Msg::Left => match self.section {
                0 if matches!(self.focus.focused(), Some(id) if id == F_VOLUME) => {
                    self.volume = (self.volume - 5.0).max(0.0);
                }
                1 => self.page = self.page.saturating_sub(1),
                2 => self.cal_day = self.cal_day.saturating_sub(1).max(1),
                _ => {}
            },
            Msg::Right => match self.section {
                0 if matches!(self.focus.focused(), Some(id) if id == F_VOLUME) => {
                    self.volume = (self.volume + 5.0).min(100.0);
                }
                1 => self.page = (self.page + 1).min(7),
                2 => self.cal_day = (self.cal_day + 1).min(31),
                _ => {}
            },
            Msg::Type(c) => self.type_char(c),
            Msg::Backspace => self.backspace(),
            Msg::Enter => {
                if self.palette_open {
                    let matches = self.palette_matches();
                    if let Some(&cmd) = matches.get(self.palette_hl) {
                        self.palette_open = false;
                        match cmd {
                            "Go to Inputs" => self.section = 0,
                            "Go to Select" => self.section = 1,
                            "Go to Data" => self.section = 2,
                            "Go to Feedback" => self.section = 3,
                            "Go to Layout" => self.section = 4,
                            "Toggle Modal" => self.modal_open = !self.modal_open,
                            "Toggle Drawer" => self.drawer_open = !self.drawer_open,
                            "Push Toast" => self.push_toast(1, "Command executed"),
                            _ => {}
                        }
                    }
                } else if self.section == 0
                    && matches!(self.focus.focused(), Some(id) if id == F_DOC)
                {
                    self.doc.insert_newline();
                } else if self.section == 0
                    && matches!(self.focus.focused(), Some(id) if id == F_SUBMIT)
                {
                    self.push_toast(1, "Form submitted");
                } else if self.section == 1 {
                    self.select_open = false;
                    self.push_toast(0, "Selection committed");
                } else {
                    self.push_toast(0, "Activated");
                }
            }
            Msg::Space => match self.section {
                0 => match self.focus.focused() {
                    Some(id) if id == F_AGREE => self.agree = !self.agree,
                    Some(id) if id == F_WIFI => self.wifi = !self.wifi,
                    Some(id) if id == F_QUALITY => self.quality = (self.quality + 1) % 3,
                    Some(id) if id == F_SUBMIT => self.push_toast(1, "Form submitted"),
                    _ => {}
                },
                1 => {
                    self.select_open = !self.select_open;
                    if !self.select_open {
                        self.select_sel = self.nav.min(3);
                    }
                }
                4 => {
                    let i = self.nav % 3;
                    self.accordion[i] = !self.accordion[i];
                }
                _ => {}
            },
            Msg::OpenPalette => {
                self.palette_open = true;
                self.palette_query.clear();
                self.palette_hl = 0;
            }
            Msg::ToggleModal => self.modal_open = !self.modal_open,
            Msg::ToggleDrawer => self.drawer_open = !self.drawer_open,
            Msg::ToggleHelp => self.help_open = !self.help_open,
            Msg::TogglePopover => self.show_popover = !self.show_popover,
            Msg::PushToast => {
                let kind = (self.tick % 4) as u8;
                self.push_toast(kind, "Toast pushed — expires on a later tick");
            }
            Msg::Escape => {
                if self.palette_open {
                    self.palette_open = false;
                } else if self.help_open {
                    self.help_open = false;
                } else if self.modal_open {
                    self.modal_open = false;
                } else if self.drawer_open {
                    self.drawer_open = false;
                }
            }
            Msg::Quit => return Cmd::quit(),
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        if area.width < 4 || area.height < 4 {
            // Totality: a too-small surface clips to a marker, never panics.
            if area.width >= 1 && area.height >= 1 {
                frame.render_widget("rstui", area);
            }
            return;
        }

        // Top tabs (1) / body (rest) / status bar (1) — the canonical shell.
        let [top, body, status] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);

        self.view_tabs(frame, top);

        // Body: a left sidebar rail + the active section pane.
        let [rail, content] =
            Layout::horizontal([Constraint::Length(14), Constraint::Fill(1)]).areas(body);
        self.view_sidebar(frame, rail);

        match self.section {
            0 => self.view_inputs(frame, content),
            1 => self.view_selection(frame, content),
            2 => self.view_data(frame, content),
            3 => self.view_feedback(frame, content),
            4 => self.view_layout(frame, content),
            _ => self.view_observe(frame, content),
        }

        self.view_status(frame, status);

        // Overlays last, over the whole screen, in z-order.
        self.view_overlays(frame, area);
    }
}

// ---------------------------------------------------------------------------
// Shell chrome
// ---------------------------------------------------------------------------
impl Gallery {
    fn view_tabs(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(
            Tabs::new(SECTIONS)
                .selected(Some(self.section))
                .style(Style::new().fg(Color::Gray))
                .highlight_style(
                    Style::new()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            area,
        );
    }

    fn view_sidebar(&self, frame: &mut Frame<'_>, area: Rect) {
        let items = vec![
            SidebarItem::group("WIDGETS"),
            SidebarItem::new("Inputs").icon('>'),
            SidebarItem::new("Select").icon('='),
            SidebarItem::new("Data").icon('#'),
            SidebarItem::new("Feedback").icon('*'),
            SidebarItem::new("Layout").icon('+'),
        ];
        frame.render_widget(
            Sidebar::new(&items)
                .selected(Some(self.section + 1))
                .block(Block::bordered().title("nav"))
                .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                .group_style(Style::new().fg(Color::DarkGray)),
            area,
        );
    }

    fn view_status(&self, frame: &mut Frame<'_>, area: Rect) {
        let hints = "Tab focus · ←↑↓→ move · 1-5 tab · ^P palette · m/d/?/t · q quit";
        frame.render_widget(
            StatusBar::new()
                .left(Line::raw(format!(" {} ", SECTIONS[self.section])))
                .center(Line::raw(hints))
                .right(Line::raw(format!("tick {} ", self.tick)))
                .style(Style::new().fg(Color::Gray).bg(Color::DarkGray)),
            area,
        );
    }
}

// ---------------------------------------------------------------------------
// Section: Inputs — editable text, focus ring, form controls
// ---------------------------------------------------------------------------
impl Gallery {
    fn view_inputs(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::bordered().title("Inputs · Tab cycles focus, type to edit");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 8 || inner.height < 6 {
            return;
        }

        let focus_style = Style::new().fg(Color::Black).bg(Color::Cyan);
        let label = Style::new().fg(Color::Cyan);

        // A Form lays out label rows; we render our own controls into them.
        let form = Form::new()
            .label_width(8)
            .label_style(label)
            .help_style(Style::new().fg(Color::DarkGray))
            .field(FormField::new("Name", 1).help("Input — a single-line TextEdit"))
            .field(FormField::new("Secret", 1).help("MaskedInput — press the toggle below"))
            .field(
                FormField::new("Notes", 3)
                    .help("Paragraph — caller-owned TextArea (Editor: rstui-code)"),
            )
            .field(FormField::new("Volume", 1).help("Slider — ←/→ when focused"));
        let rects = form.layout(inner);
        frame.render_widget(form, inner);

        if let Some(&r) = rects.first() {
            frame.render_widget(
                Input::new(&self.name)
                    .focused(self.focus.is_focused(F_NAME))
                    .placeholder("your name")
                    .focus_style(focus_style),
                r,
            );
        }
        if let Some(&r) = rects.get(1) {
            frame.render_widget(
                MaskedInput::new(&self.secret)
                    .focused(self.focus.is_focused(F_SECRET))
                    .unmasked(self.unmask)
                    .focus_style(focus_style),
                r,
            );
        }
        if let Some(&r) = rects.get(2) {
            // ADR 0024: the `Editor` code-editing widget moved to the
            // `rstui-code` crate (which `rstui-widgets` cannot depend on —
            // that would cycle). The multi-line code-editor showcase now
            // lives in `crates/rstui-code/examples/code_editor.rs`. Here we
            // keep the caller-owned `TextArea` (still edited via F_DOC keys)
            // but project it through the in-crate `Paragraph`.
            let style = if self.focus.is_focused(F_DOC) {
                Style::new().bg(Color::Black)
            } else {
                Style::new()
            };
            frame.render_widget(Paragraph::new(self.doc.lines().join("\n")).style(style), r);
        }
        if let Some(&r) = rects.get(3) {
            frame.render_widget(
                Slider::new()
                    .range(0.0, 100.0)
                    .value(self.volume)
                    .focused(self.focus.is_focused(F_VOLUME))
                    .thumb_style(Style::new().fg(Color::Cyan))
                    .focus_style(focus_style),
                r,
            );
        }

        // Bottom strip: the no-data toggles + an action Button.
        if inner.height >= 9 {
            let controls = Rect::new(inner.x, inner.bottom().saturating_sub(2), inner.width, 1);
            let cols = Layout::horizontal([
                Constraint::Length(14),
                Constraint::Length(14),
                Constraint::Length(8),
                Constraint::Fill(1),
            ])
            .split(controls);
            frame.render_widget(
                Checkbox::new("Agree")
                    .checked(self.agree)
                    .focused(self.focus.is_focused(F_AGREE))
                    .focus_style(focus_style),
                cols[0],
            );
            frame.render_widget(
                Radio::new(["Low", "Med", "High"][self.quality])
                    .selected(true)
                    .focused(self.focus.is_focused(F_QUALITY))
                    .focus_style(focus_style),
                cols[1],
            );
            frame.render_widget(
                Switch::new()
                    .on(self.wifi)
                    .focused(self.focus.is_focused(F_WIFI))
                    .focus_style(focus_style),
                cols[2],
            );
            frame.render_widget(
                Button::new("Submit")
                    .focused(self.focus.is_focused(F_SUBMIT))
                    .focus_style(focus_style),
                cols[3],
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Section: Select — list / tree / table / menu / select / pagination / stepper
// ---------------------------------------------------------------------------
impl Gallery {
    fn view_selection(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::bordered().title("Select · ↑↓ move, Space opens dropdown, ←→ page");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 10 || inner.height < 6 {
            return;
        }
        let hl = Style::new().fg(Color::Black).bg(Color::Cyan);

        let [left, right] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Fill(1)]).areas(inner);
        let [l_top, l_bot] =
            Layout::vertical([Constraint::Percentage(55), Constraint::Fill(1)]).areas(left);
        let [r_top, r_mid, r_bot] = Layout::vertical([
            Constraint::Percentage(45),
            Constraint::Length(5),
            Constraint::Fill(1),
        ])
        .areas(right);

        let menu_labels = ["Open", "Save", "Save As", "Close"];
        frame.render_widget(
            List::new(menu_labels)
                .block(Block::bordered().title("List"))
                .selected(Some(self.nav.min(menu_labels.len() - 1)))
                .highlight_style(hl),
            l_top,
        );

        let tree = vec![
            TreeItem::new(0, "src").expandable(true),
            TreeItem::new(1, "main.rs"),
            TreeItem::new(1, "widgets").expandable(true),
            TreeItem::new(2, "list.rs"),
            TreeItem::new(0, "Cargo.toml"),
        ];
        frame.render_widget(
            Tree::new(tree)
                .block(Block::bordered().title("Tree"))
                .guides(TreeGuides::Lines)
                .guide_style(Style::new().fg(Color::DarkGray))
                .selected(Some(self.nav.min(4)))
                .highlight_style(hl),
            l_bot,
        );

        frame.render_widget(
            Table::new(
                [
                    Row::new(["init", "ok"]),
                    Row::new(["build", "ok"]),
                    Row::new(["deploy", "fail"]),
                ],
                [Constraint::Percentage(60), Constraint::Fill(1)],
            )
            .header(Row::new(["stage", "state"]).style(Style::new().add_modifier(Modifier::BOLD)))
            .block(Block::bordered().title("Table"))
            .selected(Some(self.nav.min(2)))
            .highlight_style(hl),
            r_top,
        );

        let select_block = Block::bordered().title("Select (Space)");
        let select_inner = select_block.inner(r_mid);
        frame.render_widget(select_block, r_mid);
        frame.render_widget(
            Select::new(["Dark", "Light", "Solarized", "High Contrast"])
                .open(self.select_open)
                .selected(Some(self.select_sel))
                .highlight(self.nav.min(3))
                .focused(true)
                .focus_style(hl)
                .highlight_style(hl),
            select_inner,
        );

        let items = vec![
            MenuItem::new("Cut").key_hint("^X"),
            MenuItem::new("Copy").key_hint("^C"),
            MenuItem::separator(),
            MenuItem::new("Paste").key_hint("^V").disabled(true),
        ];
        let [m_left, m_right] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Fill(1)]).areas(r_bot);
        frame.render_widget(
            Menu::new(&items)
                .highlight(self.nav.min(3))
                .block(Block::bordered().title("Menu"))
                .highlight_style(hl)
                .disabled_style(Style::new().fg(Color::DarkGray)),
            m_left,
        );

        let [pg, st] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(m_right);
        frame.render_widget(
            Pagination::new(self.page, 8)
                .current_style(hl)
                .control_style(Style::new().fg(Color::DarkGray)),
            pg,
        );
        frame.render_widget(
            Stepper::new([Step::new("Plan"), Step::new("Build"), Step::new("Ship")])
                .current((self.tick / 12) % 3)
                .current_style(hl)
                .done_style(Style::new().fg(Color::Green))
                .pending_style(Style::new().fg(Color::DarkGray)),
            st,
        );
    }
}

// ---------------------------------------------------------------------------
// Section: Data — paragraph / markdown / diff / sparkline / chart / lists
// ---------------------------------------------------------------------------
impl Gallery {
    fn view_data(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::bordered().title("Data · ↑↓ scroll markdown, ←→ pick a day");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 12 || inner.height < 8 {
            return;
        }

        let [left, right] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Fill(1)]).areas(inner);
        let [md_area, diff_area] =
            Layout::vertical([Constraint::Percentage(55), Constraint::Fill(1)]).areas(left);

        const DOC: &str = "# rstui gallery\n\nWidgets are **pure projections**.\n\n- caller-owned state\n- no retained tree\n- `Markdown` parses inline\n\n> Scroll with the arrows.\n";
        frame.render_widget(
            Markdown::new(DOC)
                .scroll(self.md_scroll)
                .block(Block::bordered().title("Markdown")),
            md_area,
        );

        // ADR 0024: the `Diff` widget moved to the `rstui-code` crate
        // (`rstui-widgets` cannot depend on it — that would cycle). The full
        // unified-diff showcase is `crates/rstui-code/examples/diff_demo.rs`;
        // here we just show the raw patch through the in-crate `Paragraph`.
        const PATCH: &str = "@@ -1,3 +1,3 @@\n fn main() {\n-    old();\n+    new();\n }\n";
        frame.render_widget(
            Paragraph::new(PATCH).block(Block::bordered().title("Diff (see rstui-code)")),
            diff_area,
        );

        let [spark_area, chart_area, meta_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Percentage(50),
            Constraint::Fill(1),
        ])
        .areas(right);

        let sb = Block::bordered().title("Sparkline (rolling)");
        let sb_inner = sb.inner(spark_area);
        frame.render_widget(sb, spark_area);
        frame.render_widget(
            Sparkline::new(&self.spark).style(Style::new().fg(Color::Green)),
            sb_inner,
        );

        frame.render_widget(
            BarChart::new([Bar::new(42, "Rust"), Bar::new(30, "Go"), Bar::new(17, "TS")])
                .direction(BarChartDirection::Vertical)
                .bar_style(Style::new().fg(Color::Magenta))
                .block(Block::bordered().title("BarChart")),
            chart_area,
        );

        let [cal_area, right_meta] =
            Layout::horizontal([Constraint::Length(24), Constraint::Fill(1)]).areas(meta_area);
        frame.render_widget(
            Calendar::new(2026, 5, 31, 5)
                .selected(Some(self.cal_day))
                .today(Some(17))
                .selected_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                .today_style(Style::new().fg(Color::Yellow))
                .weekday_style(Style::new().fg(Color::DarkGray))
                .block(Block::bordered().title("Calendar")),
            cal_area,
        );
        let [dv_a, dp_a, desc_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(right_meta);
        frame.render_widget(
            Divider::new()
                .label("meta")
                .orientation(DividerOrientation::Horizontal)
                .style(Style::new().fg(Color::DarkGray))
                .label_style(Style::new().fg(Color::Yellow)),
            dv_a,
        );
        // A closed DatePicker field (←/→ moves the selected day, like the
        // Calendar): the Select anchored-panel idiom, kept closed here.
        frame.render_widget(
            DatePicker::new(2026, 5, 31, 5)
                .selected(Some(self.cal_day))
                .today(Some(17))
                .selected_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                .focus_style(Style::new().fg(Color::Black).bg(Color::Cyan)),
            dp_a,
        );
        let rows = [
            DescriptionRow::new(
                Span::styled("Day", Style::new().fg(Color::Cyan)),
                Line::raw(format!("{}", self.cal_day)),
            ),
            DescriptionRow::new(
                Span::styled("Scroll", Style::new().fg(Color::Cyan)),
                Line::raw(format!("{}", self.md_scroll)),
            ),
            DescriptionRow::new(
                Span::styled("Tick", Style::new().fg(Color::Cyan)),
                Line::raw(format!("{}", self.tick)),
            ),
        ];
        frame.render_widget(
            DescriptionList::new(rows).block(Block::bordered().title("DescriptionList")),
            desc_area,
        );
    }
}

// ---------------------------------------------------------------------------
// Section: Observe — the observability widget family (metrics / traces / logs)
// ---------------------------------------------------------------------------
impl Gallery {
    fn view_observe(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::bordered().title("Observe · metrics · traces · logs");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 16 || inner.height < 10 {
            return;
        }
        let t = self.tick;

        let [tiles, mid, low] = Layout::vertical([
            Constraint::Length(5),
            Constraint::Length(9),
            Constraint::Fill(1),
        ])
        .areas(inner);

        // Golden-signal stat tiles.
        let tcols = Layout::horizontal([Constraint::Fill(1); 3]).split(tiles);
        let stats: [(&str, String, Trend); 3] = [
            (
                "req/s",
                format!("{:.1}k", 11.0 + (t % 30) as f64 / 10.0),
                Trend::Up,
            ),
            (
                "err %",
                format!("{:.2}", 0.3 + (t % 17) as f64 / 100.0),
                Trend::Down,
            ),
            ("p99 ms", format!("{}", 150 + t % 40), Trend::Up),
        ];
        for ((cap, val, trend), cell) in stats.iter().zip(tcols.iter()) {
            let spark: Vec<u64> = (0..cell.width.max(1))
                .map(|x| ((f64::from(x) * 0.5 + t as f64 * 0.2).sin() * 7.0 + 9.0) as u64)
                .collect();
            frame.render_widget(
                StatPanel::new(val.clone())
                    .caption(*cap)
                    .delta("vs 1h")
                    .trend(*trend)
                    .trend_style(Style::new().fg(Color::Cyan))
                    .sparkline(&spark)
                    .spark_style(Style::new().fg(Color::DarkGray))
                    .block(Block::bordered()),
                *cell,
            );
        }

        // Latency line chart + distribution histogram.
        let [chart, dist] =
            Layout::horizontal([Constraint::Percentage(56), Constraint::Fill(1)]).areas(mid);
        let p50: Vec<(f64, f64)> = (0..60)
            .map(|x| {
                (
                    f64::from(x),
                    (f64::from(x) * 0.2 + t as f64 * 0.1).sin() * 12.0 + 40.0,
                )
            })
            .collect();
        let p99: Vec<(f64, f64)> = (0..60)
            .map(|x| {
                (
                    f64::from(x),
                    (f64::from(x) * 0.2 + t as f64 * 0.1).sin() * 20.0 + 90.0,
                )
            })
            .collect();
        let series = [
            Series::new("p50", &p50).style(Style::new().fg(Color::Green)),
            Series::new("p99", &p99).style(Style::new().fg(Color::Red)),
        ];
        frame.render_widget(
            LineChart::new(&series)
                .x_bounds(AxisBounds::new(0.0, 60.0))
                .y_bounds(AxisBounds::new(0.0, 130.0))
                .block(Block::bordered().title("Latency")),
            chart,
        );
        let buckets: Vec<HistogramBucket> = ["10", "25", "50", "75", "100", "250"]
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let bell = 1.0 - ((i as f64 - 2.5) / 3.0).powi(2);
                HistogramBucket::new(
                    (bell.max(0.05) * 80.0) as u64 + (t % 5) as u64,
                    format!("≤{b}"),
                )
            })
            .collect();
        let pcts = [
            Percentile::new(0.5, "p50").style(Style::new().fg(Color::Green)),
            Percentile::new(0.95, "p95").style(Style::new().fg(Color::Yellow)),
        ];
        frame.render_widget(
            Histogram::new(&buckets)
                .bar_width(3)
                .bar_gap(1)
                .percentiles(&pcts)
                .bar_style(Style::new().fg(Color::Magenta))
                .block(Block::bordered().title("Distribution")),
            dist,
        );

        // Trace waterfall ⇄ flame graph + a heatmap + a log stream.
        let [trace, rest] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Fill(1)]).areas(low);
        let [flame_a, hl] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Fill(1)]).areas(rest);
        let spans = [
            TraceSpan::new(0, 0, 100, "GET /checkout").style(Style::new().fg(Color::Cyan)),
            TraceSpan::new(1, 8, 34, "auth.verify").style(Style::new().fg(Color::Blue)),
            TraceSpan::new(1, 44, 48, "db.query").style(Style::new().fg(Color::Green)),
            TraceSpan::new(2, 50, 30, "pg.scan").style(Style::new().fg(Color::Yellow)),
        ];
        frame.render_widget(
            TraceWaterfall::new(&spans)
                .total(Some(100))
                .name_width(12)
                .selected(Some((t / 4) % 4))
                .block(Block::bordered().title("Trace")),
            trace,
        );
        let frames = [
            FlameFrame::new(0, 0, 100, "main").style(Style::new().fg(Color::Black).bg(Color::Blue)),
            FlameFrame::new(1, 0, 58, "parse").style(Style::new().fg(Color::Black).bg(Color::Cyan)),
            FlameFrame::new(1, 58, 42, "eval")
                .style(Style::new().fg(Color::Black).bg(Color::Green)),
            FlameFrame::new(2, 0, 30, "lex").style(Style::new().fg(Color::Black).bg(Color::Yellow)),
        ];
        let [flame, heat] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Fill(1)]).areas(flame_a);
        frame.render_widget(
            FlameGraph::new(&frames)
                .total(Some(100))
                .block(Block::bordered().title("Flame")),
            flame,
        );
        let cells: Vec<f64> = (0..5 * 14)
            .map(|n| {
                let r = (n / 14) as f64;
                let c = (n % 14) as f64;
                ((c * 0.5 + r + t as f64 * 0.15).sin() * 0.5 + 0.5).clamp(0.0, 1.0)
            })
            .collect();
        frame.render_widget(
            Heatmap::new(&cells, 14)
                .min(Some(0.0))
                .max(Some(1.0))
                .glyph_ramp(true)
                .block(Block::bordered().title("Heat")),
            heat,
        );
        let recs = [
            LogRecord::new(LogLevel::Info, "request accepted")
                .timestamp("12:00:01")
                .target("edge"),
            LogRecord::new(LogLevel::Warn, "retry budget 60% consumed")
                .timestamp("12:00:02")
                .target("checkout"),
            LogRecord::new(LogLevel::Error, "payment gateway timeout")
                .timestamp("12:00:03")
                .target("payment"),
            LogRecord::new(LogLevel::Debug, "circuit breaker half-open")
                .timestamp("12:00:04")
                .target("inventory"),
        ];
        frame.render_widget(
            LogStream::new(&recs)
                .palette(LogPalette::default())
                .block(Block::bordered().title("Logs")),
            hl,
        );
    }
}

// ---------------------------------------------------------------------------
// Section: Feedback — animated / status widgets driven by the model tick
// ---------------------------------------------------------------------------
impl Gallery {
    fn view_feedback(&self, frame: &mut Frame<'_>, area: Rect) {
        let block =
            Block::bordered().title("Feedback · animated by the tick (press t for a toast)");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 12 || inner.height < 8 {
            return;
        }

        let [top, mid, bot] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Fill(1),
        ])
        .areas(inner);

        // Spinner + Skeleton (tick-driven), and a progressing Gauge.
        let [spin, skel, gauge] = Layout::horizontal([
            Constraint::Length(12),
            Constraint::Percentage(40),
            Constraint::Fill(1),
        ])
        .areas(top);
        let sp = Block::bordered().title("Spin");
        let sp_in = sp.inner(spin);
        frame.render_widget(sp, spin);
        if sp_in.area() > 0 {
            frame.render_widget(
                Spinner::new()
                    .tick(self.tick)
                    .style(Style::new().fg(Color::Cyan)),
                Rect::new(sp_in.x, sp_in.y, 1, 1),
            );
        }
        let sk = Block::bordered().title("Skeleton");
        let sk_in = sk.inner(skel);
        frame.render_widget(sk, skel);
        frame.render_widget(
            Skeleton::new()
                .tick(self.tick)
                .style(Style::new().fg(Color::DarkGray))
                .shimmer_style(Style::new().fg(Color::White)),
            sk_in,
        );
        frame.render_widget(
            Gauge::default()
                .ratio(((self.tick % 50) as f64) / 50.0)
                .gauge_style(Style::new().fg(Color::Green).bg(Color::Black))
                .block(Block::bordered().title("Gauge")),
            gauge,
        );

        // Alert + an Avatar/Badge/Kbd row.
        let [alert_a, badges] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Fill(1)]).areas(mid);
        frame.render_widget(
            Alert::new(AlertLevel::Info, "Heads up")
                .body("Everything here is a pure projection of the tick.")
                .info_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                .block(Block::bordered()),
            alert_a,
        );
        let bblock = Block::bordered().title("Badges/Avatar/Kbd");
        let bin = bblock.inner(badges);
        frame.render_widget(bblock, badges);
        if bin.height >= 1 {
            let cols = Layout::horizontal([
                Constraint::Length(5),
                Constraint::Length(7),
                Constraint::Fill(1),
            ])
            .split(Rect::new(bin.x, bin.y, bin.width, 1));
            frame.render_widget(
                Avatar::new("AM").style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                cols[0],
            );
            frame.render_widget(Badge::new("LIVE").level(BadgeLevel::Success), cols[1]);
            frame.render_widget(
                Kbd::new(["Ctrl", "P"]).key_style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                cols[2],
            );
        }

        // Breadcrumb + Divider + a ScrollView with its own Scrollbar.
        let [crumb_a, sv_a] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(bot);
        let crumbs = [
            Line::raw("rstui"),
            Line::raw("gallery"),
            Line::raw(SECTIONS[self.section]),
        ];
        frame.render_widget(
            Breadcrumb::new(&crumbs)
                .separator_style(Style::new().fg(Color::DarkGray))
                .emphasis_style(Style::new().fg(Color::Cyan)),
            crumb_a,
        );

        if sv_a.width >= 6 && sv_a.height >= 3 {
            let svb = Block::bordered().title("ScrollView + Scrollbar");
            let sv_in = svb.inner(sv_a);
            frame.render_widget(svb, sv_a);
            let mut content = Buffer::empty(Rect::new(0, 0, sv_in.width.max(1), 24));
            let body: String = (0..24).map(|i| format!("scrollback line {i}\n")).collect();
            Paragraph::new(body).render(content.area(), &mut content);
            let row = (self.tick % 18) as u16;
            let [view_a, bar_a] =
                Layout::horizontal([Constraint::Fill(1), Constraint::Length(1)]).areas(sv_in);
            frame.render_widget(
                ScrollView::new(&content)
                    .row_offset(row)
                    .thumb_style(Style::new().fg(Color::Cyan)),
                view_a,
            );
            frame.render_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .content_length(24)
                    .viewport_length(sv_in.height as usize)
                    .position(row as usize)
                    .thumb_style(Style::new().fg(Color::Cyan)),
                bar_a,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Section: Layout — grid / split / align / accordion / card / divider
// ---------------------------------------------------------------------------
impl Gallery {
    fn view_layout(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::bordered().title("Layout · Tab+Space toggles an accordion section");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 12 || inner.height < 8 {
            return;
        }

        let [left, right] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Fill(1)]).areas(inner);

        // Grid: 2x2 of cards.
        let grid = Grid::new(
            [Constraint::Fill(1), Constraint::Fill(1)],
            [Constraint::Fill(1), Constraint::Fill(1)],
        )
        .spacing(1)
        .block(Block::bordered().title("Grid"));
        let cells = grid.split(left);
        frame.render_widget(grid, left);
        for (r, row) in cells.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                frame.render_widget(
                    Paragraph::new(format!("cell {r},{c}"))
                        .block(Block::bordered().border_type(BorderType::Rounded)),
                    *cell,
                );
            }
        }

        let [sp_a, acc_a] =
            Layout::vertical([Constraint::Percentage(45), Constraint::Fill(1)]).areas(right);

        // SplitPane with a draggable-looking divider.
        let sp = SplitPane::horizontal(Constraint::Percentage(40))
            .block(Block::bordered().title("SplitPane"))
            .divider_style(Style::new().fg(Color::DarkGray));
        let (a, b) = sp.split(sp_a);
        frame.render_widget(sp, sp_a);
        frame.render_widget(List::new(["one", "two", "three"]), a);
        // Align centres a child on both axes inside the right split.
        let al = Align::new()
            .horizontal(Alignment::Center)
            .vertical(VerticalAlignment::Center)
            .width(Constraint::Length(9))
            .height(Constraint::Length(1));
        frame.render_widget(Paragraph::new("centred"), al.rect(b));

        // Accordion: caller-owned expanded flags; bodies are projected rects.
        let acc = Accordion::new([
            AccordionSection::new("General").expanded(self.accordion[0]),
            AccordionSection::new("Appearance").expanded(self.accordion[1]),
            AccordionSection::new("Advanced").expanded(self.accordion[2]),
        ])
        .block(Block::bordered().title("Accordion"))
        .header_style(Style::new().fg(Color::Black).bg(Color::Cyan));
        let bodies = acc.layout(acc_a);
        frame.render_widget(acc, acc_a);
        for (i, body) in bodies.iter().enumerate() {
            if let Some(b) = body {
                let card = Card::new()
                    .block(Block::bordered())
                    .header(Line::raw(format!("section {i}")))
                    .header_style(Style::new().fg(Color::Cyan));
                let cinner = card.inner(*b);
                frame.render_widget(card, *b);
                frame.render_widget(
                    Paragraph::new("body content\nwrapped to fit").wrap(Wrap { trim: false }),
                    cinner,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Overlays — palette / modal / drawer / help / popover / tooltip / toast
// ---------------------------------------------------------------------------
impl Gallery {
    fn view_overlays(&self, frame: &mut Frame<'_>, area: Rect) {
        if self.show_popover {
            // Anchor a Popover + a Tooltip to a fixed field-ish rect.
            let anchor = Rect::new(
                area.x + 2,
                area.y + 2,
                14.min(area.width.saturating_sub(2)),
                1,
            );
            if anchor.width >= 4 {
                let pop = Popover::new()
                    .side(PopoverSide::Bottom)
                    .width(20)
                    .height(3)
                    .block(Block::bordered().title("Popover"))
                    .style(Style::new().fg(Color::Black).bg(Color::Cyan));
                let pin = pop.inner(anchor, area);
                frame.render_widget(pop, anchor);
                frame.render_widget(Paragraph::new("anchored panel"), pin);

                let tip = Rect::new(area.x + 2, area.bottom().saturating_sub(3), 18, 1);
                frame.render_widget(
                    Tooltip::new("press v to hide")
                        .block(Block::bordered())
                        .style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                    tip,
                );
            }
        }

        if self.drawer_open {
            let drawer = Drawer::new()
                .open(true)
                .side(DrawerSide::Left)
                .size(Constraint::Length(20.min(area.width)))
                .block(Block::bordered().title("Drawer"))
                .style(Style::new().bg(Color::Blue).fg(Color::White))
                .backdrop_style(Style::new().fg(Color::DarkGray));
            let dinner = drawer.inner(area);
            frame.render_widget(drawer, area);
            frame.render_widget(
                List::new(["Dashboard", "Projects", "Settings"])
                    .style(Style::new().bg(Color::Blue).fg(Color::White)),
                dinner,
            );
        }

        if self.modal_open {
            let modal = Modal::new()
                .block(Block::bordered().title("Modal (Esc closes)"))
                .width(Constraint::Length(36.min(area.width)))
                .height(Constraint::Length(7.min(area.height)))
                .backdrop_style(Style::new().fg(Color::DarkGray));
            let minner = modal.inner(area);
            frame.render_widget(modal, area);
            frame.render_widget(
                Paragraph::new("This is an opaque modal dialog.\nEsc or m to dismiss.")
                    .wrap(Wrap { trim: false }),
                minner,
            );
        }

        if self.help_open {
            let entries = [
                HelpEntry::new(["Tab"], "Move focus / selection"),
                HelpEntry::new(["1", "5"], "Jump to a section"),
                HelpEntry::new(["Ctrl", "P"], "Command palette"),
                HelpEntry::new(["m"], "Toggle modal"),
                HelpEntry::new(["d"], "Toggle drawer"),
                HelpEntry::new(["Esc"], "Close / quit"),
            ];
            frame.render_widget(
                HelpOverlay::new(&entries)
                    .block(Block::bordered().title("Keybindings"))
                    .key_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .backdrop_style(Style::new().fg(Color::DarkGray)),
                area,
            );
        }

        if self.palette_open {
            let matches = self.palette_matches();
            let results: Vec<Line> = matches.iter().map(|s| Line::raw(*s)).collect();
            frame.render_widget(
                CommandPalette::new(&self.palette_query, &results)
                    .highlight(self.palette_hl.min(results.len().saturating_sub(1)))
                    .prompt("> ")
                    .block(Block::bordered().title("Command Palette"))
                    .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .backdrop_style(Style::new().fg(Color::DarkGray)),
                area,
            );
        }

        // Toasts float last, top-right, opaque.
        if !self.toasts.is_empty() {
            let msgs: Vec<ToastMessage> = self
                .toasts
                .iter()
                .map(|t| ToastMessage::new(toast_level(t.kind), t.text.as_str()))
                .collect();
            frame.render_widget(
                Toast::new(&msgs)
                    .width(Constraint::Length(34.min(area.width)))
                    .block(Block::bordered().border_type(BorderType::Rounded))
                    .info_style(Style::new().fg(Color::Cyan))
                    .success_style(Style::new().fg(Color::Green))
                    .warning_style(Style::new().fg(Color::Yellow))
                    .error_style(Style::new().fg(Color::Red)),
                area,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Scripted, deterministic tour — the same loop the live binary runs, with
// keystrokes scripted so `cargo run` prints a stable snapshot reel and the
// tests below assert it. No TTY, threads, or clock.
// ---------------------------------------------------------------------------
const W: u16 = 96;
const H: u16 = 30;

fn key(c: char) -> Event {
    Event::from(KeyEvent::char(c))
}
fn code(k: KeyCode) -> Event {
    Event::from(KeyEvent::from_code(k))
}
fn ctrl(c: char) -> Event {
    Event::from(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

fn main() {
    let mut h = Harness::new(Gallery::default(), W, H);
    println!("start — Inputs section, Name focused:\n{}", h.snapshot());

    // Type into the focused Input, then Tab to the masked field.
    for c in "ce".chars() {
        h.handle(key(c));
    }
    h.handle(code(KeyCode::Tab));
    println!("typed into Name, Tab -> Secret:\n{}", h.snapshot());

    // Jump to the Select section, move the shared selection, open the dropdown.
    h.handle(key('2'));
    h.handle(code(KeyCode::Down));
    h.handle(code(KeyCode::Down));
    h.handle(key(' '));
    println!(
        "Select section, ↓↓ then Space (dropdown open):\n{}",
        h.snapshot()
    );

    // Animate: ticks roll the sparkline / spinner / gauge on the Feedback tab.
    h.handle(key('4'));
    for _ in 0..15 {
        h.tick();
    }
    h.handle(key('t'));
    println!(
        "Feedback section after 15 ticks + a toast:\n{}",
        h.snapshot()
    );

    // Open the command palette, filter, and run a command.
    h.handle(ctrl('p'));
    for c in "modal".chars() {
        h.handle(key(c));
    }
    h.handle(code(KeyCode::Enter));
    println!(
        "palette: typed 'modal' + Enter -> modal opens:\n{}",
        h.snapshot()
    );
    h.handle(code(KeyCode::Esc)); // close modal

    // The observability section: metrics, traces, and logs together.
    h.handle(key('6'));
    for _ in 0..6 {
        h.tick();
    }
    println!(
        "Observe section — line chart / histogram / trace / flame / heatmap / logs:\n{}",
        h.snapshot()
    );

    // Resilience: shrink to a sliver, then quit.
    h.resize(3, 2);
    println!("resized 3x2 (clipped, no panic):\n{}", h.snapshot());
    h.resize(W, H);
    h.handle(key('q'));
    println!("q -> quit (running = {})", h.is_running());
    assert!(!h.is_running(), "q must quit");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> Harness<Gallery> {
        Harness::new(Gallery::default(), W, H)
    }

    #[test]
    fn initial_frame_renders_shell_without_panic() {
        let h = app();
        let snap = h.snapshot();
        assert_eq!(snap.lines().count(), H as usize);
        for line in snap.lines() {
            assert_eq!(line.chars().count(), W as usize, "row not full width");
        }
        // Tabs strip, sidebar rail, and status hints are all present.
        assert!(snap.contains("Inputs"));
        assert!(snap.contains("nav"));
        assert!(snap.contains("palette"));
        assert!(h.is_running());
    }

    #[test]
    fn typing_routes_to_the_focused_text_field() {
        let mut h = app();
        for c in "xyz".chars() {
            h.handle(key(c));
        }
        assert_eq!(h.app().name.value(), "adaxyz");
        // Tab moves focus off Name; the same keys now miss it.
        h.handle(code(KeyCode::Tab));
        h.handle(key('!'));
        assert_eq!(h.app().name.value(), "adaxyz", "Name no longer focused");
        assert!(h.app().focus.is_focused(F_SECRET));
    }

    #[test]
    fn digits_switch_sections_and_render() {
        let mut h = app();
        h.handle(key('3'));
        assert_eq!(h.app().section, 2);
        assert!(h.snapshot().contains("Markdown"));
        h.handle(key('5'));
        assert_eq!(h.app().section, 4);
        assert!(h.snapshot().contains("Accordion"));
        h.handle(key('6'));
        assert_eq!(h.app().section, 5);
        assert!(
            h.snapshot().contains("Latency"),
            "the Observe section renders the metrics line chart"
        );
    }

    #[test]
    fn ticks_roll_the_sparkline_window() {
        let mut h = app();
        let before = h.app().spark.clone();
        for _ in 0..20 {
            h.tick();
        }
        let after = &h.app().spark;
        assert_ne!(&before, after, "the tick must roll the sparkline");
        assert!(after.len() <= 40, "window stays bounded");
        assert_eq!(h.app().tick, 20);
    }

    #[test]
    fn command_palette_filters_and_executes() {
        let mut h = app();
        h.handle(ctrl('p'));
        assert!(h.app().palette_open);
        for c in "drawer".chars() {
            h.handle(key(c));
        }
        // The reducer (not the widget) filtered to the Drawer command.
        assert_eq!(h.app().palette_matches(), vec!["Toggle Drawer"]);
        assert!(h.snapshot().contains("Command Palette"));
        h.handle(code(KeyCode::Enter));
        assert!(!h.app().palette_open);
        assert!(h.app().drawer_open, "running the command opened the drawer");
    }

    #[test]
    fn toast_is_pushed_then_expires_on_a_later_tick() {
        let mut h = app();
        h.handle(key('t'));
        assert_eq!(h.app().toasts.len(), 1);
        assert!(h.snapshot().contains("Toast pushed"));
        for _ in 0..30 {
            h.tick();
        }
        assert!(h.app().toasts.is_empty(), "toast expired via the reducer");
    }

    #[test]
    fn esc_pops_overlays_then_quits() {
        let mut h = app();
        h.handle(key('m'));
        assert!(h.app().modal_open);
        h.handle(code(KeyCode::Esc)); // pops the modal, does not quit
        assert!(!h.app().modal_open);
        assert!(h.is_running());
        h.handle(code(KeyCode::Esc)); // no overlay -> quit
        assert!(!h.is_running());
    }

    #[test]
    fn tiny_and_resized_terminals_do_not_panic() {
        let mut h = app();
        for (w, hh) in [(1, 1), (3, 2), (8, 4), (20, 6), (200, 60)] {
            h.resize(w, hh);
            let snap = h.snapshot();
            assert_eq!(snap.lines().count(), hh as usize);
        }
        // Still interactive after the resize storm.
        h.resize(W, H);
        h.handle(key('2'));
        assert_eq!(h.app().section, 1);
    }
}
