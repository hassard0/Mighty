//! Registry configuration + index types for the v0.4 GitHub-Releases
//! backed registry.
//!
//! A "Mighty registry" is a GitHub repository whose Releases host
//! published packages. Each release:
//!
//! - **Tag**: `<package-name>-<version>` (e.g. `otel-0.1.0`).
//! - **Assets**: `<package-name>-<version>.tar.gz` (gzipped tar of the
//!   package source) plus `<package-name>-<version>.tar.gz.sha256`
//!   (single line, lowercase hex of the tarball's sha256).
//! - **Body** (release description): a copy of the package's
//!   `mighty.toml` manifest.
//!
//! The index is the list of releases from GitHub's REST API; the
//! fetcher caches it locally with a 1-hour TTL.
//!
//! ### `[registry]` section
//!
//! Manifests may add an optional `[registry]` table to opt into
//! additional registries:
//!
//! ```toml
//! [registry]
//! default = "stardust-pkg/registry"        # the official one
//! extras = ["myorg/private-stardust-pkgs"] # additional registries
//! ```
//!
//! Multiple registries are unioned at lookup time; on duplicate
//! `(name, version)`, the **first-listed** registry wins (the default,
//! then each extra in order).
//!
//! Auth tokens are stored in `~/.config/mighty/auth.toml`; see
//! [`AuthStore`].

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The official Mighty registry repository slug. The cloud control
/// plane will own `stardust-pkg/registry` once v0.5 spins it up; until
/// then this default points at a slug that does not yet exist on
/// GitHub and any fetch attempt will surface a clear "release not
/// found" error.
pub const DEFAULT_REGISTRY_SLUG: &str = "stardust-pkg/registry";

/// In-package cache TTL for a fetched registry index (1 hour).
pub const INDEX_TTL_SECS: u64 = 60 * 60;

/// `[registry]` section of `mighty.toml`.
///
/// Parsed independently of the `Manifest` struct (which lives in
/// `mty-driver`) so this slice doesn't have to touch the driver.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RegistryConfig {
    /// Default registry — `<owner>/<repo>`. Falls back to
    /// [`DEFAULT_REGISTRY_SLUG`] when omitted.
    #[serde(default)]
    pub default: Option<String>,
    /// Additional registries layered on top of `default`. Earlier
    /// entries win on `(name, version)` collisions.
    #[serde(default)]
    pub extras: Vec<String>,
}

impl RegistryConfig {
    /// The ordered list of registry slugs to consult, default first.
    pub fn slugs(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(1 + self.extras.len());
        out.push(
            self.default
                .clone()
                .unwrap_or_else(|| DEFAULT_REGISTRY_SLUG.into()),
        );
        out.extend(self.extras.iter().cloned());
        // De-dup, preserving first occurrence.
        let mut seen = std::collections::BTreeSet::new();
        out.retain(|s| seen.insert(s.clone()));
        out
    }
}

/// Read `[registry]` from a `mighty.toml` at `path`. Missing section
/// returns the default config (which still includes the default slug).
pub fn load_registry_config(path: &Path) -> Result<RegistryConfig, RegistryError> {
    if !path.exists() {
        return Ok(RegistryConfig::default());
    }
    let src = std::fs::read_to_string(path)?;
    #[derive(Deserialize)]
    struct Wrapper {
        #[serde(default)]
        registry: Option<RegistryConfig>,
    }
    let w: Wrapper = toml::from_str(&src)?;
    Ok(w.registry.unwrap_or_default())
}

/// Errors raised by registry-config + index code.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("toml serialize: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("json parse: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid registry slug `{0}`; expected `<owner>/<repo>`")]
    BadSlug(String),
}

/// Split `<owner>/<repo>` into `(owner, repo)`. Rejects empty or
/// triple-segment values.
pub fn parse_slug(slug: &str) -> Result<(String, String), RegistryError> {
    let parts: Vec<&str> = slug.split('/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(RegistryError::BadSlug(slug.into()));
    }
    Ok((parts[0].into(), parts[1].into()))
}

/// Filesystem-safe key for a registry: `<owner>__<repo>`.
pub fn slug_to_cache_key(slug: &str) -> String {
    slug.replace('/', "__")
}

