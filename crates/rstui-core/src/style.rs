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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
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
}
