#![cfg(feature = "host-toolchain")]
//! v0.41 T3 — examples conformance suite.
//!
//! L1 says: `mty build` (native Cranelift) lags `mty run` (interpreter),
//! and built binaries have **no interpreter fallback**. The fix is to
//! pin native↔interpreter parity per-example: each `examples/*.mty`
//! file runs under both the interpreter (`mty run --legacy-interp`)
//! and the native JIT (`mty run`, the same Cranelift pipeline that
//! backs `mty build`). Stdout and exit code MUST match, otherwise
//! the example is a documented known-failing case for v0.41 with a
//! row in `KNOWN_FAILING` carrying the rationale.
//!
//! The harness deliberately uses the JIT (`mty run`) and not
//! `mty build && ./out`, because:
//!
//!   1. On Windows CI without `clang`/`gcc` in PATH, `mty build`
//!      emits the object file but stops short of linking (see L2/L10
//!      — the linker driver is clang-only). That makes "build + exec"
//!      unavailable on the very platform v0.41 ships first.
//!   2. The JIT goes through the **same** Cranelift lowering as
//!      `mty build`, so any codegen gap surfaces under JIT just as
//!      it would under a fully-linked native binary. The dynamic-log
//!      test (`dynamic_log.rs`) and U8-widening test
//!      (`u8_widening.rs`) follow the same model.
//!
//! When a host `clang`/`cc` IS available we additionally pin the
//! object-emission path (`mty build` exits 0 with a writeable .o).
//! That guards the L10 surface — `mty build` of a `log("hi")` program
//! should never crash at codegen time even on the Windows-no-clang
//! path that only emits the object.
//!
//! Adding a new example: drop it under `examples/`, run
//! `cargo test conformance_examples -p mty-cli` — if it diverges from
//! the interpreter, either (a) fix the divergence in
//! `crates/mty-codegen-cranelift/src/lower.rs`, or (b) add a row to
//! `KNOWN_FAILING` documenting the gap. Both paths are valid as long
//! as the gap has an issue tag for follow-up.

use std::path::{Path, PathBuf};
use std::process::Command;

/// One entry per known divergence between `mty run --legacy-interp`
/// and `mty run` (the JIT/native path). Used both for asserting "the
/// floor doesn't regress" and for documenting v0.42 follow-up work.
///
/// Categories:
///   * `LibraryOnly`: the example has no `fn main` (or a `main` with
///     parameters the runtime can't supply). Interpreter exits
///     non-zero "no main"; JIT exits 0 silently. The example exists
///     for `mty check` / `mty fmt` parity only; running it isn't
///     meaningful. The JIT's silent-0 is a divergence on its own —
///     ideally both backends would agree on the "no callable main"
///     exit code (v0.42 follow-up).
///   * `NativeSegfault`: the JIT segfaults but the interpreter runs
///     clean. These are the real L1 codegen gaps; the conformance
///     suite gates the v0.41 floor and the codegen-side tests in
///     `crates/mty-codegen-cranelift/tests/` cover the minimal repros.
struct KnownFailing {
    name: &'static str,
    reason: KnownReason,
}

