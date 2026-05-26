//! User-supplied WIT resolution for `mighty.toml` packages.
//!
//! v0.13 adds an optional `[wit]` section to the manifest:
//!
//! ```toml
//! [wit]
//! world = "my-world"
//! files = ["wit/world.wit", "wit/types.wit"]
//! ```
//!
//! When present, this module loads the listed `.wit` files, concatenates
//! them, and returns the text + selected world to the WASM codegen
//! (specifically [`mty_codegen_wasm::preview2`]) for merging into the
//! component world.
//!
//! ### Why concatenate raw text?
//!
//! The codegen pipeline already runs every world through
//! `wit_parser::Resolve::push_str` for validation. Loading user files
//! as raw text keeps `mty-pkg` decoupled from the `wit_parser` API and
//! avoids re-implementing the parser's package-merging logic — the
//! Resolve does it for us when codegen pushes the combined text. The
//! tradeoff: parse errors surface during codegen, not during
//! `mty pkg fetch`. The error messages still carry filenames because
//! we annotate each file's text with a `// file: <path>` comment
//! before concatenation.
//!
//! ### Errors
//!
//! Surface-level errors (missing manifest section, file-not-found,
//! UTF-8 issues) are returned as [`WitResolveError`]. Parse errors
//! are *not* caught here — they fire later when codegen feeds the
//! text to `wit_parser`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Optional `[wit]` section on a Mighty manifest.
///
/// We parse this from `mighty.toml` *out-of-band* (rather than adding
/// a field to `mty_driver::manifest::Manifest`) so the driver crate
/// stays free of P2-specific schema. Callers either:
///
/// 1. Use [`load_from_manifest`] which reads `mighty.toml` directly,
///    or
/// 2. Build a `WitSection` themselves and pass it to [`load_user_wit`].
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WitSection {
    /// Optional explicit world name. When `None`, the codegen picks
    /// the only world in the user's package; ambiguous packages error
    /// out at component-wrap time.
    #[serde(default)]
    pub world: Option<String>,
    /// List of `.wit` files (relative to the package root) to include
    /// in the component world.
    #[serde(default)]
    pub files: Vec<String>,
}

