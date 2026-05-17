//! The built-in theme catalogue and the public [`Theme`] handle.
//!
//! Every gpui-component theme is vendored verbatim and embedded with
//! `include_str!`, so the catalogue needs no filesystem and works in a single
//! static binary. Resolution mirrors gpui-component's registry exactly: the
//! canonical light/dark bases are `_default-theme.json` run through the
//! [`cascade`](crate::cascade) against an all-transparent default, then each
//! theme resolves its sparse overrides against the base for its mode.

use crate::cascade::ThemeColor;
use crate::palette::ThemePalette;
use crate::schema::{ThemeMode, ThemeSet};
use std::fmt;
use std::sync::OnceLock;

/// gpui-component's shadcn base theme — the light/dark palette every other
/// theme falls back to (its overrides use named scales, hence [`crate::shadcn`]).
const DEFAULT_THEME_JSON: &str = include_str!("../themes/_default-theme.json");

/// Every vendored gpui-component theme set, embedded by filename. One file is
/// a [`ThemeSet`]; a set yields one or more [`Theme`]s (light/dark variants).
const THEME_SETS: &[(&str, &str)] = &[
    ("adventure.json", include_str!("../themes/adventure.json")),
    ("alduin.json", include_str!("../themes/alduin.json")),
    ("asciinema.json", include_str!("../themes/asciinema.json")),
    ("ayu.json", include_str!("../themes/ayu.json")),
    ("catppuccin.json", include_str!("../themes/catppuccin.json")),
    ("everforest.json", include_str!("../themes/everforest.json")),
    ("fahrenheit.json", include_str!("../themes/fahrenheit.json")),
    ("flexoki.json", include_str!("../themes/flexoki.json")),
    ("gruvbox.json", include_str!("../themes/gruvbox.json")),
    ("harper.json", include_str!("../themes/harper.json")),
    ("hybrid.json", include_str!("../themes/hybrid.json")),
    ("jellybeans.json", include_str!("../themes/jellybeans.json")),
    ("kibble.json", include_str!("../themes/kibble.json")),
    (
        "macos-classic.json",
        include_str!("../themes/macos-classic.json"),
    ),
    ("matrix.json", include_str!("../themes/matrix.json")),
    (
        "mellifluous.json",
        include_str!("../themes/mellifluous.json"),
    ),
    ("molokai.json", include_str!("../themes/molokai.json")),
    ("solarized.json", include_str!("../themes/solarized.json")),
    ("spaceduck.json", include_str!("../themes/spaceduck.json")),
    ("tokyonight.json", include_str!("../themes/tokyonight.json")),
    ("twilight.json", include_str!("../themes/twilight.json")),
];

/// A failure loading a theme set from JSON (built-in catalogue or user file).
#[derive(Debug)]
pub enum ThemeError {
    /// The JSON did not match gpui-component's `ThemeSet` schema.
    Parse(serde_json::Error),
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeError::Parse(e) => write!(f, "invalid theme JSON: {e}"),
        }
    }
}

impl std::error::Error for ThemeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ThemeError::Parse(e) => Some(e),
        }
    }
}

/// The canonical light/dark base palettes, resolved once.
///
/// gpui builds these by running `_default-theme.json` through the cascade
/// against `ThemeColor::default()` (all transparent). Because the cascade and
/// the named-colour table are faithful ports, these are byte-identical to the
/// values gpui-component renders.
fn canonical_base(mode: ThemeMode) -> &'static ThemeColor {
    static BASES: OnceLock<(ThemeColor, ThemeColor)> = OnceLock::new();
    let (light, dark) = BASES.get_or_init(|| {
        let set: ThemeSet = serde_json::from_str(DEFAULT_THEME_JSON)
            .expect("vendored _default-theme.json must parse");
        let mut light = ThemeColor::default();
        let mut dark = ThemeColor::default();
        for cfg in &set.themes {
            let mut tc = ThemeColor::default();
            tc.apply_config(cfg, &ThemeColor::default());
            if cfg.mode.is_dark() {
                dark = tc;
            } else {
                light = tc;
            }
        }
        (light, dark)
    });
    match mode {
        ThemeMode::Light => light,
        ThemeMode::Dark => dark,
    }
}

/// One ready-to-use theme: identity plus its terminal-ready [`ThemePalette`].
///
/// Obtain the built-ins with [`Theme::all`] / [`Theme::by_name`], or load a
/// user theme file with [`Theme::from_set_json`]. The palette carries every
/// colour and the [`Style`](rstui_core::Style) constructors widgets consume.
#[derive(Debug, Clone)]
pub struct Theme {
    /// The owning set's name (e.g. `"Catppuccin"`); equals [`name`](Self::name)
    /// for single-theme sets.
    pub set_name: String,
    /// This theme's display name (e.g. `"Catppuccin Macchiato"`).
    pub name: String,
    /// Whether the set marked this its default theme.
    pub is_default: bool,
    /// The resolved, terminal-ready colours and style constructors.
    pub palette: ThemePalette,
}

impl Theme {
    /// Parse a gpui-component `ThemeSet` JSON document into its themes,
    /// resolving each against the matching built-in base. Use this for
    /// user-supplied theme files; the format is identical to the vendored
    /// ones.
    pub fn from_set_json(json: &str) -> Result<Vec<Theme>, ThemeError> {
        let set: ThemeSet = serde_json::from_str(json).map_err(ThemeError::Parse)?;
        let set_name = set.name;
        Ok(set
            .themes
            .into_iter()
            .map(|cfg| {
                let mut tc = ThemeColor::default();
                tc.apply_config(&cfg, canonical_base(cfg.mode));
                Theme {
                    set_name: set_name.clone(),
                    is_default: cfg.is_default,
                    palette: ThemePalette::from_theme_color(cfg.name.clone(), cfg.mode, &tc),
                    name: cfg.name,
                }
            })
            .collect())
    }

    /// Every built-in theme, sorted the way gpui-component sorts them: any
    /// set-default first, then case-insensitively by name. Each light/dark
    /// variant is its own entry, so a "pick a theme" list is just this.
    #[must_use]
    pub fn all() -> Vec<Theme> {
        let mut out = Vec::new();
        for (file, json) in THEME_SETS {
            match Theme::from_set_json(json) {
                Ok(themes) => out.extend(themes),
                // A vendored file failing to parse is a build-time defect, not
                // a runtime condition; surface it loudly in tests, skip here.
                Err(e) => debug_assert!(false, "vendored {file} failed: {e}"),
            }
        }
        out.sort_by(|a, b| {
            b.is_default
                .cmp(&a.is_default)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out
    }

    /// The built-in theme whose name matches `name` case-insensitively, if
    /// any (e.g. `Theme::by_name("Tokyo Night Storm")`).
    #[must_use]
    pub fn by_name(name: &str) -> Option<Theme> {
        Theme::all()
            .into_iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
    }

    /// gpui-component's default dark theme (the shadcn neutral base) — a safe
    /// out-of-the-box choice with no name lookup.
    #[must_use]
    pub fn default_dark() -> Theme {
        let mut tc = ThemeColor::default();
        let set: ThemeSet = serde_json::from_str(DEFAULT_THEME_JSON).expect("base parses");
        let cfg = set
            .themes
            .iter()
            .find(|t| t.mode.is_dark())
            .expect("base has a dark theme");
        tc.apply_config(cfg, canonical_base(ThemeMode::Dark));
        Theme {
            set_name: set.name,
            name: cfg.name.clone(),
            is_default: true,
            palette: ThemePalette::from_theme_color(cfg.name.clone(), ThemeMode::Dark, &tc),
        }
    }
}
