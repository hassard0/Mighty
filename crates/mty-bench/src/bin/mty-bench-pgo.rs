//! `mty-bench-pgo` — wrapper binary used by `scripts/build-pgo.{sh,ps1}`
//! during the profile-collection phase of the v0.22 PGO pipeline.
//!
//! Why a dedicated binary?
//!
//! The profile collection phase needs to drive *representative*
//! workloads through the instrumented `mty` binary so the resulting
//! `merged.profdata` reflects how the optimiser should lay out the
//! hot paths. The existing `mty-bench-runner` mixes in criterion-style
//! sampling + JSON emission which we don't need for PGO. This binary
//! is a thin sequencer that:
//!
//!   - parses + types + IRs the bundled examples;
//!   - runs the in-process compile sweep used by
//!     `mty_bench::run_compile`-equivalent benchmarks;
//!   - exercises the cranelift + wasm codegen paths.
//!
//! It is intentionally tiny so that the PGO collection step takes
//! tens of seconds, not minutes. Each phase is gated by a `--quick`
//! flag (the default for CI) and a `--full` flag (for local
//! measurement runs where a wider profile is worth the wall-clock).
//!
//! Usage:
//!
//! ```text
//! mty-bench-pgo                # default: --quick
//! mty-bench-pgo --quick
//! mty-bench-pgo --full
//! mty-bench-pgo --examples-dir ./examples
//! ```
//!
//! Output is a one-line summary per workload; no JSON.

use clap::Parser;
use mty_bench::fixtures::synth_source;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "mty-bench-pgo",
    version,
    about = "PGO profile-collection driver for the v0.22 build pipeline"
)]
struct Args {
    /// Fast profile collection (parse + small compile sweep). Default.
    #[arg(long, conflicts_with = "full")]
    quick: bool,
    /// Wider profile collection — adds a larger synthetic compile + a
    /// per-example parse pass. Roughly 4-5× the wall-clock of --quick.
    #[arg(long)]
    full: bool,
    /// Override the path to the bundled examples directory.
    #[arg(long, default_value = "examples")]
    examples_dir: PathBuf,
}

fn main() {
    let args = Args::parse();
    let mode = if args.full { Mode::Full } else { Mode::Quick };
    println!("mty-bench-pgo: mode={mode:?}");

    let t_start = Instant::now();

    run_parse_sweep(&args.examples_dir, mode);
    run_synth_compile(mode);

    println!(
        "mty-bench-pgo: done in {:.2}s",
        t_start.elapsed().as_secs_f64()
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Quick,
    Full,
}

/// Parse + lex every `.mty` file in the examples directory. We don't
/// bail on parse errors — the goal is to exercise lexer/parser code
/// paths, not to validate the examples.
fn run_parse_sweep(dir: &std::path::Path, mode: Mode) {
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(e) => {
            eprintln!("  parse_sweep: cannot read {}: {e}", dir.display());
            return;
        }
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("mty"))
        .collect();
    files.sort();

    let t0 = Instant::now();
    let mut bytes_total = 0usize;
    let mut count = 0usize;
    // In quick mode we cap at 12 files; full mode walks them all.
    let cap = if mode == Mode::Full {
        files.len()
    } else {
        files.len().min(12)
    };
    for f in files.iter().take(cap) {
        let Ok(src) = std::fs::read_to_string(f) else {
            continue;
        };
        bytes_total += src.len();
        let name = f
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let parsed = mty_driver::parse_source(src, name);
        std::hint::black_box(&parsed);
        count += 1;
    }
    println!(
        "  parse_sweep: {} files, {} bytes, {:.2}ms",
        count,
        bytes_total,
        t0.elapsed().as_secs_f64() * 1000.0
    );
}

/// Drive the wasm32-wasi compile pipeline against a synthetic ~50 LOC
/// (quick) / ~250 LOC (full) source. This exercises the codegen-wasm
/// + driver hot path which is what we most want PGO to optimise.
fn run_synth_compile(mode: Mode) {
    use mty_codegen_cranelift::artifact::BuildMode;
    use mty_codegen_wasm::WasmTarget;
    use mty_driver::{build_wasm, BuildOptions, BuildTarget};

    let units = if mode == Mode::Full { 25 } else { 5 };
    let src = synth_source(units);
    let tmp = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  synth_compile: tempdir failed: {e}");
            return;
        }
    };
    let opts = BuildOptions {
        target: BuildTarget::Wasm(WasmTarget::Wasi),
        mode: BuildMode::Release,
        out_dir: tmp.path().to_path_buf(),
        binary_name: "pgo_synth".to_string(),
        no_component: true,
        wasi_preview: mty_driver::build::WasiPreview::P1,
        user_wit: None,

        extern_libs: Vec::new(),

        manifest_dir: None,
    };
    let t0 = Instant::now();
    let _ = build_wasm(src, "pgo_synth.mty".into(), &opts, WasmTarget::Wasi);
    println!(
        "  synth_compile: units={units} {:.2}ms",
        t0.elapsed().as_secs_f64() * 1000.0
    );
}