#[derive(Clone, Copy)]
enum KnownReason {
    /// Example has no callable `fn main()` — purely a `mty check`
    /// fixture. Interp returns "no main" (exit 2); JIT silently
    /// exits 0. Tracking the JIT silent-success-on-no-main as a
    /// v0.42 ergonomic gap, not a v0.41 release blocker.
    LibraryOnly,
    /// `main(net: Net, model: Model)` — the runtime can't supply
    /// capability params; interpreter exits 1 with a runtime error,
    /// JIT segfaults. v0.42 work to either accept capability
    /// stubs or make codegen refuse the call cleanly.
    MainTakesCapabilities,
    /// `Vec[T]` where `T` is an aggregate (`String`, `struct`, …).
    ///
    /// This is a LAYERED defect. v0.48 (#297) fixed three of the four
    /// layers (all regression-free, verified by CLIF + this sweep):
    ///   1. Push-only element inference — a Vec whose `T` was pinned
    ///      only by `.push(x)` (no annotation / annotated return) left
    ///      `T` an unresolved var. Fixed in mty-types: `Vec.new()` /
    ///      `with_capacity()` now synth `Vec[?E]` and `push(x)` unifies
    ///      `x` with `E`.
    ///   2. `Vec.new()` result-temp typing — the temp holding the
    ///      constructor was `IrTy::Error`, so `emit_vec_new` read the
    ///      element size off an Error `current_dest_ty` and stored the
    ///      8-byte fallback in the header. Fixed in mty-ir
    ///      (`vec_call_result_ty`): the temp now carries the real
    ///      `Vec[T]`, so the header records the true element size (16
    ///      for `String`), and the element store memcpys the right width.
    ///   3. (Consequence of 1+2) the grow-buffer allocation + element
    ///      store now use a consistent, correct element size.
    ///
    ///   4. (v0.48 #297) Vec[String] element PUSH no longer SIGSEGVs for
    ///      a valid String operand: `emit_vec_push` routes String/Str/
    ///      Bytes elements through `string_pair` (correct for both the
    ///      literal fast-path and the slot-backed case) instead of
    ///      memcpy-from-operand-address. Validated: `Vec[String]` with
    ///      literal pushes (`v.push("alpha")`) + `len()` now JIT-matches
    ///      the interpreter. Lock-in tests in
    ///      `crates/mty-codegen-cranelift/tests/vec_string_push_v048.rs`.
    ///
    /// REMAINING (still KNOWN_FAILING): native String codegen. The
    /// examples build their strings via `String.from_str(...)` /
    /// `with_capacity` / `new` and read them via `String.len()` /
    /// `push_str` / `format!` — NONE of which have native cranelift
    /// implementations (interp-only; `String.from_str` lowers to an
    /// unhandled `BuiltinId::Extern` that yields garbage in JIT, so
    /// `let s = String.from_str("a"); s.len()` gives interp 5 / JIT 0).
    /// Flipping 26/42/43 needs the whole native-String surface, a
    /// separate feature; this entry's fixes land the Vec-storage half.
    VecOfAggregate,
    /// v0.45 T1 native `std.fs` now actually touches disk under the
    /// JIT path. Examples that write to absolute hard-coded paths
    /// (`/tmp/...`) succeed under the interpreter's hosted dispatcher
    /// but fail under JIT on hosts without that prefix (Windows) or
    /// where the path is otherwise unwritable. v0.46 follow-up: rework
    /// these examples to use a per-run tempdir before re-enabling the
    /// JIT parity check.
    NativeFsRealDisk,
}

/// v0.41 ships the suite green-on-everything-else by closing the top
/// codegen gaps. Anything in this list is a documented known divergence
/// — adding to it requires a real reason + a v0.42 follow-up tag.
const KNOWN_FAILING: &[KnownFailing] = &[
    // Library-only examples (no callable main / main with non-default
    // params). Twelve at v0.41-time; they exist for `mty check` parity.
    KnownFailing {
        name: "02_struct_enum",
        reason: KnownReason::LibraryOnly,
    },
    KnownFailing {
        name: "03_generic_fn",
        reason: KnownReason::LibraryOnly,
    },
    KnownFailing {
        name: "04_result_propagation",
        reason: KnownReason::LibraryOnly,
    },
    KnownFailing {
        name: "07_agent_echo",
        reason: KnownReason::LibraryOnly,
    },
    KnownFailing {
        name: "08_agent_state",
        reason: KnownReason::LibraryOnly,
    },
    KnownFailing {
        name: "09_send_ask_deadline",
        reason: KnownReason::LibraryOnly,
    },
    KnownFailing {
        name: "10_supervisor",
        reason: KnownReason::LibraryOnly,
    },
    KnownFailing {
        name: "12_arena",
        reason: KnownReason::LibraryOnly,
    },
    KnownFailing {
        name: "13_capabilities",
        reason: KnownReason::LibraryOnly,
    },
    KnownFailing {
        name: "18_sandbox",
        reason: KnownReason::LibraryOnly,
    },
    KnownFailing {
        name: "20_frontend_component",
        reason: KnownReason::LibraryOnly,
    },
    KnownFailing {
        name: "25_agent_array",
        reason: KnownReason::LibraryOnly,
    },
    // Real codegen gaps (v0.42 follow-ups).
    KnownFailing {
        name: "19_backend_service",
        reason: KnownReason::MainTakesCapabilities,
    },
    KnownFailing {
        name: "26_string_vec",
        reason: KnownReason::VecOfAggregate,
    },
    KnownFailing {
        name: "42_crypto_url",
        reason: KnownReason::VecOfAggregate,
    },
    KnownFailing {
        name: "43_secure_session",
        reason: KnownReason::VecOfAggregate,
    },
    // v0.45 T1 native std.fs exposes hard-coded absolute-path writes
    // that previously no-op'd under the interpreter's hosted dispatcher.
    // v0.46 follow-up rewrites these examples to use a per-run tempdir.
    KnownFailing {
        name: "34_taint_untaint",
        reason: KnownReason::NativeFsRealDisk,
    },
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

fn examples_dir() -> PathBuf {
    workspace_root().join("examples")
}

/// All `examples/*.mty` files. Sorted by filename so the failure list
/// stays stable across runs / OSes.
fn enumerate_examples() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let dir = examples_dir();
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read examples dir {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|s| s.to_str()) == Some("mty") {
            out.push(path);
        }
    }
    out.sort();
    out
}

