//! [`ThemePalette`] — the public, terminal-ready face of a resolved theme.
//!
//! [`crate::cascade`] resolves a theme into ~110 floating-point [`Hsla`]
//! colours, several deliberately translucent. A terminal cell is opaque and
//! 24-bit at best, so this module performs the one irreversible step: every
//! colour is composited onto the theme background and reduced to an
//! [`rstui_core::Color`]. The result is a flat struct of [`Color`] fields
//! (data parity with gpui-component, so an app can build any [`Style`] it
//! needs) plus a curated set of [`Style`] constructors for the wiring rstui
//! widgets actually expect — the same "thread a `Style` into the builder"
//! shape `DiffTheme`/`MarkdownTheme` and the kitchen-sink already use.

use crate::cascade::ThemeColor;
use crate::hsla::Hsla;
use crate::schema::ThemeMode;
use rstui_core::{Color, Modifier, Style};

/// Every theme colour as an opaque terminal [`Color`], plus the theme's
/// identity. Fields mirror gpui-component's `ThemeColor` one-to-one (already
/// composited over [`background`](Self::background)); the methods cover the
/// common widget-styling cases so callers rarely touch raw fields.
#[derive(Debug, Clone)]
pub struct ThemePalette {
    /// The theme's display name, e.g. `"Catppuccin Macchiato"`.
    pub name: String,
    /// Whether this is a light or dark theme.
    pub mode: ThemeMode,
    /// Used for accents such as hover background on MenuItem, ListItem, etc.
    pub accent: Color,
    /// Used for accent text color.
    pub accent_foreground: Color,
    /// Accordion background color.
    pub accordion: Color,
    /// Accordion hover background color.
    pub accordion_hover: Color,
    /// Default background color.
    pub background: Color,
    /// Default border color.
    pub border: Color,
    /// Button primary background color, fallback to `primary`.
    pub button_primary: Color,
    /// Button primary active background color, fallback to `primary_active`.
    pub button_primary_active: Color,
    /// Button primary text color, fallback to `primary_foreground`.
    pub button_primary_foreground: Color,
    /// Button primary hover background color, fallback to `primary_hover`.
    pub button_primary_hover: Color,
    /// Background color for GroupBox.
    pub group_box: Color,
    /// Text color for GroupBox.
    pub group_box_foreground: Color,
    /// Input caret color (Blinking cursor).
    pub caret: Color,
    /// Chart 1 color.
    pub chart_1: Color,
    /// Chart 2 color.
    pub chart_2: Color,
    /// Chart 3 color.
    pub chart_3: Color,
    /// Chart 4 color.
    pub chart_4: Color,
    /// Chart 5 color.
    pub chart_5: Color,
    /// Bullish color for candlestick charts (upward price movement).
    pub chart_bullish: Color,
    /// Bearish color for candlestick charts (downward price movement).
    pub chart_bearish: Color,
    /// Danger background color.
    pub danger: Color,
    /// Danger active background color.
    pub danger_active: Color,
    /// Danger text color.
    pub danger_foreground: Color,
    /// Danger hover background color.
    pub danger_hover: Color,
    /// Description List label background color.
    pub description_list_label: Color,
    /// Description List label foreground color.
    pub description_list_label_foreground: Color,
    /// Drag border color.
    pub drag_border: Color,
    /// Drop target background color.
    pub drop_target: Color,
    /// Default text color.
    pub foreground: Color,
    /// Info background color.
    pub info: Color,
    /// Info active background color.
    pub info_active: Color,
    /// Info text color.
    pub info_foreground: Color,
    /// Info hover background color.
    pub info_hover: Color,
    /// Border color for inputs such as Input, Select, etc.
    pub input: Color,
    /// Link text color.
    pub link: Color,
    /// Active link text color.
    pub link_active: Color,
    /// Hover link text color.
    pub link_hover: Color,
    /// Background color for List and ListItem.
    pub list: Color,
    /// Background color for active ListItem.
    pub list_active: Color,
    /// Border color for active ListItem.
    pub list_active_border: Color,
    /// Stripe background color for even ListItem.
    pub list_even: Color,
    /// Background color for List header.
    pub list_head: Color,
    /// Hover background color for ListItem.
    pub list_hover: Color,
    /// Muted backgrounds such as Skeleton and Switch.
    pub muted: Color,
    /// Muted text color, as used in disabled text.
    pub muted_foreground: Color,
    /// Background color for Popover.
    pub popover: Color,
    /// Text color for Popover.
    pub popover_foreground: Color,
    /// Primary background color.
    pub primary: Color,
    /// Active primary background color.
    pub primary_active: Color,
    /// Primary text color.
    pub primary_foreground: Color,
    /// Hover primary background color.
    pub primary_hover: Color,
    /// Progress bar background color.
    pub progress_bar: Color,
    /// Used for focus ring.
    pub ring: Color,
    /// Scrollbar background color.
    pub scrollbar: Color,
    /// Scrollbar thumb background color.
    pub scrollbar_thumb: Color,
    /// Scrollbar thumb hover background color.
    pub scrollbar_thumb_hover: Color,
    /// Secondary background color.
    pub secondary: Color,
    /// Active secondary background color.
    pub secondary_active: Color,
    /// Secondary text color, used for secondary Button text color or secondary text.
    pub secondary_foreground: Color,
    /// Hover secondary background color.
    pub secondary_hover: Color,
    /// Input selection background color.
    pub selection: Color,
    /// Sidebar background color.
    pub sidebar: Color,
    /// Sidebar accent background color.
    pub sidebar_accent: Color,
    /// Sidebar accent text color.
    pub sidebar_accent_foreground: Color,
    /// Sidebar border color.
    pub sidebar_border: Color,
    /// Sidebar text color.
    pub sidebar_foreground: Color,
    /// Sidebar primary background color.
    pub sidebar_primary: Color,
    /// Sidebar primary text color.
    pub sidebar_primary_foreground: Color,
    /// Skeleton background color.
    pub skeleton: Color,
    /// Slider bar background color.
    pub slider_bar: Color,
    /// Slider thumb background color.
    pub slider_thumb: Color,
    /// Success background color.
    pub success: Color,
    /// Success text color.
    pub success_foreground: Color,
    /// Success hover background color.
    pub success_hover: Color,
    /// Success active background color.
    pub success_active: Color,
    /// Switch background color.
    pub switch: Color,
    /// Switch thumb background color.
    pub switch_thumb: Color,
    /// Tab background color.
    pub tab: Color,
    /// Tab active background color.
    pub tab_active: Color,
    /// Tab active text color.
    pub tab_active_foreground: Color,
    /// TabBar background color.
    pub tab_bar: Color,
    /// TabBar segmented background color.
    pub tab_bar_segmented: Color,
    /// Tab text color.
    pub tab_foreground: Color,
    /// Table background color.
    pub table: Color,
    /// Table active item background color.
    pub table_active: Color,
    /// Table active item border color.
    pub table_active_border: Color,
    /// Stripe background color for even TableRow.
    pub table_even: Color,
    /// Table head background color.
    pub table_head: Color,
    /// Table head text color.
    pub table_head_foreground: Color,
    /// Table footer background color.
    pub table_foot: Color,
    /// Table footer text color.
    pub table_foot_foreground: Color,
    /// Table item hover background color.
    pub table_hover: Color,
    /// Table row border color.
    pub table_row_border: Color,
    /// TitleBar background color, use for Window title bar.
    pub title_bar: Color,
    /// TitleBar border color.
    pub title_bar_border: Color,
    /// Background color for Tiles.
    pub tiles: Color,
    /// Warning background color.
    pub warning: Color,
    /// Warning active background color.
    pub warning_active: Color,
    /// Warning hover background color.
    pub warning_hover: Color,
    /// Warning foreground color.
    pub warning_foreground: Color,
    /// Overlay background color.
    pub overlay: Color,
    /// Window border color. # Platform specific: This is only works on Linux, other platforms we can't change the window border color.
    pub window_border: Color,
    /// The base red color.
    pub red: Color,
    /// The base red light color.
    pub red_light: Color,
    /// The base green color.
    pub green: Color,
    /// The base green light color.
    pub green_light: Color,
    /// The base blue color.
    pub blue: Color,
    /// The base blue light color.
    pub blue_light: Color,
    /// The base yellow color.
    pub yellow: Color,
    /// The base yellow light color.
    pub yellow_light: Color,
    /// The base magenta color.
    pub magenta: Color,
    /// The base magenta light color.
    pub magenta_light: Color,
    /// The base cyan color.
    pub cyan: Color,
    /// The base cyan light color.
    pub cyan_light: Color,
}

