//! The composable rendering abstraction: a [`Widget`] draws itself into a
//! [`Rect`] of a [`Buffer`].
//!
//! Up to now a view wrote raw strings straight into the buffer. A `Widget` is
//! the seam that turns "describe the screen" into reusable pieces: each widget
//! is a cheap value constructed in the render pass, handed an area (usually
//! carved out by [`Layout`](crate::Layout)), and asked to paint it. Widgets
//! compose — a widget renders sub-widgets into sub-rects — which is the
//! property the whole component set is built on.
//!
//! `render` takes `self` by value: widgets are commands built fresh each frame
//! from borrowed app state, not retained objects, so consuming them is the
//! ergonomic and allocation-free choice for the `view` pattern. The bound is
//! still spelled `where Self: Sized` so `dyn Widget` stays a legal type for a
//! future heterogeneous widget list, even though rendering itself is always
//! monomorphized.
//!
//! This module ships only the **trait** and trivial blanket impls (`&str`,
//! `String`, `Option<W>`). Concrete widgets — `Block`, `Paragraph`, and the
//! component set that follows — live in the separate `rstui-widgets` crate so
//! `rstui-core` stays a small, slow-moving, dependency-free primitives layer
//! (see [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)).
//! Every widget crate, first- and third-party, depends on `rstui-core` and
//! implements this trait.
//!
//! # Example: authoring a widget
//!
//! Implementing `Widget` and stamping glyphs through the public, bounds-safe
//! [`Buffer::set_cell`] is the entire third-party authoring contract — the
//! same path the first-party `rstui-widgets` crate uses:
//!
//! ```
//! use rstui_core::{Buffer, Position, Rect, Style, Widget};
//!
//! /// A horizontal rule that fills its row with one character.
//! struct Rule(char);
//!
//! impl Widget for Rule {
//!     fn render(self, area: Rect, buf: &mut Buffer) {
//!         for x in area.left()..area.right() {
//!             buf.set_cell(Position::new(x, area.top()), self.0, Style::new());
//!         }
//!     }
//! }
//!
//! let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
//! Rule('=').render(buf.area(), &mut buf);
//! assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '=');
//! assert_eq!(buf.get(Position::new(3, 0)).unwrap().symbol, '=');
//! ```

use crate::buffer::Buffer;
use crate::geometry::{Position, Rect};
use crate::style::Style;

/// A value that can draw itself into a [`Rect`] region of a [`Buffer`].
///
/// Implement this for your own components; the runtime (via
/// [`Frame::render_widget`](crate::Frame::render_widget)) or another widget
/// supplies the area and buffer. `render` consumes `self` because widgets are
/// throwaway draw commands rebuilt every frame — see the [module
/// docs](self) for why that is the idiomatic choice here.
pub trait Widget {
    /// Draws this widget into `area` of `buf`.
    ///
    /// Implementations must stay within `area` and must tolerate an `area`
    /// smaller than they would like (including zero-sized) without panicking;
    /// the bounds-safe [`Buffer`] accessors make clipping the default.
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized;
}

/// Renders a string slice as a single clipped line at the area's origin.
///
/// Lets `frame.render_widget("hello", area)` work with no wrapper type. Text
/// is truncated at the right edge of `area`; wrapping and multi-line text are
/// the `Paragraph` widget's job (in the `rstui-widgets` crate).
impl Widget for &str {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let mut x = area.left();
        for ch in self.chars() {
            if x >= area.right() {
                break;
            }
            buf.set_cell(Position::new(x, area.top()), ch, Style::new());
            x = x.saturating_add(1);
        }
    }
}

/// Renders an owned `String` exactly like its [`&str`](Widget::render) slice.
impl Widget for String {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.as_str().render(area, buf);
    }
}

/// Renders the inner widget when present, and nothing when `None`.
///
/// Handy for optional UI (`frame.render_widget(error.map(...), area)`) without
/// a branch at the call site.
impl<W: Widget> Widget for Option<W> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if let Some(widget) = self {
            widget.render(area, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row.
    fn lines<W: Widget>(widget: W, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn str_widget_clips_to_the_area_not_the_buffer() {
        // Render into a sub-area narrower than the buffer: text must stop at
        // the area's right edge, leaving the rest of the row blank.
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        "hello world".render(Rect::new(2, 0, 4, 1), &mut buf);
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, ' ');
        assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'h');
        assert_eq!(buf.get(Position::new(5, 0)).unwrap().symbol, 'l');
        assert_eq!(buf.get(Position::new(6, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn option_widget_renders_only_when_some() {
        assert_eq!(lines(None::<&str>, 3, 1), "   \n");
        assert_eq!(lines(Some("ab"), 3, 1), "ab \n");
    }
}
