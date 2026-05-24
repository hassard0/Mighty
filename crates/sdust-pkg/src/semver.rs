//! Tiny semver-requirement matcher.
//!
//! We do *not* depend on the `semver` crate — v0.2's resolver only
//! understands a small subset, and pulling a new dep just for it is
//! unnecessary. The subset:
//!
//! - `"1.2.3"` — exact match (interpreted as `=1.2.3`).
//! - `"=1.2.3"` — exact match.
//! - `"^1.2"` — caret: same leading non-zero component
//!   (`^1.2` matches `1.x.y` with `x.y >= 2.0`; `^0.1` matches
//!   `0.1.x`; `^0.0.3` matches only `0.0.3`).
//! - `"~1.2"` — tilde: same major+minor (`1.2.x`).
//! - `"1.2"` / `"1"` — bare partials, treated as `^1.2` / `^1`.
//! - `"*"` — wildcard.
//!
//! Pre-release tags and build metadata are not supported in v0.2.

use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Version {
    pub fn parse(s: &str) -> Result<Self, SemverError> {
        let s = s.trim();
        let parts: Vec<&str> = s.split('.').collect();
        if parts.is_empty() || parts.len() > 3 {
            return Err(SemverError::BadVersion(s.into()));
        }
        let major = parse_num(parts[0]).ok_or_else(|| SemverError::BadVersion(s.into()))?;
        let minor = parts
            .get(1)
            .map(|p| parse_num(p).ok_or_else(|| SemverError::BadVersion(s.into())))
            .transpose()?
            .unwrap_or(0);
        let patch = parts
            .get(2)
            .map(|p| parse_num(p).ok_or_else(|| SemverError::BadVersion(s.into())))
            .transpose()?
            .unwrap_or(0);
        Ok(Version {
            major,
            minor,
            patch,
        })
    }
}

fn parse_num(s: &str) -> Option<u64> {
    // Strip any +build suffix (we ignore build metadata).
    let s = s.split('+').next().unwrap_or(s);
    // Reject pre-release tags for v0.2.
    if s.contains('-') {
        return None;
    }
    s.parse().ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionReq {
    Wildcard,
    Exact(Version),
    /// `^X.Y.Z` semantics: same left-most non-zero component, version
    /// >= the floor.
    Caret(Version, CaretFloorWidth),
    /// `~X.Y.Z`: same major+minor.
    Tilde(Version),
}

/// How many components the user actually wrote in a caret req, e.g.
/// `^1` (Major), `^1.2` (Minor), `^1.2.3` (Patch). Affects the floor
/// comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretFloorWidth {
    Major,
    Minor,
    Patch,
}

impl VersionReq {
    pub fn parse(s: &str) -> Result<Self, SemverError> {
        let s = s.trim();
        if s == "*" {
            return Ok(VersionReq::Wildcard);
        }
        if let Some(rest) = s.strip_prefix('=') {
            return Ok(VersionReq::Exact(Version::parse(rest.trim())?));
        }
        if let Some(rest) = s.strip_prefix('^') {
            let (v, w) = parse_partial(rest.trim())?;
            return Ok(VersionReq::Caret(v, w));
        }
        if let Some(rest) = s.strip_prefix('~') {
            let (v, _) = parse_partial(rest.trim())?;
            return Ok(VersionReq::Tilde(v));
        }
        // Bare partial — treat like caret. Bare full version `1.2.3`
        // is also caret, matching cargo's behaviour.
        let (v, w) = parse_partial(s)?;
        Ok(VersionReq::Caret(v, w))
    }

    pub fn matches(&self, v: &Version) -> bool {
        match self {
            VersionReq::Wildcard => true,
            VersionReq::Exact(req) => req == v,
            VersionReq::Caret(req, width) => caret_matches(req, *width, v),
            VersionReq::Tilde(req) => v.major == req.major && v.minor == req.minor && v >= req,
        }
    }
}

