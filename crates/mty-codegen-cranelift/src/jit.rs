//! JIT driver (slice 8).
//!
//! Wraps `cranelift_jit::JITModule`, links runtime symbols, lowers
//! every fn in the SIR `Program`, finalizes, and hands back a fn-ptr
//! to `main` (if present).

use crate::error::{CodegenError, CompileResult};
use crate::lower::{default_flags, LowerCtx};
use cranelift_codegen::isa::{self};
use cranelift_jit::{JITBuilder, JITModule};
#[allow(unused_imports)]
use cranelift_module::Module;
use mty_ir::ir::Program;
use std::collections::HashMap;
use target_lexicon::Triple;

/// One JIT-compiled program. Drop = unload all generated code, so the
/// caller must hold this alive for as long as it intends to call the
/// returned fn-ptr.
pub struct JitCompiled {
    pub module: JITModule,
    /// Address of `main` if the program defined one, as a raw byte
    /// pointer. The caller must `transmute` it to the correct signature.
    pub main_ptr: Option<*const u8>,
    /// True when codegen succeeded with no fallback paths.
    pub fully_compiled: bool,
    /// True if `main` returns an integer (compiled via JitMainI64);
    /// false for Unit-returning main (compiled via JitMain).
    pub main_returns_int: bool,
}

/// Convenience newtype for a `main()` fn pointer (no return).
pub type JitMain = extern "C" fn();
/// Variant used when the program declared a `-> Int` main.
pub type JitMainI64 = extern "C" fn() -> i64;

/// v0.36 Track T2 — default no-op resolver for extern fns the JIT
/// caller didn't provide. Returns 0 so callers that read the i64
/// return value see a deterministic placeholder.
///
/// This is the JIT-side analogue of an empty stub body: the AOT path
/// resolves these from the manifest's `[[extern_lib]]` archives, but
/// JIT can't link archives so we provide a synthetic body instead.
/// Variadic signatures don't reach this path — the extern_c_matrix
/// doc lists them as v0.37 follow-up.
extern "C" fn jit_extern_default_trap() -> i64 {
    0
}

/// Build a JIT module and lower every function in `prog`. The runtime
/// symbol table is registered via `symbols`; pass the runtime's
/// `register_with` helper to populate it.
pub fn build_jit(prog: &Program, symbols: &[(String, *const u8)]) -> CompileResult<JitCompiled> {
    let triple = Triple::host();
    let isa_builder = isa::lookup(triple.clone())
        .map_err(|e| CodegenError::Module(format!("isa lookup: {e}")))?;
    let flags = default_flags(false); // cranelift-jit requires non-PIC
    let isa = isa_builder
        .finish(flags)
        .map_err(|e| CodegenError::Module(format!("isa finish: {e}")))?;

    let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
    for (name, addr) in symbols {
        builder.symbol(name.clone(), *addr);
    }
    // v0.36 Track T2 — extern c fns now get `Linkage::Import` (so the
    // AOT linker can resolve them from a vendored archive). The JIT
    // path has no archive to consult; any extern fn the user actually
    // calls would fail with "can't resolve symbol". Provide a default
    // no-op trap for every extern binding so JIT execution stays
    // tolerant — same behaviour as the pre-v0.36 stub path. Callers
    // who want a real implementation can override by passing the
    // symbol through `symbols`.
    for binding in prog.extern_bindings.values() {
        // Skip if the caller already provided a real impl.
        if symbols.iter().any(|(n, _)| n == &binding.name) {
            continue;
        }
        builder.symbol(binding.name.clone(), jit_extern_default_trap as *const u8);
    }
    let mut module = JITModule::new(builder);

    let mut ctx = LowerCtx::new(&mut module, triple);
    ctx.declare_fns(prog)?;

    let mut fully = true;
    let mut errors: Vec<(String, CodegenError)> = Vec::new();
    for f in &prog.fns {
        if let Err(e) = ctx.define_fn(prog, f) {
            fully = false;
            errors.push((f.name.clone(), e));
            // Continue — we'll fall back to interpreter for fns that
            // failed to compile. But cranelift requires all declared
            // fns to be defined before finalize, so we re-declare as
            // a trap-stub.
            // (Slice-8 simplification: if anything fails, we treat the
            // whole program as not-fully-compiled; the driver chooses
            // whether to interpret.)
        }
    }

    // If we failed to define anything, bail before finalize so cranelift
    // doesn't crash on an undefined-fn assertion.
    if !errors.is_empty() {
        // Surface the first error so the driver can decide.
        let (name, err) = errors.into_iter().next().unwrap();
        return Err(CodegenError::Unsupported(format!("fn `{name}`: {err}")));
    }

    // Capture main's FuncId before consuming ctx.
    let main_fn_id = prog
        .fn_by_name("main")
        .and_then(|f| ctx.fn_ids.get(&f.id).copied());

    // Drop ctx (releases its &mut module).
    drop(ctx);

    module
        .finalize_definitions()
        .map_err(|e| CodegenError::Module(format!("finalize: {e}")))?;

    let main_ptr = main_fn_id.map(|fid| module.get_finalized_function(fid));
    // Capture main's return type so call_main knows which transmute
    // shape to use.
    let main_returns_int = prog
        .fn_by_name("main")
        .map(|f| !matches!(f.ret_ty, mty_ir::ir::IrTy::Unit | mty_ir::ir::IrTy::Never))
        .unwrap_or(false);
    Ok(JitCompiled {
        module,
        main_ptr,
        fully_compiled: fully,
        main_returns_int,
    })
}

