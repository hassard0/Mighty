//! Capability enforcement for MCP tool calls (v0.26 Track B).
//!
//! This is the LOAD-BEARING half of the @tool guarantee: the runtime
//! checks the caller's [`CapabilitySet`] BEFORE the tool body runs.
//! If the cap is missing, OR the requested resource is outside the
//! granted scope, the call short-circuits with
//! [`super::ToolError::CapabilityDenied`]. The LLM gets the denial as
//! a tool-result; the host never touches the resource.
//!
//! ## Mapping to the v0.21 cap-resolver
//!
//! The compile-time cap-resolver in `mty_types::cap_resolver` checks
//! that a Mighty fn that declares `cap: fs.read` is statically wired
//! to a fs-family capability. The runtime check here is the
//! *operational* half: even when the type system blesses the wiring,
//! the actual cap value supplied by the agent's manifest narrows
//! WHICH paths / hosts / models the tool may touch.
//!
//! ## Cap grant shapes
//!
//! - [`CapabilityGrant::Fs`] — read or read/write access to a list of
//!   path prefixes. Equivalent to v0.5's `FsCap::rooted(...)` plus a
//!   `mode` field.
//! - [`CapabilityGrant::Net`] — list of allowed host names (matched
//!   as suffixes — `"example.com"` grants `api.example.com` too).
//! - [`CapabilityGrant::Clock`] — read-only access to wall-clock /
//!   monotonic time.
//! - [`CapabilityGrant::Model`] — call an LLM provider; list of
//!   allowed provider names (`anthropic`, `openai`, …).
//! - [`CapabilityGrant::Custom`] — open-ended grant keyed by a string
//!   tag. Used by application-defined cap families that don't fit the
//!   built-in shapes.

use std::sync::{OnceLock, RwLock};

/// One capability grant. Each grant pins a family (Fs / Net / …) and
/// the resources the holder is allowed to touch within that family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityGrant {
    /// Filesystem grant. `roots` lists path prefixes; an empty list
    /// means "no restriction" (unrestricted within the family).
    Fs {
        mode: FsMode,
        roots: Vec<std::path::PathBuf>,
    },
    /// Network grant. `hosts` lists allowed hostnames (suffix match).
    /// Empty list = unrestricted within the family.
    Net { hosts: Vec<String> },
    /// Wall-clock / monotonic time access. No further narrowing today
    /// — either you have Clock or you don't.
    Clock,
    /// LLM provider grant. `providers` lists allowed providers
    /// (`anthropic`, `openai`, `gemini`, `bedrock`, …). Empty =
    /// unrestricted within the family.
    Model { providers: Vec<String> },
    /// Application-defined grant. The tag identifies the family; the
    /// `resources` list narrows within it (interpreted by the caller).
    Custom {
        family: String,
        resources: Vec<String>,
    },
}

/// Filesystem access mode for [`CapabilityGrant::Fs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsMode {
    /// Read-only: `fs.read`, `fs.list`, `fs.exists`.
    Read,
    /// Read + write: includes everything `Read` allows plus
    /// `fs.write`, `fs.remove`, …
    ReadWrite,
}

/// Bundle of capabilities held by a Mighty agent at one point in
/// time. The MCP server stamps each tool call with the agent's
/// current cap-set before invoking the tool body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilitySet {
    grants: Vec<CapabilityGrant>,
}

impl CapabilitySet {
    /// Empty cap set — denies every tool that declares any cap.
    pub fn empty() -> Self {
        Self { grants: Vec::new() }
    }

    /// Cap set from a list of grants.
    pub fn from_grants(grants: impl IntoIterator<Item = CapabilityGrant>) -> Self {
        Self {
            grants: grants.into_iter().collect(),
        }
    }

    /// Grant unrestricted access to every family. Useful for tests
    /// and fully-trusted CLI entry points. Production code should
    /// NEVER grant this to LLM-driven flows.
    pub fn unrestricted() -> Self {
        Self {
            grants: vec![
                CapabilityGrant::Fs {
                    mode: FsMode::ReadWrite,
                    roots: vec![],
                },
                CapabilityGrant::Net { hosts: vec![] },
                CapabilityGrant::Clock,
                CapabilityGrant::Model { providers: vec![] },
            ],
        }
    }

    /// Add a grant to the set. Returns the set for builder-style use.
    pub fn with(mut self, grant: CapabilityGrant) -> Self {
        self.grants.push(grant);
        self
    }

