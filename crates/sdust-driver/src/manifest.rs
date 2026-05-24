//! Package manifest (`star.toml`) types and loader.
//!
//! v0.1 shipped a minimal `Manifest { package, deps: BTreeMap<String,
//! String> }`. v0.2 extends `[deps]` to support detailed source
//! specifications (path / git / registry) and adds an optional `[build]`
//! section for the build-script sandbox scaffold (spec §5.4).
//!
//! The flexible `Dep` enum uses serde's untagged form so both
//! ```toml
//! foo = "0.1"
//! ```
//! and
//! ```toml
//! foo = { version = "0.1", path = "../foo" }
//! ```
//! parse correctly. The simple string form is canonicalised to
//! `Dep::Version(_)`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub package: Package,
    #[serde(default)]
    pub deps: BTreeMap<String, Dep>,
    /// Build-script sandbox scaffold (spec §5.4).
    ///
    /// Enforcement is deferred to a post-v0.2 slice; v0.2 only parses
    /// and records the section so manifests with `[build]` continue to
    /// load.
    #[serde(default)]
    pub build: Option<BuildConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub edition: String,
    #[serde(default = "default_profile")]
    pub profile: String,
}

fn default_profile() -> String {
    "host".into()
}

/// Dependency specification.
///
/// Accepts either a bare version string (`foo = "0.1"`) or a detailed
/// table with any combination of `version`, `path`, `git`, `rev`, and
/// `hash`. Exactly one *source* is expected (registry-via-`version`,
/// `path`, or `git`); the resolver validates this.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Dep {
    Version(String),
    Detailed(DetailedDep),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DetailedDep {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// Pre-computed sha256 of the dependency contents (lockfile pinning).
    ///
    /// `sha256:<hex>` form. Optional in `star.toml`; required in
    /// `star.lock` for fetch-time verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

impl Dep {
    /// Return the version requirement string, if any.
    pub fn version(&self) -> Option<&str> {
        match self {
            Dep::Version(v) => Some(v),
            Dep::Detailed(d) => d.version.as_deref(),
        }
    }

    /// Return the local-path source, if any.
    pub fn path(&self) -> Option<&str> {
        match self {
            Dep::Version(_) => None,
            Dep::Detailed(d) => d.path.as_deref(),
        }
    }

    /// Return the git URL, if any.
    pub fn git(&self) -> Option<&str> {
        match self {
            Dep::Version(_) => None,
            Dep::Detailed(d) => d.git.as_deref(),
        }
    }

    /// Return the git rev (commit / tag / branch), if any.
    pub fn rev(&self) -> Option<&str> {
        match self {
            Dep::Version(_) => None,
            Dep::Detailed(d) => d.rev.as_deref(),
        }
    }

    /// Pre-computed sha256 hash, if recorded.
    pub fn hash(&self) -> Option<&str> {
        match self {
            Dep::Version(_) => None,
            Dep::Detailed(d) => d.hash.as_deref(),
        }
    }

    /// Construct a simple registry-version dependency.
    pub fn from_version(v: impl Into<String>) -> Self {
        Dep::Version(v.into())
    }

    /// Classify which source this dependency draws from.
    pub fn source_kind(&self) -> DepSourceKind {
        match self {
            Dep::Version(_) => DepSourceKind::Registry,
            Dep::Detailed(d) => {
                if d.path.is_some() {
                    DepSourceKind::Path
                } else if d.git.is_some() {
                    DepSourceKind::Git
                } else {
                    DepSourceKind::Registry
                }
            }
        }
    }
}

/// Which kind of fetcher should handle this dep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepSourceKind {
    Registry,
    Path,
    Git,
}

/// Build-script sandbox configuration scaffold (spec §5.4).
///
/// Lists the network domains and filesystem paths a `build.sd` script
/// is allowed to access. v0.2 only parses + records; enforcement is
/// deferred to a later slice. Documented in
/// `docs/internals/package-manager.md`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BuildConfig {
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub allow_net: Vec<String>,
    #[serde(default)]
    pub allow_fs: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("read error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
}

pub fn load(path: &std::path::Path) -> Result<Manifest, ManifestError> {
    let src = std::fs::read_to_string(path)?;
    let m: Manifest = toml::from_str(&src)?;
    Ok(m)
}

/// Serialize a `Manifest` back to TOML.
///
/// Used by `sdust pkg add` / `sdust pkg remove` to round-trip the
/// manifest without losing fields. Comments and key ordering are *not*
/// preserved — this is a coarse-grained MVP rewrite.
pub fn save(m: &Manifest, path: &std::path::Path) -> Result<(), ManifestError> {
    let text = toml::to_string_pretty(m)?;
    std::fs::write(path, text)?;
    Ok(())
}