fn name_of(path: &Path) -> String {
    path.file_stem().unwrap().to_string_lossy().into_owned()
}

fn is_known_failing(name: &str) -> Option<KnownReason> {
    KNOWN_FAILING
        .iter()
        .find(|k| k.name == name)
        .map(|k| k.reason)
}

/// Drive `mty` against a single example. `extra_args` is `["--legacy-interp"]`
/// for the interp lane and `[]` for the JIT/native lane. We do NOT
/// forward stdin/stderr; the test only cares about stdout + exit code.
fn run_mty(extra_args: &[&str], example: &Path) -> (i32, String) {
    // v0.48 — run each example in its OWN tempdir (cwd + TMP/TMPDIR) so
    // the sweep can run examples concurrently without fs collisions: a
    // few examples write relative/temp files, and serial-only execution
    // is what made the Windows sweep run for >30 min (hundreds of
    // subprocess spawns). The example is passed by absolute path, so the
    // cwd swap doesn't affect resolving it.
    let work = tempfile::tempdir().expect("per-run tempdir");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mty"));
    cmd.arg("run");
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.arg(example)
        .current_dir(work.path())
        .env("TMPDIR", work.path())
        .env("TMP", work.path())
        .env("TEMP", work.path());
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("spawn mty for {}: {e}", example.display()));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let code = out.status.code().unwrap_or(-1);
    (code, stdout)
}

