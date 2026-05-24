//! Greedy dependency resolver.
//!
//! v0.2 ships an MVP resolver intended for hand-authored manifests
//! with shallow dep trees. It walks the dep graph depth-first,
//! recording one chosen version per package. Conflicts (two requests
//! for the same package that don't intersect) error out — there is no
//! backtracking or unification.
//!
//! Sources of version information:
//!
//! - **Path / git deps**: the chosen "version" is whatever the
//!   manifest at that source reports. No semver check beyond the
//!   manifest's own version string (registry version requirements on
//!   path/git deps are ignored — matches cargo's behaviour).
//! - **Registry deps**: v0.2's registry is a stub. The resolver
//!   records the request and lets the lockfile pin a synthesised
//!   version equal to the requirement floor (e.g. `^0.1.0` → `0.1.0`).
//!   When the real registry comes online, this becomes an HTTP lookup.

use crate::lockfile::{LockedPackage, Lockfile, DEFAULT_REGISTRY};
use crate::semver::{Version, VersionReq};
use sdust_driver::manifest::{Dep, DepSourceKind, Manifest};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("manifest error in {pkg}: {source}")]
    Manifest {
        pkg: String,
        #[source]
        source: sdust_driver::manifest::ManifestError,
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
/// relative path deps.
pub struct Resolver {
    root: PathBuf,
}

impl Resolver {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Resolver { root: root.into() }
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
                        let sub = sdust_driver::manifest::load(&star_toml).map_err(|e| {
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
                // Try to read the path-dep's manifest for its version.
                let star = abs.join(crate::MANIFEST_NAME);
                let version = if star.exists() {
                    let sub = sdust_driver::manifest::load(&star).map_err(|e| {
                        ResolveError::Manifest {
                            pkg: name.into(),
                            source: e,
                        }
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
                // We cannot know the version of a git dep without
                // cloning. Pre-fetch we synthesise 0.0.0; the fetcher
                // can rewrite this once the manifest is on disk.
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
                // Registry stub: pick the requirement floor.
                let v = requirement_floor(&req);
                let source = LockedPackage::registry_source(DEFAULT_REGISTRY);
                Ok((v, source, None))
            }
        }
    }
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

/// Pick the lowest version satisfying `req`. v0.2's registry has no
/// catalogue, so this synthesises a version from the requirement
/// itself.
fn requirement_floor(req: &VersionReq) -> Version {
    use crate::semver::CaretFloorWidth;
    match req {
        VersionReq::Wildcard => Version::parse("0.0.0").unwrap(),
        VersionReq::Exact(v) => *v,
        VersionReq::Caret(v, _) | VersionReq::Tilde(v) => {
            // For partial caret reqs (`^1` / `^1.2`) the floor is the
            // version with missing components zeroed, which is exactly
            // what the parser stored.
            let _ = CaretFloorWidth::Patch; // suppress unused warning if linted
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
    use sdust_driver::manifest::{DetailedDep, Package};

    fn pkg(name: &str) -> Package {
        Package {
            name: name.into(),
            version: "0.1.0".into(),
            edition: "2026".into(),
            profile: "host".into(),
        }
    }

    #[test]
    fn resolves_single_registry_dep() {
        let mut deps = BTreeMap::new();
        deps.insert("std".into(), Dep::Version("0.1".into()));
        let m = Manifest {
            package: pkg("app"),
            deps,
            build: None,
        };
        let dir = tempfile::tempdir().unwrap();
        let lock = Resolver::new(dir.path()).resolve(&m).unwrap();
        assert_eq!(lock.packages.len(), 1);
        assert_eq!(lock.packages[0].name, "std");
        assert!(lock.packages[0].source.starts_with("registry+"));
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
        };
        let dir = tempfile::tempdir().unwrap();
        let err = Resolver::new(dir.path()).resolve(&m).unwrap_err();
        assert!(matches!(err, ResolveError::EmptySource { .. }));
    }
}
