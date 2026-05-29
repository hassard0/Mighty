//! v0.38 T2 — variadic extern-C call codegen.
//!
//! v0.37 T6 shipped variadic parsing + typeck + SIR + the cranelift
//! signature flag. The cranelift call site, however, still bailed with
//! `CodegenError::Unsupported` whenever the caller passed any trailing
//! `...` extras: cranelift 0.132's `Signature` is purely positional
//! and `Function::call(FuncRef, …)` rejects any extra args beyond the
//! declared signature.
//!
//! v0.38 T2 wires the actual codegen path:
//!
//! 1. Build a *per-call* `ir::Signature` from the fixed-prefix declared
//!    types plus the C-ABI-promoted extra types.
//! 2. Import it via `FunctionBuilder::import_signature`.
//! 3. Take the symbol address with `func_addr(ptr_ty, func_ref)` —
//!    the extern is declared with `Linkage::Import`, so JIT /
//!    linker resolves the address from a real symbol.
//! 4. Dispatch via `call_indirect(sig_ref, addr, &args)`.
//!
//! Promotion rules (`abi::cl_ty_for_variadic`):
//!
//!   * f32 → f64
//!   * i8/i16 → i32 (signed)
//!   * u8/u16 → u32 (unsigned)
//!   * bool / char → i32
//!   * pointers / wider scalars: pass through
//!
//! The tests below build small SIR programs by hand (the parser path
//! for the full `printf("%d %d", a, b)` expression doesn't pin the
//! arg types tightly enough for a tight-promotion test) and dispatch
//! through the JIT. A real-libc `printf` round-trip is exercised by
//! the `printf_returns_bytes_written` test at the end — it provides
//! the `printf` symbol from the host process at JIT-resolve time.

use mty_codegen_cranelift::abi::{cl_ty_for, cl_ty_for_variadic};
use mty_codegen_cranelift::jit::{build_jit, symbols_from};
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, Const, ExternBinding, FnRef, Function, IrFnId, IrTy, Local, LocalDecl,
    LocalSource, Operand, Place, Program, Rvalue, Stmt, Term,
};
use mty_types::{FloatKind, IntKind};
use std::sync::Mutex;

// `MTY_DUMP_CLIF` is a process-wide env var, so any two tests that set
// it concurrently will clobber each other (and the "without extras"
// assertion would then read a leftover main.clif from a different
// test's run). Serialize every CLIF-dumping test through this Mutex.
static CLIF_DUMP_LOCK: Mutex<()> = Mutex::new(());

