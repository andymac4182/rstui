//! A dependency-free, **fail-closed** semver subset for the `api_version`
//! gate (ADR 0007 §2 — "semver-gated like opencode's `engines.opencode`:
//! incompatible ⇒ refuse to load").
//!
//! The host implements a concrete protocol [`Version`] (e.g. `1.0.0`); a
//! plugin's manifest declares the [`VersionReq`] it can speak. The host
//! refuses to spawn unless the requirement [`matches`](VersionReq::matches)
//! its version. A `semver` crate would be the simplest implementation, but
//! this crate is dependency-free by policy (ADR 0007 driver 4), so the
//! supported grammar is deliberately small, exactly specified here, and
//! anything outside it is a parse error that the host treats as
//! incompatible (fail-closed — an un-understood requirement is never
//! optimistically allowed).
//!
//! # Grammar (exactly this — everything else is rejected)
//!
//! A [`Version`] is `MAJOR[.MINOR[.PATCH]]`, each component ASCII digits
//! only (missing components are `0`). A [`VersionReq`] is a non-empty,
//! comma-separated **AND**-list of comparators; every comparator must hold.
//! A comparator is one of:
//!
//! | Form | Meaning |
//! |---|---|
//! | `*` or `x` | any version |
//! | `MAJOR` / `MAJOR.x` | `>=MAJOR.0.0, <(MAJOR+1).0.0` |
//! | `MAJOR.MINOR` / `MAJOR.MINOR.x` | `>=MAJOR.MINOR.0, <MAJOR.(MINOR+1).0` |
//! | `MAJOR.MINOR.PATCH` | exactly that version |
//! | `^MAJOR.MINOR.PATCH` | `>=` it, `<` the compatibility ceiling¹ |
//! | `=` / `>=` / `<=` / `>` / `<` then a version | the obvious comparison |
//!
//! ¹ caret ceiling: `MAJOR>0` ⇒ `(MAJOR+1).0.0`; `MAJOR==0, MINOR given`
//! ⇒ `0.(MINOR+1).0`; `MAJOR==0, MINOR absent` ⇒ `1.0.0` (npm semantics).
//!
//! ```
//! use rstui_plugin_host::version::is_compatible;
//!
//! // Host implements 1.4.2; a plugin that targets major 1 is compatible.
//! assert_eq!(is_compatible("1.4.2", "1"), Ok(true));
//! assert_eq!(is_compatible("1.4.2", "^1.2.0"), Ok(true));
//! assert_eq!(is_compatible("2.0.0", "1"), Ok(false));
//! // A requirement the grammar does not understand is rejected, not guessed.
//! assert!(is_compatible("1.0.0", "~1.2").is_err());
//! ```

use std::fmt;
use std::str::FromStr;

/// A concrete `MAJOR.MINOR.PATCH` version (missing components are `0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// Major component.
    pub major: u64,
    /// Minor component.
    pub minor: u64,
    /// Patch component.
    pub patch: u64,
}

impl Version {
    /// A version with the given components.
    #[must_use]
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A parse failure. Every variant means "treat as incompatible"
/// (fail-closed): the host never spawns a plugin whose `api_version` it
/// cannot fully understand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionError {
    /// The version or requirement string was empty.
    Empty,
    /// A numeric component was missing, non-digit, or overflowed `u64`.
    BadComponent(String),
    /// More than three dotted components.
    TooManyComponents(String),
    /// A comparator did not match any supported form.
    BadComparator(String),
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty version string"),
            Self::BadComponent(s) => write!(f, "invalid version component: `{s}`"),
            Self::TooManyComponents(s) => write!(f, "too many version components: `{s}`"),
            Self::BadComparator(s) => write!(f, "unsupported version comparator: `{s}`"),
        }
    }
}

impl std::error::Error for VersionError {}

/// Parse 1–3 dotted ASCII-digit components into `(major, minor, patch)`,
/// missing components defaulting to `0`. `allow_wildcard` accepts a final
/// `x`/`*` component, reported via the returned precision.
fn parse_parts(s: &str, allow_wildcard: bool) -> Result<(Version, Precision), VersionError> {
    if s.is_empty() {
        return Err(VersionError::Empty);
    }
    let mut nums = [0u64; 3];
    let mut precision = Precision::Patch;
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() > 3 {
        return Err(VersionError::TooManyComponents(s.to_string()));
    }
    for (i, part) in parts.iter().enumerate() {
        if allow_wildcard && (*part == "x" || *part == "X" || *part == "*") {
            // A wildcard ends the meaningful prefix. Its range is the range
            // of the last *concrete* component before it: `1.x` ranges over
            // major 1 (Major precision, set by index 0), `1.2.x` over minor
            // 1.2 (Minor, index 1). A leading wildcard with a concrete tail
            // (`x.2`) is nonsensical — bare `*`/`x`/`X` is handled before
            // this is ever called, so reaching here at i==0 is rejected
            // fail-closed.
            if i == 0 {
                return Err(VersionError::BadComponent((*part).to_string()));
            }
            return Ok((Version::new(nums[0], nums[1], nums[2]), precision));
        }
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(VersionError::BadComponent((*part).to_string()));
        }
        nums[i] = part
            .parse::<u64>()
            .map_err(|_| VersionError::BadComponent((*part).to_string()))?;
        precision = match i {
            0 => Precision::Major,
            1 => Precision::Minor,
            _ => Precision::Patch,
        };
    }
    Ok((Version::new(nums[0], nums[1], nums[2]), precision))
}

