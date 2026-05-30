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
//!      enabled on at least one native platform. v0.39 T4 state:
//!      linux-x86_64 (cargo-pgo + BOLT), windows-x86_64 (build-pgo.ps1),
//!      darwin-arm64 (cargo-pgo on toolchain 1.96.0 retry). The
//!      assertion now requires ≥ 2 PGO platforms — linux + windows
//!      are the always-on baseline; darwin is the optional 3rd.
//!   4. The release workflow's `use_bolt: true` matrix must run a
//!      `cargo pgo bolt build` step. BOLT layout shipped in v0.39 T4
//!      on linux-x86_64 (the only ELF platform in the matrix; bolt
//!      PE/COFF + Mach-O support is too rough to ship).
//!   5. v0.40 T1: BOLT steps must use `--profile release-pgo-bolt`
//!      (not `release-pgo`) so the linker doesn't try to combine
//!      `--strip-all` (from release-pgo's `strip = true`) with
//!      `--emit-relocs` (which BOLT needs). The dedicated profile
//!      sets `strip = "none"` so the relocs survive. A v0.39.0
//!      regression that bit prod ships forward as a structural pin.

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
    // v0.38.3: 2 PGO platforms (linux-x86_64 via cargo-pgo +
    // windows-x86_64 via build-pgo.ps1).
    // v0.39 T4: 3 PGO platforms — darwin-arm64 retry on toolchain
    // 1.96.0. Baseline assertion: ≥ 2 native PGO platforms (linux +
    // windows always; darwin is the optional 3rd that may flip off
    // again if the toolchain bump doesn't fix the within-channel
    // raw=8/expected=10 mismatch). We don't pin the count at 3 so
    // the darwin retry can be reverted to use_pgo: false without
    // touching this test.
    let pgo_true_count = yml.matches("use_pgo: true").count();
    assert!(
        pgo_true_count >= 2,
        "release.yml should have `use_pgo: true` on at least 2 matrix \
         entries (linux-x86_64 + windows-x86_64 as the v0.39 T4 \
         baseline; darwin-arm64 may add a 3rd). Found {pgo_true_count}"
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
fn release_workflow_includes_bolt_step_on_pgo_platforms() {
    let yml = read_text(".github/workflows/release.yml");
    // v0.39 T4: BOLT layout optimisation runs on top of PGO for the
    // platforms where llvm-bolt is mature. The matrix entry has
    // `use_bolt: true` and the steps include the cargo-pgo BOLT
    // subcommands. Asserting both halves so a future edit that
    // removes one but not the other fails fast.
    let bolt_true_count = yml.matches("use_bolt: true").count();
    assert!(
        bolt_true_count >= 1,
        "release.yml should have `use_bolt: true` on at least 1 matrix \
         entry (linux-x86_64 as the v0.39 T4 baseline). Found {bolt_true_count}"
    );

    // The cargo-pgo BOLT subcommands must be present. cargo-pgo's
    // BOLT pipeline is: `cargo pgo bolt build` (instrument) → run
    // training corpus → `cargo pgo bolt optimize` (re-layout). Pin
    // both calls so a partial revert doesn't ship a half-BOLT build.
    assert!(
        yml.contains("cargo pgo bolt build"),
        "release.yml must run `cargo pgo bolt build` to instrument \
         the PGO-optimised binary for BOLT layout collection."
    );
    assert!(
        yml.contains("cargo pgo bolt optimize"),
        "release.yml must run `cargo pgo bolt optimize` to apply the \
         collected BOLT layout to the final binary."
    );
    // And the install step (llvm-bolt isn't on the runner by default).
    assert!(
        yml.contains("llvm-bolt"),
        "release.yml must install llvm-bolt on the BOLT legs (apt-get \
         install llvm-bolt on ubuntu)."
    );
}

#[test]
fn release_workflow_per_matrix_toolchain_overrides_workspace_pin() {
    let yml = read_text(".github/workflows/release.yml");
    // v0.39 T4: per-matrix `toolchain` field lets darwin-arm64 retry
    // PGO on a newer rust channel without touching the workspace's
    // rust-toolchain.toml pin. Two structural pins:
    //
    //   1. The dtolnay/rust-toolchain step uses ${{ matrix.toolchain }}
    //      (not a hard-coded "1.95.0" literal).
    //   2. RUSTUP_TOOLCHAIN is exported into GITHUB_ENV so cargo's
    //      rust-toolchain.toml resolution is overridden for the rest
    //      of the job.
    //
    // Without (2), the dtolnay action would install 1.96.0 on the
    // darwin leg but `cargo +1.95.0` (or unprefixed `cargo` honouring
    // rust-toolchain.toml) would still get 1.95.0, defeating the retry.
    assert!(
        yml.contains("toolchain: ${{ matrix.toolchain }}"),
        "release.yml's Install Rust toolchain step must read from \
         matrix.toolchain so darwin-arm64 can pin a different channel \
         than the workspace default."
    );
    assert!(
        yml.contains("RUSTUP_TOOLCHAIN=${{ matrix.toolchain }}"),
        "release.yml must export RUSTUP_TOOLCHAIN=${{{{ matrix.toolchain }}}} \
         into GITHUB_ENV to override the workspace rust-toolchain.toml pin."
    );
    // Sanity: at least one matrix entry must request the 1.96.0 retry.
    // If a future revert drops the darwin retry entirely, this test
    // should fail so the call-site comment + docs stay accurate.
    assert!(
        yml.contains("toolchain: \"1.96.0\""),
        "release.yml should have at least one matrix entry on \
         toolchain 1.96.0 (the v0.39 T4 darwin-arm64 PGO retry)."
    );
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

#[test]
fn release_workflow_uses_release_pgo_bolt_profile_when_bolting() {
    let yml = read_text(".github/workflows/release.yml");
    // v0.40 T1: BOLT steps MUST invoke `--profile release-pgo-bolt`
    // (not `release-pgo`). The release-pgo profile sets `strip = true`
    // which lowers to `--strip-all` at link time and is incompatible
    // with the `--emit-relocs` that `cargo pgo bolt build` injects for
    // BOLT instrumentation. v0.39.0 hit:
    //   rust-lld: error: --strip-all and --emit-relocs may not be used together
    // The dedicated release-pgo-bolt profile inherits everything else
    // and only overrides `strip = "none"`.
    //
    // Scan the BOLT-related steps (lines mentioning `cargo pgo bolt`)
    // and check each one passes the new profile flag.
    let mut in_bolt_step = false;
    let mut bolt_profile_lines = 0usize;
    let mut wrong_profile_lines: Vec<String> = Vec::new();
    for line in yml.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("cargo pgo bolt ") {
            in_bolt_step = true;
        }
        if in_bolt_step {
            if line.contains("--profile release-pgo-bolt") {
                bolt_profile_lines += 1;
                in_bolt_step = false;
            } else if line.contains("--profile release-pgo")
                && !line.contains("--profile release-pgo-bolt")
            {
                wrong_profile_lines.push(line.to_string());
                in_bolt_step = false;
            }
            // Stop the lookahead when we hit a blank line so we don't
            // accidentally pick up a `--profile release-pgo` from a
            // later, non-BOLT step.
            if trimmed.is_empty() {
                in_bolt_step = false;
            }
        }
    }
    assert!(
        wrong_profile_lines.is_empty(),
        "release.yml BOLT steps must pass `--profile release-pgo-bolt`, \
         not `--profile release-pgo` (strip-all/emit-relocs conflict). \
         Offending lines: {wrong_profile_lines:?}"
    );
    assert!(
        bolt_profile_lines >= 2,
        "release.yml must invoke `cargo pgo bolt build` AND `cargo pgo \
         bolt optimize` with `--profile release-pgo-bolt`. Found only \
         {bolt_profile_lines} such invocation(s)."
    );
}

#[test]
fn release_pgo_bolt_profile_disables_strip() {
    let toml = read_text("Cargo.toml");
    // v0.40 T1: the `[profile.release-pgo-bolt]` section must exist
    // and must set `strip = "none"`. This is the structural fix for
    // the v0.39.0 BOLT failure — without `strip = "none"`, the linker
    // emits `--strip-all` which conflicts with `--emit-relocs` that
    // cargo-pgo's BOLT step injects.
    let start = toml.find("[profile.release-pgo-bolt]").unwrap_or_else(|| {
        panic!(
            "[profile.release-pgo-bolt] missing from workspace Cargo.toml. \
             v0.40 T1 added it to fix the BOLT strip-all/emit-relocs conflict."
        )
    });
    // Slice from the section marker to the start of the next section
    // (or EOF) so we only inspect this profile's body.
    let rest = &toml[start..];
    let body_end = rest[1..].find("\n[").map_or(rest.len(), |i| i + 1);
    let body = &rest[..body_end];

    assert!(
        body.contains("inherits = \"release-pgo\""),
        "release-pgo-bolt must inherit from release-pgo so it picks up \
         fat LTO + codegen-units=1. Got:\n{body}"
    );
    assert!(
        body.contains("strip = \"none\""),
        "release-pgo-bolt MUST set `strip = \"none\"` to avoid the \
         strip-all/emit-relocs linker conflict that bricked v0.39.0. \
         Got:\n{body}"
    );
    // And the parent release-pgo profile must NOT have been changed
    // to strip = "none" itself — non-BOLT PGO legs (windows-x86_64,
    // darwin-arm64) still want the strip = true behaviour.
    let parent_start = toml
        .find("[profile.release-pgo]")
        .expect("[profile.release-pgo] missing from workspace Cargo.toml");
    let parent_rest = &toml[parent_start..];
    let parent_body_end = parent_rest[1..]
        .find("\n[")
        .map_or(parent_rest.len(), |i| i + 1);
    let parent_body = &parent_rest[..parent_body_end];
    assert!(
        parent_body.contains("strip = true"),
        "release-pgo profile must keep `strip = true` so non-BOLT PGO \
         legs (windows-x86_64, darwin-arm64) still drop debug bloat. \
         Got:\n{parent_body}"
    );
}
