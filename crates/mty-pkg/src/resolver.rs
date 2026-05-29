//! Greedy dependency resolver.
//!
//! v0.2 shipped an MVP resolver intended for hand-authored manifests
//! with shallow dep trees. v0.4 wires it up to the
//! GitHub-Releases-backed registry: registry deps are now resolved
//! against a cached index of `(name, version)` pairs, picking the
//! highest version that satisfies the req. When no index is available
//! (no network, registry empty, fetch error), the resolver falls back
//! to the v0.2 "requirement floor" synthesis so offline development
//! keeps working.
//!
//! Sources of version information:
//!
//! - **Path / git deps**: the chosen "version" is whatever the
//!   manifest at that source reports. No semver check beyond the
//!   manifest's own version string.
//! - **Registry deps**: union the indexes of the configured registries
//!   (default first, extras in order; first match wins on duplicate
//!   `(name, version)`). Pick the highest version satisfying the req.
//!   Fall back to the requirement floor if no index entry matches and
//!   no network is available.

use crate::lockfile::{LockedPackage, Lockfile};
use crate::registry::{self, RegistryConfig};
use crate::semver::{Version, VersionReq};
use mty_driver::manifest::{Dep, DepSourceKind, Manifest};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("manifest error in {pkg}: {source}")]
    Manifest {
        pkg: String,
        #[source]
        source: mty_driver::manifest::ManifestError,
    },
    #[error("semver error for `{dep}`: {source}")]
    Semver {
        dep: String,
        #[source]
        source: crate::semver::SemverError,
    },
    #[error(
        "version conflict for `{name}`: already chose {chosen}, but `{requestor}` requires {req}"
    )]
    VersionConflict {
        name: String,
        chosen: String,
        requestor: String,
        req: String,
    },
    #[error("dependency `{name}` declares both path and git sources; choose one")]
    AmbiguousSource { name: String },
    #[error("dependency `{name}` has no version, path, or git source")]
    EmptySource { name: String },
    #[error("path dependency `{name}` source not found at {path}")]
    PathMissing { name: String, path: PathBuf },
}

/// Greedy resolver state. Owns the package-root path for resolving
/// relative path deps + the registry config used for registry-dep
/// lookups.
pub struct Resolver {
    root: PathBuf,
    registries: RegistryConfig,
    /// Optional pre-loaded indexes (one per slug). When set, the
    /// resolver uses these in preference to hitting the network. Tests
    /// inject hand-rolled indexes here.
    pub injected_indexes: Vec<crate::registry::RegistryIndex>,
}

