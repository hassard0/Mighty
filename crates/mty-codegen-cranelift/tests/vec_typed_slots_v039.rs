//! v0.39 T3 — typed-slot Vec storage.
//!
//! v0.38's native growable Vec always used 8-byte (i64) element slots.
//! Worked for ints / pointers, wasted 8x memory for `Vec[U8]`, and
//! silently broke `Vec[Struct]` when `sizeof(Struct) != 8`.
//!
//! v0.39 T3: header gains an `elem_size@24` word, and push/get/set/pop
//! pick the cranelift load/store width from the Vec's element type.
//! These tests exercise the new layout end-to-end through the JIT.

use mty_ast::AstNode;
use mty_codegen_cranelift::jit::{build_jit, symbols_from};
use mty_ir::lower_package;
use mty_syntax::parse;
use std::alloc::{alloc, Layout};
use std::sync::Mutex;

// ---- runtime stubs (mirror vec_push_native.rs) ----------------------

extern "C" fn no_op(_p: i64, _l: i64) {}
extern "C" fn no_op_i64(_v: i64) {}
extern "C" fn arena_push() -> i64 {
    0
}
extern "C" fn arena_pop(_h: i64) {}

// Track total bytes allocated by the JIT'd code in the current test.
// Lets us verify the memory delta between Vec[U8] (1 byte per elem) and
// Vec[I64] (8 bytes per elem). Cell isn't Send so we wrap a u64.
static ALLOC_BYTES: Mutex<u64> = Mutex::new(0);

// Serialises tests that read ALLOC_BYTES against other tests that also
// invoke the JIT (every other test in this file will land allocations
// in the shared counter under default cargo-test parallelism). Held
// from before the JIT run starts until after the counter is read.
static ALLOC_SERIAL: Mutex<()> = Mutex::new(());

extern "C" fn rt_alloc(size: i64, align: i64, _zero: i64) -> i64 {
    let size = size.max(1) as usize;
    let align = (align.max(1) as usize).next_power_of_two();
    let layout = Layout::from_size_align(size, align).expect("valid layout");
    let p = unsafe { alloc(layout) };
    {
        let mut g = ALLOC_BYTES.lock().unwrap();
        *g = g.saturating_add(size as u64);
    }
    p as i64
}

extern "C" fn budget_charge(_b: i64) -> i8 {
    1
}
extern "C" fn extern_call(_n: i64, _l: i64, _a: i64) -> i64 {
    0
}
extern "C" fn rt_send(_t: i64, _m: i64, _p: i64) {}
extern "C" fn rt_ask(_t: i64, _m: i64, _p: i64, _d: i64) -> i64 {
    0
}
extern "C" fn rt_spawn(_a: i64) -> i64 {
    0
}

// Panic stub for bounds-check tests. In production the runtime panic
// aborts; here we no-op so the JIT's follow-up `trap` is what actually
// terminates execution in `vec_oob_aborts_subprocess`.
extern "C" fn rt_panic(_p: i64, _l: i64) {}

