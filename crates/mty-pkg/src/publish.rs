//! `sdust pkg publish` — bundle and (optionally) upload a package
//! release.
//!
//! v0.4 produces a real `tar.gz` of the package contents (excluding
//! `.git`, `target`, `.stardust`) plus a sha256 sidecar file, and —
//! when a GitHub token is available for the configured registry —
//! creates the GitHub release that hosts them.
//!
//! Without a token, the bundle is still written to
//! `.stardust/publish/` and the function returns a clear "auth
//! required for upload" message that includes the file paths so the
//! user can drop them onto the release page manually.
//!
//! ### Determinism
//!
//! The tar archive is built from a sorted list of files, with mtimes
//! pinned to the Unix epoch and ownership stripped, so two runs over
//! the same tree produce byte-identical tarballs (and identical
//! sha256s). Gzip is invoked with the default compression level —
//! `flate2` produces deterministic output for that path.

use crate::hash;
use crate::registry::{self, RegistryError};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("missing manifest at {0}")]
    NoManifest(PathBuf),
    #[error("manifest error: {0}")]
    Manifest(#[from] mty_driver::manifest::ManifestError),
    #[error("registry config: {0}")]
    Registry(#[from] RegistryError),
    #[error("upload error: {0}")]
    Upload(String),
}

/// Result of [`bundle`] — the local artefacts.
#[derive(Debug, Clone)]
pub struct PublishOutcome {
    pub bundle_path: PathBuf,
    pub sha256_path: PathBuf,
    pub hash: String,
    pub manifest_snapshot: String,
    pub tag: String,
    pub package_name: String,
    pub package_version: String,
}

/// Bundle the package into `.stardust/publish/<name>-<version>.tar.gz`
/// + `.tar.gz.sha256`. Does **not** upload.
pub fn bundle(repo_root: &Path) -> Result<PublishOutcome, PublishError> {
    let manifest_path = repo_root.join(crate::MANIFEST_NAME);
    if !manifest_path.exists() {
        return Err(PublishError::NoManifest(manifest_path));
    }
    let manifest = mty_driver::manifest::load(&manifest_path)?;
    let manifest_snapshot = std::fs::read_to_string(&manifest_path)?;

    let name = manifest.package.name.clone();
    let version = manifest.package.version.clone();
    let tag = registry::make_tag(&name, &version);

    let out_dir = repo_root.join(".stardust").join("publish");
    std::fs::create_dir_all(&out_dir)?;
    let bundle_path = out_dir.join(format!("{tag}.tar.gz"));
    let sha256_path = out_dir.join(format!("{tag}.tar.gz.sha256"));

    let mut entries = Vec::new();
    collect_publishable(repo_root, repo_root, &mut entries)?;
    entries.sort();

    let tar_bytes = build_tar(repo_root, &entries, &name, &version)?;
    let gz_bytes = gzip(&tar_bytes)?;

    std::fs::write(&bundle_path, &gz_bytes)?;
    let h = hash::hash_bytes(&gz_bytes);
    let hex = h.trim_start_matches("sha256:");
    let sidecar = format!("{hex}  {tag}.tar.gz\n");
    std::fs::write(&sha256_path, sidecar)?;

    Ok(PublishOutcome {
        bundle_path,
        sha256_path,
        hash: h,
        manifest_snapshot,
        tag,
        package_name: name,
        package_version: version,
    })
}

/// Legacy entrypoint preserved for downstream callers — alias for
/// [`bundle`].
pub fn publish(repo_root: &Path) -> Result<PublishOutcome, PublishError> {
    bundle(repo_root)
}

// ============================================================
// Build a deterministic tar archive
// ============================================================

fn build_tar(
    root: &Path,
    rels: &[String],
    pkg_name: &str,
    pkg_version: &str,
) -> Result<Vec<u8>, PublishError> {
    let mut builder = tar::Builder::new(Vec::new());
    builder.mode(tar::HeaderMode::Deterministic);
    let prefix = format!("{pkg_name}-{pkg_version}");
    for rel in rels {
        let abs = root.join(rel);
        let body = std::fs::read(&abs)?;
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_entry_type(tar::EntryType::Regular);
        // Place every entry under `<name>-<version>/` so extraction
        // produces a tidy single top-level dir.
        let rel_norm = rel.replace('\\', "/");
        let path_in_tar = format!("{prefix}/{rel_norm}");
        header
            .set_path(&path_in_tar)
            .map_err(|e| PublishError::Upload(format!("tar path `{path_in_tar}`: {e}")))?;
        header.set_cksum();
        builder
            .append(&header, &body[..])
            .map_err(PublishError::Io)?;
    }
    builder.finish().map_err(PublishError::Io)?;
    builder.into_inner().map_err(PublishError::Io)
}

fn gzip(input: &[u8]) -> std::io::Result<Vec<u8>> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    // Use a fixed compression level and clear the filename/mtime so
    // the gzip header is identical across runs (flate2 elides those
    // by default for `GzEncoder::new`).
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(input)?;
    enc.finish()
}

