use mty_driver::{
    discover_package_sources, find_manifest_root, jit_run, parse_source, run_file,
    run_file_with_runtime, ParsedFile,
};
use std::fs;
use std::path::Path;

/// Slice-8 default: try JIT first (Cranelift); fall back to the
/// slice-7 runtime (tokio + interpreter per-turn) on
/// `CodegenError::Unsupported`. With `--legacy-interp`, skip JIT and
/// use the slice-6 tree-walker directly.
///
/// v0.27 Track E (QoL #3): `argv` is the trailing positional tail
/// the user passed after `--`. We stash it into the process-wide
/// `mty_stdlib::env::ARGS` channel before invoking the runtime so
/// `std.env.args()` calls inside the Mighty program resolve to it.
///
/// v0.41 T2: if `path` lives inside a Mighty package (a `mighty.toml`
/// is reachable by walking parents), the source text we hand to the
/// JIT / runtime is the concatenation of every `src/**/*.mty` module
/// in the package plus the target file itself. This mirrors the
/// package-aware lift `mty test` and `mty check` do, so a script that
/// calls into sibling modules resolves against the same world the
/// test runner uses. The Cranelift JIT re-parses the concatenated
/// source as one file; byte positions in any backend diagnostic shift
/// relative to the original source, but for the no-package case
/// (standalone scripts) the function returns the original source
/// unchanged so source-location fidelity is preserved there.
pub fn run(path: &Path, legacy: bool, argv: Vec<String>) -> i32 {
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read {}: {}", path.display(), e);
            return 1;
        }
    };
    // Forward the `--` tail to `std.env.args()` before the runtime
    // boots. Calling `set_args` with an empty `Vec` is the no-op
    // default that matches Mighty programs run without trailing args.
    mty_stdlib::env::set_args(argv);
    let id = path.display().to_string();
    // v0.41 T2 — package-aware lift. Only kicks in when the target
    // file is anchored to a `mighty.toml` AND has at least one sibling
    // `src/**.mty` module. Without sibling modules there's nothing
    // to fold in and the standalone path keeps its source-location
    // fidelity.
    let (src, id) = lift_package_sources(path, src, id);
    if legacy {
        return run_file(src, id);
    }
    // Slice-8 fast path: try JIT.
    match jit_run(src.clone(), id.clone()) {
        Ok(Some(code)) => code,
        Ok(None) => {
            // Codegen reported Unsupported — fall back to runtime+interp.
            run_file_with_runtime(src, id)
        }
        Err(code) => code,
    }
}

/// v0.41 T2 — fold every `src/**/*.mty` of the surrounding Mighty
/// package onto the source text + id of `path` so the JIT / runtime
/// sees the package's module-level namespace. Files are concatenated
/// in lexicographic order with the target file last (so its `fn main`
/// resolves to the only `main` in the merged surface). Returns the
/// inputs unmodified when no package context is found OR when the
/// only file is the target itself (the standalone-script shape).
fn lift_package_sources(path: &Path, src: String, id: String) -> (String, String) {
    let Some(manifest_dir) = find_manifest_root(path) else {
        return (src, id);
    };
    let canon_target = path.canonicalize().ok();
    let sibling_files: Vec<_> = discover_package_sources(&manifest_dir)
        .into_iter()
        .filter(|p| match (&canon_target, p.canonicalize().ok()) {
            (Some(a), Some(b)) => *a != b,
            _ => true,
        })
        .collect();
    if sibling_files.is_empty() {
        return (src, id);
    }
    // Each sibling source is appended with a separating newline so
    // adjacent EOFs / top-level items don't fuse. The parser is
    // file-position agnostic for HIR resolution (every top-level fn
    // ends up keyed by simple name), so any byte-position drift only
    // shifts span ranges, not lookup correctness.
    //
    // We also pre-parse each sibling to discard files that fail the
    // lexer — adding an unparseable extra file to `mty run` would
    // turn the target script's clean run into a parse error from a
    // sibling, which would surprise the user. Treat unparseable
    // siblings as if they didn't exist.
    let mut concat = String::new();
    for sib in &sibling_files {
        if let Ok(s) = std::fs::read_to_string(sib) {
            let parsed: ParsedFile = parse_source(s.clone(), sib.display().to_string());
            if parsed
                .diagnostics
                .iter()
                .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
            {
                continue;
            }
            concat.push_str(&s);
            if !concat.ends_with('\n') {
                concat.push('\n');
            }
        }
    }
    concat.push_str(&src);
    (concat, id)
}
