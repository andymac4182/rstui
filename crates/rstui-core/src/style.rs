//! Colors, text modifiers, and composable [`Style`] values.
//!
//! [`Style`] follows a *patch* model borrowed from proven TUI ecosystems: a
//! style is a sparse overlay rather than a complete description. Unset color
//! fields leave whatever is underneath untouched, and modifiers are split into
//! "add" and "remove" sets so partial styles compose predictably. This is the
//! property themes, focus rings, and selection highlights all depend on.

/// A terminal color.
///
/// `Reset` maps to the terminal's configured default. The named colors are the
/// standard 16-color ANSI palette; [`Color::Indexed`] addresses the 256-color
/// palette and [`Color::Rgb`] requests a 24-bit truecolor value (subject to
/// terminal support, resolved by the backend).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    /// The terminal's default foreground/background.
    #[default]
    Reset,
    /// ANSI black (palette index 0).
    Black,
    /// ANSI red (palette index 1).
    Red,
    /// ANSI green (palette index 2).
    Green,
    /// ANSI yellow (palette index 3).
    Yellow,
    /// ANSI blue (palette index 4).
    Blue,
    /// ANSI magenta (palette index 5).
    Magenta,
    /// ANSI cyan (palette index 6).
    Cyan,
    /// ANSI white / light gray (palette index 7).
    Gray,
    /// ANSI bright black (palette index 8).
    DarkGray,
    /// ANSI bright red (palette index 9).
    LightRed,
    /// ANSI bright green (palette index 10).
    LightGreen,
    /// ANSI bright yellow (palette index 11).
    LightYellow,
    /// ANSI bright blue (palette index 12).
    LightBlue,
    /// ANSI bright magenta (palette index 13).
    LightMagenta,
    /// ANSI bright cyan (palette index 14).
    LightCyan,
    /// ANSI bright white (palette index 15).
    White,
    /// A 256-color palette index.
    Indexed(u8),
    /// A 24-bit truecolor value.
    Rgb(u8, u8, u8),
}

/// How much colour fidelity a terminal supports.
///
/// rstui never assumes truecolor: the backend detects the level (env-driven)
/// and [`Color::degrade`] reduces every color to fit, so a 24-bit theme still
/// renders sensibly over SSH, in tmux, or on a 16-color terminal instead of
/// emitting `38;2` escapes a terminal cannot parse.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorLevel {
    /// No colour at all (`NO_COLOR`, `TERM=dumb`): everything becomes `Reset`.
    NoColor,
    /// The 16 ANSI colours only.
    Ansi16,
    /// The 256-colour palette.
    Ansi256,
    /// Full 24-bit truecolor (the default; the backend downgrades from here).
    #[default]
    TrueColor,
}

/// The 16 ANSI colours as RGB, indexed 0–15 to match the named [`Color`]
/// variants (`Black`=0 … `White`=15). The classic VGA/xterm "system" palette
/// (the same constants Rich uses for its STANDARD palette); the nearest-match
/// downgrade is computed against these.
const ANSI16_RGB: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (170, 0, 0),
    (0, 170, 0),
    (170, 85, 0),
    (0, 0, 170),
    (170, 0, 170),
    (0, 170, 170),
    (170, 170, 170),
    (85, 85, 85),
    (255, 85, 85),
    (85, 255, 85),
    (255, 255, 85),
    (85, 85, 255),
    (255, 85, 255),
    (85, 255, 255),
    (255, 255, 255),
];

impl Color {
    /// This color's ANSI index (0–15) if it is one of the 16 named colours.
    const fn named_index(self) -> Option<u8> {
        Some(match self {
            Color::Black => 0,
            Color::Red => 1,
            Color::Green => 2,
            Color::Yellow => 3,
            Color::Blue => 4,
            Color::Magenta => 5,
            Color::Cyan => 6,
            Color::Gray => 7,
            Color::DarkGray => 8,
            Color::LightRed => 9,
            Color::LightGreen => 10,
            Color::LightYellow => 11,
            Color::LightBlue => 12,
            Color::LightMagenta => 13,
            Color::LightCyan => 14,
            Color::White => 15,
            Color::Reset | Color::Indexed(_) | Color::Rgb(..) => return None,
        })
    }

