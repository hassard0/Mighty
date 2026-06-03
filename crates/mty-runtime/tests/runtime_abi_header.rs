//! v0.46 T1 — drift gates for the runtime ABI header artifact.
//! v0.47 T3 — extended with `@since` / numeric-macro / deprecation
//! gates, plus an optional clang compat probe for the new
//! `MTY_RUNTIME_ABI_VERSION_MINOR` macro.
//!
//! These integration tests are the public face of L51's stability
//! promise: the canonical C header
//! (`crates/mty-runtime/include/mty_runtime_abi.h`) must always be in
//! sync with the `#[no_mangle]` surface in `src/codegen_abi.rs`.
//!
//! If a swarm agent adds (or removes) a `mty_runtime_*` extern fn
//! without re-running `cargo build -p mty-runtime` (which regenerates
//! the in-tree header via `build.rs`), `header_matches_build_output`
//! fails loudly so the divergence is caught before it ships to
//! downstream consumers.
//!
//! v0.47 T3 also requires every new `#[no_mangle]` fn to carry a
//! `// @since X.Y.Z` doc comment above its attribute. The
//! `every_no_mangle_fn_has_since_tag` test below enforces that —
//! see `crates/mty-runtime/build.rs` for the parser and
//! `docs/internals/runtime-abi.md` for the consumer side.

use mty_runtime::abi_export::{
    RUNTIME_ABI_HEADER, RUNTIME_ABI_SIGNATURES, RUNTIME_ABI_STABILITY, RUNTIME_ABI_VERSION,
    RUNTIME_ABI_VERSION_MAJOR, RUNTIME_ABI_VERSION_MINOR, RUNTIME_ABI_VERSION_NUMBER,
    RUNTIME_ABI_VERSION_PATCH,
};

#[test]
fn header_matches_build_output() {
    // The check-in path is the path agents browsing the repo see; the
    // build-pinned bytes are what the CLI / consumers actually use.
    // They must match — otherwise a stale check-in could mislead
    // someone reading the source.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("include")
        .join("mty_runtime_abi.h");
    let on_disk =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    assert_eq!(
        on_disk, RUNTIME_ABI_HEADER,
        "in-tree header drift — regenerate with `cargo build -p mty-runtime`"
    );
}

#[test]
fn every_no_mangle_fn_is_in_signature_table() {
    // Cheap text scan against `codegen_abi.rs` — every
    // `#[no_mangle]`-decorated `pub extern \"C\" fn mty_runtime_*`
    // must appear in `RUNTIME_ABI_SIGNATURES`. The build.rs parser
    // is the source of truth; this test catches any future parser
    // regression that would silently drop entries.
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("codegen_abi.rs"),
    )
    .expect("read codegen_abi.rs");

    let mut found = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "#[no_mangle]" {
            // Look ahead for the `pub extern "C" fn <name>` line.
            let mut j = i + 1;
            while j < lines.len() {
                let l = lines[j].trim_start();
                if let Some(rest) = l.strip_prefix("pub extern \"C\" fn ") {
                    if let Some(lp) = rest.find('(') {
                        let name = rest[..lp].trim().to_string();
                        if name.starts_with("mty_runtime_") {
                            found.push(name);
                        }
                    }
                    break;
                }
                j += 1;
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }

    let registered: std::collections::HashSet<_> = RUNTIME_ABI_SIGNATURES
        .iter()
        .map(|s| s.name.to_string())
        .collect();
    let missing: Vec<_> = found.iter().filter(|n| !registered.contains(*n)).collect();
    assert!(
        missing.is_empty(),
        "the build.rs parser missed these no_mangle fns: {missing:?}"
    );
}

#[test]
fn every_no_mangle_fn_has_since_tag() {
    // v0.47 T3 — drift gate. Every entry in `RUNTIME_ABI_SIGNATURES`
    // must carry a `since: Some(...)` field, which means the
    // `codegen_abi.rs` `#[no_mangle]` it came from has a
    // `// @since X.Y.Z` doc comment above the attribute.
    //
    // If you're hitting this test, add a comment like:
    //
    //   // @since 0.47.0
    //   #[no_mangle]
    //   pub extern "C" fn mty_runtime_your_new_thing(...) { ... }
    //
    // to `crates/mty-runtime/src/codegen_abi.rs`, then re-run
    // `cargo build -p mty-runtime` so the generated header picks it
    // up.
    let missing: Vec<&str> = RUNTIME_ABI_SIGNATURES
        .iter()
        .filter(|s| s.since.is_none())
        .map(|s| s.name)
        .collect();
    assert!(
        missing.is_empty(),
        "the following no_mangle fns lack a `// @since X.Y.Z` doc comment: {missing:?}\n\
         add one above the `#[no_mangle]` attribute in codegen_abi.rs"
    );
}

#[test]
fn since_tags_look_like_semver() {
    // Quick sanity — every `@since` value must look like `N.N.N`.
    // The build.rs parser doesn't validate, so this test catches a
    // typo in the source comment before it ships to consumers.
    for sig in RUNTIME_ABI_SIGNATURES {
        let Some(since) = sig.since else { continue };
        let parts: Vec<&str> = since.split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "`@since` for `{}` should look like `N.N.N`, got `{since}`",
            sig.name
        );
        for p in &parts {
            assert!(
                p.chars().all(|c| c.is_ascii_digit()),
                "`@since` for `{}` has non-numeric component `{p}` (full: `{since}`)",
                sig.name
            );
        }
    }
}

