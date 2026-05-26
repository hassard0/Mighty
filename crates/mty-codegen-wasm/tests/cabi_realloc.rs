//! v0.18 — focused test suite for the extracted `cabi_realloc`
//! allocator module (`crate::cabi_realloc`).
//!
//! KNOWN_ISSUES #1 asked for a real free-list (or buddy) allocator
//! plus a dedicated test file under
//! `crates/mty-codegen-wasm/tests/cabi_realloc.rs`. The free-list
//! allocator itself was shipped in v0.10 (see
//! [`tests/cabi_realloc_real.rs`] for that suite); v0.18 extracts
//! the body builder into `crate::cabi_realloc` and adds these
//! additional scenarios that exercise the v0.18-mandated coverage:
//!
//! - `alloc_returns_distinct_pointers` — 100 allocations don't
//!   overlap.
//! - `free_then_alloc_reuses_slot` — alloc + free + alloc returns
//!   the freed region.
//! - `realloc_grow_in_place_if_possible` — 16-byte alloc reallocs
//!   to 32 either in place or moved; old bytes preserved.
//! - `realloc_shrink_in_place` — 64-byte alloc shrinks to 32
//!   without leaking the slot (the surplus stays accessible to
//!   later allocs).
//! - `alloc_after_many_frees_doesnt_grow_memory` — 100 alloc/free
//!   pairs leave the linear-memory size unchanged.
//! - `bump_fallback_when_freelist_empty` — initial allocs go
//!   through the bump path (no free-list slot to reuse yet).
//! - `module_emission_is_deterministic` — emitting the same SIR
//!   program twice yields byte-identical wasm modules.
//! - `state_region_initialised_to_zero` — every free-list head
//!   starts at 0 before the first alloc.
//!
//! These tests instantiate the compiled wasm with `wasmtime` (an
//! existing dev-dep) and exercise `cabi_realloc` directly.

use mty_codegen_wasm::cabi_realloc::{
    CABI_REALLOC_HEAP_BASE, CABI_REALLOC_NUM_CLASSES, CABI_REALLOC_STATE_BASE,
};
use mty_codegen_wasm::emit::compile_program_to_bytes;
use mty_codegen_wasm::target::WasmTarget;
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, Const, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Program, Term,
};
use wasmtime::{Caller, Engine, Linker, Module, Store, TypedFunc};

/// Minimal SIR program — one empty `main` — so the emitter has
/// something to encode + the resulting module exports `cabi_realloc`.
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

/// Compile the empty-main program + return a wasmtime harness with a
/// typed handle to `cabi_realloc`.
fn harness() -> (Store<()>, wasmtime::Instance, Realloc) {
    let bytes = compile_program_to_bytes(&empty_main(), WasmTarget::Wasi).expect("compile");
    let engine = Engine::default();
    let module = Module::new(&engine, &bytes).expect("module compile");
    let mut store: Store<()> = Store::new(&engine, ());
    let mut linker: Linker<()> = Linker::new(&engine);
    // The Wasi target's `log` is the only import the empty-main
    // module declares. Stub it out so the instantiation succeeds.
    let _ = linker.func_wrap(
        "wasi:cli/log",
        "log",
        |_caller: Caller<'_, ()>, _ptr: i32, _len: i32| {},
    );
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate");
    let realloc = instance
        .get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "cabi_realloc")
        .expect("cabi_realloc export");
    (store, instance, realloc)
}