/// On-disk registry index — what we cache per registry.
///
/// Captures the GitHub-Releases derived (name, version) → release-tag
/// catalogue. Body excerpts (manifest snapshot) are not cached — they
/// are fetched on demand by `pkg info`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RegistryIndex {
    /// `<owner>/<repo>` slug this index was built from.
    pub slug: String,
    /// Unix epoch seconds when this index was last refreshed.
    #[serde(default)]
    pub fetched_at: u64,
    /// Last `Last-Modified` header observed (for conditional refetch).
    #[serde(default)]
    pub last_modified: Option<String>,
    /// Release tag → parsed release metadata. Tags shaped
    /// `<name>-<version>` are catalogued; anything else is ignored.
    #[serde(default)]
    pub releases: Vec<RegistryRelease>,
}

impl RegistryIndex {
    pub fn new(slug: impl Into<String>) -> Self {
        RegistryIndex {
            slug: slug.into(),
            fetched_at: 0,
            last_modified: None,
            releases: Vec::new(),
        }
    }

    /// Whether this cached index is older than `INDEX_TTL_SECS`.
    pub fn is_stale(&self, now: u64) -> bool {
        now.saturating_sub(self.fetched_at) > INDEX_TTL_SECS
    }

    /// All `(name, version)` pairs known to this index.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.releases
            .iter()
            .map(|r| (r.name.as_str(), r.version.as_str()))
    }

    /// All versions known for `name`.
    pub fn versions_for<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.releases
            .iter()
            .filter(move |r| r.name == name)
            .map(|r| r.version.as_str())
    }

    /// Lookup a specific `(name, version)`.
    pub fn find(&self, name: &str, version: &str) -> Option<&RegistryRelease> {
        self.releases
            .iter()
            .find(|r| r.name == name && r.version == version)
    }
}

/// One catalogued release. Mirrors the bits of the GitHub release API
/// shape that the fetcher actually consumes.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistryRelease {
    pub name: String,
    pub version: String,
    pub tag: String,
    /// `<package-name>-<version>.tar.gz` asset's download URL.
    #[serde(default)]
    pub tarball_url: Option<String>,
    /// `<package-name>-<version>.tar.gz.sha256` asset's download URL.
    #[serde(default)]
    pub sha256_url: Option<String>,
    /// Release page URL (for `pkg info`).
    #[serde(default)]
    pub html_url: Option<String>,
    /// First few hundred chars of the release body, for `pkg info`
    /// previews. Set only when fetched in detail mode.
    #[serde(default)]
    pub body_preview: Option<String>,
}

/// Compute the cache file path for a registry slug under
/// `<repo_root>/.mighty/registry/<owner>__<repo>/index.json`.
pub fn cache_path(repo_root: &Path, slug: &str) -> PathBuf {
    repo_root
        .join(".mighty")
        .join("registry")
        .join(slug_to_cache_key(slug))
        .join("index.json")
}

/// Load a cached index from disk; returns `None` if it doesn't exist.
pub fn load_cached_index(
    repo_root: &Path,
    slug: &str,
) -> Result<Option<RegistryIndex>, RegistryError> {
    let p = cache_path(repo_root, slug);
    if !p.exists() {
        return Ok(None);
    }
    let src = std::fs::read_to_string(&p)?;
    let idx: RegistryIndex = serde_json::from_str(&src)?;
    Ok(Some(idx))
}

/// Persist an index to the on-disk cache (creating parent dirs).
pub fn save_cached_index(repo_root: &Path, idx: &RegistryIndex) -> Result<(), RegistryError> {
    let p = cache_path(repo_root, &idx.slug);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(idx)?;
    std::fs::write(p, text)?;
    Ok(())
}

/// Parse a GitHub-Releases API JSON page into the catalogue subset we
/// keep. Returns the parsed releases plus any tag that didn't conform
/// to `<name>-<version>` (for diagnostics; currently we silently drop).
///
/// The expected JSON shape (simplified) is the standard `/releases`
/// listing — an array of release objects with `tag_name`, `html_url`,
/// `body`, and an `assets` array of `{ name, browser_download_url }`.
pub fn parse_releases_page(body: &str) -> Result<Vec<RegistryRelease>, RegistryError> {
    let arr: serde_json::Value = serde_json::from_str(body)?;
    let mut out = Vec::new();
    let Some(items) = arr.as_array() else {
        return Ok(out);
    };
    for item in items {
        let Some(tag) = item.get("tag_name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some((name, version)) = split_tag(tag) else {
            continue;
        };
        let html_url = item
            .get("html_url")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let body_preview = item
            .get("body")
            .and_then(|v| v.as_str())
            .map(|b| b.chars().take(400).collect::<String>());

        let mut tarball_url = None;
        let mut sha256_url = None;
        if let Some(assets) = item.get("assets").and_then(|v| v.as_array()) {
            let want_tar = format!("{name}-{version}.tar.gz");
            let want_sha = format!("{name}-{version}.tar.gz.sha256");
            for asset in assets {
                let aname = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let aurl = asset
                    .get("browser_download_url")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                if aname == want_tar {
                    tarball_url = aurl.clone();
                } else if aname == want_sha {
                    sha256_url = aurl.clone();
                }
            }
        }
        out.push(RegistryRelease {
            name,
            version,
            tag: tag.into(),
            tarball_url,
            sha256_url,
            html_url,
            body_preview,
        });
    }
    Ok(out)
}