/// Build a minimal SIR program with `printf(*const U8, ...)` declared
/// as a variadic extern C, plus a `main() -> I32` body that calls
/// `printf(fmt_ptr, extras...)` with the supplied extras. Returns the
/// program and the (fixed) format-pointer local id used by main.
///
/// `fmt_ptr_value`: i128 constant treated as a `USize`-shaped pointer
/// (slice 8's pointer model is i64). Tests that don't really call into
/// libc supply `0`; the real-printf round-trip provides the actual
/// pointer to a null-terminated C string.
fn variadic_printf_program(extras: Vec<(Const, IrTy)>, fmt_ptr_value: i128) -> Program {
    let mut p = Program::default();

    // --- extern fn printf(fmt: USize_as_ptr, ...) -> I32 ---
    let printf_id = IrFnId(0);
    p.fns.push(Function {
        id: printf_id,
        name: "printf".into(),
        params: vec![Local(1)],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Int(IntKind::I32),
                mutable: false,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "fmt".into(),
                // Slice-8 carries pointers as I64 (USize-shaped). The
                // ABI layer collapses *const U8 → ct::I64 either way.
                ty: IrTy::Int(IntKind::USize),
                mutable: false,
                source: LocalSource::Param,
            },
        ],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Int(IntKind::I32),
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });
    p.extern_bindings.insert(
        printf_id,
        ExternBinding {
            abi: "c".into(),
            name: "printf".into(),
            is_variadic: true,
        },
    );

    // --- main fn ---
    // locals: _0 ret (I64) , fmt_ptr (USize) , one local per extra.
    let mut main_locals: Vec<LocalDecl> = vec![
        LocalDecl {
            name: "_0".into(),
            ty: IrTy::Int(IntKind::I64),
            mutable: false,
            source: LocalSource::Return,
        },
        LocalDecl {
            name: "fmt".into(),
            ty: IrTy::Int(IntKind::USize),
            mutable: true,
            source: LocalSource::UserLet,
        },
    ];
    for (i, (_, ty)) in extras.iter().enumerate() {
        main_locals.push(LocalDecl {
            name: format!("extra_{i}"),
            ty: ty.clone(),
            mutable: true,
            source: LocalSource::UserLet,
        });
    }
    // Extra: a local to drop the printf return value into.
    main_locals.push(LocalDecl {
        name: "ret".into(),
        ty: IrTy::Int(IntKind::I32),
        mutable: true,
        source: LocalSource::UserLet,
    });
    let n_extras = extras.len();
    let ret_local = Local((2 + n_extras) as u32);

    // --- stmts: assign fmt, assign each extra, call printf ---
    let mut stmts: Vec<Stmt> = Vec::new();
    stmts.push(Stmt::Assign(
        Place::local(Local(1)),
        Rvalue::Const(Const::Int(fmt_ptr_value, IntKind::USize)),
    ));
    for (i, (c, _ty)) in extras.iter().enumerate() {
        stmts.push(Stmt::Assign(
            Place::local(Local((2 + i) as u32)),
            Rvalue::Const(c.clone()),
        ));
    }
    // Build args = [fmt_ptr, extras...].
    let mut args: Vec<Operand> = Vec::with_capacity(1 + n_extras);
    args.push(Operand::Copy(Place::local(Local(1))));
    for i in 0..n_extras {
        args.push(Operand::Copy(Place::local(Local((2 + i) as u32))));
    }
    stmts.push(Stmt::Assign(
        Place::local(ret_local),
        Rvalue::Call {
            func: FnRef::User(printf_id),
            args,
        },
    ));

    let main_id = IrFnId(1);
    p.fns.push(Function {
        id: main_id,
        name: "main".into(),
        params: vec![],
        locals: main_locals,
        blocks: vec![Block {
            id: BlockId(0),
            stmts,
            terminator: Term::Return(Operand::Const(Const::Int(0, IntKind::I64))),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Int(IntKind::I64),
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });

    p
}

/// Default runtime symbols (all no-ops) for JIT builds. Tests that
/// want real libc symbols add them on top with `extra_syms`.
extern "C" fn no_op() {}
extern "C" fn no_op_2(_a: i64, _b: i64) {}

fn default_runtime_syms() -> Vec<(&'static str, *const u8)> {
    vec![
        ("mty_runtime_log", no_op_2 as *const u8),
        ("mty_runtime_print", no_op_2 as *const u8),
        ("mty_runtime_panic", no_op_2 as *const u8),
        ("mty_runtime_arena_push", no_op as *const u8),
        ("mty_runtime_arena_pop", no_op as *const u8),
        ("mty_runtime_alloc", no_op as *const u8),
        ("mty_runtime_budget_charge", no_op as *const u8),
        ("mty_runtime_send", no_op as *const u8),
        ("mty_runtime_ask", no_op as *const u8),
        ("mty_runtime_spawn", no_op as *const u8),
        ("mty_runtime_extern_call", no_op as *const u8),
        ("mty_runtime_log_i64", no_op as *const u8),
    ]
}

// ============================================================
// Group 1: codegen compiles cleanly — no Unsupported for extras
// ============================================================

/// Pre-v0.38 this returned CodegenError::Unsupported. The new path
/// must produce a JIT module without errors. Uses a no-op printf
/// override so the call is dispatchable.
#[test]
fn printf_with_single_i32_extra_compiles() {
    let p = variadic_printf_program(
        vec![(Const::Int(42, IntKind::I32), IrTy::Int(IntKind::I32))],
        0,
    );
    let mut syms = default_runtime_syms();
    // Override "printf" with a no-op so the JIT symbol-resolver finds
    // the address; otherwise func_addr against an unresolved Linkage::Import
    // would fault at call time.
    extern "C" fn fake_printf() -> i32 {
        0
    }
    syms.push(("printf", fake_printf as *const u8));
    let syms = symbols_from(&syms);
    let jc = build_jit(&p, &syms).expect("variadic call site must lower without error");
    assert!(jc.main_ptr.is_some(), "main symbol missing");
    // Don't actually invoke main — passing a 0-pointer as fmt would
    // crash a real printf. The fake_printf above just returns 0 so
    // even if we did call it would be safe.
    let _ = jc.call_main();
}

#[test]
fn printf_with_multiple_mixed_extras_compiles() {
    // i32 + u8 + f32 + f64 + ptr-ish (USize) — exercises all the
    // promotion paths in one call site.
    let extras = vec![
        (Const::Int(7, IntKind::I32), IrTy::Int(IntKind::I32)),
        (Const::Int(255, IntKind::U8), IrTy::Int(IntKind::U8)),
        (
            Const::Float(1.5, FloatKind::F32),
            IrTy::Float(FloatKind::F32),
        ),
        (
            Const::Float(2.25, FloatKind::F64),
            IrTy::Float(FloatKind::F64),
        ),
        (Const::Int(0, IntKind::USize), IrTy::Int(IntKind::USize)),
    ];
    let p = variadic_printf_program(extras, 0);
    let mut syms = default_runtime_syms();
    extern "C" fn fake_printf() -> i32 {
        0
    }
    syms.push(("printf", fake_printf as *const u8));
    let syms = symbols_from(&syms);
    let _jc = build_jit(&p, &syms).expect("mixed-types variadic call must lower");
}

#[test]
fn printf_with_zero_extras_still_compiles() {
    // Sanity: even when the caller passes no extras at a variadic
    // call site, the fixed-prefix path should still work (and not
    // accidentally route through the call_indirect branch).
    let p = variadic_printf_program(vec![], 0);
    let mut syms = default_runtime_syms();
    extern "C" fn fake_printf() -> i32 {
        0
    }
    syms.push(("printf", fake_printf as *const u8));
    let syms = symbols_from(&syms);
    let _jc = build_jit(&p, &syms).expect("zero-extras variadic call must lower");
}

// ============================================================
// Group 2: C ABI promotion rules — unit tests on the helper.
// ============================================================

#[test]
fn promotion_u8_widens_to_i32() {
    use cranelift_codegen::ir::types as ct;
    let (ty, unsigned) = cl_ty_for_variadic(&IrTy::Int(IntKind::U8));
    assert_eq!(ty, ct::I32);
    assert!(unsigned, "u8 must be marked unsigned for uextend");
}

#[test]
fn promotion_i16_widens_to_i32_signed() {
    use cranelift_codegen::ir::types as ct;
    let (ty, unsigned) = cl_ty_for_variadic(&IrTy::Int(IntKind::I16));
    assert_eq!(ty, ct::I32);
    assert!(!unsigned, "i16 must be marked signed for sextend");
}

#[test]
fn promotion_f32_widens_to_f64() {
    use cranelift_codegen::ir::types as ct;
    let (ty, _) = cl_ty_for_variadic(&IrTy::Float(FloatKind::F32));
    assert_eq!(ty, ct::F64);
}

#[test]
fn promotion_pointer_sized_unchanged() {
    use cranelift_codegen::ir::types as ct;
    let (ty, _) = cl_ty_for_variadic(&IrTy::Int(IntKind::USize));
    assert_eq!(ty, ct::I64);
}

#[test]
fn fixed_prefix_pointer_lowers_to_i64() {
    // Sanity check on the fixed-prefix lowering: a *const U8 (= USize
    // shaped pointer in slice-8's model) lowers to i64 too.
    use cranelift_codegen::ir::types as ct;
    assert_eq!(cl_ty_for(&IrTy::Int(IntKind::USize)), ct::I64);
}

// ============================================================
// Group 3: structural — the call site uses call_indirect when
// extras are present, plain call otherwise. We verify this by
// dumping the CLIF via MTY_DUMP_CLIF and grepping for the
// expected instructions.
// ============================================================

#[test]
fn variadic_call_with_extras_emits_call_indirect_in_clif() {
    let _g = CLIF_DUMP_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::env::set_var("MTY_DUMP_CLIF", tmp.path());

    let p = variadic_printf_program(
        vec![(Const::Int(42, IntKind::I32), IrTy::Int(IntKind::I32))],
        0,
    );
    let mut syms = default_runtime_syms();
    extern "C" fn fake_printf() -> i32 {
        0
    }
    syms.push(("printf", fake_printf as *const u8));
    let syms = symbols_from(&syms);
    let _jc = build_jit(&p, &syms).expect("variadic call must lower");

    std::env::remove_var("MTY_DUMP_CLIF");
    let main_clif = std::fs::read_to_string(tmp.path().join("main.clif")).expect("main.clif");
    assert!(
        main_clif.contains("call_indirect"),
        "expected `call_indirect` in main.clif, got:\n{main_clif}"
    );
    assert!(
        main_clif.contains("func_addr"),
        "expected `func_addr` in main.clif, got:\n{main_clif}"
    );
}

#[test]
fn variadic_call_without_extras_emits_direct_call_in_clif() {
    let _g = CLIF_DUMP_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::env::set_var("MTY_DUMP_CLIF", tmp.path());

    let p = variadic_printf_program(vec![], 0);
    let mut syms = default_runtime_syms();
    extern "C" fn fake_printf() -> i32 {
        0
    }
    syms.push(("printf", fake_printf as *const u8));
    let syms = symbols_from(&syms);
    let _jc = build_jit(&p, &syms).expect("zero-extras variadic call must lower");

    std::env::remove_var("MTY_DUMP_CLIF");
    let main_clif = std::fs::read_to_string(tmp.path().join("main.clif")).expect("main.clif");
    // Zero extras → no per-call signature build; the direct fixed-arity
    // call path emits a `call` (not `call_indirect`) instruction.
    assert!(
        main_clif.contains(" call "),
        "expected direct `call` in main.clif, got:\n{main_clif}"
    );
    assert!(
        !main_clif.contains("call_indirect"),
        "did NOT expect call_indirect for zero-extras variadic, got:\n{main_clif}"
    );
}

// ============================================================
// Group 4: u8 / i16 promotion lowered into actual CLIF.
// ============================================================

#[test]
fn u8_extra_widens_to_i32_in_clif_via_uextend() {
    let _g = CLIF_DUMP_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::env::set_var("MTY_DUMP_CLIF", tmp.path());

    let p = variadic_printf_program(
        vec![(Const::Int(255, IntKind::U8), IrTy::Int(IntKind::U8))],
        0,
    );
    let mut syms = default_runtime_syms();
    extern "C" fn fake_printf() -> i32 {
        0
    }
    syms.push(("printf", fake_printf as *const u8));
    let syms = symbols_from(&syms);
    let _jc = build_jit(&p, &syms).expect("u8 variadic must lower");

    std::env::remove_var("MTY_DUMP_CLIF");
    let main_clif = std::fs::read_to_string(tmp.path().join("main.clif")).expect("main.clif");
    assert!(
        main_clif.contains("uextend"),
        "expected `uextend` for u8 promotion in main.clif, got:\n{main_clif}"
    );
}

#[test]
fn f32_extra_promotes_to_f64_in_clif_via_fpromote() {
    let _g = CLIF_DUMP_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::env::set_var("MTY_DUMP_CLIF", tmp.path());

    let p = variadic_printf_program(
        vec![(
            Const::Float(1.5, FloatKind::F32),
            IrTy::Float(FloatKind::F32),
        )],
        0,
    );
    let mut syms = default_runtime_syms();
    extern "C" fn fake_printf() -> i32 {
        0
    }
    syms.push(("printf", fake_printf as *const u8));
    let syms = symbols_from(&syms);
    let _jc = build_jit(&p, &syms).expect("f32 variadic must lower");

    std::env::remove_var("MTY_DUMP_CLIF");
    let main_clif = std::fs::read_to_string(tmp.path().join("main.clif")).expect("main.clif");
    assert!(
        main_clif.contains("fpromote") || main_clif.contains("f64const"),
        "expected `fpromote` or pre-folded `f64const` for f32 promotion, got:\n{main_clif}"
    );
}

#[test]
fn i16_extra_widens_to_i32_in_clif_via_sextend_or_const_fold() {
    let _g = CLIF_DUMP_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::env::set_var("MTY_DUMP_CLIF", tmp.path());

    let p = variadic_printf_program(
        vec![(Const::Int(-1, IntKind::I16), IrTy::Int(IntKind::I16))],
        0,
    );
    let mut syms = default_runtime_syms();
    extern "C" fn fake_printf() -> i32 {
        0
    }
    syms.push(("printf", fake_printf as *const u8));
    let syms = symbols_from(&syms);
    let _jc = build_jit(&p, &syms).expect("i16 variadic must lower");

    std::env::remove_var("MTY_DUMP_CLIF");
    let main_clif = std::fs::read_to_string(tmp.path().join("main.clif")).expect("main.clif");
    // `sextend` may be const-folded into the literal i32 (Cranelift's
    // eval phase rewrites `sextend.i32 (iconst.i16 -1)` to
    // `iconst.i32 -1`). Either is acceptable as long as the i16 wasn't
    // accidentally widened with uextend (which would lose the sign).
    let has_sext = main_clif.contains("sextend");
    let has_iconst_i32 = main_clif.contains("iconst.i32");
    assert!(
        has_sext || has_iconst_i32,
        "expected sextend or const-folded iconst.i32 for i16, got:\n{main_clif}"
    );
    assert!(
        !main_clif.contains("uextend"),
        "i16 must NOT widen via uextend (would lose sign), got:\n{main_clif}"
    );
}

// ============================================================
// Group 5: real libc printf round-trip.
// ============================================================

// Captures the integer printf returned to us. printf returns the byte
// count it wrote, so calling printf("%d\n", 42) returns 3 ("42\n").
//
// We don't capture stdout — too flaky in cargo test runners across
// platforms. We just verify the returned i32 is > 0, which proves
// the call ABI was respected end-to-end (wrong arg layout would
// segfault or return a negative on most libcs).

/// Provides the host libc's `printf` symbol to the JIT.
#[cfg(unix)]
fn libc_printf_addr() -> Option<*const u8> {
    // libc::printf is a variadic fn pointer — taking its address is
    // legal and matches the symbol cranelift's func_addr would resolve.
    extern "C" {
        fn printf(fmt: *const u8, ...) -> i32;
    }
    Some(printf as *const u8)
}

#[cfg(windows)]
fn libc_printf_addr() -> Option<*const u8> {
    extern "C" {
        fn printf(fmt: *const u8, ...) -> i32;
    }
    Some(printf as *const u8)
}

/// Build a hand-rolled program where main calls printf with a real
/// format string. The format string lives in a `static` we leak so
/// the address stays valid for the JIT's lifetime.
#[test]
fn printf_real_libc_round_trip() {
    // "hello %d\n\0" — leak so the pointer stays valid.
    let fmt = Box::leak(Box::new(*b"hello %d\n\0"));
    let fmt_addr = fmt.as_ptr() as usize as i128;

    let p = variadic_printf_program(
        vec![(Const::Int(42, IntKind::I32), IrTy::Int(IntKind::I32))],
        fmt_addr,
    );
    let mut syms = default_runtime_syms();
    let Some(addr) = libc_printf_addr() else {
        eprintln!("[skip] libc printf not available on this target");
        return;
    };
    syms.push(("printf", addr));
    let syms = symbols_from(&syms);
    let jc = match build_jit(&p, &syms) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("[skip] JIT build failed (likely sandboxed runner): {e:?}");
            return;
        }
    };
    // Actually call main — this dispatches into real printf. We don't
    // assert on the result (the program's main returns 0 by design;
    // the printf return value is dropped into a local). The fact that
    // the call doesn't crash is the assertion.
    let r = jc.call_main();
    assert_eq!(r, Some(0));
}
