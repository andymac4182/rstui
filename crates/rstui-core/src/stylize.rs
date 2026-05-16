//! Fluent color and attribute shorthands: the [`Stylize`] extension trait.
//!
//! [`Style`] and the [text model](crate::text) already compose correctly, but
//! writing
//! `Span::styled("saved", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD))`
//! for every styled run is the kind of friction that makes a TUI tedious to
//! build, and "build powerful terminal applications *quickly*" is the whole
//! point. Every widely-used TUI toolkit ships a fluent shorthand for exactly
//! this; rstui's is [`Stylize`] — one extension trait that gives `&str`,
//! [`String`], [`Span`], [`Line`], [`Text`], and [`Style`] itself chainable
//! `.green().bold()` / `.on_blue()` methods:
//!
//! ```
//! use rstui_core::{Color, Modifier, Span, Stylize};
//!
//! let span: Span = "saved".green().bold();
//! assert_eq!(span.content, "saved");
//! assert_eq!(span.style.fg, Some(Color::Green));
//! assert!(span.style.add_modifier.contains(Modifier::BOLD));
//! ```
//!
//! # How it composes
//!
//! [`Stylize`] is a blanket impl over anything that implements [`Styled`] —
//! "has a [`Style`] and can return a restyled copy of itself". Each shorthand
//! reads the running style, patches exactly one field through the existing
//! [`Style`] builder, and threads the value on, so chains compose with the
//! same order-independence [`Style::patch`] already guarantees, and a borrowed
//! `&str` widens to an owned [`Span`] on the first call so the rest of the
//! chain just flows.
//!
//! Implementing [`Styled`] for your own widget is all it takes to get the
//! whole shorthand vocabulary for free — the same extensibility seam the
//! style cascade and themes build on.
//!
//! # Deliberate scope
//!
//! Only the text-carrying types get this in this slice. [`Cell`] stores its
//! attributes as flat `fg`/`bg`/`modifier` fields rather than a [`Style`], and
//! [`Block`] has *three* style fields (base, border, title), so "which one
//! does `.red()` touch?" is a real design question — both are left to a
//! focused follow-up rather than guessed at here.
//!
//! [`Cell`]: crate::Cell
//! [`Block`]: crate::Block

use crate::style::{Color, Modifier, Style};
use crate::text::{Line, Span, Text};

/// A value that carries a [`Style`] and can produce a restyled copy.
///
/// This is the seam [`Stylize`] builds on: implement it for a type and the
/// blanket [`Stylize`] impl gives that type the full fluent shorthand set for
/// free. [`Self::Item`](Styled::Item) is usually `Self`, but a borrowed `&str`
/// widens to an owned [`Span`] because a bare string has nowhere to keep a
/// style.
pub trait Styled {
    /// The value produced by restyling.
    type Item;

    /// The style currently applied to this value.
    fn style(&self) -> Style;

    /// Returns this value with its style replaced by `style`.
    #[must_use]
    fn set_style(self, style: Style) -> Self::Item;
}

/// Generates the named-color shorthands as documented provided trait methods.
///
/// Each pair maps a `foreground` / `on_background` method to one [`Color`]
/// variant; the bodies just defer to the required `fg`/`bg` methods so the
/// blanket impl stays the single source of truth.
macro_rules! color_shorthands {
    ($($fg:ident / $bg:ident => $variant:ident),+ $(,)?) => {
        $(
            #[doc = concat!("Sets the foreground color to `", stringify!($variant), "`.")]
            #[must_use]
            fn $fg(self) -> Self::Item {
                self.fg(Color::$variant)
            }
            #[doc = concat!("Sets the background color to `", stringify!($variant), "`.")]
            #[must_use]
            fn $bg(self) -> Self::Item {
                self.bg(Color::$variant)
            }
        )+
    };
}

/// Generates the attribute shorthands as documented provided trait methods.
///
/// Each pair maps an `on` / `not_…` method to one [`Modifier`]; the `not_…`
/// variant removes the attribute, which is how a child run cancels an
/// attribute inherited from its enclosing line or text.
macro_rules! modifier_shorthands {
    ($($on:ident / $off:ident => $cnst:ident),+ $(,)?) => {
        $(
            #[doc = concat!("Turns on the `", stringify!($cnst), "` attribute.")]
            #[must_use]
            fn $on(self) -> Self::Item {
                self.add_modifier(Modifier::$cnst)
            }
            #[doc = concat!("Turns off the `", stringify!($cnst), "` attribute.")]
            #[must_use]
            fn $off(self) -> Self::Item {
                self.remove_modifier(Modifier::$cnst)
            }
        )+
    };
}

