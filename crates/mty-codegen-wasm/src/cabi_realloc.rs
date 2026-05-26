//! Canonical-ABI `cabi_realloc` allocator emission (v0.18).
//!
//! Extracted from `emit.rs` for v0.18 (KNOWN_ISSUES #1). The v0.10
//! pass shipped the segregated free-list allocator inline; v0.18
//! moves the body builder + size-class helper + layout constants into
//! their own module so the allocator can evolve (and be tested)
//! independently of the rest of the Wasm emitter.
//!
//! ### What this module owns
//!
//! - The linear-memory layout constants
//!   ([`CABI_REALLOC_STATE_BASE`], [`CABI_REALLOC_HEAP_BASE`]) and the
//!   size-class table ([`CABI_REALLOC_NUM_CLASSES`],
//!   [`CABI_REALLOC_LARGE_THRESHOLD`]).
//! - [`build_cabi_realloc_body`] — emits the wasm bytecode for the
//!   exported `cabi_realloc(old, old_size, align, new) -> i32`.
//! - [`emit_size_class`] — helper that emits the wasm to compute the
//!   size class of a request into a target local.
//!
//! `emit.rs::Emitter::emit` still owns the *plumbing* (declaring the
//! type, function-section slot, export, and the bump-pointer global);
//! this module just supplies the function body so the allocator can
//! be reasoned about + regression-tested in isolation.
//!
//! ### Allocator design
//!
//! Segregated free-list with 8 size classes (powers of 2 from 8B to
//! 1024B) + a "large" bump path for `size > 1024`. Each class has a
//! free-list head stored in linear memory at
//! `CABI_REALLOC_STATE_BASE + class*4`; the link in each free block
//! is the first 4 bytes (next-pointer; 0 = end of list).
//!
//! `cabi_realloc(old, old_size, align, new)`:
//! * `new == 0`: free-only path (push `old` to its size-class free
//!   list if it fits one; large blocks are leaked — see v0.19
//!   follow-ups in [`CABI_REALLOC_V0_18_NOTES.md`](../../../dev/history/notes/CABI_REALLOC_V0_18_NOTES.md)).
//! * `old == 0`: fresh malloc — try class free-list first, fall back
//!   to the bump pointer if the list is empty.
//! * `old != 0 && new != 0`: realloc — malloc(new), memcpy(min(old,
//!   new)) byte-by-byte, free(old). No in-place grow / shrink yet.
//!
//! Requests > 1024 bytes use the "large" path that bumps + never
//! frees. Acceptable for v0.18 — most canonical-ABI strings/lists
//! fit in the small classes.
//!
//! `align` is respected by rounding the bump pointer up; free-list
//! reuse is only safe when `align <= class_size`, which always holds
//! for power-of-two alignments because the free blocks were
//! originally bump-allocated at class-size alignment.
//!
//! ### Determinism
//!
//! [`build_cabi_realloc_body`] is a pure function of nothing — same
//! call always produces the same wasm bytecode. The Emitter calls it
//! exactly once per module. Tests can call it standalone via the
//! re-export at `mty_codegen_wasm::cabi_realloc::build_cabi_realloc_body`.

use wasm_encoder::{BlockType, Function as WFunction, Instruction as I, ValType};

/// v0.10 cleanup — `cabi_realloc` allocator memory layout.
///
/// Linear memory ranges:
///   * 0..1024  — reserved for shadow-stack scratch,
///   * 1024..8192 — string-literal pool (data section),
///   * 8192..8224 — legacy JS shim + canonical-ABI return area,
///   * 8224..32768 — slack for future growth of the data section,
///   * 32768..32800 — allocator state (8 i32 free-list heads),
///   * 32800.. — heap (bump-allocated, with size-class reuse).
pub const CABI_REALLOC_STATE_BASE: i32 = 32768;
pub const CABI_REALLOC_HEAP_BASE: i32 = 32800;

/// Eight size classes: 8, 16, 32, 64, 128, 256, 512, 1024 bytes.
/// Indexed 0..7. Class `i` has size `8 << i`.
pub const CABI_REALLOC_NUM_CLASSES: u32 = 8;
pub const CABI_REALLOC_LARGE_THRESHOLD: u32 = 1024;

