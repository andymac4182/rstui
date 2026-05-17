//! The floating-point colour the cascade computes in, before it is reduced to
//! a terminal [`Color`].
//!
//! gpui-component authors a theme as a sparse set of overrides and *derives*
//! the rest by compositing — `background.blend(primary.opacity(0.9))`,
//! `primary.darken(0.2)`. Reproducing a theme faithfully therefore means
//! reproducing those operations exactly, in the same colour space gpui uses:
//! straight-alpha sRGB for [`blend`](Hsla::blend), HSL lightness scaling for
//! [`lighten`](Hsla::lighten)/[`darken`](Hsla::darken). [`Hsla`] is that
//! working representation; [`Rgba`] is the channel form blend math needs.
//!
//! A terminal cell has no alpha, so the final step
//! ([`Hsla::over`]) composites the resolved colour onto the theme background
//! and hands back an opaque [`Color::Rgb`] — the reason a `#CDA86911` "active
//! row" tint renders as a faint wash rather than solid gold.

use rstui_core::Color;

/// A straight-alpha sRGB colour, every channel in `0.0..=1.0`.
///
/// This is the form [`Hsla::blend`] composites in (gpui blends in sRGB, not a
/// linear space) and the form hex strings parse into.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    /// Red, `0.0..=1.0`.
    pub r: f32,
    /// Green, `0.0..=1.0`.
    pub g: f32,
    /// Blue, `0.0..=1.0`.
    pub b: f32,
    /// Alpha, `0.0` (transparent) ..= `1.0` (opaque).
    pub a: f32,
}

/// A hue/saturation/lightness/alpha colour, every component in `0.0..=1.0`
/// (the hue is normalised, *not* degrees — matching gpui's `Hsla`).
///
/// The cascade keeps colours here because gpui's lightness operations
/// ([`lighten`](Self::lighten)/[`darken`](Self::darken)) are defined on the
/// `l` channel; blending drops to [`Rgba`] and back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsla {
    /// Hue, normalised to `0.0..=1.0` (multiply by 360 for degrees).
    pub h: f32,
    /// Saturation, `0.0..=1.0`.
    pub s: f32,
    /// Lightness, `0.0..=1.0`.
    pub l: f32,
    /// Alpha, `0.0` (transparent) ..= `1.0` (opaque).
    pub a: f32,
}

impl Rgba {
    /// Fully-transparent black — the cascade's zero value, matching
    /// `ThemeColor::default()` in gpui (every unset field starts here).
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    /// Parses `#RGB`, `#RGBA`, `#RRGGBB`, or `#RRGGBBAA` (the leading `#` is
    /// optional, case-insensitive). Returns `None` on any other shape so a
    /// malformed override falls back to the default exactly as gpui's
    /// `try_parse_color` does.
    #[must_use]
    pub fn parse_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        let nibble = |c: u8| -> Option<u8> {
            match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                b'A'..=b'F' => Some(c - b'A' + 10),
                _ => None,
            }
        };
        let bytes = hex.as_bytes();
        // (channel count incl. alpha, chars per channel)
        let (channels, width) = match bytes.len() {
            3 => (3, 1),
            4 => (4, 1),
            6 => (3, 2),
            8 => (4, 2),
            _ => return None,
        };
        let mut chan = [255u8; 4];
        for i in 0..channels {
            let v = if width == 1 {
                let n = nibble(bytes[i])?;
                n * 17 // #abc => #aabbcc
            } else {
                nibble(bytes[2 * i])? * 16 + nibble(bytes[2 * i + 1])?
            };
            chan[i] = v;
        }
        Some(Self {
            r: f32::from(chan[0]) / 255.0,
            g: f32::from(chan[1]) / 255.0,
            b: f32::from(chan[2]) / 255.0,
            a: f32::from(chan[3]) / 255.0,
        })
    }
}

