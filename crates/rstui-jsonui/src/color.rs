//! Theme-token colours for projected components and charts.
//!
//! An agent names a colour semantically (`"color":"success"`), as a
//! chart series, or as a raw fallback (`"#1f77b4"` / `"cyan"`) — the
//! *tokens-with-raw-fallback* contract. [`parse_token`] classifies the
//! string into a [`ColorToken`]; the projection resolves it against the
//! active [`Palette`] (chart series auto-cycle `chart_1..=chart_5`).
//!
//! `rstui-jsonui` stays theme-system-agnostic: [`Palette`] is plain
//! colour data, not an `rstui-theme` dependency. A host (the ACP
//! client) maps its live `ThemePalette` tokens into a `Palette` at the
//! boundary; the dep-free [`Palette::ANSI`] default keeps charts
//! coloured — and tests deterministic — when no theme is supplied.

use rstui_core::Color;

/// A semantic / chart-series / raw colour an agent can put on a
/// component or chart. `Chart(n)` is 1-based and cycles past 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorToken {
    /// The accent / primary brand colour.
    Accent,
    /// Informational (neutral-positive) colour.
    Info,
    /// Success / positive colour.
    Success,
    /// Warning / caution colour.
    Warning,
    /// Danger / error / destructive colour.
    Danger,
    /// Muted / secondary / dim text colour.
    Muted,
    /// Default foreground text colour.
    Text,
    /// Border / divider colour.
    Border,
    /// A chart data series (1-based; cycles `chart_1..=chart_5`).
    Chart(u8),
    /// Upward / positive financial movement (candlestick etc.).
    Bullish,
    /// Downward / negative financial movement.
    Bearish,
    /// A raw `#rgb`/`#rrggbb` or basic named colour (the fallback when
    /// the agent does not use a theme token).
    Raw(Color),
}

/// The resolved colour for every [`ColorToken`] — a plain data palette
/// so `rstui-jsonui` carries no theme-system dependency. The ACP client
/// builds one from its active `ThemePalette`; [`Palette::ANSI`] is the
/// dep-free default used by tests and theme-less rendering.
#[derive(Debug, Clone)]
pub struct Palette {
    /// Accent / primary.
    pub accent: Color,
    /// Informational.
    pub info: Color,
    /// Success / positive.
    pub success: Color,
    /// Warning / caution.
    pub warning: Color,
    /// Danger / error.
    pub danger: Color,
    /// Muted / secondary.
    pub muted: Color,
    /// Default foreground.
    pub text: Color,
    /// Border / divider.
    pub border: Color,
    /// The five chart series colours (`chart_1..=chart_5`).
    pub chart: [Color; 5],
    /// Upward financial movement.
    pub bullish: Color,
    /// Downward financial movement.
    pub bearish: Color,
}

impl Palette {
    /// The dependency-free ANSI default: every token maps to a basic
    /// ANSI colour so charts are coloured and tests are deterministic
    /// even when no host theme is supplied.
    pub const ANSI: Self = Self {
        accent: Color::Cyan,
        info: Color::Blue,
        success: Color::Green,
        warning: Color::Yellow,
        danger: Color::Red,
        muted: Color::Gray,
        text: Color::Reset,
        border: Color::Gray,
        chart: [
            Color::Cyan,
            Color::Green,
            Color::Yellow,
            Color::Magenta,
            Color::Blue,
        ],
        bullish: Color::Green,
        bearish: Color::Red,
    };

    /// The concrete colour for `token` against this palette. `Chart(n)`
    /// is 1-based and cycles every five series; `Raw` passes through.
    #[must_use]
    pub fn resolve(&self, token: ColorToken) -> Color {
        match token {
            ColorToken::Accent => self.accent,
            ColorToken::Info => self.info,
            ColorToken::Success => self.success,
            ColorToken::Warning => self.warning,
            ColorToken::Danger => self.danger,
            ColorToken::Muted => self.muted,
            ColorToken::Text => self.text,
            ColorToken::Border => self.border,
            ColorToken::Chart(n) => {
                let idx = usize::from(n.max(1) - 1) % self.chart.len();
                self.chart[idx]
            }
            ColorToken::Bullish => self.bullish,
            ColorToken::Bearish => self.bearish,
            ColorToken::Raw(color) => color,
        }
    }

    /// The colour for a chart series by 0-based index (cycles).
    #[must_use]
    pub fn series(&self, index: usize) -> Color {
        self.chart[index % self.chart.len()]
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::ANSI
    }
}