impl ThemePalette {
    /// Composite a resolved [`ThemeColor`] onto its own background and reduce
    /// every channel to a terminal [`Color`]. This is the sole bridge from
    /// the floating-point cascade to what a backend can emit.
    pub(crate) fn from_theme_color(name: String, mode: ThemeMode, tc: &ThemeColor) -> Self {
        let bg = Hsla {
            a: 1.0,
            ..tc.background
        };
        Self {
            name,
            mode,
            accent: tc.accent.over(bg),
            accent_foreground: tc.accent_foreground.over(bg),
            accordion: tc.accordion.over(bg),
            accordion_hover: tc.accordion_hover.over(bg),
            background: tc.background.over(bg),
            border: tc.border.over(bg),
            button_primary: tc.button_primary.over(bg),
            button_primary_active: tc.button_primary_active.over(bg),
            button_primary_foreground: tc.button_primary_foreground.over(bg),
            button_primary_hover: tc.button_primary_hover.over(bg),
            group_box: tc.group_box.over(bg),
            group_box_foreground: tc.group_box_foreground.over(bg),
            caret: tc.caret.over(bg),
            chart_1: tc.chart_1.over(bg),
            chart_2: tc.chart_2.over(bg),
            chart_3: tc.chart_3.over(bg),
            chart_4: tc.chart_4.over(bg),
            chart_5: tc.chart_5.over(bg),
            chart_bullish: tc.chart_bullish.over(bg),
            chart_bearish: tc.chart_bearish.over(bg),
            danger: tc.danger.over(bg),
            danger_active: tc.danger_active.over(bg),
            danger_foreground: tc.danger_foreground.over(bg),
            danger_hover: tc.danger_hover.over(bg),
            description_list_label: tc.description_list_label.over(bg),
            description_list_label_foreground: tc.description_list_label_foreground.over(bg),
            drag_border: tc.drag_border.over(bg),
            drop_target: tc.drop_target.over(bg),
            foreground: tc.foreground.over(bg),
            info: tc.info.over(bg),
            info_active: tc.info_active.over(bg),
            info_foreground: tc.info_foreground.over(bg),
            info_hover: tc.info_hover.over(bg),
            input: tc.input.over(bg),
            link: tc.link.over(bg),
            link_active: tc.link_active.over(bg),
            link_hover: tc.link_hover.over(bg),
            list: tc.list.over(bg),
            list_active: tc.list_active.over(bg),
            list_active_border: tc.list_active_border.over(bg),
            list_even: tc.list_even.over(bg),
            list_head: tc.list_head.over(bg),
            list_hover: tc.list_hover.over(bg),
            muted: tc.muted.over(bg),
            muted_foreground: tc.muted_foreground.over(bg),
            popover: tc.popover.over(bg),
            popover_foreground: tc.popover_foreground.over(bg),
            primary: tc.primary.over(bg),
            primary_active: tc.primary_active.over(bg),
            primary_foreground: tc.primary_foreground.over(bg),
            primary_hover: tc.primary_hover.over(bg),
            progress_bar: tc.progress_bar.over(bg),
            ring: tc.ring.over(bg),
            scrollbar: tc.scrollbar.over(bg),
            scrollbar_thumb: tc.scrollbar_thumb.over(bg),
            scrollbar_thumb_hover: tc.scrollbar_thumb_hover.over(bg),
            secondary: tc.secondary.over(bg),
            secondary_active: tc.secondary_active.over(bg),
            secondary_foreground: tc.secondary_foreground.over(bg),
            secondary_hover: tc.secondary_hover.over(bg),
            selection: tc.selection.over(bg),
            sidebar: tc.sidebar.over(bg),
            sidebar_accent: tc.sidebar_accent.over(bg),
            sidebar_accent_foreground: tc.sidebar_accent_foreground.over(bg),
            sidebar_border: tc.sidebar_border.over(bg),
            sidebar_foreground: tc.sidebar_foreground.over(bg),
            sidebar_primary: tc.sidebar_primary.over(bg),
            sidebar_primary_foreground: tc.sidebar_primary_foreground.over(bg),
            skeleton: tc.skeleton.over(bg),
            slider_bar: tc.slider_bar.over(bg),
            slider_thumb: tc.slider_thumb.over(bg),
            success: tc.success.over(bg),
            success_foreground: tc.success_foreground.over(bg),
            success_hover: tc.success_hover.over(bg),
            success_active: tc.success_active.over(bg),
            switch: tc.switch.over(bg),
            switch_thumb: tc.switch_thumb.over(bg),
            tab: tc.tab.over(bg),
            tab_active: tc.tab_active.over(bg),
            tab_active_foreground: tc.tab_active_foreground.over(bg),
            tab_bar: tc.tab_bar.over(bg),
            tab_bar_segmented: tc.tab_bar_segmented.over(bg),
            tab_foreground: tc.tab_foreground.over(bg),
            table: tc.table.over(bg),
            table_active: tc.table_active.over(bg),
            table_active_border: tc.table_active_border.over(bg),
            table_even: tc.table_even.over(bg),
            table_head: tc.table_head.over(bg),
            table_head_foreground: tc.table_head_foreground.over(bg),
            table_foot: tc.table_foot.over(bg),
            table_foot_foreground: tc.table_foot_foreground.over(bg),
            table_hover: tc.table_hover.over(bg),
            table_row_border: tc.table_row_border.over(bg),
            title_bar: tc.title_bar.over(bg),
            title_bar_border: tc.title_bar_border.over(bg),
            tiles: tc.tiles.over(bg),
            warning: tc.warning.over(bg),
            warning_active: tc.warning_active.over(bg),
            warning_hover: tc.warning_hover.over(bg),
            warning_foreground: tc.warning_foreground.over(bg),
            overlay: tc.overlay.over(bg),
            window_border: tc.window_border.over(bg),
            red: tc.red.over(bg),
            red_light: tc.red_light.over(bg),
            green: tc.green.over(bg),
            green_light: tc.green_light.over(bg),
            blue: tc.blue.over(bg),
            blue_light: tc.blue_light.over(bg),
            yellow: tc.yellow.over(bg),
            yellow_light: tc.yellow_light.over(bg),
            magenta: tc.magenta.over(bg),
            magenta_light: tc.magenta_light.over(bg),
            cyan: tc.cyan.over(bg),
            cyan_light: tc.cyan_light.over(bg),
        }
    }