impl Resolver {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let cfg =
            registry::load_registry_config(&root.join(crate::MANIFEST_NAME)).unwrap_or_default();
        Resolver {
            root,
            registries: cfg,
            injected_indexes: Vec::new(),
        }
    }

    /// Construct a resolver with an explicit registry config. Used by
    /// tests + by callers that have already parsed the manifest.
    pub fn with_registry_config(root: impl Into<PathBuf>, cfg: RegistryConfig) -> Self {
        Resolver {
            root: root.into(),
            registries: cfg,
            injected_indexes: Vec::new(),
        }
    }

    /// Resolve `manifest` into a lockfile.
    ///
    /// The root package itself is *not* added to the lockfile — only
    /// its transitive deps. The lockfile entries have empty `hash`
    /// fields; a later `fetch` step fills them in.
    pub fn resolve(&self, manifest: &Manifest) -> Result<Lockfile, ResolveError> {
        let mut chosen: BTreeMap<String, ChosenDep> = BTreeMap::new();
        let mut visited: BTreeSet<String> = BTreeSet::new();
        self.walk(manifest, &self.root, &mut chosen, &mut visited)?;

        let mut lock = Lockfile::new();
        for (name, dep) in chosen {
            lock.upsert(LockedPackage {
                name,
                version: dep.version.to_string(),
                source: dep.source,
                hash: None,
                dependencies: dep.dependencies,
            });
        }
        Ok(lock)
    }

    fn walk(
        &self,
        manifest: &Manifest,
        manifest_root: &Path,
        chosen: &mut BTreeMap<String, ChosenDep>,
        visited: &mut BTreeSet<String>,
    ) -> Result<(), ResolveError> {
        for (name, dep) in &manifest.deps {
            validate_dep(name, dep)?;
            let kind = dep.source_kind();
            let (version, source, sub_manifest_dir) =
                self.resolve_one(name, dep, kind, manifest_root)?;

            // Conflict check.
            if let Some(existing) = chosen.get(name) {
                if existing.version != version {
                    return Err(ResolveError::VersionConflict {
                        name: name.clone(),
                        chosen: existing.version.to_string(),
                        requestor: manifest.package.name.clone(),
                        req: version.to_string(),
                    });
                }
                continue;
            }

            // Record the choice up-front so cycles terminate.
            chosen.insert(
                name.clone(),
                ChosenDep {
                    version,
                    source: source.clone(),
                    dependencies: Vec::new(),
                },
            );

            // Recurse into sub-manifests if we have a real directory
            // to read (path / git after fetch). For registry deps we
            // can't recurse without the registry, so we stop there.
            if visited.insert(name.clone()) {
                if let Some(dir) = sub_manifest_dir {
                    let star_toml = dir.join(crate::MANIFEST_NAME);
                    if star_toml.exists() {
                        let sub = mty_driver::manifest::load(&star_toml).map_err(|e| {
                            ResolveError::Manifest {
                                pkg: name.clone(),
                                source: e,
                            }
                        })?;
                        let direct_deps: Vec<String> = sub.deps.keys().cloned().collect();
                        if let Some(slot) = chosen.get_mut(name) {
                            slot.dependencies = direct_deps;
                        }
                        self.walk(&sub, &dir, chosen, visited)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn resolve_one(
        &self,
        name: &str,
        dep: &Dep,
        kind: DepSourceKind,
        manifest_root: &Path,
    ) -> Result<(Version, String, Option<PathBuf>), ResolveError> {
        match kind {
            DepSourceKind::Path => {
                let rel = dep.path().expect("path source has path()");
                let abs = manifest_root.join(rel);
                if !abs.exists() {
                    return Err(ResolveError::PathMissing {
                        name: name.into(),
                        path: abs,
                    });
                }
                let star = abs.join(crate::MANIFEST_NAME);
                let version = if star.exists() {
                    let sub =
                        mty_driver::manifest::load(&star).map_err(|e| ResolveError::Manifest {
                            pkg: name.into(),
                            source: e,
                        })?;
                    Version::parse(&sub.package.version).map_err(|e| ResolveError::Semver {
                        dep: name.into(),
                        source: e,
                    })?
                } else {
                    Version::parse("0.0.0").unwrap()
                };
                let source = LockedPackage::path_source(&abs);
                Ok((version, source, Some(abs)))
            }
            DepSourceKind::Git => {
                let url = dep.git().expect("git source has git()");
                let rev = dep.rev();
                let v = Version::parse("0.0.0").unwrap();
                let source = LockedPackage::git_source(url, rev);
                Ok((v, source, None))
            }
            DepSourceKind::Registry => {
                let req_str = dep.version().unwrap_or("*");
                let req = VersionReq::parse(req_str).map_err(|e| ResolveError::Semver {
                    dep: name.into(),
                    source: e,
                })?;
                // Try each configured registry in order; first hit wins.
                let slugs = self.registries.slugs();
                for slug in &slugs {
                    if let Some((v, _)) = self.lookup_in_registry(slug, name, &req) {
                        let source = registry::gh_source(slug);
                        return Ok((v, source, None));
                    }
                }
                // Fallback: synthesise the requirement floor and pin
                // the default registry as the source. This keeps
                // offline development working — the lockfile is still
                // usable; `pkg fetch` will fail with a clear error if
                // the package really isn't available.
                let v = requirement_floor(&req);
                let default_slug = slugs
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| crate::registry::DEFAULT_REGISTRY_SLUG.to_string());
                let source = registry::gh_source(&default_slug);
                Ok((v, source, None))
            }
        }
    }

    /// Look up `(name, req)` against the index of `slug`. Prefers
    /// `injected_indexes` (tests); otherwise reads the on-disk cache.
    /// Does **not** hit the network — keeps `resolve` fast + offline.
    /// Operators run `pkg update --refresh` to pull a fresh index.
    fn lookup_in_registry(
        &self,
        slug: &str,
        name: &str,
        req: &VersionReq,
    ) -> Option<(Version, String)> {
        // Injected first.
        let injected = self.injected_indexes.iter().find(|i| i.slug == slug);
        if let Some(idx) = injected {
            if let Some(v) = highest_matching(idx, name, req) {
                return Some((v, slug.into()));
            }
            return None;
        }
        // On-disk cache.
        let idx = registry::load_cached_index(&self.root, slug)
            .ok()
            .flatten()?;
        highest_matching(&idx, name, req).map(|v| (v, slug.into()))
    }
}

fn highest_matching(
    idx: &crate::registry::RegistryIndex,
    name: &str,
    req: &VersionReq,
) -> Option<Version> {
    let mut best: Option<Version> = None;
    for vstr in idx.versions_for(name) {
        let Ok(v) = Version::parse(vstr) else {
            continue;
        };
        if req.matches(&v) && Some(v) > best {
            best = Some(v);
        }
    }
    best
}

fn validate_dep(name: &str, dep: &Dep) -> Result<(), ResolveError> {
    if dep.path().is_some() && dep.git().is_some() {
        return Err(ResolveError::AmbiguousSource { name: name.into() });
    }
    if matches!(dep, Dep::Detailed(d) if d.version.is_none() && d.path.is_none() && d.git.is_none())
    {
        return Err(ResolveError::EmptySource { name: name.into() });
    }
    Ok(())
}

/// Pick the lowest version satisfying `req` when no index is
/// available — synthesises a version from the requirement itself.
fn requirement_floor(req: &VersionReq) -> Version {
    use crate::semver::CaretFloorWidth;
    match req {
        VersionReq::Wildcard => Version::parse("0.0.0").unwrap(),
        VersionReq::Exact(v) => *v,
        VersionReq::Caret(v, _) | VersionReq::Tilde(v) => {
            let _ = CaretFloorWidth::Patch;
            *v
        }
    }
}

/// Helper for `pkg list` / debug printing.
#[derive(Debug, Clone)]
pub struct ChosenDep {
    pub version: Version,
    pub source: String,
    pub dependencies: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{RegistryIndex, RegistryRelease};
    use mty_driver::manifest::{DetailedDep, Package};

    fn pkg(name: &str) -> Package {
        Package {
            name: name.into(),
            version: "0.1.0".into(),
            edition: "2026".into(),
            profile: "host".into(),
        }
    }

    fn release(name: &str, version: &str) -> RegistryRelease {
        RegistryRelease {
            name: name.into(),
            version: version.into(),
            tag: format!("{name}-{version}"),
            tarball_url: None,
            sha256_url: None,
            html_url: None,
            body_preview: None,
        }
    }

    #[test]
    fn resolves_single_registry_dep_with_default_slug() {
        let mut deps = BTreeMap::new();
        deps.insert("std".into(), Dep::Version("0.1".into()));
        let m = Manifest {
            package: pkg("app"),
            deps,
            build: None,
            cluster: None,
            extern_libs: Vec::new(),
        };
        let dir = tempfile::tempdir().unwrap();
        let lock = Resolver::new(dir.path()).resolve(&m).unwrap();
        assert_eq!(lock.packages.len(), 1);
        assert_eq!(lock.packages[0].name, "std");
        assert!(lock.packages[0].source.starts_with("registry+gh://"));
    }

    #[test]
    fn empty_source_errors() {
        let mut deps = BTreeMap::new();
        deps.insert(
            "bad".into(),
            Dep::Detailed(DetailedDep {
                ..Default::default()
            }),
        );
        let m = Manifest {
            package: pkg("app"),
            deps,
            build: None,
            cluster: None,
            extern_libs: Vec::new(),
        };
        let dir = tempfile::tempdir().unwrap();
        let err = Resolver::new(dir.path()).resolve(&m).unwrap_err();
        assert!(matches!(err, ResolveError::EmptySource { .. }));
    }

    #[test]
    fn injected_index_picks_highest_match() {
        let mut deps = BTreeMap::new();
        deps.insert("std".into(), Dep::Version("^0.1".into()));
        let m = Manifest {
            package: pkg("app"),
            deps,
            build: None,
            cluster: None,
            extern_libs: Vec::new(),
        };
        let dir = tempfile::tempdir().unwrap();
        let mut r = Resolver::with_registry_config(
            dir.path(),
            RegistryConfig {
                default: Some("foo/bar".into()),
                extras: vec![],
                signing: crate::registry::SigningConfig::default(),
            },
        );
        let mut idx = RegistryIndex::new("foo/bar");
        idx.releases.push(release("std", "0.1.0"));
        idx.releases.push(release("std", "0.1.7"));
        idx.releases.push(release("std", "0.2.0")); // not matching ^0.1
        r.injected_indexes.push(idx);
        let lock = r.resolve(&m).unwrap();
        assert_eq!(lock.packages[0].version, "0.1.7");
        assert_eq!(lock.packages[0].source, "registry+gh://foo/bar");
    }

    #[test]
    fn multi_registry_first_match_wins() {
        let mut deps = BTreeMap::new();
        deps.insert("foo".into(), Dep::Version("*".into()));
        let m = Manifest {
            package: pkg("app"),
            deps,
            build: None,
            cluster: None,
            extern_libs: Vec::new(),
        };
        let dir = tempfile::tempdir().unwrap();
        let mut r = Resolver::with_registry_config(
            dir.path(),
            RegistryConfig {
                default: Some("a/a".into()),
                extras: vec!["b/b".into()],
                signing: crate::registry::SigningConfig::default(),
            },
        );
        let mut idx_b = RegistryIndex::new("b/b");
        idx_b.releases.push(release("foo", "9.9.9"));
        let mut idx_a = RegistryIndex::new("a/a");
        idx_a.releases.push(release("foo", "0.0.1"));
        // Insert b first so we can tell `a/a` (default) still wins.
        r.injected_indexes.push(idx_b);
        r.injected_indexes.push(idx_a);
        let lock = r.resolve(&m).unwrap();
        assert_eq!(lock.packages[0].version, "0.0.1");
        assert_eq!(lock.packages[0].source, "registry+gh://a/a");
    }

    #[test]
    fn registry_fallback_to_requirement_floor() {
        // No indexes loaded at all -> floor synthesis.
        let mut deps = BTreeMap::new();
        deps.insert("std".into(), Dep::Version("^0.2.5".into()));
        let m = Manifest {
            package: pkg("app"),
            deps,
            build: None,
            cluster: None,
            extern_libs: Vec::new(),
        };
        let dir = tempfile::tempdir().unwrap();
        let lock = Resolver::new(dir.path()).resolve(&m).unwrap();
        assert_eq!(lock.packages[0].version, "0.2.5");
    }
}