/// Run `diff_one` over every example, in parallel across worker threads
/// (each example is independent — its own subprocesses + per-run
/// tempdir). Returns `(name, result)` pairs in arbitrary order. This is
/// what lets the sweep finish in minutes on the slow Windows runner.
fn diff_all(examples: &[PathBuf]) -> Vec<(String, Result<(), String>)> {
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, examples.len().max(1));
    let chunk_sz = examples.len().div_ceil(n);
    std::thread::scope(|s| {
        let handles: Vec<_> = examples
            .chunks(chunk_sz.max(1))
            .map(|chunk| {
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|p| (name_of(p), diff_one(p)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("conformance worker thread panicked"))
            .collect()
    })
}

/// Compare interp vs JIT for `path`. Returns Ok(()) when they match,
/// Err(msg) otherwise. The msg includes BOTH sides so a CI diff is
/// actionable.
fn diff_one(path: &Path) -> Result<(), String> {
    let (interp_code, interp_out) = run_mty(&["--legacy-interp"], path);
    let (jit_code, jit_out) = run_mty(&[], path);
    if interp_code == jit_code && interp_out == jit_out {
        Ok(())
    } else {
        Err(format!(
            "interp[code={interp_code}, out={interp_out:?}] != \
             jit[code={jit_code}, out={jit_out:?}]"
        ))
    }
}

// ----------------------------------------------------------------------
// Public surface: one big test that runs every example. We split the
// expected-failure detection out so a v0.42 fix that flips a known
// failure to passing is loud about it (the test fails with "now
// passes, please remove from KNOWN_FAILING").
// ----------------------------------------------------------------------

#[test]
fn examples_conformance_sweep() {
    let examples = enumerate_examples();
    assert!(
        !examples.is_empty(),
        "examples dir should not be empty (looked in {})",
        examples_dir().display()
    );

    let mut passing = Vec::new();
    let mut known_failures: Vec<(String, KnownReason)> = Vec::new();
    let mut unexpected_failures: Vec<(String, String)> = Vec::new();
    let mut unexpected_passes: Vec<String> = Vec::new();

    for (name, result) in diff_all(&examples) {
        let expected = is_known_failing(&name);
        match result {
            Ok(()) => {
                if expected.is_some() {
                    unexpected_passes.push(name);
                } else {
                    passing.push(name);
                }
            }
            Err(msg) => match expected {
                Some(reason) => known_failures.push((name, reason)),
                None => unexpected_failures.push((name, msg)),
            },
        }
    }

    eprintln!("[conformance] {} examples", examples.len());
    eprintln!("[conformance] passing:           {}", passing.len());
    eprintln!("[conformance] known failures:    {}", known_failures.len());
    eprintln!(
        "[conformance] unexpected fails:  {}",
        unexpected_failures.len()
    );
    eprintln!(
        "[conformance] unexpected passes: {}",
        unexpected_passes.len()
    );

    if !unexpected_failures.is_empty() {
        let mut buf = String::from("Unexpected interp/JIT divergence:\n");
        for (name, msg) in &unexpected_failures {
            buf.push_str(&format!("  - {name}: {msg}\n"));
        }
        panic!("{buf}");
    }
    if !unexpected_passes.is_empty() {
        panic!(
            "Examples in KNOWN_FAILING that now pass — please remove from the list:\n  {:?}",
            unexpected_passes
        );
    }
}

// ----------------------------------------------------------------------
// Floor check: pin the minimum number of passing examples so future
// regressions surface as "examples_floor_dropped" instead of as a
// surprise inside the sweep diff.
// ----------------------------------------------------------------------

#[test]
fn examples_passing_floor_holds() {
    let examples = enumerate_examples();
    let passing = diff_all(&examples)
        .into_iter()
        .filter(|(_, r)| r.is_ok())
        .count();
    // v0.42 T1 bumped the floor to 28 with the L28/L21 regression
    // example (`examples/44_vec_growth_in_loop.mty`) joining the
    // clean-passing set. v0.45 T1 native std.fs reclaims one slot
    // (`34_taint_untaint` now writes to a hard-coded `/tmp/...`
    // path under the JIT — v0.46 follow-up), so the net floor sits
    // at 27 until that example is reworked. v0.41 T3 baseline was
    // 27 (out of ~43 total, depending on add/remove); pre-v0.41-T3
    // the count was 24.
    const FLOOR: usize = 27;
    assert!(
        passing >= FLOOR,
        "expected >= {FLOOR} examples to pass interp/JIT diff, only {passing} did",
    );
}

// ----------------------------------------------------------------------
// `mty build` smoke (object-emission path). Even on Windows-no-clang
// the build path MUST emit a .o without crashing — that's the L10
// floor the conformance suite gates. We run this against a tiny
// hand-rolled program (not every example) because some examples
// type-check but exercise codegen paths whose object-emission is
// itself the open follow-up.
// ----------------------------------------------------------------------

#[test]
fn mty_build_hello_emits_object_or_exe() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("hello.mty");
    std::fs::write(&src, "fn main() { log(\"hello, conformance\") }\n").expect("write hello.mty");
    let out = Command::new(env!("CARGO_BIN_EXE_mty"))
        .arg("build")
        .arg(&src)
        .arg("--out-dir")
        .arg(dir.path())
        .output()
        .expect("spawn mty build");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    if out.status.success() {
        // Either "wrote target/hello.exe" (linker found) or "wrote
        // object hello.o (no linker found)" is a valid completion shape.
        assert!(
            stdout.contains("wrote"),
            "expected 'wrote ...' line in stdout; got: {stdout:?}"
        );
    } else {
        assert_eq!(
            out.status.code(),
            Some(2),
            "unexpected mty build exit: stdout={stdout:?} stderr={stderr:?}"
        );
        assert!(
            stderr.contains("link error after writing object"),
            "expected explicit link error; stderr={stderr:?}"
        );
        let obj = dir.path().join("hello.o");
        assert!(
            obj.exists(),
            "link error must still leave object artifact at {}",
            obj.display()
        );
    }
}
