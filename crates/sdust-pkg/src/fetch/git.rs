//! Git fetcher: clone + checkout via `git2`.
//!
//! Source URL form: `git+<url>` or `git+<url>#<rev>`.
//!
//! When the `git-fetch` cargo feature is disabled (e.g. very tight
//! build environments), this module returns a clear error. The
//! default build includes it.

use super::{FetchError, Fetched};
use crate::lockfile::LockedPackage;
use std::path::Path;

#[cfg(feature = "git-fetch")]
pub fn fetch(locked: &LockedPackage, slot: &Path) -> Result<Fetched, FetchError> {
    use crate::hash;

    let (url, rev) = split_url(&locked.source)?;

    if slot.exists() {
        std::fs::remove_dir_all(slot)?;
    }
    std::fs::create_dir_all(slot)?;

    let repo = git2::Repository::clone(url, slot).map_err(|e| FetchError::Git(e.to_string()))?;

    if let Some(rev) = rev {
        let oid = repo
            .revparse_single(rev)
            .map_err(|e| FetchError::Git(format!("rev `{rev}`: {e}")))?
            .id();
        let commit = repo
            .find_commit(oid)
            .map_err(|e| FetchError::Git(e.to_string()))?;
        repo.checkout_tree(commit.as_object(), None)
            .map_err(|e| FetchError::Git(e.to_string()))?;
        repo.set_head_detached(oid)
            .map_err(|e| FetchError::Git(e.to_string()))?;
    }

    let actual_hash = hash::hash_tree(slot)?;
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

#[cfg(not(feature = "git-fetch"))]
pub fn fetch(_locked: &LockedPackage, _slot: &Path) -> Result<Fetched, FetchError> {
    Err(FetchError::Git(
        "git fetcher disabled (build without --no-default-features to enable)".into(),
    ))
}

fn split_url(source: &str) -> Result<(&str, Option<&str>), FetchError> {
    let rest = source
        .strip_prefix("git+")
        .ok_or_else(|| FetchError::UnsupportedSource(source.into()))?;
    match rest.split_once('#') {
        Some((url, rev)) => Ok((url, Some(rev))),
        None => Ok((rest, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_url_with_rev() {
        let (url, rev) = split_url("git+https://github.com/foo/bar#abc123").unwrap();
        assert_eq!(url, "https://github.com/foo/bar");
        assert_eq!(rev, Some("abc123"));
    }

    #[test]
    fn parses_url_without_rev() {
        let (url, rev) = split_url("git+https://github.com/foo/bar").unwrap();
        assert_eq!(url, "https://github.com/foo/bar");
        assert_eq!(rev, None);
    }

    #[test]
    fn rejects_non_git_source() {
        assert!(split_url("registry+https://x").is_err());
    }
}
