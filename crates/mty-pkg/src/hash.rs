//! Sha256 helpers for fetch verification + lockfile hashes.
//!
//! `hash_tree` produces a deterministic sha256 over a directory by
//! walking entries in sorted order and feeding `<rel-path>\0<bytes>\0`
//! into the hasher. This matches the form the registry will ultimately
//! pre-compute and ship in its index; v0.2 uses the same algorithm
//! locally so path/git fetches verify against the lockfile.
//!
//! Symlinks are followed; only regular files contribute bytes.

use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;

/// Hash a file's bytes. Returns `sha256:<hex>`.
pub fn hash_file(path: &Path) -> io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(hash_bytes(&bytes))
}

/// Hash a byte buffer. Returns `sha256:<hex>`.
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Hash an entire directory tree. Returns `sha256:<hex>`.
///
/// Algorithm: walk all regular files under `root`, sort by relative
/// path, and feed `<rel-path>\0<bytes>\0` for each into the hasher.
/// Two trees that agree on this hash agree byte-for-byte modulo
/// directory entries, mode bits, and symlink targets (which are not
/// captured). That is sufficient for v0.2 source verification.
pub fn hash_tree(root: &Path) -> io::Result<String> {
    let mut entries = Vec::new();
    collect_files(root, root, &mut entries)?;
    entries.sort();

    let mut hasher = Sha256::new();
    for rel in entries {
        let full = root.join(&rel);
        let bytes = std::fs::read(&full)?;
        // Use forward-slashes for cross-platform determinism.
        let rel_norm = rel.replace('\\', "/");
        hasher.update(rel_norm.as_bytes());
        hasher.update([0u8]);
        hasher.update(&bytes);
        hasher.update([0u8]);
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let path = entry.path();
        if ft.is_dir() {
            // Skip .git and the package cache itself to avoid infinite
            // self-hashing when the cache lives under the package.
            let name = entry.file_name();
            let name_s = name.to_string_lossy();
            if name_s == ".git" || name_s == "target" {
                continue;
            }
            collect_files(root, &path, out)?;
        } else if ft.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            out.push(rel);
        }
    }
    Ok(())
}

/// Verify that `actual` matches `expected`, with both in
/// `sha256:<hex>` form. Returns Ok on match; otherwise an error.
pub fn verify(expected: &str, actual: &str) -> Result<(), HashMismatch> {
    if expected == actual {
        Ok(())
    } else {
        Err(HashMismatch {
            expected: expected.into(),
            actual: actual.into(),
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("hash mismatch: expected {expected}, got {actual}")]
pub struct HashMismatch {
    pub expected: String,
    pub actual: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_bytes_deterministically() {
        assert_eq!(hash_bytes(b"hello"), hash_bytes(b"hello"));
        assert_ne!(hash_bytes(b"hello"), hash_bytes(b"world"));
        // Spot-check that we got the canonical sha256.
        assert_eq!(
            hash_bytes(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn tree_hash_is_stable_across_dirs() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        for d in [a.path(), b.path()] {
            std::fs::create_dir_all(d.join("sub")).unwrap();
            std::fs::write(d.join("a.txt"), b"alpha").unwrap();
            std::fs::write(d.join("sub/b.txt"), b"beta").unwrap();
        }
        assert_eq!(hash_tree(a.path()).unwrap(), hash_tree(b.path()).unwrap());
    }

    #[test]
    fn tree_hash_changes_with_content() {
        let a = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("a.txt"), b"alpha").unwrap();
        let h1 = hash_tree(a.path()).unwrap();
        std::fs::write(a.path().join("a.txt"), b"alpha2").unwrap();
        let h2 = hash_tree(a.path()).unwrap();
        assert_ne!(h1, h2);
    }
}