/// Chainable color and attribute shorthands for any [`Styled`] value.
///
/// `"err".red().bold()` instead of a hand-built [`Span`] + [`Style`]. The
/// chain behaves identically on `&str`, [`String`], [`Span`], [`Line`],
/// [`Text`], and [`Style`]. Every call patches exactly one style field, so
/// unrelated fields never interfere — the [`Style::patch`] property — which
/// means `x.red().bold()` and `x.bold().red()` produce the same value.
///
/// The named-color (`.green()`, `.on_blue()`, …) and attribute (`.bold()`,
/// `.not_bold()`, …) methods are provided in terms of [`fg`](Stylize::fg),
/// [`bg`](Stylize::bg), [`add_modifier`](Stylize::add_modifier), and
/// [`remove_modifier`](Stylize::remove_modifier), so implementors only supply
/// those.
pub trait Stylize: Sized {
    /// The restyled output type (see [`Styled::Item`]).
    type Item;

    /// Sets the foreground color.
    #[must_use]
    fn fg(self, color: Color) -> Self::Item;

    /// Sets the background color.
    #[must_use]
    fn bg(self, color: Color) -> Self::Item;

    /// Resets foreground, background, and every attribute to the terminal
    /// default. Unlike the other shorthands this is an explicit reset, not an
    /// inherit, so it overrides whatever was underneath.
    #[must_use]
    fn reset(self) -> Self::Item;

    /// Turns the given attributes on.
    #[must_use]
    fn add_modifier(self, modifier: Modifier) -> Self::Item;

    /// Turns the given attributes off. Useful to cancel an attribute inherited
    /// from an enclosing [`Line`]/[`Text`] (e.g. a non-bold run inside a bold
    /// line).
    #[must_use]
    fn remove_modifier(self, modifier: Modifier) -> Self::Item;

    color_shorthands! {
        black / on_black => Black,
        red / on_red => Red,
        green / on_green => Green,
        yellow / on_yellow => Yellow,
        blue / on_blue => Blue,
        magenta / on_magenta => Magenta,
        cyan / on_cyan => Cyan,
        gray / on_gray => Gray,
        dark_gray / on_dark_gray => DarkGray,
        light_red / on_light_red => LightRed,
        light_green / on_light_green => LightGreen,
        light_yellow / on_light_yellow => LightYellow,
        light_blue / on_light_blue => LightBlue,
        light_magenta / on_light_magenta => LightMagenta,
        light_cyan / on_light_cyan => LightCyan,
        white / on_white => White,
    }

    modifier_shorthands! {
        bold / not_bold => BOLD,
        dim / not_dim => DIM,
        italic / not_italic => ITALIC,
        underlined / not_underlined => UNDERLINED,
        slow_blink / not_slow_blink => SLOW_BLINK,
        rapid_blink / not_rapid_blink => RAPID_BLINK,
        reversed / not_reversed => REVERSED,
        hidden / not_hidden => HIDDEN,
        crossed_out / not_crossed_out => CROSSED_OUT,
    }
}

/// One blanket impl threads every shorthand through [`Styled`]: read the
/// running style, patch one field with the proven [`Style`] builder, store it
/// back. This is why a custom widget gets the whole vocabulary just by
/// implementing [`Styled`].
impl<T> Stylize for T
where
    T: Styled,
{
    type Item = <T as Styled>::Item;

    fn fg(self, color: Color) -> Self::Item {
        let style = self.style().fg(color);
        self.set_style(style)
    }

    fn bg(self, color: Color) -> Self::Item {
        let style = self.style().bg(color);
        self.set_style(style)
    }

    fn reset(self) -> Self::Item {
        self.set_style(Style::reset())
    }

    fn add_modifier(self, modifier: Modifier) -> Self::Item {
        let style = self.style().add_modifier(modifier);
        self.set_style(style)
    }

    fn remove_modifier(self, modifier: Modifier) -> Self::Item {
        let style = self.style().remove_modifier(modifier);
        self.set_style(style)
    }
}

impl Styled for Style {
    type Item = Style;

    fn style(&self) -> Style {
        *self
    }

    fn set_style(self, style: Style) -> Style {
        style
    }
}

impl<'a> Styled for &'a str {
    // A bare string has no style field, so it widens to a `Span` borrowing it.
    type Item = Span<'a>;

    fn style(&self) -> Style {
        Style::new()
    }