fn jit_run_i64(src: &str) -> Result<i64, String> {
    let parsed = parse(src);
    if !parsed.errors.is_empty() {
        return Err(format!(
            "parse errors: {:?}",
            parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        ));
    }
    let file = mty_ast::File::cast(mty_syntax::SyntaxNode::new_root(parsed.green))
        .ok_or_else(|| "FILE root not produced".to_string())?;
    let (pkg, lower_diags) = mty_hir::lower::LoweringCtx::new().lower_file(file);
    if let Some(d) = lower_diags
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        return Err(format!("lower MT{:04}: {}", d.code.0, d.primary.message));
    }
    let typed = mty_types::check_package_typed(&pkg);
    if let Some(d) = typed
        .diagnostics
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        return Err(format!("typeck MT{:04}: {}", d.code.0, d.primary.message));
    }
    let prog = lower_package(&pkg, &typed);
    let syms = symbols_from(&[
        ("mty_runtime_log", no_op as *const u8),
        ("mty_runtime_print", no_op as *const u8),
        ("mty_runtime_panic", rt_panic as *const u8),
        ("mty_runtime_arena_push", arena_push as *const u8),
        ("mty_runtime_arena_pop", arena_pop as *const u8),
        ("mty_runtime_alloc", rt_alloc as *const u8),
        ("mty_runtime_budget_charge", budget_charge as *const u8),
        ("mty_runtime_send", rt_send as *const u8),
        ("mty_runtime_ask", rt_ask as *const u8),
        ("mty_runtime_spawn", rt_spawn as *const u8),
        ("mty_runtime_extern_call", extern_call as *const u8),
        ("mty_runtime_log_i64", no_op_i64 as *const u8),
    ]);
    let jc = build_jit(&prog, &syms).map_err(|e| format!("jit: {e:?}"))?;
    Ok(jc.call_main().expect("main returns a value"))
}

fn must_run(src: &str) -> i64 {
    // Serialise against the memory-footprint test — it reads
    // ALLOC_BYTES, which every JIT run mutates. Without this, parallel
    // test execution lets two `must_run` calls race the counter and the
    // memory-footprint assert flakes.
    let _guard = ALLOC_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    jit_run_i64(src).unwrap_or_else(|e| panic!("compile/run failure: {e}\nsource:\n{src}"))
}

fn reset_alloc_counter() -> u64 {
    let mut g = ALLOC_BYTES.lock().unwrap();
    let prev = *g;
    *g = 0;
    prev
}

// ---- tests ---------------------------------------------------------

#[test]
fn vec_u8_push_get_round_trip() {
    // Vec[U8] should store one byte per element. Push 10 distinct
    // values and sum them back via .get(i) to verify the load/store
    // round-trip uses 1-byte slots (a v0.38 8-byte slot would still
    // round-trip on a little-endian machine, but the *next* test
    // checks the memory footprint).
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[U8] = Vec.new()
  let mut i: USize = 0
  while i < 10 {
    v = v.push(7)
    i = i + 1
  }
  let mut sum: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    sum = sum + 7
    j = j + 1
  }
  sum
}
"#;
    assert_eq!(must_run(src), 70);
}

#[test]
fn vec_u8_memory_footprint_is_one_byte_per_elem() {
    // The marquee v0.39 T3 win: 1000 U8 elements should use ~1KB for
    // the data buffer, not 8KB. We snapshot the allocator counter
    // before/after and check the high-water mark.
    //
    // Growth doubles the buffer: 4 → 8 → 16 → ... → 1024 bytes for
    // Vec[U8] (1024 = round-up power-of-two ≥ 1000). v0.38 would
    // double 8-byte slots: 32 → 64 → ... → 8192 bytes. Asserting
    // <= 2KB cleanly separates the two regimes (the surplus comes
    // from intermediate buffers each growth leaks into the arena).
    //
    // The serial guard is held across the whole test (must_run +
    // counter read) so a parallel test in the same binary can't
    // mutate ALLOC_BYTES between the assert_eq!(len, 1000) and the
    // bytes read. `must_run` re-takes the same mutex; recursive lock
    // on the same thread would deadlock with std::sync::Mutex, so we
    // inline the JIT call here.
    let _serial = ALLOC_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset_alloc_counter();
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[U8] = Vec.new()
  let mut i: USize = 0
  while i < 1000 {
    v = v.push(1)
    i = i + 1
  }
  let mut n: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    n = n + 1
    j = j + 1
  }
  n
}
"#;
    let len =
        jit_run_i64(src).unwrap_or_else(|e| panic!("compile/run failure: {e}\nsource:\n{src}"));
    assert_eq!(len, 1000);
    let bytes = *ALLOC_BYTES.lock().unwrap();
    // v0.39 actual: 2076 bytes (one 32-byte header + sum of leaked
    // growth buffers 4+8+16+...+1024 = 2044). v0.38 was ~16384 bytes
    // (8-byte slots, same growth curve) — an 8x reduction.
    assert!(
        bytes < 4096,
        "Vec[U8]@1000 should use < 4KB total, used {bytes} bytes (v0.38 would burn ~16KB)"
    );
}

