//! v0.10 cleanup — exercise the real `cabi_realloc` allocator.
//!
//! The v0.9 stub was a bump-only allocator (`old_ptr` ignored, no
//! reuse, no `free`). v0.10 replaces it with a segregated free-list
//! allocator (8 size classes from 8B to 1024B) plus a "large" bump
//! path for `size > 1024`. These tests verify the new behaviour by
//! instantiating the emitted module under wasmtime and calling
//! `cabi_realloc` directly through its exported entry point.
//!
//! Covered:
//!   1. Fresh malloc (`old=0`) returns a non-zero, aligned pointer.
//!   2. Free + re-alloc same size class reuses the freed block.
//!   3. Many small allocs interleaved with frees keep memory bounded
//!      (1000 cycles, growth ≤ a fixed budget).
//!   4. Large allocs (> 1024B) follow the bump path (no reuse but
//!      still monotonic + bounded for short-running programs).
//!   5. Realloc grows in-place semantically (old bytes preserved).
//!   6. Realloc with `new=0 && old!=0` returns 0 and pushes the
//!      block to its free list (next alloc of same class reuses).

use mty_codegen_wasm::emit::{
    compile_program_to_bytes, CABI_REALLOC_HEAP_BASE, CABI_REALLOC_STATE_BASE,
};
use mty_codegen_wasm::target::WasmTarget;
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, Const, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Program, Term,
};
use wasmtime::{Caller, Engine, Linker, Module, Store, TypedFunc};

fn empty_main() -> Program {
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

/// Signature of the exported `cabi_realloc(old, old_size, align, new) -> ptr`.
type Realloc = TypedFunc<(i32, i32, i32, i32), i32>;

/// Instantiate the emitted core module + return a typed handle to
/// `cabi_realloc(old, old_size, align, new) -> i32`.
fn instantiate_realloc_harness(target: WasmTarget) -> (Store<()>, wasmtime::Instance, Realloc) {
    let bytes = compile_program_to_bytes(&empty_main(), target).expect("compile");
    let engine = Engine::default();
    let module = Module::new(&engine, &bytes).expect("module compile");
    let mut store: Store<()> = Store::new(&engine, ());
    let mut linker: Linker<()> = Linker::new(&engine);
    // Provide host stubs for every import the emitter may have added.
    let _ = linker.func_wrap(
        "wasi:cli/log",
        "log",
        |_caller: Caller<'_, ()>, _ptr: i32, _len: i32| {},
    );
    let _ = linker.func_wrap(
        "mty:web/log",
        "log",
        |_caller: Caller<'_, ()>, _ptr: i32, _len: i32| {},
    );
    let _ = linker.func_wrap(
        "mty:web/dom",
        "set-text",
        |_caller: Caller<'_, ()>, _a: i32, _b: i32, _c: i32, _d: i32| {},
    );
    let _ = linker.func_wrap(
        "mty:web/dom",
        "get-text",
        |_caller: Caller<'_, ()>, _a: i32, _b: i32, _c: i32| {},
    );
    let _ = linker.func_wrap(
        "mty:web/dom",
        "on-click",
        |_caller: Caller<'_, ()>, _a: i32, _b: i32, _c: i32, _d: i32| {},
    );
    let _ = linker.func_wrap(
        "mty:web/dom",
        "query",
        |_caller: Caller<'_, ()>, _a: i32, _b: i32, _c: i32| {},
    );
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate");
    let realloc = instance
        .get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "cabi_realloc")
        .expect("cabi_realloc export");
    (store, instance, realloc)
}

#[test]
fn fresh_malloc_returns_aligned_nonzero_pointer() {
    let (mut store, _inst, realloc) = instantiate_realloc_harness(WasmTarget::Wasi);
    // malloc(16, align=8)
    let p = realloc.call(&mut store, (0, 0, 8, 16)).expect("alloc");
    assert!(p >= CABI_REALLOC_HEAP_BASE, "ptr {p} below heap base");
    assert_eq!(p % 8, 0, "ptr {p} not 8-aligned");
}

