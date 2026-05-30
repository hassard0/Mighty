//! v0.36 T5: smoke tests for the PGO build scripts.
//!
//! The PGO release pipeline at `scripts/build-pgo.{sh,ps1}` is the
//! highest-leverage artefact in the workspace — it ships every tagged
//! `mty` binary at peak perf. Two bugs in v0.35 silently disabled PGO
//! for two full minors (v0.35.2 + v0.35.5) before v0.36 T5 fixed
//! them, so the v0.36+ workspace pays for a CI smoke that pins the
//! fixes structurally:
//!
//!   1. Phase 0 must wipe stale `target/release-pgo/` build artifacts.
//!      Without it, instrumented Phase 1 re-uses Phase 4's prior
//!      `-Cprofile-use` codegen and the profile-format header
//!      mismatches (raw=8 vs expected=10) on macOS+Windows.
//!   2. Phase 4 must NOT pass `-Clinker-plugin-lto`. Fat LTO is
//!      already on via `[profile.release-pgo]`; the extra flag
//!      collides with PGO's `CG Profile` module metadata on
//!      linux-x86_64 (`LLVM ERROR: Broken module found, module flag
//!      identifiers must be unique !"CG Profile"`).
//!   3. The release workflow's `use_pgo: true` matrix must stay
//!      enabled on at least one native platform. v0.38.1 contingency:
//!      linux-x86_64 only (darwin-arm64 + windows-msvc PGO disabled
//!      after cargo-pgo migration surfaces; v0.39 follow-up).

#![cfg(feature = "host-toolchain")]

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    // crates/mty-cli → up two levels → workspace root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn read_text(rel: &str) -> String {
    let p = workspace_root().join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

#[test]
fn build_pgo_sh_wipes_stale_release_pgo_dir() {
    let sh = read_text("scripts/build-pgo.sh");
    // Phase 0 must wipe `target/release-pgo/{build,deps}` to force
    // fresh codegen. Otherwise the profile-format mismatch trips on
    // macOS+Windows. Crude string scan but adequate.
    assert!(
        sh.contains("target/release-pgo/build"),
        "scripts/build-pgo.sh Phase 0 must wipe target/release-pgo/build"
    );
    assert!(
        sh.contains("target/release-pgo/deps"),
        "scripts/build-pgo.sh Phase 0 must wipe target/release-pgo/deps"
    );
}

#[test]
fn build_pgo_ps1_wipes_stale_release_pgo_dir() {
    let ps = read_text("scripts/build-pgo.ps1");
    assert!(
        ps.contains("target/release-pgo/build")
            || ps.contains("target/release-pgo/$sub")
            || ps.contains("release-pgo/$sub"),
        "scripts/build-pgo.ps1 Phase 0 must wipe target/release-pgo/build (or via loop variable)"
    );
    // The wipe loop must enumerate at minimum build + deps.
    assert!(
        ps.contains("\"build\"") && ps.contains("\"deps\""),
        "scripts/build-pgo.ps1 wipe must cover at least build + deps subdirs"
    );
}

#[test]
fn build_pgo_sh_does_not_pass_linker_plugin_lto() {
    let sh = read_text("scripts/build-pgo.sh");
    // The literal flag must NOT appear in the active build command.
    // It's allowed in comments (we explicitly document why it was
    // dropped). Strip comments before the search.
    let no_comments: String = sh
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !no_comments.contains("-Clinker-plugin-lto"),
        "scripts/build-pgo.sh must NOT pass -Clinker-plugin-lto in Phase 4 \
         (it collides with PGO CG Profile on linux-x86_64). Active code: {no_comments}"
    );
}

#[test]
fn build_pgo_ps1_does_not_pass_linker_plugin_lto() {
    let ps = read_text("scripts/build-pgo.ps1");
    let no_comments: String = ps
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !no_comments.contains("-Clinker-plugin-lto"),
        "scripts/build-pgo.ps1 must NOT pass -Clinker-plugin-lto in Phase 4. \
         Active code: {no_comments}"
    );
}

#[test]
fn release_workflow_enables_pgo_on_at_least_one_native_platform() {
    let yml = read_text(".github/workflows/release.yml");
    // v0.38.1: cargo-pgo migration retained only linux-x86_64 PGO
    // after the Release-run revealed cargo-pgo doesn't fix
    // darwin-arm64's toolchain-internal raw=8/expected=10 mismatch
    // and writes no profraws on windows-msvc. Both PGO legs are
    // disabled until the v0.39 follow-up. Assertion: ≥1 PGO platform.
    let pgo_true_count = yml.matches("use_pgo: true").count();
    assert!(
        pgo_true_count >= 1,
        "release.yml should have `use_pgo: true` on at least 1 matrix \
         entry (currently linux-x86_64 only after v0.38.1 contingency). \
         Found {pgo_true_count}"
    );

    // Pin each triple still appears in the matrix (PGO state varies).
    for triple in [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(
            yml.contains(triple),
            "release.yml is missing matrix entry for {triple}"
        );
    }
}

#[test]
fn release_workflow_keeps_pgo_off_on_cross_compile_legs() {
    let yml = read_text(".github/workflows/release.yml");
    // darwin-x86_64 (rosetta) and linux-aarch64 (cross) MUST stay
    // `use_pgo: false` — the instrumented binary can't run on the
    // build host. We sanity-check by counting `use_pgo: false` entries.
    let pgo_false_count = yml.matches("use_pgo: false").count();
    assert!(
        pgo_false_count >= 2,
        "release.yml should keep `use_pgo: false` on at least 2 entries \
         (darwin-x86_64 + linux-aarch64). Found {pgo_false_count}"
    );
}

#[test]
fn release_workflow_documents_pgo_re_enable() {
    let yml = read_text(".github/workflows/release.yml");
    // The v0.36 T5 narrative must be in the workflow comments so the
    // next integrator who sees `use_pgo: false` understands why.
    assert!(
        yml.contains("v0.36 T5"),
        "release.yml should reference the v0.36 T5 PGO re-enable in matrix comments"
    );
}

#[test]
fn release_pgo_profile_inherits_fat_lto() {
    let toml = read_text("Cargo.toml");
    // The `release-pgo` profile must pin `lto = "fat"` — Phase 4's
    // bare `-Cprofile-use=...` flag relies on the profile already
    // having fat LTO turned on (we dropped `-Clinker-plugin-lto`).
    let start = toml
        .find("[profile.release-pgo]")
        .expect("[profile.release-pgo] missing from workspace Cargo.toml");
    let profile_block = &toml[start..];
    // Take ~30 lines after the marker; enough for the profile body.
    let head: String = profile_block
        .lines()
        .take(15)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        head.contains("lto = \"fat\""),
        "release-pgo profile must keep `lto = \"fat\"` (Phase 4 relies on it \
         after dropping -Clinker-plugin-lto). Got:\n{head}"
    );
}

#[test]
fn release_workflow_cache_keys_segregate_pgo() {
    let yml = read_text(".github/workflows/release.yml");
    // v0.36 T5: cache key must be different for PGO vs non-PGO. Pin
    // both discriminators are present.
    assert!(
        yml.contains("cargo-release-pgo-"),
        "release.yml cache key must include `cargo-release-pgo-` for PGO legs"
    );
    assert!(
        yml.contains("cargo-release-noPGO-"),
        "release.yml cache key must include `cargo-release-noPGO-` for non-PGO legs"
    );
}
