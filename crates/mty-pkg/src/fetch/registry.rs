//! Registry fetcher — GitHub Releases backend (v0.4).
//!
//! A registry is a GitHub repository whose Releases host published
//! packages. See [`crate::registry`] for the storage convention. This
//! module implements the *fetch* side: resolving an index, locating a
//! release, downloading its `.tar.gz` asset, verifying the sha256
//! sidecar, and extracting into `.stardust/pkgs/<name>-<version>/`.
//!
//! Source URLs in `star.lock` use the form `registry+gh://<owner>/<repo>`.
//! The legacy `registry+https://<host>` shape from v0.2 is still
//! parsed and triggers a clear error directing the user to switch
//! their lockfile over.

use super::{FetchError, Fetched};
use crate::lockfile::LockedPackage;
use crate::registry::{self, RegistryIndex, RegistryRelease};
use std::path::{Path, PathBuf};

#[cfg(feature = "registry-fetch")]
pub fn fetch(locked: &LockedPackage, slot: &Path) -> Result<Fetched, FetchError> {
    let slug = match registry::slug_from_source(&locked.source) {
        Some(s) => s.to_string(),
        None => {
            return Err(FetchError::Registry(format!(
                "lockfile source `{}` uses the legacy `registry+https://` scheme; \
                 re-run `sdust pkg update` to migrate to the GitHub-Releases registry",
                locked.source
            )));
        }
    };
    // Cache base = the repo root containing the package's
    // `.stardust/pkgs/<slot>` dir. We walk two levels up from the slot
    // to recover it.
    let repo_root = slot
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .ok_or_else(|| {
            FetchError::Registry("could not determine repo root from package slot".into())
        })?
        .to_path_buf();

    let client = http_client()?;
    let index = ensure_index(&client, &repo_root, &slug, false)?;
    let release = index.find(&locked.name, &locked.version).ok_or_else(|| {
        FetchError::Registry(format!(
            "release `{}-{}` not found in registry `{}`",
            locked.name, locked.version, slug
        ))
    })?;
    let tarball_url = release.tarball_url.as_deref().ok_or_else(|| {
        FetchError::Registry(format!(
            "release `{}` in `{}` is missing its `.tar.gz` asset",
            release.tag, slug
        ))
    })?;
    let expected_sha = release
        .sha256_url
        .as_deref()
        .map(|u| download_text(&client, u, Some(&slug)))
        .transpose()?
        .map(|s| normalise_sha256_line(&s));

    let bytes = download_bytes(&client, tarball_url, Some(&slug))?;
    let actual_sha = crate::hash::hash_bytes(&bytes);
    if let Some(expected) = expected_sha.as_deref() {
        // The sidecar file stores a bare hex digest; our `hash_bytes`
        // produces `sha256:<hex>`. Compare against the suffix.
        let actual_hex = actual_sha.trim_start_matches("sha256:");
        if expected != actual_hex {
            return Err(FetchError::HashMismatch {
                name: locked.name.clone(),
                expected: format!("sha256:{expected}"),
                actual: actual_sha,
            });
        }
    }
    if let Some(expected) = &locked.hash {
        if expected != &actual_sha {
            return Err(FetchError::HashMismatch {
                name: locked.name.clone(),
                expected: expected.clone(),
                actual: actual_sha,
            });
        }
    }

    // Extract tar.gz into `slot`.
    if slot.exists() {
        std::fs::remove_dir_all(slot)?;
    }
    std::fs::create_dir_all(slot)?;
    extract_targz(&bytes, slot)?;

    Ok(Fetched {
        root: slot.to_path_buf(),
        hash: actual_sha,
    })
}

#[cfg(not(feature = "registry-fetch"))]
pub fn fetch(_locked: &LockedPackage, _slot: &Path) -> Result<Fetched, FetchError> {
    Err(FetchError::Registry(
        "registry fetcher disabled at build time".into(),
    ))
}

// ============================================================
// Index management
// ============================================================

/// Ensure a cached index exists for `slug`; refetch from GitHub when
/// the cache is missing, stale, or `force` is set.
///
/// `repo_root` is the package directory that owns the `.stardust`
/// cache.
#[cfg(feature = "registry-fetch")]
pub fn ensure_index(
    client: &reqwest::blocking::Client,
    repo_root: &Path,
    slug: &str,
    force: bool,
) -> Result<RegistryIndex, FetchError> {
    let cached = registry::load_cached_index(repo_root, slug)
        .map_err(|e| FetchError::Registry(format!("cache read: {e}")))?;
    let now = now_secs();
    if !force {
        if let Some(idx) = &cached {
            if !idx.is_stale(now) {
                return Ok(idx.clone());
            }
        }
    }
    // Refetch.
    let if_modified_since = cached.as_ref().and_then(|i| i.last_modified.clone());
    match refresh_index(client, slug, if_modified_since.as_deref())? {
        IndexRefresh::Updated(mut idx) => {
            idx.fetched_at = now;
            registry::save_cached_index(repo_root, &idx)
                .map_err(|e| FetchError::Registry(format!("cache write: {e}")))?;
            Ok(idx)
        }
        IndexRefresh::NotModified => {
            // Update the freshness timestamp on the cached copy so we
            // don't keep hammering the API every call.
            let mut idx = cached.unwrap_or_else(|| RegistryIndex::new(slug));
            idx.fetched_at = now;
            registry::save_cached_index(repo_root, &idx)
                .map_err(|e| FetchError::Registry(format!("cache write: {e}")))?;
            Ok(idx)
        }
    }
}

