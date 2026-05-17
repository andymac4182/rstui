//! The kitchen-sink colour theme.
//!
//! Every colour is 24-bit [`Color::Rgb`] truecolor, so the app is a live
//! demonstration of full-colour support, and the whole palette is swappable
//! at runtime (the settings [`Drawer`](rstui_widgets::Drawer) toggles
//! [`Mode`]) to prove the colour path reflows end to end.

use rstui_core::{Color, Modifier, Style};

/// Which palette is active. The settings drawer flips this live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Mode {
    /// The default dark palette.
    #[default]
    Dark,
    /// A light palette, to prove the colour path reflows on a runtime swap.
    Light,
}

impl Mode {
    /// The other mode — what toggling produces.
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    /// A short human label for the status bar / drawer.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

/// A cohesive 24-bit colour palette plus the styles the chrome and screens
/// reuse, so colour decisions live in exactly one place.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Theme {
    /// Which palette this is.
    pub(crate) mode: Mode,
    /// The whole-screen background.
    pub(crate) base: Color,
    /// Panel / card surface, one step off [`base`](Self::base).
    pub(crate) surface: Color,
    /// Raised surface (sidebar, status bar).
    pub(crate) raised: Color,
    /// Primary foreground text.
    pub(crate) text: Color,
    /// De-emphasised text (hints, captions).
    pub(crate) dim: Color,
    /// The brand accent (selection, focus, links).
    pub(crate) accent: Color,
    /// A secondary accent for variety in the data screens.
    pub(crate) accent_alt: Color,
    /// Unfocused borders.
    pub(crate) border: Color,
    /// Success / positive.
    pub(crate) ok: Color,
    /// Warning / caution.
    pub(crate) warn: Color,
    /// Error / danger.
    pub(crate) err: Color,
    /// Informational.
    pub(crate) info: Color,
}

impl Theme {
    /// The palette for `mode`.
    pub(crate) fn new(mode: Mode) -> Self {
        match mode {
            Mode::Dark => Self {
                mode,
                base: Color::Rgb(13, 17, 23),
                surface: Color::Rgb(22, 27, 34),
                raised: Color::Rgb(33, 38, 45),
                text: Color::Rgb(230, 237, 243),
                dim: Color::Rgb(125, 133, 144),
                accent: Color::Rgb(88, 166, 255),
                accent_alt: Color::Rgb(188, 140, 255),
                border: Color::Rgb(48, 54, 61),
                ok: Color::Rgb(63, 185, 80),
                warn: Color::Rgb(210, 153, 34),
                err: Color::Rgb(248, 81, 73),
                info: Color::Rgb(88, 166, 255),
            },
            Mode::Light => Self {
                mode,
                base: Color::Rgb(255, 255, 255),
                surface: Color::Rgb(246, 248, 250),
                raised: Color::Rgb(234, 238, 242),
                text: Color::Rgb(31, 35, 40),
                dim: Color::Rgb(101, 109, 118),
                accent: Color::Rgb(9, 105, 218),
                accent_alt: Color::Rgb(130, 80, 223),
                border: Color::Rgb(208, 215, 222),
                ok: Color::Rgb(26, 127, 55),
                warn: Color::Rgb(154, 103, 0),
                err: Color::Rgb(209, 36, 47),
                info: Color::Rgb(9, 105, 218),
            },
        }
    }

    /// The whole-screen background fill style.
    pub(crate) fn screen(&self) -> Style {
        Style::new().fg(self.text).bg(self.base)
    }

    /// Plain body text on a panel surface.
    pub(crate) fn body(&self) -> Style {
        Style::new().fg(self.text).bg(self.surface)
    }

    /// De-emphasised caption text on a panel surface.
    pub(crate) fn caption(&self) -> Style {
        Style::new().fg(self.dim).bg(self.surface)
    }

    /// A bold accent heading.
    pub(crate) fn heading(&self) -> Style {
        Style::new()
            .fg(self.accent)
            .bg(self.surface)
            .add_modifier(Modifier::BOLD)
    }

    /// An unfocused panel border.
    pub(crate) fn border(&self) -> Style {
        Style::new().fg(self.border).bg(self.surface)
    }

    /// A focused panel border (bright accent, bold).
    pub(crate) fn border_focused(&self) -> Style {
        Style::new()
            .fg(self.accent)
            .bg(self.surface)
            .add_modifier(Modifier::BOLD)
    }

    /// The selection / highlight bar.
    pub(crate) fn selection(&self) -> Style {
        Style::new()
            .fg(self.base)
            .bg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    /// A keyboard-focused input field cue.
    pub(crate) fn focus_field(&self) -> Style {
        Style::new().fg(self.base).bg(self.accent)
    }

    /// The brand accent as a foreground only (links, emphasis).
    pub(crate) fn accent_text(&self) -> Style {
        Style::new()
            .fg(self.accent)
            .bg(self.surface)
            .add_modifier(Modifier::BOLD)
    }
}
