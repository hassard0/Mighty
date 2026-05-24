//! Fetchers materialise a resolved package source onto local disk
//! under `.mighty/pkgs/<name>-<version-or-rev>/` and return its
//! sha256.
//!
//! The trait `Fetcher` is intentionally narrow so future kinds (e.g. a
//! signed-bundle source) plug in without touching callers. v0.2 ships
//! three fetchers:
//!
//! - [`path`] — copy a local path into the cache.
//! - [`git`] — clone a git repo + checkout a rev (via `git2`).
//! - [`registry`] — fetch + extract a `.tar.gz` from the registry
//!   (HTTP, via `reqwest::blocking`). v0.2's registry is **not yet
//!   live** — the fetcher returns a clear error until it is.

pub mod git;
pub mod path;
pub mod registry;

use crate::lockfile::LockedPackage;
use std::path::PathBuf;

/// Fetch outcome: the directory we materialised into, plus the sha256
/// hash of its contents.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub root: PathBuf,
    pub hash: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("hash mismatch for {name}: expected {expected}, got {actual}")]
    HashMismatch {
        name: String,
        expected: String,
        actual: String,
    },
    #[error("unsupported source `{0}`")]
    UnsupportedSource(String),
    #[error("registry fetch failed: {0}")]
    Registry(String),
    #[error("git fetch failed: {0}")]
    Git(String),
}

/// Compute the on-disk slot under `<root>/.mighty/pkgs/` for a
/// locked package. Stable per (name, version), so re-running `fetch`
/// is idempotent.
pub fn package_slot(repo_root: &std::path::Path, locked: &LockedPackage) -> PathBuf {
    repo_root
        .join(".mighty")
        .join("pkgs")
        .join(format!("{}-{}", locked.name, locked.version))
}

/// Dispatch on `locked.source` and run the appropriate fetcher.
pub fn fetch_one(
    repo_root: &std::path::Path,
    locked: &LockedPackage,
) -> Result<Fetched, FetchError> {
    let slot = package_slot(repo_root, locked);
    if let Some(parent) = slot.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if locked.source.starts_with("path+") {
        path::fetch(locked, &slot)
    } else if locked.source.starts_with("git+") {
        git::fetch(locked, &slot)
    } else if locked.source.starts_with("registry+") {
        registry::fetch(locked, &slot)
    } else {
        Err(FetchError::UnsupportedSource(locked.source.clone()))
    }
}
