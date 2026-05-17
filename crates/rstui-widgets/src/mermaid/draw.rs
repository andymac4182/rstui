//! Re-export of the shared [`crate::diagram`] drawing surface.
//!
//! The character grid the non-flowchart Mermaid renderers compose onto used
//! to live here; it is now the crate-wide [`crate::diagram`] component shared
//! with the [`Structurizr`](crate::Structurizr) C4 renderer (the user-driven
//! "factor out what's common between the diagram widgets" refactor). This
//! module stays as a thin alias so every `super::draw::{Surface, BoxStyle}`
//! import across the Mermaid renderers keeps resolving unchanged — Mermaid's
//! behaviour and its snapshot tests are byte-identical; only the
//! implementation's home moved.

pub(crate) use crate::diagram::{BoxStyle, Surface};

#[cfg(test)]
pub(crate) use crate::diagram::dump;
