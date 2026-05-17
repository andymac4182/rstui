//! The colour cascade — gpui-component's `apply_config` ported field-for-field.
//!
//! This is the fidelity-critical core. A gpui theme JSON sets only a handful of
//! the ~110 semantic colours; every other colour is *derived* by compositing
//! already-resolved ones (`background.blend(primary.opacity(0.9))`,
//! `primary.darken(0.2)`), and the derivation differs by light/dark mode. To
//! render a theme the way its author sees it in the GUI, that exact chain — the
//! same operations, the same order, the same mode-dependent constants, the same
//! final alpha clamps — has to run here. The order of statements below is load-
//! bearing: each fallback reads fields resolved by the lines above it.
//!
//! Faithfulness note: gpui's macro, in its *fallback* arm, leaves a field
//! untouched if an override is present but unparseable (a latent upstream bug).
//! Every vendored theme is valid hex, so that arm never triggers for them and
//! behaviour is identical; for arbitrary user JSON this port falls back to the
//! derived value instead of a stray zero, which is strictly safer and never
//! observable on real themes.

use crate::hsla::Hsla;
use crate::schema::ThemeConfig;
use crate::shadcn::try_parse_color;

/// Every resolved semantic colour of a theme, in gpui-component's field order.
///
/// Produced by [`ThemeColor::apply_config`]; consumed by
/// [`crate::palette::ThemePalette`], which composites each colour onto the
/// background and reduces it to a terminal [`rstui_core::Color`].
#[derive(Debug, Clone, Copy)]
pub struct ThemeColor {
    /// Used for accents such as hover background on MenuItem, ListItem, etc.
    pub accent: Hsla,
    /// Used for accent text color.
    pub accent_foreground: Hsla,
    /// Accordion background color.
    pub accordion: Hsla,
    /// Accordion hover background color.
    pub accordion_hover: Hsla,
    /// Default background color.
    pub background: Hsla,
    /// Default border color.
    pub border: Hsla,
    /// Button primary background color, fallback to `primary`.
    pub button_primary: Hsla,
    /// Button primary active background color, fallback to `primary_active`.
    pub button_primary_active: Hsla,
    /// Button primary text color, fallback to `primary_foreground`.
    pub button_primary_foreground: Hsla,
    /// Button primary hover background color, fallback to `primary_hover`.
    pub button_primary_hover: Hsla,
    /// Background color for GroupBox.
    pub group_box: Hsla,
    /// Text color for GroupBox.
    pub group_box_foreground: Hsla,
    /// Input caret color (Blinking cursor).
    pub caret: Hsla,
    /// Chart 1 color.
    pub chart_1: Hsla,
    /// Chart 2 color.
    pub chart_2: Hsla,
    /// Chart 3 color.
    pub chart_3: Hsla,
    /// Chart 4 color.
    pub chart_4: Hsla,
    /// Chart 5 color.
    pub chart_5: Hsla,
    /// Bullish color for candlestick charts (upward price movement).
    pub chart_bullish: Hsla,
    /// Bearish color for candlestick charts (downward price movement).
    pub chart_bearish: Hsla,
    /// Danger background color.
    pub danger: Hsla,
    /// Danger active background color.
    pub danger_active: Hsla,
    /// Danger text color.
    pub danger_foreground: Hsla,
    /// Danger hover background color.
    pub danger_hover: Hsla,
    /// Description List label background color.
    pub description_list_label: Hsla,
    /// Description List label foreground color.
    pub description_list_label_foreground: Hsla,
    /// Drag border color.
    pub drag_border: Hsla,
    /// Drop target background color.
    pub drop_target: Hsla,
    /// Default text color.
    pub foreground: Hsla,
    /// Info background color.
    pub info: Hsla,
    /// Info active background color.
    pub info_active: Hsla,
    /// Info text color.
    pub info_foreground: Hsla,
    /// Info hover background color.
    pub info_hover: Hsla,
    /// Border color for inputs such as Input, Select, etc.
    pub input: Hsla,
    /// Link text color.
    pub link: Hsla,
    /// Active link text color.
    pub link_active: Hsla,
    /// Hover link text color.
    pub link_hover: Hsla,
    /// Background color for List and ListItem.
    pub list: Hsla,
    /// Background color for active ListItem.
    pub list_active: Hsla,
    /// Border color for active ListItem.
    pub list_active_border: Hsla,
    /// Stripe background color for even ListItem.
    pub list_even: Hsla,
    /// Background color for List header.
    pub list_head: Hsla,
    /// Hover background color for ListItem.
    pub list_hover: Hsla,
    /// Muted backgrounds such as Skeleton and Switch.
    pub muted: Hsla,
    /// Muted text color, as used in disabled text.
    pub muted_foreground: Hsla,
    /// Background color for Popover.
    pub popover: Hsla,
    /// Text color for Popover.
    pub popover_foreground: Hsla,
    /// Primary background color.
    pub primary: Hsla,
    /// Active primary background color.
    pub primary_active: Hsla,
    /// Primary text color.
    pub primary_foreground: Hsla,
    /// Hover primary background color.
    pub primary_hover: Hsla,
    /// Progress bar background color.
    pub progress_bar: Hsla,
    /// Used for focus ring.
    pub ring: Hsla,
    /// Scrollbar background color.
    pub scrollbar: Hsla,
    /// Scrollbar thumb background color.
    pub scrollbar_thumb: Hsla,
    /// Scrollbar thumb hover background color.
    pub scrollbar_thumb_hover: Hsla,
    /// Secondary background color.
    pub secondary: Hsla,
    /// Active secondary background color.
    pub secondary_active: Hsla,
    /// Secondary text color, used for secondary Button text color or secondary text.
    pub secondary_foreground: Hsla,
    /// Hover secondary background color.
    pub secondary_hover: Hsla,
    /// Input selection background color.
    pub selection: Hsla,
    /// Sidebar background color.
    pub sidebar: Hsla,
    /// Sidebar accent background color.
    pub sidebar_accent: Hsla,
    /// Sidebar accent text color.
    pub sidebar_accent_foreground: Hsla,
    /// Sidebar border color.
    pub sidebar_border: Hsla,
    /// Sidebar text color.
    pub sidebar_foreground: Hsla,
    /// Sidebar primary background color.
    pub sidebar_primary: Hsla,
    /// Sidebar primary text color.
    pub sidebar_primary_foreground: Hsla,
    /// Skeleton background color.
    pub skeleton: Hsla,
    /// Slider bar background color.
    pub slider_bar: Hsla,
    /// Slider thumb background color.
    pub slider_thumb: Hsla,
    /// Success background color.
    pub success: Hsla,
    /// Success text color.
    pub success_foreground: Hsla,
    /// Success hover background color.
    pub success_hover: Hsla,
    /// Success active background color.
    pub success_active: Hsla,
    /// Switch background color.
    pub switch: Hsla,
    /// Switch thumb background color.
    pub switch_thumb: Hsla,
    /// Tab background color.
    pub tab: Hsla,
    /// Tab active background color.
    pub tab_active: Hsla,
    /// Tab active text color.
    pub tab_active_foreground: Hsla,
    /// TabBar background color.
    pub tab_bar: Hsla,
    /// TabBar segmented background color.
    pub tab_bar_segmented: Hsla,
    /// Tab text color.
    pub tab_foreground: Hsla,
    /// Table background color.
    pub table: Hsla,
    /// Table active item background color.
    pub table_active: Hsla,
    /// Table active item border color.
    pub table_active_border: Hsla,
    /// Stripe background color for even TableRow.
    pub table_even: Hsla,
    /// Table head background color.
    pub table_head: Hsla,
    /// Table head text color.
    pub table_head_foreground: Hsla,
    /// Table footer background color.
    pub table_foot: Hsla,
    /// Table footer text color.
    pub table_foot_foreground: Hsla,
    /// Table item hover background color.
    pub table_hover: Hsla,
    /// Table row border color.
    pub table_row_border: Hsla,
    /// TitleBar background color, use for Window title bar.
    pub title_bar: Hsla,
    /// TitleBar border color.
    pub title_bar_border: Hsla,
    /// Background color for Tiles.
    pub tiles: Hsla,
    /// Warning background color.
    pub warning: Hsla,
    /// Warning active background color.
    pub warning_active: Hsla,
    /// Warning hover background color.
    pub warning_hover: Hsla,
    /// Warning foreground color.
    pub warning_foreground: Hsla,
    /// Overlay background color.
    pub overlay: Hsla,
    /// Window border color. # Platform specific: This is only works on Linux, other platforms we can't change the window border color.
    pub window_border: Hsla,
    /// The base red color.
    pub red: Hsla,
    /// The base red light color.
    pub red_light: Hsla,
    /// The base green color.
    pub green: Hsla,
    /// The base green light color.
    pub green_light: Hsla,
    /// The base blue color.
    pub blue: Hsla,
    /// The base blue light color.
    pub blue_light: Hsla,
    /// The base yellow color.
    pub yellow: Hsla,
    /// The base yellow light color.
    pub yellow_light: Hsla,
    /// The base magenta color.
    pub magenta: Hsla,
    /// The base magenta light color.
    pub magenta_light: Hsla,
    /// The base cyan color.
    pub cyan: Hsla,
    /// The base cyan light color.
    pub cyan_light: Hsla,
}