#[test]
fn numeric_version_macros_match_string() {
    // The numeric macros come from splitting the version string on
    // '.'. Verify both ends agree.
    let s = format!(
        "{}.{}.{}",
        RUNTIME_ABI_VERSION_MAJOR, RUNTIME_ABI_VERSION_MINOR, RUNTIME_ABI_VERSION_PATCH
    );
    // The version string may have a pre-release suffix on dev
    // builds (e.g. `"0.47.0-rc1"`); strip that for the equality.
    let canonical = RUNTIME_ABI_VERSION
        .split(['-', '+'])
        .next()
        .unwrap_or(RUNTIME_ABI_VERSION);
    assert_eq!(
        s, canonical,
        "numeric MAJOR.MINOR.PATCH macros disagree with RUNTIME_ABI_VERSION"
    );

    // The combined NUMBER macro packs the components as
    // MAJOR*10000 + MINOR*100 + PATCH (so e.g. 0.47.0 -> 4700), the
    // form the header documents for `#if ... >= 4700` compat checks.
    let expected_number = RUNTIME_ABI_VERSION_MAJOR * 10000
        + RUNTIME_ABI_VERSION_MINOR * 100
        + RUNTIME_ABI_VERSION_PATCH;
    assert_eq!(
        RUNTIME_ABI_VERSION_NUMBER, expected_number,
        "RUNTIME_ABI_VERSION_NUMBER must equal MAJOR*10000 + MINOR*100 + PATCH"
    );
    assert!(
        RUNTIME_ABI_HEADER.contains(&format!(
            "#define MTY_RUNTIME_ABI_VERSION_NUMBER {expected_number}"
        )),
        "header must emit the MTY_RUNTIME_ABI_VERSION_NUMBER macro matching the Rust const"
    );

    // Pre-1.0 the whole ABI surface is experimental; the Rust const and
    // the C macro must advertise the same tier.
    assert_eq!(
        RUNTIME_ABI_STABILITY, "experimental",
        "pre-1.0 runtime ABI must advertise the `experimental` stability tier"
    );
    assert!(
        RUNTIME_ABI_HEADER.contains(&format!(
            "#define MTY_RUNTIME_ABI_STABILITY \"{RUNTIME_ABI_STABILITY}\""
        )),
        "header must emit the MTY_RUNTIME_ABI_STABILITY macro matching the Rust const"
    );
}

#[test]
fn header_compiles_under_clang_when_available() {
    // Optional — only runs if `clang` is on PATH. We don't want to
    // gate CI on a clang install (most GHA runners have it though).
    let Some(clang) = which_first(&["clang", "clang.exe"]) else {
        eprintln!("skipping clang fsyntax test — no clang on PATH");
        return;
    };

    // `clang -fsyntax-only -x c -` reads the header bytes from stdin
    // and just runs the parser/typecheck. No assembler / linker
    // round-trip means no PIC/PIE complications across platforms.
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(&clang)
        .args(["-fsyntax-only", "-x", "c", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn clang");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(RUNTIME_ABI_HEADER.as_bytes())
        .expect("pipe header to clang stdin");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("clang wait");
    assert!(
        output.status.success(),
        "clang -fsyntax-only rejected the generated header:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn header_minor_macro_accepts_minimum_compat_check() {
    // v0.47 T3 — verify a downstream consumer can write
    //   #if MTY_RUNTIME_ABI_VERSION_MINOR >= <released-minor>
    // …and the header still compiles. This is the consumer-side
    // affordance the numeric macros enable; we ship the header in
    // every release and want to catch a regression that would break
    // soft-pinning. Uses a temp file + `clang -include` so the test
    // program is plain C with no on-disk header path.
    let Some(clang) = which_first(&["clang", "clang.exe"]) else {
        eprintln!("skipping minor-macro compat test — no clang on PATH");
        return;
    };

    use std::io::Write;
    use std::process::{Command, Stdio};

    let tmpdir = std::env::temp_dir().join(format!("mty-abi-compat-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmpdir);
    let header_path = tmpdir.join("mty_runtime_abi.h");
    std::fs::write(&header_path, RUNTIME_ABI_HEADER).expect("write tmp header");

    let test_c = format!(
        "#include \"{}\"\n\
         #if !(MTY_RUNTIME_ABI_VERSION_MAJOR == {} && MTY_RUNTIME_ABI_VERSION_MINOR >= {})\n\
         #error \"runtime ABI is older than caller expects\"\n\
         #endif\n\
         int main(void) {{ return 0; }}\n",
        header_path.display().to_string().replace('\\', "/"),
        RUNTIME_ABI_VERSION_MAJOR,
        RUNTIME_ABI_VERSION_MINOR
    );

    let mut child = Command::new(&clang)
        .args(["-fsyntax-only", "-x", "c", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn clang");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(test_c.as_bytes())
        .expect("pipe test C to clang");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("clang wait");
    let _ = std::fs::remove_dir_all(&tmpdir);
    assert!(
        output.status.success(),
        "clang rejected the compat-check program — numeric version macros are broken?\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn version_constant_is_non_empty() {
    assert!(
        !RUNTIME_ABI_VERSION.is_empty(),
        "RUNTIME_ABI_VERSION must not be empty"
    );
}

fn which_first(candidates: &[&str]) -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for candidate in candidates {
            let full = dir.join(candidate);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}
