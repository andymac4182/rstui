//! [`Link`] — the link-span model and its [`LinkActivation`] event shape,
//! shared by every document widget that can carry clickable references.
//!
//! # A pure projection, like the rest of the widget set
//!
//! rstui widgets never mutate during render and never smuggle in input
//! routing (see [`Checkbox`](crate::Checkbox) on deferring focus). Links
//! follow that exact discipline: a rendered document *exposes* its links in
//! document order (e.g. [`Markdown::links`](crate::Markdown::links)); the
//! application owns "which link is focused" as ordinary state (an index, the
//! same shape [`FocusRing`](rstui_core::FocusRing) and [`List`](crate::List)
//! selection already use); and the reducer turns an Enter press or a mouse
//! click into a [`LinkActivation`] by indexing that list. The widget layer
//! decides *what a link looks like*, not *what happens when you press it* —
//! activation is the application's concern, kept here as a tiny, explicit
//! data shape rather than a hidden callback.
//!
//! ```
//! use rstui_widgets::{Link, Markdown};
//!
//! let doc = Markdown::new("see [the spec](https://example.com/spec) today");
//! let links = doc.links();
//! assert_eq!(links, vec![Link::new("the spec", "https://example.com/spec")]);
//!
//! // The app keeps a focused index; the reducer activates that entry.
//! let focused = 0;
//! let event = links[focused].activate(focused);
//! assert_eq!(event.href, "https://example.com/spec");
//! assert_eq!(event.index, 0);
//! ```
//!
//! # Terminal hyperlinks (OSC 8)
//!
//! Emitting the OSC 8 hyperlink escape so a terminal makes the text itself
//! clickable is a *backend* capability (it lives in the cell/escape layer,
//! owned by the runtime/backend crates), not a widget concern. It is a
//! documented future integration point; this slice models links logically and
//! styles them, which is what makes focus, click hit-testing, and activation
//! possible regardless of whether the terminal also underlines them natively.

use std::borrow::Cow;

/// One link: the visible `label` and the `href` it points at.
///
/// Both are [`Cow<str>`](std::borrow::Cow) so a parser can borrow from its
/// source or hand over owned strings without the caller caring which.
/// Equality is by value, which is what makes `links()` output assertable in
/// snapshot-style tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link<'a> {
    /// The text shown to the reader (already inline-resolved, no markup).
    pub label: Cow<'a, str>,
    /// The activation target — a URL, path, anchor, or anything the host
    /// chooses to resolve.
    pub href: Cow<'a, str>,
}

impl<'a> Link<'a> {
    /// A link from a `label` and an `href`.
    pub fn new(label: impl Into<Cow<'a, str>>, href: impl Into<Cow<'a, str>>) -> Self {
        Self {
            label: label.into(),
            href: href.into(),
        }
    }

    /// The display width of the label in the single-`char`-cell model.
    #[must_use]
    pub fn width(&self) -> usize {
        self.label.chars().count()
    }

    /// Builds the [`LinkActivation`] for this link at registry position
    /// `index` — what a reducer emits when the focused link is activated.
    #[must_use]
    pub fn activate(&self, index: usize) -> LinkActivation {
        LinkActivation {
            index,
            href: self.href.clone().into_owned(),
        }
    }
}

/// The event a reducer produces when a link is activated (Enter on the focused
/// link, or a click that hit-tests to it).
///
/// It is deliberately just data: `index` is the link's position in the
/// document's registry (the focus key) and `href` is the resolved target the
/// host should open. The widget never constructs this itself — building it is
/// the application's decision, so opening a URL, routing in-app, or ignoring
/// it stays entirely the reducer's policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkActivation {
    /// Position of the activated link in the document's ordered registry.
    pub index: usize,
    /// The target to act on (owned so it outlives the borrowed document).
    pub href: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activate_carries_the_index_and_owned_href() {
        let l = Link::new("docs", "https://example.com");
        let e = l.activate(3);
        assert_eq!(
            e,
            LinkActivation {
                index: 3,
                href: "https://example.com".to_owned(),
            }
        );
    }

    #[test]
    fn width_counts_chars_not_bytes() {
        assert_eq!(Link::new("café", "x").width(), 4);
        assert_eq!(Link::new("", "x").width(), 0);
    }

    #[test]
    fn equality_is_by_value() {
        assert_eq!(Link::new("a", "b"), Link::new(String::from("a"), "b"));
        assert_ne!(Link::new("a", "b"), Link::new("a", "c"));
    }
}
