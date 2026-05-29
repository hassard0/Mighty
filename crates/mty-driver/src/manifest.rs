//! Package manifest (`mighty.toml`) types and loader.
//!
//! v0.1 shipped a minimal `Manifest { package, deps: BTreeMap<String,
//! String> }`. v0.2 extends `[deps]` to support detailed source
//! specifications (path / git / registry) and adds an optional `[build]`
//! section for the build-script sandbox scaffold (spec §5.4).
//!
//! The flexible `Dep` enum uses serde's untagged form so both
//! ```toml
//! foo = "0.1"
//! ```
//! and
//! ```toml
//! foo = { version = "0.1", path = "../foo" }
//! ```
//! parse correctly. The simple string form is canonicalised to
//! `Dep::Version(_)`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub package: Package,
    #[serde(default)]
    pub deps: BTreeMap<String, Dep>,
    /// Build-script sandbox scaffold (spec §5.4).
    ///
    /// Enforcement is deferred to a post-v0.2 slice; v0.2 only parses
    /// and records the section so manifests with `[build]` continue to
    /// load.
    #[serde(default)]
    pub build: Option<BuildConfig>,
    /// v0.36 Track T2 — first-class FFI surface. Each `[[extern_lib]]`
    /// block names a native library the Mighty linker should link
    /// against when building this package. Required for any program
    /// containing `extern c { ... }` declarations whose symbols don't
    /// resolve through the host's already-linked libc / dynamic loader.
    ///
    /// Example:
    /// ```toml
    /// [[extern_lib]]
    /// name = "winit"
    /// kind = "static"
    /// path = "vendor/libwinit.a"
    /// link_args_macos = ["-framework", "Cocoa"]
    /// link_args_linux = ["-lX11", "-lxkbcommon"]
    /// ```
    ///
    /// See `docs/internals/extern-c-matrix.md` for the full schema and
    /// per-shape examples.
    #[serde(default, rename = "extern_lib")]
    pub extern_libs: Vec<ExternLib>,
    /// v0.19 Tier 4.1 (continued): cluster mesh configuration.
    ///
    /// Optional `[cluster]` block carrying the local node id, the TLS
    /// listen address, and the static peer list. See the runtime's
    /// `ClusterConfig` for the runtime-side shape.
    ///
    /// Example:
    ///
    /// ```toml
    /// [cluster]
    /// node_id = "node-a"
    /// listen  = "0.0.0.0:9700"
    ///
    /// [[cluster.peers]]
    /// node_id = "node-b"
    /// addr    = "10.0.0.7:9700"
    /// server_name = "node-b.cluster.local"   # optional
    /// ```
    #[serde(default)]
    pub cluster: Option<ClusterManifest>,
}

/// `[cluster]` manifest block. Parser-only — the runtime translates
/// this into a fully-formed `ClusterConfig` (with TLS bits) at boot
/// time. We deliberately do NOT put `rustls` types here so that
/// `mty-driver` and `mty-pkg` stay TLS-free.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ClusterManifest {
    /// Local node id. Falls back to `MTY_NODE_ID` if unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    /// `host:port` to listen on for inbound mesh connections.
    /// Optional — leave unset for client-only nodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen: Option<String>,
    /// Static peer list. Empty by default.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "peers")]
    pub peers: Vec<ClusterPeerManifest>,
    /// TLS configuration. Currently parsed-and-recorded only; the
    /// runtime can build a `rustls::ServerConfig` from the named
    /// PEM files at startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<ClusterTlsManifest>,
    /// v0.21 Tier 4.3: optional placement-policy block. Drives the
    /// cluster supervisor's restart hints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<ClusterPlacementManifest>,
}

/// `[cluster.placement]` block. Selects one of the bundled placement
/// policies. Custom policies are wired via Rust API only.
///
/// ```toml
/// [cluster.placement]
/// policy = "sticky"           # or "least-loaded" or "static"
/// default_node = "node-a"     # required when policy = "static"
/// ```
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ClusterPlacementManifest {
    /// Policy name: `"sticky"`, `"least-loaded"`, or `"static"`.
    #[serde(default = "default_placement_policy")]
    pub policy: String,
    /// Required only when `policy = "static"`. Ignored otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_node: Option<String>,
}

