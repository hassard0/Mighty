//! `std.fs` — capability-gated filesystem ops.
//!
//! Each function takes an opaque `Fs` capability value (constructed by
//! the runtime when the agent's manifest grants `fs` access). At the
//! Rust layer the cap is represented as a `FsCap` newtype carrying the
//! allowed roots; we reuse the runtime's `BudgetTracker::check_*_path`
//! semantics so policy stays in one place.
//!
//! ## Backend dispatch (v0.16 P2 direct lowering)
//!
//! When a program is compiled with `--wasi=p2` (the default since
//! v0.15), `std.fs.*` calls now lower to **direct** P2 imports of
//! the `wasi:filesystem/types@0.2.3` resource methods instead of
//! routing through the `wasi_snapshot_preview1` adapter. The
//! canonical import shapes are exposed below as
//! [`P2_DIRECT_IMPORT_OPEN_AT`] / [`P2_DIRECT_IMPORT_READ_VIA_STREAM`]
//! / [`P2_DIRECT_IMPORT_WRITE_VIA_STREAM`] / [`P2_DIRECT_IMPORT_STAT`]
//! / [`P2_DIRECT_IMPORT_CLOSE`] — they match the variants of
//! `mty_codegen_wasm::P2DirectImport` and are pinned here so the
//! stdlib and codegen layers never drift on naming.
//!
//! The v0.16 emitter wiring is conservative: the SIR layer hasn't
//! yet lifted preopened-descriptor handles into the call shape, so
//! the open + drop scaffold around `read_via_stream` is emitted as
//! placeholder `(handle=0)` arguments — what's PINNED is that the
//! versioned import lands in the import section (no
//! `wasi_snapshot_preview1` hop) and the component validates.
//! Full descriptor lifecycle is a v0.17 follow-up.
//!
//! The native runtime path is unchanged — the import-shape switch
//! is purely a Wasm-side concern.
//!
//! ## v0.5 dogfood Gap-5 — process-wide default caps
//!
//! Mighty source code today calls `std.fs.read("./in")` without
//! constructing an explicit `Fs` cap value (the lowerer doesn't yet
//! materialise per-call caps from the agent's sandbox manifest). To
//! still enforce the manifest's `fs.read = [...]` allow-list, the
//! driver installs a process-wide [`FsCap`] via
//! [`install_default_read_cap`] / [`install_default_write_cap`]. The
//! `host` dispatcher consults that cap on every `fs.*` call.

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

/// Canonical P2 import name for `descriptor.open-at`. See module doc
/// for the v0.16 dispatch rationale.
pub const P2_DIRECT_IMPORT_OPEN_AT: (&str, &str) =
    ("wasi:filesystem/types@0.2.3", "[method]descriptor.open-at");

/// Canonical P2 import name for `descriptor.read-via-stream`. Wired
/// into the v0.16 emitter as the entry-point for `std.fs.read_file`.
pub const P2_DIRECT_IMPORT_READ_VIA_STREAM: (&str, &str) = (
    "wasi:filesystem/types@0.2.3",
    "[method]descriptor.read-via-stream",
);

/// Canonical P2 import name for `descriptor.write-via-stream`. Wired
/// into the v0.16 emitter as the entry-point for `std.fs.write_file`.
pub const P2_DIRECT_IMPORT_WRITE_VIA_STREAM: (&str, &str) = (
    "wasi:filesystem/types@0.2.3",
    "[method]descriptor.write-via-stream",
);

/// Canonical P2 import name for `descriptor.stat`.
pub const P2_DIRECT_IMPORT_STAT: (&str, &str) =
    ("wasi:filesystem/types@0.2.3", "[method]descriptor.stat");

/// Canonical P2 import name for the descriptor resource-drop
/// intrinsic (Mighty's `std.fs.close`).
pub const P2_DIRECT_IMPORT_CLOSE: (&str, &str) =
    ("wasi:filesystem/types@0.2.3", "[resource-drop]descriptor");

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
///
/// On `wasm32-wasi` builds with `--wasi=p2`, the Mighty codegen
/// lowers calls to this function to a direct
/// `wasi:filesystem/types@0.2.3.descriptor.read-via-stream` import
/// (see [`P2_DIRECT_IMPORT_READ_VIA_STREAM`]). The native runtime
/// path is unchanged.
pub fn read(cap: &FsCap, path: &Path) -> Result<Vec<u8>, IoErr> {
    cap.check(path)?;
    Ok(std::fs::read(path)?)
}

/// Alias for [`read`] — surfaced for parity with the codegen
/// extern-name (`std.fs.read_file`) so the v0.16 P2 dispatch
/// table has a one-to-one mapping between the extern path and the
/// stdlib entry point. Most call sites should prefer [`read`].
pub fn read_file(cap: &FsCap, path: &Path) -> Result<Vec<u8>, IoErr> {
    read(cap, path)
}

/// Write bytes to a file (creates or truncates).
///
/// On `wasm32-wasi` builds with `--wasi=p2`, the Mighty codegen
/// lowers calls to this function to a direct
/// `wasi:filesystem/types@0.2.3.descriptor.write-via-stream` import
/// (see [`P2_DIRECT_IMPORT_WRITE_VIA_STREAM`]). The native runtime
/// path is unchanged.
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