impl Default for ThemeColor {
    /// gpui's `ThemeColor::default()`: every field fully-transparent black.
    /// The canonical light/dark bases are this, resolved through
    /// [`apply_config`](ThemeColor::apply_config) against itself — exactly how
    /// gpui-component's registry builds them.
    fn default() -> Self {
        const ZERO: Hsla = Hsla {
            h: 0.0,
            s: 0.0,
            l: 0.0,
            a: 0.0,
        };
        Self {
            accent: ZERO,
            accent_foreground: ZERO,
            accordion: ZERO,
            accordion_hover: ZERO,
            background: ZERO,
            border: ZERO,
            button_primary: ZERO,
            button_primary_active: ZERO,
            button_primary_foreground: ZERO,
            button_primary_hover: ZERO,
            group_box: ZERO,
            group_box_foreground: ZERO,
            caret: ZERO,
            chart_1: ZERO,
            chart_2: ZERO,
            chart_3: ZERO,
            chart_4: ZERO,
            chart_5: ZERO,
            chart_bullish: ZERO,
            chart_bearish: ZERO,
            danger: ZERO,
            danger_active: ZERO,
            danger_foreground: ZERO,
            danger_hover: ZERO,
            description_list_label: ZERO,
            description_list_label_foreground: ZERO,
            drag_border: ZERO,
            drop_target: ZERO,
            foreground: ZERO,
            info: ZERO,
            info_active: ZERO,
            info_foreground: ZERO,
            info_hover: ZERO,
            input: ZERO,
            link: ZERO,
            link_active: ZERO,
            link_hover: ZERO,
            list: ZERO,
            list_active: ZERO,
            list_active_border: ZERO,
            list_even: ZERO,
            list_head: ZERO,
            list_hover: ZERO,
            muted: ZERO,
            muted_foreground: ZERO,
            popover: ZERO,
            popover_foreground: ZERO,
            primary: ZERO,
            primary_active: ZERO,
            primary_foreground: ZERO,
            primary_hover: ZERO,
            progress_bar: ZERO,
            ring: ZERO,
            scrollbar: ZERO,
            scrollbar_thumb: ZERO,
            scrollbar_thumb_hover: ZERO,
            secondary: ZERO,
            secondary_active: ZERO,
            secondary_foreground: ZERO,
            secondary_hover: ZERO,
            selection: ZERO,
            sidebar: ZERO,
            sidebar_accent: ZERO,
            sidebar_accent_foreground: ZERO,
            sidebar_border: ZERO,
            sidebar_foreground: ZERO,
            sidebar_primary: ZERO,
            sidebar_primary_foreground: ZERO,
            skeleton: ZERO,
            slider_bar: ZERO,
            slider_thumb: ZERO,
            success: ZERO,
            success_foreground: ZERO,
            success_hover: ZERO,
            success_active: ZERO,
            switch: ZERO,
            switch_thumb: ZERO,
            tab: ZERO,
            tab_active: ZERO,
            tab_active_foreground: ZERO,
            tab_bar: ZERO,
            tab_bar_segmented: ZERO,
            tab_foreground: ZERO,
            table: ZERO,
            table_active: ZERO,
            table_active_border: ZERO,
            table_even: ZERO,
            table_head: ZERO,
            table_head_foreground: ZERO,
            table_foot: ZERO,
            table_foot_foreground: ZERO,
            table_hover: ZERO,
            table_row_border: ZERO,
            title_bar: ZERO,
            title_bar_border: ZERO,
            tiles: ZERO,
            warning: ZERO,
            warning_active: ZERO,
            warning_hover: ZERO,
            warning_foreground: ZERO,
            overlay: ZERO,
            window_border: ZERO,
            red: ZERO,
            red_light: ZERO,
            green: ZERO,
            green_light: ZERO,
            blue: ZERO,
            blue_light: ZERO,
            yellow: ZERO,
            yellow_light: ZERO,
            magenta: ZERO,
            magenta_light: ZERO,
            cyan: ZERO,
            cyan_light: ZERO,
        }
    }
}

