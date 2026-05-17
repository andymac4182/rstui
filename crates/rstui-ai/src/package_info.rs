//! [`PackageInfo`] — an npm/crate package card: the "added `serde@1.0` —
//! 12 deps" beat an agent's dependency tools project.
//!
//! # A pure projection, reusing [`Badge`]
//!
//! The ai-elements `PackageInfo` is a card with the name, version, a
//! change-type badge, a description, and a dependency list — all the caller's
//! data, no interaction. So `PackageInfo` owns nothing: it projects a
//! caller-owned [`Package`].
//!
//! It is a framed [`Card`] and the change-type chip is a
//! real [`Badge`] (level-accented) — we *reuse* both,
//! not reinvent them.
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area, an
//! empty dependency list, and a long name/description are all safe clips —
//! never a panic.

use rstui_core::{Buffer, Position, Rect, Style, Widget};
use rstui_widgets::{Badge, BadgeLevel, Block, Card};

/// What kind of change produced this [`Package`] entry, selecting the badge
/// label and accent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    /// A semver-major bump (error accent).
    Major,
    /// A semver-minor bump (warning accent).
    Minor,
    /// A semver-patch bump (success accent) — the default.
    #[default]
    Patch,
    /// A newly added dependency (info accent).
    Added,
    /// A removed dependency (neutral accent).
    Removed,
}

impl ChangeType {
    /// The badge label for this change.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Patch => "patch",
            Self::Added => "added",
            Self::Removed => "removed",
        }
    }

    /// The [`BadgeLevel`] accent for this change.
    #[must_use]
    pub fn level(self) -> BadgeLevel {
        match self {
            Self::Major => BadgeLevel::Error,
            Self::Minor => BadgeLevel::Warning,
            Self::Patch => BadgeLevel::Success,
            Self::Added => BadgeLevel::Info,
            Self::Removed => BadgeLevel::Neutral,
        }
    }
}

/// The facts of a package card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The package name.
    pub name: String,
    /// The version string.
    pub version: String,
    /// What kind of change this entry is.
    pub change: ChangeType,
    /// A one-line description.
    pub description: String,
    /// The dependency names.
    pub dependencies: Vec<String>,
}

impl Package {
    /// A package `name` at `version` with `change`, `description`, and
    /// `dependencies`.
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        change: ChangeType,
        description: impl Into<String>,
        dependencies: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            change,
            description: description.into(),
            dependencies: dependencies.into_iter().collect(),
        }
    }
}

/// An npm/crate package card.
///
/// A framed [`Card`] titled `name@version`: the body is
/// the change-type [`Badge`], then the description,
/// then the dependency names (one per row, `· dep`). `PackageInfo` owns no
/// state — see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::package_info::{ChangeType, Package, PackageInfo};
///
/// let pkg = Package::new(
///     "serde", "1.0.0", ChangeType::Added, "Serialization",
///     vec!["serde_derive".to_string()],
/// );
/// let mut buf = Buffer::empty(Rect::new(0, 0, 24, 6));
/// PackageInfo::new(&pkg).render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌'); // framed
/// ```
#[derive(Debug, Clone)]
pub struct PackageInfo<'a> {
    package: &'a Package,
    style: Style,
}

impl<'a> PackageInfo<'a> {
    /// A card for `package`.
    #[must_use]
    pub fn new(package: &'a Package) -> Self {
        Self {
            package,
            style: Style::new(),
        }
    }

    /// Sets the base [`Style`] (the card frame/background).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The framing card titled `name@version`.
    fn card(&self) -> Card<'a> {
        Card::new().block(
            Block::bordered()
                .style(self.style)
                .title(format!("{}@{}", self.package.name, self.package.version)),
        )
    }
}

impl Widget for PackageInfo<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let card = self.card();
        let body = card.inner(area);
        card.render(area, buf);
        if body.is_empty() {
            return;
        }

        // Row 0: the change-type badge.
        Badge::new(self.package.change.label())
            .level(self.package.change.level())
            .render(Rect::new(body.left(), body.top(), body.width, 1), buf);

        // Row 1: the description.
        if body.height > 1 {
            let dy = body.top().saturating_add(1);
            let mut x = body.left();
            for ch in self.package.description.chars() {
                if x >= body.right() {
                    break;
                }
                buf.set_cell(Position::new(x, dy), ch, self.style);
                x = x.saturating_add(1);
            }
        }

        // Rows 2..: the dependency list.
        for (n, dep) in self.package.dependencies.iter().enumerate() {
            let row = 2u16.saturating_add(n as u16);
            if row >= body.height {
                break;
            }
            let y = body.top().saturating_add(row);
            let line = format!("· {dep}");
            let mut x = body.left();
            for ch in line.chars() {
                if x >= body.right() {
                    break;
                }
                buf.set_cell(Position::new(x, y), ch, self.style);
                x = x.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg() -> Package {
        Package::new(
            "serde",
            "1.0.0",
            ChangeType::Minor,
            "Serialization framework",
            vec!["serde_derive".to_string(), "serde_json".to_string()],
        )
    }

    fn lines(widget: PackageInfo<'_>, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_card_is_titled_name_at_version() {
        let p = pkg();
        let out = lines(PackageInfo::new(&p), 24, 7);
        assert!(out.contains("serde@1.0.0"), "got {out:?}");
    }

    #[test]
    fn the_body_is_badge_description_then_deps() {
        let p = pkg();
        let out = lines(PackageInfo::new(&p), 26, 7);
        assert!(out.contains("minor"), "got {out:?}"); // change badge
        assert!(out.contains("Serialization framework"), "got {out:?}");
        assert!(out.contains("· serde_derive"), "got {out:?}");
        assert!(out.contains("· serde_json"), "got {out:?}");
    }

    #[test]
    fn change_type_labels_and_levels_are_distinct() {
        assert_eq!(ChangeType::Major.label(), "major");
        assert_eq!(ChangeType::Major.level(), BadgeLevel::Error);
        assert_eq!(ChangeType::Minor.level(), BadgeLevel::Warning);
        assert_eq!(ChangeType::Patch.level(), BadgeLevel::Success);
        assert_eq!(ChangeType::Added.level(), BadgeLevel::Info);
        assert_eq!(ChangeType::Removed.level(), BadgeLevel::Neutral);
    }

    #[test]
    fn an_empty_dependency_list_is_safe() {
        let p = Package::new("x", "0.1", ChangeType::Added, "tiny", vec![]);
        let out = lines(PackageInfo::new(&p), 16, 5);
        assert!(out.contains("added"), "got {out:?}");
        assert!(out.contains("tiny"), "got {out:?}");
    }

    #[test]
    fn a_tiny_card_clips_without_panicking() {
        let p = pkg();
        // No room for a body at all — just the frame.
        let out = lines(PackageInfo::new(&p), 8, 2);
        assert!(out.starts_with('┌'), "got {out:?}");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let p = pkg();
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        PackageInfo::new(&p).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