/// Split a release tag `<name>-<version>` into its parts. The split is
/// on the *last* `-` followed by a digit — package names may contain
/// dashes, but versions start with a digit.
pub fn split_tag(tag: &str) -> Option<(String, String)> {
    let bytes = tag.as_bytes();
    // Find the rightmost `-` whose following byte is ASCII digit.
    let mut idx = None;
    for i in (0..bytes.len()).rev() {
        if bytes[i] == b'-' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            idx = Some(i);
            break;
        }
    }
    let i = idx?;
    let name = &tag[..i];
    let version = &tag[i + 1..];
    if name.is_empty() || version.is_empty() {
        return None;
    }
    Some((name.into(), version.into()))
}

/// Build a release tag from `(name, version)`.
pub fn make_tag(name: &str, version: &str) -> String {
    format!("{name}-{version}")
}

/// Source URL helper for the new `registry+gh://<owner>/<repo>` scheme
/// emitted into `mighty.lock`.
pub fn gh_source(slug: &str) -> String {
    format!("registry+gh://{slug}")
}

/// Inverse of [`gh_source`]: extract the slug from a
/// `registry+gh://<owner>/<repo>` source URL. Returns `None` for the
/// legacy `registry+https://...` shape.
pub fn slug_from_source(source: &str) -> Option<&str> {
    source.strip_prefix("registry+gh://")
}

// ============================================================
// Auth store: ~/.config/mighty/auth.toml
// ============================================================

/// Persisted per-registry auth tokens. Stored as plaintext at
/// `~/.config/mighty/auth.toml`; the file is created with `0600`
/// permissions on Unix (a no-op on Windows).
///
/// Security tradeoff: plaintext is the same model `gh` CLI uses for
/// `~/.config/gh/hosts.yml`. Documented in
/// `docs/reference/registry.md`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AuthStore {
    /// Map `<owner>/<repo>` → token. Unknown keys are preserved on
    /// round-trip via BTreeMap.
    #[serde(default)]
    pub tokens: BTreeMap<String, String>,
}

impl AuthStore {
    /// Default on-disk location for the auth file.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("mty").join("auth.toml"))
    }

    /// Load the auth store from `path`. Missing file → empty store.
    pub fn load(path: &Path) -> Result<Self, RegistryError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let src = std::fs::read_to_string(path)?;
        let store: AuthStore = toml::from_str(&src)?;
        Ok(store)
    }

    /// Persist the auth store. Creates parent dirs, restricts perms
    /// to `0600` on Unix.
    pub fn save(&self, path: &Path) -> Result<(), RegistryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        restrict_perms_0600(path)?;
        Ok(())
    }

    /// Look up the token for `slug`. Falls back to the `GITHUB_TOKEN`
    /// env var so CI can drive `pkg publish` without dropping a file.
    pub fn token_for(&self, slug: &str) -> Option<String> {
        if let Some(t) = self.tokens.get(slug) {
            return Some(t.clone());
        }
        std::env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty())
    }

    /// Insert or update a token.
    pub fn set_token(&mut self, slug: impl Into<String>, token: impl Into<String>) {
        self.tokens.insert(slug.into(), token.into());
    }
}

#[cfg(unix)]
fn restrict_perms_0600(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perm = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perm)
}