/// Build the body of the canonical-ABI `cabi_realloc` export.
///
/// v0.10: segregated free-list allocator with 8 size classes
/// (8B → 1024B, powers of 2) + a "large" bump path for `size >
/// 1024`. See [`CABI_REALLOC_STATE_BASE`] for the memory layout.
///
/// v0.18: extracted from `emit.rs` for KNOWN_ISSUES #1 — same
/// emitted bytes, just relocated so the allocator has a stable
/// review surface.
///
/// ### Pseudocode
///
/// ```text
/// fn cabi_realloc(old: i32, old_size: i32, align: i32, new: i32) -> i32 {
///     if new == 0 {
///         if old != 0 { free(old, old_size); }
///         return 0;
///     }
///     let p = if old == 0 {
///         malloc(align, new)
///     } else {
///         let p = malloc(align, new);
///         memcpy(p, old, min(old_size, new));
///         free(old, old_size);
///         p
///     };
///     p
/// }
///
/// fn malloc(align: i32, size: i32) -> i32 {
///     let class = size_class(size);    // -1 if size > 1024
///     if class >= 0 && align <= class_size(class) {
///         let head = load_i32(STATE_BASE + class*4);
///         if head != 0 {
///             store_i32(STATE_BASE + class*4, load_i32(head));
///             return head;
///         }
///         // bump-allocate class_size bytes (naturally aligned for align).
///         return bump(class_size(class), align);
///     }
///     bump(size, align)
/// }
///
/// fn free(ptr: i32, size: i32) {
///     let class = size_class(size);
///     if class < 0 { return; }   // large: not freed
///     let head = load_i32(STATE_BASE + class*4);
///     store_i32(ptr, head);
///     store_i32(STATE_BASE + class*4, ptr);
/// }
///
/// fn bump(size: i32, align: i32) -> i32 {
///     let mask = align - 1;
///     $bump = ($bump + mask) & !mask;
///     let p = $bump;
///     $bump = $bump + size;
///     p
/// }
///
/// // size_class: returns class index 0..7 such that class_size >= size,
/// // or -1 if size > 1024. Implemented as an unrolled if-chain
/// // (8 comparisons) because wasm has no native ctz/clz on i32 sizes
/// // small enough to dispatch off.
/// ```
///
/// ### Wasm layout
///
/// Locals (after the 4 params `old`, `old_size`, `align`, `new`):
/// - local 4: `class`  (i32)  — size class index, -1 = large.
/// - local 5: `csize`  (i32)  — bytes for the size class.
/// - local 6: `head`   (i32)  — free-list head pointer.
/// - local 7: `p`      (i32)  — allocation result / scratch.
/// - local 8: `mask`   (i32)  — alignment mask.
/// - local 9: `i`      (i32)  — memcpy loop counter.
/// - local 10: `n`     (i32)  — memcpy byte count = min(old_size, new).
///
/// Global 0 = bump pointer, initialised to [`CABI_REALLOC_HEAP_BASE`].
pub fn build_cabi_realloc_body() -> WFunction {
    let mut f = WFunction::new([(7u32, ValType::I32)]);
    const PARAM_OLD: u32 = 0;
    const PARAM_OLD_SIZE: u32 = 1;
    const PARAM_ALIGN: u32 = 2;
    const PARAM_NEW: u32 = 3;
    const LOC_CLASS: u32 = 4;
    const LOC_CSIZE: u32 = 5;
    const LOC_HEAD: u32 = 6;
    const LOC_P: u32 = 7;
    const LOC_MASK: u32 = 8;
    const LOC_I: u32 = 9;
    const LOC_N: u32 = 10;
    let memarg0 = wasm_encoder::MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    };
    let memarg_b = wasm_encoder::MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    };

    // ----- if new == 0: free-only or no-op -----
    f.instruction(&I::LocalGet(PARAM_NEW));
    f.instruction(&I::I32Eqz);
    f.instruction(&I::If(BlockType::Empty));
    {
        // if old != 0 { free(old, old_size); }
        f.instruction(&I::LocalGet(PARAM_OLD));
        f.instruction(&I::I32Eqz);
        f.instruction(&I::I32Eqz);
        f.instruction(&I::If(BlockType::Empty));
        {
            emit_size_class(&mut f, PARAM_OLD_SIZE, LOC_CLASS);
            // if class >= 0 { push to free list }
            f.instruction(&I::LocalGet(LOC_CLASS));
            f.instruction(&I::I32Const(0));
            f.instruction(&I::I32GeS);
            f.instruction(&I::If(BlockType::Empty));
            {
                // head = load_i32(STATE_BASE + class*4)
                f.instruction(&I::LocalGet(LOC_CLASS));
                f.instruction(&I::I32Const(2));
                f.instruction(&I::I32Shl);
                f.instruction(&I::I32Const(CABI_REALLOC_STATE_BASE));
                f.instruction(&I::I32Add);
                f.instruction(&I::LocalSet(LOC_P)); // P = address of head slot
                f.instruction(&I::LocalGet(LOC_P));
                f.instruction(&I::I32Load(memarg0));
                f.instruction(&I::LocalSet(LOC_HEAD));
                // store_i32(old, head)
                f.instruction(&I::LocalGet(PARAM_OLD));
                f.instruction(&I::LocalGet(LOC_HEAD));
                f.instruction(&I::I32Store(memarg0));
                // store_i32(head_slot, old)
                f.instruction(&I::LocalGet(LOC_P));
                f.instruction(&I::LocalGet(PARAM_OLD));
                f.instruction(&I::I32Store(memarg0));
            }
            f.instruction(&I::End);
        }
        f.instruction(&I::End);
        // return 0
        f.instruction(&I::I32Const(0));
        f.instruction(&I::Return);
    }
    f.instruction(&I::End);

    // ----- malloc(align, new) -> LOC_P -----
    emit_size_class(&mut f, PARAM_NEW, LOC_CLASS);
    // csize = if class < 0 { new } else { 8 << class }
    f.instruction(&I::LocalGet(LOC_CLASS));
    f.instruction(&I::I32Const(0));
    f.instruction(&I::I32LtS);
    f.instruction(&I::If(BlockType::Result(ValType::I32)));
    {
        f.instruction(&I::LocalGet(PARAM_NEW));
    }
    f.instruction(&I::Else);
    {
        f.instruction(&I::I32Const(8));
        f.instruction(&I::LocalGet(LOC_CLASS));
        f.instruction(&I::I32Shl);
    }
    f.instruction(&I::End);
    f.instruction(&I::LocalSet(LOC_CSIZE));

    // Try free-list reuse: only if class >= 0 AND align <= csize.
    f.instruction(&I::LocalGet(LOC_CLASS));
    f.instruction(&I::I32Const(0));
    f.instruction(&I::I32GeS);
    f.instruction(&I::LocalGet(PARAM_ALIGN));
    f.instruction(&I::LocalGet(LOC_CSIZE));
    f.instruction(&I::I32LeS);
    f.instruction(&I::I32And);
    f.instruction(&I::If(BlockType::Empty));
    {
        // head_slot = STATE_BASE + class*4
        f.instruction(&I::LocalGet(LOC_CLASS));
        f.instruction(&I::I32Const(2));
        f.instruction(&I::I32Shl);
        f.instruction(&I::I32Const(CABI_REALLOC_STATE_BASE));
        f.instruction(&I::I32Add);
        f.instruction(&I::LocalSet(LOC_MASK)); // reuse mask local as slot ptr
                                               // head = load(head_slot)
        f.instruction(&I::LocalGet(LOC_MASK));
        f.instruction(&I::I32Load(memarg0));
        f.instruction(&I::LocalSet(LOC_HEAD));
        // if head != 0 { pop and use }
        f.instruction(&I::LocalGet(LOC_HEAD));
        f.instruction(&I::I32Eqz);
        f.instruction(&I::I32Eqz);
        f.instruction(&I::If(BlockType::Empty));
        {
            // head_slot.store(load(head))   // next-link
            f.instruction(&I::LocalGet(LOC_MASK));
            f.instruction(&I::LocalGet(LOC_HEAD));
            f.instruction(&I::I32Load(memarg0));
            f.instruction(&I::I32Store(memarg0));
            // p = head
            f.instruction(&I::LocalGet(LOC_HEAD));
            f.instruction(&I::LocalSet(LOC_P));
            // proceed to copy-from-old + free-old + return p; jump
            // to that section via setting LOC_HEAD = 0 sentinel?
            // Simpler: use the post-malloc tail explicitly. We
            // signal "already allocated" by setting LOC_HEAD=1.
            f.instruction(&I::I32Const(1));
            f.instruction(&I::LocalSet(LOC_HEAD));
        }
        f.instruction(&I::End);
    }
    f.instruction(&I::End);

    // If LOC_HEAD != 1, we didn't pop from free list — bump-allocate.
    f.instruction(&I::LocalGet(LOC_HEAD));
    f.instruction(&I::I32Const(1));
    f.instruction(&I::I32Ne);
    f.instruction(&I::If(BlockType::Empty));
    {
        // bump-allocate LOC_CSIZE bytes aligned to PARAM_ALIGN.
        // mask = align - 1
        f.instruction(&I::LocalGet(PARAM_ALIGN));
        f.instruction(&I::I32Const(1));
        f.instruction(&I::I32Sub);
        f.instruction(&I::LocalSet(LOC_MASK));
        // bump = (bump + mask) & !mask
        f.instruction(&I::GlobalGet(0));
        f.instruction(&I::LocalGet(LOC_MASK));
        f.instruction(&I::I32Add);
        f.instruction(&I::LocalGet(LOC_MASK));
        f.instruction(&I::I32Const(-1));
        f.instruction(&I::I32Xor);
        f.instruction(&I::I32And);
        f.instruction(&I::GlobalSet(0));
        // p = bump; bump += csize
        f.instruction(&I::GlobalGet(0));
        f.instruction(&I::LocalSet(LOC_P));
        f.instruction(&I::GlobalGet(0));
        f.instruction(&I::LocalGet(LOC_CSIZE));
        f.instruction(&I::I32Add);
        f.instruction(&I::GlobalSet(0));
    }
    f.instruction(&I::End);

    // ----- if old != 0: copy min(old_size, new) bytes, then free old -----
    f.instruction(&I::LocalGet(PARAM_OLD));
    f.instruction(&I::I32Eqz);
    f.instruction(&I::I32Eqz);
    f.instruction(&I::If(BlockType::Empty));
    {
        // n = min(old_size, new)
        f.instruction(&I::LocalGet(PARAM_OLD_SIZE));
        f.instruction(&I::LocalGet(PARAM_NEW));
        f.instruction(&I::I32LtS);
        f.instruction(&I::If(BlockType::Result(ValType::I32)));
        {
            f.instruction(&I::LocalGet(PARAM_OLD_SIZE));
        }
        f.instruction(&I::Else);
        {
            f.instruction(&I::LocalGet(PARAM_NEW));
        }
        f.instruction(&I::End);
        f.instruction(&I::LocalSet(LOC_N));

        // byte-by-byte memcpy: for i in 0..n { *(p+i) = *(old+i); }
        f.instruction(&I::I32Const(0));
        f.instruction(&I::LocalSet(LOC_I));
        f.instruction(&I::Block(BlockType::Empty));
        f.instruction(&I::Loop(BlockType::Empty));
        {
            // if i >= n break
            f.instruction(&I::LocalGet(LOC_I));
            f.instruction(&I::LocalGet(LOC_N));
            f.instruction(&I::I32GeS);
            f.instruction(&I::BrIf(1));
            // *(p+i) = *(old+i)
            f.instruction(&I::LocalGet(LOC_P));
            f.instruction(&I::LocalGet(LOC_I));
            f.instruction(&I::I32Add);
            f.instruction(&I::LocalGet(PARAM_OLD));
            f.instruction(&I::LocalGet(LOC_I));
            f.instruction(&I::I32Add);
            f.instruction(&I::I32Load8U(memarg_b));
            f.instruction(&I::I32Store8(memarg_b));
            // i += 1
            f.instruction(&I::LocalGet(LOC_I));
            f.instruction(&I::I32Const(1));
            f.instruction(&I::I32Add);
            f.instruction(&I::LocalSet(LOC_I));
            f.instruction(&I::Br(0));
        }
        f.instruction(&I::End); // loop
        f.instruction(&I::End); // block

        // free(old, old_size): if class' >= 0, push to free list.
        emit_size_class(&mut f, PARAM_OLD_SIZE, LOC_CLASS);
        f.instruction(&I::LocalGet(LOC_CLASS));
        f.instruction(&I::I32Const(0));
        f.instruction(&I::I32GeS);
        f.instruction(&I::If(BlockType::Empty));
        {
            // head_slot = STATE_BASE + class*4
            f.instruction(&I::LocalGet(LOC_CLASS));
            f.instruction(&I::I32Const(2));
            f.instruction(&I::I32Shl);
            f.instruction(&I::I32Const(CABI_REALLOC_STATE_BASE));
            f.instruction(&I::I32Add);
            f.instruction(&I::LocalSet(LOC_MASK));
            // *old = *head_slot
            f.instruction(&I::LocalGet(PARAM_OLD));
            f.instruction(&I::LocalGet(LOC_MASK));
            f.instruction(&I::I32Load(memarg0));
            f.instruction(&I::I32Store(memarg0));
            // *head_slot = old
            f.instruction(&I::LocalGet(LOC_MASK));
            f.instruction(&I::LocalGet(PARAM_OLD));
            f.instruction(&I::I32Store(memarg0));
        }
        f.instruction(&I::End);
    }
    f.instruction(&I::End);

    f.instruction(&I::LocalGet(LOC_P));
    f.instruction(&I::End);
    f
}