impl JitCompiled {
    /// Call `main` if present. Returns the int-coerced return value,
    /// or 0 for a Unit-returning main.
    pub fn call_main(&self) -> Option<i64> {
        let p = self.main_ptr?;
        if self.main_returns_int {
            let f: JitMainI64 = unsafe { std::mem::transmute(p) };
            Some(f())
        } else {
            let f: JitMain = unsafe { std::mem::transmute(p) };
            f();
            Some(0)
        }
    }
}

// SAFETY: `JITModule` holds raw pointers; we promise to keep it alive
// alongside any fn-ptrs we hand out. JitCompiled is not Send across
// threads in slice-8 (each codegen run owns its own module).
unsafe impl Send for JitCompiled {}

/// Compile and immediately run `main`, returning its exit code.
/// Convenience for `mty run` integration tests.
pub fn jit_compile_and_run_main(
    prog: &Program,
    symbols: &[(String, *const u8)],
) -> CompileResult<i64> {
    let jc = build_jit(prog, symbols)?;
    Ok(jc.call_main().unwrap_or(0))
}

/// Build a symbol table mapping from a slice of (name, raw fn-ptr).
/// Helper for callers that have addresses but want them as the
/// `(String, *const u8)` shape `JITBuilder::symbol` expects.
pub fn symbols_from(pairs: &[(&str, *const u8)]) -> Vec<(String, *const u8)> {
    pairs.iter().map(|(n, p)| (n.to_string(), *p)).collect()
}

// Dummy placeholder to suppress unused warning when no fn-ids accessed.
#[allow(dead_code)]
fn _unused_helper(_: &HashMap<u32, u32>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_hir::SourceSpan;
    use mty_ir::ir::{
        Block, BlockId, Const, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Program,
        Term,
    };

    extern "C" fn no_op_log(_p: i64, _l: i64) {}
    extern "C" fn no_op_print(_p: i64, _l: i64) {}
    extern "C" fn no_op_panic(_p: i64, _l: i64) {}

    fn empty_main_program() -> Program {
        let mut p = Program::default();
        p.fns.push(Function {
            id: IrFnId(0),
            name: "main".into(),
            params: vec![],
            locals: vec![LocalDecl {
                name: "_0".into(),
                ty: IrTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            }],
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![],
                terminator: Term::Return(Operand::Const(Const::Unit)),
            }],
            entry: BlockId(0),
            ret_ty: IrTy::Unit,
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 0, end: 0 },
        });
        p
    }

    #[test]
    fn empty_main_compiles_and_runs() {
        let p = empty_main_program();
        let syms = symbols_from(&[
            ("mty_runtime_log", no_op_log as *const u8),
            ("mty_runtime_print", no_op_print as *const u8),
            ("mty_runtime_panic", no_op_panic as *const u8),
            ("mty_runtime_arena_push", no_op_log as *const u8),
            ("mty_runtime_arena_pop", no_op_log as *const u8),
            ("mty_runtime_alloc", no_op_log as *const u8),
            ("mty_runtime_budget_charge", no_op_log as *const u8),
            ("mty_runtime_send", no_op_log as *const u8),
            ("mty_runtime_ask", no_op_log as *const u8),
            ("mty_runtime_spawn", no_op_log as *const u8),
            ("mty_runtime_extern_call", no_op_log as *const u8),
            ("mty_runtime_log_i64", no_op_log as *const u8),
        ]);
        let jc = build_jit(&p, &syms).expect("jit build");
        assert!(jc.main_ptr.is_some());
        let _ = jc.call_main();
    }
}