    /// Read-only access to the grant list.
    pub fn grants(&self) -> &[CapabilityGrant] {
        &self.grants
    }

    /// Check that the cap declared by a tool is satisfied for the
    /// given operation. The `required` argument is the dotted cap name
    /// from the @tool macro (`fs.read`, `net.get`, …); `resource` is
    /// the concrete value the tool is about to touch (a path for fs,
    /// a host for net, a provider for model, etc.).
    ///
    /// Returns `Ok(())` if the operation is allowed; otherwise
    /// `Err(reason)` with a human-readable explanation suitable for
    /// embedding in a [`super::ToolError::CapabilityDenied`].
    pub fn check(&self, required: &str, resource: &str) -> Result<(), String> {
        let (family, op) = split_cap(required);
        match family {
            "fs" => self.check_fs(op, resource),
            "net" => self.check_net(resource),
            "clock" => self.check_clock(),
            "model" => self.check_model(resource),
            other => self.check_custom(other, resource),
        }
    }

    fn check_fs(&self, op: &str, path_str: &str) -> Result<(), String> {
        let path = std::path::Path::new(path_str);
        let need_write = matches!(op, "write" | "remove" | "create" | "rw");
        let mut saw_family = false;
        for grant in &self.grants {
            if let CapabilityGrant::Fs { mode, roots } = grant {
                saw_family = true;
                if need_write && *mode != FsMode::ReadWrite {
                    continue;
                }
                if roots.is_empty() {
                    return Ok(());
                }
                if roots.iter().any(|r| path.starts_with(r)) {
                    return Ok(());
                }
            }
        }
        if !saw_family {
            return Err(format!(
                "capability set has no `fs` grant (path `{path_str}` denied)"
            ));
        }
        Err(format!(
            "path `{path_str}` is outside the granted `fs` roots"
        ))
    }

    fn check_net(&self, host: &str) -> Result<(), String> {
        let mut saw_family = false;
        for grant in &self.grants {
            if let CapabilityGrant::Net { hosts } = grant {
                saw_family = true;
                if hosts.is_empty() {
                    return Ok(());
                }
                if hosts
                    .iter()
                    .any(|h| host == h || host.ends_with(&format!(".{h}")))
                {
                    return Ok(());
                }
            }
        }
        if !saw_family {
            return Err(format!(
                "capability set has no `net` grant (host `{host}` denied)"
            ));
        }
        Err(format!(
            "host `{host}` is outside the granted `net` allowlist"
        ))
    }

    fn check_clock(&self) -> Result<(), String> {
        if self
            .grants
            .iter()
            .any(|g| matches!(g, CapabilityGrant::Clock))
        {
            Ok(())
        } else {
            Err("capability set has no `clock` grant".into())
        }
    }

    fn check_model(&self, provider: &str) -> Result<(), String> {
        let mut saw_family = false;
        for grant in &self.grants {
            if let CapabilityGrant::Model { providers } = grant {
                saw_family = true;
                if providers.is_empty() {
                    return Ok(());
                }
                if providers.iter().any(|p| p == provider) {
                    return Ok(());
                }
            }
        }
        if !saw_family {
            return Err(format!(
                "capability set has no `model` grant (provider `{provider}` denied)"
            ));
        }
        Err(format!(
            "provider `{provider}` is outside the granted `model` allowlist"
        ))
    }

    fn check_custom(&self, family: &str, resource: &str) -> Result<(), String> {
        let mut saw_family = false;
        for grant in &self.grants {
            if let CapabilityGrant::Custom {
                family: f,
                resources,
            } = grant
            {
                if f == family {
                    saw_family = true;
                    if resources.is_empty() {
                        return Ok(());
                    }
                    if resources.iter().any(|r| r == resource) {
                        return Ok(());
                    }
                }
            }
        }
        if !saw_family {
            return Err(format!(
                "capability set has no `{family}` grant (resource `{resource}` denied)"
            ));
        }
        Err(format!(
            "resource `{resource}` is outside the granted `{family}` allowlist"
        ))
    }
}

/// Split a cap name like `"fs.read"` into `("fs", "read")`. Bare
/// names (no dot) treat the entire string as the family with empty
/// op.
fn split_cap(required: &str) -> (&str, &str) {
    match required.find('.') {
        Some(idx) => (&required[..idx], &required[idx + 1..]),
        None => (required, ""),
    }
}

// ---------------------------------------------------------------------------
// Process-wide default capability set
// ---------------------------------------------------------------------------
//
// Mirrors the `fs::install_default_*_cap` pattern from v0.5: the
// driver installs the agent's manifest-derived cap set at process
// start, and tool invocations consult it when no per-call cap is
// supplied.