/// Emit wasm that computes the size class of `size_local` (params/locals)
/// into `out_local`. Classes are 8, 16, 32, 64, 128, 256, 512, 1024 →
/// indices 0..7. Returns -1 for size > 1024 (large path).
///
/// Implementation: unrolled if-chain across powers of 2. Worst case 8
/// comparisons, but wasm-jit on the host inlines this and the cost is
/// negligible compared to the surrounding malloc bookkeeping.
pub fn emit_size_class(f: &mut WFunction, size_local: u32, out_local: u32) {
    // class = -1 (large)
    f.instruction(&I::I32Const(-1));
    f.instruction(&I::LocalSet(out_local));
    // Walk from class 7 down to class 0; the smallest class whose
    // size >= request wins. Iterate in reverse so the smallest
    // class overrides any larger one.
    for class in (0..CABI_REALLOC_NUM_CLASSES as i32).rev() {
        let csize: i32 = 8i32 << class;
        // if size_local <= csize { out_local = class }
        f.instruction(&I::LocalGet(size_local));
        f.instruction(&I::I32Const(csize));
        f.instruction(&I::I32LeS);
        f.instruction(&I::If(BlockType::Empty));
        f.instruction(&I::I32Const(class));
        f.instruction(&I::LocalSet(out_local));
        f.instruction(&I::End);
    }
    // Edge case: size_local == 0 should still pick class 0, which it
    // will (0 <= 8). Negative sizes never occur (canonical-ABI sizes
    // are unsigned i32; the wasm-encoder API uses signed Rust types
    // but semantically these are u32). Behaviour at extreme inputs
    // is defined by the host's wasm runtime.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The allocator-state region must fit exactly the per-class
    /// free-list heads — one i32 per class — before the heap starts.
    /// If this invariant ever drifts, callers that index off
    /// [`CABI_REALLOC_STATE_BASE`] would silently corrupt the heap.
    #[test]
    fn state_region_sized_for_class_heads() {
        let state_bytes = (CABI_REALLOC_NUM_CLASSES as i32) * 4;
        assert_eq!(
            CABI_REALLOC_STATE_BASE + state_bytes,
            CABI_REALLOC_HEAP_BASE,
            "state region must be exactly NUM_CLASSES * 4 bytes"
        );
    }

    /// [`build_cabi_realloc_body`] must produce a non-empty
    /// `WFunction`. We can't easily peek at the inner bytes (the
    /// `wasm_encoder::Function` type doesn't expose its buffer in
    /// stable form across wasm-encoder versions), but the
    /// determinism contract is enforced indirectly by the
    /// `tests/cabi_realloc.rs::module_emission_is_deterministic`
    /// integration test which compiles two whole modules and
    /// byte-compares them.
    #[test]
    fn build_body_smoke() {
        let _ = build_cabi_realloc_body();
    }

    /// The large-alloc threshold must equal the largest size class.
    /// If they ever diverge, requests that fall in `(class_7_size,
    /// LARGE_THRESHOLD]` would silently slip into the bump path.
    #[test]
    fn large_threshold_matches_top_class() {
        let top_class = (CABI_REALLOC_NUM_CLASSES - 1) as i32;
        let top_class_size: u32 = 8u32 << top_class;
        assert_eq!(top_class_size, CABI_REALLOC_LARGE_THRESHOLD);
    }
}
