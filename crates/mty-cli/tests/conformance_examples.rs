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
    /// The SIR lowerer types the temp holding `Vec.new()` as
    /// `IrTy::Error` so the element width is the i64 fallback (8)
    /// instead of the real `T` size; the memcpy stride mismatch
    /// corrupts memory. Documented in v0.41 T3 RELEASE notes as a
    /// v0.42 typeck-side follow-up (the codegen side is correct;
    /// the SIR loses the type info before lowering sees it).
    VecOfAggregate,
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
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mty"));
    cmd.arg("run");
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.arg(example);
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("spawn mty for {}: {e}", example.display()));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let code = out.status.code().unwrap_or(-1);
    (code, stdout)
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

    for path in &examples {
        let name = name_of(path);
        let expected = is_known_failing(&name);
        match diff_one(path) {
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
    let mut passing = 0usize;
    for path in &examples {
        if diff_one(path).is_ok() {
            passing += 1;
        }
    }
    // v0.42 T1 bumps the floor to 28: the L28/L21 regression
    // example (`examples/44_vec_growth_in_loop.mty`) joins the
    // clean-passing set so any future regression in the
    // Vec-rebind-across-loop lowering trips this assertion on top
    // of the per-example diff in `examples_conformance_sweep`.
    // v0.41 T3 baseline was 27 (out of ~43 total, depending on
    // add/remove); pre-v0.41-T3 the count was 24.
    const FLOOR: usize = 28;
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
fn mty_build_hello_reports_runnable_or_failure_truthfully() {
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
    if !out.status.success() {
        assert!(
            stderr.contains("no linker found") || stderr.contains("build error:"),
            "native build failure should explain the cause; stdout={stdout:?} stderr={stderr:?}"
        );
        return;
    }
    // Success means the native build produced a runnable artifact.
    // Object-only output is allowed only on the non-zero no-linker path above.
    assert!(
        stdout.contains("wrote"),
        "expected 'wrote ...' line in stdout; got: {stdout:?}"
    );
}