/// 100 distinct allocations of varied sizes must return pointers that
/// don't overlap (no two pointers share the same byte range). This
/// catches both bump-pointer regression (overlapping due to forgotten
/// advance) and free-list mis-pop (returning the same head twice).
#[test]
fn alloc_returns_distinct_pointers() {
    let (mut store, _inst, realloc) = harness();
    // Vary size across classes to exercise multiple free lists.
    let sizes = [8, 16, 24, 32, 48, 64, 96, 128, 200, 256];
    let mut allocs: Vec<(i32, i32)> = Vec::new(); // (ptr, size)
    for i in 0..100 {
        let size = sizes[i % sizes.len()];
        let p = realloc.call(&mut store, (0, 0, 8, size)).expect("alloc");
        assert!(p >= CABI_REALLOC_HEAP_BASE, "ptr below heap base: {p}");
        allocs.push((p, size));
    }
    // O(n^2) overlap check — n=100 is fine.
    for (i, &(p1, s1)) in allocs.iter().enumerate() {
        let end1 = p1 + s1;
        for &(p2, s2) in &allocs[i + 1..] {
            let end2 = p2 + s2;
            // Disjoint if end1 <= p2 OR end2 <= p1.
            let disjoint = end1 <= p2 || end2 <= p1;
            assert!(disjoint, "overlap: [{p1},{end1}) and [{p2},{end2})");
        }
    }
}

/// Allocate -> free -> allocate-same-size must return the same
/// pointer. This is the canonical Component-Model recycle pattern
/// (lift string, use, drop, lift next string).
#[test]
fn free_then_alloc_reuses_slot() {
    let (mut store, _inst, realloc) = harness();
    let p1 = realloc.call(&mut store, (0, 0, 8, 32)).expect("alloc-1");
    realloc.call(&mut store, (p1, 32, 8, 0)).expect("free");
    let p2 = realloc.call(&mut store, (0, 0, 8, 32)).expect("alloc-2");
    assert_eq!(p1, p2, "freed slot must be reused (LIFO free-list)");
}

/// Realloc 16 -> 32 must either keep the same pointer (in-place
/// grow if the size class can absorb it) or move + preserve the
/// old bytes. Today's allocator always returns a new pointer when
/// the size class changes (16->32 crosses class 1 -> class 2), but
/// the contract is "old bytes preserved either way".
#[test]
fn realloc_grow_in_place_if_possible() {
    let (mut store, instance, realloc) = harness();
    // Allocate 16 bytes, stamp a sentinel, realloc to 32.
    let p1 = realloc.call(&mut store, (0, 0, 8, 16)).expect("alloc-1");
    let mem = instance.get_memory(&mut store, "memory").expect("memory");
    let sentinel = b"GROW_IN_PLACE_v1";
    assert_eq!(sentinel.len(), 16);
    mem.data_mut(&mut store)[p1 as usize..p1 as usize + 16].copy_from_slice(sentinel);

    let p2 = realloc.call(&mut store, (p1, 16, 8, 32)).expect("realloc");
    // Either same pointer (in-place) or distinct + copied bytes.
    let mem = instance.get_memory(&mut store, "memory").expect("memory");
    let dst = &mem.data(&store)[p2 as usize..p2 as usize + 16];
    assert_eq!(
        dst, sentinel,
        "first 16 bytes preserved across realloc (p1={p1} p2={p2})"
    );
}

/// Realloc 64 -> 32: the canonical-ABI contract is that the result
/// pointer must be valid for at least `new` bytes. Today's allocator
/// always re-malloc/copy/free even on shrinks, so p1 != p2, but the
/// shrunk region's bytes must match the original. Tests verifies
/// data preservation without locking us into "same pointer" — that's
/// an implementation detail.
#[test]
fn realloc_shrink_in_place() {
    let (mut store, instance, realloc) = harness();
    let p1 = realloc.call(&mut store, (0, 0, 8, 64)).expect("alloc-64");
    let mem = instance.get_memory(&mut store, "memory").expect("memory");
    let sentinel: Vec<u8> = (0..64u8).collect();
    mem.data_mut(&mut store)[p1 as usize..p1 as usize + 64].copy_from_slice(&sentinel);

    let p2 = realloc.call(&mut store, (p1, 64, 8, 32)).expect("shrink");
    let mem = instance.get_memory(&mut store, "memory").expect("memory");
    let kept = &mem.data(&store)[p2 as usize..p2 as usize + 32];
    assert_eq!(
        kept,
        &sentinel[..32],
        "first 32 bytes preserved across shrink (p1={p1} p2={p2})"
    );
    // The old block must be reusable for the next alloc of class 64.
    let p3 = realloc.call(&mut store, (0, 0, 8, 64)).expect("alloc-3");
    assert_eq!(
        p3, p1,
        "shrinking should free the old class-64 slot for reuse"
    );
}

