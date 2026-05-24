//! Registry fetcher — v0.2 stub.
//!
//! Talks to `https://pkg.stardust.dev/<name>/<version>.tar.gz` via
//! `reqwest::blocking`. The registry itself is **not yet live**;
//! production use will land in a later slice when the cloud control
//! plane spins it up. Until then this fetcher will return a clear
//! "registry unreachable" error from any network attempt.
//!
//! For tests that need a registry shape, point a local HTTP fixture
//! at the same URL pattern; the fetcher does not care where the bytes
//! come from.

use super::{FetchError, Fetched};
use crate::lockfile::LockedPackage;
use std::path::Path;

#[cfg(feature = "registry-fetch")]
pub fn fetch(locked: &LockedPackage, slot: &Path) -> Result<Fetched, FetchError> {
    use crate::hash;

    let base = locked
        .source
        .strip_prefix("registry+")
        .ok_or_else(|| FetchError::UnsupportedSource(locked.source.clone()))?;
    let url = format!(
        "{}/{}/{}.tar.gz",
        base.trim_end_matches('/'),
        locked.name,
        locked.version
    );

    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("sdust-pkg/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| FetchError::Registry(e.to_string()))?;

    let resp = client.get(&url).send().map_err(|e| {
        FetchError::Registry(format!(
            "could not reach `{url}` (the Stardust registry is not yet live in v0.2): {e}"
        ))
    })?;
    if !resp.status().is_success() {
        return Err(FetchError::Registry(format!(
            "registry returned HTTP {} for {}",
            resp.status(),
            url
        )));
    }
    let bytes = resp
        .bytes()
        .map_err(|e| FetchError::Registry(e.to_string()))?;

    // Extraction: rather than pulling in tar+flate2 in v0.2, we
    // record the tarball bytes verbatim and hash them. A later slice
    // will extract into `slot`.
    if slot.exists() {
        std::fs::remove_dir_all(slot)?;
    }
    std::fs::create_dir_all(slot)?;
    let tarball_path = slot.join("source.tar.gz");
    std::fs::write(&tarball_path, &bytes)?;

    let actual_hash = hash::hash_bytes(&bytes);
    if let Some(expected) = &locked.hash {
        if expected != &actual_hash {
            return Err(FetchError::HashMismatch {
                name: locked.name.clone(),
                expected: expected.clone(),
                actual: actual_hash,
            });
        }
    }
    Ok(Fetched {
        root: slot.to_path_buf(),
        hash: actual_hash,
    })
}

#[cfg(not(feature = "registry-fetch"))]
pub fn fetch(_locked: &LockedPackage, _slot: &Path) -> Result<Fetched, FetchError> {
    Err(FetchError::Registry(
        "registry fetcher disabled at build time".into(),
    ))
}