#[cfg(not(unix))]
fn restrict_perms_0600(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_dedup_and_default_first() {
        let cfg = RegistryConfig {
            default: Some("a/b".into()),
            extras: vec!["a/b".into(), "c/d".into(), "c/d".into()],
        };
        let slugs = cfg.slugs();
        assert_eq!(slugs, vec!["a/b".to_string(), "c/d".into()]);
    }

    #[test]
    fn slugs_default_falls_back() {
        let cfg = RegistryConfig::default();
        let slugs = cfg.slugs();
        assert_eq!(slugs, vec![DEFAULT_REGISTRY_SLUG.to_string()]);
    }

    #[test]
    fn parse_slug_round_trip() {
        let (owner, repo) = parse_slug("foo/bar").unwrap();
        assert_eq!((owner.as_str(), repo.as_str()), ("foo", "bar"));
        assert!(parse_slug("nope").is_err());
        assert!(parse_slug("").is_err());
        assert!(parse_slug("foo/bar/baz").is_err());
    }

    #[test]
    fn split_tag_basic() {
        assert_eq!(
            split_tag("otel-0.1.0"),
            Some(("otel".into(), "0.1.0".into()))
        );
        // Package name with a dash: split on the dash before a digit.
        assert_eq!(
            split_tag("my-lib-1.2.3"),
            Some(("my-lib".into(), "1.2.3".into()))
        );
        assert_eq!(split_tag("v1.2.3"), None); // no dash
        assert_eq!(split_tag("just-words"), None); // no digit after dash
    }

    #[test]
    fn registry_config_loads_from_star_toml() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("mighty.toml");
        std::fs::write(
            &p,
            r#"
[package]
name = "app"
version = "0.1.0"
edition = "2026"

[registry]
default = "myorg/registry"
extras = ["other/private"]

[deps]
foo = "0.1"
"#,
        )
        .unwrap();
        let cfg = load_registry_config(&p).unwrap();
        assert_eq!(cfg.default.as_deref(), Some("myorg/registry"));
        assert_eq!(cfg.extras, vec!["other/private".to_string()]);
    }

    #[test]
    fn registry_config_missing_section_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("mighty.toml");
        std::fs::write(
            &p,
            r#"
[package]
name = "app"
version = "0.1.0"
edition = "2026"
"#,
        )
        .unwrap();
        let cfg = load_registry_config(&p).unwrap();
        assert_eq!(cfg.slugs(), vec![DEFAULT_REGISTRY_SLUG.to_string()]);
    }

    #[test]
    fn auth_store_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("auth.toml");
        let mut store = AuthStore::default();
        store.set_token("a/b", "ghp_abc");
        store.save(&p).unwrap();
        let reloaded = AuthStore::load(&p).unwrap();
        assert_eq!(
            reloaded.tokens.get("a/b").map(String::as_str),
            Some("ghp_abc")
        );
    }

    #[test]
    fn auth_store_falls_back_to_env() {
        let store = AuthStore::default();
        std::env::remove_var("GITHUB_TOKEN");
        assert!(store.token_for("x/y").is_none());
        std::env::set_var("GITHUB_TOKEN", "from-env");
        assert_eq!(store.token_for("x/y").as_deref(), Some("from-env"));
        std::env::remove_var("GITHUB_TOKEN");
    }

    #[test]
    fn parse_releases_page_picks_up_assets() {
        let body = r#"[
            {
                "tag_name": "otel-0.1.0",
                "html_url": "https://github.com/x/y/releases/tag/otel-0.1.0",
                "body": "stardust manifest",
                "assets": [
                    {"name": "otel-0.1.0.tar.gz", "browser_download_url": "https://example.com/otel-0.1.0.tar.gz"},
                    {"name": "otel-0.1.0.tar.gz.sha256", "browser_download_url": "https://example.com/otel-0.1.0.tar.gz.sha256"}
                ]
            },
            {
                "tag_name": "garbage",
                "assets": []
            }
        ]"#;
        let releases = parse_releases_page(body).unwrap();
        assert_eq!(releases.len(), 1);
        let r = &releases[0];
        assert_eq!(r.name, "otel");
        assert_eq!(r.version, "0.1.0");
        assert!(r.tarball_url.as_deref().unwrap().ends_with(".tar.gz"));
        assert!(r.sha256_url.as_deref().unwrap().ends_with(".sha256"));
    }

    #[test]
    fn cache_path_shape() {
        let p = cache_path(Path::new("/tmp/r"), "foo/bar");
        assert!(p.ends_with(".mighty/registry/foo__bar/index.json"));
    }

    #[test]
    fn gh_source_round_trip() {
        let url = gh_source("a/b");
        assert_eq!(url, "registry+gh://a/b");
        assert_eq!(slug_from_source(&url), Some("a/b"));
        assert_eq!(slug_from_source("registry+https://x"), None);
    }
}