    /// The approximate RGB of any concrete colour. `Reset` has no fixed RGB
    /// (it is the terminal default), so it returns `None`.
    fn approx_rgb(self) -> Option<(u8, u8, u8)> {
        match self {
            Color::Reset => None,
            Color::Rgb(r, g, b) => Some((r, g, b)),
            Color::Indexed(i) => Some(index_to_rgb(i)),
            named => Some(ANSI16_RGB[named.named_index().unwrap_or(0) as usize]),
        }
    }

    /// This colour as a 256-palette index.
    ///
    /// Named colours and existing indices pass through unchanged; `Rgb` is
    /// quantised with the Rich/xterm algorithm — a saturation-gated grayscale
    /// ramp (indices 232–255 plus 16/231 for the ends) for near-greys, and the
    /// non-linear 6×6×6 colour cube otherwise (the cube levels are 0,95,135,
    /// 175,215,255, hence the `/95` then `/40` split, not a naive `c/255*5`).
    #[must_use]
    pub fn to_indexed(self) -> u8 {
        match self {
            Color::Reset => 0,
            Color::Indexed(i) => i,
            Color::Rgb(r, g, b) => rgb_to_index(r, g, b),
            named => named.named_index().unwrap_or(0),
        }
    }

    /// The nearest of the 16 ANSI colours, as a 0–15 index, using the
    /// perceptual "redmean" weighted-RGB distance (markedly better than plain
    /// Euclidean — the same metric Rich/colour-science use).
    #[must_use]
    pub fn to_ansi16(self) -> u8 {
        if let Some(i) = self.named_index() {
            return i;
        }
        let Some((r, g, b)) = self.approx_rgb() else {
            return 7; // Reset has no RGB; default-ish grey if forced.
        };
        let mut best = 0u8;
        let mut best_dist = i64::MAX;
        for (i, &(pr, pg, pb)) in ANSI16_RGB.iter().enumerate() {
            let d = redmean(r, g, b, pr, pg, pb);
            if d < best_dist {
                best_dist = d;
                best = i as u8;
            }
        }
        best
    }

    /// Express this colour within what `level` can render. The backend calls
    /// this before mapping every cell colour, so a theme authored in truecolor
    /// degrades gracefully instead of emitting unsupported escapes.
    #[must_use]
    pub fn degrade(self, level: ColorLevel) -> Color {
        match level {
            ColorLevel::TrueColor => self,
            ColorLevel::NoColor => Color::Reset,
            ColorLevel::Ansi256 => match self {
                Color::Rgb(..) => Color::Indexed(self.to_indexed()),
                other => other,
            },
            ColorLevel::Ansi16 => match self {
                Color::Reset => Color::Reset,
                // Already a 16-colour: keep it (named, or a low index).
                Color::Indexed(i) if i < 16 => Color::Indexed(i),
                _ if self.named_index().is_some() => self,
                _ => Color::Indexed(self.to_ansi16()),
            },
        }
    }
}

/// xterm 256-palette index → RGB. 0–15 system (the ANSI16 table), 16–231 the
/// 6×6×6 cube (level = 0 or 55+40·v), 232–255 the 24-step grayscale ramp.
fn index_to_rgb(i: u8) -> (u8, u8, u8) {
    if i < 16 {
        return ANSI16_RGB[i as usize];
    }
    if i >= 232 {
        let v = 8 + (i as u16 - 232) * 10;
        let v = v.min(255) as u8;
        return (v, v, v);
    }
    let i = i as u16 - 16;
    let level = |c: u16| -> u8 { if c == 0 { 0 } else { (55 + c * 40) as u8 } };
    (level(i / 36), level((i / 6) % 6), level(i % 6))
}

