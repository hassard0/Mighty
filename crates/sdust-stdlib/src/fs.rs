//! `std.fs` — capability-gated filesystem ops.
//!
//! Each function takes an opaque `Fs` capability value (constructed by
//! the runtime when the agent's manifest grants `fs` access). At the
//! Rust layer the cap is represented as a `FsCap` newtype carrying the
//! allowed roots; we reuse the runtime's `BudgetTracker::check_*_path`
//! semantics so policy stays in one place.
//!
//! ## v0.5 dogfood Gap-5 — process-wide default caps
//!
//! Stardust source code today calls `std.fs.read("./in")` without
//! constructing an explicit `Fs` cap value (the lowerer doesn't yet
//! materialise per-call caps from the agent's sandbox manifest). To
//! still enforce the manifest's `fs.read = [...]` allow-list, the
//! driver installs a process-wide [`FsCap`] via
//! [`install_default_read_cap`] / [`install_default_write_cap`]. The
//! `host` dispatcher consults that cap on every `fs.*` call.

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

#[derive(Debug, thiserror::Error)]
pub enum IoErr {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("denied: path {0} outside capability roots")]
    Denied(String),
    /// v0.5 Gap-5 — symmetric with `Denied` but emitted when the cap
    /// is the process-wide default installed from a sandbox manifest.
    /// `Denied` historically came from in-source cap narrowing;
    /// keeping both lets older tests pattern-match on either.
    #[error("forbidden: path {0} outside sandbox allow-list")]
    Forbidden(String),
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
    /// v0.5 Gap-5 — public allowlist query used by callers that want
    /// to short-circuit before touching the filesystem.
    pub fn allows(&self, path: &Path) -> bool {
        self.check(path).is_ok()
    }
    fn check(&self, path: &Path) -> Result<(), IoErr> {
        if self.allowed.is_empty() {
            return Ok(());
        }
        let ok = self.allowed.iter().any(|r| path.starts_with(r));
        if ok {
            Ok(())
        } else {
            Err(IoErr::Forbidden(path.display().to_string()))
        }
    }
}

// ----- v0.5 Gap-5 process-wide default caps ----------------------------

static DEFAULT_READ_CAP: OnceLock<RwLock<FsCap>> = OnceLock::new();
static DEFAULT_WRITE_CAP: OnceLock<RwLock<FsCap>> = OnceLock::new();

fn default_read_slot() -> &'static RwLock<FsCap> {
    DEFAULT_READ_CAP.get_or_init(|| RwLock::new(FsCap::unrestricted()))
}
fn default_write_slot() -> &'static RwLock<FsCap> {
    DEFAULT_WRITE_CAP.get_or_init(|| RwLock::new(FsCap::unrestricted()))
}

/// Install the process-wide default read cap, returning the previous
/// one (so tests can save+restore around a scoped override). The
/// `host::fs_read` / `fs_list_dir` / `fs_exists` dispatchers consult
/// this cap when the call shape doesn't carry an explicit one.
pub fn install_default_read_cap(cap: FsCap) -> FsCap {
    let mut g = default_read_slot()
        .write()
        .expect("DEFAULT_READ_CAP poisoned");
    std::mem::replace(&mut *g, cap)
}

/// Install the process-wide default write cap. Companion to
/// [`install_default_read_cap`].
pub fn install_default_write_cap(cap: FsCap) -> FsCap {
    let mut g = default_write_slot()
        .write()
        .expect("DEFAULT_WRITE_CAP poisoned");
    std::mem::replace(&mut *g, cap)
}

/// Snapshot the current default read cap (clone; the lock is not held
/// across the host's actual fs call).
pub fn current_default_read_cap() -> FsCap {
    default_read_slot()
        .read()
        .expect("DEFAULT_READ_CAP poisoned")
        .clone()
}

pub fn current_default_write_cap() -> FsCap {
    default_write_slot()
        .read()
        .expect("DEFAULT_WRITE_CAP poisoned")
        .clone()
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