fn collect_publishable(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let path = entry.path();
        let name = entry.file_name();
        let name_s = name.to_string_lossy();
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

// ============================================================
// Optional upload to GitHub Releases
// ============================================================

/// Upload a previously-bundled outcome to the named registry. Returns
/// the URL of the newly-created release. Requires a GitHub token.
#[cfg(feature = "registry-fetch")]
pub fn upload(slug: &str, outcome: &PublishOutcome) -> Result<String, PublishError> {
    let token = resolve_token(slug).ok_or_else(|| {
        PublishError::Upload(format!(
            "no auth token for `{slug}` — set GITHUB_TOKEN or run `sdust pkg login {slug}`"
        ))
    })?;
    let (owner, repo) = registry::parse_slug(slug)?;

    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("mty-pkg/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| PublishError::Upload(e.to_string()))?;

    // Create the release.
    let create_url = format!("https://api.github.com/repos/{owner}/{repo}/releases");
    let body_json = serde_json::json!({
        "tag_name": outcome.tag,
        "name": outcome.tag,
        "body": outcome.manifest_snapshot,
        "draft": false,
        "prerelease": false,
    });
    let resp = client
        .post(&create_url)
        .bearer_auth(&token)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .body(serde_json::to_string(&body_json).map_err(|e| PublishError::Upload(e.to_string()))?)
        .send()
        .map_err(|e| PublishError::Upload(format!("create release: {e}")))?;
    if !resp.status().is_success() {
        return Err(PublishError::Upload(format!(
            "create release: HTTP {} — {}",
            resp.status(),
            resp.text().unwrap_or_default()
        )));
    }
    let release_body = resp
        .text()
        .map_err(|e| PublishError::Upload(format!("read release body: {e}")))?;
    let release_json: serde_json::Value = serde_json::from_str(&release_body)
        .map_err(|e| PublishError::Upload(format!("parse release: {e}")))?;
    let upload_url_template = release_json
        .get("upload_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PublishError::Upload("release has no upload_url".into()))?;
    let html_url = release_json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // GitHub's `upload_url` is a URI template ending in `{?name,label}`.
    let upload_base = upload_url_template
        .split('{')
        .next()
        .unwrap_or(upload_url_template);

    // Upload tarball.
    let tar_bytes = std::fs::read(&outcome.bundle_path)?;
    upload_asset(
        &client,
        upload_base,
        &token,
        &format!("{}.tar.gz", outcome.tag),
        "application/gzip",
        tar_bytes,
    )?;
    // Upload sha256 sidecar.
    let sha_bytes = std::fs::read(&outcome.sha256_path)?;
    upload_asset(
        &client,
        upload_base,
        &token,
        &format!("{}.tar.gz.sha256", outcome.tag),
        "text/plain",
        sha_bytes,
    )?;
    Ok(html_url)
}

#[cfg(not(feature = "registry-fetch"))]
pub fn upload(_slug: &str, _outcome: &PublishOutcome) -> Result<String, PublishError> {
    Err(PublishError::Upload(
        "registry feature disabled at build time".into(),
    ))
}

#[cfg(feature = "registry-fetch")]
fn upload_asset(
    client: &reqwest::blocking::Client,
    upload_base: &str,
    token: &str,
    name: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<(), PublishError> {
    let url = format!("{upload_base}?name={name}");
    let resp = client
        .post(&url)
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE, content_type)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .body(body)
        .send()
        .map_err(|e| PublishError::Upload(format!("upload {name}: {e}")))?;
    if !resp.status().is_success() {
        return Err(PublishError::Upload(format!(
            "upload {name}: HTTP {} — {}",
            resp.status(),
            resp.text().unwrap_or_default()
        )));
    }
    Ok(())
}

#[cfg(feature = "registry-fetch")]
fn resolve_token(slug: &str) -> Option<String> {
    if let Some(p) = crate::registry::AuthStore::default_path() {
        if let Ok(store) = crate::registry::AuthStore::load(&p) {
            if let Some(t) = store.tokens.get(slug) {
                return Some(t.clone());
            }
        }
    }
    std::env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty())
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;

    fn write_pkg(dir: &Path, name: &str, version: &str) {
        std::fs::write(
            dir.join("mighty.toml"),
            format!(
                r#"
[package]
name = "{name}"
version = "{version}"
edition = "2026"
"#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("main.sd"), b"fn main() {}").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.sd"), b"// lib").unwrap();
    }

    #[test]
    fn bundle_produces_tarball_and_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        write_pkg(dir.path(), "demo", "0.1.0");

        let out = bundle(dir.path()).unwrap();
        assert!(out.bundle_path.exists());
        assert!(out.sha256_path.exists());
        assert!(out.hash.starts_with("sha256:"));
        assert_eq!(out.tag, "demo-0.1.0");

        // Sidecar matches bundle hash.
        let side = std::fs::read_to_string(&out.sha256_path).unwrap();
        let hex = out.hash.trim_start_matches("sha256:");
        assert!(side.starts_with(hex));
        assert!(side.contains("demo-0.1.0.tar.gz"));
    }

    #[test]
    fn bundle_is_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        write_pkg(dir.path(), "demo", "0.1.0");
        let out1 = bundle(dir.path()).unwrap();
        let h1 = out1.hash.clone();
        // Re-bundle.
        let out2 = bundle(dir.path()).unwrap();
        assert_eq!(h1, out2.hash);
    }

    #[test]
    fn bundle_contents_round_trip_through_tar() {
        let dir = tempfile::tempdir().unwrap();
        write_pkg(dir.path(), "demo", "0.2.3");
        let out = bundle(dir.path()).unwrap();

        let gz = std::fs::read(&out.bundle_path).unwrap();
        let dec = GzDecoder::new(&gz[..]);
        let mut archive = tar::Archive::new(dec);
        let mut names: Vec<String> = archive
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        // All entries should be under `<name>-<version>/`.
        assert!(names.iter().all(|n| n.starts_with("demo-0.2.3/")));
        // The manifest must be in there.
        assert!(names.iter().any(|n| n.ends_with("mighty.toml")));
        assert!(names.iter().any(|n| n.ends_with("main.sd")));
        assert!(names.iter().any(|n| n.ends_with("src/lib.sd")));
    }

    #[test]
    fn bundle_excludes_stardust_and_target() {
        let dir = tempfile::tempdir().unwrap();
        write_pkg(dir.path(), "demo", "0.1.0");
        std::fs::create_dir_all(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/junk"), b"x").unwrap();
        std::fs::create_dir_all(dir.path().join(".stardust/whatever")).unwrap();
        std::fs::write(dir.path().join(".stardust/whatever/x"), b"y").unwrap();

        let out = bundle(dir.path()).unwrap();
        let gz = std::fs::read(&out.bundle_path).unwrap();
        let dec = GzDecoder::new(&gz[..]);
        let mut archive = tar::Archive::new(dec);
        for e in archive.entries().unwrap() {
            let e = e.unwrap();
            let p = e.path().unwrap().to_string_lossy().into_owned();
            assert!(!p.contains("/target/"), "target leaked: {p}");
            assert!(!p.contains("/.stardust/"), ".stardust leaked: {p}");
        }
    }
}