#[cfg(feature = "registry-fetch")]
enum IndexRefresh {
    Updated(RegistryIndex),
    NotModified,
}

#[cfg(feature = "registry-fetch")]
fn refresh_index(
    client: &reqwest::blocking::Client,
    slug: &str,
    if_modified_since: Option<&str>,
) -> Result<IndexRefresh, FetchError> {
    let (owner, repo) = registry::parse_slug(slug)
        .map_err(|e| FetchError::Registry(format!("bad slug `{slug}`: {e}")))?;
    let mut all_releases = Vec::new();
    let mut page = 1u32;
    let mut last_modified: Option<String> = None;
    loop {
        let url = format!(
            "https://api.github.com/repos/{owner}/{repo}/releases?per_page=100&page={page}"
        );
        let mut req = client.get(&url);
        if let Some(ims) = if_modified_since {
            if page == 1 {
                req = req.header(reqwest::header::IF_MODIFIED_SINCE, ims);
            }
        }
        let resp = req
            .send()
            .map_err(|e| FetchError::Registry(format!("GET {url} ({}): {e}", short_slug(slug))))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_MODIFIED && page == 1 {
            return Ok(IndexRefresh::NotModified);
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(FetchError::Registry(format!(
                "registry `{slug}` not found on GitHub (HTTP 404 from {url})"
            )));
        }
        if !status.is_success() {
            return Err(FetchError::Registry(format!(
                "registry `{slug}` returned HTTP {status} for {url}"
            )));
        }
        if page == 1 {
            last_modified = resp
                .headers()
                .get(reqwest::header::LAST_MODIFIED)
                .and_then(|h| h.to_str().ok())
                .map(str::to_string);
        }
        let body = resp
            .text()
            .map_err(|e| FetchError::Registry(format!("read body: {e}")))?;
        let releases = registry::parse_releases_page(&body)
            .map_err(|e| FetchError::Registry(format!("parse index page {page}: {e}")))?;
        let returned = releases.len();
        all_releases.extend(releases);
        // GitHub returns up to per_page items; a short page or empty
        // means we're done. We also cap pagination at 50 pages
        // (5000 releases) to avoid runaway loops on misbehaving APIs.
        if returned < 100 || page >= 50 {
            break;
        }
        page += 1;
    }
    let idx = RegistryIndex {
        slug: slug.into(),
        fetched_at: now_secs(),
        last_modified,
        releases: all_releases,
    };
    Ok(IndexRefresh::Updated(idx))
}

// ============================================================
// HTTP helpers
// ============================================================

#[cfg(feature = "registry-fetch")]
fn http_client() -> Result<reqwest::blocking::Client, FetchError> {
    let mut builder = reqwest::blocking::Client::builder()
        .user_agent(concat!("mty-pkg/", env!("CARGO_PKG_VERSION")));
    // Pick up GITHUB_TOKEN unconditionally — increases the API quota
    // from 60/hr unauth to 5000/hr authed. Per-registry tokens are
    // applied per-request inside `download_*`.
    if let Ok(t) = std::env::var("GITHUB_TOKEN") {
        if !t.is_empty() {
            let mut headers = reqwest::header::HeaderMap::new();
            let val = reqwest::header::HeaderValue::from_str(&format!("Bearer {t}"))
                .map_err(|e| FetchError::Registry(format!("bad token: {e}")))?;
            headers.insert(reqwest::header::AUTHORIZATION, val);
            headers.insert(
                reqwest::header::ACCEPT,
                reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
            );
            builder = builder.default_headers(headers);
        }
    }
    builder
        .build()
        .map_err(|e| FetchError::Registry(format!("http client: {e}")))
}

#[cfg(feature = "registry-fetch")]
fn download_bytes(
    client: &reqwest::blocking::Client,
    url: &str,
    slug: Option<&str>,
) -> Result<Vec<u8>, FetchError> {
    let mut req = client.get(url);
    if let Some(s) = slug {
        if let Some(t) = per_registry_token(s) {
            req = req.bearer_auth(t);
        }
    }
    let resp = req
        .send()
        .map_err(|e| FetchError::Registry(format!("GET {url}: {e}")))?;
    if !resp.status().is_success() {
        return Err(FetchError::Registry(format!(
            "HTTP {} for {url}",
            resp.status()
        )));
    }
    let bytes = resp
        .bytes()
        .map_err(|e| FetchError::Registry(format!("read body of {url}: {e}")))?;
    Ok(bytes.to_vec())
}

#[cfg(feature = "registry-fetch")]
fn download_text(
    client: &reqwest::blocking::Client,
    url: &str,
    slug: Option<&str>,
) -> Result<String, FetchError> {
    let bytes = download_bytes(client, url, slug)?;
    String::from_utf8(bytes).map_err(|e| FetchError::Registry(format!("utf8: {e}")))
}