impl ThemeColor {
    /// Resolve `config`'s sparse overrides over `default` (the mode's base
    /// palette), producing a fully-populated `ThemeColor`. A field is taken
    /// from the override when present and parseable, otherwise from its
    /// gpui-defined fallback (or `default` for the handful of fields gpui
    /// gives no fallback). Statement order matches gpui-component exactly.
    pub fn apply_config(&mut self, config: &ThemeConfig, default: &ThemeColor) {
        let c = &config.colors;

        // `apply!(field)`             — gpui's `apply_color!($f)`:
        //     present&ok -> value; else -> default.field
        // `apply!(field, fb_expr)`    — gpui's `apply_color!($f, fallback=…)`:
        //     present&ok -> value; else -> fb_expr
        macro_rules! apply {
            ($field:ident) => {
                self.$field = match c.$field.as_deref().map(try_parse_color) {
                    Some(Some(col)) => col,
                    _ => default.$field,
                };
            };
            ($field:ident, $fallback:expr) => {
                self.$field = match c.$field.as_deref().map(try_parse_color) {
                    Some(Some(col)) => col,
                    _ => $fallback,
                };
            };
        }

        apply!(background);

        // Base ANSI colours (everything else can be derived from these).
        apply!(red);
        apply!(red_light, self.background.blend(self.red.opacity(0.8)));
        apply!(green);
        apply!(green_light, self.background.blend(self.green.opacity(0.8)));
        apply!(blue);
        apply!(blue_light, self.background.blend(self.blue.opacity(0.8)));
        apply!(magenta);
        apply!(
            magenta_light,
            self.background.blend(self.magenta.opacity(0.8))
        );
        apply!(yellow);
        apply!(
            yellow_light,
            self.background.blend(self.yellow.opacity(0.8))
        );
        apply!(cyan);
        apply!(cyan_light, self.background.blend(self.cyan.opacity(0.8)));

        apply!(border);
        apply!(foreground);
        apply!(muted);
        apply!(
            muted_foreground,
            self.muted.blend(self.foreground.opacity(0.7))
        );

        // Button / state colours. Active darkening and the group-box wash are
        // mode-dependent in gpui — preserve that.
        let active_darken = if config.mode.is_dark() { 0.2 } else { 0.1 };
        let hover_opacity = 0.9;
        apply!(primary);
        apply!(primary_foreground, self.foreground);
        apply!(
            primary_hover,
            self.background.blend(self.primary.opacity(hover_opacity))
        );
        apply!(primary_active, self.primary.darken(active_darken));
        apply!(button_primary, self.primary);
        apply!(button_primary_foreground, self.primary_foreground);
        apply!(button_primary_hover, self.primary_hover);
        apply!(button_primary_active, self.primary_active);
        apply!(secondary);
        apply!(secondary_foreground, self.foreground);
        apply!(
            secondary_hover,
            self.background.blend(self.secondary.opacity(hover_opacity))
        );
        apply!(secondary_active, self.secondary.darken(active_darken));
        apply!(success, self.green);
        apply!(success_foreground, self.primary_foreground);
        apply!(
            success_hover,
            self.background.blend(self.success.opacity(hover_opacity))
        );
        apply!(success_active, self.success.darken(active_darken));
        apply!(info, self.cyan);
        apply!(info_foreground, self.primary_foreground);
        apply!(
            info_hover,
            self.background.blend(self.info.opacity(hover_opacity))
        );
        apply!(info_active, self.info.darken(active_darken));
        apply!(warning, self.yellow);
        apply!(warning_foreground, self.primary_foreground);
        apply!(
            warning_hover,
            self.background.blend(self.warning.opacity(0.9))
        );
        apply!(
            warning_active,
            self.background.blend(self.warning.darken(active_darken))
        );

        // Everything else, each derived from the resolved colours above.
        apply!(accent, self.secondary);
        apply!(accent_foreground, self.foreground);
        apply!(accordion, self.background);
        apply!(accordion_hover, self.accent.opacity(0.8));
        apply!(
            group_box,
            self.background
                .blend(
                    self.secondary
                        .opacity(if config.mode.is_dark() { 0.3 } else { 0.4 })
                )
        );
        apply!(group_box_foreground, self.foreground);
        apply!(caret, self.primary);
        apply!(chart_1, self.blue.lighten(0.4));
        apply!(chart_2, self.blue.lighten(0.2));
        apply!(chart_3, self.blue);
        apply!(chart_4, self.blue.darken(0.2));
        apply!(chart_5, self.blue.darken(0.4));
        apply!(chart_bullish, self.green);
        apply!(chart_bearish, self.red);
        apply!(danger, self.red);
        apply!(danger_active, self.danger.darken(active_darken));
        apply!(danger_foreground, self.primary_foreground);
        apply!(
            danger_hover,
            self.background.blend(self.danger.opacity(0.9))
        );
        apply!(
            description_list_label,
            self.background.blend(self.border.opacity(0.2))
        );
        apply!(description_list_label_foreground, self.muted_foreground);
        apply!(drag_border, self.primary.opacity(0.65));
        apply!(drop_target, self.primary.opacity(0.2));
        apply!(input, self.border);
        apply!(link, self.primary);
        apply!(link_active, self.link);
        apply!(link_hover, self.link);
        apply!(list, self.background);
        apply!(
            list_active,
            self.background.blend(self.primary.opacity(0.1))
        );
        apply!(
            list_active_border,
            self.background.blend(self.primary.opacity(0.6))
        );
        apply!(list_even, self.list);
        apply!(list_head, self.list);
        apply!(list_hover, self.accent.opacity(0.6));
        apply!(popover, self.background);
        apply!(popover_foreground, self.foreground);
        apply!(progress_bar, self.primary);
        apply!(ring, self.blue);
        apply!(scrollbar, self.background);
        apply!(scrollbar_thumb, self.accent);
        apply!(scrollbar_thumb_hover, self.scrollbar_thumb);
        apply!(selection, self.primary);
        apply!(sidebar, self.background.blend(self.border.opacity(0.15)));
        apply!(sidebar_accent, self.accent);
        apply!(sidebar_accent_foreground, self.accent_foreground);
        apply!(sidebar_border, self.border);
        apply!(sidebar_foreground, self.foreground);
        apply!(sidebar_primary, self.primary);
        apply!(sidebar_primary_foreground, self.primary_foreground);
        apply!(skeleton, self.secondary);
        apply!(slider_bar, self.primary);
        apply!(slider_thumb, self.primary_foreground);
        apply!(switch, self.secondary_active);
        apply!(switch_thumb, self.background);
        apply!(tab, self.background);
        apply!(tab_active, self.background);
        apply!(tab_active_foreground, self.foreground);
        apply!(tab_bar, self.background);
        apply!(tab_bar_segmented, self.secondary);
        apply!(tab_foreground, self.foreground);
        apply!(table, self.list);
        apply!(table_active, self.list_active);
        apply!(table_active_border, self.list_active_border);
        apply!(table_even, self.list_even);
        apply!(table_head, self.list_head);
        apply!(table_head_foreground, self.muted_foreground);
        apply!(table_foot, self.list_head);
        apply!(table_foot_foreground, self.muted_foreground);
        apply!(table_hover, self.list_hover);
        apply!(table_row_border, self.border);
        apply!(title_bar, self.background);
        apply!(title_bar_border, self.border);
        apply!(tiles, self.background);
        apply!(overlay);
        apply!(window_border, self.border);

        // gpui's final clamps: these tints must stay subtle no matter what the
        // override or fallback produced.
        self.list_active = self.list_active.alpha(self.list_active.a.min(0.2));
        self.table_active = self.table_active.alpha(self.table_active.a.min(0.2));
        self.selection = self.selection.alpha(self.selection.a.min(0.3));
    }
}