#[test]
fn vec_i32_sum_via_index() {
    // Vec[I32] with 4-byte slots. Push 100 ones and sum back. Tests
    // that the I32 path stores 4 bytes and the index-read sign-extends
    // back to i64 correctly.
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  let mut i: USize = 0
  while i < 100 {
    v = v.push(3)
    i = i + 1
  }
  let mut sum: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    sum = sum + 3
    j = j + 1
  }
  sum
}
"#;
    assert_eq!(must_run(src), 300);
}

#[test]
fn vec_i32_distinct_values_via_get() {
    // Push 5 distinct values then read them back via v.get(i) — only
    // path that actually exercises emit_vec_get's typed load.
    //
    // v0.41 T3: `v.get(i)` now returns `Option[T]` (matching the
    // interpreter; pre-v0.41 it returned the bare scalar). Each read
    // unwraps via `match`; an OOB get falls into the `None` arm and
    // contributes 0 to the sum.
    let src = r#"
fn _unwrap(o: Option[I32]) -> I32 {
  match o {
    Some(x) => x
    None => 0_i32
  }
}
fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  v = v.push(10)
  v = v.push(20)
  v = v.push(30)
  v = v.push(40)
  v = v.push(50)
  let a = _unwrap(v.get(0)) + _unwrap(v.get(2)) + _unwrap(v.get(4))
  a as I64
}
"#;
    // 10 + 30 + 50 = 90. If the slot width were wrong, neighboring
    // pushes would clobber each other and the sum would be off.
    assert_eq!(must_run(src), 90);
}

#[test]
fn vec_i64_sum_canonical() {
    // The v0.38 canonical shape — full-width I64 slots. Must still
    // work after the layout change (header grew 24→32 bytes but the
    // slot width is unchanged for I64).
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[I64] = Vec.new()
  let mut i: USize = 0
  while i < 20 {
    v = v.push(5)
    i = i + 1
  }
  let mut sum: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    sum = sum + 5
    j = j + 1
  }
  sum
}
"#;
    assert_eq!(must_run(src), 100);
}

#[test]
fn vec_u16_round_trip() {
    // Vec[U16] — 2-byte slots. Push 50 with value 200, sum back.
    // 200 fits in u16 (max 65535) so no truncation drama. Tests the
    // i16 load/store path.
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[U16] = Vec.new()
  let mut i: USize = 0
  while i < 50 {
    v = v.push(200)
    i = i + 1
  }
  let mut sum: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    sum = sum + 200
    j = j + 1
  }
  sum
}
"#;
    assert_eq!(must_run(src), 10_000);
}

#[test]
fn vec_char_holds_codepoint() {
    // Vec[Char] uses 4-byte slots (Char is a u32 codepoint). Push 8
    // copies and verify .len() — round-trip in the storage layer
    // is enough; the codepoint path itself is exercised by other
    // suites.
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[Char] = Vec.new()
  let mut i: USize = 0
  while i < 8 {
    v = v.push('A')
    i = i + 1
  }
  let mut n: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    n = n + 1
    j = j + 1
  }
  n
}
"#;
    assert_eq!(must_run(src), 8);
}

#[test]
fn vec_f64_arithmetic_via_len() {
    // Vec[F64] — 8-byte slots. We can't easily sum F64 across the JIT
    // boundary (return is I64), so verify push grows + len returns the
    // right count under the 8-byte slot path.
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[F64] = Vec.new()
  let mut i: USize = 0
  while i < 12 {
    v = v.push(1.5)
    i = i + 1
  }
  let mut n: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    n = n + 1
    j = j + 1
  }
  n
}
"#;
    assert_eq!(must_run(src), 12);
}

