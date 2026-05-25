//! High-level operations behind `mty pkg <subcmd>`.
//!
//! Each function takes the package root (the directory holding
//! `mighty.toml`) and returns either a printable summary string or an
//! error. The CLI wrapper in `mty-cli` is a thin pass-through.

use crate::fetch::{self, Fetched};
use crate::lockfile::{self, Lockfile};
use crate::publish;
use crate::registry::{self, AuthStore, RegistryError};
use crate::resolver::Resolver;
use crate::signing::{self, SigningError};
use mty_driver::manifest::{self, Dep, DetailedDep, Manifest};
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
    #[error("signing error: {0}")]
    Signing(#[from] SigningError),
    #[error("registry error: {0}")]
    Registry(#[from] RegistryError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("dep `{0}` not found in manifest")]
    DepNotFound(String),
    #[error("no mighty.toml at {0}")]
    NoManifest(PathBuf),
    #[error("auth required: {0}")]
    AuthRequired(String),
}

/// Resolve and write the lockfile to disk. Returns the lockfile.
pub fn resolve_and_lock(root: &Path) -> Result<Lockfile, PkgError> {
    let manifest = load_manifest(root)?;
    let resolver = Resolver::new(root);
    let lock = resolver.resolve(&manifest)?;
    lockfile::save(&lock, &root.join(crate::LOCKFILE_NAME))?;
    Ok(lock)
}

/// `mty pkg add <name>[@version]`.
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

pub fn add_detailed(root: &Path, name: &str, detailed: DetailedDep) -> Result<String, PkgError> {
    let mut manifest = load_manifest(root)?;
    manifest
        .deps
        .insert(name.to_string(), Dep::Detailed(detailed));
    manifest::save(&manifest, &root.join(crate::MANIFEST_NAME))?;
    let _ = resolve_and_lock(root)?;
    Ok(format!("added `{name}` (detailed source)"))
}

pub fn remove(root: &Path, name: &str) -> Result<String, PkgError> {
    let mut manifest = load_manifest(root)?;
    if manifest.deps.remove(name).is_none() {
        return Err(PkgError::DepNotFound(name.into()));
    }
    manifest::save(&manifest, &root.join(crate::MANIFEST_NAME))?;
    let _ = resolve_and_lock(root)?;
    Ok(format!("removed `{name}`"))
}

/// `mty pkg update [name] [--refresh]`. With `refresh=true` the
/// cached registry indexes are revalidated against GitHub before
/// re-resolution.
pub fn update(root: &Path, name: Option<&str>, refresh: bool) -> Result<String, PkgError> {
    if refresh {
        refresh_indexes(root)?;
    }
    let _ = resolve_and_lock(root)?;
    match name {
        Some(n) => Ok(format!("updated `{n}` (and re-resolved transitive deps)")),
        None => Ok("re-resolved all dependencies".into()),
    }
}

/// Refresh every configured registry's index. Errors per-registry are
/// reported but do not abort the others.
pub fn refresh_indexes(root: &Path) -> Result<String, PkgError> {
    let manifest_path = root.join(crate::MANIFEST_NAME);
    let cfg = registry::load_registry_config(&manifest_path)?;
    let slugs = cfg.slugs();
    let mut out = String::new();
    for slug in &slugs {
        match fetch::registry::load_index_for(root, slug, true) {
            Ok(idx) => {
                out.push_str(&format!(
                    "refreshed `{}` — {} releases\n",
                    slug,
                    idx.releases.len()
                ));
            }
            Err(e) => {
                out.push_str(&format!("warn: refresh `{slug}` failed: {e}\n"));
            }
        }
    }
    if out.is_empty() {
        out.push_str("no registries configured\n");
    }
    Ok(out)
}

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

/// `mty pkg search <query>`. Substring-matches both name and
/// version across the cached indexes of every configured registry.
/// Refreshes indexes only when none are cached for a given slug.
pub fn search(root: &Path, query: &str) -> Result<String, PkgError> {
    let cfg = registry::load_registry_config(&root.join(crate::MANIFEST_NAME))?;
    let slugs = cfg.slugs();
    let mut hits: Vec<(String, String, String)> = Vec::new(); // (slug, name, version)
    let mut total_releases = 0usize;
    for slug in &slugs {
        let cached = registry::load_cached_index(root, slug)?;
        let idx = match cached {
            Some(i) => i,
            None => {
                // Best-effort prefetch — never fatal.
                match fetch::registry::load_index_for(root, slug, false) {
                    Ok(i) => i,
                    Err(_) => continue,
                }
            }
        };
        total_releases += idx.releases.len();
        for r in &idx.releases {
            if r.name.contains(query) || r.version.contains(query) {
                hits.push((slug.clone(), r.name.clone(), r.version.clone()));
            }
        }
    }
    hits.sort();
    hits.dedup();
    if hits.is_empty() {
        if total_releases == 0 {
            return Ok(format!(
                "no results for `{query}` (no cached registry indexes; run `mty pkg update --refresh` to populate one)\n"
            ));
        }
        return Ok(format!("no results for `{query}`\n"));
    }
    let mut out = String::new();
    for (slug, name, version) in hits {
        out.push_str(&format!("{name}@{version}    [{slug}]\n"));
    }
    Ok(out)
}

/// `mty pkg info <name>[@version]`. Resolves to a concrete release
/// across configured registries and prints its metadata + body
/// preview. With no version specifier the latest known version is
/// shown.
pub fn info(root: &Path, query: &str) -> Result<String, PkgError> {
    let (name, version) = match query.split_once('@') {
        Some((n, v)) => (n.to_string(), Some(v.to_string())),
        None => (query.to_string(), None),
    };
    let cfg = registry::load_registry_config(&root.join(crate::MANIFEST_NAME))?;
    let slugs = cfg.slugs();
    for slug in &slugs {
        let cached = registry::load_cached_index(root, slug)?;
        let idx = match cached {
            Some(i) => i,
            None => match fetch::registry::load_index_for(root, slug, false) {
                Ok(i) => i,
                Err(_) => continue,
            },
        };
        // Pick the requested version or the latest known.
        let release = match &version {
            Some(v) => idx.find(&name, v).cloned(),
            None => idx
                .releases
                .iter()
                .filter(|r| r.name == name)
                .max_by(|a, b| {
                    let va = crate::semver::Version::parse(&a.version).ok();
                    let vb = crate::semver::Version::parse(&b.version).ok();
                    va.cmp(&vb)
                })
                .cloned(),
        };
        let Some(release) = release else {
            continue;
        };
        let mut out = String::new();
        out.push_str(&format!(
            "{}@{}    [{}]\n",
            release.name, release.version, slug
        ));
        if let Some(url) = &release.html_url {
            out.push_str(&format!("  release : {url}\n"));
        }
        if let Some(tar) = &release.tarball_url {
            out.push_str(&format!("  tarball : {tar}\n"));
        }
        // Try to fetch the full body for a manifest snapshot.
        match fetch::registry::fetch_release_body(slug, &release) {
            Ok(body) if !body.is_empty() => {
                out.push_str("  manifest:\n");
                for line in body.lines().take(40) {
                    out.push_str(&format!("    {line}\n"));
                }
            }
            _ => {
                if let Some(prev) = &release.body_preview {
                    if !prev.is_empty() {
                        out.push_str("  preview :\n");
                        for line in prev.lines().take(8) {
                            out.push_str(&format!("    {line}\n"));
                        }
                    }
                }
            }
        }
        return Ok(out);
    }
    let q = match version {
        Some(v) => format!("{name}@{v}"),
        None => name,
    };
    Err(PkgError::DepNotFound(q))
}

/// `mty pkg login [registry]` — guided token setup. When
/// interactive input isn't available the function returns an error
/// describing how to drop the token via env-var instead. In this
/// non-interactive build we accept the token via env-var
/// `SDUST_PKG_LOGIN_TOKEN` (used by tests + scripted setups).
pub fn login(slug: Option<&str>, root: &Path) -> Result<String, PkgError> {
    let slug = match slug {
        Some(s) => s.to_string(),
        None => {
            let cfg = registry::load_registry_config(&root.join(crate::MANIFEST_NAME))?;
            cfg.default
                .unwrap_or_else(|| registry::DEFAULT_REGISTRY_SLUG.into())
        }
    };
    // Validate.
    let _ = registry::parse_slug(&slug)?;
    let token = std::env::var("SDUST_PKG_LOGIN_TOKEN").ok();
    let token = token.ok_or_else(|| {
        PkgError::AuthRequired(format!(
            "pass the token via SDUST_PKG_LOGIN_TOKEN=<ghp_…> mty pkg login {slug} \
             (interactive prompts are disabled in v0.4)"
        ))
    })?;
    let auth_path = AuthStore::default_path().ok_or_else(|| {
        PkgError::AuthRequired("could not locate ~/.config/mighty/auth.toml".into())
    })?;
    let mut store = AuthStore::load(&auth_path)?;
    store.set_token(&slug, token);
    store.save(&auth_path)?;
    Ok(format!(
        "stored token for `{slug}` at {}\n",
        auth_path.display()
    ))
}

/// `mty pkg publish`. Produces the bundle, signs it (mode selected
/// by `[registry.signing] mode` — see
/// `crates/mty-pkg/src/signing.rs`), then either uploads it (when a
/// token is available for the configured default registry) or
/// reports the local artefacts + a clear "set GITHUB_TOKEN" message.
///
/// v0.10 cleanup: signing is mode-aware. The default mode is
/// `"stub"` (deterministic SHA-256 envelope, v0.9 shape). Setting
/// `[registry.signing] mode = "keyless"` opts into real sigstore
/// signing when the `sigstore-real` cargo feature is compiled in;
/// without the feature, the keyless path quietly degrades to stub
/// (so `mty pkg publish` never aborts because the binary was built
/// without the optional dep).
pub fn publish(root: &Path) -> Result<String, PkgError> {
    let outcome = publish::bundle(root)?;
    let cfg = registry::load_registry_config(&root.join(crate::MANIFEST_NAME))?;
    let mode = signing::SigningMode::parse(cfg.signing.mode.as_deref());
    let signed = signing::sign_bundle_with_mode(&outcome, mode)?;
    // If the user asked for keyless but we degraded to stub, surface
    // a one-line note — useful in CI to spot a misconfigured runner.
    let degraded_note =
        if mode == signing::SigningMode::Keyless && signed.mode == signing::SigningMode::Stub {
            "note: keyless signing requested but binary built without `sigstore-real` feature \
         (or no ambient OIDC identity available); falling back to stub envelope.\n"
        } else {
            ""
        };
    let slug = cfg
        .default
        .clone()
        .unwrap_or_else(|| registry::DEFAULT_REGISTRY_SLUG.into());
    let has_token = std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
        .is_some()
        || AuthStore::default_path()
            .and_then(|p| AuthStore::load(&p).ok())
            .map(|s| s.tokens.contains_key(&slug))
            .unwrap_or(false);
    if !has_token {
        return Ok(format!(
            "{degraded}bundle ready at `{bundle}` ({hash})\n\
             sidecar     `{side}`\n\
             signature   `{sig}` (mode: {mode:?})\n\
             envelope    `{env}`\n\
             upload skipped: no auth token for `{slug}`.\n\
             Set GITHUB_TOKEN or run `mty pkg login {slug}` and retry.\n\
             To upload manually, drag the four files onto the release page for tag `{tag}`.\n",
            degraded = degraded_note,
            bundle = outcome.bundle_path.display(),
            hash = outcome.hash,
            side = outcome.sha256_path.display(),
            sig = signed.sig_path.display(),
            env = signed.envelope_path.display(),
            mode = signed.mode,
            slug = slug,
            tag = outcome.tag,
        ));
    }
    let url = publish::upload(&slug, &outcome)?;
    Ok(format!(
        "{degraded}published `{tag}` to `{slug}` — {url}\nbundle: {bundle} ({hash})\nsidecar: {side}\nsignature: {sig} (mode: {mode:?})\nenvelope: {env}\n",
        degraded = degraded_note,
        tag = outcome.tag,
        slug = slug,
        url = url,
        bundle = outcome.bundle_path.display(),
        hash = outcome.hash,
        side = outcome.sha256_path.display(),
        sig = signed.sig_path.display(),
        mode = signed.mode,
        env = signed.envelope_path.display(),
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
    if s.starts_with("registry+") {
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
        std::fs::write(dir.join("mighty.toml"), body).unwrap();
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
        let m = manifest::load(&dir.path().join("mighty.toml")).unwrap();
        assert!(m.deps.contains_key("std"));
        let lock = lockfile::load(&dir.path().join("mighty.lock")).unwrap();
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
        let lock = lockfile::load(&dir.path().join("mighty.lock")).unwrap();
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

    #[test]
    fn search_empty_returns_message() {
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
        // No cached indexes -> "no results" with a hint.
        let out = search(dir.path(), "foo").unwrap();
        assert!(out.contains("no results"));
    }

    #[test]
    fn publish_without_token_reports_bundle_path() {
        // Make sure no env-token bleeds in.
        std::env::remove_var("GITHUB_TOKEN");
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            r#"
[package]
name = "demo"
version = "0.1.0"
edition = "2026"
"#,
        );
        // Point auth.toml at a tempdir so we don't read the real one.
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", home.path());
        let msg = publish(dir.path()).unwrap();
        assert!(msg.contains("bundle ready"));
        assert!(msg.contains("upload skipped"));
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}
