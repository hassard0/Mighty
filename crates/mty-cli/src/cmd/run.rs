use mty_driver::{jit_run, run_file, run_file_with_runtime};
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
