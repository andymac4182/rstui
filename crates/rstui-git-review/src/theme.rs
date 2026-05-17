//! git-review's colour theme: the small set of semantic styles the chrome
//! uses, projected from any [`rstui_theme`] theme (all 36 gpui-component
//! themes), plus startup-resolution and persistence helpers.
//!
//! This replaces the old fixed ANSI `palette` module: the same role names
//! (`dim`/`accent`/`graph`/`good`/`bad`/`selection`) now read a caller-owned
//! [`GrTheme`], so picking a theme reskins the whole reviewer. Mirrors the
//! kitchen-sink / ACP-client `from_theme` pattern.

use rstui_core::{Color, Modifier, Style};
use std::path::PathBuf;

/// git-review's semantic palette (one place every colour decision lives).
#[derive(Debug, Clone)]
pub struct GrTheme {
    /// The active theme's display name (shown in the status bar).
    pub name: String,
    /// Whole-pane background.
    pub bg: Color,
    /// Primary foreground text.
    pub fg: Color,
    /// De-emphasised text (gutters, dates, hints).
    pub dim_c: Color,
    /// Brand accent — focused borders, hashes, keys.
    pub accent_c: Color,
    /// The `git log --graph` DAG art.
    pub graph_c: Color,
    /// Success / additions / status messages.
    pub good_c: Color,
    /// Error / deletions / fatal load failure.
    pub bad_c: Color,
    /// Selection-bar foreground.
    pub sel_fg: Color,
    /// Selection-bar background.
    pub sel_bg: Color,
}

impl Default for GrTheme {
    fn default() -> Self {
        Self::from_theme(&rstui_theme::Theme::default_dark())
    }
}

impl GrTheme {
    /// Project a full [`rstui_theme::Theme`] onto git-review's role set.
    #[must_use]
    pub fn from_theme(t: &rstui_theme::Theme) -> Self {
        let p = &t.palette;
        Self {
            name: t.name.clone(),
            bg: p.background,
            fg: p.foreground,
            dim_c: p.muted_foreground,
            accent_c: p.primary,
            graph_c: p.accent,
            good_c: p.success,
            bad_c: p.danger,
            sel_fg: p.primary_foreground,
            sel_bg: p.primary,
        }
    }

    /// Whole-pane base style.
    #[must_use]
    pub fn base(&self) -> Style {
        Style::new().fg(self.fg).bg(self.bg)
    }

    /// De-emphasised text.
    #[must_use]
    pub fn dim(&self) -> Style {
        Style::new().fg(self.dim_c)
    }

    /// Brand accent (focused border, hashes, keys).
    #[must_use]
    pub fn accent(&self) -> Style {
        Style::new().fg(self.accent_c)
    }

    /// The commit-graph DAG art.
    #[must_use]
    pub fn graph(&self) -> Style {
        Style::new().fg(self.graph_c)
    }

    /// Success / positive.
    #[must_use]
    pub fn good(&self) -> Style {
        Style::new().fg(self.good_c)
    }

    /// Error / danger.
    #[must_use]
    pub fn bad(&self) -> Style {
        Style::new().fg(self.bad_c)
    }

    /// The selection bar.
    #[must_use]
    pub fn selection(&self) -> Style {
        Style::new()
            .fg(self.sel_fg)
            .bg(self.sel_bg)
            .add_modifier(Modifier::BOLD)
    }
}

/// Where the in-app picker persists the chosen theme name
/// (`$XDG_CONFIG_HOME` or `~/.config` → `rstui/git-review.theme`).
#[must_use]
pub fn theme_config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".rstui"));
    base.join("rstui").join("git-review.theme")
}

/// The theme to start in: an explicit `RSTUI_THEME` (a built-in name or a
/// path to a theme file) wins; otherwise the picker's saved choice;
/// otherwise the default dark theme. Never fails — a bad value falls back.
#[must_use]
pub fn startup_theme() -> GrTheme {
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
            return GrTheme::from_theme(&t);
        }
    }
    if let Some(t) = rstui_theme::Theme::read_choice(theme_config_path()) {
        return GrTheme::from_theme(&t);
    }
    GrTheme::default()
}
