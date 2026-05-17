//! The shadcn/Tailwind named palette, and the colour-token parser the cascade
//! resolves every override through.
//!
//! gpui-component's *default* theme (the light/dark base every other theme
//! falls back to) is authored against named scales — `neutral-100`,
//! `green-600`, `white` — not hex. Resolving it faithfully needs that palette,
//! so gpui-component's own `default-colors.json` is vendored verbatim and its
//! `hex` field is the source of truth (no HSL-channel reconstruction, so the
//! numbers are byte-identical to upstream). The 21 shipped themes are pure
//! hex; this path exists for the base and for user JSON that uses names.
//!
//! [`try_parse_color`] is the single entry point and mirrors gpui's
//! `try_parse_color`: `#rgb[a]`/`#rrggbb[aa]`, the literals `black`/`white`,
//! `family[-scale]`, and an optional `/opacity` percent suffix.

use crate::hsla::{Hsla, Rgba};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

/// gpui-component's vendored Tailwind/shadcn palette (its `hex` fields are the
/// source of truth — see the module note).
const DEFAULT_COLORS_JSON: &str = include_str!("../themes/_default-colors.json");

/// `"family-scale"` / `"black"` / `"white"` → resolved [`Rgba`]. Built once
/// from the vendored JSON; a bare `family` resolves to its `500` stop, the
/// shadcn default (matching gpui).
///
/// The file mixes shapes per key — a single swatch object (`"black"`), a ramp
/// array (`"slate"`), and string aliases (`"inherit"`); only the first two
/// carry a `hex`, and string aliases are never referenced by a theme, so they
/// are skipped rather than modelled.
fn named_table() -> &'static HashMap<String, Rgba> {
    static TABLE: OnceLock<HashMap<String, Rgba>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let raw: serde_json::Map<String, Value> = serde_json::from_str(DEFAULT_COLORS_JSON)
            .expect("vendored _default-colors.json must parse");
        let hex_of = |v: &Value| -> Option<Rgba> {
            v.get("hex")
                .and_then(Value::as_str)
                .and_then(Rgba::parse_hex)
        };
        let mut table = HashMap::new();
        for (name, value) in raw {
            match value {
                // A single swatch: `"black"` / `"white"`.
                Value::Object(_) => {
                    if let Some(c) = hex_of(&value) {
                        table.insert(name, c);
                    }
                }
                // A scale ramp: `"slate"`, `"neutral"`, …
                Value::Array(stops) => {
                    for stop in &stops {
                        let scale = stop.get("scale").and_then(Value::as_u64);
                        if let (Some(scale), Some(c)) = (scale, hex_of(stop)) {
                            if scale == 500 {
                                table.insert(name.clone(), c);
                            }
                            table.insert(format!("{name}-{scale}"), c);
                        }
                    }
                }
                // String aliases (`"inherit"`, `"transparent"`): never read.
                _ => {}
            }
        }
        table
    })
}

/// Parses one theme colour token into the cascade's working colour.
///
/// Returns `None` for anything unrecognised so the caller falls back to the
/// default for that field — exactly gpui's `apply_color!` behaviour, which is
/// what keeps a partial or slightly-malformed theme rendering instead of
/// going blank.
#[must_use]
pub fn try_parse_color(token: &str) -> Option<Hsla> {
    let token = token.trim();
    if token.starts_with('#') {
        return Rgba::parse_hex(token).map(Hsla::from);
    }

    // `family[-scale][/opacity]` — opacity is a 0..=100 percent.
    let (name, opacity) = match token.split_once('/') {
        Some((n, o)) => (n, o.parse::<f32>().ok()),
        None => (token, None),
    };

    let base = named_table().get(name).copied()?;
    let mut color = Hsla::from(base);
    if let Some(pct) = opacity {
        if !(0.0..=100.0).contains(&pct) {
            return None;
        }
        color = color.opacity(pct / 100.0);
    }
    Some(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_passthrough() {
        let c = try_parse_color("#141414").unwrap();
        let rgba: Rgba = c.into();
        assert!((rgba.r - 20.0 / 255.0).abs() < 1e-3);
    }

    #[test]
    fn named_scales_resolve_from_vendored_palette() {
        // Tailwind facts: white is #ffffff; neutral-950 is very dark; a bare
        // family is its 500 stop.
        let white: Rgba = try_parse_color("white").unwrap().into();
        assert!(white.r > 0.99 && white.g > 0.99 && white.b > 0.99);

        let n950: Rgba = try_parse_color("neutral-950").unwrap().into();
        assert!(n950.r < 0.1 && n950.g < 0.1 && n950.b < 0.1);

        assert_eq!(try_parse_color("blue"), try_parse_color("blue-500"));
        assert!(try_parse_color("not-a-color").is_none());
    }

    #[test]
    fn opacity_suffix_multiplies_alpha() {
        let c = try_parse_color("neutral-500/30").unwrap();
        assert!((c.a - 0.3).abs() < 1e-6);
        assert!(try_parse_color("white/250").is_none());
    }

    #[test]
    fn every_token_used_by_the_vendored_themes_resolves() {
        // The closed set of non-hex tokens that appear across the base theme
        // and all 21 shipped themes (see the project memory). If the palette
        // ever drops one, the base would silently go blank — fail loudly.
        for tok in [
            "white",
            "neutral-50",
            "neutral-100",
            "neutral-200",
            "neutral-300",
            "neutral-400",
            "neutral-500",
            "neutral-800",
            "neutral-900",
            "neutral-950",
            "red-300",
            "red-400",
            "red-500",
            "red-600",
            "green-300",
            "green-400",
            "green-500",
            "green-600",
            "blue-300",
            "blue-400",
            "blue-600",
            "cyan-300",
            "cyan-400",
            "cyan-500",
            "cyan-600",
            "yellow-300",
            "yellow-400",
            "yellow-500",
            "yellow-600",
            "purple-300",
            "purple-400",
            "purple-600",
        ] {
            assert!(try_parse_color(tok).is_some(), "{tok} must resolve");
        }
    }
}