/// Truecolor → 256-palette index (Rich's `downgrade`): grayscale ramp for
/// low-saturation colours, the non-linear cube otherwise.
fn rgb_to_index(r: u8, g: u8, b: u8) -> u8 {
    let (rf, gf, bf) = (
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
    );
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let l = (max + min) / 2.0;
    let s = if (max - min).abs() < f64::EPSILON {
        0.0
    } else if l < 0.5 {
        (max - min) / (max + min)
    } else {
        (max - min) / (2.0 - max - min)
    };
    if s < 0.15 {
        // Near-grey: the dedicated 232–255 ramp (+ 16 black / 231 white) is
        // far better than the cube's grey diagonal.
        let gray = (l * 25.0).round() as i32;
        return match gray {
            0 => 16,
            25 => 231,
            g => (231 + g) as u8,
        };
    }
    // Non-linear cube: < 95 is the bottom step, then 40-wide steps.
    let six = |c: u8| -> i64 {
        let c = f64::from(c);
        let v = if c < 95.0 {
            c / 95.0
        } else {
            1.0 + (c - 95.0) / 40.0
        };
        v.round() as i64
    };
    (16 + 36 * six(r) + 6 * six(g) + six(b)) as u8
}

/// "Redmean" weighted-RGB distance (squared): a cheap perceptual metric that
/// beats plain Euclidean for nearest-palette matching.
fn redmean(r1: u8, g1: u8, b1: u8, r2: u8, g2: u8, b2: u8) -> i64 {
    let rmean = (i64::from(r1) + i64::from(r2)) / 2;
    let dr = i64::from(r1) - i64::from(r2);
    let dg = i64::from(g1) - i64::from(g2);
    let db = i64::from(b1) - i64::from(b2);
    (((512 + rmean) * dr * dr) >> 8) + 4 * dg * dg + (((767 - rmean) * db * db) >> 8)
}

/// A set of text rendering attributes, stored as a small bitset.
///
/// Hand-rolled rather than pulling in a bitflags dependency: the operations
/// are trivial and the core crate stays dependency-free.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Modifier(u16);

impl Modifier {
    /// No attributes set.
    pub const EMPTY: Self = Self(0);
    /// Increased intensity / bold.
    pub const BOLD: Self = Self(1 << 0);
    /// Decreased intensity / faint.
    pub const DIM: Self = Self(1 << 1);
    /// Italic.
    pub const ITALIC: Self = Self(1 << 2);
    /// Underlined.
    pub const UNDERLINED: Self = Self(1 << 3);
    /// Slowly blinking.
    pub const SLOW_BLINK: Self = Self(1 << 4);
    /// Rapidly blinking.
    pub const RAPID_BLINK: Self = Self(1 << 5);
    /// Swap foreground and background.
    pub const REVERSED: Self = Self(1 << 6);
    /// Hidden / concealed.
    pub const HIDDEN: Self = Self(1 << 7);
    /// Struck through.
    pub const CROSSED_OUT: Self = Self(1 << 8);

    /// Returns `true` if every attribute in `other` is also set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns `true` if no attributes are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns `self` with the attributes in `other` added.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns `self` with the attributes in `other` removed.
    #[must_use]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

impl std::ops::BitOr for Modifier {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for Modifier {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

/// A composable description of how a cell should be drawn.
///
/// Colors are optional: `None` means "inherit". Modifiers are tracked as two
/// disjoint sets so that [`Style::patch`] can both add and clear attributes
/// without the order of composition mattering for unrelated bits.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Style {
    /// Foreground color, or `None` to inherit.
    pub fg: Option<Color>,
    /// Background color, or `None` to inherit.
    pub bg: Option<Color>,
    /// Attributes this style turns on.
    pub add_modifier: Modifier,
    /// Attributes this style turns off.
    pub sub_modifier: Modifier,
}

impl Style {
    /// An empty style that changes nothing when patched onto another.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            fg: None,
            bg: None,
            add_modifier: Modifier::EMPTY,
            sub_modifier: Modifier::EMPTY,
        }
    }

    /// A style that resets foreground, background, and all attributes to the
    /// terminal default.
    #[must_use]
    pub const fn reset() -> Self {
        Self {
            fg: Some(Color::Reset),
            bg: Some(Color::Reset),
            add_modifier: Modifier::EMPTY,
            sub_modifier: Modifier::EMPTY,
        }
    }