/// `t` wrapped into `0.0..1.0`, then the HSL hue→channel ramp.
fn hue_to_channel(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

impl From<Rgba> for Hsla {
    fn from(c: Rgba) -> Self {
        let max = c.r.max(c.g).max(c.b);
        let min = c.r.min(c.g).min(c.b);
        let l = (max + min) / 2.0;
        if (max - min).abs() < f32::EPSILON {
            return Self {
                h: 0.0,
                s: 0.0,
                l,
                a: c.a,
            };
        }
        let d = max - min;
        let s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        let h = if (max - c.r).abs() < f32::EPSILON {
            (c.g - c.b) / d + if c.g < c.b { 6.0 } else { 0.0 }
        } else if (max - c.g).abs() < f32::EPSILON {
            (c.b - c.r) / d + 2.0
        } else {
            (c.r - c.g) / d + 4.0
        };
        Self {
            h: h / 6.0,
            s,
            l,
            a: c.a,
        }
    }
}

impl From<Hsla> for Rgba {
    fn from(c: Hsla) -> Self {
        let l = c.l.clamp(0.0, 1.0);
        let s = c.s.clamp(0.0, 1.0);
        if s.abs() < f32::EPSILON {
            return Self {
                r: l,
                g: l,
                b: l,
                a: c.a,
            };
        }
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        let h = c.h;
        Self {
            r: hue_to_channel(p, q, h + 1.0 / 3.0),
            g: hue_to_channel(p, q, h),
            b: hue_to_channel(p, q, h - 1.0 / 3.0),
            a: c.a,
        }
    }
}

impl Hsla {
    /// A new colour with alpha multiplied by `factor` (gpui's `opacity`):
    /// `0.8` makes it 80 % as opaque. Used by the blend-based fallbacks.
    #[must_use]
    pub fn opacity(self, factor: f32) -> Self {
        Self {
            a: self.a * factor.clamp(0.0, 1.0),
            ..self
        }
    }

    /// A new colour with alpha *set* to `a` (gpui's `alpha`), distinct from
    /// [`opacity`](Self::opacity)'s multiply — the final list/table/selection
    /// clamps use this.
    #[must_use]
    pub fn alpha(self, a: f32) -> Self {
        Self {
            a: a.clamp(0.0, 1.0),
            ..self
        }
    }

    /// gpui `lighten`: scale lightness *up* by `factor` (`l * (1 + factor)`),
    /// clamped on conversion. Drives the lighter `chart_*` ramp.
    #[must_use]
    pub fn lighten(self, factor: f32) -> Self {
        Self {
            l: self.l * (1.0 + factor.clamp(0.0, 1.0)),
            ..self
        }
    }

    /// gpui `darken`: scale lightness *down* by `factor` (`l * (1 - factor)`).
    /// Drives every `*_active` fallback.
    #[must_use]
    pub fn darken(self, factor: f32) -> Self {
        Self {
            l: self.l * (1.0 - factor.clamp(0.0, 1.0)),
            ..self
        }
    }

    /// gpui `blend`: composite `other` *over* `self` using `other`'s alpha
    /// (straight-alpha source-over in sRGB), yielding an opaque colour. This
    /// is the operation almost every derived theme colour is built from.
    #[must_use]
    pub fn blend(self, other: Hsla) -> Hsla {
        let alpha = other.a.clamp(0.0, 1.0);
        if alpha >= 1.0 {
            return Hsla { a: 1.0, ..other };
        }
        if alpha <= 0.0 {
            return Hsla { a: 1.0, ..self };
        }
        let base: Rgba = self.into();
        let top: Rgba = other.into();
        let mix = |b: f32, t: f32| b * (1.0 - alpha) + t * alpha;
        Rgba {
            r: mix(base.r, top.r),
            g: mix(base.g, top.g),
            b: mix(base.b, top.b),
            a: 1.0,
        }
        .into()
    }

    /// Composite this (possibly translucent) colour onto an opaque `bg` and
    /// reduce to a terminal [`Color::Rgb`]. A terminal cell has no alpha, so
    /// this is the single point where the theme's translucent tints become
    /// the concrete colour a backend emits.
    #[must_use]
    pub fn over(self, bg: Hsla) -> Color {
        let opaque = if self.a >= 1.0 {
            self
        } else {
            Hsla { a: 1.0, ..bg }.blend(self)
        };
        let c: Rgba = opaque.into();
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color::Rgb(q(c.r), q(c.g), q(c.b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parses_every_width() {
        assert_eq!(Rgba::parse_hex("#000000"), Some(Rgba::TRANSPARENT.opaque()));
        assert_eq!(Rgba::parse_hex("#fff"), Rgba::parse_hex("#ffffff"));
        assert_eq!(Rgba::parse_hex("#f00f"), Rgba::parse_hex("#ff0000ff"));
        let half = Rgba::parse_hex("#00000080").unwrap();
        assert!((half.a - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(Rgba::parse_hex("nope"), None);
        assert_eq!(Rgba::parse_hex("#12345"), None);
    }

    #[test]
    fn rgb_hsl_roundtrips() {
        for hex in ["#141414", "#cda869", "#2b2b2b", "#dcdcdc", "#ff8800"] {
            let rgba = Rgba::parse_hex(hex).unwrap();
            let back: Rgba = Hsla::from(rgba).into();
            assert!((rgba.r - back.r).abs() < 1e-3, "{hex} r");
            assert!((rgba.g - back.g).abs() < 1e-3, "{hex} g");
            assert!((rgba.b - back.b).abs() < 1e-3, "{hex} b");
        }
    }

    #[test]
    fn blend_is_opaque_source_over() {
        let bg: Hsla = Rgba::parse_hex("#000000").unwrap().into();
        let white: Hsla = Rgba {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.5,
        }
        .into();
        let mixed: Rgba = bg.blend(white).into();
        assert!((mixed.r - 0.5).abs() < 1e-2);
        assert_eq!(mixed.a, 1.0);
        // Fully-opaque top replaces the base.
        assert_eq!(bg.blend(Hsla { a: 1.0, ..white }).a, 1.0);
    }

    #[test]
    fn over_composites_translucent_tint_onto_background() {
        // Twilight's faint gold active-row tint over its #141414 base.
        let bg: Hsla = Rgba::parse_hex("#141414").unwrap().into();
        let tint: Hsla = Rgba::parse_hex("#CDA86911").unwrap().into();
        let Color::Rgb(r, g, b) = tint.over(bg) else {
            panic!("expected rgb");
        };
        // ~7 % gold over near-black: barely lifted, nowhere near #CDA869.
        assert!(r < 50 && g < 45 && b < 40, "got {r},{g},{b}");
    }

    impl Rgba {
        fn opaque(self) -> Self {
            Self { a: 1.0, ..self }
        }
    }
}