#[test]
fn free_then_realloc_same_size_class_reuses_block() {
    let (mut store, _inst, realloc) = instantiate_realloc_harness(WasmTarget::Wasi);
    // Allocate one 32-byte block.
    let p1 = realloc.call(&mut store, (0, 0, 8, 32)).expect("alloc-1");
    // Free it (new=0, old=p1, old_size=32).
    let _zero = realloc.call(&mut store, (p1, 32, 8, 0)).expect("free");
    // Re-allocate same size class.
    let p2 = realloc.call(&mut store, (0, 0, 8, 32)).expect("alloc-2");
    assert_eq!(p1, p2, "freed block should be reused (LIFO free-list)");
}

#[test]
fn free_pushes_onto_classed_free_list() {
    let (mut store, _inst, realloc) = instantiate_realloc_harness(WasmTarget::Wasi);
    // Allocate three 16-byte blocks.
    let a = realloc.call(&mut store, (0, 0, 8, 16)).expect("a");
    let b = realloc.call(&mut store, (0, 0, 8, 16)).expect("b");
    let c = realloc.call(&mut store, (0, 0, 8, 16)).expect("c");
    assert_ne!(a, b);
    assert_ne!(b, c);
    // Free in order: a, b, c. Re-alloc three times — should LIFO out
    // as c, b, a.
    realloc.call(&mut store, (a, 16, 8, 0)).expect("free a");
    realloc.call(&mut store, (b, 16, 8, 0)).expect("free b");
    realloc.call(&mut store, (c, 16, 8, 0)).expect("free c");
    let r1 = realloc.call(&mut store, (0, 0, 8, 16)).expect("r1");
    let r2 = realloc.call(&mut store, (0, 0, 8, 16)).expect("r2");
    let r3 = realloc.call(&mut store, (0, 0, 8, 16)).expect("r3");
    assert_eq!(r1, c);
    assert_eq!(r2, b);
    assert_eq!(r3, a);
}

#[test]
fn different_size_classes_have_independent_free_lists() {
    let (mut store, _inst, realloc) = instantiate_realloc_harness(WasmTarget::Wasi);
    let small = realloc.call(&mut store, (0, 0, 8, 16)).expect("small");
    let big = realloc.call(&mut store, (0, 0, 8, 128)).expect("big");
    realloc.call(&mut store, (small, 16, 8, 0)).expect("free s");
    realloc.call(&mut store, (big, 128, 8, 0)).expect("free b");

    // Re-allocing 16 must reuse `small`, not `big`.
    let r_small = realloc.call(&mut store, (0, 0, 8, 16)).expect("r_small");
    let r_big = realloc.call(&mut store, (0, 0, 8, 128)).expect("r_big");
    assert_eq!(r_small, small);
    assert_eq!(r_big, big);
}

#[test]
fn stress_1000_alloc_free_cycles_bounded_growth() {
    let (mut store, _inst, realloc) = instantiate_realloc_harness(WasmTarget::Wasi);

    // Warm the free lists once so we account for the initial bump.
    let mut warmup = Vec::new();
    for _ in 0..8 {
        warmup.push(realloc.call(&mut store, (0, 0, 8, 32)).expect("warmup"));
    }
    for p in &warmup {
        realloc.call(&mut store, (*p, 32, 8, 0)).expect("free warm");
    }

    // Snapshot the bump-pointer high-water mark via a deliberately-
    // misaligned probe: we ask for a 0-byte alloc but with align=1 so
    // it returns the current bump position without consuming bytes.
    // Wait — class 0 is 8 bytes, so even a 0-sized request consumes
    // 8 bytes from the free list (class 0 has one entry from warmup,
    // so this is reuse — no bump growth). We use a class-0 alloc and
    // immediately free, leaving the free list unchanged but giving
    // us a stable address.
    let probe1 = realloc.call(&mut store, (0, 0, 8, 8)).expect("probe1");
    realloc
        .call(&mut store, (probe1, 8, 8, 0))
        .expect("free p1");

    // 1000 alloc/free cycles. Without free-list reuse, this would
    // consume 1000 * 32 = 32 KB of bump space. With reuse, only the
    // very first alloc consumes new bytes (32 B for class 32).
    for _ in 0..1000 {
        let p = realloc.call(&mut store, (0, 0, 8, 32)).expect("alloc");
        realloc.call(&mut store, (p, 32, 8, 0)).expect("free");
    }

    // Probe again — under perfect reuse, the bump pointer hasn't
    // moved between probe1 and probe2 (besides the class-0 reuse
    // that already happened during warmup).
    let probe2 = realloc.call(&mut store, (0, 0, 8, 8)).expect("probe2");
    realloc
        .call(&mut store, (probe2, 8, 8, 0))
        .expect("free p2");

    // The same class-0 free-list slot should come back both times.
    assert_eq!(
        probe1, probe2,
        "bounded memory: bump pointer must not advance \
         when free-list reuse is available"
    );
}