fn default_placement_policy() -> String {
    "sticky".into()
}

/// A single `[[cluster.peers]]` entry.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ClusterPeerManifest {
    pub node_id: String,
    /// Peer's `host:port` for the outbound TLS dial.
    pub addr: String,
    /// Optional SNI / server-name to validate. Defaults to `node_id`
    /// when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
}

/// `[cluster.tls]` block. Paths are filesystem-relative; the runtime
/// resolves them against the manifest's directory.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ClusterTlsManifest {
    /// Server cert chain (PEM).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_pem: Option<String>,
    /// Server private key (PEM).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_pem: Option<String>,
    /// Roots the client side will trust when dialing peers. PEM, one
    /// or more.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trusted_roots: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub edition: String,
    #[serde(default = "default_profile")]
    pub profile: String,
}

fn default_profile() -> String {
    "host".into()
}

/// Dependency specification.
///
/// Accepts either a bare version string (`foo = "0.1"`) or a detailed
/// table with any combination of `version`, `path`, `git`, `rev`, and
/// `hash`. Exactly one *source* is expected (registry-via-`version`,
/// `path`, or `git`); the resolver validates this.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Dep {
    Version(String),
    Detailed(DetailedDep),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DetailedDep {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// Pre-computed sha256 of the dependency contents (lockfile pinning).
    ///
    /// `sha256:<hex>` form. Optional in `mighty.toml`; required in
    /// `mighty.lock` for fetch-time verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

impl Dep {
    /// Return the version requirement string, if any.
    pub fn version(&self) -> Option<&str> {
        match self {
            Dep::Version(v) => Some(v),
            Dep::Detailed(d) => d.version.as_deref(),
        }
    }

    /// Return the local-path source, if any.
    pub fn path(&self) -> Option<&str> {
        match self {
            Dep::Version(_) => None,
            Dep::Detailed(d) => d.path.as_deref(),
        }
    }

    /// Return the git URL, if any.
    pub fn git(&self) -> Option<&str> {
        match self {
            Dep::Version(_) => None,
            Dep::Detailed(d) => d.git.as_deref(),
        }
    }

    /// Return the git rev (commit / tag / branch), if any.
    pub fn rev(&self) -> Option<&str> {
        match self {
            Dep::Version(_) => None,
            Dep::Detailed(d) => d.rev.as_deref(),
        }
    }

    /// Pre-computed sha256 hash, if recorded.
    pub fn hash(&self) -> Option<&str> {
        match self {
            Dep::Version(_) => None,
            Dep::Detailed(d) => d.hash.as_deref(),
        }
    }

    /// Construct a simple registry-version dependency.
    pub fn from_version(v: impl Into<String>) -> Self {
        Dep::Version(v.into())
    }

    /// Classify which source this dependency draws from.
    pub fn source_kind(&self) -> DepSourceKind {
        match self {
            Dep::Version(_) => DepSourceKind::Registry,
            Dep::Detailed(d) => {
                if d.path.is_some() {
                    DepSourceKind::Path
                } else if d.git.is_some() {
                    DepSourceKind::Git
                } else {
                    DepSourceKind::Registry
                }
            }
        }
    }
}

/// Which kind of fetcher should handle this dep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepSourceKind {
    Registry,
    Path,
    Git,
}

/// Build-script sandbox configuration scaffold (spec §5.4).
///
/// Lists the network domains and filesystem paths a `build.mty` script
/// is allowed to access. v0.2 only parses + records; enforcement is
/// deferred to a later slice. Documented in
/// `docs/internals/package-manager.md`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BuildConfig {
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub allow_net: Vec<String>,
    #[serde(default)]
    pub allow_fs: Vec<String>,
}