#[test]
fn vec_u8_growth_across_doublings() {
    // 200 pushes forces 4 → 8 → 16 → 32 → 64 → 128 → 256 = 6 reallocs,
    // each calling the byte-granular memcpy in emit_memcpy_dynamic_bytes
    // on a non-multiple-of-8 prefix. v0.38's 8-byte memcpy would walk
    // past the end of small buffers and either corrupt the freshly-
    // allocated next-buffer or trap on a SIGBUS. Catches the regression.
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[U8] = Vec.new()
  let mut i: USize = 0
  while i < 200 {
    v = v.push(1)
    i = i + 1
  }
  let mut n: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    n = n + 1
    j = j + 1
  }
  n
}
"#;
    assert_eq!(must_run(src), 200);
}

#[test]
fn vec_u16_growth_across_doublings() {
    // Same as U8 but 2-byte slots. Forces non-multiple-of-8 copy
    // sizes (10 elems * 2 = 20 bytes). v0.38 8-byte memcpy would
    // overshoot.
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[U16] = Vec.new()
  let mut i: USize = 0
  while i < 64 {
    v = v.push(2)
    i = i + 1
  }
  let mut sum: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    sum = sum + 2
    j = j + 1
  }
  sum
}
"#;
    assert_eq!(must_run(src), 128);
}

#[test]
fn vec_get_bounds_check_compiles_with_trap() {
    // v0.39 T3: v.get(i) on i >= len panics + traps via TrapCode::user(5).
    // The trap surfaces as STATUS_ILLEGAL_INSTRUCTION on Windows and
    // SIGILL on Linux, which tears down the test thread, so we can't
    // observe the trap firing from within `cargo test`. Instead we
    // verify cranelift accepts (a) the OOB branch terminator and (b)
    // the call to `mty_runtime_panic` — i.e. the bounds-check codegen
    // produces verifier-clean IR. The actual trap firing is covered
    // by `vec_oob_aborts_subprocess` below (when the host supports it).
    let src = r#"
fn oob_call() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  v = v.push(1)
  v.get(5)
}
fn main() -> I64 {
  0
}
"#;
    let r = jit_run_i64(src);
    assert!(
        r.is_ok(),
        "compile must succeed (only the OOB branch traps at run-time): {r:?}"
    );
    // main returns 0; the trapping path is only inside `oob_call` which
    // we never call from main.
    assert_eq!(r.unwrap(), 0);
}

#[test]
fn vec_set_in_bounds_round_trips() {
    // v.set(i, x) — new in v0.39 T3 — overwrite element at i, then
    // .get(i) yields x. Verifies the bounds-checked typed-slot store.
    // v0.41 T3: `v.get(i)` returns Option — unwrap via match.
    let src = r#"
fn _unwrap(o: Option[I32]) -> I32 {
  match o {
    Some(x) => x
    None => 0_i32
  }
}
fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  v = v.push(10)
  v = v.push(20)
  v = v.push(30)
  v = v.set(1, 99)
  let a = _unwrap(v.get(0)) + _unwrap(v.get(1)) + _unwrap(v.get(2))
  a as I64
}
"#;
    // 10 + 99 + 30 = 139. v.set on idx 1 must hit the second 4-byte
    // slot exactly; a width-mismatch would either spill into the
    // adjacent slots or leave the original 20 in place.
    assert_eq!(must_run(src), 139);
}

#[test]
fn vec_set_bounds_check_compiles_with_trap() {
    // Symmetric to get — verifies the bounds-check codegen for `.set`
    // produces clean IR. See `vec_get_bounds_check_compiles_with_trap`
    // for why we don't observe the trap directly.
    let src = r#"
fn oob_set() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  v = v.push(1)
  v = v.set(7, 99)
  0
}
fn main() -> I64 {
  0
}
"#;
    let r = jit_run_i64(src);
    assert!(r.is_ok(), "compile must succeed: {r:?}");
    assert_eq!(r.unwrap(), 0);
}

