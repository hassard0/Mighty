//! `sdust pkg publish` bundler.
//!
//! v0.2's registry is not yet live, so `publish` produces the bundle
//! tarball + sha256 locally and points the user at it. A later slice
//! will switch on an actual upload.
//!
//! v0.2 ships a *deterministic* uncompressed tar-like bundle: a tiny
//! header-per-file format consisting of
//! `<path-len:u32-le><path-bytes><body-len:u64-le><body-bytes>`
//! repeated. This avoids pulling in `tar` + `flate2` for the v0.2
//! slice; the on-disk artifact is still labelled `.tar.gz` for
//! forward-compat (the registry upload step will re-encode it).

use crate::hash;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("missing manifest at {0}")]
    NoManifest(PathBuf),
    #[error("manifest error: {0}")]
    Manifest(#[from] sdust_driver::manifest::ManifestError),
}

#[derive(Debug, Clone)]
pub struct PublishOutcome {
    pub bundle_path: PathBuf,
    pub hash: String,
}

pub fn publish(repo_root: &Path) -> Result<PublishOutcome, PublishError> {
    let manifest_path = repo_root.join(crate::MANIFEST_NAME);
    if !manifest_path.exists() {
        return Err(PublishError::NoManifest(manifest_path));
    }
    let manifest = sdust_driver::manifest::load(&manifest_path)?;

    let out_dir = repo_root.join(".stardust").join("publish");
    std::fs::create_dir_all(&out_dir)?;
    let bundle_path = out_dir.join(format!(
        "{}-{}.tar.gz",
        manifest.package.name, manifest.package.version
    ));

    let mut entries = Vec::new();
    collect_publishable(repo_root, repo_root, &mut entries)?;
    entries.sort();

    // Determinism: keep buffer in memory so we can hash the exact bytes.
    let mut buf = Vec::new();
    for rel in &entries {
        let body = std::fs::read(repo_root.join(rel))?;
        let path_norm = rel.replace('\\', "/");
        let plen = path_norm.len() as u32;
        buf.write_all(&plen.to_le_bytes())?;
        buf.write_all(path_norm.as_bytes())?;
        let blen = body.len() as u64;
        buf.write_all(&blen.to_le_bytes())?;
        buf.write_all(&body)?;
    }
    std::fs::write(&bundle_path, &buf)?;

    let h = hash::hash_bytes(&buf);
    Ok(PublishOutcome {
        bundle_path,
        hash: h,
    })
}

fn collect_publishable(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let path = entry.path();
        let name = entry.file_name();
        let name_s = name.to_string_lossy();
        // Exclude vcs, build output, and our own cache + publish dir.
        if name_s == ".git" || name_s == "target" || name_s == ".stardust" {
            continue;
        }
        if ft.is_dir() {
            collect_publishable(root, &path, out)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_bundle_and_hash() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("star.toml"),
            br#"
[package]
name = "demo"
version = "0.1.0"
edition = "2026"
"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("main.sd"), b"fn main() {}").unwrap();

        let out = publish(dir.path()).unwrap();
        assert!(out.bundle_path.exists());
        assert!(out.hash.starts_with("sha256:"));
        // Re-run is deterministic.
        let out2 = publish(dir.path()).unwrap();
        assert_eq!(out.hash, out2.hash);
    }
}