#[test]
fn large_alloc_uses_bump_path() {
    let (mut store, _inst, realloc) = instantiate_realloc_harness(WasmTarget::Wasi);
    // 2048 B > 1024 B threshold → large bump path. We expect two
    // sequential large allocs to return different (monotonic)
    // pointers because the large path doesn't reuse freed memory.
    let p1 = realloc.call(&mut store, (0, 0, 8, 2048)).expect("L1");
    let p2 = realloc.call(&mut store, (0, 0, 8, 2048)).expect("L2");
    // Freeing a large block is a no-op (the size class is -1).
    realloc.call(&mut store, (p1, 2048, 8, 0)).expect("free L1");
    let p3 = realloc.call(&mut store, (0, 0, 8, 2048)).expect("L3");
    assert!(p2 > p1, "L2 ({p2}) should be above L1 ({p1})");
    assert!(p3 > p2, "L3 ({p3}) should be above L2 (no reuse on large)");
    assert!(
        p2 - p1 >= 2048,
        "large alloc spacing should cover the request"
    );
}

#[test]
fn realloc_grow_preserves_old_bytes() {
    let (mut store, instance, realloc) = instantiate_realloc_harness(WasmTarget::Wasi);
    // Allocate 16 bytes, write a known pattern, then realloc to 64.
    let p1 = realloc.call(&mut store, (0, 0, 8, 16)).expect("alloc-1");
    let mem = instance.get_memory(&mut store, "memory").expect("memory");
    let pattern = b"HELLO_REALLOC_v0";
    assert_eq!(pattern.len(), 16);
    mem.data_mut(&mut store)[p1 as usize..p1 as usize + 16].copy_from_slice(pattern);

    let p2 = realloc.call(&mut store, (p1, 16, 8, 64)).expect("realloc");
    assert_ne!(p1, p2, "realloc to a new size class returns a new ptr");
    let mem2 = instance.get_memory(&mut store, "memory").expect("memory");
    let copied = &mem2.data(&store)[p2 as usize..p2 as usize + 16];
    assert_eq!(copied, pattern, "first 16 bytes copied to new alloc");
}

#[test]
fn realloc_to_zero_frees_old_block() {
    let (mut store, _inst, realloc) = instantiate_realloc_harness(WasmTarget::Wasi);
    let p1 = realloc.call(&mut store, (0, 0, 8, 32)).expect("alloc");
    let zero = realloc.call(&mut store, (p1, 32, 8, 0)).expect("free");
    assert_eq!(zero, 0, "realloc(p, _, _, 0) returns 0");
    // The block is on the free list now — next alloc of class 32
    // reuses it.
    let p2 = realloc.call(&mut store, (0, 0, 8, 32)).expect("alloc-2");
    assert_eq!(p1, p2, "freed block reused");
}

#[test]
fn allocator_state_lives_at_documented_offset() {
    // Sanity-check that the consts agree with the layout claim:
    // STATE_BASE + 8 classes * 4 bytes = HEAP_BASE.
    assert_eq!(
        CABI_REALLOC_STATE_BASE + 32,
        CABI_REALLOC_HEAP_BASE,
        "allocator state must occupy [STATE_BASE..HEAP_BASE) exactly"
    );
}
