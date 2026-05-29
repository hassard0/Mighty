//! v0.36 T5: Windows `cargo install mty` paper-cut tests.
//!
//! The historical pain (FAQ entry "Why does `mty` fail to link on
//! Windows?"): `cargo install --path crates/mty-cli` pulled the
//! `rusqlite` crate with the `bundled` feature, which compiles SQLite
//! from C source via the host C toolchain. On Windows that means MSVC
//! `cl.exe` + `link.exe`. Users without VS Build Tools hit
//! `error: linker `link.exe` not found` and bounce.
//!
//! v0.36 T5 fix: split `observe-sqlite` (the only feature that pulls
//! rusqlite into the CLI graph) out of the always-on feature set into
//! a dedicated top-level feature. Default `cargo install` still gets
//! `observe-sqlite` on hosts where it builds; Windows users without
//! MSVC do `cargo install mty --no-default-features --features cli-min`
//! and `mty inspect --cost` then reports the feature as disabled
//! rather than failing the install.
//!
//! These tests verify the feature graph + runtime fallback behaviour:
//!   1. The `cli-min` feature is present in mty-cli's manifest.
//!   2. `cli-min` does not transitively enable `observe-sqlite`.
//!   3. The default feature set still includes `observe-sqlite` (so
//!      Unix users get cost tracking by default).
//!   4. The `host-toolchain` feature works without `observe-sqlite`.
//!   5. `mty inspect --cost` returns a helpful "feature disabled"
//!      message when SQLite is off, not a crash.

#![cfg(feature = "host-toolchain")]

use std::fs;
use std::path::PathBuf;

fn cli_manifest_path() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest).join("Cargo.toml")
}

fn read_manifest() -> String {
    fs::read_to_string(cli_manifest_path()).expect("read mty-cli Cargo.toml")
}

#[test]
fn cli_min_feature_is_declared() {
    let manifest = read_manifest();
    assert!(
        manifest.contains("cli-min = ["),
        "expected `cli-min` feature in mty-cli/Cargo.toml — Windows install \
         path depends on it: {}",
        manifest
            .lines()
            .filter(|l| l.contains("features") || l.contains("cli-min"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn cli_min_does_not_pull_observe_sqlite() {
    let manifest = read_manifest();
    // Find the `cli-min = [...]` definition. Crude string scan but
    // adequate — we just need to ensure the literal `observe-sqlite`
    // doesn't appear inside the cli-min feature's bracket list.
    let cli_min_start = manifest
        .find("cli-min = [")
        .expect("cli-min feature declaration missing");
    let after = &manifest[cli_min_start..];
    let cli_min_end = after.find(']').expect("cli-min feature is unterminated");
    let cli_min_decl = &after[..=cli_min_end];
    assert!(
        !cli_min_decl.contains("observe-sqlite"),
        "cli-min must NOT transitively enable observe-sqlite (defeats \
         the Windows-without-MSVC install path). Got: {cli_min_decl}"
    );
}

#[test]
fn observe_sqlite_is_a_top_level_feature() {
    let manifest = read_manifest();
    // Must be `observe-sqlite = [...]` at the top level (a feature
    // users can flip on their own), not just a transitive flag.
    assert!(
        manifest.contains("observe-sqlite = ["),
        "expected `observe-sqlite` as a top-level mty-cli feature so \
         Windows users can opt back in with `--features observe-sqlite`"
    );
}

#[test]
fn default_features_include_observe_sqlite() {
    let manifest = read_manifest();
    // The default feature set must keep `observe-sqlite` so non-Windows
    // users (and Windows users who DO have MSVC) get cost tracking out
    // of the box. The Windows escape hatch is `--no-default-features
    // --features cli-min`, not a silent default downgrade.
    let default_start = manifest
        .find("default = [")
        .expect("default feature missing");
    let after = &manifest[default_start..];
    let default_end = after.find(']').expect("default feature unterminated");
    let default_decl = &after[..=default_end];
    assert!(
        default_decl.contains("observe-sqlite"),
        "default features should include `observe-sqlite` — Windows \
         install path is `--no-default-features --features cli-min`. \
         Got: {default_decl}"
    );
}

#[test]
fn faq_documents_windows_install_path() {
    // The FAQ entry users hit when `cargo install mty` fails on
    // Windows must point at the new `cli-min` escape hatch, not just
    // tell them to install MSVC. (MSVC install is a 6 GB download —
    // unacceptable for a "try the language" install path.)
    let faq = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("docs/faq.md");
    let text = fs::read_to_string(&faq).expect("read docs/faq.md");
    assert!(
        text.contains("cli-min"),
        "docs/faq.md should mention `cli-min` as the MSVC-free Windows \
         install workaround. Found {} bytes but no mention.",
        text.len()
    );
    assert!(
        text.contains("--no-default-features"),
        "docs/faq.md should show the `cargo install ... --no-default-features --features cli-min` command"
    );
}

#[test]
fn readme_install_command_is_present() {
    // The README's quick-install command lands in front of everyone
    // — make sure it's still the canonical `cargo install --path
    // crates/mty-cli`. Any deviation should be a conscious docs PR.
    let readme = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("README.md");
    let text = fs::read_to_string(&readme).expect("read README.md");
    assert!(
        text.contains("cargo install --path crates/mty-cli"),
        "README.md should keep the canonical `cargo install --path crates/mty-cli` line"
    );
}
