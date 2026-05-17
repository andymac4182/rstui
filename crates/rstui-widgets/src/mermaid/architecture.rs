//! `architecture` Mermaid diagram renderer (scaffold stub).
//!
//! The dispatcher in [`super`] routes this diagram type here. The real
//! parser + deterministic layout + [`super::draw::Surface`] render lands in
//! the fan-out; until then this draws the shared honest placeholder so the
//! whole `Mermaid` widget is wired, compiles, and never panics on a
//! `architecture` source.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;

/// Renders a `architecture` Mermaid diagram from `src` into `area`.
pub(crate) fn render(
    _src: &str,
    area: Rect,
    buf: &mut Buffer,
    base: Style,
    theme: &MermaidTheme,
) {
    super::diagram_placeholder("architecture", "renderer pending", area, buf, base, theme);
}