static DEFAULT_CAP_SET: OnceLock<RwLock<CapabilitySet>> = OnceLock::new();

fn default_slot() -> &'static RwLock<CapabilitySet> {
    DEFAULT_CAP_SET.get_or_init(|| RwLock::new(CapabilitySet::empty()))
}

/// Install the process-wide default capability set, returning the
/// previous one (so tests can save+restore around a scope).
pub fn install_default_capability_set(caps: CapabilitySet) -> CapabilitySet {
    let mut g = default_slot().write().expect("DEFAULT_CAP_SET poisoned");
    std::mem::replace(&mut *g, caps)
}

/// Snapshot the current default cap set.
pub fn current_default_capability_set() -> CapabilitySet {
    default_slot()
        .read()
        .expect("DEFAULT_CAP_SET poisoned")
        .clone()
}

/// Save/restore the default cap set around a closure. Tests that
/// mutate the default cap set should use this helper to avoid leaking
/// state across runs.
pub fn with_default_capability_set<R>(caps: CapabilitySet, body: impl FnOnce() -> R) -> R {
    let prev = install_default_capability_set(caps);
    let result = body();
    let _ = install_default_capability_set(prev);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_set_denies_everything() {
        let caps = CapabilitySet::empty();
        assert!(caps.check("fs.read", "/tmp/x").is_err());
        assert!(caps.check("net.get", "example.com").is_err());
        assert!(caps.check("clock.now", "").is_err());
        assert!(caps.check("model.call", "anthropic").is_err());
    }

    #[test]
    fn unrestricted_set_allows_everything() {
        let caps = CapabilitySet::unrestricted();
        assert!(caps.check("fs.read", "/etc/passwd").is_ok());
        assert!(caps.check("fs.write", "/var/log/x").is_ok());
        assert!(caps.check("net.get", "example.com").is_ok());
        assert!(caps.check("clock.now", "").is_ok());
        assert!(caps.check("model.call", "anthropic").is_ok());
    }

    #[test]
    fn fs_read_grant_allows_read_denies_write() {
        let caps = CapabilitySet::from_grants([CapabilityGrant::Fs {
            mode: FsMode::Read,
            roots: vec![],
        }]);
        assert!(caps.check("fs.read", "/tmp/x").is_ok());
        let err = caps.check("fs.write", "/tmp/x").unwrap_err();
        assert!(err.contains("fs"), "{err}");
    }

    #[test]
    fn fs_roots_narrow_the_grant() {
        let caps = CapabilitySet::from_grants([CapabilityGrant::Fs {
            mode: FsMode::Read,
            roots: vec!["/data".into()],
        }]);
        assert!(caps.check("fs.read", "/data/inner/file").is_ok());
        let err = caps.check("fs.read", "/etc/passwd").unwrap_err();
        assert!(err.contains("/etc/passwd"), "{err}");
    }

    #[test]
    fn net_grant_suffix_matches_subdomains() {
        let caps = CapabilitySet::from_grants([CapabilityGrant::Net {
            hosts: vec!["example.com".into()],
        }]);
        assert!(caps.check("net.get", "example.com").is_ok());
        assert!(caps.check("net.get", "api.example.com").is_ok());
        assert!(caps.check("net.get", "evil.com").is_err());
    }

    #[test]
    fn model_grant_narrows_providers() {
        let caps = CapabilitySet::from_grants([CapabilityGrant::Model {
            providers: vec!["anthropic".into()],
        }]);
        assert!(caps.check("model.call", "anthropic").is_ok());
        assert!(caps.check("model.call", "openai").is_err());
    }

    #[test]
    fn custom_grant_routes_by_family_tag() {
        let caps = CapabilitySet::from_grants([CapabilityGrant::Custom {
            family: "secrets".into(),
            resources: vec!["api_key".into()],
        }]);
        assert!(caps.check("secrets.read", "api_key").is_ok());
        assert!(caps.check("secrets.read", "db_password").is_err());
        assert!(caps.check("other.read", "api_key").is_err());
    }

    #[test]
    fn default_cap_set_round_trip() {
        let original = current_default_capability_set();
        let prev = install_default_capability_set(CapabilitySet::unrestricted());
        assert!(current_default_capability_set()
            .check("fs.read", "/")
            .is_ok());
        let _ = install_default_capability_set(prev);
        let _ = original; // suppress unused warning
    }
}
