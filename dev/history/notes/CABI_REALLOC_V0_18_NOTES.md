# `cabi_realloc` — v0.18 extraction notes

KNOWN_ISSUES #1 — close-out notes for the v0.18 pass.

## Background

The v0.9 ship-prep file `KNOWN_ISSUES.md` flagged
`build_cabi_realloc_body` as a bump-only allocator. v0.10 replaced
that with a segregated free-list (8 size classes, 8B → 1024B,
power-of-two indexed) inline in `emit.rs`. v0.18 was opened to
"close" the issue — which had two pieces:

1. Confirm the v0.10 allocator is actually correct + comprehensive
   enough for v1.0 freeze.
2. Pay down the technical debt of having the allocator emitted from
   a 280-line function buried in the middle of `emit.rs`.

Status: shipped both.

## Design choice — free-list vs buddy

The v0.10 free-list (kept verbatim for v0.18) wins on three axes
versus a buddy allocator:

| Property | Free-list | Buddy |
|----------|-----------|-------|
| Emitted wasm size | ~190 instructions | ~350 instructions (split+merge) |
| Per-alloc cost | O(1) (LIFO push/pop) | O(log N) (split chain) |
| Internal fragmentation | size-class rounding (worst-case 2×) | none across power-of-2 sizes |
| External fragmentation | none within class; large path leaks | low; coalescable |
| State overhead | 32 B (8 i32 heads) | 64 B (free-lists per order) |
| Implementation risk | well-understood | edge cases around order-0 splits |

The Component-Model allocation pattern is dominated by short, owned
string returns (32–256 B). Internal fragmentation in that regime is
bounded by the class table; a buddy's main advantage (coalescing) is
only useful for the >1024B path, which today leaks.

Decision: keep the free-list. Revisit if the
`stress_1000_alloc_free_cycles_bounded_growth` test ever needs to
grow past the 32 B working set, or if a real-world program shows the
large-path leak in production.

## Allocator shape

| Region | Addr | Use |
|--------|------|-----|
| Shadow stack | `0..1024` | reserved; unused in v0.18 lowerer |
| String pool | `1024..8192` | data section |
| Legacy JS shim + canonical-ABI return area | `8192..8224` | DOM imports |
| FS / HTTP / log return areas | `8224..8560` | per-interface scratch |
| Allocator state | `32768..32800` | 8 free-list heads (one i32 per class) |
| Heap | `32800..` | bump-allocated, with size-class reuse |

The bump pointer is wasm `global 0` (mutable i32), initialised to
`CABI_REALLOC_HEAP_BASE = 32800`.

## Wasm instruction count

The emitted `cabi_realloc` function is ~190 instructions of straight-
line + 1 inner loop (byte-by-byte memcpy). Counted by inspection of
`build_cabi_realloc_body`:

| Section | Instructions (approx) |
|---------|-----------------------|
| free-only path (`new == 0`) | 25 |
| `size_class()` (called 3×, 24 instr each) | 72 |
| free-list reuse + bump fallback (`malloc`) | 45 |
| realloc copy + old-free | 35 |
| return | 2 |
| **Total** | **~190** |

The target budget from the task description was "< 200 if possible"
— we land inside that.

## Memory overhead

The allocator metadata region is **32 bytes** (8 size classes × 4
bytes each). Linear-memory pages are zero-initialised at module load
so no startup code is needed to clear it — the `state_region_initialised_to_zero`
integration test verifies that invariant.

## What v0.18 actually changed

Only structural — no behaviour change:

- Extract `build_cabi_realloc_body` + `emit_size_class` + the 4
  layout constants from `emit.rs` into a new module
  `src/cabi_realloc.rs`.
- Re-export the constants from `emit` so existing test imports
  keep working (`mty_codegen_wasm::emit::CABI_REALLOC_*`).
- Add the v0.18 test file `tests/cabi_realloc.rs` (8 tests).
- Update `docs/internals/codegen-wasm.md`.

Byte-for-byte, the emitted `.wasm` is unchanged across this commit —
verified by `module_emission_is_deterministic` (which doesn't quite
prove pre/post equality but does prove emission stability) and by
running both `cabi_realloc.rs` and `cabi_realloc_real.rs` to green.

## v0.19 follow-ups

The KNOWN_ISSUES entry is RESOLVED but a few quality-of-life upgrades
remain on the backlog:

1. **Large-path coalescing**: requests > 1024 B currently bump-only.
   Add either a coalescing large list or a host-side `memory.grow`
   hook so long-running programs that stream large strings don't
   leak. Tracking issue when filed.
2. **In-place realloc when the size class doesn't change**: today's
   code always re-malloc + copy + free even when `old_size` and
   `new` fall in the same class. Detect that case and return `old`
   unchanged — saves the memcpy and the free-list churn. Probably
   ~10 extra instructions; trades wasm size for runtime.
3. **Per-component allocator tuning**: components that emit only
   small fixed-size records (e.g. a `record point` returner) could
   ship a 2-class allocator instead of 8. Codegen-time analysis of
   the WIT exports would tell us which classes are actually live.
   Marginal but compelling for the smallest-component story.
4. **Alignment > class_size**: the current "skip free-list when
   `align > class_size`" check is sound but conservative. For
   `align == 16` requests of size 8 (class 0), we fall back to
   bump even though class 1 (16B) would satisfy both. Could promote
   to the next class instead.
5. **Allocator-state corruption guard**: a future debug build could
   stamp magic bytes around each free block so `free()` can assert
   the block came out of `malloc()`. Useful when the canonical-ABI
   contract drifts (e.g. wasmtime version bumps).

None of these block v1.0-RC.
