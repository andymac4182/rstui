//! The ACP client's colour theme: a compact semantic palette projected from
//! any [`rstui_theme`] theme (all 36 gpui-component themes), plus the
//! startup-resolution and persistence helpers.
//!
//! The client keeps its own small set so the chrome stays readable; this is
//! the one place colour decisions live, so picking a theme reskins the app
//! without touching every render site. Mirrors the kitchen-sink's
//! `Theme::from_palette` pattern.

use rstui_core::{Color, Modifier, Style};
use std::path::PathBuf;

/// A cohesive semantic palette for the client chrome.
#[derive(Debug, Clone)]
pub struct AcpTheme {
    /// The active theme's display name (shown in the header).
    pub name: String,
    /// Whole-screen background.
    pub bg: Color,
    /// Primary foreground text.
    pub fg: Color,
    /// De-emphasised text (hints, captions, system lines).
    pub dim: Color,
    /// Brand accent — the header bar and emphasis.
    pub accent: Color,
    /// Text drawn on top of [`accent`](Self::accent).
    pub accent_fg: Color,
    /// Panel borders / the footer base.
    pub border: Color,
    /// Selection / active-row background.
    pub sel_bg: Color,
    /// Selection / active-row foreground.
    pub sel_fg: Color,
    /// Success / positive.
    pub ok: Color,
    /// Warning / caution.
    pub warn: Color,
    /// Error / danger.
    pub err: Color,
    /// Informational.
    pub info: Color,
}

impl Default for AcpTheme {
    fn default() -> Self {
        Self::from_theme(&rstui_theme::Theme::default_dark())
    }
}

impl AcpTheme {
    /// Project a full [`rstui_theme::Theme`] onto the client's semantic set.
    #[must_use]
    pub fn from_theme(t: &rstui_theme::Theme) -> Self {
        let p = &t.palette;
        Self {
            name: t.name.clone(),
            bg: p.background,
            fg: p.foreground,
            dim: p.muted_foreground,
            accent: p.primary,
            accent_fg: p.primary_foreground,
            border: p.border,
            sel_bg: p.list_active,
            sel_fg: p.foreground,
            ok: p.success,
            warn: p.warning,
            err: p.danger,
            info: p.info,
        }
    }

    /// The whole-screen base style.
    #[must_use]
    pub fn base(&self) -> Style {
        Style::new().fg(self.fg).bg(self.bg)
    }

    /// The top bar: contrasting text on the brand accent.
    #[must_use]
    pub fn header(&self) -> Style {
        Style::new()
            .fg(self.accent_fg)
            .bg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    /// The bottom status bar.
    #[must_use]
    pub fn footer(&self) -> Style {
        Style::new().fg(self.dim).bg(self.border)
    }

    /// A selected / active row.
    #[must_use]
    pub fn selection(&self) -> Style {
        Style::new()
            .fg(self.sel_fg)
            .bg(self.sel_bg)
            .add_modifier(Modifier::BOLD)
    }

    /// De-emphasised text.
    #[must_use]
    pub fn dim_text(&self) -> Style {
        Style::new().fg(self.dim)
    }

    /// Accent (link/emphasis) text.
    #[must_use]
    pub fn accent_text(&self) -> Style {
        Style::new().fg(self.accent)
    }
}

/// Where the in-app picker persists the chosen theme name
/// (`$XDG_CONFIG_HOME` or `~/.config` → `rstui/acp-client.theme`).
#[must_use]
pub fn theme_config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".rstui"));
    base.join("rstui").join("acp-client.theme")
}

/// The theme to start in: an explicit `RSTUI_THEME` (a built-in name or a
/// path to a theme file) wins; otherwise whatever the picker last saved;
/// otherwise the default dark theme. Never fails — a bad value falls back.
#[must_use]
pub fn startup_theme() -> AcpTheme {
    if let Ok(spec) = std::env::var("RSTUI_THEME") {
        let picked = if std::path::Path::new(&spec).is_file() {
            rstui_theme::Theme::from_set_file(&spec).ok().and_then(|v| {
                v.iter()
                    .find(|t| t.is_default)
                    .or_else(|| v.first())
                    .cloned()
            })
        } else {
            rstui_theme::Theme::by_name(&spec)
        };
        if let Some(t) = picked {
            return AcpTheme::from_theme(&t);
        }
    }
    if let Some(t) = rstui_theme::Theme::read_choice(theme_config_path()) {
        return AcpTheme::from_theme(&t);
    }
    AcpTheme::default()
}