/// v0.36 Track T2 — declarative shape of a `[[extern_lib]]` entry.
///
/// Each entry tells the linker to pull in a native library when the
/// final executable is produced. Both static archives (`.a` / `.lib`)
/// and dynamic libraries (`.so` / `.dylib` / `.dll`) are supported via
/// the `kind` field.
///
/// Two ways to identify the library:
///
/// * `path` — explicit filesystem path (relative to the manifest dir).
///   Bypasses the linker's search path; perfect for vendored archives.
/// * neither — fall back to `-l<name>` (or the MSVC equivalent), letting
///   the linker find the library on the system search path.
///
/// `link_args` lets callers append raw linker flags (e.g.
/// `-framework Cocoa` on macOS, `Userenv.lib` on Windows). The per-OS
/// variants (`link_args_linux`, `link_args_macos`, `link_args_windows`)
/// are filtered by `cfg(target_os)` at build time so the manifest can
/// declare every platform at once.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ExternLib {
    /// Logical library name. Used as the `-l<name>` argument when no
    /// explicit `path` is given, and surfaced in error messages.
    pub name: String,

    /// `"static"` (the default) or `"dynamic"`. Static archives are
    /// pulled in whole; dynamic libraries record a runtime dependency.
    #[serde(default = "default_extern_kind")]
    pub kind: String,

    /// Optional filesystem path to the library. Resolved relative to
    /// the manifest's directory at build time. When `None`, the linker
    /// searches its default library path for `-l<name>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// Cross-platform raw linker flags (always applied).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub link_args: Vec<String>,

    /// Linker flags applied only on Linux hosts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub link_args_linux: Vec<String>,

    /// Linker flags applied only on macOS hosts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub link_args_macos: Vec<String>,

    /// Linker flags applied only on Windows hosts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub link_args_windows: Vec<String>,
}

fn default_extern_kind() -> String {
    "static".into()
}

/// Host operating-system tag for filtering per-platform link_args.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOs {
    Linux,
    Macos,
    Windows,
    Other,
}

impl HostOs {
    /// Detect the current host. Used by the build driver when filtering
    /// `link_args_*` entries from an `[[extern_lib]]` block.
    pub fn current() -> Self {
        if cfg!(target_os = "linux") {
            HostOs::Linux
        } else if cfg!(target_os = "macos") {
            HostOs::Macos
        } else if cfg!(target_os = "windows") {
            HostOs::Windows
        } else {
            HostOs::Other
        }
    }
}

impl ExternLib {
    /// `true` when this entry declares a static archive (the default).
    pub fn is_static(&self) -> bool {
        // Be lenient about case so manifests using "Static" or
        // "STATIC" still work. Any string other than "dynamic"
        // (case-insensitive) is treated as static so a typo surfaces
        // as a link error against a well-known archive name, not as
        // a silent dlopen.
        !self.kind.eq_ignore_ascii_case("dynamic")
    }

    /// `true` when this entry declares a dynamic library.
    pub fn is_dynamic(&self) -> bool {
        self.kind.eq_ignore_ascii_case("dynamic")
    }

    /// All linker arguments contributed by this entry on the given host.
    /// The output is the concatenation of `link_args` (always) plus the
    /// host-specific variant.
    pub fn resolved_link_args(&self, host: HostOs) -> Vec<String> {
        let mut out = self.link_args.clone();
        match host {
            HostOs::Linux => out.extend(self.link_args_linux.iter().cloned()),
            HostOs::Macos => out.extend(self.link_args_macos.iter().cloned()),
            HostOs::Windows => out.extend(self.link_args_windows.iter().cloned()),
            HostOs::Other => {}
        }
        out
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("read error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
}

pub fn load(path: &std::path::Path) -> Result<Manifest, ManifestError> {
    let src = std::fs::read_to_string(path)?;
    let m: Manifest = toml::from_str(&src)?;
    Ok(m)
}

/// Serialize a `Manifest` back to TOML.
///
/// Used by `mty pkg add` / `mty pkg remove` to round-trip the
/// manifest without losing fields. Comments and key ordering are *not*
/// preserved — this is a coarse-grained MVP rewrite.
pub fn save(m: &Manifest, path: &std::path::Path) -> Result<(), ManifestError> {
    let text = toml::to_string_pretty(m)?;
    std::fs::write(path, text)?;
    Ok(())
}
