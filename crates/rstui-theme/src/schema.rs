//! The on-disk theme format: serde structs for gpui-component's `ThemeSet`
//! JSON, used verbatim for both the vendored themes and user-supplied files.
//!
//! Every colour field is an `Option<String>` keyed by gpui-component's dotted
//! JSON name (`accent.background`, `base.red.light`, …). `None` is the load-
//! bearing case: an absent key is *not* an error, it means "use the cascade's
//! derived fallback" ([`crate::cascade`]). Unknown keys (`$schema`,
//! `highlight`, font/radius metadata rstui has no use for) are ignored rather
//! than rejected, so a theme file authored for the GUI loads unchanged.

use serde::Deserialize;

/// A theme file: metadata plus one or more concrete [`ThemeConfig`]s (a
/// light/dark pair is the common shape — each becomes one selectable theme).
#[derive(Debug, Clone, Deserialize)]
pub struct ThemeSet {
    /// The set's display name (e.g. `"Catppuccin"`).
    pub name: String,
    /// Optional theme author, surfaced for attribution.
    #[serde(default)]
    pub author: Option<String>,
    /// Optional upstream URL, surfaced for attribution.
    #[serde(default)]
    pub url: Option<String>,
    /// The concrete themes this file defines.
    #[serde(default)]
    pub themes: Vec<ThemeConfig>,
}

/// Whether a [`ThemeConfig`] is a light or dark theme. Selects which built-in
/// base palette its unset colours fall back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    /// A light theme (falls back to the light base palette).
    #[default]
    Light,
    /// A dark theme (falls back to the dark base palette).
    Dark,
}

impl ThemeMode {
    /// `true` for [`ThemeMode::Dark`] — mirrors gpui's `is_dark`, which the
    /// cascade branches on (active-state darkening differs by mode).
    #[must_use]
    pub fn is_dark(self) -> bool {
        matches!(self, ThemeMode::Dark)
    }
}

/// One concrete theme: a name, a mode, and its sparse colour overrides.
#[derive(Debug, Clone, Deserialize)]
pub struct ThemeConfig {
    /// The theme's display name (e.g. `"Catppuccin Macchiato"`).
    pub name: String,
    /// Light or dark; selects the fallback base palette.
    #[serde(default)]
    pub mode: ThemeMode,
    /// Whether the set marks this as its default theme (sorted first).
    #[serde(default)]
    pub is_default: bool,
    /// The sparse colour overrides; everything unset is derived.
    #[serde(default)]
    pub colors: ThemeColorConfig,
}

