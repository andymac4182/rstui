//! The eight content screens and the dispatch that routes input + rendering
//! to whichever one the sidebar has selected.
//!
//! Every screen owns its interactive state as a plain struct on
//! [`ScreenState`]; the active screen's handler is the only thing that
//! mutates it, exactly as the top-level [`App`](rstui_runtime::App) reducer
//! does for the shell.

pub(crate) mod colour_lab;
pub(crate) mod containers;
pub(crate) mod data_views;
pub(crate) mod feedback;
pub(crate) mod forms;
pub(crate) mod navigation;
pub(crate) mod rich_text;
pub(crate) mod welcome;

use rstui_core::{KeyCode, Position, Rect};
use rstui_runtime::Frame;
use rstui_widgets::ToastLevel;

use crate::theme::Theme;

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

/// The eight screens, in sidebar / hotkey order.
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
}

impl Screen {
    /// The screens in fixed display order; the sidebar and the `1`..`8`
    /// hotkeys both index this, so they cannot disagree.
    pub(crate) const ALL: [Screen; 8] = [
        Screen::Welcome,
        Screen::Forms,
        Screen::Navigation,
        Screen::Data,
        Screen::Feedback,
        Screen::Containers,
        Screen::RichText,
        Screen::Colour,
    ];

    /// This screen's index into [`ALL`](Self::ALL).
    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|&s| s == self).unwrap_or(0)
    }

    /// The sidebar label (with a leading glyph).
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
        }
    }
}

/// Every screen's interactive state, plus the dispatch to it.
#[derive(Debug)]
pub(crate) struct ScreenState {
    /// `Forms & Input` model.
    pub(crate) forms: forms::State,
    /// `Navigation` model.
    pub(crate) navigation: navigation::State,
    /// `Data Display` model.
    pub(crate) data: data_views::State,
    /// `Feedback` model.
    pub(crate) feedback: feedback::State,
    /// `Containers` model.
    pub(crate) containers: containers::State,
    /// `Rich Text` model.
    pub(crate) rich_text: rich_text::State,
    /// `Colour Lab` model.
    pub(crate) colour: colour_lab::State,
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
            Screen::Forms => self.forms.on_click(pos, content),
            Screen::Navigation => self.navigation.on_click(pos, content),
            Screen::Colour => self.colour.on_click(pos, content),
            Screen::RichText => self.rich_text.on_click(pos, content),
            _ => ScreenOutcome::ignored(),
        }
    }

    /// Route a wheel scroll to the active screen.
    pub(crate) fn on_scroll(&mut self, screen: Screen, up: bool) {
        match screen {
            Screen::Navigation => self.navigation.on_scroll(up),
            Screen::Containers => self.containers.on_scroll(up),
            Screen::RichText => self.rich_text.on_scroll(up),
            Screen::Data => self.data.on_scroll(up),
            _ => {}
        }
    }

    /// Route a paste to the active screen (the forms screen accepts it).
    pub(crate) fn on_paste(&mut self, screen: Screen, text: &str) {
        if screen == Screen::Forms {
            self.forms.on_paste(text);
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
        }
    }
}
