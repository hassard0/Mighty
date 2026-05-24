//! mty-pkg — Mighty package manager.
//!
//! v0.2 shipped the resolver + lockfile + 4 source types. v0.4 turns
//! the registry path from a stub into a real transport backed by
//! GitHub Releases:
//!
//! - A registry is a GitHub repo (`<owner>/<repo>`) whose Releases
//!   host published packages, one release per `(name, version)`.
//! - The fetcher caches the release index at
//!   `.mighty/registry/<owner>__<repo>/index.json` with a 1-hour
//!   TTL and honours `If-Modified-Since` for cheap refetches.
//! - The lockfile records sources as `registry+gh://<owner>/<repo>`.
//! - Auth (for private registries + publish) is per-user, stored in
//!   `~/.config/sdust/auth.toml` (plaintext, `0600` on Unix).
//! - Publish bundles the package as a real `tar.gz` + sha256 sidecar
//!   and creates a GitHub release when a token is available.
//!
//! See `docs/internals/package-manager.md` and
//! `docs/reference/registry.md` for the full architecture.
//!
//! # Surface
//!
//! - [`manifest`] — re-export of the extended manifest types
//!   (`Manifest`, `Dep`, `BuildConfig`) from `mty-driver`.
//! - [`lockfile`] — `mighty.lock` schema, parse + serialize.
//! - [`registry`] — `[registry]` config, auth store, cached index.
//! - [`resolver`] — greedy DFS dep-graph walk with semver matching +
//!   index-aware registry lookups.
//! - [`fetch`] — pluggable fetchers for path / git / registry sources.
//! - [`hash`] — sha256-of-tree helper for fetch verification.
//! - [`publish`] — tarball bundler used by `mty pkg publish`.
//! - [`commands`] — high-level operations driving `mty pkg <cmd>`.

pub mod commands;
pub mod fetch;
pub mod hash;
pub mod lockfile;
pub mod publish;
pub mod registry;
pub mod resolver;
pub mod semver;

pub use mty_driver::manifest::{
    BuildConfig, Dep, DepSourceKind, DetailedDep, Manifest, ManifestError, Package,
};

pub use lockfile::{LockedPackage, Lockfile, LockfileError, DEFAULT_REGISTRY};
pub use registry::{
    AuthStore, RegistryConfig, RegistryError, RegistryIndex, RegistryRelease,
    DEFAULT_REGISTRY_SLUG, INDEX_TTL_SECS,
};
pub use resolver::{ResolveError, Resolver};

/// Conventional path to the lockfile relative to a package root.
pub const LOCKFILE_NAME: &str = "mighty.lock";

/// Conventional path to the manifest relative to a package root.
pub const MANIFEST_NAME: &str = "mighty.toml";

/// Default subdirectory under the package root where fetched dependency
/// trees are materialised.
pub const PKG_CACHE_DIR: &str = ".mighty/pkgs";

/// Default subdirectory under the package root where cached registry
/// indexes live.
pub const REGISTRY_CACHE_DIR: &str = ".mighty/registry";