/// How precisely a partial version was written — drives range expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Precision {
    Major,
    Minor,
    Patch,
}

impl FromStr for Version {
    type Err = VersionError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (v, _) = parse_parts(s.trim(), false)?;
        Ok(v)
    }
}

/// One comparator within a [`VersionReq`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum Comparator {
    Any,
    /// `>=lo, <hi` (hi exclusive). Exact `=x.y.z` is `[v, v]` via `Exact`.
    Range {
        lo: Version,
        hi: Option<Version>,
    },
    Exact(Version),
    Gt(Version),
    Ge(Version),
    Lt(Version),
    Le(Version),
}

impl Comparator {
    fn matches(&self, v: &Version) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(x) => v == x,
            Self::Gt(x) => v > x,
            Self::Ge(x) => v >= x,
            Self::Lt(x) => v < x,
            Self::Le(x) => v <= x,
            Self::Range { lo, hi } => v >= lo && hi.as_ref().is_none_or(|h| v < h),
        }
    }
}

fn caret(v: Version, p: Precision) -> Comparator {
    let _ = p;
    let hi = if v.major > 0 {
        Version::new(v.major + 1, 0, 0)
    } else if v.minor > 0 {
        Version::new(0, v.minor + 1, 0)
    } else {
        Version::new(1, 0, 0)
    };
    Comparator::Range {
        lo: v,
        hi: Some(hi),
    }
}

/// A partial version (e.g. `1`, `1.2`, `1.x`) expands to the natural range.
fn partial_range(v: Version, p: Precision) -> Comparator {
    match p {
        Precision::Major => Comparator::Range {
            lo: Version::new(v.major, 0, 0),
            hi: Some(Version::new(v.major + 1, 0, 0)),
        },
        Precision::Minor => Comparator::Range {
            lo: Version::new(v.major, v.minor, 0),
            hi: Some(Version::new(v.major, v.minor + 1, 0)),
        },
        Precision::Patch => Comparator::Exact(v),
    }
}

fn parse_comparator(raw: &str) -> Result<Comparator, VersionError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(VersionError::BadComparator(raw.to_string()));
    }
    if s == "*" || s == "x" || s == "X" {
        return Ok(Comparator::Any);
    }
    if let Some(rest) = s.strip_prefix('^') {
        let (v, p) = parse_parts(rest.trim(), false)?;
        return Ok(caret(v, p));
    }
    for (op, build) in [(">=", 0u8), ("<=", 1), (">", 2), ("<", 3), ("=", 4)] {
        if let Some(rest) = s.strip_prefix(op) {
            let (v, _) = parse_parts(rest.trim(), false)?;
            return Ok(match build {
                0 => Comparator::Ge(v),
                1 => Comparator::Le(v),
                2 => Comparator::Gt(v),
                3 => Comparator::Lt(v),
                _ => Comparator::Exact(v),
            });
        }
    }
    // Bare partial / wildcard version.
    let (v, p) = parse_parts(s, true)?;
    Ok(partial_range(v, p))
}

/// A parsed `api_version` requirement: a non-empty AND-list of comparators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReq {
    comparators: Vec<Comparator>,
}

impl VersionReq {
    /// Whether `version` satisfies every comparator.
    #[must_use]
    pub fn matches(&self, version: &Version) -> bool {
        self.comparators.iter().all(|c| c.matches(version))
    }
}

impl FromStr for VersionReq {
    type Err = VersionError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(VersionError::Empty);
        }
        let comparators = s
            .split(',')
            .map(parse_comparator)
            .collect::<Result<Vec<_>, _>>()?;
        if comparators.is_empty() {
            return Err(VersionError::Empty);
        }
        Ok(Self { comparators })
    }
}