#[cfg(feature = "registry-fetch")]
fn per_registry_token(slug: &str) -> Option<String> {
    let path = crate::registry::AuthStore::default_path()?;
    let store = crate::registry::AuthStore::load(&path).ok()?;
    store.token_for(slug)
}

fn short_slug(slug: &str) -> String {
    // Used purely for error context — keeps log lines compact when the
    // slug is long.
    if slug.len() <= 48 {
        slug.into()
    } else {
        format!("{}…", &slug[..47])
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn normalise_sha256_line(line: &str) -> String {
    // Accept either a bare hex digest, `sha256:<hex>`, or a coreutils
    // sha256sum line `<hex>  <filename>`.
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("sha256:") {
        return rest.trim().to_lowercase();
    }
    let first = trimmed.split_whitespace().next().unwrap_or(trimmed);
    first.to_lowercase()
}

// ============================================================
// tar.gz extraction
// ============================================================

/// Extract a gzip-compressed tar archive into `dst`. Skips entries
/// whose paths try to escape `dst` (path traversal defence).
pub fn extract_targz(bytes: &[u8], dst: &Path) -> Result<(), FetchError> {
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);
    archive.set_preserve_permissions(false);
    archive.set_overwrite(true);
    for entry in archive
        .entries()
        .map_err(|e| FetchError::Registry(format!("read tar entries: {e}")))?
    {
        let mut entry = entry.map_err(|e| FetchError::Registry(format!("tar entry: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| FetchError::Registry(format!("tar entry path: {e}")))?
            .into_owned();
        if is_traversal(&path) {
            return Err(FetchError::Registry(format!(
                "refusing to extract entry with traversal path: {}",
                path.display()
            )));
        }
        let target = dst.join(&path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry
            .unpack(&target)
            .map_err(|e| FetchError::Registry(format!("unpack {}: {e}", target.display())))?;
    }
    Ok(())
}

fn is_traversal(p: &Path) -> bool {
    // Absolute paths (any platform shape) or any `..` component count.
    // On Windows a Unix-style `/etc/passwd` parses as a relative path
    // whose first component is `RootDir`, so handle that explicitly
    // too — we never want a tarball to escape the slot.
    if p.is_absolute() {
        return true;
    }
    p.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    })
}

// ============================================================
// Helpers exposed to other modules
// ============================================================

/// Public entrypoint used by `commands::search` etc. — ensure an index
/// for `slug` is loaded (refreshing if stale) and return it.
#[cfg(feature = "registry-fetch")]
pub fn load_index_for(
    repo_root: &Path,
    slug: &str,
    force: bool,
) -> Result<RegistryIndex, FetchError> {
    let client = http_client()?;
    ensure_index(&client, repo_root, slug, force)
}

#[cfg(not(feature = "registry-fetch"))]
pub fn load_index_for(
    _repo_root: &Path,
    _slug: &str,
    _force: bool,
) -> Result<RegistryIndex, FetchError> {
    Err(FetchError::Registry(
        "registry fetcher disabled at build time".into(),
    ))
}

/// Return a freshly-fetched copy of `release` with the full body text
/// (for `pkg info`). Re-uses the index for the asset URLs but pulls
/// the release JSON directly because the cached index only keeps a
/// preview.
#[cfg(feature = "registry-fetch")]
pub fn fetch_release_body(slug: &str, release: &RegistryRelease) -> Result<String, FetchError> {
    let (owner, repo) = registry::parse_slug(slug)
        .map_err(|e| FetchError::Registry(format!("bad slug `{slug}`: {e}")))?;
    let url = format!(
        "https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}",
        tag = release.tag
    );
    let client = http_client()?;
    let body = download_text(&client, &url, Some(slug))?;
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| FetchError::Registry(format!("json: {e}")))?;
    Ok(v.get("body")
        .and_then(|b| b.as_str())
        .unwrap_or("")
        .to_string())
}

#[cfg(not(feature = "registry-fetch"))]
pub fn fetch_release_body(_slug: &str, _release: &RegistryRelease) -> Result<String, FetchError> {
    Err(FetchError::Registry(
        "registry fetcher disabled at build time".into(),
    ))
}

// Re-export of a Path-buf helper unused outside this module — keeps
// PathBuf import live without an unused-import warning.
#[allow(dead_code)]
fn _path_buf_keepalive() -> PathBuf {
    PathBuf::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_handles_three_shapes() {
        assert_eq!(normalise_sha256_line("ABCDEF"), "abcdef");
        assert_eq!(normalise_sha256_line("sha256:abc"), "abc");
        assert_eq!(normalise_sha256_line("abcdef  bundle.tar.gz\n"), "abcdef");
    }

    #[test]
    fn traversal_paths_rejected() {
        assert!(is_traversal(Path::new("../etc/passwd")));
        assert!(is_traversal(Path::new("/etc/passwd")));
        assert!(!is_traversal(Path::new("foo/bar.sd")));
    }
}