/// Parse an agent-supplied colour string into a [`ColorToken`].
///
/// Order: semantic token → chart series (`chart1`/`series2`/`s3`) →
/// `bullish`/`bearish` → a raw `#rgb`/`#rrggbb` hex or the basic named
/// set (the *tokens-with-raw-fallback* contract). `None` for an empty
/// or unrecognised string (total — the caller leaves the default).
#[must_use]
pub fn parse_token(spec: &str) -> Option<ColorToken> {
    let s = spec.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    match s.as_str() {
        "accent" | "primary" | "brand" => return Some(ColorToken::Accent),
        "info" | "information" => return Some(ColorToken::Info),
        "success" | "ok" | "positive" | "good" => return Some(ColorToken::Success),
        "warning" | "warn" | "caution" => return Some(ColorToken::Warning),
        "danger" | "error" | "destructive" | "critical" | "negative" => {
            return Some(ColorToken::Danger);
        }
        "muted" | "secondary" | "subtle" | "dim" => return Some(ColorToken::Muted),
        "text" | "foreground" | "fg" | "default" => return Some(ColorToken::Text),
        "border" | "divider" | "outline" => return Some(ColorToken::Border),
        "bullish" | "up" | "gain" => return Some(ColorToken::Bullish),
        "bearish" | "down" | "loss" => return Some(ColorToken::Bearish),
        _ => {}
    }
    // chart series: `chart3` / `series3` / `s3` (1-based, clamped 1..=5).
    for prefix in ["chart_", "chart", "series_", "series", "s"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            if let Ok(n) = rest.parse::<u8>() {
                if (1..=99).contains(&n) {
                    return Some(ColorToken::Chart(((n - 1) % 5) + 1));
                }
            }
        }
    }
    parse_raw(&s).map(ColorToken::Raw)
}

/// A raw `#rgb` / `#rrggbb` hex or basic named colour (the fallback).
fn parse_raw(s: &str) -> Option<Color> {
    if let Some(hex) = s.strip_prefix('#') {
        let (r, g, b) = match hex.len() {
            3 => {
                let v = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok().map(|h| h * 17);
                (v(0)?, v(1)?, v(2)?)
            }
            6 => {
                let v = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
                (v(0)?, v(2)?, v(4)?)
            }
            _ => return None,
        };
        return Some(Color::Rgb(r, g, b));
    }
    Some(match s {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" | "purple" => Color::Magenta,
        "cyan" | "teal" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "white" => Color::White,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_chart_and_raw_tokens_parse_and_resolve() {
        let p = Palette::ANSI;
        assert_eq!(parse_token("success"), Some(ColorToken::Success));
        assert_eq!(p.resolve(ColorToken::Success), Color::Green);
        assert_eq!(parse_token("Danger"), Some(ColorToken::Danger));
        assert_eq!(parse_token("muted"), Some(ColorToken::Muted));

        // Chart series, 1-based, cycles past five.
        assert_eq!(parse_token("chart1"), Some(ColorToken::Chart(1)));
        assert_eq!(parse_token("series_3"), Some(ColorToken::Chart(3)));
        assert_eq!(parse_token("s7"), Some(ColorToken::Chart(2)));
        assert_eq!(p.resolve(ColorToken::Chart(1)), Color::Cyan);
        assert_eq!(
            p.resolve(ColorToken::Chart(6)),
            p.resolve(ColorToken::Chart(1))
        );
        assert_eq!(p.series(0), Color::Cyan);
        assert_eq!(p.series(5), p.series(0));

        // Raw fallback: hex (#rgb / #rrggbb) and named.
        assert_eq!(
            parse_token("#0a0b0c"),
            Some(ColorToken::Raw(Color::Rgb(10, 11, 12)))
        );
        assert_eq!(
            parse_token("#abc"),
            Some(ColorToken::Raw(Color::Rgb(170, 187, 204)))
        );
        assert_eq!(parse_token("teal"), Some(ColorToken::Raw(Color::Cyan)));
        assert_eq!(
            p.resolve(ColorToken::Raw(Color::Rgb(1, 2, 3))),
            Color::Rgb(1, 2, 3)
        );

        // Unknown / empty → None (caller keeps the default).
        assert_eq!(parse_token(""), None);
        assert_eq!(parse_token("chartreuse-ish"), None);
        assert_eq!(Palette::default().resolve(ColorToken::Accent), Color::Cyan);
    }
}