fn parse_partial(s: &str) -> Result<(Version, CaretFloorWidth), SemverError> {
    let parts: Vec<&str> = s.split('.').collect();
    let major = parse_num(parts.first().copied().unwrap_or(""))
        .ok_or_else(|| SemverError::BadReq(s.into()))?;
    let (minor, has_minor) = match parts.get(1) {
        Some(p) => (
            parse_num(p).ok_or_else(|| SemverError::BadReq(s.into()))?,
            true,
        ),
        None => (0, false),
    };
    let (patch, has_patch) = match parts.get(2) {
        Some(p) => (
            parse_num(p).ok_or_else(|| SemverError::BadReq(s.into()))?,
            true,
        ),
        None => (0, false),
    };
    let width = match (has_minor, has_patch) {
        (false, _) => CaretFloorWidth::Major,
        (true, false) => CaretFloorWidth::Minor,
        (true, true) => CaretFloorWidth::Patch,
    };
    Ok((
        Version {
            major,
            minor,
            patch,
        },
        width,
    ))
}

fn caret_matches(req: &Version, width: CaretFloorWidth, v: &Version) -> bool {
    // Cargo-style caret. Compatibility ceiling raised by the leftmost
    // non-zero component.
    if v < req {
        return false;
    }
    if req.major > 0 {
        return v.major == req.major;
    }
    if req.minor > 0 {
        return v.major == 0 && v.minor == req.minor;
    }
    // major=0, minor=0
    match width {
        CaretFloorWidth::Patch => v.major == 0 && v.minor == 0 && v.patch == req.patch,
        // `^0` / `^0.0` are uncommon but cargo allows any 0.x.y.
        CaretFloorWidth::Major => v.major == 0,
        CaretFloorWidth::Minor => v.major == 0 && v.minor == 0,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SemverError {
    #[error("invalid version `{0}`")]
    BadVersion(String),
    #[error("invalid version requirement `{0}`")]
    BadReq(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(
            Version::parse("1.2.3").unwrap(),
            Version {
                major: 1,
                minor: 2,
                patch: 3
            }
        );
        assert_eq!(
            Version::parse("0.1").unwrap(),
            Version {
                major: 0,
                minor: 1,
                patch: 0
            }
        );
        assert!(Version::parse("a.b").is_err());
    }

    #[test]
    fn caret_matches_cargo_semantics() {
        let req = VersionReq::parse("^1.2.3").unwrap();
        assert!(req.matches(&Version::parse("1.2.3").unwrap()));
        assert!(req.matches(&Version::parse("1.9.0").unwrap()));
        assert!(!req.matches(&Version::parse("2.0.0").unwrap()));
        assert!(!req.matches(&Version::parse("1.2.2").unwrap()));

        let req = VersionReq::parse("^0.1").unwrap();
        assert!(req.matches(&Version::parse("0.1.5").unwrap()));
        assert!(!req.matches(&Version::parse("0.2.0").unwrap()));

        let req = VersionReq::parse("^0.0.3").unwrap();
        assert!(req.matches(&Version::parse("0.0.3").unwrap()));
        assert!(!req.matches(&Version::parse("0.0.4").unwrap()));
    }

    #[test]
    fn exact_and_wildcard() {
        let req = VersionReq::parse("=1.2.3").unwrap();
        assert!(req.matches(&Version::parse("1.2.3").unwrap()));
        assert!(!req.matches(&Version::parse("1.2.4").unwrap()));

        let req = VersionReq::parse("*").unwrap();
        assert!(req.matches(&Version::parse("99.0.0").unwrap()));
    }

    #[test]
    fn tilde_matches_minor_band() {
        let req = VersionReq::parse("~1.2").unwrap();
        assert!(req.matches(&Version::parse("1.2.0").unwrap()));
        assert!(req.matches(&Version::parse("1.2.9").unwrap()));
        assert!(!req.matches(&Version::parse("1.3.0").unwrap()));
    }
}