/// A user-WIT package, ready to hand to the codegen.
///
/// Mirrors `mty_codegen_wasm::preview2::UserWit` but lives here to
/// keep the codegen → pkg dependency one-way. The CLI wires the two
/// together at the call site.
#[derive(Debug, Clone)]
pub struct LoadedUserWit {
    /// Concatenated text of every user `.wit` file (with package
    /// declarations preserved).
    pub text: String,
    /// Optional explicit world name (`--world <name>` or
    /// `[wit] world = "..."`).
    pub world: Option<String>,
    /// Source label used in `wit_parser` diagnostics.
    pub source_label: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WitResolveError {
    #[error("io reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("no [wit] section in {0}")]
    NoWitSection(PathBuf),
    #[error("[wit] section has no `files`")]
    NoFiles,
    #[error("invalid TOML in {0}: {1}")]
    Toml(PathBuf, Box<toml::de::Error>),
}

/// Load `[wit]` from the package's `mighty.toml`, if present.
///
/// Returns `Ok(None)` when the manifest exists but has no `[wit]`
/// section — that's the common case for v0.13 packages and is *not* an
/// error.
pub fn read_section(manifest_path: &Path) -> Result<Option<WitSection>, WitResolveError> {
    let text = std::fs::read_to_string(manifest_path).map_err(|e| WitResolveError::Io {
        path: manifest_path.to_path_buf(),
        source: e,
    })?;
    // Parse the raw TOML to a `toml::Value` so we don't have to touch
    // the strongly-typed `Manifest` schema.
    let val: toml::Value = toml::from_str(&text)
        .map_err(|e| WitResolveError::Toml(manifest_path.to_path_buf(), Box::new(e)))?;
    let Some(wit_val) = val.get("wit") else {
        return Ok(None);
    };
    let section: WitSection = wit_val
        .clone()
        .try_into()
        .map_err(|e| WitResolveError::Toml(manifest_path.to_path_buf(), Box::new(e)))?;
    Ok(Some(section))
}

/// Resolve + load the user's WIT files relative to `pkg_root`.
///
/// `world_override`, when `Some`, replaces `section.world` (used by the
/// CLI's `--world` flag).
pub fn load_user_wit(
    pkg_root: &Path,
    section: &WitSection,
    world_override: Option<String>,
) -> Result<LoadedUserWit, WitResolveError> {
    if section.files.is_empty() {
        return Err(WitResolveError::NoFiles);
    }
    let mut combined = String::new();
    let mut labels = Vec::with_capacity(section.files.len());
    for rel in &section.files {
        let path = pkg_root.join(rel);
        let body = std::fs::read_to_string(&path).map_err(|e| WitResolveError::Io {
            path: path.clone(),
            source: e,
        })?;
        combined.push_str(&format!("// file: {}\n", rel));
        combined.push_str(&body);
        if !body.ends_with('\n') {
            combined.push('\n');
        }
        combined.push('\n');
        labels.push(rel.clone());
    }
    Ok(LoadedUserWit {
        text: combined,
        world: world_override.or_else(|| section.world.clone()),
        source_label: format!("user-wit({})", labels.join(",")),
    })
}

/// Convenience: read `[wit]` from `<pkg_root>/mighty.toml` and load
/// the files in one step. Returns `Ok(None)` when no `[wit]` section
/// exists (the common case).
pub fn load_from_manifest(
    pkg_root: &Path,
    world_override: Option<String>,
) -> Result<Option<LoadedUserWit>, WitResolveError> {
    let manifest_path = pkg_root.join(crate::MANIFEST_NAME);
    let Some(section) = read_section(&manifest_path)? else {
        return Ok(None);
    };
    Ok(Some(load_user_wit(pkg_root, &section, world_override)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(dir: &Path, rel: &str, body: &str) -> PathBuf {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn missing_wit_section_returns_none() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "mighty.toml",
            "[package]\nname = \"x\"\nversion = \"0.1\"\nedition = \"2025\"\n",
        );
        let r = load_from_manifest(dir.path(), None).expect("ok");
        assert!(r.is_none());
    }

    #[test]
    fn loads_files_from_section() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "mighty.toml",
            r#"
[package]
name = "x"
version = "0.1"
edition = "2025"

[wit]
world = "my-world"
files = ["wit/a.wit", "wit/b.wit"]
"#,
        );
        write(dir.path(), "wit/a.wit", "package demo:a;\n");
        write(dir.path(), "wit/b.wit", "package demo:b;\n");
        let r = load_from_manifest(dir.path(), None)
            .expect("ok")
            .expect("some");
        assert_eq!(r.world.as_deref(), Some("my-world"));
        assert!(r.text.contains("package demo:a;"));
        assert!(r.text.contains("package demo:b;"));
        assert!(r.text.contains("// file: wit/a.wit"));
    }

    #[test]
    fn world_override_wins() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "mighty.toml",
            r#"
[package]
name = "x"
version = "0.1"
edition = "2025"

[wit]
world = "default-world"
files = ["w.wit"]
"#,
        );
        write(dir.path(), "w.wit", "package demo:x;\n");
        let r = load_from_manifest(dir.path(), Some("override-world".into()))
            .expect("ok")
            .expect("some");
        assert_eq!(r.world.as_deref(), Some("override-world"));
    }

    #[test]
    fn missing_file_errors() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "mighty.toml",
            r#"
[package]
name = "x"
version = "0.1"
edition = "2025"

[wit]
files = ["wit/missing.wit"]
"#,
        );
        let r = load_from_manifest(dir.path(), None);
        assert!(matches!(r, Err(WitResolveError::Io { .. })));
    }

    #[test]
    fn empty_files_list_errors() {
        let section = WitSection {
            world: None,
            files: vec![],
        };
        let r = load_user_wit(Path::new("."), &section, None);
        assert!(matches!(r, Err(WitResolveError::NoFiles)));
    }
}
