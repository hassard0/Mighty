//! `std.fs` — capability-gated filesystem ops.
//!
//! Each function takes an opaque `Fs` capability value (constructed by
//! the runtime when the agent's manifest grants `fs` access). At the
//! Rust layer the cap is represented as a `FsCap` newtype carrying the
//! allowed roots; we reuse the runtime's `BudgetTracker::check_*_path`
//! semantics so policy stays in one place.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum IoErr {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("denied: path {0} outside capability roots")]
    Denied(String),
    #[error("not utf-8: {0}")]
    Utf8(String),
}

/// Filesystem capability. `allowed` lists prefix paths the cap permits;
/// an empty list means "no restriction" (used by tests + fully-trusted
/// CLI entry points). Operations that touch paths outside any allowed
/// prefix fail with `IoErr::Denied`.
#[derive(Debug, Clone)]
pub struct FsCap {
    pub allowed: Vec<PathBuf>,
}

impl FsCap {
    pub fn unrestricted() -> Self {
        Self { allowed: vec![] }
    }
    pub fn rooted<P: Into<PathBuf>>(roots: impl IntoIterator<Item = P>) -> Self {
        Self {
            allowed: roots.into_iter().map(Into::into).collect(),
        }
    }
    fn check(&self, path: &Path) -> Result<(), IoErr> {
        if self.allowed.is_empty() {
            return Ok(());
        }
        let ok = self.allowed.iter().any(|r| path.starts_with(r));
        if ok {
            Ok(())
        } else {
            Err(IoErr::Denied(path.display().to_string()))
        }
    }
}

/// Read a file as bytes.
pub fn read(cap: &FsCap, path: &Path) -> Result<Vec<u8>, IoErr> {
    cap.check(path)?;
    Ok(std::fs::read(path)?)
}

/// Write bytes to a file (creates or truncates).
pub fn write(cap: &FsCap, path: &Path, data: &[u8]) -> Result<(), IoErr> {
    cap.check(path)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, data)?;
    Ok(())
}

/// Return true iff `path` exists.
pub fn exists(cap: &FsCap, path: &Path) -> bool {
    if cap.check(path).is_err() {
        return false;
    }
    path.exists()
}

/// List the entries of a directory. Order is lexicographic by file name
/// so callers get deterministic results (matches `std.test`'s discovery
/// contract).
pub fn list_dir(cap: &FsCap, path: &Path) -> Result<Vec<PathBuf>, IoErr> {
    cap.check(path)?;
    let mut entries: Vec<PathBuf> = std::fs::read_dir(path)?
        .filter_map(|e| e.ok().map(|d| d.path()))
        .collect();
    entries.sort();
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_unrestricted_allows() {
        let cap = FsCap::unrestricted();
        assert!(cap.check(Path::new("/tmp/whatever")).is_ok());
    }

    #[test]
    fn check_rooted_denies_outside() {
        let cap = FsCap::rooted(["/tmp/allowed"]);
        assert!(cap.check(Path::new("/tmp/allowed/x")).is_ok());
        assert!(cap.check(Path::new("/tmp/elsewhere")).is_err());
    }
}