    /// Sets the foreground color.
    #[must_use]
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    /// Sets the background color.
    #[must_use]
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// Turns the given attributes on (and cancels any matching "remove").
    #[must_use]
    pub const fn add_modifier(mut self, modifier: Modifier) -> Self {
        self.sub_modifier = self.sub_modifier.difference(modifier);
        self.add_modifier = self.add_modifier.union(modifier);
        self
    }

    /// Turns the given attributes off (and cancels any matching "add").
    #[must_use]
    pub const fn remove_modifier(mut self, modifier: Modifier) -> Self {
        self.add_modifier = self.add_modifier.difference(modifier);
        self.sub_modifier = self.sub_modifier.union(modifier);
        self
    }

    /// Overlays `other` on top of `self`, returning the combined style.
    ///
    /// Set colors in `other` win; unset colors fall through to `self`.
    /// Modifier add/remove sets accumulate so that, for example, patching a
    /// "bold" style then a "not bold" style leaves the attribute cleared.
    #[must_use]
    pub fn patch(mut self, other: Self) -> Self {
        self.fg = other.fg.or(self.fg);
        self.bg = other.bg.or(self.bg);
        self.add_modifier = self.add_modifier.difference(other.sub_modifier);
        self.add_modifier = self.add_modifier.union(other.add_modifier);
        self.sub_modifier = self.sub_modifier.difference(other.add_modifier);
        self.sub_modifier = self.sub_modifier.union(other.sub_modifier);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_set_operations() {
        let m = Modifier::BOLD | Modifier::ITALIC;
        assert!(m.contains(Modifier::BOLD));
        assert!(m.contains(Modifier::ITALIC));
        assert!(!m.contains(Modifier::UNDERLINED));
        assert!(m.difference(Modifier::BOLD).contains(Modifier::ITALIC));
        assert!(!m.difference(Modifier::BOLD).contains(Modifier::BOLD));
        assert!(Modifier::EMPTY.is_empty());
    }

    #[test]
    fn patch_lets_set_colors_win_and_inherits_unset() {
        let base = Style::new().fg(Color::Red).bg(Color::Black);
        let overlay = Style::new().fg(Color::Green);
        let merged = base.patch(overlay);
        assert_eq!(merged.fg, Some(Color::Green));
        assert_eq!(merged.bg, Some(Color::Black));
    }

    #[test]
    fn patch_resolves_modifier_conflicts_by_recency() {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let not_bold = Style::new().remove_modifier(Modifier::BOLD);

        let cleared = bold.patch(not_bold);
        assert!(!cleared.add_modifier.contains(Modifier::BOLD));
        assert!(cleared.sub_modifier.contains(Modifier::BOLD));

        let restored = not_bold.patch(bold);
        assert!(restored.add_modifier.contains(Modifier::BOLD));
        assert!(!restored.sub_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn empty_patch_is_identity() {
        let s = Style::new()
            .fg(Color::Cyan)
            .add_modifier(Modifier::UNDERLINED);
        assert_eq!(s.patch(Style::new()), s);
        assert_eq!(Style::new().patch(s), s);
    }

    #[test]
    fn reset_requests_explicit_defaults() {
        let r = Style::reset();
        assert_eq!(r.fg, Some(Color::Reset));
        assert_eq!(r.bg, Some(Color::Reset));
    }

    #[test]
    fn named_and_indexed_colours_pass_through_quantisation() {
        // The 16 named colours map to their own ANSI index, both ways.
        for (c, idx) in [
            (Color::Black, 0),
            (Color::Red, 1),
            (Color::White, 15),
            (Color::DarkGray, 8),
            (Color::LightCyan, 14),
        ] {
            assert_eq!(c.to_ansi16(), idx, "{c:?} -> ansi16");
            assert_eq!(c.to_indexed(), idx, "{c:?} -> indexed");
        }
        // An existing palette index is never re-quantised.
        assert_eq!(Color::Indexed(200).to_indexed(), 200);
        assert_eq!(Color::Indexed(7).to_ansi16(), 7);
    }

    #[test]
    fn rgb_quantises_greys_to_the_ramp_and_ends() {
        // Pure black/white hit the dedicated cube ends, not the grey ramp.
        assert_eq!(Color::Rgb(0, 0, 0).to_indexed(), 16);
        assert_eq!(Color::Rgb(255, 255, 255).to_indexed(), 231);
        // A mid grey lands in the 232–255 ramp.
        let mid = Color::Rgb(128, 128, 128).to_indexed();
        assert!((232..=255).contains(&mid), "mid grey -> ramp, got {mid}");
    }

    #[test]
    fn rgb_quantises_saturated_colours_into_the_cube() {
        // Pure red is the cube's max-red corner: 16 + 36*5 = 196.
        assert_eq!(Color::Rgb(255, 0, 0).to_indexed(), 196);
        // Pure green / blue corners.
        assert_eq!(Color::Rgb(0, 255, 0).to_indexed(), 46);
        assert_eq!(Color::Rgb(0, 0, 255).to_indexed(), 21);
        // Every cube index is in range.
        let i = Color::Rgb(123, 45, 200).to_indexed();
        assert!((16..=231).contains(&i), "saturated -> cube, got {i}");
    }

    #[test]
    fn rgb_finds_the_nearest_ansi16() {
        // An exact palette point matches itself (redmean distance 0).
        for (i, &(r, g, b)) in ANSI16_RGB.iter().enumerate() {
            assert_eq!(
                Color::Rgb(r, g, b).to_ansi16(),
                i as u8,
                "exact palette point {i} must match itself"
            );
        }
        // Near-grey extremes resolve to black / white.
        assert_eq!(Color::Rgb(8, 8, 8).to_ansi16(), 0);
        assert_eq!(Color::Rgb(248, 248, 248).to_ansi16(), 15);
        // A deep saturated red is nearer ANSI 1 (170,0,0) than the pinkish
        // bright-red (255,85,85) under the perceptual redmean metric — the
        // same result Rich produces for this palette.
        assert_eq!(Color::Rgb(250, 10, 10).to_ansi16(), 1);
    }

    #[test]
    fn degrade_respects_each_level() {
        let rgb = Color::Rgb(200, 30, 40);
        // TrueColor is the identity.
        assert_eq!(rgb.degrade(ColorLevel::TrueColor), rgb);
        // 256: Rgb collapses to an index; named/Reset untouched.
        assert_eq!(
            rgb.degrade(ColorLevel::Ansi256),
            Color::Indexed(rgb.to_indexed())
        );
        assert_eq!(Color::Red.degrade(ColorLevel::Ansi256), Color::Red);
        assert_eq!(Color::Reset.degrade(ColorLevel::Ansi256), Color::Reset);
        // 16: named stays named, Rgb/high-index collapse to a 0–15 index.
        assert_eq!(Color::Red.degrade(ColorLevel::Ansi16), Color::Red);
        assert_eq!(
            Color::Indexed(5).degrade(ColorLevel::Ansi16),
            Color::Indexed(5)
        );
        match rgb.degrade(ColorLevel::Ansi16) {
            Color::Indexed(i) => assert!(i < 16),
            other => panic!("Ansi16 must yield a 0–15 index, got {other:?}"),
        }
        // NoColor erases all colour.
        assert_eq!(rgb.degrade(ColorLevel::NoColor), Color::Reset);
        assert_eq!(Color::Red.degrade(ColorLevel::NoColor), Color::Reset);
    }

    #[test]
    fn quantisation_is_total_over_every_input() {
        // No index or RGB triple may panic or escape range (the iter-25 rule).
        for i in 0u8..=255 {
            let _ = index_to_rgb(i);
            let c = Color::Indexed(i);
            assert!(c.to_ansi16() < 16);
            assert_eq!(c.to_indexed(), i);
            assert_eq!(c.degrade(ColorLevel::NoColor), Color::Reset);
        }
        for &(r, g, b) in &[(0, 0, 0), (255, 255, 255), (1, 254, 7), (128, 64, 200)] {
            let c = Color::Rgb(r, g, b);
            assert!((16..=255).contains(&c.to_indexed()));
            assert!(c.to_ansi16() < 16);
        }
    }
}
