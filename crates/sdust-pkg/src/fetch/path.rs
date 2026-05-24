//! Local path fetcher: copy the source tree into the package cache.
//!
//! On Windows symlinking requires elevated privileges, so we just
//! copy. On Unix we could prefer symlink for speed but copy keeps
//! cross-platform behaviour identical.

use super::{FetchError, Fetched};
use crate::hash;
use crate::lockfile::LockedPackage;
use std::path::Path;

pub fn fetch(locked: &LockedPackage, slot: &Path) -> Result<Fetched, FetchError> {
    let src = source_path(&locked.source)?;
    if !src.exists() {
        return Err(FetchError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("path dep source not found: {}", src.display()),
        )));
    }

    // Wipe + recopy to keep the slot in sync with source.
    if slot.exists() {
        std::fs::remove_dir_all(slot)?;
    }
    std::fs::create_dir_all(slot)?;
    copy_dir_recursive(&src, slot)?;

    let actual_hash = hash::hash_tree(slot)?;
    if let Some(expected) = &locked.hash {
        if expected != &actual_hash {
            return Err(FetchError::HashMismatch {
                name: locked.name.clone(),
                expected: expected.clone(),
                actual: actual_hash,
            });
        }
    }
    Ok(Fetched {
        root: slot.to_path_buf(),
        hash: actual_hash,
    })
}

fn source_path(source: &str) -> Result<std::path::PathBuf, FetchError> {
    let rest = source
        .strip_prefix("path+file:///")
        .or_else(|| source.strip_prefix("path+file://"))
        .ok_or_else(|| FetchError::UnsupportedSource(source.into()))?;
    let cleaned = rest.trim_start_matches('/');
    // On Windows the canonical form is `path+file:///C:/foo/bar`; we
    // stripped `path+file:///` so `cleaned` is `C:/foo/bar`. On Unix
    // it's `path+file:///abs/path` → `abs/path` → must reabsoluteise.
    let candidate = std::path::PathBuf::from(cleaned);
    if candidate.is_absolute() {
        Ok(candidate)
    } else {
        // Treat as Unix absolute.
        Ok(std::path::PathBuf::from(format!("/{cleaned}")))
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let name = entry.file_name();
        let name_s = name.to_string_lossy();
        // Skip vcs / build artefacts to keep the cache lean and the
        // tree hash deterministic across machines.
        if name_s == ".git" || name_s == "target" {
            continue;
        }
        if ft.is_dir() {
            std::fs::create_dir_all(&to)?;
            copy_dir_recursive(&from, &to)?;
        } else if ft.is_file() {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lockfile::LockedPackage;

    #[test]
    fn round_trips_source_url() {
        // Picks the right side of the prefix on both platforms.
        let p = std::path::PathBuf::from("/tmp/foo");
        let url = LockedPackage::path_source(&p);
        assert!(url.starts_with("path+file:///"));
        let back = source_path(&url).unwrap();
        // On unix, "/tmp/foo" round-trips; on windows the leading
        // slash gets re-attached so we just check the suffix.
        let s = back.to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("tmp/foo"));
    }
}