    /// `true` for a dark theme.
    #[must_use]
    pub fn is_dark(&self) -> bool {
        self.mode.is_dark()
    }

    // --- Style constructors: the shapes rstui widgets actually consume. ---

    /// The app's root style: default text on the theme background. Use for the
    /// terminal-clearing base layer every screen draws onto.
    #[must_use]
    pub fn screen(&self) -> Style {
        Style::new().fg(self.foreground).bg(self.background)
    }

    /// Primary body text (no background — composes over whatever is beneath).
    #[must_use]
    pub fn text(&self) -> Style {
        Style::new().fg(self.foreground)
    }

    /// De-emphasised text (hints, disabled, secondary captions).
    #[must_use]
    pub fn dim_text(&self) -> Style {
        Style::new().fg(self.muted_foreground)
    }

    /// A raised surface (popovers, cards, dialogs): body text on the popover
    /// background.
    #[must_use]
    pub fn surface(&self) -> Style {
        Style::new().fg(self.popover_foreground).bg(self.popover)
    }

    /// A plain border (the common `Block::border_style` argument).
    #[must_use]
    pub fn border_style(&self) -> Style {
        Style::new().fg(self.border)
    }

    /// The focus ring — a focused input/button border, gpui's `ring`.
    #[must_use]
    pub fn focus_ring(&self) -> Style {
        Style::new().fg(self.ring)
    }