/// Alias for [`write`] — see [`read_file`] for the parallel
/// rationale (parity with the v0.16 extern-name `std.fs.write_file`).
pub fn write_file(cap: &FsCap, path: &Path, data: &[u8]) -> Result<(), IoErr> {
    write(cap, path, data)
}

/// Summary of filesystem metadata returned by [`stat`]. Mirrors the
/// subset of WASI's `descriptor-stat` record Mighty programs use
/// today (file size + a 3-state file-type discriminator).
#[derive(Debug, Clone, Copy)]
pub struct StatResult {
    /// Total size in bytes for regular files; `0` for directories
    /// and other types.
    pub size: u64,
    /// File-type enum: `0` = regular file, `1` = directory,
    /// `2` = symlink, `3` = other. Mirrors the WASI
    /// `descriptor-type` variant tags so the wasm-side decoder is
    /// trivially compatible.
    pub kind: u8,
}

/// Return basic metadata for `path`. Native path uses [`std::fs::metadata`];
/// on `wasm32-wasi` builds with `--wasi=p2` the Mighty codegen
/// lowers calls to this function to a direct
/// `wasi:filesystem/types@0.2.3.descriptor.stat` import (see
/// [`P2_DIRECT_IMPORT_STAT`]).
pub fn stat(cap: &FsCap, path: &Path) -> Result<StatResult, IoErr> {
    cap.check(path)?;
    let md = std::fs::metadata(path)?;
    let kind = if md.is_dir() {
        1
    } else if md.file_type().is_symlink() {
        2
    } else if md.is_file() {
        0
    } else {
        3
    };
    Ok(StatResult {
        size: md.len(),
        kind,
    })
}

/// Opaque file handle returned by [`open`]. On native this is a
/// thin wrapper around [`std::fs::File`]; on `wasm32-wasi` the
/// codegen lowers to a `wasi:filesystem` descriptor handle
/// (`i32` at the canonical-ABI boundary). Mighty programs treat
/// the value as opaque and pass it to [`close`].
#[derive(Debug)]
pub struct FileHandle {
    #[allow(dead_code)] // wasm32-wasi reads this as a descriptor index
    pub(crate) inner: std::fs::File,
}

/// Open `path` for read access. On `wasm32-wasi` builds with
/// `--wasi=p2`, the Mighty codegen lowers calls to this function to
/// a direct `wasi:filesystem/types@0.2.3.descriptor.open-at` import
/// (see [`P2_DIRECT_IMPORT_OPEN_AT`]).
pub fn open(cap: &FsCap, path: &Path) -> Result<FileHandle, IoErr> {
    cap.check(path)?;
    let inner = std::fs::File::open(path)?;
    Ok(FileHandle { inner })
}

/// Close a file handle obtained from [`open`]. On `wasm32-wasi` the
/// Mighty codegen lowers this to the canonical-ABI resource-drop
/// intrinsic for the descriptor resource (see
/// [`P2_DIRECT_IMPORT_CLOSE`]). On native this just drops the
/// `std::fs::File`.
pub fn close(_cap: &FsCap, handle: FileHandle) -> Result<(), IoErr> {
    drop(handle);
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

    #[test]
    fn p2_direct_import_constants_are_canonical() {
        // Pin the import shapes so a regression in either the
        // codegen layer or this stdlib doesn't drift them apart.
        assert_eq!(
            P2_DIRECT_IMPORT_OPEN_AT,
            ("wasi:filesystem/types@0.2.3", "[method]descriptor.open-at")
        );
        assert_eq!(
            P2_DIRECT_IMPORT_READ_VIA_STREAM,
            (
                "wasi:filesystem/types@0.2.3",
                "[method]descriptor.read-via-stream"
            )
        );
        assert_eq!(
            P2_DIRECT_IMPORT_WRITE_VIA_STREAM,
            (
                "wasi:filesystem/types@0.2.3",
                "[method]descriptor.write-via-stream"
            )
        );
        assert_eq!(
            P2_DIRECT_IMPORT_STAT,
            ("wasi:filesystem/types@0.2.3", "[method]descriptor.stat")
        );
        assert_eq!(
            P2_DIRECT_IMPORT_CLOSE,
            ("wasi:filesystem/types@0.2.3", "[resource-drop]descriptor")
        );
    }

    #[test]
    fn stat_returns_file_kind_zero_for_regular_file() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(tmp.path(), b"hello").unwrap();
        let cap = FsCap::unrestricted();
        let s = stat(&cap, tmp.path()).expect("stat");
        assert_eq!(s.kind, 0);
        assert_eq!(s.size, 5);
    }

    #[test]
    fn read_file_and_write_file_aliases_round_trip() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let cap = FsCap::unrestricted();
        write_file(&cap, tmp.path(), b"alpha").expect("write_file");
        let got = read_file(&cap, tmp.path()).expect("read_file");
        assert_eq!(got, b"alpha");
    }

    #[test]
    fn open_close_round_trip() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(tmp.path(), b"x").unwrap();
        let cap = FsCap::unrestricted();
        let h = open(&cap, tmp.path()).expect("open");
        close(&cap, h).expect("close");
    }
}