/// The sparse colour overrides of a [`ThemeConfig`].
///
/// Field order is gpui-component's; the cascade in [`crate::cascade`] depends
/// on it (later fallbacks reference earlier resolved fields).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ThemeColorConfig {
    /// Override for gpui-component's `accent.background` colour (unset = derived).
    #[serde(rename = "accent.background")]
    pub accent: Option<String>,
    /// Override for gpui-component's `accent.foreground` colour (unset = derived).
    #[serde(rename = "accent.foreground")]
    pub accent_foreground: Option<String>,
    /// Override for gpui-component's `accordion.background` colour (unset = derived).
    #[serde(rename = "accordion.background")]
    pub accordion: Option<String>,
    /// Override for gpui-component's `accordion.hover.background` colour (unset = derived).
    #[serde(rename = "accordion.hover.background")]
    pub accordion_hover: Option<String>,
    /// Override for gpui-component's `background` colour (unset = derived).
    #[serde(rename = "background")]
    pub background: Option<String>,
    /// Override for gpui-component's `border` colour (unset = derived).
    #[serde(rename = "border")]
    pub border: Option<String>,
    /// Override for gpui-component's `button.primary.background` colour (unset = derived).
    #[serde(rename = "button.primary.background")]
    pub button_primary: Option<String>,
    /// Override for gpui-component's `button.primary.active.background` colour (unset = derived).
    #[serde(rename = "button.primary.active.background")]
    pub button_primary_active: Option<String>,
    /// Override for gpui-component's `button.primary.foreground` colour (unset = derived).
    #[serde(rename = "button.primary.foreground")]
    pub button_primary_foreground: Option<String>,
    /// Override for gpui-component's `button.primary.hover.background` colour (unset = derived).
    #[serde(rename = "button.primary.hover.background")]
    pub button_primary_hover: Option<String>,
    /// Override for gpui-component's `group_box.background` colour (unset = derived).
    #[serde(rename = "group_box.background")]
    pub group_box: Option<String>,
    /// Override for gpui-component's `group_box.foreground` colour (unset = derived).
    #[serde(rename = "group_box.foreground")]
    pub group_box_foreground: Option<String>,
    /// Override for gpui-component's `group_box.title.foreground` colour (unset = derived).
    #[serde(rename = "group_box.title.foreground")]
    pub group_box_title_foreground: Option<String>,
    /// Override for gpui-component's `caret` colour (unset = derived).
    #[serde(rename = "caret")]
    pub caret: Option<String>,
    /// Override for gpui-component's `chart.1` colour (unset = derived).
    #[serde(rename = "chart.1")]
    pub chart_1: Option<String>,
    /// Override for gpui-component's `chart.2` colour (unset = derived).
    #[serde(rename = "chart.2")]
    pub chart_2: Option<String>,
    /// Override for gpui-component's `chart.3` colour (unset = derived).
    #[serde(rename = "chart.3")]
    pub chart_3: Option<String>,
    /// Override for gpui-component's `chart.4` colour (unset = derived).
    #[serde(rename = "chart.4")]
    pub chart_4: Option<String>,
    /// Override for gpui-component's `chart.5` colour (unset = derived).
    #[serde(rename = "chart.5")]
    pub chart_5: Option<String>,
    /// Override for gpui-component's `chart_bullish` colour (unset = derived).
    #[serde(rename = "chart_bullish")]
    pub chart_bullish: Option<String>,
    /// Override for gpui-component's `chart_bearish` colour (unset = derived).
    #[serde(rename = "chart_bearish")]
    pub chart_bearish: Option<String>,
    /// Override for gpui-component's `danger.background` colour (unset = derived).
    #[serde(rename = "danger.background")]
    pub danger: Option<String>,
    /// Override for gpui-component's `danger.active.background` colour (unset = derived).
    #[serde(rename = "danger.active.background")]
    pub danger_active: Option<String>,
    /// Override for gpui-component's `danger.foreground` colour (unset = derived).
    #[serde(rename = "danger.foreground")]
    pub danger_foreground: Option<String>,
    /// Override for gpui-component's `danger.hover.background` colour (unset = derived).
    #[serde(rename = "danger.hover.background")]
    pub danger_hover: Option<String>,
    /// Override for gpui-component's `description_list.label.background` colour (unset = derived).
    #[serde(rename = "description_list.label.background")]
    pub description_list_label: Option<String>,
    /// Override for gpui-component's `description_list.label.foreground` colour (unset = derived).
    #[serde(rename = "description_list.label.foreground")]
    pub description_list_label_foreground: Option<String>,
    /// Override for gpui-component's `drag.border` colour (unset = derived).
    #[serde(rename = "drag.border")]
    pub drag_border: Option<String>,
    /// Override for gpui-component's `drop_target.background` colour (unset = derived).
    #[serde(rename = "drop_target.background")]
    pub drop_target: Option<String>,
    /// Override for gpui-component's `foreground` colour (unset = derived).
    #[serde(rename = "foreground")]
    pub foreground: Option<String>,
    /// Override for gpui-component's `info.background` colour (unset = derived).
    #[serde(rename = "info.background")]
    pub info: Option<String>,
    /// Override for gpui-component's `info.active.background` colour (unset = derived).
    #[serde(rename = "info.active.background")]
    pub info_active: Option<String>,
    /// Override for gpui-component's `info.foreground` colour (unset = derived).
    #[serde(rename = "info.foreground")]
    pub info_foreground: Option<String>,
    /// Override for gpui-component's `info.hover.background` colour (unset = derived).
    #[serde(rename = "info.hover.background")]
    pub info_hover: Option<String>,
    /// Override for gpui-component's `input.border` colour (unset = derived).
    #[serde(rename = "input.border")]
    pub input: Option<String>,
    /// Override for gpui-component's `link` colour (unset = derived).
    #[serde(rename = "link")]
    pub link: Option<String>,
    /// Override for gpui-component's `link.active` colour (unset = derived).
    #[serde(rename = "link.active")]
    pub link_active: Option<String>,
    /// Override for gpui-component's `link.hover` colour (unset = derived).
    #[serde(rename = "link.hover")]
    pub link_hover: Option<String>,
    /// Override for gpui-component's `list.background` colour (unset = derived).
    #[serde(rename = "list.background")]
    pub list: Option<String>,
    /// Override for gpui-component's `list.active.background` colour (unset = derived).
    #[serde(rename = "list.active.background")]
    pub list_active: Option<String>,
    /// Override for gpui-component's `list.active.border` colour (unset = derived).
    #[serde(rename = "list.active.border")]
    pub list_active_border: Option<String>,
    /// Override for gpui-component's `list.even.background` colour (unset = derived).
    #[serde(rename = "list.even.background")]
    pub list_even: Option<String>,
    /// Override for gpui-component's `list.head.background` colour (unset = derived).
    #[serde(rename = "list.head.background")]
    pub list_head: Option<String>,
    /// Override for gpui-component's `list.hover.background` colour (unset = derived).
    #[serde(rename = "list.hover.background")]
    pub list_hover: Option<String>,
    /// Override for gpui-component's `muted.background` colour (unset = derived).
    #[serde(rename = "muted.background")]
    pub muted: Option<String>,
    /// Override for gpui-component's `muted.foreground` colour (unset = derived).
    #[serde(rename = "muted.foreground")]
    pub muted_foreground: Option<String>,
    /// Override for gpui-component's `popover.background` colour (unset = derived).
    #[serde(rename = "popover.background")]
    pub popover: Option<String>,
    /// Override for gpui-component's `popover.foreground` colour (unset = derived).
    #[serde(rename = "popover.foreground")]
    pub popover_foreground: Option<String>,
    /// Override for gpui-component's `primary.background` colour (unset = derived).
    #[serde(rename = "primary.background")]
    pub primary: Option<String>,
    /// Override for gpui-component's `primary.active.background` colour (unset = derived).
    #[serde(rename = "primary.active.background")]
    pub primary_active: Option<String>,
    /// Override for gpui-component's `primary.foreground` colour (unset = derived).
    #[serde(rename = "primary.foreground")]
    pub primary_foreground: Option<String>,
    /// Override for gpui-component's `primary.hover.background` colour (unset = derived).
    #[serde(rename = "primary.hover.background")]
    pub primary_hover: Option<String>,
    /// Override for gpui-component's `progress.bar.background` colour (unset = derived).
    #[serde(rename = "progress.bar.background")]
    pub progress_bar: Option<String>,
    /// Override for gpui-component's `ring` colour (unset = derived).
    #[serde(rename = "ring")]
    pub ring: Option<String>,
    /// Override for gpui-component's `scrollbar.background` colour (unset = derived).
    #[serde(rename = "scrollbar.background")]
    pub scrollbar: Option<String>,
    /// Override for gpui-component's `scrollbar.thumb.background` colour (unset = derived).
    #[serde(rename = "scrollbar.thumb.background")]
    pub scrollbar_thumb: Option<String>,
    /// Override for gpui-component's `scrollbar.thumb.hover.background` colour (unset = derived).
    #[serde(rename = "scrollbar.thumb.hover.background")]
    pub scrollbar_thumb_hover: Option<String>,
    /// Override for gpui-component's `secondary.background` colour (unset = derived).
    #[serde(rename = "secondary.background")]
    pub secondary: Option<String>,
    /// Override for gpui-component's `secondary.active.background` colour (unset = derived).
    #[serde(rename = "secondary.active.background")]
    pub secondary_active: Option<String>,
    /// Override for gpui-component's `secondary.foreground` colour (unset = derived).
    #[serde(rename = "secondary.foreground")]
    pub secondary_foreground: Option<String>,
    /// Override for gpui-component's `secondary.hover.background` colour (unset = derived).
    #[serde(rename = "secondary.hover.background")]
    pub secondary_hover: Option<String>,
    /// Override for gpui-component's `selection.background` colour (unset = derived).
    #[serde(rename = "selection.background")]
    pub selection: Option<String>,
    /// Override for gpui-component's `sidebar.background` colour (unset = derived).
    #[serde(rename = "sidebar.background")]
    pub sidebar: Option<String>,
    /// Override for gpui-component's `sidebar.accent.background` colour (unset = derived).
    #[serde(rename = "sidebar.accent.background")]
    pub sidebar_accent: Option<String>,
    /// Override for gpui-component's `sidebar.accent.foreground` colour (unset = derived).
    #[serde(rename = "sidebar.accent.foreground")]
    pub sidebar_accent_foreground: Option<String>,
    /// Override for gpui-component's `sidebar.border` colour (unset = derived).
    #[serde(rename = "sidebar.border")]
    pub sidebar_border: Option<String>,
    /// Override for gpui-component's `sidebar.foreground` colour (unset = derived).
    #[serde(rename = "sidebar.foreground")]
    pub sidebar_foreground: Option<String>,
    /// Override for gpui-component's `sidebar.primary.background` colour (unset = derived).
    #[serde(rename = "sidebar.primary.background")]
    pub sidebar_primary: Option<String>,
    /// Override for gpui-component's `sidebar.primary.foreground` colour (unset = derived).
    #[serde(rename = "sidebar.primary.foreground")]
    pub sidebar_primary_foreground: Option<String>,
    /// Override for gpui-component's `skeleton.background` colour (unset = derived).
    #[serde(rename = "skeleton.background")]
    pub skeleton: Option<String>,
    /// Override for gpui-component's `slider.background` colour (unset = derived).
    #[serde(rename = "slider.background")]
    pub slider_bar: Option<String>,
    /// Override for gpui-component's `slider.thumb.background` colour (unset = derived).
    #[serde(rename = "slider.thumb.background")]
    pub slider_thumb: Option<String>,
    /// Override for gpui-component's `success.background` colour (unset = derived).
    #[serde(rename = "success.background")]
    pub success: Option<String>,
    /// Override for gpui-component's `success.foreground` colour (unset = derived).
    #[serde(rename = "success.foreground")]
    pub success_foreground: Option<String>,
    /// Override for gpui-component's `success.hover.background` colour (unset = derived).
    #[serde(rename = "success.hover.background")]
    pub success_hover: Option<String>,
    /// Override for gpui-component's `success.active.background` colour (unset = derived).
    #[serde(rename = "success.active.background")]
    pub success_active: Option<String>,
    /// Override for gpui-component's `switch.background` colour (unset = derived).
    #[serde(rename = "switch.background")]
    pub switch: Option<String>,
    /// Override for gpui-component's `switch.thumb.background` colour (unset = derived).
    #[serde(rename = "switch.thumb.background")]
    pub switch_thumb: Option<String>,
    /// Override for gpui-component's `tab.background` colour (unset = derived).
    #[serde(rename = "tab.background")]
    pub tab: Option<String>,
    /// Override for gpui-component's `tab.active.background` colour (unset = derived).
    #[serde(rename = "tab.active.background")]
    pub tab_active: Option<String>,
    /// Override for gpui-component's `tab.active.foreground` colour (unset = derived).
    #[serde(rename = "tab.active.foreground")]
    pub tab_active_foreground: Option<String>,
    /// Override for gpui-component's `tab_bar.background` colour (unset = derived).
    #[serde(rename = "tab_bar.background")]
    pub tab_bar: Option<String>,
    /// Override for gpui-component's `tab_bar.segmented.background` colour (unset = derived).
    #[serde(rename = "tab_bar.segmented.background")]
    pub tab_bar_segmented: Option<String>,
    /// Override for gpui-component's `tab.foreground` colour (unset = derived).
    #[serde(rename = "tab.foreground")]
    pub tab_foreground: Option<String>,
    /// Override for gpui-component's `table.background` colour (unset = derived).
    #[serde(rename = "table.background")]
    pub table: Option<String>,
    /// Override for gpui-component's `table.active.background` colour (unset = derived).
    #[serde(rename = "table.active.background")]
    pub table_active: Option<String>,
    /// Override for gpui-component's `table.active.border` colour (unset = derived).
    #[serde(rename = "table.active.border")]
    pub table_active_border: Option<String>,
    /// Override for gpui-component's `table.even.background` colour (unset = derived).
    #[serde(rename = "table.even.background")]
    pub table_even: Option<String>,
    /// Override for gpui-component's `table.head.background` colour (unset = derived).
    #[serde(rename = "table.head.background")]
    pub table_head: Option<String>,
    /// Override for gpui-component's `table.head.foreground` colour (unset = derived).
    #[serde(rename = "table.head.foreground")]
    pub table_head_foreground: Option<String>,
    /// Override for gpui-component's `table.foot.background` colour (unset = derived).
    #[serde(rename = "table.foot.background")]
    pub table_foot: Option<String>,
    /// Override for gpui-component's `table.foot.foreground` colour (unset = derived).
    #[serde(rename = "table.foot.foreground")]
    pub table_foot_foreground: Option<String>,
    /// Override for gpui-component's `table.hover.background` colour (unset = derived).
    #[serde(rename = "table.hover.background")]
    pub table_hover: Option<String>,
    /// Override for gpui-component's `table.row.border` colour (unset = derived).
    #[serde(rename = "table.row.border")]
    pub table_row_border: Option<String>,
    /// Override for gpui-component's `title_bar.background` colour (unset = derived).
    #[serde(rename = "title_bar.background")]
    pub title_bar: Option<String>,
    /// Override for gpui-component's `title_bar.border` colour (unset = derived).
    #[serde(rename = "title_bar.border")]
    pub title_bar_border: Option<String>,
    /// Override for gpui-component's `tiles.background` colour (unset = derived).
    #[serde(rename = "tiles.background")]
    pub tiles: Option<String>,
    /// Override for gpui-component's `warning.background` colour (unset = derived).
    #[serde(rename = "warning.background")]
    pub warning: Option<String>,
    /// Override for gpui-component's `warning.active.background` colour (unset = derived).
    #[serde(rename = "warning.active.background")]
    pub warning_active: Option<String>,
    /// Override for gpui-component's `warning.hover.background` colour (unset = derived).
    #[serde(rename = "warning.hover.background")]
    pub warning_hover: Option<String>,
    /// Override for gpui-component's `warning.foreground` colour (unset = derived).
    #[serde(rename = "warning.foreground")]
    pub warning_foreground: Option<String>,
    /// Override for gpui-component's `overlay` colour (unset = derived).
    #[serde(rename = "overlay")]
    pub overlay: Option<String>,
    /// Override for gpui-component's `window.border` colour (unset = derived).
    #[serde(rename = "window.border")]
    pub window_border: Option<String>,
    /// Override for gpui-component's `base.blue` colour (unset = derived).
    #[serde(rename = "base.blue")]
    pub blue: Option<String>,
    /// Override for gpui-component's `base.blue.light` colour (unset = derived).
    #[serde(rename = "base.blue.light")]
    pub blue_light: Option<String>,
    /// Override for gpui-component's `base.cyan` colour (unset = derived).
    #[serde(rename = "base.cyan")]
    pub cyan: Option<String>,
    /// Override for gpui-component's `base.cyan.light` colour (unset = derived).
    #[serde(rename = "base.cyan.light")]
    pub cyan_light: Option<String>,
    /// Override for gpui-component's `base.green` colour (unset = derived).
    #[serde(rename = "base.green")]
    pub green: Option<String>,
    /// Override for gpui-component's `base.green.light` colour (unset = derived).
    #[serde(rename = "base.green.light")]
    pub green_light: Option<String>,
    /// Override for gpui-component's `base.magenta` colour (unset = derived).
    #[serde(rename = "base.magenta")]
    pub magenta: Option<String>,
    /// Override for gpui-component's `base.magenta.light` colour (unset = derived).
    #[serde(rename = "base.magenta.light")]
    pub magenta_light: Option<String>,
    /// Override for gpui-component's `base.red` colour (unset = derived).
    #[serde(rename = "base.red")]
    pub red: Option<String>,
    /// Override for gpui-component's `base.red.light` colour (unset = derived).
    #[serde(rename = "base.red.light")]
    pub red_light: Option<String>,
    /// Override for gpui-component's `base.yellow` colour (unset = derived).
    #[serde(rename = "base.yellow")]
    pub yellow: Option<String>,
    /// Override for gpui-component's `base.yellow.light` colour (unset = derived).
    #[serde(rename = "base.yellow.light")]
    pub yellow_light: Option<String>,
}