/// 100 alloc/free pairs in a loop must NOT advance the bump
/// pointer past the first allocation. The 16-page (1 MiB) initial
/// linear memory is plenty; the assertion is that we don't grow it.
#[test]
fn alloc_after_many_frees_doesnt_grow_memory() {
    let (mut store, instance, realloc) = harness();
    let mem = instance.get_memory(&mut store, "memory").expect("memory");
    let initial_pages = mem.size(&store);

    for _ in 0..100 {
        let p = realloc.call(&mut store, (0, 0, 8, 64)).expect("alloc");
        realloc.call(&mut store, (p, 64, 8, 0)).expect("free");
    }

    let mem = instance.get_memory(&mut store, "memory").expect("memory");
    let final_pages = mem.size(&store);
    assert_eq!(
        final_pages, initial_pages,
        "100 alloc/free cycles must not grow linear memory \
         (initial={initial_pages} final={final_pages})"
    );
}

/// The first allocation must come straight out of the bump pointer
/// (no slot to reuse). Specifically, the returned pointer must be
/// exactly [`CABI_REALLOC_HEAP_BASE`] (modulo alignment rounding).
#[test]
fn bump_fallback_when_freelist_empty() {
    let (mut store, _inst, realloc) = harness();
    // The first alloc must come from the bump path. Alignment is 8,
    // and HEAP_BASE is already 8-aligned (32800 % 8 == 0), so the
    // pointer must equal HEAP_BASE exactly.
    let p = realloc.call(&mut store, (0, 0, 8, 32)).expect("alloc");
    assert_eq!(
        p, CABI_REALLOC_HEAP_BASE,
        "first alloc (bump) should land at HEAP_BASE"
    );
}

/// Emitting the same SIR program twice must produce byte-identical
/// modules — the allocator's body builder is deterministic, the
/// emitter's section ordering is deterministic, and `wasm-encoder`
/// is deterministic.
#[test]
fn module_emission_is_deterministic() {
    let a = compile_program_to_bytes(&empty_main(), WasmTarget::Wasi).expect("compile-a");
    let b = compile_program_to_bytes(&empty_main(), WasmTarget::Wasi).expect("compile-b");
    assert_eq!(
        a, b,
        "same SIR program must yield byte-identical wasm \
         (cabi_realloc emission must be deterministic)"
    );
    // Sanity: the module must be non-trivial in size — if both
    // builds came out empty/identical-by-trivial-coincidence this
    // catches it.
    assert!(
        a.len() > 256,
        "module unexpectedly small: {} bytes",
        a.len()
    );
}

/// Each free-list head slot in [`CABI_REALLOC_STATE_BASE`..)
/// starts at 0 (empty list). This is the implicit-init guarantee:
/// wasm linear memory pages are zero-filled, so the state region
/// is already in the "no freed blocks" state at module-load time.
#[test]
fn state_region_initialised_to_zero() {
    let (mut store, instance, _realloc) = harness();
    let mem = instance.get_memory(&mut store, "memory").expect("memory");
    let base = CABI_REALLOC_STATE_BASE as usize;
    let end = base + (CABI_REALLOC_NUM_CLASSES as usize) * 4;
    let region = &mem.data(&store)[base..end];
    for (i, byte) in region.iter().enumerate() {
        assert_eq!(
            *byte, 0,
            "state region must be zero-initialised; byte {i} = {byte}"
        );
    }
}
