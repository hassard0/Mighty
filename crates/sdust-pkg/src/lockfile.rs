//! `star.lock` — content-addressed dependency lockfile.
//!
//! Format (TOML):
//!
//! ```toml
//! version = 1
//!
//! [[package]]
//! name = "std"
//! version = "0.1.0"
//! source = "registry+https://pkg.stardust.dev"
//! hash = "sha256:abc..."
//! dependencies = []
//!
//! [[package]]
//! name = "localdep"
//! version = "0.1.0"
//! source = "path+file:///abs/path"
//! hash = "sha256:..."
//! dependencies = []
//! ```
//!
//! The `version` field versions the *lockfile schema*, not the
//! package. v0.2 emits and accepts only schema version `1`.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Hard-coded registry URL prefix used in lockfile `source = ...`
/// values for registry-sourced packages.
pub const DEFAULT_REGISTRY: &str = "https://pkg.stardust.dev";

/// Current lockfile schema version. Bumped only on incompatible
/// format changes.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Lockfile {
    pub version: u32,
    #[serde(default, rename = "package")]
    pub packages: Vec<LockedPackage>,
}

impl Default for Lockfile {
    fn default() -> Self {
        Self::new()
    }
}

impl Lockfile {
    pub fn new() -> Self {
        Lockfile {
            version: SCHEMA_VERSION,
            packages: Vec::new(),
        }
    }

    /// Look up a locked package by name.
    pub fn find(&self, name: &str) -> Option<&LockedPackage> {
        self.packages.iter().find(|p| p.name == name)
    }

    /// Insert or replace a locked package entry.
    pub fn upsert(&mut self, pkg: LockedPackage) {
        match self.packages.iter_mut().find(|p| p.name == pkg.name) {
            Some(existing) => *existing = pkg,
            None => self.packages.push(pkg),
        }
        // Keep deterministic ordering.
        self.packages.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Remove a package and return whether one was found.
    pub fn remove(&mut self, name: &str) -> bool {
        let len_before = self.packages.len();
        self.packages.retain(|p| p.name != name);
        self.packages.len() < len_before
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    /// Canonical source URL of the form
    /// `<kind>+<url>` where `<kind>` is `registry` / `path` / `git`.
    pub source: String,
    /// Content hash of the resolved package tree.
    ///
    /// Optional in lockfiles produced before a fetcher has run (e.g.
    /// `pkg add` against an unreachable registry); required to be
    /// non-empty by `pkg fetch` verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Names of direct dependencies (resolved entries in this same
    /// lockfile).
    #[serde(default)]
    pub dependencies: Vec<String>,
}

impl LockedPackage {
    pub fn registry_source(registry: &str) -> String {
        format!("registry+{}", registry)
    }

    pub fn path_source(path: &Path) -> String {
        // Use forward slashes for cross-platform determinism.
        let p = path.to_string_lossy().replace('\\', "/");
        format!("path+file:///{}", p.trim_start_matches('/'))
    }

    pub fn git_source(url: &str, rev: Option<&str>) -> String {
        match rev {
            Some(r) => format!("git+{}#{}", url, r),
            None => format!("git+{}", url),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LockfileError {
    #[error("read error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("unsupported lockfile schema version {0}; this toolchain understands {1}")]
    UnsupportedVersion(u32, u32),
}

pub fn load(path: &Path) -> Result<Lockfile, LockfileError> {
    let src = std::fs::read_to_string(path)?;
    let lock: Lockfile = toml::from_str(&src)?;
    if lock.version != SCHEMA_VERSION {
        return Err(LockfileError::UnsupportedVersion(
            lock.version,
            SCHEMA_VERSION,
        ));
    }
    Ok(lock)
}

pub fn save(lock: &Lockfile, path: &Path) -> Result<(), LockfileError> {
    let text = toml::to_string_pretty(lock)?;
    std::fs::write(path, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_empty_lockfile() {
        let lock = Lockfile::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("star.lock");
        save(&lock, &path).unwrap();
        let reloaded = load(&path).unwrap();
        assert_eq!(reloaded.version, SCHEMA_VERSION);
        assert!(reloaded.packages.is_empty());
    }

    #[test]
    fn roundtrips_with_packages() {
        let mut lock = Lockfile::new();
        lock.upsert(LockedPackage {
            name: "std".into(),
            version: "0.1.0".into(),
            source: LockedPackage::registry_source(DEFAULT_REGISTRY),
            hash: Some("sha256:abc".into()),
            dependencies: vec![],
        });
        lock.upsert(LockedPackage {
            name: "otel".into(),
            version: "0.1.0".into(),
            source: LockedPackage::registry_source(DEFAULT_REGISTRY),
            hash: Some("sha256:def".into()),
            dependencies: vec!["std".into()],
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("star.lock");
        save(&lock, &path).unwrap();
        let reloaded = load(&path).unwrap();
        assert_eq!(reloaded.packages.len(), 2);
        // Deterministic sort by name.
        assert_eq!(reloaded.packages[0].name, "otel");
        assert_eq!(reloaded.packages[1].name, "std");
    }

    #[test]
    fn rejects_unsupported_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("star.lock");
        std::fs::write(&path, "version = 999\n").unwrap();
        assert!(matches!(
            load(&path),
            Err(LockfileError::UnsupportedVersion(999, 1))
        ));
    }
}
