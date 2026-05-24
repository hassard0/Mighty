//! sdust-pkg — Stardust package manager.
//!
//! v0.2 ships the resolver + lockfile + fetcher infrastructure that
//! the spec §5 package model needs. The registry endpoint
//! `https://pkg.stardust.dev` is *not yet live*; the registry fetcher
//! exists as scaffolding and will error with a clear message until the
//! cloud control plane comes up post-v0.2.
//!
//! # Surface
//!
//! - [`manifest`] — re-export of the extended manifest types
//!   (`Manifest`, `Dep`, `BuildConfig`) from `sdust-driver`.
//! - [`lockfile`] — `star.lock` schema, parse + serialize.
//! - [`resolver`] — greedy DFS dep-graph walk with semver matching.
//! - [`fetch`] — pluggable fetchers for path / git / registry sources.
//! - [`hash`] — sha256-of-tree helper for fetch verification.
//! - [`publish`] — tarball bundler used by `sdust pkg publish`.
//! - [`commands`] — high-level operations driving `sdust pkg <cmd>`.

pub mod commands;
pub mod fetch;
pub mod hash;
pub mod lockfile;
pub mod publish;
pub mod resolver;
pub mod semver;

pub use sdust_driver::manifest::{
    BuildConfig, Dep, DepSourceKind, DetailedDep, Manifest, ManifestError, Package,
};

pub use lockfile::{LockedPackage, Lockfile, LockfileError, DEFAULT_REGISTRY};
pub use resolver::{ResolveError, Resolver};

/// Conventional path to the lockfile relative to a package root.
pub const LOCKFILE_NAME: &str = "star.lock";

/// Conventional path to the manifest relative to a package root.
pub const MANIFEST_NAME: &str = "star.toml";

/// Default subdirectory under the package root where fetched dependency
/// trees are materialised.
pub const PKG_CACHE_DIR: &str = ".stardust/pkgs";