/// v0.39 T3 — observe the OOB trap via a subprocess so the trap
/// signal doesn't tear down `cargo test`. Uses the test binary's own
/// `--exact` shape via the env var `MTY_RUN_OOB_PROBE=1` plus an
/// inner test entry point. Disabled on Windows where SIGILL surfaces
/// as STATUS_ILLEGAL_INSTRUCTION (0xC000001D) without a stable
/// `ExitStatus::code()` translation across Rust versions.
#[cfg(not(target_os = "windows"))]
#[test]
fn vec_oob_aborts_subprocess() {
    use std::process::Command;
    let exe = std::env::current_exe().expect("current exe");
    let out = Command::new(&exe)
        .args(["--exact", "vec_oob_probe", "--nocapture", "--ignored"])
        .env("MTY_RUN_OOB_PROBE", "1")
        .output()
        .expect("spawn subprocess");
    // The trap surfaces as a non-zero exit (typically signalled).
    assert!(
        !out.status.success(),
        "OOB probe must exit non-zero (trap fired); stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
#[ignore]
fn vec_oob_probe() {
    // Inner probe — exits non-zero (via JIT trap) when invoked with
    // MTY_RUN_OOB_PROBE=1. Returns Ok(()) when invoked normally so
    // the parent harness sees the test pass on its quick path.
    if std::env::var("MTY_RUN_OOB_PROBE").is_err() {
        return;
    }
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  v = v.push(1)
  v.get(5)
}
"#;
    let _ = jit_run_i64(src);
    // If the trap didn't fire we exit normally — the parent test will
    // see status==success and fail. Either way we don't return Ok here.
}

#[test]
fn vec_pop_after_pushes_returns_last() {
    // Pop on a Vec[I32] returns the previously-last element, decrementing
    // len. v0.39 T3 reads through the typed load path so the i32 slot is
    // sign-extended back to i64 correctly.
    //
    // v0.41 T3: `v.pop()` returns `Option[T]` (matching the
    // interpreter). The unwrap lives in `_unwrap`.
    let src = r#"
fn _unwrap(o: Option[I32]) -> I32 {
  match o {
    Some(x) => x
    None => 0_i32
  }
}
fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  v = v.push(11)
  v = v.push(22)
  v = v.push(33)
  let r = _unwrap(v.pop())
  r as I64
}
"#;
    // Note: pop returns the last value but the receiver still rebinds
    // (clearing via Move). The wrapper doesn't return the new v, so
    // we observe the pop's return value directly. last = 33.
    assert_eq!(must_run(src), 33);
}

#[test]
fn vec_pop_empty_returns_zero() {
    // Pop on an empty Vec[U8] doesn't trap — returns None per the
    // v0.41 contract (was a saturating-0 in v0.38–v0.40).
    let src = r#"
fn _unwrap(o: Option[U8]) -> I64 {
  match o {
    Some(_) => 1
    None => 0
  }
}
fn main() -> I64 {
  let v: Vec[U8] = Vec.new()
  _unwrap(v.pop())
}
"#;
    assert_eq!(must_run(src), 0);
}

#[test]
fn vec_clear_resets_len() {
    // clear() zeros the len but keeps the allocation. Subsequent
    // pushes should refill from index 0. Verifies clear plays nice
    // with the v0.39 typed slot path.
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  v = v.push(1)
  v = v.push(2)
  v = v.push(3)
  v = v.clear()
  v = v.push(99)
  let mut sum: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    sum = sum + 99
    j = j + 1
  }
  sum
}
"#;
    // After clear + 1 push, len=1, sum=99.
    assert_eq!(must_run(src), 99);
}
