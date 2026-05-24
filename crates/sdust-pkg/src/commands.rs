//! High-level operations behind `sdust pkg <subcmd>`.
//!
//! Each function takes the package root (the directory holding
//! `star.toml`) and returns either a printable summary string or an
//! error. The CLI wrapper in `sdust-cli` is a thin pass-through.

use crate::fetch::{self, Fetched};
use crate::lockfile::{self, Lockfile};
use crate::publish;
use crate::resolver::Resolver;
use sdust_driver::manifest::{self, Dep, DetailedDep, Manifest};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PkgError {
    #[error("manifest error: {0}")]
    Manifest(#[from] manifest::ManifestError),
    #[error("lockfile error: {0}")]
    Lockfile(#[from] lockfile::LockfileError),
    #[error("resolve error: {0}")]
    Resolve(#[from] crate::resolver::ResolveError),
    #[error("fetch error: {0}")]
    Fetch(#[from] fetch::FetchError),
    #[error("publish error: {0}")]
    Publish(#[from] publish::PublishError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("dep `{0}` not found in manifest")]
    DepNotFound(String),
    #[error("no star.toml at {0}")]
    NoManifest(PathBuf),
}

/// Resolve and write the lockfile to disk. Returns the lockfile.
pub fn resolve_and_lock(root: &Path) -> Result<Lockfile, PkgError> {
    let manifest = load_manifest(root)?;
    let resolver = Resolver::new(root);
    let lock = resolver.resolve(&manifest)?;
    lockfile::save(&lock, &root.join(crate::LOCKFILE_NAME))?;
    Ok(lock)
}

/// `sdust pkg add <name>[@version]`. Mutates `star.toml`, re-resolves,
/// writes the lockfile. Does *not* fetch — call [`fetch_all`] after.
pub fn add(root: &Path, name: &str, version: Option<&str>) -> Result<String, PkgError> {
    let mut manifest = load_manifest(root)?;
    let version = version.unwrap_or("*").to_string();
    manifest
        .deps
        .insert(name.to_string(), Dep::Version(version.clone()));
    manifest::save(&manifest, &root.join(crate::MANIFEST_NAME))?;
    let _ = resolve_and_lock(root)?;
    Ok(format!("added `{name}` = \"{version}\""))
}

/// `sdust pkg add <name> --path <p>` (or git). Same shape as `add`
/// but installs a detailed dep.
pub fn add_detailed(root: &Path, name: &str, detailed: DetailedDep) -> Result<String, PkgError> {
    let mut manifest = load_manifest(root)?;
    manifest
        .deps
        .insert(name.to_string(), Dep::Detailed(detailed));
    manifest::save(&manifest, &root.join(crate::MANIFEST_NAME))?;
    let _ = resolve_and_lock(root)?;
    Ok(format!("added `{name}` (detailed source)"))
}

/// `sdust pkg remove <name>`.
pub fn remove(root: &Path, name: &str) -> Result<String, PkgError> {
    let mut manifest = load_manifest(root)?;
    if manifest.deps.remove(name).is_none() {
        return Err(PkgError::DepNotFound(name.into()));
    }
    manifest::save(&manifest, &root.join(crate::MANIFEST_NAME))?;
    // Re-resolve so the lockfile drops orphans.
    let _ = resolve_and_lock(root)?;
    Ok(format!("removed `{name}`"))
}

/// `sdust pkg update [name]`. In v0.2 this re-runs the resolver,
/// which is enough to refresh path/git deps. With the registry
/// offline, "newest compatible" is whatever the resolver synthesises
/// (typically the requirement floor).
pub fn update(root: &Path, name: Option<&str>) -> Result<String, PkgError> {
    // For v0.2 we always re-resolve from scratch; the `name` filter
    // is informational. A registry-backed implementation will use it.
    let _ = resolve_and_lock(root)?;
    match name {
        Some(n) => Ok(format!("updated `{n}` (and re-resolved transitive deps)")),
        None => Ok("re-resolved all dependencies".into()),
    }
}

/// `sdust pkg fetch`. Walks the existing lockfile and runs each
/// fetcher. Updates the lockfile in-place to record hashes computed
/// on first fetch (when none was previously recorded).
pub fn fetch_all(root: &Path) -> Result<Vec<Fetched>, PkgError> {
    let lock_path = root.join(crate::LOCKFILE_NAME);
    let mut lock = if lock_path.exists() {
        lockfile::load(&lock_path)?
    } else {
        resolve_and_lock(root)?
    };

    let mut results = Vec::new();
    for pkg in lock.packages.clone() {
        let fetched = fetch::fetch_one(root, &pkg)?;
        // Pin the hash if it was previously empty.
        if pkg.hash.is_none() {
            let mut updated = pkg.clone();
            updated.hash = Some(fetched.hash.clone());
            lock.upsert(updated);
        }
        results.push(fetched);
    }
    lockfile::save(&lock, &lock_path)?;
    Ok(results)
}

/// `sdust pkg list`. Renders a simple tree of (name, version, source).
pub fn list(root: &Path) -> Result<String, PkgError> {
    let lock_path = root.join(crate::LOCKFILE_NAME);
    let lock = if lock_path.exists() {
        lockfile::load(&lock_path)?
    } else {
        resolve_and_lock(root)?
    };
    let manifest = load_manifest(root)?;
    let mut out = String::new();
    out.push_str(&format!(
        "{} v{}\n",
        manifest.package.name, manifest.package.version
    ));
    for pkg in &lock.packages {
        out.push_str(&format!(
            "├── {} v{} ({})\n",
            pkg.name,
            pkg.version,
            short_source(&pkg.source)
        ));
        for dep in &pkg.dependencies {
            out.push_str(&format!("│   └── {dep}\n"));
        }
    }
    if lock.packages.is_empty() {
        out.push_str("(no dependencies)\n");
    }
    Ok(out)
}

/// `sdust pkg publish`. Produces the bundle + sha256 and explains the
/// v0.2 caveat that the registry is not yet live.
pub fn publish(root: &Path) -> Result<String, PkgError> {
    let outcome = publish::publish(root)?;
    Ok(format!(
        "Stardust registry is not yet live; bundle prepared at `{}` ({})",
        outcome.bundle_path.display(),
        outcome.hash
    ))
}

fn load_manifest(root: &Path) -> Result<Manifest, PkgError> {
    let p = root.join(crate::MANIFEST_NAME);
    if !p.exists() {
        return Err(PkgError::NoManifest(p));
    }
    Ok(manifest::load(&p)?)
}

fn short_source(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix("registry+") {
        // Strip the URL so listings stay readable.
        let _ = rest;
        "registry"
    } else if s.starts_with("path+") {
        "path"
    } else if s.starts_with("git+") {
        "git"
    } else {
        "?"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &Path, body: &str) {
        std::fs::write(dir.join("star.toml"), body).unwrap();
    }

    #[test]
    fn add_writes_dep_and_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            r#"
[package]
name = "app"
version = "0.1.0"
edition = "2026"
"#,
        );
        let msg = add(dir.path(), "std", Some("0.1")).unwrap();
        assert!(msg.contains("std"));
        let m = manifest::load(&dir.path().join("star.toml")).unwrap();
        assert!(m.deps.contains_key("std"));
        let lock = lockfile::load(&dir.path().join("star.lock")).unwrap();
        assert_eq!(lock.packages.len(), 1);
        assert_eq!(lock.packages[0].name, "std");
    }

    #[test]
    fn remove_drops_dep_and_relocks() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            r#"
[package]
name = "app"
version = "0.1.0"
edition = "2026"

[deps]
std = "0.1"
otel = "0.1"
"#,
        );
        resolve_and_lock(dir.path()).unwrap();
        let msg = remove(dir.path(), "otel").unwrap();
        assert!(msg.contains("otel"));
        let lock = lockfile::load(&dir.path().join("star.lock")).unwrap();
        assert_eq!(lock.packages.len(), 1);
        assert_eq!(lock.packages[0].name, "std");
    }

    #[test]
    fn remove_unknown_errors() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            r#"
[package]
name = "app"
version = "0.1.0"
edition = "2026"
"#,
        );
        let err = remove(dir.path(), "nope").unwrap_err();
        assert!(matches!(err, PkgError::DepNotFound(_)));
    }
}
