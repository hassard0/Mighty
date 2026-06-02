//! v0.46 T1 — drift gates for the runtime ABI header artifact.
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

use mty_runtime::abi_export::{RUNTIME_ABI_HEADER, RUNTIME_ABI_SIGNATURES, RUNTIME_ABI_VERSION};

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