/// Parse `host` as a [`Version`] and `requirement` as a [`VersionReq`] and
/// report whether the host satisfies it.
///
/// Returns `Err` if either string is outside the supported grammar; the
/// host treats that exactly like `Ok(false)` (fail-closed).
///
/// # Errors
///
/// [`VersionError`] if `host` is not a plain version or `requirement` is
/// not a supported requirement.
pub fn is_compatible(host: &str, requirement: &str) -> Result<bool, VersionError> {
    let host: Version = host.parse()?;
    let req: VersionReq = requirement.parse()?;
    Ok(req.matches(&host))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(s: &str) -> VersionReq {
        s.parse().expect("req parses")
    }
    fn ver(s: &str) -> Version {
        s.parse().expect("ver parses")
    }

    #[test]
    fn version_parses_partial_components_as_zero() {
        assert_eq!(ver("1"), Version::new(1, 0, 0));
        assert_eq!(ver("1.2"), Version::new(1, 2, 0));
        assert_eq!(ver("1.2.3"), Version::new(1, 2, 3));
        assert_eq!(ver(" 2.0.1 "), Version::new(2, 0, 1));
    }

    #[test]
    fn version_rejects_garbage_fail_closed() {
        assert!("".parse::<Version>().is_err());
        assert!("1.".parse::<Version>().is_err());
        assert!("1.x".parse::<Version>().is_err()); // wildcard not a concrete version
        assert!("a".parse::<Version>().is_err());
        assert!("1.2.3.4".parse::<Version>().is_err());
        assert!("-1".parse::<Version>().is_err());
        assert!("01a".parse::<Version>().is_err());
    }

    #[test]
    fn bare_major_is_major_compatible_range() {
        let r = req("1");
        assert!(r.matches(&ver("1.0.0")));
        assert!(r.matches(&ver("1.9.9")));
        assert!(!r.matches(&ver("2.0.0")));
        assert!(!r.matches(&ver("0.9.9")));
    }

    #[test]
    fn bare_major_minor_is_minor_range() {
        let r = req("1.2");
        assert!(r.matches(&ver("1.2.0")));
        assert!(r.matches(&ver("1.2.99")));
        assert!(!r.matches(&ver("1.3.0")));
        assert!(!r.matches(&ver("1.1.9")));
    }

    #[test]
    fn full_triple_is_exact() {
        let r = req("1.2.3");
        assert!(r.matches(&ver("1.2.3")));
        assert!(!r.matches(&ver("1.2.4")));
        assert!(!r.matches(&ver("1.2.2")));
    }

    #[test]
    fn wildcards_behave_like_partials() {
        assert!(req("1.x").matches(&ver("1.7.0")));
        assert!(!req("1.x").matches(&ver("2.0.0")));
        assert!(req("1.2.x").matches(&ver("1.2.9")));
        assert!(!req("1.2.x").matches(&ver("1.3.0")));
        assert!(req("*").matches(&ver("9.9.9")));
        assert!(req("x").matches(&ver("0.0.1")));
    }

    #[test]
    fn caret_uses_npm_compatibility_ceiling() {
        assert!(req("^1.2.3").matches(&ver("1.2.3")));
        assert!(req("^1.2.3").matches(&ver("1.9.0")));
        assert!(!req("^1.2.3").matches(&ver("2.0.0")));
        assert!(!req("^1.2.3").matches(&ver("1.2.2")));
        // 0.x caret pins the minor.
        assert!(req("^0.3.1").matches(&ver("0.3.9")));
        assert!(!req("^0.3.1").matches(&ver("0.4.0")));
        // 0.0.x caret ceiling is 1.0.0.
        assert!(req("^0.0.2").matches(&ver("0.0.2")));
    }

    #[test]
    fn explicit_comparators_and_and_lists() {
        assert!(req(">=1.2.0").matches(&ver("1.2.0")));
        assert!(req(">=1.2.0").matches(&ver("9.9.9")));
        assert!(!req(">1.2.0").matches(&ver("1.2.0")));
        assert!(req("<2").matches(&ver("1.9.9")));
        assert!(req("<=1.2.3").matches(&ver("1.2.3")));
        assert!(req("=1.2.3").matches(&ver("1.2.3")));
        // AND-list: both must hold.
        let r = req(">=1.2.0, <2.0.0");
        assert!(r.matches(&ver("1.5.0")));
        assert!(!r.matches(&ver("2.0.0")));
        assert!(!r.matches(&ver("1.1.0")));
    }

    #[test]
    fn unsupported_requirement_is_an_error_not_a_guess() {
        assert!("~1.2".parse::<VersionReq>().is_err());
        assert!("1.2.3 - 2.0.0".parse::<VersionReq>().is_err());
        assert!("".parse::<VersionReq>().is_err());
        assert!(">=".parse::<VersionReq>().is_err());
        assert!("^".parse::<VersionReq>().is_err());
        assert!("1.2.3,".parse::<VersionReq>().is_err());
    }

    #[test]
    fn is_compatible_is_fail_closed_on_bad_input() {
        assert_eq!(is_compatible("1.0.0", "1"), Ok(true));
        assert_eq!(is_compatible("2.0.0", "1"), Ok(false));
        assert!(is_compatible("not-a-version", "1").is_err());
        assert!(is_compatible("1.0.0", "~1").is_err());
    }
}