    /// The selected row/item highlight (e.g. `List::highlight_style`):
    /// foreground over the subtle, alpha-clamped active tint.
    #[must_use]
    pub fn selection(&self) -> Style {
        Style::new().fg(self.foreground).bg(self.list_active)
    }

    /// Selected text inside an input/editor (gpui's `selection`).
    #[must_use]
    pub fn text_selection(&self) -> Style {
        Style::new().fg(self.foreground).bg(self.selection)
    }

    /// A primary (call-to-action) button face.
    #[must_use]
    pub fn button_primary(&self) -> Style {
        Style::new()
            .fg(self.primary_foreground)
            .bg(self.primary)
            .add_modifier(Modifier::BOLD)
    }

    /// A secondary / neutral button face.
    #[must_use]
    pub fn button_secondary(&self) -> Style {
        Style::new()
            .fg(self.secondary_foreground)
            .bg(self.secondary)
    }

    /// An accent style (active tab, link-ish emphasis): accent text.
    #[must_use]
    pub fn accent_text(&self) -> Style {
        Style::new().fg(self.accent_foreground)
    }

    /// A hyperlink.
    #[must_use]
    pub fn link_style(&self) -> Style {
        Style::new()
            .fg(self.link)
            .add_modifier(Modifier::UNDERLINED)
    }

    /// Informational status text (the `info` accent as foreground).
    #[must_use]
    pub fn info_text(&self) -> Style {
        Style::new().fg(self.info)
    }

    /// Success status text.
    #[must_use]
    pub fn success_text(&self) -> Style {
        Style::new().fg(self.success)
    }

    /// Warning status text.
    #[must_use]
    pub fn warning_text(&self) -> Style {
        Style::new().fg(self.warning)
    }

    /// Danger / error status text.
    #[must_use]
    pub fn danger_text(&self) -> Style {
        Style::new().fg(self.danger)
    }
}