    fn set_style(self, style: Style) -> Span<'a> {
        Span::styled(self, style)
    }
}

impl Styled for String {
    // The owned string moves into an owned (`'static`-capable) `Span`.
    type Item = Span<'static>;

    fn style(&self) -> Style {
        Style::new()
    }

    fn set_style(self, style: Style) -> Span<'static> {
        Span::styled(self, style)
    }
}

impl<'a> Styled for Span<'a> {
    type Item = Span<'a>;

    fn style(&self) -> Style {
        self.style
    }

    fn set_style(self, style: Style) -> Span<'a> {
        Span { style, ..self }
    }
}

impl<'a> Styled for Line<'a> {
    type Item = Line<'a>;

    fn style(&self) -> Style {
        self.style
    }

    fn set_style(self, style: Style) -> Line<'a> {
        Line { style, ..self }
    }
}

impl<'a> Styled for Text<'a> {
    type Item = Text<'a>;

    fn style(&self) -> Style {
        self.style
    }

    fn set_style(self, style: Style) -> Text<'a> {
        Text { style, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::geometry::{Position, Rect};
    use crate::widget::Widget;

    #[test]
    fn str_widens_to_a_span_then_keeps_chaining() {
        let span: Span = "hi".red().bold();
        assert_eq!(span.content, "hi");
        assert_eq!(span.style.fg, Some(Color::Red));
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(span.style.bg, None); // untouched fields stay inherit
    }

    #[test]
    fn string_moves_into_an_owned_static_span() {
        // The `'static` annotation proves the owned content is not borrowed.
        let span: Span<'static> = String::from("owned").on_blue();
        assert_eq!(span.content, "owned");
        assert_eq!(span.style.bg, Some(Color::Blue));
    }

    #[test]
    fn style_is_self_styled_and_reset_overrides() {
        let s = Style::new().green().bold();
        assert_eq!(s.fg, Some(Color::Green));
        assert!(s.add_modifier.contains(Modifier::BOLD));

        // reset() is an explicit reset, not an inherit: it wins over a prior fg.
        let r = Style::new().green().reset();
        assert_eq!(r.fg, Some(Color::Reset));
        assert_eq!(r.bg, Some(Color::Reset));
        assert!(r.add_modifier.is_empty());
    }

    #[test]
    fn span_line_text_restyle_without_losing_content() {
        let span = Span::raw("a").cyan();
        assert_eq!(span.content, "a");
        assert_eq!(span.style.fg, Some(Color::Cyan));

        let line = Line::from(vec![Span::raw("x"), Span::raw("y")]).on_red();
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.style.bg, Some(Color::Red));

        let text = Text::raw("p\nq").italic();
        assert_eq!(text.lines.len(), 2);
        assert!(text.style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn patch_order_does_not_matter_for_unrelated_fields() {
        // The headline Stylize guarantee, asserted on the value itself.
        assert_eq!("z".red().bold(), "z".bold().red());
        assert_eq!(
            Style::new().on_green().underlined(),
            Style::new().underlined().on_green()
        );
    }

    #[test]
    fn representative_named_shorthands_map_to_the_right_field() {
        assert_eq!("a".dark_gray().style.fg, "a".fg(Color::DarkGray).style.fg);
        assert_eq!(Style::new().on_light_cyan().bg, Some(Color::LightCyan));
        assert!(
            Style::new()
                .crossed_out()
                .add_modifier
                .contains(Modifier::CROSSED_OUT)
        );
        assert!(
            Style::new()
                .not_underlined()
                .sub_modifier
                .contains(Modifier::UNDERLINED)
        );
        assert_eq!(Span::raw("w").white().style.fg, Some(Color::White));
    }

    #[test]
    fn not_modifier_cancels_an_inherited_attribute_end_to_end() {
        // A bold line with one run that opts out: the cascade + Stylize must
        // leave that run un-bold while its sibling stays bold.
        let line = Line::from(vec![Span::raw("X").not_bold(), Span::raw("Y")]).bold();

        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        line.render(buf.area(), &mut buf);

        let x = buf.get(Position::new(0, 0)).unwrap();
        let y = buf.get(Position::new(1, 0)).unwrap();
        assert_eq!(x.symbol, 'X');
        assert!(
            !x.modifier.contains(Modifier::BOLD),
            "the .not_bold() run must cancel the line's inherited bold"
        );
        assert_eq!(y.symbol, 'Y');
        assert!(
            y.modifier.contains(Modifier::BOLD),
            "the sibling run must keep the line's bold"
        );
    }
}
